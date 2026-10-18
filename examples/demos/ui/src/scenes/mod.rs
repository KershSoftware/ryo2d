//! The demo's pages: the four acceptance pages (a game's menus, settings,
//! inventory and dialogue, all played with the pad), one feature each, then
//! the acceptance pages built from node trees and UI files and the Mixed page

mod art;
mod clipping_demo;
mod dialogue_data;
mod dialogue_page;
mod dropdown_demo;
mod file_dialogue_page;
mod file_inventory_page;
mod file_menu_page;
mod file_settings_page;
mod inventory_model;
mod inventory_page;
mod layout_demo;
mod menu_data;
mod menu_page;
mod mixed_page;
mod scroll_view_demo;
mod settings_data;
mod settings_page;
mod text_demo;
mod text_input_demo;
mod theme_demo;

pub use clipping_demo::ClippingDemo;
pub use dialogue_page::DialoguePage;
pub use dropdown_demo::DropdownDemo;
pub use file_dialogue_page::FileDialoguePage;
pub use file_inventory_page::FileInventoryPage;
pub use file_menu_page::FileMenuPage;
pub use file_settings_page::FileSettingsPage;
pub use inventory_model::{EquipSlot, Item, ItemKind, SlotRef};
pub use inventory_page::InventoryPage;
pub use layout_demo::LayoutDemo;
pub use menu_page::MenuPage;
pub use mixed_page::MixedPage;
pub use scroll_view_demo::ScrollViewDemo;
pub use settings_page::SettingsPage;
pub use text_demo::TextDemo;
pub use text_input_demo::TextInputDemo;
pub use theme_demo::ThemeDemo;
