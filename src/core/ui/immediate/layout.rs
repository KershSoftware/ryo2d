//! Layout containers for immediate-mode UI
//!
//! These are stateless rect generators that help position widgets in rows, columns,
//! or stacked layers. They work with any widget by generating `Rect` values.
//!
//! # Example
//!
//! ```ignore
//! // Horizontal row of buttons
//! let mut row = HBox::new(10.0, 10.0, 8.0);
//! if ui.button("A", row.next(80.0, 30.0)) { /* ... */ }
//! if ui.button("B", row.next(80.0, 30.0)) { /* ... */ }
//! if ui.button("C", row.next(80.0, 30.0)) { /* ... */ }
//!
//! // Vertical column of controls
//! let mut col = VBox::new(10.0, 50.0, 6.0);
//! ui.label("Volume", col.next(100.0, 20.0).x, col.current_y(), Color::WHITE, None);
//! ui.slider(col.next(200.0, 20.0), &mut volume, 0.0, 1.0);
//! ui.checkbox(col.next(20.0, 20.0), &mut muted, Some("Mute"));
//! ```

use crate::core::ui::types::Rect;

/// Horizontal alignment options for layout containers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HAlign {
    /// Align to the left (default)
    #[default]
    Left,
    /// Center horizontally
    Center,
    /// Align to the right
    Right,
}

/// Vertical alignment options for layout containers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VAlign {
    /// Align to the top (default)
    #[default]
    Top,
    /// Center vertically
    Center,
    /// Align to the bottom
    Bottom,
}

// ============================================================================
// HBox - Horizontal Layout
// ============================================================================

/// Horizontal layout container that places widgets in a row.
///
/// # Example
///
/// ```ignore
/// let mut row = HBox::new(10.0, 10.0, 8.0);
/// ui.button("A", row.next(80.0, 30.0));
/// ui.button("B", row.next(80.0, 30.0));
/// ui.button("C", row.next(80.0, 30.0));
/// ```
#[derive(Debug, Clone)]
pub struct HBox {
    /// Starting X position
    start_x: f32,
    /// Current X position (advances with each widget)
    current_x: f32,
    /// Y position (constant for all widgets)
    y: f32,
    /// Spacing between widgets
    spacing: f32,
    /// Vertical alignment within the row
    valign: VAlign,
    /// Maximum height seen (for alignment calculations)
    max_height: f32,
    /// Whether this is the first widget
    first: bool,
}

impl HBox {
    /// Create a new horizontal layout starting at the given position.
    ///
    /// # Arguments
    /// * `x` - Starting X position
    /// * `y` - Y position for the row
    /// * `spacing` - Horizontal spacing between widgets
    pub fn new(x: f32, y: f32, spacing: f32) -> Self {
        Self {
            start_x: x,
            current_x: x,
            y,
            spacing,
            valign: VAlign::Top,
            max_height: 0.0,
            first: true,
        }
    }

    /// Set the vertical alignment for widgets in this row.
    pub fn with_valign(mut self, valign: VAlign) -> Self {
        self.valign = valign;
        self
    }

    /// Get the next rect for a widget with the given size.
    ///
    /// Advances the cursor by the widget width plus spacing.
    pub fn next(&mut self, width: f32, height: f32) -> Rect {
        if !self.first {
            self.current_x += self.spacing;
        }
        self.first = false;

        let x = self.current_x;
        self.current_x += width;
        self.max_height = self.max_height.max(height);

        // Apply vertical alignment
        let y = match self.valign {
            VAlign::Top => self.y,
            VAlign::Center => self.y + (self.max_height - height) / 2.0,
            VAlign::Bottom => self.y + self.max_height - height,
        };

        Rect::new(x, y, width, height)
    }

    /// Get the current X position (after all placed widgets).
    pub fn current_x(&self) -> f32 {
        self.current_x
    }

    /// Get the Y position of the row.
    pub fn y(&self) -> f32 {
        self.y
    }

    /// Get the total width used so far.
    pub fn width(&self) -> f32 {
        self.current_x - self.start_x
    }

    /// Get the maximum height of widgets in the row.
    pub fn height(&self) -> f32 {
        self.max_height
    }

