//! Character: the controller and its presets on a tile level, top-down
//! movement, and custom movement on the bare controller. Loads no files, so
//! it runs on the web too.

use std::f32::consts::{PI, TAU};

use demo_kit::prelude::*;

fn main() {
  Demo::new("Character")
    .resolution(960, 640)
    .page(PlatformerPage::default)
    .page(TopDownPage::default)
    .page(CustomPage::default)
    .run();
}

/// WASD moves (the arrows belong to the panel), Space jumps, Shift dashes.
fn bindings() -> MoveBindings {
  MoveBindings::default()
    .with_left([KeyCode::KeyA])
    .with_right([KeyCode::KeyD])
    .with_up([KeyCode::KeyW])
    .with_down([KeyCode::KeyS])
}

const PLAYER: Color = Color::WHITE;
const DYNAMIC: Color = Color::new(90, 220, 110, 255);
const KINEMATIC: Color = Color::new(255, 170, 40, 255);
const FIXED: Color = Color::new(110, 150, 255, 255);
const COIN: Color = Color::new(255, 230, 60, 255);
const SPIKES: Color = Color::new(255, 80, 80, 255);
const FLAG: Color = Color::new(80, 230, 230, 255);
/// One tile, px. Pages keep their scenes left of x = 520, clear of the settings panel.
const TILE: f32 = 16.0;
/// Where each page's tile grid has its top-left corner.
const ORIGIN: Vec2 = Vec2::new(20.0, 90.0);

/// Marks the page's player so the outline can pick it out.
#[derive(Component)]
struct Player;

/// `collider`'s outline on an entity at `global`, in world space (a circle
/// gets a spoke so its spin shows).
fn outline(global: &GlobalTransform, collider: &Collider) -> Vec<Vec2> {
  let arc = |centre: Vec2, radius: f32, from: f32, to: f32| {
    (0..=12).map(move |i| centre + Vec2::from_angle(from + (to - from) * i as f32 / 12.0) * radius)
  };
  let local: Vec<Vec2> = match &collider.shape {
    ColliderShape::Rect { size } => {
      let h = *size / 2.0;
      vec![
        Vec2::new(-h.x, -h.y),
        Vec2::new(h.x, -h.y),
        h,
        Vec2::new(-h.x, h.y),
        Vec2::new(-h.x, -h.y),
      ]
    }
    ColliderShape::Circle { radius } => arc(Vec2::ZERO, *radius, 0.0, TAU)
      .chain([Vec2::ZERO])
      .collect(),
    ColliderShape::Capsule { height, radius } => {
      let h = (height / 2.0 - radius).max(0.0);
      let mut points: Vec<Vec2> = arc(Vec2::new(0.0, h), *radius, 0.0, PI)
        .chain(arc(Vec2::new(0.0, -h), *radius, PI, TAU))
        .collect();
      points.push(points[0]);
      points
    }
    ColliderShape::Polygon { points } => points
      .iter()
      .copied()
      .chain(points.first().copied())
      .collect(),
    ColliderShape::Segment { a, b } => vec![*a, *b],
    ColliderShape::Polyline { points } => points.clone(),
    _ => Vec::new(),
  };
  let affine = global.affine();
  let turn = Vec2::from_angle(collider.rotation);
  local
    .into_iter()
    .map(|p| affine.transform_point2(collider.offset + turn.rotate(p)))
    .collect()
}

/// The colour of a collider: the player white, zones by what they do,
/// the rest by the kind of body they belong to (their own or their parent's).
fn colour_of(world: &World, entity: Entity, collider: &Collider) -> Color {
  if world.get::<Player>(entity).is_some() {
    return PLAYER;
  }
  if world.get::<KillZone>(entity).is_some() {
    return SPIKES;
  }
  if world.get::<Checkpoint>(entity).is_some() {
    return FLAG;
  }
  if world.get::<Pickup>(entity).is_some() || collider.is_trigger {
    return COIN;
  }
  let body = world.get::<RigidBody>(entity).or_else(|| {
    world
      .get::<ChildOf>(entity)
      .and_then(|child_of| world.get::<RigidBody>(child_of.parent()))
  });
  match body.map(|body| body.kind) {
    Some(BodyKind::Dynamic) => DYNAMIC,
    Some(BodyKind::Kinematic) => KINEMATIC,
    _ => FIXED,
  }
}

