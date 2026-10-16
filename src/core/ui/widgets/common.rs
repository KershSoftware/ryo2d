//! What every widget builder shares: its options and the builder methods
//! that set them (`widget_builders!`)

use crate::core::ui::Rect;
use crate::core::ui::layout::UiStyle;

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
}

/// The builder methods every widget shares, for a widget type with an
/// `opts: WidgetOpts` field: `id`, `enabled`, `at`, `width`, `height`,
/// `fill_width`, `margin`, `anchor`, `offset`, `grow`, `min_width` and
/// `max_width`.
macro_rules! widget_builders {
  ($widget:ty) => {
    impl $widget {
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

      /// Placed at `rect`, its size too, outside the layout (inside a
      /// container the layout places it)
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
    }
  };
}

pub(crate) use widget_builders;
