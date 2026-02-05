//! Drop Shadow Scene
//!
//! Demonstrates artist-controlled drop shadows for sprites.
//! Drop shadows are simple offset shadows that don't require light sources.

use ryo2d::prelude::*;

/// Shadow preset for demonstration
#[derive(Clone, Copy, PartialEq)]
enum ShadowPreset {
  Default,
  Below,
  Diagonal,
  Squashed,
  Custom,
}

impl ShadowPreset {
  fn name(&self) -> &'static str {
    match self {
      ShadowPreset::Default => "Default",
      ShadowPreset::Below => "Below",
      ShadowPreset::Diagonal => "Diagonal",
      ShadowPreset::Squashed => "Squashed (Isometric)",
      ShadowPreset::Custom => "Custom",
    }
  }

  fn create(&self, custom: &CustomShadowParams) -> DropShadow {
    match self {
      ShadowPreset::Default => DropShadow::default(),
      ShadowPreset::Below => DropShadow::below(8.0),
      ShadowPreset::Diagonal => DropShadow::diagonal(6.0),
      ShadowPreset::Squashed => DropShadow::squashed(12.0, 0.3),
      ShadowPreset::Custom => DropShadow::new()
        .offset(custom.offset_x, custom.offset_y)
        .color(Color::new(0, 0, 0, custom.alpha))
        .scale(custom.scale_x, custom.scale_y),
    }
  }
}

/// Parameters for custom shadow
struct CustomShadowParams {
  offset_x: f32,
  offset_y: f32,
  alpha: u8,
  scale_x: f32,
  scale_y: f32,
}

impl Default for CustomShadowParams {
  fn default() -> Self {
    Self {
      offset_x: 4.0,
      offset_y: 8.0,
      alpha: 100,
      scale_x: 1.0,
      scale_y: 0.6,
    }
  }
}

/// Drop shadow demo scene
pub struct DropShadowsScene {
  screen_w: f32,
  screen_h: f32,

  // Demo sprites
  sprites: Vec<Entity>,

  // Current shadow preset
  current_preset: ShadowPreset,
  custom_params: CustomShadowParams,

  // Animation
  animation_time: f32,
  animate_sprites: bool,
}

impl DropShadowsScene {
  pub fn new() -> Self {
    Self {
      screen_w: 1280.0,
      screen_h: 720.0,
      sprites: Vec::new(),
      current_preset: ShadowPreset::Default,
      custom_params: CustomShadowParams::default(),
      animation_time: 0.0,
      animate_sprites: true,
    }
  }

  fn update_all_shadows(&mut self, ctx: &mut SceneContext) {
    let shadow = self.current_preset.create(&self.custom_params);
    for &entity in &self.sprites {
      if let Some(mut renderable) = ctx.world.get_mut::<Renderable>(entity) {
        renderable.drop_shadow = Some(shadow.clone());
      }
    }
  }

  fn spawn_demo_sprite(
    ctx: &mut SceneContext,
    x: f32,
    y: f32,
    size: u32,
    color: (u8, u8, u8),
    name: &str,
    shadow: DropShadow,
  ) -> Entity {
    let sprite = ctx.assets.create_sprite(
      Sprite::solid_rect(size, size, color.0, color.1, color.2, 255),
      name,
    );

    ctx.world.spawn((
      Transform::new(x, y).with_pivot_center(),
      Renderable::sprite(sprite)
        .with_z_order(5)
        .with_drop_shadow(shadow),
    ))
  }

  fn spawn_character_sprite(
    ctx: &mut SceneContext,
    x: f32,
    y: f32,
    name: &str,
    shadow: DropShadow,
  ) -> Entity {
    // Create a simple character-like sprite (head + body)
    let sprite = ctx.assets.create_sprite(
      Sprite::solid_rect(32, 48, 100, 150, 200, 255),
      name,
    );

    ctx.world.spawn((
      Transform::new(x, y).with_pivot(0.5, 1.0), // Pivot at feet
      Renderable::sprite(sprite)
        .with_z_order(5)
        .with_drop_shadow(shadow),
    ))
  }
}

impl Scene for DropShadowsScene {
  fn on_exit(&mut self, ctx: &mut SceneContext) {
    // Clean up all entities when leaving scene
    ctx.world.clear();
    self.sprites.clear();
  }

  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let (w, h) = ctx.screen_size();
    self.screen_w = w;
    self.screen_h = h;