/// Outline every collider.
fn draw_colliders(ctx: &mut SceneContext) {
  let mut query = ctx.world.query::<(Entity, &GlobalTransform, &Collider)>();
  let lines: Vec<(Vec<Vec2>, Color)> = query
    .iter(ctx.world)
    .map(|(entity, global, collider)| {
      (
        outline(global, collider),
        colour_of(ctx.world, entity, collider),
      )
    })
    .collect();
  let mut debug = ctx.debug_draw();
  let mut pen = debug.width(2.0);
  for (points, color) in lines {
    pen.polyline(&points, color);
  }
}

/// A tile level from a string grid: `#` solid, `=` one-way, `/` and `\` 45°
/// slopes, anything else empty.
fn level(rows: &[&str]) -> TileColliders {
  let width = rows
    .iter()
    .map(|row| row.chars().count())
    .max()
    .unwrap_or(0);
  let cells = rows
    .iter()
    .flat_map(|row| {
      row
        .chars()
        .chain(std::iter::repeat('.'))
        .take(width)
        .map(|c| match c {
          '#' => TileCell::Solid,
          '=' => TileCell::OneWay,
          '/' => TileCell::Slope(SlopeCorner::BottomRight),
          '\\' => TileCell::Slope(SlopeCorner::BottomLeft),
          _ => TileCell::Empty,
        })
    })
    .collect();
  TileColliders::from_cells(width as u32, rows.len() as u32, Vec2::splat(TILE), cells)
}

fn spawn_level(ctx: &mut SceneContext, rows: &[&str]) {
  ctx
    .world
    .spawn((Transform::from_translation(ORIGIN), level(rows)));
}

/// The centre of tile (column, row)'s top edge, in world space.
fn tile_top(column: f32, row: f32) -> Vec2 {
  ORIGIN + Vec2::new((column + 0.5) * TILE, row * TILE)
}

/// Reverse `entity` at the ends of its path along `axis` (x or y) between
/// `low` and `high`, at `speed` px/s. Its `Velocity` carries the character
/// standing on it.
fn ping_pong(
  ctx: &mut SceneContext,
  entity: Entity,
  horizontal: bool,
  low: f32,
  high: f32,
  speed: f32,
) {
  let Some(at) = ctx.world.get::<Transform>(entity).map(|t| t.translation) else {
    return;
  };
  let position = if horizontal { at.x } else { at.y };
  let Some(mut velocity) = ctx.world.get_mut::<Velocity>(entity) else {
    return;
  };
  let current = if horizontal { velocity.dx } else { velocity.dy };
  let next = if position >= high {
    -speed
  } else if position <= low || current == 0.0 {
    speed
  } else {
    current
  };
  *velocity = if horizontal {
    Velocity::new(next, 0.0)
  } else {
    Velocity::new(0.0, next)
  };
}

fn player_state(ctx: &SceneContext, player: Option<Entity>) -> Option<CharacterState> {
  ctx.world.get::<CharacterState>(player?).cloned()
}

// --- Platformer -------------------------------------------------------------

/// 30 x 30 tiles. The ground has a gap (a lift of its own crosses it, with
/// spikes' worth of kill zone below); a hill of slopes, one-way shelves, a
/// ledge reached by a lift, and a shaft to wall-jump up to the top ledge.
const PLATFORMER_LEVEL: [&str; 30] = [
  "##############################",
  "#............................#",
  "#............................#",
  "#............................#",
  "#............................#",
  "#............................#",
  "#............................#",
  "#............................#",
  "#............................#",
  "#............................#",
  "#...................######...#",
  "#........................#...#",
  "#........................#...#",
  "#........................#...#",
  "#........................#...#",
  "#........................#...#",
  "#........................#...#",
  "#...#####................#...#",
  "#........................#...#",
  "#..........======........#...#",
  "#........................#...#",
  "#........................#...#",
  "#.................======.#...#",
  "#........................#...#",
  "#............................#",
  "#........====................#",
  "#..../\\......................#",
  "#.../##\\.....................#",
  "#############........#########",
  "#############........#########",
];

/// The platform over the gap runs between these x positions (its ends flush
/// with the ground); the lift runs at `LIFT_X`.
const ACROSS: (f32, f32) = (256.0, 328.0);
const LIFT_X: f32 = 56.0;

