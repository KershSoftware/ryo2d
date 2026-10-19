//! Widgets: builders that record a node, its draws and its record, and
//! report a [`Response`](super::Response)

mod button;
mod checkbox;
mod choice_list;
pub(crate) mod common;
mod custom;
pub(crate) mod drag;
pub(crate) mod dropdown;
mod image;
pub(crate) mod list;
mod mark_row;
pub(super) mod modal;
mod progress;
pub(crate) mod prompt;
mod radio;
pub(crate) mod rebind;
pub(crate) mod scroll;
mod separator;
mod slider;
mod slot;
mod tabs;
pub(crate) mod text_edit;
mod text_input;
mod toggle;
mod tooltip;

pub use button::Button;
pub use checkbox::Checkbox;
pub use choice_list::ChoiceList;
pub use custom::{Custom, Painter};
pub use dropdown::Dropdown;
pub use image::Image;
pub use list::{List, ListResponse};
pub use modal::Modal;
pub use progress::Progress;
pub use prompt::Prompt;
pub use radio::Radio;
pub use rebind::Rebind;
pub use scroll::Scroll;
pub use slider::Slider;
pub use slot::Slot;
pub use tabs::Tabs;
pub use text_input::TextInput;
pub use toggle::Toggle;

/// A widget state kept in `UiMemory` in `Debug` form (`?` for a type no
/// widget keeps): what the test harnesses' UI snapshots compare
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn state_debug(value: &dyn std::any::Any) -> String {
  fn as_debug<T: std::fmt::Debug + 'static>(value: &dyn std::any::Any) -> Option<String> {
    value.downcast_ref::<T>().map(|value| format!("{value:?}"))
  }
  as_debug::<scroll::ScrollState>(value)
    .or_else(|| as_debug::<list::ListState>(value))
    .or_else(|| as_debug::<text_edit::TextEdit>(value))
    .or_else(|| as_debug::<dropdown::DropdownMemory>(value))
    .or_else(|| as_debug::<rebind::RebindState>(value))
    .or_else(|| as_debug::<tooltip::TooltipTimer>(value))
    .or_else(|| as_debug::<u32>(value))
    .unwrap_or_else(|| "?".to_string())
}
