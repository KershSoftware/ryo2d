//! State Machine Demo - Ryo2D Example
//!
//! Demonstrates both StateMachine (FSM) and StateStack:
//!
//! ## Left Side: Character FSM
//! A finite state machine controlling character states with explicit transitions.
//!
//! ## Right Side: Game State Stack
//! A pushdown automata for interruptible/nested game states.
//!
//! Runs on the demo-kit shell.

use demo_kit::prelude::*;

// =============================================================================
// Character State Machine (FSM)
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum CharacterState {
  Idle,
  Walking,
  Running,
  Jumping,
  Falling,
  Crouching,
  Attacking,
  Stunned,
}

impl CharacterState {
  fn color(&self) -> (u8, u8, u8) {
    match self {
      CharacterState::Idle => (100, 200, 100),      // Green
      CharacterState::Walking => (100, 150, 255),   // Blue
      CharacterState::Running => (255, 150, 50),    // Orange
      CharacterState::Jumping => (255, 255, 100),   // Yellow
      CharacterState::Falling => (200, 100, 255),   // Purple
      CharacterState::Crouching => (150, 150, 150), // Gray
      CharacterState::Attacking => (255, 80, 80),   // Red
      CharacterState::Stunned => (255, 50, 150),    // Pink
    }
  }

  fn name(&self) -> &'static str {
    match self {
      CharacterState::Idle => "Idle",
      CharacterState::Walking => "Walking",
      CharacterState::Running => "Running",
      CharacterState::Jumping => "Jumping",
      CharacterState::Falling => "Falling",
      CharacterState::Crouching => "Crouching",
      CharacterState::Attacking => "Attacking",
      CharacterState::Stunned => "Stunned",
    }
  }
}

// =============================================================================
// Game State Stack
// =============================================================================

#[derive(Debug, Clone, PartialEq)]
enum GameState {
  Playing,
  Paused,
  Inventory,
  Options,
  OptionsAudio,
  OptionsVideo,
}

impl GameState {
  fn color(&self) -> (u8, u8, u8) {
    match self {
      GameState::Playing => (100, 255, 100),      // Green
      GameState::Paused => (255, 200, 100),       // Orange
      GameState::Inventory => (100, 200, 255),    // Blue
      GameState::Options => (200, 150, 255),      // Purple
      GameState::OptionsAudio => (255, 150, 200), // Pink
      GameState::OptionsVideo => (150, 255, 200), // Cyan
    }
  }

  fn name(&self) -> &'static str {
    match self {
      GameState::Playing => "Playing",
      GameState::Paused => "Paused",
      GameState::Inventory => "Inventory",
      GameState::Options => "Options",
      GameState::OptionsAudio => "Audio Settings",
      GameState::OptionsVideo => "Video Settings",
    }
  }
}

// =============================================================================
// Demo Scene
// =============================================================================

struct StateMachineDemoScene {
  // Character FSM
  character_fsm: StateMachine<CharacterState>,
  character_entity: Option<Entity>,
  character_x: f32,
  character_y: f32,
  velocity_y: f32,
  facing_right: bool,

  // Game state stack
  game_stack: StateStack<GameState>,

  // Visual
  screen_w: f32,
  screen_h: f32,
  ground_y: f32,

  // Transition log for display
  fsm_log: Vec<String>,
  stack_log: Vec<String>,
}

