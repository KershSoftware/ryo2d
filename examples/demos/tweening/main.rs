//! Tweening Demo - Ryo2D Example
//!
//! This example demonstrates the Tween system for smooth interpolation:
//!
//! ## Scenes (press number keys to switch)
//!
//! 1. **Easing Functions** - Various easing curves with loop mode controls
//! 2. **Speed Control** - Playback speed manipulation (0.5x, 1x, 2x, pause)
//! 3. **Callbacks** - on_start, on_update, on_complete callbacks with visual feedback
//! 4. **Combined Effects** - Position + scale + rotation combinations
//! 5. **Sequences** - TweenSequence for chained animations
//! 6. **Color & Alpha** - Alpha fade, tint colors, and combined color effects
//!
//! ## Controls
//!
//! - 1-6: Switch scenes
//! - Space: Restart tweens (scene-specific)
//! - Escape: Exit

use ryo2d::prelude::*;
use std::f32::consts::PI;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

// ============================================================================
// Scene 1: Easing Functions
// ============================================================================

#[derive(Debug, Clone)]
struct PositionDemo;

#[derive(Debug)]
struct RotationDemo;

#[derive(Debug)]
struct ScaleDemo;

struct EasingScene {
  position_entities: Vec<Entity>,
  rotation_entity: Option<Entity>,
  scale_entity: Option<Entity>,
  loop_mode: TweenLoop,
  screen_w: f32,
  screen_h: f32,
}

impl EasingScene {
  fn new() -> Self {
    Self {
      position_entities: Vec::new(),
      rotation_entity: None,
      scale_entity: None,
      loop_mode: TweenLoop::PingPong,
      screen_w: 1024.0,
      screen_h: 768.0,
    }
  }

  fn create_square(r: u8, g: u8, b: u8) -> SpriteHandle {
    let sprite = Sprite::solid_rect(24, 24, r, g, b, 255);
    SpriteHandle::from_sprite(sprite, format!("square_{}_{}_{}", r, g, b))
  }

  fn loop_mode_name(&self) -> &'static str {
    match self.loop_mode {
      TweenLoop::Once => "Once",
      TweenLoop::Loop => "Loop",
      TweenLoop::PingPong => "PingPong",
      TweenLoop::Times(_) => "Times(3)",
    }
  }

  fn respawn_with_loop_mode(&mut self, ctx: &mut SceneContext) {
    self.cleanup(ctx);
    self.spawn_demos(ctx);
  }

  fn spawn_demos(&mut self, ctx: &mut SceneContext) {
    let screen_w = self.screen_w;
    let screen_h = self.screen_h;

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

    for (i, (_name, ease, (r, g, b))) in easings.iter().enumerate() {
      let y = start_y + i as f32 * row_height;
      let sprite = Self::create_square(*r, *g, *b);

      let entity = ctx.world.spawn((
        PositionDemo,
        Transform::new(start_x, y).with_pivot_center(),
        Renderable::sprite(sprite).with_z_order(10),
        Tween::new(2.0)
          .to_x(target_x)
          .with_easing(*ease)
          .with_loop(self.loop_mode),
      ));

      self.position_entities.push(entity);
    }

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

  fn cleanup(&mut self, ctx: &mut SceneContext) {
    for entity in self.position_entities.drain(..) {
      let _ = ctx.world.despawn(entity);
    }
    if let Some(entity) = self.rotation_entity.take() {
      let _ = ctx.world.despawn(entity);
    }
    if let Some(entity) = self.scale_entity.take() {
      let _ = ctx.world.despawn(entity);
    }
  }
}

