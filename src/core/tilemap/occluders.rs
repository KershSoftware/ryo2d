//! Map occluders: a spawned map's occluding tile layers cast 2D shadows
//! through `LightOccluder2D` entities, kept in step with their tiles.
//!
//! A layer occludes when its `LayerInfo::occludes` says so, else when it
//! collides (`collides`); a disabled map (or layer) has no occluders, and
//! enabling it builds them from its tiles as they then are. Each occluder
//! is a child of its layer marked `MapOccluder` with the chunk it was built
//! from, its `Transform` at the centre of its geometry (the light's reach
//! test reads the entity's position) and its corners around it, wound as
//! the lighting module takes them. A tile occludes with its shapes marked
//! `occluder`, else with its collision shapes (one-way ones too: light
//! does not care which way a platform lets bodies through), placed where
//! it draws as collision places them; an IntGrid layer's `Solid` cells
//! occlude whole and its slopes as their half cell. Shapes that fill their
//! cell merge into rectangles within each chunk (greedily, row by row);
//! every other shape is an occluder of its own, as drawn (not clipped to
//! its cell). A chunk whose version moved is built again, its occluders
//! replaced and every other chunk's kept.

use glam::{IVec2, UVec2, Vec2};

use super::collision::{
  TilesetStamp, cleaned, full_cell, intgrid_table, placed, stamps, stamps_match, tileset_of,
  twice_area,
};
use super::spawn::{IntGridKey, MapLayers, MapRoot, TileLayerData, root_map};
use super::{CHUNK_SIZE, LayerInfo, LdtkProject, Tile, TileMap, Tileset};
use crate::core::asset::Assets;
use crate::core::ecs::{
  Allow, ChildOf, Commands, Component, DetectChanges, Disabled, Entity, Has, Query, Ref, Res,
  Transform,
};
use crate::core::hash::FastMap;
use crate::core::lighting::LightOccluder2D;
use crate::core::physics::{SlopeCorner, TileCell};

/// Marks an occluder a map's tile layer keeps from its tiles: a child of
/// the layer, rebuilt with its chunk. Games can find a map's shadows by it;
/// the map owns them, so an edit to one lasts until its chunk changes
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapOccluder {
  /// The chunk of the layer it was built from
  pub chunk: UVec2,
}

/// A cell's occluding geometry, in tiles from its top-left
#[derive(Debug, Default)]
struct Occluding {
  /// A shape fills the cell
  full: bool,
  /// The other shapes, each an occluder
  polygons: Vec<Vec<Vec2>>,
}

/// A half cell filled at `corner`, in tiles
fn slope(corner: SlopeCorner) -> Vec<Vec2> {
  let (tl, tr, br, bl) = (Vec2::ZERO, Vec2::X, Vec2::ONE, Vec2::Y);
  match corner {
    SlopeCorner::BottomLeft => vec![tl, br, bl],
    SlopeCorner::BottomRight => vec![tr, br, bl],
    SlopeCorner::TopLeft => vec![tl, tr, bl],
    SlopeCorner::TopRight => vec![tl, tr, br],
  }
}

/// How an IntGrid cell of `kind` occludes: whole if solid, a half cell if
/// a slope, not at all otherwise (a one-way cell is solid only on its top
/// edge, which has no area to block light with)
fn intgrid_occluding(kind: TileCell) -> Occluding {
  match kind {
    TileCell::Solid => Occluding {
      full: true,
      polygons: Vec::new(),
    },
    TileCell::Slope(corner) => Occluding {
      full: false,
      polygons: vec![slope(corner)],
    },
    _ => Occluding::default(),
  }
}

