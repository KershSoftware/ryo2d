//! Physics: shapes and stacking, joints, scene queries, layers and triggers.
//! Everything is drawn as outlines of what the simulation holds. Loads no
//! files, so it runs on the web too.

use std::f32::consts::{PI, TAU};

use demo_kit::prelude::*;

fn main() {
  Demo::new("Physics")
    .resolution(960, 640)
    .page(ShapesPage::default)
    .page(JointsPage::default)
    .page(QueriesPage::default)
    .page(LayersPage::default)
    .run();
}

const DYNAMIC: Color = Color::new(90, 220, 110, 255);
const KINEMATIC: Color = Color::new(255, 170, 40, 255);
const FIXED: Color = Color::new(110, 150, 255, 255);
const TRIGGER: Color = Color::new(255, 230, 60, 255);
const JOINT: Color = Color::new(240, 80, 240, 255);
const HIGHLIGHT: Color = Color::WHITE;
/// Pages keep their scenes left of the settings panel.
const RIGHT_EDGE: f32 = 520.0;

/// A repeatable pseudo-random number in [0, 1) for spawn variety.
fn noise(i: u32) -> f32 {
  ((i as f32 * 12.9898).sin() * 43_758.547).fract().abs()
}

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

/// The kind of body a collider belongs to (its own or its parent's).
fn colour_of(world: &World, entity: Entity, collider: &Collider) -> Color {
  if collider.is_trigger {
    return TRIGGER;
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

/// Outline every collider, coloured by `colour` (default: by body kind).
fn draw_colliders(ctx: &mut SceneContext, colour: impl Fn(&World, Entity, &Collider) -> Color) {
  let mut query = ctx.world.query::<(Entity, &GlobalTransform, &Collider)>();
  let lines: Vec<(Vec<Vec2>, Color)> = query
    .iter(ctx.world)
    .map(|(entity, global, collider)| {
      (
        outline(global, collider),
        colour(ctx.world, entity, collider),
      )
    })
    .collect();
  let mut debug = ctx.debug_draw();
  for (points, color) in lines {
    debug.polyline(&points, color);
  }
}

/// A line between every joint's two bodies.
fn draw_joints(ctx: &mut SceneContext) {
  let mut query = ctx.world.query::<&Joint>();
  let ends: Vec<(Vec2, Vec2)> = query
    .iter(ctx.world)
    .filter_map(|joint| {
      let a = ctx.world.get::<GlobalTransform>(joint.a)?.translation();
      let b = ctx.world.get::<GlobalTransform>(joint.b)?.translation();
      Some((a, b))
    })
    .collect();
  let mut debug = ctx.debug_draw();
  for (a, b) in ends {
    debug.line(a, b, JOINT);
  }
}

fn mouse_world(ctx: &mut SceneContext) -> Vec2 {
  let (x, y) = ctx.input.mouse_position();
  ctx.camera().screen_to_world(Vec2::new(x, y))
}

/// Walls and a floor around the page's play area.
fn arena(ctx: &mut SceneContext) {
  ctx.world.spawn((
    Transform::IDENTITY,
    Collider::polyline([
      Vec2::new(20.0, 80.0),
      Vec2::new(20.0, 600.0),
      Vec2::new(RIGHT_EDGE, 600.0),
      Vec2::new(RIGHT_EDGE, 80.0),
    ]),
  ));
}

// --- Shapes ---------------------------------------------------------------

const SHAPES: [&str; 5] = ["Mix", "Box", "Circle", "Capsule", "Triangle"];
const MAX_BODIES: u32 = 300;

struct ShapesPage {
  shape: usize,
  bounce: f32,
  friction: f32,
  spawned: u32,
}

impl Default for ShapesPage {
  fn default() -> Self {
    Self {
      shape: 0,
      bounce: 0.1,
      friction: 0.5,
      spawned: 0,
    }
  }
}

impl ShapesPage {
  fn spawn(&mut self, ctx: &mut SceneContext, at: Vec2) {
    if self.spawned >= MAX_BODIES {
      return;
    }
    let i = self.spawned;
    self.spawned += 1;
    let size = 14.0 + noise(i) * 18.0;
    let pick = if self.shape == 0 {
      1 + i as usize % 4
    } else {
      self.shape
    };
    let collider = match pick {
      1 => Collider::rect(size, size * (0.6 + noise(i + 7) * 0.8)),
      2 => Collider::circle(size / 2.0),
      3 => Collider::capsule(size * 1.8, size / 2.0),
      _ => Collider::polygon([
        Vec2::new(-size, size / 2.0),
        Vec2::new(size, size / 2.0),
        Vec2::new(0.0, -size),
      ]),
    };
    ctx.world.spawn((
      Transform::from_translation(at).with_rotation(noise(i + 3) * TAU),
      RigidBody::dynamic(),
      collider
        .with_restitution(self.bounce)
        .with_friction(self.friction),
    ));
  }
}

impl DemoPage for ShapesPage {
  fn name(&self) -> &str {
    "Shapes"
  }
  fn description(&self) -> &str {
    "Boxes, circles, capsules and polygons that tumble, stack and sleep (F2 shows the engine overlay, where sleeping bodies are dimmed). Bounce and Friction apply to bodies dropped afterwards."
  }
  fn settings(&mut self, s: &mut Settings) {
    s.choice("Shape", &mut self.shape, &SHAPES);
    s.float("Bounce", &mut self.bounce, 0.0..=1.0, 0.05);
    s.float("Friction", &mut self.friction, 0.0..=1.0, 0.05);
  }
  fn controls(&self) -> Vec<Control> {
    vec![
      Control::mouse("Drop one"),
      Control::key(KeyCode::Space, "Drop ten"),
    ]
  }
  fn enter(&mut self, ctx: &mut SceneContext) {
    arena(ctx);
    // A valley with a ledge
    ctx.world.spawn((
      Transform::IDENTITY,
      Collider::polyline([
        Vec2::new(20.0, 420.0),
        Vec2::new(200.0, 560.0),
        Vec2::new(380.0, 560.0),
      ]),
    ));
    ctx.world.spawn((
      Transform::from_xy(430.0, 380.0).with_rotation(-0.3),
      Collider::rect(160.0, 16.0),
    ));
    for i in 0..20 {
      self.spawn(
        ctx,
        Vec2::new(
          120.0 + (i % 10) as f32 * 30.0,
          120.0 + (i / 10) as f32 * 40.0,
        ),
      );
    }
  }
  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    if ctx.input.mouse_pressed_and_consume(MouseButton::Left) {
      let at = mouse_world(ctx);
      self.spawn(ctx, at);
    }
    if ctx.input.key_pressed_and_consume(KeyCode::Space) {
      for k in 0..10 {
        self.spawn(ctx, Vec2::new(80.0 + k as f32 * 40.0, 100.0));
      }
    }
    draw_colliders(ctx, colour_of);
  }
  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    let area = content_area(ui);
    ui.draw_text(
      &format!("Bodies: {} / {MAX_BODIES}", self.spawned),
      area.x,
      area.y,
      Color::WHITE,
    );
  }
}

