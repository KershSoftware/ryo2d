//! Immediate-mode UI drawing context
//!
//! The `UiContext` provides a simple API for drawing shapes, text, and
//! interactive elements. Create one per frame and pass it to render functions.
//!
//! # Clipping
//!
//! The UI context supports nested clip rects for scroll views and overflow handling.
//! Use `push_clip()` and `pop_clip()` to manage the clip stack:
//!
//! ```ignore
//! // Set up a scroll region
//! ui.push_clip(scroll_area);
//!
//! // Draw content - anything outside scroll_area is clipped
//! ui.draw_text("Item 1", 10.0, 10.0, Color::WHITE);
//! ui.draw_text("Item 2", 10.0, 30.0, Color::WHITE);
//!
//! // Restore previous clip state
//! ui.pop_clip();
//! ```

use winit::event::MouseButton;

use crate::core::input::InputState;

use super::font::BitmapFont;
use crate::core::ui::theme::Theme;
use crate::core::ui::types::{Anchor, Color, Rect};

/// Maximum depth of nested clip rects.
const MAX_CLIP_STACK_DEPTH: usize = 16;

// ============================================================================
// UiContext - Immediate Mode Drawing
// ============================================================================

/// Context for immediate-mode UI rendering.
///
/// Provides methods for drawing shapes, text, and interactive elements.
/// Create one per frame and pass it to your render functions.
///
/// # Example
///
/// ```ignore
/// fn render_ui(&mut self, ui: &mut UiContext) {
///     // Draw a simple label
///     ui.draw_text("Score: 100", 10.0, 10.0, Color::WHITE);
///
///     // Draw a button that returns true when clicked
///     if ui.button("Start", Rect::new(100.0, 100.0, 120.0, 40.0)) {
///         // Handle button click
///     }
/// }
/// ```
pub struct UiContext<'a> {
    /// Frame buffer (RGBA, 4 bytes per pixel)
    frame: &'a mut [u8],
    /// Frame width in pixels
    frame_width: u32,
    /// Frame height in pixels
    frame_height: u32,
    /// Current input state (for click detection)
    input: &'a InputState,
    /// Font for text rendering
    font: &'a BitmapFont,
    /// Optional theme for styled widgets
    theme: Option<&'a Theme>,
    /// Stack of clip rects for nested clipping
    clip_stack: Vec<Rect>,
    /// Current effective clip rect (intersection of all stacked clips)
    current_clip: Option<Rect>,
}

impl<'a> UiContext<'a> {
    /// Create a new UI context for the current frame.
    pub fn new(
        frame: &'a mut [u8],
        frame_width: u32,
        frame_height: u32,
        input: &'a InputState,
        font: &'a BitmapFont,
    ) -> Self {
        Self {
            frame,
            frame_width,
            frame_height,
            input,
            font,
            theme: None,
            clip_stack: Vec::with_capacity(MAX_CLIP_STACK_DEPTH),
            current_clip: None,
        }
    }

    /// Create a new UI context with a theme.
    pub fn with_theme(
        frame: &'a mut [u8],
        frame_width: u32,
        frame_height: u32,
        input: &'a InputState,
        font: &'a BitmapFont,
        theme: &'a Theme,
    ) -> Self {
        Self {
            frame,
            frame_width,
            frame_height,
            input,
            font,
            theme: Some(theme),
            clip_stack: Vec::with_capacity(MAX_CLIP_STACK_DEPTH),
            current_clip: None,
        }
    }

    /// Get the current theme, if any.
    pub fn theme(&self) -> Option<&Theme> {
        self.theme
    }

    /// Get the screen dimensions.
    pub fn screen_size(&self) -> (u32, u32) {
        (self.frame_width, self.frame_height)
    }

    // =========================================================================
    // Clipping
    // =========================================================================

