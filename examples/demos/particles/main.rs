//! Particle System Demo - Ryo2D
//!
//! Comprehensive showcase of all particle presets and features.
//! Switch pages with `[` / `]` (the demo-kit shell).

use demo_kit::prelude::*;

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
      DemoScene::Fire => "Bonfire, torches, candles, and rising embers",
      DemoScene::Smoke => "Columns, thick plumes, ground fog, and colored steam",
      DemoScene::Explosions => "Auto-repeating demos + Click to spawn more",
      DemoScene::Magic => "Sparkles, healing, magic ring, and particle fountain",
      DemoScene::Weather => "Rain with wind (left) and snow (right)",
      DemoScene::Impacts => "Auto-repeating demos + Click to spawn more",
      DemoScene::Celebration => "Confetti bursts, bubbles, and fireworks",
      DemoScene::Trails => "Comet trail (mouse) and exhaust jets",
      DemoScene::Attractors => "Point, Vortex, Turbulence, and Repel attractors",
      DemoScene::Combined => "Fire, magic, confetti, gravity well, and more",
    }
  }
}

// =============================================================================
// Main Demo
// =============================================================================

struct ParticlesPage {
  current_scene: DemoScene,
  particle_sprite: SpriteHandle,
  screen_width: f32,
  screen_height: f32,

  // Particle system
  particle_system: ParticleSystem,

  // Scene-specific entities to track
  emitters: Vec<Entity>,
  attractors: Vec<Entity>,

  // Trail scene state
  trail_target_x: f32,
  trail_target_y: f32,
}

impl ParticlesPage {
  fn new(scene: DemoScene) -> Self {
    // Create a temporary placeholder sprite - will be replaced in enter
    let placeholder = Sprite::solid_circle(8, 255, 255, 255, 255);
    Self {
      current_scene: scene,
      particle_sprite: SpriteHandle::from_sprite(placeholder, "placeholder"),
      screen_width: 0.0,
      screen_height: 0.0,
      particle_system: ParticleSystem::new(),
      emitters: Vec::new(),
      attractors: Vec::new(),
      trail_target_x: 0.0,
      trail_target_y: 0.0,
    }
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
    let cx = self.screen_width / 2.0;

    // Large bonfire in center
    let fire = world
      .spawn((
        Transform::from_xy(cx, self.screen_height - 100.0),
        ParticleEmitter::fire(self.particle_sprite.clone()),
      ))
      .id();
    self.emitters.push(fire);

    // Small flames on sides (torches)
    for x in [200.0, self.screen_width - 200.0] {
      let flame = world
        .spawn((
          Transform::from_xy(x, self.screen_height - 80.0),
          ParticleEmitter::flame_small(self.particle_sprite.clone()),
        ))
        .id();
      self.emitters.push(flame);
    }

    // Row of candles spread across the scene
    for i in 0..7 {
      let x = 200.0 + i as f32 * 130.0;
      let candle = world
        .spawn((
          Transform::from_xy(x, self.screen_height - 220.0),
          ParticleEmitter::flame_small(self.particle_sprite.clone())
            .with_mode(EmissionMode::Continuous { rate: 8.0 }),
        ))
        .id();
      self.emitters.push(candle);
    }

    // Rising embers from the bonfire area
    let embers = world
      .spawn((
        Transform::from_xy(cx, self.screen_height - 120.0),
        ParticleEmitter::new(self.particle_sprite.clone())
          .with_mode(EmissionMode::Continuous { rate: 12.0 })
          .with_shape(EmissionShape::Line {
            start_x: -60.0,
            start_y: 0.0,
            end_x: 60.0,
            end_y: 0.0,
          })
          .with_lifetime(2.0, 4.0)
          .with_size(2.0, 4.0)
          .with_color(Color::new(255, 120, 20, 255), Color::new(255, 60, 0, 0))
          .with_gravity(0.0, -40.0)
          .with_speed(10.0, 30.0),
      ))
      .id();
    self.emitters.push(embers);
  }

  // ---------------------------------------------------------------------------
  // Smoke Scene
  // ---------------------------------------------------------------------------

