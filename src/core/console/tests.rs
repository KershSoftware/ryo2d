//! The console's own tests (its drawing's are in `view.rs`).

use super::*;
use crate::core::clipboard::FakeClipboard;
use winit::event::ElementState;

#[test]
fn the_console_names_no_network_code() {
  // Built here, so this file's own text does not match
  let needle = ["std", "net"].join("::");
  let sources = [
    ("mod.rs", include_str!("mod.rs")),
    ("command.rs", include_str!("command.rs")),
    ("registry.rs", include_str!("registry.rs")),
    ("builtins.rs", include_str!("builtins.rs")),
    ("dispatch.rs", include_str!("dispatch.rs")),
    ("scope.rs", include_str!("scope.rs")),
    ("view.rs", include_str!("view.rs")),
    ("tests.rs", include_str!("tests.rs")),
  ];
  for (name, text) in sources {
    assert!(!text.contains(&needle), "{name} names {needle}");
  }
}

// ============================================================================
// Console Basic Operations
// ============================================================================

#[test]
fn test_console_new_initial_state() {
  let console = Console::new();
  assert!(!console.visible);
  assert!(console.focused);
  assert!(console.input_buffer.is_empty());
  assert_eq!(console.edit.cursor(), 0);
  assert_eq!(console.scroll_offset, 0.0);
  // Welcome message adds lines
  assert!(!console.output_lines.is_empty());
}

#[test]
fn the_wheel_over_the_console_scrolls_it_and_not_the_game() {
  let mut console = Console::new();
  console.toggle();
  let rect = console.window_rect();
  let mut input = Input::new();
  input.handle_mouse_move(
    f64::from(rect.x + rect.width / 2.0),
    f64::from(rect.y + rect.height / 2.0),
  );
  input.handle_scroll(winit::event::MouseScrollDelta::LineDelta(0.0, 1.0));
  console.handle_input(&mut input);
  assert_eq!(input.scroll_delta(), (0.0, 0.0));
}

/// A console of `lines` short lines with the pointer over its output
pub(super) fn scrolling_console(lines: usize) -> (Console, Input) {
  let mut console = Console::new();
  console.toggle();
  console.clear();
  for i in 0..lines {
    console.print(format!("line {i}"));
  }
  let rect = console.window_rect();
  let mut input = Input::new();
  input.handle_mouse_move(
    f64::from(rect.x + rect.width / 2.0),
    f64::from(rect.y + rect.height / 2.0),
  );
  (console, input)
}

/// One frame as the engine runs it: ease, then read the input; `scroll`
/// is a trackpad's logical pixels and `lines` a wheel's
pub(super) fn console_frame(console: &mut Console, input: &mut Input, lines: f32, scroll: f32) {
  input.begin_frame();
  if lines != 0.0 {
    input.handle_scroll(winit::event::MouseScrollDelta::LineDelta(0.0, lines));
  }
  if scroll != 0.0 {
    input.handle_scroll_pixels(0.0, scroll);
  }
  console.update(1.0 / 60.0);
  console.handle_input(input);
}

/// How far the view is scrolled, in the console's drawing units
pub(super) fn scrolled_units(console: &Console) -> f32 {
  console.scroll_offset * console.line_height
}

#[test]
fn a_trackpad_moves_the_console_exactly_as_far_as_the_fingers() {
  let (mut console, mut input) = scrolling_console(50);
  for _ in 0..20 {
    console_frame(&mut console, &mut input, 0.0, 3.0);
  }
  let units = scrolled_units(&console);
  assert!((units - 60.0).abs() < 0.01, "60 units, got {units}");
  for _ in 0..20 {
    console_frame(&mut console, &mut input, 0.0, -3.0);
  }
  assert!(scrolled_units(&console).abs() < 0.01, "and back down");
}

#[test]
fn a_trackpad_is_converted_to_the_consoles_units() {
  let (mut console, mut input) = scrolling_console(50);
  console.set_pixel_scale(2.0);
  for _ in 0..10 {
    console_frame(&mut console, &mut input, 0.0, 3.0);
  }
  let units = scrolled_units(&console);
  assert!(
    (units - 60.0).abs() < 0.01,
    "10 x 3 px at 2 units a px, {units}"
  );
}

#[test]
fn a_wheel_line_eases_toward_three_lines() {
  let (mut console, mut input) = scrolling_console(50);
  let line = console.line_height;
  console_frame(&mut console, &mut input, 1.0, 0.0);
  console_frame(&mut console, &mut input, 0.0, 0.0);
  let early = scrolled_units(&console);
  assert!(early > 0.0 && early < 3.0 * line, "easing, at {early}");
  for _ in 0..60 {
    console_frame(&mut console, &mut input, 0.0, 0.0);
  }
  let settled = scrolled_units(&console);
  assert!((settled - 3.0 * line).abs() < 0.1, "three lines, {settled}");
}

