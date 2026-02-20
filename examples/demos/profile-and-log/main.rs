//! Profile and Log Demo - Showcases the Ryo2D profiler and logger windows
//!
//! Run with: cargo run -p profile-and-log --features profiler
//!
//! Controls:
//! - 1/2: Add/Remove CPU-heavy entities (physics simulation)
//! - 3/4: Add/Remove GPU-heavy entities (many sprites)
//! - 5/6: Add/Remove mixed entities (both CPU and GPU load)
//! - 7/8: Increase/Decrease work multiplier (CPU work per entity)
//! - Space: Toggle auto-spawn mode
//! - R: Reset all entities
//! - F1: Toggle debug overlay
//! - F9: Toggle logger window
//! - F10: Toggle profiler window

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use ryo2d::prelude::*;

const SCREEN_WIDTH: u32 = 1280;
const SCREEN_HEIGHT: u32 = 720;

// Spawn area (centered region where entities appear)
const SPAWN_WIDTH: f32 = 600.0;
const SPAWN_HEIGHT: f32 = 400.0;
const SPAWN_LEFT: f32 = (SCREEN_WIDTH as f32 - SPAWN_WIDTH) / 2.0;
const SPAWN_TOP: f32 = (SCREEN_HEIGHT as f32 - SPAWN_HEIGHT) / 2.0;
const SPAWN_RIGHT: f32 = SPAWN_LEFT + SPAWN_WIDTH;
const SPAWN_BOTTOM: f32 = SPAWN_TOP + SPAWN_HEIGHT;

// Batch sizes for spawning
const CPU_BATCH: usize = 50;
const GPU_BATCH: usize = 200;
const MIXED_BATCH: usize = 25;

fn main() {
  game_info!(module: "Main", "Starting Profile and Log Demo");

  let engine = EngineBuilder::new()
    .title("Profile and Log Demo")
    .resolution(SCREEN_WIDTH, SCREEN_HEIGHT)
    .target_fps(144)
    .with_scene(DemoScene::new())
    .build();

  if let Err(e) = engine.run() {
    game_error!(module: "Main", "Engine error: {}", e);
  }
}

/// Marker for CPU-heavy entities (complex physics)
struct CpuHeavy {
  angle: f32,
  angular_velocity: f32,
  orbit_radius: f32,
  orbit_center: (f32, f32),
  bounce_phase: f32,
}

/// Marker for GPU-heavy entities (just sprites, minimal logic)
struct GpuHeavy {
  drift_x: f32,
  drift_y: f32,
}

/// Marker for mixed entities (both CPU and GPU load)
struct MixedLoad {
  target_x: f32,
  target_y: f32,
  speed: f32,
  think_timer: f32,
}

struct DemoScene {
  cpu_sprite: Sprite,
  gpu_sprite: Sprite,
  mixed_sprite: Sprite,
  cpu_count: usize,
  gpu_count: usize,
  mixed_count: usize,
  auto_spawn: bool,
  auto_spawn_timer: f32,
  /// Work multiplier - increases CPU work per entity (7/8 to adjust)
  work_multiplier: u32,
}

impl DemoScene {
  fn new() -> Self {
    Self {
      // Create simple colored sprites for each type
      cpu_sprite: Sprite::solid_rect(8, 8, 100, 150, 255, 255), // Blue
      gpu_sprite: Sprite::solid_rect(4, 4, 150, 255, 100, 255), // Green
      mixed_sprite: Sprite::solid_rect(6, 6, 255, 180, 100, 255), // Orange
      cpu_count: 0,
      gpu_count: 0,
      mixed_count: 0,
      auto_spawn: false,
      auto_spawn_timer: 0.0,
      work_multiplier: 1,
    }
  }