    // Configure lighting - brighter for drop shadow demo
    if let Some(lighting) = ctx.lighting {
      lighting.set_ui_affected_by_lighting(false);
      lighting.set_ambient(AmbientLight::new(Color::WHITE, 0.9));
    }

    // Create floor/ground
    let floor = ctx.assets.create_sprite(
      Sprite::solid_rect(1280, 400, 80, 120, 80, 255), // Grass-like green
      "ground",
    );
    ctx.world.spawn((
      Transform::new(640.0, 520.0).with_pivot_center(),
      Renderable::sprite(floor).with_z_order(0),
    ));

    // Create sky background
    let sky = ctx.assets.create_sprite(
      Sprite::solid_rect(1280, 320, 135, 180, 220, 255), // Light blue
      "sky",
    );
    ctx.world.spawn((
      Transform::new(640.0, 160.0).with_pivot_center(),
      Renderable::sprite(sky).with_z_order(-1),
    ));

    let shadow = self.current_preset.create(&self.custom_params);

    // Spawn demo sprites in a row
    // Different colored boxes to show shadows on various sprites
    let colors = [
      (220, 80, 80),   // Red
      (80, 180, 80),   // Green
      (80, 80, 220),   // Blue
      (220, 180, 80),  // Yellow
      (180, 80, 180),  // Purple
    ];

    let start_x = 200.0;
    let spacing = 180.0;
    let y = 400.0;

    for (i, &color) in colors.iter().enumerate() {
      let entity = Self::spawn_demo_sprite(
        ctx,
        start_x + i as f32 * spacing,
        y,
        48,
        color,
        &format!("box_{}", i),
        shadow.clone(),
      );
      self.sprites.push(entity);
    }

    // Add some "character" sprites at the bottom
    let char_y = 500.0;
    for i in 0..3 {
      let entity = Self::spawn_character_sprite(
        ctx,
        300.0 + i as f32 * 250.0,
        char_y,
        &format!("char_{}", i),
        shadow.clone(),
      );
      self.sprites.push(entity);
    }
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    // Scene switching
    if ctx.input.key_pressed(KeyCode::Digit1) {
      return SceneAction::Switch(Box::new(super::HardShadowsScene::new()));
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
      return SceneAction::Switch(Box::new(super::ContactShadowsScene::new()));
    }
    if ctx.input.key_pressed(KeyCode::Digit7) {
      return SceneAction::Switch(Box::new(super::AmbientOcclusionScene::new()));
    }

