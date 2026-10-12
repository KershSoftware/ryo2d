//! UI Demo - Ryo2D's immediate-mode UI, one feature per page

mod scenes;

use demo_kit::prelude::*;
use scenes::{
  ClippingDemo, DropdownDemo, LayoutDemo, ScrollViewDemo, TextDemo, TextInputDemo, ThemeDemo,
};

fn main() {
  Demo::new("UI")
    .resolution(1024, 768)
    .page(ThemeDemo::new)
    .page(LayoutDemo::new)
    .page(ClippingDemo::new)
    .page(TextDemo::new)
    .page(TextInputDemo::new)
    .page(ScrollViewDemo::new)
    .page(DropdownDemo::new)
    .run();
}
