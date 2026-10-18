//! UI Demo - Ryo2D's UI. The first four pages are the acceptance showcase,
//! a game's screens played with the gamepad alone, and the next seven show
//! one feature each. The last five are the four showcase pages built from
//! node trees, and a closure screen around an embedded node grid. A library,
//! so its tests drive the pages headlessly.

pub mod scenes;

use demo_kit::prelude::*;
use scenes::{
  ClippingDemo, DialoguePage, DropdownDemo, InventoryPage, LayoutDemo, MenuPage, MixedPage,
  NodeDialoguePage, NodeInventoryPage, NodeMenuPage, NodeSettingsPage, ScrollViewDemo,
  SettingsPage, TextDemo, TextInputDemo, ThemeDemo,
};

/// The demo's sixteen pages: Menus, Settings, Inventory, Dialogue, Theme,
/// Layout, Clipping & Modals, Text, Text Input, Scroll Views and Dropdowns,
/// then Node Menus, Node Settings, Node Inventory, Node Dialogue and Mixed
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