#[test]
fn the_console_scroll_stops_at_the_top_and_the_bottom() {
  let (mut console, mut input) = scrolling_console(50);
  let (_, _, view, _) = console.output_area();
  let max = 50.0 * console.line_height - view;
  console_frame(&mut console, &mut input, 0.0, 100000.0);
  assert!((scrolled_units(&console) - max).abs() < 0.01, "the top");
  console_frame(&mut console, &mut input, 0.0, -100000.0);
  assert_eq!(scrolled_units(&console), 0.0, "the bottom");
  console_frame(&mut console, &mut input, 100.0, 0.0);
  for _ in 0..600 {
    console_frame(&mut console, &mut input, 0.0, 0.0);
  }
  assert!((scrolled_units(&console) - max).abs() < 0.01, "a wheel too");
}

#[test]
fn submitting_a_command_jumps_to_the_bottom_but_other_output_does_not() {
  let (mut console, mut input) = scrolling_console(50);
  console_frame(&mut console, &mut input, 0.0, 60.0);
  console.print_error("a capture report");
  assert!(
    (scrolled_units(&console) - 60.0).abs() < 0.01,
    "left in place"
  );
  assert_eq!(console.unseen_lines, 1);
  console.input_buffer = "help".to_string();
  console.edit.set_cursor("help", 4, false);
  input.queue(crate::core::input::InputEvent::Key {
    key: KeyCode::Enter,
    pressed: true,
  });
  input.begin_frame();
  assert_eq!(console.handle_input(&mut input).as_deref(), Some("help"));
  assert_eq!(console.scroll_offset, 0.0);
  assert_eq!(console.scroll_target, 0.0);
}

#[test]
fn an_offset_beyond_the_end_is_brought_back_before_scrolling() {
  let (mut console, mut input) = scrolling_console(50);
  let (_, _, view, _) = console.output_area();
  let line = console.line_height;
  let max = 50.0 * line - view;
  console.scroll_offset = 500.0;
  console.scroll_target = 500.0;
  console_frame(&mut console, &mut input, -1.0, 0.0);
  for _ in 0..120 {
    console_frame(&mut console, &mut input, 0.0, 0.0);
  }
  let units = scrolled_units(&console);
  assert!(
    (units - (max - 3.0 * line)).abs() < 0.1,
    "a notch down moved it: {units}"
  );
}

#[test]
fn a_tiny_distance_from_the_bottom_counts_as_the_bottom() {
  let (mut console, mut input) = scrolling_console(50);
  console_frame(&mut console, &mut input, 0.0, 60.0);
  console_frame(&mut console, &mut input, 0.0, -59.8);
  assert_eq!(console.scroll_target, 0.0);
  assert_eq!(console.scroll_offset, 0.0);
  console.print("more");
  assert_eq!(scrolled_units(&console), 0.0, "it follows the output");
}

#[test]
fn a_zero_time_step_does_not_jump_the_ease() {
  let (mut console, mut input) = scrolling_console(50);
  console_frame(&mut console, &mut input, 1.0, 0.0);
  console.update(0.0);
  assert_eq!(console.scroll_offset, 0.0);
}

#[test]
fn test_console_toggle() {
  let mut console = Console::new();
  assert!(!console.visible);

  console.toggle();
  assert!(console.visible);
  assert!(console.focused);

  console.toggle();
  assert!(!console.visible);
}

#[test]
fn test_console_print() {
  let mut console = Console::new();
  let initial_lines = console.output_lines.len();

  console.print("Hello, World!");
  assert_eq!(console.output_lines.len(), initial_lines + 1);

  let last_line = console.output_lines.back().unwrap();
  assert_eq!(last_line.text, "Hello, World!");
  assert_eq!(last_line.color, colors::TEXT);
}

#[test]
fn test_console_print_colored() {
  let mut console = Console::new();
  let custom_color = Color::rgb(255, 128, 0);

  console.print_colored("Orange text", custom_color);

  let last_line = console.output_lines.back().unwrap();
  assert_eq!(last_line.text, "Orange text");
  assert_eq!(last_line.color, custom_color);
}

#[test]
fn test_console_print_command() {
  let mut console = Console::new();
  console.print_command("> test command");

  let last_line = console.output_lines.back().unwrap();
  assert_eq!(last_line.color, colors::COMMAND);
}

#[test]
fn test_console_print_error() {
  let mut console = Console::new();
  console.print_error("Something went wrong");

  let last_line = console.output_lines.back().unwrap();
  assert_eq!(last_line.color, colors::ERROR);
}

#[test]
fn test_console_print_system() {
  let mut console = Console::new();
  console.print_system("System message");

  let last_line = console.output_lines.back().unwrap();
  assert_eq!(last_line.color, colors::SYSTEM);
}

