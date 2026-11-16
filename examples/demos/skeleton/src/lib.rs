//! Skeleton Demo - skeletal animation, page by page, on one cutout hero:
//! crossfades between its animations, a second track layered over the
//! first, events whose sounds the engine plays and whose footsteps puff
//! dust, a sword on a bone's socket, skins worn together, bones aimed at
//! the pointer, a crowd of two hundred, two meshes (a cape weighted over
//! the hero's bones, which springs swing, and a banner rippled by deform
//! keys), and constraints: an arm reaching by IK with a tray held level, a
//! walker planting its feet, and a tail reaching by chain IK kept to one
//! side. The hero is `assets/hero.skeleton.ron`, written by hand over
//! parts packed into one atlas; the banner, the arm, the walker and the
//! tail have files of their own (`assets/README.md`). The web build plays
//! the files embedded in it ([`embedded`]), as the web has no file system.
//!
//! Each page adds `Skeleton` entities and tells their `SkeletonAnimation`
//! what to play; the engine steps and draws them. A library, so its tests
//! drive the pages headlessly

mod embedded;
mod hero;
mod pages;

use demo_kit::prelude::*;

pub use embedded::{EMBEDDED, embedded};
pub use pages::constraints::ground_height;

/// The demo's nine pages
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
    .page(pages::meshes::MeshesPage::default)
    .page(pages::constraints::ConstraintsPage::default)
}
