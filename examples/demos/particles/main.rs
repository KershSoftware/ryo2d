//! Particle System Demo - Ryo2D
//!
//! Comprehensive showcase of all particle presets and features.
//! Navigate between scenes with number keys or arrow keys.

use ryo2d::prelude::*;

// =============================================================================
// Scene Types
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DemoScene {
  Fire,
  Smoke,
  Explosions,
  Magic,
  Weather,
  Impacts,
  Celebration,
  Trails,
  Attractors,
  Combined,
}

impl DemoScene {
  const ALL: [DemoScene; 10] = [
    DemoScene::Fire,
    DemoScene::Smoke,
    DemoScene::Explosions,
    DemoScene::Magic,
    DemoScene::Weather,
    DemoScene::Impacts,
    DemoScene::Celebration,
    DemoScene::Trails,
    DemoScene::Attractors,
    DemoScene::Combined,
  ];

  fn index(&self) -> usize {
    Self::ALL.iter().position(|s| s == self).unwrap_or(0)
  }

  fn from_index(idx: usize) -> Self {
    Self::ALL.get(idx).copied().unwrap_or(DemoScene::Fire)
  }

  fn name(&self) -> &'static str {
    match self {
      DemoScene::Fire => "Fire Effects",
      DemoScene::Smoke => "Smoke Effects",
      DemoScene::Explosions => "Explosions",
      DemoScene::Magic => "Magic Effects",
      DemoScene::Weather => "Weather",
      DemoScene::Impacts => "Impacts",
      DemoScene::Celebration => "Celebration",
      DemoScene::Trails => "Trail Effects",
      DemoScene::Attractors => "Attractors",
      DemoScene::Combined => "Combined Demo",
    }
  }

  fn description(&self) -> &'static str {
    match self {
      DemoScene::Fire => "fire() and flame_small() presets",
      DemoScene::Smoke => "smoke() and smoke_thick() presets",
      DemoScene::Explosions => "explosion() and explosion_debris() - Click to spawn",
      DemoScene::Magic => "magic() and magic_heal() presets",
      DemoScene::Weather => "rain() and snow() presets",
      DemoScene::Impacts => "sparks() and dust() - Click to spawn",
      DemoScene::Celebration => "confetti() and bubbles() presets",
      DemoScene::Trails => "comet_trail() and exhaust() - Move with mouse",
      DemoScene::Attractors => "Point, Vortex, and Turbulence attractors",
      DemoScene::Combined => "Multiple effects with attractors",
    }
  }

  fn next(&self) -> Self {
    let idx = (self.index() + 1) % Self::ALL.len();
    Self::from_index(idx)
  }

  fn prev(&self) -> Self {
    let idx = if self.index() == 0 {
      Self::ALL.len() - 1
    } else {
      self.index() - 1
    };
    Self::from_index(idx)
  }
}

// =============================================================================
// Main Demo
// =============================================================================

struct ParticlesDemo {
  current_scene: DemoScene,
  particle_sprite: SpriteHandle,
  screen_width: f32,
  screen_height: f32,
  nav_cooldown: f32,

  // Particle system
  particle_system: ParticleSystem,

  // Scene-specific entities to track
  emitters: Vec<Entity>,
  attractors: Vec<Entity>,

  // Trail scene state
  trail_target_x: f32,
  trail_target_y: f32,
}

impl ParticlesDemo {
  fn new() -> Self {
    // Create a temporary placeholder sprite - will be replaced in init
    let placeholder = Sprite::solid_circle(8, 255, 255, 255, 255);
    Self {
      current_scene: DemoScene::Fire,
      particle_sprite: SpriteHandle::from_sprite(placeholder, "placeholder"),
      screen_width: 800.0,
      screen_height: 600.0,
      nav_cooldown: 0.0,
      particle_system: ParticleSystem::new(),
      emitters: Vec::new(),
      attractors: Vec::new(),
      trail_target_x: 400.0,
      trail_target_y: 300.0,
    }
  }

  fn clear_scene(&mut self, world: &mut World) {
    // Despawn all emitters
    for entity in self.emitters.drain(..) {
      let _ = world.despawn(entity);
    }

    // Despawn all attractors
    for entity in self.attractors.drain(..) {
      let _ = world.despawn(entity);
    }

    // Clear all particles from the particle system
    self.particle_system.clear();
  }

