//! Lighting Demo
//!
//! Demonstrates the 2D lighting system with a pendulum lamp swinging in a dark room.
//!
//! Features:
//! - Dynamic point light attached to a swinging lamp
//! - Light occluders casting shadows
//! - Various shapes to demonstrate lighting effects
//! - Pendulum physics simulation

use ryo2d::prelude::*;
use std::f32::consts::PI;

// Pendulum physics constants
const PENDULUM_LENGTH: f32 = 200.0;
const PENDULUM_ANCHOR_Y: f32 = 50.0;
const PENDULUM_GRAVITY: f32 = 980.0; // pixels/s²
const PENDULUM_DAMPING: f32 = 0.995; // velocity decay per frame
const INITIAL_ANGLE: f32 = PI / 6.0; // 30 degrees

// Bulb secondary pendulum physics (rotation only - pivots at connection point)
const BULB_EFFECTIVE_LENGTH: f32 = 20.0; // effective pendulum length for physics feel
const BULB_DAMPING: f32 = 0.98; // bulb swing damping
const BULB_GRAVITY: f32 = 980.0; // gravity for bulb pendulum
const BULB_RADIUS: f32 = 12.0; // half the bulb diameter, for centering

/// Pendulum state for physics simulation
struct PendulumState {
  angle: f32,            // current angle in radians (0 = straight down)
  angular_velocity: f32, // angular velocity in radians/s
  anchor_x: f32,         // x position of anchor point
  anchor_y: f32,         // y position of anchor point
  prev_bob_x: f32,       // previous bob x for velocity tracking
  prev_bob_y: f32,       // previous bob y for velocity tracking
  bob_vel_x: f32,        // bob velocity x
  bob_vel_y: f32,        // bob velocity y
}

impl PendulumState {
  fn new(anchor_x: f32, anchor_y: f32, initial_angle: f32) -> Self {
    let bob_x = anchor_x + PENDULUM_LENGTH * initial_angle.sin();
    let bob_y = anchor_y + PENDULUM_LENGTH * initial_angle.cos();
    Self {
      angle: initial_angle,
      angular_velocity: 0.0,
      anchor_x,
      anchor_y,
      prev_bob_x: bob_x,
      prev_bob_y: bob_y,
      bob_vel_x: 0.0,
      bob_vel_y: 0.0,
    }
  }

  /// Update pendulum physics
  fn update(&mut self, dt: f32) {
    // Store previous position for velocity calculation
    let (old_x, old_y) = self.bob_position();
    self.prev_bob_x = old_x;
    self.prev_bob_y = old_y;

    // Angular acceleration from gravity: α = -(g/L) * sin(θ)
    let angular_acceleration = -(PENDULUM_GRAVITY / PENDULUM_LENGTH) * self.angle.sin();

    // Update angular velocity and angle
    self.angular_velocity += angular_acceleration * dt;
    self.angular_velocity *= PENDULUM_DAMPING; // damping
    self.angle += self.angular_velocity * dt;

    // Calculate bob velocity
    let (new_x, new_y) = self.bob_position();
    if dt > 0.0 {
      self.bob_vel_x = (new_x - self.prev_bob_x) / dt;
      self.bob_vel_y = (new_y - self.prev_bob_y) / dt;
    }
  }

  /// Get the current bob (lamp) position
  fn bob_position(&self) -> (f32, f32) {
    let x = self.anchor_x + PENDULUM_LENGTH * self.angle.sin();
    let y = self.anchor_y + PENDULUM_LENGTH * self.angle.cos();
    (x, y)
  }
}

/// Main lighting demo scene
struct LightingScene {
  pendulum: PendulumState,
  rope_entities: Vec<Entity>, // Store rope segment entities for reliable updates
  bulb_entity: Option<Entity>,
  light_entity: Option<Entity>,
  // Bulb secondary pendulum state
  bulb_angle: f32,            // angle relative to straight down
  bulb_angular_velocity: f32, // angular velocity of bulb swing
  // Light properties (adjustable with arrow keys)
  light_intensity: f32,
  light_radius: f32,
  // Ambient darkness (adjustable with Q/E keys)
  ambient_intensity: f32,
}

