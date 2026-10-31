//! Map collision: a spawned map's colliding tile layers become
//! `TileColliders`, kept in step with their tiles.
//!
//! A tile layer collides when its `LayerInfo::collides` is set and its
//! root has not turned collision off. Colliding layers are grouped by their
//! cell size, offset and collision material (`collision_layer`,
//! `collision_mask`, `friction`, `restitution`: a layer's properties over
//! its map's): the group of the map's own tile size, no offset and the
//! map's material unions into one `TileColliders` on the map entity, and
//! any other group (an LDtk IntGrid of 8 px cells under 16 px art, a layer
//! shifted by an offset, an ice layer) gets its own on its first layer's
//! entity, whose `Transform` carries the offset. A cell takes the
//! strongest kind across its group: `Solid`, then `Shape`, then `Slope`,
//! then `OneWay`; of two shapes (or two slopes) the upper layer's wins.
//! What a cell is comes from `kinds.rs`: an IntGrid layer's values by name,
//! any other layer's tiles' collision shapes placed where they draw.
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
//! Each step, every chunk whose version moved since the group last looked
//! is classified again and only the cells whose kind changed are set, so
//! the tile colliders rebuild only the outlines an edit touched. A change
//! of what the colliders are built from (which layers collide, their cell
//! sizes, offsets, materials or sizes, the tilesets' content, the IntGrid
//! overrides) builds every group again.

use std::sync::Arc;

use glam::{IVec2, UVec2, Vec2};

use super::kinds::{
  Context, Layers, Purpose, ShapeKey, ShapeSink, ShapeSource, Stores, TilesetStamp, Tracked,
  changed_chunks, layer_data, layer_kind, over, shapes_kept, sources, stamps,
};
use super::spawn::{IntGridKey, MapLayers, MapRoot, TileLayerData, root_map};
use super::{CHUNK_SIZE, LayerInfo, Properties, PropertyValue, Tile};
use crate::core::ecs::{
  Allow, Commands, Component, DetectChangesMut, Disabled, Entity, Has, Local, Query, Without, World,
};
use crate::core::hash::{FastMap, FastSet};
use crate::core::physics::{CellShape, TileCell, TileColliders, Traced};

/// The most cells a `TileColliders` holds a side
const MAX_SIDE: u32 = 8192;

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

/// What a group of layers shares: a cell size, an offset and a material
/// (floats by their bits)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct GroupKey {
  cell: UVec2,
  offset: [u32; 2],
  material: [u32; 4],
}

