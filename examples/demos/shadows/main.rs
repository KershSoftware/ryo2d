//! Shadow Demo
//!
//! Eight shadow pages on the demo-kit shell, one each: hard (shadow
//! volumes), drop, blob, soft, colored, contact, ambient occlusion and
//! occluders placed around a light.

mod scenes;

use demo_kit::prelude::*;
use scenes::*;

fn main() {
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
    .run();
}
