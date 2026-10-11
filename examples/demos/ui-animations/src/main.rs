//! UI Animations Demo - Ryo2D's UI tweening system
//!
//! Easing curves, property animations (panels, fades, colour, position, rect)
//! and animated buttons driven by `UiTween`. Runs on the demo-kit shell;
//! switch pages with `[` / `]`.

mod scenes;

use demo_kit::prelude::*;
use scenes::{EasingShowcase, InteractiveDemo, PropertyDemo};

fn main() {
  Demo::new("UI Animations")
    .resolution(800, 600)
    .configure(|b| b.window_mode(WindowMode::ResizableAspectRatio))
    .page(EasingShowcase::new)
    .page(PropertyDemo::new)
    .page(InteractiveDemo::new)
    .run();
}