impl Scene for EasingScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let (screen_w, screen_h) = ctx.screen_size();
    self.screen_w = screen_w;
    self.screen_h = screen_h;
    self.spawn_demos(ctx);
  }

  fn on_exit(&mut self, ctx: &mut SceneContext) {
    self.cleanup(ctx);
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Quit;
    }

    // Scene switching (number keys)
    if ctx.input.key_pressed(KeyCode::Digit2) {
      return SceneAction::Switch(Box::new(SpeedControlScene::new()));
    }
    if ctx.input.key_pressed(KeyCode::Digit3) {
      return SceneAction::Switch(Box::new(CallbacksScene::new()));
    }
    if ctx.input.key_pressed(KeyCode::Digit4) {
      return SceneAction::Switch(Box::new(CombinedEffectsScene::new()));
    }
    if ctx.input.key_pressed(KeyCode::Digit5) {
      return SceneAction::Switch(Box::new(SequenceScene::new()));
    }
    if ctx.input.key_pressed(KeyCode::Digit6) {
      return SceneAction::Switch(Box::new(ColorAlphaScene::new()));
    }

    // Loop mode controls (Q/W/E/R)
    let mut mode_changed = false;
    if ctx.input.key_pressed(KeyCode::KeyQ) {
      self.loop_mode = TweenLoop::Once;
      mode_changed = true;
    }
    if ctx.input.key_pressed(KeyCode::KeyW) {
      self.loop_mode = TweenLoop::Loop;
      mode_changed = true;
    }
    if ctx.input.key_pressed(KeyCode::KeyE) {
      self.loop_mode = TweenLoop::PingPong;
      mode_changed = true;
    }
    if ctx.input.key_pressed(KeyCode::KeyR) {
      self.loop_mode = TweenLoop::Times(3);
      mode_changed = true;
    }

    if mode_changed {
      self.respawn_with_loop_mode(ctx);
    }

    // Restart with Space
    if ctx.input.key_pressed(KeyCode::Space) {
      self.respawn_with_loop_mode(ctx);
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let (screen_w, screen_h) = ui.screen_size();
    let screen_w = screen_w as f32;
    let screen_h = screen_h as f32;

    let text_light = Color::rgb(220, 220, 220);
    let text_accent = Color::rgb(130, 180, 255);
    let text_muted = Color::rgb(160, 160, 160);

    ui.draw_text("[1] Easing Functions", 10.0, 10.0, text_accent);

    let mode_text = format!("Loop: {} (Q/W/E/R to change)", self.loop_mode_name());
    ui.draw_text(&mode_text, 10.0, 30.0, text_light);

    ui.draw_text("2-6: Scenes | Space: Restart | ESC: Exit", 10.0, 50.0, text_muted);

    let easings = [
      "Linear", "InQuad", "OutQuad", "InOutQuad", "InCubic",
      "OutCubic", "InOutCubic", "OutBack", "OutBounce", "OutElastic",
    ];
    let row_height = 50.0;
    let start_y = 80.0;

    for (i, name) in easings.iter().enumerate() {
      let y = start_y + i as f32 * row_height;
      ui.draw_text(name, screen_w - 90.0, y - 5.0, text_light);
    }

    let bottom_y = screen_h - 80.0;
    ui.draw_text("Rotation", 160.0, bottom_y, Color::rgb(255, 180, 100));
    ui.draw_text("Scale", screen_w / 2.0 - 20.0, bottom_y, Color::rgb(100, 180, 255));
  }
}

// ============================================================================
// Scene 2: Speed Control
// ============================================================================

struct SpeedControlScene {
  entity: Option<Entity>,
  speed: f32,
  paused: bool,
  screen_w: f32,
  screen_h: f32,
}

impl SpeedControlScene {
  fn new() -> Self {
    Self {
      entity: None,
      speed: 1.0,
      paused: false,
      screen_w: 1024.0,
      screen_h: 768.0,
    }
  }

  fn apply_speed(&self, ctx: &mut SceneContext) {
    if let Some(entity) = self.entity {
      if let Some(mut tween) = ctx.world.get_mut::<Tween>(entity) {
        if self.paused {
          tween.pause();
        } else {
          tween.resume();
          tween.set_speed(self.speed);
        }
      }
    }
  }
}

