---
name: ryo-ui
description: Build game UI in the Scene::ui hook - widgets as builders laid out in rows, columns and grids, focus scopes for keyboard and gamepad navigation, scroll areas, popups and modals, drag and drop, themes, and the input the UI consumes. Use when creating HUDs, menus, settings screens, inventories, dialogue or any in-game interface.
metadata:
  version: 4.0.0
---

# Ryo2D UI Specialist

Build game UI with `UiContext` inside `Scene::ui`. Complete signatures are in `ryo-api`. Text in depth (fonts, markup, effects, the typewriter) is in `ryo-text`; actions and pads in `ryo-controller`.

The model in one breath: every frame `ui` calls builders (`ui.button("Play")`), containers lay them out (`ui.column().show(|ui| ..)`), and each widget answers from what the player did to what was drawn last frame (`.clicked()`). Your game's values stay in your scene; what a widget remembers of itself (an open list, a caret, a scroll offset, the focus) lives in the UI's memory under its id. Keyboard and pad navigation work inside **focus scopes** you mark, so a menu is pad-playable and a HUD never steals the arrows.

## 1. The `ui` Hook

```rust
impl Scene for Hud {
  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
    let score = ctx.resource::<Score>().map_or(0, |s| s.0);
    ui.text(format!("Score {score}")).anchor(Anchor::TopRight);
    if ui.button("Pause").at(Rect::new(10.0, 10.0, 80.0, 28.0)).clicked() {
      return SceneAction::Push(Box::new(PauseMenu::default()));
    }
    SceneAction::Continue
  }
}
```

- Runs **once per frame, before the fixed steps** (`update`), for every visible scene bottom-up (the top scene, plus the scenes under a chain of `is_transparent` scenes).
- **The top-scene rule.** Only the top scene's widgets take input: pointer, wheel, focus, keys and Back. A lower scene still draws, but its widgets report nothing (no `clicked`, a focused text field stops getting keys) and `ui.back()` is false there. A pause menu pushed over gameplay gets every click.
- Only the top scene's returned `SceneAction` applies, and it applies at once: when it brings a new scene to the top, the UI is drawn again so the new scene shows this frame. While a scene transition runs, a scene change returned from `ui` is ignored (only `Quit` applies), so a second click cannot restart a fade.
- `ctx` is the full `SceneContext`: read the world for a HUD, write messages, change the theme (`ctx.ui_theme()`) or the actions (`ctx.actions()`).
- The UI is drawn into its own image, composited last, above the world, lighting and post-processing; it is never lit. Engine overlays and the console draw above it. Alpha blends in sRGB space, as on the web: a 50% black panel over white is mid grey (128).
- `ui.dt()` is the **real** frame delta (it runs through pause, slow motion and hitstop).

## 2. UI Units

- Every coordinate and size is in **UI units**. With the automatic scale (the default) a UI unit is a screen unit at any display scale: the window in logical pixels, or the pixel-art target. Text is laid out at render pixels, so it is sharp on HiDPI displays.
- `ui.screen_size()` is the UI's size in UI units `(u32, u32)`; `ui.ui_scale()` is render pixels per UI unit.
- `EngineBuilder::ui_scale(Some(2.0))` (or `ctx.request_ui_scale(Some(2.0))` at runtime) draws the whole UI twice as big; `None` is automatic. A value that is not a positive number is warned and treated as `None`.
- `EngineBuilder::ui_resolution(UiResolution::Render)` (default) draws the UI at the render size, so in pixel-art mode the UI is pixel art too; `UiResolution::Window` draws it at window resolution over pixel art.

## 3. Widgets Are Builders

```rust
if ui.button("Play").clicked() { /* .. */ }      // shorthand: draw it, say whether it was clicked
let r = ui.slider(&mut self.volume, 0.0..=1.0).step(0.05).show();   // .show() gives the Response
ui.button("Delete").variant("danger");           // a statement: a builder records itself when dropped
```

- **Ids come from the call site**: the line a builder is called on, how often that line ran in the enclosing container this frame, and the container. A helper called in a loop gets one id per call. Skipping an earlier call (a conditional widget) shifts the later ones by one, so their state (focus, an open list) moves with them.
- **Lists that reorder or filter use `.id(key)`**: the key replaces the count, so a row keeps its focus and state when the list is sorted. Keys are per container; a duplicate key is suffixed and warned once, never shared.

  ```rust
  for item in &self.items {
    if ui.button(item.name.as_str()).id(item.id).clicked() { self.selected = Some(item.id); }
  }
  ```

- **`ui.scope(key, |ui| ..)`** keys everything inside without adding a layout node: a component drawn twice, or a helper drawing several widgets per item.

  ```rust
  for item in &self.items {
    ui.scope(item.id, |ui| item_row(ui, item));
  }
  ```