/// How `tile` occludes on cells of `cell` pixels: its shapes marked
/// `occluder`, else its collision shapes, placed where it draws
fn tile_occluding(tileset: Option<&Tileset>, tile: Tile, cell: Vec2) -> Occluding {
  let Some((tileset, data)) = tileset.and_then(|t| Some((t, t.tile(tile.id)?))) else {
    return Occluding::default();
  };
  let marked = data.shapes.iter().any(|s| s.occluder);
  let mut occluding = Occluding::default();
  for shape in data.shapes.iter().filter(|s| s.occluder == marked) {
    let polygon = placed(tileset, tile, &shape.points, cell);
    if full_cell(&polygon) {
      occluding.full = true;
    } else {
      occluding.polygons.push(polygon);
    }
  }
  occluding
}

/// The rectangles (min and size, in cells) the set cells of a `w` x `h`
/// block merge into, greedily: each from its first free cell, as wide as
/// it runs, then down while the rows below run as wide
fn rects(full: &[bool], w: usize, h: usize) -> Vec<(UVec2, UVec2)> {
  let mut taken = vec![false; w * h];
  let free = |taken: &[bool], x: usize, y: usize| full[y * w + x] && !taken[y * w + x];
  let mut found = Vec::new();
  for y in 0..h {
    for x in 0..w {
      if !free(&taken, x, y) {
        continue;
      }
      let mut width = 1;
      while x + width < w && free(&taken, x + width, y) {
        width += 1;
      }
      let mut height = 1;
      while y + height < h && (x..x + width).all(|cx| free(&taken, cx, y + height)) {
        height += 1;
      }
      for row in y..y + height {
        taken[row * w + x..row * w + x + width].fill(true);
      }
      found.push((
        UVec2::new(x as u32, y as u32),
        UVec2::new(width as u32, height as u32),
      ));
    }
  }
  found
}

/// What a layer's occluders were built from, and its occluders by chunk.
/// On the layer entity
#[derive(Component, Debug)]
pub(crate) struct LayerShadows {
  cell: UVec2,
  size: UVec2,
  intgrid: bool,
  tilesets: Vec<TilesetStamp>,
  overrides: Vec<(IntGridKey, TileCell)>,
  /// Each chunk's version as last built (0 for none yet)
  versions: Vec<u64>,
  grid_version: u64,
  /// Each chunk's occluders
  chunks: Vec<Vec<Entity>>,
  /// Each tile's geometry, worked out once
  kinds: FastMap<Tile, Occluding>,
  /// An IntGrid layer's kind of each value
  table: Option<FastMap<i32, TileCell>>,
}

impl LayerShadows {
  fn new(
    data: &TileLayerData,
    tile_map: &TileMap,
    tilesets: Option<&Assets<Tileset>>,
    overrides: &[(IntGridKey, TileCell)],
  ) -> LayerShadows {
    let chunks = data.grid.chunks();
    let count = chunks.x as usize * chunks.y as usize;
    LayerShadows {
      cell: data.cell_size,
      size: data.grid.size(),
      intgrid: data.intgrid.is_some(),
      tilesets: stamps(tile_map, tilesets),
      overrides: overrides.to_vec(),
      versions: vec![0; count],
      grid_version: 0,
      chunks: vec![Vec::new(); count],
      kinds: FastMap::default(),
      table: data
        .intgrid
        .as_ref()
        .map(|intgrid| intgrid_table(&intgrid.names, overrides)),
    }
  }

  /// Whether it was built from what the layer and its map are now
  fn current(
    &self,
    data: &TileLayerData,
    tile_map: &TileMap,
    tilesets: Option<&Assets<Tileset>>,
    overrides: &[(IntGridKey, TileCell)],
  ) -> bool {
    self.cell == data.cell_size
      && self.size == data.grid.size()
      && self.intgrid == data.intgrid.is_some()
      && self.overrides == overrides
      && stamps_match(&self.tilesets, tile_map, tilesets)
  }

  /// Despawns every occluder
  fn clear(&mut self, commands: &mut Commands) {
    for chunk in &mut self.chunks {
      for occluder in chunk.drain(..) {
        commands.entity(occluder).try_despawn();
      }
    }
  }
}

