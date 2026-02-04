//! Validated Animator Demo
//!
//! Demonstrates the ValidatedAnimator which enforces transition rules
//! between animation states. Shows how certain transitions are blocked
//! while others are allowed.
//!
//! Controls:
//! - 1: Try to go to Idle
//! - 2: Try to Walk
//! - 3: Try to Attack
//! - 4: Try to get Hurt
//! - 5: Try to Die
//! - Space: Force transition to Idle (bypasses rules)
//! - ESC: Quit

use ryo2d::prelude::*;

const SCREEN_WIDTH: f32 = 800.0;
const SCREEN_HEIGHT: f32 = 600.0;

// Animation state identifiers
const IDLE: &str = "idle";
const WALK: &str = "walk";
const ATTACK: &str = "attack";
const HURT: &str = "hurt";
const DEATH: &str = "death";

struct ValidatedAnimatorDemo {
  animator: ValidatedAnimator,
  last_transition_result: Option<(String, bool)>,
  transition_log: Vec<String>,
}

impl ValidatedAnimatorDemo {
  fn new() -> Self {
    // Create simple colored frame animations (simulating sprite sheets)
    let idle_anim = Self::create_animation("idle", 4, 0.3);
    let walk_anim = Self::create_animation("walk", 6, 0.1);
    let attack_anim = Self::create_animation("attack", 4, 0.08).with_loop_mode(LoopMode::Once);
    let hurt_anim = Self::create_animation("hurt", 2, 0.15).with_loop_mode(LoopMode::Once);
    let death_anim = Self::create_animation("death", 5, 0.2).with_loop_mode(LoopMode::Once);

    // Create validated animator with transition rules:
    //
    //   idle <--> walk     (can freely switch between idle and walk)
    //   idle --> attack    (can attack from idle)
    //   walk --> attack    (can attack from walk)
    //   attack --> idle    (attack returns to idle)
    //   * --> hurt         (can be hurt from any state)
    //   * --> death        (can die from any state)
    //   hurt --> idle      (recover from hurt to idle)
    //
    // NOT ALLOWED:
    //   attack --> walk    (can't walk mid-attack)
    //   death --> *        (can't recover from death)
    //
    let animator = ValidatedAnimator::new(idle_anim, IDLE)
      .with_animation(walk_anim, WALK)
      .with_animation(attack_anim, ATTACK)
      .with_animation(hurt_anim, HURT)
      .with_animation(death_anim, DEATH)
      // Idle <-> Walk bidirectional
      .with_bidirectional(IDLE, WALK)
      // Can attack from idle or walk
      .with_transition(IDLE, ATTACK)
      .with_transition(WALK, ATTACK)
      // Attack must return to idle
      .with_transition(ATTACK, IDLE)
      // Hurt can be entered from anywhere
      .with_transition_from_any(HURT)
      // Death can be entered from anywhere
      .with_transition_from_any(DEATH)
      // Can recover from hurt
      .with_transition(HURT, IDLE);

    Self {
      animator,
      last_transition_result: None,
      transition_log: vec!["Started in IDLE".to_string()],
    }
  }

  fn create_animation(name: &str, frame_count: usize, frame_duration: f32) -> Animation {
    // Create placeholder frames using solid color sprites
    // In a real game, these would be actual sprite frames
    let frames: Vec<SpriteHandle> = (0..frame_count)
      .map(|i| {
        let color = Self::state_color(name, i);
        SpriteHandle::from_sprite(
          Sprite::solid_rect(64, 64, color.0, color.1, color.2, 255),
          name,
        )
      })
      .collect();

    Animation::new(name, frames, frame_duration)
  }

  fn state_color(state: &str, frame: usize) -> (u8, u8, u8) {
    // Each state has a base color, frames vary in brightness
    let brightness = 180 + ((frame as i32 * 15) % 75) as u8;
    match state {
      "idle" => (0, brightness, 0),              // Green
      "walk" => (0, brightness, brightness),     // Cyan
      "attack" => (brightness, brightness, 0),   // Yellow
      "hurt" => (brightness, brightness / 2, 0), // Orange
      "death" => (brightness, 0, 0),             // Red
      _ => (brightness, brightness, brightness), // White
    }
  }

