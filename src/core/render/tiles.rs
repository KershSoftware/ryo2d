//! Tile drawing, CPU half: a chunk of a tile layer becomes `SpriteInstance`s,
//! and [`TileDrawer`] puts the frame's visible chunks of every spawned map in
//! the draw list, building those that changed into the batch's chunk store.
//!
//! The instances are independent of where the map is, how the camera moves
//! and where the tileset's image sits in the atlas: they hold layer-local
//! pixels (cell (x, y) at (x, y) times the layer's cell size, y down) and
//! UVs over the whole padded tileset image. The draw applies the map's
//! transform, the camera's parallax and the image's slot.
//!
//! # Instance lanes
//!
//! - `transform`: the quad is centred on the tile's drawn rectangle; row 0
//!   is the quad's x axis and row 1 its y axis, in pixels. Unflipped, row 0
//!   is `[w, 0]` and row 1 `[0, h]`; with the diagonal flip they swap:
//!   row 0 `[0, h]`, row 1 `[w, 0]` (w and h being the drawn width and
//!   height, already swapped for a diagonal flip). Row 2 is the centre.
//! - `uv_rect`: the tile's texels in the padded image, 0..1 over the whole
//!   image; flips are swaps of its min and max.
//! - `uv_bounds`: the tile's padded cell in the same space.
//! - `tint`: the layer's tint, its alpha times the layer's opacity.
//! - `effect_params`: `[0, 0, 0, 0]`; for an animated tile `[frames,
//!   seconds per frame, step_u, step_v]`, a step being one cell in UV.
//! - `effect_extra`: zero; for an animated tile `[u, v, columns, 0]`, the
//!   first frame's cell origin in UV and the padded image's cells per row.
//!   `uv_rect` and `uv_bounds` are the first frame's. Frame `k` is cell
//!   `first + k` counted row-major in the padded image, rows wrapping at
//!   `columns`: its offset from the first frame is `((c0 + k) % columns -
//!   c0, (c0 + k) / columns)` cells, `c0` being `u / step_u`.

use std::hash::{DefaultHasher, Hash, Hasher};

use glam::{Affine2, IVec2, UVec2, Vec2};

use super::batch::{BlendMode, ChunkKey, ChunkPart, ChunkStamp, ChunkStore, Filter, TileSlot};
use super::{CameraDraw, DrawChunk, DrawList};
use crate::core::asset::Assets;
use crate::core::ecs::interpolate::render_global;
use crate::core::ecs::kernel::QueryState;
use crate::core::ecs::{
  Allow, DetectChangesMut, Disabled, Entity, GlobalTransform, Has, Mut, PreviousGlobalTransform,
  World,
};
use crate::core::graphics::{Image, RenderLayers, SpriteGpuRenderer, SpriteInstance, TextureId};
use crate::core::tilemap::padded::{PaddedLayout, PaddedTile, PaddedTilesets};
use crate::core::tilemap::{
  CHUNK_SIZE, LayerInfo, MapLayerRef, MapLayers, MapRoot, TileFlip, TileGrid, TileLayerData,
  Tileset, map_asset,
};
use crate::core::ui::Color;

/// What a layer paints every tile with
#[derive(Clone, Copy, Debug)]
pub(crate) struct LayerLook {
  pub tint: Color,
  pub opacity: f32,
}

/// Appends to `out` (cleared first) an instance for each tile of `chunk`
/// that has a picture, as `(tileset, instance)`, grouped by tileset index
/// and in row order within a tileset. A cell whose tileset or tile the map
/// lacks makes none.
pub(crate) fn chunk_instances(
  grid: &TileGrid,
  chunk: UVec2,
  cell_size: Vec2,
  layouts: &[Option<PaddedLayout>],
  look: &LayerLook,
  out: &mut Vec<(u16, SpriteInstance)>,
) {
  out.clear();
  let size = grid.size();
  let first = chunk.saturating_mul(UVec2::splat(CHUNK_SIZE));
  let last = first.saturating_add(UVec2::splat(CHUNK_SIZE)).min(size);
  for y in first.y..last.y {
    for x in first.x..last.x {
      let cell = IVec2::new(x as i32, y as i32);
      let Some(tile) = grid.get(cell) else {
        continue;
      };
      let Some(layout) = layouts.get(tile.tileset as usize).and_then(Option::as_ref) else {
        continue;
      };
      let Some(padded) = layout.tile(tile.id) else {
        continue;
      };
      out.push((
        tile.tileset,
        instance(cell, cell_size, tile.flip, layout, padded, look),
      ));
    }
  }
  // Stable, so each tileset keeps its row order
  out.sort_by_key(|(tileset, _)| *tileset);
}

