use ryo2d::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq)]
enum DemoEffect {
  None,
  Fade,
  ColorTint,
  Vignette,
  ScreenFlash,
  Crt,
  ChromaticAberration,
}

impl DemoEffect {
  fn name(&self) -> &'static str {
    match self {
      DemoEffect::None => "None",
      DemoEffect::Fade => "Fade",
      DemoEffect::ColorTint => "Color Tint (Sepia)",
      DemoEffect::Vignette => "Vignette",
      DemoEffect::ScreenFlash => "Screen Flash",
      DemoEffect::Crt => "CRT",
      DemoEffect::ChromaticAberration => "Chromatic Aberration",
    }
  }

  fn description(&self) -> &'static str {
    match self {
      DemoEffect::None => "No post-processing effects",
      DemoEffect::Fade => "Alpha fade for scene transitions\nUp/Down: Adjust fade amount",
      DemoEffect::ColorTint => "Sepia color grading\nUp/Down: Adjust sepia intensity",
      DemoEffect::Vignette => "Edge darkening\nUp/Down: Radius  Left/Right: Softness",
      DemoEffect::ScreenFlash => "Impact flash effect\nSpace: Trigger flash",
      DemoEffect::Crt => "Retro CRT scanlines & distortion\nAlways-on effect",
      DemoEffect::ChromaticAberration => "RGB channel offset\nUp/Down: Adjust offset strength",
    }
  }
}

struct PostProcessingDemo {
  // Scene objects
  background_sprites: Vec<Entity>,
  rotating_sprites: Vec<Entity>,

  // Effect state
  current_effect: DemoEffect,

  // Effect parameters (adjustable via keyboard)
  fade_amount: f32,
  sepia_intensity: f32,
  vignette_radius: f32,
  vignette_softness: f32,
  chromatic_strength: f32,

  // Animation
  time: f32,
}

impl PostProcessingDemo {
  fn new() -> Self {
    Self {
      background_sprites: Vec::new(),
      rotating_sprites: Vec::new(),
      current_effect: DemoEffect::None,
      fade_amount: 0.5,
      sepia_intensity: 0.8,
      vignette_radius: 0.7,
      vignette_softness: 0.5,
      chromatic_strength: 0.005,
      time: 0.0,
    }
  }

