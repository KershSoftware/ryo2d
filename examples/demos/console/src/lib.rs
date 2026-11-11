//! Console Demo - the developer console's commands and the log in it, page
//! by page: commands that add and remove entities and show a phrase, a
//! command only one page has, and buttons that log at every level with the
//! console mirroring the warnings. A library, so its tests drive the pages
//! headlessly.
//!
//! The console is a development tool: a build without the tools compiles
//! every call here and does nothing, so the pages say so.

mod log_page;
mod shared;
mod workshop;
mod world;

use demo_kit::prelude::*;

pub use log_page::LIBRARY_TARGET;
pub use shared::{MAX_SPAWN, Spawned};

/// The demo's three pages
pub fn demo() -> Demo {
  Demo::new("Console")
    .resolution(960, 600)
    .configure(|b| b.project(ryo2d::project!()))
    .page(world::WorldPage::new)
    .page(workshop::WorkshopPage::new)
    .page(log_page::LogPage::new)
}