struct PlatformerPage {
  run_speed: f32,
  jump_height: f32,
  time_to_apex: f32,
  coyote_time: f32,
  jump_buffer: f32,
  jump_cut: f32,
  air_jumps: i32,
  wall_slide: bool,
  wall_jump: bool,
  dash: bool,
  player: Option<Entity>,
  across: Option<Entity>,
  lift: Option<Entity>,
  coins: u32,
  deaths: u32,
  last_event: String,
}

impl Default for PlatformerPage {
  fn default() -> Self {
    let base = PlatformerMovement::default();
    Self {
      run_speed: base.run_speed,
      jump_height: base.jump_height,
      time_to_apex: base.time_to_apex,
      coyote_time: base.coyote_time,
      jump_buffer: base.jump_buffer,
      jump_cut: base.jump_cut,
      air_jumps: 0,
      wall_slide: false,
      wall_jump: false,
      dash: false,
      player: None,
      across: None,
      lift: None,
      coins: 0,
      deaths: 0,
      last_event: "-".to_string(),
    }
  }
}

impl PlatformerPage {
  /// The preset the settings describe.
  fn movement(&self) -> PlatformerMovement {
    let mut movement = PlatformerMovement::default()
      .with_run(self.run_speed, 0.08, 0.06)
      .with_jump(self.jump_height, self.time_to_apex)
      .with_coyote_time(self.coyote_time)
      .with_jump_buffer(self.jump_buffer)
      .with_jump_cut(self.jump_cut)
      .with_air_jumps(self.air_jumps.max(0) as u32);
    if self.wall_slide {
      movement = movement.with_wall_slide(90.0);
    }
    if self.wall_jump {
      movement = movement.with_wall_jump(Vec2::new(200.0, 380.0), 0.18);
    }
    if self.dash {
      movement = movement.with_dash(420.0, 0.15, 0.5);
    }
    movement
  }
}

