//! Tile Grids Demo - the engine's tile grids page by page, with Kenney's
//! CC0 art: a hand-made meadow in an endless world, a Tiled infinite map
//! whose chunks (at negative cells too) are the meadow and whose stream's
//! generator goes on past both its ends, a runner running on across it and
//! an overlay telling the hand-made chunks from the generated; an empty
//! infinite map built in code that grows wherever the mouse paints, with a
//! ball bouncing on what is painted; an endless world streamed from a
//! generator, where a digger digs and builds and finds it all as it left it
//! when it comes back (`SavedChunks`); then the other grids Tiled has: an
//! isometric town of tall walls and trees a knight walks behind and in
//! front of, and raised ground a block up, on a floor of its own, that it
//! climbs a ramp onto, walking there by itself along a path up the ramp to
//! the pointer, the town's terrain painted with the mouse; an island on a
//! staggered grid, its rows or its columns shifted; a
//! hex board that picks, selects and shows a hex's range, ring, line and
//! neighbours, its sand turned in sixths, and walks a walker by the
//! cheapest path round its rocks; a skewed oblique dungeon and its mirror;
//! and the speed guards' 1024 x 1024 map on each grid, flown over with the
//! frame's cost shown.
//! The maps are files in `assets/` written by Tiled through
//! `src/bin/make-grid-maps.rs` (the speed page's are built in code); the
//! generators' noise is `ryo2d-procgen`'s. A library, so its tests drive
//! the pages headlessly

mod endless;
mod hexagonal;
mod infinite;
mod isometric;
mod oblique;
mod overlay;
mod paint;
mod shared;
mod speed;
mod staggered;
mod terrain;
mod walker;

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
    .page(isometric::TownPage::new)
    .page(staggered::StaggeredPage::new)
    .page(hexagonal::HexPage::new)
    .page(oblique::ObliquePage::new)
    .page(speed::SpeedPage::new)
}
