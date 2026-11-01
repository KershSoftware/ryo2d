//! Map drawing, CPU half: a chunk of a tile layer becomes `SpriteInstance`s,
//! and [`TileDrawer`] gathers the frame's visible chunks of every spawned
//! map (a y-sorted layer's row by row), building those that changed into
//! the batch's chunk store, with its tile objects, to take their places
//! among the entities' draws; and puts its image layers in the draw list.
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
//! - `effect_extra`: zero; for an animated tile `[c0, r0, columns, 0]`, the
//!   first frame's cell's column and row and the padded image's cells per
//!   row. `uv_rect` and `uv_bounds` are the first frame's. Frame `k` is cell
//!   `first + k` counted row-major in the padded image, rows wrapping at
//!   `columns`: its offset from the first frame is `((c0 + k) % columns -
//!   c0, (c0 + k) / columns)` cells.

use std::hash::{DefaultHasher, Hash, Hasher};

use glam::{Affine2, IVec2, UVec2, Vec2};

use super::batch::{
  BlendMode, ChunkCopy, ChunkKey, ChunkPart, ChunkRow, ChunkStamp, ChunkStore, Filter, TileSlot,
};
use super::draw_list::reach;
use super::entities::{DrawOrder, OrderedDraw, OrderedPayload};
use super::{CameraDraw, DrawChunk, DrawKind, DrawList, SpriteLook, SpriteShading};
use crate::core::asset::{AssetId, Assets, Handle};
use crate::core::ecs::interpolate::render_global;
use crate::core::ecs::kernel::QueryState;
use crate::core::ecs::{
  Allow, Children, DetectChangesMut, Disabled, Entity, GlobalTransform, Has, Mut,
  PreviousGlobalTransform, World,
};
use crate::core::graphics::{Image, RenderLayers, SpriteGpuRenderer, SpriteInstance, TextureId};
use crate::core::hash::{FastMap, FastSet};
use crate::core::tilemap::padded::{PaddedCache, PaddedLayout, PaddedTile};
use crate::core::tilemap::{
  CHUNK_SIZE, ImageLayer, LayerInfo, MapLayerRef, MapLayers, MapObject, MapRoot, TileFlip,
  TileGrid, TileLayerData, Tileset, map_asset,
};
use crate::core::ui::Color;

/// What a layer paints every tile with
#[derive(Clone, Copy, Debug)]
pub(crate) struct LayerLook {
  pub tint: Color,
  pub opacity: f32,
}

/// Appends to `out` (cleared first) an instance for each tile of `chunk`
/// (signed) that has a picture, as `(tileset, row, instance)` (`row` being
/// the cell's row in the layer), grouped by tileset index and in row order
/// within a tileset. A cell whose tileset or tile the map lacks makes none.
pub(crate) fn chunk_instances(
  grid: &TileGrid,
  chunk: IVec2,
  cell_size: Vec2,
  layouts: &[Option<PaddedLayout>],
  look: &LayerLook,
  out: &mut Vec<(u16, i32, SpriteInstance)>,
) {
  out.clear();
  // Row by row, each cell read where it lies, not looked up
  let Some(cells) = grid.chunk_cells(chunk) else {
    return;
  };
  let (origin, side) = (chunk * CHUNK_SIZE as i32, CHUNK_SIZE as usize);
  for (at, &raw) in cells.iter().enumerate() {
    let Some(tile) = TileGrid::decode(raw) else {
      continue;
    };
    let cell = origin + IVec2::new((at % side) as i32, (at / side) as i32);
    let Some(layout) = layouts.get(tile.tileset as usize).and_then(Option::as_ref) else {
      continue;
    };
    let Some(padded) = layout.tile(tile.id) else {
      continue;
    };
    out.push((
      tile.tileset,
      cell.y,
      instance(cell, cell_size, tile.flip, layout, padded, look),
    ));
  }
  // Stable, so each tileset keeps its row order. A chunk of one tileset
  // (or already in order) needs none: the sort would take a scratch buffer
  // of the chunk's size (about 140 KB for a full chunk) to move nothing
  if !out.is_sorted_by_key(|(tileset, ..)| *tileset) {
    out.sort_by_key(|(tileset, ..)| *tileset);
  }
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
  let [u0, v0, u1, v1] = flipped_uv(layout, tile, slot, flip);
  let mut instance = SpriteInstance::new()
    .with_uv(u0, v0, u1, v1)
    .with_tint(look.tint, look.opacity);
  let (x_axis, y_axis) = axes(drawn, diagonal);
  instance.transform[0][..2].copy_from_slice(&x_axis.to_array());
  instance.transform[1][..2].copy_from_slice(&y_axis.to_array());
  instance.transform[2][..2].copy_from_slice(&centre.to_array());
  let image = layout.size.as_vec2();
  let origin = layout.cell_origin(slot).as_vec2();
  let cell_min = origin / image;
  let cell_max = (origin + Vec2::splat(layout.cell as f32)) / image;
  instance.uv_bounds = [cell_min.x, cell_min.y, cell_max.x, cell_max.y];
  if let Some(a) = tile.animation {
    let step = Vec2::splat(layout.cell as f32) / image;
    instance.effect_params = [a.frames as f32, a.frame_duration, step.x, step.y];
    let (column, row) = (slot % layout.columns, slot / layout.columns);
    instance.effect_extra = [column as f32, row as f32, layout.columns as f32, 0.0];
  }
  instance
}

