//! Particle Physics Demo - Ryo2D
//!
//! Demonstrates rendering 1000+ particles with physics:
//! - Gravity-driven falling
//! - Collision with walls and each other
//! - Three different rendering modes
//!
//! ## Scenes
//!
//! 1. **Created Sprites**: Procedurally generated colored circles
//! 2. **Imported Sprites**: Individual image-based particles
//! 3. **Hierarchical Sprites**: Parent-child particle clusters
//!
//! ## Controls
//!
//! - 1/2/3: Switch between scenes
//! - Space: Spawn more particles (100 at a time)
//! - R: Reset scene
//! - Escape: Quit

use ryo2d::prelude::*;

// GPU batching types

const WIDTH: u32 = 800;
const HEIGHT: u32 = 600;
const PARTICLE_RADIUS: f32 = 4.0;
const INITIAL_PARTICLES: usize = 1000;
const SPAWN_BATCH: usize = 100;
const MAX_PARTICLES: usize = 2000;

// Collision layers
const LAYER_PARTICLE: u32 = 1;
const LAYER_WALL: u32 = 2;

/// Which scene/rendering mode is active
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DemoScene {
  /// Procedurally generated colored circles
  CreatedSprites,
  /// Image-based particle sprites
  ImportedSprites,
  /// Parent-child particle clusters
  HierarchicalSprites,
}

impl DemoScene {
  fn name(&self) -> &'static str {
    match self {
      DemoScene::CreatedSprites => "Created Sprites",
      DemoScene::ImportedSprites => "Imported Sprites",
      DemoScene::HierarchicalSprites => "Hierarchical Sprites",
    }
  }
}

/// Marker component for particles
#[derive(Debug)]
struct Particle;

/// Stores which sprite index this particle uses (for GPU batching)
#[derive(Debug)]
struct ParticleSpriteIndex(usize);

/// Marker component for walls
#[derive(Debug)]
struct Wall;

struct ParticlesDemo {
  current_scene: DemoScene,
  particle_count: usize,
  // Sprite handles
  particle_sprites: Vec<SpriteHandle>,
  imported_sprite: Option<SpriteHandle>,
  nav_cooldown: f32,
}

impl ParticlesDemo {
  fn new() -> Self {
    Self {
      current_scene: DemoScene::CreatedSprites,
      particle_count: 0,
      particle_sprites: Vec::new(),
      imported_sprite: None,
      nav_cooldown: 0.0,
    }
  }

  /// Create a variety of colored particle sprites
  fn create_particle_sprites(&mut self) {
    // Create 8 different colored particle sprites
    let colors = [
      (255, 100, 100), // Red
      (255, 180, 100), // Orange
      (255, 255, 100), // Yellow
      (100, 255, 100), // Green
      (100, 255, 255), // Cyan
      (100, 100, 255), // Blue
      (255, 100, 255), // Magenta
      (255, 255, 255), // White
    ];

    for (i, (r, g, b)) in colors.iter().enumerate() {
      let sprite = create_particle_sprite(*r, *g, *b);
      let handle = SpriteHandle::from_sprite(sprite, &format!("particle_{}", i));
      self.particle_sprites.push(handle);
    }
  }

  /// Create the "imported" sprite (simulated as a generated gradient particle)
  fn create_imported_sprite(&mut self) {
    // Create a more complex particle with gradient and glow effect
    let sprite = create_glow_particle();
    self.imported_sprite = Some(SpriteHandle::from_sprite(sprite, "imported_particle"));
  }

  /// Spawn walls around the screen boundaries
  fn spawn_walls(&self, world: &mut World) {
    let wall_thickness = 20.0;

    // Bottom wall (flush with bottom edge of viewport)
    // Transform is top-left corner, collider auto-centers
    world.spawn((
      Wall,
      Transform::new(0.0, HEIGHT as f32 - wall_thickness),
      RigidBody::kinematic(),
      Collider::box_collider(WIDTH as f32, wall_thickness)
        .with_layer(LAYER_WALL)
        .with_restitution(0.7)
        .with_friction(0.1),
    ));

    // Left wall (flush with left edge of viewport)
    world.spawn((
      Wall,
      Transform::new(0.0, 0.0),
      RigidBody::kinematic(),
      Collider::box_collider(wall_thickness, HEIGHT as f32)
        .with_layer(LAYER_WALL)
        .with_restitution(0.7)
        .with_friction(0.1),
    ));

    // Right wall (flush with right edge of viewport)
    world.spawn((
      Wall,
      Transform::new(WIDTH as f32 - wall_thickness, 0.0),
      RigidBody::kinematic(),
      Collider::box_collider(wall_thickness, HEIGHT as f32)
        .with_layer(LAYER_WALL)
        .with_restitution(0.7)
        .with_friction(0.1),
    ));
  }