#[test]
fn test_console_clear() {
  let mut console = Console::new();
  console.print("Line 1");
  console.print("Line 2");
  console.scroll_offset = 5.0;

  console.clear();

  assert!(console.output_lines.is_empty());
  assert_eq!(console.scroll_offset, 0.0);
}

#[test]
fn test_console_max_output_lines() {
  let mut console = Console::new();
  console.clear();

  // Add more than MAX_OUTPUT_LINES
  for i in 0..MAX_OUTPUT_LINES + 100 {
    console.print(format!("Line {}", i));
  }

  assert_eq!(console.output_lines.len(), MAX_OUTPUT_LINES);
  // Oldest lines should be removed, newest kept
  let first_line = console.output_lines.front().unwrap();
  assert!(first_line.text.starts_with("Line 100")); // First 100 lines removed
}

#[test]
fn test_console_update_increments_time() {
  let mut console = Console::new();
  let initial_time = console.total_time;

  console.update(0.016); // ~60fps frame
  assert!(console.total_time > initial_time);
  assert!((console.total_time - initial_time - 0.016).abs() < 0.0001);
}

#[test]
fn test_console_cursor_blink_wraps() {
  let mut console = Console::new();
  console.visible = true;
  console.edit.tick(0.9);
  assert!(!console.edit.caret_shown());

  console.update(0.2); // Push past 1.0

  assert!(console.edit.caret_shown()); // Wrapped around
}

// ============================================================================
// Geometry and Rectangle Calculations
// ============================================================================

#[test]
fn test_window_rect() {
  let mut console = Console::new();
  console.x = 100.0;
  console.y = 50.0;
  console.width = 400.0;
  console.height = 300.0;

  let rect = console.window_rect();
  assert_eq!(rect.x, 100.0);
  assert_eq!(rect.y, 50.0);
  assert_eq!(rect.width, 400.0);
  assert_eq!(rect.height, 300.0);
}

#[test]
fn test_title_bar_rect() {
  let mut console = Console::new();
  console.x = 100.0;
  console.y = 50.0;
  console.width = 400.0;

  let rect = console.title_bar_rect();
  assert_eq!(rect.x, 100.0);
  assert_eq!(rect.y, 50.0);
  assert_eq!(rect.width, 400.0);
  assert_eq!(rect.height, TITLE_BAR_HEIGHT);
}

#[test]
fn test_close_button_rect_position() {
  let mut console = Console::new();
  console.x = 100.0;
  console.y = 50.0;
  console.width = 400.0;

  let rect = console.close_button_rect();

  // Close button should be in top-right of title bar
  let expected_right = 100.0 + 400.0;
  let button_right = rect.x + rect.width + (TITLE_BAR_HEIGHT - CLOSE_BUTTON_SIZE) / 2.0;
  assert!((expected_right - button_right).abs() < 0.01);
  assert_eq!(rect.width, CLOSE_BUTTON_SIZE);
  assert_eq!(rect.height, CLOSE_BUTTON_SIZE);
}

#[test]
fn test_resize_handle_rect_position() {
  let mut console = Console::new();
  console.x = 100.0;
  console.y = 50.0;
  console.width = 400.0;
  console.height = 300.0;

  let rect = console.resize_handle_rect();

  // Resize handle should be in bottom-right corner
  assert_eq!(rect.x, 100.0 + 400.0 - RESIZE_HANDLE_SIZE);
  assert_eq!(rect.y, 50.0 + 300.0 - RESIZE_HANDLE_SIZE);
  assert_eq!(rect.width, RESIZE_HANDLE_SIZE);
  assert_eq!(rect.height, RESIZE_HANDLE_SIZE);
}

#[test]
fn test_output_area_calculation() {
  let mut console = Console::new();
  console.x = 0.0;
  console.y = 0.0;
  console.width = 600.0;
  console.height = 400.0;

  let (output_top, output_bottom, output_height, padding) = console.output_area();

  // Output area should be between title bar and input line
  assert_eq!(padding, 8.0);
  assert_eq!(output_top, TITLE_BAR_HEIGHT + padding);
  assert_eq!(output_bottom, 400.0 - INPUT_HEIGHT - padding);
  assert_eq!(output_height, output_bottom - output_top);
  assert!(output_height > 0.0);
}

#[test]
fn test_scrollbar_track_rect() {
  let mut console = Console::new();
  console.x = 0.0;
  console.y = 0.0;
  console.width = 600.0;
  console.height = 400.0;

  let track = console.scrollbar_track_rect();
  let (output_top, _, output_height, _) = console.output_area();

  // Track should be within output area
  assert_eq!(track.y, output_top);
  assert_eq!(track.height, output_height);
  assert_eq!(track.width, SCROLLBAR_WIDTH);
  // Track should be on the right side
  assert!((track.x - (600.0 - SCROLLBAR_WIDTH - 4.0)).abs() < 0.01);
}