// --- Joints ---------------------------------------------------------------

const STRENGTHS: [&str; 3] = ["Unbreakable", "Strong", "Weak"];

struct JointsPage {
  motor: f32,
  strength: usize,
  broken: u32,
  car: Vec<Entity>,
  bridge: Vec<Entity>,
}

impl Default for JointsPage {
  fn default() -> Self {
    Self {
      motor: 6.0,
      strength: 1,
      broken: 0,
      car: Vec::new(),
      bridge: Vec::new(),
    }
  }
}

impl JointsPage {
  fn break_force(&self) -> Option<f32> {
    match self.strength {
      1 => Some(20_000.0),
      2 => Some(4_000.0),
      _ => None,
    }
  }
}

fn fixed_point(ctx: &mut SceneContext, at: Vec2) -> Entity {
  ctx
    .world
    .spawn((Transform::from_translation(at), RigidBody::fixed()))
    .id()
}

impl DemoPage for JointsPage {
  fn name(&self) -> &str {
    "Joints"
  }
  fn description(&self) -> &str {
    "A pendulum, a plank bridge that can break, a spring and a car driven by wheel motors."
  }
  fn settings(&mut self, s: &mut Settings) {
    s.float("Motor (rad/s)", &mut self.motor, -15.0..=15.0, 1.0);
    s.choice("Bridge", &mut self.strength, &STRENGTHS);
  }
  fn controls(&self) -> Vec<Control> {
    vec![Control::key(KeyCode::Space, "Drop a crate on the bridge")]
  }
  fn enter(&mut self, ctx: &mut SceneContext) {
    arena(ctx);
    // Pendulum
    let pivot = fixed_point(ctx, Vec2::new(90.0, 110.0));
    let bob = ctx
      .world
      .spawn((
        Transform::from_xy(170.0, 110.0),
        RigidBody::dynamic(),
        Collider::circle(14.0),
      ))
      .id();
    ctx
      .world
      .spawn(Joint::revolute(pivot, bob).at(Vec2::new(90.0, 110.0)));
    // Spring
    let hook = fixed_point(ctx, Vec2::new(440.0, 100.0));
    let weight = ctx
      .world
      .spawn((
        Transform::from_xy(440.0, 200.0),
        RigidBody::dynamic(),
        Collider::rect(30.0, 30.0),
      ))
      .id();
    ctx
      .world
      .spawn(Joint::spring(hook, weight, 80.0, 30.0, 2.0));
    // Bridge: planks hinged end to end between two posts
    let (left, right) = (Vec2::new(200.0, 300.0), Vec2::new(480.0, 300.0));
    let planks = 9;
    let step = (right.x - left.x) / planks as f32;
    let mut previous = fixed_point(ctx, left);
    for i in 0..planks {
      let centre = Vec2::new(left.x + step * (i as f32 + 0.5), left.y);
      let plank = ctx
        .world
        .spawn((
          Transform::from_translation(centre),
          RigidBody::dynamic(),
          Collider::rect(step - 2.0, 8.0),
        ))
        .id();
      let hinge = Vec2::new(left.x + step * i as f32, left.y);
      self.bridge.push(
        ctx
          .world
          .spawn(Joint::revolute(previous, plank).at(hinge))
          .id(),
      );
      previous = plank;
    }
    let post = fixed_point(ctx, right);
    self.bridge.push(
      ctx
        .world
        .spawn(Joint::revolute(previous, post).at(right))
        .id(),
    );
    // Car: a chassis on two motored wheels
    let chassis = ctx
      .world
      .spawn((
        Transform::from_xy(110.0, 545.0),
        RigidBody::dynamic(),
        Collider::rect(80.0, 18.0),
      ))
      .id();
    for x in [85.0, 135.0] {
      let at = Vec2::new(x, 570.0);
      let wheel = ctx
        .world
        .spawn((
          Transform::from_translation(at),
          RigidBody::dynamic(),
          Collider::circle(14.0).with_friction(1.0),
        ))
        .id();
      self.car.push(
        ctx
          .world
          .spawn(
            Joint::revolute(chassis, wheel)
              .at(at)
              .with_motor_velocity(self.motor, 50_000.0),
          )
          .id(),
      );
    }
  }
  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    if ctx.input.key_pressed_and_consume(KeyCode::Space) {
      ctx.world.spawn((
        Transform::from_xy(340.0, 160.0),
        RigidBody::dynamic(),
        Collider::rect(40.0, 40.0).with_mass(1.0),
      ));
    }
    // Settings apply in place: the joints are edited, not rebuilt
    let motor = Some(JointMotor::Velocity {
      target: self.motor,
      max_force: 50_000.0,
    });
    for &joint in &self.car {
      if let Some(mut j) = ctx.world.get_mut::<Joint>(joint)
        && j.motor != motor
      {
        j.motor = motor;
      }
    }
    let break_force = self.break_force();
    for &joint in &self.bridge {
      if let Some(mut j) = ctx.world.get_mut::<Joint>(joint)
        && j.break_force != break_force
      {
        j.break_force = break_force;
      }
    }
    self.broken += ctx.messages::<JointBroken>().len() as u32;
    draw_colliders(ctx, colour_of);
    draw_joints(ctx);
  }
  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    let area = content_area(ui);
    ui.draw_text(
      &format!("Broken joints: {}", self.broken),
      area.x,
      area.y,
      Color::WHITE,
    );
  }
}