/// The UV rect (0..1 over the padded image) of `tile`'s texels in cell
/// `slot`, its sides swapped for `flip`'s mirrors. Screen x shows the
/// tile's u, or its v under the diagonal flip, which swaps the axes
fn flipped_uv(layout: &PaddedLayout, tile: &PaddedTile, slot: u32, flip: TileFlip) -> [f32; 4] {
  let image = layout.size.as_vec2();
  let origin = layout.cell_origin(slot).as_vec2();
  let inset = ((UVec2::splat(layout.cell) - tile.size) / 2).as_vec2();
  let min = (origin + inset) / image;
  let max = (origin + inset + tile.size.as_vec2()) / image;
  let (mut u, mut v) = ((min.x, max.x), (min.y, max.y));
  let (flip_u, flip_v) = if flip.contains(TileFlip::DIAGONAL) {
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
  [u.0, v.0, u.1, v.1]
}

/// A quad's x and y axes (rows 0 and 1 of its transform) drawing `drawn`
/// pixels: across and down, or, under the diagonal flip, the quad's x
/// (the tile's u) running down and its y across
fn axes(drawn: Vec2, diagonal: bool) -> (Vec2, Vec2) {
  if diagonal {
    (Vec2::new(0.0, drawn.y), Vec2::new(drawn.x, 0.0))
  } else {
    (Vec2::new(drawn.x, 0.0), Vec2::new(0.0, drawn.y))
  }
}

/// A tile object's instance in the world: its tile (the frame for game
/// time `time`, if animated) drawn over `size` pixels centred on the
/// object's origin, placed by `affine` (the object's world transform), with
/// the tile's flips and the layer's look
fn object_instance(
  affine: Affine2,
  size: Vec2,
  flip: TileFlip,
  (layout, tile): (&PaddedLayout, &PaddedTile),
  look: &LayerLook,
  time: f32,
) -> SpriteInstance {
  let slot = match tile.animation {
    Some(a) if a.frames > 1 && a.frame_duration > 0.0 => {
      let beats = (time / a.frame_duration).floor();
      let k = beats
        .rem_euclid(a.frames as f32)
        .clamp(0.0, (a.frames - 1) as f32);
      a.first_slot + k as u32
    }
    Some(a) => a.first_slot,
    None => tile.slot,
  };
  let [u0, v0, u1, v1] = flipped_uv(layout, tile, slot, flip);
  let mut instance = SpriteInstance::new()
    .with_uv(u0, v0, u1, v1)
    .with_tint(look.tint, look.opacity);
  let (x_axis, y_axis) = axes(size, flip.contains(TileFlip::DIAGONAL));
  let x_axis = affine.transform_vector2(x_axis);
  let y_axis = affine.transform_vector2(y_axis);
  instance.transform[0][..2].copy_from_slice(&x_axis.to_array());
  instance.transform[1][..2].copy_from_slice(&y_axis.to_array());
  instance.transform[2][..2].copy_from_slice(&affine.translation.to_array());
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

/// A layer of a spawned map: tiles, an image, or objects (its children)
type LayerItem = (
  &'static MapLayerRef,
  &'static LayerInfo,
  Option<&'static TileLayerData>,
  Option<&'static ImageLayer>,
  &'static GlobalTransform,
  Option<&'static PreviousGlobalTransform>,
  Option<&'static RenderLayers>,
  Option<&'static Children>,
);

/// An object of a spawned map
type ObjectItem = (
  Entity,
  &'static MapObject,
  &'static GlobalTransform,
  Option<&'static PreviousGlobalTransform>,
);

/// What a frame's tile layers are drawn for
pub(crate) struct TileLayerFrame<'a> {
  pub cameras: &'a [CameraDraw],
  /// Render interpolation factor
  pub alpha: f32,
  /// Game time in seconds, which tile objects show an animated tile's
  /// frame for (chunks step theirs on the GPU)
  pub game_time: f32,
}

/// Draws the layers of spawned maps. A tile layer's view (every camera at
/// once, or each camera for a layer with parallax) draws the chunks its
/// cameras can see through the view's slot for each tileset: a plain
/// layer's each tileset's tiles of a chunk as one [`DrawChunk`] at the
/// layer's z, a y-sorted layer's each row of them as one, sorted with the
/// y-sorted entities at its z by the row's bottom edge. Chunks are built
/// into the batch's [`ChunkStore`] when first seen and again when their
/// tiles or what they are drawn from change. An image layer draws as one
/// quad per view, its picture repeating across the view along the axes the
/// editor says (one quad for every camera when it neither repeats nor has
/// parallax, else one per camera); an object layer draws its visible tile
/// objects as sprites.
///
/// Image layers go straight into the draw list. Chunks, rows and tile
/// objects take their places among the entities' draws
/// ([`ordered`](Self::ordered), which `EntityDrawer::collect` merges).
#[derive(Default)]
pub(crate) struct TileDrawer {
  /// The world's padded tilesets, which landings hand paddings to
  padded: Option<PaddedCache>,
  maps: Option<QueryState<MapItem, Allow<Disabled>>>,
  layers: Option<QueryState<LayerItem>>,
  objects: Option<QueryState<ObjectItem>>,
  /// The map being drawn: each tileset's padded layout and texture
  layouts: Vec<Option<PaddedLayout>>,
  textures: Vec<Option<TextureId>>,
  /// The view being drawn: each tileset's slot, once it has one
  slots: Vec<Option<u32>>,
  /// A chunk's parts and rows, copied out of the store
  parts: Vec<ChunkPart>,
  rows: Vec<ChunkRow>,
  /// The frame's draws that sort among the entities', in draw order once
  /// collected
  draws: Vec<OrderedDraw>,
  /// Image layers warned about an image too small to repeat over a view
  warned: FastSet<Entity>,
  /// Image layers' pictures with their transparent colour drawn clear, by
  /// picture and colour (`keyed_image`)
  keyed: FastMap<(AssetId<Image>, [u8; 3]), KeyedImage>,
  /// Frames collected, for letting go of keyed pictures no longer drawn
  frames: u64,
}

/// A picture with its transparent colour drawn clear
struct KeyedImage {
  /// The version of the picture it was made from
  version: Option<u64>,
  handle: Handle<Image>,
  /// The frame it was last drawn in
  drawn: u64,
}

/// Frames a keyed picture is kept after it was last drawn
const KEYED_KEEP_FRAMES: u64 = 120;

/// `source` with its texels of colour `key` clear, made once per version of
/// it (again in place when it changes, keeping the handle) and kept while
/// drawn; `None` while the picture is not loaded
fn keyed_image(
  cache: &mut FastMap<(AssetId<Image>, [u8; 3]), KeyedImage>,
  (source, key): (&Handle<Image>, [u8; 3]),
  images: &mut Assets<Image>,
  frame: u64,
) -> Option<Handle<Image>> {
  let version = images.version(source);
  let id = (source.id(), key);
  if let Some(entry) = cache.get_mut(&id)
    && entry.version == version
  {
    entry.drawn = frame;
    return Some(entry.handle.clone());
  }
  let picture = images.get(source)?;
  let mut data = picture.data().to_vec();
  for texel in data.as_chunks_mut::<4>().0 {
    if texel[..3] == key {
      *texel = [0; 4];
    }
  }
  let keyed = Image::from_rgba(data, picture.width(), picture.height());
  let handle = match cache.remove(&id) {
    Some(old) => match images.get_mut(&old.handle) {
      Some(slot) => {
        *slot = keyed;
        old.handle
      }
      None => images.add(keyed),
    },
    None => images.add(keyed),
  };
  cache.insert(
    id,
    KeyedImage {
      version,
      handle: handle.clone(),
      drawn: frame,
    },
  );
  Some(handle)
}

/// One tile layer as a frame draws it
struct LayerDraw<'a> {
  layer: Entity,
  /// What its draws' ties go by: the layer, or the layer a sub-layer is
  /// part of, so its rows sort with entities as that layer's do
  ties: Entity,
  data: &'a TileLayerData,
  look: LayerLook,
  /// What its chunks are built from besides their tiles
  stamp: u64,
  z: i32,
  lit: bool,
  blend: BlendMode,
  /// Drawn row by row, each sorted by its bottom edge
  y_sorted: bool,
  /// The render layers it is on: its own, else its map's
  layers: RenderLayers,
  /// How far past its cell a tile may draw, in the layer's pixels
  margin: f32,
}