  /// Spawn a batch of particles at random positions at the top
  fn spawn_particles(&mut self, ctx: &mut GameContext, count: usize) {
    let to_spawn = count.min(MAX_PARTICLES - self.particle_count);

    for _ in 0..to_spawn {
      self.spawn_single_particle(ctx);
    }
  }

  /// Spawn a single particle based on current scene
  fn spawn_single_particle(&mut self, ctx: &mut GameContext) {
    if self.particle_count >= MAX_PARTICLES {
      return;
    }

    // Random position across top of screen
    let x = rand_range(50.0, WIDTH as f32 - 50.0);
    let y = rand_range(-50.0, 50.0);

    // Random initial velocity
    let vx = rand_range(-50.0, 50.0);
    let vy = rand_range(0.0, 100.0);

    match self.current_scene {
      DemoScene::CreatedSprites => {
        self.spawn_created_particle(ctx.world, x, y, vx, vy);
      }
      DemoScene::ImportedSprites => {
        self.spawn_imported_particle(ctx.world, x, y, vx, vy);
      }
      DemoScene::HierarchicalSprites => {
        self.spawn_hierarchical_particle(ctx.world, x, y, vx, vy);
      }
    }

    self.particle_count += 1;
  }

  fn spawn_created_particle(&self, world: &mut World, x: f32, y: f32, vx: f32, vy: f32) {
    let color_idx = (self.particle_count % self.particle_sprites.len()) as usize;

    // Store which sprite index this particle uses (for GPU batching lookup)
    world.spawn((
      Particle,
      ParticleSpriteIndex(color_idx),
      Transform::new(x, y),
      Velocity::new(vx, vy),
      // No Renderable - we'll use GPU batching in render_gpu()
      RigidBody::new().with_gravity_scale(1.0).with_drag(0.01),
      Collider::circle_collider(PARTICLE_RADIUS)
        .with_layer(LAYER_PARTICLE)
        .with_mask(LAYER_PARTICLE | LAYER_WALL)
        .with_restitution(0.8)
        .with_friction(0.1),
    ));
  }

  fn spawn_imported_particle(&self, world: &mut World, x: f32, y: f32, vx: f32, vy: f32) {
    // Use a special index (usize::MAX) to indicate imported sprite
    world.spawn((
      Particle,
      ParticleSpriteIndex(usize::MAX),
      Transform::new(x, y),
      Velocity::new(vx, vy),
      RigidBody::new().with_gravity_scale(1.0).with_drag(0.01),
      Collider::circle_collider(PARTICLE_RADIUS * 1.5)
        .with_layer(LAYER_PARTICLE)
        .with_mask(LAYER_PARTICLE | LAYER_WALL)
        .with_restitution(0.85)
        .with_friction(0.05),
    ));
  }

  fn spawn_hierarchical_particle(&mut self, world: &mut World, x: f32, y: f32, vx: f32, vy: f32) {
    // Hierarchical mode uses larger scaled particles
    let parent_idx = self.particle_count % self.particle_sprites.len();

    let mut parent_transform = Transform::new(x, y);
    parent_transform.scale_x = 1.5;
    parent_transform.scale_y = 1.5;

    world.spawn((
      Particle,
      ParticleSpriteIndex(parent_idx),
      parent_transform,
      Velocity::new(vx, vy),
      RigidBody::new().with_gravity_scale(1.0).with_drag(0.02),
      Collider::circle_collider(PARTICLE_RADIUS * 2.0)
        .with_layer(LAYER_PARTICLE)
        .with_mask(LAYER_PARTICLE | LAYER_WALL)
        .with_restitution(0.75)
        .with_friction(0.15),
    ));
  }

  /// Clear all particles and reset the scene
  fn reset_scene(&mut self, ctx: &mut GameContext) {
    // Despawn all particles
    let particles = ctx.world.entities_with::<Particle>();
    for entity in particles {
      let _ = ctx.world.despawn(entity);
    }

    self.particle_count = 0;

    // Spawn initial batch
    self.spawn_particles(ctx, INITIAL_PARTICLES);
  }
}