impl LightingScene {
  fn new() -> Self {
    Self {
      pendulum: PendulumState::new(640.0, PENDULUM_ANCHOR_Y, INITIAL_ANGLE),
      rope_entities: Vec::new(),
      bulb_entity: None,
      light_entity: None,
      bulb_angle: 0.0,
      bulb_angular_velocity: 0.0,
      light_intensity: 1.2,
      light_radius: 250.0,
      ambient_intensity: 0.0, // Start pitch black
    }
  }
}

impl Scene for LightingScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    // Configure lighting to not affect UI - keeps text readable in darkness
    if let Some(lighting) = ctx.lighting {
      lighting.set_ui_affected_by_lighting(false);
    }

    // Create rope segments (visual only, connected chain of rectangles)
    let rope_segments = 20;
    let segment_length = PENDULUM_LENGTH / rope_segments as f32;

    for i in 0..rope_segments {
      // Each segment starts at point i and extends toward point i+1
      let t = i as f32 / rope_segments as f32;
      let x = self.pendulum.anchor_x + PENDULUM_LENGTH * self.pendulum.angle.sin() * t;
      let y = self.pendulum.anchor_y + PENDULUM_LENGTH * self.pendulum.angle.cos() * t;

      // Segment is tall enough to connect to next segment (with slight overlap)
      let rope_sprite = ctx.assets.create_sprite(
        Sprite::solid_rect(4, (segment_length + 2.0) as u32, 139, 90, 43, 255), // brown rope
        &format!("rope_segment_{}", i),
      );

      // Use top-center pivot so segment rotates from its top attachment point
      // Negate angle because positive pendulum angle swings right, but positive
      // rotation is counterclockwise (appears as left-tilt on screen)
      let entity = ctx.world.spawn((
        Transform::new(x, y)
          .with_pivot_top_center()
          .with_rotation(-self.pendulum.angle),
        Renderable::sprite(rope_sprite).with_z_order(10),
      ));
      self.rope_entities.push(entity);
    }

    // Create lamp shade (triangle pointing down) at the bob position
    let (bob_x, bob_y) = self.pendulum.bob_position();

    // Create the light bulb at the end of the rope
    let bulb_sprite = ctx.assets.create_sprite(
      Sprite::solid_circle(24, 255, 255, 200, 255), // warm white bulb
      "bulb",
    );

    // Use pivot_top_center so the bulb rotates from its connection point with the rope
    self.bulb_entity = Some(ctx.world.spawn((
      Transform::new(bob_x, bob_y).with_pivot_top_center(),
      Renderable::sprite(bulb_sprite).with_z_order(12),
    )));

    // Add the light source attached to the bulb position
    self.light_entity = Some(ctx.world.spawn((
      Transform::new(bob_x, bob_y),
      Light2D::torch(self.light_radius).with_intensity(self.light_intensity),
    )));

    // Add some floor shapes to demonstrate lighting

    // Floor occluders (walls)
    ctx.world.spawn((
      Transform::new(640.0, 650.0).with_pivot_center(),
      LightOccluder2D::rect(1280.0, 100.0),
    ));

    // Left wall
    ctx.world.spawn((
      Transform::new(50.0, 360.0).with_pivot_center(),
      LightOccluder2D::rect(20.0, 720.0),
    ));

    // Right wall
    ctx.world.spawn((
      Transform::new(1230.0, 360.0).with_pivot_center(),
      LightOccluder2D::rect(20.0, 720.0),
    ));

    // Ceiling
    ctx.world.spawn((
      Transform::new(640.0, 20.0).with_pivot_center(),
      LightOccluder2D::rect(1280.0, 40.0),
    ));

    // Floor shapes - boxes on the floor
    let box_positions = [
      (200.0, 600.0, 60.0, 60.0, Color::new(100, 150, 200, 255)),
      (400.0, 580.0, 80.0, 80.0, Color::new(200, 100, 100, 255)),
      (900.0, 590.0, 70.0, 70.0, Color::new(100, 200, 100, 255)),
      (1100.0, 600.0, 60.0, 60.0, Color::new(200, 200, 100, 255)),
    ];

    for (i, (x, y, w, h, color)) in box_positions.iter().enumerate() {
      let sprite = ctx.assets.create_sprite(
        Sprite::solid_rect(*w as u32, *h as u32, color.r, color.g, color.b, color.a),
        &format!("box_{}", i),
      );

      ctx.world.spawn((
        Transform::new(*x, *y).with_pivot_center(),
        Renderable::sprite(sprite).with_z_order(5),
        LightOccluder2D::rect(*w, *h),
      ));
    }

    // Floating shapes (to show off-floor lighting)
    let floating_shapes = [
      (300.0, 300.0, 50.0, 50.0, Color::new(180, 100, 220, 255)),
      (800.0, 400.0, 40.0, 40.0, Color::new(100, 220, 180, 255)),
      (1000.0, 250.0, 45.0, 45.0, Color::new(220, 180, 100, 255)),
    ];

    for (i, (x, y, w, h, color)) in floating_shapes.iter().enumerate() {
      let sprite = ctx.assets.create_sprite(
        Sprite::solid_rect(*w as u32, *h as u32, color.r, color.g, color.b, color.a),
        &format!("floating_{}", i),
      );

      ctx.world.spawn((
        Transform::new(*x, *y).with_pivot_center(),
        Renderable::sprite(sprite).with_z_order(5),
        LightOccluder2D::rect(*w, *h),
      ));
    }

    // Central pillar
    let pillar_sprite = ctx
      .assets
      .create_sprite(Sprite::solid_rect(40, 200, 80, 80, 80, 255), "pillar");

    ctx.world.spawn((
      Transform::new(640.0, 450.0).with_pivot_center(),
      Renderable::sprite(pillar_sprite).with_z_order(5),
      LightOccluder2D::rect(40.0, 200.0),
    ));
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    // Quit on Escape
    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Quit;
    }

    // Reset pendulum on Space
    if ctx.input.key_pressed(KeyCode::Space) {
      self.pendulum = PendulumState::new(640.0, PENDULUM_ANCHOR_Y, INITIAL_ANGLE);
      self.bulb_angle = 0.0;
      self.bulb_angular_velocity = 0.0;
    }

    // Adjust light intensity with Up/Down arrows
    if ctx.input.key_down(KeyCode::ArrowUp) {
      self.light_intensity = (self.light_intensity + 0.02).min(3.0);
    }
    if ctx.input.key_down(KeyCode::ArrowDown) {
      self.light_intensity = (self.light_intensity - 0.02).max(0.1);
    }

    // Adjust light radius with Left/Right arrows
    if ctx.input.key_down(KeyCode::ArrowRight) {
      self.light_radius = (self.light_radius + 2.0).min(500.0);
    }
    if ctx.input.key_down(KeyCode::ArrowLeft) {
      self.light_radius = (self.light_radius - 2.0).max(50.0);
    }

    // Adjust ambient darkness with Q/E keys
    if ctx.input.key_down(KeyCode::KeyQ) {
      self.ambient_intensity = (self.ambient_intensity - 0.01).max(0.0);
    }
    if ctx.input.key_down(KeyCode::KeyE) {
      self.ambient_intensity = (self.ambient_intensity + 0.01).min(1.0);
    }

    // Update ambient light if it changed
    if let Some(lighting) = ctx.lighting {
      lighting.ambient_mut().set_intensity(self.ambient_intensity);
    }

    // Update light properties if they changed
    if let Some(light_entity) = self.light_entity {
      if let Some(mut light) = ctx.world.get_mut::<Light2D>(light_entity) {
        *light = Light2D::torch(self.light_radius).with_intensity(self.light_intensity);
      }
    }

    // Update pendulum physics
    self.pendulum.update(dt);
    let (bob_x, bob_y) = self.pendulum.bob_position();

    // Calculate horizontal acceleration of the rope end
    // When rope decelerates (changes direction), this force swings the bulb
    let bob_accel_x = -self.pendulum.angular_velocity
      * self.pendulum.angular_velocity
      * PENDULUM_LENGTH
      * self.pendulum.angle.sin()
      - (PENDULUM_GRAVITY / PENDULUM_LENGTH)
        * self.pendulum.angle.sin()
        * PENDULUM_LENGTH
        * self.pendulum.angle.cos();

    // Bulb secondary pendulum physics (rotation about connection point)
    // The "fictitious force" from rope acceleration acts on the bulb
    let fictitious_acceleration = -bob_accel_x / BULB_EFFECTIVE_LENGTH * 0.3; // scale factor for feel

    // Gravity restoring force on bulb pendulum
    let gravity_acceleration = -(BULB_GRAVITY / BULB_EFFECTIVE_LENGTH) * self.bulb_angle.sin();

    // Update bulb angular velocity and angle
    self.bulb_angular_velocity += (gravity_acceleration + fictitious_acceleration) * dt;
    self.bulb_angular_velocity *= BULB_DAMPING;
    self.bulb_angle += self.bulb_angular_velocity * dt;

    // Clamp bulb angle to prevent wild swings
    self.bulb_angle = self.bulb_angle.clamp(-PI / 4.0, PI / 4.0);

    // With pivot_top_center, we just set position to the rope end and rotation
    // The engine automatically rotates around the top-center pivot point
    if let Some(bulb_entity) = self.bulb_entity {
      if let Some(mut transform) = ctx.world.get_mut::<Transform>(bulb_entity) {
        transform.x = bob_x;
        transform.y = bob_y;
        transform.rotation = self.bulb_angle;
      }
    }

    // Calculate where the bulb center ends up after pivot rotation (for light positioning)
    let bulb_center_x = bob_x + BULB_RADIUS * self.bulb_angle.sin();
    let bulb_center_y = bob_y + BULB_RADIUS * self.bulb_angle.cos();

    // Update light position (at center of bulb)
    if let Some(light_entity) = self.light_entity {
      if let Some(mut transform) = ctx.world.get_mut::<Transform>(light_entity) {
        transform.x = bulb_center_x;
        transform.y = bulb_center_y;
      }
    }

    // Update rope segments to follow pendulum (using stored entity IDs for correct order)
    for (i, &entity) in self.rope_entities.iter().enumerate() {
      let t = i as f32 / self.rope_entities.len() as f32;
      let x = self.pendulum.anchor_x + PENDULUM_LENGTH * self.pendulum.angle.sin() * t;
      let y = self.pendulum.anchor_y + PENDULUM_LENGTH * self.pendulum.angle.cos() * t;

      if let Some(mut transform) = ctx.world.get_mut::<Transform>(entity) {
        transform.x = x;
        transform.y = y;
        transform.rotation = -self.pendulum.angle;
      }
    }
    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    ui.label(
      "Lighting Demo",
      10.0,
      10.0,
      Color::WHITE,
      Some(Color::new(0, 0, 0, 180)),
    );

    ui.label(
      "Press SPACE to reset pendulum",
      10.0,
      35.0,
      Color::new(200, 200, 200, 255),
      Some(Color::new(0, 0, 0, 160)),
    );

    ui.label(
      "Arrow UP/DOWN: Adjust light intensity",
      10.0,
      60.0,
      Color::new(200, 200, 200, 255),
      Some(Color::new(0, 0, 0, 160)),
    );

    ui.label(
      "Arrow LEFT/RIGHT: Adjust light radius",
      10.0,
      85.0,
      Color::new(200, 200, 200, 255),
      Some(Color::new(0, 0, 0, 160)),
    );

    ui.label(
      "Q/E: Decrease/Increase darkness",
      10.0,
      110.0,
      Color::new(200, 200, 200, 255),
      Some(Color::new(0, 0, 0, 160)),
    );

    ui.label(
      &format!("Light Intensity: {:.2}", self.light_intensity),
      10.0,
      135.0,
      Color::new(255, 255, 100, 255),
      Some(Color::new(0, 0, 0, 160)),
    );

    ui.label(
      &format!("Light Radius: {:.0}", self.light_radius),
      10.0,
      160.0,
      Color::new(255, 255, 100, 255),
      Some(Color::new(0, 0, 0, 160)),
    );

    ui.label(
      &format!("Ambient Darkness: {:.2}", 1.0 - self.ambient_intensity),
      10.0,
      185.0,
      Color::new(255, 255, 100, 255),
      Some(Color::new(0, 0, 0, 160)),
    );

    ui.label(
      "Press ESC to quit",
      10.0,
      210.0,
      Color::new(200, 200, 200, 255),
      Some(Color::new(0, 0, 0, 160)),
    );
  }
}

fn main() {
  let engine = EngineBuilder::new()
    .title("Lighting Demo - Pendulum Lamp")
    .resolution(1280, 720)
    .fps(60)
    .with_scene(LightingScene::new())
    .build();

  if let Err(e) = engine.run() {
    eprintln!("Engine error: {}", e);
    std::process::exit(1);
  }
}