#[test]
fn test_scrollbar_thumb_rect_no_scrollbar_needed() {
  let mut console = Console::new();
  console.clear();
  console.x = 0.0;
  console.y = 0.0;
  console.width = 600.0;
  console.height = 400.0;

  // Add fewer lines than visible
  console.print("Line 1");
  console.print("Line 2");

  // With few lines, no scrollbar needed
  let thumb = console.scrollbar_thumb_rect(20, 2);
  assert!(thumb.is_none());
}

#[test]
fn test_scrollbar_thumb_rect_with_scrollbar() {
  let mut console = Console::new();
  console.x = 0.0;
  console.y = 0.0;
  console.width = 600.0;
  console.height = 400.0;

  // Scrollbar needed when total > visible
  let thumb = console.scrollbar_thumb_rect(10, 100);
  assert!(thumb.is_some());

  let rect = thumb.unwrap();
  assert!(rect.height >= SCROLLBAR_MIN_THUMB);
  assert_eq!(rect.width, SCROLLBAR_WIDTH);
}

#[test]
fn test_scrollbar_thumb_position_at_bottom() {
  let mut console = Console::new();
  console.x = 0.0;
  console.y = 0.0;
  console.width = 600.0;
  console.height = 400.0;
  console.scroll_offset = 0.0; // At bottom

  let (output_top, _, output_height, _) = console.output_area();
  let thumb = console.scrollbar_thumb_rect(10, 100).unwrap();

  // At scroll_offset=0, thumb should be at bottom
  let thumb_bottom = thumb.y + thumb.height;
  let track_bottom = output_top + output_height;
  assert!((thumb_bottom - track_bottom).abs() < 1.0);
}

#[test]
fn test_scrollbar_thumb_position_at_top() {
  let mut console = Console::new();
  console.x = 0.0;
  console.y = 0.0;
  console.width = 600.0;
  console.height = 400.0;
  console.scroll_offset = 90.0; // At top (max for 100 total, 10 visible)

  let (output_top, _, _, _) = console.output_area();
  let thumb = console.scrollbar_thumb_rect(10, 100).unwrap();

  // At max scroll_offset, thumb should be at top
  assert!((thumb.y - output_top).abs() < 1.0);
}

#[test]
fn test_max_text_width_with_scrollbar() {
  let mut console = Console::new();
  console.width = 600.0;

  let width_with_scrollbar = console.max_text_width(true);
  let width_without_scrollbar = console.max_text_width(false);

  // Width should be smaller when scrollbar is present
  assert!(width_with_scrollbar < width_without_scrollbar);
  assert!(width_with_scrollbar > 0.0);
  assert!(width_without_scrollbar > 0.0);
}

// ============================================================================
// Drag State
// ============================================================================

#[test]
fn test_drag_state_equality() {
  assert_eq!(DragState::None, DragState::None);

  let drag1 = DragState::Dragging {
    offset_x: 10.0,
    offset_y: 20.0,
  };
  let drag2 = DragState::Dragging {
    offset_x: 10.0,
    offset_y: 20.0,
  };
  assert_eq!(drag1, drag2);

  let resize1 = DragState::Resizing {
    start_w: 100.0,
    start_h: 200.0,
    start_mouse_x: 50.0,
    start_mouse_y: 60.0,
  };
  let resize2 = DragState::Resizing {
    start_w: 100.0,
    start_h: 200.0,
    start_mouse_x: 50.0,
    start_mouse_y: 60.0,
  };
  assert_eq!(resize1, resize2);

  let scroll1 = DragState::ScrollbarDragging {
    start_scroll_offset: 10.0,
    start_mouse_y: 100.0,
  };
  let scroll2 = DragState::ScrollbarDragging {
    start_scroll_offset: 10.0,
    start_mouse_y: 100.0,
  };
  assert_eq!(scroll1, scroll2);

  assert_ne!(DragState::None, drag1);
}

// ============================================================================
// ConsoleLine
// ============================================================================

#[test]
fn test_console_line_creation() {
  let line = ConsoleLine::new("Test text", Color::WHITE, 1.5);

  assert_eq!(line.text, "Test text");
  assert_eq!(line.color, Color::WHITE);
  assert_eq!(line.timestamp, 1.5);
}

#[test]
fn test_console_line_into_string() {
  let line = ConsoleLine::new(String::from("Owned string"), Color::WHITE, 0.0);
  assert_eq!(line.text, "Owned string");
}

// ============================================================================
// Cursor Style
// ============================================================================

#[test]
fn test_desired_cursor_when_not_visible() {
  let console = Console::new();
  // Console not visible, should return None
  assert!(console.desired_cursor(100.0, 100.0).is_none());
}