  fn setup_smoke(&mut self, world: &mut World) {
    let cx = self.screen_width / 2.0;

    // Light smoke columns spread across scene
    for x in [200.0, 540.0, 880.0] {
      let smoke = world
        .spawn((
          Transform::from_xy(x, self.screen_height - 50.0),
          ParticleEmitter::smoke(self.particle_sprite.clone()),
        ))
        .id();
      self.emitters.push(smoke);
    }

    // Thick smoke (like from a fire)
    let thick = world
      .spawn((
        Transform::from_xy(cx, self.screen_height - 150.0),
        ParticleEmitter::smoke_thick(self.particle_sprite.clone()),
      ))
      .id();
    self.emitters.push(thick);

    // Ground-level fog drifting across the scene
    let fog = world
      .spawn((
        Transform::from_xy(cx, self.screen_height - 30.0),
        ParticleEmitter::new(self.particle_sprite.clone())
          .with_mode(EmissionMode::Continuous { rate: 8.0 })
          .with_shape(EmissionShape::Line {
            start_x: -400.0,
            start_y: 0.0,
            end_x: 400.0,
            end_y: 0.0,
          })
          .with_lifetime(3.0, 5.0)
          .with_size(20.0, 40.0)
          .with_color(Color::new(180, 180, 180, 40), Color::new(120, 120, 120, 0))
          .with_gravity(15.0, -5.0)
          .with_speed(2.0, 8.0),
      ))
      .id();
    self.emitters.push(fog);

    // Colored steam vent
    let steam = world
      .spawn((
        Transform::from_xy(self.screen_width - 200.0, self.screen_height - 50.0),
        ParticleEmitter::new(self.particle_sprite.clone())
          .with_mode(EmissionMode::Continuous { rate: 15.0 })
          .with_shape(EmissionShape::Cone {
            direction: -std::f32::consts::FRAC_PI_2,
            spread: 0.4,
            min_speed: 30.0,
            max_speed: 60.0,
          })
          .with_lifetime(1.5, 3.0)
          .with_size(6.0, 12.0)
          .with_color(Color::new(100, 200, 180, 120), Color::new(80, 160, 140, 0))
          .with_gravity(0.0, -10.0),
      ))
      .id();
    self.emitters.push(steam);
  }

  // ---------------------------------------------------------------------------
  // Explosions Scene (click to spawn)
  // ---------------------------------------------------------------------------

  fn setup_explosions(&mut self, world: &mut World) {
    // Auto-repeating explosion demos so the scene isn't empty
    let explosion1 = world
      .spawn((
        Transform::from_xy(300.0, self.screen_height / 2.0),
        ParticleEmitter::explosion(self.particle_sprite.clone(), 30).with_mode(
          EmissionMode::BurstRepeating {
            count: 30,
            interval: 2.5,
            repeats: None,
          },
        ),
      ))
      .id();
    self.emitters.push(explosion1);

    let explosion2 = world
      .spawn((
        Transform::from_xy(self.screen_width - 300.0, self.screen_height / 2.0),
        ParticleEmitter::explosion_debris(self.particle_sprite.clone(), 20).with_mode(
          EmissionMode::BurstRepeating {
            count: 20,
            interval: 3.0,
            repeats: None,
          },
        ),
      ))
      .id();
    self.emitters.push(explosion2);

    let explosion3 = world
      .spawn((
        Transform::from_xy(self.screen_width / 2.0, self.screen_height / 2.0 - 80.0),
        ParticleEmitter::explosion(self.particle_sprite.clone(), 50).with_mode(
          EmissionMode::BurstRepeating {
            count: 50,
            interval: 4.0,
            repeats: None,
          },
        ),
      ))
      .id();
    self.emitters.push(explosion3);
  }

