//! Particles Demo - the engine's particles, page by page: every preset
//! effect, effects of the demo's own, bursts on a click, attractors, and
//! what is new: life curves, emission, collisions and effect files. The web
//! build plays the files embedded in it ([`embedded`]), as the web has no file
//! system.
//!
//! Each page adds its effects once and places them with `ParticleEmitter`
//! entities; the engine steps and draws them. A library, so its tests
//! drive the pages headlessly

mod attractors;
mod celebration;
mod collisions;
mod combined;
mod embedded;
mod emission;
mod explosions;
mod files;
mod fire;
mod impacts;
mod life;
mod magic;
mod shared;
mod smoke;
mod trails;
mod weather;

use demo_kit::prelude::*;

pub use embedded::{EMBEDDED, embedded};

/// The demo's fourteen pages
pub fn demo() -> Demo {
  Demo::new("Particles")
    .resolution(1280, 720)
    .configure(|b| b.target_fps(144).fixed_hz(144).vsync(false))
    .page(fire::FirePage::default)
    .page(smoke::SmokePage::default)
    .page(explosions::ExplosionsPage::default)
    .page(magic::MagicPage::default)
    .page(weather::WeatherPage::default)
    .page(impacts::ImpactsPage::default)
    .page(celebration::CelebrationPage::default)
    .page(trails::TrailsPage::default)
    .page(attractors::AttractorsPage::default)
    .page(combined::CombinedPage::default)
    .page(life::LifePage::default)
    .page(emission::EmissionPage::default)
    .page(collisions::CollisionsPage::default)
    .page(files::FilesPage::default)
}
