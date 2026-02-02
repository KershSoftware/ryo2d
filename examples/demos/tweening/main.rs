//! Tweening Demo - Ryo2D Example
//!
//! This example demonstrates the Tween system for smooth interpolation:
//! - Various easing functions (Linear, Quad, Cubic, Back, Bounce, Elastic, etc.)
//! - Position, rotation, and scale tweening
//! - Loop modes (Once, Loop, PingPong)
//!
//! ## Controls
//!
//! - Space: Restart all tweens
//! - 1-4: Change loop mode (Once, Loop, PingPong, Times(3))
//! - Escape: Exit

use ryo2d::prelude::*;
use std::f32::consts::PI;

/// Marker for position tween demos
#[derive(Debug, Clone)]
struct PositionDemo {
  #[allow(dead_code)]
  ease_name: &'static str,
  ease: Ease,
  start_x: f32,
}

/// Marker for the rotation demo
#[derive(Debug)]
struct RotationDemo;

/// Marker for the scale demo
#[derive(Debug)]
struct ScaleDemo;

/// Main demo scene
struct TweeningDemoScene {
  position_entities: Vec<Entity>,
  rotation_entity: Option<Entity>,
  scale_entity: Option<Entity>,
  loop_mode: TweenLoop,
  screen_w: f32,
  screen_h: f32,
}

impl TweeningDemoScene {
  fn new() -> Self {
    Self {
      position_entities: Vec::new(),
      rotation_entity: None,
      scale_entity: None,
      loop_mode: TweenLoop::Once,
      screen_w: 1024.0,
      screen_h: 768.0,
    }
  }

  /// Create a colored square sprite
  fn create_square(r: u8, g: u8, b: u8) -> SpriteHandle {
    let sprite = Sprite::solid_rect(24, 24, r, g, b, 255);
    SpriteHandle::from_sprite(sprite, format!("square_{}_{}_{}", r, g, b))
  }

  /// Restart all tweens with current loop mode
  fn restart_tweens(&self, world: &mut World) {
    // Restart position tweens
    for &entity in &self.position_entities {
      // Get the demo info
      let demo_info = world.get::<PositionDemo>(entity).map(|d| (d.start_x, d.ease));

      if let Some((start_x, _ease)) = demo_info {
        // Reset transform position
        if let Some(mut transform) = world.get_mut::<Transform>(entity) {
          transform.x = start_x;
        }

        // Restart the tween (all entities are spawned with tweens)
        if let Some(mut tween) = world.get_mut::<Tween>(entity) {
          tween.restart();
        }
      }
    }

    // Restart rotation tween
    if let Some(entity) = self.rotation_entity {
      if let Some(mut transform) = world.get_mut::<Transform>(entity) {
        transform.rotation = 0.0;
      }
      if let Some(mut tween) = world.get_mut::<Tween>(entity) {
        tween.restart();
      }
    }

    // Restart scale tween
    if let Some(entity) = self.scale_entity {
      if let Some(mut transform) = world.get_mut::<Transform>(entity) {
        transform.scale_x = 1.0;
        transform.scale_y = 1.0;
      }
      if let Some(mut tween) = world.get_mut::<Tween>(entity) {
        tween.restart();
      }
    }
  }

  fn loop_mode_name(&self) -> &'static str {
    match self.loop_mode {
      TweenLoop::Once => "Once",
      TweenLoop::Loop => "Loop",
      TweenLoop::PingPong => "PingPong",
      TweenLoop::Times(_) => "Times(3)",
    }
  }

  /// Respawn all entities with new loop mode
  fn respawn_with_loop_mode(&mut self, ctx: &mut SceneContext) {
    // Despawn all existing entities
    for entity in self.position_entities.drain(..) {
      let _ = ctx.world.despawn(entity);
    }
    if let Some(entity) = self.rotation_entity.take() {
      let _ = ctx.world.despawn(entity);
    }
    if let Some(entity) = self.scale_entity.take() {
      let _ = ctx.world.despawn(entity);
    }

    // Respawn with new loop mode
    self.spawn_demos(ctx);
  }

  fn spawn_demos(&mut self, ctx: &mut SceneContext) {
    let screen_w = self.screen_w;
    let screen_h = self.screen_h;

    // Easing functions to demonstrate
    let easings: Vec<(&str, Ease, (u8, u8, u8))> = vec![
      ("Linear", Ease::Linear, (200, 200, 200)),
      ("InQuad", Ease::InQuad, (255, 100, 100)),
      ("OutQuad", Ease::OutQuad, (100, 255, 100)),
      ("InOutQuad", Ease::InOutQuad, (100, 100, 255)),
      ("InCubic", Ease::InCubic, (255, 150, 50)),
      ("OutCubic", Ease::OutCubic, (50, 255, 150)),
      ("InOutCubic", Ease::InOutCubic, (150, 50, 255)),
      ("OutBack", Ease::OutBack, (255, 255, 100)),
      ("OutBounce", Ease::OutBounce, (100, 255, 255)),
      ("OutElastic", Ease::OutElastic, (255, 100, 255)),
    ];

    let start_x = 100.0;
    let target_x = screen_w - 100.0;
    let row_height = 50.0;
    let start_y = 80.0;

    // Create position tween demos
    for (i, (name, ease, (r, g, b))) in easings.iter().enumerate() {
      let y = start_y + i as f32 * row_height;
      let sprite = Self::create_square(*r, *g, *b);

      let entity = ctx.world.spawn((
        PositionDemo {
          ease_name: name,
          ease: *ease,
          start_x,
        },
        Transform::new(start_x, y).with_pivot_center(),
        Renderable::sprite(sprite).with_z_order(10),
        Tween::new(2.0)
          .to_x(target_x)
          .with_easing(*ease)
          .with_loop(self.loop_mode),
      ));

      self.position_entities.push(entity);
    }

    // Create rotation demo (bottom left area)
    let rotation_sprite = Self::create_square(255, 200, 100);
    let rotation_y = screen_h - 120.0;
    let rotation_x = 200.0;

    self.rotation_entity = Some(ctx.world.spawn((
      RotationDemo,
      Transform::new(rotation_x, rotation_y).with_pivot_center(),
      Renderable::sprite(rotation_sprite).with_z_order(10),
      Tween::to_rotation(PI * 2.0, 2.0)
        .with_easing(Ease::InOutCubic)
        .with_loop(self.loop_mode),
    )));

    // Create scale demo (bottom center area)
    let scale_sprite = Self::create_square(100, 200, 255);
    let scale_x = screen_w / 2.0;
    let scale_y = screen_h - 120.0;

    self.scale_entity = Some(ctx.world.spawn((
      ScaleDemo,
      Transform::new(scale_x, scale_y).with_pivot_center(),
      Renderable::sprite(scale_sprite).with_z_order(10),
      Tween::to_scale(2.0, 2.0, 1.0)
        .with_easing(Ease::OutElastic)
        .with_loop(self.loop_mode),
    )));
  }
}

