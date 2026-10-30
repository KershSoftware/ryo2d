//! Tilemap Demo - the engine's tile maps page by page, with Kenney's CC0
//! art: a Tiled map and an LDtk level side by side, flips and a layer of two
//! tilesets, animated tiles on the game clock, a top-down garden (y-sorted
//! trees, parallax clouds, an image layer), a hero on the map's own
//! collision (slopes, a one-way bridge), objects the game gives behaviour
//! (coins, spikes, a door and its switch), painting terrain through a Wang
//! set, a procgen dungeon as a map, a world of rooms spawned near the hero,
//! a 1024 x 1024 map of four layers, and hot reload. Every map is a file in
//! `assets/` (written by `src/bin/make-maps.rs`) but the dungeon's and the
//! big map's, which are made in code. A library, so its tests drive the
//! pages headlessly; the web build plays the files embedded in it
//! ([`embedded`]), as the web has no file system

mod animated;
mod big_map;
mod collision;
mod dungeon;
mod editing;
mod embedded;
mod flips;
mod hot_reload;
mod objects;
mod rooms;
mod shared;
mod sources;
mod top_down;

use demo_kit::prelude::*;

pub use embedded::{EMBEDDED, embedded};
pub use hot_reload::MapReloads;
pub use objects::Coins;
pub use shared::Player;

/// The demo's eleven pages
pub fn demo() -> Demo {
  Demo::new("Tilemap")
    .resolution(960, 600)
    .configure(|b| b.show_fps(true))
    .page(sources::SourcesPage::new)
    .page(flips::FlipsPage::new)
    .page(animated::AnimatedPage::new)
    .page(top_down::TopDownPage::new)
    .page(collision::CollisionPage::new)
    .page(objects::ObjectsPage::new)
    .page(editing::EditingPage::new)
    .page(dungeon::DungeonPage::new)
    .page(rooms::RoomsPage::new)
    .page(big_map::BigMapPage::new)
    .page(hot_reload::HotReloadPage::new)
}