    /// Get the bounding rect of all widgets placed so far.
    pub fn bounds(&self) -> Rect {
        Rect::new(self.start_x, self.y, self.width(), self.max_height)
    }

    /// Reset the layout to start a new row at the same position.
    pub fn reset(&mut self) {
        self.current_x = self.start_x;
        self.max_height = 0.0;
        self.first = true;
    }

    /// Move to a new row below the current one.
    pub fn new_row(&mut self, row_spacing: f32) {
        self.y += self.max_height + row_spacing;
        self.reset();
    }
}

// ============================================================================
// VBox - Vertical Layout
// ============================================================================

/// Vertical layout container that places widgets in a column.
///
/// # Example
///
/// ```ignore
/// let mut col = VBox::new(10.0, 10.0, 6.0);
/// ui.label("Settings", col.next(100.0, 20.0).x, col.current_y() - 26.0, Color::WHITE, None);
/// ui.slider(col.next(200.0, 20.0), &mut volume, 0.0, 1.0);
/// ui.checkbox(col.next(20.0, 20.0), &mut enabled, Some("Enabled"));
/// ```
#[derive(Debug, Clone)]
pub struct VBox {
    /// X position (constant for all widgets)
    x: f32,
    /// Starting Y position
    start_y: f32,
    /// Current Y position (advances with each widget)
    current_y: f32,
    /// Spacing between widgets
    spacing: f32,
    /// Horizontal alignment within the column
    halign: HAlign,
    /// Maximum width seen (for alignment calculations)
    max_width: f32,
    /// Whether this is the first widget
    first: bool,
}

impl VBox {
    /// Create a new vertical layout starting at the given position.
    ///
    /// # Arguments
    /// * `x` - X position for the column
    /// * `y` - Starting Y position
    /// * `spacing` - Vertical spacing between widgets
    pub fn new(x: f32, y: f32, spacing: f32) -> Self {
        Self {
            x,
            start_y: y,
            current_y: y,
            spacing,
            halign: HAlign::Left,
            max_width: 0.0,
            first: true,
        }
    }

    /// Set the horizontal alignment for widgets in this column.
    pub fn with_halign(mut self, halign: HAlign) -> Self {
        self.halign = halign;
        self
    }

    /// Get the next rect for a widget with the given size.
    ///
    /// Advances the cursor by the widget height plus spacing.
    pub fn next(&mut self, width: f32, height: f32) -> Rect {
        if !self.first {
            self.current_y += self.spacing;
        }
        self.first = false;

        let y = self.current_y;
        self.current_y += height;
        self.max_width = self.max_width.max(width);

        // Apply horizontal alignment
        let x = match self.halign {
            HAlign::Left => self.x,
            HAlign::Center => self.x + (self.max_width - width) / 2.0,
            HAlign::Right => self.x + self.max_width - width,
        };

        Rect::new(x, y, width, height)
    }

    /// Get the X position of the column.
    pub fn x(&self) -> f32 {
        self.x
    }

    /// Get the current Y position (after all placed widgets).
    pub fn current_y(&self) -> f32 {
        self.current_y
    }

    /// Get the total height used so far.
    pub fn height(&self) -> f32 {
        self.current_y - self.start_y
    }

    /// Get the maximum width of widgets in the column.
    pub fn width(&self) -> f32 {
        self.max_width
    }

    /// Get the bounding rect of all widgets placed so far.
    pub fn bounds(&self) -> Rect {
        Rect::new(self.x, self.start_y, self.max_width, self.height())
    }

    /// Reset the layout to start a new column at the same position.
    pub fn reset(&mut self) {
        self.current_y = self.start_y;
        self.max_width = 0.0;
        self.first = true;
    }

    /// Move to a new column to the right of the current one.
    pub fn new_column(&mut self, column_spacing: f32) {
        self.x += self.max_width + column_spacing;
        self.reset();
    }
}

// ============================================================================
// Stack - Layered Layout
// ============================================================================

