//! What every widget builder shares: its options and the builder methods
//! that set them (`widget_builders!`)

use crate::core::text::RichText;
use crate::core::ui::layout::UiStyle;
use crate::core::ui::memory::Flags;
use crate::core::ui::theme::LookPatch;
use crate::core::ui::{Rect, UiId};

/// A widget's shared options, set by the `widget_builders!` methods
#[derive(Debug, Clone, Default)]
pub(crate) struct WidgetOpts {
  /// Its id key (`.id(key)`), in place of how often its line ran
  pub(crate) key: Option<u64>,
  /// `.enabled(false)`
  pub(crate) disabled: bool,
  /// Its size and place in the layout
  pub(crate) style: UiStyle,
  /// Placed at this rect, outside any container
  pub(crate) at: Option<Rect>,
  /// `.default_focus()`: its scope focuses it first
  pub(crate) default_focus: bool,
  /// `.nav_up(..)` and the rest: its Up, Down, Left, Right neighbours (`NONE`)
  pub(crate) nav: [UiId; 4],
  /// `.nav_skip()`: directions pass it by
  pub(crate) nav_skip: bool,
  /// `.focusable(..)`, when set: whether it takes focus, whatever its kind
  pub(crate) focusable: Option<bool>,
  /// `.variant(name)`: the theme variant it is drawn in
  pub(crate) variant: Option<String>,
  /// `.style(patch)`: what this one widget changes in its look
  pub(crate) look_patch: Option<LookPatch>,
  /// `.tooltip(text)`: shown after the pointer or focus rests on it
  pub(crate) tooltip: Option<RichText>,
}

impl WidgetOpts {
  /// `flags` with the focus options: `FOCUSABLE` when the widget takes
  /// focus (`focusable` by default for its kind, unless `.focusable(..)`
  /// said otherwise), `DEFAULT_FOCUS`, `NAV_SKIP` and `DISABLED`
  pub(crate) fn flags(&self, mut flags: Flags, focusable: bool) -> Flags {
    if self.focusable.unwrap_or(focusable) {
      flags |= Flags::FOCUSABLE;
    }
    if self.default_focus {
      flags |= Flags::DEFAULT_FOCUS;
    }
    if self.nav_skip {
      flags |= Flags::NAV_SKIP;
    }
    if self.disabled {
      flags |= Flags::DISABLED;
    }
    flags
  }
}