impl DemoPage for PlatformerPage {
  fn name(&self) -> &str {
    "Platformer"
  }
  fn description(&self) -> &str {
    "A tile level: slopes, one-way shelves, a moving platform and a lift. Spikes and the pit send you back to the last flag."
  }
  fn settings(&mut self, s: &mut Settings) {
    s.float("Run speed", &mut self.run_speed, 60.0..=400.0, 10.0);
    s.float("Jump height", &mut self.jump_height, 16.0..=128.0, 4.0);
    s.float("Time to apex", &mut self.time_to_apex, 0.15..=0.8, 0.01);
    s.float("Coyote time", &mut self.coyote_time, 0.0..=0.3, 0.01);
    s.float("Jump buffer", &mut self.jump_buffer, 0.0..=0.3, 0.01);
    s.float("Jump cut", &mut self.jump_cut, 0.0..=1.0, 0.05);
    s.int("Air jumps", &mut self.air_jumps, 0..=2);
    s.toggle("Wall slide", &mut self.wall_slide);
    s.toggle("Wall jump", &mut self.wall_jump);
    s.toggle("Dash", &mut self.dash);
  }
  fn controls(&self) -> Vec<Control> {
    vec![
      Control::keys(
        "WASD",
        &[KeyCode::KeyW, KeyCode::KeyA, KeyCode::KeyS, KeyCode::KeyD],
        "Move",
      ),
      Control::key(KeyCode::Space, "Jump (S + Space drops through a shelf)"),
      Control::key(KeyCode::ShiftLeft, "Dash"),
    ]
  }
  fn enter(&mut self, ctx: &mut SceneContext) {
    spawn_level(ctx, &PLATFORMER_LEVEL);
    // A platform crossing the gap in the ground, and a lift beside the hill
    self.across = Some(
      ctx
        .world
        .spawn((
          Transform::from_xy(ACROSS.0, tile_top(0.0, 28.0).y + 6.0),
          RigidBody::kinematic(),
          Collider::rect(56.0, 12.0),
          Velocity::new(50.0, 0.0),
        ))
        .id(),
    );
    self.lift = Some(
      ctx
        .world
        .spawn((
          Transform::from_xy(LIFT_X, tile_top(0.0, 28.0).y + 6.0),
          RigidBody::kinematic(),
          Collider::rect(36.0, 12.0),
          Velocity::new(0.0, -45.0),
        ))
        .id(),
    );
    // The pit under the gap, and spikes on the ground by the shaft
    ctx.world.spawn((
      Transform::from_translation(tile_top(16.5, 31.0)),
      Collider::rect(140.0, 24.0).as_trigger(),
      KillZone::new(),
    ));
    ctx.world.spawn((
      Transform::from_translation(tile_top(23.5, 28.0) - Vec2::new(0.0, 5.0)),
      Collider::rect(32.0, 10.0).as_trigger(),
      KillZone::new(),
    ));
    // Crates to push and stack, past the gap
    let ground = tile_top(0.0, 28.0).y;
    for at in [
      Vec2::new(368.0, ground - 10.0),
      Vec2::new(368.0, ground - 30.0),
      Vec2::new(452.0, ground - 10.0),
    ] {
      ctx.world.spawn((
        Transform::from_translation(at),
        RigidBody::dynamic(),
        Collider::rect(20.0, 20.0),
      ));
    }
    for (column, row) in [
      (5.5, 26.0),
      (10.5, 25.0),
      (15.5, 27.0),
      (20.5, 22.0),
      (13.5, 19.0),
      (5.0, 17.0),
      (27.0, 18.0),
      (27.0, 13.0),
      (21.0, 10.0),
    ] {
      ctx.world.spawn((
        Transform::from_translation(tile_top(column, row) - Vec2::new(0.0, 12.0)),
        Collider::rect(10.0, 10.0).as_trigger(),
        Pickup::new().respawn_after(3.0),
      ));
    }
    for (column, row) in [(7.0, 17.0), (23.5, 10.0)] {
      ctx.world.spawn((
        Transform::from_translation(tile_top(column, row) - Vec2::new(0.0, 16.0)),
        Collider::rect(12.0, 32.0).as_trigger(),
        Checkpoint::new(),
      ));
    }
    self.player = Some(
      ctx
        .world
        .spawn((
          Player,
          Transform::from_translation(tile_top(9.5, 28.0) - Vec2::new(0.0, 14.0)),
          CharacterController::platformer(),
          Collider::rect(14.0, 26.0),
          self.movement(),
          bindings(),
          Respawn::here(),
        ))
        .id(),
    );
  }
  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    if let Some(player) = self.player {
      let movement = self.movement();
      if let Some(mut current) = ctx.world.get_mut::<PlatformerMovement>(player)
        && *current != movement
      {
        *current = movement;
      }
    }
    if let Some(across) = self.across {
      ping_pong(ctx, across, true, ACROSS.0, ACROSS.1, 50.0);
    }
    if let Some(lift) = self.lift {
      let (high, low) = (tile_top(0.0, 17.0).y + 6.0, tile_top(0.0, 28.0).y + 6.0);
      ping_pong(ctx, lift, false, high, low, 45.0);
    }
    self.coins += ctx.messages::<PickedUp>().len() as u32;
    self.deaths += ctx.messages::<Killed>().len() as u32;
    for event in ctx.messages::<MovementEvent>() {
      if Some(event.entity) == self.player {
        self.last_event = format!("{:?}", event.kind);
      }
    }
    draw_colliders(ctx);
  }
  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    let area = content_area(ui);
    ui.draw_text(
      &format!(
        "Coins: {}   Deaths: {}   Last: {}",
        self.coins, self.deaths, self.last_event
      ),
      area.x,
      area.y,
      Color::WHITE,
    );
  }
}

// --- Top-down ---------------------------------------------------------------

/// A walled room with pillars and a short inner wall.
fn room() -> Vec<String> {
  let mut rows: Vec<Vec<char>> = (0..30)
    .map(|row| {
      (0..30)
        .map(|column| {
          if row == 0 || row == 29 || column == 0 || column == 29 {
            '#'
          } else {
            '.'
          }
        })
        .collect()
    })
    .collect();
  for (column, row) in [(6, 6), (21, 6), (6, 21), (21, 21)] {
    for (dc, dr) in [(0, 0), (1, 0), (0, 1), (1, 1), (2, 0), (2, 1)] {
      rows[row + dr][column + dc] = '#';
    }
  }
  rows[14][10..19].fill('#');
  rows
    .into_iter()
    .map(|row| row.into_iter().collect())
    .collect()
}

struct TopDownPage {
  max_speed: f32,
  accel_time: f32,
  decel_time: f32,
  dash: bool,
  player: Option<Entity>,
  coins: u32,
}

impl Default for TopDownPage {
  fn default() -> Self {
    let base = TopDownMovement::default();
    Self {
      max_speed: base.max_speed,
      accel_time: base.accel_time,
      decel_time: base.decel_time,
      dash: false,
      player: None,
      coins: 0,
    }
  }
}

