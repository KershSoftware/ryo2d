//! Shadow Demo
//!
//! Ten shadow pages on the demo-kit shell, one each: hard (shadow volumes),
//! drop, blob, soft, colored, contact, ambient occlusion, occluders placed
//! around a light, a lantern carried among pillars and sixty-four shadowed
//! torches over a tile-map maze. A library, so its tests drive the pages
//! headlessly.

pub mod scenes;

use demo_kit::prelude::*;
use scenes::*;

/// The demo's ten pages
pub fn demo() -> Demo {
  Demo::new("Shadows")
    .resolution(1280, 720)
    .configure(|b| b.target_fps(60))
    .page(HardShadowsPage::new)
    .page(DropShadowsPage::new)
    .page(BlobShadowsPage::new)
    .page(SoftShadowsPage::new)
    .page(ColoredShadowsPage::new)
    .page(ContactShadowsPage::new)
    .page(AmbientOcclusionPage::new)
    .page(PlacedOccludersPage::new)
    .page(LanternPage::new)
    .page(ManyLightsPage::new)
}