/// What building a layer's occluders reads
struct Context<'a> {
  layer: Entity,
  tile_map: &'a TileMap,
  tilesets: Option<&'a Assets<Tileset>>,
  overrides: &'a [(IntGridKey, TileCell)],
}

/// Builds again the chunks of a layer whose version moved since it last
/// looked (every chunk, when its IntGrid's names changed what they name)
fn update(
  shadows: &mut LayerShadows,
  data: &Ref<TileLayerData>,
  context: &Context<'_>,
  commands: &mut Commands,
) {
  let mut all = false;
  if data.is_changed()
    && let (Some(table), Some(intgrid)) = (&mut shadows.table, &data.intgrid)
  {
    let now = intgrid_table(&intgrid.names, context.overrides);
    if now != *table {
      *table = now;
      all = true;
    }
  }
  let grid = &data.grid;
  if !all && grid.version() == shadows.grid_version {
    return;
  }
  let chunks = grid.chunks();
  for y in 0..chunks.y {
    for x in 0..chunks.x {
      let index = (y * chunks.x + x) as usize;
      let version = grid.chunk_version(UVec2::new(x, y));
      if all || shadows.versions[index] != version {
        shadows.versions[index] = version;
        build_chunk(shadows, UVec2::new(x, y), index, data, context, commands);
      }
    }
  }
  shadows.grid_version = grid.version();
}

/// Replaces chunk `chunk`'s occluders with those of its cells as they are
fn build_chunk(
  shadows: &mut LayerShadows,
  chunk: UVec2,
  index: usize,
  data: &TileLayerData,
  context: &Context<'_>,
  commands: &mut Commands,
) {
  for occluder in shadows.chunks[index].drain(..) {
    commands.entity(occluder).try_despawn();
  }
  let size = data.cell_size.as_vec2();
  let min = chunk * CHUNK_SIZE;
  let max = (min + UVec2::splat(CHUNK_SIZE)).min(data.grid.size());
  let extent = max - min;
  let (w, h) = (extent.x as usize, extent.y as usize);
  let mut full = vec![false; w * h];
  let mut spawned = Vec::new();
  let spawn = |commands: &mut Commands, at: Vec2, occluder: LightOccluder2D| {
    commands
      .spawn((
        Transform::from_translation(at),
        occluder,
        MapOccluder { chunk },
        ChildOf(context.layer),
      ))
      .id()
  };
  for y in 0..h {
    for x in 0..w {
      let cell = (min + UVec2::new(x as u32, y as u32)).as_ivec2();
      let owned;
      let occluding = match &shadows.table {
        Some(table) => {
          let kind = data
            .intgrid
            .as_ref()
            .and_then(|intgrid| intgrid.get(cell))
            .and_then(|value| table.get(&value).copied())
            .unwrap_or(TileCell::Empty);
          owned = intgrid_occluding(kind);
          &owned
        }
        None => match data.grid.get(cell) {
          Some(tile) => shadows.kinds.entry(tile).or_insert_with(|| {
            let tileset = tileset_of(&context.tile_map.tilesets, tile.tileset, context.tilesets);
            tile_occluding(tileset, tile, size)
          }),
          None => continue,
        },
      };
      full[y * w + x] = occluding.full;
      for polygon in &occluding.polygons {
        if let Some((at, corners)) = polygon_at(cell, size, polygon) {
          spawned.push(spawn(commands, at, LightOccluder2D::polygon(corners)));
        }
      }
    }
  }
  for (at, cells) in rects(&full, w, h) {
    let corner = (min + at).as_vec2() * size;
    let extent = cells.as_vec2() * size;
    let occluder = LightOccluder2D::rect(extent.x, extent.y);
    spawned.push(spawn(commands, corner + extent * 0.5, occluder));
  }
  shadows.chunks[index] = spawned;
}