impl Scene for SpeedControlScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let (screen_w, screen_h) = ctx.screen_size();
    self.screen_w = screen_w;
    self.screen_h = screen_h;

    let sprite = Sprite::solid_circle(30, 100, 200, 255, 255);
    let handle = SpriteHandle::from_sprite(sprite, "speed_ball");

    let center_y = screen_h / 2.0;

    self.entity = Some(ctx.world.spawn((
      Transform::new(100.0, center_y).with_pivot_center(),
      Renderable::sprite(handle).with_z_order(10),
      Tween::to_position(screen_w - 100.0, center_y, 3.0)
        .with_easing(Ease::InOutCubic)
        .with_loop(TweenLoop::PingPong)
        .with_speed(self.speed),
    )));
  }

  fn on_exit(&mut self, ctx: &mut SceneContext) {
    if let Some(entity) = self.entity.take() {
      let _ = ctx.world.despawn(entity);
    }
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Quit;
    }

    // Scene switching
    if ctx.input.key_pressed(KeyCode::Digit1) {
      return SceneAction::Switch(Box::new(EasingScene::new()));
    }
    if ctx.input.key_pressed(KeyCode::Digit3) {
      return SceneAction::Switch(Box::new(CallbacksScene::new()));
    }
    if ctx.input.key_pressed(KeyCode::Digit4) {
      return SceneAction::Switch(Box::new(CombinedEffectsScene::new()));
    }
    if ctx.input.key_pressed(KeyCode::Digit5) {
      return SceneAction::Switch(Box::new(SequenceScene::new()));
    }
    if ctx.input.key_pressed(KeyCode::Digit6) {
      return SceneAction::Switch(Box::new(ColorAlphaScene::new()));
    }

    // Speed control
    let mut changed = false;

    if ctx.input.key_pressed(KeyCode::KeyQ) {
      self.speed = 0.25;
      self.paused = false;
      changed = true;
    }
    if ctx.input.key_pressed(KeyCode::KeyW) {
      self.speed = 0.5;
      self.paused = false;
      changed = true;
    }
    if ctx.input.key_pressed(KeyCode::KeyE) {
      self.speed = 1.0;
      self.paused = false;
      changed = true;
    }
    if ctx.input.key_pressed(KeyCode::KeyR) {
      self.speed = 2.0;
      self.paused = false;
      changed = true;
    }
    if ctx.input.key_pressed(KeyCode::KeyT) {
      self.speed = 4.0;
      self.paused = false;
      changed = true;
    }

    // Space toggles pause
    if ctx.input.key_pressed(KeyCode::Space) {
      self.paused = !self.paused;
      changed = true;
    }

    if changed {
      self.apply_speed(ctx);
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let text_accent = Color::rgb(130, 180, 255);
    let text_muted = Color::rgb(160, 160, 160);
    let text_light = Color::rgb(220, 220, 220);

    ui.draw_text("[2] Speed Control", 10.0, 10.0, text_accent);
    ui.draw_text("1,3-6: Other scenes | ESC: Exit", 10.0, 30.0, text_muted);

    ui.draw_text("Speed: Q=0.25x W=0.5x E=1x R=2x T=4x", 10.0, 70.0, text_light);
    ui.draw_text("Space: Toggle Pause", 10.0, 90.0, text_light);

    let status = if self.paused {
      "PAUSED".to_string()
    } else {
      format!("{:.2}x speed", self.speed)
    };

    let status_color = if self.paused {
      Color::RED
    } else if self.speed < 1.0 {
      Color::rgb(255, 200, 100)
    } else if self.speed > 1.0 {
      Color::GREEN
    } else {
      text_light
    };

    ui.draw_text(&format!("Status: {}", status), 10.0, 130.0, status_color);
  }
}

// ============================================================================
// Scene 3: Callbacks
// ============================================================================

struct CallbacksScene {
  entity: Option<Entity>,
  start_count: Arc<AtomicUsize>,
  update_count: Arc<AtomicUsize>,
  complete_count: Arc<AtomicUsize>,
  screen_w: f32,
  screen_h: f32,
}

impl CallbacksScene {
  fn new() -> Self {
    Self {
      entity: None,
      start_count: Arc::new(AtomicUsize::new(0)),
      update_count: Arc::new(AtomicUsize::new(0)),
      complete_count: Arc::new(AtomicUsize::new(0)),
      screen_w: 1024.0,
      screen_h: 768.0,
    }
  }

  fn spawn_entity(&mut self, ctx: &mut SceneContext) {
    // Reset counters
    self.start_count.store(0, Ordering::SeqCst);
    self.update_count.store(0, Ordering::SeqCst);
    self.complete_count.store(0, Ordering::SeqCst);

    let sprite = Sprite::solid_rect(40, 40, 255, 150, 50, 255);
    let handle = SpriteHandle::from_sprite(sprite, "callback_box");

    let center_y = self.screen_h / 2.0;

    let start_clone = self.start_count.clone();
    let update_clone = self.update_count.clone();
    let complete_clone = self.complete_count.clone();

    self.entity = Some(ctx.world.spawn((
      Transform::new(100.0, center_y).with_pivot_center(),
      Renderable::sprite(handle).with_z_order(10),
      Tween::to_position(self.screen_w - 100.0, center_y, 2.0)
        .with_easing(Ease::OutCubic)
        .with_loop(TweenLoop::Times(3))
        .on_start(move || {
          start_clone.fetch_add(1, Ordering::SeqCst);
        })
        .on_update(move || {
          update_clone.fetch_add(1, Ordering::SeqCst);
        })
        .on_complete(move || {
          complete_clone.fetch_add(1, Ordering::SeqCst);
        }),
    )));
  }
}

