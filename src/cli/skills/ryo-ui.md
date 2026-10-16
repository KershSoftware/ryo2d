---
name: ryo-ui
description: Build game UI in the Scene::ui hook with immediate-mode and retained-mode widgets including buttons, sliders, checkboxes, text inputs, dropdowns, scroll views, and layout containers, and consume the input the UI uses. Use when creating HUDs, menus, settings screens, or any in-game interface.
metadata:
  version: 3.0.0
---

# Ryo2D UI Specialist

Build game UI with `UiContext` inside `Scene::ui`. Complete signatures are in `ryo-api`. Text in depth (fonts, markup, effects, the typewriter) is in `ryo-text`.

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
- Every call records a command; the UI camera draws them on the GPU into the UI image, which is composited last, above the world, lighting and post-processing. The UI is never lit. Engine overlays (FPS, stats) and the console draw over it, open dropdown lists included.
- **Alpha blends in sRGB space, as on the web:** a 50% black panel over white is mid grey (128), not the lighter result linear blending would give.
- Cameras never move, zoom, turn or shake the UI.

## UI Units and Scale

- Every coordinate and size is in **UI units**. With the automatic scale (the default) a UI unit is a screen unit at any display scale: the window in logical pixels, or the pixel-art target. Existing `draw_rect` / `draw_text` coordinates keep their meaning, and text is laid out at render pixels, so it is sharp on HiDPI displays.
- `ui.screen_size()` is the UI's size in UI units `(u32, u32)`; `ui.ui_scale()` is render pixels per UI unit (`f32`).
- **`ui_scale` is `Option<f32>`.** `None` (the default) is automatic. `Some(2.0)` draws the whole UI, rects and text alike, twice as big, and mouse positions are converted to match. Set it with `EngineBuilder::ui_scale(Some(2.0))` or at runtime with `ctx.request_ui_scale(Some(2.0))` (applied after the frame; `None` goes back to automatic). A value that is not a positive number is warned about and treated as `None`.
- **`UiResolution`** (`EngineBuilder::ui_resolution`): `Render` (the default) draws the UI at the render size, so in pixel-art mode the UI is pixel art too; `Window` draws it at the window's resolution (smooth text and shapes over pixel art).
- `ui.dt()` is the **real** frame delta (it keeps running through pause, slow motion and hitstop). Animate UI with it.

## Input and Consumption

The UI pass reads the frame's input through `ui.input()` (a `UiInput` snapshot: `key_pressed`, `key_down`, `mouse_position`, `mouse_pressed`, `scroll_delta`, `text_input`, `gamepad_button_pressed`, `left_stick`, ...). What the UI consumes is applied to `Input` before the fixed steps, so `ctx.input` in `update` reads it as **not pressed, held state included**, for the rest of the frame.

| Consumed by | What |
|-------------|------|
| `button`, `checkbox`, `toggle`, `text_input`, `dropdown` | the left click they act on |
| `slider`, a `scroll_view`'s scrollbar (vertical or horizontal) | the press and drag |
| A hovered `scroll_view` whose content overflows | the mouse wheel |
| A focused text input | the whole keyboard and the typed text |
| An open dropdown | Escape (closes the list) |
| `ui.consume_click()` | the left click |
| `ui.consume_escape()` | Escape |
| `ui.consume_scroll()` | the mouse wheel (say, over a panel, so the camera does not zoom) |
| `ui.consume_key(key)` | any key |
| `ui.input().consume_text()` | the frame's typed text (`ctx.input.text_input()`) |

- Keys a menu acts on: read them from `ui.input()` and consume them so gameplay does not act too.

  ```rust
  if ui.input().key_pressed(KeyCode::Escape) {
    ui.consume_escape();
    return SceneAction::Pop;
  }
  ```