/// A polygon (tiles from `cell`'s top-left) in the layer's pixels, as an
/// occluder's position and corners around it, wound as the lighting module
/// takes them (its outward normals facing away); `None` for one with no
/// area
fn polygon_at(cell: IVec2, size: Vec2, polygon: &[Vec2]) -> Option<(Vec2, Vec<Vec2>)> {
  let mut points: Vec<Vec2> = cleaned(polygon)
    .iter()
    .map(|p| (cell.as_vec2() + *p) * size)
    .collect();
  if points.len() < 3 || !points.iter().all(|p| p.is_finite()) {
    return None;
  }
  let area = twice_area(&points);
  if area.abs() <= f32::EPSILON {
    return None;
  }
  // Lighting's counter-clockwise on screen is a negative area with y down
  if area > 0.0 {
    points.reverse();
  }
  let low = points.iter().copied().fold(Vec2::MAX, Vec2::min);
  let high = points.iter().copied().fold(Vec2::MIN, Vec2::max);
  let at = (low + high) * 0.5;
  Some((at, points.into_iter().map(|p| p - at).collect()))
}

/// The asset stores occluders read
type Stores<'w> = (
  Option<Res<'w, Assets<TileMap>>>,
  Option<Res<'w, Assets<LdtkProject>>>,
  Option<Res<'w, Assets<Tileset>>>,
);
type Maps<'w, 's> =
  Query<'w, 's, (&'static MapRoot, Option<&'static MapLayers>, Has<Disabled>), Allow<Disabled>>;
type Layers<'w, 's> = Query<
  'w,
  's,
  (
    &'static LayerInfo,
    Ref<'static, TileLayerData>,
    Option<&'static mut LayerShadows>,
    Has<Disabled>,
  ),
  Allow<Disabled>,
>;

