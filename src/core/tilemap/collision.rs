//! Map collision: a spawned map's colliding tile layers become
//! `TileColliders`, kept in step with their tiles.
//!
//! A tile layer collides when its `LayerInfo::collides` is set and its
//! root has not turned collision off. Colliding layers are grouped by their
//! cell size, floor, place and collision material (`collision_layer`,
//! `collision_mask`, `friction`, `restitution`: a layer's properties over
//! its map's), its place being where it stands in ground space (its
//! offset, less its floor's height above the ground floor): the group of
//! the map's own tile size on the ground floor, no offset and the map's
//! material unions into one `TileColliders` on the map entity, and any
//! other group (an LDtk IntGrid of 8 px cells under 16 px art, a layer
//! shifted by an offset, an ice layer, a higher floor's layers) gets its
//! own on its first layer's entity, whose `Transform` carries the place.
//! Each grid is on its floor (`TileColliders::floor`), so floors collide
//! apart. A cell takes the strongest kind across its group: `Solid`, then
//! `Shape`, then `Slope`, then `OneWay`; of two shapes (or two slopes) the
//! upper layer's wins. What a cell is comes from `kinds.rs`: an IntGrid
//! layer's values by name, any other layer's tiles' collision shapes placed
//! where they draw.
//!
//! Each floor above the ground has an edge: its footprint (the cells its
//! tile layers fill, and the stairs up to it on lower floors' layers),
//! `Solid` cells traced as a grid of their own on a [`MapFloor`] child of
//! the map where the floor's layers stand, with the map's material, on that
//! floor. Outlines are hollow, so a character on the floor stays inside its
//! edge, and the floor's walls, a grid of their own, keep their outlines.
//! On the ground floor the edge is nothing: a raised floor blocks only
//! where its own tiles collide, so a bridge stands over open ground.
//!
//! A map's first grids are made off the main thread, with the rest of its
//! landing (`spawn.rs`): `prepare` classifies every cell from the map's
//! asset and traces each grid (`Traced`), and `land` puts them on the map
//! and its layers as they spawn, so the map, its colliders and
//! `MapEvent::Spawned` arrive in one frame and physics only spawns the
//! parts. Whatever builds the map's grids again after that (it is enabled
//! again, a layer's flags, the tilesets or the overrides change) builds at
//! once on the main thread.
//!
//! A group's grid holds the chunks its layers have: a group of bounded
//! layers is bounded by their largest, and a group with an unbounded layer
//! is unbounded (`TileColliders::unbounded`), its cells signed. Each chunk
//! is classified from its layers' cells where they lie, never by a lookup
//! per cell.
//!
//! Each step, every chunk whose version moved since the group last looked
//! is classified again and only the cells whose kind changed are set, so
//! the tile colliders rebuild only the outlines an edit touched, a floor's
//! edge too. A change of what the colliders are built from (which layers
//! collide, their cell sizes, floors, places, materials or bounds, the
//! tilesets' shapes and stairs, the IntGrid overrides) builds every group
//! again.

use std::sync::Arc;

use glam::{IVec2, Vec2};

use super::floors::{MapFloor, MapFloors, ground_offset};
use super::kinds::{
  CELLS, ChunkKinds, Context, Footing, GridKey, Layers, Part, Purpose, ShapeKey, ShapeSink,
  ShapeSource, StairSource, Stores, TilesetStamp, Tracked, build_chunks, changed_chunks,
  chunk_origin, footprint_kinds, lattice_of, layer_data, reindexing, scatter, shapes_kept, sources,
  stair_sources, stamps, union_kinds,
};
use super::spawn::{IntGridKey, MapLayers, MapRoot, TileLayerData, child_of, root_map};
use super::{LayerInfo, MapGrid, Properties, PropertyValue};
use crate::core::ecs::{
  Allow, ChildOf, Commands, Component, DetectChangesMut, Disabled, Entity, Floor, Has, Local,
  Query, Transform, Without, World,
};
use crate::core::hash::{FastMap, FastSet};
use crate::core::physics::{CellRect, CellShape, TileCell, TileColliders, Traced};

/// A grid's shapes before the grid exists, numbered as `add_shape` will
struct Pending(Vec<CellShape>);

impl ShapeSink for Pending {
  fn add(&mut self, shape: CellShape) -> u16 {
    // `add_shape` names none past 65,535 (and logs it)
    let index = u16::try_from(self.0.len()).unwrap_or(u16::MAX);
    self.0.push(shape);
    index
  }
}

/// What a group of layers shares: a grid (its shape, cell size and
/// origin), a floor, where its layers stand in the map (their ground
/// offset, `floors::ground_offset`), a material (floats by their bits), and
/// whether it is a higher floor's edge (its footprint) rather than its
/// layers' collision
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct GroupKey {
  grid: GridKey,
  floor: u8,
  offset: [u32; 2],
  material: [u32; 4],
  edge: bool,
}

impl GroupKey {
  /// The key of colliding layers on `grid`, on `floor`, standing at
  /// `offset`
  fn of(grid: &MapGrid, (floor, offset): (u8, Vec2), material: Material) -> GroupKey {
    GroupKey {
      grid: GridKey::of(grid),
      floor,
      offset: [offset.x.to_bits(), offset.y.to_bits()],
      material: [
        material.layer,
        material.mask,
        material.friction.to_bits(),
        material.restitution.to_bits(),
      ],
      edge: false,
    }
  }

  /// Where its layers stand in the map
  fn offset(&self) -> Vec2 {
    Vec2::new(
      f32::from_bits(self.offset[0]),
      f32::from_bits(self.offset[1]),
    )
  }

  fn material(&self) -> Material {
    Material {
      layer: self.material[0],
      mask: self.material[1],
      friction: f32::from_bits(self.material[2]),
      restitution: f32::from_bits(self.material[3]),
    }
  }
}

/// A colliding layer, or a layer a higher floor's footprint reads, as the
/// colliders were built from it: its place among the map's layers and its
/// entity (a stand-in until the layers spawn)
#[derive(Clone, PartialEq, Debug)]
pub(crate) struct Planned {
  entity: Entity,
  index: usize,
  key: GroupKey,
  /// Its grid's bounds; `None` for an unbounded grid
  bounds: Option<CellRect>,
  intgrid: bool,
  /// What it gives the footprint, for a layer of a floor's edge; `None`
  /// for a colliding layer
  footing: Option<Footing>,
}

/// Layers sharing a grid, floor, place and material, and the grid they
/// collide in; or the layers of a higher floor's footprint, and its edge
#[derive(Debug)]
struct Group {
  key: GroupKey,
  /// Where its `TileColliders` is: the map, its first layer, or for a
  /// floor's edge a `MapFloor` entity of its own
  target: Entity,
  target_layer: Option<usize>,
  /// The cells its grid holds: its layers' largest bounds (all bounded
  /// grids start at (0, 0)), or `None` with an unbounded layer
  bounds: Option<CellRect>,
  layers: Vec<Tracked>,
  /// What each layer gives the footprint, for a floor's edge; empty for
  /// collision
  footing: Vec<Footing>,
  /// Each tile's kind (by its cell's value), its shapes numbered in this
  /// group's grid
  kinds: FastMap<u32, TileCell>,
  shapes: FastMap<ShapeKey, u16>,
}

impl Group {
  /// The kinds of `chunk` across the group's layers (`data`, in its
  /// order): their collision, or for a floor's edge its footprint
  fn kinds_of(
    &mut self,
    data: &[Option<&TileLayerData>],
    chunk: IVec2,
    sink: &mut dyn ShapeSink,
    context: &Context<'_>,
  ) -> ChunkKinds {
    if self.key.edge {
      return footprint_kinds(data, &self.footing, (chunk, self.key.floor), context);
    }
    let at = (chunk, Part::WHOLE);
    let kinds = (&mut self.kinds, &mut self.shapes);
    union_kinds(&self.layers, data, at, kinds, sink, context)
  }
}

/// The collision material properties give
#[derive(Clone, Copy, PartialEq, Debug)]
struct Material {
  layer: u32,
  mask: u32,
  friction: f32,
  restitution: f32,
}

impl Material {
  /// `TileColliders`' own defaults
  const DEFAULT: Material = Material {
    layer: 1,
    mask: u32::MAX,
    friction: 0.5,
    restitution: 0.0,
  };

  /// From `collision_layer`, `collision_mask`, `friction` and
  /// `restitution` in `properties`, each over the same in `under` (a
  /// layer's over its map's); a property of the wrong type is left out
  /// (what is under it counts) and `warn` hears of it
  fn of(
    properties: &Properties,
    under: Material,
    mut warn: impl FnMut(&'static str, &PropertyValue, &'static str),
  ) -> Material {
    let mut bits = |name: &'static str, under: u32| match properties.get(name) {
      None => under,
      Some(value) => match properties.int(name) {
        // A mask of every bit is -1 in a 32-bit int
        Some(int) if (i64::from(i32::MIN)..=i64::from(u32::MAX)).contains(&int) => int as u32,
        _ => {
          warn(name, value, "a whole number of 32 bits");
          under
        }
      },
    };
    let layer = bits("collision_layer", under.layer);
    let mask = bits("collision_mask", under.mask);
    let mut float = |name: &'static str, under: f32| match properties.get(name) {
      None => under,
      Some(value) => match properties.float(name) {
        Some(f) if f.is_finite() => f as f32,
        _ => {
          warn(name, value, "a number");
          under
        }
      },
    };
    let friction = float("friction", under.friction);
    let restitution = float("restitution", under.restitution);
    Material {
      layer,
      mask,
      friction,
      restitution,
    }
  }

  fn apply(&self, tiles: &mut TileColliders) {
    tiles.layer = self.layer;
    tiles.mask = self.mask;
    tiles.friction = self.friction;
    tiles.restitution = self.restitution;
  }
}

/// A tile layer as planning and a build read it: its place among the map's
/// layers, its entity (a stand-in before the layers spawn), name, info and
/// tiles
#[derive(Clone, Copy)]
pub(super) struct LayerView<'a> {
  pub(super) index: usize,
  pub(super) entity: Entity,
  pub(super) info: &'a LayerInfo,
  pub(super) data: &'a TileLayerData,
}

/// Properties of the wrong type warned about: the layer's place (none for
/// the map's) and the property's name
type Warned = FastSet<(Option<usize>, &'static str)>;

/// Warns, once a layer (`warned`), that its hexagons are more than the
/// collision tracer takes (over 16,384 px, or under 2 px)
fn warn_untraceable(map: Entity, layer: &LayerView<'_>, warned: &mut Warned) {
  if warned.insert((Some(layer.index), "hexagonal")) {
    crate::engine_warn!(
      module: "Tilemap",
      "map {map}: layer \"{}\" has hexagonal cells over 16,384 px or under 2 px, which don't collide",
      layer.info.name
    );
  }
}

/// The colliding layers among `layers` into `planned`, each keyed on its
/// floor and where it stands (`floors`, the map's), then the layers of each
/// higher floor's footprint (`plan_edge`), and the map's own group key
/// (`grid`, the map's grid, on the ground floor). A property of the wrong
/// type warns once, and so does a colliding layer with parallax, which
/// collides where it would draw without it, parallax being a drawing
/// effect, an IntGrid override to `TileCell::Shape`, which names no shape
/// and collides as nothing, and a colliding layer of hexagons the collision
/// tracer does not take (over 16,384 px, or under 2 px), which builds
/// nothing (`warned`)
fn plan<'a>(
  map: Entity,
  layers: impl Iterator<Item = LayerView<'a>> + Clone,
  (collision, properties, grid, overrides): (
    bool,
    &Properties,
    &MapGrid,
    &[(IntGridKey, TileCell)],
  ),
  floors: Option<&MapFloors>,
  warned: &mut Warned,
  planned: &mut Vec<Planned>,
) -> GroupKey {
  let is_shape = |(_, cell): &&(IntGridKey, TileCell)| matches!(cell, TileCell::Shape(_));
  if overrides.iter().any(|o| is_shape(&o)) && warned.insert((None, "with_intgrid_collision")) {
    let shapes: Vec<&IntGridKey> = overrides.iter().filter(is_shape).map(|(k, _)| k).collect();
    crate::engine_warn!(
      module: "Tilemap",
      "map {map}: with_intgrid_collision gives the IntGrid values {shapes:?} a TileCell::Shape, which names no shape, so their cells collide as nothing; give them Solid, OneWay or a Slope"
    );
  }
  let warn = |warned: &mut Warned,
              layer: Option<(usize, &str)>,
              name: &'static str,
              value: &PropertyValue,
              wanted: &str| {
    if warned.insert((layer.map(|(index, _)| index), name)) {
      let whose = match layer {
        Some((_, layer)) => format!("layer \"{layer}\""),
        None => "the map".to_string(),
      };
      crate::engine_warn!(
        module: "Tilemap",
        "map {map}: {whose}'s property {name} is {value:?}, not {wanted}; its colliders take what is under it (the map's, else the default)"
      );
    }
  };
  let map_material = Material::of(properties, Material::DEFAULT, |name, value, wanted| {
    warn(warned, None, name, value, wanted)
  });
  planned.clear();
  if !collision {
    return GroupKey::of(grid, (0, Vec2::ZERO), map_material);
  }
  for layer in layers.clone().filter(|l| l.info.collides) {
    let hex = layer.data.grid.hex_cells();
    if hex.is_some_and(|hex| !hex.traceable()) {
      warn_untraceable(map, &layer, warned);
      continue;
    }
    let material = Material::of(
      &layer.info.properties,
      map_material,
      |name, value, wanted| {
        let place = Some((layer.index, layer.info.name.as_str()));
        warn(warned, place, name, value, wanted)
      },
    );
    let parallax = layer.info.parallax;
    if parallax != Vec2::ONE && warned.insert((Some(layer.index), "parallax")) {
      crate::engine_warn!(
        module: "Tilemap",
        "map {map}: layer \"{}\" collides and has parallax {}, {}: collision ignores parallax, so it collides where it would draw with parallax 1, not where it draws",
        layer.info.name,
        parallax.x,
        parallax.y
      );
    }
    let stands = (layer.info.floor, ground_offset(layer.info, floors));
    planned.push(Planned {
      entity: layer.entity,
      index: layer.index,
      key: GroupKey::of(&layer.data.grid, stands, material),
      bounds: layer.data.tiles.bounds(),
      intgrid: layer.data.intgrid.is_some(),
      footing: None,
    });
  }
  if let Some(floors) = floors {
    for floor in floors.floors().filter_map(|floor| floor.number()) {
      if floor > 0 {
        plan_edge(
          map,
          layers.clone(),
          (floor, floors),
          map_material,
          (warned, planned),
        );
      }
    }
  }
  GroupKey::of(grid, (0, Vec2::ZERO), map_material)
}

/// The layers of floor `floor`'s footprint among `layers` into `planned`:
/// the tile layers on it standing where its bottom tile layer does (on its
/// grid, at its ground offset), all their tiles, and the tile layers of
/// lower floors standing there, their stairs up to it. A layer on it
/// standing elsewhere is left out, warned once a map (`warned`); a floor
/// with no tile layer has no footprint, nor does one whose hexagons the
/// tracer does not take
fn plan_edge<'a>(
  map: Entity,
  layers: impl Iterator<Item = LayerView<'a>> + Clone,
  (floor, floors): (u8, &MapFloors),
  material: Material,
  (warned, planned): (&mut Warned, &mut Vec<Planned>),
) {
  let Some(bottom) = layers.clone().find(|l| l.info.floor == floor) else {
    return;
  };
  if bottom
    .data
    .grid
    .hex_cells()
    .is_some_and(|hex| !hex.traceable())
  {
    warn_untraceable(map, &bottom, warned);
    return;
  }
  // The edge's key where a layer stands: the bottom tile layer's, or
  // another for a layer elsewhere
  let stands = |layer: &LayerView<'_>| {
    let at = (floor, ground_offset(layer.info, Some(floors)));
    GroupKey {
      edge: true,
      ..GroupKey::of(&layer.data.grid, at, material)
    }
  };
  let key = stands(&bottom);
  for layer in layers {
    let footing = match layer.info.floor.cmp(&floor) {
      std::cmp::Ordering::Equal => Footing::Tiles,
      std::cmp::Ordering::Less => Footing::Stairs,
      std::cmp::Ordering::Greater => continue,
    };
    if stands(&layer) != key {
      if footing == Footing::Tiles && warned.insert((None, "floor edge")) {
        crate::engine_warn!(
          module: "Tilemap",
          "map {map}: layer \"{}\" on floor {floor} stands elsewhere than the floor's bottom tile layer \"{}\" (another grid or offset), so the floor's edge leaves its cells out",
          layer.info.name,
          bottom.info.name
        );
      }
      continue;
    }
    planned.push(Planned {
      entity: layer.entity,
      index: layer.index,
      key,
      bounds: layer.data.tiles.bounds(),
      intgrid: layer.data.intgrid.is_some(),
      footing: Some(footing),
    });
  }
}

/// Warns, once a map (`warned`), that `context` met one-way tiles off an
/// orthogonal grid, which collided both ways
fn warn_both_ways(map: Entity, context: &Context<'_>, warned: &mut Warned) {
  if context.both_ways.get() && warned.insert((None, "one_way")) {
    crate::engine_warn!(
      module: "Tilemap",
      "map {map}: its one-way tiles collide both ways: one-way platforms are for orthogonal maps"
    );
  }
}

/// What a build of a map's groups makes: the groups, and the grid of each
/// group (by its index), traced when built off the main thread
struct Built {
  groups: Vec<Group>,
  grids: Vec<(usize, TileColliders, Option<Traced>)>,
}

/// The bounds of a group of layers of `a`'s and `b`'s bounds: the larger,
/// or `None` (unbounded) if either is
fn joined(a: Option<CellRect>, b: Option<CellRect>) -> Option<CellRect> {
  Some(a?.union(b?))
}

