//! Clipping demo scene - demonstrates scissor rect clipping and modal overlays

use ryo2d::prelude::*;

pub struct ClippingDemo {
  // Scroll state
  scroll_offset: f32,
  scroll_velocity: f32,
  // Scroll region focus/interaction
  scroll_focused: bool,
  scroll_dragging: bool,
  drag_start_y: f32,
  drag_start_offset: f32,
  // Modal state
  show_modal: bool,
  modal_result: Option<bool>,
  // Demo items
  items: Vec<String>,
  // Cached scroll area rect (set during render, used in update)
  scroll_area: Rect,
  content_area: Rect,
}

impl ClippingDemo {
  pub fn new() -> Self {
    Self {
      scroll_offset: 0.0,
      scroll_velocity: 0.0,
      scroll_focused: false,
      scroll_dragging: false,
      drag_start_y: 0.0,
      drag_start_offset: 0.0,
      show_modal: false,
      modal_result: None,
      items: (1..=20)
        .map(|i| format!("Item {} - This is a scrollable list item", i))
        .collect(),
      scroll_area: Rect::new(30.0, 90.0, 280.0, 150.0),
      content_area: Rect::new(35.0, 95.0, 270.0, 140.0),
    }
  }
}

impl Default for ClippingDemo {
  fn default() -> Self {
    Self::new()
  }
}

impl Scene for ClippingDemo {
  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    if ctx.input.key_pressed(KeyCode::Escape) {
      if self.show_modal {
        self.show_modal = false;
      } else {
        return SceneAction::Pop;
      }
    }

    let (mx, my) = ctx.input.mouse_position();
    let mouse_in_scroll_area = self.scroll_area.contains(mx, my);
    let content_height = self.items.len() as f32 * 25.0;
    // max_scroll = total content height - visible area height
    let max_scroll = (content_height - self.content_area.height).max(0.0);

    // Handle focus - click to focus, click outside to unfocus
    if ctx.input.mouse_pressed(MouseButton::Left) {
      self.scroll_focused = mouse_in_scroll_area;
    }

    // Scrollbar dimensions
    let visible_ratio = self.content_area.height / content_height;
    let scrollbar_height = (visible_ratio * self.content_area.height).max(20.0);
    let track_height = self.content_area.height - scrollbar_height;
    let scroll_ratio = if max_scroll > 0.0 {
      self.scroll_offset / max_scroll
    } else {
      0.0
    };
    let scrollbar_y = self.content_area.y + scroll_ratio * track_height;
    let scrollbar_rect = Rect::new(
      self.scroll_area.right() - 10.0,
      scrollbar_y,
      8.0,
      scrollbar_height,
    );

    // Handle scrollbar dragging
    if ctx.input.mouse_pressed(MouseButton::Left) && scrollbar_rect.contains(mx, my) {
      self.scroll_dragging = true;
      self.drag_start_y = my;
      self.drag_start_offset = self.scroll_offset;
      self.scroll_focused = true;
    }

    if self.scroll_dragging {
      if ctx.input.mouse_down(MouseButton::Left) {
        // Calculate new scroll based on drag distance
        let drag_delta = my - self.drag_start_y;
        let scroll_ratio = drag_delta / (self.content_area.height - scrollbar_height);
        self.scroll_offset =
          (self.drag_start_offset + scroll_ratio * max_scroll).clamp(0.0, max_scroll);
        self.scroll_velocity = 0.0; // Stop momentum while dragging
      } else {
        self.scroll_dragging = false;
      }
    }

    // Apply scroll velocity with friction (only when not dragging)
    if !self.scroll_dragging {
      self.scroll_offset += self.scroll_velocity * dt;
      self.scroll_velocity *= 0.92; // Slightly stronger friction

      // Stop very small velocities
      if self.scroll_velocity.abs() < 1.0 {
        self.scroll_velocity = 0.0;
      }
    }

    // Clamp scroll offset
    self.scroll_offset = self.scroll_offset.clamp(0.0, max_scroll);

    // Handle scroll input - only when focused
    if self.scroll_focused {
      if ctx.input.key_down(KeyCode::ArrowUp) {
        self.scroll_velocity = -300.0;
      }
      if ctx.input.key_down(KeyCode::ArrowDown) {
        self.scroll_velocity = 300.0;
      }
    }