/// `PostUpdate`, before transforms propagate: every spawned map's occluding
/// layers keep an occluder per merged rectangle and per other shape of
/// their tiles, each chunk built again when its version moves. An idle
/// frame costs a look at each tile layer's version and its map's tilesets
pub(crate) fn sync_map_occluders(
  maps: Maps,
  mut layers: Layers,
  (tile_maps, projects, tilesets): Stores,
  mut commands: Commands,
) {
  let tilesets = tilesets.as_deref();
  for (root, map_layers, disabled) in &maps {
    let Some(map_layers) = map_layers else {
      continue;
    };
    let tile_map = root_map(root, tile_maps.as_deref(), projects.as_deref());
    for &layer in &map_layers.0 {
      let Ok((info, data, shadows, layer_disabled)) = layers.get_mut(layer) else {
        continue;
      };
      let occludes = !disabled && !layer_disabled && info.occludes.unwrap_or(info.collides);
      if !occludes {
        if let Some(mut shadows) = shadows {
          shadows.clear(&mut commands);
          commands.entity(layer).remove::<LayerShadows>();
        }
        continue;
      }
      // Without its map's asset the tiles cannot be read: what is there stays
      let Some(tile_map) = tile_map else {
        continue;
      };
      let context = Context {
        layer,
        tile_map,
        tilesets,
        overrides: &root.intgrid_collision,
      };
      match shadows {
        Some(mut shadows) => {
          if !shadows.current(&data, tile_map, tilesets, context.overrides) {
            shadows.clear(&mut commands);
            *shadows = LayerShadows::new(&data, tile_map, tilesets, context.overrides);
          }
          update(&mut shadows, &data, &context, &mut commands);
        }
        None => {
          let mut shadows = LayerShadows::new(&data, tile_map, tilesets, context.overrides);
          update(&mut shadows, &data, &context, &mut commands);
          commands.entity(layer).insert(shadows);
        }
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use std::sync::Arc;

  use glam::{IVec2, UVec2, Vec2};

  use super::*;
  use crate::core::asset::{Assets, AssetsView, Handle};
  use crate::core::ecs::{ChildOf, Disabled, Entity, Transform, World};
  use crate::core::lighting::LightOccluder2D;
  use crate::core::physics::{Collider, Physics, TilePart};
  use crate::core::tilemap::test_util::MapWorld;
  use crate::core::tilemap::{
    MapLayerRef, MapRoot, MapsView, Tile, TileData, TileMap, TileShape, Tileset, TilesetImage,
    TilesetSource,
  };

  fn load(world: &mut World, path: &str) -> Handle<TileMap> {
    match AssetsView::new(world).load_now::<TileMap>(path) {
      Ok(handle) => handle,
      Err(error) => panic!("{path}: {}", error.report()),
    }
  }

  fn layers_of(world: &mut World, map: Entity) -> Vec<Entity> {
    let mut found: Vec<(usize, Entity)> = world
      .query::<(Entity, &MapLayerRef)>()
      .iter(world)
      .filter(|(_, l)| l.map == map)
      .map(|(e, l)| (l.index, e))
      .collect();
    found.sort();
    found.into_iter().map(|(_, e)| e).collect()
  }

  /// An occluder of a layer: its entity, chunk and corners in the layer's
  /// space, and where its entity sits
  #[derive(Debug, Clone)]
  struct Found {
    entity: Entity,
    chunk: UVec2,
    at: Vec2,
    corners: Vec<Vec2>,
    local: Vec<Vec2>,
  }

  fn occluders_of(world: &mut World, layer: Entity) -> Vec<Found> {
    let mut found: Vec<Found> = world
      .query::<(Entity, &ChildOf, &MapOccluder, &Transform, &LightOccluder2D)>()
      .iter(world)
      .filter(|(_, child_of, ..)| child_of.parent() == layer)
      .map(|(entity, _, marker, transform, occluder)| Found {
        entity,
        chunk: marker.chunk,
        at: transform.translation,
        corners: occluder
          .vertices()
          .iter()
          .map(|v| *v + transform.translation)
          .collect(),
        local: occluder.vertices().to_vec(),
      })
      .collect();
    found.sort_by_key(|f| f.entity);
    found
  }

  fn count<C: Component>(world: &mut World) -> usize {
    world.query::<&C>().iter(world).count()
  }

  /// The occluder whose corners are exactly `corners` (layer px), any order
  fn with_corners<'a>(found: &'a [Found], corners: &[(f32, f32)]) -> &'a Found {
    let same = |f: &Found| {
      f.corners.len() == corners.len()
        && corners.iter().all(|&(x, y)| {
          f.corners
            .iter()
            .any(|c| (*c - Vec2::new(x, y)).abs().max_element() < 1e-3)
        })
    };
    match found.iter().find(|f| same(f)) {
      Some(f) => f,
      None => panic!("no occluder with corners {corners:?} in {found:#?}"),
    }
  }

  /// The occluder covering exactly the rectangle from `min` to `max`
  fn rect_at(found: &[Found], min: (f32, f32), max: (f32, f32)) -> &Found {
    with_corners(found, &[min, (max.0, min.1), max, (min.0, max.1)])
  }

  /// Twice the signed area, y down: negative for the winding the lighting
  /// module takes as counter-clockwise (its outward normals face away)
  fn signed_area(points: &[Vec2]) -> f32 {
    (0..points.len())
      .map(|i| points[i].perp_dot(points[(i + 1) % points.len()]))
      .sum()
  }

  /// Every occluder sits at its corners' centre, wound as lighting wants
  fn assert_placed(found: &[Found]) {
    for f in found {
      let min = f.local.iter().copied().fold(Vec2::MAX, Vec2::min);
      let max = f.local.iter().copied().fold(Vec2::MIN, Vec2::max);
      assert!(
        ((min + max) * 0.5).abs().max_element() < 1e-3,
        "off its geometry: {f:?}"
      );
      assert!(signed_area(&f.local) < 0.0, "wound clockwise: {f:?}");
    }
  }

  #[test]
  fn occluders_follow_tiles_and_sit_on_their_geometry() {
    let mut maps = MapWorld::new();
    let level = load(&mut maps.world, "tiled/maps/level.tmj");
    let map = maps.spawn(MapRoot::new(level));
    let layers = layers_of(&mut maps.world, map);
    let ground = occluders_of(&mut maps.world, layers[0]);
    assert_placed(&ground);
    assert!(ground.iter().all(|f| f.chunk == UVec2::ZERO));
    // Full cells merge into rectangles: the floor's twelve and the four
    // one-way platform tiles (light does not care which way they let bodies)
    let floor = rect_at(&ground, (0.0, 176.0), (192.0, 192.0));
    assert_eq!(floor.at, Vec2::new(96.0, 184.0));
    rect_at(&ground, (64.0, 112.0), (128.0, 128.0));
    // Every other shape is a polygon of its own: the half block, the slopes
    // as they draw, and the tile's occluder shape (it has no other)
    let half = rect_at(&ground, (288.0, 168.0), (304.0, 176.0));
    assert_eq!(half.at, Vec2::new(296.0, 172.0));
    with_corners(&ground, &[(336.0, 160.0), (336.0, 176.0), (320.0, 176.0)]);
    with_corners(&ground, &[(336.0, 176.0), (352.0, 176.0), (336.0, 160.0)]);
    with_corners(&ground, &[(368.0, 176.0), (368.0, 160.0), (352.0, 160.0)]);
    with_corners(&ground, &[(368.0, 176.0), (384.0, 176.0), (368.0, 160.0)]);
    let light = rect_at(&ground, (164.0, 82.0), (172.0, 94.0));
    assert_eq!(light.at, Vec2::new(168.0, 88.0));
    assert_eq!(ground.len(), 8, "{ground:#?}");
    let more = occluders_of(&mut maps.world, layers[1]);
    assert_placed(&more);
    assert_eq!(more.len(), 1);
    rect_at(&more, (192.0, 176.0), (384.0, 192.0));
    assert!(
      occluders_of(&mut maps.world, layers[2]).is_empty(),
      "a layer that does not collide does not occlude unless it says so"
    );
  }

  #[test]
  fn map_occluders_cast_their_shadows_away_from_a_light() {
    use crate::core::ecs::GlobalTransform;
    use crate::core::lighting::{Light2D, push_shadow_volumes};
    use crate::core::render::batch::Triangles;
    let mut maps = MapWorld::new();
    let level = load(&mut maps.world, "tiled/maps/level.tmj");
    let map = maps.spawn(MapRoot::new(level));
    let layer = layers_of(&mut maps.world, map)[0];
    let ground = occluders_of(&mut maps.world, layer);
    let light = Light2D::point(crate::core::ui::Color::WHITE, 400.0).with_shadows();
    // Lit from straight left of each, an occluder's edges facing away from
    // the light cast its shadow: further right, on average, than those the
    // same corners wound the other way would cast (the side facing it)
    let from = Vec2::new(-200.0, 0.0);
    let mean_edge_x = |occluder: &LightOccluder2D, at: Vec2| {
      let place = GlobalTransform::from_transform(&Transform::from_translation(at));
      let mut triangles = Triangles::default();
      push_shadow_volumes(at + from, &light, &[(place, occluder)], &mut triangles);
      // Each quad starts with its edge's two corners
      let mids: Vec<f32> = triangles
        .vertices
        .chunks(4)
        .map(|quad| (quad[0].position[0] + quad[1].position[0]) * 0.5)
        .collect();
      mids.iter().sum::<f32>() / mids.len().max(1) as f32
    };
    for found in &ground {
      let occluder = maps
        .world
        .get::<LightOccluder2D>(found.entity)
        .expect("an occluder")
        .clone();
      let reversed = LightOccluder2D::polygon(occluder.vertices().iter().rev().copied().collect());
      assert!(
        mean_edge_x(&occluder, found.at) > mean_edge_x(&reversed, found.at),
        "the side facing the light casts: {found:?}"
      );
    }
  }

  /// 16 px tiles: a solid block (0) and a half block (1)
  fn blocks() -> Tileset {
    let mut set = Tileset::new("blocks", UVec2::splat(16), TilesetImage::Collection);
    let block = |y: f32| TileData {
      shapes: vec![TileShape {
        points: vec![
          Vec2::new(0.0, y),
          Vec2::new(16.0, y),
          Vec2::new(16.0, 16.0),
          Vec2::new(0.0, 16.0),
        ],
        ..TileShape::default()
      }],
      ..TileData::default()
    };
    set.insert_tile(0, block(0.0));
    set.insert_tile(1, block(8.0));
    set
  }

  /// A map of `size` cells over `blocks`, one layer called "ground" with a
  /// solid floor along its bottom row
  fn floor_map(world: &mut World, size: UVec2, edit: impl FnOnce(&mut TileMap)) -> Handle<TileMap> {
    let mut map = TileMap::new(size, UVec2::splat(16));
    let set = map.add_tileset(TilesetSource::Embedded(Arc::new(blocks())));
    let layer = map.add_tile_layer("ground");
    if let Some(grid) = map.tiles_mut(layer) {
      for x in 0..size.x as i32 {
        grid.set(IVec2::new(x, size.y as i32 - 1), Some(Tile::new(set, 0)));
      }
    }
    edit(&mut map);
    world.resource_mut::<Assets<TileMap>>().add(map)
  }

  #[test]
  fn a_layer_occludes_as_it_says_else_as_it_collides() {
    let mut maps = MapWorld::new();
    let lit_only = floor_map(&mut maps.world, UVec2::new(4, 2), |map| {
      map.layers[0].info.collides = false;
      map.layers[0].info.occludes = Some(true);
    });
    let lit_only = maps.spawn(MapRoot::new(lit_only));
    let solid_only = floor_map(&mut maps.world, UVec2::new(4, 2), |map| {
      map.layers[0].info.occludes = Some(false);
    });
    let solid_only = maps.spawn(MapRoot::new(solid_only));
    let lit_only = layers_of(&mut maps.world, lit_only)[0];
    assert_eq!(occluders_of(&mut maps.world, lit_only).len(), 1);
    let solid_only = layers_of(&mut maps.world, solid_only)[0];
    assert!(occluders_of(&mut maps.world, solid_only).is_empty());
  }

  #[test]
  fn an_int_grid_occludes_with_its_solid_cells_and_slopes() {
    let mut maps = MapWorld::new();
    let handle = floor_map(&mut maps.world, UVec2::new(4, 2), |map| {
      if let crate::core::tilemap::LayerKind::Tiles { intgrid, .. } = &mut map.layers[0].kind {
        let names = [
          (1, "Solid"),
          (2, "one_way"),
          (3, "slope_bottom_left"),
          (4, "Wall"),
        ];
        *intgrid = Some(crate::core::tilemap::IntGrid {
          size: UVec2::new(4, 2),
          values: vec![1, 3, 2, 4, 0, 0, 0, 0],
          names: names.iter().map(|(v, n)| (*v, n.to_string())).collect(),
        });
      }
    });
    let root =
      MapRoot::new(handle).with_intgrid_collision("wall", crate::core::physics::TileCell::Solid);
    let map = maps.spawn(root);
    let layer = layers_of(&mut maps.world, map)[0];
    let found = occluders_of(&mut maps.world, layer);
    assert_placed(&found);
    // Its IntGrid, not its tiles (the floor along the bottom row): the solid
    // cell, the slope as its half cell, the wall it was told about; the
    // one-way cell has no area to occlude with
    rect_at(&found, (0.0, 0.0), (16.0, 16.0));
    with_corners(&found, &[(16.0, 0.0), (32.0, 16.0), (16.0, 16.0)]);
    rect_at(&found, (48.0, 0.0), (64.0, 16.0));
    assert_eq!(found.len(), 3, "{found:#?}");
  }

  #[test]
  fn an_edit_rebuilds_only_its_chunk_s_occluders() {
    let mut maps = MapWorld::new();
    // Two chunks wide: each chunk merges its own part of the floor
    let handle = floor_map(&mut maps.world, UVec2::new(64, 32), |_| {});
    let map = maps.spawn(MapRoot::new(handle));
    let layer = layers_of(&mut maps.world, map)[0];
    let before = occluders_of(&mut maps.world, layer);
    assert_eq!(before.len(), 2, "{before:#?}");
    rect_at(&before, (0.0, 496.0), (512.0, 512.0));
    let far = rect_at(&before, (512.0, 496.0), (1024.0, 512.0)).clone();
    assert_eq!(far.chunk, UVec2::new(1, 0));
    assert!(MapsView::new(&mut maps.world).set_tile(
      map,
      "ground",
      IVec2::new(5, 10),
      Some(Tile::new(0, 1))
    ));
    maps.post_update();
    let after = occluders_of(&mut maps.world, layer);
    assert_placed(&after);
    assert_eq!(after.len(), 3, "{after:#?}");
    assert!(
      after.iter().any(|f| f.entity == far.entity),
      "the far chunk kept its occluder"
    );
    let near: Vec<&Found> = after.iter().filter(|f| f.chunk == UVec2::ZERO).collect();
    assert_eq!(near.len(), 2);
    assert!(
      near
        .iter()
        .all(|f| before.iter().all(|b| b.entity != f.entity)),
      "the edited chunk's occluders were built again"
    );
    rect_at(&after, (80.0, 168.0), (96.0, 176.0));
    // Clearing it takes it away again; the far chunk still keeps its own
    assert!(MapsView::new(&mut maps.world).set_tile(map, "ground", IVec2::new(5, 10), None));
    maps.post_update();
    let cleared = occluders_of(&mut maps.world, layer);
    assert_eq!(cleared.len(), 2);
    assert!(cleared.iter().any(|f| f.entity == far.entity));
  }

  #[test]
  fn a_disabled_map_casts_no_shadows_until_enabled() {
    let mut maps = MapWorld::new();
    let handle = floor_map(&mut maps.world, UVec2::new(4, 2), |_| {});
    let map = maps.spawn(MapRoot::new(handle));
    assert_eq!(count::<MapOccluder>(&mut maps.world), 1);
    maps.world.entity_mut(map).insert(Disabled);
    maps.frame();
    assert_eq!(count::<MapOccluder>(&mut maps.world), 0);
    assert!(MapsView::new(&mut maps.world).set_tile(
      map,
      "ground",
      IVec2::new(1, 0),
      Some(Tile::new(0, 1))
    ));
    maps.frame();
    assert_eq!(count::<MapOccluder>(&mut maps.world), 0);
    maps.world.entity_mut(map).remove::<Disabled>();
    maps.frame();
    assert_eq!(count::<MapOccluder>(&mut maps.world), 2);
  }

  #[test]
  fn a_despawned_map_leaves_no_colliders_or_occluders() {
    let mut maps = MapWorld::new();
    let level = load(&mut maps.world, "tiled/maps/level.tmj");
    let map = maps.spawn(MapRoot::new(level));
    // A second map offset onto layer-level colliders
    let shifted = floor_map(&mut maps.world, UVec2::new(4, 2), |map| {
      map.layers[0].info.offset = Vec2::new(0.0, 4.0);
    });
    let other = maps.spawn(MapRoot::new(shifted));
    maps.frame();
    assert!(count::<TilePart>(&mut maps.world) > 0);
    assert!(count::<MapOccluder>(&mut maps.world) > 0);
    assert!(!maps.world.resource::<Physics>().collider_entries.is_empty());
    maps.world.despawn(map);
    maps.world.despawn(other);
    maps.frame();
    assert_eq!(count::<TilePart>(&mut maps.world), 0);
    assert_eq!(count::<Collider>(&mut maps.world), 0);
    assert_eq!(count::<MapOccluder>(&mut maps.world), 0);
    assert_eq!(count::<LightOccluder2D>(&mut maps.world), 0);
    assert!(maps.world.resource::<Physics>().collider_entries.is_empty());
  }
}