/// How an image or object layer draws
struct LayerStyle {
  z: i32,
  look: LayerLook,
  sprite: SpriteLook,
  y_sorted: bool,
  layers: RenderLayers,
}

impl TileDrawer {
  /// Gathers the frame's map layers: image layers onto `list`, the visible
  /// chunks, rows and tile objects into [`ordered`](Self::ordered),
  /// building into `store` the chunks that changed; then frees the chunks
  /// of layers that are gone and those long undrawn, and the padded images
  /// no map asked for
  pub(crate) fn collect(
    &mut self,
    world: &mut World,
    frame: &TileLayerFrame<'_>,
    gpu: &mut SpriteGpuRenderer,
    store: &mut ChunkStore,
    list: &mut DrawList,
  ) {
    store.begin_frame();
    self.draws.clear();
    self.frames += 1;
    // The padded tilesets are the world's (the engine's world has them from
    // the start), so a map's landing can hand over those it padded ahead.
    // With no map left, the rounds free them
    let cache = self
      .padded
      .get_or_insert_with(|| {
        world
          .get_resource::<PaddedCache>()
          .cloned()
          .unwrap_or_else(|| {
            let cache = PaddedCache::default();
            world.insert_resource(cache.clone());
            cache
          })
      })
      .clone();
    let mut padded = cache.lock();
    padded.begin_round();
    self.layers.get_or_insert_with(|| world.query());
    self.objects.get_or_insert_with(|| world.query());
    let maps = self.maps.get_or_insert_with(|| world.query_filtered());
    if maps.iter(world).next().is_some() {
      world.try_resource_scope(|world, mut images: Mut<Assets<Image>>| {
        let images = images.bypass_change_detection();
        let Some(tilesets) = world.get_resource::<Assets<Tileset>>() else {
          return;
        };
        let world: &World = world;
        let padded = &mut *padded;
        let Self {
          padded: _,
          maps,
          layers,
          objects,
          layouts,
          textures,
          slots,
          parts,
          rows,
          draws,
          warned,
          keyed,
          frames,
        } = self;
        let (Some(maps), Some(layers), Some(objects)) =
          (maps.as_mut(), layers.as_mut(), objects.as_mut())
        else {
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
          for &entity in &map_layers.0 {
            let Ok((
              layer_ref,
              own_info,
              tiles,
              image,
              layer_global,
              layer_previous,
              own,
              children,
            )) = layers.get(world, entity)
            else {
              continue;
            };
            // An LDtk sub-layer draws as part of its layer: that layer's
            // settings, place, z and render layers (a hidden, disabled or
            // gone one takes it along), its own offset and opacity over them
            let (info, ties, layer_ref, layer_global, layer_previous, own, part) = match own_info
              .part_of
            {
              None => (
                own_info,
                entity,
                layer_ref,
                layer_global,
                layer_previous,
                own,
                None,
              ),
              Some(id) => {
                let base = map_layers.0.iter().find_map(|&base| {
                  let item = layers.get(world, base).ok()?;
                  (item.1.id == id && item.1.part_of.is_none()).then_some((base, item))
                });
                let Some((base, (base_ref, base_info, _, _, global, previous, base_own, _))) = base
                else {
                  continue;
                };
                (
                  base_info,
                  base,
                  base_ref,
                  global,
                  previous,
                  base_own,
                  Some(own_info),
                )
              }
            };
            let mask = own.copied().unwrap_or(map_layers_mask);
            if !info.visible || !frame.cameras.iter().any(|c| c.layers.intersects(&mask)) {
              continue;
            }
            let look = LayerLook {
              tint: info.tint,
              opacity: info.opacity * part.map_or(1.0, |part| part.opacity),
            };
            let index = i32::try_from(layer_ref.index).unwrap_or(i32::MAX);
            let z = root.z.saturating_add(index);
            let mut affine = render_global(layer_global, layer_previous, frame.alpha).affine();
            if let Some(part) = part {
              affine *= Affine2::from_translation(part.offset);
            }
            let origin = asset.parallax_origin;
            let views = views(frame.cameras, mask, (map_affine, origin), info.parallax);
            let style = LayerStyle {
              z,
              look,
              sprite: SpriteLook {
                shading: SpriteShading::Plain,
                blend: info.blend,
                filter: Filter::Default,
                lit: info.lit,
              },
              y_sorted: info.y_sorted,
              layers: mask,
            };
            if let Some(data) = tiles {
              if data.cell_size.min_element() == 0 {
                continue;
              }
              let mut stamp = DefaultHasher::new();
              (paddings, look.tint, look.opacity.to_bits(), data.cell_size).hash(&mut stamp);
              let draw = LayerDraw {
                layer: entity,
                ties,
                data,
                look,
                stamp: stamp.finish(),
                z,
                lit: info.lit,
                blend: info.blend,
                y_sorted: info.y_sorted,
                layers: mask,
                margin,
              };
              let mut view = View {
                store: &mut *store,
                draws: &mut *draws,
                layouts,
                textures,
                slots,
                parts,
                rows,
              };
              for (camera, offset) in views {
                match camera {
                  Some(camera) => {
                    let one = std::iter::once(camera);
                    view.draw(&draw, affine, offset, one, Some(camera.entity));
                  }
                  None => {
                    let all = frame.cameras.iter().filter(|c| c.layers.intersects(&mask));
                    view.draw(&draw, affine, Vec2::ZERO, all, None);
                  }
                }
              }
            } else if let Some(image) = image {
              let Some(source) = image.image.as_ref() else {
                continue;
              };
              // A transparent colour draws clear: from a keyed copy
              let id = match image.transparent {
                None => source.id(),
                Some(c) => match keyed_image(keyed, (source, [c.r, c.g, c.b]), images, *frames) {
                  Some(handle) => handle.id(),
                  None => continue,
                },
              };
              let Some(shown) = gpu.shown_image(id, images) else {
                continue;
              };
              let (width, height) = shown.size();
              let texture = gpu.shown_texture(shown);
              let picture = Picture {
                layer: entity,
                texture,
                size: Vec2::new(width as f32, height as f32),
                repeat: (image.repeat_x, image.repeat_y),
              };
              let parallax = ((map_affine, origin), info.parallax);
              let drawn = (&style, &picture, info.name.as_str());
              image_layer(list, frame.cameras, drawn, affine, parallax, warned);
            } else if let Some(children) = children {
              let mut objects = Objects {
                query: objects,
                world,
                layouts,
                textures,
                draws: &mut *draws,
              };
              for &child in children {
                objects.draw(child, &style, frame, views.clone());
              }
            }
          }
        }
      });
    }
    padded.end_round();
    drop(padded);
    store.end_frame(|layer| world.get::<TileLayerData>(layer).is_some());
    if !self.warned.is_empty() {
      self
        .warned
        .retain(|&layer| world.get::<ImageLayer>(layer).is_some());
    }
    if !self.keyed.is_empty() {
      let frame = self.frames;
      self
        .keyed
        .retain(|_, keyed| frame - keyed.drawn < KEYED_KEEP_FRAMES);
    }
    OrderedDraw::sort(&mut self.draws);
  }