#[test]
fn test_desired_cursor_during_resize() {
  let mut console = Console::new();
  console.visible = true;
  console.drag_state = DragState::Resizing {
    start_w: 600.0,
    start_h: 400.0,
    start_mouse_x: 100.0,
    start_mouse_y: 100.0,
  };

  let cursor = console.desired_cursor(0.0, 0.0);
  assert_eq!(cursor, Some(CursorStyle::ResizeNwSe));
}

#[test]
fn test_desired_cursor_during_drag() {
  let mut console = Console::new();
  console.visible = true;
  console.drag_state = DragState::Dragging {
    offset_x: 10.0,
    offset_y: 10.0,
  };

  let cursor = console.desired_cursor(0.0, 0.0);
  assert_eq!(cursor, Some(CursorStyle::Move));
}

#[test]
fn test_desired_cursor_over_resize_handle() {
  let mut console = Console::new();
  console.visible = true;
  console.x = 100.0;
  console.y = 100.0;
  console.width = 600.0;
  console.height = 400.0;

  // Mouse over resize handle (bottom-right corner)
  let mouse_x = 100.0 + 600.0 - RESIZE_HANDLE_SIZE / 2.0;
  let mouse_y = 100.0 + 400.0 - RESIZE_HANDLE_SIZE / 2.0;

  let cursor = console.desired_cursor(mouse_x, mouse_y);
  assert_eq!(cursor, Some(CursorStyle::ResizeNwSe));
}

#[test]
fn the_console_asks_for_no_cursor_once_a_resize_drag_ends_outside_it() {
  use crate::core::input::InputEvent;
  let mut console = Console::new();
  console.toggle();
  let handle = console.resize_handle_rect();
  let (hx, hy) = (
    handle.x + handle.width / 2.0,
    handle.y + handle.height / 2.0,
  );
  let mut input = Input::new();
  let mut frame = |console: &mut Console, events: Vec<InputEvent>| {
    for event in events {
      input.queue(event);
    }
    input.begin_frame();
    console.handle_input(&mut input);
    let (x, y) = input.mouse_position();
    console.desired_cursor(x, y)
  };
  let mouse = |x: f32, y: f32| InputEvent::MouseMove {
    x: f64::from(x),
    y: f64::from(y),
  };
  let button = |pressed| InputEvent::MouseButton {
    button: MouseButton::Left,
    pressed,
  };
  assert_eq!(
    frame(&mut console, vec![mouse(hx, hy)]),
    Some(CursorStyle::ResizeNwSe),
    "over the handle"
  );
  frame(&mut console, vec![button(true)]);
  // Far past the minimum size: the window stops shrinking, the pointer goes on
  assert_eq!(
    frame(&mut console, vec![mouse(hx - 900.0, hy - 600.0)]),
    Some(CursorStyle::ResizeNwSe),
    "resizing"
  );
  // Released with no further move, outside the window
  assert_eq!(
    frame(&mut console, vec![button(false)]),
    None,
    "the game's cursor is back"
  );
  // Over the window it is the default, focused or not
  let rect = console.window_rect();
  let inside = mouse(rect.x + 20.0, rect.y + 20.0);
  assert_eq!(
    frame(&mut console, vec![inside]),
    Some(CursorStyle::Default)
  );
  console.focused = false;
  let (x, y) = (rect.x + 20.0, rect.y + 20.0);
  assert_eq!(
    console.desired_cursor(x, y),
    Some(CursorStyle::Default),
    "a game's crosshair does not show over the console"
  );
}

// ============================================================================
// Focus State
// ============================================================================

#[test]
fn test_is_focused() {
  let mut console = Console::new();
  assert!(console.is_focused());

  console.focused = false;
  assert!(!console.is_focused());
}

// ============================================================================
// Color Constants
// ============================================================================

#[test]
fn test_color_constants_are_valid() {
  // Just verify the color constants are accessible and reasonable
  assert_eq!(colors::TEXT, Color::WHITE);
  // Use const blocks for compile-time assertions on constants
  const _: () = assert!(colors::BACKGROUND_FOCUSED.a > colors::BACKGROUND_UNFOCUSED.a);
  const _: () = assert!(colors::ERROR.r > colors::ERROR.g); // Red > green for error color
  const _: () = assert!(colors::COMMAND.b > colors::COMMAND.r); // Blue > red for command color
}

// ============================================================================
// Constants
// ============================================================================

