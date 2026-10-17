//! UI Demo - Ryo2D's UI. The first four pages are the acceptance showcase,
//! a game's screens played with the gamepad alone; the rest show one feature
//! each. A library, so its tests drive the pages headlessly.

pub mod scenes;

use demo_kit::prelude::*;
use scenes::{
  ClippingDemo, DialoguePage, DropdownDemo, InventoryPage, LayoutDemo, MenuPage, MixedPage,
  NodeDialoguePage, NodeInventoryPage, NodeMenuPage, NodeSettingsPage, ScrollViewDemo,
  SettingsPage, TextDemo, TextInputDemo, ThemeDemo,
};

/// The demo: Menus, Settings, Inventory, Dialogue, Theme, Layout, Clipping &
/// Modals, Text, Text Input, Scroll Views and Dropdowns, then the node
/// versions of Menus, Settings, Inventory and Dialogue, and the Mixed page
pub fn demo() -> Demo {
  Demo::new("UI")
    .resolution(1024, 768)
    .page(MenuPage::new)
    .page(SettingsPage::new)
    .page(InventoryPage::new)
    .page(DialoguePage::new)
    .page(ThemeDemo::new)
    .page(LayoutDemo::new)
    .page(ClippingDemo::new)
    .page(TextDemo::new)
    .page(TextInputDemo::new)
    .page(ScrollViewDemo::new)
    .page(DropdownDemo::new)
    .page(NodeMenuPage::new)
    .page(NodeSettingsPage::new)
    .page(NodeInventoryPage::new)
    .page(NodeDialoguePage::new)
    .page(MixedPage::new)
}