  /// The frame's chunks, rows and tile objects, in draw order: each takes
  /// its place among the entities' draws
  pub(crate) fn ordered(&self) -> &[OrderedDraw] {
    &self.draws
  }
}

impl TileDrawer {
  /// How many padded tileset images are kept (tests)
  #[cfg(test)]
  pub(crate) fn padded_count(world: &World) -> usize {
    world
      .get_resource::<PaddedCache>()
      .map_or(0, |cache| cache.lock().len())
  }

  /// How many image layers were warned about an image too small to repeat
  /// over a camera's view (tests)
  #[cfg(test)]
  pub(crate) fn too_small_warnings(&self) -> usize {
    self.warned.len()
  }
}

/// The views a layer on render layers `mask` draws in: one for every camera
/// at once (`None`, no offset) when it has no parallax, else one for each
/// camera on `mask`, moved by that camera's parallax offset. `map` is the
/// map's placement and its parallax origin
fn views(
  cameras: &[CameraDraw],
  mask: RenderLayers,
  map: (Affine2, Vec2),
  factor: Vec2,
) -> impl Iterator<Item = (Option<&CameraDraw>, Vec2)> + Clone {
  let shared = factor == Vec2::ONE;
  let all = shared.then_some((None, Vec2::ZERO));
  let each = cameras
    .iter()
    .filter(move |camera| !shared && camera.layers.intersects(&mask))
    .filter_map(move |camera| Some((Some(camera), parallax(map, factor, camera.view.center)?)));
  all.into_iter().chain(each)
}