- **Containers need `.show(|ui| ..)`**; widgets record on `.show()`, on a shorthand (`.clicked()`, a choice list's `.show() -> Option<usize>`), or on drop.
- **`Response`** (from `.show()`): `id`, `rect`, `hovered()`, `pressed()`, `clicked()` (released over it, or Confirm while focused), `secondary_clicked()`, `double_clicked()`, `drag_started()`, `dragging()`, `drag_delta()`, `drag_released()`, `changed()` (a value widget changed your data), `focused()`, `gained_focus()`, `lost_focus()`, `submitted()` (Enter in a text field), `pointer()`, plus drag and drop's `dropped::<T>()`, `drag_hover::<T>()`, `drag_started_here()`.
- **Clicks land on what was drawn.** The pointer is resolved once a frame, before any `ui`, against last frame's rects: a click hits what the player saw under the pointer, a widget not drawn this frame never reports `clicked()`, and the topmost widget wins (a panel blocks what is under it). A click is the release over the widget it was pressed on.
- **Shared options on every widget:** `.id(key)`, `.enabled(bool)` (dimmed, reports nothing, still blocks the pointer), `.at(rect)`, `.width(..)`, `.height(..)`, `.fill_width()`, `.margin(..)`, `.anchor(..)`, `.offset(x, y)`, `.grow(w)`, `.min_width(w)`, `.max_width(w)`, `.tooltip(text)`, `.variant(name)`, `.style(LookPatch)`, the focus options (section 7) and drag and drop (section 9).

## 4. Layout

Containers lay their children out with flexbox and grid. A top-level container (one with no container open around it) is laid out when its `.show()` returns, so its `Response` has this frame's rect; a container inside another has last frame's.

```rust
ui.panel()                       // a column with the panel look and padding; it blocks the pointer
  .anchor(Anchor::Center)        // top level: placed in the UI by its anchor (default TopLeft)...
  .offset(0.0, -40.0)            // ...and moved by its offset
  .width(320.0)
  .gap(8.0)
  .show(|ui| {
    ui.text("Options");
    ui.row().gap(8.0).align(Align::Center).show(|ui| {
      ui.text("Volume");
      ui.spacer();               // takes the room left, pushing what follows to the end
      ui.slider(&mut self.volume, 0.0..=1.0).width(160.0);
    });
    ui.separator();              // a line across: horizontal in a column, vertical in a row
    ui.button("Done").fill_width();
  });
```

| Container | Lays out |
|---|---|
| `ui.column()` | top to bottom, as wide as the widest child (children stretch across) |
| `ui.row()` | left to right, as tall as the tallest |
| `ui.grid(columns)` | equal columns filled row by row; `.tracks(&[80.0, 160.0, 80.0])` sets each column's width |
| `ui.stack()` | children over each other, each placed by its own `.anchor(..)` and `.offset(..)` |
| `ui.panel()` | a column with the theme's panel look and padding, blocking the pointer |
| `ui.scroll()` | a column seen through a view (section 6) |

- **Sizes** are `Length`s: a number is UI units (`.width(120.0)`), `Length::Pct(0.5)` a fraction of the parent, `Length::Fill` the room the parent has left, `Length::Fit` the content (the default). `.fill_width()`, `.fill_height()`, `.fill()` are `Fill`; `.min_width`, `.max_width`, `.min_height`, `.max_height` bound them. A fixed-size widget (a button, a slider, a slot, an image) that fills is never smaller than its own size, or its `max_width` when that is smaller.
- **Spacing:** `.padding(8.0)`, `.padding((x, y))` or `Edges::new(l, t, r, b)`; `.margin(..)` outside; `.gap(g)` between children.
- **Alignment:** `.align(Align::Start | Center | End | Stretch)` across the flow (default `Stretch`), `.justify(Justify::Start | Center | End | SpaceBetween | SpaceAround | SpaceEvenly)` along it, `.center()` both. `.grow(w)` shares the room left by weight; `.aspect(ratio)` keeps a shape.
- **Wrapping:** `ui.row().wrap_items()` moves children that do not fit onto the next line (give it a width).
- **Anchors and offsets** place a top-level container (or a widget outside any container) in the UI, and a child in a `stack`: `Anchor::TopLeft`, `TopCenter`, `TopRight`, `CenterLeft`, `Center`, `CenterRight`, `BottomLeft`, `BottomCenter`, `BottomRight`. Inside a column or row they do nothing.
- **`.at(rect)`** places a widget or container at that rect, its size too, out of the flow, even inside a container: it takes no room there and moves no sibling, but keeps its parent's ids, focus scope and clip.
- **Text is a layout leaf**: `ui.text("..")` inside a container is measured, wrapped to the width it gets and placed like any widget. `.at(x, y)` takes it out of the layout. A theme's text roles style it: `let heading = ui.theme().text.heading.style();` then `ui.text("Title").style(&heading);`.
- **Degenerate input is safe**: NaN, negative or infinite sizes lay out as zero (warned once), and long lists lay out fine.

## 5. The Widget Catalogue

| Widget | One line | Notes |
|---|---|---|
| Button | `if ui.button("Play").clicked() {}` | `.icon(&sprite)` left of the label, `.variant("danger")`, `.style(LookPatch::default().fill(c))` |
| Text | `ui.text("Score 1200");` | a layout leaf; `.style`, `.size`, `.color`, `.wrap(w)`, `.align`, `.max_lines`, `.ellipsis()`, `.opacity`, `.typewriter(&mut reveal)` (see `ryo-text`) |
| Image | `ui.image(&sprite).size(32.0, 32.0).filter(Filter::Nearest);` | `.tint(c)`, `.uv([l, t, r, b])`; the sprite's size by default |
| Checkbox | `if ui.checkbox(&mut self.sound, "Sound").show().changed() {}` | the whole row clicks |
| Toggle | `ui.toggle(&mut self.fullscreen, "Fullscreen");` | a switch |
| Radio | `ui.radio(&mut self.difficulty, Difficulty::Hard, "Hard");` | any `PartialEq + Copy` value |
| Slider | `ui.slider(&mut self.volume, 0.0..=1.0).step(0.05).label("Volume").show_value();` | drag or click the track; Left/Right step it (a twentieth of the range without `.step`) |
| Progress | `ui.progress(self.health).label("HP").width(160.0);` | 0..1, clamped |
| Text field | `let r = ui.text_input(&mut self.name).placeholder("Name").max_len(16).show();` | one line; `.password()`; `r.submitted()` on Enter; caret, selection, clipboard, IME |
| Dropdown | `if ui.dropdown(&mut self.quality, &["Low", "High"]).width(160.0).show().changed() {}` | the list opens on the popup layer and traps focus; Back closes it |
| Tabs | `ui.tabs(&mut self.tab, &["Audio", "Video"]);` | share their row's width; Q/E and the shoulder buttons switch them |
| Choice list | `if let Some(i) = ui.choice_list(&["Yes", "No"]).show() {}` | a focus scope of choices; keys 1-9 pick; `.default(i)` |
| Slot | `ui.slot().icon(&sprite).count(3).selected(true);` | a square for inventories; no icon is an empty slot; `.filter(..)` |
| Prompt | `ui.prompt("jump");` | the key, button or glyph for an action on the player's last device |
| Rebind row | `ui.rebind("jump").label("Jump");` | Confirm waits for the next key or pad button; `.slot(1)` for the second binding |
| Custom | `ui.custom(120.0, 24.0).sense(Sense::click()).paint(\|p, r\| ..)` | paint in its own units with a `Painter` (below) |
| Separator, spacer | `ui.separator(); ui.spacer();` | a line across; empty room that grows |
| Tooltip | `ui.button("Save").tooltip("Saves to slot 1");` | after the theme's delay (0.5 s) of hover, or of focus with the ring showing |
| Disabled | `ui.button("Load").enabled(false);` | dimmed, never focused, reports nothing |
| Popup, modal, confirm | section 8 | |

**Custom widgets** paint in their own units: (0, 0) is their top-left wherever layout puts them, and `painter.size()` their size. The closure runs inside `ui`, so it may borrow the scene.

```rust
let gauge = ui.custom(120.0, 24.0).sense(Sense::click()).paint(|p, response| {
  let size = p.size();
  let fill = if response.hovered() { Color::YELLOW } else { Color::GRAY };
  p.rounded_rect(Rect::new(0.0, 0.0, size.x * self.charge, size.y), 4.0, fill);
  p.text("Charge", 4.0, 4.0, &TextStyle::new().size(12.0));
});
```

`Painter`: `size()`, `rect`, `rounded_rect`, `bordered_rect(rect, fill, radius, border_width, border_color)`, `text(text, x, y, &style)`, `sprite(&sprite, rect, tint)`, `line(&points, width, color)`. `Sense::hover()` (the default: clicks pass through to what is under it), `click()`, `drag()`, `click_and_drag()`. A custom widget is not focusable unless `.focusable(true)`; then `.takes_vertical()` / `.takes_horizontal()` keep those directions for it, read with `ui.nav_step(response.id)`, and `.takes_confirm(false)` leaves Space, Enter and A to gameplay while it is focused.

```rust
let list = ui.custom(200.0, 120.0).focusable(true).takes_vertical().paint(|p, _| draw_rows(p, &self.rows));
match ui.nav_step(list.id) {
  Some(Direction::Down) => self.row += 1,
  Some(Direction::Up) => self.row = self.row.saturating_sub(1),
  _ => {}
}
```

## 6. Scroll Areas

```rust
let area = ui.scroll().height(300.0).fill_width().gap(4.0).show(|ui| {
  for line in &self.log {
    ui.text(line.as_str());
  }
});
```

- Contents are laid out as a column (`.horizontal()` for a row, `.both()` for both ways) and shown through the view you size; they are clipped for drawing and for hits. `.no_bar()` hides the scrollbar.
- **The wheel** goes to the scroll area under the pointer (the topmost thing there decides, so a panel or popup over a list keeps the wheel from it), innermost first: an area at its edge passes the wheel to the one around it. Shift scrolls sideways. Scrolling eases (0.08 s). A wheel a scroll area took never reaches gameplay.
- **The pad** walks a long list: focus moves onto rows scrolled out of view and scrolls them in. The right stick scrolls the focused widget's area, or the first area in its focus scope (a dialog's text above its Close button).
- `ui.scroll_to(id)` brings a widget into view from the next frame; `ui.scroll_offset(area.id)` reads where an area shows from; `ui.set_scroll_offset(area.id, Vec2::new(0.0, 0.0))` jumps there without easing.

