//! Map occluders: a spawned map's occluding tile layers cast 2D shadows
//! through `LightOccluder2D` chains, kept in step with their tiles.
//!
//! A layer occludes when its `LayerInfo::occludes` says so, else when it
//! collides (`collides`); a disabled map (or layer) has no occluders, and
//! enabling it builds them from its tiles as they then are. Occluding
//! layers sharing a cell size and offset occlude as one: each cell takes
//! the strongest kind across them, as collision's union does. What a cell
//! occludes with: a tile's shapes marked `occluder`, else its collision
//! shapes (one-way ones too: light does not care which way a platform lets
//! bodies through), all placed where it draws as collision places them; an
//! IntGrid's solid cells whole and its slopes as their half cell.
//!
//! The occluders are the boundary of what occludes, worked out by the tile
//! colliders' outline units (`physics::Boundary`): an edge where one side
//! occludes and the other does not, shapes fitted to their cells on the
//! outline grid, edges two cells share cancelling, across chunk borders too.
//! So an edge inside a solid mass never casts its shadow through the lit
//! art beyond it. Each chunk owns the borders on the left of and above its
//! cells (and the grid's right and bottom ones where it reaches them); its
//! edges are chained into open chains (`LightOccluder2D::chain`), each a
//! child of the group's first layer marked `MapOccluder` with its chunk, its
//! entity at the centre of its corners' box (the light's reach test reads
//! the entity's position). A chunk whose version moved is built again, and
//! so is the chunk right of it or below it when the cells it shares a
//! border with changed kind; every other chunk keeps its occluders.

use glam::{IVec2, UVec2, Vec2};

use super::collision::{
  Context, Layers, Purpose, ShapeKey, Stores, TilesetStamp, Tracked, changed_chunks, layer_data,
  layer_kind, over, stamps, stamps_match,
};
use super::spawn::{IntGridKey, MapLayers, MapRoot, root_map};
use super::{CHUNK_SIZE, Tile};
use crate::core::ecs::{
  Allow, ChildOf, Commands, Component, DetectChangesMut, Disabled, Entity, Has, Query, Ref,
  Transform,
};
use crate::core::hash::FastMap;
use crate::core::lighting::LightOccluder2D;
use crate::core::physics::{Boundary, GRID, Point, TileCell};

/// Marks an occluder a map keeps from its tiles: a child of the first layer
/// of the layers occluding as one, rebuilt with its chunk. Games can find a
/// map's shadows by it; the map owns them, so an edit to one lasts until
/// its chunk changes
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapOccluder {
  /// The chunk of the layers it was built from
  pub chunk: UVec2,
}

/// An occluding layer as the occluders were built from it
#[derive(Clone, PartialEq, Debug)]
pub(crate) struct Lit {
  entity: Entity,
  cell: UVec2,
  offset: [u32; 2],
  size: UVec2,
  intgrid: bool,
}

/// What a chunk's cells along its right and bottom sides were when it was
/// last built: the chunks right of it and below read them
#[derive(Debug, Default, PartialEq)]
struct Sides {
  right: Vec<TileCell>,
  bottom: Vec<TileCell>,
}

/// Layers sharing a cell size and offset, occluding as one
#[derive(Debug)]
struct Group {
  cell: UVec2,
  offset: [u32; 2],
  /// Its occluders' parent: its first layer
  parent: Entity,
  size: UVec2,
  layers: Vec<Tracked>,
  /// Each tile's kind and the shapes they name
  kinds: FastMap<Tile, TileCell>,
  shapes: FastMap<ShapeKey, u16>,
  boundary: Boundary,
  /// Each chunk's occluders and sides
  chunks: Vec<Vec<Entity>>,
  sides: Vec<Sides>,
}

