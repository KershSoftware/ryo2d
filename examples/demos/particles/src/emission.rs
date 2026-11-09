//! Emission: bursts that repeat, a trail laid by distance, the four shapes,
//! and world and local emitters on the move

use std::f32::consts::TAU;

use demo_kit::prelude::*;

use crate::shared::{count_label_below, emit, label, label_centered, particle_image, pointer};

/// Seconds without the mouse moving before the trail goes its own way
const IDLE: f32 = 2.0;

pub struct EmissionPage {
  /// The trail's emitter, which follows the pointer
  trail: Option<Entity>,
  /// The world and local emitters, which orbit
  orbiters: [Option<Entity>; 2],
  /// Seconds the page has run
  time: f32,
  /// Seconds since the mouse last moved
  still: f32,
  /// Orbit speed, turns per second
  orbit_speed: f32,
  /// How far round the orbit, radians
  orbit: f32,
}

impl Default for EmissionPage {
  fn default() -> Self {
    Self {
      trail: None,
      orbiters: [None; 2],
      time: 0.0,
      still: 0.0,
      orbit_speed: 0.25,
      orbit: 0.0,
    }
  }
}

/// Where the orbiting emitters circle, and how far out
const ORBIT_CENTER: Vec2 = Vec2::new(560.0, 560.0);
const ORBIT_RADIUS: f32 = 80.0;

/// The trail's path when the mouse is still: a loop of the lower left
fn wander(time: f32) -> Vec2 {
  Vec2::new(
    230.0 + 170.0 * (time * 1.3).cos(),
    540.0 + 90.0 * (time * 2.1).sin(),
  )
}

impl DemoPage for EmissionPage {
  fn name(&self) -> &str {
    "Emission"
  }

  fn description(&self) -> &str {
    "Repeating bursts, a trail by distance (mouse), four shapes, world and local emitters"
  }

  fn settings(&mut self, s: &mut Settings) {
    s.float("Orbit speed", &mut self.orbit_speed, 0.0..=1.0, 0.05);
  }