    /// Push a clip rect onto the stack.
    ///
    /// All subsequent drawing will be clipped to the intersection of the
    /// current clip and this new rect. Use `pop_clip()` to restore the
    /// previous clip state.
    ///
    /// # Panics
    /// Panics if the clip stack exceeds `MAX_CLIP_STACK_DEPTH` (16 levels).
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Create a scroll region
    /// let scroll_area = Rect::new(10.0, 10.0, 200.0, 150.0);
    /// ui.push_clip(scroll_area);
    ///
    /// // Draw clipped content
    /// for (i, item) in items.iter().enumerate() {
    ///     let y = 10.0 + i as f32 * 20.0 - scroll_offset;
    ///     ui.draw_text(item, 15.0, y, Color::WHITE);
    /// }
    ///
    /// ui.pop_clip();
    /// ```
    pub fn push_clip(&mut self, rect: Rect) {
        assert!(
            self.clip_stack.len() < MAX_CLIP_STACK_DEPTH,
            "Clip stack overflow: exceeded {} nested clips",
            MAX_CLIP_STACK_DEPTH
        );

        self.clip_stack.push(rect);
        self.update_current_clip();
    }

    /// Pop the most recent clip rect from the stack.
    ///
    /// Restores the clip state to what it was before the corresponding
    /// `push_clip()` call.
    ///
    /// # Panics
    /// Panics if called when the clip stack is empty.
    pub fn pop_clip(&mut self) {
        assert!(
            !self.clip_stack.is_empty(),
            "Clip stack underflow: pop_clip() called with empty stack"
        );

        self.clip_stack.pop();
        self.update_current_clip();
    }

    /// Get the current effective clip rect, if any.
    ///
    /// Returns `None` if no clip is active (full screen is visible).
    pub fn current_clip(&self) -> Option<Rect> {
        self.current_clip
    }

    /// Check if a clip is currently active.
    pub fn has_clip(&self) -> bool {
        self.current_clip.is_some()
    }

    /// Get the current clip stack depth.
    pub fn clip_depth(&self) -> usize {
        self.clip_stack.len()
    }

    /// Clear all clips from the stack.
    ///
    /// Useful for drawing modal overlays that should ignore parent clips.
    pub fn clear_clips(&mut self) {
        self.clip_stack.clear();
        self.current_clip = None;
    }

    /// Temporarily clear clips, run a closure, then restore.
    ///
    /// Useful for drawing modal backgrounds or overlays.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Draw a dimmed background over everything
    /// ui.with_no_clip(|ui| {
    ///     let (w, h) = ui.screen_size();
    ///     ui.draw_rect(Rect::new(0.0, 0.0, w as f32, h as f32),
    ///                  Color::rgba(0, 0, 0, 128));
    /// });
    /// ```
    pub fn with_no_clip<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        let saved_stack = std::mem::take(&mut self.clip_stack);
        let saved_clip = self.current_clip.take();

        let result = f(self);

        self.clip_stack = saved_stack;
        self.current_clip = saved_clip;

