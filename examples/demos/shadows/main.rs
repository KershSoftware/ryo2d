//! Shadow Demo
//!
//! Eight shadow techniques on the demo-kit shell, one page each: hard
//! (shadow volumes), drop, blob, soft, colored, contact, ambient occlusion
//! and ray-marched shadows.

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
    .page(RaymarchedShadowsPage::new)
    .run();
}