#[test]
fn test_constants_are_reasonable() {
  // Use const blocks for compile-time assertions on constants
  const _: () = assert!(MAX_OUTPUT_LINES > 100);
  const _: () = assert!(MAX_COMMAND_HISTORY > 10);
  const _: () = assert!(MIN_WIDTH > 0.0);
  const _: () = assert!(MIN_HEIGHT > 0.0);
  const _: () = assert!(DEFAULT_WIDTH >= MIN_WIDTH);
  const _: () = assert!(DEFAULT_HEIGHT >= MIN_HEIGHT);
  const _: () = assert!(TITLE_BAR_HEIGHT > 0.0);
  const _: () = assert!(INPUT_HEIGHT > 0.0);
  const _: () = assert!(SCROLLBAR_WIDTH > 0.0);
  const _: () = assert!(SCROLLBAR_MIN_THUMB > 0.0);
}

// ============================================================================
// Window Rect Public API
// ============================================================================

#[test]
fn test_window_rect_is_public() {
  // window_rect() must be public so the engine can check mouse-in-console
  let console = Console::new();
  let rect = console.window_rect();
  assert_eq!(rect.x, console.x);
  assert_eq!(rect.y, console.y);
  assert_eq!(rect.width, console.width);
  assert_eq!(rect.height, console.height);
}

#[test]
fn test_window_rect_contains_point_inside() {
  let mut console = Console::new();
  console.x = 50.0;
  console.y = 50.0;
  console.width = 200.0;
  console.height = 150.0;

  let rect = console.window_rect();
  assert!(rect.contains(100.0, 100.0));
  assert!(!rect.contains(10.0, 10.0));
  assert!(!rect.contains(300.0, 300.0));
}

// ============================================================================
// Unfocused Text Color Alpha
// ============================================================================

#[test]
fn test_unfocused_text_alpha_scaling() {
  // Verify the alpha scaling formula: (a * 160 / 255)
  let color = colors::COMMAND; // rgb(100, 180, 255, 255)
  let scaled_alpha = (color.a as u16 * 160 / 255) as u8;
  assert_eq!(scaled_alpha, 160); // 255 * 160 / 255 = 160

  let error_color = colors::ERROR; // rgb(255, 100, 100, 255)
  let error_scaled = (error_color.a as u16 * 160 / 255) as u8;
  assert_eq!(error_scaled, 160);

  // System color also fully opaque
  let system_scaled = (colors::SYSTEM.a as u16 * 160 / 255) as u8;
  assert_eq!(system_scaled, 160);
}

#[test]
fn test_unfocused_text_preserves_rgb() {
  // When unfocused, only alpha changes — RGB should be preserved
  let color = Color::new(100, 180, 255, 255);
  let unfocused = Color::new(
    color.r,
    color.g,
    color.b,
    (color.a as u16 * 160 / 255) as u8,
  );
  assert_eq!(unfocused.r, 100);
  assert_eq!(unfocused.g, 180);
  assert_eq!(unfocused.b, 255);
  assert_eq!(unfocused.a, 160);
}

// ============================================================================
// Focus State
// ============================================================================

#[test]
fn test_focus_state_unfocused_after_outside_click() {
  // Verify that focused starts true but can be set to false
  let mut console = Console::new();
  console.toggle(); // make visible
  assert!(console.is_focused());

  console.focused = false;
  assert!(!console.is_focused());
}

// ============================================================================
// Editing the input line
// ============================================================================

const SHIFT: KeyCode = KeyCode::ShiftLeft;
const CMD: KeyCode = KeyCode::SuperLeft;
const CTRL: KeyCode = KeyCode::ControlLeft;

use ElementState::{Pressed, Released};

/// One frame as the engine runs it: keys change, `text` is typed, then the
/// console reads. Returns a submitted command.
fn frame(
  console: &mut Console,
  input: &mut Input,
  keys: &[(KeyCode, ElementState)],
  text: &str,
) -> Option<String> {
  input.begin_frame();
  for &(key, state) in keys {
    input.handle_keyboard(key, state);
  }
  if !text.is_empty() {
    input.handle_text_input(text);
  }
  console.handle_input(input)
}

/// An open console whose line is `line`, the cursor at byte `cursor`
fn console_with(line: &str, cursor: usize) -> (Console, Input) {
  let mut console = Console::new();
  console.toggle();
  console.input_buffer = line.to_string();
  console.edit.set_cursor(line, cursor, false);
  console.tab_line = Some("stale".to_string());
  (console, Input::new())
}

/// Hold `modifier`, tap `key`, release both
fn chord(console: &mut Console, input: &mut Input, modifier: KeyCode, key: KeyCode) {
  frame(console, input, &[(modifier, Pressed), (key, Pressed)], "");
  frame(console, input, &[(key, Released), (modifier, Released)], "");
}

fn tap(console: &mut Console, input: &mut Input, key: KeyCode) {
  frame(console, input, &[(key, Pressed)], "");
  frame(console, input, &[(key, Released)], "");
}