  fn setup_scene(&mut self, world: &mut World) {
    match self.current_scene {
      DemoScene::Fire => self.setup_fire(world),
      DemoScene::Smoke => self.setup_smoke(world),
      DemoScene::Explosions => self.setup_explosions(world),
      DemoScene::Magic => self.setup_magic(world),
      DemoScene::Weather => self.setup_weather(world),
      DemoScene::Impacts => self.setup_impacts(world),
      DemoScene::Celebration => self.setup_celebration(world),
      DemoScene::Trails => self.setup_trails(world),
      DemoScene::Attractors => self.setup_attractors(world),
      DemoScene::Combined => self.setup_combined(world),
    }
  }

  // ---------------------------------------------------------------------------
  // Fire Scene
  // ---------------------------------------------------------------------------

  fn setup_fire(&mut self, world: &mut World) {
    // Large bonfire in center
    let fire = world.spawn((
      Transform::new(self.screen_width / 2.0, self.screen_height - 100.0),
      ParticleEmitter::fire(self.particle_sprite.clone()),
    ));
    self.emitters.push(fire);

    // Small flames on sides (torches)
    for x in [150.0, self.screen_width - 150.0] {
      let flame = world.spawn((
        Transform::new(x, self.screen_height - 80.0),
        ParticleEmitter::flame_small(self.particle_sprite.clone()),
      ));
      self.emitters.push(flame);
    }

    // Row of candles
    for i in 0..5 {
      let x = 200.0 + i as f32 * 100.0;
      let candle = world.spawn((
        Transform::new(x, self.screen_height - 200.0),
        ParticleEmitter::flame_small(self.particle_sprite.clone())
          .with_mode(EmissionMode::Continuous { rate: 8.0 }),
      ));
      self.emitters.push(candle);
    }
  }

  // ---------------------------------------------------------------------------
  // Smoke Scene
  // ---------------------------------------------------------------------------

  fn setup_smoke(&mut self, world: &mut World) {
    // Light smoke sources
    for i in 0..3 {
      let x = 150.0 + i as f32 * 250.0;
      let smoke = world.spawn((
        Transform::new(x, self.screen_height - 50.0),
        ParticleEmitter::smoke(self.particle_sprite.clone()),
      ));
      self.emitters.push(smoke);
    }

    // Thick smoke (like from a fire)
    let thick = world.spawn((
      Transform::new(self.screen_width / 2.0, self.screen_height - 150.0),
      ParticleEmitter::smoke_thick(self.particle_sprite.clone()),
    ));
    self.emitters.push(thick);
  }

  // ---------------------------------------------------------------------------
  // Explosions Scene (click to spawn)
  // ---------------------------------------------------------------------------

  fn setup_explosions(&mut self, _world: &mut World) {
    // Explosions are spawned on click
  }

  fn spawn_explosion(&mut self, world: &mut World, x: f32, y: f32, debris: bool) {
    if debris {
      let explosion = world.spawn((
        Transform::new(x, y),
        ParticleEmitter::explosion_debris(self.particle_sprite.clone(), 25),
      ));
      self.emitters.push(explosion);
    } else {
      let explosion = world.spawn((
        Transform::new(x, y),
        ParticleEmitter::explosion(self.particle_sprite.clone(), 40),
      ));
      self.emitters.push(explosion);
    }
  }

  // ---------------------------------------------------------------------------
  // Magic Scene
  // ---------------------------------------------------------------------------

  fn setup_magic(&mut self, world: &mut World) {
    // Magic sparkle effect
    let magic = world.spawn((
      Transform::new(self.screen_width / 2.0 - 150.0, self.screen_height / 2.0),
      ParticleEmitter::magic(self.particle_sprite.clone()),
    ));
    self.emitters.push(magic);

    // Healing effect
    let heal = world.spawn((
      Transform::new(self.screen_width / 2.0 + 150.0, self.screen_height / 2.0),
      ParticleEmitter::magic_heal(self.particle_sprite.clone()),
    ));
    self.emitters.push(heal);
  }

  // ---------------------------------------------------------------------------
  // Weather Scene
  // ---------------------------------------------------------------------------