impl TopDownPage {
  fn movement(&self) -> TopDownMovement {
    let movement =
      TopDownMovement::default().with_speed(self.max_speed, self.accel_time, self.decel_time);
    if self.dash {
      movement.with_dash(360.0, 0.15, 0.5)
    } else {
      movement
    }
  }
}

impl DemoPage for TopDownPage {
  fn name(&self) -> &str {
    "Top-down"
  }
  fn description(&self) -> &str {
    "Eight-direction movement with no gravity: slide along walls, push crates, diagonals are no faster."
  }
  fn settings(&mut self, s: &mut Settings) {
    s.float("Max speed", &mut self.max_speed, 60.0..=400.0, 10.0);
    s.float("Acceleration time", &mut self.accel_time, 0.0..=0.5, 0.01);
    s.float("Deceleration time", &mut self.decel_time, 0.0..=0.5, 0.01);
    s.toggle("Dash", &mut self.dash);
  }
  fn controls(&self) -> Vec<Control> {
    vec![
      Control::keys(
        "WASD",
        &[KeyCode::KeyW, KeyCode::KeyA, KeyCode::KeyS, KeyCode::KeyD],
        "Move",
      ),
      Control::key(KeyCode::ShiftLeft, "Dash"),
    ]
  }
  fn enter(&mut self, ctx: &mut SceneContext) {
    let rows = room();
    let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
    spawn_level(ctx, &rows);
    let cell = |column: f32, row: f32| ORIGIN + Vec2::new(column, row) * TILE;
    for (column, row) in [
      (9.0, 10.0),
      (15.0, 10.0),
      (12.0, 18.0),
      (20.0, 14.0),
      (4.0, 15.0),
    ] {
      ctx.world.spawn((
        Transform::from_translation(cell(column, row)),
        RigidBody::dynamic()
          .with_gravity_scale(0.0)
          .with_linear_damping(10.0)
          .lock_rotation(),
        Collider::rect(20.0, 20.0),
      ));
    }
    for (column, row) in [
      (3.0, 3.0),
      (14.0, 3.0),
      (26.0, 3.0),
      (14.0, 12.0),
      (3.0, 26.0),
      (26.0, 26.0),
    ] {
      ctx.world.spawn((
        Transform::from_translation(cell(column, row)),
        Collider::circle(6.0).as_trigger(),
        Pickup::new(),
      ));
    }
    self.player = Some(
      ctx
        .world
        .spawn((
          Player,
          Transform::from_translation(cell(14.0, 22.0)),
          CharacterController::top_down(),
          Collider::circle(9.0),
          self.movement(),
          bindings(),
        ))
        .id(),
    );
  }
  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    if let Some(player) = self.player {
      let movement = self.movement();
      if let Some(mut current) = ctx.world.get_mut::<TopDownMovement>(player)
        && *current != movement
      {
        *current = movement;
      }
    }
    self.coins += ctx.messages::<PickedUp>().len() as u32;
    draw_colliders(ctx);
  }
  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    let area = content_area(ui);
    ui.draw_text(
      &format!("Coins: {} / 6", self.coins),
      area.x,
      area.y,
      Color::WHITE,
    );
  }
}

// --- Custom -----------------------------------------------------------------

/// Ledges to leap between, and a pillar to bounce off.
const CUSTOM_LEVEL: [&str; 30] = [
  "####################",
  "#..................#",
  "#..................#",
  "#..................#",
  "#..................#",
  "#..................#",
  "#..................#",
  "#..................#",
  "#........#########.#",
  "#..................#",
  "#..................#",
  "#..................#",
  "#..................#",
  "#..................#",
  "#.#########........#",
  "#..................#",
  "#..................#",
  "#..................#",
  "#..................#",
  "#..................#",
  "#........#######...#",
  "#..................#",
  "#..................#",
  "#..................#",
  "#..................#",
  "#...####...........#",
  "#..................#",
  "#..................#",
  "####################",
  "####################",
];

struct CustomPage {
  gravity: f32,
  charge_time: f32,
  leap_power: f32,
  bounce: f32,
  player: Option<Entity>,
  charge: f32,
  aim: f32,
  report: Vec<String>,
}

impl Default for CustomPage {
  fn default() -> Self {
    Self {
      gravity: 1400.0,
      charge_time: 0.8,
      leap_power: 720.0,
      bounce: 0.7,
      player: None,
      charge: 0.0,
      aim: 0.0,
      report: Vec::new(),
    }
  }
}

