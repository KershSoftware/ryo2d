//! Steering Behaviors Demo - Ryo2D Example
//!
//! This example demonstrates steering behaviors for AI movement:
//! - Separation: Avoid crowding nearby boids
//! - Cohesion: Steer toward average position of neighbors
//! - Alignment: Match velocity with nearby boids
//! - Flee: Escape from predator (mouse position when held)
//!
//! Runs on the demo-kit shell; the weights and trails are settings.

use demo_kit::prelude::*;

const INITIAL_BOIDS: usize = 50;
const BOID_SPEED: f32 = 120.0;
const NEIGHBOR_RADIUS: f32 = 80.0;
const PREDATOR_FEAR_RADIUS: f32 = 150.0;

/// Boid component with velocity
#[derive(Debug, Clone)]
struct Boid {
  velocity: Vec2,
  #[allow(dead_code)]
  color: (u8, u8, u8),
}

/// Predator marker (follows mouse when active)
#[derive(Debug, Clone)]
struct Predator;

struct SteeringDemoScene {
  boids: Vec<Entity>,
  predator: Option<Entity>,
  screen_w: f32,
  screen_h: f32,

  // Steering weights
  separation_weight: f32,
  cohesion_weight: f32,
  alignment_weight: f32,
  flee_weight: f32,

  // Visual options
  show_trails: bool,
}

impl SteeringDemoScene {
  fn new() -> Self {
    Self {
      boids: Vec::new(),
      predator: None,
      screen_w: 1280.0,
      screen_h: 720.0,
      separation_weight: 1.5,
      cohesion_weight: 1.0,
      alignment_weight: 1.0,
      flee_weight: 2.5,
      show_trails: true,
    }
  }

  fn spawn_boid(&mut self, ctx: &mut SceneContext, x: f32, y: f32) {
    let mut rng = rand::thread_rng();

    // Random color
    let colors = [
      (100, 200, 255), // Blue
      (255, 150, 100), // Orange
      (100, 255, 150), // Green
      (255, 100, 200), // Pink
      (200, 200, 100), // Yellow
      (200, 100, 255), // Purple
      (100, 255, 255), // Cyan
      (255, 200, 150), // Peach
    ];
    let (r, g, b) = colors[rng.gen_range(0..colors.len())];

    // Random initial velocity
    let angle = rng.gen_range(0.0..std::f32::consts::TAU);
    let velocity = Vec2::new(angle.cos(), angle.sin()) * BOID_SPEED;

    // Create sprite
    let sprite = Sprite::solid_rect(8, 8, r, g, b, 255);
    let handle = SpriteHandle::from_sprite(sprite, format!("boid_{}", self.boids.len()));

    // Trail with boid's color
    // min_distance must be less than distance traveled per frame:
    // at 120 px/s and 60 FPS = 2 px/frame, so use 1.0 to capture movement
    let trail = Trail2D::new(Color::new(r, g, b, 200), 3.0)
      .with_z_order(10) // With the boid, so the trail stays in front of the ground
      .with_length(25)
      .with_fade(true)
      .with_min_distance(1.0);

    let entity = ctx.world.spawn((
      Boid {
        velocity,
        color: (r, g, b),
      },
      Transform::new(x, y).with_pivot_center(),
      Renderable::sprite(handle).with_z_order(10),
      trail,
    ));

    self.boids.push(entity);
  }

  fn spawn_initial_boids(&mut self, ctx: &mut SceneContext) {
    let mut rng = rand::thread_rng();

    for _ in 0..INITIAL_BOIDS {
      let x = rng.gen_range(50.0..self.screen_w - 50.0);
      let y = rng.gen_range(50.0..self.screen_h - 50.0);
      self.spawn_boid(ctx, x, y);
    }
  }

  fn spawn_predator(&mut self, ctx: &mut SceneContext, x: f32, y: f32) {
    if self.predator.is_some() {
      return;
    }

    let sprite = Sprite::solid_rect(20, 20, 255, 50, 50, 255);
    let handle = SpriteHandle::from_sprite(sprite, "predator");

    let entity = ctx.world.spawn((
      Predator,
      Transform::new(x, y).with_pivot_center(),
      Renderable::sprite(handle).with_z_order(20),
    ));

    self.predator = Some(entity);
  }

  fn despawn_predator(&mut self, ctx: &mut SceneContext) {
    if let Some(entity) = self.predator.take() {
      let _ = ctx.world.despawn(entity);
    }
  }

