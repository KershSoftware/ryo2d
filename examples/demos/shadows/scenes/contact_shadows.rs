//! Contact Shadows page
//!
//! Demonstrates contact shadows - small, concentrated shadows at contact points
//! between objects and surfaces. Contact shadows simulate ambient occlusion and
//! create visual grounding for objects.

use demo_kit::prelude::*;

/// Contact shadow page state
pub struct ContactShadowsPage {
  // Demo objects with contact shadows
  objects: Vec<Entity>,

  // Shadow settings
  radius: f32,
  intensity: f32,
  falloff: f32,
}

impl ContactShadowsPage {
  pub fn new() -> Self {
    Self {
      objects: Vec::new(),
      radius: 8.0,
      intensity: 0.6,
      falloff: 2.0,
    }
  }

  fn create_contact_shadow(&self) -> ContactShadow {
    ContactShadow::new()
      .radius(self.radius)
      .intensity(self.intensity)
      .falloff(self.falloff)
  }

  fn update_all_shadows(&mut self, ctx: &mut SceneContext) {
    let shadow = self.create_contact_shadow();
    for &entity in &self.objects {
      if let Some(mut renderable) = ctx.world.get_mut::<Renderable>(entity) {
        renderable.contact_shadow = Some(shadow.clone());
      }
    }
  }

  #[allow(clippy::too_many_arguments)]
  fn spawn_object(
    ctx: &mut SceneContext,
    x: f32,
    y: f32,
    width: u32,
    height: u32,
    color: (u8, u8, u8),
    name: &str,
    shadow: ContactShadow,
  ) -> Entity {
    let sprite = ctx.assets.create_sprite(
      Sprite::solid_rect(width, height, color.0, color.1, color.2, 255),
      name,
    );

    ctx.world.spawn((
      Transform::new(x, y).with_pivot(0.5, 1.0), // Pivot at bottom center
      Renderable::sprite(sprite)
        .with_z_order(5)
        .with_contact_shadow(shadow),
    ))
  }
}

impl DemoPage for ContactShadowsPage {
  fn name(&self) -> &str {
    "Contact"
  }

  fn description(&self) -> &str {
    "Small, concentrated shadows at object bases that ground them on the surface."
  }

  fn settings(&mut self, s: &mut Settings) {
    s.float("Radius", &mut self.radius, 2.0..=30.0, 1.0);
    s.float("Intensity", &mut self.intensity, 0.1..=1.0, 0.05);
    s.float("Falloff", &mut self.falloff, 0.5..=4.0, 0.25);
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    // Bright lighting to show contact shadows clearly
    if let Some(lighting) = ctx.lighting {
      lighting.set_ui_affected_by_lighting(false);
      lighting.set_ambient(AmbientLight::new(Color::WHITE, 0.95));
    }

    // Ground plane
    let ground = ctx
      .assets
      .create_sprite(Sprite::solid_rect(1280, 300, 180, 170, 160, 255), "ground");
    ctx.world.spawn((
      Transform::new(0.0, 420.0),
      Renderable::sprite(ground).with_z_order(0),
    ));

    // Sky
    let sky = ctx
      .assets
      .create_sprite(Sprite::solid_rect(1280, 420, 200, 220, 240, 255), "sky");
    ctx.world.spawn((
      Transform::new(0.0, 0.0),
      Renderable::sprite(sky).with_z_order(-1),
    ));

    let shadow = self.create_contact_shadow();

    // Create various objects showing contact shadows
    // Small boxes
    for i in 0..5 {
      let x = 150.0 + i as f32 * 100.0;
      let entity = Self::spawn_object(
        ctx,
        x,
        450.0,
        30,
        30,
        (150, 100, 80),
        &format!("box_{}", i),
        shadow.clone(),
      );
      self.objects.push(entity);
    }

    // Tall objects
    let tall_objects = [
      (700.0, 450.0, 20, 80, (100, 120, 150)),
      (800.0, 450.0, 25, 60, (120, 100, 150)),
      (900.0, 450.0, 30, 100, (150, 120, 100)),
    ];

    for (x, y, w, h, color) in tall_objects {
      let entity = Self::spawn_object(
        ctx,
        x,
        y,
        w,
        h,
        color,
        &format!("tall_{}", x as i32),
        shadow.clone(),
      );
      self.objects.push(entity);
    }

    // Wide objects (furniture-like)
    let wide_objects = [
      (1050.0, 450.0, 80, 40, (140, 100, 80)),
      (1150.0, 450.0, 60, 25, (100, 140, 100)),
    ];

    for (x, y, w, h, color) in wide_objects {
      let entity = Self::spawn_object(
        ctx,
        x,
        y,
        w,
        h,
        color,
        &format!("wide_{}", x as i32),
        shadow.clone(),
      );
      self.objects.push(entity);
    }

    // Character-like sprites
    let characters = [
      (250.0, 550.0, 32, 48, (100, 150, 200)),
      (400.0, 550.0, 32, 48, (200, 100, 150)),
      (550.0, 550.0, 32, 48, (150, 200, 100)),
    ];

    for (x, y, w, h, color) in characters {
      let entity = Self::spawn_object(
        ctx,
        x,
        y,
        w,
        h,
        color,
        &format!("char_{}", x as i32),
        shadow.clone(),
      );
      self.objects.push(entity);
    }
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    self.update_all_shadows(ctx);
  }
}
