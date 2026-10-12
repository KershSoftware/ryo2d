---
name: ryo-ui
description: Build game UI in the Scene::ui hook with immediate-mode and retained-mode widgets including buttons, sliders, checkboxes, text inputs, dropdowns, scroll views, and layout containers, and consume the input the UI uses. Use when creating HUDs, menus, settings screens, or any in-game interface.
metadata:
  version: 2.0.0
---

# Ryo2D UI Specialist

Build game UI with `UiContext` inside `Scene::ui`. Complete signatures are in `ryo-api`.

## The `ui` Hook

```rust
impl Scene for Hud {
  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
    let score = ctx.resource::<Score>().map_or(0, |s| s.0);
    ui.draw_text_anchored(&format!("Score: {score}"), Anchor::TopRight, -10.0, 10.0, Color::YELLOW);
    if ui.button("Pause", Rect::new(10.0, 10.0, 80.0, 28.0)) {
      return SceneAction::Push(Box::new(PauseMenu));
    }
    SceneAction::Continue
  }
}
```

- Runs **once per frame, before the fixed steps** (`update`), for every visible scene bottom-up (the top scene, plus the scenes under a chain of `is_transparent` scenes).
- Only the top scene's returned `SceneAction` is applied, and it applies at once: when it brings a new scene to the top, the UI is drawn again so the new scene shows this frame, and no fixed step runs after a `Quit`.
- `ctx` is the full `SceneContext`: read the world or resources for a HUD, write messages, request window changes.
- The UI is drawn into its own buffer and composited last, above the world, lighting and post-processing; it is never lit. Engine overlays (FPS, stats) and the console draw over it.
- Coordinates are screen pixels; `ui.screen_size()` is `(u32, u32)`.
- `ui.dt()` is the **real** frame delta (it keeps running through pause, slow motion and hitstop). Animate UI with it.

## Input and Consumption

The UI pass reads the frame's input through `ui.input()` (a `UiInput` snapshot: `key_pressed`, `key_down`, `mouse_position`, `mouse_pressed`, `scroll_delta`, `text_input`, `gamepad_button_pressed`, `left_stick`, ...). What the UI consumes is applied to `Input` before the fixed steps, so `ctx.input` in `update` reads it as **not pressed, held state included**, for the rest of the frame.

| Consumed by | What |
|-------------|------|
| `button`, `button_styled`, `checkbox`, `toggle`, `text_input`, `dropdown` | the left click they act on |
| A focused text input or an open dropdown | Escape (leaves the field, closes the list) |
| `ui.consume_click()` | the left click |
| `ui.consume_escape()` | Escape |
| `ui.consume_key(key)` | any key |
| `ui.input().consume_text()` | the frame's typed text (`ctx.input.text_input()`) |

- Keys a menu acts on: read them from `ui.input()` and consume them so gameplay does not act too.

  ```rust
  if ui.input().key_pressed(KeyCode::Escape) {
    ui.consume_escape();
    return SceneAction::Pop;
  }
  ```

- `slider` and `scroll_view` do not consume: if gameplay reads the mouse, call `ui.consume_click()` while the pointer is over them (`ui.is_hovered(rect)`).
- A key consumed earlier in the pass also reads as not pressed through `ui.input()`, so Escape that closed a dropdown does not also close the menu. `ui.click_consumed()` / `ui.escape_consumed()` ask directly.
- Gameplay keys stay in `update` with `ctx.input`; a click the UI took never reaches them.
- Typing is not consumed: a focused text field leaves letter keys visible to `update`. Record `ui.wants_keyboard()` (true while a text input is focused or a dropdown is open) at the end of `ui` and skip key bindings in `update` while it is set; the UI runs first, so the flag is current for the same frame.
- Scenes below a transparent overlay draw first and see input first. Do not draw clickable widgets in a scene that is covered by an overlay (track it with `on_pause` / `on_resume`).

## Core Types

```rust
Color::WHITE, Color::YELLOW, Color::DARK_GRAY, Color::PANEL_BG, Color::HEALTH_RED, ...
Color::hex(0xFF8000)            // RGB
Color::hex(0xFF800080)          // RGBA
Color::rgb(255, 128, 0)
Color::new(255, 128, 0, 128)    // RGBA
color.with_alpha(128)

Rect::new(x, y, width, height)
Rect::centered_at(cx, cy, width, height)
Rect::from_pos_size((x, y), (w, h))
rect.contains(px, py); rect.center(); rect.right(); rect.bottom()
rect.shrink(padding); rect.expand(padding)

Anchor::TopLeft, TopCenter, TopRight, CenterLeft, Center, CenterRight, BottomLeft, BottomCenter, BottomRight
```

`Anchor` is the same type sprites use for `Renderable::with_anchor`.