  fn spawn_cpu(&mut self, ctx: &mut SceneContext, count: usize) {
    #[cfg(feature = "profiler")]
    profile_scope!("spawn_cpu");

    let mut rng = rand::thread_rng();
    let sprite = SpriteHandle::from_sprite(self.cpu_sprite.clone(), "cpu_sprite");

    for _ in 0..count {
      let center_x = rng.gen_range(SPAWN_LEFT + 50.0..SPAWN_RIGHT - 50.0);
      let center_y = rng.gen_range(SPAWN_TOP + 50.0..SPAWN_BOTTOM - 50.0);

      ctx.world.spawn((
        Transform::new(center_x, center_y),
        Renderable::sprite(sprite.clone()).with_z_order(10),
        CpuHeavy {
          angle: rng.gen_range(0.0..std::f32::consts::TAU),
          angular_velocity: rng.gen_range(1.0..5.0),
          orbit_radius: rng.gen_range(20.0..60.0),
          orbit_center: (center_x, center_y),
          bounce_phase: rng.gen_range(0.0..std::f32::consts::TAU),
        },
      ));
    }

    game_info!(module: "Spawner", "Spawned {} CPU-heavy entities", count);
  }

  fn despawn_cpu(&mut self, ctx: &mut SceneContext, count: usize) {
    #[cfg(feature = "profiler")]
    profile_scope!("despawn_cpu");

    let entities: Vec<_> = ctx.world.entities_with::<CpuHeavy>();
    let actual = entities.len().min(count);
    for entity in entities.into_iter().take(count) {
      let _ = ctx.world.despawn(entity);
    }

    if actual > 0 {
      game_info!(module: "Spawner", "Despawned {} CPU-heavy entities", actual);
    }
  }

  fn spawn_gpu(&mut self, ctx: &mut SceneContext, count: usize) {
    #[cfg(feature = "profiler")]
    profile_scope!("spawn_gpu");

    let mut rng = rand::thread_rng();
    let sprite = SpriteHandle::from_sprite(self.gpu_sprite.clone(), "gpu_sprite");

    for _ in 0..count {
      ctx.world.spawn((
        Transform::new(
          rng.gen_range(SPAWN_LEFT..SPAWN_RIGHT),
          rng.gen_range(SPAWN_TOP..SPAWN_BOTTOM),
        ),
        Renderable::sprite(sprite.clone()).with_z_order(5),
        GpuHeavy {
          drift_x: rng.gen_range(-20.0..20.0),
          drift_y: rng.gen_range(-20.0..20.0),
        },
      ));
    }

    game_info!(module: "Spawner", "Spawned {} GPU-heavy entities", count);
  }

  fn despawn_gpu(&mut self, ctx: &mut SceneContext, count: usize) {
    #[cfg(feature = "profiler")]
    profile_scope!("despawn_gpu");

    let entities: Vec<_> = ctx.world.entities_with::<GpuHeavy>();
    let actual = entities.len().min(count);
    for entity in entities.into_iter().take(count) {
      let _ = ctx.world.despawn(entity);
    }

    if actual > 0 {
      game_info!(module: "Spawner", "Despawned {} GPU-heavy entities", actual);
    }
  }

  fn spawn_mixed(&mut self, ctx: &mut SceneContext, count: usize) {
    #[cfg(feature = "profiler")]
    profile_scope!("spawn_mixed");

    let mut rng = rand::thread_rng();
    let sprite = SpriteHandle::from_sprite(self.mixed_sprite.clone(), "mixed_sprite");

    for _ in 0..count {
      ctx.world.spawn((
        Transform::new(
          rng.gen_range(SPAWN_LEFT..SPAWN_RIGHT),
          rng.gen_range(SPAWN_TOP..SPAWN_BOTTOM),
        ),
        Renderable::sprite(sprite.clone()).with_z_order(15),
        MixedLoad {
          target_x: rng.gen_range(SPAWN_LEFT..SPAWN_RIGHT),
          target_y: rng.gen_range(SPAWN_TOP..SPAWN_BOTTOM),
          speed: rng.gen_range(50.0..150.0),
          think_timer: rng.gen_range(0.0..1.0),
        },
      ));
    }

    game_debug!(module: "Spawner", "Spawned {} mixed entities", count);
  }

