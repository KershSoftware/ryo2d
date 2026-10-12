//! The demo's single page: a lobby that turns into a match

use demo_kit::prelude::*;

use super::{Lobby, Match};

/// Lobby or running match; creating or joining a room switches to the match,
/// leaving it switches back.
enum State {
  Lobby(Lobby),
  Match(Match),
}

pub struct NetplayPage {
  state: State,
}

impl NetplayPage {
  pub fn new() -> Self {
    Self {
      state: State::Lobby(Lobby::new()),
    }
  }
}

impl DemoPage for NetplayPage {
  fn name(&self) -> &str {
    "Netplay"
  }

  fn description(&self) -> &str {
    "Two-player networked platformer: create a room to host, or join one with its code and address."
  }

  fn controls(&self) -> Vec<Control> {
    match self.state {
      State::Lobby(_) => vec![Control::mouse("Lobby buttons and fields")],
      State::Match(_) => vec![
        Control::keys("A D", &[KeyCode::KeyA, KeyCode::KeyD], "Move"),
        Control::keys("Space W", &[KeyCode::Space, KeyCode::KeyW], "Jump"),
        Control::key(KeyCode::KeyL, "Leave the match"),
      ],
    }
  }

  /// Resetting the page (R) ends a running match cleanly
  fn exit(&mut self, _ctx: &mut SceneContext) {
    if let State::Match(game) = &mut self.state {
      game.disconnect();
    }
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    match &mut self.state {
      State::Lobby(lobby) => {
        lobby.update(ctx, dt);
        if let Some(mut game) = lobby.take_match() {
          game.enter(ctx);
          self.state = State::Match(game);
        }
      }
      State::Match(game) => {
        game.update(ctx, dt);
        if game.leave_requested() {
          game.disconnect();
          game.despawn(ctx);
          self.state = State::Lobby(Lobby::new());
        }
      }
    }
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    match &mut self.state {
      State::Lobby(lobby) => lobby.ui(ui),
      State::Match(game) => game.ui(ui),
    }
  }
}