  fn spawn_explosion(&mut self, world: &mut World, x: f32, y: f32, debris: bool) {
    if debris {
      let explosion = world
        .spawn((
          Transform::from_xy(x, y),
          ParticleEmitter::explosion_debris(self.particle_sprite.clone(), 25),
        ))
        .id();
      self.emitters.push(explosion);
    } else {
      let explosion = world
        .spawn((
          Transform::from_xy(x, y),
          ParticleEmitter::explosion(self.particle_sprite.clone(), 40),
        ))
        .id();
      self.emitters.push(explosion);
    }
  }

  // ---------------------------------------------------------------------------
  // Magic Scene
  // ---------------------------------------------------------------------------

  fn setup_magic(&mut self, world: &mut World) {
    let cx = self.screen_width / 2.0;
    let cy = self.screen_height / 2.0;

    // Magic sparkle effect
    let magic = world
      .spawn((
        Transform::from_xy(cx - 250.0, cy),
        ParticleEmitter::magic(self.particle_sprite.clone()),
      ))
      .id();
    self.emitters.push(magic);

    // Healing effect
    let heal = world
      .spawn((
        Transform::from_xy(cx + 250.0, cy),
        ParticleEmitter::magic_heal(self.particle_sprite.clone()),
      ))
      .id();
    self.emitters.push(heal);

    // Magic ring (hollow circle emitter)
    let ring = world
      .spawn((
        Transform::from_xy(cx, cy - 50.0),
        ParticleEmitter::new(self.particle_sprite.clone())
          .with_mode(EmissionMode::Continuous { rate: 40.0 })
          .with_shape(EmissionShape::Circle {
            radius: 80.0,
            hollow: true,
          })
          .with_lifetime(1.0, 2.0)
          .with_size(3.0, 6.0)
          .with_color(Color::new(180, 100, 255, 200), Color::new(100, 50, 255, 0))
          .with_speed(2.0, 8.0),
      ))
      .id();
    self.emitters.push(ring);

    // Particle fountain (cone pointing up)
    let fountain = world
      .spawn((
        Transform::from_xy(cx, self.screen_height - 80.0),
        ParticleEmitter::new(self.particle_sprite.clone())
          .with_mode(EmissionMode::Continuous { rate: 25.0 })
          .with_shape(EmissionShape::Cone {
            direction: -std::f32::consts::FRAC_PI_2,
            spread: 0.3,
            min_speed: 80.0,
            max_speed: 150.0,
          })
          .with_lifetime(1.5, 2.5)
          .with_size(4.0, 8.0)
          .with_color(Color::new(100, 200, 255, 200), Color::new(200, 100, 255, 0))
          .with_gravity(0.0, 60.0),
      ))
      .id();
    self.emitters.push(fountain);
  }

  // ---------------------------------------------------------------------------
  // Weather Scene
  // ---------------------------------------------------------------------------

  fn setup_weather(&mut self, world: &mut World) {
    // Rain covering left half (multiple emitters for wider coverage)
    for x in [200.0, 420.0] {
      let rain = world
        .spawn((
          Transform::from_xy(x, 0.0),
          // Live long enough to fall the full screen height
          ParticleEmitter::rain(self.particle_sprite.clone())
            .with_mode(EmissionMode::Continuous { rate: 60.0 })
            .with_lifetime(1.3, 1.5),
        ))
        .id();
      self.emitters.push(rain);
    }

    // Snow covering right half
    for x in [820.0, 1050.0] {
      let snow = world
        .spawn((
          Transform::from_xy(x, 0.0),
          // Snow drifts at about 40 px/s, so it needs ~20 s to cross the screen
          ParticleEmitter::snow(self.particle_sprite.clone())
            .with_mode(EmissionMode::Continuous { rate: 20.0 })
            .with_lifetime(19.0, 21.0),
        ))
        .id();
      self.emitters.push(snow);
    }

    // Wind pushing rain sideways
    let wind = world
      .spawn((
        Transform::from_xy(0.0, 0.0),
        ParticleAttractor::area(self.screen_width / 2.0, self.screen_height, 30.0, 0.0),
      ))
      .id();
    self.attractors.push(wind);
  }

  // ---------------------------------------------------------------------------
  // Impacts Scene (click to spawn)
  // ---------------------------------------------------------------------------