impl StateMachineDemoScene {
  fn new() -> Self {
    // Create character FSM with transitions
    let character_fsm = StateMachine::new(CharacterState::Idle)
      // From Idle
      .with_transition(CharacterState::Idle, CharacterState::Walking)
      .with_transition(CharacterState::Idle, CharacterState::Running)
      .with_transition(CharacterState::Idle, CharacterState::Jumping)
      .with_transition(CharacterState::Idle, CharacterState::Crouching)
      .with_transition(CharacterState::Idle, CharacterState::Attacking)
      // From Walking
      .with_transition(CharacterState::Walking, CharacterState::Idle)
      .with_transition(CharacterState::Walking, CharacterState::Running)
      .with_transition(CharacterState::Walking, CharacterState::Jumping)
      // From Running
      .with_transition(CharacterState::Running, CharacterState::Idle)
      .with_transition(CharacterState::Running, CharacterState::Walking)
      .with_transition(CharacterState::Running, CharacterState::Jumping)
      // From Jumping
      .with_transition(CharacterState::Jumping, CharacterState::Falling)
      // From Falling
      .with_transition(CharacterState::Falling, CharacterState::Idle)
      // From Crouching
      .with_transition(CharacterState::Crouching, CharacterState::Idle)
      // From Attacking
      .with_transition(CharacterState::Attacking, CharacterState::Stunned)
      .with_transition(CharacterState::Attacking, CharacterState::Idle)
      // From Stunned
      .with_transition(CharacterState::Stunned, CharacterState::Idle)
      // Enable history
      .with_history(10);

    // Create game state stack
    let game_stack = StateStack::new(GameState::Playing);

    Self {
      character_fsm,
      character_entity: None,
      character_x: 200.0,
      character_y: 400.0,
      velocity_y: 0.0,
      facing_right: true,

      game_stack,

      screen_w: 0.0,
      screen_h: 0.0,
      ground_y: 450.0,

      fsm_log: vec!["Started in Idle".to_string()],
      stack_log: vec!["Started in Playing".to_string()],
    }
  }

  fn create_character_sprite() -> SpriteHandle {
    // White sprite that we'll tint with state color
    let sprite = Sprite::solid_rect(40, 60, 255, 255, 255, 255);
    SpriteHandle::from_sprite(sprite, "character")
  }

  fn spawn_character(&mut self, ctx: &mut SceneContext) {
    let sprite = Self::create_character_sprite();
    let (r, g, b) = self.character_fsm.current().color();
    self.character_entity = Some(
      ctx.world.spawn((
        Transform::new(self.character_x, self.character_y).with_pivot_center(),
        Renderable::sprite(sprite)
          .with_z_order(10)
          .with_tint(Color::rgb(r, g, b)),
      )),
    );
  }

  fn update_character_tint(&mut self, ctx: &mut SceneContext) {
    if let Some(entity) = self.character_entity {
      let (r, g, b) = self.character_fsm.current().color();
      if let Some(mut renderable) = ctx.world.get_mut::<Renderable>(entity) {
        renderable.tint = Color::rgb(r, g, b);
      }
    }
  }

  fn log_fsm_transition(&mut self, from: &str, to: &str) {
    let msg = format!("{} -> {}", from, to);
    self.fsm_log.push(msg);
    if self.fsm_log.len() > 8 {
      self.fsm_log.remove(0);
    }
  }

  fn log_stack_action(&mut self, action: &str) {
    self.stack_log.push(action.to_string());
    if self.stack_log.len() > 8 {
      self.stack_log.remove(0);
    }
  }