impl Scene for CallbacksScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let (screen_w, screen_h) = ctx.screen_size();
    self.screen_w = screen_w;
    self.screen_h = screen_h;
    self.spawn_entity(ctx);
  }

  fn on_exit(&mut self, ctx: &mut SceneContext) {
    if let Some(entity) = self.entity.take() {
      let _ = ctx.world.despawn(entity);
    }
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Quit;
    }

    // Scene switching
    if ctx.input.key_pressed(KeyCode::Digit1) {
      return SceneAction::Switch(Box::new(EasingScene::new()));
    }
    if ctx.input.key_pressed(KeyCode::Digit2) {
      return SceneAction::Switch(Box::new(SpeedControlScene::new()));
    }
    if ctx.input.key_pressed(KeyCode::Digit4) {
      return SceneAction::Switch(Box::new(CombinedEffectsScene::new()));
    }
    if ctx.input.key_pressed(KeyCode::Digit5) {
      return SceneAction::Switch(Box::new(SequenceScene::new()));
    }
    if ctx.input.key_pressed(KeyCode::Digit6) {
      return SceneAction::Switch(Box::new(ColorAlphaScene::new()));
    }

    // Restart
    if ctx.input.key_pressed(KeyCode::Space) {
      if let Some(entity) = self.entity.take() {
        let _ = ctx.world.despawn(entity);
      }
      self.spawn_entity(ctx);
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let text_accent = Color::rgb(130, 180, 255);
    let text_muted = Color::rgb(160, 160, 160);
    let text_light = Color::rgb(220, 220, 220);

    ui.draw_text("[3] Callbacks", 10.0, 10.0, text_accent);
    ui.draw_text("1-2,4-6: Other scenes | Space: Restart | ESC: Exit", 10.0, 30.0, text_muted);

    ui.draw_text("Tween loops 3 times with callbacks:", 10.0, 70.0, text_light);

    let start_count = self.start_count.load(Ordering::SeqCst);
    let update_count = self.update_count.load(Ordering::SeqCst);
    let complete_count = self.complete_count.load(Ordering::SeqCst);

    let start_text = format!("on_start called: {} time(s)", start_count);
    ui.draw_text(&start_text, 10.0, 100.0, Color::GREEN);

    let update_text = format!("on_update called: {} times", update_count);
    ui.draw_text(&update_text, 10.0, 120.0, Color::rgb(255, 200, 100));

    let complete_text = format!("on_complete called: {} time(s)", complete_count);
    ui.draw_text(&complete_text, 10.0, 140.0, Color::rgb(255, 100, 100));

    ui.draw_text("Note: on_start fires once, on_complete fires each iteration", 10.0, 180.0, text_muted);
  }
}

// ============================================================================
// Scene 4: Combined Effects (position + scale + rotation)
// ============================================================================

#[derive(Debug)]
struct CombinedDemo;

struct CombinedEffectsScene {
  entities: Vec<Entity>,
  screen_w: f32,
  screen_h: f32,
}

impl CombinedEffectsScene {
  fn new() -> Self {
    Self {
      entities: Vec::new(),
      screen_w: 1024.0,
      screen_h: 768.0,
    }
  }

  fn spawn_demos(&mut self, ctx: &mut SceneContext) {
    let center_x = self.screen_w / 2.0;
    let center_y = self.screen_h / 2.0;

    // Demo 1: Position + Scale (bouncing ball effect)
    let sprite1 = Sprite::solid_circle(25, 100, 200, 255, 255);
    let handle1 = SpriteHandle::from_sprite(sprite1, "bounce_ball");

    self.entities.push(ctx.world.spawn((
      CombinedDemo,
      Transform::new(center_x - 200.0, center_y - 100.0).with_pivot_center(),
      Renderable::sprite(handle1).with_z_order(10),
      Tween::new(0.6)
        .to_y(center_y + 100.0)
        .to_scale_x(1.3)
        .to_scale_y(0.7)
        .with_easing(Ease::InQuad)
        .with_loop(TweenLoop::PingPong),
    )));

    // Demo 2: Position + Rotation (orbiting)
    let sprite2 = Sprite::solid_rect(30, 30, 255, 150, 50, 255);
    let handle2 = SpriteHandle::from_sprite(sprite2, "orbit_box");

    self.entities.push(ctx.world.spawn((
      CombinedDemo,
      Transform::new(center_x, center_y).with_pivot_center(),
      Renderable::sprite(handle2).with_z_order(10),
      TweenSequence::new()
        .then(
          Tween::new(1.0)
            .to_x(center_x + 100.0)
            .to_rotation_value(PI / 2.0)
            .with_easing(Ease::InOutSine),
        )
        .then(
          Tween::new(1.0)
            .to_y(center_y + 100.0)
            .to_rotation_value(PI)
            .with_easing(Ease::InOutSine),
        )
        .then(
          Tween::new(1.0)
            .to_x(center_x)
            .to_rotation_value(PI * 1.5)
            .with_easing(Ease::InOutSine),
        )
        .then(
          Tween::new(1.0)
            .to_y(center_y)
            .to_rotation_value(PI * 2.0)
            .with_easing(Ease::InOutSine),
        )
        .with_loop(),
    )));

    // Demo 3: Scale pulsing (heartbeat)
    let sprite3 = Sprite::solid_circle(20, 255, 80, 80, 255);
    let handle3 = SpriteHandle::from_sprite(sprite3, "heart");

    self.entities.push(ctx.world.spawn((
      CombinedDemo,
      Transform::new(center_x + 200.0, center_y).with_pivot_center(),
      Renderable::sprite(handle3).with_z_order(10),
      TweenSequence::new()
        .then(
          Tween::to_uniform_scale(1.3, 0.15)
            .with_easing(Ease::OutQuad),
        )
        .then(
          Tween::to_uniform_scale(1.0, 0.15)
            .with_easing(Ease::InQuad),
        )
        .then(
          Tween::to_uniform_scale(1.2, 0.1)
            .with_easing(Ease::OutQuad),
        )
        .then(
          Tween::to_uniform_scale(1.0, 0.3)
            .with_easing(Ease::InQuad),
        )
        .then(
          Tween::new(0.3), // Pause between beats
        )
        .with_loop(),
    )));
  }

