//! The character the town, the island, the hex board and the dungeon
//! share: a top-down walker, Tiny Dungeon's hero scaled to the map's cells,
//! y-sorted at a map layer's z so it walks behind and in front of what that
//! layer draws, and where it draws, for a camera to follow, on whatever
//! floor it is; where a map's `player_start` stands; and, for the town and
//! the board, the cheapest path between two cells of a navigation grid, or
//! across a map's floors by its stairs, and a walk along it

use demo_kit::prelude::*;

use crate::shared::{bindings, draw_path, hero};

/// A character's look and stride
#[derive(Debug, Clone, Copy)]
pub(crate) struct Walk {
  /// The hero on Tiny Dungeon's sheet
  pub(crate) sprite: u32,
  /// The sprite's scale: whole numbers keep its pixels square
  pub(crate) scale: f32,
  /// Its collider, a circle round its feet, in world pixels
  pub(crate) radius: f32,
  /// Its top speed, in world pixels a second
  pub(crate) speed: f32,
}

/// The character at `at` (its feet), walked by WASD or a pad, its sprite
/// y-sorted at `z`. Its feet are the collider's centre and the sprite's
/// bottom, which it sorts by
pub(crate) fn spawn_walker(ctx: &mut SceneContext, at: Vec2, walk: Walk, z: i32) -> Entity {
  let walker = ctx
    .world
    .spawn((
      Transform::from_translation(at),
      Interpolated,
      Collider::circle(walk.radius),
      TopDownMovement::default().with_speed(walk.speed, 0.06, 0.08),
      bindings(),
    ))
    .id();
  if let Some(image) = hero(ctx, walk.sprite) {
    ctx.spawn_child(
      walker,
      (
        Transform::IDENTITY.with_scale(Vec2::splat(walk.scale)),
        Renderable::image(image)
          .with_anchor(Anchor::BottomCenter)
          .with_z_order(z),
        YSort::new(),
      ),
    );
  }
  walker
}

/// Turns the character's sprite the way it walks
pub(crate) fn face(ctx: &mut SceneContext, walker: Entity) {
  let Some(dx) = ctx.world.get::<Velocity>(walker).map(|v| v.dx) else {
    return;
  };
  if dx.abs() < 1.0 {
    return;
  }
  let sprites: Vec<Entity> = ctx
    .world
    .get::<Children>(walker)
    .map(|children| children.iter().copied().collect())
    .unwrap_or_default();
  for sprite in sprites {
    if let Some(mut renderable) = ctx.world.get_mut::<Renderable>(sprite)
      && renderable.flip_x != (dx < 0.0)
    {
      renderable.flip_x = dx < 0.0;
    }
  }
}

/// Where the character is, once it has spawned
pub(crate) fn place_of(ctx: &SceneContext, walker: Option<Entity>) -> Option<Vec2> {
  walker
    .and_then(|walker| ctx.world.get::<Transform>(walker))
    .map(|transform| transform.translation)
}

/// Where the character draws, once it has spawned: where it stands (its
/// `GlobalTransform`) lifted by the height of the floor it is on, or as
/// far up a flight of stairs as it has climbed, so a camera that follows
/// it keeps its sprite in the middle on every floor
pub(crate) fn drawn_at(ctx: &mut SceneContext, walker: Option<Entity>) -> Option<Vec2> {
  let walker = walker?;
  let stands = ctx.world.get::<GlobalTransform>(walker)?.translation();
  Some(stands + ctx.maps().drawn_lift(walker))
}

/// Where the designer's `player_start` stands in the map's pixels: on
/// isometric and oblique maps the loader has already projected it
pub(crate) fn map_start(map: &TileMap) -> Option<Vec2> {
  map.layers.iter().find_map(|layer| {
    let LayerKind::Objects(objects) = &layer.kind else {
      return None;
    };
    objects
      .iter()
      .find(|object| object.class.as_deref() == Some("player_start"))
      .map(|object| layer.info.offset + object.position)
  })
}

/// The cheapest path between two cells of a navigation grid, or of a
/// map's floors
pub(crate) struct CellPath {
  /// Its cells, the start and the goal included
  pub(crate) cells: Vec<IVec2>,
  /// The floor each is on: a path across floors has two at a stair cell,
  /// one on each floor, where it changes floor
  pub(crate) floors: Vec<Floor>,
  /// Their centres in the world, where a character on their floor stands
  /// (its storey's height below where it draws)
  pub(crate) centres: Vec<Vec2>,
  /// What the pathfinder found it costs: each step's length over the
  /// grid's shortest step, times the cost of the cell it enters
  pub(crate) cost: f32,
}