/// The world offset a layer of parallax `factor` draws at for a camera
/// centred on `center`, on a map placed by `map` with its parallax origin
/// (in map pixels): the part of the camera's position from that origin the
/// layer does not follow, so a camera centred on the origin shows every
/// layer where it would be without parallax (Tiled's rule). `None` for a
/// map scaled to nothing
fn parallax((map, origin): (Affine2, Vec2), factor: Vec2, center: Vec2) -> Option<Vec2> {
  let det = map.matrix2.determinant();
  if !det.is_finite() || det.abs() < 1e-12 {
    return None;
  }
  let local = map.inverse().transform_point2(center);
  Some(map.transform_vector2((Vec2::ONE - factor) * (local - origin)))
}

/// `affine` moved by `offset`
fn moved(affine: Affine2, offset: Vec2) -> Affine2 {
  Affine2::from_mat2_translation(affine.matrix2, affine.translation + offset)
}

/// What one view of a tile layer draws with
struct View<'s> {
  store: &'s mut ChunkStore,
  draws: &'s mut Vec<OrderedDraw>,
  layouts: &'s [Option<PaddedLayout>],
  textures: &'s [Option<TextureId>],
  slots: &'s mut Vec<Option<u32>>,
  parts: &'s mut Vec<ChunkPart>,
  rows: &'s mut Vec<ChunkRow>,
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
    let placed = moved(affine, offset);
    let det = placed.matrix2.determinant();
    if !det.is_finite() || det.abs() < 1e-12 {
      return;
    }
    let to_layer = placed.inverse();
    let grid = &layer.data.grid;
    // The chunks the layer reaches: all of a bounded layer's, the span of
    // an unbounded one's (whose chunks drawn are the ones it has)
    let extent = grid.extent();
    if extent.is_empty() {
      return;
    }
    let (reach_first, reach_last) = (
      TileGrid::chunk_of(extent.min),
      TileGrid::chunk_of(extent.max - IVec2::ONE),
    );
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
      // may draw past its cell; then the signed chunks it meets, within
      // those the layer reaches
      let (low, high) = camera_box(camera, &to_layer);
      let (low, high) = (low - layer.margin, high + layer.margin);
      let first = (low / chunk_px).floor().max(reach_first.as_vec2());
      let last = (high / chunk_px).floor().min(reach_last.as_vec2());
      // Not finite, or off the layer
      if !(first.x <= last.x && first.y <= last.y) {
        continue;
      }
      let first = first.as_ivec2().max(reach_first);
      let last = last.as_ivec2().min(reach_last);
      if grid.bounds().is_some() {
        for y in first.y..=last.y {
          for x in first.x..=last.x {
            self.chunk(layer, IVec2::new(x, y), view, (&placed, slot), only);
          }
        }
      } else {
        // Only the chunks an unbounded layer has, found by walking the
        // view's chunks or the layer's, whichever are fewer
        for chunk in grid.chunks_within(first, last) {
          self.chunk(layer, chunk, view, (&placed, slot), only);
        }
      }
    }
  }

  /// Pushes `chunk` of `layer` for `view` (built first if it changed),
  /// each tileset's tiles through the view's slot for it: whole, or row by
  /// row for a y-sorted layer
  fn chunk(
    &mut self,
    layer: &LayerDraw<'_>,
    chunk: IVec2,
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
    let build = |out: &mut Vec<(u16, i32, SpriteInstance)>| {
      chunk_instances(grid, chunk, cell, layouts, &layer.look, out);
    };
    // Its parts, and its rows for a y-sorted layer, copied out of the store
    let out = ChunkCopy {
      parts: &mut *self.parts,
      rows: layer.y_sorted.then_some(&mut *self.rows),
    };
    if !self
      .store
      .chunk((layer.layer, chunk), stamp, view, build, out)
    {
      return;
    }
    for part in self.parts.iter() {
      let tileset = usize::from(part.tileset);
      let Some(&Some(texture)) = self.textures.get(tileset) else {
        continue;
      };
      let Some(entry) = self.slots.get_mut(tileset) else {
        continue;
      };
      let slot = *entry.get_or_insert_with(|| self.store.push_slot(slot, texture));
      let chunk_draw = |first, count, (min, max)| DrawChunk {
        key: ChunkKey {
          layer: layer.layer,
          chunk,
          tileset: part.tileset,
        },
        texture,
        first,
        count,
        blend: layer.blend,
        filter: Filter::Default,
        layers: layer.layers,
        min,
        max,
        slot,
      };
      let mut push = |order, chunk| {
        let index = self.draws.len() as u32;
        self.draws.push(OrderedDraw {
          order,
          entity: layer.ties,
          index,
          lit: layer.lit,
          only_camera: only,
          payload: OrderedPayload::Chunk(chunk),
        });
      };
      if !layer.y_sorted {
        let bounds = world_box(placed, part.min, part.max);
        push(DrawOrder::plain(layer.z), chunk_draw(0, part.count, bounds));
        continue;
      }
      // Each row sorts by its bottom edge, at the layer's left (so every
      // chunk of a row sorts alike, on a turned map too)
      for row in self.rows.iter().filter(|row| row.tileset == part.tileset) {
        let bottom = Vec2::new(0.0, (row.row as f32 + 1.0) * cell.y);
        let sort_y = placed.transform_point2(bottom).y;
        let bounds = world_box(placed, row.min, row.max);
        let order = DrawOrder::y_sorted(layer.z, sort_y);
        push(order, chunk_draw(row.first, row.count, bounds));
      }
    }
  }
}