## 7. Focus and Navigation

```rust
ui.panel().focus_scope().wrap_focus().anchor(Anchor::Center).show(|ui| {
  ui.button("New Game").default_focus();
  ui.button("Options");
  ui.button("Quit");
});
```

- **Explicit scopes.** The arrows, the D-pad (and left stick), Tab, Confirm (Enter, Space, A) and Back (Escape, B) navigate only inside **focus scopes**: containers marked `.focus_scope()`, open dropdown lists, `ui.popup`s and modals. With no scope on screen they stay gameplay's, so a HUD button is mouse-only and never takes the arrows, Space or A. Every menu marks its container `.focus_scope()`. (A text field outside any scope still takes focus by click.)
- **Where focus starts.** When a scope appears and nothing is focused, focus goes to the widget it remembers, else its `.default_focus()` widget, else its first. Focus memory lasts for the scene's lifetime and covers the scopes around the focused widget too, so a menu returning from a scene pushed over it (a quit confirm answered No) restores its focus, nested scopes reopen where they were, and a page a dialog's answer shows, or a scene it switches to, takes its default.
- **Moving.** A direction goes to the explicit neighbour (`.nav_up(id)`, `.nav_down(id)`, `.nav_left(id)`, `.nav_right(id)`), else the nearest widget that way inside the scope, then out to the scope around it. `.wrap_focus()` wraps at a scope's edges; `.nav_skip()` lets directions pass a widget by (Tab and the pointer still reach it); `.focusable(false)` takes a widget out of navigation. Holding a direction repeats after 0.35 s, every 0.1 s. Tab moves in order through every focusable (Shift+Tab back).
- **Traps.** `.trap()` keeps focus inside a scope; modals, open lists and `ui.popup`s are traps. A trap opens on its `.default_focus()` widget, gives the focus back when it closes, and while it is open nothing outside it (pointer, request or press) takes the focus.
- **Focus never lands** on a disabled, hidden or clipped-out widget. Focus follows the pointer onto widgets, but never onto (or off) a text field: a field takes the keys only by a click, navigation or a request.
- **`ui.back()`** is true when Back was pressed and no widget used it (closing a list, leaving a text field, cancelling a drag or a rebind). It is true for the first caller only, and only in the top scene, so close the innermost thing first: `if ui.back() { return SceneAction::Pop; }`.
- **Reading and setting:** `ui.focused() -> Option<UiId>`, `ui.is_focused(id)`, `ui.request_focus(id)` (from the next frame), `response.focused()` / `gained_focus()` / `lost_focus()`. `ui.focus_visible()` is true when the last device was the keyboard or a pad; the focus ring (2 units wide, 2 outside the widget) shows only then.
- **Tabs and shoulder buttons.** Q/E and the bumpers (`ui.tab_prev` / `ui.tab_next`) switch the first `tabs` drawn in the top scene, and only when no trap is open or the tabs are inside the innermost one. With no tabs on screen they are not consumed, so Q and E stay gameplay's.
- **Sliders, text fields and custom widgets** that take a direction keep it while focused (Left/Right step a slider); the rest move focus.

## 8. Popups, Modals and `ui.confirm`

All three are built where you call them and drawn above the page when the scene's UI ends, on the popup layer.

```rust
// A modal: dims the UI, blocks the pointer, traps the focus, gives it back when it goes.
// It never closes itself: the game closes it, on a button or ui.back().
if self.options_open {
  let mut done = false;
  ui.modal().width(320.0).gap(12.0).show(|ui| {
    ui.text("Options").style(&heading);
    ui.toggle(&mut self.subtitles, "Subtitles");
    done = ui.button("Done").default_focus().clicked();
  });
  if done || ui.back() { self.options_open = false; }
}

// A yes/no question: opens on No, Back answers No. Keyed by its texts, so one dialog wherever it is called.
match ui.confirm("Quit to desktop?", "Unsaved progress is lost.") {   // or confirm_with(title, message, yes, no)
  Some(true) => return SceneAction::Quit,
  Some(false) => self.quitting = false,
  None => {}                                                           // still open
}

// A popup below a widget: a context menu. The game decides when it shows.
let more = ui.button("More").show();
if more.clicked() { self.menu_open = !self.menu_open; }
if self.menu_open {
  ui.popup(more.id).show(|ui| {
    if ui.button("Rename").default_focus().clicked() { self.menu_open = false; }
    if ui.button("Delete").variant("danger").clicked() { self.menu_open = false; }
  });
  if ui.back() { self.menu_open = false; }
}
```