/// Every group of a map's colliding layers (`planned`, read through
/// `views`), and each higher floor's edge, built from their tiles as they
/// are: each group's grid holds the chunks its layers have, each
/// classified across them from the cells where they lie, on the group's
/// floor. A floor's edge has no target until its entity spawns. Traced if
/// `trace`
fn build(
  map: Entity,
  (map_key, planned, views): (GroupKey, &[Planned], &[LayerView<'_>]),
  context: &Context<'_>,
  trace: bool,
) -> Built {
  let view = |index: usize| views.iter().find(|v| v.index == index);
  let mut groups: Vec<Group> = Vec::new();
  for layer in planned {
    let Some(view) = view(layer.index) else {
      continue;
    };
    let tracked = Tracked::new((layer.index, layer.entity), view.data, context.overrides);
    match groups.iter_mut().find(|g| g.key == layer.key) {
      Some(group) => {
        group.bounds = joined(group.bounds, layer.bounds);
        group.layers.push(tracked);
        group.footing.extend(layer.footing);
      }
      None => {
        let own = layer.key != map_key && !layer.key.edge;
        groups.push(Group {
          key: layer.key,
          target: match (layer.key.edge, own) {
            (true, _) => Entity::PLACEHOLDER,
            (false, true) => layer.entity,
            (false, false) => map,
          },
          target_layer: own.then_some(layer.index),
          bounds: layer.bounds,
          layers: vec![tracked],
          footing: layer.footing.into_iter().collect(),
          kinds: FastMap::default(),
          shapes: FastMap::default(),
        });
      }
    }
  }
  let mut grids = Vec::new();
  for (index, group) in groups.iter_mut().enumerate() {
    let grid = group.key.grid.grid();
    let cell = grid.cell.as_vec2();
    // A staggered grid's cells go to their lattice cells, which a rectangle
    // from (0, 0) does not hold
    let lattice = reindexing(&grid);
    let mut tiles = match (group.bounds, lattice) {
      (Some(bounds), None) => {
        let size = bounds.max.max(IVec2::ZERO).as_uvec2();
        TileColliders::new(size.x, size.y, cell)
      }
      _ => TileColliders::unbounded(cell),
    };
    if let Some(hex) = grid.hex_cells() {
      tiles = tiles.with_hex(hex, grid.origin);
    } else if let Some(to_pixel) = lattice_of(&grid) {
      tiles = tiles.with_lattice(to_pixel);
    }
    group.key.material().apply(&mut tiles);
    tiles.floor = Floor::On(group.key.floor);
    let data: Vec<Option<&TileLayerData>> = group
      .layers
      .iter()
      .map(|t| view(t.index).map(|v| v.data))
      .collect();
    // The chunks any of its layers has (an IntGrid layer's IntGrid too),
    // in row order
    let mut chunks: Vec<IVec2> = data
      .iter()
      .flatten()
      .flat_map(|data| build_chunks(data))
      .collect();
    chunks.sort_unstable_by_key(|c| (c.y, c.x));
    chunks.dedup();
    let mut pending = Pending(Vec::new());
    // A staggered grid's lattice chunks, gathered across its chunks
    let mut gathered: FastMap<IVec2, Box<ChunkKinds>> = FastMap::default();
    for chunk in chunks {
      let cells = group.kinds_of(&data, chunk, &mut pending, context);
      match &lattice {
        Some(lattice) => {
          for (at, kind) in scatter(lattice, chunk, &cells) {
            if kind == TileCell::Empty {
              continue;
            }
            let side = IVec2::splat(super::CHUNK_SIZE as i32);
            let local = at.rem_euclid(side);
            gathered
              .entry(at.div_euclid(side))
              .or_insert_with(|| Box::new([TileCell::Empty; CELLS]))
              [(local.y * side.x + local.x) as usize] = kind;
          }
        }
        None => tiles.put_chunk(chunk, &cells),
      }
    }
    let mut gathered: Vec<(IVec2, Box<ChunkKinds>)> = gathered.into_iter().collect();
    gathered.sort_unstable_by_key(|(c, _)| (c.y, c.x));
    for (chunk, cells) in gathered {
      tiles.put_chunk(chunk, &cells);
    }
    for shape in pending.0 {
      tiles.add_shape(shape);
    }
    let traced = if trace { tiles.trace() } else { None };
    grids.push((index, tiles, traced));
  }
  Built { groups, grids }
}

/// What a map's first build reads besides its layers: its root's settings,
/// its own properties and tile size, and its tilesets' shapes, stairs and
/// stamps
pub(crate) struct Inputs {
  pub(crate) collision: bool,
  pub(crate) overrides: Vec<(IntGridKey, TileCell)>,
  pub(crate) properties: Properties,
  /// The map's grid
  pub(crate) grid: MapGrid,
  pub(crate) shapes: Arc<[Option<ShapeSource>]>,
  pub(crate) stairs: Arc<[Option<StairSource>]>,
  pub(crate) tilesets: Vec<TilesetStamp>,
}

/// A map's collision made off the main thread, to land with its layers
pub(crate) struct Prepared {
  state: MapCollision,
  grids: Vec<(usize, TileColliders, Option<Traced>)>,
}

/// The collision of the map `map` as its layers (each with its place among
/// them) will spawn, on its floors (`floors`): every grid classified and
/// traced. Pure: any thread
pub(crate) fn prepare(
  map: Entity,
  layers: &[LayerView<'_>],
  inputs: Inputs,
  floors: Option<&MapFloors>,
) -> Prepared {
  let mut warned = Warned::default();
  let mut planned = Vec::new();
  let map_key = plan(
    map,
    layers.iter().copied(),
    (
      inputs.collision,
      &inputs.properties,
      &inputs.grid,
      &inputs.overrides,
    ),
    floors,
    &mut warned,
    &mut planned,
  );
  let context =
    Context::new(&inputs.shapes, &inputs.overrides, Purpose::Collision).with_stairs(&inputs.stairs);
  let built = build(map, (map_key, &planned, layers), &context, true);
  warn_both_ways(map, &context, &mut warned);
  Prepared {
    state: MapCollision {
      layers: planned,
      grid: inputs.grid,
      tilesets: inputs.tilesets,
      overrides: inputs.overrides,
      shapes: inputs.shapes,
      stairs: inputs.stairs,
      groups: built.groups,
      torn: false,
      warned_properties: warned,
    },
    grids: built.grids,
  }
}

/// Puts a map's prepared collision on it and its layers (`layers`, the
/// entities its layers spawned as, by place), and its floors' edges on
/// entities of their own, as the map lands
pub(crate) fn land(world: &mut World, map: Entity, layers: &[Entity], prepared: Prepared) {
  let Prepared { mut state, grids } = prepared;
  for planned in &mut state.layers {
    if let Some(&entity) = layers.get(planned.index) {
      planned.entity = entity;
    }
  }
  for group in &mut state.groups {
    group.target = if group.key.edge {
      spawn_edge(world, map, &group.key)
    } else {
      group
        .target_layer
        .and_then(|index| layers.get(index).copied())
        .unwrap_or(map)
    };
    for tracked in &mut group.layers {
      tracked.relink(layers);
    }
  }
  for (group, tiles, traced) in grids {
    let target = state.groups[group].target;
    if let Ok(mut entity) = world.get_entity_mut(target) {
      match traced {
        Some(traced) => entity.insert((tiles, traced)),
        None => entity.insert(tiles),
      };
    }
  }
  if let Ok(mut entity) = world.get_entity_mut(map) {
    entity.insert(state);
  }
}

/// The edge of the floor `key` is the edge of, as it lands: a child of
/// `map` where the floor's layers stand, a [`MapFloor`] (`PLACEHOLDER` for a
/// map gone)
fn spawn_edge(world: &mut World, map: Entity, key: &GroupKey) -> Entity {
  if world.get_entity(map).is_err() {
    return Entity::PLACEHOLDER;
  }
  let edge = child_of(world, map, Transform::from_translation(key.offset()));
  if let Ok(mut entity) = world.get_entity_mut(edge) {
    entity.insert(MapFloor { floor: key.floor });
  }
  edge
}

/// Despawns the floors' edges `map`'s collision spawned, as its root goes
/// (put over or taken away), so a root landing at once in its place never
/// leaves them behind; the next step takes the rest
pub(crate) fn drop_edges(world: &mut World, map: Entity) {
  let Some(state) = world.get::<MapCollision>(map) else {
    return;
  };
  let edges: Vec<Entity> = state
    .groups
    .iter()
    .filter(|group| group.key.edge)
    .map(|group| group.target)
    .collect();
  for edge in edges {
    let _ = world.try_despawn(edge);
  }
}

/// Takes down what `groups` put on: each grid from the map or its layer,
/// and a floor's edge with its entity
fn take_down(groups: &[Group], commands: &mut Commands) {
  for group in groups {
    let mut target = commands.entity(group.target);
    if group.key.edge {
      target.try_despawn();
    } else {
      target.try_remove::<TileColliders>();
    }
  }
}

/// A map's collision: what it was built from, its groups, and what it has
/// warned about. On the map entity
#[derive(Component, Debug)]
pub(crate) struct MapCollision {
  layers: Vec<Planned>,
  /// The map's grid
  grid: MapGrid,
  tilesets: Vec<TilesetStamp>,
  overrides: Vec<(IntGridKey, TileCell)>,
  /// The tilesets' shapes and stairs it reads
  shapes: Arc<[Option<ShapeSource>]>,
  stairs: Arc<[Option<StairSource>]>,
  groups: Vec<Group>,
  /// Its colliders were taken away (the map disabled, or one went missing):
  /// the next step builds them all
  torn: bool,
  warned_properties: Warned,
}

impl MapCollision {
  /// Takes every `TileColliders` this map put on, and its floors' edges
  fn tear_down(&mut self, commands: &mut Commands) {
    take_down(&self.groups, commands);
    self.torn = true;
  }
}

/// Brings a group's grid up to its layers' tiles: the cells of every chunk
/// whose version moved since it last looked, classified across its layers
/// from the cells where they lie, setting only those whose kind changed,
/// and every chunk its layers no longer have (unloaded) taken out whole.
/// On a staggered grid each cell is set at its lattice cell, and an
/// unloaded chunk's cells leave one by one, since they straddle the
/// lattice's chunks: those left holding nothing go
fn update(group: &mut Group, tiles: &mut TileColliders, layers: &Layers, context: &Context<'_>) {
  let changed = changed_chunks(&mut group.layers, layers, context.overrides);
  if changed.is_empty() {
    return;
  }
  let lattice = reindexing(&group.key.grid.grid());
  let data = layer_data(&group.layers, layers);
  let data: Vec<Option<&TileLayerData>> = data.iter().map(|d| d.as_deref()).collect();
  for chunk in changed {
    if chunk_origin(chunk).is_none() {
      continue;
    }
    let now = group.kinds_of(&data, chunk, &mut *tiles, context);
    if let Some(lattice) = &lattice {
      tiles.set_cells(scatter(lattice, chunk, &now));
      continue;
    }
    let empty = [TileCell::Empty; CELLS];
    // A chunk none of the group's grids has (unloaded) and nothing else
    // fills goes whole, its storage with it
    let gone = now == empty && data.iter().flatten().all(|d| !d.tiles.has_chunk(chunk));
    if gone {
      tiles.unload_chunk(chunk);
      continue;
    }
    tiles.set_chunk(chunk, &now);
  }
  // The extent worked out once for every chunk that went
  tiles.settle_extent();
}

type Maps<'w, 's> = Query<
  'w,
  's,
  (
    Entity,
    &'static MapRoot,
    Option<&'static MapLayers>,
    Option<&'static MapFloors>,
    Option<&'static mut MapCollision>,
    Has<Disabled>,
  ),
  Allow<Disabled>,
>;
type Unrooted<'w, 's> =
  Query<'w, 's, (Entity, &'static mut MapCollision), (Without<MapRoot>, Allow<Disabled>)>;
type Grids<'w, 's> = Query<'w, 's, &'static mut TileColliders, Allow<Disabled>>;

/// The map's enabled tile layers, as planning and a build read them
fn views<'a>(
  map_layers: &'a MapLayers,
  layers: &'a Layers,
) -> impl Iterator<Item = LayerView<'a>> + Clone + 'a {
  map_layers
    .0
    .iter()
    .enumerate()
    .filter_map(|(index, &entity)| {
      let (info, data, disabled) = layers.get(entity).ok()?;
      (!disabled).then(|| LayerView {
        index,
        entity,
        info,
        data: data.into_inner(),
      })
    })
}

/// `FixedSet::Movement`, before the tile colliders rebuild: every spawned
/// map's colliding layers into its `TileColliders`, as their tiles are now.
/// A disabled map has none (enabled, it builds them from the tiles as they
/// then are); a map whose root went takes its colliders away. An idle step
/// costs a look at each map's colliding layers, tilesets and properties
pub(crate) fn sync_map_colliders(
  mut maps: Maps,
  mut unrooted: Unrooted,
  layers: Layers,
  mut grids: Grids,
  (tile_maps, projects, tilesets): Stores,
  mut planned: Local<Vec<Planned>>,
  mut commands: Commands,
) {
  let tilesets = tilesets.as_deref();
  for (map, mut state) in &mut unrooted {
    state.tear_down(&mut commands);
    commands.entity(map).try_remove::<MapCollision>();
  }
  for (map, root, map_layers, floors, mut state, disabled) in &mut maps {
    let Some(map_layers) = map_layers else {
      // The root was put over or taken away, and its layers with it
      if let Some(mut state) = state {
        state.tear_down(&mut commands);
        commands.entity(map).try_remove::<MapCollision>();
      }
      continue;
    };
    if disabled {
      if let Some(mut state) = state.filter(|s| !s.torn) {
        state.tear_down(&mut commands);
      }
      continue;
    }
    let Some(tile_map) = root_map(root, tile_maps.as_deref(), projects.as_deref()) else {
      continue;
    };
    // A property of the wrong type warns once a map
    let mut first_warnings = Warned::default();
    let warned = match state.as_mut() {
      Some(state) => &mut state.bypass_change_detection().warned_properties,
      None => &mut first_warnings,
    };
    let map_key = plan(
      map,
      views(map_layers, &layers),
      (
        root.collision,
        &tile_map.properties,
        &tile_map.grid(),
        &root.intgrid_collision,
      ),
      floors,
      warned,
      &mut planned,
    );
    let same = state.as_mut().is_some_and(|s| {
      let s = s.bypass_change_detection();
      let read = (&s.shapes[..], Some(&s.stairs[..]));
      !s.torn
        && s.layers == *planned
        && s.grid == tile_map.grid()
        && s.overrides == root.intgrid_collision
        && shapes_kept(&mut s.tilesets, read, tile_map, tilesets)
    });
    match state {
      Some(mut state) if same => {
        let state = state.bypass_change_detection();
        let context = Context::new(&state.shapes, &root.intgrid_collision, Purpose::Collision)
          .with_stairs(&state.stairs);
        for group in &mut state.groups {
          let Ok(mut tiles) = grids.get_mut(group.target) else {
            // Taken away by something else: all of it is built next step
            state.torn = true;
            continue;
          };
          update(group, &mut tiles, &layers, &context);
        }
        warn_both_ways(map, &context, &mut state.warned_properties);
      }
      state => {
        // Built at once, so a map never lacks its colliders
        let shapes = sources(tile_map, tilesets);
        let stairs = stair_sources(tile_map, tilesets);
        let mut warned_properties = match &state {
          Some(old) => old.warned_properties.clone(),
          None => first_warnings,
        };
        if let Some(old) = &state {
          take_down(&old.groups, &mut commands);
        }
        let context =
          Context::new(&shapes, &root.intgrid_collision, Purpose::Collision).with_stairs(&stairs);
        let views: Vec<LayerView<'_>> = views(map_layers, &layers).collect();
        let Built { mut groups, grids } = build(map, (map_key, &planned, &views), &context, false);
        warn_both_ways(map, &context, &mut warned_properties);
        for (group, tiles, _) in grids {
          let group = &mut groups[group];
          if !group.key.edge {
            commands.entity(group.target).try_insert(tiles);
            continue;
          }
          // A floor's edge on an entity of its own, linked to the map before
          // anything a game observes goes on it
          let at = Transform::from_translation(group.key.offset());
          group.target = commands.spawn((at, ChildOf(map))).id();
          let edge = MapFloor {
            floor: group.key.floor,
          };
          commands.entity(group.target).try_insert((edge, tiles));
        }
        let fresh = MapCollision {
          layers: planned.clone(),
          grid: tile_map.grid(),
          tilesets: stamps(tile_map, tilesets),
          overrides: root.intgrid_collision.clone(),
          shapes: Arc::clone(&shapes),
          stairs: Arc::clone(&stairs),
          groups,
          torn: false,
          warned_properties,
        };
        match state {
          Some(mut state) => *state = fresh,
          None => {
            commands.entity(map).try_insert(fresh);
          }
        }
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use std::sync::{Arc, Mutex};

  use glam::{IVec2, UVec2, Vec2};

  use crate::core::asset::{Assets, AssetsView, Handle};
  use crate::core::ecs::Transform;
  use crate::core::ecs::{ChildOf, Disabled, Entity, Floor, World};
  use crate::core::engine::EngineBuilder;
  use crate::core::gameplay::{MoveIntent, PlatformerMovement, TopDownMovement};
  use crate::core::physics::CharacterState;
  use crate::core::physics::{
    CellRect, Collider, ColliderShape, SlopeCorner, TileCell, TileColliders, TilePart,
    in_grid_space,
  };
  use crate::core::scene::{Scene, SceneContext};
  use crate::core::testing::HeadlessEngine;
  use crate::core::tilemap::test_util::{
    MapWorld, Px, files_with, layers_of, load_map as load, union_border, whole_hex_set, whole_px,
  };
  use crate::core::tilemap::{
    GridShape, IntGrid, LayerKind, MapFloor, MapGrid, MapRoot, MapsView, PropertyValue,
    StaggerAxis, StaggerIndex, Tile, TileData, TileFlip, TileLayerData, TileMap, TileShape,
    Tileset, TilesetImage, TilesetSource,
  };

  /// Tile ids of `shapes`
  const SOLID: u32 = 0;
  const SLOPE: u32 = 1;
  const LIGHT_ONLY: u32 = 2;
  const HALF: u32 = 3;
  const PLATFORM: u32 = 4;

  fn rect(x: f32, y: f32, w: f32, h: f32) -> Vec<Vec2> {
    vec![
      Vec2::new(x, y),
      Vec2::new(x + w, y),
      Vec2::new(x + w, y + h),
      Vec2::new(x, y + h),
    ]
  }

  fn shape(points: Vec<Vec2>, occluder: bool) -> TileShape {
    TileShape {
      points,
      occluder,
      ..TileShape::default()
    }
  }

  /// 16 px tiles: a solid block, a slope filled at its bottom right, a
  /// shape that only blocks light, and a half block
  fn shapes() -> Tileset {
    let mut set = Tileset::new("shapes", UVec2::splat(16), TilesetImage::Collection);
    let with = |shapes: Vec<TileShape>| TileData {
      shapes,
      ..TileData::default()
    };
    set.insert_tile(SOLID, with(vec![shape(rect(0.0, 0.0, 16.0, 16.0), false)]));
    let slope = vec![
      Vec2::new(16.0, 0.0),
      Vec2::new(16.0, 16.0),
      Vec2::new(0.0, 16.0),
    ];
    set.insert_tile(SLOPE, with(vec![shape(slope, false)]));
    set.insert_tile(
      LIGHT_ONLY,
      with(vec![shape(rect(4.0, 2.0, 8.0, 12.0), true)]),
    );
    set.insert_tile(HALF, with(vec![shape(rect(0.0, 8.0, 16.0, 8.0), false)]));
    let platform = TileShape {
      one_way: true,
      ..shape(rect(0.0, 0.0, 16.0, 16.0), false)
    };
    set.insert_tile(PLATFORM, with(vec![platform]));
    set
  }

  /// A map of `size` 16 px cells over `shapes`, with a tile layer of each
  /// name holding the tiles listed
  fn code_map(size: UVec2, layers: &[(&str, &[(IVec2, u32)])]) -> TileMap {
    let mut map = TileMap::new(size, UVec2::splat(16));
    let set = map.add_tileset(TilesetSource::Embedded(Arc::new(shapes())));
    for (name, tiles) in layers {
      let layer = map.add_tile_layer(name);
      let grid = map.tiles_mut(layer).expect("a tile layer");
      for &(cell, id) in *tiles {
        grid.set(cell, Some(Tile::new(set, id)));
      }
    }
    map
  }

  fn add(world: &mut World, map: TileMap) -> Handle<TileMap> {
    world.resource_mut::<Assets<TileMap>>().add(map)
  }

  fn colliders(world: &World, entity: Entity) -> TileColliders {
    match world.get::<TileColliders>(entity) {
      Some(tiles) => tiles.clone(),
      None => panic!("{entity:?} has no TileColliders"),
    }
  }

  fn count_parts(world: &mut World) -> usize {
    world.query::<&TilePart>().iter(world).count()
  }

  /// The parts under `parent`, by entity, their colliders in its space
  /// (a part sits at its chunk's top-left)
  fn parts_of(world: &mut World, parent: Entity) -> Vec<(Entity, Collider)> {
    let mut found: Vec<(Entity, Collider)> = world
      .query::<(Entity, &ChildOf, &Collider, &Transform)>()
      .iter(world)
      .filter(|(_, child_of, ..)| child_of.parent() == parent)
      .map(|(e, _, c, at)| (e, in_grid_space(c.clone(), at.translation)))
      .collect();
    found.sort_by_key(|(e, _)| *e);
    found
  }

  /// The one part with a point in `min..=max` (px)
  fn part_in(parts: &[(Entity, Collider)], min: Vec2, max: Vec2) -> Entity {
    let points = |c: &Collider| match &c.shape {
      ColliderShape::Polyline { points }
      | ColliderShape::Polygon { points }
      | ColliderShape::Segments { points, .. } => points.clone(),
      ColliderShape::Segment { a, b } => vec![*a, *b],
      _ => Vec::new(),
    };
    let inside = |p: &Vec2| p.cmpge(min).all() && p.cmple(max).all();
    let found: Vec<Entity> = parts
      .iter()
      .filter(|(_, c)| points(c).iter().any(inside))
      .map(|(e, _)| *e)
      .collect();
    assert_eq!(found.len(), 1, "one part in {min}..{max}: {parts:?}");
    found[0]
  }

  /// Whether `polygon` has exactly the points `expected`, in any order
  fn same_points(polygon: &[Vec2], expected: &[(f32, f32)]) -> bool {
    polygon.len() == expected.len()
      && expected.iter().all(|&(x, y)| {
        polygon
          .iter()
          .any(|p| (*p - Vec2::new(x, y)).abs().max_element() < 1e-4)
      })
  }

  #[test]
  fn tiled_collision_shapes_become_cells_after_flips() {
    use SlopeCorner::{BottomLeft, BottomRight, TopRight};
    let mut maps = MapWorld::new();
    let level = load(&mut maps.world, "tiled/maps/level.tmj");
    let map = maps.spawn(MapRoot::new(level));
    let tiles = colliders(&maps.world, map);
    assert_eq!(
      tiles.bounds(),
      Some(CellRect::from_size(UVec2::new(24, 12)))
    );
    assert_eq!(tiles.tile_size, Vec2::splat(16.0));
    // The floor is two layers' tiles, in one grid
    assert!((0..24).all(|x| tiles.get(IVec2::new(x, 11)) == TileCell::Solid));
    // One slope tile, as drawn, mirrored and turned
    assert_eq!(tiles.get(IVec2::new(20, 10)), TileCell::Slope(BottomRight));
    assert_eq!(
      tiles.get(IVec2::new(21, 10)),
      TileCell::Slope(BottomLeft),
      "mirrored"
    );
    assert_eq!(
      tiles.get(IVec2::new(22, 10)),
      TileCell::Slope(TopRight),
      "upside down"
    );
    assert_eq!(
      tiles.get(IVec2::new(23, 10)),
      TileCell::Slope(BottomLeft),
      "turned"
    );
    assert!((4..8).all(|x| tiles.get(IVec2::new(x, 7)) == TileCell::OneWay));
    let TileCell::Shape(half) = tiles.get(IVec2::new(18, 10)) else {
      panic!("the half block is {:?}", tiles.get(IVec2::new(18, 10)))
    };
    let half = tiles.shape(half).expect("the half block's shape");
    assert!(!half.one_way);
    assert_eq!(half.polygons.len(), 1);
    assert!(
      same_points(
        &half.polygons[0],
        &[(0.0, 0.5), (1.0, 0.5), (1.0, 1.0), (0.0, 1.0)]
      ),
      "{:?}",
      half.polygons
    );
    assert_eq!(
      tiles.get(IVec2::new(14, 10)),
      TileCell::Empty,
      "decor does not collide"
    );
    assert_eq!(
      tiles.get(IVec2::new(10, 5)),
      TileCell::Empty,
      "an occluder only occludes"
    );
    // The material is the map's properties (its layers say none)
    assert_eq!((tiles.layer, tiles.mask), (2, u32::MAX));
    assert_eq!((tiles.friction, tiles.restitution), (0.25, 0.0));
    for layer in layers_of(&mut maps.world, map) {
      assert!(maps.world.get::<TileColliders>(layer).is_none());
    }
    // and physics builds it in that step
    assert!(!parts_of(&mut maps.world, map).is_empty());
  }

  #[test]
  fn flips_and_offsets_place_shapes_where_the_tile_draws() {
    use SlopeCorner::{BottomLeft, BottomRight, TopLeft, TopRight};
    let h = TileFlip::HORIZONTAL;
    let v = TileFlip::VERTICAL;
    let d = TileFlip::DIAGONAL;
    let mut maps = MapWorld::new();
    let flips = [
      (TileFlip::NONE, BottomRight),
      (h, BottomLeft),
      (v, TopRight),
      (h | v, TopLeft),
      // the diagonal mirrors this one onto itself
      (d, BottomRight),
      (d | h, BottomLeft),
      (d | v, TopRight),
      (d | h | v, TopLeft),
    ];
    let mut map = code_map(UVec2::new(8, 2), &[("ground", &[])]);
    for (x, (flip, _)) in flips.iter().enumerate() {
      let tile = Tile::new(0, SLOPE).with_flip(*flip);
      map
        .tiles_mut(0)
        .map(|g| g.set(IVec2::new(x as i32, 0), Some(tile)));
    }
    // A half block in a tileset drawn 8 px lower: its shape leaves the cell
    // through the bottom, and the cell keeps the top half of what is left
    let mut low = shapes();
    low.tile_offset = IVec2::new(0, -8);
    let low = map.add_tileset(TilesetSource::Embedded(Arc::new(low)));
    map
      .tiles_mut(0)
      .map(|g| g.set(IVec2::new(0, 1), Some(Tile::new(low, HALF))));
    let handle = add(&mut maps.world, map);
    let map = maps.spawn(MapRoot::new(handle));
    let tiles = colliders(&maps.world, map);
    for (x, (flip, corner)) in flips.iter().enumerate() {
      assert_eq!(
        tiles.get(IVec2::new(x as i32, 0)),
        TileCell::Slope(*corner),
        "flip {flip:?}"
      );
    }
    let TileCell::Shape(raised) = tiles.get(IVec2::new(0, 1)) else {
      panic!("the raised half block is {:?}", tiles.get(IVec2::new(0, 1)))
    };
    let raised = tiles.shape(raised).expect("its shape");
    assert!(
      same_points(
        &raised.polygons[0],
        &[(0.0, 0.0), (1.0, 0.0), (1.0, 0.5), (0.0, 0.5)]
      ),
      "{:?}",
      raised.polygons
    );
  }

  #[test]
  fn a_tile_larger_than_its_cell_collides_where_it_draws() {
    // A 32 x 32 tile on 16 px cells stands on its cell's bottom edge from
    // the left: its bottom-left quarter is the cell
    let mut big = Tileset::new("big", UVec2::splat(32), TilesetImage::Collection);
    big.insert_tile(
      0,
      TileData {
        shapes: vec![shape(rect(0.0, 16.0, 16.0, 16.0), false)],
        ..TileData::default()
      },
    );
    big.insert_tile(
      1,
      TileData {
        shapes: vec![shape(rect(16.0, 16.0, 16.0, 16.0), false)],
        ..TileData::default()
      },
    );
    let mut map = TileMap::new(UVec2::new(2, 1), UVec2::splat(16));
    let set = map.add_tileset(TilesetSource::Embedded(Arc::new(big)));
    let layer = map.add_tile_layer("trees");
    if let Some(grid) = map.tiles_mut(layer) {
      grid.set(IVec2::new(0, 0), Some(Tile::new(set, 0)));
      grid.set(IVec2::new(1, 0), Some(Tile::new(set, 1)));
    }
    let mut maps = MapWorld::new();
    let handle = add(&mut maps.world, map);
    let map = maps.spawn(MapRoot::new(handle));
    let tiles = colliders(&maps.world, map);
    assert_eq!(
      tiles.get(IVec2::new(0, 0)),
      TileCell::Solid,
      "the quarter over its cell"
    );
    // the other tile's square is right of its cell: nothing of it is inside
    match tiles.get(IVec2::new(1, 0)) {
      TileCell::Empty => {}
      TileCell::Shape(index) => {
        let polygon = &tiles.shape(index).expect("its shape").polygons[0];
        assert!(polygon.iter().all(|p| p.x >= 1.0 - 1e-4), "{polygon:?}");
      }
      other => panic!("the square right of its cell is {other:?}"),
    }
  }

  #[test]
  fn int_grid_names_collide_by_default_and_overrides_apply() {
    use SlopeCorner::{BottomLeft, TopRight};
    use TileCell::{Empty, OneWay, Slope, Solid};
    let mut map = code_map(
      UVec2::new(8, 1),
      &[("collision", &[(IVec2::new(7, 0), SOLID)])],
    );
    let names = [
      (1, "Solid"),
      (2, "one_way"),
      (3, "SLOPE_BOTTOM_LEFT"),
      (4, "slope_top_right"),
      (5, "Wall"),
      (6, "lava"),
      (7, "water"),
    ];
    if let LayerKind::Tiles { intgrid, .. } = &mut map.layers[0].kind {
      *intgrid = Some(IntGrid {
        size: UVec2::new(8, 1),
        values: vec![1, 2, 3, 4, 5, 6, 7, 0],
        names: names.iter().map(|(v, n)| (*v, n.to_string())).collect(),
      });
    }
    let mut maps = MapWorld::new();
    let root = MapRoot::new(add(&mut maps.world, map))
      .with_intgrid_collision("wall", Solid)
      .with_intgrid_collision("lava", Solid)
      .with_intgrid_collision(6, OneWay)
      .with_intgrid_collision(2, Empty);
    let map = maps.spawn(root);
    let tiles = colliders(&maps.world, map);
    let row: Vec<TileCell> = (0..8).map(|x| tiles.get(IVec2::new(x, 0))).collect();
    // by name (any case), a value over a name, nothing for an unknown name;
    // the layer's tiles (its auto-tiles) do not collide, its IntGrid does
    assert_eq!(
      row,
      [
        Solid,
        Empty,
        Slope(BottomLeft),
        Slope(TopRight),
        Solid,
        OneWay,
        Empty,
        Empty
      ]
    );
  }

  #[test]
  fn an_int_grid_edit_re_collides_only_its_chunk() {
    use TileCell::{Empty, OneWay, Solid};
    // 64 x 40 cells: two chunks across, two down
    let size = UVec2::new(64, 40);
    let mut map = code_map(size, &[("collision", &[]), ("art", &[])]);
    if let LayerKind::Tiles { intgrid, .. } = &mut map.layers[0].kind {
      *intgrid = Some(IntGrid {
        size,
        values: vec![0; 64 * 40],
        names: [(1, "solid"), (2, "one_way")]
          .iter()
          .map(|(v, n)| (*v, n.to_string()))
          .collect(),
      });
    }
    let mut maps = MapWorld::new();
    let handle = add(&mut maps.world, map);
    let map = maps.spawn(MapRoot::new(handle));
    let layer = layers_of(&mut maps.world, map)[0];
    let chunks = [
      IVec2::new(0, 0),
      IVec2::new(1, 0),
      IVec2::new(0, 1),
      IVec2::new(1, 1),
    ];
    let versions = |world: &World| -> Vec<u64> {
      let Some(data) = world.get::<TileLayerData>(layer) else {
        panic!("no layer data");
      };
      chunks
        .iter()
        .map(|&c| data.tiles.chunk_version(c))
        .collect()
    };
    let before = versions(&maps.world);

    // One cell: its chunk alone moves, and it collides in the next step
    let cell = IVec2::new(40, 5);
    let mut view = MapsView::new(&mut maps.world);
    assert!(view.set_intgrid(map, "collision", cell, 1));
    assert!(!view.set_intgrid(map, "collision", cell, 1), "unchanged");
    assert_eq!(view.intgrid(map, "collision", cell), Some(1));
    let after = versions(&maps.world);
    assert_eq!(after[0], before[0]);
    assert_ne!(after[1], before[1], "the cell's chunk moved");
    assert_eq!(after[2..], before[2..]);
    maps.frame();
    assert_eq!(colliders(&maps.world, map).get(IVec2::new(40, 5)), Solid);

    // A rectangle: the chunks it changed alone move
    let mut view = MapsView::new(&mut maps.world);
    let filled = view.fill_intgrid(map, "collision", IVec2::new(-4, 36), IVec2::new(70, 99), 2);
    assert_eq!(filled, 64 * 4, "clipped to the layer");
    let moved = versions(&maps.world);
    assert_eq!(moved[..2], after[..2]);
    assert!(moved[2] != after[2] && moved[3] != after[3]);
    maps.frame();
    let tiles = colliders(&maps.world, map);
    assert_eq!(
      (tiles.get(IVec2::new(0, 36)), tiles.get(IVec2::new(63, 39))),
      (OneWay, OneWay)
    );
    assert_eq!(tiles.get(IVec2::new(0, 35)), Empty);

    // 0 empties a cell; outside, or a layer with no IntGrid, changes nothing
    let mut view = MapsView::new(&mut maps.world);
    assert!(view.set_intgrid(map, "collision", cell, 0));
    assert_eq!(view.intgrid(map, "collision", cell), None);
    assert!(!view.set_intgrid(map, "collision", IVec2::new(64, 0), 1));
    assert!(!view.set_intgrid(map, "art", IVec2::ZERO, 1));
    assert_eq!(view.fill_intgrid(map, "art", IVec2::ZERO, IVec2::ONE, 1), 0);
    maps.frame();
    assert_eq!(colliders(&maps.world, map).get(IVec2::new(40, 5)), Empty);
  }

  #[test]
  fn an_int_grid_whose_grid_a_game_swapped_collides_and_casts_shadows_after_a_rebuild() {
    // 64 x 64 cells, four chunks, a solid IntGrid cell in the last
    let size = UVec2::new(64, 64);
    let far = IVec2::new(40, 40);
    let mut map = code_map(size, &[("collision", &[])]);
    if let LayerKind::Tiles { intgrid, .. } = &mut map.layers[0].kind {
      let mut values = vec![0; 64 * 64];
      values[(far.y * 64 + far.x) as usize] = 1;
      *intgrid = Some(IntGrid {
        size,
        values,
        names: [(1, "solid".to_string())].into_iter().collect(),
      });
    }
    let mut maps = MapWorld::new();
    let handle = add(&mut maps.world, map);
    let map = maps.spawn(MapRoot::new(handle));
    let layer = layers_of(&mut maps.world, map)[0];
    let shadowed = |world: &mut World| {
      world
        .query::<&crate::core::tilemap::MapOccluder>()
        .iter(world)
        .any(|o| o.chunk == IVec2::ONE)
    };
    // The game swaps the layer's grid for a new one holding one tile, so
    // the new grid has one chunk, not the IntGrid's
    let mut grid = crate::core::tilemap::TileGrid::new(size);
    grid.set(IVec2::ONE, Some(Tile::new(0, SOLID)));
    if let Some(mut data) = maps.world.get_mut::<TileLayerData>(layer) {
      data.tiles = grid;
    }
    maps.frame();
    assert_eq!(colliders(&maps.world, map).get(far), TileCell::Solid);
    assert!(shadowed(&mut maps.world), "the IntGrid casts its shadow");
    // A full rebuild (the map disabled, then enabled) reads every chunk of
    // the IntGrid, not only the grid's
    maps.world.entity_mut(map).insert(Disabled);
    maps.frame();
    maps.world.entity_mut(map).remove::<Disabled>();
    maps.frame();
    assert_eq!(
      colliders(&maps.world, map).get(far),
      TileCell::Solid,
      "the IntGrid still collides"
    );
    assert!(shadowed(&mut maps.world), "and still casts its shadow");
  }

  #[test]
  fn an_int_grid_shape_override_collides_as_nothing_and_warns_once() {
    let mut map = code_map(UVec2::new(4, 1), &[("collision", &[])]);
    if let LayerKind::Tiles { intgrid, .. } = &mut map.layers[0].kind {
      *intgrid = Some(IntGrid {
        size: UVec2::new(4, 1),
        values: vec![1, 1, 2, 0],
        names: [(1, "lava-7c1e"), (2, "solid")]
          .iter()
          .map(|(v, n)| (*v, n.to_string()))
          .collect(),
      });
    }
    let mut maps = MapWorld::new();
    let handle = add(&mut maps.world, map);
    let root = MapRoot::new(handle).with_intgrid_collision("lava-7c1e", TileCell::Shape(0));
    let map = maps.spawn(root);
    maps.frame();
    // An edit rebuilds through the main thread's path: still one warning
    assert!(MapsView::new(&mut maps.world).set_intgrid(map, 0, IVec2::new(3, 0), 1));
    maps.frame();
    let tiles = colliders(&maps.world, map);
    let row: Vec<TileCell> = (0..4).map(|x| tiles.get(IVec2::new(x, 0))).collect();
    use TileCell::{Empty, Solid};
    assert_eq!(row, [Empty, Empty, Solid, Empty]);
    let warned = crate::core::dev_log::get_entries()
      .iter()
      .filter(|e| e.message.contains("lava-7c1e") && e.message.contains("Shape"))
      .count();
    assert_eq!(warned, 1, "warned once, naming the value");
  }

  #[test]
  fn an_ldtk_int_grid_collides_by_its_names() {
    let mut maps = MapWorld::new();
    let project = match AssetsView::new(&mut maps.world)
      .load_now::<crate::core::tilemap::LdtkProject>("ldtk/internal.ldtk")
    {
      Ok(project) => project,
      Err(error) => panic!("{}", error.report()),
    };
    let level = maps
      .world
      .resource::<Assets<crate::core::tilemap::LdtkProject>>()
      .get(&project)
      .and_then(|p| p.levels.first().map(|l| l.identifier.clone()))
      .expect("a level");
    let root = MapRoot::ldtk(project, &level).with_intgrid_collision("Wall", TileCell::Solid);
    let map = maps.spawn(root);
    // The IntGrid layer is the one that collides; its walls are solid
    let walls = maps
      .world
      .get::<TileColliders>(map)
      .map(|tiles| {
        let cells = tiles.extent();
        (cells.min.y..cells.max.y)
          .flat_map(|y| (cells.min.x..cells.max.x).map(move |x| IVec2::new(x, y)))
          .filter(|&cell| tiles.get(cell) == TileCell::Solid)
          .count()
      })
      .unwrap_or(0);
    assert_eq!(walls, 9, "the level's walls collide");
  }

  #[test]
  fn a_layer_with_collides_false_adds_nothing() {
    let floor: Vec<(IVec2, u32)> = (0..4).map(|x| (IVec2::new(x, 0), SOLID)).collect();
    let mut maps = MapWorld::new();
    let mut off = code_map(UVec2::new(4, 1), &[("walls", &floor)]);
    off.layers[0].info.collides = false;
    let handle = add(&mut maps.world, off);
    let off = maps.spawn(MapRoot::new(handle));
    let on = code_map(UVec2::new(4, 1), &[("walls", &floor)]);
    let on = add(&mut maps.world, on);
    let quiet = maps.spawn(MapRoot::new(on.clone()).without_collision());
    maps.frame();
    for map in [off, quiet] {
      assert!(maps.world.get::<TileColliders>(map).is_none());
      for layer in layers_of(&mut maps.world, map) {
        assert!(maps.world.get::<TileColliders>(layer).is_none());
      }
    }
    assert_eq!(count_parts(&mut maps.world), 0);
    // The same layer colliding builds its floor
    let loud = maps.spawn(MapRoot::new(on));
    assert_eq!(
      colliders(&maps.world, loud).get(IVec2::new(3, 0)),
      TileCell::Solid
    );
    assert_eq!(count_parts(&mut maps.world), 1);
  }

  /// A floor 40 cells long on row 3 and an island at (36, 0), on 16 px
  /// cells: the floor's outline is chunk 0's collider, the island's chunk 1's
  fn floor_and_island(maps: &mut MapWorld) -> Entity {
    let mut tiles: Vec<(IVec2, u32)> = (0..40).map(|x| (IVec2::new(x, 3), SOLID)).collect();
    tiles.push((IVec2::new(36, 0), SOLID));
    let map = code_map(UVec2::new(40, 4), &[("ground", &tiles)]);
    let handle = add(&mut maps.world, map);
    maps.spawn(MapRoot::new(handle))
  }

  #[test]
  fn an_edit_updates_its_cells_before_the_next_step() {
    let mut maps = MapWorld::new();
    let map = floor_and_island(&mut maps);
    let parts = parts_of(&mut maps.world, map);
    let island_area = (Vec2::new(576.0, 0.0), Vec2::new(592.0, 16.0));
    let island = part_in(&parts, island_area.0, island_area.1);
    let mut view = MapsView::new(&mut maps.world);
    assert!(view.set_tile(map, "ground", IVec2::new(10, 1), Some(Tile::new(0, SOLID))));
    assert!(view.set_tile(map, "ground", IVec2::new(39, 3), None));
    // One fixed step: the cells change and physics builds them
    maps.step();
    let tiles = colliders(&maps.world, map);
    assert_eq!(tiles.get(IVec2::new(10, 1)), TileCell::Solid);
    assert_eq!(tiles.get(IVec2::new(39, 3)), TileCell::Empty);
    let parts = parts_of(&mut maps.world, map);
    part_in(&parts, Vec2::new(160.0, 16.0), Vec2::new(176.0, 32.0));
    let floor = part_in(&parts, Vec2::new(0.0, 48.0), Vec2::new(0.0, 64.0));
    let right = parts
      .iter()
      .find(|(e, _)| *e == floor)
      .map(|(_, c)| match &c.shape {
        ColliderShape::Polyline { points } | ColliderShape::Segments { points, .. } => {
          points.iter().map(|p| p.x).fold(0.0, f32::max)
        }
        _ => 0.0,
      });
    assert_eq!(right, Some(39.0 * 16.0), "the floor lost its last cell");
    // The island's outline was not touched
    assert_eq!(part_in(&parts, island_area.0, island_area.1), island);
  }

  #[test]
  fn an_edit_synced_early_by_a_navigation_grid_collides_in_that_step() {
    use crate::core::navigation::GridCoord;
    use crate::core::physics::Physics;
    let mut maps = MapWorld::new();
    let map = floor_and_island(&mut maps);
    let parts = parts_of(&mut maps.world, map);
    let island_area = (Vec2::new(576.0, 0.0), Vec2::new(592.0, 16.0));
    let island = part_in(&parts, island_area.0, island_area.1);
    let floor_area = (Vec2::new(0.0, 48.0), Vec2::new(0.0, 64.0));
    let floor = part_in(&parts, floor_area.0, floor_area.1);
    let mut view = MapsView::new(&mut maps.world);
    assert!(view.set_tile(map, "ground", IVec2::new(10, 1), Some(Tile::new(0, SOLID))));
    assert!(view.set_tile(map, "ground", IVec2::new(39, 3), None));
    // A navigation grid taken in the same update runs the collision sync
    // ahead of the step, and reads the edits
    let window = CellRect::from_size(UVec2::new(40, 4));
    let Some(grid) = view.navigation(map, "ground", window) else {
      panic!("the ground is a tile layer")
    };
    assert!(!grid.is_walkable(GridCoord::new(10, 1)));
    assert!(grid.is_walkable(GridCoord::new(39, 3)));
    let tiles = colliders(&maps.world, map);
    assert_eq!(tiles.get(IVec2::new(10, 1)), TileCell::Solid);
    assert_eq!(tiles.get(IVec2::new(39, 3)), TileCell::Empty);

    // The step's own sync finds nothing more, and physics builds the
    // changes in that step, once: the new block is registered, the floor
    // took its new shape in place and the island was not touched, so
    // nothing was built whole
    maps.step();
    let parts = parts_of(&mut maps.world, map);
    let block = part_in(&parts, Vec2::new(160.0, 16.0), Vec2::new(176.0, 32.0));
    let physics = maps.world.resource::<Physics>();
    assert!(physics.collider_entries.contains_key(&block));
    assert_eq!(part_in(&parts, floor_area.0, floor_area.1), floor);
    assert_eq!(part_in(&parts, island_area.0, island_area.1), island);
    let right = parts
      .iter()
      .find(|(e, _)| *e == floor)
      .map(|(_, c)| match &c.shape {
        ColliderShape::Polyline { points } | ColliderShape::Segments { points, .. } => {
          points.iter().map(|p| p.x).fold(0.0, f32::max)
        }
        _ => 0.0,
      });
    assert_eq!(right, Some(39.0 * 16.0), "the floor lost its last cell");
  }

  #[test]
  fn an_edit_that_changes_no_cell_kind_changes_no_cell() {
    let mut maps = MapWorld::new();
    let map = floor_and_island(&mut maps);
    let before = colliders(&maps.world, map);
    // The same block mirrored, where one was: still a solid cell
    assert!(MapsView::new(&mut maps.world).set_tile(
      map,
      "ground",
      IVec2::new(5, 3),
      Some(Tile::new(0, SOLID).with_flip(TileFlip::HORIZONTAL))
    ));
    maps.step();
    assert_eq!(colliders(&maps.world, map), before);
  }

  #[test]
  fn the_strongest_kind_wins_a_cell_and_the_upper_layer_a_tie() {
    let at = |x: i32| IVec2::new(x, 0);
    let flipped = Tile::new(0, HALF).with_flip(TileFlip::VERTICAL);
    let mut map = code_map(
      UVec2::new(5, 1),
      &[
        (
          "below",
          &[
            (at(0), SLOPE),
            (at(1), SOLID),
            (at(2), PLATFORM),
            (at(3), HALF),
          ],
        ),
        (
          "above",
          &[
            (at(0), HALF),
            (at(1), SLOPE),
            (at(2), SLOPE),
            (at(4), PLATFORM),
          ],
        ),
      ],
    );
    map.tiles_mut(1).map(|g| g.set(at(3), Some(flipped)));
    let mut maps = MapWorld::new();
    let handle = add(&mut maps.world, map);
    let map = maps.spawn(MapRoot::new(handle));
    let tiles = colliders(&maps.world, map);
    let shape_of = |cell: TileCell| match cell {
      TileCell::Shape(index) => tiles.shape(index).map(|s| s.polygons[0].clone()),
      _ => None,
    };
    // A shape over a slope, a solid cell over a slope, a slope over a
    // one-way cell, a one-way cell alone
    let bottom_half = [(0.0, 0.5), (1.0, 0.5), (1.0, 1.0), (0.0, 1.0)];
    let top_half = [(0.0, 0.0), (1.0, 0.0), (1.0, 0.5), (0.0, 0.5)];
    assert!(shape_of(tiles.get(IVec2::new(0, 0))).is_some_and(|p| same_points(&p, &bottom_half)));
    assert_eq!(tiles.get(IVec2::new(1, 0)), TileCell::Solid);
    assert_eq!(
      tiles.get(IVec2::new(2, 0)),
      TileCell::Slope(SlopeCorner::BottomRight)
    );
    assert_eq!(tiles.get(IVec2::new(4, 0)), TileCell::OneWay);
    // Of two shapes, the upper layer's
    assert!(shape_of(tiles.get(IVec2::new(3, 0))).is_some_and(|p| same_points(&p, &top_half)));
    // and an edit to the layer below changes nothing the one above covers
    assert!(MapsView::new(&mut maps.world).set_tile(map, "below", at(3), None));
    maps.step();
    assert!(
      shape_of(colliders(&maps.world, map).get(IVec2::new(3, 0)))
        .is_some_and(|p| same_points(&p, &top_half))
    );
  }

  #[test]
  fn layers_of_another_cell_size_or_offset_collide_on_their_own_entity() {
    let mut map = code_map(
      UVec2::new(4, 2),
      &[
        ("art", &[(IVec2::new(0, 1), SOLID)]),
        ("more art", &[(IVec2::new(1, 1), SOLID)]),
        ("shifted", &[(IVec2::new(3, 0), SOLID)]),
      ],
    );
    map.layers[2].info.offset = Vec2::new(4.0, 0.0);
    // An 8 px IntGrid under the 16 px art, as LDtk levels mix them
    let fine = map.add_tile_layer("fine");
    map.layers[fine].kind = LayerKind::Tiles {
      tiles: crate::core::tilemap::TileGrid::new(UVec2::new(8, 4)),
      intgrid: Some(IntGrid {
        size: UVec2::new(8, 4),
        values: (0..32).map(|i| i32::from(i == 31)).collect(),
        names: [(1, "solid".to_string())].into_iter().collect(),
      }),
      grid: MapGrid::new(GridShape::Orthogonal, UVec2::splat(8)),
    };
    let mut maps = MapWorld::new();
    let handle = add(&mut maps.world, map);
    let map = maps.spawn(MapRoot::new(handle));
    let layers = layers_of(&mut maps.world, map);
    // The two layers sharing the map's cells and no offset are one grid
    let shared = colliders(&maps.world, map);
    assert_eq!(shared.bounds(), Some(CellRect::from_size(UVec2::new(4, 2))));
    assert_eq!(shared.get(IVec2::new(0, 1)), TileCell::Solid);
    assert_eq!(shared.get(IVec2::new(1, 1)), TileCell::Solid);
    assert_eq!(shared.get(IVec2::new(3, 0)), TileCell::Empty);
    for layer in &layers[..2] {
      assert!(maps.world.get::<TileColliders>(*layer).is_none());
    }
    let shifted = colliders(&maps.world, layers[2]);
    assert_eq!(shifted.get(IVec2::new(3, 0)), TileCell::Solid);
    assert_eq!(shifted.tile_size, Vec2::splat(16.0));
    let fine = colliders(&maps.world, layers[3]);
    assert_eq!(
      (fine.bounds(), fine.tile_size),
      (
        Some(CellRect::from_size(UVec2::new(8, 4))),
        Vec2::splat(8.0)
      )
    );
    assert_eq!(fine.get(IVec2::new(7, 3)), TileCell::Solid);
    assert_eq!(fine.get(IVec2::new(0, 0)), TileCell::Empty);
    // Each built its parts
    assert_eq!(parts_of(&mut maps.world, layers[2]).len(), 1);
    assert_eq!(parts_of(&mut maps.world, layers[3]).len(), 1);
  }

  #[test]
  fn a_disabled_map_collides_with_nothing_until_enabled() {
    let mut map = code_map(
      UVec2::new(4, 2),
      &[
        ("ground", &[(IVec2::new(0, 1), SOLID)]),
        ("shifted", &[(IVec2::new(3, 0), SOLID)]),
      ],
    );
    map.layers[1].info.offset = Vec2::new(0.0, 4.0);
    let mut maps = MapWorld::new();
    let handle = add(&mut maps.world, map);
    let map = maps.spawn(MapRoot::new(handle));
    maps.frame();
    assert_eq!(count_parts(&mut maps.world), 2);
    maps.world.entity_mut(map).insert(Disabled);
    maps.frame();
    assert_eq!(
      count_parts(&mut maps.world),
      0,
      "a disabled map's layers too"
    );
    // An edit while disabled is there when it comes back
    assert!(MapsView::new(&mut maps.world).set_tile(
      map,
      "shifted",
      IVec2::new(0, 0),
      Some(Tile::new(0, SOLID))
    ));
    maps.frame();
    assert_eq!(count_parts(&mut maps.world), 0);
    maps.world.entity_mut(map).remove::<Disabled>();
    maps.frame();
    assert_eq!(count_parts(&mut maps.world), 2);
    let shifted = layers_of(&mut maps.world, map)[1];
    assert_eq!(
      colliders(&maps.world, shifted).get(IVec2::new(0, 0)),
      TileCell::Solid
    );
    // The shifted layer's two islands, in one chunk, are one collider
    let parts = parts_of(&mut maps.world, shifted);
    assert!(
      matches!(&parts[..], [(_, c)] if matches!(&c.shape, ColliderShape::Segments { segments, .. } if segments.len() == 8)),
      "{parts:?}"
    );
  }

  #[test]
  fn a_colliding_layer_wider_than_8192_cells_collides() {
    // 6a capped a group at 8,192 cells a side; the colliders hold the
    // chunks their layers have, so a long level collides end to end
    let far = IVec2::new(9999, 0);
    let mut map = code_map(
      UVec2::new(10_000, 1),
      &[("long-7c1e", &[(IVec2::ZERO, SOLID), (far, SOLID)])],
    );
    // and a layer offset onto its own grid collides beside it
    let small = map.add_tile_layer("small");
    map.layers[small].kind = LayerKind::Tiles {
      tiles: {
        let mut grid = crate::core::tilemap::TileGrid::new(UVec2::new(2, 1));
        grid.set(IVec2::ZERO, Some(Tile::new(0, SOLID)));
        grid
      },
      intgrid: None,
      grid: MapGrid::new(GridShape::Orthogonal, UVec2::splat(16)),
    };
    map.layers[small].info.offset = Vec2::new(0.0, 32.0);
    let mut maps = MapWorld::new();
    let handle = add(&mut maps.world, map);
    let map = maps.spawn(MapRoot::new(handle));
    maps.frame();
    let tiles = colliders(&maps.world, map);
    assert_eq!(
      tiles.bounds(),
      Some(CellRect::from_size(UVec2::new(10_000, 1)))
    );
    assert_eq!(tiles.get(IVec2::ZERO), TileCell::Solid);
    assert_eq!(tiles.get(far), TileCell::Solid);
    let warned = crate::core::dev_log::get_entries()
      .iter()
      .filter(|e| e.message.contains("long-7c1e"))
      .count();
    assert_eq!(warned, 0, "no warning names the layer");
    // A part at the far end, at its chunk's top-left
    let parts = parts_of(&mut maps.world, map);
    part_in(
      &parts,
      Vec2::new(9999.0 * 16.0, 0.0),
      Vec2::new(10_000.0 * 16.0, 16.0),
    );
    let small = layers_of(&mut maps.world, map)[1];
    assert_eq!(
      colliders(&maps.world, small).get(IVec2::new(0, 0)),
      TileCell::Solid
    );
  }

  #[test]
  fn a_colliding_layer_with_parallax_collides_where_it_is_and_warns_once() {
    let mut map = code_map(
      UVec2::new(4, 1),
      &[
        ("far-hills-91b3", &[(IVec2::ZERO, SOLID)]),
        ("far-clouds-91b3", &[(IVec2::new(2, 0), SOLID)]),
      ],
    );
    map.layers[0].info.parallax = Vec2::new(0.5, 1.0);
    // A layer that does not collide may move as it likes
    map.layers[1].info.parallax = Vec2::new(0.25, 0.25);
    map.layers[1].info.collides = false;
    let mut maps = MapWorld::new();
    let handle = add(&mut maps.world, map);
    let map = maps.spawn(MapRoot::new(handle));
    maps.frame();
    maps.frame();
    // It collides at its cells: collision ignores parallax
    let tiles = colliders(&maps.world, map);
    assert_eq!(tiles.get(IVec2::new(0, 0)), TileCell::Solid);
    assert_eq!(tiles.get(IVec2::new(2, 0)), TileCell::Empty);
    let warned = |name: &str| {
      crate::core::dev_log::get_entries()
        .iter()
        .filter(|e| e.message.contains(name) && e.message.contains("parallax"))
        .count()
    };
    assert_eq!(warned("far-hills-91b3"), 1, "warned once, naming the layer");
    assert_eq!(
      warned("far-clouds-91b3"),
      0,
      "a layer that does not collide"
    );
  }

  #[test]
  fn map_properties_set_the_material_and_a_wrong_type_warns_once() {
    let mut map = code_map(UVec2::new(2, 1), &[("ground", &[(IVec2::ZERO, SOLID)])]);
    map
      .properties
      .insert("collision_layer", PropertyValue::Int(4));
    map
      .properties
      .insert("collision_mask", PropertyValue::Int(-1));
    map.properties.insert("friction", PropertyValue::Int(1));
    map.properties.insert(
      "restitution",
      PropertyValue::String("bouncy-3f9a".to_string()),
    );
    let mut maps = MapWorld::new();
    let handle = add(&mut maps.world, map);
    let map = maps.spawn(MapRoot::new(handle));
    maps.frame();
    maps.frame();
    let tiles = colliders(&maps.world, map);
    assert_eq!((tiles.layer, tiles.mask), (4, u32::MAX));
    assert_eq!((tiles.friction, tiles.restitution), (1.0, 0.0));
    let warned = crate::core::dev_log::get_entries()
      .iter()
      .filter(|e| e.message.contains("restitution") && e.message.contains("bouncy-3f9a"))
      .count();
    assert_eq!(warned, 1, "a wrong type warns once");
    let parts = parts_of(&mut maps.world, map);
    assert!(parts.iter().all(|(_, c)| c.layer == 4 && c.friction == 1.0));
  }

  #[test]
  fn a_layer_s_own_material_overrides_the_map_s() {
    let mut map = code_map(
      UVec2::new(4, 1),
      &[
        ("ground", &[(IVec2::new(0, 0), SOLID)]),
        ("ice", &[(IVec2::new(1, 0), SOLID)]),
        ("same", &[(IVec2::new(2, 0), SOLID)]),
      ],
    );
    map
      .properties
      .insert("friction", PropertyValue::Float(0.25));
    map
      .properties
      .insert("collision_layer", PropertyValue::Int(2));
    // Ice is slippery and on its own collision layer; "same" says what the
    // map says
    let ice = &mut map.layers[1].info.properties;
    ice.insert("friction", PropertyValue::Float(0.0));
    ice.insert("collision_layer", PropertyValue::Int(8));
    map.layers[2]
      .info
      .properties
      .insert("friction", PropertyValue::Float(0.25));
    let mut maps = MapWorld::new();
    let handle = add(&mut maps.world, map);
    let map = maps.spawn(MapRoot::new(handle));
    let layers = layers_of(&mut maps.world, map);
    let shared = colliders(&maps.world, map);
    assert_eq!((shared.friction, shared.layer), (0.25, 2));
    assert_eq!(shared.get(IVec2::new(0, 0)), TileCell::Solid);
    assert_eq!(
      shared.get(IVec2::new(2, 0)),
      TileCell::Solid,
      "a layer saying what the map says"
    );
    assert_eq!(shared.get(IVec2::new(1, 0)), TileCell::Empty);
    let ice = colliders(&maps.world, layers[1]);
    assert_eq!((ice.friction, ice.layer, ice.mask), (0.0, 8, u32::MAX));
    assert_eq!(ice.get(IVec2::new(1, 0)), TileCell::Solid);
    assert_eq!(ice.get(IVec2::new(0, 0)), TileCell::Empty);
    assert!(maps.world.get::<TileColliders>(layers[2]).is_none());
    let parts = parts_of(&mut maps.world, layers[1]);
    assert!(!parts.is_empty() && parts.iter().all(|(_, c)| c.friction == 0.0 && c.layer == 8));
  }

  #[test]
  fn a_swapped_grid_collides_as_its_tiles_say() {
    let mut maps = MapWorld::new();
    let map = floor_and_island(&mut maps);
    let ground = layers_of(&mut maps.world, map)[0];
    // A new grid of the same size, filled alike but for one cell: its
    // versions are new, so every chunk is read again
    let mut grid = crate::core::tilemap::TileGrid::new(UVec2::new(40, 4));
    grid.fill(
      IVec2::new(0, 3),
      IVec2::new(39, 3),
      Some(Tile::new(0, SOLID)),
    );
    grid.set(IVec2::new(7, 1), Some(Tile::new(0, SOLID)));
    if let Some(mut data) = maps
      .world
      .get_mut::<crate::core::tilemap::TileLayerData>(ground)
    {
      data.tiles = grid;
    }
    maps.step();
    let tiles = colliders(&maps.world, map);
    assert_eq!(tiles.get(IVec2::new(7, 1)), TileCell::Solid);
    assert_eq!(
      tiles.get(IVec2::new(36, 0)),
      TileCell::Empty,
      "the island went with the old grid"
    );
    assert_eq!(tiles.get(IVec2::new(39, 3)), TileCell::Solid);
  }

  #[test]
  fn a_grid_taken_away_is_built_again() {
    let mut maps = MapWorld::new();
    let map = floor_and_island(&mut maps);
    maps.world.entity_mut(map).remove::<TileColliders>();
    maps.step();
    maps.step();
    let tiles = colliders(&maps.world, map);
    assert_eq!(tiles.get(IVec2::new(36, 0)), TileCell::Solid);
    assert_eq!(parts_of(&mut maps.world, map).len(), 2);
  }

  #[test]
  fn a_tileset_s_new_content_builds_every_group_again() {
    let mut maps = MapWorld::new();
    let set = maps.world.resource_mut::<Assets<Tileset>>().add(shapes());
    let mut map = TileMap::new(UVec2::new(4, 1), UVec2::splat(16));
    map.add_tileset(TilesetSource::Asset(set.clone()));
    let layer = map.add_tile_layer("ground");
    map
      .tiles_mut(layer)
      .map(|g| g.fill(IVec2::ZERO, IVec2::new(3, 0), Some(Tile::new(0, SOLID))));
    let handle = add(&mut maps.world, map);
    let map = maps.spawn(MapRoot::new(handle));
    assert_eq!(
      colliders(&maps.world, map).get(IVec2::new(0, 0)),
      TileCell::Solid
    );
    // The solid block becomes a half block: every cell holding it changes
    if let Some(tileset) = maps.world.resource_mut::<Assets<Tileset>>().get_mut(&set) {
      tileset.insert_tile(
        SOLID,
        TileData {
          shapes: vec![shape(rect(0.0, 8.0, 16.0, 8.0), false)],
          ..TileData::default()
        },
      );
    }
    maps.step();
    let tiles = colliders(&maps.world, map);
    assert!((0..4).all(|x| matches!(tiles.get(IVec2::new(x, 0)), TileCell::Shape(_))));
  }

  /// The map's parts with the chunks their loops start in (a part's
  /// outline loops or one-way runs, by their first point, 32 cells a chunk)
  fn parts_by_chunk(world: &mut World, map: Entity) -> Vec<(Entity, Vec<IVec2>, bool)> {
    parts_of(world, map)
      .into_iter()
      .map(|(part, collider)| {
        let chunk = |p: Vec2| (p / (16.0 * 32.0)).floor().as_ivec2();
        let mut chunks: Vec<IVec2> = match &collider.shape {
          ColliderShape::Polyline { points } => {
            points.first().map(|&p| chunk(p)).into_iter().collect()
          }
          ColliderShape::Segments { points, segments } => {
            let mut last = None;
            let mut starts = Vec::new();
            for &[a, b] in segments {
              if last != Some(a) {
                starts.push(chunk(points[a as usize]));
              }
              last = Some(b);
            }
            starts
          }
          ColliderShape::Segment { a, .. } => vec![chunk(*a)],
          _ => Vec::new(),
        };
        chunks.sort_by_key(|c| (c.y, c.x));
        chunks.dedup();
        (part, chunks, collider.one_way.is_some())
      })
      .collect()
  }

  #[test]
  fn a_map_registers_one_collider_per_chunk_not_per_island() {
    let mut maps = MapWorld::new();
    let handle = add(&mut maps.world, landing_map(true, true));
    let map = maps.spawn(MapRoot::new(handle));
    let parts = parts_by_chunk(&mut maps.world, map);
    // 1,024 islands in 64 chunks: a loop collider a chunk, each holding its
    // chunk's islands only
    let loops: Vec<&(Entity, Vec<IVec2>, bool)> = parts.iter().filter(|p| !p.2).collect();
    assert_eq!(loops.len(), 64, "one loop collider per chunk");
    let mut chunks: Vec<IVec2> = Vec::new();
    for (part, held, _) in &loops {
      assert_eq!(held.len(), 1, "{part:?} holds loops of chunks {held:?}");
      chunks.push(held[0]);
    }
    chunks.sort_by_key(|c| (c.y, c.x));
    chunks.dedup();
    assert_eq!(chunks.len(), 64, "a collider for each chunk");
    assert!(parts.iter().filter(|p| p.2).count() <= 64);
  }

  #[test]
  fn an_edit_re_registers_only_its_chunk() {
    use crate::core::ecs::DetectChanges;
    use crate::core::physics::Physics;
    let mut maps = MapWorld::new();
    let handle = add(&mut maps.world, landing_map(true, true));
    let map = maps.spawn(MapRoot::new(handle));
    // Each part: its chunks, its physics handle, when its collider last
    // changed and its shape
    type Registered = (Entity, Vec<IVec2>, String, String, Option<ColliderShape>);
    let registered = |world: &mut World| -> Vec<Registered> {
      let parts = parts_by_chunk(world, map);
      parts
        .into_iter()
        .map(|(part, chunks, _)| {
          let physics = world.resource::<Physics>();
          let handle = format!(
            "{:?}",
            physics.collider_entries.get(&part).map(|e| e.handle)
          );
          let collider = world.entity(part).get_ref::<Collider>();
          let changed = format!("{:?}", collider.as_ref().map(|c| c.last_changed()));
          let shape = collider.map(|c| c.shape.clone());
          (part, chunks, handle, changed, shape)
        })
        .collect()
    };
    let before = registered(&mut maps.world);
    assert!(before.len() <= 64, "{} colliders", before.len());
    // An island of chunk (5, 5) grows
    assert!(MapsView::new(&mut maps.world).set_tile(
      map,
      "ground",
      IVec2::new(5 * 32 + 5, 5 * 32 + 3),
      Some(Tile::new(0, SOLID))
    ));
    maps.step();
    let after = registered(&mut maps.world);
    let edited = IVec2::splat(5);
    for (part, chunks, handle, changed, shape) in &before {
      let now = after.iter().find(|(p, ..)| p == part);
      let Some((_, now_chunks, now_handle, now_changed, now_shape)) = now else {
        panic!("chunk {chunks:?}'s collider went");
      };
      assert!(handle.starts_with("Some"), "{handle}");
      assert_eq!(
        (now_chunks, now_handle),
        (chunks, handle),
        "it kept its handle"
      );
      if chunks.contains(&edited) {
        assert_ne!(
          now_shape, shape,
          "the edited chunk's collider took the new shape"
        );
      } else {
        assert_eq!(
          (now_changed, now_shape),
          (changed, shape),
          "chunk {chunks:?}'s collider was not touched"
        );
      }
    }
    assert_eq!(after.len(), before.len());
  }

  #[test]
  fn a_map_whose_root_goes_keeps_no_colliders() {
    let mut maps = MapWorld::new();
    let map = floor_and_island(&mut maps);
    assert!(count_parts(&mut maps.world) > 0);
    maps.world.entity_mut(map).remove::<MapRoot>();
    maps.frame();
    maps.frame();
    assert!(maps.world.get::<TileColliders>(map).is_none());
    assert_eq!(count_parts(&mut maps.world), 0);
  }

  /// Spawns the fixture level as it enters
  struct Level {
    map: Arc<Mutex<Option<Entity>>>,
  }

  impl Scene for Level {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
      let level = ctx
        .assets()
        .load_now::<TileMap>("tiled/maps/level.tmj")
        .expect("level.tmj loads");
      let map = ctx.world.spawn(MapRoot::new(level)).id();
      if let Ok(mut held) = self.map.lock() {
        *held = Some(map);
      }
    }
  }

  #[test]
  fn a_character_runs_and_lands_on_a_loaded_tiled_level() {
    let map = Arc::new(Mutex::new(None));
    let Some(mut engine) = HeadlessEngine::new_or_skip(
      EngineBuilder::new()
        .resolution(160, 120)
        .asset_source(files_with(&[]))
        .start_scene(Level { map: map.clone() }),
    ) else {
      return;
    };
    let map = map.lock().ok().and_then(|m| *m).expect("the level spawned");
    // It lands whole a frame or so in: its landing is worked out off the
    // main thread
    crate::core::tilemap::test_util::step_until_maps_land(&mut engine);
    assert!(engine.world_mut().get::<TileColliders>(map).is_some());
    let player = engine
      .world_mut()
      .spawn((
        Transform::from_xy(40.0, 120.0),
        Collider::rect(12.0, 24.0),
        PlatformerMovement::default(),
      ))
      .id();
    let state = |engine: &mut HeadlessEngine| {
      let world = engine.world_mut();
      let at = world.get::<Transform>(player).map(|t| t.translation);
      let state = world.get::<CharacterState>(player).cloned();
      (at.unwrap_or(Vec2::NAN), state.unwrap_or_default())
    };
    // It falls onto the floor, whose top is at y = 176
    let mut landed = false;
    for _ in 0..90 {
      engine.step().expect("a frame");
      if state(&mut engine).1.on_floor {
        landed = true;
        break;
      }
    }
    let (at, _) = state(&mut engine);
    assert!(landed, "it never landed: at {at}");
    assert!((at.y - 164.0).abs() < 1.0, "standing at {at}");
    // It runs right, over the seam where the floor passes from one layer to
    // the next (x = 192) and through the decor layer's block (x 224 to 240),
    // which does not collide, until the half block's side (x = 288), which
    // it cannot step up
    let mut stopped = None;
    for step in 0..120 {
      if let Some(mut intent) = engine.world_mut().get_mut::<MoveIntent>(player) {
        intent.direction = Vec2::X;
      }
      engine.step().expect("a frame");
      let (at, state) = state(&mut engine);
      assert!(state.on_floor, "step {step}: at {at}, {state:?}");
      if state.on_wall {
        stopped = Some((at, state));
        break;
      }
    }
    let Some((at, state)) = stopped else {
      panic!("it ran on to {}", state(&mut engine).0)
    };
    assert!((at.x - 282.0).abs() < 0.5, "stopped at {at}, {state:?}");
    assert!((state.wall_normal - Vec2::NEG_X).length() < 1e-3);
  }

  /// Times one tile's edit on a 1024 x 1024 map of four colliding layers:
  /// islands of solid tiles on one, and every cell of the others holding a
  /// tile with no shape (the classification's worst case). Run it in
  /// release: `cargo test --release -p ryo2d --lib one_tile_edit_sync_timing
  /// -- --ignored --nocapture`
  #[test]
  #[ignore = "timing; run in release by hand"]
  fn one_tile_edit_sync_timing() {
    use std::time::{Duration, Instant};

    use crate::core::ecs::Schedule;

    const SIDE: u32 = 1024;
    let mut map = code_map(UVec2::splat(SIDE), &[]);
    for name in ["ground", "decor 1", "decor 2", "decor 3"] {
      let layer = map.add_tile_layer(name);
      let grid = map.tiles_mut(layer).expect("a tile layer");
      if layer == 0 {
        for y in (0..SIDE as i32).step_by(8) {
          for x in (0..SIDE as i32).step_by(8) {
            grid.fill(
              IVec2::new(x + 2, y + 2),
              IVec2::new(x + 4, y + 4),
              Some(Tile::new(0, SOLID)),
            );
          }
        }
      } else {
        grid.fill(
          IVec2::ZERO,
          IVec2::splat(SIDE as i32 - 1),
          Some(Tile::new(0, 9)),
        );
      }
    }
    let mut maps = MapWorld::new();
    let handle = add(&mut maps.world, map);
    // The map's landing is worked out off the main thread (the landing
    // frames are `spawning_a_256_map_adds_under_half_a_millisecond_to_any_frame`'s)
    let start = Instant::now();
    let map = maps.spawn(MapRoot::new(handle));
    println!("landed in {:?}", start.elapsed());
    // Edits are timed in a schedule of their own, the map built
    let mut sync = Schedule::default();
    sync.add_systems(super::sync_map_colliders);
    sync.run(&mut maps.world);
    let median = |mut times: Vec<Duration>| {
      times.sort();
      (times[times.len() / 2], times[times.len() - 1])
    };
    let (mut colliders, mut steps, mut occluders) = (Vec::new(), Vec::new(), Vec::new());
    for i in 0..400 {
      let tile = (i % 2 == 0).then_some(Tile::new(0, SOLID));
      MapsView::new(&mut maps.world).set_tile(map, "ground", IVec2::new(517, 517), tile);
      let start = Instant::now();
      sync.run(&mut maps.world);
      colliders.push(start.elapsed());
      let start = Instant::now();
      maps.step();
      steps.push(start.elapsed());
      let start = Instant::now();
      maps.post_update();
      occluders.push(start.elapsed());
    }
    let mut idle = Vec::new();
    for _ in 0..400 {
      let start = Instant::now();
      sync.run(&mut maps.world);
      idle.push(start.elapsed());
    }
    let mut idle_steps = Vec::new();
    for _ in 0..400 {
      let start = Instant::now();
      maps.step();
      idle_steps.push(start.elapsed());
    }
    let mut idle_occluders = Vec::new();
    for _ in 0..400 {
      let start = Instant::now();
      maps.post_update();
      idle_occluders.push(start.elapsed());
    }
    println!(
      "an edit's collision sync (median, max): {:?}",
      median(colliders)
    );
    println!(
      "an edit's fixed step, its chunk's collider reshaped: {:?}",
      median(steps)
    );
    println!("an idle fixed step: {:?}", median(idle_steps));
    println!(
      "an edit's occluder sync, commands applied: {:?}",
      median(occluders)
    );
    println!("an idle step's collision sync: {:?}", median(idle));
    println!(
      "an idle frame's occluder sync: {:?}",
      median(idle_occluders)
    );
  }

  /// Marks a box a test dropped on a map, and how
  #[derive(crate::core::ecs::Component, Clone, Copy, Debug, PartialEq)]
  enum Dropped {
    /// By an observer of the map's objects, as they arrived
    ByObserver,
    /// By a system that read `MapEvent::Spawned`
    OnSpawned,
  }

  /// A box over the map at `x`, its bottom 6 px above the floor's top
  fn dropped(x: f32, how: Dropped) -> impl crate::core::ecs::Bundle {
    (
      Transform::from_xy(x, 150.0),
      crate::core::physics::RigidBody::dynamic(),
      Collider::rect(8.0, 8.0),
      how,
    )
  }

  /// Drops a box when the map is reported spawned, as game code would in
  /// its fixed step
  fn drop_on_spawned(
    mut events: crate::core::ecs::MessageReader<crate::core::tilemap::MapEvent>,
    mut commands: crate::core::ecs::Commands,
  ) {
    for event in events.read() {
      if let crate::core::tilemap::MapEvent::Spawned { .. } = event {
        commands.spawn(dropped(240.0, Dropped::OnSpawned));
      }
    }
  }

  #[test]
  fn a_body_on_a_map_reported_ready_never_falls_through() {
    use crate::core::ecs::{Add, IntoScheduleConfigs, On, Query};
    use crate::core::physics::Physics;
    use crate::core::schedule::{FixedSet, Phase};
    use crate::core::tilemap::{MapEvent, MapObject, MapObjectData, ObjectShape};
    // A floor along row 10 (its top at y 160) and a spawn point over it
    let floor: Vec<(IVec2, u32)> = (0..20).map(|x| (IVec2::new(x, 10), SOLID)).collect();
    let mut map = code_map(UVec2::new(20, 12), &[("ground", &floor)]);
    let things = map.add_object_layer("things");
    if let LayerKind::Objects(objects) = &mut map.layers[things].kind {
      objects.push(MapObjectData {
        id: "1".to_string(),
        name: "spawn".to_string(),
        class: None,
        position: Vec2::new(80.0, 150.0),
        size: Vec2::ZERO,
        rotation: 0.0,
        shape: ObjectShape::Point,
        tile: None,
        visible: true,
        properties: crate::core::tilemap::Properties::default(),
      });
    }
    let mut maps = MapWorld::new();
    crate::core::messages::register_message::<MapEvent>(&mut maps.world);
    maps.world.add_observer(
      |add: On<Add<MapObject>>,
       objects: Query<(&MapObject, &Transform)>,
       mut commands: crate::core::ecs::Commands| {
        if let Ok((object, at)) = objects.get(add.entity)
          && object.name == "spawn"
        {
          commands.spawn(dropped(at.translation.x, Dropped::ByObserver));
        }
      },
    );
    maps
      .schedules()
      .add_systems(Phase::FixedUpdate, drop_on_spawned.in_set(FixedSet::Game));
    let handle = add(&mut maps.world, map);
    let map = maps.world.spawn(MapRoot::new(handle)).id();
    // After the frame each box first fell in, the floor was in physics too
    let start = std::time::Instant::now();
    let mut checked = Vec::new();
    while checked.len() < 2 {
      maps.frame();
      let boxes: Vec<(Entity, Dropped)> = maps
        .world
        .query::<(Entity, &Dropped)>()
        .iter(&maps.world)
        .map(|(e, d)| (e, *d))
        .collect();
      for (body, how) in boxes {
        if checked.contains(&how)
          || !maps
            .world
            .resource::<Physics>()
            .body_entries
            .contains_key(&body)
        {
          continue;
        }
        let floor = parts_of(&mut maps.world, map);
        let physics = maps.world.resource::<Physics>();
        assert!(
          !floor.is_empty()
            && floor
              .iter()
              .all(|(part, _)| physics.collider_entries.contains_key(part)),
          "{how:?}: the box fell its first step with no floor under it"
        );
        checked.push(how);
      }
      assert!(start.elapsed().as_secs() < 5, "the boxes never came");
      std::thread::yield_now();
    }
    for _ in 0..90 {
      maps.step();
    }
    let rest: Vec<(Dropped, f32)> = maps
      .world
      .query::<(&Dropped, &Transform)>()
      .iter(&maps.world)
      .map(|(how, at)| (*how, at.translation.y))
      .collect();
    assert_eq!(rest.len(), 2);
    for (how, y) in rest {
      assert!(
        (y - 156.0).abs() < 1.0,
        "{how:?}: the box is at y {y}, not on the floor"
      );
    }
  }

  struct Empty;

  impl Scene for Empty {}

  /// 256 x 256 cells of 16 px over `shapes`, four layers: `dense` puts 3 x 3
  /// solid islands every 8 cells on one and a tile with no shape in every
  /// cell of the others; otherwise a terrain (solid below a jagged line)
  /// with one full background layer and two empty ones. `collide` false
  /// turns its collision and shadows off, as a baseline
  fn landing_map(dense: bool, collide: bool) -> TileMap {
    const SIDE: i32 = 256;
    let mut map = code_map(UVec2::splat(SIDE as u32), &[]);
    for name in ["ground", "decor 1", "decor 2", "decor 3"] {
      let layer = map.add_tile_layer(name);
      let grid = map.tiles_mut(layer).expect("a tile layer");
      let solid = Some(Tile::new(0, SOLID));
      if layer == 0 {
        if dense {
          for y in (0..SIDE).step_by(8) {
            for x in (0..SIDE).step_by(8) {
              grid.fill(IVec2::new(x + 2, y + 2), IVec2::new(x + 4, y + 4), solid);
            }
          }
        } else {
          for x in 0..SIDE {
            let top = 160 + ((x * 7) % 13) + ((x / 9) % 5);
            grid.fill(IVec2::new(x, top), IVec2::new(x, SIDE - 1), solid);
          }
        }
      } else if dense || layer == 1 {
        grid.fill(IVec2::ZERO, IVec2::splat(SIDE - 1), Some(Tile::new(0, 9)));
      }
      if !collide {
        map.layers[layer].info.occludes = Some(false);
      }
    }
    map
  }

  /// Frames before the landing frame, and after it, that the timing reports
  const BEFORE: usize = 2;
  const AFTER: usize = 6;

  /// The time of each frame around the one a 256 x 256 map lands in (its
  /// landing is worked out off the main thread, so that frame comes a few
  /// after the spawn): `BEFORE` frames, the landing frame, `AFTER` frames;
  /// each the median of `rounds` spawns, or none for a frame before the
  /// spawn (a map that lands sooner)
  fn landing_frames(dense: bool, collide: bool, rounds: usize) -> Option<Vec<Option<f64>>> {
    use std::time::Instant;
    let builder = EngineBuilder::new().resolution(320, 180).start_scene(Empty);
    let mut engine = HeadlessEngine::new_or_skip(builder)?;
    engine.step_frames(10).expect("frames");
    let handle = engine
      .world_mut()
      .resource_mut::<Assets<TileMap>>()
      .add(landing_map(dense, collide));
    let mut times = vec![Vec::new(); BEFORE + 1 + AFTER];
    for _ in 0..rounds {
      let root = MapRoot::new(handle.clone());
      let root = if collide {
        root
      } else {
        root.without_collision()
      };
      let map = engine.world_mut().spawn(root).id();
      let mut timed = Vec::new();
      let start = Instant::now();
      while !MapsView::new(engine.world_mut()).is_ready(map) {
        let at = Instant::now();
        engine.step().expect("a frame");
        timed.push(at.elapsed().as_secs_f64() * 1000.0);
        assert!(start.elapsed().as_secs() < 5, "the map never landed");
      }
      for _ in 0..AFTER {
        let at = Instant::now();
        engine.step().expect("a frame");
        timed.push(at.elapsed().as_secs_f64() * 1000.0);
      }
      // The landing frame is the last stepped before it was ready
      let landed = timed.len() - AFTER - 1;
      for (k, time) in times.iter_mut().enumerate() {
        if let Some(t) = (landed + k).checked_sub(BEFORE).and_then(|i| timed.get(i)) {
          time.push(*t);
        }
      }
      engine.world_mut().despawn(map);
      engine.step_frames(3).expect("frames");
    }
    Some(
      times
        .into_iter()
        .map(|mut t| {
          t.sort_by(f64::total_cmp);
          t.get(t.len() / 2).copied()
        })
        .collect(),
    )
  }

  /// Prints a landed dense 256 x 256 map's steady frame and physics step,
  /// with collision and shadows and without, alone and with 256 bouncing
  /// balls dropped on it. Run it in release: `cargo test --release -p ryo2d
  /// --lib a_landed_dense_map_s_steady_frame -- --ignored --nocapture`
  #[test]
  #[ignore = "timing; run in release by hand"]
  fn a_landed_dense_map_s_steady_frame_timing() {
    use std::time::Instant;

    use crate::core::physics::{Physics, RigidBody};
    let measure = |collide: bool, boxes: bool| -> Option<(f64, f64)> {
      let builder = EngineBuilder::new().resolution(320, 180).start_scene(Empty);
      let mut engine = HeadlessEngine::new_or_skip(builder)?;
      engine.step_frames(10).expect("frames");
      let handle = engine
        .world_mut()
        .resource_mut::<Assets<TileMap>>()
        .add(landing_map(true, collide));
      let root = MapRoot::new(handle);
      let root = if collide {
        root
      } else {
        root.without_collision()
      };
      let map = engine.world_mut().spawn(root).id();
      while !MapsView::new(engine.world_mut()).is_ready(map) {
        engine.step().expect("a frame");
      }
      if boxes {
        for i in 0..256 {
          let at = Vec2::new(
            40.0 + (i % 16) as f32 * 256.0,
            20.0 + (i / 16) as f32 * 256.0,
          );
          engine.world_mut().spawn((
            Transform::from_xy(at.x, at.y),
            RigidBody::dynamic().never_sleep(),
            Collider::circle(5.0)
              .with_restitution(1.0)
              .with_friction(0.0),
          ));
        }
      }
      engine.step_frames(120).expect("frames");
      let (mut frames, mut steps) = (Vec::new(), Vec::new());
      for _ in 0..600 {
        let at = Instant::now();
        engine.step().expect("a frame");
        frames.push(at.elapsed().as_secs_f64() * 1000.0);
        let world = engine.world_mut();
        let at = Instant::now();
        world.resource_scope(|_, mut physics: crate::core::ecs::Mut<Physics>| {
          physics.step(1.0 / 60.0);
        });
        steps.push(at.elapsed().as_secs_f64() * 1000.0);
      }
      let median = |mut t: Vec<f64>| {
        t.sort_by(f64::total_cmp);
        t[t.len() / 2]
      };
      Some((median(frames), median(steps)))
    };
    for boxes in [false, true] {
      let (Some(on), Some(off)) = (measure(true, boxes), measure(false, boxes)) else {
        return;
      };
      let name = if boxes { "with 256 balls" } else { "alone" };
      println!(
        "dense map {name}: frame {:.3} vs {:.3} ms ({:+.3}), physics step {:.3} vs {:.3} ms ({:+.3})",
        on.0,
        off.0,
        on.0 - off.0,
        on.1,
        off.1,
        on.1 - off.1
      );
    }
  }

  /// Prints what collision and shadows add to each frame from the one a
  /// 256 x 256 four-layer map spawns in, against the same map with neither.
  /// Run it in release: `cargo test --release -p ryo2d --lib
  /// spawning_a_256_map -- --ignored --nocapture`
  #[test]
  #[ignore = "timing; run in release by hand"]
  fn spawning_a_256_map_adds_under_half_a_millisecond_to_any_frame() {
    // The frames a process runs first are slow (about twice as slow for
    // the first second or two here, collision or not): warmed up, the maps
    // with and without collision are timed alike
    let warming = std::time::Instant::now();
    while warming.elapsed().as_secs_f32() < 2.0 {
      if landing_frames(true, false, 7).is_none() {
        return;
      }
    }
    let mut worst: f64 = 0.0;
    for dense in [true, false] {
      let (Some(on), Some(off)) = (
        landing_frames(dense, true, 7),
        landing_frames(dense, false, 7),
      ) else {
        return;
      };
      let name = if dense { "dense islands" } else { "terrain" };
      println!("{name}: frame (0 lands), with collision and shadows, without, added (ms)");
      for (k, (with, without)) in on.iter().zip(&off).enumerate() {
        let frame = k as i64 - BEFORE as i64;
        match (with, without) {
          (Some(with), Some(without)) => {
            let added = with - without;
            worst = worst.max(added);
            println!("  {frame:>+3}: {with:.3} {without:.3} {added:+.3}");
          }
          (with, without) => {
            let show = |t: &Option<f64>| t.map_or("none".to_string(), |t| format!("{t:.3}"));
            println!("  {frame:>+3}: {} {}", show(with), show(without));
          }
        }
      }
    }
    println!("worst frame added {worst:.3} ms");
    assert!(worst < 0.5, "a frame took {worst:.3} ms more");
  }

  // ---- grids of a square lattice ------------------------------------------

  /// Tile ids of `lattice_set`
  const DIAMOND: u32 = 0;
  const ONE_WAY_DIAMOND: u32 = 1;
  const BOX: u32 = 2;
  const ONE_WAY_BOX: u32 = 3;

  /// Tiles the size of `cell`: a diamond filling its box (a whole isometric
  /// or staggered cell), the same diamond one-way, a rectangle filling it
  /// (a whole orthogonal or oblique cell) and that rectangle one-way
  fn lattice_set(cell: UVec2) -> Tileset {
    let (w, h) = (cell.x as f32, cell.y as f32);
    let mut set = Tileset::new("lattice", cell, TilesetImage::Collection);
    let diamond = vec![
      Vec2::new(w / 2.0, 0.0),
      Vec2::new(w, h / 2.0),
      Vec2::new(w / 2.0, h),
      Vec2::new(0.0, h / 2.0),
    ];
    let with = |points: Vec<Vec2>, one_way: bool| TileData {
      shapes: vec![TileShape {
        one_way,
        ..shape(points, false)
      }],
      ..TileData::default()
    };
    set.insert_tile(DIAMOND, with(diamond.clone(), false));
    set.insert_tile(ONE_WAY_DIAMOND, with(diamond, true));
    set.insert_tile(BOX, with(rect(0.0, 0.0, w, h), false));
    set.insert_tile(ONE_WAY_BOX, with(rect(0.0, 0.0, w, h), true));
    set
  }

  /// A map of `shape` and `cell`-sized cells over `lattice_set`: `size`
  /// cells, or infinite for none, with a tile layer of each name holding
  /// the tiles listed
  fn lattice_map(
    shape: GridShape,
    cell: UVec2,
    size: Option<UVec2>,
    layers: &[(&str, &[(IVec2, u32)])],
  ) -> TileMap {
    let mut map = match size {
      Some(size) => TileMap::new(size, cell),
      None => TileMap::infinite(cell),
    }
    .with_shape(shape);
    let set = map.add_tileset(TilesetSource::Embedded(Arc::new(lattice_set(cell))));
    for (name, tiles) in layers {
      let layer = map.add_tile_layer(name);
      let grid = map.tiles_mut(layer).expect("a tile layer");
      for &(cell, id) in *tiles {
        grid.set(cell, Some(Tile::new(set, id)));
      }
    }
    map
  }

  /// The edges of `collider`, each from a point to the next as it runs
  fn edges_of(collider: &Collider) -> Vec<(Vec2, Vec2)> {
    match &collider.shape {
      ColliderShape::Polyline { points } => points.windows(2).map(|w| (w[0], w[1])).collect(),
      ColliderShape::Segments { points, segments } => segments
        .iter()
        .map(|&[a, b]| (points[a as usize], points[b as usize]))
        .collect(),
      ColliderShape::Segment { a, b } => vec![(*a, *b)],
      _ => Vec::new(),
    }
  }

  /// An edge between two whole lattice points
  type LatticeEdge = ((i32, i32), (i32, i32));

  /// The parts' edges under `parent` taken back through `to_pixel` to the
  /// lattice, whose points are whole there, sorted
  fn lattice_edges(world: &mut World, parent: Entity, to_pixel: glam::Affine2) -> Vec<LatticeEdge> {
    let back = to_pixel.inverse();
    let whole = |p: Vec2| {
      let at = back.transform_point2(p);
      assert!(
        (at - at.round()).abs().max_element() < 1e-3,
        "{p} is lattice point {at}, not a whole one"
      );
      (at.x.round() as i32, at.y.round() as i32)
    };
    let mut edges: Vec<LatticeEdge> = parts_of(world, parent)
      .iter()
      .flat_map(|(_, collider)| edges_of(collider))
      .map(|(a, b)| (whole(a), whole(b)))
      .collect();
    edges.sort_unstable();
    edges
  }

  /// The edges of the lattice rectangle `min..max` (cells), clockwise on
  /// screen from its top-left, cut at every lattice point, sorted (as an
  /// outline of whole cells runs, but cut where its straight runs merge)
  fn rect_edges(min: (i32, i32), max: (i32, i32)) -> Vec<LatticeEdge> {
    let mut edges = Vec::new();
    for x in min.0..max.0 {
      edges.push(((x, min.1), (x + 1, min.1)));
      edges.push(((x + 1, max.1), (x, max.1)));
    }
    for y in min.1..max.1 {
      edges.push(((max.0, y), (max.0, y + 1)));
      edges.push(((min.0, y + 1), (min.0, y)));
    }
    edges.sort_unstable();
    edges
  }

  /// `edges` cut at every whole lattice point between their ends, sorted
  fn cut(edges: &[LatticeEdge]) -> Vec<LatticeEdge> {
    let mut out = Vec::new();
    for &(a, b) in edges {
      let (dx, dy) = (b.0 - a.0, b.1 - a.1);
      let n = dx.abs().max(dy.abs()).max(1);
      assert!(
        dx == 0 || dy == 0,
        "an outline edge along the lattice: {a:?} {b:?}"
      );
      for i in 0..n {
        out.push((
          (a.0 + dx * i / n, a.1 + dy * i / n),
          (a.0 + dx * (i + 1) / n, a.1 + dy * (i + 1) / n),
        ));
      }
    }
    out.sort_unstable();
    out
  }

  #[test]
  fn an_isometric_wall_collides_along_its_diamonds() {
    // A row of three whole diamonds on a 4 x 3 isometric map of 64 x 32
    // cells, whose cell (0, 0)'s top vertex is at (96, 0)
    let wall: Vec<(IVec2, u32)> = (0..3).map(|x| (IVec2::new(x, 1), DIAMOND)).collect();
    let map = lattice_map(
      GridShape::Isometric,
      UVec2::new(64, 32),
      Some(UVec2::new(4, 3)),
      &[("walls", &wall)],
    );
    let grid = map.grid();
    let to_pixel = grid.lattice().expect("an isometric lattice").to_pixel;
    let mut maps = MapWorld::new();
    let handle = add(&mut maps.world, map);
    let map = maps.spawn(MapRoot::new(handle));
    let tiles = colliders(&maps.world, map);
    assert_eq!(tiles.lattice(), Some(to_pixel));
    assert_eq!(tiles.bounds(), Some(CellRect::from_size(UVec2::new(4, 3))));
    for x in 0..4 {
      let kind = if x < 3 {
        TileCell::Solid
      } else {
        TileCell::Empty
      };
      assert_eq!(tiles.get(IVec2::new(x, 1)), kind, "cell ({x}, 1)");
    }
    // One closed polyline round the row: the square lattice's outline
    // taken through the grid, its part unturned at its chunk's corner
    let parts = parts_of(&mut maps.world, map);
    assert_eq!(parts.len(), 1, "{parts:?}");
    let ColliderShape::Polyline { points } = &parts[0].1.shape else {
      panic!("a polyline: {:?}", parts[0].1);
    };
    assert_eq!(points.first(), points.last(), "closed");
    let corners = [(0.0, 1.0), (3.0, 1.0), (3.0, 2.0), (0.0, 2.0)]
      .map(|(x, y)| to_pixel.transform_point2(Vec2::new(x, y)));
    let expected = corners.map(|p| (p.x, p.y));
    assert!(
      same_points(&points[1..], &expected),
      "{points:?} != {expected:?}"
    );
    // The diamonds' own corners: the left one's top-left vertex and the
    // right one's bottom-right
    assert_eq!(corners[0], grid.cell_outline(IVec2::new(0, 1)).points()[0]);
    assert_eq!(corners[2], grid.cell_outline(IVec2::new(2, 1)).points()[2]);
    let part = parts[0].0;
    let at = maps.world.get::<Transform>(part).copied();
    assert_eq!(
      at.map(|t| (t.rotation, t.scale)),
      Some((Transform::IDENTITY.rotation, Vec2::ONE))
    );
    assert_eq!(at.map(|t| t.translation), Some(to_pixel.translation));
  }

  #[test]
  fn a_staggered_map_collides_as_the_isometric_map_of_the_same_cells() {
    let cell = UVec2::new(64, 32);
    // Cells across the staggered map's chunk borders and at negative cells
    let cells = [
      (0, 0),
      (1, 0),
      (0, 1),
      (1, 1),
      (2, 3),
      (-1, -2),
      (31, 5),
      (32, 5),
      (32, 6),
      (5, 31),
      (5, 32),
      (-33, 40),
    ]
    .map(|(x, y)| IVec2::new(x, y));
    for (axis, index) in [
      (StaggerAxis::Y, StaggerIndex::Odd),
      (StaggerAxis::Y, StaggerIndex::Even),
      (StaggerAxis::X, StaggerIndex::Odd),
      (StaggerAxis::X, StaggerIndex::Even),
    ] {
      let shape = GridShape::Staggered { axis, index };
      let staggered: Vec<(IVec2, u32)> = cells.iter().map(|&c| (c, DIAMOND)).collect();
      let stagger_map = lattice_map(shape, cell, None, &[("walls", &staggered)]);
      let stagger_grid = stagger_map.grid();
      let lattice = stagger_grid.lattice().expect("a staggered lattice");
      let diamonds: Vec<(IVec2, u32)> = cells
        .iter()
        .map(|&c| (lattice.lattice_cell(c), DIAMOND))
        .collect();
      let iso_map = lattice_map(GridShape::Isometric, cell, None, &[("walls", &diamonds)]);
      let iso_lattice = iso_map.grid().lattice().expect("an isometric lattice");
      let mut maps = MapWorld::new();
      let a = add(&mut maps.world, stagger_map);
      let b = add(&mut maps.world, iso_map);
      let a = maps.spawn(MapRoot::new(a));
      let b = maps.spawn(MapRoot::new(b));
      let label = format!("{axis:?} {index:?}");
      let (sa, sb) = (colliders(&maps.world, a), colliders(&maps.world, b));
      assert_eq!(sa.lattice(), Some(lattice.to_pixel), "{label}");
      assert_eq!(sa.bounds(), None, "{label}: unbounded in the lattice");
      for &c in &cells {
        let at = lattice.lattice_cell(c);
        assert_eq!(sa.get(at), TileCell::Solid, "{label}: {c} at {at}");
        assert_eq!(sb.get(at), TileCell::Solid, "{label}: {at}");
      }
      assert_eq!(sa.extent(), sb.extent(), "{label}");
      // The same outlines in the lattice, each where its grid puts it
      let ea = lattice_edges(&mut maps.world, a, lattice.to_pixel);
      let eb = lattice_edges(&mut maps.world, b, iso_lattice.to_pixel);
      assert!(!ea.is_empty());
      assert_eq!(ea, eb, "{label}");
      // and through the staggered grid, on its diamonds' corners
      let first = stagger_grid.cell_outline(cells[0]);
      let mut seen = parts_of(&mut maps.world, a)
        .iter()
        .flat_map(|(_, c)| edges_of(c))
        .flat_map(|(p, q)| [p, q])
        .collect::<Vec<_>>();
      seen.retain(|p| first.points().contains(p));
      assert!(
        !seen.is_empty(),
        "{label}: an outline meets cell (0, 0)'s corners"
      );
    }
  }

  #[test]
  fn an_oblique_map_collides_skewed() {
    let wall: Vec<(IVec2, u32)> = (0..3).map(|x| (IVec2::new(x, 1), BOX)).collect();
    for skew in [IVec2::new(8, 12), IVec2::new(-10, 0), IVec2::new(0, -12)] {
      let map = lattice_map(
        GridShape::Oblique { skew },
        UVec2::splat(32),
        Some(UVec2::new(4, 3)),
        &[("walls", &wall)],
      );
      let grid = map.grid();
      let to_pixel = grid.lattice().expect("an oblique lattice").to_pixel;
      let mut maps = MapWorld::new();
      let handle = add(&mut maps.world, map);
      let map = maps.spawn(MapRoot::new(handle));
      let tiles = colliders(&maps.world, map);
      // The rectangle filling the unskewed cell is the whole cell, skewed
      assert!((0..3).all(|x| tiles.get(IVec2::new(x, 1)) == TileCell::Solid));
      let parts = parts_of(&mut maps.world, map);
      assert_eq!(parts.len(), 1, "{skew}: {parts:?}");
      let ColliderShape::Polyline { points } = &parts[0].1.shape else {
        panic!("a polyline: {:?}", parts[0].1);
      };
      // A parallelogram: the cells' corners, each row skewed along by
      // skew.x and each column down by skew.y
      let expected = [(0, 1), (3, 1), (3, 2), (0, 2)]
        .map(|(i, j)| ((32 * i + skew.x * j) as f32, (skew.y * i + 32 * j) as f32));
      assert!(same_points(&points[1..], &expected), "{skew}: {points:?}");
      // Running clockwise round the cells in the lattice, as on squares
      assert_eq!(
        cut(&lattice_edges(&mut maps.world, map, to_pixel)),
        rect_edges((0, 1), (3, 2)),
        "{skew}"
      );
    }
  }

  #[test]
  fn an_edit_on_an_isometric_map_retraces_only_its_loops() {
    // Diamonds in chunks 0, 1 and 2 of an infinite isometric map
    let islands = [(2, 2), (40, 2), (41, 3), (70, 2)].map(|(x, y)| (IVec2::new(x, y), DIAMOND));
    let map = lattice_map(
      GridShape::Isometric,
      UVec2::new(32, 16),
      None,
      &[("ground", &islands)],
    );
    let to_pixel = map.grid().lattice().expect("an isometric lattice").to_pixel;
    let mut maps = MapWorld::new();
    let handle = add(&mut maps.world, map);
    let map = maps.spawn(MapRoot::new(handle));
    // Each part by the lattice chunk its loops start in
    let by_chunk = |world: &mut World| -> Vec<(IVec2, Entity, Collider)> {
      let back = to_pixel.inverse();
      let mut found: Vec<(IVec2, Entity, Collider)> = parts_of(world, map)
        .into_iter()
        .map(|(part, collider)| {
          let first = edges_of(&collider).first().map_or(Vec2::ZERO, |e| e.0);
          let at = back.transform_point2(first).round().as_ivec2();
          (at.div_euclid(IVec2::splat(32)), part, collider)
        })
        .collect();
      found.sort_by_key(|(chunk, ..)| (chunk.y, chunk.x));
      found
    };
    let before = by_chunk(&mut maps.world);
    let chunks: Vec<IVec2> = before.iter().map(|(c, ..)| *c).collect();
    assert_eq!(chunks, [IVec2::ZERO, IVec2::new(1, 0), IVec2::new(2, 0)]);
    // A diamond joins the two in chunk 1 into one loop
    let mut view = MapsView::new(&mut maps.world);
    assert!(view.set_tile(
      map,
      "ground",
      IVec2::new(41, 2),
      Some(Tile::new(0, DIAMOND))
    ));
    maps.step();
    let after = by_chunk(&mut maps.world);
    assert_eq!(after.len(), 3);
    for ((chunk, part, collider), (_, was, old)) in after.iter().zip(&before) {
      assert_eq!(part, was, "chunk {chunk}'s part took its shape in place");
      if *chunk == IVec2::new(1, 0) {
        assert_ne!(collider, old, "chunk {chunk} was retraced");
        let ColliderShape::Polyline { points } = &collider.shape else {
          panic!("one loop: {collider:?}");
        };
        // The L of three diamonds: six corners and the closing point
        assert_eq!(points.len(), 7, "{points:?}");
      } else {
        assert_eq!(collider, old, "chunk {chunk} was left as it was");
      }
    }
  }

  #[test]
  fn a_one_way_tile_off_an_orthogonal_map_collides_both_ways_and_warns_once() {
    let cases = [
      (GridShape::Isometric, UVec2::new(64, 32), ONE_WAY_DIAMOND),
      (
        GridShape::Staggered {
          axis: StaggerAxis::Y,
          index: StaggerIndex::Odd,
        },
        UVec2::new(64, 32),
        ONE_WAY_DIAMOND,
      ),
      (
        GridShape::Oblique {
          skew: IVec2::new(8, 0),
        },
        UVec2::splat(32),
        ONE_WAY_BOX,
      ),
    ];
    // Only this test warns of one-way tiles; its maps may share an entity
    let warnings = || {
      crate::core::dev_log::get_entries()
        .iter()
        .filter(|e| e.message.contains("one-way tiles collide both ways"))
        .count()
    };
    for (shape, cell, platform) in cases {
      let before = warnings();
      let ledges = [(IVec2::new(1, 1), platform), (IVec2::new(2, 1), platform)];
      let map = lattice_map(shape, cell, Some(UVec2::new(6, 6)), &[("ledges", &ledges)]);
      let lattice = map.grid().lattice().expect("a square lattice");
      let mut maps = MapWorld::new();
      let handle = add(&mut maps.world, map);
      let map = maps.spawn(MapRoot::new(handle));
      maps.frame();
      let tiles = colliders(&maps.world, map);
      for (cell, _) in ledges {
        let at = lattice.lattice_cell(cell);
        assert_eq!(
          tiles.get(at),
          TileCell::Solid,
          "{shape:?}: {cell} collides both ways"
        );
      }
      // Outlines round them, no one-way part
      let parts = parts_of(&mut maps.world, map);
      assert!(!parts.is_empty(), "{shape:?}");
      assert!(parts.iter().all(|(_, c)| c.one_way.is_none()), "{shape:?}");
      // More one-way tiles, by an edit, warn no more
      let mut view = MapsView::new(&mut maps.world);
      assert!(view.set_tile(
        map,
        "ledges",
        IVec2::new(4, 4),
        Some(Tile::new(0, platform))
      ));
      maps.step();
      maps.frame();
      assert_eq!(
        colliders(&maps.world, map).get(lattice.lattice_cell(IVec2::new(4, 4))),
        TileCell::Solid
      );
      assert_eq!(warnings() - before, 1, "{shape:?}: warned once for the map");
    }
    // On an orthogonal map they stay one-way, and nothing is said
    let ledges = [(IVec2::new(1, 1), ONE_WAY_BOX)];
    let map = lattice_map(
      GridShape::Orthogonal,
      UVec2::splat(16),
      Some(UVec2::new(4, 4)),
      &[("ledges", &ledges)],
    );
    let mut maps = MapWorld::new();
    let handle = add(&mut maps.world, map);
    let before = warnings();
    let map = maps.spawn(MapRoot::new(handle));
    maps.frame();
    assert_eq!(
      colliders(&maps.world, map).get(IVec2::new(1, 1)),
      TileCell::OneWay
    );
    assert_eq!(warnings(), before);
  }

  // ---- hexagonal grids ------------------------------------------------------

  /// Tile ids of the hexagonal collision fixtures' tilesets: a tile whose
  /// shape is its whole hexagon, one whose shape is a small triangle off
  /// its middle, and one with no shape
  const WHOLE_HEX: u32 = 0;
  const TRIANGLE: u32 = 1;

  /// A hexagonal collision fixture (`make-fixtures.js`' `collideHex`): its
  /// files (JSON, then XML) and its XML tilesets', its clump of whole
  /// hexagons, its triangle tiles (each with its flips), its tiles with no
  /// shape, the triangle in the tile's pixels, and its second tileset's
  /// tile
  struct HexFixture {
    files: [&'static str; 2],
    xml_sets: [&'static str; 2],
    clump: [(i32, i32); 3],
    triangles: [((i32, i32), TileFlip); 3],
    blank: [(i32, i32); 10],
    triangle: [(f32, f32); 3],
    tall: TallTile,
  }

  /// The second tileset's one tile: not its cell's size, its tileset moved
  /// by an offset, its triangle in its pixels, and the cells it is placed
  /// in with their flips
  struct TallTile {
    size: Vec2,
    offset: Vec2,
    triangle: [(f32, f32); 3],
    cells: [((i32, i32), TileFlip); 2],
  }

  fn hex_fixtures() -> [HexFixture; 2] {
    let (turned, flipped) = (TileFlip::HEX_ROTATE_60, TileFlip::HORIZONTAL);
    let (both, third) = (flipped | turned, TileFlip::HEX_ROTATE_120);
    [
      HexFixture {
        files: [
          "tiled/maps/collide-hex-y-odd.tmj",
          "tiled/xml/maps/collide-hex-y-odd.tmx",
        ],
        xml_sets: [
          "tiled/xml/tilesets/collide-hex-y.tsx",
          "tiled/xml/tilesets/collide-hex-y-tall.tsx",
        ],
        clump: [(2, 1), (3, 1), (3, 2)],
        triangles: [
          ((6, 1), TileFlip::NONE),
          ((6, 3), turned),
          ((1, 4), flipped),
        ],
        blank: [
          (1, 1),
          (4, 1),
          (2, 2),
          (4, 2),
          (5, 1),
          (7, 1),
          (5, 3),
          (7, 3),
          (0, 4),
          (2, 4),
        ],
        triangle: [(72.0, 84.0), (96.0, 100.0), (70.0, 108.0)],
        tall: TallTile {
          size: Vec2::new(80.0, 180.0),
          offset: Vec2::new(6.0, -10.0),
          triangle: [(20.0, 30.0), (60.0, 50.0), (30.0, 90.0)],
          cells: [((4, 4), both), ((6, 5), third)],
        },
      },
      HexFixture {
        files: [
          "tiled/maps/collide-hex-x-even.tmj",
          "tiled/xml/maps/collide-hex-x-even.tmx",
        ],
        xml_sets: [
          "tiled/xml/tilesets/collide-hex-x.tsx",
          "tiled/xml/tilesets/collide-hex-x-tall.tsx",
        ],
        clump: [(1, 2), (2, 1), (2, 2)],
        triangles: [
          ((5, 1), TileFlip::NONE),
          ((5, 3), turned),
          ((2, 4), flipped),
        ],
        blank: [
          (0, 2),
          (1, 1),
          (3, 1),
          (3, 2),
          (4, 1),
          (6, 1),
          (4, 3),
          (6, 3),
          (1, 4),
          (3, 4),
        ],
        triangle: [(84.0, 72.0), (100.0, 96.0), (108.0, 70.0)],
        tall: TallTile {
          size: Vec2::new(180.0, 80.0),
          offset: Vec2::new(-10.0, 6.0),
          triangle: [(30.0, 20.0), (50.0, 60.0), (90.0, 30.0)],
          cells: [((4, 5), both), ((7, 4), third)],
        },
      },
    ]
  }

  /// A map world over the fixtures, with `fixture`'s XML twins (which the
  /// fixtures leave out), and its map at `file` spawned
  fn hex_fixture_world(fixture: &HexFixture, file: &str) -> (MapWorld, Entity, MapGrid) {
    let [set, tall] = fixture.xml_sets;
    crate::core::tilemap::test_util::world_with_xml(&[fixture.files[1], set, tall], file)
  }

  /// Every edge of the parts under `parent`, in its space
  fn all_edges(world: &mut World, parent: Entity) -> Vec<(Vec2, Vec2)> {
    parts_of(world, parent)
      .iter()
      .flat_map(|(_, collider)| edges_of(collider))
      .collect()
  }

  /// Whether `edges` (each once) chain into one closed ring through them all
  fn one_closed_ring<P: Copy + PartialEq + std::fmt::Debug>(edges: &[(P, P)]) -> bool {
    let Some(&(start, mut at)) = edges.first() else {
      return false;
    };
    let mut used = vec![false; edges.len()];
    used[0] = true;
    for _ in 1..edges.len() {
      let Some(next) = (0..edges.len()).find(|&i| !used[i] && edges[i].0 == at) else {
        return false;
      };
      used[next] = true;
      at = edges[next].1;
    }
    at == start
  }

  /// Whether `p` is inside the convex outline `points` (either way round)
  fn inside_outline(points: &[Vec2], p: Vec2) -> bool {
    let crosses: Vec<f32> = (0..points.len())
      .map(|i| {
        let (a, b) = (points[i], points[(i + 1) % points.len()]);
        (b - a).perp_dot(p - a)
      })
      .filter(|c| c.abs() > 1e-3)
      .collect();
    crosses.iter().all(|&c| c > 0.0) || crosses.iter().all(|&c| c < 0.0)
  }

  /// The edges inside `cell`'s outline (their middles in it): a shape
  /// cell's ring
  fn edges_in(grid: &MapGrid, cell: IVec2, edges: &[(Vec2, Vec2)]) -> Vec<(Vec2, Vec2)> {
    let outline = grid.cell_outline(cell);
    edges
      .iter()
      .copied()
      .filter(|&(a, b)| inside_outline(outline.points(), (a + b) / 2.0))
      .collect()
  }

  /// The edges whose both ends are among `points` (within 1/100 px): the
  /// ring through them
  fn edges_through(points: &[Vec2], edges: &[(Vec2, Vec2)]) -> Vec<(Vec2, Vec2)> {
    let among = |p: Vec2| points.iter().any(|q| (*q - p).length() < 1e-2);
    edges
      .iter()
      .copied()
      .filter(|&(a, b)| among(a) && among(b))
      .collect()
  }

  /// Twice the signed area round `edges` (a closed ring's), y down:
  /// positive when they run clockwise on screen, as tile outlines do
  fn shoelace(edges: &[(Vec2, Vec2)]) -> f32 {
    edges.iter().map(|&(a, b)| a.perp_dot(b)).sum()
  }

  /// Every ring of `fixture`'s shape tiles, as drawn on `grid`: the
  /// triangle tiles' and the second tileset's, each with its cell and flips
  fn fixture_rings(fixture: &HexFixture, grid: &MapGrid) -> Vec<(IVec2, TileFlip, [Vec2; 3])> {
    let cell_size = grid.cell.as_vec2();
    let mut rings = Vec::new();
    for &((x, y), flip) in &fixture.triangles {
      let cell = IVec2::new(x, y);
      let points = fixture
        .triangle
        .map(|(px, py)| drawn(grid, cell, (cell_size, Vec2::ZERO), flip, Vec2::new(px, py)));
      rings.push((cell, flip, points));
    }
    let tall = &fixture.tall;
    for &((x, y), flip) in &tall.cells {
      let cell = IVec2::new(x, y);
      let points = tall.triangle.map(|(px, py)| {
        drawn(
          grid,
          cell,
          (tall.size, tall.offset),
          flip,
          Vec2::new(px, py),
        )
      });
      rings.push((cell, flip, points));
    }
    rings
  }

  /// Whether `edges` make one closed ring of exactly `n` points
  fn ring_of(edges: &[(Vec2, Vec2)], n: usize) -> bool {
    let mut points: Vec<Vec2> = Vec::new();
    for &(a, _) in edges {
      if !points.iter().any(|p| (*p - a).length() < 1e-3) {
        points.push(a);
      }
    }
    let starts = |p: Vec2| edges.iter().filter(|e| (e.0 - p).length() < 1e-3).count();
    let ends = |p: Vec2| edges.iter().filter(|e| (e.1 - p).length() < 1e-3).count();
    edges.len() == n && points.len() == n && points.iter().all(|&p| starts(p) == 1 && ends(p) == 1)
  }

  /// Where a point `p` of a tile image `size` px whose tileset's offset is
  /// `offset` draws in `cell` of the hexagonal `grid` with `flip`: from
  /// where `tile_anchor` stands the image, moved by the offset, its flips
  /// mirroring it, then its turns about the quad's middle where it stands
  /// unturned, as `render::tiles` draws a tile
  fn drawn(
    grid: &MapGrid,
    cell: IVec2,
    (size, offset): (Vec2, Vec2),
    flip: TileFlip,
    p: Vec2,
  ) -> Vec2 {
    let anchor = grid.tile_anchor(cell) + offset;
    let mut at = p;
    if flip.contains(TileFlip::HORIZONTAL) {
      at.x = size.x - at.x;
    }
    if flip.contains(TileFlip::VERTICAL) {
      at.y = size.y - at.y;
    }
    let placed = anchor + Vec2::new(at.x, at.y - size.y);
    let middle = anchor + Vec2::new(size.x / 2.0, -size.y / 2.0);
    middle + flip.quad_turn(true) * (placed - middle)
  }

  #[test]
  fn a_hex_layer_collides_along_its_hexagons() {
    for fixture in hex_fixtures() {
      for file in fixture.files {
        let (mut maps, map, grid) = hex_fixture_world(&fixture, file);
        let tiles = colliders(&maps.world, map);
        assert_eq!(
          tiles.bounds(),
          Some(CellRect::from_size(UVec2::new(8, 6))),
          "{file}"
        );
        let cell = |(x, y): (i32, i32)| IVec2::new(x, y);
        let clump: Vec<IVec2> = fixture.clump.iter().map(|&c| cell(c)).collect();
        for &c in &clump {
          assert_eq!(tiles.get(c), TileCell::Solid, "{file}: {c}");
        }
        for &(c, _) in &fixture.triangles {
          assert!(
            matches!(tiles.get(cell(c)), TileCell::Shape(_)),
            "{file}: {c:?} is {:?}",
            tiles.get(cell(c))
          );
        }
        for &c in &fixture.blank {
          assert_eq!(tiles.get(cell(c)), TileCell::Empty, "{file}: {c:?}");
        }
        // Two parts in chunk (0, 0): the clump's 12 edges, and the shape
        // tiles' rings, 15 edges
        let parts = parts_of(&mut maps.world, map);
        let mut counts: Vec<usize> = parts
          .iter()
          .map(|(_, collider)| edges_of(collider).len())
          .collect();
        counts.sort_unstable();
        assert_eq!(counts, [12, 15], "{file}: {parts:?}");
        let edges = all_edges(&mut maps.world, map);
        // The clump's edges are its cells' union border, each once, and
        // chain into one closed ring
        let border = union_border(&grid, &clump);
        assert_eq!(border.len(), 12, "{file}");
        let corners: Vec<Px> = border.iter().map(|&(a, _)| a).collect();
        let mut found: Vec<(Px, Px)> = edges
          .iter()
          .filter_map(|&(a, b)| Some((whole_px(a)?, whole_px(b)?)))
          .filter(|(a, b)| corners.contains(a) && corners.contains(b))
          .collect();
        found.sort_unstable();
        assert_eq!(found, border, "{file}");
        assert!(one_closed_ring(&found), "{file}: {found:?}");
        // Each triangle tile is a closed ring of three points in its cell
        for &(c, flip) in &fixture.triangles {
          let ring = edges_in(&grid, cell(c), &edges);
          assert!(ring_of(&ring, 3), "{file}: {c:?} {flip:?}: {ring:?}");
        }
        // and so is each of the second tileset's tiles, where it draws
        for (cell, flip, points) in fixture_rings(&fixture, &grid).into_iter().skip(3) {
          let ring = edges_through(&points, &edges);
          assert!(ring_of(&ring, 3), "{file}: {cell} {flip:?}: {ring:?}");
        }
        // and the shapeless tiles add nothing
        assert_eq!(edges.len(), 12 + 5 * 3, "{file}: {edges:?}");
      }
    }
  }

  #[test]
  fn a_turned_hex_tile_s_shape_turns_with_its_art() {
    for fixture in hex_fixtures() {
      for file in fixture.files {
        let (mut maps, map, grid) = hex_fixture_world(&fixture, file);
        let edges = all_edges(&mut maps.world, map);
        // The tiles are their cells' size
        let size = grid.cell.as_vec2();
        for &((x, y), flip) in &fixture.triangles {
          let cell = IVec2::new(x, y);
          let ring = edges_in(&grid, cell, &edges);
          let points: Vec<Vec2> = ring.iter().map(|&(a, _)| a).collect();
          // The plain triangle's points, flipped and turned as drawn
          let expected = fixture
            .triangle
            .map(|(px, py)| drawn(&grid, cell, (size, Vec2::ZERO), flip, Vec2::new(px, py)));
          assert_eq!(points.len(), 3, "{file}: {cell} {flip:?}: {ring:?}");
          for want in expected {
            assert!(
              points.iter().any(|p| (*p - want).length() < 1e-2),
              "{file}: {cell} {flip:?}: {want} not among {points:?}"
            );
          }
        }
        // The tile of another size, moved by its tileset's offset, flipped
        // and turned, and turned 120 degrees: its triangle mirrored, then
        // turned about the middle of its quad where it stands unturned
        for (cell, flip, points) in fixture_rings(&fixture, &grid).into_iter().skip(3) {
          let ring = edges_through(&points, &edges);
          assert_eq!(
            ring.len(),
            3,
            "{file}: {cell} {flip:?}: {points:?} in {edges:?}"
          );
        }
        // A whole hexagon is a solid cell however it is turned or flipped
        let turns = [
          TileFlip::HEX_ROTATE_60,
          TileFlip::HEX_ROTATE_120,
          TileFlip::HEX_ROTATE_60 | TileFlip::HEX_ROTATE_120,
          TileFlip::HORIZONTAL | TileFlip::HEX_ROTATE_60,
          TileFlip::VERTICAL,
        ];
        let spots = [(0, 0), (7, 0), (0, 5), (7, 5), (4, 5)];
        let mut view = MapsView::new(&mut maps.world);
        for (&(x, y), &flip) in spots.iter().zip(&turns) {
          assert!(view.set_tile(
            map,
            "walls",
            IVec2::new(x, y),
            Some(Tile::new(0, WHOLE_HEX).with_flip(flip))
          ));
        }
        maps.step();
        let tiles = colliders(&maps.world, map);
        for (&(x, y), &flip) in spots.iter().zip(&turns) {
          assert_eq!(
            tiles.get(IVec2::new(x, y)),
            TileCell::Solid,
            "{file}: {flip:?}"
          );
        }
        // and a turned triangle tile is a shape still
        let mut view = MapsView::new(&mut maps.world);
        assert!(view.set_tile(
          map,
          "walls",
          IVec2::new(4, 3),
          Some(Tile::new(0, TRIANGLE).with_flip(TileFlip::HEX_ROTATE_120))
        ));
        maps.step();
        let tiles = colliders(&maps.world, map);
        assert!(matches!(tiles.get(IVec2::new(4, 3)), TileCell::Shape(_)));
      }
    }
  }

  #[test]
  fn hex_tile_rings_wind_clockwise_after_any_flip_or_turn() {
    for fixture in hex_fixtures() {
      for file in fixture.files {
        let (mut maps, map, grid) = hex_fixture_world(&fixture, file);
        let edges = all_edges(&mut maps.world, map);
        // Every shape tile's ring, plain, flipped, turned, or both, runs
        // clockwise on screen round its shape
        for (cell, flip, points) in fixture_rings(&fixture, &grid) {
          let ring = edges_through(&points, &edges);
          assert_eq!(ring.len(), 3, "{file}: {cell} {flip:?}: {ring:?}");
          let area = shoelace(&ring);
          assert!(
            area > 0.0,
            "{file}: {cell} {flip:?} runs anticlockwise: {area} {ring:?}"
          );
        }
        // as the hexagons' own sides do round the clump
        let clump: Vec<IVec2> = fixture
          .clump
          .iter()
          .map(|&(x, y)| IVec2::new(x, y))
          .collect();
        let corners: Vec<Px> = union_border(&grid, &clump)
          .iter()
          .map(|&(a, _)| a)
          .collect();
        let border: Vec<(Vec2, Vec2)> = edges
          .iter()
          .copied()
          .filter(|&(a, b)| {
            [a, b]
              .into_iter()
              .all(|p| whole_px(p).is_some_and(|p| corners.contains(&p)))
          })
          .collect();
        assert_eq!(border.len(), 12, "{file}");
        assert!(shoelace(&border) > 0.0, "{file}");
      }
    }
  }

  #[test]
  fn whole_hexagon_shapes_on_degenerate_grids_are_solid() {
    let cell = UVec2::splat(32);
    for (axis, index) in [
      (StaggerAxis::Y, StaggerIndex::Odd),
      (StaggerAxis::X, StaggerIndex::Even),
    ] {
      // Side 0: diamonds, two of whose six corners repeat; a side the
      // cell's length or longer: rectangles in brick rows, whose six
      // corners hold a point along each long side
      for side in [0, 32, 40] {
        let grid_shape = GridShape::Hexagonal { axis, index, side };
        let mut map = TileMap::new(UVec2::new(6, 4), cell).with_shape(grid_shape);
        let grid = map.grid();
        // The image's top-left as a tile stands in cell (0, 0)
        let top_left = grid.tile_anchor(IVec2::ZERO) - Vec2::new(0.0, cell.y as f32);
        let outline: Vec<Vec2> = grid
          .cell_outline(IVec2::ZERO)
          .points()
          .iter()
          .map(|&p| p - top_left)
          .collect();
        // Its corners alone: repeats and points along a side dropped
        let mut corners = outline.clone();
        corners.dedup();
        loop {
          let n = corners.len();
          let straight = (0..n).find(|&i| {
            let (a, p, b) = (corners[(i + n - 1) % n], corners[i], corners[(i + 1) % n]);
            p == a || (p - a).perp_dot(b - p) == 0.0
          });
          match straight {
            Some(i) if n > 3 => {
              corners.remove(i);
            }
            _ => break,
          }
        }
        assert_eq!(corners.len(), 4, "{grid_shape:?}: {corners:?}");
        let mut set = Tileset::new("degenerate", cell, TilesetImage::Collection);
        for (id, points) in [(0, outline), (1, corners)] {
          set.insert_tile(
            id,
            TileData {
              shapes: vec![shape(points, false)],
              ..TileData::default()
            },
          );
        }
        let set = map.add_tileset(TilesetSource::Embedded(Arc::new(set)));
        let walls = map.add_tile_layer("walls");
        // Turned, still whole (not flipped: on a grid whose side is longer
        // than its tile, the hexagon is wider or taller than the image a
        // flip mirrors across)
        let placed = [
          ((1, 1), 0, TileFlip::NONE),
          ((2, 1), 1, TileFlip::NONE),
          ((4, 2), 1, TileFlip::HEX_ROTATE_60),
          (
            (1, 3),
            1,
            TileFlip::HEX_ROTATE_60 | TileFlip::HEX_ROTATE_120,
          ),
        ];
        let tiles = map.tiles_mut(walls).expect("a tile layer");
        for &((x, y), id, flip) in &placed {
          tiles.set(IVec2::new(x, y), Some(Tile::new(set, id).with_flip(flip)));
        }
        let mut maps = MapWorld::new();
        let handle = add(&mut maps.world, map);
        let map = maps.spawn(MapRoot::new(handle));
        maps.frame();
        let tiles = colliders(&maps.world, map);
        let cells: Vec<IVec2> = placed
          .iter()
          .map(|&((x, y), ..)| IVec2::new(x, y))
          .collect();
        for &c in &cells {
          assert_eq!(tiles.get(c), TileCell::Solid, "{grid_shape:?}: {c}");
        }
        // and they merge as solid cells do: the parts hold their border
        let mut edges: Vec<(Px, Px)> = all_edges(&mut maps.world, map)
          .into_iter()
          .filter_map(|(a, b)| Some((whole_px(a)?, whole_px(b)?)))
          .collect();
        edges.sort_unstable();
        assert_eq!(edges, union_border(&grid, &cells), "{grid_shape:?}");
      }
    }
  }

  /// An infinite map of `cell`-sized hexagons (axis Y, odd rows, side
  /// `side`) over `whole_hex_set`, its layer "ground" holding whole
  /// hexagons at `cells`
  fn hex_map(cell: UVec2, side: u32, cells: &[IVec2]) -> TileMap {
    let mut map = TileMap::infinite(cell).with_shape(GridShape::Hexagonal {
      axis: StaggerAxis::Y,
      index: StaggerIndex::Odd,
      side,
    });
    let set = whole_hex_set(&map.grid());
    let set = map.add_tileset(TilesetSource::Embedded(Arc::new(set)));
    let layer = map.add_tile_layer("ground");
    let grid = map.tiles_mut(layer).expect("a tile layer");
    for &at in cells {
      grid.set(at, Some(Tile::new(set, WHOLE_HEX)));
    }
    map
  }

  #[test]
  fn an_edit_on_a_hex_map_rebuilds_only_its_parts() {
    use crate::core::ecs::DetectChanges;
    use crate::core::physics::Physics;
    let cell = UVec2::new(120, 140);
    // A clump well inside each of chunks (0, 0), (1, 0) and (-1, -1)
    let clumps: Vec<IVec2> = [(5, 5), (6, 5), (6, 6)]
      .into_iter()
      .flat_map(|(x, y)| [(x, y), (x + 35, y), (x - 32, y - 32)])
      .map(|(x, y)| IVec2::new(x, y))
      .collect();
    let map = hex_map(cell, 70, &clumps);
    let grid = map.grid();
    let mut maps = MapWorld::new();
    let handle = add(&mut maps.world, map);
    let map = maps.spawn(MapRoot::new(handle));
    maps.frame();
    // Each part: the chunk of the cell its edges' middle is in, its
    // physics handle, when its collider last changed, and its shape
    type Held = (Entity, IVec2, String, String, Option<ColliderShape>);
    let held = |world: &mut World| -> Vec<Held> {
      let mut found: Vec<Held> = parts_of(world, map)
        .into_iter()
        .map(|(part, collider)| {
          let edges = edges_of(&collider);
          let middle = edges.iter().map(|&(a, b)| a + b).sum::<Vec2>() / (2.0 * edges.len() as f32);
          let chunk = grid.cell_at(middle).div_euclid(IVec2::splat(32));
          let physics = world.resource::<Physics>();
          let handle = format!(
            "{:?}",
            physics.collider_entries.get(&part).map(|e| e.handle)
          );
          let collider = world.entity(part).get_ref::<Collider>();
          let changed = format!("{:?}", collider.as_ref().map(|c| c.last_changed()));
          (
            part,
            chunk,
            handle,
            changed,
            collider.map(|c| c.shape.clone()),
          )
        })
        .collect();
      found.sort_by_key(|(_, chunk, ..)| (chunk.y, chunk.x));
      found
    };
    let before = held(&mut maps.world);
    let chunks: Vec<IVec2> = before.iter().map(|(_, c, ..)| *c).collect();
    assert_eq!(
      chunks,
      [IVec2::new(-1, -1), IVec2::ZERO, IVec2::new(1, 0)],
      "a part a clump"
    );
    // A hexagon joins the clump in chunk (1, 0)
    assert!(MapsView::new(&mut maps.world).set_tile(
      map,
      "ground",
      IVec2::new(42, 5),
      Some(Tile::new(0, WHOLE_HEX))
    ));
    maps.step();
    let after = held(&mut maps.world);
    assert_eq!(after.len(), before.len());
    for ((part, chunk, handle, changed, shape), was) in after.iter().zip(&before) {
      assert_eq!(
        (part, chunk, handle),
        (&was.0, &was.1, &was.2),
        "kept its entity"
      );
      assert!(handle.starts_with("Some"), "{handle}");
      if *chunk == IVec2::new(1, 0) {
        assert_ne!(
          shape, &was.4,
          "chunk {chunk}'s part took its new shape in place"
        );
      } else {
        assert_eq!(
          (changed, shape),
          (&was.3, &was.4),
          "chunk {chunk}'s part was not touched"
        );
      }
    }
    // The edited clump's edges are its four cells' border
    let edges: Vec<(Px, Px)> = edges_of(
      &parts_of(&mut maps.world, map)
        .into_iter()
        .find(|(part, _)| *part == after[2].0)
        .map(|(_, collider)| collider)
        .expect("chunk (1, 0)'s part"),
    )
    .into_iter()
    .filter_map(|(a, b)| Some((whole_px(a)?, whole_px(b)?)))
    .collect();
    let mut sorted = edges.clone();
    sorted.sort_unstable();
    let grown: Vec<IVec2> = [(40, 5), (41, 5), (41, 6), (42, 5)]
      .map(|(x, y)| IVec2::new(x, y))
      .to_vec();
    assert_eq!(sorted, union_border(&grid, &grown));
  }

  #[test]
  fn a_code_built_hex_layer_collides_and_casts_shadows() {
    use crate::core::tilemap::MapOccluder;
    // A layer added while the map was orthogonal keeps colliding when the
    // map moves onto a hexagonal grid; one added after collides too, as on
    // any grid
    let mut map = TileMap::new(UVec2::new(6, 6), UVec2::splat(32));
    map.add_tile_layer("honeycomb-5d07");
    let mut map = map.with_shape(GridShape::Hexagonal {
      axis: StaggerAxis::Y,
      index: StaggerIndex::Odd,
      side: 12,
    });
    let set = map.add_tileset(TilesetSource::Embedded(Arc::new(whole_hex_set(
      &map.grid(),
    ))));
    let floor = map.add_tile_layer("hexfloor-5d07");
    assert!(map.layers[0].info.collides && map.layers[floor].info.collides);
    for (layer, row) in [(0, 2), (floor, 4)] {
      let grid = map.tiles_mut(layer).expect("a tile layer");
      for x in 0..4 {
        grid.set(IVec2::new(x, row), Some(Tile::new(set, WHOLE_HEX)));
      }
    }
    let grid = map.grid();
    let mut maps = MapWorld::new();
    let handle = add(&mut maps.world, map);
    let map = maps.spawn(MapRoot::new(handle));
    maps.frame();
    let tiles = colliders(&maps.world, map);
    for row in [2, 4] {
      for x in 0..4 {
        assert_eq!(tiles.get(IVec2::new(x, row)), TileCell::Solid);
      }
    }
    let solid = |extra: &[IVec2]| -> Vec<IVec2> {
      let mut cells: Vec<IVec2> = (0..4)
        .flat_map(|x| [IVec2::new(x, 2), IVec2::new(x, 4)])
        .collect();
      cells.extend_from_slice(extra);
      cells
    };
    let border = |world: &mut World| -> Vec<(Px, Px)> {
      let mut edges: Vec<(Px, Px)> = all_edges(world, map)
        .into_iter()
        .filter_map(|(a, b)| Some((whole_px(a)?, whole_px(b)?)))
        .collect();
      edges.sort_unstable();
      edges
    };
    assert_eq!(border(&mut maps.world), union_border(&grid, &solid(&[])));
    // An edit collides at once: the cell between the rows joins them
    let mut view = MapsView::new(&mut maps.world);
    assert!(view.set_tile(
      map,
      "honeycomb-5d07",
      IVec2::new(1, 3),
      Some(Tile::new(0, WHOLE_HEX))
    ));
    maps.step();
    maps.frame();
    assert_eq!(
      colliders(&maps.world, map).get(IVec2::new(1, 3)),
      TileCell::Solid
    );
    assert_eq!(
      border(&mut maps.world),
      union_border(&grid, &solid(&[IVec2::new(1, 3)]))
    );
    // The layers cast shadows as they collide, under the first of them,
    // and nothing warns about either layer
    let first = layers_of(&mut maps.world, map)[0];
    let occluders: Vec<Entity> = maps
      .world
      .query::<(Entity, &MapOccluder, &ChildOf)>()
      .iter(&maps.world)
      .filter(|(_, _, child_of)| child_of.parent() == first)
      .map(|(entity, ..)| entity)
      .collect();
    assert!(!occluders.is_empty());
    assert_eq!(
      maps.world.query::<&MapOccluder>().iter(&maps.world).count(),
      occluders.len()
    );
    for layer in ["honeycomb-5d07", "hexfloor-5d07"] {
      let warned = crate::core::dev_log::get_entries()
        .iter()
        .filter(|e| e.message.contains(&format!("\"{layer}\"")))
        .count();
      assert_eq!(warned, 0, "{layer}");
    }
  }

  #[test]
  fn a_hex_grid_too_big_to_trace_warns_once_and_builds_nothing() {
    // Hexagons 16,385 px across: three tiles of a colliding layer
    let cell = UVec2::new(16_385, 16_000);
    let mut map = TileMap::new(UVec2::new(4, 2), cell).with_shape(GridShape::Hexagonal {
      axis: StaggerAxis::Y,
      index: StaggerIndex::Odd,
      side: 8_000,
    });
    let set = map.add_tileset(TilesetSource::Embedded(Arc::new(whole_hex_set(
      &map.grid(),
    ))));
    let walls = map.add_tile_layer("walls-77c1");
    let grid = map.tiles_mut(walls).expect("a tile layer");
    for x in 0..3 {
      grid.set(IVec2::new(x, 1), Some(Tile::new(set, WHOLE_HEX)));
    }
    let mut maps = MapWorld::new();
    let handle = add(&mut maps.world, map);
    let map = maps.spawn(MapRoot::new(handle));
    maps.frame();
    let mut view = MapsView::new(&mut maps.world);
    assert!(view.set_tile(
      map,
      "walls-77c1",
      IVec2::new(3, 1),
      Some(Tile::new(0, WHOLE_HEX))
    ));
    maps.step();
    maps.frame();
    let layers = layers_of(&mut maps.world, map);
    for entity in std::iter::once(map).chain(layers) {
      assert!(maps.world.get::<TileColliders>(entity).is_none());
    }
    assert_eq!(count_parts(&mut maps.world), 0);
    let warned = |what: &str| {
      crate::core::dev_log::get_entries()
        .iter()
        .filter(|e| {
          e.message
            .contains("layer \"walls-77c1\" has hexagonal cells over 16,384 px")
            && e.message.contains(what)
        })
        .count()
    };
    assert_eq!(warned("which don't collide"), 1);
    // Nor do they cast shadows, which the layer says once too
    assert_eq!(warned("which cast no shadows"), 1);
    let occluders = maps
      .world
      .query::<&crate::core::tilemap::MapOccluder>()
      .iter(&maps.world)
      .count();
    assert_eq!(occluders, 0);
  }

  // ---- floors ---------------------------------------------------------------

  /// A tile with no shapes, a solid block, and stairs up to floors 1 and 2
  const PLAIN: Tile = Tile {
    tileset: 0,
    id: 9,
    flip: TileFlip::NONE,
  };
  const WALL: Tile = Tile {
    tileset: 0,
    id: SOLID,
    flip: TileFlip::NONE,
  };
  const UP_1: Tile = Tile {
    tileset: 1,
    id: 0,
    flip: TileFlip::NONE,
  };
  const UP_2: Tile = Tile {
    tileset: 1,
    id: 1,
    flip: TileFlip::NONE,
  };

  /// Floor 1's height in the maps with floors: its bottom layer's offset
  const UP: Vec2 = Vec2::new(0.0, -32.0);

  /// 16 px stairs: tile 0 up to floor 1, tile 1 up to floor 2
  fn stairs_set() -> Tileset {
    let mut set = Tileset::new("stairs", UVec2::splat(16), TilesetImage::Collection);
    for (id, to) in [(0, 1), (1, 2)] {
      let stairs = TileData {
        stairs: Some(to),
        ..TileData::default()
      };
      set.insert_tile(id, stairs);
    }
    set
  }

  /// A tile layer of a map with floors: its name, floor, offset and tiles
  type Storey<'a> = (&'a str, u8, Vec2, &'a [(IVec2, Tile)]);

  /// A map of `size` 16 px cells (infinite for none) over `shapes` and
  /// `stairs_set`, with a tile layer for each of `layers`
  fn storeyed(size: Option<UVec2>, layers: &[Storey<'_>]) -> TileMap {
    let cell = UVec2::splat(16);
    let mut map = match size {
      Some(size) => TileMap::new(size, cell),
      None => TileMap::infinite(cell),
    };
    map.add_tileset(TilesetSource::Embedded(Arc::new(shapes())));
    map.add_tileset(TilesetSource::Embedded(Arc::new(stairs_set())));
    for &(name, floor, offset, tiles) in layers {
      let layer = map.add_tile_layer(name);
      map.layers[layer].info.floor = floor;
      map.layers[layer].info.offset = offset;
      let grid = map.tiles_mut(layer).expect("a tile layer");
      for &(cell, tile) in tiles {
        grid.set(cell, Some(tile));
      }
    }
    map
  }

  /// The cells from `min` to `max` (both included), each holding `tile`
  fn block(min: (i32, i32), max: (i32, i32), tile: Tile) -> Vec<(IVec2, Tile)> {
    (min.1..=max.1)
      .flat_map(|y| (min.0..=max.0).map(move |x| (IVec2::new(x, y), tile)))
      .collect()
  }

  /// `cells`, by row
  fn by_row(mut cells: Vec<IVec2>) -> Vec<IVec2> {
    cells.sort_by_key(|c| (c.y, c.x));
    cells.dedup();
    cells
  }

  /// The cells of `tiles`, by row
  fn cells_of(tiles: &[(IVec2, Tile)]) -> Vec<IVec2> {
    by_row(tiles.iter().map(|(cell, _)| *cell).collect())
  }

  /// The edges of the floors above the ground under `map`: each `MapFloor`
  /// child's floor and entity, by floor
  fn floor_edges(world: &mut World, map: Entity) -> Vec<(u8, Entity)> {
    let mut found: Vec<(u8, Entity)> = world
      .query::<(Entity, &MapFloor, &ChildOf)>()
      .iter(world)
      .filter(|(_, _, child_of)| child_of.parent() == map)
      .map(|(entity, edge, _)| (edge.floor, entity))
      .collect();
    found.sort();
    found
  }

  /// The entity holding floor `floor`'s edge under `map`
  fn edge_of(world: &mut World, map: Entity, floor: u8) -> Entity {
    let edges = floor_edges(world, map);
    match edges.iter().find(|(of, _)| *of == floor) {
      Some(&(_, edge)) => edge,
      None => panic!("{map} has no edge of floor {floor}: {edges:?}"),
    }
  }

  /// The `Solid` cells of `tiles`, by row
  fn solid_cells(tiles: &TileColliders) -> Vec<IVec2> {
    let extent = tiles.extent();
    (extent.min.y..extent.max.y)
      .flat_map(|y| (extent.min.x..extent.max.x).map(move |x| IVec2::new(x, y)))
      .filter(|&cell| tiles.get(cell) == TileCell::Solid)
      .collect()
  }

  /// Where `entity` stands in its parent
  fn place(world: &World, entity: Entity) -> Vec2 {
    world
      .get::<Transform>(entity)
      .map_or(Vec2::NAN, |at| at.translation)
  }

  /// The border of the union of the lattice squares `cells`, each side
  /// running clockwise on screen round its square, as outlines run, sorted
  fn square_border(cells: &[IVec2]) -> Vec<LatticeEdge> {
    let mut sides = Vec::new();
    for c in cells {
      let (x, y) = (c.x, c.y);
      sides.extend([
        ((x, y), (x + 1, y)),
        ((x + 1, y), (x + 1, y + 1)),
        ((x + 1, y + 1), (x, y + 1)),
        ((x, y + 1), (x, y)),
      ]);
    }
    let mut border: Vec<LatticeEdge> = sides
      .iter()
      .copied()
      .filter(|&(a, b)| !sides.contains(&(b, a)))
      .collect();
    border.sort_unstable();
    border
  }

  /// The warnings in the dev log holding every one of `words`
  fn warned(words: &[&str]) -> usize {
    crate::core::dev_log::get_entries()
      .iter()
      .filter(|entry| words.iter().all(|word| entry.message.contains(word)))
      .count()
  }

  /// An engine with `map` spawned and landed, and the map's entity
  fn engine_with(map: TileMap) -> Option<(HeadlessEngine, Entity)> {
    let builder = EngineBuilder::new().resolution(320, 180).start_scene(Empty);
    let mut engine = HeadlessEngine::new_or_skip(builder)?;
    let handle = engine
      .world_mut()
      .resource_mut::<Assets<TileMap>>()
      .add(map);
    let map = engine.world_mut().spawn(MapRoot::new(handle)).id();
    crate::core::tilemap::test_util::step_until_maps_land(&mut engine);
    Some((engine, map))
  }

  /// A top-down character 10 px across standing at `at`, on `floor` (none:
  /// the ground floor)
  fn walker(engine: &mut HeadlessEngine, at: Vec2, floor: Option<Floor>) -> Entity {
    let mut walker = engine.world_mut().spawn((
      Transform::from_translation(at),
      Collider::circle(5.0),
      TopDownMovement::default(),
    ));
    if let Some(floor) = floor {
      walker.insert(floor);
    }
    walker.id()
  }

  /// `steps` frames of `walkers` heading along `direction`; where each then
  /// stands
  fn walk(
    engine: &mut HeadlessEngine,
    walkers: &[Entity],
    direction: Vec2,
    steps: usize,
  ) -> Vec<Vec2> {
    for _ in 0..steps {
      for &walker in walkers {
        if let Some(mut intent) = engine.world_mut().get_mut::<MoveIntent>(walker) {
          intent.direction = direction;
        }
      }
      engine.step().expect("a frame");
    }
    walkers
      .iter()
      .map(|&walker| place(engine.world_mut(), walker))
      .collect()
  }

  #[test]
  fn layers_on_different_floors_collide_on_their_own_grids() {
    // "ground" and "upper" (floor 1, 0 px up) at one offset, the origin
    let mut maps = MapWorld::new();
    let handle = add(
      &mut maps.world,
      storeyed(
        Some(UVec2::new(8, 4)),
        &[
          ("ground", 0, Vec2::ZERO, &[(IVec2::new(1, 2), WALL)]),
          ("upper", 1, Vec2::ZERO, &[(IVec2::new(5, 2), WALL)]),
        ],
      ),
    );
    let map = maps.spawn(MapRoot::new(handle));
    let layers = layers_of(&mut maps.world, map);
    // Each floor's wall in a grid of its own, on that floor
    let ground = colliders(&maps.world, map);
    assert_eq!(ground.floor, Floor::GROUND);
    assert_eq!(solid_cells(&ground), [IVec2::new(1, 2)]);
    assert!(maps.world.get::<TileColliders>(layers[0]).is_none());
    let upper = colliders(&maps.world, layers[1]);
    assert_eq!(upper.floor, Floor::On(1));
    assert_eq!(solid_cells(&upper), [IVec2::new(5, 2)]);
    // and in physics: the floor 1 grid's parts are on floor 1, the ground
    // floor's on none
    let floors = |world: &mut World, holder: Entity| -> Vec<Option<Floor>> {
      let parts = parts_of(world, holder);
      parts
        .iter()
        .map(|(part, _)| world.get::<Floor>(*part).copied())
        .collect()
    };
    assert_eq!(floors(&mut maps.world, map), [None]);
    assert_eq!(floors(&mut maps.world, layers[1]), [Some(Floor::On(1))]);

    // A ground layer and a floor 1 layer both drawn 32 px up (floor 1's
    // height), and a floor 1 layer drawn at the origin above its floor's
    // bottom layer: each floor's layers group where they stand in ground
    // space, never with another floor's or in the map's own grid
    let handle = add(
      &mut maps.world,
      storeyed(
        Some(UVec2::new(8, 4)),
        &[
          ("ground", 0, Vec2::ZERO, &[(IVec2::new(0, 3), WALL)]),
          ("ledge", 0, UP, &[(IVec2::new(1, 3), WALL)]),
          ("upper", 1, UP, &[(IVec2::new(2, 3), WALL)]),
          ("rail", 1, Vec2::ZERO, &[(IVec2::new(3, 3), WALL)]),
        ],
      ),
    );
    let map = maps.spawn(MapRoot::new(handle));
    let layers = layers_of(&mut maps.world, map);
    let grid = |world: &World, holder: Entity| {
      let tiles = colliders(world, holder);
      (place(world, holder), tiles.floor, solid_cells(&tiles))
    };
    let one = |x: i32| vec![IVec2::new(x, 3)];
    assert_eq!(grid(&maps.world, map), (Vec2::ZERO, Floor::GROUND, one(0)));
    assert!(maps.world.get::<TileColliders>(layers[0]).is_none());
    assert_eq!(grid(&maps.world, layers[1]), (UP, Floor::GROUND, one(1)));
    assert_eq!(
      grid(&maps.world, layers[2]),
      (Vec2::ZERO, Floor::On(1), one(2))
    );
    assert_eq!(
      grid(&maps.world, layers[3]),
      (Vec2::new(0.0, 32.0), Floor::On(1), one(3))
    );
  }

  #[test]
  fn a_raised_floor_s_edge_keeps_a_character_on_it() {
    // Floor 1 raised over open ground: 6 x 4 cells, from (64, 48) to
    // (160, 112) where it stands
    let ground = block((0, 0), (19, 9), PLAIN);
    let raised = block((4, 3), (9, 6), PLAIN);
    let map = storeyed(
      Some(UVec2::new(20, 10)),
      &[
        ("ground", 0, Vec2::ZERO, &ground),
        ("raised", 1, UP, &raised),
      ],
    );
    let Some((mut engine, map)) = engine_with(map) else {
      return;
    };
    // The edge is no wall to navigation built from the whole world
    let mut paths = crate::core::navigation::NavigationGrid::new(320.0, 160.0, 16.0);
    paths.rebuild_from_world(engine.world_mut());
    for cell in cells_of(&ground) {
      assert!(paths.is_walkable(cell.into()), "{cell}");
    }
    let on_top = walker(&mut engine, Vec2::new(112.0, 80.0), Some(Floor::On(1)));
    let below = walker(&mut engine, Vec2::new(24.0, 80.0), None);
    // Heading right, the one on top stops at the floor's edge (its radius
    // in), and the one on the ground walks on under it
    let at = walk(&mut engine, &[on_top, below], Vec2::X, 90);
    assert!((at[0].x - 155.0).abs() < 1.0, "on top at {}", at[0]);
    assert!(at[1].x > 200.0, "below at {}", at[1]);
    // The edge holds it in on every side
    let at = walk(&mut engine, &[on_top], Vec2::NEG_Y, 60);
    assert!((at[0].y - 53.0).abs() < 1.0, "on top at {}", at[0]);
    let at = walk(&mut engine, &[on_top], Vec2::NEG_X, 90);
    assert!((at[0].x - 69.0).abs() < 1.0, "on top at {}", at[0]);
    let at = walk(&mut engine, &[on_top], Vec2::Y, 60);
    assert!((at[0].y - 107.0).abs() < 1.0, "on top at {}", at[0]);
    // nor to the map's own navigation
    let window = CellRect::from_size(UVec2::new(20, 10));
    let Some(grid) = MapsView::new(engine.world_mut()).navigation(map, "ground", window) else {
      panic!("{map} has a ground layer");
    };
    for cell in cells_of(&ground) {
      assert!(grid.is_walkable(cell.into()), "{cell}");
    }
  }

  #[test]
  fn the_floor_filter_holds_between_tiles_and_entities() {
    // A floor 1 wall down column 8 (128 to 144 px where it stands) of a
    // raised floor
    let ground = block((0, 0), (19, 9), PLAIN);
    let raised = block((2, 2), (13, 7), PLAIN);
    let wall = block((8, 2), (8, 7), WALL);
    let map = storeyed(
      Some(UVec2::new(20, 10)),
      &[
        ("ground", 0, Vec2::ZERO, &ground),
        ("raised", 1, UP, &raised),
        ("walls", 1, UP, &wall),
      ],
    );
    let Some((mut engine, _)) = engine_with(map) else {
      return;
    };
    // One on floor 1 and one on the ground, standing at one spot
    let upper = walker(&mut engine, Vec2::new(80.0, 80.0), Some(Floor::On(1)));
    let lower = walker(&mut engine, Vec2::new(80.0, 80.0), None);
    let at = walk(&mut engine, &[upper, lower], Vec2::X, 60);
    assert!(
      (at[0].x - 123.0).abs() < 1.0,
      "the wall stops the one on floor 1: {}",
      at[0]
    );
    assert!(
      at[1].x > 150.0,
      "the one on the ground walks through it: {}",
      at[1]
    );
  }

  #[test]
  fn stairs_join_the_upper_floor_s_footprint() {
    // Floor 1 over (4, 3) to (8, 6), with stairs up to it on the ground at
    // (3, 4) and (3, 5); floor 2 over (5, 4) to (6, 5), with stairs up to it
    // from floor 1 at (8, 6) and from the ground at (10, 4)
    let mut ground = block((0, 0), (11, 7), PLAIN);
    ground.extend([
      (IVec2::new(3, 4), UP_1),
      (IVec2::new(3, 5), UP_1),
      (IVec2::new(10, 4), UP_2),
    ]);
    let mut upper = block((4, 3), (8, 6), PLAIN);
    upper.push((IVec2::new(8, 6), UP_2));
    // Stairs on floor 2 naming floor 1 lead down: a plain tile there
    let mut roof = block((5, 4), (6, 5), PLAIN);
    roof.push((IVec2::new(11, 1), UP_1));
    // Ground stairs on a layer standing 16 px right are on no edge's grid,
    // and a floor 1 layer standing 8 px right is left out of its edge
    let steps = [(IVec2::new(1, 4), UP_1)];
    let awning = [(IVec2::new(0, 0), PLAIN)];
    let map = storeyed(
      Some(UVec2::new(12, 8)),
      &[
        ("ground", 0, Vec2::ZERO, &ground),
        ("steps", 0, Vec2::new(16.0, 0.0), &steps),
        ("upper", 1, UP, &upper),
        ("awning-3f1", 1, Vec2::new(8.0, -32.0), &awning),
        ("roof", 2, Vec2::new(0.0, -64.0), &roof),
      ],
    );
    let mut maps = MapWorld::new();
    let handle = add(&mut maps.world, map);
    let map = maps.spawn(MapRoot::new(handle));
    maps.frame();
    let floors: Vec<u8> = floor_edges(&mut maps.world, map)
      .iter()
      .map(|(floor, _)| *floor)
      .collect();
    assert_eq!(floors, [1, 2]);
    let first = edge_of(&mut maps.world, map, 1);
    let tiles = colliders(&maps.world, first);
    assert_eq!(tiles.floor, Floor::On(1));
    let mut want = cells_of(&upper);
    want.extend([IVec2::new(3, 4), IVec2::new(3, 5)]);
    let want = by_row(want);
    assert_eq!(solid_cells(&tiles), want);
    // One outline round the floor and its stairs
    let to_pixel = glam::Affine2::from_scale(Vec2::splat(16.0));
    assert_eq!(
      cut(&lattice_edges(&mut maps.world, first, to_pixel)),
      square_border(&want)
    );
    let second = edge_of(&mut maps.world, map, 2);
    let second = colliders(&maps.world, second);
    assert_eq!(second.floor, Floor::On(2));
    let mut want = cells_of(&roof);
    want.extend([IVec2::new(8, 6), IVec2::new(10, 4)]);
    assert_eq!(solid_cells(&second), by_row(want));
    assert_eq!(warned(&["\"awning-3f1\"", "floor 1"]), 1);
  }

  #[test]
  fn a_floor_s_edge_follows_edits() {
    use crate::core::ecs::DetectChanges;
    // Islands of floor 1 in chunks 0, 1 and 2
    let mut raised = Vec::new();
    for x in [4, 36, 68] {
      raised.extend(block((x, 2), (x + 3, 5), PLAIN));
    }
    let map = storeyed(
      Some(UVec2::new(96, 8)),
      &[("ground", 0, Vec2::ZERO, &[]), ("raised", 1, UP, &raised)],
    );
    let mut maps = MapWorld::new();
    let handle = add(&mut maps.world, map);
    let map = maps.spawn(MapRoot::new(handle));
    let edge = edge_of(&mut maps.world, map, 1);
    // Each of the edge's parts: its chunks, when its collider last changed
    // and its shape
    type Part = (Entity, Vec<IVec2>, String, Option<ColliderShape>);
    let parts = |world: &mut World| -> Vec<Part> {
      parts_by_chunk(world, edge)
        .into_iter()
        .map(|(part, chunks, _)| {
          let collider = world.entity(part).get_ref::<Collider>();
          let changed = format!("{:?}", collider.as_ref().map(|c| c.last_changed()));
          (part, chunks, changed, collider.map(|c| c.shape.clone()))
        })
        .collect()
    };
    let before = parts(&mut maps.world);
    let mut chunks: Vec<Vec<IVec2>> = before.iter().map(|part| part.1.clone()).collect();
    chunks.sort_by_key(|chunks| chunks.first().map(|c| (c.y, c.x)));
    let (left, middle, right) = (IVec2::ZERO, IVec2::new(1, 0), IVec2::new(2, 0));
    assert_eq!(chunks, [vec![left], vec![middle], vec![right]]);

    // Floor 1 painted a cell wider in chunk 1: only that chunk's part takes
    // a new shape, the others untouched
    let cell = IVec2::new(40, 3);
    assert!(MapsView::new(&mut maps.world).set_tile(map, "raised", cell, Some(PLAIN)));
    maps.step();
    assert_eq!(colliders(&maps.world, edge).get(cell), TileCell::Solid);
    let after = parts(&mut maps.world);
    assert_eq!(after.len(), before.len());
    for (part, chunks, changed, shape) in &before {
      let Some((_, _, now_changed, now_shape)) = after.iter().find(|p| p.0 == *part) else {
        panic!("chunk {chunks:?}'s part went");
      };
      if chunks[..] == [middle] {
        assert_ne!(
          now_shape, shape,
          "the painted chunk's part took its new shape"
        );
      } else {
        assert_eq!((now_changed, now_shape), (changed, shape), "{chunks:?}");
      }
    }
    // A cell cleared in chunk 2 leaves the edge too, which keeps its entity
    let cell = IVec2::new(68, 2);
    assert!(MapsView::new(&mut maps.world).set_tile(map, "raised", cell, None));
    maps.step();
    assert_eq!(colliders(&maps.world, edge).get(cell), TileCell::Empty);
    assert_eq!(edge_of(&mut maps.world, map, 1), edge);
  }

  #[test]
  fn a_reload_rebuilds_a_floor_s_edge() {
    // gid 1 a plain tile, gid 3 stairs up to floor 1
    const SET: &str = r#"{"type":"tileset","name":"set","tilewidth":16,"tileheight":16,
      "tilecount":16,"columns":4,"image":"../tiled/images/tiles.png","imagewidth":64,
      "imageheight":64,"tiles":[{"id":2,"properties":[{"name":"stairs","type":"int","value":1}]}]}"#;
    // A 6 x 4 map of the tile layers "ground" and "upper" (`upper` its
    // extra fields), each of gids row by row
    let level = |ground: &[u32], upper: &[u32], extra: &str| {
      let layer = |id: u32, name: &str, data: &[u32], extra: &str| {
        let data: Vec<String> = data.iter().map(u32::to_string).collect();
        format!(
          r#"{{"id":{id},"name":"{name}","type":"tilelayer","width":6,"height":4,"data":[{}]{extra}}}"#,
          data.join(",")
        )
      };
      format!(
        r#"{{"type":"map","orientation":"orthogonal","width":6,"height":4,"tilewidth":16,
          "tileheight":16,"tilesets":[{{"firstgid":1,"source":"set.tsj"}}],"layers":[{},{}]}}"#,
        layer(1, "ground", ground, ""),
        layer(2, "upper", upper, extra)
      )
    };
    // "upper" on `floor`, drawn 32 px up and `x` px right
    let on = |floor: u8, x: i32| {
      format!(
        r#","offsetx":{x},"offsety":-32,"properties":[{{"name":"floor","type":"int","value":{floor}}}]"#
      )
    };
    let gids = |cells: &[(i32, i32)], gid: u32| -> Vec<u32> {
      let mut data = vec![0; 24];
      for &(x, y) in cells {
        data[(y * 6 + x) as usize] = gid;
      }
      data
    };
    let cells = |list: &[(i32, i32)]| -> Vec<IVec2> {
      by_row(list.iter().map(|&(x, y)| IVec2::new(x, y)).collect())
    };
    let raised = [(1, 1), (2, 1), (3, 1), (1, 2), (2, 2), (3, 2)];
    let first = level(&[0; 24], &gids(&raised, 1), &on(1, 0));
    let files = files_with(&[("r/set.tsj", SET), ("r/m.tmj", &first)]);
    let mut maps = MapWorld::over(files.clone());
    let handle = load(&mut maps.world, "r/m.tmj");
    let map = maps.spawn(MapRoot::new(handle));
    // Each edge: its floor, where it stands and its cells
    let edges = |world: &mut World| -> Vec<(u8, Vec2, Vec<IVec2>)> {
      floor_edges(world, map)
        .into_iter()
        .map(|(floor, edge)| {
          (
            floor,
            place(world, edge),
            solid_cells(&colliders(world, edge)),
          )
        })
        .collect()
    };
    assert_eq!(edges(&mut maps.world), [(1, Vec2::ZERO, cells(&raised))]);
    let edge = edge_of(&mut maps.world, map, 1);

    // Saved with floor 1 a cell larger and stairs up to it on the ground:
    // the edge takes both in, in place
    let larger = [&raised[..], &[(4, 1)]].concat();
    let stairs = gids(&[(0, 1)], 3);
    files.insert("r/m.tmj", level(&stairs, &gids(&larger, 1), &on(1, 0)));
    let want = [(1, Vec2::ZERO, cells(&[&larger[..], &[(0, 1)]].concat()))];
    maps.frames_until(|world| edges(world) == want);
    assert_eq!(
      edge_of(&mut maps.world, map, 1),
      edge,
      "built again in place"
    );

    // Moved 16 px right: its layer respawns there, and the edge with it,
    // the ground's stairs no longer on its grid
    files.insert("r/m.tmj", level(&stairs, &gids(&larger, 1), &on(1, 16)));
    let want = [(1, Vec2::new(16.0, 0.0), cells(&larger))];
    maps.frames_until(|world| edges(world) == want);

    // Moved up to floor 2: floor 1's edge goes, and floor 2 has one, which
    // the ground's stairs up to floor 1 do not join
    files.insert("r/m.tmj", level(&stairs, &gids(&larger, 1), &on(2, 0)));
    let want = [(2, Vec2::ZERO, cells(&larger))];
    maps.frames_until(|world| edges(world) == want);
  }

  #[test]
  fn a_streamed_floor_s_edge_follows_its_chunks() {
    use crate::core::tilemap::{ChunkRequest, ChunkTiles, MapStream, StreamAnchor};
    /// Floor 1 raised over a 4 x 4 block of every chunk
    fn raise(_: ChunkRequest) -> ChunkTiles {
      let mut tiles = ChunkTiles::default();
      for y in 2..6 {
        for x in 2..6 {
          tiles.set("raised", UVec2::new(x, y), Some(PLAIN));
        }
      }
      tiles
    }
    let map = storeyed(
      None,
      &[("ground", 0, Vec2::ZERO, &[]), ("raised", 1, UP, &[])],
    );
    let mut maps = MapWorld::new();
    let handle = add(&mut maps.world, map);
    let stream = MapStream {
      cameras: false,
      ..MapStream::new(raise)
    };
    let map = maps.world.spawn((MapRoot::new(handle), stream)).id();
    maps.settle(map);
    let middle = |chunk: IVec2| (chunk.as_vec2() + 0.5) * 32.0 * 16.0;
    let anchor = maps
      .world
      .spawn((
        Transform::from_translation(middle(IVec2::ZERO)),
        StreamAnchor,
      ))
      .id();
    // The chunks the edge's parts start in, and the chunks resident, by row
    let state = |world: &mut World| -> (Vec<IVec2>, Vec<IVec2>) {
      let edge = edge_of(world, map, 1);
      let parts = parts_by_chunk(world, edge)
        .into_iter()
        .flat_map(|(_, chunks, _)| chunks)
        .collect();
      let layer = layers_of(world, map)[1];
      let resident = world
        .get::<TileLayerData>(layer)
        .map(|data| data.tiles.chunk_coords().collect())
        .unwrap_or_default();
      (by_row(parts), by_row(resident))
    };
    // The chunks round the anchor land, each with its part of the edge
    let around: Vec<IVec2> = by_row(
      (-1..=1)
        .flat_map(|y| (-1..=1).map(move |x| IVec2::new(x, y)))
        .collect(),
    );
    maps.frames_until(|world| {
      let (parts, resident) = state(world);
      resident == around && parts == resident
    });
    // Four chunks on, the chunks behind it unload and the chunks ahead
    // land, the edge's parts following them
    if let Some(mut at) = maps.world.get_mut::<Transform>(anchor) {
      at.translation = middle(IVec2::new(4, 0));
    }
    maps.frames_until(|world| {
      let (parts, resident) = state(world);
      resident.contains(&IVec2::new(5, 0)) && !resident.contains(&IVec2::ZERO) && parts == resident
    });
  }

  #[test]
  fn a_floor_s_edge_traces_on_every_grid() {
    // An L of floor 1 cells; its edge is the border of their union on the
    // grid, where floor 1's layers stand
    let cells: Vec<IVec2> = [(1, 1), (2, 1), (3, 1), (1, 2), (1, 3)]
      .map(|(x, y)| IVec2::new(x, y))
      .to_vec();
    let hex = |axis, index| GridShape::Hexagonal {
      axis,
      index,
      side: 12,
    };
    let staggered = |axis, index| GridShape::Staggered { axis, index };
    for (shape, cell) in [
      (GridShape::Orthogonal, UVec2::splat(16)),
      (GridShape::Isometric, UVec2::new(64, 32)),
      (
        staggered(StaggerAxis::Y, StaggerIndex::Odd),
        UVec2::new(64, 32),
      ),
      (
        staggered(StaggerAxis::X, StaggerIndex::Even),
        UVec2::new(64, 32),
      ),
      (hex(StaggerAxis::Y, StaggerIndex::Odd), UVec2::splat(32)),
      (hex(StaggerAxis::X, StaggerIndex::Even), UVec2::splat(32)),
      (
        GridShape::Oblique {
          skew: IVec2::new(8, 0),
        },
        UVec2::splat(32),
      ),
    ] {
      let label = format!("{shape:?}");
      let upper: Vec<(IVec2, u32)> = cells.iter().map(|&c| (c, 9)).collect();
      let mut map = lattice_map(
        shape,
        cell,
        Some(UVec2::new(6, 6)),
        &[("ground", &[]), ("upper", &upper)],
      );
      map.layers[1].info.floor = 1;
      map.layers[1].info.offset = Vec2::new(0.0, -(cell.y as f32));
      let grid = map.grid();
      let mut maps = MapWorld::new();
      let handle = add(&mut maps.world, map);
      let map = maps.spawn(MapRoot::new(handle));
      maps.frame();
      let edge = edge_of(&mut maps.world, map, 1);
      assert_eq!(colliders(&maps.world, edge).floor, Floor::On(1), "{label}");
      let layer = layers_of(&mut maps.world, map)[1];
      assert_eq!(
        place(&maps.world, edge),
        place(&maps.world, layer),
        "{label}"
      );
      // The edge's outline, and the border of the floor's cells, alike
      let outline = |world: &mut World, cells: &[IVec2]| match (grid.hex_cells(), grid.lattice()) {
        (Some(_), _) => {
          let mut edges: Vec<(Px, Px)> = all_edges(world, edge)
            .into_iter()
            .filter_map(|(a, b)| Some((whole_px(a)?, whole_px(b)?)))
            .collect();
          edges.sort_unstable();
          (edges, union_border(&grid, cells))
        }
        (None, Some(lattice)) => {
          // On the lattice's squares, in lattice points
          let squares: Vec<IVec2> = cells.iter().map(|&c| lattice.lattice_cell(c)).collect();
          let traced = cut(&lattice_edges(world, edge, lattice.to_pixel));
          (traced, square_border(&squares))
        }
        (None, None) => panic!("{label} has a lattice"),
      };
      let (traced, border) = outline(&mut maps.world, &cells);
      assert_eq!(traced, border, "{label}");
      // and after an edit filling the L's corner
      let corner = IVec2::new(2, 2);
      let tile = Some(Tile::new(0, 9));
      assert!(MapsView::new(&mut maps.world).set_tile(map, "upper", corner, tile));
      maps.step();
      let (traced, border) = outline(&mut maps.world, &[&cells[..], &[corner]].concat());
      assert_eq!(traced, border, "{label}: edited");
    }
  }

  #[test]
  fn a_map_without_floors_keeps_its_groups() {
    // A floor and an island sharing the map's grid, a shifted layer and an
    // icy one: three grids, on the map and on the shifted and icy layers
    let ground: Vec<(IVec2, u32)> = (0..40).map(|x| (IVec2::new(x, 3), SOLID)).collect();
    let mut map = code_map(
      UVec2::new(40, 4),
      &[
        ("ground", &ground),
        ("island", &[(IVec2::new(5, 1), SOLID)]),
        ("shifted", &[(IVec2::new(10, 0), SOLID)]),
        ("ice", &[(IVec2::new(20, 1), SOLID)]),
      ],
    );
    map.layers[2].info.offset = Vec2::new(4.0, -32.0);
    map.layers[3]
      .info
      .properties
      .insert("friction", PropertyValue::Float(0.0));
    let mut maps = MapWorld::new();
    let handle = add(&mut maps.world, map);
    let map = maps.spawn(MapRoot::new(handle));
    maps.frame();
    let layers = layers_of(&mut maps.world, map);
    assert!(
      maps
        .world
        .get::<crate::core::tilemap::MapFloors>(map)
        .is_none()
    );
    assert!(floor_edges(&mut maps.world, map).is_empty());
    assert_eq!(maps.world.query::<&MapFloor>().iter(&maps.world).count(), 0);
    let mut holders: Vec<Entity> = maps
      .world
      .query::<(Entity, &TileColliders)>()
      .iter(&maps.world)
      .map(|(entity, _)| entity)
      .collect();
    holders.sort();
    let mut want = vec![map, layers[2], layers[3]];
    want.sort();
    assert_eq!(holders, want);
    let shared = colliders(&maps.world, map);
    let mut floor = vec![IVec2::new(5, 1)];
    floor.extend((0..40).map(|x| IVec2::new(x, 3)));
    assert_eq!(solid_cells(&shared), floor);
    assert_eq!(
      (shared.friction, place(&maps.world, map)),
      (0.5, Vec2::ZERO)
    );
    let shifted = colliders(&maps.world, layers[2]);
    assert_eq!(solid_cells(&shifted), [IVec2::new(10, 0)]);
    assert_eq!(place(&maps.world, layers[2]), Vec2::new(4.0, -32.0));
    let ice = colliders(&maps.world, layers[3]);
    assert_eq!(solid_cells(&ice), [IVec2::new(20, 1)]);
    assert_eq!(ice.friction, 0.0);
    for tiles in [&shared, &shifted, &ice] {
      assert_eq!(tiles.floor, Floor::GROUND);
    }
    // A part for each grid's loops starting in a chunk, none on a floor
    let parts: Vec<Entity> = maps
      .world
      .query_filtered::<Entity, crate::core::ecs::With<TilePart>>()
      .iter(&maps.world)
      .collect();
    assert_eq!(parts.len(), 3);
    for part in parts {
      assert!(maps.world.get::<Floor>(part).is_none());
    }
  }

  #[test]
  fn a_discarded_map_takes_its_floor_edges_with_it() {
    let raised = block((2, 1), (4, 2), PLAIN);
    let mut maps = MapWorld::new();
    let handle = add(
      &mut maps.world,
      storeyed(
        Some(UVec2::new(8, 4)),
        &[("ground", 0, Vec2::ZERO, &[]), ("raised", 1, UP, &raised)],
      ),
    );
    let edges = |world: &mut World| world.query::<&MapFloor>().iter(world).count();
    let map = maps.spawn(MapRoot::new(handle.clone()));
    assert_eq!(edges(&mut maps.world), 1);
    assert!(count_parts(&mut maps.world) > 0);

    // Disabled, its edge goes with its colliders; enabled, it is back
    maps.world.entity_mut(map).insert(Disabled);
    maps.frame();
    assert_eq!(edges(&mut maps.world), 0);
    assert_eq!(count_parts(&mut maps.world), 0);
    maps.world.entity_mut(map).remove::<Disabled>();
    maps.frame();
    assert_eq!(edges(&mut maps.world), 1);
    assert!(count_parts(&mut maps.world) > 0);

    // Put over by another root landing before any fixed step: the old edge
    // goes, and the new map's comes
    maps
      .world
      .entity_mut(map)
      .insert(MapRoot::new(handle.clone()));
    assert!(!MapsView::new(&mut maps.world).is_ready(map), "taken down");
    let start = std::time::Instant::now();
    while !MapsView::new(&mut maps.world).is_ready(map) {
      maps.pre_update();
      assert!(start.elapsed().as_secs() < 5, "{map} never landed again");
      std::thread::yield_now();
    }
    assert_eq!(floor_edges(&mut maps.world, map).len(), 1);
    assert_eq!(edges(&mut maps.world), 1);

    // Its root taken away
    maps.world.entity_mut(map).remove::<MapRoot>();
    maps.frame();
    maps.frame();
    assert_eq!(edges(&mut maps.world), 0);
    assert_eq!(count_parts(&mut maps.world), 0);

    // Despawned
    let map = maps.spawn(MapRoot::new(handle));
    assert_eq!(edges(&mut maps.world), 1);
    maps.world.despawn(map);
    maps.frame();
    assert_eq!(edges(&mut maps.world), 0);
    assert_eq!(count_parts(&mut maps.world), 0);
  }
}