impl Game for ParticlesDemo {
  fn init(&mut self, ctx: &mut GameContext) {
    println!("Particles Demo - 1000+ Sprites with Physics!");
    println!();
    println!("Controls:");
    println!("  1/2/3 - Switch scenes");
    println!("  Space - Spawn 100 more particles");
    println!("  R     - Reset scene");
    println!("  Escape - Quit");
    println!();

    // Create sprite assets
    self.create_particle_sprites();
    self.create_imported_sprite();

    // Set up walls
    self.spawn_walls(ctx.world);

    // Spawn initial particles
    self.spawn_particles(ctx, INITIAL_PARTICLES);
  }

  fn update(&mut self, ctx: &mut GameContext, dt: f32) {
    // Update nav cooldown
    if self.nav_cooldown > 0.0 {
      self.nav_cooldown -= dt;
    }

    // Scene switching
    if self.nav_cooldown <= 0.0 {
      let scene_changed = if ctx.input.key_pressed(KeyCode::Digit1) {
        self.current_scene = DemoScene::CreatedSprites;
        true
      } else if ctx.input.key_pressed(KeyCode::Digit2) {
        self.current_scene = DemoScene::ImportedSprites;
        true
      } else if ctx.input.key_pressed(KeyCode::Digit3) {
        self.current_scene = DemoScene::HierarchicalSprites;
        true
      } else {
        false
      };

      if scene_changed {
        self.nav_cooldown = 0.3;
        self.reset_scene(ctx);
      }
    }

    // Spawn more particles
    if ctx.input.key_pressed(KeyCode::Space) {
      self.spawn_particles(ctx, SPAWN_BATCH);
    }

    // Reset scene
    if ctx.input.key_pressed(KeyCode::KeyR) {
      self.reset_scene(ctx);
    }

    // Quit on Escape
    if ctx.input.key_pressed(KeyCode::Escape) {
      std::process::exit(0);
    }
  }

  fn render_gpu(&mut self, gpu: &mut GpuRenderContext) {
    // Upload all particle sprite textures (cached, safe to call every frame)
    for sprite in &self.particle_sprites {
      gpu.upload_sprite(sprite);
    }
    if let Some(ref sprite) = self.imported_sprite {
      gpu.upload_sprite(sprite);
    }

    // Iterate through all particles and add to GPU batch
    for entity in gpu.world.entities_with::<Particle>() {
      // Get transform for position
      let Some(transform) = gpu.world.get_component::<Transform>(entity) else {
        continue;
      };

      // Get sprite index
      let Some(sprite_idx) = gpu.world.get_component::<ParticleSpriteIndex>(entity) else {
        continue;
      };

      // Determine which sprite to use
      let sprite = if sprite_idx.0 == usize::MAX {
        // Imported sprite
        self.imported_sprite.as_ref()
      } else {
        self.particle_sprites.get(sprite_idx.0)
      };

      let Some(sprite) = sprite else {
        continue;
      };

      // Create sprite instance with transform
      // The GPU quad is unit size (0-1), so we need to scale by sprite pixel dimensions
      // Also apply camera zoom and offset by half size to center on physics body
      let sprite_w = sprite.width() as f32;
      let sprite_h = sprite.height() as f32;
      let zoom = gpu.camera.zoom;
      let scaled_w = transform.scale_x * sprite_w * zoom;
      let scaled_h = transform.scale_y * sprite_h * zoom;

      // Convert world position to screen position using camera
      let (screen_x, screen_y) = gpu.camera.world_to_screen(transform.x, transform.y);

      let instance = SpriteInstance::new().with_transform(
        screen_x - scaled_w * 0.5,
        screen_y - scaled_h * 0.5,
        transform.rotation.to_radians(),
        scaled_w,
        scaled_h,
      );

      // Add to batch (z_order 10 for all particles)
      gpu.batch.add(sprite.id(), false, instance, 10);
    }
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    // Scene name
    ui.draw_text(
      &format!("Scene: {} (press 1/2/3)", self.current_scene.name()),
      10.0,
      10.0,
      Color::WHITE,
    );

    // Particle count
    ui.draw_text(
      &format!("Particles: {} / {}", self.particle_count, MAX_PARTICLES),
      10.0,
      30.0,
      Color::YELLOW,
    );

    // Gravity info
    ui.draw_text("Gravity: 400 px/s²", 10.0, 50.0, Color::CYAN);

    // FPS hint
    ui.draw_text(
      "Space: +100 particles | R: Reset",
      10.0,
      HEIGHT as f32 - 30.0,
      Color::new(150, 150, 150, 255),
    );
  }
}