/// One tile's instance
fn instance(
  cell: IVec2,
  cell_size: Vec2,
  flip: TileFlip,
  layout: &PaddedLayout,
  tile: &PaddedTile,
  look: &LayerLook,
) -> SpriteInstance {
  let diagonal = flip.contains(TileFlip::DIAGONAL);
  let tile_size = tile.size.as_vec2();
  // The tile as drawn: a diagonal flip swaps its sides
  let drawn = if diagonal {
    Vec2::new(tile_size.y, tile_size.x)
  } else {
    tile_size
  };
  // It stands on the cell's bottom edge from the left, however large; the
  // tileset's offset moves it from there
  let top_left = Vec2::new(
    cell.x as f32 * cell_size.x,
    (cell.y as f32 + 1.0) * cell_size.y - drawn.y,
  ) + layout.offset.as_vec2();
  let centre = top_left + drawn * 0.5;

  // An animated tile shows its first frame's cell; the draw steps from it
  let slot = tile.animation.map_or(tile.slot, |a| a.first_slot);
  let image = layout.size.as_vec2();
  let origin = layout.cell_origin(slot).as_vec2();
  let inset = ((UVec2::splat(layout.cell) - tile.size) / 2).as_vec2();
  let min = (origin + inset) / image;
  let max = (origin + inset + tile_size) / image;
  let (mut u, mut v) = ((min.x, max.x), (min.y, max.y));
  // Screen x is the tile's u, or its v under the diagonal flip, which
  // swaps the axes
  let (flip_u, flip_v) = if diagonal {
    (
      flip.contains(TileFlip::VERTICAL),
      flip.contains(TileFlip::HORIZONTAL),
    )
  } else {
    (
      flip.contains(TileFlip::HORIZONTAL),
      flip.contains(TileFlip::VERTICAL),
    )
  };
  if flip_u {
    u = (u.1, u.0);
  }
  if flip_v {
    v = (v.1, v.0);
  }

  let mut instance = SpriteInstance::new()
    .with_uv(u.0, v.0, u.1, v.1)
    .with_tint(look.tint, look.opacity);
  let (x_axis, y_axis) = if diagonal {
    ([0.0, drawn.y], [drawn.x, 0.0])
  } else {
    ([drawn.x, 0.0], [0.0, drawn.y])
  };
  instance.transform[0][..2].copy_from_slice(&x_axis);
  instance.transform[1][..2].copy_from_slice(&y_axis);
  instance.transform[2][..2].copy_from_slice(&centre.to_array());
  let cell_min = origin / image;
  let cell_max = (origin + Vec2::splat(layout.cell as f32)) / image;
  instance.uv_bounds = [cell_min.x, cell_min.y, cell_max.x, cell_max.y];
  if let Some(a) = tile.animation {
    let step = Vec2::splat(layout.cell as f32) / image;
    instance.effect_params = [a.frames as f32, a.frame_duration, step.x, step.y];
    instance.effect_extra = [cell_min.x, cell_min.y, layout.columns as f32, 0.0];
  }
  instance
}

/// A spawned map, disabled ones too (their tilesets stay padded)
type MapItem = (
  &'static MapRoot,
  &'static MapLayers,
  &'static GlobalTransform,
  Option<&'static PreviousGlobalTransform>,
  Option<&'static RenderLayers>,
  Has<Disabled>,
);

/// A layer of a spawned map
type LayerItem = (
  &'static MapLayerRef,
  &'static LayerInfo,
  &'static TileLayerData,
  &'static GlobalTransform,
  Option<&'static PreviousGlobalTransform>,
);

/// What a frame's tile layers are drawn for
pub(crate) struct TileLayerFrame<'a> {
  pub cameras: &'a [CameraDraw],
  /// Render interpolation factor
  pub alpha: f32,
}

/// Puts the tile layers of spawned maps in the frame's draw list: for each
/// view of a layer (every camera at once, or each camera for a layer with
/// parallax), the chunks its cameras can see, each tileset's tiles of a
/// chunk one [`DrawChunk`] through the view's slot for that tileset.
/// Chunks are built into the batch's [`ChunkStore`] when first seen and
/// again when their tiles or what they are drawn from change.
#[derive(Default)]
pub(crate) struct TileDrawer {
  padded: PaddedTilesets,
  maps: Option<QueryState<MapItem, Allow<Disabled>>>,
  layers: Option<QueryState<LayerItem>>,
  /// The map being drawn: each tileset's padded layout and texture
  layouts: Vec<Option<PaddedLayout>>,
  textures: Vec<Option<TextureId>>,
  /// The view being drawn: each tileset's slot, once it has one
  slots: Vec<Option<u32>>,
  /// A chunk's parts, copied out of the store
  parts: Vec<ChunkPart>,
}