impl CellPath {
  /// The cheapest path from `from` to `to` on `grid`; `None` when either
  /// is blocked or off the grid, or nothing joins them
  pub(crate) fn find(grid: &NavigationGrid, from: IVec2, to: IVec2) -> Option<CellPath> {
    let path = Pathfinder::new()
      .find_path_grid(grid, from.into(), to.into())
      .path()?;
    Some(CellPath::along(&path, |_| Some(grid)))
  }

  /// The cheapest path from `from` to `to`, each a floor and its cell,
  /// across the floors of `floors`, climbing and coming down their stairs;
  /// `None` when either is blocked or off its floor, or nothing joins them
  pub(crate) fn find_across(
    floors: &FloorNavigation,
    from: (Floor, IVec2),
    to: (Floor, IVec2),
  ) -> Option<CellPath> {
    let path = Pathfinder::new()
      .find_floor_path_grid(floors, (from.0, from.1.into()), (to.0, to.1.into()))
      .path()?;
    Some(CellPath::along(&path, |floor| floors.grid(floor)))
  }

  /// `path`'s cells, floors, centres and cost, each step's cost read from
  /// the grid of its floor (`grid_of`); a step from one floor to another,
  /// on the stairs, costs nothing
  fn along<'a>(path: &Path, grid_of: impl Fn(Floor) -> Option<&'a NavigationGrid>) -> CellPath {
    let nodes = path.waypoints();
    let cells: Vec<IVec2> = nodes.iter().map(|node| node.grid_coord.into()).collect();
    let floors: Vec<Floor> = nodes.iter().map(|node| node.floor).collect();
    let centres = nodes.iter().map(|node| Vec2::new(node.x, node.y)).collect();
    let cost = (1..nodes.len())
      .filter(|&i| floors[i - 1] == floors[i])
      .map(|i| grid_of(floors[i]).map_or(0.0, |grid| step_cost(grid, cells[i - 1], cells[i])))
      .sum();
    CellPath {
      cells,
      floors,
      centres,
      cost,
    }
  }
}

/// What the grid charges for the step from `from` to its neighbour `to`
fn step_cost(grid: &NavigationGrid, from: IVec2, to: IVec2) -> f32 {
  grid
    .neighbors(from.into())
    .into_iter()
    .find(|(cell, _)| IVec2::from(*cell) == to)
    .map_or(0.0, |(_, cost)| cost)
}

/// How near a waypoint counts as reached, in world pixels (a waypoint
/// before the last also counts once the walker has passed it)
const ARRIVE: f32 = 4.0;
/// How far from the last waypoint the walker starts to slow, in world
/// pixels
const EASE: f32 = 40.0;
/// How long a walk goes on coming no nearer its next waypoint, in seconds
const PATIENCE: f32 = 1.0;
/// How much nearer counts as getting there, in world pixels
const PROGRESS: f32 = 0.5;

/// A character walking a path waypoint to waypoint, by the `MoveIntent`
/// it writes each step: straight at each waypoint, slowing into the last.
/// `MoveBindings` fill the intent after the page's `update` and before the
/// movement preset reads it, so they would overwrite it: the walk takes
/// them off the character and puts them back when it stops. It stops on
/// the last waypoint, when the player steers (a movement key or the pad),
/// or when it has come no nearer its next waypoint for a second, held up
/// by something the grid didn't know. Call `stop` to end it early: a walk
/// dropped instead leaves the character without its bindings
#[must_use = "a walk dropped without `stop` leaves the character without its bindings"]
pub(crate) struct PathWalk {
  walker: Entity,
  route: Vec<Vec2>,
  /// Where the character was when it set off
  origin: Vec2,
  /// The waypoint it walks to
  next: usize,
  /// The character's bindings, while the walk has them
  bindings: Option<MoveBindings>,
  /// The nearest it has come to the next waypoint, and for how long it has
  /// come no nearer
  nearest: f32,
  stalled: f32,
}

