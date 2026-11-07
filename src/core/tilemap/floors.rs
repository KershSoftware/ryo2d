//! Storeys: a map's layers on floors above the ground (raised ground, upper
//! floors, bridges), each floor as high as Tiled draws it.
//!
//! A layer's floor is its `LayerInfo::floor`. A floor's height (its lift)
//! is the vertical part of the offset of its bottom layer, the first in
//! draw order on it; the ground floor's is zero. A layer above the ground
//! floor stands in ground space, at its offset less its floor's height, so
//! its objects, class tiles, colliders and shadows sit where characters
//! stand, and drawing is to add the height back. Each floor above the
//! ground collides on grids of its own, and has an edge ([`MapFloor`]) that
//! keeps what is on it from walking off.
//!
//! An entity on a floor stands in ground space too: [`lift_floored`]
//! gives it, and everything below it in the hierarchy, its floor's height
//! on the map under it ([`DrawLift`]), which the draw sites add; a climber
//! on stairs, a height eased between its stairs' two floors (`stairs.rs`).

use glam::{Affine2, Vec2};

use super::LayerInfo;
use super::spawn::{MapLayers, TileLayerData};
use super::stairs::{Climber, Stairs};
use crate::core::ecs::interpolate::render_global;
use crate::core::ecs::{
  Allow, Changed, ChildOf, Children, Commands, Component, DetectChangesMut, Disabled, DrawLift,
  Entity, Floor, GlobalTransform, Local, Or, ParentPlaced, PreviousGlobalTransform, Query,
  RemovedComponents, Res, Transform, With, Without, entity_order,
};
use crate::core::time::Time;

/// A map's floors and their heights, on its [`MapRoot`](super::MapRoot)
/// entity when some layer is above the ground floor (a map without one has
/// none). A floor's height is how far up Tiled draws it, in map pixels:
/// the vertical offset of its bottom layer. Read it in the world, through
/// the map's transform, with [`MapsView::lift`](super::MapsView::lift).
///
/// ```
/// use ryo2d::prelude::*;
///
/// /// How high the map's first storey is, in its own pixels
/// fn first_storey(ctx: &mut SceneContext<'_>, map: Entity) -> Option<Vec2> {
///   ctx.maps().floors(map)?.lift(Floor::On(1))
/// }
/// ```
#[derive(Component, Clone, Debug, PartialEq)]
pub struct MapFloors {
  /// Each floor's height by its number, `None` for a floor no layer is on;
  /// the ground floor's is zero
  lifts: Vec<Option<Vec2>>,
}

impl MapFloors {
  /// The floors of `layers` (bottom first, as they draw); `None` when none
  /// is above the ground floor
  pub(crate) fn of<'a>(layers: impl IntoIterator<Item = &'a LayerInfo>) -> Option<MapFloors> {
    let mut lifts = vec![Some(Vec2::ZERO)];
    for info in layers {
      let floor = usize::from(info.floor);
      if floor == 0 {
        continue;
      }
      if lifts.len() <= floor {
        lifts.resize(floor + 1, None);
      }
      lifts[floor].get_or_insert(Vec2::new(0.0, info.offset.y));
    }
    (lifts.len() > 1).then_some(MapFloors { lifts })
  }

  /// The floor's height in map pixels, what it is drawn lifted by: `GROUND`
  /// gives `Some(Vec2::ZERO)`; a floor no layer is on, or `Every`, gives
  /// `None`
  pub fn lift(&self, floor: Floor) -> Option<Vec2> {
    let number = usize::from(floor.number()?);
    self.lifts.get(number).copied().flatten()
  }

  /// The floors some layer is on, ascending, the ground floor first
  pub fn floors(&self) -> impl Iterator<Item = Floor> + '_ {
    self
      .lifts
      .iter()
      .enumerate()
      .filter(|(_, lift)| lift.is_some())
      .map(|(number, _)| Floor::On(u8::try_from(number).unwrap_or(u8::MAX)))
  }

  /// The highest floor a layer is on
  pub fn highest(&self) -> Floor {
    self.floors().last().unwrap_or(Floor::GROUND)
  }
}

/// A child of the map in ground space holding one higher floor's edge: the
/// floor's footprint traced as a grid of its own (`TileColliders`, floor On(floor)).
///
/// The footprint is the cells the floor's tile layers fill, and the stairs
/// up to it on lower floors' layers, where the floor's bottom tile layer
/// stands. Its outline keeps what is on that floor on it: a character there
/// walks inside it, and cannot walk off into the air. The map keeps it in
/// step with its tiles, and takes it away with its colliders.
///
/// An edit keeps the entity, but every full rebuild of the map's collision
/// replaces it: the map enabled again, a change of its layers, their
/// places, floors or materials (a reload), a tileset saved with other
/// shapes or stairs.
/// Don't keep the entity: find it again (a `MapFloor` whose `ChildOf` is
/// the map) when it is needed, or after `MapEvent::Reloaded`.
///
/// ```
/// use ryo2d::prelude::*;
///
/// /// Whether a tile collider part a character met is a raised floor's
/// /// edge, rather than a wall
/// fn is_floor_edge(world: &World, part: Entity) -> bool {
///   world
///     .get::<ChildOf>(part)
///     .is_some_and(|parent| world.get::<MapFloor>(parent.parent()).is_some())
/// }
/// ```
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapFloor {
  /// The floor whose edge it holds, 1 and up
  pub floor: u8,
}

/// Where the layer `info` stands in its map: its offset, less its floor's
/// height above the ground floor (in ground space)
pub(crate) fn ground_offset(info: &LayerInfo, floors: Option<&MapFloors>) -> Vec2 {
  match floors.and_then(|floors| floors.lift(Floor::On(info.floor))) {
    Some(lift) if info.floor > 0 => info.offset - lift,
    _ => info.offset,
  }
}