  fn setup_impacts(&mut self, world: &mut World) {
    // Auto-repeating spark demos
    let sparks1 = world
      .spawn((
        Transform::from_xy(300.0, self.screen_height / 2.0),
        ParticleEmitter::sparks(self.particle_sprite.clone()).with_mode(
          EmissionMode::BurstRepeating {
            count: 15,
            interval: 1.5,
            repeats: None,
          },
        ),
      ))
      .id();
    self.emitters.push(sparks1);

    let sparks2 = world
      .spawn((
        Transform::from_xy(self.screen_width - 300.0, self.screen_height / 2.0),
        ParticleEmitter::sparks(self.particle_sprite.clone()).with_mode(
          EmissionMode::BurstRepeating {
            count: 15,
            interval: 2.0,
            repeats: None,
          },
        ),
      ))
      .id();
    self.emitters.push(sparks2);

    // Auto-repeating dust demo
    let dust = world
      .spawn((
        Transform::from_xy(self.screen_width / 2.0, self.screen_height / 2.0 + 50.0),
        ParticleEmitter::dust(self.particle_sprite.clone()).with_mode(
          EmissionMode::BurstRepeating {
            count: 12,
            interval: 2.5,
            repeats: None,
          },
        ),
      ))
      .id();
    self.emitters.push(dust);
  }

  fn spawn_impact(&mut self, world: &mut World, x: f32, y: f32, is_sparks: bool) {
    if is_sparks {
      let sparks = world
        .spawn((
          Transform::from_xy(x, y),
          ParticleEmitter::sparks(self.particle_sprite.clone()),
        ))
        .id();
      self.emitters.push(sparks);
    } else {
      let dust = world
        .spawn((
          Transform::from_xy(x, y),
          ParticleEmitter::dust(self.particle_sprite.clone()),
        ))
        .id();
      self.emitters.push(dust);
    }
  }

  // ---------------------------------------------------------------------------
  // Celebration Scene
  // ---------------------------------------------------------------------------

  fn setup_celebration(&mut self, world: &mut World) {
    let cx = self.screen_width / 2.0;

    // Confetti bursts at multiple positions
    for x in [cx - 200.0, cx + 200.0] {
      let confetti = world
        .spawn((
          Transform::from_xy(x, self.screen_height - 50.0),
          ParticleEmitter::confetti(self.particle_sprite.clone()).with_mode(
            EmissionMode::BurstRepeating {
              count: 25,
              interval: 2.0,
              repeats: None,
            },
          ),
        ))
        .id();
      self.emitters.push(confetti);
    }

    // Bubbles rising from multiple positions
    for x in [200.0, 450.0, 700.0, 950.0, 1100.0] {
      let bubbles = world
        .spawn((
          Transform::from_xy(x, self.screen_height - 50.0),
          ParticleEmitter::bubbles(self.particle_sprite.clone()),
        ))
        .id();
      self.emitters.push(bubbles);
    }

    // Firework-like burst from top
    let firework = world
      .spawn((
        Transform::from_xy(cx, 120.0),
        ParticleEmitter::new(self.particle_sprite.clone())
          .with_mode(EmissionMode::BurstRepeating {
            count: 40,
            interval: 3.0,
            repeats: None,
          })
          .with_shape(EmissionShape::Point)
          .with_lifetime(1.5, 3.0)
          .with_size(3.0, 6.0)
          .with_color(Color::new(255, 200, 50, 255), Color::new(255, 80, 0, 0))
          .with_speed(40.0, 120.0)
          .with_gravity(0.0, 40.0),
      ))
      .id();
    self.emitters.push(firework);
  }

  // ---------------------------------------------------------------------------
  // Trails Scene
  // ---------------------------------------------------------------------------