  fn cleanup(&mut self, ctx: &mut SceneContext) {
    for entity in self.entities.drain(..) {
      let _ = ctx.world.despawn(entity);
    }
  }
}

impl Scene for CombinedEffectsScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let (screen_w, screen_h) = ctx.screen_size();
    self.screen_w = screen_w;
    self.screen_h = screen_h;
    self.spawn_demos(ctx);
  }

  fn on_exit(&mut self, ctx: &mut SceneContext) {
    self.cleanup(ctx);
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Quit;
    }

    // Scene switching
    if ctx.input.key_pressed(KeyCode::Digit1) {
      return SceneAction::Switch(Box::new(EasingScene::new()));
    }
    if ctx.input.key_pressed(KeyCode::Digit2) {
      return SceneAction::Switch(Box::new(SpeedControlScene::new()));
    }
    if ctx.input.key_pressed(KeyCode::Digit3) {
      return SceneAction::Switch(Box::new(CallbacksScene::new()));
    }
    if ctx.input.key_pressed(KeyCode::Digit5) {
      return SceneAction::Switch(Box::new(SequenceScene::new()));
    }
    if ctx.input.key_pressed(KeyCode::Digit6) {
      return SceneAction::Switch(Box::new(ColorAlphaScene::new()));
    }

    // Restart
    if ctx.input.key_pressed(KeyCode::Space) {
      self.cleanup(ctx);
      self.spawn_demos(ctx);
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let text_accent = Color::rgb(130, 180, 255);
    let text_muted = Color::rgb(160, 160, 160);
    let text_light = Color::rgb(220, 220, 220);

    ui.draw_text("[4] Combined Effects", 10.0, 10.0, text_accent);
    ui.draw_text("1-3,5-6: Other scenes | Space: Restart | ESC: Exit", 10.0, 30.0, text_muted);

    let (w, h) = ui.screen_size();
    let center_x = w as f32 / 2.0;
    let center_y = h as f32 / 2.0;

    ui.draw_text("Squash/Stretch", center_x - 240.0, center_y + 140.0, text_light);
    ui.draw_text("Position + Y", center_x - 240.0, center_y + 160.0, text_muted);

    ui.draw_text("Orbit + Rotate", center_x - 45.0, center_y + 140.0, text_light);
    ui.draw_text("Sequence", center_x - 30.0, center_y + 160.0, text_muted);

    ui.draw_text("Heartbeat", center_x + 170.0, center_y + 140.0, text_light);
    ui.draw_text("Scale Sequence", center_x + 150.0, center_y + 160.0, text_muted);
  }
}

// ============================================================================
// Scene 5: Tween Sequences
// ============================================================================

#[derive(Debug)]
struct SequenceDemo;

struct SequenceScene {
  entity: Option<Entity>,
  screen_w: f32,
  screen_h: f32,
}

impl SequenceScene {
  fn new() -> Self {
    Self {
      entity: None,
      screen_w: 1024.0,
      screen_h: 768.0,
    }
  }

  fn spawn_entity(&mut self, ctx: &mut SceneContext) {
    let sprite = Sprite::solid_rect(40, 40, 50, 200, 150, 255);
    let handle = SpriteHandle::from_sprite(sprite, "sequence_box");

    // Square path: start at top-left, go right, down, left, up
    let margin = 100.0;
    let right = self.screen_w - margin;
    let bottom = self.screen_h - margin;

    self.entity = Some(ctx.world.spawn((
      SequenceDemo,
      Transform::new(margin, margin).with_pivot_center(),
      Renderable::sprite(handle).with_z_order(10),
      TweenSequence::new()
        // Move right
        .then(
          Tween::to_position(right, margin, 1.0)
            .with_easing(Ease::InOutCubic),
        )
        // Move down
        .then(
          Tween::to_position(right, bottom, 1.0)
            .with_easing(Ease::InOutCubic),
        )
        // Move left
        .then(
          Tween::to_position(margin, bottom, 1.0)
            .with_easing(Ease::InOutCubic),
        )
        // Move up (back to start)
        .then(
          Tween::to_position(margin, margin, 1.0)
            .with_easing(Ease::InOutCubic),
        )
        .with_loop(),
    )));
  }
}

