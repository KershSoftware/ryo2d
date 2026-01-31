//! Hierarchy Sprites Demo - Ryo2D Example
//!
//! This example demonstrates the parent-child entity relationship system:
//! - WorldTransform propagation through hierarchies
//! - Children orbiting parents (solar system style)
//! - Nested hierarchies (planet → moon → satellite)
//! - The unified Renderable component for sprite rendering
//!
//! ## Controls
//!
//! - Space: Toggle automatic rotation
//! - Escape: Exit

use ryo2d::prelude::*;

/// Screen dimensions
const WIDTH: u32 = 800;
const HEIGHT: u32 = 600;

/// Rotation speeds (degrees per second)
const SUN_ROTATION: f32 = 10.0;
const PLANET_ORBIT: f32 = 45.0;
const MOON_ORBIT: f32 = 120.0;
const SATELLITE_ORBIT: f32 = 240.0;

/// Tag components for our celestial bodies
#[derive(Debug)]
struct Sun;

#[derive(Debug)]
struct Planet;

#[derive(Debug)]
struct Moon;

#[derive(Debug)]
struct Satellite;

/// Stores entity references for our solar system
struct SolarSystem {
  sun: Entity,
  #[allow(dead_code)]
  planet: Entity,
  #[allow(dead_code)]
  moon: Entity,
  #[allow(dead_code)]
  satellite: Entity,
  rotating: bool,
}

/// Main game scene demonstrating hierarchies
struct HierarchyScene {
  solar_system: Option<SolarSystem>,
}

impl HierarchyScene {
  fn new() -> Self {
    Self { solar_system: None }
  }
}

impl Scene for HierarchyScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    println!("Hierarchy Sprites Demo initialized!");
    println!("  Press SPACE to toggle rotation");
    println!("  Press ESC to exit");

    // Create sprites for our celestial bodies
    // Sun: large yellow circle
    let sun_sprite = Sprite::solid_circle(40, 255, 220, 50, 255);
    let sun_handle = SpriteHandle::from_sprite(sun_sprite, "sun");

    // Planet: medium blue circle
    let planet_sprite = Sprite::solid_circle(20, 100, 150, 255, 255);
    let planet_handle = SpriteHandle::from_sprite(planet_sprite, "planet");

    // Moon: small gray circle
    let moon_sprite = Sprite::solid_circle(8, 180, 180, 180, 255);
    let moon_handle = SpriteHandle::from_sprite(moon_sprite, "moon");

    // Satellite: tiny red square
    let satellite_sprite = Sprite::solid_rect(6, 6, 255, 80, 80, 255);
    let satellite_handle = SpriteHandle::from_sprite(satellite_sprite, "satellite");

    // Create the sun at center of screen
    let sun = ctx.world.spawn((
      Sun,
      Transform::new((WIDTH / 2) as f32, (HEIGHT / 2) as f32),
      Renderable::sprite(sun_handle),
    ));

    // Create planet as child of sun (orbits around sun)
    // Local position is offset from sun center
    let planet = ctx.world.spawn_child(
      sun,
      (
        Planet,
        Transform::new(120.0, 0.0), // 120 pixels right of sun
        Renderable::sprite(planet_handle),
      ),
    );

    // Create moon as child of planet (orbits around planet)
    let moon = ctx.world.spawn_child(
      planet,
      (
        Moon,
        Transform::new(40.0, 0.0), // 40 pixels right of planet
        Renderable::sprite(moon_handle),
      ),
    );

    // Create satellite as child of moon (orbits around moon)
    let satellite = ctx.world.spawn_child(
      moon,
      (
        Satellite,
        Transform::new(15.0, 0.0), // 15 pixels right of moon
        Renderable::sprite(satellite_handle),
      ),
    );

    self.solar_system = Some(SolarSystem {
      sun,
      planet,
      moon,
      satellite,
      rotating: true,
    });
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    // Toggle rotation with space
    if ctx.input.key_pressed(KeyCode::Space) {
      if let Some(ref mut ss) = self.solar_system {
        ss.rotating = !ss.rotating;
        println!("Rotation: {}", if ss.rotating { "ON" } else { "OFF" });
      }
    }

    // Exit on escape
    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Quit;
    }

    // Update rotations if enabled
    if let Some(ref ss) = self.solar_system {
      if ss.rotating {
        // Rotate the sun (affects entire hierarchy)
        if let Some(mut transform) = ctx.world.get_mut::<Transform>(ss.sun) {
          transform.rotation += SUN_ROTATION * dt;
        }

        // Rotate all planets (makes them orbit)
        for (transform, _planet) in ctx.world.query_mut::<(&mut Transform, &Planet)>() {
          transform.rotation += PLANET_ORBIT * dt;
        }

        // Rotate all moons (makes them orbit their planets)
        for (transform, _moon) in ctx.world.query_mut::<(&mut Transform, &Moon)>() {
          transform.rotation += MOON_ORBIT * dt;
        }

        // Rotate all satellites
        for (transform, _sat) in ctx.world.query_mut::<(&mut Transform, &Satellite)>() {
          transform.rotation += SATELLITE_ORBIT * dt;
        }
      }
    }

    SceneAction::Continue
  }

  fn render(&mut self, _ctx: &mut SceneContext, _alpha: f32) {
    // The Renderable components are drawn automatically by the engine
    // because we're using the new Renderable system!
    // No custom rendering needed here.
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    // Display controls and rotation status
    ui.draw_text("Controls:", 10.0, 10.0, Color::WHITE);
    ui.draw_text("  SPACE - Toggle rotation", 10.0, 30.0, Color::WHITE);
    ui.draw_text("  ESC   - Exit", 10.0, 50.0, Color::WHITE);

    // Show current rotation state
    if let Some(ref ss) = self.solar_system {
      let status_color = if ss.rotating {
        Color::GREEN
      } else {
        Color::RED
      };
      let status_text = if ss.rotating {
        "Rotation: ON"
      } else {
        "Rotation: OFF"
      };
      ui.draw_text(status_text, 10.0, 80.0, status_color);
    }
  }
}

fn main() {
  let engine = EngineBuilder::new()
    .title("Hierarchy Sprites Demo - Ryo2D")
    .resolution(WIDTH, HEIGHT)
    .window_mode(WindowMode::ResizableIntegerScale) // Pixel-perfect scaling
    .fps(60)
    .physics_hz(60)
    .with_scene(HierarchyScene::new())
    .build();

  if let Err(e) = engine.run() {
    eprintln!("Engine error: {}", e);
  }
}