/// `floor`'s height on a map (`floors`, `None` for a map without floors)
/// as a world vector through `to_world`, the map's transform: turned and
/// scaled with the map, where it stands adding nothing. `GROUND` gives
/// zero on any map; a floor no layer is on, or `Every`, gives `None`
pub(crate) fn world_lift(
  floors: Option<&MapFloors>,
  floor: Floor,
  to_world: Affine2,
) -> Option<Vec2> {
  let lift = match floors {
    Some(floors) => floors.lift(floor)?,
    None if floor == Floor::GROUND => Vec2::ZERO,
    None => return None,
  };
  Some(to_world.transform_vector2(lift))
}

/// How deep `lift_floored` looks up a hierarchy for an ancestor on a floor;
/// a deeper chain is taken for a cycle
const MAX_DEPTH: usize = 256;

/// A map with floors, for finding the map under an entity: the box its tile
/// layers' cells cover, in its pixels
pub(super) struct MapArea {
  pub(super) map: Entity,
  /// The map's pixels to the world, and back
  to_world: Affine2,
  to_map: Affine2,
  min: Vec2,
  max: Vec2,
}

impl MapArea {
  /// The area of `map`, placed by `global`, over its `layers`; `None` for a
  /// map scaled to nothing or with no cells
  fn of(
    map: Entity,
    global: &GlobalTransform,
    layers: &MapLayers,
    tiles: &TileLayers,
  ) -> Option<MapArea> {
    let to_world = global.affine();
    if to_world.matrix2.determinant() == 0.0 {
      return None;
    }
    let mut area: Option<(Vec2, Vec2)> = None;
    for &layer in &layers.0 {
      let Ok((transform, data)) = tiles.get(layer) else {
        continue;
      };
      let cells = data.tiles.extent();
      if cells.is_empty() {
        continue;
      }
      // The layer's box of cells in the map's pixels, through its transform
      let bounds = data.grid.bounds_of(cells);
      let to_map = transform.to_affine();
      let (x0, x1) = (bounds.x, bounds.x + bounds.width);
      let (y0, y1) = (bounds.y, bounds.y + bounds.height);
      for corner in [(x0, y0), (x1, y0), (x0, y1), (x1, y1)] {
        let at = to_map.transform_point2(Vec2::from(corner));
        area = Some(match area {
          Some((min, max)) => (min.min(at), max.max(at)),
          None => (at, at),
        });
      }
    }
    let (min, max) = area?;
    Some(MapArea {
      map,
      to_world,
      to_map: to_world.inverse(),
      min,
      max,
    })
  }

  /// Whether the world point `at` lies over the map's cells
  pub(super) fn holds(&self, at: Vec2) -> bool {
    let local = self.local(at);
    local.cmpge(self.min).all() && local.cmple(self.max).all()
  }

  /// The world point `at` in the map's pixels
  pub(super) fn local(&self, at: Vec2) -> Vec2 {
    self.to_map.transform_point2(at)
  }
}

/// The areas of the maps with floors (`maps`) into `areas`, in entity
/// order, for finding the map under an entity: the first that holds it
pub(super) fn find_areas(areas: &mut Vec<MapArea>, maps: &Maps, tiles: &TileLayers) {
  areas.clear();
  areas.extend(
    maps
      .iter()
      .filter_map(|(map, _, global, layers)| MapArea::of(map, global, layers, tiles)),
  );
  areas.sort_unstable_by_key(|area| entity_order(&area.map));
}

/// What `lift_floored` keeps between runs, so a warm run allocates nothing
/// (each list empty at the end of a run but `areas`)
#[derive(Default)]
pub(crate) struct Lifting {
  /// The maps with floors, in entity order
  areas: Vec<MapArea>,
  /// The entities on a floor whose subtrees are lifted again this run
  seeds: Vec<Entity>,
  /// Lifted entities that lost their floor or their place below one
  loose: Vec<Entity>,
  /// The walk down a subtree
  stack: Vec<Entity>,
}

/// An entity on a floor and where it stands. A map's own collider parts
/// and shadow chains are drawn by their layers, and stay out of the scan.
/// Every query of the lift sees disabled entities too, so a pool's or a
/// put-away entity's lift keeps step with its hierarchy
type Floored<'w, 's> = Query<
  'w,
  's,
  (Entity, &'static Floor, Option<&'static GlobalTransform>),
  (Without<ParentPlaced>, Allow<Disabled>),
>;

/// An entity on a floor whose floor, place or children changed
type Restood<'w, 's> = Query<
  'w,
  's,
  Entity,
  (
    With<Floor>,
    Without<ParentPlaced>,
    Or<(Changed<Floor>, Changed<GlobalTransform>, Changed<Children>)>,
    Allow<Disabled>,
  ),
>;

/// A lifted entity below a floor that was put under another parent, or
/// given children
type Relinked<'w, 's> = Query<
  'w,
  's,
  Entity,
  (
    With<DrawLift>,
    Without<Floor>,
    Or<(Changed<ChildOf>, Changed<Children>)>,
    Allow<Disabled>,
  ),
>;

/// The maps with floors, and those moved, turned or given new floors
pub(super) type Maps<'w, 's> = Query<
  'w,
  's,
  (
    Entity,
    &'static MapFloors,
    &'static GlobalTransform,
    &'static MapLayers,
  ),
>;
type MapsChanged<'w, 's> = Query<
  'w,
  's,
  (),
  (
    With<MapFloors>,
    Or<(Changed<GlobalTransform>, Changed<MapFloors>)>,
  ),
>;

/// A map's tile layers, where they stand in its pixels
pub(super) type TileLayers<'w, 's> = Query<'w, 's, (&'static Transform, &'static TileLayerData)>;

/// The hierarchy, walked down from an entity on a floor (stopping at the
/// entities on a floor of their own) and up from a loose one
type Tree<'w, 's> = (
  Query<'w, 's, &'static Children, Allow<Disabled>>,
  Query<'w, 's, &'static ChildOf, Allow<Disabled>>,
  HasFloor<'w, 's>,
);

/// Whether an entity has a `Floor` of its own
type HasFloor<'w, 's> = Query<'w, 's, (), (With<Floor>, Allow<Disabled>)>;

