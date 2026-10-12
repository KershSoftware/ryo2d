//! Hierarchy Sprites Demo - Ryo2D Example
//!
//! This example demonstrates the parent-child entity relationship system:
//! - `GlobalTransform` propagation through hierarchies (`ctx.spawn_child`, `ChildOf`)
//! - Children orbiting parents (solar system style)
//! - Nested hierarchies (planet → moon → satellite)
//! - Detaching a child in place with `detach_keep_world`
//! - Render interpolation: only the sun is `Interpolated`, and its children
//!   are interpolated with it
//!
//! A child's position is relative to its parent, so when the sun rotates the
//! planet orbits it, and so on down the chain. The fixed step runs at 20 Hz so
//! the difference interpolation makes is easy to see. Runs on the demo-kit shell.

use demo_kit::prelude::*;

/// Rotation speeds (degrees per second)
const SUN_ROTATION: f32 = 10.0;
const PLANET_ORBIT: f32 = 45.0;
const MOON_ORBIT: f32 = 120.0;
const SATELLITE_ORBIT: f32 = 240.0;

/// The satellite's place in the moon's frame
const SATELLITE_LOCAL: Vec2 = Vec2::new(15.0, 0.0);

/// Fixed steps per second: slow enough that motion steps visibly without interpolation
const FIXED_HZ: u32 = 20;

/// Tag components for our celestial bodies
#[derive(Component, Debug)]
struct Sun;

#[derive(Component, Debug)]
struct Planet;

#[derive(Component, Debug)]
struct Moon;

#[derive(Component, Debug)]
struct Satellite;

/// Page demonstrating hierarchies
struct HierarchyPage {
  sun: Option<Entity>,
  moon: Option<Entity>,
  satellite: Option<Entity>,
  rotating: bool,
  interpolate: bool,
  /// The interpolation setting last applied to the world
  applied_interpolate: bool,
  /// The satellite follows the moon (false once detached)
  attached: bool,
}

impl HierarchyPage {
  fn new() -> Self {
    Self {
      sun: None,
      moon: None,
      satellite: None,
      rotating: true,
      interpolate: true,
      applied_interpolate: true,
      attached: true,
    }
  }

  /// The hierarchy roots: the sun, and the satellite while it is detached
  fn roots(&self) -> impl Iterator<Item = Entity> {
    let detached = self.satellite.filter(|_| !self.attached);
    self.sun.into_iter().chain(detached)
  }

  /// Mark the roots `Interpolated` or not; their children follow them
  fn apply_interpolation(&mut self, world: &mut World) {
    for root in self.roots() {
      let mut root = world.entity_mut(root);
      if self.interpolate {
        root.insert(Interpolated);
      } else {
        root.remove::<Interpolated>();
      }
    }
    self.applied_interpolate = self.interpolate;
  }

  /// Detach the satellite where it is, or put it back in orbit around the moon
  fn toggle_satellite(&mut self, ctx: &mut SceneContext) {
    let (Some(moon), Some(satellite)) = (self.moon, self.satellite) else {
      return;
    };
    if self.attached {
      // Keeps its world position and stops following the moon; as a root it
      // is interpolated only if it is marked itself
      ctx.world.detach_keep_world(satellite);
      if self.interpolate {
        ctx.world.entity_mut(satellite).insert(Interpolated);
      }
    } else {
      ctx
        .world
        .entity_mut(satellite)
        .insert((ChildOf(moon), Transform::from_translation(SATELLITE_LOCAL)))
        .remove::<Interpolated>();
      // Jump straight back into orbit instead of sliding there
      ctx.skip_interpolation(satellite);
    }
    self.attached = !self.attached;
  }
}

impl DemoPage for HierarchyPage {
  fn name(&self) -> &str {
    "Solar System"
  }

  fn description(&self) -> &str {
    "Sun > planet > moon > satellite: each child is positioned relative to its parent\n\
     Rotating the sun carries the whole chain; each body also spins to orbit its parent.\n\
     Only the sun is Interpolated; its children are interpolated with it.\n\
     The fixed step runs at 20 Hz: turn Interpolation off to see the steps.\n\
     A detached satellite keeps its world position (detach_keep_world)."
  }

  fn settings(&mut self, s: &mut Settings) {
    s.toggle("Rotation", &mut self.rotating);
    s.toggle("Interpolation", &mut self.interpolate);
  }

  fn controls(&self) -> Vec<Control> {
    vec![Control::key(
      KeyCode::Space,
      "Detach / re-attach the satellite",
    )]
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

    // Create the sun at center of screen; it is the only entity marked
    // Interpolated, and its descendants are interpolated with it
    let (width, height) = ctx.screen_size();
    let sun = ctx
      .world
      .spawn((
        Sun,
        Transform::from_xy(width / 2.0, height / 2.0),
        Renderable::sprite(sun_handle),
        Interpolated,
      ))
      .id();

    // Create planet as child of sun (orbits around sun)
    // Local position is offset from sun center
    let planet = ctx.spawn_child(
      sun,
      (
        Planet,
        Transform::from_xy(120.0, 0.0), // 120 pixels right of sun
        Renderable::sprite(planet_handle),
      ),
    );

    // Create moon as child of planet (orbits around planet)
    let moon = ctx.spawn_child(
      planet,
      (
        Moon,
        Transform::from_xy(40.0, 0.0), // 40 pixels right of planet
        Renderable::sprite(moon_handle),
      ),
    );

    // Create satellite as child of moon (orbits around moon)
    let satellite = ctx.spawn_child(
      moon,
      (
        Satellite,
        Transform::from_translation(SATELLITE_LOCAL), // 15 pixels right of moon
        Renderable::sprite(satellite_handle),
      ),
    );

    self.sun = Some(sun);
    self.moon = Some(moon);
    self.satellite = Some(satellite);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    if self.interpolate != self.applied_interpolate {
      self.apply_interpolation(ctx.world);
    }

    if ctx.input.key_pressed(KeyCode::Space) {
      self.toggle_satellite(ctx);
    }

    // Update rotations if enabled
    if self.rotating
      && let Some(sun) = self.sun
    {
      // Rotate the sun (affects entire hierarchy)
      if let Some(mut transform) = ctx.world.get_mut::<Transform>(sun) {
        transform.rotation += SUN_ROTATION.to_radians() * dt;
      }

      // Rotate all planets (makes them orbit)
      for mut transform in ctx
        .world
        .query_filtered::<&mut Transform, With<Planet>>()
        .iter_mut(ctx.world)
      {
        transform.rotation += PLANET_ORBIT.to_radians() * dt;
      }

      // Rotate all moons (makes them orbit their planets)
      for mut transform in ctx
        .world
        .query_filtered::<&mut Transform, With<Moon>>()
        .iter_mut(ctx.world)
      {
        transform.rotation += MOON_ORBIT.to_radians() * dt;
      }

      // Rotate all satellites (spins in place once detached)
      for mut transform in ctx
        .world
        .query_filtered::<&mut Transform, With<Satellite>>()
        .iter_mut(ctx.world)
      {
        transform.rotation += SATELLITE_ORBIT.to_radians() * dt;
      }
    }
  }
}

fn main() {
  Demo::new("Hierarchy Sprites")
    .resolution(800, 600)
    .configure(|b| b.target_fps(60).fixed_hz(FIXED_HZ))
    .page(HierarchyPage::new)
    .run();
}