  fn setup_weather(&mut self, world: &mut World) {
    // Rain on left half
    let rain = world.spawn((
      Transform::new(self.screen_width / 4.0, 0.0),
      ParticleEmitter::rain(self.particle_sprite.clone())
        .with_mode(EmissionMode::Continuous { rate: 80.0 }),
    ));
    self.emitters.push(rain);

    // Snow on right half
    let snow = world.spawn((
      Transform::new(self.screen_width * 3.0 / 4.0, 0.0),
      ParticleEmitter::snow(self.particle_sprite.clone())
        .with_mode(EmissionMode::Continuous { rate: 25.0 }),
    ));
    self.emitters.push(snow);
  }

  // ---------------------------------------------------------------------------
  // Impacts Scene (click to spawn)
  // ---------------------------------------------------------------------------

  fn setup_impacts(&mut self, _world: &mut World) {
    // Impacts spawned on click
  }

  fn spawn_impact(&mut self, world: &mut World, x: f32, y: f32, is_sparks: bool) {
    if is_sparks {
      let sparks = world.spawn((
        Transform::new(x, y),
        ParticleEmitter::sparks(self.particle_sprite.clone()),
      ));
      self.emitters.push(sparks);
    } else {
      let dust = world.spawn((
        Transform::new(x, y),
        ParticleEmitter::dust(self.particle_sprite.clone()),
      ));
      self.emitters.push(dust);
    }
  }

  // ---------------------------------------------------------------------------
  // Celebration Scene
  // ---------------------------------------------------------------------------

  fn setup_celebration(&mut self, world: &mut World) {
    // Confetti burst (repeating)
    let confetti = world.spawn((
      Transform::new(self.screen_width / 2.0, self.screen_height - 50.0),
      ParticleEmitter::confetti(self.particle_sprite.clone()).with_mode(
        EmissionMode::BurstRepeating {
          count: 30,
          interval: 2.0,
          repeats: None,
        },
      ),
    ));
    self.emitters.push(confetti);

    // Bubbles
    for x in [200.0, 400.0, 600.0] {
      let bubbles = world.spawn((
        Transform::new(x, self.screen_height - 50.0),
        ParticleEmitter::bubbles(self.particle_sprite.clone()),
      ));
      self.emitters.push(bubbles);
    }
  }

  // ---------------------------------------------------------------------------
  // Trails Scene
  // ---------------------------------------------------------------------------

  fn setup_trails(&mut self, world: &mut World) {
    self.trail_target_x = self.screen_width / 2.0;
    self.trail_target_y = self.screen_height / 2.0;

    // Comet trail (follows mouse)
    let comet = world.spawn((
      Transform::new(self.trail_target_x, self.trail_target_y),
      ParticleEmitter::comet_trail(self.particle_sprite.clone()),
    ));
    self.emitters.push(comet);

    // Exhaust trails at bottom
    for (i, x) in [250.0, self.screen_width - 250.0].iter().enumerate() {
      let exhaust = world.spawn((
        Transform::new(*x, self.screen_height - 100.0),
        ParticleEmitter::exhaust(self.particle_sprite.clone()).with_shape(EmissionShape::Cone {
          direction: if i == 0 {
            -std::f32::consts::FRAC_PI_4
          } else {
            -std::f32::consts::FRAC_PI_4 * 3.0
          },
          spread: 0.3,
          min_speed: 50.0,
          max_speed: 100.0,
        }),
      ));
      self.emitters.push(exhaust);
    }
  }

  fn update_trails(&mut self, world: &mut World, target_x: f32, target_y: f32) {
    self.trail_target_x = target_x;
    self.trail_target_y = target_y;

    // Update first emitter (comet) position
    if let Some(&entity) = self.emitters.first()
      && let Some(mut transform) = world.get_mut::<Transform>(entity) {
        // Smooth follow
        transform.x += (target_x - transform.x) * 0.1;
        transform.y += (target_y - transform.y) * 0.1;
      }
  }

  // ---------------------------------------------------------------------------
  // Attractors Scene
  // ---------------------------------------------------------------------------