// --- Queries --------------------------------------------------------------

#[derive(Default)]
struct QueriesPage {
  triggers: bool,
  report: String,
}

impl DemoPage for QueriesPage {
  fn name(&self) -> &str {
    "Queries"
  }
  fn description(&self) -> &str {
    "Move the mouse: a ray toward it, a circle cast down from above it, an overlap box and the nearest surface."
  }
  fn settings(&mut self, s: &mut Settings) {
    s.toggle("Hit triggers", &mut self.triggers);
  }
  fn controls(&self) -> Vec<Control> {
    vec![Control::mouse("Aim the queries")]
  }
  fn enter(&mut self, ctx: &mut SceneContext) {
    arena(ctx);
    for (x, y, w, h, turn) in [
      (150.0, 250.0, 80.0, 30.0, 0.3),
      (380.0, 200.0, 40.0, 120.0, -0.2),
      (260.0, 450.0, 140.0, 24.0, 0.0),
    ] {
      ctx.world.spawn((
        Transform::from_xy(x, y).with_rotation(turn),
        Collider::rect(w, h),
      ));
    }
    ctx
      .world
      .spawn((Transform::from_xy(420.0, 420.0), Collider::circle(40.0)));
    ctx.world.spawn((
      Transform::from_xy(120.0, 470.0),
      Collider::polygon([
        Vec2::new(-40.0, 30.0),
        Vec2::new(40.0, 30.0),
        Vec2::new(0.0, -40.0),
      ]),
    ));
    ctx.world.spawn((
      Transform::from_xy(300.0, 320.0),
      Collider::rect(90.0, 60.0).as_trigger(),
    ));
  }
  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    let mouse = mouse_world(ctx);
    let filter = if self.triggers {
      QueryFilter::new().with_triggers()
    } else {
      QueryFilter::new()
    };
    let origin = Vec2::new(40.0, 320.0);
    let probe = Collider::rect(120.0, 80.0);
    let (ray, cast, near, overlapping) = {
      let physics = ctx.physics();
      (
        physics.raycast(origin, mouse - origin, 700.0, &filter),
        physics.shape_cast(
          &ColliderShape::Circle { radius: 12.0 },
          Vec2::new(mouse.x, 90.0),
          0.0,
          Vec2::Y,
          600.0,
          &filter,
        ),
        physics.nearest(mouse, 200.0, &filter),
        physics.overlap_shape(&probe.shape, mouse, 0.0, &filter),
      )
    };
    draw_colliders(ctx, |_, entity, collider| {
      if overlapping.iter().any(|o| o.collider == entity) {
        HIGHLIGHT
      } else if collider.is_trigger {
        TRIGGER
      } else {
        FIXED
      }
    });
    let mut debug = ctx.debug_draw();
    debug.rect(mouse, Vec2::new(120.0, 80.0), Color::new(255, 255, 255, 90));
    match ray {
      Some(hit) => {
        debug.line(origin, hit.point, Color::new(255, 120, 120, 255));
        debug.arrow(
          hit.point,
          hit.point + hit.normal * 30.0,
          Color::new(255, 120, 120, 255),
        );
      }
      None => debug.line(
        origin,
        origin + (mouse - origin).normalize_or_zero() * 700.0,
        Color::new(255, 120, 120, 90),
      ),
    }
    if let Some(hit) = cast {
      let centre = Vec2::new(mouse.x, 90.0 + hit.distance);
      debug.line(
        Vec2::new(mouse.x, 90.0),
        centre,
        Color::new(120, 200, 255, 160),
      );
      debug.circle(centre, 12.0, Color::new(120, 200, 255, 255));
    }
    if let Some(near) = near {
      debug.line(mouse, near.point, Color::new(200, 255, 160, 255));
    }
    self.report = match ray {
      Some(hit) => format!(
        "Ray: hit at ({:.0}, {:.0}), {:.0} px away; {} overlapping",
        hit.point.x,
        hit.point.y,
        hit.distance,
        overlapping.len()
      ),
      None => format!("Ray: no hit; {} overlapping", overlapping.len()),
    };
  }
  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    let area = content_area(ui);
    ui.draw_text(&self.report, area.x, area.y, Color::WHITE);
  }
}