- A modal's dialog panel is opaque; a page panel keeps the theme's translucency.
- **Back closes the innermost first**: an open dropdown list inside a modal takes Back itself; once it is gone `ui.back()` is the game's, to close the modal.
- `ui.wants_keyboard()` is true while an open list or `ui.popup` is up (not for a modal).
- Absolute `draw_*` calls inside a `ui.modal()` or `ui.popup()` closure draw onto the page, under the dim: draw inside a modal with widgets, `ui.custom`, or `ui.text`.
- A popup is placed 2 units below its anchor's rect, or above it when only that fits, kept on screen and at least as wide as the anchor.

## 9. Drag and Drop

Any widget or container can be a source and a target. Payloads are typed and the game decides what a drop means (a move, a swap, a merge): the UI only carries a clone of the source's payload to the target.

```rust
#[derive(Clone, Copy, PartialEq, Eq)]
enum Place { Bag(usize), Equip(usize) }

let r = ui.slot().id(i).icon(&icon).drop_target::<Place>().drag_source(Place::Bag(i)).show();
if let Some(from) = r.dropped::<Place>() { self.move_item(from, Place::Bag(i)); }
```

- **Mouse:** dragging a source past 4 UI units picks it up; releasing over a target accepting its type drops it (`dropped::<T>()` on the target, `drag_released()` on the source, never `clicked()`). A panel or modal over the target keeps the drop from it.
- **Pad:** Confirm on a focused source picks it up (`drag_started_here()`, not `clicked()`); Confirm on a focused target accepting the type puts it down; Back puts it back (so `ui.back()` is false while something is held).
- `r.drag_hover::<T>()` is true while a `T` is held over a target (highlight it); `ui.held::<P>()` is the payload held now, from the pick-up to the drop.
- **The ghost** follows the pointer (or sits beside the pad's focus) at 80% opacity: `.drag_ghost(&sprite)`, else a slot's icon, else a box in the accent colour.

## 10. Theme

The theme is the `UiTheme` world resource: a palette, metrics (radius, timings, widget sizes, the focus ring), text roles (`body`, `heading`, `button`, `caption`, `mono`), a look per widget kind and state, named variants, nine-slice skins and sounds. `UiContext` takes it once per pass, so a change applies from the next frame.

```rust
EngineBuilder::new().ui_theme(Theme::pixel());               // or Theme::dark() (the default), Theme::light()
ctx.ui_theme().set(Theme::load("assets/ui/theme.ron").unwrap_or_default());   // merged over dark; reloads when saved, in debug builds
ctx.ui_theme().modify(|t| t.palette.accent = Color::rgb(255, 120, 40));
let accent = ui.theme().palette.accent;                      // read it while drawing
let heading = ui.theme().text.heading.style();               // a text role as a TextStyle
```

- **Looks per kind and state.** `theme.widgets.button` (and `panel`, `checkbox`, `toggle`, `radio`, `slider`, `text_input`, `dropdown`, `tab`, `choice`, `tooltip`, `scroll`, `slot`, `progress`, `rebind`, `modal`, `prompt`) is a `Looks`: a `normal` `Look` (fill, border, border width, radius, text colour, padding, offset, skin) and `LookPatch`es for `hovered`, `pressed`, `focused` (with the ring showing) and `disabled`, each changing only what it names.
- **Variants** are named changes to a kind's looks, picked per widget with `.variant(name)`. The built-ins carry `danger`, `accent` and `tab` for buttons and `selected` for tabs. `Variant::disabled(..)` styles a disabled widget of the variant; without it, a disabled variant widget takes its kind's disabled look. An unknown name warns once and draws the plain look.

  ```rust
  ctx.ui_theme().modify(|t| {
    t.variants.insert("gold".into(), Variant::of(WidgetKind::Button)
      .normal(LookPatch::default().fill(Color::rgb(200, 160, 40)))
      .hovered(LookPatch::default().fill(Color::rgb(220, 180, 60))));
  });
  ui.button("Buy").variant("gold");
  ui.button("One-off").style(LookPatch::default().fill(Color::RED));   // this widget only, in every state
  ```

- **Skins** are nine-slice sprites a look draws in place of its fill and border (the fill tints it): `Theme::dark().skin("wood", Skin::from_path("assets/ui/wood.png", [6.0; 4], Filter::Nearest))`, then `LookPatch::default().skin("wood")` or `theme.widgets.panel.normal.skin = Some("wood".into())`. Insets are left, top, right, bottom in sprite pixels.
- **Sounds:** `theme.sounds` names files for `focus` (navigation moved), `confirm` (a click or Confirm), `back`, `tick` (a slider step, at most every 0.05 s) and `error` (a refused drop, a cancelled rebind), at `volume`. None by default.
- **RON files** state only what they change (a nested struct keeps dark's other fields; an option needs no `Some(..)`). A variant's `base`, a text role's `font` and a skin's `filter` are strings there:

  ```ron
  (
    palette: (accent: (r: 255, g: 150, b: 60, a: 255)),
    widgets: (button: (normal: (radius: 8.0), hovered: (fill: (r: 95, g: 55, b: 30, a: 255)))),
    variants: {"gold": (base: "Button", normal: (fill: (r: 200, g: 160, b: 40, a: 255)))},
  )
  ```

  `Theme::save(path)` writes one. A file that fails to load returns a `ThemeError` naming the path; a hot reload that fails warns once and keeps the old theme.

## 11. Input and Consumption

The UI pass runs before gameplay and consumes what it uses. What it consumed reads as **not pressed, held state included**, to `update` and systems for the rest of the frame.

| Consumed | When |
|---|---|
| A mouse press, its held frames and its release | it landed on any top-scene widget, panel, popup or modal (`ui.wants_pointer()` is true) |
| The wheel | a scroll area took it |
| The whole keyboard and the typed text | a text field has the focus |
| The bindings of a UI action, until released | navigation used it: a direction or Tab while a scope is on screen, Confirm with a focus, Back a widget used or `ui.back()` returned |
| Q/E and the bumpers | a drawn `tabs` widget took them |
| The digit key | a choice list picked by number |
| The captured key or button | a rebind row captured it |
| What you name | `ui.consume_key(key)`, `ui.consume_scroll()`, `ui.input().consume_text()` |

- **`ui.wants_pointer()`**: the pointer is over the top scene's UI, or a widget holds a press. Check it before acting on a click in the world in `ui`; in `update`, a click the UI took is already gone.
- **`ui.wants_keyboard()`**: a text field or a rebind row waiting for a press has the focus, or an open dropdown list or `ui.popup` is up. A scene reading keys in `ui` through `ui.input()` leaves them alone while it is true.
- Read keys a menu acts on through `ui.input()` (`key_pressed`, `mouse_position`, `scroll_delta`, `gamepad_button_pressed`, ...), but prefer `ui.back()` for Escape/B and actions for the rest. `ui.input()` is the same snapshot for every scene; only widgets and `ui.back()` follow the top-scene rule.
- With no focus scope on screen, the arrows, Space, Enter and A are never the UI's: gameplay keeps them under a HUD.

## 12. Absolute Drawing and Clips

For HUD bars, markers, debug overlays and shapes no widget covers. These draw at once, at the position given, outside layout.

```rust
ui.draw_rect(rect, Color::RED);
ui.draw_rect_outline(rect, Color::WHITE, 2.0);                          // thickness inside the rect
ui.draw_rounded_rect(rect, 8.0, Color::DARK_GRAY);
ui.draw_rect_bordered(rect, fill, 8.0, 2.0, border_color);              // fill, radius, border width, border colour
ui.draw_panel(rect, Color::DARK_GRAY, Some(Color::WHITE));
ui.draw_progress_bar(rect, 0.75, Color::HEALTH_RED, Color::DARK_GRAY);
ui.draw_sprite(&handle, x, y);                                          // one UI unit per texel; _scaled, _rect
ui.draw_nine_slice(&handle, rect, [8.0, 8.0, 8.0, 8.0], Color::WHITE);  // insets left, top, right, bottom in texels
ui.draw_line(&[Vec2::new(10.0, 10.0), Vec2::new(90.0, 40.0)], 2.0, Color::WHITE);
ui.draw_modal_overlay(Color::new(0, 0, 0, 160));                        // dims the whole UI, ignoring clips

ui.draw_text("Hello", x, y, Color::WHITE);                              // default style (FontRole::Ui, 14 px)
ui.draw_text_anchored("Score: 100", Anchor::TopRight, -10.0, 10.0, Color::YELLOW);
ui.draw_text_centered("Title", rect, Color::WHITE);
ui.text(RichText::markup("Deal [color=#f44]20[/color]")).at(x, y).size(20.0).draw();   // -> Vec2
let size: Vec2 = ui.measure_text("Hello", &style);
```

- **Fade text through opacity** (`ui.text(..).opacity(a)`, or the alpha of `draw_text`'s colour), never by changing a style's colour each frame: colour is part of the layout cache key.
- **Clips:** `ui.push_clip(rect)` / `ui.pop_clip()` nest and intersect, with no depth limit; popping an empty stack does nothing (warned once). `ui.with_no_clip(|ui| ..)` draws unclipped and restores the stack. `ui.current_clip()`, `has_clip()`, `clip_depth()`, `point_in_clip(x, y)`. Containers clip their own contents (scroll areas) without these.
- Draw order: everything a scene draws, absolute or laid out, keeps the order it was recorded in; popups, tooltips and the drag ghost draw above the page.

## 13. UI Tweens

`UiTween<T>` (`TweenFloat`, `TweenVec2`, `TweenColor`, `TweenRect`) is owned and advanced by your scene. Advance it in `ui` with `ui.dt()` so it keeps playing during pause.

```rust
// field: slide: TweenFloat = TweenFloat::new(-300.0, 0.0, 0.4).with_ease(Ease::OutBack)
self.slide.update(ui.dt());
ui.panel().anchor(Anchor::CenterLeft).offset(self.slide.value(), 0.0).show(|ui| { /* .. */ });
```

`with_delay`, `with_loop(UiTweenLoop::PingPong | Loop | Times(n))`, `on_start`, `on_complete`, `retarget`, `restart`, `pause`, `resume`, `stop`, `is_finished`, `progress`.

## 14. Recipes

Each compiles as written. They name each other's scenes (`Level`, `PauseMenu`, `SettingsScreen`, `QuitConfirm`, `MainMenu`).

### Main menu

```rust
/// The title screen: one menu the mouse, the keys and the pad all play
#[derive(Default)]
struct MainMenu {
  has_save: bool,
}

impl Scene for MainMenu {
  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    let title = ui.theme().text.heading.style().size(48.0);
    ui.column()
      .anchor(Anchor::TopCenter)
      .offset(0.0, 80.0)
      .show(|ui| {
        ui.text("RYO QUEST").style(&title);
      });

    let mut action = SceneAction::Continue;
    let has_save = self.has_save;
    ui.panel()
      .focus_scope() // the arrows, the D-pad, Enter and A move and pick in here
      .wrap_focus() // Down from Quit goes round to Continue
      .anchor(Anchor::Center)
      .width(260.0)
      .gap(8.0)
      .show(|ui| {
        let resume = ui.button("Continue").fill_width().enabled(has_save);
        if resume.clicked() {
          action = SceneAction::Switch(Box::new(Level::default()));
        }
        if ui.button("New Game").fill_width().default_focus().clicked() {
          action = SceneAction::Switch(Box::new(Level::default()));
        }
        if ui.button("Settings").fill_width().clicked() {
          action = SceneAction::Push(Box::new(SettingsScreen::default()));
        }
        if ui.button("Quit").fill_width().variant("danger").clicked() {
          action = SceneAction::Push(Box::new(QuitConfirm));
        }
      });

    let muted = ui.theme().palette.muted;
    ui.column()
      .anchor(Anchor::BottomCenter)
      .offset(0.0, -32.0)
      .show(|ui| {
        ui.text(RichText::markup(
          "[prompt=ui.confirm] Select    [prompt=ui.back] Quit",
        ))
        .color(muted);
      });
    if ui.back() {
      action = SceneAction::Push(Box::new(QuitConfirm));
    }
    action
  }
}
```

### Pause menu with a settings modal

```rust
/// Pushed over the level, which draws under it, paused
struct PauseMenu {
  settings_open: bool,
  volume: f32,
  shake: bool,
}

impl Default for PauseMenu {
  fn default() -> Self {
    Self {
      settings_open: false,
      volume: 0.8,
      shake: true,
    }
  }
}

enum Pause {
  Resume,
  Settings,
  Menu,
}

impl Scene for PauseMenu {
  fn is_transparent(&self) -> bool {
    true
  }

  fn is_physics_paused(&self) -> bool {
    true
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
    let heading = ui.theme().text.heading.style();
    ui.draw_modal_overlay(Color::new(0, 0, 0, 150));
    let mut choice = None;
    ui.panel()
      .focus_scope()
      .anchor(Anchor::Center)
      .width(240.0)
      .gap(8.0)
      .show(|ui| {
        ui.text("Paused").style(&heading);
        if ui.button("Resume").fill_width().default_focus().clicked() {
          choice = Some(Pause::Resume);
        }
        if ui.button("Settings").fill_width().clicked() {
          choice = Some(Pause::Settings);
        }
        if ui.button("Quit to menu").fill_width().clicked() {
          choice = Some(Pause::Menu);
        }
      });

    if self.settings_open {
      // A modal: it dims the menu, takes the pointer and holds the focus
      // until it closes, then gives the focus back to Settings
      let (volume, shake) = (&mut self.volume, &mut self.shake);
      let (mut changed, mut done) = (false, false);
      ui.modal().width(320.0).gap(12.0).show(|ui| {
        ui.text("Settings").style(&heading);
        changed = ui
          .slider(volume, 0.0..=1.0)
          .step(0.05)
          .label("Volume")
          .show_value()
          .show()
          .changed();
        ui.toggle(shake, "Screen shake").show();
        ui.row().justify(Justify::End).show(|ui| {
          done = ui.button("Done").default_focus().clicked();
        });
      });
      if changed {
        ctx.audio.set_master_volume(self.volume);
      }
      // Back closes the innermost first: the modal now, the menu after
      if done || ui.back() {
        self.settings_open = false;
      }
    } else if ui.back() {
      choice = Some(Pause::Resume);
    }

    match choice {
      Some(Pause::Resume) => SceneAction::Pop,
      Some(Pause::Settings) => {
        self.settings_open = true;
        SceneAction::Continue
      }
      Some(Pause::Menu) => SceneAction::Reset(Box::new(MainMenu { has_save: true })),
      None => SceneAction::Continue,
    }
  }
}
```

### Tabbed settings screen with rebinding

```rust
const TABS: [&str; 3] = ["Audio", "Video", "Controls"];
const WINDOWS: [&str; 3] = ["Windowed", "Borderless", "Fullscreen"];
/// The game's actions and their labels
const ACTIONS: [(&str, &str); 4] = [
  ("move_left", "Move left"),
  ("move_right", "Move right"),
  ("jump", "Jump"),
  ("attack", "Attack"),
];

struct SettingsScreen {
  tab: usize,
  music: f32,
  effects: f32,
  window: usize,
  vsync: bool,
}

impl Default for SettingsScreen {
  fn default() -> Self {
    Self {
      tab: 0,
      music: 0.6,
      effects: 0.8,
      window: 0,
      vsync: true,
    }
  }
}

/// The game's actions with their default bindings. `bind` again is
/// harmless: what the player rebound (or saved) stays.
fn bind_actions(actions: &mut Actions) {
  use Binding::{Key, Pad};
  actions
    .bind("move_left", [Key(KeyCode::KeyA), Key(KeyCode::ArrowLeft)])
    .bind("move_right", [Key(KeyCode::KeyD), Key(KeyCode::ArrowRight)])
    .bind("jump", [Key(KeyCode::Space), Pad(GamepadButton::South)])
    .bind("attack", [Key(KeyCode::KeyJ), Pad(GamepadButton::West)]);
}

/// A settings row: `label` on the left, the widget `widget` draws on the right
fn setting_row(ui: &mut UiContext, label: &str, widget: impl FnOnce(&mut UiContext)) {
  ui.row()
    .id(label)
    .gap(12.0)
    .align(Align::Center)
    .show(|ui| {
      ui.column().grow(1.0).show(|ui| {
        ui.text(label);
      });
      widget(ui);
    });
}

impl Scene for SettingsScreen {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    bind_actions(ctx.actions());
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
    let heading = ui.theme().text.heading.style();
    let mut reset = false;
    ui.panel()
      .focus_scope()
      .anchor(Anchor::Center)
      .width(560.0)
      .height(420.0)
      .gap(12.0)
      .show(|ui| {
        ui.text("Settings").style(&heading);
        // Q / E and the shoulder buttons switch tabs while tabs are drawn
        ui.tabs(&mut self.tab, &TABS).show();
        ui.scroll()
          .fill_width()
          .grow(1.0)
          .gap(10.0)
          .show(|ui| match self.tab {
            0 => {
              ui.slider(&mut self.music, 0.0..=1.0)
                .step(0.05)
                .label("Music")
                .show_value()
                .show();
              ui.slider(&mut self.effects, 0.0..=1.0)
                .step(0.05)
                .label("Effects")
                .show_value()
                .show();
            }
            1 => {
              setting_row(ui, "Window", |ui| {
                ui.dropdown(&mut self.window, &WINDOWS).width(200.0).show();
              });
              ui.toggle(&mut self.vsync, "VSync").show();
            }
            _ => {
              for (action, label) in ACTIONS {
                // Confirm or a click waits for the next key or pad button
                // (Escape cancels); a binding another action has swaps
                ui.rebind(action).id(action).label(label).show();
              }
              reset = ui.button("Reset controls").clicked();
            }
          });
        ui.text(RichText::markup(
          "[prompt=ui.tab_prev] [prompt=ui.tab_next] Switch tab    [prompt=ui.back] Back",
        ));
      });
    if reset {
      ctx.actions().reset_defaults();
    }
    if ui.back() {
      return SceneAction::Pop;
    }
    SceneAction::Continue
  }
}
```

To keep the player's bindings between runs, build the engine with `.with_user_preferences([UserPreference::Bindings])`: bindings that differ from the defaults are saved to `system.json` when they change and loaded at startup (`ryo-config-save`).

### Inventory grid with drag and drop and equipment

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
  Sword,
  Helmet,
  Potion,
}

impl Kind {
  fn name(self) -> &'static str {
    match self {
      Kind::Sword => "Sword",
      Kind::Helmet => "Helmet",
      Kind::Potion => "Potion",
    }
  }

  /// The equipment slot it goes in, if any
  fn equips(self) -> Option<usize> {
    match self {
      Kind::Helmet => Some(0),
      Kind::Sword => Some(1),
      Kind::Potion => None,
    }
  }
}

#[derive(Debug, Clone, Copy)]
struct Item {
  kind: Kind,
  count: u32,
}

/// Where an item is: the drag payload
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
  Bag(usize),
  Equip(usize),
}