/// `camera`'s view under `to_layer`: the box its corners span there
fn camera_box(camera: &CameraDraw, to_layer: &Affine2) -> (Vec2, Vec2) {
  let b = camera.view.bounds;
  let corners = [
    Vec2::new(b.left, b.top),
    Vec2::new(b.right, b.top),
    Vec2::new(b.left, b.bottom),
    Vec2::new(b.right, b.bottom),
  ]
  .map(|corner| to_layer.transform_point2(corner));
  let low = corners.iter().fold(Vec2::MAX, |m, c| m.min(*c));
  let high = corners.iter().fold(Vec2::MIN, |m, c| m.max(*c));
  (low, high)
}

/// The most copies of its image a repeated image layer draws along an axis
/// for one camera: an 8 px image covers a 4K view at the smallest zoom
const MAX_REPEATS: f32 = 65_536.0;
/// The most of its image's texels a repeated image layer's quad spans along
/// an axis: its UVs, counted in copies, keep a sixteenth of a texel there
const MAX_REPEAT_TEXELS: f32 = 524_288.0;

/// An image layer's picture
struct Picture {
  /// The layer entity, which a warning is given once for
  layer: Entity,
  texture: TextureId,
  /// In the layer's pixels
  size: Vec2,
  /// Whether it repeats across, and down
  repeat: (bool, bool),
}

/// The copies of an image `size` px along an axis that a view spanning
/// `low..high` there (centred on `centre`) needs: the first and how many,
/// and whether they had to be cut to [`MAX_REPEATS`] (or
/// [`MAX_REPEAT_TEXELS`]), in which case they lie about the centre. One
/// copy, the first, along an axis that does not repeat
fn copies(
  repeats: bool,
  (low, high, centre): (f32, f32, f32),
  size: f32,
) -> Option<(f32, f32, bool)> {
  if !repeats {
    return Some((0.0, 1.0, false));
  }
  let (first, last) = ((low / size).floor(), (high / size).floor());
  if !(first.is_finite() && last.is_finite()) {
    return None;
  }
  let cap = (MAX_REPEAT_TEXELS / size).floor().clamp(1.0, MAX_REPEATS);
  let count = last - first + 1.0;
  if count <= cap {
    return Some((first, count, false));
  }
  let middle = (centre / size).floor();
  Some((middle - (cap / 2.0).floor(), cap, true))
}