    // Mouse wheel scrolling - only when hovering over scroll area
    if mouse_in_scroll_area {
      let scroll_delta = ctx.input.scroll_delta();
      if scroll_delta.1.abs() > 0.01 {
        // Much higher multiplier for better responsiveness
        self.scroll_velocity = -scroll_delta.1 * 400.0;
      }
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction {
    let (screen_w, screen_h) = ui.screen_size();

    // Background
    ui.draw_rect(
      Rect::new(0.0, 0.0, screen_w as f32, screen_h as f32),
      Color::rgb(25, 25, 35),
    );

    // Title
    ui.draw_text_anchored(
      "Clipping & Modal Demo",
      Anchor::TopCenter,
      0.0,
      20.0,
      Color::WHITE,
    );
    ui.draw_text_anchored(
      "Scissor clipping for scroll regions and overlays",
      Anchor::TopCenter,
      0.0,
      45.0,
      Color::GRAY,
    );

    // =====================================================================
    // Section 1: Clipped Scroll Region
    // =====================================================================
    let scroll_x = 30.0;
    let scroll_y = 90.0;
    ui.draw_text(
      "Clipped Scroll Region:",
      scroll_x,
      scroll_y - 20.0,
      Color::WHITE,
    );

    // Update stored rects for use in update()
    self.scroll_area = Rect::new(scroll_x, scroll_y, 280.0, 150.0);
    self.content_area = self.scroll_area.shrink(5.0);

    // Draw focus indicator if focused
    let border_color = if self.scroll_focused {
      Color::rgb(100, 150, 255)
    } else {
      Color::rgb(60, 60, 80)
    };
    ui.draw_panel(self.scroll_area, Color::rgb(35, 35, 45), Some(border_color));

    // Push clip for scroll content
    ui.push_clip(self.content_area);

    // Draw scrollable items
    for (i, item) in self.items.iter().enumerate() {
      let item_y = self.content_area.y + 5.0 + (i as f32 * 25.0) - self.scroll_offset;

      // Alternate row colors for visibility
      let row_color = if i % 2 == 0 {
        Color::rgb(40, 40, 55)
      } else {
        Color::rgb(35, 35, 50)
      };

      ui.draw_rect(
        Rect::new(
          self.content_area.x,
          item_y,
          self.content_area.width - 12.0,
          23.0,
        ),
        row_color,
      );
      ui.draw_text(
        item,
        self.content_area.x + 5.0,
        item_y + 5.0,
        Color::LIGHT_GRAY,
      );
    }

    ui.pop_clip();

    // Scrollbar track
    let track_rect = Rect::new(
      self.scroll_area.right() - 10.0,
      self.content_area.y,
      8.0,
      self.content_area.height,
    );
    ui.draw_rect(track_rect, Color::rgb(30, 30, 40));

    // Scrollbar thumb
    let content_height = self.items.len() as f32 * 25.0;
    let max_scroll = (content_height - self.content_area.height).max(0.0);
    let visible_ratio = self.content_area.height / content_height;
    let scrollbar_height = (visible_ratio * self.content_area.height).max(20.0);
    let track_height = self.content_area.height - scrollbar_height;
    let scroll_ratio = if max_scroll > 0.0 {
      self.scroll_offset / max_scroll
    } else {
      0.0
    };
    let scrollbar_y = self.content_area.y + scroll_ratio * track_height;

    let scrollbar_rect = Rect::new(
      self.scroll_area.right() - 10.0,
      scrollbar_y,
      8.0,
      scrollbar_height,
    );

    // Scrollbar color based on state
    let scrollbar_color = if self.scroll_dragging {
      Color::rgb(150, 150, 180) // Dragging - bright
    } else if ui.is_hovered(scrollbar_rect) {
      Color::rgb(130, 130, 160) // Hovered
    } else {
      Color::rgb(100, 100, 120) // Normal
    };
    ui.draw_rect(scrollbar_rect, scrollbar_color);

    // Scroll instructions
    let instruction = if self.scroll_focused {
      "Click and drag scrollbar, or use UP/DOWN arrows"
    } else {
      "Click to focus, scroll wheel to scroll"
    };
    ui.draw_text(
      instruction,
      scroll_x,
      self.scroll_area.bottom() + 10.0,
      Color::rgb(80, 80, 100),
    );

    // =====================================================================
    // Section 2: Nested Clips Demo
    // =====================================================================
    let nested_x = 30.0;
    let nested_y = 310.0;
    ui.draw_text("Nested Clips:", nested_x, nested_y - 20.0, Color::WHITE);

    // Outer clip region
    let outer_rect = Rect::new(nested_x, nested_y, 200.0, 150.0);
    ui.draw_panel(
      outer_rect,
      Color::rgb(35, 35, 45),
      Some(Color::rgb(60, 60, 80)),
    );

    ui.push_clip(outer_rect.shrink(10.0));

    // Show clip depth while inside first clip
    ui.draw_text(
      &format!("Outer clip depth: {}", ui.clip_depth()),
      nested_x + 10.0,
      nested_y + 10.0,
      Color::rgb(150, 150, 180),
    );

    // Inner clip region (partially overlapping)
    let inner_rect = Rect::new(nested_x + 50.0, nested_y + 30.0, 200.0, 150.0);
    ui.draw_panel(
      inner_rect,
      Color::rgb(45, 35, 55),
      Some(Color::rgb(80, 60, 100)),
    );

    ui.push_clip(inner_rect.shrink(10.0));

    // Show depth during nested clips
    ui.draw_text(
      &format!("Inner clip depth: {}", ui.clip_depth()),
      inner_rect.x + 10.0,
      inner_rect.y + 10.0,
      Color::rgb(180, 150, 200),
    );

    // Content that will be clipped to intersection
    ui.draw_text(
      "This text is clipped",
      inner_rect.x + 10.0,
      inner_rect.y + 30.0,
      Color::WHITE,
    );
    ui.draw_text(
      "to the intersection",
      inner_rect.x + 10.0,
      inner_rect.y + 48.0,
      Color::WHITE,
    );
    ui.draw_text(
      "of both clip rects",
      inner_rect.x + 10.0,
      inner_rect.y + 66.0,
      Color::WHITE,
    );

    // Draw a rect that extends beyond clips
    ui.draw_rect(
      Rect::new(inner_rect.x + 50.0, inner_rect.y + 90.0, 200.0, 100.0),
      Color::new(100, 150, 255, 100),
    );

    ui.pop_clip();
    ui.pop_clip();

    // Explanation text (outside clips)
    ui.draw_text(
      "The purple box extends beyond both clips.",
      nested_x,
      nested_y + 170.0,
      Color::rgb(80, 80, 100),
    );
    ui.draw_text(
      "Only the intersection region is visible.",
      nested_x,
      nested_y + 188.0,
      Color::rgb(80, 80, 100),
    );

    // =====================================================================
    // Section 3: Modal Button
    // =====================================================================
    let modal_section_x = 300.0;
    let modal_section_y = 310.0;
    ui.draw_text(
      "Modal Overlay:",
      modal_section_x,
      modal_section_y,
      Color::WHITE,
    );

    let modal_btn = Rect::new(modal_section_x, modal_section_y + 25.0, 150.0, 35.0);
    if ui.button("Show Modal Dialog", modal_btn) {
      self.show_modal = true;
      self.modal_result = None;
    }

    // Show result of last modal
    if let Some(confirmed) = self.modal_result {
      let result_text = if confirmed { "Confirmed!" } else { "Cancelled" };
      let result_color = if confirmed {
        Color::rgb(80, 200, 80)
      } else {
        Color::rgb(200, 80, 80)
      };
      ui.draw_text(
        &format!("Last result: {}", result_text),
        modal_section_x,
        modal_section_y + 70.0,
        result_color,
      );
    }

    // =====================================================================
    // Modal Overlay (drawn on top of everything)
    // =====================================================================
    if self.show_modal {
      // Dim the background
      ui.draw_modal_overlay(Color::new(0, 0, 0, 180));

      // Dialog box
      let dialog_w = 300.0;
      let dialog_h = 150.0;
      let dialog = Rect::centered_at(
        screen_w as f32 / 2.0,
        screen_h as f32 / 2.0,
        dialog_w,
        dialog_h,
      );

      ui.draw_panel(dialog, Color::rgb(45, 45, 60), Some(Color::WHITE));

      // Dialog title
      ui.draw_text_centered(
        "Confirm Action",
        Rect::new(dialog.x, dialog.y + 15.0, dialog_w, 20.0),
        Color::WHITE,
      );

      // Dialog message
      ui.draw_text_centered(
        "Are you sure you want to proceed?",
        Rect::new(dialog.x, dialog.y + 50.0, dialog_w, 20.0),
        Color::LIGHT_GRAY,
      );

      // Buttons
      let btn_y = dialog.y + dialog_h - 50.0;
      let confirm_btn = Rect::new(dialog.x + 30.0, btn_y, 100.0, 35.0);
      let cancel_btn = Rect::new(dialog.x + dialog_w - 130.0, btn_y, 100.0, 35.0);

      if ui.button("Confirm", confirm_btn) {
        self.show_modal = false;
        self.modal_result = Some(true);
      }

      if ui.button("Cancel", cancel_btn) {
        self.show_modal = false;
        self.modal_result = Some(false);
      }
    }

    // Instructions
    ui.draw_text_anchored(
      "Press ESC to return (or close modal)",
      Anchor::BottomRight,
      -10.0,
      -10.0,
      Color::rgb(80, 80, 100),
    );
    SceneAction::Continue
  }
}