/// Every entity's lift, disabled ones' too
type Lifts<'w, 's> = Query<'w, 's, &'static mut DrawLift, Allow<Disabled>>;

/// The climbers, whose lift eases with where they draw on stairs (from
/// where they stood the step before, for an interpolated one), the maps'
/// stairs, and the clocks
type Climbing<'w, 's> = (
  Query<
    'w,
    's,
    (Entity, Option<&'static PreviousGlobalTransform>),
    (With<Climber>, Without<ParentPlaced>, Allow<Disabled>),
  >,
  Stairs<'w, 's>,
  Option<Res<'w, Time>>,
);

/// After transforms propagate, in each fixed step (before trails record,
/// so a trail spawned on a floor records its first point lifted) and each
/// frame (`PostUpdate`, before the cameras): every entity on a floor draws
/// lifted by its floor's height on the map under it, and so does
/// everything below it in the hierarchy ([`DrawLift`]), down to an entity
/// on a floor of its own; disabled entities too, so their lift keeps step.
///
/// The map under an entity is the first map with floors, in entity order,
/// whose tile layers' box of cells holds where it stands
/// (`GlobalTransform`); off every one the lift is zero. An entity on the
/// ground floor, unlifted, has no `DrawLift`, and nor does anything below
/// it. A run lifts again the entities whose `Floor`, `GlobalTransform` or
/// `Children` changed, and every entity on a floor once a map with floors
/// moved, turned, or gained or lost its floors; an entity that lost its
/// `Floor`, or a lifted one moved to another parent or given children,
/// takes the lift of its nearest ancestor on a floor, or none. A map with
/// floors enabled again lifts everything on a floor again too. A map's box
/// changing on its own (cells landing or edited past it, the map disabled)
/// reaches an entity when it next moves.
///
/// A [`Climber`] is lifted again on every run while a map with floors is
/// there: on a run of stairs of one of its floors it takes the run's two
/// floors' heights eased by how far up it is, where it draws (each frame,
/// between the last two steps by the overstep, as the draw sites place it;
/// in a fixed step, where the step left it). With no entity on a floor and
/// none lifted it costs two empty looks, so maps and entities without
/// floors pay nothing for it
#[allow(clippy::too_many_arguments)] // one query or removal reader per input
pub(crate) fn lift_floored(
  floored: Floored,
  restood: Restood,
  relinked: Relinked,
  (maps, maps_changed, tiles): (Maps, MapsChanged, TileLayers),
  (children, parents, has_floor): Tree,
  mut lifts: Lifts,
  (mut unfloored, mut unparented, mut unmapped, mut enabled): (
    RemovedComponents<Floor>,
    RemovedComponents<ChildOf>,
    RemovedComponents<MapFloors>,
    RemovedComponents<Disabled>,
  ),
  (climbers, stairs, time): Climbing,
  mut commands: Commands,
  mut state: Local<Lifting>,
) {
  if floored.is_empty() && lifts.is_empty() {
    unfloored.clear();
    unparented.clear();
    unmapped.clear();
    enabled.clear();
    return;
  }
  let Lifting {
    areas,
    seeds,
    loose,
    stack,
  } = &mut *state;

  // A map with floors moved, turned, landed, went or was enabled again:
  // everything on a floor stands somewhere else on it
  let unmapped = unmapped.read().count() > 0;
  let enabled = enabled
    .read()
    .filter(|&entity| maps.contains(entity))
    .count()
    > 0;
  if unmapped || enabled || !maps_changed.is_empty() {
    seeds.extend(floored.iter().map(|(entity, ..)| entity));
  } else {
    seeds.extend(restood.iter());
    // A climber's lift on stairs moves with where it draws
    if !maps.is_empty() {
      seeds.extend(climbers.iter().map(|(entity, _)| entity));
    }
  }
  loose.extend(unfloored.read());
  loose.extend(
    unparented
      .read()
      .filter(|&entity| lifts.contains(entity) && !has_floor.contains(entity)),
  );
  loose.extend(relinked.iter());
  // A loose entity takes its nearest ancestor's floor, or draws unlifted
  let unlifted = DrawLift {
    lift: Vec2::ZERO,
    floor: Floor::GROUND,
  };
  for &entity in loose.iter() {
    if has_floor.contains(entity) {
      continue;
    }
    match floored_ancestor(entity, &parents, &floored) {
      Some(ancestor) => seeds.push(ancestor),
      None => write_down(
        entity,
        unlifted,
        (&children, &has_floor),
        &mut lifts,
        &mut commands,
        stack,
      ),
    }
  }
  loose.clear();
  if seeds.is_empty() {
    return;
  }

  seeds.sort_unstable_by_key(entity_order);
  seeds.dedup();
  find_areas(areas, &maps, &tiles);
  // Where a climber draws: blended by the frame's overstep, or in a fixed
  // step where it stands
  let alpha = time
    .as_deref()
    .filter(|time| !time.in_fixed())
    .map(|time| time.fixed.overstep_fraction());
  for &seed in seeds.iter() {
    // Despawned, or its floor taken since
    let Ok((_, &floor, global)) = floored.get(seed) else {
      continue;
    };
    let lift = global.map_or(Vec2::ZERO, |global| match climbers.get(seed) {
      Ok((_, previous)) => {
        let drawn = alpha.map_or(*global, |alpha| render_global(global, previous, alpha));
        lift_at(areas, &maps, floor, (drawn.translation(), Some(&stairs)))
      }
      Err(_) => lift_at(areas, &maps, floor, (global.translation(), None)),
    });
    let drawn = DrawLift { lift, floor };
    write_down(
      seed,
      drawn,
      (&children, &has_floor),
      &mut lifts,
      &mut commands,
      stack,
    );
  }
  seeds.clear();
}

/// The lift of an entity on `floor` standing at `at` (world): the floor's
/// height on the first of `areas` that holds it, through that map's
/// transform, or for a climber (the maps' `stairs` given) on a run of
/// stairs of that floor the run's two floors' heights eased by how far up
/// it is; zero off every one, and for a floor the map lacks or `Every`
fn lift_at(
  areas: &[MapArea],
  maps: &Maps,
  floor: Floor,
  (at, stairs): (Vec2, Option<&Stairs>),
) -> Vec2 {
  areas
    .iter()
    .find(|area| area.holds(at))
    .and_then(|area| {
      let (_, floors, ..) = maps.get(area.map).ok()?;
      let climbing = stairs
        .and_then(|stairs| stairs.get(area.map).ok())
        .and_then(|stairs| stairs.climbing(area.local(at), floor));
      if let Some((run, up)) = climbing {
        let low = world_lift(Some(floors), Floor::On(run.from), area.to_world)?;
        let high = world_lift(Some(floors), Floor::On(run.to), area.to_world)?;
        return Some(low.lerp(high, up));
      }
      world_lift(Some(floors), floor, area.to_world)
    })
    .unwrap_or(Vec2::ZERO)
}

/// The nearest ancestor of `entity` on a floor
fn floored_ancestor(
  entity: Entity,
  parents: &Query<&ChildOf, Allow<Disabled>>,
  floored: &Floored,
) -> Option<Entity> {
  let mut child = entity;
  for _ in 0..MAX_DEPTH {
    let parent = parents.get(child).ok()?.parent();
    if floored.contains(parent) {
      return Some(parent);
    }
    child = parent;
  }
  None
}

/// Gives `root` and everything below it `drawn`, but the entities on a
/// floor of their own and what is below them; where `drawn` is the ground
/// floor unlifted, takes their `DrawLift` away instead
fn write_down(
  root: Entity,
  drawn: DrawLift,
  (children, has_floor): (&Query<&Children, Allow<Disabled>>, &HasFloor),
  lifts: &mut Lifts,
  commands: &mut Commands,
  stack: &mut Vec<Entity>,
) {
  let keep = drawn.floor != Floor::GROUND || drawn.lift != Vec2::ZERO;
  stack.clear();
  stack.push(root);
  while let Some(entity) = stack.pop() {
    match lifts.get_mut(entity) {
      Ok(mut current) if keep => {
        current.set_if_neq(drawn);
      }
      Ok(_) => {
        commands.entity(entity).try_remove::<DrawLift>();
      }
      Err(_) if keep => {
        commands.entity(entity).try_insert(drawn);
      }
      Err(_) => {}
    }
    if let Ok(below) = children.get(entity) {
      stack.extend(
        below
          .iter()
          .copied()
          .filter(|&child| !has_floor.contains(child)),
      );
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn layer(floor: u8, offset: Vec2) -> LayerInfo {
    let mut info = LayerInfo::new(0, "layer");
    info.floor = floor;
    info.offset = offset;
    info
  }

  #[test]
  fn a_floor_s_height_is_its_first_layer_s_vertical_offset() {
    let layers = [
      layer(0, Vec2::new(0.0, -8.0)),
      layer(2, Vec2::new(5.0, -64.0)),
      layer(2, Vec2::new(0.0, -72.0)),
    ];
    let floors = MapFloors::of(&layers).expect("floors above the ground");
    assert_eq!(floors.lift(Floor::GROUND), Some(Vec2::ZERO));
    assert_eq!(floors.lift(Floor::On(1)), None, "no layer is on it");
    assert_eq!(floors.lift(Floor::On(2)), Some(Vec2::new(0.0, -64.0)));
    assert_eq!(floors.lift(Floor::On(3)), None);
    assert_eq!(floors.lift(Floor::Every), None);
    assert_eq!(
      floors.floors().collect::<Vec<_>>(),
      [Floor::GROUND, Floor::On(2)]
    );
    assert_eq!(floors.highest(), Floor::On(2));
    let ground: Vec<Vec2> = layers
      .iter()
      .map(|info| ground_offset(info, Some(&floors)))
      .collect();
    assert_eq!(
      ground,
      [
        Vec2::new(0.0, -8.0),
        Vec2::new(5.0, 0.0),
        Vec2::new(0.0, -8.0)
      ]
    );
    // The top floor a u8 names
    let top = MapFloors::of(&[layer(255, Vec2::new(0.0, -1.0))]).expect("floors");
    assert_eq!(top.highest(), Floor::On(255));
    assert_eq!(top.lift(Floor::On(255)), Some(Vec2::new(0.0, -1.0)));
  }

  #[test]
  fn layers_all_on_the_ground_floor_make_no_floors() {
    let layers = [layer(0, Vec2::ZERO), layer(0, Vec2::new(3.0, -32.0))];
    assert_eq!(MapFloors::of(&layers), None);
    assert_eq!(MapFloors::of(&[]), None);
    assert_eq!(ground_offset(&layers[1], None), Vec2::new(3.0, -32.0));
  }

  mod lifting {
    use glam::{UVec2, Vec2};

    use crate::core::asset::Assets;
    use crate::core::ecs::kernel::Disabled;
    use crate::core::ecs::{
      Anchor, ChildOf, DrawLift, Entity, Floor, IntoScheduleConfigs, Renderable, Transform, World,
      YSort,
    };
    use crate::core::graphics::Image;
    use crate::core::render::draw_orders;
    use crate::core::tilemap::test_util::MapWorld;
    use crate::core::tilemap::{MapRoot, MapsView, TileMap};

    /// A map of 4 x 3 cells of 16 px (0..64 by 0..48 in its pixels) placed
    /// by `transform`: "ground" on the ground floor, "upper" on floor 1,
    /// 32 px up, and "roof" on floor 2, 64 px up; landed
    fn storeyed(maps: &mut MapWorld, transform: Transform) -> Entity {
      storeyed_at(maps, transform, 32.0)
    }

    /// `storeyed`, its floor 1 `height` px up
    fn storeyed_at(maps: &mut MapWorld, transform: Transform, height: f32) -> Entity {
      let mut map = TileMap::new(UVec2::new(4, 3), UVec2::splat(16));
      map.add_tile_layer("ground");
      let upper = map.add_tile_layer("upper");
      map.layers[upper].info.floor = 1;
      map.layers[upper].info.offset = Vec2::new(0.0, -height);
      let roof = map.add_tile_layer("roof");
      map.layers[roof].info.floor = 2;
      map.layers[roof].info.offset = Vec2::new(0.0, -64.0);
      spawn(maps, map, transform)
    }

    fn spawn(maps: &mut MapWorld, map: TileMap, transform: Transform) -> Entity {
      let handle = maps.world.resource_mut::<Assets<TileMap>>().add(map);
      let root = maps.world.spawn((MapRoot::new(handle), transform)).id();
      maps.settle(root);
      root
    }

    fn drawn(world: &World, entity: Entity) -> Option<DrawLift> {
      world.get::<DrawLift>(entity).copied()
    }

    /// Lifted 32 px up on floor 1
    const UP: DrawLift = DrawLift {
      lift: Vec2::new(0.0, -32.0),
      floor: Floor::On(1),
    };

    fn close(a: Vec2, b: Vec2) -> bool {
      (a - b).length() < 1e-3
    }

    #[test]
    fn a_floor_entity_s_children_draw_lifted_with_it() {
      let mut maps = MapWorld::new();
      storeyed(&mut maps, Transform::IDENTITY);
      // A hero on floor 1, its sprite a child and the torch it holds a
      // grandchild
      let world = &mut maps.world;
      let hero = world
        .spawn((Transform::from_xy(20.0, 30.0), Floor::On(1)))
        .id();
      let sprite = world
        .spawn((Transform::from_xy(0.0, -8.0), ChildOf(hero)))
        .id();
      let torch = world
        .spawn((Transform::from_xy(4.0, 0.0), ChildOf(sprite)))
        .id();
      maps.frame();
      for entity in [hero, sprite, torch] {
        assert_eq!(drawn(&maps.world, entity), Some(UP), "{entity}");
      }
      assert_eq!(
        MapsView::new(&mut maps.world).drawn_lift(torch),
        Vec2::new(0.0, -32.0)
      );

      // A sword put in the sprite's hand later draws lifted too; taken off
      // it, it draws where it stands
      let sword = maps
        .world
        .spawn((Transform::IDENTITY, ChildOf(sprite)))
        .id();
      maps.frame();
      assert_eq!(drawn(&maps.world, sword), Some(UP));
      maps.world.entity_mut(sword).remove::<ChildOf>();
      maps.frame();
      assert_eq!(drawn(&maps.world, sword), None);
      assert_eq!(MapsView::new(&mut maps.world).drawn_lift(sword), Vec2::ZERO);

      // The torch on a floor of its own draws on that floor, not its
      // ancestor's
      maps.world.entity_mut(torch).insert(Floor::GROUND);
      maps.frame();
      assert_eq!(drawn(&maps.world, torch), None);
      assert_eq!(drawn(&maps.world, sprite), Some(UP));

      // Back on the ground floor, the hero and its sprite draw unlifted
      maps.world.entity_mut(hero).insert(Floor::GROUND);
      maps.frame();
      assert_eq!(drawn(&maps.world, hero), None);
      assert_eq!(drawn(&maps.world, sprite), None);
    }

    #[test]
    fn a_turned_map_lifts_its_floor_entities_along_its_own_up() {
      use std::f32::consts::{FRAC_PI_2, PI};
      let mut maps = MapWorld::new();
      // Turned a quarter and doubled: floor 1's 32 px up points along the
      // world's x, 64 px long
      let turned = Transform::from_xy(50.0, 20.0)
        .with_rotation(FRAC_PI_2)
        .with_uniform_scale(2.0);
      let map = storeyed(&mut maps, turned);
      // On the map's pixel (32, 24), its middle
      let middle = Vec2::new(32.0, 24.0);
      let at = turned.to_affine().transform_point2(middle);
      let hero = maps
        .world
        .spawn((Transform::from_translation(at), Floor::On(1)))
        .id();
      let sprite = maps.world.spawn((Transform::IDENTITY, ChildOf(hero))).id();
      maps.frame();
      for entity in [hero, sprite] {
        let lift = MapsView::new(&mut maps.world).drawn_lift(entity);
        assert!(close(lift, Vec2::new(64.0, 0.0)), "{entity}: {lift}");
      }
      let view = MapsView::new(&mut maps.world);
      assert_eq!(view.lift(map, Floor::On(1)), Some(view.drawn_lift(hero)));

      // Turned upside down about the hero, which stays where it is: its
      // lift turns with the map, though nothing on it moved
      let upside = Transform::from_translation(at + Vec2::new(64.0, 48.0))
        .with_rotation(PI)
        .with_uniform_scale(2.0);
      assert!(close(upside.to_affine().transform_point2(middle), at));
      if let Some(mut transform) = maps.world.get_mut::<Transform>(map) {
        *transform = upside;
      }
      maps.frame();
      for entity in [hero, sprite] {
        let lift = MapsView::new(&mut maps.world).drawn_lift(entity);
        assert!(close(lift, Vec2::new(0.0, 64.0)), "{entity}: {lift}");
      }
    }

    #[test]
    fn an_entity_off_every_map_with_floors_draws_unlifted() {
      let mut maps = MapWorld::new();
      storeyed(&mut maps, Transform::IDENTITY);
      // A map without floors 200 px right
      let plain = TileMap::new(UVec2::new(4, 3), UVec2::splat(16));
      spawn(&mut maps, plain, Transform::from_xy(200.0, 0.0));
      let world = &mut maps.world;
      let beside = world
        .spawn((Transform::from_xy(-30.0, 20.0), Floor::On(1)))
        .id();
      let on_plain = world
        .spawn((Transform::from_xy(220.0, 20.0), Floor::On(1)))
        .id();
      maps.frame();
      let unlifted = DrawLift {
        lift: Vec2::ZERO,
        floor: Floor::On(1),
      };
      for entity in [beside, on_plain] {
        assert_eq!(drawn(&maps.world, entity), Some(unlifted), "{entity}");
        assert_eq!(
          MapsView::new(&mut maps.world).drawn_lift(entity),
          Vec2::ZERO
        );
      }
      // Stepping onto the map with floors lifts it, and off again drops it
      let walk = |maps: &mut MapWorld, x: f32| {
        if let Some(mut transform) = maps.world.get_mut::<Transform>(beside) {
          transform.translation.x = x;
        }
        maps.frame();
        drawn(&maps.world, beside)
      };
      assert_eq!(walk(&mut maps, 20.0), Some(UP));
      assert_eq!(walk(&mut maps, -30.0), Some(unlifted));
    }

    #[test]
    fn removing_a_floor_drops_the_lift_from_the_entity_and_its_children() {
      let mut maps = MapWorld::new();
      storeyed(&mut maps, Transform::IDENTITY);
      let world = &mut maps.world;
      let hero = world
        .spawn((Transform::from_xy(20.0, 30.0), Floor::On(1)))
        .id();
      let sprite = world.spawn((Transform::IDENTITY, ChildOf(hero))).id();
      let torch = world.spawn((Transform::IDENTITY, ChildOf(sprite))).id();
      maps.frame();
      assert_eq!(drawn(&maps.world, torch), Some(UP));
      maps.world.entity_mut(hero).remove::<Floor>();
      maps.frame();
      for entity in [hero, sprite, torch] {
        assert_eq!(drawn(&maps.world, entity), None, "{entity}");
      }

      // A child on the ground floor of its own under a hero on floor 1
      // draws unlifted; losing its floor, it takes the hero's
      let hero = maps
        .world
        .spawn((Transform::from_xy(20.0, 30.0), Floor::On(1)))
        .id();
      let child = maps
        .world
        .spawn((Transform::IDENTITY, Floor::GROUND, ChildOf(hero)))
        .id();
      let held = maps.world.spawn((Transform::IDENTITY, ChildOf(child))).id();
      maps.frame();
      assert_eq!(drawn(&maps.world, hero), Some(UP));
      assert_eq!(drawn(&maps.world, child), None);
      assert_eq!(drawn(&maps.world, held), None);
      maps.world.entity_mut(child).remove::<Floor>();
      maps.frame();
      assert_eq!(drawn(&maps.world, child), Some(UP));
      assert_eq!(drawn(&maps.world, held), Some(UP));
    }

    #[test]
    fn lifting_never_moves_an_entity_s_sort_point() {
      let mut maps = MapWorld::new();
      storeyed(&mut maps, Transform::IDENTITY);
      let image = maps
        .world
        .resource_mut::<Assets<Image>>()
        .add(Image::solid_rect(8, 16, 255, 255, 255, 255));
      let sprite = || {
        Renderable::image(image.clone())
          .with_anchor(Anchor::BottomCenter)
          .with_z_order(3)
      };
      // Both with their feet at y 30: the one on floor 1 spawned first, so
      // the entities' order alone would draw it first
      let upper = maps
        .world
        .spawn((
          Transform::from_xy(40.0, 30.0),
          sprite(),
          YSort::new(),
          Floor::On(1),
        ))
        .id();
      let ground = maps
        .world
        .spawn((Transform::from_xy(20.0, 30.0), sprite(), YSort::new()))
        .id();
      // Each in the order drawn, with its sort y and floor
      let orders = |world: &mut World| -> Vec<(Entity, Option<f32>, u8)> {
        draw_orders(world)
          .into_iter()
          .map(|(entity, order)| (entity, order.sort_y, order.floor))
          .collect()
      };
      assert_eq!(
        orders(&mut maps.world),
        [(upper, Some(30.0), 0), (ground, Some(30.0), 0)]
      );
      maps.frame();
      assert_eq!(drawn(&maps.world, upper), Some(UP));
      // Lifted 32 px, it still sorts where its feet stand, and at one
      // ground y the lower floor draws first
      assert_eq!(
        orders(&mut maps.world),
        [(ground, Some(30.0), 0), (upper, Some(30.0), 1)]
      );
    }

    #[test]
    fn a_trail_spawned_on_a_floor_records_its_first_point_lifted() {
      use crate::core::ecs::propagate::propagate_transforms;
      use crate::core::schedule::{FixedSet, Phase};
      use crate::core::systems::record_trail_points;
      use crate::core::trail::Trail2D;
      use crate::core::ui::Color;
      let mut maps = MapWorld::new();
      // The step's propagation and trails, as the engine places them
      maps.schedules().add_systems(
        Phase::FixedUpdate,
        (
          propagate_transforms.in_set(FixedSet::Transforms),
          record_trail_points.in_set(FixedSet::Trails),
        ),
      );
      storeyed(&mut maps, Transform::IDENTITY);
      // A bolt fired on floor 1 records its first point, in its first
      // step, where it draws
      let bolt = maps
        .world
        .spawn((
          Transform::from_xy(20.0, 30.0),
          Floor::On(1),
          Trail2D::new(Color::WHITE, 2.0),
        ))
        .id();
      maps.step();
      let points: Vec<(f32, f32)> = maps
        .world
        .get::<Trail2D>(bolt)
        .map(|trail| trail.points().iter().map(|p| (p.x, p.y)).collect())
        .unwrap_or_default();
      assert_eq!(points, [(20.0, -2.0)]);
      assert_eq!(drawn(&maps.world, bolt), Some(UP));
    }

    #[test]
    fn an_entity_on_every_floor_draws_unlifted_and_sorts_with_the_ground() {
      let mut maps = MapWorld::new();
      storeyed(&mut maps, Transform::IDENTITY);
      let trigger = maps
        .world
        .spawn((Transform::from_xy(20.0, 30.0), Floor::Every))
        .id();
      let child = maps
        .world
        .spawn((Transform::IDENTITY, ChildOf(trigger)))
        .id();
      maps.frame();
      let every = DrawLift {
        lift: Vec2::ZERO,
        floor: Floor::Every,
      };
      for entity in [trigger, child] {
        assert_eq!(drawn(&maps.world, entity), Some(every), "{entity}");
      }
      assert_eq!(every.draw_floor(), 0);
    }

    #[test]
    fn a_child_moved_between_floored_parents_takes_its_new_parent_s_lift() {
      let mut maps = MapWorld::new();
      storeyed(&mut maps, Transform::IDENTITY);
      let world = &mut maps.world;
      let low = world
        .spawn((Transform::from_xy(20.0, 30.0), Floor::On(1)))
        .id();
      let high = world
        .spawn((Transform::from_xy(40.0, 30.0), Floor::On(2)))
        .id();
      let lamp = world.spawn((Transform::IDENTITY, ChildOf(low))).id();
      let flame = world.spawn((Transform::IDENTITY, ChildOf(lamp))).id();
      maps.frame();
      assert_eq!(drawn(&maps.world, flame), Some(UP));
      // Handed from the one on floor 1 to the one on floor 2
      maps.world.entity_mut(lamp).insert(ChildOf(high));
      maps.frame();
      let higher = DrawLift {
        lift: Vec2::new(0.0, -64.0),
        floor: Floor::On(2),
      };
      for entity in [lamp, flame] {
        assert_eq!(drawn(&maps.world, entity), Some(higher), "{entity}");
      }
      assert_eq!(drawn(&maps.world, low), Some(UP));
    }

    #[test]
    fn where_maps_with_floors_overlap_the_first_lifts() {
      let mut maps = MapWorld::new();
      // The first map's floor 1 is 32 px up; the second's, 40 px right
      // and spawned later, 8 px up
      storeyed(&mut maps, Transform::IDENTITY);
      storeyed_at(&mut maps, Transform::from_xy(40.0, 0.0), 8.0);
      let world = &mut maps.world;
      let both = world
        .spawn((Transform::from_xy(50.0, 20.0), Floor::On(1)))
        .id();
      let second = world
        .spawn((Transform::from_xy(80.0, 20.0), Floor::On(1)))
        .id();
      maps.frame();
      let view = MapsView::new(&mut maps.world);
      assert_eq!(view.drawn_lift(both), Vec2::new(0.0, -32.0));
      assert_eq!(view.drawn_lift(second), Vec2::new(0.0, -8.0));
    }

    #[test]
    fn a_still_entity_takes_its_lift_when_its_map_is_enabled_again() {
      let mut maps = MapWorld::new();
      let map = storeyed(&mut maps, Transform::IDENTITY);
      maps.world.entity_mut(map).insert(Disabled);
      // A lamp put on floor 1 while its map is put away stands over no map
      let lamp = maps
        .world
        .spawn((Transform::from_xy(20.0, 30.0), Floor::On(1)))
        .id();
      maps.frame();
      let unlifted = DrawLift {
        lift: Vec2::ZERO,
        floor: Floor::On(1),
      };
      assert_eq!(drawn(&maps.world, lamp), Some(unlifted));
      // The map enabled again: the lamp, standing still, draws lifted
      maps.world.entity_mut(map).remove::<Disabled>();
      maps.frame();
      assert_eq!(drawn(&maps.world, lamp), Some(UP));
    }

    #[test]
    fn a_disabled_child_keeps_its_lift_in_step_with_its_parent() {
      use crate::core::ecs::DetectChanges;
      let mut maps = MapWorld::new();
      storeyed(&mut maps, Transform::IDENTITY);
      let hero = maps
        .world
        .spawn((Transform::from_xy(20.0, 30.0), Floor::On(1)))
        .id();
      // A sheathed sword, disabled while it is put away
      let sword = maps
        .world
        .spawn((Transform::IDENTITY, ChildOf(hero), Disabled))
        .id();
      maps.frame();
      assert_eq!(drawn(&maps.world, sword), Some(UP));
      let changed = |world: &World| {
        world
          .entity(sword)
          .get_ref::<DrawLift>()
          .map(|lift| lift.last_changed())
      };
      let then = changed(&maps.world);
      // The hero walks on, on the same floor: the sword's lift is not
      // written again
      for x in [24.0, 28.0] {
        if let Some(mut transform) = maps.world.get_mut::<Transform>(hero) {
          transform.translation.x = x;
        }
        maps.frame();
      }
      assert_eq!(changed(&maps.world), then);
      // Down on the ground floor, the sword's lift goes with the hero's
      maps.world.entity_mut(hero).insert(Floor::GROUND);
      maps.frame();
      assert_eq!(drawn(&maps.world, sword), None);
      maps.world.entity_mut(sword).remove::<Disabled>();
      assert_eq!(MapsView::new(&mut maps.world).drawn_lift(sword), Vec2::ZERO);
    }
  }

  /// Times what floors add to a frame: 64 climbers, each with a child,
  /// walking a 256 x 256 isometric map of 32 x 16 cells with 64 raised
  /// floors (16 x 16 cells, 16 px up), each up and down a run of two stairs
  /// onto its own floor and back. Each step the climbers move, then
  /// `climb_stairs` runs (the fixed step's), then `lift_floored` twice (the
  /// fixed step's and the frame's), each in a schedule of its own, commands
  /// applied. Run it in release: `cargo test --release -p ryo2d --lib
  /// floors_frame_timing -- --ignored --nocapture`
  #[test]
  #[ignore = "timing; run in release by hand"]
  fn floors_frame_timing() {
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use glam::{IVec2, UVec2};

    use crate::core::asset::Assets;
    use crate::core::ecs::{MessageCursor, Messages, Schedule};
    use crate::core::tilemap::stairs::{FloorChanged, MapStairs, climb_stairs};
    use crate::core::tilemap::test_util::MapWorld;
    use crate::core::tilemap::{
      GridShape, MapRoot, Tile, TileData, TileMap, Tileset, TilesetImage, TilesetSource,
    };

    const SIDE: i32 = 256;
    const CLIMBERS: usize = 64;
    /// How far a climber walks a step, in map pixels (120 px/s at 60 Hz)
    const SPEED: f32 = 2.0;
    let mut set = Tileset::new("steps", UVec2::new(32, 16), TilesetImage::Collection);
    let stairs = TileData {
      stairs: Some(1),
      ..TileData::default()
    };
    set.insert_tile(1, stairs);
    let mut map =
      TileMap::new(UVec2::splat(SIDE as u32), UVec2::new(32, 16)).with_shape(GridShape::Isometric);
    let set = map.add_tileset(TilesetSource::Embedded(Arc::new(set)));
    let ground = map.add_tile_layer("ground");
    let upper = map.add_tile_layer("upper");
    map.layers[upper].info.floor = 1;
    map.layers[upper].info.offset = Vec2::new(0.0, -16.0);
    // Each raised floor's first cell, 32 cells apart
    let corners: Vec<IVec2> = (0..8)
      .flat_map(|j| (0..8).map(move |i| IVec2::new(32 * i + 8, 32 * j + 8)))
      .collect();
    if let Some(tiles) = map.tiles_mut(ground) {
      tiles.fill(IVec2::ZERO, IVec2::splat(SIDE - 1), Some(Tile::new(set, 0)));
      // Two stairs leading into each floor's side, halfway along it
      for &corner in &corners {
        for dx in [-2, -1] {
          tiles.set(corner + IVec2::new(dx, 8), Some(Tile::new(set, 1)));
        }
      }
    }
    if let Some(tiles) = map.tiles_mut(upper) {
      for &corner in &corners {
        tiles.fill(corner, corner + IVec2::splat(15), Some(Tile::new(set, 0)));
      }
    }
    let grid = map.grid();
    let mut maps = MapWorld::new();
    let handle = maps.world.resource_mut::<Assets<TileMap>>().add(map);
    let root = maps.spawn(MapRoot::new(handle));
    let runs = maps
      .world
      .get::<MapStairs>(root)
      .map_or(0, |stairs| stairs.runs.len());
    assert_eq!(runs, corners.len(), "a run onto each floor");

    // Each climber walks from three cells before its stairs' foot to three
    // cells onto its floor and back, starting at its own place on the way
    let ways: Vec<(Vec2, Vec2)> = corners
      .iter()
      .map(|&corner| {
        let from = grid.cell_center(corner + IVec2::new(-5, 8));
        let to = grid.cell_center(corner + IVec2::new(3, 8));
        (from, to)
      })
      .collect();
    let climbers: Vec<Entity> = (0..CLIMBERS)
      .map(|_| {
        let climber = maps.world.spawn((Transform::IDENTITY, Climber)).id();
        maps
          .world
          .spawn((Transform::from_xy(0.0, -8.0), ChildOf(climber)));
        climber
      })
      .collect();
    let walk = |maps: &mut MapWorld, step: usize| {
      for (i, (&climber, &(from, to))) in climbers.iter().zip(&ways).enumerate() {
        let length = from.distance(to);
        let along = (step as f32 * SPEED / length + i as f32 / CLIMBERS as f32 * 2.0) % 2.0;
        let at = from.lerp(to, if along < 1.0 { along } else { 2.0 - along });
        let placed = Transform::from_translation(at);
        if let Some(mut transform) = maps.world.get_mut::<Transform>(climber) {
          *transform = placed;
        }
        if let Some(mut global) = maps.world.get_mut::<GlobalTransform>(climber) {
          *global = GlobalTransform::from_transform(&placed);
        }
      }
    };

    let mut climb = Schedule::default();
    climb.add_systems(climb_stairs);
    let mut lift_step = Schedule::default();
    lift_step.add_systems(lift_floored);
    let mut lift_frame = Schedule::default();
    lift_frame.add_systems(lift_floored);
    let mut cursor = MessageCursor::<FloorChanged>::default();
    let (mut climbs, mut steps, mut frames, mut all) =
      (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let (mut changes, mut easing) = (0, 0);
    const WARM: usize = 200;
    const TIMED: usize = 2_000;
    for step in 0..WARM + TIMED {
      walk(&mut maps, step);
      let start = Instant::now();
      climb.run(&mut maps.world);
      let climbed = start.elapsed();
      maps.world.resource_mut::<Time>().set_in_fixed(true);
      let start = Instant::now();
      lift_step.run(&mut maps.world);
      let lifted = start.elapsed();
      maps.world.resource_mut::<Time>().set_in_fixed(false);
      let start = Instant::now();
      lift_frame.run(&mut maps.world);
      let drawn = start.elapsed();
      // The floor changes and the climbers eased along their stairs, read
      // outside the measure
      let changed = cursor
        .read(maps.world.resource::<Messages<FloorChanged>>())
        .count();
      maps.world.resource_mut::<Messages<FloorChanged>>().update();
      let eased = climbers
        .iter()
        .filter_map(|&climber| maps.world.get::<DrawLift>(climber))
        .filter(|drawn| drawn.lift.y < -0.01 && drawn.lift.y > -15.99)
        .count();
      if step >= WARM {
        climbs.push(climbed);
        steps.push(lifted);
        frames.push(drawn);
        all.push(climbed + lifted + drawn);
        changes += changed;
        easing += eased;
      }
    }
    assert!(changes > TIMED / 2, "climbers change floor: {changes}");
    assert!(easing > TIMED, "climbers ease along their stairs: {easing}");

    let median_and_p99 = |mut times: Vec<Duration>| {
      times.sort();
      (times[times.len() / 2], times[times.len() * 99 / 100])
    };
    println!(
      "{CLIMBERS} climbers on {runs} runs of a {SIDE} x {SIDE} isometric map, {TIMED} steps: {changes} floor changes, {:.1} climbers on stairs a step",
      easing as f32 / TIMED as f32
    );
    for (name, times) in [
      ("climb_stairs", climbs),
      ("lift_floored, the fixed step's", steps),
      ("lift_floored, the frame's", frames),
      ("all three, a frame's floors", all),
    ] {
      let (median, p99) = median_and_p99(times);
      println!("{name} (median, 99th percentile): {median:?}, {p99:?}");
    }
  }
}
