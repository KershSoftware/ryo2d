//! Asset Loading Demo - the asset registry's load states. The Loading
//! screen page asks for every image, both fonts, a UI file and a file that
//! is not there on the worker threads, and draws their progress as a bar and
//! each asset's state; the Hot reload page loads them at once and counts
//! each asset's `AssetEvent::Modified` while `assets/` is edited. A library,
//! so its tests drive the pages headlessly.

mod hot_reload;
mod loading;
mod tracked;

use demo_kit::prelude::*;

/// The demo's two pages: Loading screen and Hot reload
pub fn demo() -> Demo {
  Demo::new("Asset Loading")
    .resolution(800, 600)
    .configure(|b| b.show_fps(true))
    .page(loading::LoadingPage::new)
    .page(hot_reload::HotReloadPage::new)
}