## Immediate Mode Drawing

```rust
// Text (bitmap font)
ui.draw_text("Hello", x, y, Color::WHITE);
ui.draw_text_anchored("Score: 100", Anchor::TopRight, -10.0, 10.0, Color::YELLOW);
ui.draw_text_centered("Title", rect, Color::WHITE);
ui.draw_text_wrapped("Long text...", x, y, max_width, Color::WHITE);   // max_width: u32
ui.draw_text_wrapped_centered("Long text...", rect, Color::WHITE);
ui.label("Paused", x, y, Color::WHITE, Some(Color::PANEL_BG));
let (w, h) = ui.text_size("Hello");                                    // (u32, u32)
let lines = ui.wrap_text("Long text...", max_width);                    // Vec<String>

// Shapes
ui.draw_rect(rect, Color::RED);
ui.draw_rect_outline(rect, Color::WHITE, 2.0);
ui.draw_panel(rect, Color::DARK_GRAY, Some(Color::WHITE));
ui.draw_progress_bar(rect, 0.75, Color::HEALTH_RED, Color::DARK_GRAY);
ui.draw_progress_bar_with_border(rect, 0.75, fill, bg, border);

// Sprites (uniform scale)
ui.draw_sprite(&sprite, x, y);
ui.draw_sprite_scaled(&sprite, x, y, 2.0);
ui.draw_sprite_handle(&handle, x, y);
ui.draw_sprite_handle_scaled(&handle, x, y, 2.0);
```

## Interactive Widgets

```rust
if ui.button("Start Game", Rect::new(100.0, 200.0, 150.0, 40.0)) { /* clicked */ }
if ui.button_styled("Custom", rect, normal_bg, hover_bg, pressed_bg, text_color) { }
if ui.slider(rect, &mut self.volume, 0.0, 1.0) { /* changed */ }
if ui.checkbox(rect, &mut self.enabled, Some("Enable Sound")) { /* toggled */ }
if ui.toggle(rect, &mut self.on) { /* toggled */ }

ui.is_hovered(rect); ui.is_clicked(rect); ui.is_held(rect); ui.point_in_clip(x, y);
```

## Retained Widgets

Their state structs live in your scene and persist across frames.

### Text Input

```rust
// field: name: TextInputState  (TextInputState::new() or ::with_text("Player"))
if ui.text_input(&mut self.name, rect, "Name", ui.dt()) {
  // text changed
}
let text = self.name.text();
```

Cursor, selection, clipboard (Ctrl+C/V/X), Shift+arrows, Home/End, IME. A click anywhere else unfocuses it.

### Dropdown

```rust
// field: difficulty: DropdownState  (DropdownState::with_selected(1))
if ui.dropdown(&mut self.difficulty, rect, &["Easy", "Normal", "Hard"]) {
  let index = self.difficulty.selected();
}
// Last in `ui`: draws open lists over everything else
ui.finish_dropdowns();
```

### Scroll View

```rust
// field: scroll: ScrollViewState
ui.scroll_view(&mut self.scroll, visible_rect, content_height, ui.dt(), |ui, offset| {
  for (i, item) in items.iter().enumerate() {
    let y = visible_rect.y + i as f32 * 30.0 - offset;
    ui.draw_text(item, visible_rect.x + 10.0, y, Color::WHITE);
  }
});
ui.scroll_view_horizontal(&mut self.scroll, rect, content_width, ui.dt(), |ui, offset| { });
```

Content is clipped to the view. `scroll_to_top()`, `scroll_to_bottom(content_height, view_height)`, `set_scroll_y(..)` on the state.

## Layout

```rust
let mut row = HBox::new(x, y, spacing).with_valign(VAlign::Center);
ui.button("One", row.next(80.0, 30.0));
ui.button("Two", row.next(80.0, 30.0));
let bounds = row.bounds();

let mut column = VBox::new(x, y, spacing).with_halign(HAlign::Center);
ui.button("Option 1", column.next(200.0, 30.0));

let (sw, sh) = ui.screen_size();
let stack = Stack::new(0.0, 0.0, sw as f32, sh as f32);  // or Stack::from_rect(rect)
let (header, body) = stack.split_vertical(50.0);
let (sidebar, content) = stack.split_horizontal(200.0);
let dialog = stack.centered(300.0, 200.0);
let corner = stack.aligned(150.0, 100.0, HAlign::Right, VAlign::Top);
let inner = stack.padded(20.0);
```

## Clipping

```rust
ui.push_clip(Rect::new(10.0, 10.0, 200.0, 100.0));
ui.draw_text("clipped", 5.0, 5.0, Color::WHITE);
ui.pop_clip();

ui.with_no_clip(|ui| ui.draw_rect(full_screen, Color::BLACK));
ui.has_clip(); ui.clip_depth(); ui.current_clip();
```

