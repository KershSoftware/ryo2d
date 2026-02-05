//! Contact Shadow Scene
//!
//! Demonstrates contact shadows - small, concentrated shadows at contact points
//! between objects and surfaces. Contact shadows simulate ambient occlusion and
//! create visual grounding for objects.

use ryo2d::prelude::*;

/// Contact shadow demo scene
pub struct ContactShadowsScene {
  screen_w: f32,
  screen_h: f32,

  // Demo objects with contact shadows
  objects: Vec<Entity>,

  // Shadow settings
  radius: f32,
  intensity: f32,
  falloff: f32,
}

impl ContactShadowsScene {
  pub fn new() -> Self {
    Self {
      screen_w: 1280.0,
      screen_h: 720.0,
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
    let sprite = ctx
      .assets
      .create_sprite(Sprite::solid_rect(width, height, color.0, color.1, color.2, 255), name);

    ctx.world.spawn((
      Transform::new(x, y).with_pivot(0.5, 1.0), // Pivot at bottom center
      Renderable::sprite(sprite)
        .with_z_order(5)
        .with_contact_shadow(shadow),
    ))
  }
}

impl Scene for ContactShadowsScene {
  fn on_exit(&mut self, ctx: &mut SceneContext) {
    ctx.world.clear();
    self.objects.clear();
  }

  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let (w, h) = ctx.screen_size();
    self.screen_w = w;
    self.screen_h = h;

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
      let entity = Self::spawn_object(ctx, x, y, w, h, color, &format!("tall_{}", x as i32), shadow.clone());
      self.objects.push(entity);
    }

    // Wide objects (furniture-like)
    let wide_objects = [
      (1050.0, 450.0, 80, 40, (140, 100, 80)),
      (1150.0, 450.0, 60, 25, (100, 140, 100)),
    ];

    for (x, y, w, h, color) in wide_objects {
      let entity = Self::spawn_object(ctx, x, y, w, h, color, &format!("wide_{}", x as i32), shadow.clone());
      self.objects.push(entity);
    }

    // Character-like sprites
    let characters = [
      (250.0, 550.0, 32, 48, (100, 150, 200)),
      (400.0, 550.0, 32, 48, (200, 100, 150)),
      (550.0, 550.0, 32, 48, (150, 200, 100)),
    ];

    for (x, y, w, h, color) in characters {
      let entity = Self::spawn_object(ctx, x, y, w, h, color, &format!("char_{}", x as i32), shadow.clone());
      self.objects.push(entity);
    }
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    // Scene switching
    if ctx.input.key_pressed(KeyCode::Digit1) {
      return SceneAction::Switch(Box::new(super::HardShadowsScene::new()));
    }
    if ctx.input.key_pressed(KeyCode::Digit2) {
      return SceneAction::Switch(Box::new(super::DropShadowsScene::new()));
    }
    if ctx.input.key_pressed(KeyCode::Digit3) {
      return SceneAction::Switch(Box::new(super::BlobShadowsScene::new()));
    }
    if ctx.input.key_pressed(KeyCode::Digit4) {
      return SceneAction::Switch(Box::new(super::SoftShadowsScene::new()));
    }
    if ctx.input.key_pressed(KeyCode::Digit5) {
      return SceneAction::Switch(Box::new(super::ColoredShadowsScene::new()));
    }
    if ctx.input.key_pressed(KeyCode::Digit6) {
      // Already on this scene, but keeping consistent pattern
    }
    if ctx.input.key_pressed(KeyCode::Digit7) {
      return SceneAction::Switch(Box::new(super::AmbientOcclusionScene::new()));
    }
    if ctx.input.key_pressed(KeyCode::Digit8) {
      return SceneAction::Switch(Box::new(super::RaymarchedShadowsScene::new()));
    }

    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Quit;
    }

    let speed = 1.0 / 60.0;
    let mut changed = false;

    // Adjust radius (Q/A)
    if ctx.input.key_down(KeyCode::KeyQ) {
      self.radius = (self.radius + 10.0 * speed).min(30.0);
      changed = true;
    }
    if ctx.input.key_down(KeyCode::KeyA) {
      self.radius = (self.radius - 10.0 * speed).max(2.0);
      changed = true;
    }

    // Adjust intensity (W/S)
    if ctx.input.key_down(KeyCode::KeyW) {
      self.intensity = (self.intensity + 0.5 * speed).min(1.0);
      changed = true;
    }
    if ctx.input.key_down(KeyCode::KeyS) {
      self.intensity = (self.intensity - 0.5 * speed).max(0.1);
      changed = true;
    }

    // Adjust falloff (E/D)
    if ctx.input.key_down(KeyCode::KeyE) {
      self.falloff = (self.falloff + 2.0 * speed).min(4.0);
      changed = true;
    }
    if ctx.input.key_down(KeyCode::KeyD) {
      self.falloff = (self.falloff - 2.0 * speed).max(0.5);
      changed = true;
    }

    // Presets
    if ctx.input.key_pressed(KeyCode::Digit7) {
      // Sharp preset
      self.radius = 6.0;
      self.intensity = 0.7;
      self.falloff = 3.0;
      changed = true;
    }
    if ctx.input.key_pressed(KeyCode::Digit8) {
      // Soft preset
      self.radius = 12.0;
      self.intensity = 0.4;
      self.falloff = 1.0;
      changed = true;
    }
    if ctx.input.key_pressed(KeyCode::Digit9) {
      // Default preset
      self.radius = 8.0;
      self.intensity = 0.6;
      self.falloff = 2.0;
      changed = true;
    }

    if changed {
      self.update_all_shadows(ctx);
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let white = Color::WHITE;
    let gray = Color::new(180, 180, 180, 255);
    let highlight = Color::new(255, 255, 100, 255);
    let bg = Some(Color::new(0, 0, 0, 180));

    // Scene indicator
    ui.label(
      "[1] Hard [2] Drop [3] Blob [4] Soft [5] Color [6] Contact [7] AO [8] Ray",
      10.0,
      10.0,
      highlight,
      bg,
    );

    // Title
    ui.label("Contact Shadows (Ambient Occlusion)", 10.0, 40.0, white, bg);

    // Current settings
    ui.label(
      &format!("Radius: {:.1} (Q/A)", self.radius),
      10.0,
      70.0,
      gray,
      bg,
    );
    ui.label(
      &format!("Intensity: {:.2} (W/S)", self.intensity),
      10.0,
      95.0,
      gray,
      bg,
    );

    let falloff_desc = match self.falloff {
      f if f < 1.5 => "Soft",
      f if f < 2.5 => "Medium",
      _ => "Sharp",
    };
    ui.label(
      &format!("Falloff: {:.1} ({}) (E/D)", self.falloff, falloff_desc),
      10.0,
      120.0,
      gray,
      bg,
    );

    // Presets
    ui.label("Presets: [7] Sharp  [8] Soft  [9] Default", 10.0, 155.0, gray, bg);

    // Controls
    ui.label("Controls:", 10.0, 190.0, white, bg);
    ui.label("  Q/A: Radius +/-", 10.0, 210.0, gray, bg);
    ui.label("  W/S: Intensity +/-", 10.0, 230.0, gray, bg);
    ui.label("  E/D: Falloff +/-", 10.0, 250.0, gray, bg);
    ui.label("  1-6: Switch shadow scenes", 10.0, 270.0, gray, bg);
    ui.label("  ESC: Quit", 10.0, 290.0, gray, bg);

    // Description
    ui.label(
      "Small shadows at object bases",
      self.screen_w - 260.0,
      self.screen_h - 30.0,
      gray,
      bg,
    );
  }
}
