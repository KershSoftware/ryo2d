//! Netplay - Multiplayer Platformer Demo
//!
//! A two-player networked platformer: a listen-server host and a client that
//! joins with a 5-character room code, with player movement, jumping,
//! collision and position synchronization. The lobby and the match are one
//! page on the demo-kit shell; `L` leaves a match back to the lobby.

use demo_kit::prelude::*;
use networking::prelude::*;

fn main() {
  Demo::new("Netplay")
    .resolution(800, 600)
    .page(NetplayPage::new)
    .run();
}
