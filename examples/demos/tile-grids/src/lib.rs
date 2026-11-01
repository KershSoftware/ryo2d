//! Tile Grids Demo - the engine's tile grids page by page, with Kenney's
//! CC0 art: a map without fixed edges in Tiled's infinite format, its
//! chunks at negative cells, the hand-made part of a world walled at its
//! ends, with a runner patrolling it and an overlay of the chunks there are
//! and the map's bounds; and an empty infinite map built in code that grows
//! wherever the mouse paints, with a ball bouncing on what is painted. The
//! meadow is a file in `assets/` written by Tiled through
//! `src/bin/make-grid-maps.rs`. A library, so its tests drive the pages
//! headlessly. Later pages (isometric, hexagonal and oblique grids,
//! streaming, floors) join these

mod infinite;
mod overlay;
mod paint;
mod shared;

use demo_kit::prelude::*;

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
}