/// The builder methods every widget shares, for a widget type with an
/// `opts: WidgetOpts` field: `id`, `enabled`, `at`, `width`, `height`,
/// `fill_width`, `margin`, `anchor`, `offset`, `grow`, `min_width`,
/// `max_width`, and for focus `default_focus`, `nav_up`, `nav_down`,
/// `nav_left`, `nav_right`, `nav_skip` and `focusable`, and for looks
/// `variant` and `style`, and `tooltip`. A widget with type parameters passes
/// them first: `widget_builders!([T: Copy] Radio<'_, '_, T>)`.
macro_rules! widget_builders {
  ([$($generics:tt)*] $widget:ty) => {
    impl<$($generics)*> $widget {
      /// Its id from `key` instead of how often this line ran: for widgets
      /// made in a loop over items that can reorder or be filtered
      pub fn id(mut self, key: impl ::std::hash::Hash) -> Self {
        self.opts.key = Some($crate::core::ui::id::key_hash(&key));
        self
      }

      /// Whether it takes input (default `true`). A disabled widget is drawn
      /// dimmed, reports nothing, and still blocks the pointer from what is
      /// under it
      pub fn enabled(mut self, enabled: bool) -> Self {
        self.opts.disabled = !enabled;
        self
      }

      /// Placed at `rect`, its size too, out of the layout: inside a
      /// container it takes no room there (and stays in its focus scope)
      pub fn at(mut self, rect: $crate::core::ui::Rect) -> Self {
        self.opts.at = Some(rect);
        self
      }

      /// Its width: UI units (`120.0`), a fraction of its parent
      /// (`Length::Pct(0.5)`), `Length::Fill` or `Length::Fit` (its own
      /// size, the default)
      pub fn width(mut self, width: impl Into<$crate::core::ui::Length>) -> Self {
        self.opts.style.width = width.into();
        self
      }

      /// Its height, as [`width`](Self::width)
      pub fn height(mut self, height: impl Into<$crate::core::ui::Length>) -> Self {
        self.opts.style.height = height.into();
        self
      }

      /// Take the width its parent has left
      pub fn fill_width(self) -> Self {
        self.width($crate::core::ui::Length::Fill)
      }

      /// Room around it, outside its edges: `8.0`, `(x, y)` or `Edges`
      pub fn margin(mut self, margin: impl Into<$crate::core::ui::Edges>) -> Self {
        self.opts.style.margin = margin.into();
        self
      }

      /// Where it sits in a stack, or in the UI outside any container
      /// (default the top-left)
      pub fn anchor(mut self, anchor: $crate::core::ui::Anchor) -> Self {
        self.opts.style.anchor = anchor;
        self
      }

      /// Moved by this much from where its anchor puts it (in a stack or
      /// outside any container)
      pub fn offset(mut self, x: f32, y: f32) -> Self {
        self.opts.style.offset = ::glam::Vec2::new(x, y);
        self
      }

      /// How much of the room its parent has left it takes, shared by weight
      pub fn grow(mut self, grow: f32) -> Self {
        self.opts.style.grow = grow;
        self
      }

      /// At least this wide
      pub fn min_width(mut self, width: f32) -> Self {
        self.opts.style.min_width = Some(width);
        self
      }

      /// At most this wide
      pub fn max_width(mut self, width: f32) -> Self {
        self.opts.style.max_width = Some(width);
        self
      }

      /// Focus it first when its focus scope appears (unless the scope
      /// remembers another widget)
      pub fn default_focus(mut self) -> Self {
        self.opts.default_focus = true;
        self
      }

      /// Up from it goes to `id`, wherever that is
      pub fn nav_up(mut self, id: $crate::core::ui::UiId) -> Self {
        self.opts.nav[0] = id;
        self
      }

      /// Down from it goes to `id`, wherever that is
      pub fn nav_down(mut self, id: $crate::core::ui::UiId) -> Self {
        self.opts.nav[1] = id;
        self
      }

      /// Left from it goes to `id`, wherever that is
      pub fn nav_left(mut self, id: $crate::core::ui::UiId) -> Self {
        self.opts.nav[2] = id;
        self
      }

      /// Right from it goes to `id`, wherever that is
      pub fn nav_right(mut self, id: $crate::core::ui::UiId) -> Self {
        self.opts.nav[3] = id;
        self
      }

      /// The arrows and the D-pad pass it by; Tab and the pointer still
      /// reach it
      pub fn nav_skip(mut self) -> Self {
        self.opts.nav_skip = true;
        self
      }

      /// Whether it takes the keyboard and gamepad focus (default: buttons
      /// and other controls do, custom widgets do not)
      pub fn focusable(mut self, focusable: bool) -> Self {
        self.opts.focusable = Some(focusable);
        self
      }

      /// Draw it in the theme variant `name` (`"danger"`, `"accent"`, or
      /// one the theme adds). An unknown name warns once and draws the plain
      /// look.
      pub fn variant(mut self, name: &str) -> Self {
        self.opts.variant = Some(name.to_string());
        self
      }

      /// Change what `patch` names in its look, in every state, after the
      /// theme and the variant
      pub fn style(mut self, patch: $crate::core::ui::LookPatch) -> Self {
        self.opts.look_patch = Some(patch);
        self
      }

      /// Show `text` in a tooltip once the pointer (or the focus ring) has
      /// rested on it for the theme's tooltip delay
      pub fn tooltip(mut self, text: impl Into<$crate::core::text::RichText>) -> Self {
        self.opts.tooltip = Some(text.into());
        self
      }
    }
  };
  ($widget:ty) => {
    $crate::core::ui::widgets::common::widget_builders!([] $widget);
  };
}

pub(crate) use widget_builders;