  fn setup_trails(&mut self, world: &mut World) {
    self.trail_target_x = self.screen_width / 2.0;
    self.trail_target_y = self.screen_height / 2.0;

    // Comet trail (follows mouse)
    let comet = world
      .spawn((
        Transform::from_xy(self.trail_target_x, self.trail_target_y),
        ParticleEmitter::comet_trail(self.particle_sprite.clone()),
      ))
      .id();
    self.emitters.push(comet);

    // Exhaust trails at bottom corners
    for (i, x) in [300.0, self.screen_width - 300.0].iter().enumerate() {
      let exhaust = world
        .spawn((
          Transform::from_xy(*x, self.screen_height - 100.0),
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
        ))
        .id();
      self.emitters.push(exhaust);
    }

    // Center exhaust pointing straight up
    let center_exhaust = world
      .spawn((
        Transform::from_xy(self.screen_width / 2.0, self.screen_height - 60.0),
        ParticleEmitter::exhaust(self.particle_sprite.clone()).with_shape(EmissionShape::Cone {
          direction: -std::f32::consts::FRAC_PI_2,
          spread: 0.2,
          min_speed: 60.0,
          max_speed: 120.0,
        }),
      ))
      .id();
    self.emitters.push(center_exhaust);
  }

  fn update_trails(&mut self, world: &mut World, target_x: f32, target_y: f32) {
    self.trail_target_x = target_x;
    self.trail_target_y = target_y;

    // Update first emitter (comet) position
    if let Some(&entity) = self.emitters.first()
      && let Some(mut transform) = world.get_mut::<Transform>(entity)
    {
      // Smooth follow
      let target = Vec2::new(target_x, target_y);
      let current = transform.translation;
      transform.translation += (target - current) * 0.1;
    }
  }

  // ---------------------------------------------------------------------------
  // Attractors Scene
  // ---------------------------------------------------------------------------

  fn setup_attractors(&mut self, world: &mut World) {
    let cx = self.screen_width / 2.0;
    let cy = self.screen_height / 2.0;
    let [point_at, vortex_at, turbulence_at, repel_at] =
      attractor_sites(self.screen_width, self.screen_height).map(|site| (site.x, site.y));

    // Emitter near point attractor (top left)
    let emitter1 = world
      .spawn((
        Transform::from_xy(150.0, 250.0),
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
      ))
      .id();
    self.emitters.push(emitter1);

    // Emitter near vortex attractor (top right)
    let emitter2 = world
      .spawn((
        Transform::from_xy(self.screen_width - 150.0, 250.0),
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
      ))
      .id();
    self.emitters.push(emitter2);

    // Emitter near turbulence (bottom center)
    let emitter3 = world
      .spawn((
        Transform::from_xy(cx, self.screen_height - 200.0),
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
      ))
      .id();
    self.emitters.push(emitter3);

    // Emitter at center (affected by repel)
    let emitter4 = world
      .spawn((
        Transform::from_xy(cx, cy),
        ParticleEmitter::new(self.particle_sprite.clone())
          .with_mode(EmissionMode::Continuous { rate: 40.0 })
          .with_shape(EmissionShape::Point)
          .with_lifetime(3.0, 5.0)
          .with_size(5.0, 10.0)
          .with_color(Color::RED, Color::new(255, 50, 0, 0))
          .with_speed(10.0, 30.0),
      ))
      .id();
    self.emitters.push(emitter4);

    // Point attractor (gravity well) - top left
    let point = world
      .spawn((
        Transform::from_xy(point_at.0, point_at.1),
        ParticleAttractor::point(200.0, 180.0),
      ))
      .id();
    self.attractors.push(point);

    // Vortex attractor - top right
    let vortex = world
      .spawn((
        Transform::from_xy(vortex_at.0, vortex_at.1),
        ParticleAttractor::vortex(120.0, 180.0),
      ))
      .id();
    self.attractors.push(vortex);

    // Turbulence - bottom center
    let turbulence = world
      .spawn((
        Transform::from_xy(turbulence_at.0, turbulence_at.1),
        ParticleAttractor::turbulence(80.0, 40.0, 1.5),
      ))
      .id();
    self.attractors.push(turbulence);

    // Repelling attractor - center (pushes red particles outward)
    let repel = world
      .spawn((
        Transform::from_xy(repel_at.0, repel_at.1),
        ParticleAttractor::point(-120.0, 150.0), // Negative strength = repel
      ))
      .id();
    self.attractors.push(repel);
  }

