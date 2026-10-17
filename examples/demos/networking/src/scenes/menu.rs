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
  room_code: String,
  address: String,
  error_message: Option<String>,
  pending_match: Option<Match>,
}

impl Lobby {
  pub fn new() -> Self {
    Self {
      room_code: String::new(),
      address: DEFAULT_ADDRESS.to_string(),
      error_message: None,
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
    let room_code = self.room_code.trim().to_uppercase();
    let address = self.address.trim().to_string();

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

  pub fn ui(&mut self, ui: &mut UiContext) {
    let area = content_area(ui);
    let (sw, sh) = ui.screen_size();

    // Background
    ui.draw_rect(
      Rect::new(0.0, 0.0, sw as f32, sh as f32),
      Color::rgb(BACKGROUND_COLOR.0, BACKGROUND_COLOR.1, BACKGROUND_COLOR.2),
    );

    // Title
    ui.draw_text_anchored(
      "NETPLAY",
      Anchor::TopCenter,
      0.0,
      area.y + 10.0,
      Color::WHITE,
    );
    ui.draw_text_anchored(
      "Multiplayer Platformer",
      Anchor::TopCenter,
      0.0,
      area.y + 35.0,
      Color::GRAY,
    );

    // Create a room, or join one; Enter in either field joins
    let mut create = false;
    let mut join = false;
    let (room_code, address) = (&mut self.room_code, &mut self.address);
    let error = self.error_message.as_deref();
    ui.column()
      .focus_scope()
      .anchor(Anchor::Center)
      .offset(0.0, 60.0)
      .gap(12.0)
      .align(Align::Center)
      .show(|ui| {
        create = ui
          .button("Create Room")
          .width(200.0)
          .height(40.0)
          .default_focus()
          .clicked();

        ui.text("- or -").color(Color::GRAY);

        ui.row().gap(20.0).show(|ui| {
          ui.column().gap(6.0).show(|ui| {
            ui.text("Room Code:").color(Color::WHITE);
            join |= ui
              .text_input(room_code)
              .placeholder("XXXXX")
              .width(100.0)
              .height(32.0)
              .show()
              .submitted();
          });
          ui.column().gap(6.0).show(|ui| {
            ui.text("Address:").color(Color::WHITE);
            join |= ui
              .text_input(address)
              .placeholder("127.0.0.1:7777")
              .width(160.0)
              .height(32.0)
              .show()
              .submitted();
          });
        });

        join |= ui.button("Join Room").width(200.0).height(40.0).clicked();

        if let Some(err) = error {
          ui.text(err).color(Color::RED);
        }
      });

    if create {
      self.try_create_room();
    } else if join {
      self.try_join_room();
    }
  }
}