// --- Layers -----------------------------------------------------------------

const LAYER_COLOURS: [Color; 3] = [
  Color::new(240, 90, 80, 255),
  Color::new(90, 220, 110, 255),
  Color::new(90, 140, 255, 255),
];

struct LayersPage {
  threshold: f32,
  timer: f32,
  dropped: u32,
  zone: Option<Entity>,
  inside: usize,
  flashes: Vec<(Vec2, f32)>,
}

impl Default for LayersPage {
  fn default() -> Self {
    Self {
      threshold: 400.0,
      timer: 0.0,
      dropped: 0,
      zone: None,
      inside: 0,
      flashes: Vec::new(),
    }
  }
}

impl DemoPage for LayersPage {
  fn name(&self) -> &str {
    "Layers"
  }
  fn description(&self) -> &str {
    "Each shelf catches only its own colour; a trigger counts the balls in it; hard impacts flash. Impact threshold applies to bodies dropped afterwards."
  }
  fn settings(&mut self, s: &mut Settings) {
    s.float("Impact threshold", &mut self.threshold, 50.0..=2000.0, 50.0);
  }
  fn controls(&self) -> Vec<Control> {
    Vec::new()
  }
  fn enter(&mut self, ctx: &mut SceneContext) {
    arena(ctx);
    for (k, y) in [(0u32, 220.0), (1, 330.0), (2, 440.0)] {
      let layer = 1 << (k + 1);
      ctx.world.spawn((
        Transform::from_xy(150.0 + k as f32 * 110.0, y).with_rotation(0.15),
        Collider::rect(200.0, 12.0)
          .with_layer(layer)
          .with_mask(layer),
      ));
    }
    self.zone = Some(
      ctx
        .world
        .spawn((
          Transform::from_xy(270.0, 560.0),
          Collider::rect(500.0, 80.0).as_trigger(),
        ))
        .id(),
    );
  }
  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    self.timer += dt;
    if self.timer > 0.25 && self.dropped < 150 {
      self.timer = 0.0;
      let k = self.dropped % 3;
      ctx.world.spawn((
        Transform::from_xy(60.0 + noise(self.dropped) * 420.0, 100.0),
        RigidBody::dynamic(),
        Collider::circle(9.0)
          .with_layer(1 << (k + 1))
          .with_restitution(0.3)
          .with_force_events(self.threshold),
      ));
      self.dropped += 1;
    }
    for impact in ctx.messages::<ContactForce>() {
      if impact.started
        && let Some(global) = ctx.world.get::<GlobalTransform>(impact.body_a)
      {
        self.flashes.push((global.translation(), 0.3));
      }
    }
    self.flashes.retain_mut(|(_, life)| {
      *life -= dt;
      *life > 0.0
    });
    if let Some(zone) = self.zone {
      self.inside = ctx.physics().overlaps(zone).len();
    }
    draw_colliders(ctx, |_, _, collider| {
      if collider.is_trigger {
        TRIGGER
      } else {
        (0..3)
          .find(|k| collider.layer == 1 << (k + 1))
          .map_or(FIXED, |k| LAYER_COLOURS[k])
      }
    });
    let flashes = self.flashes.clone();
    let mut debug = ctx.debug_draw();
    for (at, life) in flashes {
      debug.circle(at, 24.0 * (1.0 - life / 0.3) + 8.0, Color::WHITE);
    }
  }
  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    let area = content_area(ui);
    ui.draw_text(
      &format!("In the trigger: {}", self.inside),
      area.x,
      area.y,
      Color::WHITE,
    );
  }
}