impl DemoPage for CustomPage {
  fn name(&self) -> &str {
    "Custom"
  }
  fn description(&self) -> &str {
    "The bare controller with movement written in update: charge a leap and bounce off walls. The readout is what it reports."
  }
  fn settings(&mut self, s: &mut Settings) {
    s.float("Gravity", &mut self.gravity, 400.0..=2400.0, 100.0);
    s.float("Charge time", &mut self.charge_time, 0.3..=1.5, 0.05);
    s.float("Leap power", &mut self.leap_power, 400.0..=1000.0, 20.0);
    s.float("Wall bounce", &mut self.bounce, 0.0..=1.0, 0.05);
  }
  fn controls(&self) -> Vec<Control> {
    vec![
      Control::keys("A D", &[KeyCode::KeyA, KeyCode::KeyD], "Aim the leap"),
      Control::key(KeyCode::Space, "Hold to charge, release to leap"),
    ]
  }
  fn enter(&mut self, ctx: &mut SceneContext) {
    spawn_level(ctx, &CUSTOM_LEVEL);
    self.player = Some(
      ctx
        .world
        .spawn((
          Player,
          Transform::from_translation(tile_top(16.5, 28.0) - Vec2::new(0.0, 14.0)),
          CharacterController::platformer(),
          Collider::rect(14.0, 26.0),
        ))
        .id(),
    );
  }
  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    let Some(player) = self.player else {
      return;
    };
    let Some(state) = player_state(ctx, self.player) else {
      return;
    };
    let held = ctx.input.key_down(KeyCode::Space);
    let steer = f32::from(u8::from(ctx.input.key_down(KeyCode::KeyD)))
      - f32::from(u8::from(ctx.input.key_down(KeyCode::KeyA)));
    let mut velocity = ctx
      .world
      .get::<Velocity>(player)
      .map_or(Vec2::ZERO, Velocity::as_vec2);
    let mut leapt = false;
    if state.on_floor {
      velocity.x = 0.0;
      if held {
        self.charge = (self.charge + dt).min(self.charge_time);
        self.aim = steer;
      } else if self.charge > 0.0 {
        let power = self.leap_power * (0.4 + 0.6 * self.charge / self.charge_time);
        velocity = Vec2::new(self.aim * power * 0.55, -power);
        self.charge = 0.0;
        leapt = true;
      }
    } else {
      self.charge = 0.0;
      // The wall stopped the run; the bounce is what it asked for, reversed
      if state.on_wall {
        velocity.x = -state.velocity_before.x * self.bounce;
      }
    }
    if !leapt {
      velocity.y = (velocity.y + self.gravity * dt).min(900.0);
    }
    if let Some(mut current) = ctx.world.get_mut::<Velocity>(player) {
      *current = Velocity::from_vec2(velocity);
    }
    self.report = vec![
      format!(
        "On floor: {}   wall: {}   ceiling: {}",
        state.on_floor, state.on_wall, state.on_ceiling
      ),
      format!(
        "Floor normal ({:.2}, {:.2})   wall normal ({:.2}, {:.2})",
        state.floor_normal.x, state.floor_normal.y, state.wall_normal.x, state.wall_normal.y
      ),
      format!(
        "Floor: {:?}   platform velocity ({:.0}, {:.0})",
        state.floor, state.platform_velocity.x, state.platform_velocity.y
      ),
      format!(
        "Velocity before ({:.0}, {:.0})   hits: {}",
        state.velocity_before.x,
        state.velocity_before.y,
        state.hits.len()
      ),
    ];
    draw_colliders(ctx);
    if let Some(at) = ctx.world.get::<Transform>(player).map(|t| t.translation)
      && self.charge > 0.0
    {
      let fraction = self.charge / self.charge_time;
      let mut debug = ctx.debug_draw();
      debug.rect(
        at + Vec2::new(0.0, -26.0),
        Vec2::new(30.0 * fraction, 4.0),
        COIN,
      );
      if self.aim != 0.0 {
        debug.arrow(
          at,
          at + Vec2::new(self.aim * 0.55, -1.0).normalize() * (16.0 + 26.0 * fraction),
          FLAG,
        );
      }
    }
  }
  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    let area = content_area(ui);
    for (i, line) in self.report.iter().enumerate() {
      ui.draw_text(line, area.x, area.y + i as f32 * 16.0, Color::WHITE);
    }
  }
}