  fn try_transition(&mut self, target: &'static str) {
    let from = self.animator.current();
    let success = self.animator.set_animation(target);

    let msg = if success {
      format!("{} -> {} [OK]", from, target)
    } else {
      format!("{} -> {} [BLOCKED]", from, target)
    };

    self.last_transition_result = Some((target.to_string(), success));
    self.transition_log.insert(0, msg);
    if self.transition_log.len() > 10 {
      self.transition_log.pop();
    }
  }

  fn force_transition(&mut self, target: &'static str) {
    let from = self.animator.current();
    self.animator.force_animation(target);

    let msg = format!("{} -> {} [FORCED]", from, target);
    self.last_transition_result = Some((target.to_string(), true));
    self.transition_log.insert(0, msg);
    if self.transition_log.len() > 10 {
      self.transition_log.pop();
    }
  }
}

impl Scene for ValidatedAnimatorDemo {
  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    // Handle input
    if ctx.input.key_pressed(KeyCode::Digit1) {
      self.try_transition(IDLE);
    }
    if ctx.input.key_pressed(KeyCode::Digit2) {
      self.try_transition(WALK);
    }
    if ctx.input.key_pressed(KeyCode::Digit3) {
      self.try_transition(ATTACK);
    }
    if ctx.input.key_pressed(KeyCode::Digit4) {
      self.try_transition(HURT);
    }
    if ctx.input.key_pressed(KeyCode::Digit5) {
      self.try_transition(DEATH);
    }
    if ctx.input.key_pressed(KeyCode::Space) {
      self.force_transition(IDLE);
    }
    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Quit;
    }

    // Auto-transition when attack/hurt finishes
    if self.animator.just_finished() {
      let current = self.animator.current();
      if current == ATTACK || current == HURT {
        self.try_transition(IDLE);
      }
    }

    // Update animator
    self.animator.update(dt);

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    // Title
    ui.draw_text("Validated Animator Demo", 20.0, 20.0, Color::WHITE);

    // Current state display (large, centered)
    let state_y = 80.0;
    let current = self.animator.current();
    let state_color = match current {
      IDLE => Color::hex(0x00FF00),
      WALK => Color::hex(0x00FFFF),
      ATTACK => Color::hex(0xFFFF00),
      HURT => Color::hex(0xFFA500),
      DEATH => Color::hex(0xFF0000),
      _ => Color::WHITE,
    };

    ui.draw_text(
      "Current State:",
      SCREEN_WIDTH / 2.0 - 60.0,
      state_y,
      Color::WHITE,
    );
    ui.draw_text(
      &current.to_uppercase(),
      SCREEN_WIDTH / 2.0 - 30.0,
      state_y + 25.0,
      state_color,
    );

    // Time in state
    ui.draw_text(
      &format!("Time: {:.1}s", self.animator.time_in_state()),
      SCREEN_WIDTH / 2.0 - 40.0,
      state_y + 50.0,
      Color::hex(0xAAAAAA),
    );

    // Animated sprite preview (centered box showing current frame color)
    let preview_x = SCREEN_WIDTH / 2.0 - 40.0;
    let preview_y = state_y + 90.0;
    let preview_size = 80.0;

    // Draw border
    ui.draw_rect(
      Rect::new(
        preview_x - 2.0,
        preview_y - 2.0,
        preview_size + 4.0,
        preview_size + 4.0,
      ),
      Color::hex(0x444444),
    );

    // Draw current frame color
    let frame_color = Self::state_color(current, 0);
    ui.draw_rect(
      Rect::new(preview_x, preview_y, preview_size, preview_size),
      Color::rgb(frame_color.0, frame_color.1, frame_color.2),
    );

    // Transition rules panel (left side)
    let rules_x = 20.0;
    let rules_y = 200.0;
    ui.draw_text("Transition Rules:", rules_x, rules_y, Color::WHITE);

    let rules = [
      ("idle <-> walk", true),
      ("idle -> attack", true),
      ("walk -> attack", true),
      ("attack -> idle", true),
      ("attack -> walk", false),
      ("* -> hurt", true),
      ("* -> death", true),
      ("hurt -> idle", true),
      ("death -> *", false),
    ];

    for (i, (rule, allowed)) in rules.iter().enumerate() {
      let color = if *allowed {
        Color::hex(0x88FF88)
      } else {
        Color::hex(0xFF8888)
      };
      let symbol = if *allowed { "[+]" } else { "[x]" };
      ui.draw_text(
        &format!("{} {}", symbol, rule),
        rules_x,
        rules_y + 25.0 + (i as f32 * 18.0),
        color,
      );
    }

    // Available transitions from current state (right side)
    let avail_x = SCREEN_WIDTH - 200.0;
    let avail_y = 200.0;
    ui.draw_text("Can Transition To:", avail_x, avail_y, Color::WHITE);

    let all_states = [IDLE, WALK, ATTACK, HURT, DEATH];
    for (i, state) in all_states.iter().enumerate() {
      let can = self.animator.can_transition_to(state);
      let color = if can {
        Color::hex(0x88FF88)
      } else {
        Color::hex(0x666666)
      };
      let symbol = if can { "->" } else { "xx" };
      ui.draw_text(
        &format!("{} {}", symbol, state),
        avail_x,
        avail_y + 25.0 + (i as f32 * 18.0),
        color,
      );
    }

    // Transition log (bottom)
    let log_y = SCREEN_HEIGHT - 220.0;
    ui.draw_text("Transition Log:", 20.0, log_y, Color::WHITE);

    for (i, entry) in self.transition_log.iter().enumerate() {
      let color = if entry.contains("[OK]") {
        Color::hex(0x88FF88)
      } else if entry.contains("[BLOCKED]") {
        Color::hex(0xFF8888)
      } else {
        Color::hex(0xFFFF88)
      };
      let alpha = 255 - (i as u8 * 20).min(200);
      ui.draw_text(
        entry,
        20.0,
        log_y + 20.0 + (i as f32 * 16.0),
        color.with_alpha(alpha),
      );
    }

    // Controls
    let controls_x = SCREEN_WIDTH - 250.0;
    let controls_y = SCREEN_HEIGHT - 180.0;
    ui.draw_text("Controls:", controls_x, controls_y, Color::WHITE);

    let controls = [
      ("1", "Idle"),
      ("2", "Walk"),
      ("3", "Attack"),
      ("4", "Hurt"),
      ("5", "Death"),
      ("Space", "Force Idle"),
      ("ESC", "Quit"),
    ];

    for (i, (key, action)) in controls.iter().enumerate() {
      ui.draw_text(
        &format!("[{}] {}", key, action),
        controls_x,
        controls_y + 20.0 + (i as f32 * 18.0),
        Color::hex(0xAAAAAA),
      );
    }

    // Last result indicator
    if let Some((target, success)) = &self.last_transition_result {
      let msg = if *success {
        format!("Transitioned to {}", target)
      } else {
        format!("Blocked: cannot go to {}", target)
      };
      let color = if *success {
        Color::hex(0x00FF00)
      } else {
        Color::hex(0xFF4444)
      };
      ui.draw_text(&msg, SCREEN_WIDTH / 2.0 - 80.0, state_y + 180.0, color);
    }
  }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
  EngineBuilder::new()
    .title("Ryo2D Validated Animator Demo")
    .resolution(SCREEN_WIDTH as u32, SCREEN_HEIGHT as u32)
    .window_mode(WindowMode::ResizableAspectRatio)
    .with_scene(ValidatedAnimatorDemo::new())
    .build()
    .run()?;

  Ok(())
}