        result
    }

    /// Recalculate the current effective clip rect.
    fn update_current_clip(&mut self) {
        if self.clip_stack.is_empty() {
            self.current_clip = None;
        } else {
            // Start with first clip rect
            let mut result = self.clip_stack[0];

            // Intersect with all subsequent clips
            for clip in &self.clip_stack[1..] {
                result = Self::intersect_rects(result, *clip);
            }

            // Check if the intersection is valid (non-empty)
            if result.width > 0.0 && result.height > 0.0 {
                self.current_clip = Some(result);
            } else {
                // Empty intersection - nothing visible
                self.current_clip = Some(Rect::new(0.0, 0.0, 0.0, 0.0));
            }
        }
    }

    /// Calculate the intersection of two rectangles.
    fn intersect_rects(a: Rect, b: Rect) -> Rect {
        let x = a.x.max(b.x);
        let y = a.y.max(b.y);
        let right = a.right().min(b.right());
        let bottom = a.bottom().min(b.bottom());

        let width = (right - x).max(0.0);
        let height = (bottom - y).max(0.0);

        Rect::new(x, y, width, height)
    }

    /// Apply clipping to a rectangle and return the visible portion.
    ///
    /// Returns `None` if the rect is completely outside the clip area.
    fn clip_rect(&self, rect: Rect) -> Option<Rect> {
        // First clip to screen bounds
        let screen = Rect::new(0.0, 0.0, self.frame_width as f32, self.frame_height as f32);
        let clipped = Self::intersect_rects(rect, screen);

        if clipped.width <= 0.0 || clipped.height <= 0.0 {
            return None;
        }

        // Then clip to current clip rect if any
        if let Some(clip) = self.current_clip {
            let final_rect = Self::intersect_rects(clipped, clip);
            if final_rect.width > 0.0 && final_rect.height > 0.0 {
                Some(final_rect)
            } else {
                None
            }
        } else {
            Some(clipped)
        }
    }

    // =========================================================================
    // Shape Drawing
    // =========================================================================

    /// Draw a filled rectangle.
    ///
    /// The rectangle is automatically clipped to the current clip rect if one
    /// is active, and to screen bounds.
    pub fn draw_rect(&mut self, rect: Rect, color: Color) {
        // Apply clipping
        let Some(clipped) = self.clip_rect(rect) else {
            return; // Completely outside clip area
        };

        let x0 = clipped.x as i32;
        let y0 = clipped.y as i32;
        let x1 = clipped.right() as i32;
        let y1 = clipped.bottom() as i32;

        if x0 >= x1 || y0 >= y1 || color.a == 0 {
            return;
        }

        if color.a == 255 {
            // Opaque - direct write
            for y in y0..y1 {
                for x in x0..x1 {
                    let idx = (y as usize * self.frame_width as usize + x as usize) * 4;
                    self.frame[idx] = color.r;
                    self.frame[idx + 1] = color.g;
                    self.frame[idx + 2] = color.b;
                    self.frame[idx + 3] = 255;
                }
            }
        } else {
            // Alpha blending
            let alpha = color.a as u16;
            let inv_alpha = 255 - alpha;
            for y in y0..y1 {
                for x in x0..x1 {
                    let idx = (y as usize * self.frame_width as usize + x as usize) * 4;
                    self.frame[idx] =
                        ((color.r as u16 * alpha + self.frame[idx] as u16 * inv_alpha) / 255) as u8;
                    self.frame[idx + 1] = ((color.g as u16 * alpha
                        + self.frame[idx + 1] as u16 * inv_alpha)
                        / 255) as u8;
                    self.frame[idx + 2] = ((color.b as u16 * alpha
                        + self.frame[idx + 2] as u16 * inv_alpha)
                        / 255) as u8;
                    self.frame[idx + 3] = 255;
                }
            }
        }
    }

    /// Draw a rectangle outline (border only).
    pub fn draw_rect_outline(&mut self, rect: Rect, color: Color, thickness: f32) {
        let t = thickness;
        // Top
        self.draw_rect(Rect::new(rect.x, rect.y, rect.width, t), color);
        // Bottom
        self.draw_rect(
            Rect::new(rect.x, rect.bottom() - t, rect.width, t),
            color,
        );
        // Left
        self.draw_rect(
            Rect::new(rect.x, rect.y + t, t, rect.height - t * 2.0),
            color,
        );
        // Right
        self.draw_rect(
            Rect::new(rect.right() - t, rect.y + t, t, rect.height - t * 2.0),
            color,
        );
    }

    /// Draw a panel with background and optional border.
    pub fn draw_panel(&mut self, rect: Rect, bg_color: Color, border_color: Option<Color>) {
        self.draw_rect(rect, bg_color);
        if let Some(border) = border_color {
            self.draw_rect_outline(rect, border, 1.0);
        }
    }

    /// Draw a themed panel using the current theme.
    ///
    /// Falls back to default colors if no theme is set.
    pub fn draw_themed_panel(&mut self, rect: Rect) {
        if let Some(theme) = self.theme {
            self.draw_rect(rect, theme.panel.background);
            if let Some(border) = theme.panel.border {
                self.draw_rect_outline(rect, border, theme.panel.border_thickness);
            }
        } else {
            self.draw_panel(rect, Color::PANEL_BG, Some(Color::PANEL_BORDER));
        }
    }

    /// Draw a horizontal progress bar.
    ///
    /// # Arguments
    /// * `rect` - Bounding rectangle for the progress bar
    /// * `progress` - Fill amount (0.0 to 1.0)
    /// * `fill_color` - Color of the filled portion
    /// * `bg_color` - Color of the background
    pub fn draw_progress_bar(
        &mut self,
        rect: Rect,
        progress: f32,
        fill_color: Color,
        bg_color: Color,
    ) {
        // Background
        self.draw_rect(rect, bg_color);

        // Fill
        let progress = progress.clamp(0.0, 1.0);
        if progress > 0.0 {
            let fill_width = rect.width * progress;
            self.draw_rect(
                Rect::new(rect.x, rect.y, fill_width, rect.height),
                fill_color,
            );
        }
    }

    /// Draw a progress bar with border.
    pub fn draw_progress_bar_with_border(
        &mut self,
        rect: Rect,
        progress: f32,
        fill_color: Color,
        bg_color: Color,
        border_color: Color,
    ) {
        self.draw_progress_bar(rect, progress, fill_color, bg_color);
        self.draw_rect_outline(rect, border_color, 1.0);
    }

    /// Draw a themed progress bar using the current theme.
    ///
    /// Falls back to default colors if no theme is set.
    pub fn draw_themed_progress_bar(&mut self, rect: Rect, progress: f32) {
        if let Some(theme) = self.theme {
            self.draw_progress_bar(
                rect,
                progress,
                theme.progress_bar.fill,
                theme.progress_bar.background,
            );
            if let Some(border) = theme.progress_bar.border {
                self.draw_rect_outline(rect, border, theme.progress_bar.border_thickness);
            }
        } else {
            self.draw_progress_bar(rect, progress, Color::rgb(80, 150, 255), Color::DARK_GRAY);
        }
    }

    // =========================================================================
    // Text Drawing
    // =========================================================================

    /// Draw text at the given position.
    ///
    /// Text is clipped per-pixel to the current clip rect if one is active.
    pub fn draw_text(&mut self, text: &str, x: f32, y: f32, color: Color) {
        // Quick bounds check if clip is active
        if let Some(clip) = self.current_clip {
            let (text_w, text_h) = self.font.text_size(text);
            let text_rect = Rect::new(x, y, text_w as f32, text_h as f32);

            // Skip if text is completely outside clip region
            if text_rect.right() < clip.x
                || text_rect.x > clip.right()
                || text_rect.bottom() < clip.y
                || text_rect.y > clip.bottom()
            {
                return;
            }

            // Use per-pixel clipped rendering
            self.font.draw_text_clipped(
                self.frame,
                self.frame_width,
                self.frame_height,
                text,
                x as i32,
                y as i32,
                color,
                clip.x as i32,
                clip.y as i32,
                clip.width as i32,
                clip.height as i32,
            );
        } else {
            self.font.draw_text(
                self.frame,
                self.frame_width,
                self.frame_height,
                text,
                x as i32,
                y as i32,
                color,
            );
        }
    }

    /// Draw text with anchor-based positioning.
    pub fn draw_text_anchored(
        &mut self,
        text: &str,
        anchor: Anchor,
        offset_x: f32,
        offset_y: f32,
        color: Color,
    ) {
        let (text_w, text_h) = self.font.text_size(text);
        let (x, y) = anchor.calculate_position(
            offset_x,
            offset_y,
            text_w as f32,
            text_h as f32,
            self.frame_width as f32,
            self.frame_height as f32,
        );
        self.draw_text(text, x, y, color);
    }

    /// Draw centered text within a rectangle.
    pub fn draw_text_centered(&mut self, text: &str, rect: Rect, color: Color) {
        let (text_w, text_h) = self.font.text_size(text);
        let x = rect.x + (rect.width - text_w as f32) / 2.0;
        let y = rect.y + (rect.height - text_h as f32) / 2.0;
        self.draw_text(text, x, y, color);
    }

    /// Calculate the pixel size of text.
    pub fn text_size(&self, text: &str) -> (u32, u32) {
        self.font.text_size(text)
    }

    // =========================================================================
    // Interactive Widgets
    // =========================================================================

    /// Check if a rectangle is being clicked this frame.
    ///
    /// Returns `true` if the mouse button was just pressed while over the rectangle.
    pub fn is_clicked(&self, rect: Rect) -> bool {
        if !self.input.mouse_pressed(MouseButton::Left) {
            return false;
        }
        let (mx, my) = self.input.mouse_position();
        rect.contains(mx, my)
    }

    /// Check if a rectangle is being hovered.
    pub fn is_hovered(&self, rect: Rect) -> bool {
        let (mx, my) = self.input.mouse_position();
        rect.contains(mx, my)
    }

    /// Check if a rectangle is being held (mouse down while hovering).
    pub fn is_held(&self, rect: Rect) -> bool {
        if !self.input.mouse_down(MouseButton::Left) {
            return false;
        }
        let (mx, my) = self.input.mouse_position();
        rect.contains(mx, my)
    }

    /// Draw an interactive button and return whether it was clicked.
    ///
    /// Draws a button with hover/pressed states and returns `true` on click.
    pub fn button(&mut self, text: &str, rect: Rect) -> bool {
        let hovered = self.is_hovered(rect);
        let held = self.is_held(rect);
        let clicked = self.is_clicked(rect);

        // Use theme colors if available, otherwise use defaults
        let (bg_color, border_color, text_color) = if let Some(theme) = self.theme {
            let bg = if held {
                theme.button.background_pressed
            } else if hovered {
                theme.button.background_hovered
            } else {
                theme.button.background
            };
            let border = if hovered {
                theme.button.border_hovered
            } else {
                theme.button.border
            };
            let text = if held {
                Color::LIGHT_GRAY
            } else {
                theme.button.text
            };
            (bg, border, text)
        } else {
            let bg = if held {
                Color::rgb(60, 60, 80)
            } else if hovered {
                Color::rgb(50, 50, 70)
            } else {
                Color::rgb(40, 40, 60)
            };
            let border = if hovered {
                Color::rgb(100, 100, 140)
            } else {
                Color::rgb(80, 80, 100)
            };
            let text = if held { Color::LIGHT_GRAY } else { Color::WHITE };
            (bg, border, text)
        };

        self.draw_panel(rect, bg_color, Some(border_color));
        self.draw_text_centered(text, rect, text_color);

        clicked
    }

    /// Draw an interactive button with custom colors.
    pub fn button_styled(
        &mut self,
        text: &str,
        rect: Rect,
        normal_bg: Color,
        hover_bg: Color,
        pressed_bg: Color,
        text_color: Color,
    ) -> bool {
        let hovered = self.is_hovered(rect);
        let held = self.is_held(rect);
        let clicked = self.is_clicked(rect);

        let bg_color = if held {
            pressed_bg
        } else if hovered {
            hover_bg
        } else {
            normal_bg
        };

        self.draw_rect(rect, bg_color);
        self.draw_text_centered(text, rect, text_color);

        clicked
    }

    /// Draw a label with optional background panel.
    pub fn label(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        text_color: Color,
        bg_color: Option<Color>,
    ) {
        if let Some(bg) = bg_color {
            let (w, h) = self.text_size(text);
            let padding = 4.0;
            self.draw_rect(
                Rect::new(
                    x - padding,
                    y - padding,
                    w as f32 + padding * 2.0,
                    h as f32 + padding * 2.0,
                ),
                bg,
            );
        }
        self.draw_text(text, x, y, text_color);
    }

    // =========================================================================
    // Sprite Drawing
    // =========================================================================

    /// Draw a sprite at the given position.
    pub fn draw_sprite(&mut self, sprite: &crate::core::sprite::Sprite, x: f32, y: f32) {
        sprite.draw(
            self.frame,
            self.frame_width,
            self.frame_height,
            x as i32,
            y as i32,
        );
    }

    /// Draw a sprite scaled uniformly.
    pub fn draw_sprite_scaled(
        &mut self,
        sprite: &crate::core::sprite::Sprite,
        x: f32,
        y: f32,
        scale: f32,
    ) {
        sprite.draw_scaled(
            self.frame,
            self.frame_width,
            self.frame_height,
            x as i32,
            y as i32,
            scale,
        );
    }

    /// Draw a sprite handle at the given position.
    pub fn draw_sprite_handle(
        &mut self,
        handle: &crate::core::asset::SpriteHandle,
        x: f32,
        y: f32,
    ) {
        handle.with_sprite(|sprite| {
            sprite.draw(
                self.frame,
                self.frame_width,
                self.frame_height,
                x as i32,
                y as i32,
            );
        });
    }

    /// Draw a sprite handle scaled uniformly.
    pub fn draw_sprite_handle_scaled(
        &mut self,
        handle: &crate::core::asset::SpriteHandle,
        x: f32,
        y: f32,
        scale: f32,
    ) {
        handle.with_sprite(|sprite| {
            sprite.draw_scaled(
                self.frame,
                self.frame_width,
                self.frame_height,
                x as i32,
                y as i32,
                scale,
            );
        });
    }

    // =========================================================================
    // Slider Widget
    // =========================================================================

    /// Draw an interactive horizontal slider.
    ///
    /// Returns true if the value was changed this frame.
    ///
    /// # Arguments
    /// * `rect` - The bounding box for the slider track
    /// * `value` - Mutable reference to the value being controlled (will be updated if dragged)
    /// * `min` - Minimum value
    /// * `max` - Maximum value
    pub fn slider(&mut self, rect: Rect, value: &mut f32, min: f32, max: f32) -> bool {
        // Get colors from theme or use defaults
        let (track_color, fill_color, handle_color, hover_color) = if let Some(theme) = self.theme {
            (
                theme.slider.track_background,
                theme.slider.track_fill,
                theme.slider.handle,
                theme.slider.handle_hovered,
            )
        } else {
            (
                Color::rgb(60, 60, 80),
                Color::rgb(80, 150, 255),
                Color::WHITE,
                Color::rgb(200, 220, 255),
            )
        };

        let mouse_pos = self.input.mouse_position();

        // Interaction area - must click here to start dragging, and stay roughly here to continue
        // Horizontal is generous (allow overshooting ends), vertical is tight (avoid other widgets)
        let interaction_rect = Rect::new(
            rect.x - 20.0,
            rect.y - 15.0,
            rect.width + 40.0,
            rect.height + 30.0,
        );
        let mouse_in_interaction = interaction_rect.contains(mouse_pos.0, mouse_pos.1);

        // Calculate handle position
        let normalized = (*value - min) / (max - min).max(0.0001);
        let handle_x = rect.x + normalized * rect.width;
        let handle_width = 12.0;
        let handle_height = rect.height + 8.0;
        let handle_rect = Rect::new(
            handle_x - handle_width / 2.0,
            rect.y - 4.0,
            handle_width,
            handle_height,
        );

        let mut changed = false;

        // Handle dragging - mouse must be within the interaction area
        // (In pure immediate mode, we can't track "which widget started the drag",
        // so we require staying near the slider to avoid affecting other widgets)
        let is_dragging = self.input.mouse_down(MouseButton::Left) && mouse_in_interaction;

        if is_dragging {
            // Calculate new value based on mouse position (clamped to slider bounds)
            let mouse_x = mouse_pos.0.clamp(rect.x, rect.right());
            let new_normalized = (mouse_x - rect.x) / rect.width;
            let new_value = min + new_normalized * (max - min);
            let clamped_value = new_value.clamp(min, max);

            if (*value - clamped_value).abs() > 0.001 {
                *value = clamped_value;
                changed = true;
            }
        }

        // Draw track background
        self.draw_rect(rect, track_color);

        // Draw filled portion
        let fill_rect = Rect::new(rect.x, rect.y, normalized * rect.width, rect.height);
        self.draw_rect(fill_rect, fill_color);

        // Draw handle
        let handle_visual_color = if mouse_in_interaction || is_dragging {
            hover_color
        } else {
            handle_color
        };
        self.draw_rect(handle_rect, handle_visual_color);

        changed
    }

    // =========================================================================
    // Checkbox Widget
    // =========================================================================

    /// Draw an interactive checkbox.
    ///
    /// Returns true if the checkbox was toggled this frame.
    ///
    /// # Arguments
    /// * `rect` - The bounding box for the checkbox (typically square)
    /// * `checked` - Mutable reference to the checked state
    /// * `label` - Optional label text to display next to the checkbox
    pub fn checkbox(&mut self, rect: Rect, checked: &mut bool, label: Option<&str>) -> bool {
        let hovered = self.is_hovered(rect);
        let clicked = self.is_clicked(rect);

        // Get colors from theme or use defaults
        let (bg_color, border_color, checkmark_color) = if let Some(theme) = self.theme {
            let bg = if *checked {
                theme.checkbox.background_checked
            } else if hovered {
                theme.checkbox.background_hovered
            } else {
                theme.checkbox.background
            };
            (bg, theme.checkbox.border, theme.checkbox.checkmark)
        } else {
            let bg = if *checked {
                Color::rgb(60, 120, 200)
            } else if hovered {
                Color::rgb(50, 50, 70)
            } else {
                Color::rgb(40, 40, 60)
            };
            (bg, Color::rgb(80, 80, 100), Color::WHITE)
        };

        // Toggle on click
        if clicked {
            *checked = !*checked;
        }

        // Draw checkbox background
        self.draw_panel(rect, bg_color, Some(border_color));

        // Draw checkmark if checked - simple filled inner square
        if *checked {
            let padding = (rect.width.min(rect.height) * 0.25).max(3.0);
            let inner_rect = rect.shrink(padding);
            self.draw_rect(inner_rect, checkmark_color);
        }

        // Draw label if provided
        if let Some(label_text) = label {
            let text_color = self.theme.map(|t| t.label.text).unwrap_or(Color::WHITE);
            let text_x = rect.right() + 8.0;
            let (_, text_h) = self.text_size(label_text);
            let text_y = rect.y + (rect.height - text_h as f32) / 2.0;
            self.draw_text(label_text, text_x, text_y, text_color);
        }

        clicked
    }

    // =========================================================================
    // Toggle Widget
    // =========================================================================

    /// Draw an interactive toggle switch.
    ///
    /// Returns true if the toggle was changed this frame.
    ///
    /// # Arguments
    /// * `rect` - The bounding box for the toggle (width should be ~2x height)
    /// * `on` - Mutable reference to the toggle state
    pub fn toggle(&mut self, rect: Rect, on: &mut bool) -> bool {
        let hovered = self.is_hovered(rect);
        let clicked = self.is_clicked(rect);

        // Get colors from theme or use defaults
        let (track_color, knob_color) = if let Some(theme) = self.theme {
            let track = if *on {
                theme.toggle.track_on
            } else if hovered {
                theme.toggle.track_hovered
            } else {
                theme.toggle.track_off
            };
            (track, theme.toggle.knob)
        } else {
            let track = if *on {
                Color::rgb(60, 150, 80)
            } else if hovered {
                Color::rgb(70, 70, 90)
            } else {
                Color::rgb(60, 60, 80)
            };
            (track, Color::WHITE)
        };

        // Toggle on click
        if clicked {
            *on = !*on;
        }

        // Draw track (rounded appearance via rect)
        self.draw_rect(rect, track_color);

        // Draw knob
        let knob_size = rect.height - 4.0;
        let knob_x = if *on {
            rect.right() - knob_size - 2.0
        } else {
            rect.x + 2.0
        };
        let knob_rect = Rect::new(knob_x, rect.y + 2.0, knob_size, knob_size);
        self.draw_rect(knob_rect, knob_color);

        clicked
    }

    // =========================================================================
    // Modal & Overlay Support
    // =========================================================================

    /// Draw a modal overlay that dims the background.
    ///
    /// This clears all current clips and draws a semi-transparent rectangle
    /// over the entire screen. Useful as a backdrop for modal dialogs.
    ///
    /// # Arguments
    /// * `dim_color` - The color to overlay (typically semi-transparent black)
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Dim the background
    /// ui.draw_modal_overlay(Color::rgba(0, 0, 0, 128));
    ///
    /// // Draw the modal dialog on top
    /// let dialog = Rect::centered_at(center_x, center_y, 300.0, 200.0);
    /// ui.draw_panel(dialog, Color::rgb(40, 40, 50), Some(Color::WHITE));
    /// ui.draw_text_centered("Confirm?", dialog, Color::WHITE);
    /// ```
    pub fn draw_modal_overlay(&mut self, dim_color: Color) {
        self.with_no_clip(|ui| {
            let (w, h) = ui.screen_size();
            ui.draw_rect(Rect::new(0.0, 0.0, w as f32, h as f32), dim_color);
        });
    }

    /// Check if a point is within the current clip region.
    ///
    /// Returns `true` if no clip is active or if the point is inside the clip.
    pub fn point_in_clip(&self, x: f32, y: f32) -> bool {
        match self.current_clip {
            Some(clip) => clip.contains(x, y),
            None => {
                // Check screen bounds
                x >= 0.0
                    && y >= 0.0
                    && x < self.frame_width as f32
                    && y < self.frame_height as f32
            }
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_context(width: u32, height: u32, frame: &mut [u8]) -> UiContext<'_> {
        let input = InputState::new();
        // Leak the input to get a static reference for testing
        let input_ref: &'static InputState = Box::leak(Box::new(input));
        let font = super::super::font::create_debug_font();
        let font_ref: &'static BitmapFont = Box::leak(Box::new(font));

        UiContext::new(frame, width, height, input_ref, font_ref)
    }

    #[test]
    fn clip_stack_push_pop() {
        let mut frame = vec![0u8; 100 * 100 * 4];
        let mut ui = create_test_context(100, 100, &mut frame);

        assert_eq!(ui.clip_depth(), 0);
        assert!(!ui.has_clip());

        ui.push_clip(Rect::new(10.0, 10.0, 50.0, 50.0));
        assert_eq!(ui.clip_depth(), 1);
        assert!(ui.has_clip());
        assert_eq!(ui.current_clip(), Some(Rect::new(10.0, 10.0, 50.0, 50.0)));

        ui.pop_clip();
        assert_eq!(ui.clip_depth(), 0);
        assert!(!ui.has_clip());
    }

    #[test]
    fn nested_clips_intersect() {
        let mut frame = vec![0u8; 100 * 100 * 4];
        let mut ui = create_test_context(100, 100, &mut frame);

        // First clip: 10,10 to 60,60
        ui.push_clip(Rect::new(10.0, 10.0, 50.0, 50.0));
        // Second clip: 30,30 to 80,80
        ui.push_clip(Rect::new(30.0, 30.0, 50.0, 50.0));

        // Intersection should be: 30,30 to 60,60 (30x30)
        let clip = ui.current_clip().unwrap();
        assert_eq!(clip.x, 30.0);
        assert_eq!(clip.y, 30.0);
        assert_eq!(clip.width, 30.0);
        assert_eq!(clip.height, 30.0);
    }

    #[test]
    fn clear_clips_resets_stack() {
        let mut frame = vec![0u8; 100 * 100 * 4];
        let mut ui = create_test_context(100, 100, &mut frame);

        ui.push_clip(Rect::new(10.0, 10.0, 50.0, 50.0));
        ui.push_clip(Rect::new(20.0, 20.0, 30.0, 30.0));
        assert_eq!(ui.clip_depth(), 2);

        ui.clear_clips();
        assert_eq!(ui.clip_depth(), 0);
        assert!(!ui.has_clip());
    }

    #[test]
    fn with_no_clip_restores_state() {
        let mut frame = vec![0u8; 100 * 100 * 4];
        let mut ui = create_test_context(100, 100, &mut frame);

        ui.push_clip(Rect::new(10.0, 10.0, 50.0, 50.0));
        let original_clip = ui.current_clip();

        let inside_clip = ui.with_no_clip(|ui| {
            assert!(!ui.has_clip());
            42
        });

        assert_eq!(inside_clip, 42);
        assert_eq!(ui.current_clip(), original_clip);
        assert_eq!(ui.clip_depth(), 1);
    }

    #[test]
    fn point_in_clip_checks_bounds() {
        let mut frame = vec![0u8; 100 * 100 * 4];
        let mut ui = create_test_context(100, 100, &mut frame);

        // No clip - check screen bounds
        assert!(ui.point_in_clip(50.0, 50.0));
        assert!(!ui.point_in_clip(-1.0, 50.0));
        assert!(!ui.point_in_clip(100.0, 50.0));

        // With clip
        ui.push_clip(Rect::new(20.0, 20.0, 40.0, 40.0));
        assert!(ui.point_in_clip(30.0, 30.0)); // Inside clip
        assert!(!ui.point_in_clip(10.0, 30.0)); // Left of clip
        assert!(!ui.point_in_clip(70.0, 30.0)); // Right of clip
    }

    #[test]
    fn empty_clip_intersection() {
        let mut frame = vec![0u8; 100 * 100 * 4];
        let mut ui = create_test_context(100, 100, &mut frame);

        // Two non-overlapping clips
        ui.push_clip(Rect::new(10.0, 10.0, 20.0, 20.0)); // 10,10 to 30,30
        ui.push_clip(Rect::new(50.0, 50.0, 20.0, 20.0)); // 50,50 to 70,70

        // Intersection is empty
        let clip = ui.current_clip().unwrap();
        assert!(clip.width == 0.0 || clip.height == 0.0);
    }

    #[test]
    fn rect_intersection_calculation() {
        // Overlapping rects
        let a = Rect::new(10.0, 10.0, 50.0, 50.0);
        let b = Rect::new(30.0, 30.0, 50.0, 50.0);
        let result = UiContext::intersect_rects(a, b);
        assert_eq!(result.x, 30.0);
        assert_eq!(result.y, 30.0);
        assert_eq!(result.width, 30.0);
        assert_eq!(result.height, 30.0);

        // Non-overlapping rects
        let c = Rect::new(0.0, 0.0, 10.0, 10.0);
        let d = Rect::new(20.0, 20.0, 10.0, 10.0);
        let result2 = UiContext::intersect_rects(c, d);
        assert!(result2.width == 0.0 || result2.height == 0.0);
    }
}