  fn update_character_fsm(&mut self, ctx: &mut SceneContext, dt: f32) {
    let input = &ctx.input;
    let prev_state = *self.character_fsm.current();

    // Update FSM time
    self.character_fsm.update(dt);

    // Handle state-specific logic and transitions
    match self.character_fsm.current() {
      CharacterState::Idle => {
        // Transition to other states
        if input.key_pressed(KeyCode::Space) {
          let _ = self.character_fsm.transition_to(CharacterState::Jumping);
          self.velocity_y = -400.0;
        } else if input.key_pressed(KeyCode::KeyX) {
          let _ = self.character_fsm.transition_to(CharacterState::Attacking);
        } else if input.key_down(KeyCode::KeyS) {
          let _ = self.character_fsm.transition_to(CharacterState::Crouching);
        } else if input.key_down(KeyCode::KeyA) || input.key_down(KeyCode::KeyD) {
          if input.key_down(KeyCode::ShiftLeft) {
            let _ = self.character_fsm.transition_to(CharacterState::Running);
          } else {
            let _ = self.character_fsm.transition_to(CharacterState::Walking);
          }
          self.facing_right = input.key_down(KeyCode::KeyD);
        }
      }

      CharacterState::Walking => {
        let speed = 100.0;
        if input.key_down(KeyCode::KeyD) {
          self.character_x += speed * dt;
          self.facing_right = true;
        } else if input.key_down(KeyCode::KeyA) {
          self.character_x -= speed * dt;
          self.facing_right = false;
        }

        // Transitions
        if input.key_pressed(KeyCode::Space) {
          let _ = self.character_fsm.transition_to(CharacterState::Jumping);
          self.velocity_y = -400.0;
        } else if input.key_down(KeyCode::ShiftLeft) {
          let _ = self.character_fsm.transition_to(CharacterState::Running);
        } else if !input.key_down(KeyCode::KeyA) && !input.key_down(KeyCode::KeyD) {
          let _ = self.character_fsm.transition_to(CharacterState::Idle);
        }
      }

      CharacterState::Running => {
        let speed = 200.0;
        if input.key_down(KeyCode::KeyD) {
          self.character_x += speed * dt;
          self.facing_right = true;
        } else if input.key_down(KeyCode::KeyA) {
          self.character_x -= speed * dt;
          self.facing_right = false;
        }

        // Transitions
        if input.key_pressed(KeyCode::Space) {
          let _ = self.character_fsm.transition_to(CharacterState::Jumping);
          self.velocity_y = -500.0; // Higher jump when running
        } else if !input.key_down(KeyCode::ShiftLeft) {
          let _ = self.character_fsm.transition_to(CharacterState::Walking);
        } else if !input.key_down(KeyCode::KeyA) && !input.key_down(KeyCode::KeyD) {
          let _ = self.character_fsm.transition_to(CharacterState::Idle);
        }
      }

      CharacterState::Jumping => {
        // Horizontal movement while jumping (air control + sprint)
        let air_speed = if input.key_down(KeyCode::ShiftLeft) {
          180.0
        } else {
          120.0
        };
        if input.key_down(KeyCode::KeyD) {
          self.character_x += air_speed * dt;
        } else if input.key_down(KeyCode::KeyA) {
          self.character_x -= air_speed * dt;
        }

        // Apply gravity
        self.velocity_y += 800.0 * dt;
        self.character_y += self.velocity_y * dt;

        // Fast-fall / dive (crouch in air)
        if input.key_down(KeyCode::KeyS) {
          self.velocity_y += 1200.0 * dt; // Extra downward force
        }

        // Transition to falling
        if self.velocity_y > 0.0 {
          let _ = self.character_fsm.transition_to(CharacterState::Falling);
        }
      }

      CharacterState::Falling => {
        // Horizontal movement while falling (air control + sprint)
        let air_speed = if input.key_down(KeyCode::ShiftLeft) {
          180.0
        } else {
          120.0
        };
        if input.key_down(KeyCode::KeyD) {
          self.character_x += air_speed * dt;
        } else if input.key_down(KeyCode::KeyA) {
          self.character_x -= air_speed * dt;
        }

        // Apply gravity
        let gravity = if input.key_down(KeyCode::KeyS) {
          1600.0
        } else {
          800.0
        }; // Fast-fall
        self.velocity_y += gravity * dt;
        self.character_y += self.velocity_y * dt;

        // Land
        if self.character_y >= self.ground_y {
          self.character_y = self.ground_y;
          self.velocity_y = 0.0;
          let _ = self.character_fsm.transition_to(CharacterState::Idle);
        }
      }

      CharacterState::Crouching => {
        if !input.key_down(KeyCode::KeyS) {
          let _ = self.character_fsm.transition_to(CharacterState::Idle);
        }
      }

      CharacterState::Attacking => {
        // Attack for 0.3 seconds, then transition
        if self.character_fsm.time_in_state() > 0.3 {
          let _ = self.character_fsm.transition_to(CharacterState::Stunned);
        }
      }

      CharacterState::Stunned => {
        // Recover after 0.5 seconds
        if self.character_fsm.time_in_state() > 0.5 {
          let _ = self.character_fsm.transition_to(CharacterState::Idle);
        }
      }
    }

    // Keep character in bounds
    self.character_x = self.character_x.clamp(50.0, 350.0);

    // Update entity position
    if let Some(entity) = self.character_entity
      && let Some(mut transform) = ctx.world.get_mut::<Transform>(entity)
    {
      transform.x = self.character_x;
      transform.y = self.character_y;
    }

    // Log transition and update tint
    let new_state = *self.character_fsm.current();
    if new_state != prev_state {
      self.log_fsm_transition(prev_state.name(), new_state.name());
      self.update_character_tint(ctx);
    }
  }