- A press the UI takes stays the UI's until the button is released: gameplay sees neither its held frames nor its release, so holding the mouse on a button never fires a weapon. The mouse wheel is not consumed.
- A key consumed earlier in the pass also reads as not pressed through `ui.input()`, so Escape that closed a dropdown does not also close the menu. `ui.click_consumed()` / `ui.escape_consumed()` ask directly.
- Gameplay keys stay in `update` with `ctx.input`; a click the UI took never reaches them.
- A focused text field takes the keyboard: `update` sees no keys while one has focus. `ui.wants_keyboard()` (true while a text input is focused or a dropdown is open) is still there for code that wants to know.
- Scenes below a transparent overlay draw first but run their `ui` with the pointer occluded, so the overlay's widgets get the clicks. They still see the keyboard.

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

### Text

Text is laid out and drawn by the text engine (Inter by default, 14 px). The shorthand family uses the default style; `ui.text(..)` takes every option. See `ryo-text` for fonts, `TextStyle`, markup, effects and the typewriter.

```rust
// Shorthand: default style (FontRole::Ui, 14 px), colour as given
ui.draw_text("Hello", x, y, Color::WHITE);
ui.draw_text_anchored("Score: 100", Anchor::TopRight, -10.0, 10.0, Color::YELLOW);
ui.draw_text_centered("Title", rect, Color::WHITE);
ui.draw_text_wrapped("Long text...", x, y, max_width, Color::WHITE);   // max_width: u32
ui.draw_text_wrapped_centered("Long text...", rect, Color::WHITE);
ui.label("Paused", x, y, Color::WHITE, Some(Color::PANEL_BG));

// Styled
let style = TextStyle::new().bold().size(24.0).outline(2.0, Color::BLACK);
ui.draw_text_styled("Level 3", x, y, &style);

// Every option
let size = ui.text(RichText::markup("Deal [color=#f44]20[/color] damage"))
  .style(&style)             // or .size(..) / .color(..)
  .at(x, y)
  .wrap(300.0)
  .align(TextAlign::Center)
  .max_lines(3)
  .ellipsis()
  .anchor(Anchor::TopLeft)
  .opacity(0.8)              // fades without a relayout
  .draw();                   // -> Vec2, the size in UI units

// Measure
let size: Vec2 = ui.measure_text("Hello", &style);
let line: f32 = ui.line_height(&style);
let (w, h) = ui.text_size("Hello");                                    // default style, (u32, u32)
let lines = ui.wrap_text("Long text...", max_width);                    // Vec<String>
let lines = ui.wrap_text_styled("Long text...", 300.0, &style);
```

- **Fade text through opacity** (`ui.text(..).opacity(a)`, or the alpha of the colour you give `draw_text`), never by changing a style's colour each frame: colour is part of the layout cache key, so that lays the text out again every frame.
- `.typewriter(&mut TextReveal)` reveals a text over time on the UI's real `dt` (call it once per frame per reveal; check `reveal.is_done()`); `.reveal(shown)` and `.fade(..)` are the manual controls.

### Shapes, sprites and lines

```rust
ui.draw_rect(rect, Color::RED);
ui.draw_rect_outline(rect, Color::WHITE, 2.0);                          // thickness inside the rect
ui.draw_rounded_rect(rect, 8.0, Color::DARK_GRAY);                      // radius in UI units
ui.draw_rect_bordered(rect, fill, 8.0, 2.0, border_color);              // fill, radius, border width, border colour
ui.draw_panel(rect, Color::DARK_GRAY, Some(Color::WHITE));
ui.draw_progress_bar(rect, 0.75, Color::HEALTH_RED, Color::DARK_GRAY);
ui.draw_progress_bar_with_border(rect, 0.75, fill, bg, border);

// Sprites are SpriteHandles
ui.draw_sprite(&handle, x, y);                                          // one UI unit per texel
ui.draw_sprite_scaled(&handle, x, y, 2.0);
ui.draw_sprite_rect(&handle, rect, Color::WHITE);                       // stretched over a rect, tinted
ui.draw_nine_slice(&handle, rect, [8.0, 8.0, 8.0, 8.0], Color::WHITE);  // insets [left, top, right, bottom] in texels

// Lines
ui.draw_line(&[Vec2::new(10.0, 10.0), Vec2::new(90.0, 40.0)], 2.0, Color::WHITE);   // a polyline, width in UI units
```