  fn despawn_mixed(&mut self, ctx: &mut SceneContext, count: usize) {
    #[cfg(feature = "profiler")]
    profile_scope!("despawn_mixed");

    let entities: Vec<_> = ctx.world.entities_with::<MixedLoad>();
    let actual = entities.len().min(count);
    for entity in entities.into_iter().take(count) {
      let _ = ctx.world.despawn(entity);
    }

    if actual > 0 {
      game_debug!(module: "Spawner", "Despawned {} mixed entities", actual);
    }
  }

  fn reset_all(&mut self, ctx: &mut SceneContext) {
    #[cfg(feature = "profiler")]
    profile_scope!("reset");

    let cpu_entities: Vec<_> = ctx.world.entities_with::<CpuHeavy>();
    let gpu_entities: Vec<_> = ctx.world.entities_with::<GpuHeavy>();
    let mixed_entities: Vec<_> = ctx.world.entities_with::<MixedLoad>();

    let total = cpu_entities.len() + gpu_entities.len() + mixed_entities.len();

    for entity in cpu_entities {
      let _ = ctx.world.despawn(entity);
    }
    for entity in gpu_entities {
      let _ = ctx.world.despawn(entity);
    }
    for entity in mixed_entities {
      let _ = ctx.world.despawn(entity);
    }

    self.auto_spawn = false;

    game_warn!(module: "Demo", "Reset all entities (removed {})", total);
  }

  fn update_cpu_entities(&mut self, ctx: &mut SceneContext, dt: f32) {
    #[cfg(feature = "profiler")]
    profile_scope!("physics_sim");

    // Expensive physics simulation - lots of trig and calculations
    let entities: Vec<_> = ctx.world.entities_with::<CpuHeavy>();

    for entity in entities {
      // Get component data
      let (new_x, new_y, new_angle, new_bounce) = {
        let cpu = ctx.world.get_component::<CpuHeavy>(entity);
        let transform = ctx.world.get_component::<Transform>(entity);

        if let (Some(cpu), Some(_t)) = (cpu, transform) {
          // Complex orbital motion with bounce
          let new_angle = cpu.angle + cpu.angular_velocity * dt;
          let new_bounce = cpu.bounce_phase + dt * 3.0;
          let bounce = new_bounce.sin() * 10.0;

          let radius = cpu.orbit_radius + bounce;
          let new_x = cpu.orbit_center.0 + radius * new_angle.cos();
          let new_y = cpu.orbit_center.1 + radius * new_angle.sin();

          // Extra CPU work: scaled by work_multiplier
          // Use black_box to prevent compiler from optimizing away
          for _ in 0..self.work_multiplier {
            std::hint::black_box((0..100).fold(0.0f32, |acc, i| {
              acc + (new_angle * i as f32).sin() * (new_x * 0.01).cos()
            }));
          }

          (new_x, new_y, new_angle, new_bounce)
        } else {
          continue;
        }
      };

      // Update components
      if let Some(mut transform) = ctx.world.get_component_mut::<Transform>(entity) {
        transform.x = new_x;
        transform.y = new_y;
      }
      if let Some(mut cpu) = ctx.world.get_component_mut::<CpuHeavy>(entity) {
        cpu.angle = new_angle;
        cpu.bounce_phase = new_bounce;
      }
    }
  }

  fn update_gpu_entities(&mut self, ctx: &mut SceneContext, dt: f32) {
    #[cfg(feature = "profiler")]
    profile_scope!("gpu_drift");

    // Minimal CPU work - just drift
    let entities: Vec<_> = ctx.world.entities_with::<GpuHeavy>();

    for entity in entities {
      let drift = {
        let gpu = ctx.world.get_component::<GpuHeavy>(entity);
        gpu.map(|g| (g.drift_x, g.drift_y))
      };

      if let Some((dx, dy)) = drift
        && let Some(mut transform) = ctx.world.get_component_mut::<Transform>(entity)
      {
        transform.x += dx * dt;
        transform.y += dy * dt;

        // Wrap around spawn area
        if transform.x < SPAWN_LEFT {
          transform.x = SPAWN_RIGHT;
        }
        if transform.x > SPAWN_RIGHT {
          transform.x = SPAWN_LEFT;
        }
        if transform.y < SPAWN_TOP {
          transform.y = SPAWN_BOTTOM;
        }
        if transform.y > SPAWN_BOTTOM {
          transform.y = SPAWN_TOP;
        }
      }
    }
  }

