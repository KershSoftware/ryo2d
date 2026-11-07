//! Storeys: a map's layers on floors above the ground (raised ground, upper
//! floors, bridges), each floor as high as Tiled draws it.
//!
//! A layer's floor is its `LayerInfo::floor`. A floor's height (its lift)
//! is the vertical part of the offset of its bottom layer, the first in
//! draw order on it; the ground floor's is zero. A layer above the ground
//! floor stands in ground space, at its offset less its floor's height, so
//! its objects, class tiles, colliders and shadows sit where characters
//! stand, and drawing is to add the height back.
//!
//! An entity on a floor stands in ground space too: [`lift_floored`]
//! gives it, and everything below it in the hierarchy, its floor's height
//! on the map under it ([`DrawLift`]), which the draw sites add.

use glam::{Affine2, Vec2};

use super::LayerInfo;
use super::spawn::{MapLayers, TileLayerData};
use crate::core::ecs::{
  Changed, ChildOf, Children, Commands, Component, DetectChangesMut, DrawLift, Entity, Floor,
  GlobalTransform, Local, Or, ParentPlaced, Query, RemovedComponents, Transform, With, Without,
  entity_order,
};

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
struct MapArea {
  map: Entity,
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
  fn holds(&self, at: Vec2) -> bool {
    let local = self.to_map.transform_point2(at);
    local.cmpge(self.min).all() && local.cmple(self.max).all()
  }
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
/// and shadow chains are drawn by their layers, and stay out of the scan
type Floored<'w, 's> =
  Query<'w, 's, (Entity, &'static Floor, Option<&'static GlobalTransform>), Without<ParentPlaced>>;

/// An entity on a floor whose floor, place or children changed
type Restood<'w, 's> = Query<
  'w,
  's,
  Entity,
  (
    With<Floor>,
    Without<ParentPlaced>,
    Or<(Changed<Floor>, Changed<GlobalTransform>, Changed<Children>)>,
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
  ),
>;

/// The maps with floors, and those moved, turned or given new floors
type Maps<'w, 's> = Query<
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
type TileLayers<'w, 's> = Query<'w, 's, (&'static Transform, &'static TileLayerData)>;

/// The hierarchy, walked down from an entity on a floor (stopping at the
/// entities on a floor of their own) and up from a loose one
type Tree<'w, 's> = (
  Query<'w, 's, &'static Children>,
  Query<'w, 's, &'static ChildOf>,
  Query<'w, 's, (), With<Floor>>,
);

/// `PostUpdate`, after transforms propagate, before the cameras: every
/// entity on a floor draws lifted by its floor's height on the map under
/// it, and so does everything below it in the hierarchy ([`DrawLift`]),
/// down to an entity on a floor of its own.
///
/// The map under an entity is the first map with floors, in entity order,
/// whose tile layers' box of cells holds where it stands
/// (`GlobalTransform`); off every one the lift is zero. An entity on the
/// ground floor, unlifted, has no `DrawLift`, and nor does anything below
/// it. A run lifts again the entities whose `Floor`, `GlobalTransform` or
/// `Children` changed, and every entity on a floor once a map with floors
/// moved, turned, or gained or lost its floors; an entity that lost its
/// `Floor`, or a lifted one moved to another parent or given children,
/// takes the lift of its nearest ancestor on a floor, or none. With no
/// entity on a floor and none lifted it costs two empty looks, so maps and
/// entities without floors pay nothing for it
#[allow(clippy::too_many_arguments)] // one query or removal reader per input
pub(crate) fn lift_floored(
  floored: Floored,
  restood: Restood,
  relinked: Relinked,
  (maps, maps_changed, tiles): (Maps, MapsChanged, TileLayers),
  (children, parents, has_floor): Tree,
  mut lifts: Query<&mut DrawLift>,
  (mut unfloored, mut unparented, mut unmapped): (
    RemovedComponents<Floor>,
    RemovedComponents<ChildOf>,
    RemovedComponents<MapFloors>,
  ),
  mut commands: Commands,
  mut state: Local<Lifting>,
) {
  if floored.is_empty() && lifts.is_empty() {
    unfloored.clear();
    unparented.clear();
    unmapped.clear();
    return;
  }
  let Lifting {
    areas,
    seeds,
    loose,
    stack,
  } = &mut *state;

  // A map with floors moved, turned, landed or went: everything on a floor
  // stands somewhere else on it
  let unmapped = unmapped.read().count() > 0;
  if unmapped || !maps_changed.is_empty() {
    seeds.extend(floored.iter().map(|(entity, ..)| entity));
  } else {
    seeds.extend(restood.iter());
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
  areas.clear();
  areas.extend(
    maps
      .iter()
      .filter_map(|(map, _, global, layers)| MapArea::of(map, global, layers, &tiles)),
  );
  areas.sort_unstable_by_key(|area| entity_order(&area.map));
  for &seed in seeds.iter() {
    // Despawned, or its floor taken since
    let Ok((_, &floor, global)) = floored.get(seed) else {
      continue;
    };
    let lift = global.map_or(Vec2::ZERO, |global| {
      lift_at(areas, &maps, floor, global.translation())
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
/// transform; zero off every one, and for a floor the map lacks or `Every`
fn lift_at(areas: &[MapArea], maps: &Maps, floor: Floor, at: Vec2) -> Vec2 {
  areas
    .iter()
    .find(|area| area.holds(at))
    .and_then(|area| {
      let (_, floors, ..) = maps.get(area.map).ok()?;
      world_lift(Some(floors), floor, area.to_world)
    })
    .unwrap_or(Vec2::ZERO)
}

/// The nearest ancestor of `entity` on a floor
fn floored_ancestor(
  entity: Entity,
  parents: &Query<&ChildOf>,
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
  (children, has_floor): (&Query<&Children>, &Query<(), With<Floor>>),
  lifts: &mut Query<&mut DrawLift>,
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
    use crate::core::ecs::{
      Anchor, ChildOf, DrawLift, Entity, Floor, Renderable, Transform, World, YSort,
    };
    use crate::core::graphics::Image;
    use crate::core::render::draw_orders;
    use crate::core::tilemap::test_util::MapWorld;
    use crate::core::tilemap::{MapRoot, MapsView, TileMap};

    /// A map of 4 x 3 cells of 16 px (0..64 by 0..48 in its pixels) placed
    /// by `transform`: "ground" on the ground floor and "upper" on floor
    /// 1, 32 px up; landed
    fn storeyed(maps: &mut MapWorld, transform: Transform) -> Entity {
      let mut map = TileMap::new(UVec2::new(4, 3), UVec2::splat(16));
      map.add_tile_layer("ground");
      let upper = map.add_tile_layer("upper");
      map.layers[upper].info.floor = 1;
      map.layers[upper].info.offset = Vec2::new(0.0, -32.0);
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
  }
}
