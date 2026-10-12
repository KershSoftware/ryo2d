//! Drop Shadows page
//!
//! Demonstrates artist-controlled drop shadows for sprites.
//! Drop shadows are simple offset shadows that don't require light sources.

use demo_kit::prelude::*;

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
  const ALL: [ShadowPreset; 5] = [
    ShadowPreset::Default,
    ShadowPreset::Below,
    ShadowPreset::Diagonal,
    ShadowPreset::Squashed,
    ShadowPreset::Custom,
  ];

  const NAMES: [&'static str; 5] = ["Default", "Below", "Diagonal", "Squashed", "Custom"];

  fn create(&self, custom: &CustomShadowParams) -> DropShadow {
    match self {
      ShadowPreset::Default => DropShadow::default(),
      ShadowPreset::Below => DropShadow::below(8.0),
      ShadowPreset::Diagonal => DropShadow::diagonal(6.0),
      ShadowPreset::Squashed => DropShadow::squashed(12.0, 0.3),
      ShadowPreset::Custom => DropShadow::new()
        .offset(custom.offset_x, custom.offset_y)
        .color(Color::new(0, 0, 0, custom.alpha.clamp(0, 255) as u8))
        .scale(custom.scale_x, custom.scale_y),
    }
  }
}

/// Parameters for custom shadow
struct CustomShadowParams {
  offset_x: f32,
  offset_y: f32,
  alpha: i32,
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

/// Drop shadow page state
pub struct DropShadowsPage {
  // Demo sprites
  sprites: Vec<Entity>,

  // Current shadow preset (index into ShadowPreset::ALL)
  preset: usize,
  custom_params: CustomShadowParams,

  // Animation
  animation_time: f32,
  animate_sprites: bool,
}

impl DropShadowsPage {
  pub fn new() -> Self {
    Self {
      sprites: Vec::new(),
      preset: 0,
      custom_params: CustomShadowParams::default(),
      animation_time: 0.0,
      animate_sprites: true,
    }
  }

  fn current_preset(&self) -> ShadowPreset {
    ShadowPreset::ALL[self.preset.min(ShadowPreset::ALL.len() - 1)]
  }

  fn update_all_shadows(&mut self, ctx: &mut SceneContext) {
    let shadow = self.current_preset().create(&self.custom_params);
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

    ctx
      .world
      .spawn((
        Transform::from_xy(x, y),
        Renderable::sprite(sprite)
          .with_z_order(5)
          .with_drop_shadow(shadow),
      ))
      .id()
  }

  fn spawn_character_sprite(
    ctx: &mut SceneContext,
    x: f32,
    y: f32,
    name: &str,
    shadow: DropShadow,
  ) -> Entity {
    // Create a simple character-like sprite (head + body)
    let sprite = ctx
      .assets
      .create_sprite(Sprite::solid_rect(32, 48, 100, 150, 200, 255), name);

    ctx
      .world
      .spawn((
        Transform::from_xy(x, y),
        Renderable::sprite(sprite)
          .with_anchor(Anchor::BottomCenter) // Placed by the feet
          .with_z_order(5)
          .with_drop_shadow(shadow),
      ))
      .id()
  }
}

impl DemoPage for DropShadowsPage {
  fn name(&self) -> &str {
    "Drop"
  }

  fn description(&self) -> &str {
    "Artist-controlled offset shadows via Renderable.with_drop_shadow()."
  }

  fn settings(&mut self, s: &mut Settings) {
    s.choice("Style", &mut self.preset, &ShadowPreset::NAMES);
    s.toggle("Animation", &mut self.animate_sprites);
    if self.current_preset() == ShadowPreset::Custom {
      let p = &mut self.custom_params;
      s.float("Offset X", &mut p.offset_x, -40.0..=40.0, 1.0);
      s.float("Offset Y", &mut p.offset_y, -40.0..=40.0, 1.0);
      s.int("Alpha", &mut p.alpha, 0..=255);
      s.float("Scale X", &mut p.scale_x, 0.1..=2.0, 0.1);
      s.float("Scale Y", &mut p.scale_y, 0.1..=2.0, 0.1);
    }
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    // Configure lighting - brighter for drop shadow demo
    ctx
      .lighting()
      .set_ambient(AmbientLight::new(Color::WHITE, 0.9));

    // Create floor/ground
    let floor = ctx.assets.create_sprite(
      Sprite::solid_rect(1280, 400, 80, 120, 80, 255), // Grass-like green
      "ground",
    );
    ctx.world.spawn((
      Transform::from_xy(640.0, 520.0),
      Renderable::sprite(floor).with_z_order(0),
    ));

    // Create sky background
    let sky = ctx.assets.create_sprite(
      Sprite::solid_rect(1280, 320, 135, 180, 220, 255), // Light blue
      "sky",
    );
    ctx.world.spawn((
      Transform::from_xy(640.0, 160.0),
      Renderable::sprite(sky).with_z_order(-1),
    ));

    let shadow = self.current_preset().create(&self.custom_params);

    // Spawn demo sprites in a row
    // Different colored boxes to show shadows on various sprites
    let colors = [
      (220, 80, 80),  // Red
      (80, 180, 80),  // Green
      (80, 80, 220),  // Blue
      (220, 180, 80), // Yellow
      (180, 80, 180), // Purple
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

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    self.update_all_shadows(ctx);

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
            transform.translation.y = 400.0 + bob;
          } else {
            // Characters bob less
            transform.translation.y = 500.0 + bob * 0.5;
          }
        }
      }
    }
  }
}
