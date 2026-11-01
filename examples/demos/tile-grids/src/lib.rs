//! Tile Grids Demo - the engine's tile grids page by page, with Kenney's
//! CC0 art: a hand-made meadow in an endless world, a Tiled infinite map
//! whose chunks (at negative cells too) are the meadow and whose stream's
//! generator goes on past both its ends, a runner running on across it and
//! an overlay telling the hand-made chunks from the generated; an empty
//! infinite map built in code that grows wherever the mouse paints, with a
//! ball bouncing on what is painted; and an endless world streamed from a
//! generator, where a digger digs and builds and finds it all as it left it
//! when it comes back (`SavedChunks`). The meadow is a file in `assets/`
//! written by Tiled through `src/bin/make-grid-maps.rs`; the generators'
//! noise is `ryo2d-procgen`'s. A library, so its tests drive the pages
//! headlessly. Later pages (isometric, hexagonal and oblique grids, floors)
//! join these

mod endless;
mod infinite;
mod overlay;
mod paint;
mod shared;
mod terrain;

use demo_kit::prelude::*;

pub use endless::Digger;
pub use infinite::Runner;
pub use overlay::ChunkLabel;
pub use paint::Ball;

/// The demo's pages
pub fn demo() -> Demo {
  Demo::new("Tile Grids")
    .resolution(960, 600)
    .configure(|b| b.show_fps(true))
    .page(infinite::InfinitePage::new)
    .page(paint::PaintPage::new)
    .page(endless::EndlessPage::new)
}