const EQUIPMENT: [&str; 2] = ["Head", "Weapon"];

struct Inventory {
  bag: [Option<Item>; 16],
  equipment: [Option<Item>; 2],
  /// An icon per `Kind`
  icons: Vec<SpriteHandle>,
  status: String,
}

impl Default for Inventory {
  fn default() -> Self {
    let mut bag = [None; 16];
    bag[0] = Some(Item {
      kind: Kind::Sword,
      count: 1,
    });
    bag[1] = Some(Item {
      kind: Kind::Helmet,
      count: 1,
    });
    bag[2] = Some(Item {
      kind: Kind::Potion,
      count: 5,
    });
    Self {
      bag,
      equipment: [None; 2],
      icons: Vec::new(),
      status: String::new(),
    }
  }
}

impl Inventory {
  fn item(&self, at: Place) -> Option<Item> {
    match at {
      Place::Bag(i) => self.bag.get(i).copied().flatten(),
      Place::Equip(i) => self.equipment.get(i).copied().flatten(),
    }
  }

  fn put(&mut self, at: Place, item: Option<Item>) {
    let slot = match at {
      Place::Bag(i) => self.bag.get_mut(i),
      Place::Equip(i) => self.equipment.get_mut(i),
    };
    if let Some(slot) = slot {
      *slot = item;
    }
  }