#[test]
fn cmd_v_pastes_the_clipboard_at_the_cursor() {
  let _clipboard = FakeClipboard::holding(Some("MID"));
  let (mut console, mut input) = console_with("abcd", 2);
  console.edit.tick(0.7);
  frame(
    &mut console,
    &mut input,
    &[(CMD, Pressed), (KeyCode::KeyV, Pressed)],
    "",
  );
  assert_eq!(console.input_buffer, "abMIDcd");
  assert_eq!(console.edit.cursor(), 5);
  assert!(console.edit.caret_shown(), "the blink restarts");
  assert!(console.tab_line.is_none(), "Tab completion starts afresh");
}

#[test]
fn ctrl_v_pastes_too() {
  let _clipboard = FakeClipboard::holding(Some("pasted"));
  let (mut console, mut input) = console_with("", 0);
  chord(&mut console, &mut input, CTRL, KeyCode::KeyV);
  assert_eq!(console.input_buffer, "pasted");
  assert_eq!(console.edit.cursor(), 6);
}

#[test]
fn a_paste_is_made_one_line() {
  let _clipboard = FakeClipboard::holding(Some("say \"hi\"\r\nnext\tline\u{7}\n"));
  let (mut console, mut input) = console_with("", 0);
  chord(&mut console, &mut input, CMD, KeyCode::KeyV);
  assert_eq!(console.input_buffer, "say \"hi\" next line");
}

#[test]
fn a_paste_is_cut_at_the_line_limit() {
  let _clipboard = FakeClipboard::holding(Some(&"a".repeat(MAX_INPUT_CHARS + 50)));
  let (mut console, mut input) = console_with("", 0);
  chord(&mut console, &mut input, CMD, KeyCode::KeyV);
  assert_eq!(console.input_buffer.chars().count(), MAX_INPUT_CHARS);
}

#[test]
fn one_press_of_v_pastes_once() {
  let _clipboard = FakeClipboard::holding(Some("x"));
  let (mut console, mut input) = console_with("", 0);
  frame(
    &mut console,
    &mut input,
    &[(CTRL, Pressed), (KeyCode::KeyV, Pressed)],
    "",
  );
  for _ in 0..3 {
    frame(&mut console, &mut input, &[], "");
  }
  assert_eq!(console.input_buffer, "x");
  frame(&mut console, &mut input, &[(KeyCode::KeyV, Released)], "");
  frame(&mut console, &mut input, &[(KeyCode::KeyV, Pressed)], "");
  assert_eq!(console.input_buffer, "xx");
}

#[test]
fn a_shortcut_types_nothing() {
  let _clipboard = FakeClipboard::holding(None);
  let (mut console, mut input) = console_with("", 0);
  frame(&mut console, &mut input, &[(CMD, Pressed)], "v");
  assert_eq!(console.input_buffer, "");
  assert!(!input.has_text_input(), "the text is taken all the same");
  frame(&mut console, &mut input, &[(CMD, Released)], "v");
  assert_eq!(console.input_buffer, "v");
}

#[test]
fn a_failed_clipboard_read_changes_nothing() {
  let _clipboard = FakeClipboard::holding(None);
  let (mut console, mut input) = console_with("abcd", 2);
  chord(&mut console, &mut input, CMD, KeyCode::KeyV);
  assert_eq!(console.input_buffer, "abcd");
  assert_eq!(console.edit.cursor(), 2);
  assert!(console.tab_line.is_some());
}

#[test]
fn select_all_then_copy_puts_the_line_on_the_clipboard() {
  let clipboard = FakeClipboard::holding(Some("old"));
  for modifier in [CMD, CTRL] {
    clipboard.set(Some("old"));
    let (mut console, mut input) = console_with("help me", 2);
    chord(&mut console, &mut input, modifier, KeyCode::KeyA);
    assert_eq!(console.edit.selection(), Some(0..7));
    chord(&mut console, &mut input, modifier, KeyCode::KeyC);
    assert_eq!(clipboard.text().as_deref(), Some("help me"));
    assert_eq!(console.input_buffer, "help me", "a copy leaves the line");
  }
}

#[test]
fn cut_takes_the_selection_and_leaves_the_rest() {
  let clipboard = FakeClipboard::holding(None);
  let (mut console, mut input) = console_with("hello world", 0);
  frame(&mut console, &mut input, &[(SHIFT, Pressed)], "");
  for _ in 0..5 {
    tap(&mut console, &mut input, KeyCode::ArrowRight);
  }
  frame(&mut console, &mut input, &[(SHIFT, Released)], "");
  chord(&mut console, &mut input, CMD, KeyCode::KeyX);
  assert_eq!(clipboard.text().as_deref(), Some("hello"));
  assert_eq!(console.input_buffer, " world");
  assert_eq!(console.edit.cursor(), 0);
  assert!(console.tab_line.is_none());
}

