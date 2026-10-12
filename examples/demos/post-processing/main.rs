//! Post-Processing Effects Demo
//!
//! Fade, sepia, vignette, screen flash, CRT, chromatic aberration and a custom
//! WGSL scanlines effect (`assets/shaders/scanlines.wgsl`) over a colourful tile
//! scene. Runs on the demo-kit shell; the chosen effect's values are settings.

use demo_kit::prelude::*;

/// How dark the vignette's edges get; its settings move where and how softly
const VIGNETTE_INTENSITY: f32 = 0.8;

#[derive(Debug, Clone, Copy, PartialEq)]
enum DemoEffect {
  None,
  Fade,
  ColorTint,
  Vignette,
  ScreenFlash,
  Crt,
  ChromaticAberration,
  CustomScanlines,
}

impl DemoEffect {
  const ALL: [DemoEffect; 8] = [
    DemoEffect::None,
    DemoEffect::Fade,
    DemoEffect::ColorTint,
    DemoEffect::Vignette,
    DemoEffect::ScreenFlash,
    DemoEffect::Crt,
    DemoEffect::ChromaticAberration,
    DemoEffect::CustomScanlines,
  ];

  const NAMES: [&'static str; 8] = [
    "None",
    "Fade",
    "Sepia",
    "Vignette",
    "Screen flash",
    "CRT",
    "Chromatic aberration",
    "Custom scanlines",
  ];

  fn description(&self) -> &'static str {
    match self {
      DemoEffect::None => "No post-processing effects.",
      DemoEffect::Fade => "Alpha fade for scene transitions.",
      DemoEffect::ColorTint => "Sepia colour grading.",
      DemoEffect::Vignette => "Edge darkening.",
      DemoEffect::ScreenFlash => "Impact flash effect: press Space.",
      DemoEffect::Crt => "Retro CRT scanlines and distortion. Always on.",
      DemoEffect::ChromaticAberration => "RGB channel offset.",
      DemoEffect::CustomScanlines => "Game-authored WGSL post-process effect (scanlines.wgsl).",
    }
  }
}

struct PostProcessingPage {
  // Scene objects
  background_sprites: Vec<Entity>,
  rotating_sprites: Vec<Entity>,

  // Effect state
  effect: usize,
  last_effect: usize,

  // Effect parameters (settings rows)
  fade_amount: f32,
  sepia_intensity: f32,
  vignette_radius: f32,
  vignette_softness: f32,
  chromatic_strength: f32,
  scanline_strength: f32,

  // Custom WGSL effect, None if the shader file could not be loaded
  scanlines: Option<ShaderHandle>,

  // Animation
  time: f32,

  // Screen dimensions (set in enter)
  screen_width: f32,
  screen_height: f32,
}

impl PostProcessingPage {
  fn new() -> Self {
    Self {
      background_sprites: Vec::new(),
      rotating_sprites: Vec::new(),
      effect: 0,
      last_effect: 0,
      fade_amount: 0.5,
      sepia_intensity: 0.8,
      vignette_radius: 0.7,
      vignette_softness: 0.5,
      chromatic_strength: 0.005,
      scanline_strength: 0.4,
      scanlines: None,
      time: 0.0,
      screen_width: 0.0,
      screen_height: 0.0,
    }
  }

  fn create_colorful_scene(&mut self, ctx: &mut SceneContext) {
    let cx = self.screen_width / 2.0;
    let cy = self.screen_height / 2.0;

    // Create a grid of colorful rectangles in the background
    let colors = [
      (255, 100, 100), // Red
      (100, 255, 100), // Green
      (100, 100, 255), // Blue
      (255, 255, 100), // Yellow
      (255, 100, 255), // Magenta
      (100, 255, 255), // Cyan
    ];

    let grid_size = 4;
    let spacing = 120.0;
    let start_x = cx - (grid_size as f32 * spacing) / 2.0 + spacing / 2.0;
    let start_y = cy - (grid_size as f32 * spacing) / 2.0 + spacing / 2.0;

    for row in 0..grid_size {
      for col in 0..grid_size {
        let (r, g, b) = colors[(row + col) % colors.len()];
        let sprite = Sprite::solid_rect(80, 80, r, g, b, 255);
        let name = format!("tile_{}_{}", row, col);
        let handle = ctx.assets.create_sprite(sprite, &name);

        let entity = ctx
          .world
          .spawn((
            Transform::from_xy(
              start_x + col as f32 * spacing,
              start_y + row as f32 * spacing,
            ),
            Renderable::sprite(handle).with_z_order(0),
          ))
          .id();

        self.background_sprites.push(entity);
      }
    }

    // Create some rotating sprites in the center
    let center_colors = [
      (255, 0, 0), // Red
      (0, 255, 0), // Green
      (0, 0, 255), // Blue
    ];

    for (i, &(r, g, b)) in center_colors.iter().enumerate() {
      let sprite = Sprite::solid_rect(60, 60, r, g, b, 255);
      let name = format!("rotating_{}", i);
      let handle = ctx.assets.create_sprite(sprite, &name);

      let angle = (i as f32 / 3.0) * std::f32::consts::TAU;
      let radius = 150.0;

      let entity = ctx
        .world
        .spawn((
          Transform::from_xy(cx + angle.cos() * radius, cy + angle.sin() * radius),
          Renderable::sprite(handle).with_z_order(1),
        ))
        .id();

      self.rotating_sprites.push(entity);
    }
  }