/// Pushes an image layer's picture onto `list` at the layer's z, after the
/// entities there, as one quad over the copies a view needs
/// (`SpriteShading::Repeat`): the image once at the layer's origin, or
/// repeated along the axes the editor says over a camera's view. The quad
/// is placed as a tile layer is, the layer's origin snapped to a whole pixel
/// once on the pixel-art target, so no gap opens between copies at any
/// zoom. A picture that neither repeats nor has parallax is one quad for
/// every camera; otherwise each camera on its layers gets its own, moved by
/// its parallax offset. An image too small to cover a camera's view in
/// [`MAX_REPEATS`] copies covers that many about the view's centre, and its
/// layer is warned about once (`warned`)
fn image_layer(
  list: &mut DrawList,
  cameras: &[CameraDraw],
  (style, picture, name): (&LayerStyle, &Picture, &str),
  affine: Affine2,
  (map, factor): ((Affine2, Vec2), Vec2),
  warned: &mut FastSet<Entity>,
) {
  let size = picture.size;
  if !(size.x > 0.0 && size.y > 0.0) {
    return;
  }
  let look = SpriteLook {
    shading: SpriteShading::Repeat,
    ..style.sprite
  };
  // The copies `first..first + count` (in images along each axis)
  let mut push = |placed: Affine2, first: Vec2, count: Vec2, only: Option<Entity>| {
    let extent = count * size;
    let centre = placed.transform_point2(first * size + extent * 0.5);
    let x_axis = placed.transform_vector2(Vec2::new(extent.x, 0.0));
    let y_axis = placed.transform_vector2(Vec2::new(0.0, extent.y));
    let origin = placed.translation;
    if !(centre.is_finite() && x_axis.is_finite() && y_axis.is_finite() && origin.is_finite()) {
      return;
    }
    let mut instance = SpriteInstance::new()
      .with_uv(0.0, 0.0, count.x, count.y)
      .with_tint(style.look.tint, style.look.opacity);
    instance.transform[0][..3].copy_from_slice(&[x_axis.x, x_axis.y, origin.x]);
    instance.transform[1][..3].copy_from_slice(&[y_axis.x, y_axis.y, origin.y]);
    instance.transform[2][..2].copy_from_slice(&centre.to_array());
    let seen = (style.layers, None, only);
    list.push_sprite_in(
      DrawKind::Sprite,
      style.z,
      picture.texture,
      look,
      instance,
      seen,
    );
  };
  let (repeat_x, repeat_y) = picture.repeat;
  if !repeat_x && !repeat_y && factor == Vec2::ONE {
    push(affine, Vec2::ZERO, Vec2::ONE, None);
    return;
  }
  for camera in cameras
    .iter()
    .filter(|c| c.layers.intersects(&style.layers))
  {
    let offset = if factor == Vec2::ONE {
      Vec2::ZERO
    } else {
      let Some(offset) = parallax(map, factor, camera.view.center) else {
        continue;
      };
      offset
    };
    let placed = moved(affine, offset);
    let det = placed.matrix2.determinant();
    if !det.is_finite() || det.abs() < 1e-12 {
      continue;
    }
    // The copies the camera's view meets, along the repeating axes
    let to_layer = placed.inverse();
    let (low, high) = camera_box(camera, &to_layer);
    let centre = to_layer.transform_point2(camera.view.center);
    let across = copies(repeat_x, (low.x, high.x, centre.x), size.x);
    let down = copies(repeat_y, (low.y, high.y, centre.y), size.y);
    let (Some((x, columns, cut_x)), Some((y, rows, cut_y))) = (across, down) else {
      continue;
    };
    if (cut_x || cut_y) && warned.insert(picture.layer) {
      crate::engine_warn!(
        module: "Tilemap",
        "image layer {name:?} repeats a {}x{} px image, too small to cover a camera's view in {} copies; it covers the middle of the view",
        size.x,
        size.y,
        MAX_REPEATS
      );
    }
    let first = Vec2::new(x, y);
    push(placed, first, Vec2::new(columns, rows), Some(camera.entity));
  }
}

/// What an object layer's tile objects draw with
struct Objects<'s, 'w> {
  query: &'s mut QueryState<ObjectItem>,
  world: &'w World,
  layouts: &'s [Option<PaddedLayout>],
  textures: &'s [Option<TextureId>],
  draws: &'s mut Vec<OrderedDraw>,
}