  // ---------------------------------------------------------------------------
  // Combined Scene
  // ---------------------------------------------------------------------------

  fn setup_combined(&mut self, world: &mut World) {
    let cx = self.screen_width / 2.0;
    let cy = self.screen_height / 2.0;

    // Fire + smoke on the left (well-inset from edge)
    let fire = world
      .spawn((
        Transform::from_xy(200.0, self.screen_height - 80.0),
        ParticleEmitter::fire(self.particle_sprite.clone())
          .with_mode(EmissionMode::Continuous { rate: 40.0 }),
      ))
      .id();
    self.emitters.push(fire);

    let smoke = world
      .spawn((
        Transform::from_xy(200.0, self.screen_height - 160.0),
        ParticleEmitter::smoke(self.particle_sprite.clone()),
      ))
      .id();
    self.emitters.push(smoke);

    // Magic with vortex on the right (well-inset from edge)
    let magic = world
      .spawn((
        Transform::from_xy(self.screen_width - 250.0, cy),
        ParticleEmitter::magic(self.particle_sprite.clone())
          .with_mode(EmissionMode::Continuous { rate: 25.0 }),
      ))
      .id();
    self.emitters.push(magic);

    let vortex = world
      .spawn((
        Transform::from_xy(self.screen_width - 250.0, cy),
        ParticleAttractor::vortex(100.0, 180.0),
      ))
      .id();
    self.attractors.push(vortex);

    // Central gravity well
    let gravity = world
      .spawn((
        Transform::from_xy(cx, cy - 30.0),
        ParticleAttractor::gravity_well(80.0, 160.0),
      ))
      .id();
    self.attractors.push(gravity);

    // Sparkle particles falling toward gravity well
    let sparkle = world
      .spawn((
        Transform::from_xy(cx, 100.0),
        ParticleEmitter::new(self.particle_sprite.clone())
          .with_mode(EmissionMode::Continuous { rate: 25.0 })
          .with_shape(EmissionShape::Line {
            start_x: -150.0,
            start_y: 0.0,
            end_x: 150.0,
            end_y: 0.0,
          })
          .with_lifetime(3.0, 5.0)
          .with_size(3.0, 7.0)
          .with_color(Color::YELLOW, Color::new(255, 100, 0, 0))
          .with_gravity(0.0, 30.0)
          .with_speed(5.0, 15.0),
      ))
      .id();
    self.emitters.push(sparkle);

    // Bubbles rising from bottom-right (inset from edge)
    let bubbles = world
      .spawn((
        Transform::from_xy(self.screen_width - 150.0, self.screen_height - 50.0),
        ParticleEmitter::bubbles(self.particle_sprite.clone()),
      ))
      .id();
    self.emitters.push(bubbles);

    // Confetti burst from bottom-center
    let confetti = world
      .spawn((
        Transform::from_xy(cx, self.screen_height - 40.0),
        ParticleEmitter::confetti(self.particle_sprite.clone()).with_mode(
          EmissionMode::BurstRepeating {
            count: 20,
            interval: 3.5,
            repeats: None,
          },
        ),
      ))
      .id();
    self.emitters.push(confetti);

    // Small candle flames along the middle
    for i in 0..3 {
      let x = 400.0 + i as f32 * 200.0;
      let candle = world
        .spawn((
          Transform::from_xy(x, self.screen_height - 200.0),
          ParticleEmitter::flame_small(self.particle_sprite.clone())
            .with_mode(EmissionMode::Continuous { rate: 10.0 }),
        ))
        .id();
      self.emitters.push(candle);
    }
  }

  // ---------------------------------------------------------------------------
  // Cleanup finished one-shot emitters
  // ---------------------------------------------------------------------------

