//! Hierarchy Sprites Demo - Ryo2D Example
//!
//! This example demonstrates the parent-child entity relationship system:
//! - WorldTransform propagation through hierarchies
//! - Children orbiting parents (solar system style)
//! - Nested hierarchies (planet → moon → satellite)
//! - The unified Renderable component for sprite rendering
//!
//! A child's position is relative to its parent, so when the sun rotates the
//! planet orbits it, and so on down the chain. Runs on the demo-kit shell.

use demo_kit::prelude::*;

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

/// Page demonstrating hierarchies
struct HierarchyPage {
  sun: Option<Entity>,
  rotating: bool,
}

impl HierarchyPage {
  fn new() -> Self {
    Self {
      sun: None,
      rotating: true,
    }
  }
}

impl DemoPage for HierarchyPage {
  fn name(&self) -> &str {
    "Solar System"
  }

  fn description(&self) -> &str {
    "Sun > planet > moon > satellite: each child is positioned relative to its parent\n\
     Rotating the sun carries the whole chain; each body also spins to orbit its parent"
  }

  fn settings(&mut self, s: &mut Settings) {
    s.toggle("Rotation", &mut self.rotating);
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
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
    let (center_x, center_y) = ctx.screen_size();
    let sun = ctx.world.spawn((
      Sun,
      Transform::new(center_x / 2.0, center_y / 2.0),
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
    ctx.world.spawn_child(
      moon,
      (
        Satellite,
        Transform::new(15.0, 0.0), // 15 pixels right of moon
        Renderable::sprite(satellite_handle),
      ),
    );

    self.sun = Some(sun);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    // Update rotations if enabled
    if self.rotating
      && let Some(sun) = self.sun
    {
      // Rotate the sun (affects entire hierarchy)
      if let Some(mut transform) = ctx.world.get_mut::<Transform>(sun) {
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
}

fn main() {
  Demo::new("Hierarchy Sprites")
    .resolution(800, 600)
    .configure(|b| {
      b.window_mode(WindowMode::ResizableIntegerScale) // Pixel-perfect scaling
        .target_fps(60)
        .physics_hz(60)
    })
    .page(HierarchyPage::new)
    .run();
}