/// Layered layout container that places widgets at the same position.
///
/// Useful for overlays, backgrounds behind content, or z-ordered elements.
///
/// # Example
///
/// ```ignore
/// let stack = Stack::new(100.0, 100.0, 200.0, 150.0);
///
/// // Background panel
/// ui.draw_panel(stack.full(), Color::PANEL_BG, Some(Color::PANEL_BORDER));
///
/// // Content with padding
/// let content = stack.padded(10.0);
/// ui.draw_text("Hello", content.x, content.y, Color::WHITE);
///
/// // Centered element
/// let centered = stack.centered(80.0, 30.0);
/// ui.button("OK", centered);
/// ```
#[derive(Debug, Clone, Copy)]
pub struct Stack {
    /// Bounding rectangle
    rect: Rect,
}

impl Stack {
    /// Create a new stack layout with the given bounds.
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            rect: Rect::new(x, y, width, height),
        }
    }

    /// Create a stack from an existing rect.
    pub fn from_rect(rect: Rect) -> Self {
        Self { rect }
    }

    /// Get the full bounds of the stack.
    pub fn full(&self) -> Rect {
        self.rect
    }

    /// Get bounds with padding applied.
    pub fn padded(&self, padding: f32) -> Rect {
        self.rect.shrink(padding)
    }

    /// Get bounds with asymmetric padding.
    pub fn padded_xy(&self, pad_x: f32, pad_y: f32) -> Rect {
        Rect::new(
            self.rect.x + pad_x,
            self.rect.y + pad_y,
            (self.rect.width - pad_x * 2.0).max(0.0),
            (self.rect.height - pad_y * 2.0).max(0.0),
        )
    }

    /// Get a rect centered within the stack.
    pub fn centered(&self, width: f32, height: f32) -> Rect {
        let (cx, cy) = self.rect.center();
        Rect::centered_at(cx, cy, width, height)
    }

    /// Get a rect aligned to a corner or edge.
    pub fn aligned(&self, width: f32, height: f32, halign: HAlign, valign: VAlign) -> Rect {
        let x = match halign {
            HAlign::Left => self.rect.x,
            HAlign::Center => self.rect.x + (self.rect.width - width) / 2.0,
            HAlign::Right => self.rect.right() - width,
        };

        let y = match valign {
            VAlign::Top => self.rect.y,
            VAlign::Center => self.rect.y + (self.rect.height - height) / 2.0,
            VAlign::Bottom => self.rect.bottom() - height,
        };

        Rect::new(x, y, width, height)
    }

    /// Get a rect aligned with padding from the edge.
    pub fn aligned_with_padding(
        &self,
        width: f32,
        height: f32,
        halign: HAlign,
        valign: VAlign,
        padding: f32,
    ) -> Rect {
        let padded = self.padded(padding);
        Stack::from_rect(padded).aligned(width, height, halign, valign)
    }

    /// Split the stack horizontally, returning left and right portions.
    pub fn split_horizontal(&self, left_width: f32) -> (Rect, Rect) {
        let left = Rect::new(self.rect.x, self.rect.y, left_width, self.rect.height);
        let right = Rect::new(
            self.rect.x + left_width,
            self.rect.y,
            (self.rect.width - left_width).max(0.0),
            self.rect.height,
        );
        (left, right)
    }

    /// Split the stack vertically, returning top and bottom portions.
    pub fn split_vertical(&self, top_height: f32) -> (Rect, Rect) {
        let top = Rect::new(self.rect.x, self.rect.y, self.rect.width, top_height);
        let bottom = Rect::new(
            self.rect.x,
            self.rect.y + top_height,
            self.rect.width,
            (self.rect.height - top_height).max(0.0),
        );
        (top, bottom)
    }

    /// Get the position (top-left corner).
    pub fn position(&self) -> (f32, f32) {
        (self.rect.x, self.rect.y)
    }

    /// Get the size.
    pub fn size(&self) -> (f32, f32) {
        (self.rect.width, self.rect.height)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hbox_positions_widgets() {
        let mut row = HBox::new(10.0, 20.0, 5.0);

        let r1 = row.next(100.0, 30.0);
        assert_eq!(r1, Rect::new(10.0, 20.0, 100.0, 30.0));

        let r2 = row.next(80.0, 30.0);
        assert_eq!(r2, Rect::new(115.0, 20.0, 80.0, 30.0)); // 10 + 100 + 5

        let r3 = row.next(60.0, 30.0);
        assert_eq!(r3, Rect::new(200.0, 20.0, 60.0, 30.0)); // 115 + 80 + 5
    }

    #[test]
    fn hbox_tracks_bounds() {
        let mut row = HBox::new(10.0, 20.0, 5.0);
        row.next(100.0, 30.0);
        row.next(80.0, 40.0);

        assert_eq!(row.width(), 185.0); // 100 + 5 + 80
        assert_eq!(row.height(), 40.0); // max height
        assert_eq!(row.bounds(), Rect::new(10.0, 20.0, 185.0, 40.0));
    }

    #[test]
    fn vbox_positions_widgets() {
        let mut col = VBox::new(10.0, 20.0, 5.0);

        let r1 = col.next(100.0, 30.0);
        assert_eq!(r1, Rect::new(10.0, 20.0, 100.0, 30.0));

        let r2 = col.next(100.0, 25.0);
        assert_eq!(r2, Rect::new(10.0, 55.0, 100.0, 25.0)); // 20 + 30 + 5

        let r3 = col.next(100.0, 20.0);
        assert_eq!(r3, Rect::new(10.0, 85.0, 100.0, 20.0)); // 55 + 25 + 5
    }

    #[test]
    fn vbox_tracks_bounds() {
        let mut col = VBox::new(10.0, 20.0, 5.0);
        col.next(100.0, 30.0);
        col.next(120.0, 25.0);

        assert_eq!(col.height(), 60.0); // 30 + 5 + 25
        assert_eq!(col.width(), 120.0); // max width
        assert_eq!(col.bounds(), Rect::new(10.0, 20.0, 120.0, 60.0));
    }

    #[test]
    fn stack_centered() {
        let stack = Stack::new(0.0, 0.0, 200.0, 100.0);
        let centered = stack.centered(50.0, 30.0);
        assert_eq!(centered, Rect::new(75.0, 35.0, 50.0, 30.0));
    }

    #[test]
    fn stack_aligned() {
        let stack = Stack::new(0.0, 0.0, 200.0, 100.0);

        let top_left = stack.aligned(50.0, 30.0, HAlign::Left, VAlign::Top);
        assert_eq!(top_left, Rect::new(0.0, 0.0, 50.0, 30.0));

        let bottom_right = stack.aligned(50.0, 30.0, HAlign::Right, VAlign::Bottom);
        assert_eq!(bottom_right, Rect::new(150.0, 70.0, 50.0, 30.0));

        let center = stack.aligned(50.0, 30.0, HAlign::Center, VAlign::Center);
        assert_eq!(center, Rect::new(75.0, 35.0, 50.0, 30.0));
    }

    #[test]
    fn stack_split() {
        let stack = Stack::new(0.0, 0.0, 200.0, 100.0);

        let (left, right) = stack.split_horizontal(80.0);
        assert_eq!(left, Rect::new(0.0, 0.0, 80.0, 100.0));
        assert_eq!(right, Rect::new(80.0, 0.0, 120.0, 100.0));

        let (top, bottom) = stack.split_vertical(40.0);
        assert_eq!(top, Rect::new(0.0, 0.0, 200.0, 40.0));
        assert_eq!(bottom, Rect::new(0.0, 40.0, 200.0, 60.0));
    }

    #[test]
    fn stack_padded() {
        let stack = Stack::new(10.0, 20.0, 200.0, 100.0);
        let padded = stack.padded(10.0);
        assert_eq!(padded, Rect::new(20.0, 30.0, 180.0, 80.0));
    }

    #[test]
    fn hbox_new_row() {
        let mut row = HBox::new(10.0, 20.0, 5.0);
        row.next(100.0, 30.0);
        row.new_row(10.0);

        let r = row.next(100.0, 30.0);
        assert_eq!(r, Rect::new(10.0, 60.0, 100.0, 30.0)); // y = 20 + 30 + 10
    }

    #[test]
    fn vbox_new_column() {
        let mut col = VBox::new(10.0, 20.0, 5.0);
        col.next(100.0, 30.0);
        col.new_column(10.0);

        let r = col.next(80.0, 30.0);
        assert_eq!(r, Rect::new(120.0, 20.0, 80.0, 30.0)); // x = 10 + 100 + 10
    }
}