  /// An equipment slot takes only its kind of item
  fn fits(item: Item, at: Place) -> bool {
    match at {
      Place::Equip(slot) => item.kind.equips() == Some(slot),
      Place::Bag(_) => true,
    }
  }

  /// Move the item at `from` to `to`, swapping with what is there
  fn move_item(&mut self, from: Place, to: Place) {
    let Some(moving) = self.item(from).filter(|_| from != to) else {
      return;
    };
    let there = self.item(to);
    if !Self::fits(moving, to) || there.is_some_and(|other| !Self::fits(other, from)) {
      self.status = format!("The {} doesn't go there.", moving.kind.name());
      return;
    }
    self.put(to, Some(moving));
    self.put(from, there);
    self.status = format!("Moved the {}.", moving.kind.name());
  }

  /// One slot: a drop target for any item, the drag source of its own
  fn slot(&self, ui: &mut UiContext, at: Place, key: usize) -> Response {
    let mut slot = ui
      .slot()
      .id(key)
      .filter(Filter::Nearest)
      .drop_target::<Place>();
    if let Some(item) = self.item(at) {
      if let Some(icon) = self.icons.get(item.kind as usize) {
        slot = slot.icon(icon);
      }
      slot = slot
        .count(item.count)
        .tooltip(item.kind.name())
        .drag_source(at);
    }
    slot.show()
  }
}