impl PathWalk {
  /// `walker` sets off along `route` (world points), from the first
  /// waypoint (the middle of the cell it stands in) unless it is already
  /// past it toward the second; `None` for an empty route or a character
  /// that is gone
  #[must_use]
  pub(crate) fn start(ctx: &mut SceneContext, walker: Entity, route: Vec<Vec2>) -> Option<Self> {
    let origin = place_of(ctx, Some(walker))?;
    let ahead = match route.as_slice() {
      [first, second, ..] => (*second - *first).dot(origin - *first) > 0.0,
      [_] => false,
      [] => return None,
    };
    let bindings = ctx
      .world
      .get_entity_mut(walker)
      .ok()?
      .take::<MoveBindings>();
    Some(PathWalk {
      walker,
      route,
      origin,
      next: usize::from(ahead),
      bindings,
      nearest: f32::INFINITY,
      stalled: 0.0,
    })
  }

  /// One step of `dt` seconds; `None` once the walk has stopped
  #[must_use]
  pub(crate) fn step(mut self, ctx: &mut SceneContext, dt: f32) -> Option<Self> {
    let at = place_of(ctx, Some(self.walker));
    let steered = self
      .bindings
      .as_ref()
      .is_some_and(|bindings| steers(bindings, ctx.input));
    let Some(at) = at.filter(|_| !steered) else {
      self.stop(ctx);
      return None;
    };
    while let Some(&point) = self.route.get(self.next) {
      let last = self.next + 1 == self.route.len();
      let from = self
        .next
        .checked_sub(1)
        .and_then(|before| self.route.get(before))
        .copied()
        .unwrap_or(self.origin);
      let passed = !last && (point - from).dot(at - point) > 0.0;
      if at.distance(point) > ARRIVE && !passed {
        break;
      }
      self.next += 1;
      self.nearest = f32::INFINITY;
      self.stalled = 0.0;
    }
    let Some(&target) = self.route.get(self.next) else {
      self.stop(ctx);
      return None;
    };
    let distance = at.distance(target);
    if distance < self.nearest - PROGRESS {
      self.nearest = distance;
      self.stalled = 0.0;
    } else {
      self.stalled += dt;
      if self.stalled >= PATIENCE {
        self.stop(ctx);
        return None;
      }
    }
    let direction = if self.next + 1 == self.route.len() {
      steering::arrive(at, target, 1.0, EASE)
    } else {
      steering::seek(at, target, 1.0)
    };
    self.aim(ctx, direction);
    Some(self)
  }

  /// Stops here: the character's intent still and its bindings back
  pub(crate) fn stop(mut self, ctx: &mut SceneContext) {
    self.aim(ctx, Vec2::ZERO);
    if let Some(bindings) = self.bindings.take()
      && let Ok(mut walker) = ctx.world.get_entity_mut(self.walker)
    {
      walker.insert(bindings);
    }
  }

  /// The waypoints still ahead, the next first
  pub(crate) fn ahead(&self) -> &[Vec2] {
    self.route.get(self.next..).unwrap_or_default()
  }

  /// What is left of the walk, from the character's feet at `at`, in
  /// `color`, `width` screen pixels wide
  pub(crate) fn draw(&self, debug: &mut DebugDraw, at: Vec2, color: Color, width: f32) {
    draw_path(debug, Some(at), self.ahead(), color, width);
  }

  fn aim(&self, ctx: &mut SceneContext, direction: Vec2) {
    if let Some(mut intent) = ctx.world.get_mut::<MoveIntent>(self.walker) {
      intent.direction = direction;
    }
  }
}

/// Whether the player steers with `bindings`: a movement key held, or the
/// pad's left stick or D-pad
fn steers(bindings: &MoveBindings, input: &Input) -> bool {
  let keys = [
    &bindings.left,
    &bindings.right,
    &bindings.up,
    &bindings.down,
  ];
  let key = keys
    .iter()
    .any(|keys| keys.iter().any(|&key| input.key_down(key)));
  let (x, y) = input.left_stick();
  let pad = bindings.gamepad
    && (Vec2::new(x, y).length() > 0.1
      || [
        GamepadButton::DPadLeft,
        GamepadButton::DPadRight,
        GamepadButton::DPadUp,
        GamepadButton::DPadDown,
      ]
      .into_iter()
      .any(|button| input.gamepad_button_down(button)));
  key || pad
}