  fn setup_attractors(&mut self, world: &mut World) {
    let cx = self.screen_width / 2.0;
    let cy = self.screen_height / 2.0;

    // Emitter near point attractor (top left)
    let emitter1 = world.spawn((
      Transform::new(150.0, 250.0),
      ParticleEmitter::new(self.particle_sprite.clone())
        .with_mode(EmissionMode::Continuous { rate: 25.0 })
        .with_shape(EmissionShape::Circle {
          radius: 20.0,
          hollow: false,
        })
        .with_lifetime(4.0, 6.0)
        .with_size(4.0, 8.0)
        .with_color(Color::GREEN, Color::new(0, 255, 100, 0))
        .with_speed(5.0, 15.0),
    ));
    self.emitters.push(emitter1);

    // Emitter near vortex attractor (top right)
    let emitter2 = world.spawn((
      Transform::new(self.screen_width - 150.0, 250.0),
      ParticleEmitter::new(self.particle_sprite.clone())
        .with_mode(EmissionMode::Continuous { rate: 25.0 })
        .with_shape(EmissionShape::Circle {
          radius: 20.0,
          hollow: false,
        })
        .with_lifetime(4.0, 6.0)
        .with_size(4.0, 8.0)
        .with_color(Color::CYAN, Color::new(0, 200, 255, 0))
        .with_speed(5.0, 15.0),
    ));
    self.emitters.push(emitter2);

    // Emitter near turbulence (bottom center)
    let emitter3 = world.spawn((
      Transform::new(cx, self.screen_height - 200.0),
      ParticleEmitter::new(self.particle_sprite.clone())
        .with_mode(EmissionMode::Continuous { rate: 30.0 })
        .with_shape(EmissionShape::Line {
          start_x: -80.0,
          start_y: 0.0,
          end_x: 80.0,
          end_y: 0.0,
        })
        .with_lifetime(3.0, 5.0)
        .with_size(4.0, 6.0)
        .with_color(Color::MAGENTA, Color::new(255, 0, 255, 0))
        .with_speed(5.0, 20.0),
    ));
    self.emitters.push(emitter3);

    // Emitter at center (affected by repel)
    let emitter4 = world.spawn((
      Transform::new(cx, cy),
      ParticleEmitter::new(self.particle_sprite.clone())
        .with_mode(EmissionMode::Continuous { rate: 40.0 })
        .with_shape(EmissionShape::Point)
        .with_lifetime(3.0, 5.0)
        .with_size(5.0, 10.0)
        .with_color(Color::RED, Color::new(255, 50, 0, 0))
        .with_speed(10.0, 30.0),
    ));
    self.emitters.push(emitter4);

    // Point attractor (gravity well) - top left
    let point = world.spawn((
      Transform::new(150.0, 150.0),
      ParticleAttractor::point(200.0, 180.0),
    ));
    self.attractors.push(point);

    // Vortex attractor - top right
    let vortex = world.spawn((
      Transform::new(self.screen_width - 150.0, 150.0),
      ParticleAttractor::vortex(120.0, 180.0),
    ));
    self.attractors.push(vortex);

    // Turbulence - bottom center
    let turbulence = world.spawn((
      Transform::new(cx, self.screen_height - 100.0),
      ParticleAttractor::turbulence(80.0, 40.0, 1.5),
    ));
    self.attractors.push(turbulence);

    // Repelling attractor - center (pushes red particles outward)
    let repel = world.spawn((
      Transform::new(cx, cy - 50.0),
      ParticleAttractor::point(-120.0, 150.0), // Negative strength = repel
    ));
    self.attractors.push(repel);
  }

  // ---------------------------------------------------------------------------
  // Combined Scene
  // ---------------------------------------------------------------------------

  fn setup_combined(&mut self, world: &mut World) {
    let cx = self.screen_width / 2.0;
    let cy = self.screen_height / 2.0;

    // Fire on left side (away from gravity well)
    let fire = world.spawn((
      Transform::new(100.0, self.screen_height - 80.0),
      ParticleEmitter::fire(self.particle_sprite.clone())
        .with_mode(EmissionMode::Continuous { rate: 40.0 }),
    ));
    self.emitters.push(fire);

    // Smoke above fire
    let smoke = world.spawn((
      Transform::new(100.0, self.screen_height - 150.0),
      ParticleEmitter::smoke(self.particle_sprite.clone()),
    ));
    self.emitters.push(smoke);

    // Magic with vortex on right
    let magic = world.spawn((
      Transform::new(self.screen_width - 150.0, cy),
      ParticleEmitter::magic(self.particle_sprite.clone())
        .with_mode(EmissionMode::Continuous { rate: 25.0 }),
    ));
    self.emitters.push(magic);

    let vortex = world.spawn((
      Transform::new(self.screen_width - 150.0, cy),
      ParticleAttractor::vortex(100.0, 150.0),
    ));
    self.attractors.push(vortex);

    // Central gravity well (smaller radius to not affect fire)
    let gravity = world.spawn((
      Transform::new(cx, cy - 50.0),
      ParticleAttractor::gravity_well(80.0, 150.0),
    ));
    self.attractors.push(gravity);

    // Sparkle particles falling toward gravity well
    let sparkle = world.spawn((
      Transform::new(cx, 80.0),
      ParticleEmitter::new(self.particle_sprite.clone())
        .with_mode(EmissionMode::Continuous { rate: 30.0 })
        .with_shape(EmissionShape::Line {
          start_x: -120.0,
          start_y: 0.0,
          end_x: 120.0,
          end_y: 0.0,
        })
        .with_lifetime(3.0, 5.0)
        .with_size(4.0, 8.0)
        .with_color(Color::YELLOW, Color::new(255, 100, 0, 0))
        .with_gravity(0.0, 30.0)
        .with_speed(5.0, 15.0),
    ));
    self.emitters.push(sparkle);

    // Bubbles rising on far right
    let bubbles = world.spawn((
      Transform::new(self.screen_width - 50.0, self.screen_height - 50.0),
      ParticleEmitter::bubbles(self.particle_sprite.clone()),
    ));
    self.emitters.push(bubbles);
  }