impl Scene for TweeningDemoScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    println!("Tweening Demo initialized!");
    println!("  Space: Restart tweens");
    println!("  1: Once mode");
    println!("  2: Loop mode");
    println!("  3: PingPong mode");
    println!("  4: Times(3) mode");
    println!("  ESC: Exit");

    let (screen_w, screen_h) = ctx.screen_size();
    self.screen_w = screen_w;
    self.screen_h = screen_h;

    self.spawn_demos(ctx);
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    // Exit on escape
    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Quit;
    }

    // Restart tweens
    if ctx.input.key_pressed(KeyCode::Space) {
      self.restart_tweens(ctx.world);
    }

    // Change loop mode
    let mut mode_changed = false;
    if ctx.input.key_pressed(KeyCode::Digit1) {
      self.loop_mode = TweenLoop::Once;
      mode_changed = true;
    }
    if ctx.input.key_pressed(KeyCode::Digit2) {
      self.loop_mode = TweenLoop::Loop;
      mode_changed = true;
    }
    if ctx.input.key_pressed(KeyCode::Digit3) {
      self.loop_mode = TweenLoop::PingPong;
      mode_changed = true;
    }
    if ctx.input.key_pressed(KeyCode::Digit4) {
      self.loop_mode = TweenLoop::Times(3);
      mode_changed = true;
    }

    if mode_changed {
      self.respawn_with_loop_mode(ctx);
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let (screen_w, screen_h) = ui.screen_size();
    let screen_w = screen_w as f32;
    let screen_h = screen_h as f32;

    // Dark colors for readability on warm cream background
    let text_dark = Color::rgb(40, 40, 50);
    let text_accent = Color::rgb(60, 60, 180);
    let text_muted = Color::rgb(100, 100, 110);

    // Title
    ui.draw_text("Tweening Demo", 10.0, 10.0, text_dark);

    // Current loop mode
    let mode_text = format!("Loop Mode: {} (1-4 to change)", self.loop_mode_name());
    ui.draw_text(&mode_text, 10.0, 30.0, text_accent);

    // Controls
    ui.draw_text("Space: Restart | ESC: Exit", 10.0, 50.0, text_muted);

    // Easing labels (right side)
    let easings = [
      "Linear",
      "InQuad",
      "OutQuad",
      "InOutQuad",
      "InCubic",
      "OutCubic",
      "InOutCubic",
      "OutBack",
      "OutBounce",
      "OutElastic",
    ];
    let row_height = 50.0;
    let start_y = 80.0;

    for (i, name) in easings.iter().enumerate() {
      let y = start_y + i as f32 * row_height;
      ui.draw_text(name, screen_w - 90.0, y - 5.0, text_dark);
    }

    // Bottom section labels
    let bottom_y = screen_h - 80.0;
    ui.draw_text(
      "Rotation (InOutCubic)",
      130.0,
      bottom_y,
      Color::rgb(180, 100, 40),
    );
    ui.draw_text(
      "Scale (OutElastic)",
      screen_w / 2.0 - 60.0,
      bottom_y,
      Color::rgb(40, 100, 180),
    );
  }
}

fn main() {
  let engine = EngineBuilder::new()
    .title("Tweening Demo - Ryo2D")
    .window_mode(WindowMode::ResizableAspectRatio)
    .fps(60)
    .physics_hz(60)
    .with_scene(TweeningDemoScene::new())
    .build();

  if let Err(e) = engine.run() {
    eprintln!("Engine error: {}", e);
  }
}