impl GroupKey {
  fn of(cell: UVec2, offset: Vec2, material: Material) -> GroupKey {
    GroupKey {
      cell,
      offset: [offset.x.to_bits(), offset.y.to_bits()],
      material: [
        material.layer,
        material.mask,
        material.friction.to_bits(),
        material.restitution.to_bits(),
      ],
    }
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

/// A colliding layer as the colliders were built from it: its place among
/// the map's layers and its entity (a stand-in until the layers spawn)
#[derive(Clone, PartialEq, Debug)]
pub(crate) struct Planned {
  entity: Entity,
  index: usize,
  key: GroupKey,
  size: UVec2,
  intgrid: bool,
}

/// Layers sharing a cell size, offset and material, and the grid they
/// collide in
#[derive(Debug)]
struct Group {
  key: GroupKey,
  /// Where its `TileColliders` is: the map, or its first layer
  target: Entity,
  target_layer: Option<usize>,
  size: UVec2,
  /// Over the most a `TileColliders` holds: it has none
  too_large: bool,
  layers: Vec<Tracked>,
  /// Each tile's kind, its shapes numbered in this group's grid
  kinds: FastMap<Tile, TileCell>,
  shapes: FastMap<ShapeKey, u16>,
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

/// The colliding layers among `layers` into `planned`, and the map's own
/// group key. A property of the wrong type warns once, and so does a
/// colliding layer with parallax, which collides where it would draw
/// without it, parallax being a drawing effect (`warned`)
fn plan<'a>(
  map: Entity,
  layers: impl IntoIterator<Item = LayerView<'a>>,
  (collision, properties, tile_size): (bool, &Properties, UVec2),
  warned: &mut Warned,
  planned: &mut Vec<Planned>,
) -> GroupKey {
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
  if collision {
    for layer in layers.into_iter().filter(|l| l.info.collides) {
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
      planned.push(Planned {
        entity: layer.entity,
        index: layer.index,
        key: GroupKey::of(layer.data.cell_size, layer.info.offset, material),
        size: layer.data.grid.size(),
        intgrid: layer.data.intgrid.is_some(),
      });
    }
  }
  GroupKey::of(tile_size, Vec2::ZERO, map_material)
}

/// What a build of a map's groups makes: the groups, the grid of each group
/// that has one (by its index), traced when built off the main thread, and
/// the layers (the first of each group too large) warned about
struct Built {
  groups: Vec<Group>,
  grids: Vec<(usize, TileColliders, Option<Traced>)>,
  warned_layers: FastSet<usize>,
}

/// Every group of a map's colliding layers (`planned`, read through
/// `views`), built from their tiles as they are, with the grid each puts on
/// its target (none for a group too large), traced if `trace`
fn build(
  map: Entity,
  (map_key, planned, views): (GroupKey, &[Planned], &[LayerView<'_>]),
  context: &Context<'_>,
  mut warned_layers: FastSet<usize>,
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
        group.size = group.size.max(layer.size);
        group.layers.push(tracked);
      }
      None => {
        let own = layer.key != map_key;
        groups.push(Group {
          key: layer.key,
          target: if own { layer.entity } else { map },
          target_layer: own.then_some(layer.index),
          size: layer.size,
          too_large: false,
          layers: vec![tracked],
          kinds: FastMap::default(),
          shapes: FastMap::default(),
        });
      }
    }
  }
  let mut grids = Vec::new();
  for (index, group) in groups.iter_mut().enumerate() {
    let size = group.size;
    if size.x > MAX_SIDE || size.y > MAX_SIDE {
      group.too_large = true;
      let first = group.layers.first().map(|t| t.index);
      if first.is_some_and(|first| warned_layers.insert(first)) {
        let names: Vec<String> = group
          .layers
          .iter()
          .filter_map(|t| view(t.index))
          .map(|v| format!("\"{}\"", v.info.name))
          .collect();
        let named = if names.len() == 1 { "layer" } else { "layers" };
        crate::engine_warn!(
          module: "Tilemap",
          "map {map}: {named} {} would collide in a grid of {} x {} cells, over the {MAX_SIDE} a side tile colliders hold, so nothing there collides",
          names.join(", "),
          size.x,
          size.y
        );
      }
      continue;
    }
    let mut cells = vec![TileCell::Empty; size.x as usize * size.y as usize];
    let mut pending = Pending(Vec::new());
    let Group {
      layers: tracked,
      kinds,
      shapes,
      ..
    } = group;
    for layer in tracked.iter() {
      let Some(view) = view(layer.index) else {
        continue;
      };
      let reach = view.data.grid.size().min(size);
      for y in 0..reach.y {
        for x in 0..reach.x {
          let cell = IVec2::new(x as i32, y as i32);
          let kind = layer_kind(
            layer,
            view.data,
            cell,
            (kinds, shapes),
            &mut pending,
            context,
          );
          let slot = &mut cells[y as usize * size.x as usize + x as usize];
          *slot = over(*slot, kind);
        }
      }
    }
    let mut tiles = TileColliders::from_cells(size.x, size.y, group.key.cell.as_vec2(), cells);
    group.key.material().apply(&mut tiles);
    for shape in pending.0 {
      tiles.add_shape(shape);
    }
    let traced = if trace { tiles.trace() } else { None };
    grids.push((index, tiles, traced));
  }
  Built {
    groups,
    grids,
    warned_layers,
  }
}

/// What a map's first build reads besides its layers: its root's settings,
/// its own properties and tile size, and its tilesets' shapes and stamps
pub(crate) struct Inputs {
  pub(crate) collision: bool,
  pub(crate) overrides: Vec<(IntGridKey, TileCell)>,
  pub(crate) properties: Properties,
  pub(crate) tile_size: UVec2,
  pub(crate) shapes: Arc<[Option<ShapeSource>]>,
  pub(crate) tilesets: Vec<TilesetStamp>,
}

/// A map's collision made off the main thread, to land with its layers
pub(crate) struct Prepared {
  state: MapCollision,
  grids: Vec<(usize, TileColliders, Option<Traced>)>,
}

/// The collision of the map `map` as its layers (each with its place among
/// them) will spawn: every grid classified and traced. Pure: any thread
pub(crate) fn prepare(map: Entity, layers: &[LayerView<'_>], inputs: Inputs) -> Prepared {
  let mut warned = Warned::default();
  let mut planned = Vec::new();
  let map_key = plan(
    map,
    layers.iter().copied(),
    (inputs.collision, &inputs.properties, inputs.tile_size),
    &mut warned,
    &mut planned,
  );
  let context = Context {
    shapes: &inputs.shapes,
    overrides: &inputs.overrides,
    purpose: Purpose::Collision,
  };
  let built = build(
    map,
    (map_key, &planned, layers),
    &context,
    FastSet::default(),
    true,
  );
  Prepared {
    state: MapCollision {
      layers: planned,
      tile_size: inputs.tile_size,
      tilesets: inputs.tilesets,
      overrides: inputs.overrides,
      shapes: inputs.shapes,
      groups: built.groups,
      torn: false,
      warned_layers: built.warned_layers,
      warned_properties: warned,
    },
    grids: built.grids,
  }
}

/// Puts a map's prepared collision on it and its layers (`layers`, the
/// entities its layers spawned as, by place), as the map lands
pub(crate) fn land(world: &mut World, map: Entity, layers: &[Entity], prepared: Prepared) {
  let Prepared { mut state, grids } = prepared;
  for planned in &mut state.layers {
    if let Some(&entity) = layers.get(planned.index) {
      planned.entity = entity;
    }
  }
  for group in &mut state.groups {
    group.target = group
      .target_layer
      .and_then(|index| layers.get(index).copied())
      .unwrap_or(map);
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

/// A map's collision: what it was built from, its groups, and what it has
/// warned about. On the map entity
#[derive(Component, Debug)]
pub(crate) struct MapCollision {
  layers: Vec<Planned>,
  tile_size: UVec2,
  tilesets: Vec<TilesetStamp>,
  overrides: Vec<(IntGridKey, TileCell)>,
  /// The tilesets' shapes it reads
  shapes: Arc<[Option<ShapeSource>]>,
  groups: Vec<Group>,
  /// Its colliders were taken away (the map disabled, or one went missing):
  /// the next step builds them all
  torn: bool,
  warned_layers: FastSet<usize>,
  warned_properties: Warned,
}

impl MapCollision {
  /// The entities holding a `TileColliders` of this map's
  fn targets(&self) -> impl Iterator<Item = Entity> + '_ {
    self
      .groups
      .iter()
      .filter(|g| !g.too_large)
      .map(|g| g.target)
  }

  /// Takes every `TileColliders` this map put on
  fn tear_down(&mut self, commands: &mut Commands) {
    for target in self.targets() {
      commands.entity(target).try_remove::<TileColliders>();
    }
    self.torn = true;
  }
}

/// Brings a group's grid up to its layers' tiles: the cells of every chunk
/// whose version moved since it last looked, setting only those whose kind
/// changed
fn update(group: &mut Group, tiles: &mut TileColliders, layers: &Layers, context: &Context<'_>) {
  let changed = changed_chunks(&mut group.layers, layers, context.overrides);
  if changed.is_empty() {
    return;
  }
  let data = layer_data(&group.layers, layers);
  let Group {
    layers: tracked,
    kinds,
    shapes,
    size,
    ..
  } = group;
  for chunk in changed {
    let min = chunk * CHUNK_SIZE;
    let max = (min + UVec2::splat(CHUNK_SIZE)).min(*size);
    for y in min.y..max.y {
      for x in min.x..max.x {
        let cell = IVec2::new(x as i32, y as i32);
        let mut kind = TileCell::Empty;
        for (layer, data) in tracked.iter().zip(&data) {
          if let Some(data) = data {
            let below = kind;
            kind = over(
              below,
              layer_kind(layer, data, cell, (kinds, shapes), tiles, context),
            );
          }
        }
        if tiles.get(x, y) != kind {
          tiles.set(x, y, kind);
        }
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
) -> impl Iterator<Item = LayerView<'a>> + 'a {
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
  for (map, root, map_layers, mut state, disabled) in &mut maps {
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
      (root.collision, &tile_map.properties, tile_map.tile_size),
      warned,
      &mut planned,
    );
    let same = state.as_mut().is_some_and(|s| {
      let s = s.bypass_change_detection();
      !s.torn
        && s.layers == *planned
        && s.tile_size == tile_map.tile_size
        && s.overrides == root.intgrid_collision
        && shapes_kept(&mut s.tilesets, &s.shapes, tile_map, tilesets)
    });
    match state {
      Some(mut state) if same => {
        let state = state.bypass_change_detection();
        let context = Context {
          shapes: &state.shapes,
          overrides: &root.intgrid_collision,
          purpose: Purpose::Collision,
        };
        for group in &mut state.groups {
          if group.too_large {
            continue;
          }
          let Ok(mut tiles) = grids.get_mut(group.target) else {
            // Taken away by something else: all of it is built next step
            state.torn = true;
            continue;
          };
          update(group, &mut tiles, &layers, &context);
        }
      }
      state => {
        // Built at once, so a map never lacks its colliders
        let shapes = sources(tile_map, tilesets);
        let (warned_layers, warned_properties) = match &state {
          Some(old) => (old.warned_layers.clone(), old.warned_properties.clone()),
          None => (FastSet::default(), first_warnings),
        };
        if let Some(old) = &state {
          for target in old.targets() {
            commands.entity(target).try_remove::<TileColliders>();
          }
        }
        let context = Context {
          shapes: &shapes,
          overrides: &root.intgrid_collision,
          purpose: Purpose::Collision,
        };
        let views: Vec<LayerView<'_>> = views(map_layers, &layers).collect();
        let built = build(
          map,
          (map_key, &planned, &views),
          &context,
          warned_layers,
          false,
        );
        for (group, tiles, _) in built.grids {
          commands
            .entity(built.groups[group].target)
            .try_insert(tiles);
        }
        let fresh = MapCollision {
          layers: planned.clone(),
          tile_size: tile_map.tile_size,
          tilesets: stamps(tile_map, tilesets),
          overrides: root.intgrid_collision.clone(),
          shapes: Arc::clone(&shapes),
          groups: built.groups,
          torn: false,
          warned_layers: built.warned_layers,
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
  use crate::core::ecs::{ChildOf, Disabled, Entity, World};
  use crate::core::engine::EngineBuilder;
  use crate::core::gameplay::{MoveIntent, PlatformerMovement};
  use crate::core::physics::CharacterState;
  use crate::core::physics::{
    Collider, ColliderShape, SlopeCorner, TileCell, TileColliders, TilePart,
  };
  use crate::core::scene::{Scene, SceneContext};
  use crate::core::testing::HeadlessEngine;
  use crate::core::tilemap::test_util::{MapWorld, files_with, layers_of, load_map as load};
  use crate::core::tilemap::{
    IntGrid, LayerKind, MapRoot, MapsView, PropertyValue, Tile, TileData, TileFlip, TileMap,
    TileShape, Tileset, TilesetImage, TilesetSource,
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

  /// The parts under `parent`, by entity
  fn parts_of(world: &mut World, parent: Entity) -> Vec<(Entity, Collider)> {
    let mut found: Vec<(Entity, Collider)> = world
      .query::<(Entity, &ChildOf, &Collider)>()
      .iter(world)
      .filter(|(_, child_of, _)| child_of.parent() == parent)
      .map(|(e, _, c)| (e, c.clone()))
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
    assert_eq!((tiles.width(), tiles.height()), (24, 12));
    assert_eq!(tiles.tile_size, Vec2::splat(16.0));
    // The floor is two layers' tiles, in one grid
    assert!((0..24).all(|x| tiles.get(x, 11) == TileCell::Solid));
    // One slope tile, as drawn, mirrored and turned
    assert_eq!(tiles.get(20, 10), TileCell::Slope(BottomRight));
    assert_eq!(tiles.get(21, 10), TileCell::Slope(BottomLeft), "mirrored");
    assert_eq!(tiles.get(22, 10), TileCell::Slope(TopRight), "upside down");
    assert_eq!(tiles.get(23, 10), TileCell::Slope(BottomLeft), "turned");
    assert!((4..8).all(|x| tiles.get(x, 7) == TileCell::OneWay));
    let TileCell::Shape(half) = tiles.get(18, 10) else {
      panic!("the half block is {:?}", tiles.get(18, 10))
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
    assert_eq!(tiles.get(14, 10), TileCell::Empty, "decor does not collide");
    assert_eq!(
      tiles.get(10, 5),
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
        tiles.get(x as u32, 0),
        TileCell::Slope(*corner),
        "flip {flip:?}"
      );
    }
    let TileCell::Shape(raised) = tiles.get(0, 1) else {
      panic!("the raised half block is {:?}", tiles.get(0, 1))
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
      tiles.get(0, 0),
      TileCell::Solid,
      "the quarter over its cell"
    );
    // the other tile's square is right of its cell: nothing of it is inside
    match tiles.get(1, 0) {
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
    let row: Vec<TileCell> = (0..8).map(|x| tiles.get(x, 0)).collect();
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
        (0..tiles.height())
          .flat_map(|y| (0..tiles.width()).map(move |x| (x, y)))
          .filter(|&(x, y)| tiles.get(x, y) == TileCell::Solid)
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
    assert_eq!(colliders(&maps.world, loud).get(3, 0), TileCell::Solid);
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
    assert_eq!(tiles.get(10, 1), TileCell::Solid);
    assert_eq!(tiles.get(39, 3), TileCell::Empty);
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
    assert!(shape_of(tiles.get(0, 0)).is_some_and(|p| same_points(&p, &bottom_half)));
    assert_eq!(tiles.get(1, 0), TileCell::Solid);
    assert_eq!(tiles.get(2, 0), TileCell::Slope(SlopeCorner::BottomRight));
    assert_eq!(tiles.get(4, 0), TileCell::OneWay);
    // Of two shapes, the upper layer's
    assert!(shape_of(tiles.get(3, 0)).is_some_and(|p| same_points(&p, &top_half)));
    // and an edit to the layer below changes nothing the one above covers
    assert!(MapsView::new(&mut maps.world).set_tile(map, "below", at(3), None));
    maps.step();
    assert!(
      shape_of(colliders(&maps.world, map).get(3, 0)).is_some_and(|p| same_points(&p, &top_half))
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
      grid: crate::core::tilemap::TileGrid::new(UVec2::new(8, 4)),
      intgrid: Some(IntGrid {
        size: UVec2::new(8, 4),
        values: (0..32).map(|i| i32::from(i == 31)).collect(),
        names: [(1, "solid".to_string())].into_iter().collect(),
      }),
      cell_size: UVec2::splat(8),
    };
    let mut maps = MapWorld::new();
    let handle = add(&mut maps.world, map);
    let map = maps.spawn(MapRoot::new(handle));
    let layers = layers_of(&mut maps.world, map);
    // The two layers sharing the map's cells and no offset are one grid
    let shared = colliders(&maps.world, map);
    assert_eq!((shared.width(), shared.height()), (4, 2));
    assert_eq!(shared.get(0, 1), TileCell::Solid);
    assert_eq!(shared.get(1, 1), TileCell::Solid);
    assert_eq!(shared.get(3, 0), TileCell::Empty);
    for layer in &layers[..2] {
      assert!(maps.world.get::<TileColliders>(*layer).is_none());
    }
    let shifted = colliders(&maps.world, layers[2]);
    assert_eq!(shifted.get(3, 0), TileCell::Solid);
    assert_eq!(shifted.tile_size, Vec2::splat(16.0));
    let fine = colliders(&maps.world, layers[3]);
    assert_eq!(
      (fine.width(), fine.height(), fine.tile_size),
      (8, 4, Vec2::splat(8.0))
    );
    assert_eq!(fine.get(7, 3), TileCell::Solid);
    assert_eq!(fine.get(0, 0), TileCell::Empty);
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
    assert_eq!(colliders(&maps.world, shifted).get(0, 0), TileCell::Solid);
    // The shifted layer's two islands, in one chunk, are one collider
    let parts = parts_of(&mut maps.world, shifted);
    assert!(
      matches!(&parts[..], [(_, c)] if matches!(&c.shape, ColliderShape::Segments { segments, .. } if segments.len() == 8)),
      "{parts:?}"
    );
  }

  #[test]
  fn a_colliding_layer_too_wide_for_tile_colliders_warns_once_naming_it() {
    let mut map = code_map(
      UVec2::new(8193, 1),
      &[("too-wide-7c1e", &[(IVec2::ZERO, SOLID)])],
    );
    // and a layer within the limit, offset onto its own grid, still collides
    let small = map.add_tile_layer("small");
    map.layers[small].kind = LayerKind::Tiles {
      grid: {
        let mut grid = crate::core::tilemap::TileGrid::new(UVec2::new(2, 1));
        grid.set(IVec2::ZERO, Some(Tile::new(0, SOLID)));
        grid
      },
      intgrid: None,
      cell_size: UVec2::splat(16),
    };
    map.layers[small].info.offset = Vec2::new(0.0, 32.0);
    let mut maps = MapWorld::new();
    let handle = add(&mut maps.world, map);
    let map = maps.spawn(MapRoot::new(handle));
    maps.frame();
    maps.frame();
    assert!(maps.world.get::<TileColliders>(map).is_none());
    let warned = crate::core::dev_log::get_entries()
      .iter()
      .filter(|e| e.message.contains("too-wide-7c1e"))
      .count();
    assert_eq!(warned, 1, "warned once, naming the layer");
    let small = layers_of(&mut maps.world, map)[1];
    assert_eq!(colliders(&maps.world, small).get(0, 0), TileCell::Solid);
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
    assert_eq!(tiles.get(0, 0), TileCell::Solid);
    assert_eq!(tiles.get(2, 0), TileCell::Empty);
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
    assert_eq!(shared.get(0, 0), TileCell::Solid);
    assert_eq!(
      shared.get(2, 0),
      TileCell::Solid,
      "a layer saying what the map says"
    );
    assert_eq!(shared.get(1, 0), TileCell::Empty);
    let ice = colliders(&maps.world, layers[1]);
    assert_eq!((ice.friction, ice.layer, ice.mask), (0.0, 8, u32::MAX));
    assert_eq!(ice.get(1, 0), TileCell::Solid);
    assert_eq!(ice.get(0, 0), TileCell::Empty);
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
      data.grid = grid;
    }
    maps.step();
    let tiles = colliders(&maps.world, map);
    assert_eq!(tiles.get(7, 1), TileCell::Solid);
    assert_eq!(
      tiles.get(36, 0),
      TileCell::Empty,
      "the island went with the old grid"
    );
    assert_eq!(tiles.get(39, 3), TileCell::Solid);
  }

  #[test]
  fn a_grid_taken_away_is_built_again() {
    let mut maps = MapWorld::new();
    let map = floor_and_island(&mut maps);
    maps.world.entity_mut(map).remove::<TileColliders>();
    maps.step();
    maps.step();
    let tiles = colliders(&maps.world, map);
    assert_eq!(tiles.get(36, 0), TileCell::Solid);
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
    assert_eq!(colliders(&maps.world, map).get(0, 0), TileCell::Solid);
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
    assert!((0..4).all(|x| matches!(tiles.get(x, 0), TileCell::Shape(_))));
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
}
