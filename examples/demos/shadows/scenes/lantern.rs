//! Lantern page
//!
//! A character carries a lantern among pillars that a second light also
//! lights. The lantern sits inside its bearer's occluder, and a light inside
//! a closed occluder is not shadowed by it: the bearer does not darken their
//! own light, while the pillars shadow both lights.

use demo_kit::prelude::*;

/// The character's size, and the lantern's place in it (from its centre)
const BEARER_SIZE: Vec2 = Vec2::new(24.0, 36.0);
const LANTERN_AT: Vec2 = Vec2::new(8.0, -6.0);
const SPEED: f32 = 220.0;

/// The pillars: centre and size
const PILLARS: [(f32, f32, f32, f32); 7] = [
  (420.0, 250.0, 56.0, 56.0),
  (620.0, 450.0, 48.0, 90.0),
  (300.0, 520.0, 70.0, 40.0),
  (760.0, 220.0, 40.0, 40.0),
  (840.0, 520.0, 60.0, 60.0),
  (560.0, 140.0, 90.0, 30.0),
  (180.0, 300.0, 40.0, 100.0),
];

/// The character carrying the lantern
#[derive(Component)]
pub struct Bearer;

/// The carried light, a child of the [`Bearer`]
#[derive(Component)]
pub struct Lantern;

/// Lantern page state
pub struct LanternPage {
  bearer: Option<Entity>,
  position: Vec2,
  radius: f32,
  flame: f32,
  /// The (radius, flame) the lantern was last given
  applied: Option<(f32, f32)>,
}

impl LanternPage {
  pub(crate) fn new() -> Self {
    Self {
      bearer: None,
      position: Vec2::new(240.0, 440.0),
      radius: 260.0,
      flame: 12.0,
      applied: None,
    }
  }

  fn lantern_light(&self) -> Light2D {
    Light2D::point(Color::new(255, 229, 168, 255), self.radius)
      .with_intensity(2.0)
      .with_soft_shadows(self.flame)
  }

  /// Moves by `step`, an axis at a time, out of the pillars and the screen
  fn walk(&mut self, step: Vec2) {
    let half = BEARER_SIZE / 2.0;
    for axis in [Vec2::X, Vec2::Y] {
      let mut next = self.position + step * axis;
      for (x, y, w, h) in PILLARS {
        let reach = half + Vec2::new(w, h) / 2.0;
        let apart = (next - Vec2::new(x, y)).abs();
        if apart.x < reach.x && apart.y < reach.y {
          next = self.position;
        }
      }
      self.position = next.clamp(half + Vec2::splat(8.0), Vec2::new(1272.0, 712.0) - half);
    }
  }
}

impl DemoPage for LanternPage {
  fn name(&self) -> &str {
    "Lantern"
  }

  fn description(&self) -> &str {
    "The lantern is inside its bearer, so the bearer casts no shadow from it; the pillars shadow both lights.\n\
A light inside a closed occluder is not shadowed by it."
  }

  fn settings(&mut self, s: &mut Settings) {
    s.float("Lantern radius", &mut self.radius, 100.0..=500.0, 10.0);
    s.float("Flame size", &mut self.flame, 0.0..=24.0, 2.0);
  }

  fn controls(&self) -> Vec<Control> {
    vec![Control::keys(
      "WASD",
      &[KeyCode::KeyW, KeyCode::KeyA, KeyCode::KeyS, KeyCode::KeyD],
      "Walk",
    )]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    ctx
      .lighting()
      .set_ambient(AmbientLight::new(Color::new(79, 75, 96, 255), 0.12));

    let floor = ctx
      .assets()
      .add(Image::solid_rect(1280, 720, 70, 64, 80, 255));
    ctx.world.spawn((
      Transform::from_xy(640.0, 360.0),
      Renderable::image(floor).with_z_order(0),
    ));

    for (x, y, w, h) in PILLARS {
      let sprite = ctx
        .assets()
        .add(Image::solid_rect(w as u32, h as u32, 85, 80, 100, 255));
      ctx.world.spawn((
        Transform::from_xy(x, y),
        Renderable::image(sprite).with_z_order(5),
        LightOccluder2D::rect(w, h),
      ));
    }

    // The second light: a fixed torch the pillars shadow
    let torch = ctx.assets().add(Image::solid_circle(12, 255, 150, 60, 255));
    ctx.world.spawn((
      Transform::from_xy(900.0, 360.0),
      Light2D::point(Color::new(255, 170, 90, 255), 380.0)
        .with_intensity(1.2)
        .with_soft_shadows(6.0),
      Renderable::image(torch).with_z_order(10),
    ));

    let body = ctx.assets().add(Image::solid_rect(
      BEARER_SIZE.x as u32,
      BEARER_SIZE.y as u32,
      90,
      140,
      200,
      255,
    ));
    let flame = ctx.assets().add(Image::solid_circle(8, 255, 240, 190, 255));
    let bearer = ctx
      .world
      .spawn((
        Bearer,
        Transform::from_translation(self.position),
        Renderable::image(body).with_z_order(6),
        LightOccluder2D::rect(BEARER_SIZE.x, BEARER_SIZE.y),
      ))
      .id();
    ctx.world.spawn((
      Lantern,
      ChildOf(bearer),
      Transform::from_translation(LANTERN_AT),
      self.lantern_light(),
      Renderable::image(flame).with_z_order(10),
    ));
    self.bearer = Some(bearer);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    let axis = |plus: KeyCode, minus: KeyCode| {
      f32::from(u8::from(ctx.input.key_down(plus))) - f32::from(u8::from(ctx.input.key_down(minus)))
    };
    let step = Vec2::new(
      axis(KeyCode::KeyD, KeyCode::KeyA),
      axis(KeyCode::KeyS, KeyCode::KeyW),
    );
    if step != Vec2::ZERO {
      self.walk(step.normalize() * SPEED * dt);
    }

    if let Some(bearer) = self.bearer
      && let Some(mut transform) = ctx.world.get_mut::<Transform>(bearer)
    {
      transform.translation = self.position;
    }
    if self.applied != Some((self.radius, self.flame)) {
      self.applied = Some((self.radius, self.flame));
      let light = self.lantern_light();
      let mut lanterns = ctx.world.query_filtered::<&mut Light2D, With<Lantern>>();
      for mut lantern in lanterns.iter_mut(ctx.world) {
        *lantern = light.clone();
      }
    }
  }
}