  fn update_mixed_entities(&mut self, ctx: &mut SceneContext, dt: f32) {
    #[cfg(feature = "profiler")]
    profile_scope!("ai_pathfinding");

    // Simulated AI/pathfinding - moderate CPU work
    let entities: Vec<_> = ctx.world.entities_with::<MixedLoad>();
    let mut rng = rand::thread_rng();

    for entity in entities {
      let update = {
        let mixed = ctx.world.get_component::<MixedLoad>(entity);
        let transform = ctx.world.get_component::<Transform>(entity);

        if let (Some(mixed), Some(t)) = (mixed, transform) {
          let dx = mixed.target_x - t.x;
          let dy = mixed.target_y - t.y;
          let dist = (dx * dx + dy * dy).sqrt();

          let need_new_target = dist < 5.0 || mixed.think_timer <= 0.0;
          let (new_target_x, new_target_y) = if need_new_target {
            (
              rng.gen_range(SPAWN_LEFT + 20.0..SPAWN_RIGHT - 20.0),
              rng.gen_range(SPAWN_TOP + 20.0..SPAWN_BOTTOM - 20.0),
            )
          } else {
            (mixed.target_x, mixed.target_y)
          };

          // Simulated pathfinding cost - scaled by work_multiplier
          // Use black_box to prevent compiler from optimizing away
          for _ in 0..self.work_multiplier {
            std::hint::black_box((0..50).fold(0.0f32, |acc, i| {
              acc
                + ((t.x + i as f32 * 10.0) * 0.1).sin().abs()
                + ((t.y + i as f32 * 10.0) * 0.1).cos().abs()
            }));
          }

          let move_x = if dist > 0.0 {
            dx / dist * mixed.speed * dt
          } else {
            0.0
          };
          let move_y = if dist > 0.0 {
            dy / dist * mixed.speed * dt
          } else {
            0.0
          };

          Some((move_x, move_y, new_target_x, new_target_y, need_new_target))
        } else {
          None
        }
      };

      if let Some((move_x, move_y, new_target_x, new_target_y, need_new_target)) = update {
        if let Some(mut transform) = ctx.world.get_component_mut::<Transform>(entity) {
          transform.x += move_x;
          transform.y += move_y;
        }
        if let Some(mut mixed) = ctx.world.get_component_mut::<MixedLoad>(entity) {
          mixed.think_timer -= dt;
          if need_new_target {
            mixed.target_x = new_target_x;
            mixed.target_y = new_target_y;
            mixed.think_timer = rng.gen_range(0.5..2.0);
          }
        }
      }
    }
  }

  fn update_counts(&mut self, ctx: &mut SceneContext) {
    self.cpu_count = ctx.world.entities_with::<CpuHeavy>().len();
    self.gpu_count = ctx.world.entities_with::<GpuHeavy>().len();
    self.mixed_count = ctx.world.entities_with::<MixedLoad>().len();
  }
}

