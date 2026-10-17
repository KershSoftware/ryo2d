//! Shared shell for the Ryo2D feature demos.
//!
//! Every demo gets the same controls (`[ ]` pages, arrows for settings, `R`
//! reset, `H` help, `Esc` quit), the same layout and a settings panel. See
//! `examples/demos/AGENTS.md`.

mod chrome;
mod controls;
mod demo;
mod page;
mod scene;
mod settings;
mod start;

pub use chrome::{content_area, settings_panel_height};
pub use controls::Control;
pub use demo::Demo;
pub use page::DemoPage;
pub use settings::Settings;

/// Everything a demo needs: ryo2d's prelude plus the kit
pub mod prelude {
  pub use crate::{Control, Demo, DemoPage, Settings, content_area, settings_panel_height};
  pub use ryo2d::prelude::*;
}