  fn update_boids(&mut self, ctx: &mut SceneContext, dt: f32) {
    // Collect all boid data first (position, velocity, radius for steering)
    let boid_data: Vec<(Entity, Vec2, Vec2)> = self
      .boids
      .iter()
      .filter_map(|&e| {
        let t = ctx.world.get::<Transform>(e)?;
        let b = ctx.world.get::<Boid>(e)?;
        Some((e, Vec2::new(t.x, t.y), b.velocity))
      })
      .collect();

    // Get predator position if active
    let predator_pos = self.predator.and_then(|e| {
      let t = ctx.world.get::<Transform>(e)?;
      Some(Vec2::new(t.x, t.y))
    });

    // Update each boid
    for &(entity, pos, vel) in &boid_data {
      // Find neighbors
      let neighbors: Vec<(Vec2, f32)> = boid_data
        .iter()
        .filter(|(e, _, _)| *e != entity)
        .filter(|(_, p, _)| pos.distance(*p) < NEIGHBOR_RADIUS)
        .map(|(_, p, _)| (*p, 8.0)) // 8.0 = boid radius
        .collect();

      let neighbors_with_vel: Vec<(Vec2, Vec2)> = boid_data
        .iter()
        .filter(|(e, _, _)| *e != entity)
        .filter(|(_, p, _)| pos.distance(*p) < NEIGHBOR_RADIUS)
        .map(|(_, p, v)| (*p, *v))
        .collect();

      // Calculate steering forces
      let sep = steering::separation(pos, 8.0, &neighbors, 50.0) * self.separation_weight;
      let coh = steering::cohesion(pos, &neighbors, BOID_SPEED) * self.cohesion_weight;
      let ali = steering::alignment(&neighbors_with_vel, BOID_SPEED) * self.alignment_weight;

      // Flee from predator if nearby
      let flee = if let Some(pred_pos) = predator_pos {
        if pos.distance(pred_pos) < PREDATOR_FEAR_RADIUS {
          steering::flee(pos, pred_pos, BOID_SPEED * 1.5) * self.flee_weight
        } else {
          Vec2::ZERO
        }
      } else {
        Vec2::ZERO
      };

      // Combine forces
      let steering_force = sep + coh + ali + flee;

      // Update velocity
      let new_vel = if let Some(mut boid) = ctx.world.get_mut::<Boid>(entity) {
        boid.velocity = (boid.velocity + steering_force * dt * 3.0).clamp_length_max(BOID_SPEED);

        // Ensure minimum speed
        if boid.velocity.length() < BOID_SPEED * 0.3 {
          boid.velocity = boid.velocity.normalize_or_zero() * BOID_SPEED * 0.3;
        }

        boid.velocity
      } else {
        vel
      };

      // Update position
      if let Some(mut transform) = ctx.world.get_mut::<Transform>(entity) {
        transform.x += new_vel.x * dt;
        transform.y += new_vel.y * dt;

        // Wrap around edges - clear trail when wrapping to prevent line across screen
        let mut wrapped = false;
        if transform.x < 0.0 {
          transform.x += self.screen_w;
          wrapped = true;
        }
        if transform.x > self.screen_w {
          transform.x -= self.screen_w;
          wrapped = true;
        }
        if transform.y < 0.0 {
          transform.y += self.screen_h;
          wrapped = true;
        }
        if transform.y > self.screen_h {
          transform.y -= self.screen_h;
          wrapped = true;
        }

        // Clear trail when boid wraps to prevent line shooting across screen
        if wrapped && let Some(mut trail) = ctx.world.get_mut::<Trail2D>(entity) {
          trail.clear();
        }

        // Update rotation to face velocity
        transform.rotation = new_vel.y.atan2(new_vel.x);
      }
    }
  }

  fn update_trails(&self, ctx: &mut SceneContext, _dt: f32) {
    // Engine automatically updates trails from Transform position.
    // We only need to toggle enabled state for visibility.
    for &entity in &self.boids {
      if let Some(mut trail) = ctx.world.get_mut::<Trail2D>(entity) {
        trail.set_enabled(self.show_trails);
      }
    }
  }
}

impl DemoPage for SteeringDemoScene {
  fn name(&self) -> &str {
    "Boids"
  }

  fn description(&self) -> &str {
    "Boids steer by separation, cohesion and alignment, and flee a predator you hold with the mouse"
  }

  fn settings(&mut self, s: &mut Settings) {
    s.float("Separation", &mut self.separation_weight, 0.0..=5.0, 0.5);
    s.float("Cohesion", &mut self.cohesion_weight, 0.0..=5.0, 0.5);
    s.float("Alignment", &mut self.alignment_weight, 0.0..=5.0, 0.5);
    s.toggle("Trails", &mut self.show_trails);
  }

  fn controls(&self) -> Vec<Control> {
    vec![
      Control::key(KeyCode::Space, "Add 10 boids"),
      Control::mouse("Hold left button: predator"),
    ]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    let (vw, vh) = ctx.camera.viewport_size();
    self.screen_w = vw;
    self.screen_h = vh;
    ctx.camera.set_position(vw / 2.0, vh / 2.0);

    self.spawn_initial_boids(ctx);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    // Spawn more boids
    if ctx.input.key_pressed(KeyCode::Space) {
      let mut rng = rand::thread_rng();
      for _ in 0..10 {
        let x = rng.gen_range(50.0..self.screen_w - 50.0);
        let y = rng.gen_range(50.0..self.screen_h - 50.0);
        self.spawn_boid(ctx, x, y);
      }
      println!("Total boids: {}", self.boids.len());
    }

    // Predator - spawn on mouse hold, despawn on release
    let (mx, my) = ctx.input.mouse_position();
    let (wx, wy) = ctx.camera.screen_to_world(mx, my);

    if ctx.input.mouse_down(MouseButton::Left) {
      if self.predator.is_none() {
        self.spawn_predator(ctx, wx, wy);
      } else if let Some(pred) = self.predator {
        // Move predator to mouse
        if let Some(mut t) = ctx.world.get_mut::<Transform>(pred) {
          t.x = wx;
          t.y = wy;
        }
      }
    } else {
      self.despawn_predator(ctx);
    }

    // Update boids
    self.update_boids(ctx, dt);
    self.update_trails(ctx, dt);
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let area = content_area(ui);
    let text_accent = Color::rgb(130, 180, 255);
    let text_warn = Color::rgb(255, 150, 100);

    let stats = format!("Boids: {}", self.boids.len());
    ui.draw_text(&stats, 10.0, area.y + 8.0, text_accent);

    // Predator indicator
    if self.predator.is_some() {
      ui.draw_text("PREDATOR ACTIVE", 10.0, area.y + 28.0, text_warn);
    }
  }
}

fn main() {
  Demo::new("Steering")
    .resolution(1280, 720)
    .page(SteeringDemoScene::new)
    .run();
}