  // ---------------------------------------------------------------------------
  // Cleanup finished one-shot emitters
  // ---------------------------------------------------------------------------

  fn cleanup_finished_emitters(&mut self, world: &mut World) {
    let mut to_remove = Vec::new();

    for (i, &entity) in self.emitters.iter().enumerate() {
      if let Some(emitter) = world.get_component::<ParticleEmitter>(entity)
        && emitter.is_finished() {
          to_remove.push(i);
        }
    }

    // Remove in reverse order to maintain indices
    for i in to_remove.into_iter().rev() {
      let entity = self.emitters.remove(i);
      let _ = world.despawn(entity);
    }
  }
}

impl Game for ParticlesDemo {
  fn init(&mut self, ctx: &mut GameContext) {
    println!("Particle System Demo");
    println!();
    println!("Controls:");
    println!("  Left/Right or 1-0 - Switch scenes");
    println!("  Click             - Spawn effects (Explosions/Impacts)");
    println!("  Mouse             - Control trails");
    println!("  Escape            - Quit");
    println!();

    // Store screen dimensions
    let (width, height) = ctx.screen_size();
    self.screen_width = width;
    self.screen_height = height;

    // Center camera for screen-style coordinates (0,0 = top-left)
    ctx.camera.set_position(width / 2.0, height / 2.0);

    // Create a simple white particle sprite
    let sprite = Sprite::solid_circle(16, 255, 255, 255, 255);
    self.particle_sprite = ctx.assets.create_sprite(sprite, "particle");

    // Set up initial scene
    self.setup_scene(ctx.world);
  }