/// One tile layer as a frame draws it
struct LayerDraw<'a> {
  layer: Entity,
  data: &'a TileLayerData,
  look: LayerLook,
  /// What its chunks are built from besides their tiles
  stamp: u64,
  z: i32,
  lit: bool,
  blend: BlendMode,
  /// The map's render layers
  layers: RenderLayers,
  /// How far past its cell a tile may draw, in the layer's pixels
  margin: f32,
}

impl TileDrawer {
  /// Pushes the frame's visible chunks onto `list`, building into `store`
  /// those that changed, then frees the chunks of layers that are gone and
  /// those long undrawn, and the padded images no map asked for
  pub(crate) fn collect(
    &mut self,
    world: &mut World,
    frame: &TileLayerFrame<'_>,
    gpu: &mut SpriteGpuRenderer,
    store: &mut ChunkStore,
    list: &mut DrawList,
  ) {
    store.begin_frame();
    // With no map left, the round frees every padded image
    self.padded.begin_round();
    self.layers.get_or_insert_with(|| world.query());
    let maps = self.maps.get_or_insert_with(|| world.query_filtered());
    if maps.iter(world).next().is_some() {
      world.try_resource_scope(|world, mut images: Mut<Assets<Image>>| {
        let images = images.bypass_change_detection();
        let Some(tilesets) = world.get_resource::<Assets<Tileset>>() else {
          return;
        };
        let world: &World = world;
        let Self {
          padded,
          maps,
          layers,
          layouts,
          textures,
          slots,
          parts,
        } = self;
        let (Some(maps), Some(layers)) = (maps.as_mut(), layers.as_mut()) else {
          return;
        };
        for (root, map_layers, global, previous, render_layers, disabled) in maps.iter(world) {
          let Some(asset) = map_asset(world, root) else {
            continue;
          };
          // Each tileset's padded image and its texture
          layouts.clear();
          textures.clear();
          let mut paddings = DefaultHasher::new();
          let mut margin = 0.0f32;
          for source in &asset.tilesets {
            let Some((handle, layout)) = padded.get(source, tilesets, images) else {
              layouts.push(None);
              textures.push(None);
              0u64.hash(&mut paddings);
              continue;
            };
            let texture = gpu
              .shown_image(handle.id(), images)
              .map(|shown| gpu.shown_texture(shown));
            let reach = layout.offset.abs().max_element() as f32 + layout.cell as f32;
            margin = margin.max(reach);
            layout.serial.hash(&mut paddings);
            layouts.push(Some(layout));
            textures.push(texture);
          }
          // A disabled map keeps its tilesets padded and draws nothing
          if disabled {
            continue;
          }
          let paddings = paddings.finish();
          let map_layers_mask = render_layers.copied().unwrap_or_default();
          let map_affine = render_global(global, previous, frame.alpha).affine();
          let sees = |camera: &&CameraDraw| camera.layers.intersects(&map_layers_mask);
          if !frame.cameras.iter().any(|camera| sees(&camera)) {
            continue;
          }
          for &entity in &map_layers.0 {
            let Ok((layer_ref, info, data, layer_global, layer_previous)) =
              layers.get(world, entity)
            else {
              continue;
            };
            if !info.visible || data.cell_size.min_element() == 0 {
              continue;
            }
            let look = LayerLook {
              tint: info.tint,
              opacity: info.opacity,
            };
            let mut stamp = DefaultHasher::new();
            (paddings, info.tint, info.opacity.to_bits(), data.cell_size).hash(&mut stamp);
            let index = i32::try_from(layer_ref.index).unwrap_or(i32::MAX);
            let draw = LayerDraw {
              layer: entity,
              data,
              look,
              stamp: stamp.finish(),
              z: root.z.saturating_add(index),
              lit: info.lit,
              blend: info.blend,
              layers: map_layers_mask,
              margin,
            };
            let layer_affine = render_global(layer_global, layer_previous, frame.alpha).affine();
            let mut view = View {
              store: &mut *store,
              list: &mut *list,
              layouts,
              textures,
              slots,
              parts,
            };
            if info.parallax == Vec2::ONE {
              // One view for every camera
              let cameras = frame.cameras.iter().filter(sees);
              view.draw(&draw, layer_affine, Vec2::ZERO, cameras, None);
            } else {
              // A view per camera: each moves the layer by its own offset
              for camera in frame.cameras.iter().filter(sees) {
                let Some(offset) = parallax(map_affine, info.parallax, camera.view.center) else {
                  continue;
                };
                let one = std::iter::once(camera);
                view.draw(&draw, layer_affine, offset, one, Some(camera.entity));
              }
            }
          }
        }
      });
    }
    self.padded.end_round();
    store.end_frame(|layer| world.get::<TileLayerData>(layer).is_some());
  }
}