impl Scene for DemoScene {
  fn on_enter(&mut self, _ctx: &mut SceneContext) {
    game_info!(module: "Demo", "Scene entered - press 1-6 to spawn/despawn entities");
    game_debug!(module: "Demo", "F9 opens logger, F10 opens profiler");
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    #[cfg(feature = "profiler")]
    profile_scope!("demo_update");

    // Handle input
    // CPU entities
    if ctx.input.key_pressed(KeyCode::Digit1) {
      self.spawn_cpu(ctx, CPU_BATCH);
    }
    if ctx.input.key_pressed(KeyCode::Digit2) {
      self.despawn_cpu(ctx, CPU_BATCH);
    }

    // GPU entities
    if ctx.input.key_pressed(KeyCode::Digit3) {
      self.spawn_gpu(ctx, GPU_BATCH);
    }
    if ctx.input.key_pressed(KeyCode::Digit4) {
      self.despawn_gpu(ctx, GPU_BATCH);
    }

    // Mixed entities
    if ctx.input.key_pressed(KeyCode::Digit5) {
      self.spawn_mixed(ctx, MIXED_BATCH);
    }
    if ctx.input.key_pressed(KeyCode::Digit6) {
      self.despawn_mixed(ctx, MIXED_BATCH);
    }

    // Auto-spawn toggle
    if ctx.input.key_pressed(KeyCode::Space) {
      self.auto_spawn = !self.auto_spawn;
      if self.auto_spawn {
        game_info!(module: "Demo", "Auto-spawn enabled");
      } else {
        game_info!(module: "Demo", "Auto-spawn disabled");
      }
    }

    // Reset
    if ctx.input.key_pressed(KeyCode::KeyR) {
      self.reset_all(ctx);
    }

    // Work multiplier (increases CPU work per entity)
    if ctx.input.key_pressed(KeyCode::Digit7) {
      let old = self.work_multiplier;
      self.work_multiplier = (self.work_multiplier * 2).min(1024);
      game_info!(module: "Demo", "Work multiplier: {}x -> {}x", old, self.work_multiplier);
    }
    if ctx.input.key_pressed(KeyCode::Digit8) {
      let old = self.work_multiplier;
      self.work_multiplier = (self.work_multiplier / 2).max(1);
      game_info!(module: "Demo", "Work multiplier: {}x -> {}x", old, self.work_multiplier);
    }

    // Auto-spawn mode
    if self.auto_spawn {
      self.auto_spawn_timer += dt;
      if self.auto_spawn_timer > 0.1 {
        self.auto_spawn_timer = 0.0;
        self.spawn_mixed(ctx, 5);
      }
    }

    // Update entities
    self.update_cpu_entities(ctx, dt);
    self.update_gpu_entities(ctx, dt);
    self.update_mixed_entities(ctx, dt);

    // Update counts
    self.update_counts(ctx);

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    #[cfg(feature = "profiler")]
    profile_scope!("demo_ui");

    let line_height = 18.0;
    let mut y = 10.0;

    // Title
    ui.draw_text("PROFILE AND LOG DEMO", 10.0, y, Color::WHITE);
    y += line_height + 5.0;

    // Counts with inline controls
    ui.draw_text(
      &format!(
        "[1] +{}  [2] -{}  CPU: {}",
        CPU_BATCH, CPU_BATCH, self.cpu_count
      ),
      10.0,
      y,
      Color::rgb(100, 150, 255),
    );
    y += line_height;

    ui.draw_text(
      &format!(
        "[3] +{}  [4] -{}  GPU: {}",
        GPU_BATCH, GPU_BATCH, self.gpu_count
      ),
      10.0,
      y,
      Color::rgb(150, 255, 100),
    );
    y += line_height;

    ui.draw_text(
      &format!(
        "[5] +{}  [6] -{}  Mixed: {}",
        MIXED_BATCH, MIXED_BATCH, self.mixed_count
      ),
      10.0,
      y,
      Color::rgb(255, 180, 100),
    );
    y += line_height + 5.0;

    let total = self.cpu_count + self.gpu_count + self.mixed_count;
    ui.draw_text(&format!("Total entities: {}", total), 10.0, y, Color::WHITE);
    y += line_height + 10.0;

    // Work multiplier
    ui.draw_text(
      &format!("[7] x2  [8] /2  Work: {}x", self.work_multiplier),
      10.0,
      y,
      Color::rgb(255, 255, 150),
    );
    y += line_height + 10.0;

    // Other controls
    ui.draw_text("Controls:", 10.0, y, Color::GRAY);
    y += line_height;

    let auto_status = if self.auto_spawn { "ON" } else { "OFF" };
    ui.draw_text(
      &format!("Space: Auto-spawn [{}]", auto_status),
      10.0,
      y,
      Color::GRAY,
    );
    y += line_height;

    ui.draw_text("R: Reset all", 10.0, y, Color::GRAY);
    y += line_height;

    ui.draw_text("F1: Debug overlay", 10.0, y, Color::GRAY);
    y += line_height;

    ui.draw_text("F9: Logger window", 10.0, y, Color::GRAY);
    y += line_height;

    ui.draw_text("F10: Profiler window", 10.0, y, Color::GRAY);
  }
}