  fn create_colorful_scene(&mut self, ctx: &mut GameContext) {
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
    let start_x = 200.0;
    let start_y = 100.0;

    for row in 0..grid_size {
      for col in 0..grid_size {
        let (r, g, b) = colors[(row + col) % colors.len()];
        let sprite = Sprite::solid_rect(80, 80, r, g, b, 255);
        let name = format!("tile_{}_{}", row, col);
        let handle = ctx.assets.create_sprite(sprite, &name);

        let entity = ctx.world.spawn((
          Transform::new(
            start_x + col as f32 * spacing,
            start_y + row as f32 * spacing,
          ),
          Renderable::sprite(handle).with_z_order(0),
        ));

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

      let entity = ctx.world.spawn((
        Transform::new(400.0 + angle.cos() * radius, 300.0 + angle.sin() * radius),
        Renderable::sprite(handle).with_z_order(1),
      ));

      self.rotating_sprites.push(entity);
    }
  }

  fn update_rotating_sprites(&mut self, ctx: &mut GameContext) {
    for (i, &entity) in self.rotating_sprites.iter().enumerate() {
      if let Some(mut transform) = ctx.world.get_component_mut::<Transform>(entity) {
        let angle = self.time * 0.5 + (i as f32 / 3.0) * std::f32::consts::TAU;
        let radius = 150.0;
        transform.x = 400.0 + angle.cos() * radius;
        transform.y = 300.0 + angle.sin() * radius;
        transform.rotation = self.time * 2.0;
      }
    }
  }

  fn apply_current_effect(&self, ctx: &mut GameContext) {
    // Apply the selected effect
    // Note: Don't clear effects for ScreenFlash mode, as we want timed flashes to decay
    match self.current_effect {
      DemoEffect::None => {
        ctx.effects.clear();
      }
      DemoEffect::Fade => {
        ctx.effects.clear();
        ctx.effects.set_fade(self.fade_amount);
      }
      DemoEffect::ColorTint => {
        ctx.effects.clear();
        ctx.effects.set_sepia(self.sepia_intensity);
      }
      DemoEffect::Vignette => {
        ctx.effects.clear();
        ctx.effects.set_vignette_custom(
          self.vignette_radius,
          self.vignette_radius,
          self.vignette_softness,
        );
      }
      DemoEffect::ScreenFlash => {
        // Flash is triggered manually via Space key
        // Don't clear or reapply - let timed flashes decay naturally
      }
      DemoEffect::Crt => {
        ctx.effects.clear();
        ctx.effects.enable_crt();
      }
      DemoEffect::ChromaticAberration => {
        ctx.effects.clear();
        ctx
          .effects
          .set_chromatic_aberration(self.chromatic_strength);
      }
    }
  }

  fn handle_input(&mut self, ctx: &mut GameContext) {
    // Switch effects with number keys
    if ctx.input.key_pressed(KeyCode::Digit1) {
      self.current_effect = DemoEffect::None;
      ctx.effects.clear(); // Clear when switching to None
    } else if ctx.input.key_pressed(KeyCode::Digit2) {
      self.current_effect = DemoEffect::Fade;
    } else if ctx.input.key_pressed(KeyCode::Digit3) {
      self.current_effect = DemoEffect::ColorTint;
    } else if ctx.input.key_pressed(KeyCode::Digit4) {
      self.current_effect = DemoEffect::Vignette;
    } else if ctx.input.key_pressed(KeyCode::Digit5) {
      self.current_effect = DemoEffect::ScreenFlash;
      ctx.effects.clear(); // Clear when switching to ScreenFlash mode
    } else if ctx.input.key_pressed(KeyCode::Digit6) {
      self.current_effect = DemoEffect::Crt;
    } else if ctx.input.key_pressed(KeyCode::Digit7) {
      self.current_effect = DemoEffect::ChromaticAberration;
    }

    // Adjust parameters with arrow keys
    let adjust_speed = 0.01;

    match self.current_effect {
      DemoEffect::Fade => {
        if ctx.input.key_down(KeyCode::ArrowUp) {
          self.fade_amount = (self.fade_amount + adjust_speed).min(1.0);
        }
        if ctx.input.key_down(KeyCode::ArrowDown) {
          self.fade_amount = (self.fade_amount - adjust_speed).max(0.0);
        }
      }
      DemoEffect::ColorTint => {
        if ctx.input.key_down(KeyCode::ArrowUp) {
          self.sepia_intensity = (self.sepia_intensity + adjust_speed).min(1.0);
        }
        if ctx.input.key_down(KeyCode::ArrowDown) {
          self.sepia_intensity = (self.sepia_intensity - adjust_speed).max(0.0);
        }
      }
      DemoEffect::Vignette => {
        if ctx.input.key_down(KeyCode::ArrowUp) {
          self.vignette_radius = (self.vignette_radius + adjust_speed).min(1.0);
        }
        if ctx.input.key_down(KeyCode::ArrowDown) {
          self.vignette_radius = (self.vignette_radius - adjust_speed).max(0.0);
        }
        if ctx.input.key_down(KeyCode::ArrowLeft) {
          self.vignette_softness = (self.vignette_softness - adjust_speed).max(0.0);
        }
        if ctx.input.key_down(KeyCode::ArrowRight) {
          self.vignette_softness = (self.vignette_softness + adjust_speed).min(1.0);
        }
      }
      DemoEffect::ChromaticAberration => {
        if ctx.input.key_down(KeyCode::ArrowUp) {
          self.chromatic_strength = (self.chromatic_strength + 0.0002).min(0.02);
        }
        if ctx.input.key_down(KeyCode::ArrowDown) {
          self.chromatic_strength = (self.chromatic_strength - 0.0002).max(0.0);
        }
      }
      _ => {}
    }

    // Space triggers screen flash
    if ctx.input.key_pressed(KeyCode::Space) {
      ctx
        .effects
        .flash(Color::WHITE, std::time::Duration::from_millis(300));
    }
  }
}

impl Game for PostProcessingDemo {
  fn init(&mut self, ctx: &mut GameContext) {
    println!("=== Post-Processing Effects Demo ===");
    println!("Press 1-7 to switch effects");
    println!("Use arrow keys to adjust parameters");
    println!("Press Space for screen flash");
    println!("Press ESC to quit");
    println!();

    self.create_colorful_scene(ctx);
  }

