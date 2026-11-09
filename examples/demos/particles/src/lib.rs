//! Particles Demo - the engine's particles, page by page: every preset
//! effect, effects of the demo's own, bursts on a click, and attractors.
//! Each page adds its effects once and places them with `ParticleEmitter`
//! entities; the engine steps and draws them. A library, so its tests
//! drive the pages headlessly

mod attractors;
mod celebration;
mod combined;
mod explosions;
mod fire;
mod impacts;
mod magic;
mod shared;
mod smoke;
mod trails;
mod weather;

use demo_kit::prelude::*;

/// The demo's ten pages
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
}