impl TileDrawer {
  /// How many padded tileset images are kept (tests)
  #[cfg(test)]
  pub(crate) fn padded_count(&self) -> usize {
    self.padded.len()
  }
}

/// The world offset a layer of parallax `factor` draws at for a camera
/// centred on `center`, on a map placed by `map`: the part of the camera's
/// position from the map's origin the layer does not follow (Tiled's rule,
/// its parallax origin being the map's top-left corner). `None` for a map
/// scaled to nothing
fn parallax(map: Affine2, factor: Vec2, center: Vec2) -> Option<Vec2> {
  let det = map.matrix2.determinant();
  if !det.is_finite() || det.abs() < 1e-12 {
    return None;
  }
  let local = map.inverse().transform_point2(center);
  Some(map.transform_vector2((Vec2::ONE - factor) * local))
}

/// What one view of a layer draws with
struct View<'s> {
  store: &'s mut ChunkStore,
  list: &'s mut DrawList,
  layouts: &'s [Option<PaddedLayout>],
  textures: &'s [Option<TextureId>],
  slots: &'s mut Vec<Option<u32>>,
  parts: &'s mut Vec<ChunkPart>,
}

impl View<'_> {
  /// Pushes the chunks of `layer` that `cameras` may see, the layer placed
  /// by `affine` moved by `offset`; `only` is the camera the draws are for,
  /// if one alone
  fn draw<'c>(
    &mut self,
    layer: &LayerDraw<'_>,
    affine: Affine2,
    offset: Vec2,
    cameras: impl Iterator<Item = &'c CameraDraw>,
    only: Option<Entity>,
  ) {
    let placed = Affine2::from_mat2_translation(affine.matrix2, affine.translation + offset);
    let det = placed.matrix2.determinant();
    if !det.is_finite() || det.abs() < 1e-12 {
      return;
    }
    let to_layer = placed.inverse();
    let grid = &layer.data.grid;
    let chunks = grid.chunks();
    if chunks.min_element() == 0 {
      return;
    }
    let cell = layer.data.cell_size.as_vec2();
    let chunk_px = cell * CHUNK_SIZE as f32;
    let view = self.store.begin_view();
    self.slots.clear();
    self.slots.resize(self.layouts.len(), None);
    let slot = TileSlot {
      x_axis: affine.matrix2.x_axis.to_array(),
      y_axis: affine.matrix2.y_axis.to_array(),
      origin: affine.translation.to_array(),
      parallax: offset.to_array(),
      uv: [0.0, 0.0, 1.0, 1.0],
    };
    for camera in cameras {
      // The camera's view in the layer's pixels, widened by how far a tile
      // may draw past its cell; then the chunks it meets
      let b = camera.view.bounds;
      let corners = [
        Vec2::new(b.left, b.top),
        Vec2::new(b.right, b.top),
        Vec2::new(b.left, b.bottom),
        Vec2::new(b.right, b.bottom),
      ]
      .map(|corner| to_layer.transform_point2(corner));
      let low = corners.iter().fold(Vec2::MAX, |m, c| m.min(*c)) - layer.margin;
      let high = corners.iter().fold(Vec2::MIN, |m, c| m.max(*c)) + layer.margin;
      let first = (low / chunk_px).floor().max(Vec2::ZERO);
      let last = (high / chunk_px).floor().min(chunks.as_vec2() - 1.0);
      // Not finite, or off the layer
      if !(first.x <= last.x && first.y <= last.y) {
        continue;
      }
      let (first, last) = (first.as_uvec2(), last.as_uvec2());
      for y in first.y..=last.y {
        for x in first.x..=last.x {
          self.chunk(layer, UVec2::new(x, y), view, (&placed, slot), only);
        }
      }
    }
  }

  /// Pushes `chunk` of `layer` for `view` (built first if it changed),
  /// each tileset's tiles through the view's slot for it
  fn chunk(
    &mut self,
    layer: &LayerDraw<'_>,
    chunk: UVec2,
    view: u64,
    (placed, slot): (&Affine2, TileSlot),
    only: Option<Entity>,
  ) {
    let grid = &layer.data.grid;
    let stamp = ChunkStamp {
      version: grid.chunk_version(chunk),
      look: layer.stamp,
    };
    let cell = layer.data.cell_size.as_vec2();
    let layouts = self.layouts;
    let build = |out: &mut Vec<(u16, SpriteInstance)>| {
      chunk_instances(grid, chunk, cell, layouts, &layer.look, out);
    };
    let Some(parts) = self.store.chunk(layer.layer, chunk, stamp, view, build) else {
      return;
    };
    self.parts.clear();
    self.parts.extend_from_slice(parts);
    for part in self.parts.iter() {
      let tileset = usize::from(part.tileset);
      let Some(&Some(texture)) = self.textures.get(tileset) else {
        continue;
      };
      let Some(entry) = self.slots.get_mut(tileset) else {
        continue;
      };
      let slot = *entry.get_or_insert_with(|| self.store.push_slot(slot, texture));
      let (min, max) = world_box(placed, part.min, part.max);
      let draw = DrawChunk {
        key: ChunkKey {
          layer: layer.layer,
          chunk,
          tileset: part.tileset,
        },
        texture,
        count: part.count,
        blend: layer.blend,
        filter: Filter::Default,
        layers: layer.layers,
        min,
        max,
        slot,
      };
      self.list.push_chunk(layer.lit, layer.z, only, draw);
    }
  }
}