// =============================================================================
// Sprite creation helpers
// =============================================================================

/// Create a simple circular particle sprite with the given color
fn create_particle_sprite(r: u8, g: u8, b: u8) -> Sprite {
  let size = (PARTICLE_RADIUS * 2.0) as u32 + 2;
  let mut pixels = vec![0u8; (size * size * 4) as usize];
  let center = size as f32 / 2.0;
  let radius = PARTICLE_RADIUS;

  for y in 0..size {
    for x in 0..size {
      let dx = x as f32 - center + 0.5;
      let dy = y as f32 - center + 0.5;
      let dist = (dx * dx + dy * dy).sqrt();

      let idx = ((y * size + x) * 4) as usize;

      if dist <= radius {
        // Inside the circle - gradient from center
        let t = dist / radius;
        let brightness = 1.0 - t * 0.3; // Slight gradient
        pixels[idx] = (r as f32 * brightness) as u8;
        pixels[idx + 1] = (g as f32 * brightness) as u8;
        pixels[idx + 2] = (b as f32 * brightness) as u8;
        pixels[idx + 3] = 255;
      } else if dist <= radius + 1.0 {
        // Anti-aliased edge
        let alpha = (1.0 - (dist - radius)) * 255.0;
        pixels[idx] = r;
        pixels[idx + 1] = g;
        pixels[idx + 2] = b;
        pixels[idx + 3] = alpha as u8;
      }
    }
  }

  Sprite::from_rgba(pixels, size, size)
}

/// Create a glow particle sprite (for "imported" style)
fn create_glow_particle() -> Sprite {
  let size = (PARTICLE_RADIUS * 4.0) as u32;
  let mut pixels = vec![0u8; (size * size * 4) as usize];
  let center = size as f32 / 2.0;

  for y in 0..size {
    for x in 0..size {
      let dx = x as f32 - center + 0.5;
      let dy = y as f32 - center + 0.5;
      let dist = (dx * dx + dy * dy).sqrt();

      let idx = ((y * size + x) * 4) as usize;

      // Soft glow falloff
      let max_dist = center;
      if dist < max_dist {
        let t = dist / max_dist;
        let intensity = (1.0 - t * t).max(0.0); // Quadratic falloff

        // Rainbow gradient based on angle
        let angle = dy.atan2(dx) + std::f32::consts::PI;
        let hue = angle / std::f32::consts::TAU;
        let (r, g, b) = hsv_to_rgb(hue, 0.7, intensity);

        pixels[idx] = r;
        pixels[idx + 1] = g;
        pixels[idx + 2] = b;
        pixels[idx + 3] = (intensity * 255.0) as u8;
      }
    }
  }

  Sprite::from_rgba(pixels, size, size)
}

/// Convert HSV to RGB
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
  let c = v * s;
  let x = c * (1.0 - ((h * 6.0) % 2.0 - 1.0).abs());
  let m = v - c;

  let (r, g, b) = match (h * 6.0) as u32 {
    0 => (c, x, 0.0),
    1 => (x, c, 0.0),
    2 => (0.0, c, x),
    3 => (0.0, x, c),
    4 => (x, 0.0, c),
    _ => (c, 0.0, x),
  };

  (
    ((r + m) * 255.0) as u8,
    ((g + m) * 255.0) as u8,
    ((b + m) * 255.0) as u8,
  )
}

/// Simple pseudo-random number in range
fn rand_range(min: f32, max: f32) -> f32 {
  static mut SEED: u64 = 12345;
  unsafe {
    SEED = SEED.wrapping_mul(1103515245).wrapping_add(12345);
    let t = ((SEED >> 16) & 0x7fff) as f32 / 0x7fff as f32;
    min + t * (max - min)
  }
}

fn main() {
  EngineBuilder::new()
    .title("Particles Demo - 1000+ Physics Sprites")
    .resolution(WIDTH, HEIGHT)
    .fps(144)
    .with_game(ParticlesDemo::new())
    .build()
    .run()
    .expect("Failed to run particles demo");
}