  fn update_game_stack(&mut self, ctx: &mut SceneContext, dt: f32) {
    let input = &ctx.input;
    let _prev_state = self.game_stack.current().clone();

    // Update stack time
    self.game_stack.update(dt);

    // Stack controls
    if input.key_pressed(KeyCode::KeyP) && self.game_stack.current() != &GameState::Paused {
      self.game_stack.push(GameState::Paused);
      self.log_stack_action("Push: Paused");
    }

    if input.key_pressed(KeyCode::KeyI) {
      self.game_stack.push(GameState::Inventory);
      self.log_stack_action("Push: Inventory");
    }

    if input.key_pressed(KeyCode::KeyO) {
      if self.game_stack.current() == &GameState::Options {
        // If already in options, push sub-options
        if input.key_down(KeyCode::ShiftLeft) {
          self.game_stack.push(GameState::OptionsVideo);
          self.log_stack_action("Push: Video Settings");
        } else {
          self.game_stack.push(GameState::OptionsAudio);
          self.log_stack_action("Push: Audio Settings");
        }
      } else {
        self.game_stack.push(GameState::Options);
        self.log_stack_action("Push: Options");
      }
    }

    if input.key_pressed(KeyCode::Backspace) && !self.game_stack.is_at_root() {
      let popped = self.game_stack.current().name().to_string();
      self.game_stack.pop();
      self.log_stack_action(&format!("Pop: {}", popped));
    }

    if input.key_pressed(KeyCode::Home) {
      let count = self.game_stack.pop_to_root();
      if count > 0 {
        self.log_stack_action(&format!("Pop to root ({} states)", count));
      }
    }
  }
}

impl DemoPage for StateMachineDemoScene {
  fn name(&self) -> &str {
    "State Machines"
  }

  fn description(&self) -> &str {
    "A character FSM with explicit transitions (left) beside a pushdown state stack (right)"
  }