  fn cleanup_finished_emitters(&mut self, world: &mut World) {
    let mut to_remove = Vec::new();

    for (i, &entity) in self.emitters.iter().enumerate() {
      if let Some(emitter) = world.get::<ParticleEmitter>(entity)
        && emitter.is_finished()
      {
        to_remove.push(i);
      }
    }

    // Remove in reverse order to maintain indices
    for i in to_remove.into_iter().rev() {
      let entity = self.emitters.remove(i);
      world.despawn(entity);
    }
  }
}

impl DemoPage for ParticlesPage {
  fn name(&self) -> &str {
    self.current_scene.name()
  }

  fn description(&self) -> &str {
    self.current_scene.description()
  }

  fn controls(&self) -> Vec<Control> {
    match self.current_scene {
      DemoScene::Explosions => vec![Control::mouse("Left-click explosion, right-click debris")],
      DemoScene::Impacts => vec![Control::mouse("Left-click sparks, right-click dust")],
      DemoScene::Trails => vec![Control::mouse("Move to steer the comet")],
      _ => Vec::new(),
    }
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    // Store screen dimensions
    let (width, height) = ctx.screen_size();
    self.screen_width = width;
    self.screen_height = height;
    self.trail_target_x = width / 2.0;
    self.trail_target_y = height / 2.0;

    // Center camera for screen-style coordinates (0,0 = top-left)
    ctx.camera.set_position(width / 2.0, height / 2.0);

    // Create a simple white particle sprite
    let sprite = Sprite::solid_circle(16, 255, 255, 255, 255);
    self.particle_sprite = ctx.assets.create_sprite(sprite, "particle");

    // Set up initial scene
    self.setup_scene(ctx.world);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
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
    self.particle_system.update(ctx.world, dt);
  }

  fn render(&mut self, gpu: &mut GpuRenderContext) {
    // Collect particles for rendering
    self
      .particle_system
      .collect_for_render(gpu.world, gpu.camera, gpu.batch);
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    // Particle count, top right of the content area
    let area = content_area(ui);
    let count = format!("Particles: {}", self.particle_system.particle_count());
    let (text_w, _) = ui.text_size(&count);
    ui.draw_text(
      &count,
      area.x + area.width - text_w as f32,
      area.y,
      Color::GREEN,
    );

    // Mark each attractor, with its label centred below on a backing so it
    // reads over the particles
    if self.current_scene == DemoScene::Attractors {
      for site in attractor_sites(self.screen_width, self.screen_height) {
        ui.draw_rect(Rect::new(site.x - 5.0, site.y - 1.0, 11.0, 3.0), site.color);
        ui.draw_rect(Rect::new(site.x - 1.0, site.y - 5.0, 3.0, 11.0), site.color);
        let (text_w, text_h) = ui.text_size(site.label);
        let (text_w, text_h) = (text_w as f32, text_h as f32);
        let (x, y) = (site.x - text_w / 2.0, site.y + 10.0);
        ui.draw_rect(
          Rect::new(x - 3.0, y - 2.0, text_w + 6.0, text_h + 4.0),
          Color::new(12, 12, 20, 190),
        );
        ui.draw_text(site.label, x, y, site.color);
      }
    }
  }
}

/// One of the Attractors page's attractors: where it sits and its label
struct AttractorSite {
  x: f32,
  y: f32,
  label: &'static str,
  color: Color,
}

/// The Attractors page's point, vortex, turbulence and repel attractors on
/// a `w` x `h` screen
fn attractor_sites(w: f32, h: f32) -> [AttractorSite; 4] {
  let site = |x, y, label, color| AttractorSite { x, y, label, color };
  [
    site(150.0, 150.0, "Point (attract)", Color::GREEN),
    site(w - 150.0, 150.0, "Vortex (swirl)", Color::CYAN),
    site(w / 2.0, h - 100.0, "Turbulence", Color::MAGENTA),
    site(w / 2.0, h / 2.0 - 50.0, "Repel", Color::RED),
  ]
}

fn main() {
  let mut demo = Demo::new("Particles")
    .resolution(1280, 720)
    .configure(|b| b.target_fps(144).fixed_hz(144).vsync(false));
  for scene in DemoScene::ALL {
    demo = demo.page(move || ParticlesPage::new(scene));
  }
  demo.run();
}
