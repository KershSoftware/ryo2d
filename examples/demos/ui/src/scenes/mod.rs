//! The demo's pages: the four acceptance pages (a game's menus, settings,
//! inventory and dialogue, all played with the pad), then one feature each

mod art;
mod clipping_demo;
mod dialogue_page;
mod dropdown_demo;
mod inventory_page;
mod layout_demo;
mod menu_data;
mod menu_page;
mod node_menu_page;
mod node_settings_page;
mod scroll_view_demo;
mod settings_data;
mod settings_page;
mod text_demo;
mod text_input_demo;
mod theme_demo;

pub use clipping_demo::ClippingDemo;
pub use dialogue_page::DialoguePage;
pub use dropdown_demo::DropdownDemo;
pub use inventory_page::{EquipSlot, InventoryPage, Item, ItemKind, SlotRef};
pub use layout_demo::LayoutDemo;
pub use menu_page::MenuPage;
pub use node_menu_page::NodeMenuPage;
pub use node_settings_page::NodeSettingsPage;
pub use scroll_view_demo::ScrollViewDemo;
pub use settings_page::SettingsPage;
pub use text_demo::TextDemo;
pub use text_input_demo::TextInputDemo;
pub use theme_demo::ThemeDemo;