  fn controls(&self) -> Vec<Control> {
    vec![
      Control::keys("A/D", &[KeyCode::KeyA, KeyCode::KeyD], "Walk (Shift: run)"),
      Control::key(KeyCode::Space, "Jump"),
      Control::key(KeyCode::KeyS, "Crouch (hold)"),
      Control::key(KeyCode::KeyX, "Attack"),
      Control::key(KeyCode::KeyP, "Push pause"),
      Control::key(KeyCode::KeyI, "Push inventory"),
      Control::key(KeyCode::KeyO, "Push options (Shift+O: video)"),
      Control::key(KeyCode::Backspace, "Pop state"),
      Control::key(KeyCode::Home, "Back to the root"),
    ]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    let (w, h) = ctx.screen_size();
    self.screen_w = w;
    self.screen_h = h;
    self.ground_y = h - 150.0;
    self.character_y = self.ground_y;

    self.spawn_character(ctx);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    self.update_character_fsm(ctx, dt);
    self.update_game_stack(ctx, dt);
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let area = content_area(ui);
    let oy = area.y - 16.0;
    let white = Color::rgb(240, 240, 240);
    let gray = Color::rgb(150, 150, 150);
    let dark_gray = Color::rgb(80, 80, 80);

    // =========================================================================
    // Left Side: Character FSM
    // =========================================================================
    ui.draw_text("CHARACTER FSM", 20.0, oy + 20.0, white);
    ui.draw_text("━━━━━━━━━━━━━━", 20.0, oy + 38.0, dark_gray);

    // Current state with color
    let current_state = self.character_fsm.current();
    let (r, g, b) = current_state.color();
    let state_color = Color::rgb(r, g, b);
    ui.draw_text("Current:", 20.0, oy + 60.0, gray);
    ui.draw_text(current_state.name(), 90.0, oy + 60.0, state_color);

    // Time in state
    let time_str = format!("{:.1}s", self.character_fsm.time_in_state());
    ui.draw_text("Time:", 20.0, oy + 80.0, gray);
    ui.draw_text(&time_str, 90.0, oy + 80.0, white);

    // Previous state
    if let Some(prev) = self.character_fsm.previous() {
      let (pr, pg, pb) = prev.color();
      ui.draw_text("Previous:", 20.0, oy + 100.0, gray);
      ui.draw_text(prev.name(), 90.0, oy + 100.0, Color::rgb(pr, pg, pb));
    }

    // Transition log
    ui.draw_text("Transitions:", 20.0, oy + 130.0, gray);
    for (i, log) in self.fsm_log.iter().enumerate() {
      let alpha = 255 - (i as u8 * 20);
      ui.draw_text(
        log,
        20.0,
        oy + 150.0 + (i as f32 * 16.0),
        Color::new(200, 200, 200, alpha),
      );
    }

    // Ground line
    ui.draw_rect(Rect::new(0.0, self.ground_y + 30.0, 400.0, 2.0), dark_gray);

    // =========================================================================
    // Divider
    // =========================================================================
    ui.draw_rect(Rect::new(400.0, area.y, 2.0, area.height), dark_gray);

    // =========================================================================
    // Right Side: Game State Stack
    // =========================================================================
    ui.draw_text("GAME STATE STACK", 420.0, oy + 20.0, white);
    ui.draw_text("━━━━━━━━━━━━━━━━", 420.0, oy + 38.0, dark_gray);

    // Current state
    let current_game = self.game_stack.current();
    let (gr, gg, gb) = current_game.color();
    let game_color = Color::rgb(gr, gg, gb);
    ui.draw_text("Current:", 420.0, oy + 60.0, gray);
    ui.draw_text(current_game.name(), 490.0, oy + 60.0, game_color);

    // Time in state
    let game_time = format!("{:.1}s", self.game_stack.time_in_state());
    ui.draw_text("Time:", 420.0, oy + 80.0, gray);
    ui.draw_text(&game_time, 490.0, oy + 80.0, white);

    // Stack depth
    let depth_str = format!("{}", self.game_stack.as_slice().len());
    ui.draw_text("Depth:", 420.0, oy + 100.0, gray);
    ui.draw_text(&depth_str, 490.0, oy + 100.0, white);

    // Stack visualization
    ui.draw_text("Stack:", 420.0, oy + 130.0, gray);
    let stack_slice = self.game_stack.as_slice();
    for (i, state) in stack_slice.iter().enumerate() {
      let y = oy + 150.0 + (i as f32 * 24.0);
      let (sr, sg, sb) = state.color();
      let prefix = if i == stack_slice.len() - 1 {
        "▶ "
      } else {
        "  "
      };
      let label = format!("{}{}", prefix, state.name());
      ui.draw_text(&label, 420.0, y, Color::rgb(sr, sg, sb));
    }

    // Action log
    ui.draw_text("Actions:", 420.0, oy + 280.0, gray);
    for (i, log) in self.stack_log.iter().enumerate() {
      let alpha = 255 - (i as u8 * 20);
      ui.draw_text(
        log,
        420.0,
        oy + 298.0 + (i as f32 * 16.0),
        Color::new(200, 200, 200, alpha),
      );
    }
  }
}

fn main() {
  Demo::new("State Machine")
    .resolution(800, 600)
    .configure(|b| {
      b.window_mode(WindowMode::ResizableAspectRatio)
        .target_fps(60)
    })
    .page(StateMachineDemoScene::new)
    .run();
}