- Rects are drawn analytically (rounded corners, an inner border, anti-aliased edges). Nine-slice insets clamp to the texture, and the corners shrink when the rect is smaller than their sum.
- UI sprites use linear filtering in the default presentation.
- Text, rects and sprites are drawn in submission order (later on top).

## Interactive Widgets

```rust
if ui.button("Start Game", Rect::new(100.0, 200.0, 150.0, 40.0)) { /* clicked */ }
if ui.button("Custom").at(rect).variant("danger").clicked() { }   // a theme variant (see Themes)
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

Cursor, selection, clipboard (Ctrl+C/V/X), Shift+arrows, Home/End, IME. A click anywhere else unfocuses it. The text engine places the caret and the selection, so editing works in any script (the fonts must have the characters; see `ryo-text`).

### Dropdown

```rust
// field: difficulty: DropdownState  (DropdownState::with_selected(1))
if ui.dropdown(&mut self.difficulty, rect, &["Easy", "Normal", "Hard"]) {
  let index = self.difficulty.selected();
}
```

An open list draws on the **popup layer**, above everything else the scenes drew (only the engine's overlays and the console draw above it), with nothing to call: the engine flushes it after each scene's `ui`. Only the top scene's lists use the popup layer; a lower scene's open lists stay below the scenes above it.

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

Nested clips intersect, with no depth limit; popping an empty stack does nothing (warned once). Pair every `push_clip` with `pop_clip`. A clip is a scissor per command, in UI units.

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

The theme is the `UiTheme` world resource: a palette, metrics (radius, timings, widget sizes, the focus ring), text roles, a look for each widget kind and state (`Normal`, `Hovered`, `Pressed`, `Focused`, `Disabled`; a state changes only the fields it names), named variants, nine-slice skins and sounds. Widgets read it once per pass, so a change applies from the next frame.

```rust
EngineBuilder::new().ui_theme(Theme::pixel());                // or Theme::dark() (the default), Theme::light()
ctx.ui_theme().set(Theme::load("assets/ui/theme.ron")?);      // a RON file is merged over dark, so it states only what it changes; a variant's `base`, a text role's `font` and a skin's `filter` are strings there (`base: "Button"`)
ctx.ui_theme().modify(|t| t.palette.accent = Color::rgb(255, 120, 40));

// A variant: a named change to a widget kind's look; builtin themes carry "danger", "accent" and "tab"
ctx.ui_theme().modify(|t| {
  t.variants.insert("gold".into(), Variant::of(WidgetKind::Button)
    .normal(LookPatch::default().fill(Color::rgb(200, 160, 40)))
    .hovered(LookPatch::default().fill(Color::rgb(220, 180, 60))));
});
if ui.button("Buy").variant("gold").clicked() { }
ui.button("One-off").style(LookPatch::default().fill(Color::RED));   // changes only what it names, in every state
```

A theme loaded from a file reloads when the file changes, in debug builds. A look that names a skin draws its nine-slice sprite (`Skin::from_path`, `Skin::from_sprite`) in place of its fill and border, the fill tinting it. Today the button, the panel, the scrollbar and the focus ring read the theme; the checkbox, toggle, slider, text input and dropdown calls keep their own colours until they are ported. Plain `ui.text(..)` and `draw_text*` keep the default text style.

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
  SceneAction::Continue
}
```

## Tips

- Open dropdown lists draw on the popup layer by themselves; there is no call to make.
- `draw_modal_overlay(color)` dims the whole UI area, ignoring the current clip.
- Retained widget state is owned by the scene, never by `UiContext`.
- Many-sprite or world-space visuals belong in `Scene::render` (GPU batches) or in entities, not the UI.