  fn update(&mut self, ctx: &mut GameContext, dt: f32) {
    self.time += dt;

    // Handle input
    self.handle_input(ctx);

    // Update animations
    self.update_rotating_sprites(ctx);

    // Apply current effect
    self.apply_current_effect(ctx);

    // Exit on ESC
    if ctx.input.key_pressed(KeyCode::Escape) {
      std::process::exit(0);
    }
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    // Title
    ui.draw_text("POST-PROCESSING EFFECTS DEMO", 10.0, 10.0, Color::WHITE);

    // Current effect info
    let y_offset = 40.0;
    ui.draw_text(
      &format!("Current Effect: {}", self.current_effect.name()),
      10.0,
      y_offset,
      Color::rgb(100, 255, 100),
    );

    // Description
    let desc_y = y_offset + 25.0;
    for (i, line) in self.current_effect.description().lines().enumerate() {
      ui.draw_text(
        line,
        10.0,
        desc_y + i as f32 * 15.0,
        Color::rgb(200, 200, 200),
      );
    }

    // Current parameters
    let params_y = 130.0;
    match self.current_effect {
      DemoEffect::Fade => {
        ui.draw_text(
          &format!("Fade Amount: {:.2}", self.fade_amount),
          10.0,
          params_y,
          Color::rgb(255, 255, 100),
        );
      }
      DemoEffect::ColorTint => {
        ui.draw_text(
          &format!("Sepia Intensity: {:.2}", self.sepia_intensity),
          10.0,
          params_y,
          Color::rgb(255, 255, 100),
        );
      }
      DemoEffect::Vignette => {
        ui.draw_text(
          &format!(
            "Radius: {:.2}  Softness: {:.2}",
            self.vignette_radius, self.vignette_softness
          ),
          10.0,
          params_y,
          Color::rgb(255, 255, 100),
        );
      }
      DemoEffect::ChromaticAberration => {
        ui.draw_text(
          &format!("Strength: {:.4}", self.chromatic_strength),
          10.0,
          params_y,
          Color::rgb(255, 255, 100),
        );
      }
      _ => {}
    }

    // Controls reminder (positioned to avoid cutoff at 600px height)
    let controls_y = 510.0;
    ui.draw_text("Controls:", 10.0, controls_y, Color::WHITE);
    ui.draw_text(
      "1-7: Switch effects",
      10.0,
      controls_y + 15.0,
      Color::rgb(180, 180, 180),
    );
    ui.draw_text(
      "Arrows: Adjust parameters",
      10.0,
      controls_y + 30.0,
      Color::rgb(180, 180, 180),
    );
    ui.draw_text(
      "Space: Flash",
      10.0,
      controls_y + 45.0,
      Color::rgb(180, 180, 180),
    );
    ui.draw_text(
      "ESC: Quit",
      10.0,
      controls_y + 60.0,
      Color::rgb(180, 180, 180),
    );
  }
}

fn main() {
  EngineBuilder::new()
    .title("Post-Processing Effects Demo")
    .resolution(800, 600)
    .window_mode(WindowMode::ResizableAspectRatio)
    .with_game(PostProcessingDemo::new())
    .build()
    .run()
    .expect("Failed to run engine");
}