/// The world box `min..max` (in a layer's pixels) covers under `affine`
fn world_box(affine: &Affine2, min: Vec2, max: Vec2) -> (Vec2, Vec2) {
  let corners = [min, Vec2::new(max.x, min.y), Vec2::new(min.x, max.y), max]
    .map(|corner| affine.transform_point2(corner));
  let low = corners.iter().fold(Vec2::MAX, |m, c| m.min(*c));
  let high = corners.iter().fold(Vec2::MIN, |m, c| m.max(*c));
  (low, high)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::asset::Assets;
  use crate::core::graphics::Image;
  use crate::core::tilemap::padded::pad_tileset;
  use crate::core::tilemap::padded::tests::marked_atlas;
  use crate::core::tilemap::{Tile, TileData, TileFlip, TileFrame, Tileset, TilesetImage};
  use glam::IVec2;
  use std::time::Duration;

  const LOOK: LayerLook = LayerLook {
    tint: Color::WHITE,
    opacity: 1.0,
  };

  fn layout_of(tileset: &Tileset, images: &Assets<Image>) -> PaddedLayout {
    pad_tileset(tileset, images).expect("padded").1
  }

  fn build(
    grid: &TileGrid,
    chunk: UVec2,
    cell: Vec2,
    layouts: &[Option<PaddedLayout>],
  ) -> Vec<(u16, SpriteInstance)> {
    let mut out = Vec::new();
    chunk_instances(grid, chunk, cell, layouts, &LOOK, &mut out);
    out
  }

  /// The screen position and UV at each of an instance's four corners, in
  /// order top-left, top-right, bottom-left, bottom-right of the quad
  fn corners(i: &SpriteInstance) -> [(Vec2, Vec2); 4] {
    let a = Vec2::new(i.transform[0][0], i.transform[0][1]);
    let b = Vec2::new(i.transform[1][0], i.transform[1][1]);
    let c = Vec2::new(i.transform[2][0], i.transform[2][1]);
    let uv = |fx: f32, fy: f32| {
      Vec2::new(
        i.uv_rect[0] + (i.uv_rect[2] - i.uv_rect[0]) * fx,
        i.uv_rect[1] + (i.uv_rect[3] - i.uv_rect[1]) * fy,
      )
    };
    [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)]
      .map(|(fx, fy)| (c + a * (fx - 0.5) + b * (fy - 0.5), uv(fx, fy)))
  }

  /// The corner of the drawn rectangle `corner` is on screen: the min and
  /// max of the four positions
  fn screen_box(i: &SpriteInstance) -> (Vec2, Vec2) {
    let c = corners(i);
    let min = c.iter().fold(Vec2::splat(f32::MAX), |m, (p, _)| m.min(*p));
    let max = c.iter().fold(Vec2::splat(f32::MIN), |m, (p, _)| m.max(*p));
    (min, max)
  }

  fn one_tile(flip: TileFlip, tile_w: u32, tile_h: u32) -> (SpriteInstance, PaddedLayout) {
    let mut images = Assets::new();
    let mut t = marked_atlas(&mut images, 16, 2, 2);
    t.tile_size = UVec2::new(tile_w, tile_h);
    let layout = layout_of(&t, &images);
    let mut grid = TileGrid::new(UVec2::splat(4));
    grid.set(IVec2::new(1, 1), Some(Tile::new(0, 3).with_flip(flip)));
    let out = build(
      &grid,
      UVec2::ZERO,
      Vec2::splat(16.0),
      &[Some(layout.clone())],
    );
    assert_eq!(out.len(), 1);
    (out[0].1, layout)
  }

  #[test]
  fn each_flip_maps_the_tile_s_corners_as_tiled_does() {
    let (plain, layout) = one_tile(TileFlip::NONE, 16, 16);
    let tile = layout.tile(3).expect("tile");
    let cell = layout.cell_origin(tile.slot).as_vec2() / layout.size.as_vec2();
    let px = Vec2::splat(1.0) / layout.size.as_vec2();
    // The tile's own rect: 8 texels into its cell, 16 wide
    let (u0, v0) = (cell.x + 8.0 * px.x, cell.y + 8.0 * px.y);
    let tile_uv = |fx: f32, fy: f32| Vec2::new(u0 + 16.0 * px.x * fx, v0 + 16.0 * px.y * fy);
    // Unflipped: corners show the same corners of the tile
    for ((_, uv), (fx, fy)) in
      corners(&plain)
        .iter()
        .zip([(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)])
    {
      assert!((*uv - tile_uv(fx, fy)).length() < 1e-6);
    }
    for bits in 0..8u8 {
      let mut flip = TileFlip::NONE;
      if bits & 1 != 0 {
        flip = flip | TileFlip::HORIZONTAL;
      }
      if bits & 2 != 0 {
        flip = flip | TileFlip::VERTICAL;
      }
      if bits & 4 != 0 {
        flip = flip | TileFlip::DIAGONAL;
      }
      let (i, _) = one_tile(flip, 16, 16);
      let (min, max) = screen_box(&i);
      assert_eq!(
        (min, max),
        (Vec2::new(16.0, 16.0), Vec2::new(32.0, 32.0)),
        "flip {bits}"
      );
      for (pos, uv) in corners(&i) {
        // Which screen corner this is, as a fraction of the rect
        let (sx, sy) = ((pos.x - min.x) / 16.0, (pos.y - min.y) / 16.0);
        // Tiled: the diagonal swaps x and y first, then horizontal and
        // vertical mirror the result; undo them in reverse to find the
        // tile corner shown here
        let (mut fx, mut fy) = (sx, sy);
        if bits & 2 != 0 {
          fy = 1.0 - fy;
        }
        if bits & 1 != 0 {
          fx = 1.0 - fx;
        }
        if bits & 4 != 0 {
          std::mem::swap(&mut fx, &mut fy);
        }
        assert!(
          (uv - tile_uv(fx, fy)).length() < 1e-6,
          "flip {bits}: screen ({sx}, {sy}) shows {uv:?}, wanted {:?}",
          tile_uv(fx, fy)
        );
      }
    }
  }

  #[test]
  fn a_quarter_turn_shows_the_bottom_left_at_the_top_left() {
    // Tiled's "rotate right": diagonal and horizontal
    let (i, layout) = one_tile(TileFlip::DIAGONAL | TileFlip::HORIZONTAL, 16, 16);
    let tile = layout.tile(3).expect("tile");
    let o = layout.cell_origin(tile.slot).as_vec2() + Vec2::splat(8.0);
    let (_, uv) = corners(&i)
      .into_iter()
      .min_by(|a, b| (a.0.x + a.0.y).total_cmp(&(b.0.x + b.0.y)))
      .expect("corner");
    let want = (o + Vec2::new(0.0, 16.0)) / layout.size.as_vec2();
    assert!((uv - want).length() < 1e-6);
  }

  fn two_sets() -> (Vec<Option<PaddedLayout>>, Assets<Image>) {
    let mut images = Assets::new();
    let a = marked_atlas(&mut images, 16, 2, 2);
    let b = marked_atlas(&mut images, 16, 2, 2);
    let layouts = vec![Some(layout_of(&a, &images)), Some(layout_of(&b, &images))];
    (layouts, images)
  }

  fn at(i: &SpriteInstance) -> Vec2 {
    Vec2::new(i.transform[2][0], i.transform[2][1])
  }

  #[test]
  fn a_chunk_groups_instances_by_tileset_in_row_order() {
    let (layouts, _) = two_sets();
    let mut grid = TileGrid::new(UVec2::new(40, 40));
    // Chunk (1, 0) covers cells 32..40 across and 0..32 down
    grid.set(IVec2::new(35, 2), Some(Tile::new(1, 0)));
    grid.set(IVec2::new(33, 2), Some(Tile::new(0, 1)));
    grid.set(IVec2::new(39, 0), Some(Tile::new(1, 2)));
    grid.set(IVec2::new(32, 0), Some(Tile::new(0, 3)));
    // Outside the chunk
    grid.set(IVec2::new(5, 5), Some(Tile::new(0, 0)));
    grid.set(IVec2::new(35, 33), Some(Tile::new(0, 0)));
    let out = build(&grid, UVec2::new(1, 0), Vec2::splat(16.0), &layouts);
    let sets: Vec<u16> = out.iter().map(|(s, _)| *s).collect();
    assert_eq!(sets, [0, 0, 1, 1]);
    let centres: Vec<Vec2> = out.iter().map(|(_, i)| at(i)).collect();
    let centre = |x: f32, y: f32| Vec2::new(x * 16.0 + 8.0, y * 16.0 + 8.0);
    assert_eq!(
      centres,
      [
        centre(32.0, 0.0),
        centre(33.0, 2.0),
        centre(39.0, 0.0),
        centre(35.0, 2.0)
      ]
    );
    // The last chunk of a grid is cut at the grid's edge
    let edge = build(&grid, UVec2::new(1, 1), Vec2::splat(16.0), &layouts);
    assert_eq!(edge.len(), 1);
  }

  #[test]
  fn empty_cells_make_no_instance() {
    let (layouts, _) = two_sets();
    let grid = TileGrid::new(UVec2::splat(10));
    assert!(build(&grid, UVec2::ZERO, Vec2::splat(16.0), &layouts).is_empty());
    // A chunk outside the grid too
    assert!(build(&grid, UVec2::new(3, 3), Vec2::splat(16.0), &layouts).is_empty());
  }

  #[test]
  fn a_cell_naming_a_tileset_or_tile_the_map_lacks_makes_no_instance() {
    let (mut layouts, _) = two_sets();
    let mut grid = TileGrid::new(UVec2::splat(4));
    grid.set(IVec2::new(0, 0), Some(Tile::new(2, 0)));
    grid.set(IVec2::new(1, 0), Some(Tile::new(0, 99)));
    grid.set(IVec2::new(2, 0), Some(Tile::new(255, 0)));
    grid.set(IVec2::new(3, 0), Some(Tile::new(0, 1)));
    grid.set(IVec2::new(0, 1), Some(Tile::new(1, 1)));
    layouts[1] = None;
    let out = build(&grid, UVec2::ZERO, Vec2::splat(16.0), &layouts);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].0, 0);
  }

  #[test]
  fn tint_and_opacity_come_from_the_layer() {
    let (layouts, _) = two_sets();
    let mut grid = TileGrid::new(UVec2::splat(2));
    grid.set(IVec2::ZERO, Some(Tile::new(0, 0)));
    let look = LayerLook {
      tint: Color::rgb(255, 0, 51),
      opacity: 0.5,
    };
    let mut out = Vec::new();
    chunk_instances(
      &grid,
      UVec2::ZERO,
      Vec2::splat(16.0),
      &layouts,
      &look,
      &mut out,
    );
    assert_eq!(out[0].1.tint, [1.0, 0.0, 0.2, 0.5]);
  }

  #[test]
  fn an_animated_tile_carries_its_frames() {
    let mut images = Assets::new();
    let mut t = marked_atlas(&mut images, 16, 2, 2);
    let ms = |tile| TileFrame {
      tile,
      duration: Duration::from_millis(120),
    };
    t.insert_tile(
      0,
      TileData {
        animation: vec![ms(1), ms(2), ms(3)],
        ..TileData::default()
      },
    );
    let layout = layout_of(&t, &images);
    let anim = layout.tile(0).and_then(|t| t.animation).expect("animated");
    let mut grid = TileGrid::new(UVec2::splat(2));
    grid.set(IVec2::new(1, 0), Some(Tile::new(0, 0)));
    grid.set(IVec2::new(0, 0), Some(Tile::new(0, 1)));
    let out = build(
      &grid,
      UVec2::ZERO,
      Vec2::splat(16.0),
      &[Some(layout.clone())],
    );
    let (still, animated) = (&out[0].1, &out[1].1);
    assert_eq!(still.effect_params, [0.0; 4]);
    assert_eq!(still.effect_extra, [0.0; 4]);
    let size = layout.size.as_vec2();
    let step = layout.cell as f32 / size;
    assert_eq!(animated.effect_params, [3.0, 0.12, step.x, step.y]);
    let first = layout.cell_origin(anim.first_slot).as_vec2() / size;
    assert_eq!(
      animated.effect_extra,
      [first.x, first.y, layout.columns as f32, 0.0]
    );
    // The instance shows the first frame
    let bounds = [first.x, first.y, first.x + step.x, first.y + step.y];
    assert_eq!(animated.uv_bounds, bounds);
    assert!(animated.uv_rect[0] > bounds[0] && animated.uv_rect[2] < bounds[2]);
  }

  fn tall_set(images: &mut Assets<Image>) -> Tileset {
    let image = images.add(Image::solid_rect(16, 32, 9, 9, 9, 255));
    let mut t = Tileset::new(
      "tall",
      UVec2::new(16, 32),
      TilesetImage::Atlas {
        image,
        size: UVec2::new(16, 32),
      },
    );
    t.columns = 1;
    t.tile_count = 1;
    t
  }

  #[test]
  fn a_tile_larger_than_its_cell_grows_up_and_right_from_the_bottom_left() {
    let mut images = Assets::new();
    let t = tall_set(&mut images);
    let layouts = [Some(layout_of(&t, &images))];
    let mut grid = TileGrid::new(UVec2::splat(8));
    grid.set(IVec2::new(2, 3), Some(Tile::new(0, 0)));
    let out = build(&grid, UVec2::ZERO, Vec2::splat(16.0), &layouts);
    let (min, max) = screen_box(&out[0].1);
    // The cell spans (32, 48) to (48, 64); the tile stands on its bottom
    assert_eq!((min, max), (Vec2::new(32.0, 32.0), Vec2::new(48.0, 64.0)));

    // A wide one overhangs to the right
    let mut wide = tall_set(&mut images);
    wide.tile_size = UVec2::new(32, 16);
    let layouts = [Some(layout_of(&wide, &images))];
    let out = build(&grid, UVec2::ZERO, Vec2::splat(16.0), &layouts);
    let (min, max) = screen_box(&out[0].1);
    assert_eq!((min, max), (Vec2::new(32.0, 48.0), Vec2::new(64.0, 64.0)));
  }

  #[test]
  fn the_tileset_s_tile_offset_shifts_its_tiles() {
    let (mut layouts, _) = two_sets();
    let mut grid = TileGrid::new(UVec2::splat(4));
    grid.set(IVec2::new(1, 1), Some(Tile::new(0, 0)));
    grid.set(IVec2::new(2, 2), Some(Tile::new(1, 0)));
    let mut shifted = layouts[0].clone().expect("layout");
    shifted.offset = IVec2::new(3, -5);
    layouts[0] = Some(shifted);
    let out = build(&grid, UVec2::ZERO, Vec2::splat(16.0), &layouts);
    let (min, _) = screen_box(&out[0].1);
    assert_eq!(min, Vec2::new(16.0 + 3.0, 16.0 - 5.0));
    // The other tileset is not shifted
    let (min, _) = screen_box(&out[1].1);
    assert_eq!(min, Vec2::new(32.0, 32.0));
  }

  #[test]
  fn a_diagonally_flipped_tall_tile_lies_on_its_side() {
    let mut images = Assets::new();
    let t = tall_set(&mut images);
    let layouts = [Some(layout_of(&t, &images))];
    let mut grid = TileGrid::new(UVec2::splat(8));
    grid.set(
      IVec2::new(2, 3),
      Some(Tile::new(0, 0).with_flip(TileFlip::DIAGONAL)),
    );
    let out = build(&grid, UVec2::ZERO, Vec2::splat(16.0), &layouts);
    let (min, max) = screen_box(&out[0].1);
    // 32 wide and 16 tall, on the cell's bottom edge, growing right
    assert_eq!((min, max), (Vec2::new(32.0, 48.0), Vec2::new(64.0, 64.0)));
    // Transposed: the tile's top-right corner lands bottom-left
    let c = corners(&out[0].1);
    let top_right_uv = Vec2::new(out[0].1.uv_rect[2], out[0].1.uv_rect[1]);
    let at_bottom_left = c
      .iter()
      .find(|(p, _)| (p.x - min.x).abs() < 1e-4 && (p.y - max.y).abs() < 1e-4)
      .expect("corner");
    assert!((at_bottom_left.1 - top_right_uv).length() < 1e-6);
  }

  #[test]
  fn a_layer_cell_size_places_its_cells() {
    let (layouts, _) = two_sets();
    let mut grid = TileGrid::new(UVec2::splat(4));
    grid.set(IVec2::new(3, 2), Some(Tile::new(0, 0)));
    let out = build(&grid, UVec2::ZERO, Vec2::new(8.0, 12.0), &layouts);
    let (min, max) = screen_box(&out[0].1);
    // 16 px tile on an 8 x 12 cell: bottom-left of the cell (24, 24..36)
    assert_eq!((min, max), (Vec2::new(24.0, 20.0), Vec2::new(40.0, 36.0)));
  }
}