  fn controls(&self) -> Vec<Control> {
    vec![Control::mouse("Move to lay a trail")]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    ctx.camera().set_position(640.0, 360.0);
    let image = particle_image(ctx);
    let gold = Color::new(255, 200, 80, 255);
    let fade =
      |from: Color| Gradient::new([(0.0, from), (1.0, Color::new(from.r, from.g, from.b, 0))]);

    // Three bursts of 20 a quarter second apart, then a pause: a cycle of 2 s
    let bursts = ctx.assets().add(
      ParticleEffect::new(image.clone())
        .with_rate(0.0)
        .with_burst(Burst::at(0.0, 20.0).with_cycles(3, 0.25))
        .with_duration(2.0)
        .with_looping(true)
        .with_shape(EmitShape::Circle {
          radius: 6.0,
          arc_deg: 360.0,
          thickness: 1.0,
        })
        .outward()
        .with_speed((120.0, 160.0))
        .with_lifetime((0.8, 1.0))
        .with_size((6.0, 8.0))
        .with_color_over_life(fade(gold)),
    );
    emit(ctx, 180.0, 230.0, &bursts);

    // Laid by distance: 0.4 a pixel, so nothing while it stands still
    let trail = ctx.assets().add(
      ParticleEffect::new(image.clone())
        .with_rate(0.0)
        .with_rate_per_distance(0.4)
        .with_shape(EmitShape::Circle {
          radius: 4.0,
          arc_deg: 360.0,
          thickness: 1.0,
        })
        .with_speed((0.0, 20.0))
        .outward()
        .with_lifetime((0.8, 1.2))
        .with_size((6.0, 10.0))
        .with_color_over_life(fade(Color::new(255, 120, 200, 255)))
        .with_size_over_life(Curve::new([(0.0, 1.0), (1.0, 0.0)])),
    );
    self.trail = Some(emit(ctx, wander(0.0).x, wander(0.0).y, &trail));

    // The four shapes, each filling a place with still particles
    let shaped = |shape: EmitShape, color: Color| {
      ParticleEffect::new(image.clone())
        .with_rate(120.0)
        .with_shape(shape)
        .with_speed(0.0)
        .with_lifetime(0.7)
        .with_size(4.0)
        .with_color_over_life(fade(color))
    };
    let shapes = [
      (EmitShape::Point, Color::WHITE),
      (
        EmitShape::Circle {
          radius: 30.0,
          arc_deg: 270.0,
          thickness: 0.0,
        },
        Color::GREEN,
      ),
      (
        EmitShape::Rect {
          size: Vec2::new(70.0, 46.0),
          thickness: 0.0,
        },
        Color::MAGENTA,
      ),
      (EmitShape::Line { length: 80.0 }, Color::YELLOW),
    ];
    for (i, (shape, color)) in shapes.into_iter().enumerate() {
      let effect = ctx.assets().add(shaped(shape, color));
      emit(ctx, 410.0 + 95.0 * i as f32, 230.0, &effect);
    }

    // The same effect in the world and in the emitter's own space
    let orbiting = |space: Space, color: Color| {
      ParticleEffect::new(image.clone())
        .with_rate(60.0)
        .with_space(space)
        .with_direction(0.0, 20.0)
        .with_speed((60.0, 90.0))
        .with_lifetime((0.9, 1.1))
        .with_size(8.0)
        .with_color_over_life(fade(color))
    };
    let world = ctx
      .assets()
      .add(orbiting(Space::World, Color::new(255, 150, 40, 255)));
    let local = ctx.assets().add(orbiting(Space::Local, Color::CYAN));
    self.orbiters = [
      Some(emit(ctx, ORBIT_CENTER.x, ORBIT_CENTER.y, &world)),
      Some(emit(ctx, ORBIT_CENTER.x, ORBIT_CENTER.y, &local)),
    ];
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    self.time += dt;

    // The trail follows the pointer while it moves, and wanders when it rests
    let (dx, dy) = ctx.input.mouse_delta();
    if dx != 0.0 || dy != 0.0 {
      self.still = 0.0;
    } else {
      self.still += dt;
    }
    let target = if self.still < IDLE && self.time > IDLE {
      pointer(ctx)
    } else {
      wander(self.time)
    };
    if let Some(trail) = self.trail
      && let Some(mut transform) = ctx.world.get_mut::<Transform>(trail)
    {
      let current = transform.translation;
      transform.translation += (target - current) * 0.2;
    }

    // The orbiters circle opposite each other, turning as they go
    self.orbit = (self.orbit + self.orbit_speed * TAU * dt) % TAU;
    for (i, orbiter) in self.orbiters.into_iter().enumerate() {
      let angle = self.orbit + i as f32 * std::f32::consts::PI;
      if let Some(orbiter) = orbiter
        && let Some(mut transform) = ctx.world.get_mut::<Transform>(orbiter)
      {
        transform.translation = ORBIT_CENTER + Vec2::from_angle(angle) * ORBIT_RADIUS;
        transform.rotation = angle + std::f32::consts::FRAC_PI_2;
      }
    }
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    count_label_below(ui, ctx, 1);
    let white = Color::WHITE;
    label(ui, 60.0, 130.0, "Bursts: 3 x 20, every 0.25 s", white);
    label(ui, 60.0, 330.0, "Cycle of 2 s, looping", white);
    for (i, name) in ["Point", "Circle arc", "Rect edge", "Line"]
      .into_iter()
      .enumerate()
    {
      label_centered(ui, 410.0 + 95.0 * i as f32, 290.0, name, white);
    }
    label(ui, 60.0, 430.0, "Trail: 0.4 a pixel moved", white);
    label_centered(
      ui,
      ORBIT_CENTER.x,
      440.0,
      "Orange: World, Cyan: Local",
      white,
    );
  }
}
