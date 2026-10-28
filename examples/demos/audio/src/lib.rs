//! Audio Demo - the audio engine page by page: the buses and the player's
//! six volume settings, a sound's variations, voice limits, spatial sound,
//! the mix (pause, time scale, snapshots and ducking), music, and hot
//! reload. Every sound is a WAV that `src/bin/make-sounds.rs` writes from
//! code, played through the `.sound.ron`, `.music.ron` and `.mixer.ron`
//! files in `assets/`. Each page draws what it plays as text and bars, from
//! `voices_on` and the volume settings, so the headless tour captures it.
//! A library, so its tests drive the pages headlessly; the web build plays
//! the files embedded in it ([`embedded`]), as the web has no file system

mod buses;
mod embedded;
mod hot_reload;
pub mod limits;
mod mix;
mod music;
mod shared;
mod spatial;
mod variations;

use demo_kit::prelude::*;

pub use embedded::{EMBEDDED, embedded};

/// The demo's seven pages
pub fn demo() -> Demo {
  Demo::new("Audio")
    .resolution(960, 600)
    .configure(|b| b.show_fps(true))
    .page(buses::BusesPage::new)
    .page(variations::VariationsPage::new)
    .page(limits::LimitsPage::new)
    .page(spatial::SpatialPage::new)
    .page(mix::MixPage::new)
    .page(music::MusicPage::new)
    .page(hot_reload::HotReloadPage::new)
}
