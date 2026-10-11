//! The lobby: create a room or join one

use crate::constants::*;
use demo_kit::prelude::*;

use super::Match;

/// Generate a random 5-character room code
fn generate_room_code() -> String {
  const CHARS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
  let mut rng = rand::thread_rng();
  (0..ROOM_CODE_LENGTH)
    .map(|_| CHARS[rng.gen_range(0..CHARS.len())] as char)
    .collect()
}

/// Room creation and joining; hands a `Match` to the page when one starts
pub struct Lobby {
  room_code_input: TextInputState,
  address_input: TextInputState,
  error_message: Option<String>,
  last_dt: f32,
  pending_match: Option<Match>,
}

impl Lobby {
  pub fn new() -> Self {
    let mut address_input = TextInputState::new();
    address_input.set_text(DEFAULT_ADDRESS);
    Self {
      room_code_input: TextInputState::new(),
      address_input,
      error_message: None,
      last_dt: 0.016,
      pending_match: None,
    }
  }

  fn try_create_room(&mut self) {
    let room_code = generate_room_code();
    match Match::new_host(room_code.clone()) {
      Ok(scene) => {
        self.pending_match = Some(scene);
      }
      Err(e) => {
        self.error_message = Some(format!("Failed to create: {}", e));
      }
    }
  }

  fn try_join_room(&mut self) {
    let room_code = self.room_code_input.text().trim().to_uppercase();
    let address = self.address_input.text().trim().to_string();

    if room_code.len() != ROOM_CODE_LENGTH {
      self.error_message = Some(format!("Room code must be {} characters", ROOM_CODE_LENGTH));
      return;
    }

    if address.is_empty() {
      self.error_message = Some("Enter server address".to_string());
      return;
    }

    match Match::new_client(&room_code, &address) {
      Ok(scene) => {
        self.pending_match = Some(scene);
      }
      Err(e) => {
        self.error_message = Some(format!("Failed to join: {}", e));
      }
    }
  }
}

impl Lobby {
  /// The match the player just started, if any
  pub fn take_match(&mut self) -> Option<Match> {
    self.pending_match.take()
  }

  pub fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    self.last_dt = dt;

    // Enter to join if either input is focused and both have content
    let enter_pressed = ctx.input.key_pressed(KeyCode::Enter);
    let either_focused = self.room_code_input.is_focused() || self.address_input.is_focused();
    let has_code = self.room_code_input.text().len() == ROOM_CODE_LENGTH;
    let has_address = !self.address_input.text().trim().is_empty();

    if enter_pressed && either_focused && has_code && has_address {
      self.try_join_room();
    }
  }

  pub fn render_ui(&mut self, ui: &mut UiContext) {
    let dt = self.last_dt;
    let area = content_area(ui);
    let (sw, sh) = ui.screen_size();
    let center_x = sw as f32 / 2.0;
    let center_y = sh as f32 / 2.0;

    // Background
    ui.draw_rect(
      Rect::new(0.0, 0.0, sw as f32, sh as f32),
      Color::rgb(BACKGROUND_COLOR.0, BACKGROUND_COLOR.1, BACKGROUND_COLOR.2),
    );

    // Title
    ui.draw_text_anchored("NETPLAY", Anchor::TopCenter, 0.0, area.y + 10.0, Color::WHITE);
    ui.draw_text_anchored(
      "Multiplayer Platformer",
      Anchor::TopCenter,
      0.0,
      area.y + 35.0,
      Color::GRAY,
    );

    // Create Room button
    let button_width = 200.0;
    let button_height = 40.0;
    let button_x = center_x - button_width / 2.0;

    let create_rect = Rect::new(button_x, center_y - 30.0, button_width, button_height);
    if ui.button("Create Room", create_rect) {
      self.try_create_room();
    }

    // Separator
    ui.draw_text_anchored("- or -", Anchor::Center, 0.0, 25.0, Color::GRAY);

    // Join section - Room Code
    ui.draw_text_anchored("Room Code:", Anchor::Center, -80.0, 55.0, Color::WHITE);

    let code_input_width = 100.0;
    let code_input_rect = Rect::new(
      center_x - 80.0 - code_input_width / 2.0,
      center_y + 75.0,
      code_input_width,
      32.0,
    );
    ui.text_input(&mut self.room_code_input, code_input_rect, "XXXXX", dt);

    // Join section - Server Address
    ui.draw_text_anchored("Address:", Anchor::Center, 80.0, 55.0, Color::WHITE);

    let addr_input_width = 160.0;
    let addr_input_rect = Rect::new(
      center_x + 80.0 - addr_input_width / 2.0,
      center_y + 75.0,
      addr_input_width,
      32.0,
    );
    ui.text_input(
      &mut self.address_input,
      addr_input_rect,
      "127.0.0.1:7777",
      dt,
    );

    // Join button
    let join_rect = Rect::new(button_x, center_y + 125.0, button_width, button_height);
    if ui.button("Join Room", join_rect) {
      self.try_join_room();
    }

    // Error message
    if let Some(ref err) = self.error_message {
      ui.draw_text_anchored(
        err,
        Anchor::TopCenter,
        0.0,
        join_rect.y + join_rect.height + 10.0,
        Color::RED,
      );
    }
  }
}