impl Group {
  fn chunk_count(&self) -> UVec2 {
    UVec2::new(
      self.size.x.div_ceil(CHUNK_SIZE),
      self.size.y.div_ceil(CHUNK_SIZE),
    )
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

/// A map's occluders: what they were built from and its groups. On the map
/// entity
#[derive(Component, Debug)]
pub(crate) struct MapShadows {
  layers: Vec<Lit>,
  tilesets: Vec<TilesetStamp>,
  overrides: Vec<(IntGridKey, TileCell)>,
  groups: Vec<Group>,
}

impl MapShadows {
  fn clear(&mut self, commands: &mut Commands) {
    for group in &mut self.groups {
      group.clear(commands);
    }
  }
}

/// Chains `edges` (directed) into as few open chains as it can greedily:
/// from each point with more edges leaving than arriving, then round what
/// is left (closed loops, each a chain ending where it starts). Straight
/// runs merge into one edge
fn chains(edges: &[(Point, Point)]) -> Vec<Vec<Point>> {
  let mut leaving: FastMap<Point, Vec<usize>> = FastMap::default();
  let mut balance: FastMap<Point, i32> = FastMap::default();
  for (i, &(a, b)) in edges.iter().enumerate() {
    leaving.entry(a).or_default().push(i);
    *balance.entry(a).or_insert(0) += 1;
    *balance.entry(b).or_insert(0) -= 1;
  }
  let mut used = vec![false; edges.len()];
  let mut out = Vec::new();
  // Heads first (sorted, so the chains do not depend on hashing), then
  // whatever edges are left, in order
  let mut heads: Vec<Point> = balance
    .iter()
    .filter(|(_, n)| **n > 0)
    .map(|(p, _)| *p)
    .collect();
  heads.sort_unstable();
  let starts = heads
    .into_iter()
    .chain(edges.iter().map(|&(a, _)| a))
    .collect::<Vec<_>>();
  for start in starts {
    loop {
      let mut chain = vec![start];
      let mut at = start;
      while let Some(next) = leaving
        .get_mut(&at)
        .and_then(|list| list.iter().position(|&i| !used[i]).map(|k| list.remove(k)))
      {
        used[next] = true;
        let to = edges[next].1;
        // A straight run is one edge
        if chain.len() >= 2 {
          let (p, q) = (chain[chain.len() - 2], chain[chain.len() - 1]);
          let (u, v) = (
            (i64::from(q.0 - p.0), i64::from(q.1 - p.1)),
            (i64::from(to.0 - q.0), i64::from(to.1 - q.1)),
          );
          if u.0 * v.1 - u.1 * v.0 == 0 && u.0 * v.0 + u.1 * v.1 > 0 {
            chain.pop();
          }
        }
        chain.push(to);
        at = to;
      }
      if chain.len() < 2 {
        break;
      }
      out.push(chain);
      // A head with more edges leaving starts another chain
      if !leaving
        .get(&start)
        .is_some_and(|l| l.iter().any(|&i| !used[i]))
      {
        break;
      }
    }
  }
  out
}

/// Builds chunk `chunk` (index `index`) of `group` again from its layers'
/// cells: its occluders replaced by the chains of the boundary edges it
/// owns. Returns whether its right and bottom sides changed kind (the
/// chunks across them read those cells)
fn build_chunk(
  group: &mut Group,
  chunk: UVec2,
  index: usize,
  data: &[Option<Ref<super::TileLayerData>>],
  context: &Context<'_>,
  commands: &mut Commands,
) -> (bool, bool) {
  for occluder in group.chunks[index].drain(..) {
    commands.entity(occluder).try_despawn();
  }
  let size = group.size.as_ivec2();
  let min = (chunk * CHUNK_SIZE).as_ivec2();
  let max = (min + IVec2::splat(CHUNK_SIZE as i32)).min(size);
  // The cells its borders read: its own, the column left of it and the row
  // above, as one kind each across the group's layers
  let low = (min - IVec2::ONE).max(IVec2::ZERO);
  let span = max - low;
  let mut cells = vec![TileCell::Empty; (span.x * span.y) as usize];
  let Group {
    layers,
    kinds,
    shapes,
    boundary,
    ..
  } = group;
  for y in low.y..max.y {
    for x in low.x..max.x {
      let cell = IVec2::new(x, y);
      let mut kind = TileCell::Empty;
      for (layer, data) in layers.iter().zip(data) {
        if let Some(data) = data {
          kind = over(
            kind,
            layer_kind(layer, data, cell, (kinds, shapes), boundary, context),
          );
        }
      }
      cells[((y - low.y) * span.x + (x - low.x)) as usize] = kind;
    }
  }
  let at = |x: i32, y: i32| {
    if x < low.x || y < low.y || x >= max.x || y >= max.y {
      return TileCell::Empty;
    }
    cells[((y - low.y) * span.x + (x - low.x)) as usize]
  };
  let mut edges = Vec::new();
  group.boundary.edges(
    at,
    (min.x, min.y),
    (max.x, max.y),
    (size.x, size.y),
    &mut edges,
  );
  // The outlines run clockwise on screen; lighting's polygons the other way
  let edges: Vec<_> = edges.into_iter().map(|(a, b)| (b, a)).collect();
  let cell = group.cell.as_dvec2();
  let origin = min.as_dvec2();
  let px = |(x, y): Point| {
    let point = glam::DVec2::new(f64::from(x), f64::from(y)) / f64::from(GRID);
    ((origin + point) * cell).as_vec2()
  };
  let mut spawned = Vec::new();
  for chain in chains(&edges) {
    let points: Vec<Vec2> = chain.into_iter().map(px).collect();
    let low = points.iter().copied().fold(Vec2::MAX, Vec2::min);
    let high = points.iter().copied().fold(Vec2::MIN, Vec2::max);
    let centre = (low + high) * 0.5;
    let occluder = LightOccluder2D::chain(points.into_iter().map(|p| p - centre).collect());
    spawned.push(
      commands
        .spawn((
          Transform::from_translation(centre),
          occluder,
          MapOccluder { chunk },
          ChildOf(group.parent),
        ))
        .id(),
    );
  }
  group.chunks[index] = spawned;
  let sides = Sides {
    right: (min.y..max.y).map(|y| at(max.x - 1, y)).collect(),
    bottom: (min.x..max.x).map(|x| at(x, max.y - 1)).collect(),
  };
  let old = std::mem::replace(&mut group.sides[index], sides);
  (
    old.right != group.sides[index].right,
    old.bottom != group.sides[index].bottom,
  )
}

/// Builds again every chunk of `group` that changed since it last looked,
/// and those across a side that changed kind
fn update(group: &mut Group, layers: &Layers, context: &Context<'_>, commands: &mut Commands) {
  let changed = changed_chunks(&mut group.layers, layers, context.overrides);
  if changed.is_empty() {
    return;
  }
  let data = layer_data(&group.layers, layers);
  let chunks = group.chunk_count();
  let mut due = vec![false; chunks.x as usize * chunks.y as usize];
  for chunk in changed {
    if chunk.x < chunks.x && chunk.y < chunks.y {
      due[(chunk.y * chunks.x + chunk.x) as usize] = true;
    }
  }
  // Row by row, so a chunk marks the ones right of and below it in time
  for y in 0..chunks.y {
    for x in 0..chunks.x {
      let index = (y * chunks.x + x) as usize;
      if !due[index] {
        continue;
      }
      let (right, bottom) = build_chunk(group, UVec2::new(x, y), index, &data, context, commands);
      if right && x + 1 < chunks.x {
        due[index + 1] = true;
      }
      if bottom && y + 1 < chunks.y {
        due[index + chunks.x as usize] = true;
      }
    }
  }
}

type Maps<'w, 's> = Query<
  'w,
  's,
  (
    Entity,
    &'static MapRoot,
    Option<&'static MapLayers>,
    Option<&'static mut MapShadows>,
    Has<Disabled>,
  ),
  Allow<Disabled>,
>;

/// `PostUpdate`, before transforms propagate: every spawned map's occluding
/// layers keep their boundary chains, a chunk built again when its version
/// moves (or a side it shares changed kind). A disabled map, or one whose
/// root went, has none. An idle frame costs a look at each map's occluding
/// layers and tilesets
pub(crate) fn sync_map_occluders(
  mut maps: Maps,
  layers: Layers,
  (tile_maps, projects, tilesets): Stores,
  mut planned: crate::core::ecs::Local<Vec<Lit>>,
  mut commands: Commands,
) {
  let tilesets = tilesets.as_deref();
  for (map, root, map_layers, state, disabled) in &mut maps {
    let Some(map_layers) = map_layers.filter(|_| !disabled) else {
      if let Some(mut state) = state {
        state.clear(&mut commands);
        commands.entity(map).try_remove::<MapShadows>();
      }
      continue;
    };
    // Without its map's asset the tiles cannot be read: what is there stays
    let Some(tile_map) = root_map(root, tile_maps.as_deref(), projects.as_deref()) else {
      continue;
    };
    planned.clear();
    for &entity in &map_layers.0 {
      let Ok((info, data, layer_disabled)) = layers.get(entity) else {
        continue;
      };
      if info.occludes.unwrap_or(info.collides) && !layer_disabled {
        planned.push(Lit {
          entity,
          cell: data.cell_size,
          offset: [info.offset.x.to_bits(), info.offset.y.to_bits()],
          size: data.grid.size(),
          intgrid: data.intgrid.is_some(),
        });
      }
    }
    let context = Context {
      tile_map,
      tilesets,
      overrides: &root.intgrid_collision,
      purpose: Purpose::Occlusion,
    };
    let same = state.as_ref().is_some_and(|s| {
      s.layers == *planned
        && s.overrides == root.intgrid_collision
        && stamps_match(&s.tilesets, tile_map, tilesets)
    });
    if same {
      if let Some(mut state) = state {
        for group in &mut state.bypass_change_detection().groups {
          update(group, &layers, &context, &mut commands);
        }
      }
      continue;
    }
    let mut fresh = MapShadows {
      layers: planned.clone(),
      tilesets: stamps(tile_map, tilesets),
      overrides: root.intgrid_collision.clone(),
      groups: Vec::new(),
    };
    for layer in planned.iter() {
      let Ok((_, data, _)) = layers.get(layer.entity) else {
        continue;
      };
      let tracked = Tracked::unread(layer.entity, &data, context.overrides);
      match fresh
        .groups
        .iter_mut()
        .find(|g| g.cell == layer.cell && g.offset == layer.offset)
      {
        Some(group) => {
          group.size = group.size.max(layer.size);
          group.layers.push(tracked);
        }
        None => fresh.groups.push(Group {
          cell: layer.cell,
          offset: layer.offset,
          parent: layer.entity,
          size: layer.size,
          layers: vec![tracked],
          kinds: FastMap::default(),
          shapes: FastMap::default(),
          boundary: Boundary::default(),
          chunks: Vec::new(),
          sides: Vec::new(),
        }),
      }
    }
    for group in &mut fresh.groups {
      let chunks = group.chunk_count();
      let count = chunks.x as usize * chunks.y as usize;
      group.chunks = vec![Vec::new(); count];
      group.sides = (0..count).map(|_| Sides::default()).collect();
      update(group, &layers, &context, &mut commands);
    }
    match state {
      Some(mut state) => {
        state.clear(&mut commands);
        *state = fresh;
      }
      None => {
        commands.entity(map).try_insert(fresh);
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;
  use std::sync::Arc;

  use glam::{IVec2, UVec2, Vec2};

  use super::*;
  use crate::core::asset::{Assets, AssetsView, Handle};
  use crate::core::ecs::{ChildOf, Disabled, Entity, GlobalTransform, Transform, World};
  use crate::core::lighting::{Light2D, LightOccluder2D, push_shadow_volumes};
  use crate::core::physics::{Collider, Physics, TilePart};
  use crate::core::render::batch::Triangles;
  use crate::core::tilemap::test_util::MapWorld;
  use crate::core::tilemap::{
    IntGrid, LayerKind, MapLayerRef, MapRoot, MapsView, Tile, TileData, TileMap, TileShape,
    Tileset, TilesetImage, TilesetSource,
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

  /// An occluder: its entity, chunk, where its entity sits in its parent's
  /// space and its vertices around it
  #[derive(Debug, Clone)]
  struct Found {
    entity: Entity,
    chunk: UVec2,
    at: Vec2,
    occluder: LightOccluder2D,
  }

  fn occluders_of(world: &mut World, parent: Entity) -> Vec<Found> {
    let mut found: Vec<Found> = world
      .query::<(Entity, &ChildOf, &MapOccluder, &Transform, &LightOccluder2D)>()
      .iter(world)
      .filter(|(_, child_of, ..)| child_of.parent() == parent)
      .map(|(entity, _, marker, transform, occluder)| Found {
        entity,
        chunk: marker.chunk,
        at: transform.translation,
        occluder: occluder.clone(),
      })
      .collect();
    found.sort_by_key(|f| f.entity);
    found
  }

  fn count<C: Component>(world: &mut World) -> usize {
    world.query::<&C>().iter(world).count()
  }

  type Edge = ((i32, i32), (i32, i32));

  /// Every edge of `found` in its parent's space, cut into whole-pixel
  /// steps (so edges compare however chains split and merge them), each
  /// with how many times it is there
  fn steps(found: &[Found]) -> BTreeMap<Edge, usize> {
    let mut all = BTreeMap::new();
    for f in found {
      for (a, b) in f.occluder.edges() {
        let (a, b) = (a + f.at, b + f.at);
        let rounded = |v: Vec2| {
          assert!(
            (v - v.round()).abs().max_element() < 1e-3,
            "{v} off the pixel grid"
          );
          (v.x.round() as i32, v.y.round() as i32)
        };
        add_steps(&mut all, rounded(a), rounded(b));
      }
    }
    all
  }

  fn gcd(a: i32, b: i32) -> i32 {
    if b == 0 { a.abs() } else { gcd(b, a % b) }
  }

  fn add_steps(all: &mut BTreeMap<Edge, usize>, a: (i32, i32), b: (i32, i32)) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let n = gcd(dx, dy).max(1);
    for i in 0..n {
      let from = (a.0 + dx * i / n, a.1 + dy * i / n);
      let to = (a.0 + dx * (i + 1) / n, a.1 + dy * (i + 1) / n);
      *all.entry((from, to)).or_insert(0) += 1;
    }
  }

  /// The steps of a ring of corners (px) as lighting winds them: each edge
  /// from a corner to the next and the last back to the first
  fn ring(corners: &[(i32, i32)]) -> BTreeMap<Edge, usize> {
    let mut all = BTreeMap::new();
    for (i, &a) in corners.iter().enumerate() {
      add_steps(&mut all, a, corners[(i + 1) % corners.len()]);
    }
    all
  }

  fn merged(rings: &[BTreeMap<Edge, usize>]) -> BTreeMap<Edge, usize> {
    let mut all = BTreeMap::new();
    for ring in rings {
      for (edge, n) in ring {
        *all.entry(*edge).or_insert(0) += n;
      }
    }
    all
  }

  /// Every occluder is an open chain whose entity sits at the centre of its
  /// corners' box
  fn assert_placed(found: &[Found]) {
    for f in found {
      let vertices = f.occluder.vertices();
      assert!(!f.occluder.is_closed(), "a chain: {f:?}");
      let min = vertices.iter().copied().fold(Vec2::MAX, Vec2::min);
      let max = vertices.iter().copied().fold(Vec2::MIN, Vec2::max);
      assert!(
        ((min + max) * 0.5).abs().max_element() < 1e-3,
        "off its geometry: {f:?}"
      );
    }
  }

  /// 16 px tiles: a solid block (0), a half block (1), a slope filled at its
  /// bottom right (2), a one-way block (3), and a block whose only shape is a
  /// small occluder (4)
  fn blocks() -> Tileset {
    let mut set = Tileset::new("blocks", UVec2::splat(16), TilesetImage::Collection);
    let shaped = |points: Vec<Vec2>, one_way: bool, occluder: bool| TileData {
      shapes: vec![TileShape {
        points,
        one_way,
        occluder,
        ..TileShape::default()
      }],
      ..TileData::default()
    };
    let rect = |x: f32, y: f32, w: f32, h: f32| {
      vec![
        Vec2::new(x, y),
        Vec2::new(x + w, y),
        Vec2::new(x + w, y + h),
        Vec2::new(x, y + h),
      ]
    };
    set.insert_tile(0, shaped(rect(0.0, 0.0, 16.0, 16.0), false, false));
    set.insert_tile(1, shaped(rect(0.0, 8.0, 16.0, 8.0), false, false));
    let slope = vec![
      Vec2::new(16.0, 0.0),
      Vec2::new(16.0, 16.0),
      Vec2::new(0.0, 16.0),
    ];
    set.insert_tile(2, shaped(slope, false, false));
    set.insert_tile(3, shaped(rect(0.0, 0.0, 16.0, 16.0), true, false));
    set.insert_tile(4, shaped(rect(4.0, 2.0, 8.0, 12.0), false, true));
    set
  }

  /// A map of `size` 16 px cells over `blocks`, one layer called "ground"
  /// with a solid floor along its bottom row, then `edit`
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

  fn put(map: &mut TileMap, layer: usize, cell: (i32, i32), id: u32) {
    if let Some(grid) = map.tiles_mut(layer) {
      grid.set(IVec2::new(cell.0, cell.1), Some(Tile::new(0, id)));
    }
  }

  #[test]
  fn occluders_are_the_boundary_of_what_occludes() {
    let mut maps = MapWorld::new();
    // A floor with a half block and a slope standing on it, a one-way block
    // in the air, and a block that only occludes with its small shape
    let handle = floor_map(&mut maps.world, UVec2::new(6, 3), |map| {
      put(map, 0, (1, 1), 1);
      put(map, 0, (3, 1), 2);
      put(map, 0, (5, 0), 3);
      put(map, 0, (0, 0), 4);
    });
    let map = maps.spawn(MapRoot::new(handle));
    let layer = layers_of(&mut maps.world, map)[0];
    let found = occluders_of(&mut maps.world, layer);
    assert_placed(&found);
    assert!(found.iter().all(|f| f.chunk == UVec2::ZERO));
    // Counter-clockwise on screen, as lighting winds polygons: the edges
    // between the floor and what stands on it cancel
    let ground = ring(&[
      (0, 32),
      (0, 48),
      (96, 48),
      (96, 32),
      (64, 32),
      (64, 16),
      (48, 32),
      (32, 32),
      (32, 24),
      (16, 24),
      (16, 32),
    ]);
    let platform = ring(&[(80, 0), (80, 16), (96, 16), (96, 0)]);
    let light_only = ring(&[(4, 2), (4, 14), (12, 14), (12, 2)]);
    assert_eq!(steps(&found), merged(&[ground, platform, light_only]));
  }

  #[test]
  fn occluders_meet_across_layers_and_chunk_borders() {
    let mut maps = MapWorld::new();
    let level = load(&mut maps.world, "tiled/maps/level.tmj");
    let map = maps.spawn(MapRoot::new(level));
    let layers = layers_of(&mut maps.world, map);
    // The floor's two layers occlude as one; decor does not occlude
    let found = occluders_of(&mut maps.world, layers[0]);
    assert_placed(&found);
    assert!(occluders_of(&mut maps.world, layers[1]).is_empty());
    assert!(occluders_of(&mut maps.world, layers[2]).is_empty());
    let all = steps(&found);
    let along = |x0: i32, x1: i32, y: i32| {
      all
        .keys()
        .any(|&((ax, ay), (bx, by))| ay == y && by == y && ax.min(bx) >= x0 && ax.max(bx) <= x1)
    };
    let down = |x: i32, y0: i32, y1: i32| {
      all
        .keys()
        .any(|&((ax, ay), (bx, by))| ax == x && bx == x && ay.min(by) >= y0 && ay.max(by) <= y1)
    };
    assert!(
      !down(192, 176, 192),
      "no seam where the floor's layers meet"
    );
    assert!(!along(288, 304, 176), "none under the half block");
    assert!(!along(320, 352, 176), "none under the peak of two slopes");
    assert!(!along(368, 384, 176), "none under the turned slope");
    assert!(down(0, 176, 192) && down(384, 176, 192), "the floor's ends");
    // Across a chunk border: a floor of 64 cells is one boundary
    let handle = floor_map(&mut maps.world, UVec2::new(64, 4), |_| {});
    let wide = maps.spawn(MapRoot::new(handle));
    let layer = layers_of(&mut maps.world, wide)[0];
    let found = occluders_of(&mut maps.world, layer);
    assert_placed(&found);
    assert_eq!(
      steps(&found),
      ring(&[(0, 48), (0, 64), (1024, 64), (1024, 48)])
    );
    let mut chunks: Vec<UVec2> = found.iter().map(|f| f.chunk).collect();
    chunks.sort_by_key(|c| (c.y, c.x));
    assert_eq!(chunks, [UVec2::ZERO, UVec2::new(1, 0)]);
  }

  /// Whether `point` is in a shadow `occluders` (each at its position) cast
  /// from a light at `from`
  fn shadowed(point: Vec2, from: Vec2, occluders: &[Found]) -> bool {
    let light = Light2D::point(crate::core::ui::Color::WHITE, 400.0).with_shadows();
    let placed: Vec<(GlobalTransform, &LightOccluder2D)> = occluders
      .iter()
      .map(|f| {
        let at = GlobalTransform::from_transform(&Transform::from_translation(f.at));
        (at, &f.occluder)
      })
      .collect();
    let mut triangles = Triangles::default();
    push_shadow_volumes(from, &light, &placed, &mut triangles);
    triangles.indices.chunks(3).any(|t| {
      let corner = |i: u32| Vec2::from(triangles.vertices[i as usize].position);
      let (a, b, c) = (corner(t[0]), corner(t[1]), corner(t[2]));
      let side = |p: Vec2, q: Vec2| (q - p).perp_dot(point - p);
      let (s1, s2, s3) = (side(a, b), side(b, c), side(c, a));
      (s1 > 1e-3 && s2 > 1e-3 && s3 > 1e-3) || (s1 < -1e-3 && s2 < -1e-3 && s3 < -1e-3)
    })
  }

  #[test]
  fn a_slope_beside_ground_casts_no_shadow_into_the_ground() {
    let mut maps = MapWorld::new();
    // A slope (x 48 to 64) and a half block (x 96 to 112) standing on a
    // floor (y 32 to 48)
    let handle = floor_map(&mut maps.world, UVec2::new(8, 3), |map| {
      put(map, 0, (3, 1), 2);
      put(map, 0, (6, 1), 1);
    });
    let map = maps.spawn(MapRoot::new(handle));
    let layer = layers_of(&mut maps.world, map)[0];
    let found = occluders_of(&mut maps.world, layer);
    // The floor's art under each stays lit from above them and from above
    // either side: no edge between a shape and the floor casts into it
    for (points, lights) in [
      (
        [Vec2::new(52.0, 36.0), Vec2::new(60.0, 44.0)],
        [
          Vec2::new(56.0, -40.0),
          Vec2::new(20.0, -60.0),
          Vec2::new(90.0, -60.0),
        ],
      ),
      (
        [Vec2::new(100.0, 34.0), Vec2::new(106.0, 44.0)],
        [
          Vec2::new(104.0, -40.0),
          Vec2::new(70.0, -60.0),
          Vec2::new(140.0, -60.0),
        ],
      ),
    ] {
      for point in points {
        for from in lights {
          assert!(
            !shadowed(point, from, &found),
            "{point} lit from {from} is in shadow"
          );
        }
      }
    }
    // A sanity check: behind the ground, away from a light above, is shadow
    assert!(shadowed(
      Vec2::new(60.0, 60.0),
      Vec2::new(60.0, -40.0),
      &found
    ));
  }

  #[test]
  fn map_occluders_cast_their_shadows_away_from_a_light() {
    let mut maps = MapWorld::new();
    let level = load(&mut maps.world, "tiled/maps/level.tmj");
    let map = maps.spawn(MapRoot::new(level));
    let layer = layers_of(&mut maps.world, map)[0];
    let found = occluders_of(&mut maps.world, layer);
    // Lit from above, the floor's top casts nothing and its bottom casts:
    // under the floor (y 192 on) is shadow, above it is not
    let above = Vec2::new(24.0, 100.0);
    assert!(shadowed(Vec2::new(24.0, 200.0), above, &found));
    assert!(!shadowed(Vec2::new(24.0, 170.0), above, &found));
    assert!(
      !shadowed(Vec2::new(24.0, 184.0), above, &found),
      "the floor itself"
    );
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
      if let LayerKind::Tiles { intgrid, .. } = &mut map.layers[0].kind {
        let names = [
          (1, "Solid"),
          (2, "one_way"),
          (3, "slope_bottom_left"),
          (4, "Wall"),
        ];
        *intgrid = Some(IntGrid {
          size: UVec2::new(4, 2),
          values: vec![1, 3, 2, 4, 0, 0, 0, 0],
          names: names.iter().map(|(v, n)| (*v, n.to_string())).collect(),
        });
      }
    });
    let root = MapRoot::new(handle).with_intgrid_collision("wall", TileCell::Solid);
    let map = maps.spawn(root);
    let layer = layers_of(&mut maps.world, map)[0];
    let found = occluders_of(&mut maps.world, layer);
    assert_placed(&found);
    // Its IntGrid, not its tiles (the floor along the bottom row): the solid
    // cell and the slope beside it as one, and the wall it was told about;
    // the one-way cell has no area to occlude with
    let solid_and_slope = ring(&[(0, 0), (0, 16), (32, 16), (16, 0)]);
    let wall = ring(&[(48, 0), (48, 16), (64, 16), (64, 0)]);
    assert_eq!(steps(&found), merged(&[solid_and_slope, wall]));
  }

  #[test]
  fn an_edit_rebuilds_only_its_chunk_s_occluders() {
    let mut maps = MapWorld::new();
    let handle = floor_map(&mut maps.world, UVec2::new(64, 32), |_| {});
    let map = maps.spawn(MapRoot::new(handle));
    let layer = layers_of(&mut maps.world, map)[0];
    let before = occluders_of(&mut maps.world, layer);
    assert_eq!(before.len(), 2, "{before:#?}");
    let far = before
      .iter()
      .find(|f| f.chunk == UVec2::new(1, 0))
      .expect("the far chunk's")
      .clone();
    assert!(MapsView::new(&mut maps.world).set_tile(
      map,
      "ground",
      IVec2::new(5, 10),
      Some(Tile::new(0, 1))
    ));
    maps.post_update();
    let after = occluders_of(&mut maps.world, layer);
    assert_placed(&after);
    assert!(
      after.iter().any(|f| f.entity == far.entity),
      "the far chunk kept its occluder"
    );
    let near: Vec<&Found> = after.iter().filter(|f| f.chunk == UVec2::ZERO).collect();
    assert_eq!(
      near.len(),
      2,
      "the floor's and the half block's: {after:#?}"
    );
    assert!(
      near
        .iter()
        .all(|f| before.iter().all(|b| b.entity != f.entity)),
      "the edited chunk's occluders were built again"
    );
    let half = ring(&[(80, 168), (80, 176), (96, 176), (96, 168)]);
    let floor = ring(&[(0, 496), (0, 512), (1024, 512), (1024, 496)]);
    assert_eq!(steps(&after), merged(&[half, floor]));
    // An edit on the chunk border rebuilds the chunk across it too: the
    // floor's last cell of the near chunk gone, the far chunk's floor has a
    // left end
    assert!(MapsView::new(&mut maps.world).set_tile(map, "ground", IVec2::new(31, 31), None));
    maps.post_update();
    let edged = occluders_of(&mut maps.world, layer);
    assert!(
      edged.iter().all(|f| f.entity != far.entity),
      "the far chunk was built again"
    );
    let half = ring(&[(80, 168), (80, 176), (96, 176), (96, 168)]);
    let near = ring(&[(0, 496), (0, 512), (496, 512), (496, 496)]);
    let far = ring(&[(512, 496), (512, 512), (1024, 512), (1024, 496)]);
    assert_eq!(steps(&edged), merged(&[half, near, far]));
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
    // The half block put on the floor while disabled is part of its outline
    let layer = layers_of(&mut maps.world, map)[0];
    let found = occluders_of(&mut maps.world, layer);
    assert_eq!(
      steps(&found),
      ring(&[
        (0, 16),
        (0, 32),
        (64, 32),
        (64, 16),
        (32, 16),
        (32, 8),
        (16, 8),
        (16, 16)
      ])
    );
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

  /// A white floor over the window, no ambient light, then `setup`
  struct Lit {
    size: (u32, u32),
    setup: Box<dyn FnMut(&mut crate::core::scene::SceneContext) + Send>,
  }

  impl crate::core::scene::Scene for Lit {
    fn on_enter(&mut self, ctx: &mut crate::core::scene::SceneContext) {
      use crate::core::ecs::{Anchor, Renderable};
      let (w, h) = self.size;
      let floor = ctx.assets().add(crate::core::graphics::Image::solid_rect(
        w, h, 255, 255, 255, 255,
      ));
      ctx.world.spawn((
        Transform::from_xy(0.0, 0.0),
        Renderable::image(floor).with_anchor(Anchor::TopLeft),
      ));
      ctx
        .lighting()
        .set_ambient(crate::core::lighting::AmbientLight::with_intensity(0.0));
      (self.setup)(ctx);
    }
  }

  /// The linear red of each of `pixels` in a frame of a wall two 8 px cells
  /// thick across 64 cells (two chunks, the seam at x 256; y 160 to 176),
  /// lit from above right of the seam, its shadows of `alpha` and
  /// `softness`; the wall's occluders the map's, or one rectangle
  fn wall_frame(
    map_occludes: bool,
    alpha: u8,
    softness: Option<f32>,
    pixels: &[(u32, u32)],
  ) -> Option<Vec<f32>> {
    use crate::core::engine::EngineBuilder;
    use crate::core::testing::HeadlessEngine;
    use crate::core::ui::Color;
    let setup = move |ctx: &mut crate::core::scene::SceneContext| {
      let light = Light2D::point(Color::WHITE, 300.0).with_intensity(0.5);
      let light = match softness {
        Some(softness) => light.with_soft_shadows(softness),
        None => light.with_shadows(),
      };
      let light = light.with_shadow_color(Color::new(0, 0, 0, alpha));
      ctx.world.spawn((Transform::from_xy(304.0, 120.0), light));
      let mut set = Tileset::new("blocks", UVec2::splat(8), TilesetImage::Collection);
      let block = vec![
        Vec2::new(0.0, 0.0),
        Vec2::new(8.0, 0.0),
        Vec2::new(8.0, 8.0),
        Vec2::new(0.0, 8.0),
      ];
      set.insert_tile(
        0,
        TileData {
          shapes: vec![TileShape {
            points: block,
            ..TileShape::default()
          }],
          ..TileData::default()
        },
      );
      let mut map = TileMap::new(UVec2::new(64, 32), UVec2::splat(8));
      let set = map.add_tileset(TilesetSource::Embedded(Arc::new(set)));
      let layer = map.add_tile_layer("wall");
      if let Some(grid) = map.tiles_mut(layer) {
        grid.fill(
          IVec2::new(0, 20),
          IVec2::new(63, 21),
          Some(Tile::new(set, 0)),
        );
      }
      if !map_occludes {
        map.layers[layer].info.occludes = Some(false);
        ctx.world.spawn((
          Transform::from_xy(256.0, 168.0),
          LightOccluder2D::rect(512.0, 16.0),
        ));
      }
      let handle = ctx.assets().add(map);
      ctx.world.spawn(MapRoot::new(handle));
    };
    let scene = Lit {
      size: (512, 256),
      setup: Box::new(setup),
    };
    let builder = EngineBuilder::new().resolution(512, 256).start_scene(scene);
    let mut engine = HeadlessEngine::new_or_skip(builder)?;
    engine.step_frames(6).expect("frames");
    let frame = engine.frame().expect("a frame");
    let linear = |byte: u8| {
      let c = f32::from(byte) / 255.0;
      if c <= 0.04045 {
        c / 12.92
      } else {
        ((c + 0.055) / 1.055).powf(2.4)
      }
    };
    Some(
      pixels
        .iter()
        .map(|&(x, y)| linear(frame.get_pixel(x, y).0[0]))
        .collect(),
    )
  }

  #[test]
  fn abutting_map_occluders_cast_no_seam_shadows() {
    // In the wall where the seam's wedge would fall, in the wall away from
    // it, and under the wall behind the seam and beside it
    let pixels = [(250, 172), (220, 172), (226, 196), (200, 196)];
    for (alpha, softness) in [(255, None), (128, None), (255, Some(0.6)), (128, Some(0.6))] {
      let (Some(one), Some(map)) = (
        wall_frame(false, alpha, softness, &pixels),
        wall_frame(true, alpha, softness, &pixels),
      ) else {
        return;
      };
      assert!(one[0] > 0.05, "the wall's art is lit: {one:?}");
      for (i, (expected, got)) in one.iter().zip(&map).enumerate() {
        assert!(
          (expected - got).abs() <= 0.01 + expected * 0.02,
          "alpha {alpha}, softness {softness:?}, pixel {:?}: one rectangle {expected}, the map's occluders {got}",
          pixels[i]
        );
      }
    }
  }
}