    // Quit on Escape
    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Quit;
    }

    // Cycle shadow presets with Tab
    if ctx.input.key_pressed(KeyCode::Tab) {
      self.current_preset = match self.current_preset {
        ShadowPreset::Default => ShadowPreset::Below,
        ShadowPreset::Below => ShadowPreset::Diagonal,
        ShadowPreset::Diagonal => ShadowPreset::Squashed,
        ShadowPreset::Squashed => ShadowPreset::Custom,
        ShadowPreset::Custom => ShadowPreset::Default,
      };
      self.update_all_shadows(ctx);
    }

    // Toggle animation
    if ctx.input.key_pressed(KeyCode::Space) {
      self.animate_sprites = !self.animate_sprites;
    }

    // Custom shadow adjustments (when in custom mode)
    if self.current_preset == ShadowPreset::Custom {
      let mut changed = false;

      // Offset X (A/D)
      if ctx.input.key_down(KeyCode::KeyA) {
        self.custom_params.offset_x -= 20.0 * dt;
        changed = true;
      }
      if ctx.input.key_down(KeyCode::KeyD) {
        self.custom_params.offset_x += 20.0 * dt;
        changed = true;
      }

      // Offset Y (W/S)
      if ctx.input.key_down(KeyCode::KeyW) {
        self.custom_params.offset_y -= 20.0 * dt;
        changed = true;
      }
      if ctx.input.key_down(KeyCode::KeyS) {
        self.custom_params.offset_y += 20.0 * dt;
        changed = true;
      }

      // Alpha (Q/E)
      if ctx.input.key_down(KeyCode::KeyQ) {
        self.custom_params.alpha = self.custom_params.alpha.saturating_sub((100.0 * dt) as u8);
        changed = true;
      }
      if ctx.input.key_down(KeyCode::KeyE) {
        self.custom_params.alpha = self.custom_params.alpha.saturating_add((100.0 * dt) as u8);
        changed = true;
      }

      // Scale X (Z/C)
      if ctx.input.key_down(KeyCode::KeyZ) {
        self.custom_params.scale_x = (self.custom_params.scale_x - dt).max(0.1);
        changed = true;
      }
      if ctx.input.key_down(KeyCode::KeyC) {
        self.custom_params.scale_x = (self.custom_params.scale_x + dt).min(2.0);
        changed = true;
      }

      // Scale Y (X/V) - using X for down since it's below Z
      if ctx.input.key_down(KeyCode::KeyX) {
        self.custom_params.scale_y = (self.custom_params.scale_y - dt).max(0.1);
        changed = true;
      }
      if ctx.input.key_down(KeyCode::KeyV) {
        self.custom_params.scale_y = (self.custom_params.scale_y + dt).min(2.0);
        changed = true;
      }

      if changed {
        self.update_all_shadows(ctx);
      }
    }

    // Animate sprites (bobbing motion)
    if self.animate_sprites {
      self.animation_time += dt;

      for (i, &entity) in self.sprites.iter().enumerate() {
        if let Some(mut transform) = ctx.world.get_mut::<Transform>(entity) {
          // Each sprite bobs with a phase offset
          let phase = i as f32 * 0.5;
          let bob = (self.animation_time * 3.0 + phase).sin() * 15.0;

          // Only adjust Y for the first row (boxes)
          if i < 5 {
            transform.y = 400.0 + bob;
          } else {
            // Characters bob less
            transform.y = 500.0 + bob * 0.5;
          }
        }
      }
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let white = Color::WHITE;
    let gray = Color::new(180, 180, 180, 255);
    let highlight = Color::new(255, 255, 100, 255);
    let bg = Some(Color::new(0, 0, 0, 180));

    // Scene indicator
    ui.label("[1] Hard [2] Drop [3] Blob [4] Soft [5] Color [6] Contact [7] AO", 10.0, 10.0, highlight, bg);

    // Title
    ui.label("Drop Shadows (Artist-Controlled)", 10.0, 40.0, white, bg);

    // Current preset
    ui.label(
      &format!("Preset: {} (Tab to cycle)", self.current_preset.name()),
      10.0,
      70.0,
      highlight,
      bg,
    );

    // Animation status
    let anim_status = if self.animate_sprites { "ON" } else { "OFF" };
    ui.label(
      &format!("Animation: {} (Space)", anim_status),
      10.0,
      100.0,
      gray,
      bg,
    );

    // Custom controls (only shown in custom mode)
    if self.current_preset == ShadowPreset::Custom {
      ui.label("Custom Shadow Controls:", 10.0, 135.0, white, bg);
      ui.label(
        &format!("  Offset: ({:.1}, {:.1}) [A/D, W/S]",
          self.custom_params.offset_x,
          self.custom_params.offset_y),
        10.0, 155.0, gray, bg,
      );
      ui.label(
        &format!("  Alpha: {} [Q/E]", self.custom_params.alpha),
        10.0, 175.0, gray, bg,
      );
      ui.label(
        &format!("  Scale: ({:.2}, {:.2}) [Z/C, X/V]",
          self.custom_params.scale_x,
          self.custom_params.scale_y),
        10.0, 195.0, gray, bg,
      );
    }

    // Description
    let desc_y = if self.current_preset == ShadowPreset::Custom { 230.0 } else { 135.0 };

    let description = match self.current_preset {
      ShadowPreset::Default => "Simple offset shadow (4px diagonal)",
      ShadowPreset::Below => "Shadow directly below sprite",
      ShadowPreset::Diagonal => "Classic drop shadow at 45 degrees",
      ShadowPreset::Squashed => "Flattened shadow for top-down/isometric",
      ShadowPreset::Custom => "Fully customizable shadow parameters",
    };
    ui.label(description, 10.0, desc_y, gray, bg);

    // Feature description
    ui.label(
      "Drop shadows via Renderable.with_drop_shadow()",
      self.screen_w - 360.0,
      self.screen_h - 30.0,
      gray,
      bg,
    );

    // ESC hint
    ui.label("ESC: Quit", 10.0, self.screen_h - 30.0, gray, bg);
  }
}