/// What the frame's slots reported: a drop (from, to) and the slot to
/// describe (the one under the pointer, else the focused one)
#[derive(Default)]
struct Seen {
  drop: Option<(Place, Place)>,
  shown: Option<Place>,
}

impl Seen {
  fn note(&mut self, at: Place, response: &Response) {
    if let Some(from) = response.dropped::<Place>() {
      self.drop = Some((from, at));
    }
    if response.hovered() || (self.shown.is_none() && response.focused()) {
      self.shown = Some(at);
    }
  }
}

impl Scene for Inventory {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    // Placeholder icons in `Kind` order; a game loads its own sprites
    let colours = [(200, 200, 220), (160, 120, 60), (220, 60, 80)];
    self.icons = colours
      .into_iter()
      .enumerate()
      .map(|(i, (r, g, b))| {
        let sprite = Sprite::solid_rect(12, 12, r, g, b, 255);
        ctx.assets.create_sprite(sprite, format!("icon-{i}"))
      })
      .collect();
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    let heading = ui.theme().text.heading.style();
    let mut seen = Seen::default();
    ui.column()
      .focus_scope()
      .anchor(Anchor::Center)
      .gap(12.0)
      .show(|ui| {
        ui.row().gap(16.0).show(|ui| {
          ui.panel().gap(6.0).show(|ui| {
            ui.text("Equipment").style(&heading);
            for (i, name) in EQUIPMENT.into_iter().enumerate() {
              ui.row().id(i).gap(8.0).align(Align::Center).show(|ui| {
                ui.text(name);
                ui.spacer();
                let at = Place::Equip(i);
                let response = self.slot(ui, at, i);
                seen.note(at, &response);
              });
            }
          });
          ui.panel().gap(6.0).show(|ui| {
            ui.text("Backpack").style(&heading);
            ui.grid(4).gap(6.0).show(|ui| {
              for i in 0..self.bag.len() {
                let at = Place::Bag(i);
                let response = self.slot(ui, at, i);
                seen.note(at, &response);
              }
            });
          });
        });
        let item = seen.shown.and_then(|at| self.item(at));
        ui.text(item.map_or("Nothing here", |item| item.kind.name()));
        ui.text(self.status.as_str());
      });
    if let Some((from, to)) = seen.drop {
      self.move_item(from, to);
    }
    // What the player carries (a mouse drag, or picked up with A); while it
    // is held Back puts it back, so `ui.back()` is false
    if let Some(item) = ui.held::<Place>().and_then(|at| self.item(at)) {
      self.status = format!("Holding the {}.", item.kind.name());
    }
    SceneAction::Continue
  }
}
```

The optional `ryo2d-inventory` crate holds items, stacks, equipment rules and loot; this recipe is the UI over any such model.

### Dialogue with choices and a leave confirmation

```rust
/// A line of the conversation and its choices (`None` ends it)
struct Line {
  text: &'static str,
  choices: &'static [(&'static str, Option<usize>)],
}

const LINES: [Line; 3] = [
  Line {
    text: "Welcome to the [color=#ffd166]Rusty Lantern[/color]! What can I get you?",
    choices: &[
      ("A potion, please", Some(1)),
      ("Just looking", Some(2)),
      ("Goodbye", None),
    ],
  },
  Line {
    text: "One healing potion. Drink it slowly.",
    choices: &[("Thanks", None)],
  },
  Line {
    text: "Look all you like.[pause=0.4] Touch nothing.",
    choices: &[("Goodbye", None)],
  },
];

struct Dialogue {
  line: Option<usize>,
  reveal: TextReveal,
  /// The leave question is open
  leaving: bool,
}

impl Default for Dialogue {
  fn default() -> Self {
    Self {
      line: Some(0),
      reveal: TextReveal::new(40.0),
      leaving: false,
    }
  }
}

impl Scene for Dialogue {
  fn is_transparent(&self) -> bool {
    true
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    let Some((n, line)) = self.line.and_then(|n| Some((n, LINES.get(n)?))) else {
      return SceneAction::Pop;
    };
    let heading = ui.theme().text.heading.style();
    let labels: Vec<&str> = line.choices.iter().map(|(label, _)| *label).collect();
    let mut picked = None;
    let (reveal, leaving) = (&mut self.reveal, self.leaving);
    ui.panel()
      .anchor(Anchor::BottomCenter)
      .offset(0.0, -32.0)
      .width(640.0)
      .gap(10.0)
      .show(|ui| {
        ui.text("Shopkeeper").style(&heading);
        let text = ui.text(RichText::markup(line.text));
        // Under the leave question the line waits where it is
        if leaving {
          text.reveal(reveal.drawn_clusters()).draw();
        } else {
          text.typewriter(reveal).draw();
        }
        // A new scope (a new id) once the line has shown, so the focus
        // moves from Skip to the choices as they appear
        ui.column()
          .focus_scope()
          .id((n, reveal.is_done()))
          .show(|ui| {
            if !reveal.is_done() {
              if ui.button("Skip").default_focus().clicked() {
                reveal.finish();
              }
            } else if let Some(i) = ui.choice_list(&labels).show() {
              picked = Some(i);
            }
          });
      });
    if let Some(i) = picked {
      self.line = line.choices.get(i).and_then(|(_, next)| *next);
      self.reveal = TextReveal::new(40.0);
    }
    if self.leaving {
      match ui.confirm("Leave the conversation?", "The shopkeeper will remember.") {
        Some(true) => return SceneAction::Pop,
        Some(false) => self.leaving = false,
        None => {}
      }
    } else if ui.back() {
      self.leaving = true;
    }
    if self.line.is_none() {
      return SceneAction::Pop;
    }
    SceneAction::Continue
  }
}
```

Choices also take the number keys 1-9. A confirm opened over the choices traps the focus, so A and the digits never pick a choice under it.

### HUD with no focus scope

```rust
struct Level {
  health: f32,
  gold: u32,
}

