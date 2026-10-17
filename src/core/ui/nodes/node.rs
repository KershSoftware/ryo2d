//! The node tree value: a UI described once, spawned with `ctx.spawn_ui`

use std::fmt;
use std::ops::RangeInclusive;

use glam::Vec2;

use crate::core::asset::SpriteHandle;
use crate::core::ecs::kernel::{Bundle, Component, Entity, EntityWorldMut};
use crate::core::render::batch::Filter;
use crate::core::text::{TextAlign, TextReveal, TextStyle};
use crate::core::ui::widgets::modal::MESSAGE_WIDTH as CONFIRM_MESSAGE_WIDTH;
use crate::core::ui::{Align, Anchor, Color, Edges, Justify, Length, Rect};

use super::components::UiSlot;

/// A theme text role, read when the text is drawn
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextRole {
  /// Running text
  Body,
  /// Titles
  Heading,
  /// Button labels
  Button,
  /// Small print
  Caption,
  /// Fixed-width text
  Mono,
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // a prompt's action is read once prompts are drawn
pub(crate) enum Kind {
  Column,
  Row,
  Grid(u16),
  Stack,
  Panel,
  Scroll(ScrollOpts),
  Popup { anchor: String },
  Text(TextOpts),
  Button(ButtonOpts),
  Image(ImageOpts),
  Checkbox,
  Toggle,
  RadioGroup { horizontal: bool },
  Slider(SliderOpts),
  Progress { label: Option<String> },
  TextInput(TextInputOpts),
  Dropdown,
  Tabs,
  ChoiceList { default: Option<usize> },
  Slot { filter: Option<Filter> },
  Prompt { action: String },
  Rebind(RebindOpts),
  Separator,
  Spacer,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ScrollOpts {
  pub(crate) horizontal: bool,
  pub(crate) both: bool,
  pub(crate) no_bar: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct TextOpts {
  pub(crate) markup: bool,
  pub(crate) style: Option<TextStyle>,
  pub(crate) role: Option<TextRole>,
  pub(crate) size: Option<f32>,
  pub(crate) color: Option<Color>,
  pub(crate) muted: bool,
  pub(crate) align: Option<TextAlign>,
  pub(crate) wrap: Option<f32>,
  pub(crate) max_lines: Option<u32>,
  pub(crate) ellipsis: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ButtonOpts {
  pub(crate) markup: bool,
  pub(crate) icon: Option<SpriteHandle>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ImageOpts {
  pub(crate) size: Option<Vec2>,
  pub(crate) tint: Option<Color>,
  pub(crate) filter: Option<Filter>,
  pub(crate) uv: Option<[f32; 4]>,
}

#[derive(Debug, Clone)]
pub(crate) struct SliderOpts {
  pub(crate) range: (f32, f32),
  pub(crate) step: Option<f32>,
  pub(crate) show_value: bool,
  pub(crate) label: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct TextInputOpts {
  pub(crate) placeholder: Option<String>,
  pub(crate) max_len: Option<usize>,
  pub(crate) password: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct RebindOpts {
  #[allow(dead_code)] // read once rebind rows are drawn
  pub(crate) action: String,
  pub(crate) label: Option<String>,
  pub(crate) slot: Option<usize>,
}

/// The layout options a node was given; `None` leaves the builder's default
#[derive(Debug, Clone, Default)]
pub(crate) struct NodeLayout {
  pub(crate) width: Option<Length>,
  pub(crate) height: Option<Length>,
  pub(crate) min_width: Option<f32>,
  pub(crate) max_width: Option<f32>,
  pub(crate) min_height: Option<f32>,
  pub(crate) max_height: Option<f32>,
  pub(crate) padding: Option<Edges>,
  pub(crate) margin: Option<Edges>,
  pub(crate) gap: Option<f32>,
  /// The theme's `metrics.spacing` as the gap, read when drawn (a confirm)
  pub(crate) theme_gap: bool,
  pub(crate) align: Option<Align>,
  pub(crate) justify: Option<Justify>,
  pub(crate) grow: Option<f32>,
  pub(crate) wrap_items: bool,
  pub(crate) aspect: Option<f32>,
  pub(crate) tracks: Option<Vec<f32>>,
  pub(crate) anchor: Option<Anchor>,
  pub(crate) offset: Option<Vec2>,
  pub(crate) at: Option<Rect>,
  pub(crate) background: Option<Color>,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct NodeFlags {
  pub(crate) focus_scope: bool,
  pub(crate) trap: bool,
  pub(crate) wrap_focus: bool,
  pub(crate) block_pointer: bool,
  pub(crate) default_focus: bool,
  pub(crate) nav_skip: bool,
  pub(crate) focusable: Option<bool>,
  pub(crate) drag_source: bool,
  pub(crate) drop_target: bool,
  pub(crate) modal: bool,
  /// A confirm's No: Back answers as if it were clicked
  pub(crate) answers_back: bool,
}

/// The state components a node spawns with
#[derive(Debug, Clone, Default)]
pub(crate) struct NodeState {
  pub(crate) label: Option<String>,
  pub(crate) image: Option<SpriteHandle>,
  pub(crate) checked: Option<bool>,
  pub(crate) value: Option<f32>,
  pub(crate) index: Option<usize>,
  pub(crate) options: Option<Vec<String>>,
  pub(crate) text: Option<String>,
  pub(crate) slot: Option<UiSlot>,
  pub(crate) reveal: Option<TextReveal>,
}

pub(crate) type Insert = Box<dyn FnOnce(&mut EntityWorldMut<'_>) + Send + Sync>;

/// A UI node and its children, described in the closure API's names.
///
/// Spawn a tree with `ctx.spawn_ui(tree)`: each node becomes an entity, its
/// data the state components it spawns with (`UiLabel`, `UiChecked`,
/// `UiValue`, ...), and its widget reports `UiEvent`s. A reusable component
/// is a function that returns a `UiNode`.
///
/// ```
/// use ryo2d::prelude::*;
///
/// let pause = UiNode::panel()
///   .focus_scope()
///   .anchor(Anchor::Center)
///   .width(260.0)
///   .gap(8.0)
///   .child(UiNode::button("Resume").action("resume").fill_width().default_focus())
///   .child(UiNode::button("Quit").action("quit").fill_width());
/// # let _ = pause;
/// ```
#[must_use = "spawn it with ctx.spawn_ui(..) or add it to a tree with .child(..)"]
pub struct UiNode {
  pub(crate) kind: Kind,
  pub(crate) layout: NodeLayout,
  pub(crate) flags: NodeFlags,
  pub(crate) variant: Option<String>,
  pub(crate) action: Option<String>,
  pub(crate) name: Option<String>,
  pub(crate) tooltip: Option<String>,
  pub(crate) hidden: bool,
  pub(crate) disabled: bool,
  pub(crate) embedded: bool,
  /// Neighbours by node name: up, down, left, right
  pub(crate) nav: [Option<String>; 4],
  pub(crate) state: NodeState,
  pub(crate) children: Vec<UiNode>,
  pub(crate) inserts: Vec<Insert>,
  /// Every option given, by method name, checked against the kind at spawn
  pub(crate) given: Vec<&'static str>,
}

impl fmt::Debug for UiNode {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("UiNode")
      .field("kind", &self.kind.name())
      .field("name", &self.name)
      .field("action", &self.action)
      .field("children", &self.children)
      .field("inserts", &self.inserts.len())
      .finish_non_exhaustive()
  }
}

/// What a spawned node records of its tree position: its kind, layout, flags
/// and the action it reports
#[derive(Component, Debug, Clone)]
pub struct UiNodeKind {
  pub(crate) kind: Kind,
  pub(crate) layout: NodeLayout,
  pub(crate) flags: NodeFlags,
  pub(crate) variant: Option<String>,
  pub(crate) action: Option<String>,
  /// Neighbours up, down, left, right, resolved from the names at spawn
  pub(crate) nav: [Option<Entity>; 4],
  pub(crate) anchor_node: Option<Entity>,
}

impl UiNodeKind {
  /// The constructor's name: `"column"`, `"button"`, ...
  pub fn name(&self) -> &'static str {
    self.kind.name()
  }

  /// The action the node reports, if it has one
  pub fn action(&self) -> Option<&str> {
    self.action.as_deref()
  }
}

impl Kind {
  /// The constructor's name
  pub(crate) fn name(&self) -> &'static str {
    match self {
      Kind::Column => "column",
      Kind::Row => "row",
      Kind::Grid(_) => "grid",
      Kind::Stack => "stack",
      Kind::Panel => "panel",
      Kind::Scroll(_) => "scroll",
      Kind::Popup { .. } => "popup",
      Kind::Text(_) => "text",
      Kind::Button(_) => "button",
      Kind::Image(_) => "image",
      Kind::Checkbox => "checkbox",
      Kind::Toggle => "toggle",
      Kind::RadioGroup { .. } => "radio_group",
      Kind::Slider(_) => "slider",
      Kind::Progress { .. } => "progress",
      Kind::TextInput(_) => "text_input",
      Kind::Dropdown => "dropdown",
      Kind::Tabs => "tabs",
      Kind::ChoiceList { .. } => "choice_list",
      Kind::Slot { .. } => "slot",
      Kind::Prompt { .. } => "prompt",
      Kind::Rebind(_) => "rebind",
      Kind::Separator => "separator",
      Kind::Spacer => "spacer",
    }
  }

  /// Whether the kind holds children
  pub(crate) fn is_container(&self) -> bool {
    matches!(
      self,
      Kind::Column
        | Kind::Row
        | Kind::Grid(_)
        | Kind::Stack
        | Kind::Panel
        | Kind::Scroll(_)
        | Kind::Popup { .. }
    )
  }

  /// Whether the kind takes the option (a method name, as in `given`)
  pub(crate) fn takes(&self, option: &str) -> bool {
    const SIZE: &str =
      "width height fill_width fill_height fill min_width max_width min_height max_height";
    let listed = |list: &str| list.split_whitespace().any(|name| name == option);
    match self {
      Kind::Column | Kind::Row | Kind::Grid(_) | Kind::Stack | Kind::Panel | Kind::Popup { .. } => {
        listed(SIZE)
          || listed(
            "padding margin gap align justify center grow wrap_items aspect anchor offset at \
             background focus_scope trap wrap_focus block_pointer drag_source drop_target modal \
             child embedded",
          )
          || (matches!(self, Kind::Grid(_)) && option == "tracks")
      }
      Kind::Scroll(_) => {
        listed(SIZE)
          || listed(
            "padding margin gap grow anchor offset at drag_source drop_target horizontal both \
             no_bar child embedded",
          )
      }
      Kind::RadioGroup { .. } => listed(
        "width height fill_width min_width max_width margin gap align justify grow anchor offset \
         at enabled horizontal embedded",
      ),
      Kind::Text(_) => listed(
        "embedded markup style role heading size color muted text_align wrap max_lines ellipsis \
         typewriter",
      ),
      Kind::Separator | Kind::Spacer => option == "embedded",
      widget => {
        listed(SIZE)
          || listed(
            "margin anchor offset grow at default_focus nav_up nav_down nav_left nav_right \
             nav_skip focusable variant tooltip enabled drag_source drop_target embedded",
          )
          || match widget {
            Kind::Button(_) => matches!(option, "markup" | "icon"),
            Kind::Image(_) => matches!(option, "image_size" | "tint" | "filter" | "uv"),
            Kind::Slider(_) => matches!(option, "step" | "show_value" | "label"),
            Kind::Progress { .. } => option == "label",
            Kind::TextInput(_) => matches!(option, "placeholder" | "max_len" | "password"),
            Kind::ChoiceList { .. } => option == "default_choice",
            Kind::Slot { .. } => matches!(option, "icon" | "filter" | "count" | "selected"),
            Kind::Rebind(_) => matches!(option, "label" | "binding_slot"),
            _ => false,
          }
      }
    }
  }
}

impl UiNode {
  fn of(kind: Kind) -> UiNode {
    UiNode {
      kind,
      layout: NodeLayout::default(),
      flags: NodeFlags::default(),
      variant: None,
      action: None,
      name: None,
      tooltip: None,
      hidden: false,
      disabled: false,
      embedded: false,
      nav: [None, None, None, None],
      state: NodeState::default(),
      children: Vec::new(),
      inserts: Vec::new(),
      given: Vec::new(),
    }
  }

  fn with_state(kind: Kind, state: NodeState) -> UiNode {
    UiNode {
      state,
      ..UiNode::of(kind)
    }
  }

  fn options(kind: Kind, options: &[&str], index: usize) -> UiNode {
    let options = options.iter().map(|option| option.to_string()).collect();
    UiNode::with_state(
      kind,
      NodeState {
        options: Some(options),
        index: Some(index),
        ..NodeState::default()
      },
    )
  }

  /// A vertical stack of children
  pub fn column() -> UiNode {
    UiNode::of(Kind::Column)
  }

  /// A horizontal row of children
  pub fn row() -> UiNode {
    UiNode::of(Kind::Row)
  }

  /// Children in a grid of `columns` columns
  pub fn grid(columns: u16) -> UiNode {
    UiNode::of(Kind::Grid(columns))
  }

  /// Children drawn on top of one another
  pub fn stack() -> UiNode {
    UiNode::of(Kind::Stack)
  }

  /// A themed panel holding children
  pub fn panel() -> UiNode {
    UiNode::of(Kind::Panel)
  }

  /// A scrolling area holding children
  pub fn scroll() -> UiNode {
    UiNode::of(Kind::Scroll(ScrollOpts::default()))
  }

  /// A popup opened below the node named `anchor`
  pub fn popup(anchor: impl Into<String>) -> UiNode {
    UiNode::of(Kind::Popup {
      anchor: anchor.into(),
    })
  }

  /// A line or block of text
  pub fn text(text: impl Into<String>) -> UiNode {
    let state = NodeState {
      label: Some(text.into()),
      ..NodeState::default()
    };
    UiNode::with_state(Kind::Text(TextOpts::default()), state)
  }

  /// A button labelled `label`
  pub fn button(label: impl Into<String>) -> UiNode {
    let state = NodeState {
      label: Some(label.into()),
      ..NodeState::default()
    };
    UiNode::with_state(Kind::Button(ButtonOpts::default()), state)
  }

  /// A sprite
  pub fn image(sprite: SpriteHandle) -> UiNode {
    let state = NodeState {
      image: Some(sprite),
      ..NodeState::default()
    };
    UiNode::with_state(Kind::Image(ImageOpts::default()), state)
  }

  /// A checkbox with a label
  pub fn checkbox(label: impl Into<String>, checked: bool) -> UiNode {
    let state = NodeState {
      label: Some(label.into()),
      checked: Some(checked),
      ..NodeState::default()
    };
    UiNode::with_state(Kind::Checkbox, state)
  }

  /// A toggle switch with a label
  pub fn toggle(label: impl Into<String>, on: bool) -> UiNode {
    let state = NodeState {
      label: Some(label.into()),
      checked: Some(on),
      ..NodeState::default()
    };
    UiNode::with_state(Kind::Toggle, state)
  }

  /// Radio buttons, one per option
  pub fn radio_group(options: &[&str], selected: usize) -> UiNode {
    UiNode::options(Kind::RadioGroup { horizontal: false }, options, selected)
  }

  /// A slider over `range`
  pub fn slider(range: RangeInclusive<f32>, value: f32) -> UiNode {
    let opts = SliderOpts {
      range: (*range.start(), *range.end()),
      step: None,
      show_value: false,
      label: None,
    };
    UiNode::with_state(
      Kind::Slider(opts),
      NodeState {
        value: Some(value),
        ..NodeState::default()
      },
    )
  }

  /// A progress bar filled to `fraction` (0 to 1)
  pub fn progress(fraction: f32) -> UiNode {
    UiNode::with_state(
      Kind::Progress { label: None },
      NodeState {
        value: Some(fraction),
        ..NodeState::default()
      },
    )
  }

  /// A single-line text field
  pub fn text_input(text: impl Into<String>) -> UiNode {
    let state = NodeState {
      text: Some(text.into()),
      ..NodeState::default()
    };
    UiNode::with_state(Kind::TextInput(TextInputOpts::default()), state)
  }

  /// A dropdown of options
  pub fn dropdown(options: &[&str], selected: usize) -> UiNode {
    UiNode::options(Kind::Dropdown, options, selected)
  }

  /// A row of tabs
  pub fn tabs(options: &[&str], selected: usize) -> UiNode {
    UiNode::options(Kind::Tabs, options, selected)
  }

  /// A list of options, the first highlighted. Picking one sends `Clicked`
  /// and writes the choice to its `UiIndex`. The builder gives a choice list
  /// no `Response`, so it has no `ui_id` and its `UiInteraction` is never
  /// updated.
  pub fn choice_list(options: &[&str]) -> UiNode {
    UiNode::options(Kind::ChoiceList { default: None }, options, 0)
  }

  /// An inventory-style slot
  pub fn slot() -> UiNode {
    UiNode::with_state(
      Kind::Slot { filter: None },
      NodeState {
        slot: Some(UiSlot::default()),
        ..NodeState::default()
      },
    )
  }

  /// The glyph for the input bound to `action`
  pub fn prompt(action: &str) -> UiNode {
    UiNode::of(Kind::Prompt {
      action: action.to_string(),
    })
  }

  /// A button that rebinds the input bound to `action`
  pub fn rebind(action: &str) -> UiNode {
    UiNode::of(Kind::Rebind(RebindOpts {
      action: action.to_string(),
      label: None,
      slot: None,
    }))
  }

  /// A dividing line
  pub fn separator() -> UiNode {
    UiNode::of(Kind::Separator)
  }

  /// Empty space that takes what its parent has left
  pub fn spacer() -> UiNode {
    UiNode::of(Kind::Spacer)
  }

  /// A yes/no dialog, as `ui.confirm_with` draws it: a modal panel with
  /// `title`, `message` (left out when empty) and buttons whose actions are
  /// `"yes"` and `"no"`. It opens on No, and Back answers No: a `Clicked`
  /// from the No button.
  pub fn confirm(title: &str, message: &str, yes: &str, no: &str) -> UiNode {
    let mut buttons = UiNode::row().justify(Justify::End).theme_gap();
    buttons = buttons.child(UiNode::button(yes).action("yes"));
    let mut no = UiNode::button(no).action("no").default_focus();
    no.flags.answers_back = true;
    buttons = buttons.child(no);
    let mut dialog = UiNode::panel()
      .modal()
      .theme_gap()
      .child(UiNode::text(title).heading());
    if !message.is_empty() {
      dialog = dialog.child(
        UiNode::text(message)
          .role(TextRole::Body)
          .wrap(CONFIRM_MESSAGE_WIDTH),
      );
    }
    dialog.child(buttons)
  }

  /// Reads the theme's spacing as the gap when drawn
  pub(crate) fn theme_gap(mut self) -> UiNode {
    self.layout.theme_gap = true;
    self
  }

  /// Records `option` as given; the caller applies it
  fn given(mut self, option: &'static str) -> UiNode {
    self.given.push(option);
    self
  }

  // Shared layout

  /// The width: pixels (`240.0`) or a `Length`
  pub fn width(mut self, width: impl Into<Length>) -> UiNode {
    self.layout.width = Some(width.into());
    self.given("width")
  }

  /// The height: pixels (`40.0`) or a `Length`
  pub fn height(mut self, height: impl Into<Length>) -> UiNode {
    self.layout.height = Some(height.into());
    self.given("height")
  }

  /// Take the rest of the parent's width
  pub fn fill_width(mut self) -> UiNode {
    self.layout.width = Some(Length::Fill);
    self.given("fill_width")
  }

  /// Take the rest of the parent's height
  pub fn fill_height(mut self) -> UiNode {
    self.layout.height = Some(Length::Fill);
    self.given("fill_height")
  }

  /// Take the rest of the parent in both directions
  pub fn fill(mut self) -> UiNode {
    self.layout.width = Some(Length::Fill);
    self.layout.height = Some(Length::Fill);
    self.given("fill")
  }

  /// The least width
  pub fn min_width(mut self, width: f32) -> UiNode {
    self.layout.min_width = Some(width);
    self.given("min_width")
  }

  /// The most width
  pub fn max_width(mut self, width: f32) -> UiNode {
    self.layout.max_width = Some(width);
    self.given("max_width")
  }

  /// The least height
  pub fn min_height(mut self, height: f32) -> UiNode {
    self.layout.min_height = Some(height);
    self.given("min_height")
  }

  /// The most height
  pub fn max_height(mut self, height: f32) -> UiNode {
    self.layout.max_height = Some(height);
    self.given("max_height")
  }

  /// Space inside the node: one value, `(x, y)` or `Edges`
  pub fn padding(mut self, padding: impl Into<Edges>) -> UiNode {
    self.layout.padding = Some(padding.into());
    self.given("padding")
  }

  /// Space around the node: one value, `(x, y)` or `Edges`
  pub fn margin(mut self, margin: impl Into<Edges>) -> UiNode {
    self.layout.margin = Some(margin.into());
    self.given("margin")
  }

  /// Space between children
  pub fn gap(mut self, gap: f32) -> UiNode {
    self.layout.gap = Some(gap);
    self.given("gap")
  }

  /// Where children sit across the main axis
  pub fn align(mut self, align: Align) -> UiNode {
    self.layout.align = Some(align);
    self.given("align")
  }

  /// Where children sit along the main axis
  pub fn justify(mut self, justify: Justify) -> UiNode {
    self.layout.justify = Some(justify);
    self.given("justify")
  }

  /// Centre children on both axes
  pub fn center(mut self) -> UiNode {
    self.layout.align = Some(Align::Center);
    self.layout.justify = Some(Justify::Center);
    self.given("center")
  }

  /// Share of the parent's leftover main-axis space
  pub fn grow(mut self, grow: f32) -> UiNode {
    self.layout.grow = Some(grow);
    self.given("grow")
  }

  /// Wrap children onto further lines
  pub fn wrap_items(mut self) -> UiNode {
    self.layout.wrap_items = true;
    self.given("wrap_items")
  }

  /// Keep width / height at `ratio`
  pub fn aspect(mut self, ratio: f32) -> UiNode {
    self.layout.aspect = Some(ratio);
    self.given("aspect")
  }

  /// A grid's column weights
  pub fn tracks(mut self, tracks: &[f32]) -> UiNode {
    self.layout.tracks = Some(tracks.to_vec());
    self.given("tracks")
  }

  /// Place the node against a point of the screen (or of its popup anchor)
  pub fn anchor(mut self, anchor: Anchor) -> UiNode {
    self.layout.anchor = Some(anchor);
    self.given("anchor")
  }

  /// Shift the node from its anchor
  pub fn offset(mut self, x: f32, y: f32) -> UiNode {
    self.layout.offset = Some(Vec2::new(x, y));
    self.given("offset")
  }

  /// Place the node in an exact rectangle
  pub fn at(mut self, rect: Rect) -> UiNode {
    self.layout.at = Some(rect);
    self.given("at")
  }

  /// Fill the node with a colour
  pub fn background(mut self, color: Color) -> UiNode {
    self.layout.background = Some(color);
    self.given("background")
  }

  // Focus and drag

  /// Keyboard and gamepad focus moves within this node's subtree
  pub fn focus_scope(mut self) -> UiNode {
    self.flags.focus_scope = true;
    self.given("focus_scope")
  }

  /// A focus scope that focus cannot leave
  pub fn trap(mut self) -> UiNode {
    self.flags.focus_scope = true;
    self.flags.trap = true;
    self.given("trap")
  }

  /// A focus scope whose focus wraps around its ends
  pub fn wrap_focus(mut self) -> UiNode {
    self.flags.focus_scope = true;
    self.flags.wrap_focus = true;
    self.given("wrap_focus")
  }

  /// The pointer does not reach what is behind the node
  pub fn block_pointer(mut self) -> UiNode {
    self.flags.block_pointer = true;
    self.given("block_pointer")
  }

  /// The node takes focus when its scope opens
  pub fn default_focus(mut self) -> UiNode {
    self.flags.default_focus = true;
    self.given("default_focus")
  }

  /// Focus navigation passes over the node
  pub fn nav_skip(mut self) -> UiNode {
    self.flags.nav_skip = true;
    self.given("nav_skip")
  }

  /// Force the node focusable or not
  pub fn focusable(mut self, focusable: bool) -> UiNode {
    self.flags.focusable = Some(focusable);
    self.given("focusable")
  }

  /// The node can be dragged
  pub fn drag_source(mut self) -> UiNode {
    self.flags.drag_source = true;
    self.given("drag_source")
  }

  /// The node accepts drops
  pub fn drop_target(mut self) -> UiNode {
    self.flags.drop_target = true;
    self.given("drop_target")
  }

  /// A modal: it dims and blocks what is behind it
  pub fn modal(mut self) -> UiNode {
    self.flags.modal = true;
    self.given("modal")
  }

  /// The node named `name` is where focus goes on Up
  pub fn nav_up(mut self, name: impl Into<String>) -> UiNode {
    self.nav[0] = Some(name.into());
    self.given("nav_up")
  }

  /// The node named `name` is where focus goes on Down
  pub fn nav_down(mut self, name: impl Into<String>) -> UiNode {
    self.nav[1] = Some(name.into());
    self.given("nav_down")
  }

  /// The node named `name` is where focus goes on Left
  pub fn nav_left(mut self, name: impl Into<String>) -> UiNode {
    self.nav[2] = Some(name.into());
    self.given("nav_left")
  }

  /// The node named `name` is where focus goes on Right
  pub fn nav_right(mut self, name: impl Into<String>) -> UiNode {
    self.nav[3] = Some(name.into());
    self.given("nav_right")
  }

  // Other shared

  /// A tooltip shown on hover or focus
  pub fn tooltip(mut self, tooltip: impl Into<String>) -> UiNode {
    self.tooltip = Some(tooltip.into());
    self.given("tooltip")
  }

  /// Whether the node is interactive (`false` greys it out)
  pub fn enabled(mut self, enabled: bool) -> UiNode {
    self.disabled = !enabled;
    self.given("enabled")
  }

  /// A theme variant by name, such as `"danger"`
  pub fn variant(mut self, variant: &str) -> UiNode {
    self.variant = Some(variant.to_string());
    self.given("variant")
  }

  // Node-only

  /// The action the node reports in its `UiEvent`s
  pub fn action(mut self, action: impl Into<String>) -> UiNode {
    self.action = Some(action.into());
    self
  }

  /// The node's name: in its `UiEvent`s, `UiName`, and for `nav_*` and popups
  pub fn name(mut self, name: impl Into<String>) -> UiNode {
    self.name = Some(name.into());
    self
  }

  /// Spawn the node hidden
  pub fn hidden(mut self) -> UiNode {
    self.hidden = true;
    self
  }

  /// Draw the tree inside the scene's own `ui` (at the position the game
  /// draws it) instead of as a screen-level tree
  pub fn embedded(mut self) -> UiNode {
    self.embedded = true;
    self.given("embedded")
  }

  /// Add a component or bundle to the node's entity when it spawns
  pub fn insert(mut self, bundle: impl Bundle) -> UiNode {
    self
      .inserts
      .push(Box::new(move |entity: &mut EntityWorldMut<'_>| {
        entity.insert(bundle);
      }));
    self
  }

  /// Add a child
  pub fn child(mut self, child: UiNode) -> UiNode {
    self.children.push(child);
    self.given("child")
  }

  /// Add children
  pub fn children(mut self, children: impl IntoIterator<Item = UiNode>) -> UiNode {
    let before = self.children.len();
    self.children.extend(children);
    if self.children.len() > before {
      self.given.push("child");
    }
    self
  }

  // Kind options

  /// Parse the text as markup (text, button)
  pub fn markup(mut self) -> UiNode {
    match &mut self.kind {
      Kind::Text(opts) => opts.markup = true,
      Kind::Button(opts) => opts.markup = true,
      _ => {}
    }
    self.given("markup")
  }

  /// A full text style (text)
  pub fn style(mut self, style: TextStyle) -> UiNode {
    if let Kind::Text(opts) = &mut self.kind {
      opts.style = Some(style);
    }
    self.given("style")
  }

  /// A theme text role (text)
  pub fn role(mut self, role: TextRole) -> UiNode {
    if let Kind::Text(opts) = &mut self.kind {
      opts.role = Some(role);
    }
    self.given("role")
  }

  /// The heading role (text)
  pub fn heading(mut self) -> UiNode {
    if let Kind::Text(opts) = &mut self.kind {
      opts.role = Some(TextRole::Heading);
    }
    self.given("heading")
  }

  /// The font size (text)
  pub fn size(mut self, size: f32) -> UiNode {
    if let Kind::Text(opts) = &mut self.kind {
      opts.size = Some(size);
    }
    self.given("size")
  }

  /// The text colour (text)
  pub fn color(mut self, color: Color) -> UiNode {
    if let Kind::Text(opts) = &mut self.kind {
      opts.color = Some(color);
    }
    self.given("color")
  }

  /// The theme's muted text colour (text)
  pub fn muted(mut self) -> UiNode {
    if let Kind::Text(opts) = &mut self.kind {
      opts.muted = true;
    }
    self.given("muted")
  }

  /// How lines align within the text's width (text)
  pub fn text_align(mut self, align: TextAlign) -> UiNode {
    if let Kind::Text(opts) = &mut self.kind {
      opts.align = Some(align);
    }
    self.given("text_align")
  }

  /// Wrap lines at `width` (text)
  pub fn wrap(mut self, width: f32) -> UiNode {
    if let Kind::Text(opts) = &mut self.kind {
      opts.wrap = Some(width);
    }
    self.given("wrap")
  }

  /// Show at most `lines` lines (text)
  pub fn max_lines(mut self, lines: u32) -> UiNode {
    if let Kind::Text(opts) = &mut self.kind {
      opts.max_lines = Some(lines);
    }
    self.given("max_lines")
  }

  /// End clipped text with an ellipsis (text)
  pub fn ellipsis(mut self) -> UiNode {
    if let Kind::Text(opts) = &mut self.kind {
      opts.ellipsis = true;
    }
    self.given("ellipsis")
  }

  /// Reveal the text over time (text)
  pub fn typewriter(mut self, reveal: TextReveal) -> UiNode {
    if matches!(self.kind, Kind::Text(_)) {
      self.state.reveal = Some(reveal);
    }
    self.given("typewriter")
  }

  /// An icon beside the label (button), or in the slot (slot)
  pub fn icon(mut self, icon: SpriteHandle) -> UiNode {
    match &mut self.kind {
      Kind::Button(opts) => opts.icon = Some(icon),
      Kind::Slot { .. } => {
        if let Some(slot) = &mut self.state.slot {
          slot.icon = Some(icon);
        }
      }
      _ => {}
    }
    self.given("icon")
  }

  /// Draw the image at this size (image)
  pub fn image_size(mut self, width: f32, height: f32) -> UiNode {
    if let Kind::Image(opts) = &mut self.kind {
      opts.size = Some(Vec2::new(width, height));
    }
    self.given("image_size")
  }

  /// Multiply the image by a colour (image)
  pub fn tint(mut self, tint: Color) -> UiNode {
    if let Kind::Image(opts) = &mut self.kind {
      opts.tint = Some(tint);
    }
    self.given("tint")
  }

  /// How the sprite is sampled (image, slot)
  pub fn filter(mut self, filter: Filter) -> UiNode {
    match &mut self.kind {
      Kind::Image(opts) => opts.filter = Some(filter),
      Kind::Slot { filter: slot } => *slot = Some(filter),
      _ => {}
    }
    self.given("filter")
  }

  /// The part of the sprite to draw, as `[u0, v0, u1, v1]` (image)
  pub fn uv(mut self, uv: [f32; 4]) -> UiNode {
    if let Kind::Image(opts) = &mut self.kind {
      opts.uv = Some(uv);
    }
    self.given("uv")
  }

  /// Snap the value to multiples of `step` (slider)
  pub fn step(mut self, step: f32) -> UiNode {
    if let Kind::Slider(opts) = &mut self.kind {
      opts.step = Some(step);
    }
    self.given("step")
  }

  /// Show the value beside the slider (slider)
  pub fn show_value(mut self) -> UiNode {
    if let Kind::Slider(opts) = &mut self.kind {
      opts.show_value = true;
    }
    self.given("show_value")
  }

  /// A label (slider, progress, rebind)
  pub fn label(mut self, label: impl Into<String>) -> UiNode {
    let label = label.into();
    match &mut self.kind {
      Kind::Slider(opts) => opts.label = Some(label),
      Kind::Progress { label: slot } => *slot = Some(label),
      Kind::Rebind(opts) => opts.label = Some(label),
      _ => {}
    }
    self.given("label")
  }

  /// Text shown while the field is empty (text input)
  pub fn placeholder(mut self, placeholder: impl Into<String>) -> UiNode {
    if let Kind::TextInput(opts) = &mut self.kind {
      opts.placeholder = Some(placeholder.into());
    }
    self.given("placeholder")
  }

  /// The most characters the field takes (text input)
  pub fn max_len(mut self, len: usize) -> UiNode {
    if let Kind::TextInput(opts) = &mut self.kind {
      opts.max_len = Some(len);
    }
    self.given("max_len")
  }

  /// Mask the text (text input)
  pub fn password(mut self) -> UiNode {
    if let Kind::TextInput(opts) = &mut self.kind {
      opts.password = true;
    }
    self.given("password")
  }

  /// Lay out horizontally (radio group, scroll)
  pub fn horizontal(mut self) -> UiNode {
    match &mut self.kind {
      Kind::RadioGroup { horizontal } => *horizontal = true,
      Kind::Scroll(opts) => opts.horizontal = true,
      _ => {}
    }
    self.given("horizontal")
  }

  /// Scroll in both directions (scroll)
  pub fn both(mut self) -> UiNode {
    if let Kind::Scroll(opts) = &mut self.kind {
      opts.both = true;
    }
    self.given("both")
  }

  /// Hide the scroll bar (scroll)
  pub fn no_bar(mut self) -> UiNode {
    if let Kind::Scroll(opts) = &mut self.kind {
      opts.no_bar = true;
    }
    self.given("no_bar")
  }

  /// The option the list highlights first (choice list)
  pub fn default_choice(mut self, index: usize) -> UiNode {
    if let Kind::ChoiceList { default } = &mut self.kind {
      *default = Some(index);
    }
    self.given("default_choice")
  }

  /// The stack count (slot)
  pub fn count(mut self, count: u32) -> UiNode {
    if let Some(slot) = self
      .state
      .slot
      .as_mut()
      .filter(|_| matches!(self.kind, Kind::Slot { .. }))
    {
      slot.count = count;
    }
    self.given("count")
  }

  /// Draw the slot selected (slot)
  pub fn selected(mut self, selected: bool) -> UiNode {
    if let Some(slot) = self
      .state
      .slot
      .as_mut()
      .filter(|_| matches!(self.kind, Kind::Slot { .. }))
    {
      slot.selected = selected;
    }
    self.given("selected")
  }

  /// Which binding slot the button rebinds (rebind)
  pub fn binding_slot(mut self, slot: usize) -> UiNode {
    if let Kind::Rebind(opts) = &mut self.kind {
      opts.slot = Some(slot);
    }
    self.given("binding_slot")
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_tree_keeps_its_children_in_order_and_its_options() {
    let tree = UiNode::column()
      .gap(8.0)
      .focus_scope()
      .child(UiNode::button("Resume").action("resume").default_focus())
      .child(UiNode::button("Quit").action("quit").name("quit"));
    assert_eq!(tree.kind.name(), "column");
    assert_eq!(tree.layout.gap, Some(8.0));
    assert!(tree.flags.focus_scope);
    let labels: Vec<_> = tree
      .children
      .iter()
      .map(|c| c.state.label.clone())
      .collect();
    assert_eq!(
      labels,
      [Some("Resume".to_string()), Some("Quit".to_string())]
    );
    assert_eq!(tree.children[0].action.as_deref(), Some("resume"));
    assert!(tree.children[0].flags.default_focus);
    assert_eq!(tree.children[1].name.as_deref(), Some("quit"));
  }

  #[test]
  fn a_kind_option_on_another_kind_is_recorded_and_not_applied() {
    let node = UiNode::button("A").step(0.1).gap(4.0);
    assert!(node.given.contains(&"step"));
    assert!(!node.kind.takes("step"), "a button takes no step");
    assert!(!node.kind.takes("gap"), "nor a gap");
    let slider = UiNode::slider(0.0..=1.0, 0.5).step(0.1);
    assert!(
      matches!(slider.kind, Kind::Slider(SliderOpts { step: Some(s), .. }) if (s - 0.1).abs() < 1e-6)
    );
  }

  #[test]
  fn constructors_spawn_their_state() {
    assert_eq!(UiNode::checkbox("Mute", true).state.checked, Some(true));
    assert_eq!(UiNode::slider(0.0..=2.0, 1.5).state.value, Some(1.5));
    let dropdown = UiNode::dropdown(&["A", "B"], 1);
    assert_eq!(
      dropdown.state.options,
      Some(vec!["A".to_string(), "B".to_string()])
    );
    assert_eq!(dropdown.state.index, Some(1));
    assert_eq!(UiNode::choice_list(&["X"]).state.index, Some(0));
    assert!(UiNode::slot().state.slot.is_some());
    assert_eq!(UiNode::text_input("hi").state.text.as_deref(), Some("hi"));
  }

  #[test]
  fn a_confirm_is_a_modal_panel_that_opens_on_no() {
    let confirm = UiNode::confirm("Quit?", "", "Yes", "No");
    assert!(confirm.flags.modal);
    assert_eq!(confirm.children.len(), 2, "no message, no message line");
    let buttons = &confirm.children[1].children;
    assert_eq!(buttons[0].action.as_deref(), Some("yes"));
    assert_eq!(buttons[1].action.as_deref(), Some("no"));
    assert!(buttons[1].flags.default_focus && buttons[1].flags.answers_back);
  }
}