  fn update(&mut self, ctx: &mut GameContext, dt: f32) {
    // Navigation cooldown
    if self.nav_cooldown > 0.0 {
      self.nav_cooldown -= dt;
    }

    // Scene switching with arrow keys
    if self.nav_cooldown <= 0.0 {
      let new_scene = if ctx.input.key_pressed(KeyCode::ArrowRight) {
        Some(self.current_scene.next())
      } else if ctx.input.key_pressed(KeyCode::ArrowLeft) {
        Some(self.current_scene.prev())
      } else if ctx.input.key_pressed(KeyCode::Digit1) {
        Some(DemoScene::Fire)
      } else if ctx.input.key_pressed(KeyCode::Digit2) {
        Some(DemoScene::Smoke)
      } else if ctx.input.key_pressed(KeyCode::Digit3) {
        Some(DemoScene::Explosions)
      } else if ctx.input.key_pressed(KeyCode::Digit4) {
        Some(DemoScene::Magic)
      } else if ctx.input.key_pressed(KeyCode::Digit5) {
        Some(DemoScene::Weather)
      } else if ctx.input.key_pressed(KeyCode::Digit6) {
        Some(DemoScene::Impacts)
      } else if ctx.input.key_pressed(KeyCode::Digit7) {
        Some(DemoScene::Celebration)
      } else if ctx.input.key_pressed(KeyCode::Digit8) {
        Some(DemoScene::Trails)
      } else if ctx.input.key_pressed(KeyCode::Digit9) {
        Some(DemoScene::Attractors)
      } else if ctx.input.key_pressed(KeyCode::Digit0) {
        Some(DemoScene::Combined)
      } else {
        None
      };

      if let Some(scene) = new_scene
        && scene != self.current_scene {
          self.nav_cooldown = 0.3;
          self.clear_scene(ctx.world);
          self.current_scene = scene;
          self.setup_scene(ctx.world);
        }
    }

    // Scene-specific interactions
    let (mouse_x, mouse_y) = ctx.input.mouse_position();
    let (world_x, world_y) = ctx.camera.screen_to_world(mouse_x, mouse_y);

    match self.current_scene {
      DemoScene::Explosions => {
        if ctx.input.mouse_pressed(MouseButton::Left) {
          self.spawn_explosion(ctx.world, world_x, world_y, false);
        }
        if ctx.input.mouse_pressed(MouseButton::Right) {
          self.spawn_explosion(ctx.world, world_x, world_y, true);
        }
      }
      DemoScene::Impacts => {
        if ctx.input.mouse_pressed(MouseButton::Left) {
          self.spawn_impact(ctx.world, world_x, world_y, true); // sparks
        }
        if ctx.input.mouse_pressed(MouseButton::Right) {
          self.spawn_impact(ctx.world, world_x, world_y, false); // dust
        }
      }
      DemoScene::Trails => {
        self.update_trails(ctx.world, world_x, world_y);
      }
      _ => {}
    }

    // Cleanup finished one-shot emitters
    self.cleanup_finished_emitters(ctx.world);

    // Update particle system
    self.particle_system.update(ctx.world, ctx.events, dt);

    // Quit on Escape
    if ctx.input.key_pressed(KeyCode::Escape) {
      std::process::exit(0);
    }
  }

  fn render_gpu(&mut self, gpu: &mut GpuRenderContext) {
    // Upload particle sprite texture
    gpu.upload_sprite(&self.particle_sprite);

    // Collect particles for rendering
    self
      .particle_system
      .collect_for_render(gpu.world, gpu.camera, gpu.batch);
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let (sw, sh) = ui.screen_size();

    // Scene title
    ui.draw_text(
      &format!(
        "Scene {}: {}",
        self.current_scene.index() + 1,
        self.current_scene.name()
      ),
      10.0,
      10.0,
      Color::WHITE,
    );

    // Description
    ui.draw_text(self.current_scene.description(), 10.0, 30.0, Color::YELLOW);

    // Particle count
    ui.draw_text(
      &format!("Particles: {}", self.particle_system.particle_count()),
      sw as f32 - 150.0,
      10.0,
      Color::GREEN,
    );

    // Navigation hint
    ui.draw_text(
      "Left/Right: Switch scenes | 1-0: Jump to scene",
      10.0,
      sh as f32 - 30.0,
      Color::new(150, 150, 150, 255),
    );

    // Attractor indicators
    if self.current_scene == DemoScene::Attractors {
      ui.draw_text("Point (attract)", 130.0, 170.0, Color::GREEN);
      ui.draw_text("Vortex (swirl)", sw as f32 - 220.0, 170.0, Color::CYAN);
      ui.draw_text(
        "Turbulence",
        sw as f32 / 2.0 - 40.0,
        sh as f32 - 130.0,
        Color::MAGENTA,
      );
      ui.draw_text(
        "Repel",
        sw as f32 / 2.0 - 20.0,
        sh as f32 / 2.0 - 130.0,
        Color::RED,
      );
    }

    // Scene-specific hints
    match self.current_scene {
      DemoScene::Explosions => {
        ui.draw_text(
          "Left-click: Explosion | Right-click: Debris",
          10.0,
          50.0,
          Color::CYAN,
        );
      }
      DemoScene::Impacts => {
        ui.draw_text(
          "Left-click: Sparks | Right-click: Dust",
          10.0,
          50.0,
          Color::CYAN,
        );
      }
      DemoScene::Trails => {
        ui.draw_text("Move mouse to control comet", 10.0, 50.0, Color::CYAN);
      }
      DemoScene::Weather => {
        ui.draw_text("Rain (left) | Snow (right)", 10.0, 50.0, Color::CYAN);
      }
      _ => {}
    }
  }
}

fn main() {
  EngineBuilder::new()
    .title("Particle System Demo")
    .fps(144)
    .physics_hz(144)
    .with_game(ParticlesDemo::new())
    .build()
    .run()
    .expect("Failed to run particle demo");
}