Nested clips intersect; the stack is 16 deep and pushing past it panics. Pair every `push_clip` with `pop_clip`.

## Focus

```rust
ui.handle_tab_navigation(&mut [&mut self.username, &mut self.password]);   // any Focusable
ui.handle_dropdown_tab_navigation(&mut [&mut self.dropdown_a, &mut self.dropdown_b]);

if self.username.is_focused() {
  ui.draw_focus_ring(rect, Color::BLUE, 2.0, 2.0);   // thickness, offset
}

// Modals: Tab cycles inside the trap
let mut fields: [&mut dyn Focusable; 2] = [&mut self.field_a, &mut self.field_b];
let mut trap = FocusTrap::new(&mut fields);
trap.focus_first();   // also focus_next, focus_prev, focus_last, is_capturing
```

## Themes

```rust
let theme = Theme::default();                     // also Theme::dark(), Theme::light()
let theme = Theme::load("assets/theme.ron")?;
let hot = HotReloadTheme::new("assets/theme.ron")?;   // created with defaults if missing
hot.poll_reload();                                     // reloads in debug builds only
let theme = hot.get();
```

The `UiContext` the engine passes to `ui` has no theme, so `draw_themed_panel`, `draw_themed_progress_bar` and `draw_focus_ring_themed` use the default colours there. Pass explicit colours (or colours read from your `Theme`) to the plain draw calls.

## UI Tweens

`UiTween<T>` (`TweenFloat`, `TweenVec2`, `TweenColor`, `TweenRect`) is owned and advanced by your scene; nothing drives it for you. Advance it in `ui` with `ui.dt()` so it keeps playing during pause.

```rust
// field: fade: TweenFloat = TweenFloat::new(0.0, 1.0, 0.4).with_ease(Ease::OutQuad)
self.fade.update(ui.dt());
let alpha = (self.fade.value() * 255.0) as u8;
ui.draw_text("Ready", 300.0, 200.0, Color::WHITE.with_alpha(alpha));
```

`with_delay`, `with_loop(UiTweenLoop::PingPong | Loop | Times(n))`, `on_complete`, `retarget`, `restart`, `is_finished`.

## Patterns

### Pause Overlay

```rust
struct PauseMenu;

impl Scene for PauseMenu {
  fn is_transparent(&self) -> bool {
    true
  }

  fn is_physics_paused(&self) -> bool {
    true
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    let (sw, sh) = ui.screen_size();
    ui.draw_modal_overlay(Color::new(0, 0, 0, 180));
    let mut column = VBox::new(sw as f32 / 2.0 - 75.0, sh as f32 / 2.0 - 60.0, 10.0);
    ui.draw_text_centered("PAUSED", column.next(150.0, 40.0), Color::WHITE);
    if ui.button("Resume", column.next(150.0, 40.0)) || ui.input().key_pressed(KeyCode::Escape) {
      ui.consume_escape();
      return SceneAction::Pop;
    }
    if ui.button("Quit", column.next(150.0, 40.0)) {
      return SceneAction::Quit;
    }
    SceneAction::Continue
  }
}
```

The scene below stops updating (unless the overlay's `updates_below` is true) and still draws.

### Form

```rust
fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
  let mut column = VBox::new(50.0, 50.0, 15.0);

  let row = column.next(350.0, 30.0);
  ui.draw_text("Name:", row.x, row.y + 8.0, Color::WHITE);
  ui.text_input(&mut self.name, Rect::new(row.x + 100.0, row.y, 250.0, 30.0), "", ui.dt());

  let row = column.next(350.0, 30.0);
  ui.draw_text("Volume:", row.x, row.y + 8.0, Color::WHITE);
  ui.slider(Rect::new(row.x + 100.0, row.y + 5.0, 250.0, 20.0), &mut self.volume, 0.0, 1.0);

  let row = column.next(350.0, 30.0);
  ui.draw_text("Difficulty:", row.x, row.y + 8.0, Color::WHITE);
  ui.dropdown(&mut self.difficulty, Rect::new(row.x + 100.0, row.y, 150.0, 30.0), &["Easy", "Normal", "Hard"]);

  ui.handle_tab_navigation(&mut [&mut self.name]);
  self.typing = ui.wants_keyboard();
  ui.finish_dropdowns();
  SceneAction::Continue
}
```

## Tips

- Call `finish_dropdowns()` last in `ui` when the scene has dropdowns.
- `draw_modal_overlay(color)` dims the screen and clears the clip stack.
- Retained widget state is owned by the scene, never by `UiContext`.
- Many-sprite or world-space visuals belong in `Scene::render` (GPU batches) or in entities, not the UI.