  fn update_rotating_sprites(&mut self, ctx: &mut SceneContext) {
    let cx = self.screen_width / 2.0;
    let cy = self.screen_height / 2.0;
    for (i, &entity) in self.rotating_sprites.iter().enumerate() {
      if let Some(mut transform) = ctx.world.get_mut::<Transform>(entity) {
        let angle = self.time * 0.5 + (i as f32 / 3.0) * std::f32::consts::TAU;
        let radius = 150.0;
        transform.translation = Vec2::new(cx + angle.cos() * radius, cy + angle.sin() * radius);
        transform.rotation = self.time * 2.0;
      }
    }
  }

  fn current(&self) -> DemoEffect {
    DemoEffect::ALL[self.effect.min(DemoEffect::ALL.len() - 1)]
  }

  fn apply_current_effect(&self, ctx: &mut SceneContext) {
    let mut effects = ctx.effects();
    // Apply the selected effect
    // Note: Don't clear effects for ScreenFlash mode, as we want timed flashes to decay
    match self.current() {
      DemoEffect::None => {
        effects.clear();
      }
      DemoEffect::Fade => {
        effects.clear();
        effects.set_fade(self.fade_amount);
      }
      DemoEffect::ColorTint => {
        effects.clear();
        effects.set_sepia(self.sepia_intensity);
      }
      DemoEffect::Vignette => {
        effects.clear();
        effects.set_vignette_custom(
          VIGNETTE_INTENSITY,
          self.vignette_radius,
          self.vignette_softness,
        );
      }
      DemoEffect::ScreenFlash => {
        // Flash is triggered manually via Space
        // Don't clear or reapply - let timed flashes decay naturally
      }
      DemoEffect::Crt => {
        effects.clear();
        effects.enable_crt();
      }
      DemoEffect::ChromaticAberration => {
        effects.clear();
        effects.set_chromatic_aberration(self.chromatic_strength);
      }
      DemoEffect::CustomScanlines => {
        effects.clear();
        if let Some(shader) = &self.scanlines
          && effects.add_custom("scanlines", shader.clone()).is_ok()
        {
          // Registered just above, so the name is known
          effects.set_custom_param("scanlines", "density", 600.0);
          let _ = effects.set_custom_param("scanlines", "strength", self.scanline_strength);
        }
      }
    }
  }
}

impl DemoPage for PostProcessingPage {
  fn name(&self) -> &str {
    "Effects"
  }

  fn description(&self) -> &str {
    if self.current() == DemoEffect::CustomScanlines && self.scanlines.is_none() {
      return "scanlines.wgsl failed to load (see console).";
    }
    self.current().description()
  }

  fn settings(&mut self, s: &mut Settings) {
    s.choice("Effect", &mut self.effect, &DemoEffect::NAMES);
    match self.current() {
      DemoEffect::Fade => s.float("Fade amount", &mut self.fade_amount, 0.0..=1.0, 0.05),
      DemoEffect::ColorTint => s.float(
        "Sepia intensity",
        &mut self.sepia_intensity,
        0.0..=1.0,
        0.05,
      ),
      DemoEffect::Vignette => {
        s.float(
          "Vignette radius",
          &mut self.vignette_radius,
          0.0..=1.0,
          0.05,
        );
        s.float(
          "Vignette softness",
          &mut self.vignette_softness,
          0.0..=1.0,
          0.05,
        );
      }
      DemoEffect::ChromaticAberration => s.float(
        "Chromatic strength",
        &mut self.chromatic_strength,
        0.0..=0.02,
        0.001,
      ),
      DemoEffect::CustomScanlines => s.float(
        "Scanline strength",
        &mut self.scanline_strength,
        0.0..=1.0,
        0.05,
      ),
      DemoEffect::None | DemoEffect::ScreenFlash | DemoEffect::Crt => {}
    }
  }

  fn controls(&self) -> Vec<Control> {
    vec![Control::key(KeyCode::Space, "Flash")]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    let (width, height) = ctx.screen_size();
    self.screen_width = width;
    self.screen_height = height;

    // A missing shader only disables the scanlines effect; the rest still works.
    match ctx.assets.load_shader("assets/shaders/scanlines.wgsl") {
      Ok(shader) => self.scanlines = Some(shader),
      Err(e) => eprintln!("Could not load scanlines.wgsl: {e}"),
    }

    self.create_colorful_scene(ctx);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    self.time += dt;

    // Leaving an effect behind: start clean so timed flashes can decay
    if self.effect != self.last_effect {
      ctx.effects().clear();
      self.last_effect = self.effect;
    }

    self.update_rotating_sprites(ctx);
    self.apply_current_effect(ctx);

    // Space triggers screen flash
    if ctx.input.key_pressed(KeyCode::Space) {
      ctx
        .effects()
        .flash(Color::WHITE, std::time::Duration::from_millis(300));
    }
  }
}

fn main() {
  Demo::new("Post-Processing")
    .resolution(800, 600)
    .configure(|b| b.window_mode(WindowMode::ResizableAspectRatio))
    .page(PostProcessingPage::new)
    .run();
}