impl Objects<'_, '_> {
  /// Pushes `entity`, if it is a visible tile object whose tile has a
  /// picture, for each of `views` whose cameras may see it: among the
  /// entities at the layer's z, sorted by its bottom edge on a y-sorted
  /// layer
  fn draw<'c>(
    &mut self,
    entity: Entity,
    style: &LayerStyle,
    frame: &TileLayerFrame<'c>,
    views: impl Iterator<Item = (Option<&'c CameraDraw>, Vec2)>,
  ) {
    let Ok((entity, object, global, previous)) = self.query.get(self.world, entity) else {
      return;
    };
    let Some(tile) = object.tile.filter(|_| object.visible) else {
      return;
    };
    let tileset = usize::from(tile.tileset);
    let (Some(Some(layout)), Some(&Some(texture))) =
      (self.layouts.get(tileset), self.textures.get(tileset))
    else {
      return;
    };
    let Some(padded) = layout.tile(tile.id) else {
      return;
    };
    let affine = render_global(global, previous, frame.alpha).affine();
    for (camera, offset) in views {
      let placed = moved(affine, offset);
      let instance = object_instance(
        placed,
        object.size,
        tile.flip,
        (layout, padded),
        &style.look,
        frame.game_time,
      );
      let (min, max) = reach(&instance);
      let sees = |c: &CameraDraw| c.layers.intersects(&style.layers) && c.view.is_visible(min, max);
      let seen = match camera {
        Some(camera) => sees(camera),
        None => frame.cameras.iter().any(sees),
      };
      if !seen || !min.is_finite() || !max.is_finite() {
        continue;
      }
      let order = if style.y_sorted {
        let feet = placed.transform_point2(Vec2::new(0.0, object.size.y * 0.5));
        DrawOrder::y_sorted(style.z, feet.y)
      } else {
        DrawOrder::plain(style.z)
      };
      let index = self.draws.len() as u32;
      self.draws.push(OrderedDraw {
        order,
        entity,
        index,
        lit: style.sprite.lit,
        only_camera: camera.map(|c| c.entity),
        payload: OrderedPayload::Sprite {
          texture,
          look: style.sprite,
          instance,
          layers: style.layers,
        },
      });
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
    pad_tileset(tileset, images, u32::MAX).expect("padded").1
  }

  fn build(
    grid: &TileGrid,
    chunk: IVec2,
    cell: Vec2,
    layouts: &[Option<PaddedLayout>],
  ) -> Vec<(u16, SpriteInstance)> {
    let mut out = Vec::new();
    chunk_instances(grid, chunk, cell, layouts, &LOOK, &mut out);
    out.into_iter().map(|(set, _, i)| (set, i)).collect()
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
      IVec2::ZERO,
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
    let out = build(&grid, IVec2::new(1, 0), Vec2::splat(16.0), &layouts);
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
    // Each knows its cell's row
    let mut rows = Vec::new();
    chunk_instances(
      &grid,
      IVec2::new(1, 0),
      Vec2::splat(16.0),
      &layouts,
      &LOOK,
      &mut rows,
    );
    let rows: Vec<(u16, i32)> = rows.iter().map(|(s, r, _)| (*s, *r)).collect();
    assert_eq!(rows, [(0, 0), (0, 2), (1, 0), (1, 2)]);
    // The last chunk of a grid is cut at the grid's edge
    let edge = build(&grid, IVec2::new(1, 1), Vec2::splat(16.0), &layouts);
    assert_eq!(edge.len(), 1);
  }

  #[test]
  fn empty_cells_make_no_instance() {
    let (layouts, _) = two_sets();
    let grid = TileGrid::new(UVec2::splat(10));
    assert!(build(&grid, IVec2::ZERO, Vec2::splat(16.0), &layouts).is_empty());
    // A chunk outside the grid too
    assert!(build(&grid, IVec2::new(3, 3), Vec2::splat(16.0), &layouts).is_empty());
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
    let out = build(&grid, IVec2::ZERO, Vec2::splat(16.0), &layouts);
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
      IVec2::ZERO,
      Vec2::splat(16.0),
      &layouts,
      &look,
      &mut out,
    );
    assert_eq!(out[0].2.tint, [1.0, 0.0, 0.2, 0.5]);
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
      IVec2::ZERO,
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
    // The first frame's column and row (not its UV: the cells start a
    // margin into the image)
    let (column, row) = (
      anim.first_slot % layout.columns,
      anim.first_slot / layout.columns,
    );
    assert_eq!(
      animated.effect_extra,
      [column as f32, row as f32, layout.columns as f32, 0.0]
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
    let out = build(&grid, IVec2::ZERO, Vec2::splat(16.0), &layouts);
    let (min, max) = screen_box(&out[0].1);
    // The cell spans (32, 48) to (48, 64); the tile stands on its bottom
    assert_eq!((min, max), (Vec2::new(32.0, 32.0), Vec2::new(48.0, 64.0)));

    // A wide one overhangs to the right
    let mut wide = tall_set(&mut images);
    wide.tile_size = UVec2::new(32, 16);
    let layouts = [Some(layout_of(&wide, &images))];
    let out = build(&grid, IVec2::ZERO, Vec2::splat(16.0), &layouts);
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
    let out = build(&grid, IVec2::ZERO, Vec2::splat(16.0), &layouts);
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
    let out = build(&grid, IVec2::ZERO, Vec2::splat(16.0), &layouts);
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
    let out = build(&grid, IVec2::ZERO, Vec2::new(8.0, 12.0), &layouts);
    let (min, max) = screen_box(&out[0].1);
    // 16 px tile on an 8 x 12 cell: bottom-left of the cell (24, 24..36)
    assert_eq!((min, max), (Vec2::new(24.0, 20.0), Vec2::new(40.0, 36.0)));
  }
}
