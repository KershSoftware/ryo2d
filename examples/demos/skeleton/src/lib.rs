//! Skeleton Demo - skeletal animation, page by page, on one cutout hero:
//! crossfades between its animations, a second track layered over the
//! first, events that play sounds and puff dust, a sword on a bone's
//! socket, skins worn together, bones aimed at the pointer, and a crowd of
//! two hundred. The hero is `assets/hero.skeleton.ron`, written by hand
//! over parts packed into one atlas (`assets/README.md`). The web build
//! plays the files embedded in it ([`embedded`]), as the web has no file
//! system.
//!
//! Each page adds `Skeleton` entities and tells their `SkeletonAnimation`
//! what to play; the engine steps and draws them. A library, so its tests
//! drive the pages headlessly

mod embedded;
mod hero;
mod pages;

use demo_kit::prelude::*;

pub use embedded::{EMBEDDED, embedded};

/// The demo's seven pages
pub fn demo() -> Demo {
  // Smooth art, not pixel art: a canvas that fills any window
  Demo::new("Skeleton")
    .fit(1280, 720)
    .page(pages::character::CharacterPage::default)
    .page(pages::layers::LayersPage::default)
    .page(pages::events::EventsPage::default)
    .page(pages::sockets::SocketsPage::default)
    .page(pages::skins::SkinsPage::default)
    .page(pages::aiming::AimingPage::default)
    .page(pages::crowd::CrowdPage::default)
}