impl Scene for SequenceScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let (screen_w, screen_h) = ctx.screen_size();
    self.screen_w = screen_w;
    self.screen_h = screen_h;
    self.spawn_entity(ctx);
  }

  fn on_exit(&mut self, ctx: &mut SceneContext) {
    if let Some(entity) = self.entity.take() {
      let _ = ctx.world.despawn(entity);
    }
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Quit;
    }

    // Scene switching
    if ctx.input.key_pressed(KeyCode::Digit1) {
      return SceneAction::Switch(Box::new(EasingScene::new()));
    }
    if ctx.input.key_pressed(KeyCode::Digit2) {
      return SceneAction::Switch(Box::new(SpeedControlScene::new()));
    }
    if ctx.input.key_pressed(KeyCode::Digit3) {
      return SceneAction::Switch(Box::new(CallbacksScene::new()));
    }
    if ctx.input.key_pressed(KeyCode::Digit4) {
      return SceneAction::Switch(Box::new(CombinedEffectsScene::new()));
    }
    if ctx.input.key_pressed(KeyCode::Digit6) {
      return SceneAction::Switch(Box::new(ColorAlphaScene::new()));
    }

    // Restart
    if ctx.input.key_pressed(KeyCode::Space) {
      if let Some(entity) = self.entity {
        if let Some(mut seq) = ctx.world.get_mut::<TweenSequence>(entity) {
          seq.restart();
        }
        if let Some(mut transform) = ctx.world.get_mut::<Transform>(entity) {
          transform.x = 100.0;
          transform.y = 100.0;
        }
      }
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let text_accent = Color::rgb(130, 180, 255);
    let text_muted = Color::rgb(160, 160, 160);
    let text_light = Color::rgb(220, 220, 220);

    ui.draw_text("[5] Tween Sequences", 10.0, 10.0, text_accent);
    ui.draw_text("1-4,6: Other scenes | Space: Restart | ESC: Exit", 10.0, 30.0, text_muted);

    let (w, h) = ui.screen_size();
    let center_x = w as f32 / 2.0;
    let center_y = h as f32 / 2.0;

    ui.draw_text("TweenSequence chains multiple tweens", center_x - 130.0, center_y - 20.0, text_light);
    ui.draw_text("Each tween plays after the previous completes", center_x - 155.0, center_y, text_muted);
    ui.draw_text("This box follows a square path: -> v <- ^", center_x - 145.0, center_y + 20.0, text_muted);
  }
}

// ============================================================================
// Scene 6: Color & Alpha
// ============================================================================

#[derive(Debug)]
struct ColorAlphaDemo;

struct ColorAlphaScene {
  entities: Vec<Entity>,
  screen_w: f32,
  screen_h: f32,
}

impl ColorAlphaScene {
  fn new() -> Self {
    Self {
      entities: Vec::new(),
      screen_w: 1024.0,
      screen_h: 768.0,
    }
  }