#[test]
fn a_paste_replaces_the_selection() {
  let _clipboard = FakeClipboard::holding(Some("X"));
  let (mut console, mut input) = console_with("abcd", 0);
  chord(&mut console, &mut input, CMD, KeyCode::KeyA);
  chord(&mut console, &mut input, CMD, KeyCode::KeyV);
  assert_eq!(console.input_buffer, "X");
  assert_eq!(console.edit.selection(), None);
}

#[test]
fn shift_arrows_select_and_typing_replaces_the_selection() {
  let (mut console, mut input) = console_with("abcd", 4);
  frame(&mut console, &mut input, &[(SHIFT, Pressed)], "");
  tap(&mut console, &mut input, KeyCode::ArrowLeft);
  tap(&mut console, &mut input, KeyCode::ArrowLeft);
  assert_eq!(console.edit.selection(), Some(2..4));
  tap(&mut console, &mut input, KeyCode::Home);
  assert_eq!(
    console.edit.selection(),
    Some(0..4),
    "Shift+Home reaches the start"
  );
  frame(&mut console, &mut input, &[(SHIFT, Released)], "");
  tap(&mut console, &mut input, KeyCode::End);
  assert_eq!(console.edit.selection(), None);
  frame(&mut console, &mut input, &[(SHIFT, Pressed)], "");
  tap(&mut console, &mut input, KeyCode::ArrowLeft);
  tap(&mut console, &mut input, KeyCode::ArrowLeft);
  frame(&mut console, &mut input, &[(SHIFT, Released)], "");
  frame(&mut console, &mut input, &[], "Z");
  assert_eq!(console.input_buffer, "abZ");
  assert_eq!(console.edit.cursor(), 3);
}

#[test]
fn copy_with_nothing_selected_leaves_the_clipboard_alone() {
  let clipboard = FakeClipboard::holding(Some("keep"));
  let (mut console, mut input) = console_with("abcd", 2);
  chord(&mut console, &mut input, CMD, KeyCode::KeyC);
  assert_eq!(clipboard.text().as_deref(), Some("keep"));
  chord(&mut console, &mut input, CMD, KeyCode::KeyX);
  assert_eq!(clipboard.text().as_deref(), Some("keep"));
  assert_eq!(console.input_buffer, "abcd");
}

#[test]
fn history_and_completion_put_the_cursor_at_the_end_with_no_selection() {
  let (mut console, mut input) = console_with("", 0);
  console.command_history = vec!["first one".to_string()];
  frame(&mut console, &mut input, &[], "x");
  chord(&mut console, &mut input, CMD, KeyCode::KeyA);
  assert!(console.edit.selection().is_some());
  tap(&mut console, &mut input, KeyCode::ArrowUp);
  assert_eq!(console.input_buffer, "first one");
  assert_eq!(console.edit.cursor(), 9);
  assert_eq!(console.edit.selection(), None);

  chord(&mut console, &mut input, CMD, KeyCode::KeyA);
  tap(&mut console, &mut input, KeyCode::ArrowDown);
  assert_eq!(console.input_buffer, "x");
  assert_eq!(console.edit.cursor(), 1);
  assert_eq!(console.edit.selection(), None);

  console.input_buffer = "he".to_string();
  console.edit.select_all("he");
  console.complete(|_| vec!["help".to_string()]);
  assert_eq!(console.input_buffer, "help ");
  assert_eq!(console.edit.cursor(), 5);
  assert_eq!(console.edit.selection(), None);
}

#[test]
fn the_cursor_and_selection_respect_character_boundaries() {
  let line = "a\u{e9}\u{1F600}b";
  let (mut console, mut input) = console_with(line, line.len());
  for expected in [7, 3, 1, 0] {
    tap(&mut console, &mut input, KeyCode::ArrowLeft);
    assert_eq!(console.edit.cursor(), expected);
  }
  tap(&mut console, &mut input, KeyCode::End);
  tap(&mut console, &mut input, KeyCode::Backspace);
  assert_eq!(console.input_buffer, "a\u{e9}\u{1F600}");
  tap(&mut console, &mut input, KeyCode::Backspace);
  assert_eq!(console.input_buffer, "a\u{e9}");
  frame(&mut console, &mut input, &[(SHIFT, Pressed)], "");
  tap(&mut console, &mut input, KeyCode::ArrowLeft);
  frame(&mut console, &mut input, &[(SHIFT, Released)], "");
  assert_eq!(console.edit.selection(), Some(1..3));
  tap(&mut console, &mut input, KeyCode::Delete);
  assert_eq!(console.input_buffer, "a");
}

#[test]
fn a_typed_tab_is_not_typed() {
  let (mut console, mut input) = console_with("", 0);
  frame(&mut console, &mut input, &[], "a\tb");
  assert_eq!(console.input_buffer, "ab");
}