impl Default for Level {
  fn default() -> Self {
    Self {
      health: 0.8,
      gold: 120,
    }
  }
}

impl Scene for Level {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.actions().bind(
      "pause",
      [
        Binding::Key(KeyCode::KeyP),
        Binding::Pad(GamepadButton::Start),
      ],
    );
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    // The arrows, Space and A are gameplay's: nothing on the HUD is in a
    // focus scope
    if ctx.input.action_pressed("pause") {
      return SceneAction::Push(Box::new(PauseMenu::default()));
    }
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    ui.row()
      .anchor(Anchor::TopLeft)
      .offset(16.0, 16.0)
      .gap(12.0)
      .align(Align::Center)
      .show(|ui| {
        ui.progress(self.health).label("HP").width(160.0).show();
        // A panel sizes to its text, so it grows with the number
        ui.panel().padding(6.0).show(|ui| {
          ui.text(format!("Gold {}", self.gold));
        });
      });
    let mut pause = false;
    ui.row()
      .anchor(Anchor::TopRight)
      .offset(-16.0, 16.0)
      .gap(8.0)
      .align(Align::Center)
      .show(|ui| {
        ui.prompt("pause");
        // Outside any focus scope: the mouse's only
        pause = ui.button("Pause").clicked();
      });
    if pause {
      return SceneAction::Push(Box::new(PauseMenu::default()));
    }
    SceneAction::Continue
  }
}
```

### Quit confirmation

```rust
/// Pushed over the menu when the player asks to quit
struct QuitConfirm;

impl Scene for QuitConfirm {
  fn is_transparent(&self) -> bool {
    true
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    // Opens on No; Back answers No
    match ui.confirm_with("Quit to desktop?", "Unsaved progress is lost.", "Quit", "Stay") {
      Some(true) => SceneAction::Quit,
      Some(false) => SceneAction::Pop,
      None => SceneAction::Continue,
    }
  }
}
```

## 15. Testing UI

**`ryo2d::testing::UiHarness`** runs UI frames with no window or GPU: the UI's memory, layout, text and the resolve step over scripted input. Write the UI as a function of `&mut UiContext` and drive it.

```rust
use ryo2d::prelude::*;
use ryo2d::testing::UiHarness;

fn volume(ui: &mut UiContext<'_>, level: &mut f32) {
  ui.column().focus_scope().show(|ui| {
    ui.slider(level, 0.0..=1.0).step(0.1).show();
  });
}

#[test]
fn the_slider_steps_with_the_pad() {
  let mut ui = UiHarness::new(320.0, 240.0);   // built-in fonts, scale 1, the dark theme
  let mut level = 0.5;
  ui.frame(|u| volume(u, &mut level));         // drawn once: input resolves against what was drawn
  ui.frame(|u| volume(u, &mut level));         // the scope takes focus
  ui.action("ui.right");
  ui.frame(|u| volume(u, &mut level));
  assert!((level - 0.6).abs() < 1e-4);
}
```

- `frame(|ui| ..) -> R` runs one 60 fps frame as the top scene and returns the closure's value; `move_to(x, y)`, `press()` (held until `release()`), `release()`, `wheel(dx, dy)` (lines, +y up); `action(name)` presses `ui.up`, `ui.down`, `ui.left`, `ui.right`, `ui.confirm`, `ui.back`, `ui.next`, `ui.tab_next` or `ui.tab_prev` for the next frame (another name warns once and does nothing); `focused() -> Option<UiId>`; `command_count()` (what the last frame drew); `with_theme(theme)`.
- A click is a press frame then a release frame over the widget. A frame that draws nothing leaves the UI as the last frame that drew left it.
- **With the engine**, `HeadlessEngine` (`ryo-game-dev`, Visual Tests) drives whole scenes: `tap_pad(GamepadButton::DPadDown)`, `press_pad`, `release_pad`, `set_stick(Stick::Left, x, y)`, `wheel(dx, dy)`, `tap_key`, `mouse_move`; `ui_focused()` is the focus the last step resolved, `ui_focusables()` what the D-pad can land on now (id and rect), `focus_ui(id)` focuses a widget at the next step. Walking every `ui_focusables()` with the D-pad from each one proves a page has no dead ends.
- **Scripted runs:** `RYO_INPUT` presses pad buttons and holds sticks in dev builds, for captures: `RYO_INPUT="60:PadDPadDown 90:PadSouth 120-180:LeftStick@0x-1 200:Wheel@0x-3"` (`ryo-cli`).
- The console command `debug:ui` outlines every widget with its id, hot, active and focused ones coloured (`ryo-console`).

## 16. Tips

- **Mark every menu `.focus_scope()`**, and give it a `.default_focus()`. Without a scope the pad and the arrows do nothing in it.
- **Close the innermost thing on `ui.back()`**, then the next: a modal before its menu, a menu before the scene.
- **Use `.id(key)`** for rows of a list that sorts, filters or deletes; call-site ids shift when an earlier call is skipped.
- **Your data, its state.** Widgets write your `bool`/`f32`/`usize`/`String`; an open list, a caret, a scroll offset and focus are the UI's, dropped when the widget is not drawn for a frame (focus memory lasts the scene).
- **Collect, then act.** Set a local in the closure (`clicked = ui.button(..).clicked()`) and change scenes or mutate other state after `.show()` returns.
- **Bind theme values before a closure** (`let heading = ui.theme().text.heading.style();`): `ui.theme()` borrows `ui`.
- **One theme for the game.** The theme is global; a scene that sets its own restores the old one in `on_exit`.
- A widget's `Response` inside a container has last frame's rect (this frame's for a top-level container): use it for anchoring popups, not for placing siblings.
- **Animate looks rather than layout.** Clicks are hit-tested where a widget was drawn last frame: a panel that slides in and settles is fine, a button that never stops moving is hard to click. Fade, tint or pulse instead.
- Many-sprite or world-space visuals belong in `Scene::render` (GPU batches) or in entities, not the UI.