  fn spawn_demos(&mut self, ctx: &mut SceneContext) {
    let start_y = 200.0;
    let spacing = 120.0;

    // Demo 1: Alpha fade (ping-pong)
    let sprite1 = Sprite::solid_rect(60, 60, 100, 150, 255, 255);
    let handle1 = SpriteHandle::from_sprite(sprite1, "fade_box");

    self.entities.push(ctx.world.spawn((
      ColorAlphaDemo,
      Transform::new(200.0, start_y).with_pivot_center(),
      Renderable::sprite(handle1).with_z_order(10),
      Tween::new(1.5)
        .to_alpha(0.1)
        .with_easing(Ease::InOutSine)
        .with_loop(TweenLoop::PingPong),
    )));

    // Demo 2: Tint to red
    let sprite2 = Sprite::solid_rect(60, 60, 255, 255, 255, 255);
    let handle2 = SpriteHandle::from_sprite(sprite2, "tint_red");

    self.entities.push(ctx.world.spawn((
      ColorAlphaDemo,
      Transform::new(350.0, start_y).with_pivot_center(),
      Renderable::sprite(handle2).with_z_order(10),
      Tween::new(1.0)
        .to_tint(Color::new(255, 80, 80, 255))
        .with_easing(Ease::InOutQuad)
        .with_loop(TweenLoop::PingPong),
    )));

    // Demo 3: Tint to green
    let sprite3 = Sprite::solid_rect(60, 60, 255, 255, 255, 255);
    let handle3 = SpriteHandle::from_sprite(sprite3, "tint_green");

    self.entities.push(ctx.world.spawn((
      ColorAlphaDemo,
      Transform::new(500.0, start_y).with_pivot_center(),
      Renderable::sprite(handle3).with_z_order(10),
      Tween::new(1.0)
        .to_tint(Color::new(80, 255, 80, 255))
        .with_easing(Ease::InOutQuad)
        .with_loop(TweenLoop::PingPong),
    )));

    // Demo 4: Tint to blue
    let sprite4 = Sprite::solid_rect(60, 60, 255, 255, 255, 255);
    let handle4 = SpriteHandle::from_sprite(sprite4, "tint_blue");

    self.entities.push(ctx.world.spawn((
      ColorAlphaDemo,
      Transform::new(650.0, start_y).with_pivot_center(),
      Renderable::sprite(handle4).with_z_order(10),
      Tween::new(1.0)
        .to_tint(Color::new(80, 80, 255, 255))
        .with_easing(Ease::InOutQuad)
        .with_loop(TweenLoop::PingPong),
    )));

    // Demo 5: Color cycle (sequence)
    let sprite5 = Sprite::solid_circle(40, 255, 255, 255, 255);
    let handle5 = SpriteHandle::from_sprite(sprite5, "color_cycle");

    self.entities.push(ctx.world.spawn((
      ColorAlphaDemo,
      Transform::new(825.0, start_y).with_pivot_center(),
      Renderable::sprite(handle5).with_z_order(10),
      TweenSequence::new()
        .then(Tween::new(0.6).to_tint(Color::new(255, 100, 100, 255)).with_easing(Ease::InOutSine))
        .then(Tween::new(0.6).to_tint(Color::new(100, 255, 100, 255)).with_easing(Ease::InOutSine))
        .then(Tween::new(0.6).to_tint(Color::new(100, 100, 255, 255)).with_easing(Ease::InOutSine))
        .then(Tween::new(0.6).to_tint(Color::WHITE).with_easing(Ease::InOutSine))
        .with_loop(),
    )));

    // Row 2: Combined effects
    let row2_y = start_y + spacing;

    // Demo 6: Alpha + Scale (pop in/out)
    let sprite6 = Sprite::solid_rect(50, 50, 255, 200, 100, 255);
    let handle6 = SpriteHandle::from_sprite(sprite6, "pop_box");

    self.entities.push(ctx.world.spawn((
      ColorAlphaDemo,
      Transform::new(200.0, row2_y).with_pivot_center().with_uniform_scale(0.1),
      Renderable::sprite(handle6).with_z_order(10).with_alpha(0.0),
      TweenSequence::new()
        .then(Tween::new(0.4).to_alpha(1.0).to_scale_x(1.0).to_scale_y(1.0).with_easing(Ease::OutBack))
        .then(Tween::new(0.8)) // Hold
        .then(Tween::new(0.4).to_alpha(0.0).to_scale_x(0.1).to_scale_y(0.1).with_easing(Ease::InBack))
        .then(Tween::new(0.3)) // Pause
        .with_loop(),
    )));

    // Demo 7: Tint + Position (moving color)
    let sprite7 = Sprite::solid_circle(30, 255, 255, 255, 255);
    let handle7 = SpriteHandle::from_sprite(sprite7, "moving_tint");

    self.entities.push(ctx.world.spawn((
      ColorAlphaDemo,
      Transform::new(350.0, row2_y).with_pivot_center(),
      Renderable::sprite(handle7).with_z_order(10),
      TweenSequence::new()
        .then(Tween::new(0.8).to_x(550.0).to_tint(Color::new(255, 100, 255, 255)).with_easing(Ease::InOutQuad))
        .then(Tween::new(0.8).to_x(350.0).to_tint(Color::WHITE).with_easing(Ease::InOutQuad))
        .with_loop(),
    )));

    // Demo 8: Alpha + Rotation (fading spin)
    let sprite8 = Sprite::solid_rect(45, 45, 150, 255, 200, 255);
    let handle8 = SpriteHandle::from_sprite(sprite8, "spin_fade");

    self.entities.push(ctx.world.spawn((
      ColorAlphaDemo,
      Transform::new(700.0, row2_y).with_pivot_center(),
      Renderable::sprite(handle8).with_z_order(10),
      Tween::new(2.0)
        .to_alpha(0.2)
        .to_rotation_value(PI * 2.0)
        .with_easing(Ease::Linear)
        .with_loop(TweenLoop::PingPong),
    )));

    // Row 3: Labels
    let row3_y = start_y + spacing * 2.0;

    // Demo 9: Tint darkening (multiply effect)
    let sprite9 = Sprite::solid_rect(60, 60, 255, 200, 150, 255);
    let handle9 = SpriteHandle::from_sprite(sprite9, "darken");

    self.entities.push(ctx.world.spawn((
      ColorAlphaDemo,
      Transform::new(200.0, row3_y).with_pivot_center(),
      Renderable::sprite(handle9).with_z_order(10),
      Tween::new(1.2)
        .to_tint(Color::new(80, 80, 80, 255)) // Darken by multiplying with gray
        .with_easing(Ease::InOutSine)
        .with_loop(TweenLoop::PingPong),
    )));

    // Demo 10: Full combo (position + scale + rotation + alpha + tint)
    let sprite10 = Sprite::solid_circle(25, 255, 255, 255, 255);
    let handle10 = SpriteHandle::from_sprite(sprite10, "full_combo");

    self.entities.push(ctx.world.spawn((
      ColorAlphaDemo,
      Transform::new(500.0, row3_y).with_pivot_center(),
      Renderable::sprite(handle10).with_z_order(10),
      TweenSequence::new()
        .then(
          Tween::new(1.0)
            .to_x(700.0)
            .to_scale_x(1.5).to_scale_y(1.5)
            .to_rotation_value(PI)
            .to_alpha(0.5)
            .to_tint(Color::new(255, 150, 50, 255))
            .with_easing(Ease::InOutCubic),
        )
        .then(
          Tween::new(1.0)
            .to_x(500.0)
            .to_scale_x(1.0).to_scale_y(1.0)
            .to_rotation_value(PI * 2.0)
            .to_alpha(1.0)
            .to_tint(Color::WHITE)
            .with_easing(Ease::InOutCubic),
        )
        .with_loop(),
    )));
  }

