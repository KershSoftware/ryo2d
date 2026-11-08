//! Saves Demo - the save service and the player's settings, page by page:
//! slots saved, listed and deleted with their headers, a version 1 save
//! that loads as version 3, recovery from a damaged save and a damaged
//! settings file, and settings that change and save themselves live. A
//! library, so its tests drive the pages headlessly

mod recovery;
mod settings_page;
mod shared;
mod slots;
mod upgrades;

use demo_kit::prelude::*;

pub use shared::{DemoOptions, HeroV1, HeroV2, HeroV3, OLD_SLOT, SLOTS, v1_to_v2, v2_to_v3};

/// The demo's four pages. `HeroV3` upgrades from versions 1 and 2, the demo's
/// own settings section `demo` is kept beside the engine's, and every player
/// preference is kept
pub fn demo() -> Demo {
  Demo::new("Saves")
    .resolution(960, 600)
    .configure(|b| {
      b.save_versions::<HeroV3>(
        SaveVersions::new(3)
          .step(1, |old: HeroV1| -> HeroV2 { v1_to_v2(old) })
          .step(2, |old: HeroV2| -> HeroV3 { v2_to_v3(old) }),
      )
      .game_settings::<DemoOptions>("demo")
      .with_user_preferences(UserPreferenceFilter::all())
      .project(ryo2d::project!())
    })
    .page(slots::SlotsPage::new)
    .page(upgrades::UpgradesPage::new)
    .page(recovery::RecoveryPage::new)
    .page(settings_page::SettingsPage::new)
}