  fn cleanup(&mut self, ctx: &mut SceneContext) {
    for entity in self.entities.drain(..) {
      let _ = ctx.world.despawn(entity);
    }
  }
}

impl Scene for ColorAlphaScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let (screen_w, screen_h) = ctx.screen_size();
    self.screen_w = screen_w;
    self.screen_h = screen_h;
    self.spawn_demos(ctx);
  }

  fn on_exit(&mut self, ctx: &mut SceneContext) {
    self.cleanup(ctx);
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Quit;
    }

    // Scene switching
    if ctx.input.key_pressed(KeyCode::Digit1) {
      return SceneAction::Switch(Box::new(EasingScene::new()));
    }
    if ctx.input.key_pressed(KeyCode::Digit2) {
      return SceneAction::Switch(Box::new(SpeedControlScene::new()));
    }
    if ctx.input.key_pressed(KeyCode::Digit3) {
      return SceneAction::Switch(Box::new(CallbacksScene::new()));
    }
    if ctx.input.key_pressed(KeyCode::Digit4) {
      return SceneAction::Switch(Box::new(CombinedEffectsScene::new()));
    }
    if ctx.input.key_pressed(KeyCode::Digit5) {
      return SceneAction::Switch(Box::new(SequenceScene::new()));
    }

    // Restart all
    if ctx.input.key_pressed(KeyCode::Space) {
      self.cleanup(ctx);
      self.spawn_demos(ctx);
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let text_accent = Color::rgb(130, 180, 255);
    let text_muted = Color::rgb(160, 160, 160);
    let text_white = Color::rgb(240, 240, 240);

    ui.draw_text("[6] Color & Alpha Tweening", 10.0, 10.0, text_accent);
    ui.draw_text("1-5: Other scenes | Space: Restart | ESC: Exit", 10.0, 30.0, text_muted);

    // Row labels
    ui.draw_text("Alpha Fade", 170.0, 150.0, text_white);
    ui.draw_text("Tint Red", 330.0, 150.0, text_white);
    ui.draw_text("Tint Green", 470.0, 150.0, text_white);
    ui.draw_text("Tint Blue", 630.0, 150.0, text_white);
    ui.draw_text("Color Cycle", 785.0, 150.0, text_white);

    ui.draw_text("Pop In/Out", 165.0, 270.0, text_white);
    ui.draw_text("Moving Tint", 400.0, 270.0, text_white);
    ui.draw_text("Spin Fade", 665.0, 270.0, text_white);

    ui.draw_text("Darken", 180.0, 390.0, text_white);
    ui.draw_text("Full Combo (all properties)", 500.0, 390.0, text_white);
  }
}

// ============================================================================
// Main
// ============================================================================

fn main() {
  let engine = EngineBuilder::new()
    .title("Tweening Demo - Ryo2D")
    .window_mode(WindowMode::ResizableAspectRatio)
    .fps(60)
    .physics_hz(60)
    .with_scene(EasingScene::new())
    .build();

  if let Err(e) = engine.run() {
    eprintln!("Engine error: {}", e);
  }
}
