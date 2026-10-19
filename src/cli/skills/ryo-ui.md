---
name: ryo-ui
description: Build game UI - widgets as builders in the Scene::ui hook, or node trees (UiNode) spawned once as entities - laid out in rows, columns and grids, with focus scopes for keyboard and gamepad navigation, scroll areas, popups and modals, drag and drop, themes, UI events and the input the UI consumes. Use when creating HUDs, menus, settings screens, inventories, dialogue or any in-game interface.
metadata:
  version: 4.3.0
---

# Ryo2D UI Specialist

Build game UI with `UiContext` inside `Scene::ui`. Complete signatures are in `ryo-api`. Text in depth (fonts, markup, effects, the typewriter) is in `ryo-text`; actions and pads in `ryo-controller`.

The model in one breath: every frame `ui` calls builders (`ui.button("Play")`), containers lay them out (`ui.column().show(|ui| ..)`), and each widget answers from what the player did to what was drawn last frame (`.clicked()`). Your game's values stay in your scene; what a widget remembers of itself (an open list, a caret, a scroll offset, the focus) lives in the UI's memory under its id. Keyboard and pad navigation work inside **focus scopes** you mark, so a menu is pad-playable and a HUD never steals the arrows.

There are two styles. **Closures** (sections 1 to 14) call the builders every frame. **Node trees** (section 15) describe a screen once as a `UiNode`, spawn it as entities, and report what the player did as `UiEvent`s; a tree can be written in a RON **file** that reloads while the game runs (section 15, Files). The engine draws node trees through the same builders, so everything in sections 3 to 13 applies to both, and the two mix in one scene. Lists far longer than a screen are **virtual lists** in either style (section 16): only the rows in view are built.

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

- **Helper functions** that draw widgets run one call site many times, so their widgets are told apart by occurrence, which holds only while the calls before them in the same container are not conditional. Mark the helper `#[track_caller]` (its widgets' ids then come from the helper's caller, one per call line), or key its widgets with `.id(key)` or the call with `ui.scope(key, ..)`, so ids stay stable.

  ```rust
  #[track_caller]
  fn menu_button(ui: &mut UiContext, label: &str) -> bool {
    ui.button(label).fill_width().clicked()
  }
  ```

- **Containers need `.show(|ui| ..)`**; widgets record on `.show()`, on a shorthand (`.clicked()`, a choice list's `.show() -> Option<usize>`), or on drop.
- **`Response`** (from `.show()`): `id`, `rect`, `hovered()`, `pressed()`, `clicked()` (released over it, or Confirm while focused), `secondary_clicked()`, `double_clicked()`, `drag_started()`, `dragging()`, `drag_delta()`, `drag_released()`, `changed()` (a value widget changed your data), `focused()`, `gained_focus()`, `lost_focus()`, `submitted()` (Enter in a text field), `pointer()`, plus drag and drop's `dropped::<T>()`, `drag_hover::<T>()`, `drag_started_here()`.
- **Clicks land on what was drawn.** The pointer is resolved once a frame, before any `ui`, against last frame's rects: a click hits what the player saw under the pointer, a widget not drawn this frame never reports `clicked()`, and the topmost widget wins (a panel blocks what is under it). A click is the release over the widget it was pressed on.
- **Shared options on every widget** (but `text`, `separator` and `spacer`): `.id(key)`, `.enabled(bool)` (dimmed, reports nothing, still blocks the pointer), `.at(rect)`, `.width(..)`, `.height(..)`, `.fill_width()`, `.margin(..)`, `.anchor(..)`, `.offset(x, y)`, `.grow(w)`, `.min_width(w)`, `.max_width(w)`, `.tooltip(text)`, `.variant(name)`, `.style(LookPatch)`, the focus options (section 7) and drag and drop (section 9).

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
| Dropdown | `if ui.dropdown(&mut self.quality, &["Low", "High"]).width(160.0).show().changed() {}` | the list opens on the popup layer and traps focus; Back closes it; a long list scrolls, no taller than the room below or above it |
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
- **The wheel** goes to the scroll area under the pointer (the topmost thing there decides, so a panel or popup over a list keeps the wheel from it), innermost first: an area at its edge passes the wheel to the one around it. Shift scrolls sideways. Scrolling eases (0.08 s). The wheel over any of the top scene's UI never reaches gameplay, whether it scrolled anything or not (over a panel, a modal, or a list at its edge); over nothing it is the game's.
- **The pad** walks a long list: focus moves onto rows scrolled out of view and scrolls them in, through nested areas too (a strip of cards inside a list is reached from the rows around it, and every area around the focused widget scrolls to show it). The right stick scrolls the focused widget's area, or the first area in its focus scope (a dialog's text above its Close button).
- `ui.scroll_to(id)` brings a widget into view from the next frame; `ui.scroll_offset(area.id)` reads where an area shows from; `ui.set_scroll_offset(area.id, Vec2::new(0.0, 0.0))` jumps there without easing.
- A scroll area builds everything in it every frame. For hundreds of rows or more, use a virtual list (section 16), which builds only the rows in view.

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
- **Traps.** `.trap()` keeps focus inside a scope (it makes the container one, as `.wrap_focus()` does); modals, open lists and `ui.popup`s are traps. A trap opens on its `.default_focus()` widget, gives the focus back when it closes, and while it is open nothing outside it (pointer, request or press) takes the focus.
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
| The wheel | the pointer is over any top-scene widget, panel, popup or modal, whether a scroll area moved or not |
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

## 15. Nodes

A **node tree** is UI described once as a `UiNode` value and spawned as entities, one entity per node. The engine draws a scene's trees every frame through the same builders a closure calls, so focus, hit testing, scrolling, popups, drag and drop, themes and sounds behave exactly as they do in sections 3 to 12. A node's state lives in its components: the game writes them, and hears the player's changes as `UiEvent`s.

```rust
let menu = UiNode::panel()                 // the closure API's names, as a value
  .focus_scope()
  .anchor(Anchor::Center)
  .width(260.0)
  .gap(8.0)
  .child(UiNode::button("Resume").action("resume").fill_width().default_focus())
  .child(UiNode::button("Quit").action("quit").fill_width());
let root = ctx.spawn_ui(menu);             // in on_enter: entities now, drawn from this frame
```

### Nodes or closures

| Use nodes for | Use closures for |
|---|---|
| A screen described once that changes only when the game's data changes: menus, settings, inventories, dialogue | HUDs and screens computed every frame from game state |
| Parts that systems find and change (`find_ui`, a game component inserted on the node) | `ui.custom` painters and absolute `draw_*` calls (closure only) |
| Screens you want to edit as data and see change while the game runs (Files, below) | Anything that reads best as straight-line code |

The two styles mix: a scene's trees draw before its `ui` closure, and a closure can embed a tree (Embedding, below).

### Spawning and ownership

- `ctx.spawn_ui(tree) -> Entity` spawns the tree at once and returns its root. The root belongs to the scene that spawned it: only that scene's UI pass draws it, and it follows the top-scene rule (a lower scene's nodes draw but take no input).
- `ctx.spawn_ui_in(parent, tree) -> Option<Entity>` adds a subtree as the last child of a container node. It returns `None`, with a warning, when `parent` is not a container node. A dialogue swaps its choices this way.
- **Exit despawns.** When a scene leaves the stack, every root it owns is despawned with its subtree, so a popped pause menu's nodes go with it. `ctx.world.despawn(root)` (or any node) removes a subtree at once, and the next frame draws without it.
- **Roots draw in spawn order**, a later one above an earlier one. The root's `UiRoot { anchor, offset, at }` fields are public: change them to move the tree. A text, separator or spacer root ignores placement; wrap a lone text in a `column()` to place it.
- `.hidden()` spawns a node hidden. Inserting or removing `UiHidden` hides or shows a node with its subtree; a hidden node takes no input and leaves the focus chain. `UiDisabled` (or `.enabled(false)`) greys a widget out.

### The vocabulary

Constructors and options use the closure builders' names: `UiNode::column()`, `row()`, `grid(columns)`, `stack()`, `panel()`, `scroll()`, `text(..)`, `button(..)`, `image(sprite)`, `checkbox(label, on)`, `toggle(label, on)`, `radio_group(&options, i)`, `slider(range, value)`, `progress(fraction)`, `text_input(text)`, `dropdown(&options, i)`, `tabs(&options, i)`, `choice_list(&options)`, `slot()`, `prompt(action)`, `rebind(action)`, `separator()`, `spacer()`, plus `UiNode::confirm(..)` and `UiNode::popup(name)` (Modals, below), and `UiNode::list(count, row_height)`, a virtual list (section 16). Shared options are the closure ones (`width`, `fill_width`, `gap`, `align`, `anchor`, `focus_scope`, `wrap_focus`, `default_focus`, `tooltip`, `enabled`, `variant`, `drag_source`, `drop_target`, ...). Containers (not scroll areas) in both styles also take `.background(Color)`, a fill behind their children. Use it in place of a full-screen `draw_rect`, which the closure would draw over the scene's nodes.

- **Renamed options.** `UiNode` is one type, so where a closure name means two things the node option is renamed:

  | Node option | Closure call |
  |---|---|
  | `.text_align(TextAlign)` on text | `ui.text(..).align(..)` (`.align` is a container's) |
  | `.image_size(w, h)` on an image | `ui.image(..).size(w, h)` (`.size` is text's font size) |
  | `.binding_slot(n)` on a rebind row | `ui.rebind(..).slot(n)` |
  | `.default_choice(i)` on a choice list | `ui.choice_list(..).default(i)` |

- **Text is plain unless `.markup()`** (on text and buttons). `.role(TextRole::Body)`, `.heading()` and `.muted()` read the theme when the text is drawn, so a theme change restyles them; `.style(TextStyle)` and `.color(c)` are fixed. `TextRole` is `Body`, `Heading`, `Button`, `Caption` or `Mono`.
- **Node-only options:**
  - `.action("play")`: the action name the node's events carry;
  - `.name("gold")`: a `UiName` to find it by, and the name `nav_*` and popups use;
  - `.insert(component)`: any game component or bundle on the node's entity;
  - `.child(node)` and `.children(iter)`;
  - `.hidden()`, `.modal()` and `.embedded()`.
- **Neighbours by name.** `.nav_down("quit")` names a node instead of a `UiId`, resolved within the same tree at spawn.
- **Warned once at spawn:** an option the node's kind does not take (`.tooltip` on text, `.background` on a scroll), children given to a widget, `.embedded()` on a child node, a `nav_*` or popup name the tree lacks, and a name used twice (`find_ui` finds the first).

### State components (the data)

| Node | Spawns with |
|---|---|
| `text`, `button` | `UiLabel(String)` |
| `image` | `UiImage(SpriteHandle)` |
| `checkbox`, `toggle` | `UiLabel`, `UiChecked(bool)` |
| `slider`, `progress` | `UiValue(f32)` |
| `text_input` | `UiTextValue(String)` |
| `radio_group`, `dropdown`, `tabs`, `choice_list` | `UiOptions(Vec<String>)`, `UiIndex(usize)` |
| `slot` | `UiSlot { icon, count, selected }` |
| `text(..).typewriter(reveal)` | `TextReveal` (call `finish()`, `pause()` and the rest on it) |
| `.name(..)`, `.tooltip(..)`, `.hidden()`, `.enabled(false)` | `UiName(String)`, `UiTooltip(String)`, `UiHidden`, `UiDisabled` |
| a `list`'s pooled rows | `UiListRow` (`index()`: the item the row shows; section 16) |

Every node also has `UiInteraction`, kept current by the walk: `hovered()`, `focused()`, and `id() -> Option<UiId>` once drawn.

```rust
// Game to UI: write a component, and the next UI pass draws it
if let Some(gold) = ctx.find_ui("gold")
  && let Some(mut label) = ctx.world.get_mut::<UiLabel>(gold)
{
  label.0 = format!("Gold {}", self.gold);
}
if let Some(play) = ctx.find_ui("continue") {
  ctx.world.entity_mut(play).remove::<UiDisabled>();
}

// UI to game: the player moved the slider; read the component on its event,
// or whenever the game needs it
let volume = ctx.world.get::<UiValue>(slider).map_or(1.0, |v| v.0);
```

- The walk writes a value component only when the player changed it, so a `Changed<UiValue>` query filter means a real change.
- A value the game sets out of the widget's range is drawn clamped, and kept until the player changes it.
- **Finding nodes:** `ctx.find_ui(name)` searches this scene's trees in spawn order; `ctx.find_ui_in(root, name)` searches one tree. A component inserted with `.insert(..)` finds nodes by query, like any entity.

### Events

```rust
pub struct UiEvent {
  pub node: Entity,           // the node that reported it
  pub name: Option<String>,   // its .name(..)
  pub action: Option<String>, // its .action(..)
  pub kind: UiEventKind,      // patterns use `..` for the rest
}
// UiEventKind: Clicked, SecondaryClicked, Changed, Submitted, Dropped { source }, Back, FocusGained, FocusLost
```

| Node | Sends |
|---|---|
| `button`, `image`, `slot`, `prompt`, `progress`, containers | `Clicked` |
| `checkbox`, `toggle`, `radio_group`, `slider`, `text_input`, `dropdown`, `tabs`, `rebind` | `Changed`, after the new value is in its component. A `radio_group` also sends focus events, but never `SecondaryClicked` or `Dropped` |
| `text_input` | also `Submitted` (Enter) |
| `choice_list` | `Clicked`, with the pick in its `UiIndex`. It has no `Response`, so no `ui_id`, and its `UiInteraction` is never updated |
| any drawn node except a `radio_group` and a `choice_list` | `SecondaryClicked`; a drop target `Dropped { source }`; a focusable one `FocusGained` and `FocusLost` |
| `list` | no `UiEvent` of its own: a `UiListBind` message for each row it gives an item (section 16); its rows' widgets send theirs |
| `text`, `separator`, `spacer` | nothing |

- **Read them with `ctx.ui_events()`**: this scene's own nodes' events since it last read them, in walk order. `ctx.messages::<UiEvent>()` reads every scene's, and the two share one cursor: read one or the other, in one place. Match on `action` for menus, on `name`, or on a game component of `event.node` for instances.
- **When they arrive.** A scene's trees are walked before its `ui`, and their events are delivered before `ui` runs, so `ui` reads them in the same frame. The UI pass runs before the fixed steps, so `update` does too, if `ui` did not read them first. An embedded tree's events are delivered after `ui` returns: `update` reads them that frame, `ui` the next.
- **Observers.** `UiEvent` is also an entity event triggered on its node, for one-off code: `ctx.world.entity_mut(node).observe(|event: On<UiEvent>, ..| ..)`. Observers run during the UI pass, while the scene stack is out of the world, so an observer must not try to change scenes: set a resource or write a message, and let the scene's `update` or `ui` act on it.

  ```rust
  #[derive(Resource, Default)]
  struct Picked(Option<String>);

  ctx.insert_resource(Picked::default());
  if let Some(play) = ctx.find_ui("play") {
    ctx.world.entity_mut(play).observe(|event: On<UiEvent>, mut picked: ResMut<Picked>| {
      if event.kind == UiEventKind::Clicked {
        picked.0 = event.action.clone();
      }
    });
  }
  ```

- **Drag and drop by entity.** A node's drag payload is its own entity. `.drag_source()` and `.drop_target()` take no type. A target's `Dropped { source }` names the dragged node, and the game maps both nodes to its own data, usually through a component inserted on each. `ui.held::<Entity>()` is the node being carried. A slot is a source only while it has an icon. Pad pick-up, the ghost and refusals work as in section 9.

### Ids and widget state

- **Ids are keyed by entity.** A node's id is made from its entity under its parent's id, as `.id(key)` keys a closure widget; text, separators and spacers are drawn in `ui.scope(entity, ..)`. So focus memory, scroll offsets, an open list, a caret and tooltip timers follow the node when siblings are added, removed or reordered.
- **Despawning forgets.** A despawned node's state is forgotten. Re-parenting a node changes its id, and a respawned tree starts fresh. `ctx.update_ui` and file reloads keep the entities of matched nodes instead (Files, below).
- `ctx.ui_id(node) -> Option<UiId>` gives the id once the node is drawn, for `ui.scroll_offset`, `ui.request_focus` or a closure widget's `nav_*`. `ctx.focus_ui(node)` focuses a node when it is next drawn; it does not apply to a `radio_group` or a `choice_list`, which never take the request.
- **A focused node despawned** leaves the focus at `None`, and the next navigation lands on its scope's default. This is section 7's rule for a widget that stops being drawn.

### Embedding in closure UI

Spawn a root with `.embedded()` and draw it inside a closure container with `ui.node(root, ctx.world)`. It takes the closure's flow, focus scope and clip, so one focus path runs through both. An embedded root is not drawn on its own. Drawing it twice in a frame draws it once and warns.

```rust
// on_enter: self.grid = Some(ctx.spawn_ui(UiNode::grid(4).gap(6.0).embedded().children(cells)));
fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
  let Some(grid) = self.grid else {
    return SceneAction::Continue;
  };
  ui.panel()
    .focus_scope()
    .anchor(Anchor::Center)
    .gap(10.0)
    .show(|ui| {
      ui.button("Add").default_focus();
      ui.node(grid, ctx.world); // the tree, here, in this panel's scope
      ui.button("Done");
    });
  SceneAction::Continue
}
```

### Reusable components

A reusable component is a function that returns a `UiNode`. It takes parameters and tags its nodes with names or game components, and it can go into any tree any number of times. Each instance gets its own entities, ids and state, and its events carry its own nodes, so the inserted component tells instances apart.

```rust
#[derive(Component, Clone, Copy)]
struct ShopItem(usize);

fn shop_row(name: &str, price: u32, index: usize) -> UiNode {
  UiNode::row()
    .gap(8.0)
    .align(Align::Center)
    .child(UiNode::column().grow(1.0).child(UiNode::text(name)))
    .child(UiNode::text(format!("{price} g")).muted())
    .child(UiNode::button("Buy").action("buy").insert(ShopItem(index)))
}
// A Clicked with action "buy": ctx.world.get::<ShopItem>(event.node) says which
```

### Modals, confirms, popups and Back

- **`.modal()`** on any container node draws it as `ui.modal()` does: the dim, the trap, focus on its `default_focus`, and the focus given back when it goes. A modal root opens centred unless it was given an `.anchor(..)`.
- **Back** goes to the last modal or popup node drawn in the scene's trees, the innermost, as a `Back` event carrying that node and its action. While one is drawn, Back is taken before the scene's `ui` runs, so `ui.back()` is false there. A modal or popup inside an embedded tree takes Back only through the closure's own `ui.back()`. A scene's node dialogs answer Back before its closure runs, so a closure modal drawn above a node dialog in the same scene does not get Back first. Do not mix them in one scene; if you must, close the node dialog from the closure explicitly.
- **`UiNode::confirm(title, message, yes, no)`** builds `ui.confirm_with`'s dialog as a modal tree whose buttons have the actions `"yes"` and `"no"`. It opens on No, and Back answers as No: a `Clicked` with action `"no"`. The game despawns it on either answer.
- **`UiNode::popup(name)`** opens below the node called `name` in the same tree, as `ui.popup` does. Put it in the anchor's tree and show it by removing `UiHidden`, or spawn it there with `spawn_ui_in`. Close it on its `Back` event.

```rust
// An options dialog: Done or Back closes it
self.options = Some(ctx.spawn_ui(
  UiNode::panel()
    .modal()
    .action("options")
    .width(320.0)
    .gap(12.0)
    .child(UiNode::text("Options").heading())
    .child(UiNode::toggle("Subtitles", self.subtitles).name("subtitles"))
    .child(
      UiNode::row()
        .justify(Justify::End)
        .child(UiNode::button("Done").action("done").default_focus()),
    ),
));
// In ui: (UiEventKind::Clicked, Some("done")) | (UiEventKind::Back, Some("options")) => despawn it
```

### Recipes

Each compiles as written beside section 14's. They name its `Level`.

#### Menu with a confirm

```rust
/// The title screen as a node tree: spawned once, its clicks read as events
#[derive(Default)]
struct NodeMenu {
  has_save: bool,
  /// The quit question, while it is open
  confirm: Option<Entity>,
}

impl NodeMenu {
  fn menu(has_save: bool) -> UiNode {
    let button = |label: &str, action: &str| UiNode::button(label).action(action).fill_width();
    UiNode::panel()
      .focus_scope()
      .wrap_focus()
      .anchor(Anchor::Center)
      .width(260.0)
      .gap(8.0)
      .child(UiNode::text("RYO QUEST").heading().size(32.0))
      .child(button("Continue", "continue").enabled(has_save))
      .child(button("New Game", "new").default_focus())
      .child(button("Quit", "quit").variant("danger"))
  }

  fn ask_to_quit(&mut self, ctx: &mut SceneContext) {
    if self.confirm.is_none() {
      let question = UiNode::confirm(
        "Quit to desktop?",
        "Unsaved progress is lost.",
        "Quit",
        "Stay",
      );
      self.confirm = Some(ctx.spawn_ui(question));
    }
  }
}

impl Scene for NodeMenu {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.spawn_ui(Self::menu(self.has_save));
    ctx.spawn_ui(
      UiNode::column()
        .anchor(Anchor::BottomCenter)
        .offset(0.0, -32.0)
        .child(
          UiNode::text("[prompt=ui.confirm] Select    [prompt=ui.back] Quit")
            .markup()
            .muted(),
        ),
    );
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
    for event in ctx.ui_events() {
      if event.kind != UiEventKind::Clicked {
        continue;
      }
      match event.action.as_deref() {
        Some("new" | "continue") => return SceneAction::Switch(Box::new(Level::default())),
        Some("quit") => self.ask_to_quit(ctx),
        // The confirm's buttons; Back on it is a "no" click
        Some("yes") => return SceneAction::Quit,
        Some("no") => {
          if let Some(confirm) = self.confirm.take() {
            ctx.world.despawn(confirm);
          }
        }
        _ => {}
      }
    }
    // With the confirm open, it took Back before this ran
    if ui.back() {
      self.ask_to_quit(ctx);
    }
    SceneAction::Continue
  }
}
```

#### Settings screen with tabs

```rust
const PAGES: [&str; 2] = ["Audio", "Video"];

/// Each tab's rows are a column, hidden unless its tab is picked
#[derive(Default)]
struct NodeSettings {
  /// The columns, by tab
  pages: Vec<Entity>,
}

/// A labelled row: the label takes the room the widget leaves
fn setting(label: &str, widget: UiNode) -> UiNode {
  UiNode::row()
    .gap(12.0)
    .align(Align::Center)
    .child(UiNode::column().grow(1.0).child(UiNode::text(label)))
    .child(widget)
}

impl NodeSettings {
  fn tree() -> UiNode {
    let audio = UiNode::column()
      .fill_width()
      .gap(10.0)
      .name("page-0")
      .child(setting(
        "Master",
        UiNode::slider(0.0..=1.0, 0.8)
          .step(0.05)
          .show_value()
          .width(200.0)
          .name("master"),
      ))
      .child(UiNode::checkbox("Mute in background", true).name("mute"));
    let video = UiNode::column()
      .fill_width()
      .gap(10.0)
      .name("page-1")
      .hidden()
      .child(setting(
        "Window",
        UiNode::dropdown(&["Windowed", "Borderless", "Fullscreen"], 0).width(200.0),
      ))
      .child(UiNode::toggle("VSync", true).name("vsync"));
    UiNode::panel()
      .focus_scope()
      .anchor(Anchor::Center)
      .width(520.0)
      .height(360.0)
      .gap(12.0)
      .child(UiNode::text("Settings").heading())
      .child(UiNode::tabs(&PAGES, 0).name("tabs"))
      .child(
        UiNode::scroll()
          .fill_width()
          .grow(1.0)
          .gap(10.0)
          .child(audio)
          .child(video),
      )
  }
}

impl Scene for NodeSettings {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let root = ctx.spawn_ui(Self::tree());
    self.pages = (0..PAGES.len())
      .filter_map(|i| ctx.find_ui_in(root, &format!("page-{i}")))
      .collect();
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
    for event in ctx.ui_events() {
      if event.kind != UiEventKind::Changed {
        continue;
      }
      match event.name.as_deref() {
        // Q / E and the bumpers switch tabs too
        Some("tabs") => {
          let picked = ctx.world.get::<UiIndex>(event.node).map_or(0, |i| i.0);
          for (i, &page) in self.pages.iter().enumerate() {
            if i == picked {
              ctx.world.entity_mut(page).remove::<UiHidden>();
            } else {
              ctx.world.entity_mut(page).insert(UiHidden);
            }
          }
        }
        // Applied as it changes; the others are read when needed
        Some("master") => {
          if let Some(volume) = ctx.world.get::<UiValue>(event.node).map(|v| v.0) {
            ctx.audio.set_master_volume(volume);
          }
        }
        _ => {}
      }
    }
    if ui.back() {
      return SceneAction::Pop;
    }
    SceneAction::Continue
  }
}
```

#### Inventory grid with drag and drop

```rust
/// Which bag slot a slot node shows: a game component on the node
#[derive(Component, Clone, Copy)]
struct BagSlot(usize);

#[derive(Clone)]
struct Stack {
  name: &'static str,
  icon: SpriteHandle,
  count: u32,
}

#[derive(Default)]
struct NodeInventory {
  bag: Vec<Option<Stack>>,
  /// The slot nodes, by bag index
  slots: Vec<Entity>,
  details: Option<Entity>,
}

impl NodeInventory {
  /// One bag slot: its drag payload is the node itself
  fn slot(index: usize) -> UiNode {
    UiNode::slot()
      .filter(Filter::Nearest)
      .drag_source()
      .drop_target()
      .insert(BagSlot(index))
  }

  /// Write what every slot holds
  fn refresh(&self, ctx: &mut SceneContext) {
    for (stack, &node) in self.bag.iter().zip(&self.slots) {
      if let Some(mut slot) = ctx.world.get_mut::<UiSlot>(node) {
        slot.icon = stack.as_ref().map(|s| s.icon.clone());
        slot.count = stack.as_ref().map_or(0, |s| s.count);
      }
    }
  }

  /// The stack a slot node shows
  fn stack_at(&self, ctx: &SceneContext, node: Entity) -> Option<&Stack> {
    let index = ctx.world.get::<BagSlot>(node)?.0;
    self.bag.get(index)?.as_ref()
  }
}

impl Scene for NodeInventory {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    self.bag = vec![None; 12];
    for (i, (name, (r, g, b), count)) in
      [("Sword", (200, 200, 220), 1), ("Potion", (220, 60, 80), 5)]
        .into_iter()
        .enumerate()
    {
      let icon = ctx
        .assets
        .create_sprite(Sprite::solid_rect(12, 12, r, g, b, 255), name);
      self.bag[i] = Some(Stack { name, icon, count });
    }
    let root = ctx.spawn_ui(
      UiNode::panel()
        .focus_scope()
        .anchor(Anchor::Center)
        .gap(10.0)
        .child(UiNode::text("Backpack").heading())
        .child(
          UiNode::grid(4)
            .gap(6.0)
            .name("bag")
            .children((0..12).map(Self::slot)),
        )
        .child(UiNode::text("Nothing here").muted().name("details")),
    );
    if let Some(grid) = ctx.find_ui_in(root, "bag") {
      self.slots = ctx
        .world
        .get::<Children>(grid)
        .map(|kids| kids.to_vec())
        .unwrap_or_default();
    }
    self.details = ctx.find_ui_in(root, "details");
    self.refresh(ctx);
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
    let index = |ctx: &SceneContext, node: Entity| ctx.world.get::<BagSlot>(node).map(|s| s.0);
    let mut moved = false;
    for event in ctx.ui_events() {
      // A drop names the dragged node; both map to bag indices
      if let UiEventKind::Dropped { source } = event.kind
        && let (Some(from), Some(to)) = (index(ctx, source), index(ctx, event.node))
      {
        self.bag.swap(from, to);
        moved = true;
      }
    }
    if moved {
      self.refresh(ctx);
    }
    // What is carried, else the slot under the pointer, else the focused one
    let looked_at = |pick: fn(&UiInteraction) -> bool| {
      self
        .slots
        .iter()
        .copied()
        .find(|&n| ctx.world.get::<UiInteraction>(n).is_some_and(pick))
    };
    let shown = match ui
      .held::<Entity>()
      .and_then(|node| self.stack_at(ctx, node))
    {
      Some(stack) => format!("Holding the {}.", stack.name),
      None => looked_at(UiInteraction::hovered)
        .or_else(|| looked_at(UiInteraction::focused))
        .and_then(|node| self.stack_at(ctx, node))
        .map_or("Nothing here".to_string(), |stack| stack.name.to_string()),
    };
    if let Some(details) = self.details
      && let Some(mut label) = ctx.world.get_mut::<UiLabel>(details)
      && label.0 != shown
    {
      label.0 = shown;
    }
    SceneAction::Continue
  }
}
```

#### Dialogue with swapped choices

```rust
/// A line and its choices (`None` ends the conversation)
struct Say {
  text: &'static str,
  choices: &'static [(&'static str, Option<usize>)],
}

const TALK: [Say; 2] = [
  Say {
    text: "Welcome to the [color=#ffd166]Rusty Lantern[/color]![pause=0.3] What can I get you?",
    choices: &[("A potion, please", Some(1)), ("Goodbye", None)],
  },
  Say {
    text: "One healing potion. Drink it slowly.",
    choices: &[("Thanks", None)],
  },
];

#[derive(Default)]
struct NodeDialogue {
  line: usize,
  /// The line's text node, which holds its `TextReveal`
  text: Option<Entity>,
  /// The column what the player can do next is swapped into
  below: Option<Entity>,
  /// That subtree, and the (line, revealed) it was built for
  next: Option<(Entity, (usize, bool))>,
  confirm: Option<Entity>,
}

impl NodeDialogue {
  fn reveal<'w>(&self, ctx: &'w mut SceneContext) -> Option<Mut<'w, TextReveal>> {
    ctx.world.get_mut::<TextReveal>(self.text?)
  }

  /// Swap in Skip while the line shows, then its choices: a new subtree is
  /// a new focus scope, so the focus moves into it
  fn swap_next(&mut self, ctx: &mut SceneContext) {
    let done = self.reveal(ctx).is_some_and(|reveal| reveal.is_done());
    let key = (self.line, done);
    if self.next.is_some_and(|(_, built)| built == key) {
      return;
    }
    if let Some((old, _)) = self.next.take() {
      ctx.world.despawn(old);
    }
    let (Some(below), Some(say)) = (self.below, TALK.get(self.line)) else {
      return;
    };
    let scope = UiNode::column().focus_scope();
    let scope = if done {
      let labels: Vec<&str> = say.choices.iter().map(|(label, _)| *label).collect();
      scope.child(UiNode::choice_list(&labels).action("choose"))
    } else {
      scope.child(UiNode::button("Skip").action("skip").default_focus())
    };
    self.next = ctx.spawn_ui_in(below, scope).map(|node| (node, key));
  }
}

impl Scene for NodeDialogue {
  fn is_transparent(&self) -> bool {
    true
  }

  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let root = ctx.spawn_ui(
      UiNode::panel()
        .anchor(Anchor::BottomCenter)
        .offset(0.0, -32.0)
        .width(640.0)
        .gap(10.0)
        .child(UiNode::text("Shopkeeper").heading())
        .child(
          UiNode::text(TALK[0].text)
            .markup()
            .role(TextRole::Body)
            .typewriter(TextReveal::new(40.0).with_punctuation_pause(0.15))
            .name("line"),
        ),
    );
    self.text = ctx.find_ui_in(root, "line");
    self.below = Some(root);
    self.swap_next(ctx);
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
    for event in ctx.ui_events() {
      if event.kind != UiEventKind::Clicked {
        continue;
      }
      match event.action.as_deref() {
        Some("skip") => {
          if let Some(mut reveal) = self.reveal(ctx) {
            reveal.finish();
          }
        }
        // The pick is in the list's UiIndex
        Some("choose") => {
          let picked = ctx.world.get::<UiIndex>(event.node).map(|i| i.0);
          let next = TALK
            .get(self.line)
            .and_then(|say| say.choices.get(picked?))
            .and_then(|(_, next)| *next);
          let Some(next) = next.filter(|n| *n < TALK.len()) else {
            return SceneAction::Pop;
          };
          self.line = next;
          if let Some(text) = self.text
            && let Some(mut label) = ctx.world.get_mut::<UiLabel>(text)
          {
            label.0 = TALK[next].text.to_string();
          }
          if let Some(mut reveal) = self.reveal(ctx) {
            reveal.restart();
          }
        }
        Some("yes") => return SceneAction::Pop,
        // Stay: the line carries on from where it paused
        Some("no") => {
          if let Some(confirm) = self.confirm.take() {
            ctx.world.despawn(confirm);
          }
          if let Some(mut reveal) = self.reveal(ctx) {
            reveal.resume();
          }
        }
        _ => {}
      }
    }
    if self.confirm.is_none() && ui.back() {
      let question = UiNode::confirm(
        "Leave the conversation?",
        "The shopkeeper will remember.",
        "Yes",
        "No",
      );
      self.confirm = Some(ctx.spawn_ui(question));
      // The line waits under the question
      if let Some(mut reveal) = self.reveal(ctx) {
        reveal.pause();
      }
    }
    self.swap_next(ctx);
    SceneAction::Continue
  }
}
```

A Skip finishes the line at its next draw, and the choices replace Skip in the frame after.

### Files

A screen can be written in a RON file, `assets/ui/pause.ui.ron`, instead of Rust. The file loads into the same `UiNode` as section 15's builders make, so everything above applies: spawning, ownership, state components, events, focus, modals. The file holds the look and layout; game code answers `UiEvent`s and sets values. In debug builds the running game picks up a saved file without a rebuild.

| Use | When |
|---|---|
| **A UI file** | A menu, settings page, dialog or HUD whose layout you want to change without recompiling, or want an agent to edit as data |
| **A `UiNode` tree in Rust** | A tree built from game data in one expression (a shop from its stock), or one a unit test builds |
| **A closure** (`Scene::ui`) | `ui.custom` painters, absolute `draw_*` calls, and screens computed every frame |

The three mix. A file can hold a screen while code builds its dynamic parts from the file's templates. The "+1" fly-by and the banners of breakout stay closure UI.

#### The file

A UI file is valid RON. Its top level is an unnamed struct; every field is optional, but a file needs a `root` or `templates`:

```ron
// assets/ui/pause.ui.ron
(
  imports: ["assets/ui/common.ui.ron"],      // other files whose templates this one uses
  templates: {
    "MenuButton": (
      params: ["label", "action"],
      node: Button(label: label, action: action, fill_width: true),
    ),
  },
  bindings: { "score": 0 },                  // starting values for names that only text uses
  root: Panel(
    focus_scope: true, anchor: Center, modal: true, width: 260, gap: 8,
    children: [
      Text(text: "Paused", heading: true),
      Text("Score: {score}"),
      MenuButton(label: "Resume", action: "resume", default_focus: true),
      MenuButton(label: "Quit", action: "quit"),
      Slider(range: (0, 1), value: 0.8, bind: "volume", name: "volume"),
      Text("Volume: {volume:.1}"),
      Column(name: "extras"),
    ],
  ),
)
```

- **Syntax.** `//` and `/* */` comments, trailing commas, strings (`"..."` with RON escapes, `r"..."`, `r#"..."#`), numbers, `true` and `false`, lists `[a, b]`, tuples `(a, b)`, maps `{ "key": value }` with string keys, and named values `Name`, `Name(value)`, `Name(field: value, ..)`. Not taken, each with an error saying so: `#![enable(..)]`, `Some(..)`/`None`, chars, byte strings.
- **A node is a named value.** Its name is a kind (below) or a template; its fields are options. Kinds are `UiNode`'s constructors in CamelCase (`radio_group` is `RadioGroup`); options are `UiNode`'s method names (`fill_width`, `nav_up`). A constructor's arguments are options named after its parameters (`Checkbox(label: "Sound", checked: true)`), except `Popup`'s anchor node, which is `anchor_node`, and `Prompt`'s and `Rebind`'s action, which is `input`.
- **Short forms.** RON cannot mix a positional value with named fields, so a node takes named options or its main value alone: `Text("Paused")`, `Button("OK")`, `Separator`, `Spacer`. A node with a short form and other options writes its main value by name: `Text(text: "Paused", heading: true)`.
- **`children: [..]`** lists a container's children. A widget takes none.
- **Paths are relative to the game's working directory**, as `load_sprite`'s are: `assets/ui/common.ui.ron`, `icon: "assets/items/sword.png"`.
- **Not in files:** `insert` (Rust components; add them in code to the built node) and `style` (a whole `TextStyle`; files use `role`, `size`, `color` and `text_align`).

| Kind | Short form | Constructor options (required in bold) |
|---|---|---|
| `Column`, `Row`, `Stack`, `Panel`, `Scroll`, `Slot` | none | none |
| `Grid` | `columns` | **`columns`** |
| `Popup` | none | **`anchor_node`** (a node name) |
| `Text` | `text` | `text` (default `""`) |
| `Button` | `label` | `label` (default `""`) |
| `Image` | `sprite` | **`sprite`** (an asset path) |
| `Checkbox` | `label` | `label`, `checked` (default `false`) |
| `Toggle` | `label` | `label`, `on` (default `false`) |
| `RadioGroup`, `Dropdown`, `Tabs` | none | **`options`** (a list of strings), `selected` (default 0) |
| `ChoiceList` | none | **`options`** |
| `Slider` | none | **`range`** (`(min, max)`), `value` (default min) |
| `Progress` | `fraction` | `fraction` (default 0) |
| `TextInput` | `text` | `text` (default `""`) |
| `Prompt`, `Rebind` | `input` | **`input`** (an input action name) |
| `Separator`, `Spacer` | the bare name | none |
| `Confirm` | none | **`title`**, `message` (default `""`), `yes` (default `"Yes"`), `no` (default `"No"`) |
| `List` | none | `count` (default 0), **`row_height`** (a number), **`row_shape`** (a template name, or a node; section 16) |

| Value form | Options |
|---|---|
| `true` (the method with no argument; `false` leaves it off) | `fill_width`, `fill_height`, `fill`, `center`, `wrap_items`, `focus_scope`, `trap`, `wrap_focus`, `block_pointer`, `default_focus`, `nav_skip`, `drag_source`, `drop_target`, `modal`, `hidden`, `embedded`, `markup`, `heading`, `muted`, `ellipsis`, `show_value`, `password`, `horizontal`, `both`, `no_bar` |
| a bool | `enabled`, `focusable`, `selected` (on `Slot`; on `RadioGroup`, `Dropdown` and `Tabs`, `selected` is the constructor's index) |
| a number | `min_width`, `max_width`, `min_height`, `max_height`, `gap`, `grow`, `aspect`, `size`, `wrap`, `step` |
| a whole number | `max_lines`, `max_len`, `default_choice`, `count`, `binding_slot`, `columns` and `overscan` (on `List`) |
| a length: `240`, `Pct(0.5)`, `Fill`, `Fit` | `width`, `height` |
| edges: `8`, `(x, y)`, or `(left: .., top: .., right: .., bottom: ..)` | `padding`, `margin` |
| an enum by name | `align` (`Start`, `Center`, `End`, `Stretch`); `justify` (`Start`, `Center`, `End`, `SpaceBetween`, `SpaceAround`, `SpaceEvenly`); `anchor` (`TopLeft`, `TopCenter`, `TopRight`, `CenterLeft`, `Center`, `CenterRight`, `BottomLeft`, `BottomCenter`, `BottomRight`); `text_align` (`Start`, `Center`, `End`, `Justify`); `filter` (`Nearest`, `Linear`); `role` (`Body`, `Heading`, `Button`, `Caption`, `Mono`) |
| numbers in a tuple or list | `offset` `(x, y)`, `at` `(x, y, width, height)`, `image_size` `(width, height)`, `uv` `(u0, v0, u1, v1)`, `tracks` `[1, 2, 1]` |
| a colour: `"#rrggbb"`, `"#rrggbbaa"`, or `(r: .., g: .., b: .., a: ..)` | `background`, `color`, `tint` |
| a string | `tooltip`, `variant`, `action`, `name`, `key`, `bind`, `nav_up`, `nav_down`, `nav_left`, `nav_right`, `label`, `placeholder` |
| an asset path | `icon` |
| a number (characters per second) or `(per_second: .., punctuation_pause: .., fade: ..)` | `typewriter` |

- **An option the kind does not take is an error** (the same rule as the spawn warning in Spawning, above). `name`, `action`, `hidden` and `key` suit every kind. **`Text` takes only the text options** (`markup`, `role`, `heading`, `size`, `color`, `muted`, `text_align`, `wrap`, `max_lines`, `ellipsis`, `typewriter`, `embedded`): it has no `width`, `margin` or `tooltip`. Wrap it in a sized `Column`: `Column(width: 96, children: [Text(label)])`.
- **Mistakes come with a place.** `assets/ui/pause.ui.ron:12:5: Button has no option "fil_width"; did you mean "fill_width"?` Unknown kinds, options, enum values, templates and parameters get the nearest name. One load reports every mistake in the file together.

#### Keys

Every node has a key that an update or reload matches nodes by: its `key`, else its `name`, else its kind (or template) and its index among the siblings of that kind that have no `key` or `name` (the third such `Button` is `Button#2`). Naming one node does not change its siblings' keys. Two siblings with the same key are an error in a file (a warning in code). Give a `name` or `key` to nodes that move around, so a reload or update keeps them with their state.

#### Templates

```ron
templates: {
  "ItemSlot": (
    params: ["icon"],                     // required
    defaults: { "count": 1 },            // optional, with their values (a name is in one list or the other)
    node: Slot(icon: icon, count: count, drag_source: true, drop_target: true),
  ),
}
```

- **Used like a kind:** `ItemSlot(icon: "assets/items/sword.png", count: 3)`. A use's fields are the template's parameters first, then any option of its root node, applied over the template's own: `MenuButton(label: "Resume", action: "resume", default_focus: true)`. A parameter is either required (in `params`) or has a default (in `defaults`), never both: listing a name in both is a load error. A missing required parameter or an unknown field is an error at the use.
- **`false` cannot turn off a flag the template's body sets.** A flag option (`fill_width`, `default_focus`, ..) given `false` is just not applied, so it never turns off what the template's node sets: `MenuButton(fill_width: false)` does nothing when `MenuButton`'s node says `fill_width: true`. Make a flag optional with a parameter that has a default: `defaults: { "wide": true }` and `node: Button(label: label, fill_width: wide)`, then `MenuButton(label: "Back", wide: false)`.
- **A bare lowercase name that is a parameter** stands for the argument's value wherever a value can go (`icon: icon`). Kinds, templates and enum values are CamelCase, so a lowercase name is always a parameter reference.
- **A parameter can stand for a node:** `Labelled(label: "Volume", widget: Slider(range: (0, 1), bind: "volume"))` with `node: Row(children: [Text(label), widget])`. A bare template name works as the argument too (`widget: VolumeSlider`).
- **`{param}` in a string** is replaced by the argument's text when the template expands: `Text("Hello {who}")`; `{param:.1}` gives a number argument one decimal. A `{name}` that is not a parameter stays a binding pattern (Bindings), so inside a template a parameter shadows a binding of the same name.
- **A template cannot build a binding pattern from a parameter.** `{{` stays an escape that prints a literal brace, so `"{{{name}}}"` does not give `"{master}"`. Pass the whole pattern in as an argument, next to the name: `VolumeRow(label: "Master", name: "master", shown: "{master:.0}%")`, with `Slider(bind: name)` and `Text(shown)` inside.
- **A template name is CamelCase and cannot be a kind's name.** Call a confirm template `QuitConfirm`, not `Confirm`.
- **Templates nest** and pass values explicitly (`Inner(label: label)`). A template that uses itself, directly or through others, is an error.

#### Imports

`imports: ["assets/ui/common.ui.ron"]` makes another file's templates usable here; its `root` is ignored. A template name defined twice across a file and its imports is an error, as is an import cycle. **Imports are not transitive**: a file sees its own templates and its direct imports' only, and a node passed as an argument is checked in the file that wrote it, so it may name only templates visible there. Import what you use.

#### Placeholders

An empty named container, `Column(name: "extras")`, is where code adds nodes with `ctx.spawn_ui_in(parent, node)`. A reload and `ctx.update_ui` do not despawn or move what code added there (below). A template instance among them (`ctx.ui_template`) still follows its template when the file reloads.

#### Loading and building

| Call | Does |
|---|---|
| `ctx.spawn_ui_file(path) -> Result<Entity, UiFileError>` | load, build and spawn the file's `root` |
| `ctx.assets.load_ui(path) -> AssetResult<UiFile>` | load and check a file; cached by path, imports through the same cache |
| `file.root(ctx.assets) -> Result<UiNode, UiFileError>` | build the `root` |
| `ctx.ui_template(path, name, &args) -> Result<UiNode, UiFileError>` | load a file and build one instance of its template (or an import's) |
| `file.template(name, &args, ctx.assets)` | the same, on a file you hold |
| `ctx.assets.add_ui(path, text) -> Result<UiFile, UiFileError>` | check text the game embedded and cache it under `path`; never reloaded |
| `UiFile::parse(name, text)` | check text with no asset manager (tests, tools); such a file cannot import |
| `UiArgs::new().with("label", "Resume").with("count", 3)` | a template's arguments: text, numbers, bools and lists; its parameters, and any option of its root node (`.with("default_focus", true)`) |

- The built `UiNode` is ordinary: spawn it with `spawn_ui` or `spawn_ui_in`, or put it in a code tree.
- **Give a template instance its options in `UiArgs`, not as methods on the built node.** An argument may be any option of the template's root node, as in a file: `UiArgs::new().with("default_focus", true)`, `.with("name", "slot-3")`. A reload builds the instance again from its template and its arguments, so options passed this way are applied again, while an option set in code on the result (`.default_focus()`, `.name(..)`) is gone after the next reload. What code adds that a reload keeps is `.insert(MyComponent)` components and `.child(..)` nodes.
- Images are loaded while building; a missing one is an error at the `sprite` or `icon` value.
- `UiFileError` lists every mistake, each with `path`, `line`, `column` and `message`; `Display` prints one per line. Log it with `game_error!("{error}")`. A game that cannot load a file should show something, not panic.
- **Build every tree before spawning any.** A template that fails to build then leaves nothing half on screen: build the root and every template instance first, then `spawn_ui` and `spawn_ui_in`.

#### Bindings

A binding is a name whose value text shows and value widgets read and write.

- **Patterns.** In a file, every string that becomes a label or tooltip is a pattern: `Text`'s `text`, the `label` of `Button`, `Checkbox` and `Toggle`, `tooltip`, and `Confirm`'s `title`, `message`, `yes` and `no`. `{name}` shows the value (whole numbers without a decimal point, other numbers in their shortest form, bools as `true` or `false`); `{name:.1}` gives one decimal; `{{` and `}}` print braces. **A literal brace in any of these strings is `{{`.** A `Slider`'s, `Progress`'s or `Rebind`'s `label` is a kind option, not a component: it is literal, with no patterns. A `TextInput`'s `text` and `placeholder` are not patterns.
- **`bind: "volume"`** on a value widget ties it to the name: `Slider`, `Progress`, `Checkbox`, `Toggle`, `RadioGroup`, `Dropdown`, `Tabs`, `ChoiceList` and `TextInput`. Any other kind is an error. The widget and `"Volume: {volume:.1}"` then follow each other with no game code, and so does every other widget and pattern bound to the name.
- **Values** are `BindValue`s: numbers, strings and bools (`From` for the usual types). A bound `Slider` or `Progress` takes a number; `Checkbox` and `Toggle` a bool; `RadioGroup`, `Dropdown`, `Tabs` and `ChoiceList` a whole, non-negative number (the index); `TextInput` text (a number or bool is formatted). A value of the wrong type warns once, naming the binding and the node, and leaves the widget as it is.
- **`ctx.ui_set(node, "score", 120)`** sets the name on `node`'s root: it writes every bound widget (`UiValue`, `UiChecked`, `UiIndex` or `UiTextValue`) and formats every label and tooltip that uses the name. A value equal to the one held does nothing at all (no write, no formatting), so setting a value every frame is cheap. **`ctx.ui_get::<f32>(node, "volume")`** reads it back (`f32`, `f64`, `i32`, `u32`, `usize`, `bool`, `String`).
- **One set of values per spawned root.** Two HUDs from one file never share values, and nodes code adds under a root use that root's. Pass any node of the tree: `ui_set(hud, ..)`. Each row of a list is a scope of its own, with its own values: `ui_set(row, ..)` fills that row (section 16).
- **Starting values.** A bound widget's file value (`value`, `checked`, `on`, `selected`, `text`) seeds its name (`value: 0.8` seeds `volume`); the top-level `bindings` seeds names only text uses. A seed applies only to a name with no value yet, so a reload never resets a score. A name never set shows as nothing, and debug builds warn once per name.
- **The components stay the truth.** The player moving a bound slider, `ui_set`, and game code writing `UiValue` directly all change the name, and the `Changed` `UiEvent` fires as usual. A pattern drawn after the widget shows the new value in the same frame; one drawn before it shows it the next.
- **In a code tree,** `UiNode::bind(name)` binds a value widget and `.bound()` makes a node's label and tooltip patterns (code text is literal otherwise). Binding a kind that holds no value warns once.

#### Updating a tree: `ctx.update_ui`

`ctx.update_ui(node, tree) -> Entity` brings a spawned tree in line with a new `UiNode`, in place. A reload is the same operation with the file's new tree. Rebuild a `UiNode` from your data whenever the data changes and let the engine apply only the differences.

- **Matching is by key** among siblings. Same key and kind: updated in place (the entity, its id, focus, scroll offset, open list and caret survive). Same key, different kind: despawned, and a new node spawned in the same place. Only in the new tree: spawned. Only in the old: despawned with its subtree. The new tree's order wins. If `tree`'s top is a different kind, the old node is replaced and the new entity returned. A node that was not spawned from a tree warns once and is returned unchanged.
- **The tree owns its settings** (layout, flags, variant, action, name, key, neighbours, kind options, `bind`) and rewrites them every update.
- **The game and the player own the state they wrote.** `UiLabel`, `UiChecked`, `UiValue`, `UiIndex`, `UiOptions`, `UiTextValue`, `UiSlot`, `UiImage`, `UiTooltip`, `UiHidden`, `UiDisabled` and a root's placement are written only when the tree's value differs from what the tree last applied. So an update that moves a button keeps the score the game wrote into a label and the slider the player dragged, while one that changes a label's text in the tree replaces it. Two exceptions: **a bound widget's value belongs to its binding** (the tree's value only seeds the name; an update never writes it), and **a root's placement** (`UiRoot`'s `anchor`, `offset`, `at`) is the game's to change.
- **The game's additions stay.** Components the game inserted on matched nodes stay, and a tree's `.insert` bundles apply only when a node is spawned. Subtrees added with `spawn_ui_in` stay, each after the sibling it followed, until their parent despawns. `ctx.update_ui` replaces everything else in the tree it is given; a reload replaces only what came from the file, so nodes the game put in with `.child(..)` before spawning are left too.
- **A placeholder stays the file's.** `ctx.update_ui` on a node of a file's tree that is not its root (a placeholder whose contents code rebuilds) leaves the node the file's: a reload still updates the node itself, and leaves the children `update_ui` gave it.
- **Quiet.** Components are written only on a real change, so `Changed<..>` filters stay truthful, and an update sends no `UiEvent`s.
- **Focus after an update.** An updated focus scope is not a newly opened one, so it does not take the focus again. When the update adds a widget that should have it, call `ctx.focus_ui(entity)` (find it with `ctx.find_ui_in(root, name)`; it waits until the node is drawn). A `ChoiceList` takes the focus by its scope's own rule, so `focus_ui` is not needed for it. A despawned focused node leaves the focus empty, and the next navigation lands on the scope's default.
- **Cost.** One walk over both trees. An unchanged update of the 200-widget settings tree costs about a third of spawning it.

#### Hot reload

In debug builds with an assets directory (never headless), the asset manager watches `.ui.ron` files. When one changes, it is read and checked again, and so is every loaded file that imports it. A clean file replaces the old one, and every tree built from it is reconciled before that frame's UI pass: roots and code-built template instances, in every scene. Then a `UiReloaded { root, replaced }` message goes out for each root touched: `replaced` is the old root when the file gave the root a new kind (so the root is a new entity), and `None` when it was updated in place.

- **What a reload keeps:** entities and their ids, focus, scroll, widget state, and what the game and the player wrote, unless the file changed that same value (the update rules above). Bindings keep their values and pick up new and removed names.
- **What it leaves:** components the game inserted, `spawn_ui_in` subtrees and `.child(..)` additions (a template instance among them is rebuilt from its template with its arguments and updated in place). An option set in code on a template instance's built node is lost; pass it in `UiArgs` (Loading and building). A node the file gives a new kind is a new entity, so a game that holds a root, filled a placeholder or looked up a node by entity listens for `UiReloaded`: it swaps a held root when `replaced` names it, and looks nodes up again by name (and fills them again) when `root` is its tree.
- **A broken file** keeps the old document and trees. The errors are warned once per broken text, with positions; saving a mistake again after the file was fixed or reverted warns again.
- **A file that checks but lost its `root` or a template** replaces the old document like any clean file. Trees that can no longer be built (from the lost root, or instances of the lost template) are kept as they were and warned about once, everything else is brought in line, and new `ctx.ui_template` calls for the lost template fail.
- `ctx.assets.reload_ui(path)` asks for the same reload directly (tests, headless runs). A file added with `add_ui` is never reloaded. Release builds read files once and never watch them.
- `ryo2d check-ui [files]` (`ryo-cli`) checks files without running the game: syntax, vocabulary, templates, imports, keys and image paths, with the same messages.

A game that holds a root or a node of a file's tree follows what a reload replaced:

```rust
// In `ui` or `update`: follow what a reload replaced
for reloaded in ctx.messages::<UiReloaded>() {
  if self.hud.is_some() && self.hud == reloaded.replaced {
    self.hud = Some(reloaded.root);                         // the root is a new entity
  }
  if self.hud == Some(reloaded.root) {
    self.extras = ctx.find_ui_in(reloaded.root, "extras");  // held nodes, by name again
  }
}
```

Read held nodes with `ctx.world.get_mut::<C>(node)` or `ctx.world.get_entity_mut(node)`, never `entity_mut`, which panics on an entity a reload has replaced.

#### Recipes

##### A menu file with events

```ron
// assets/ui/main_menu.ui.ron
(
  templates: {
    "MenuButton": (
      params: ["label", "action"],
      defaults: { "focus": false },
      node: Button(label: label, action: action, fill_width: true, default_focus: focus),
    ),
    // A template cannot share a kind's name, so not `Confirm`
    "QuitConfirm": (
      node: Confirm(title: "Quit to desktop?", message: "Unsaved progress is lost.", yes: "Quit", no: "Stay"),
    ),
  },
  root: Panel(
    focus_scope: true, wrap_focus: true, anchor: Center, width: 260, gap: 8,
    children: [
      Text(text: "MY GAME", heading: true),
      MenuButton(label: "Play", action: "play", focus: true),
      MenuButton(label: "Options", action: "options"),
      MenuButton(label: "Quit", action: "quit"),
    ],
  ),
)
```

```rust
const MENU: &str = "assets/ui/main_menu.ui.ron";

#[derive(Default)]
struct MainMenu {
  quit_dialog: Option<Entity>,
}

impl Scene for MainMenu {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    if let Err(error) = ctx.spawn_ui_file(MENU) {
      game_error!("{error}");
    }
  }

  fn ui(&mut self, _ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
    for event in ctx.ui_events() {
      if event.kind != UiEventKind::Clicked {
        continue;
      }
      match event.action.as_deref() {
        Some("play") => return SceneAction::Switch(Box::new(Level::default())),
        Some("quit") if self.quit_dialog.is_none() => {
          // Build, then spawn: nothing half-open if the template is broken
          match ctx.ui_template(MENU, "QuitConfirm", &UiArgs::new()) {
            Ok(dialog) => self.quit_dialog = Some(ctx.spawn_ui(dialog)),
            Err(error) => game_error!("{error}"),
          }
        }
        // The confirm's buttons; Back on it is a "no" click
        Some("yes") => return SceneAction::Quit,
        Some("no") => {
          if let Some(dialog) = self.quit_dialog.take() {
            ctx.world.despawn(dialog);
          }
        }
        _ => {}
      }
    }
    SceneAction::Continue
  }
}
```

The file's buttons carry `action` strings; the game matches them, as for any node tree. Edit the file's text, colours or order while the game runs and the menu follows.

##### A HUD with bindings

```ron
// assets/ui/hud.ui.ron
(
  templates: {
    "Line": (
      params: ["text", "color"],
      node: Column(height: 18, children: [Text(text: text, color: color)]),
    ),
  },
  bindings: { "lives": 3, "score": 0 },
  root: Row(
    anchor: TopLeft, width: Pct(1), padding: (20, 10), justify: SpaceBetween,
    children: [
      Line(text: "Lives: {lives}", color: "#ffffff"),
      Line(text: "Score: {score}", color: "#ffff00"),
    ],
  ),
)
```

```rust
// on_enter
self.hud = ctx.spawn_ui_file("assets/ui/hud.ui.ron").ok();

// update, every frame: ui_set does nothing when a value is unchanged
if let Some(hud) = self.hud {
  ctx.ui_set(hud, "lives", self.lives);
  ctx.ui_set(hud, "score", self.score);
}

// ui: a saved file that gave the root a new kind replaced it (values move with it)
for reloaded in ctx.messages::<UiReloaded>() {
  if self.hud.is_some() && self.hud == reloaded.replaced {
    self.hud = Some(reloaded.root);
  }
}
```

`Text` has no `width`, so each line sits in a sized `Column`. `Line`'s `text` is a template parameter holding a pattern: its `{lives}` is not a parameter, so it stays a binding.

##### A list built from a template into a placeholder

```ron
// assets/ui/shop.ui.ron
(
  templates: {
    "ShopRow": (
      params: ["name", "price"],
      node: Row(
        gap: 8, align: Center,
        children: [
          Column(grow: 1, children: [Text(name)]),
          Text("{price} g"),
          Button(label: "Buy", action: "buy"),
        ],
      ),
    ),
  },
  root: Panel(
    focus_scope: true, anchor: Center, width: 360, gap: 8,
    children: [Text(text: "Shop", heading: true), Column(gap: 4, name: "rows")],
  ),
)
```

```rust
#[derive(Component, Clone, Copy)]
struct ShopItem(usize);

const SHOP: &str = "assets/ui/shop.ui.ron";

fn open_shop(ctx: &mut SceneContext, stock: &[(&str, u32)]) -> Result<Entity, UiFileError> {
  // Build the root and every row first, then spawn: a failure leaves nothing on screen
  let root = ctx.assets.load_ui(SHOP)?.root(ctx.assets)?;
  let mut rows = Vec::new();
  for (i, (name, price)) in stock.iter().enumerate() {
    let args = UiArgs::new().with("name", *name).with("price", *price);
    rows.push(ctx.ui_template(SHOP, "ShopRow", &args)?.insert(ShopItem(i)));
  }
  let shop = ctx.spawn_ui(root);
  if let Some(list) = ctx.find_ui_in(shop, "rows") {
    for row in rows {
      ctx.spawn_ui_in(list, row);
    }
  }
  Ok(shop)
}
// A Clicked with action "buy" has the Buy button as `event.node`; its parent is the row
// that carries `ShopItem(i)`
```

The rows are template instances the game added: a reload of `shop.ui.ron` updates the panel, and re-expands each row from `ShopRow` with its own arguments and updates it in place, while the rows stay in `rows`. Listen for `UiReloaded` to fill `rows` again if the file gave it a new kind.

##### Rebuilding choices with `update_ui`

```rust
/// The choices column for what the player can do now: one tree from the data
fn choices_tree(ctx: &mut SceneContext, labels: Vec<String>) -> Result<UiNode, UiFileError> {
  let args = UiArgs::new().with("options", labels);
  let list = ctx.ui_template("assets/ui/dialogue.ui.ron", "Choices", &args)?;
  Ok(UiNode::column().focus_scope().name("choices").child(list))
}
// "Choices" in the file: params: ["options"], node: ChoiceList(options: options, action: "choose", name: "pick")

/// The column after the update: `column`, or the node that replaced it if the tree is another kind
fn show_choices(ctx: &mut SceneContext, column: Entity, labels: Vec<String>) -> Entity {
  match choices_tree(ctx, labels) {
    // Only the differences are applied; focus and scroll survive
    Ok(tree) => ctx.update_ui(column, tree),
    Err(error) => {
      game_error!("{error}");
      column
    }
  }
}
```

If `choices` is a placeholder in a file (`Column(focus_scope: true, name: "choices")`), it stays the file's: a reload of the file updates the column and leaves the choices in it. If the column's content can be a `Button` (a Skip, a Continue) rather than a `ChoiceList`, call `ctx.focus_ui(pick)` after the update for the new button: an updated scope is not newly opened. The `ChoiceList` takes the focus by its scope's own rule.

##### Loading with an embedded fallback

A game that ships as one binary or for the web has no assets directory at run time. Keep the file on disk for development and embed a copy:

```rust
/// A UI file of the game: from disk when `path` is there (debug builds reload it when
/// it is saved), else the copy embedded in the binary
pub fn ui_file(ctx: &mut SceneContext, path: &str, embedded: &str) -> Result<UiFile, UiFileError> {
  match ctx.assets.load_ui(path) {
    Err(AssetError::NotFound(_)) => ctx.assets.add_ui(path, embedded),
    loaded => Ok(loaded?),
  }
}

pub fn spawn_ui_file(ctx: &mut SceneContext, path: &str, embedded: &str) -> Option<Entity> {
  let tree = ui_file(ctx, path, embedded).and_then(|file| file.root(ctx.assets));
  match tree {
    Ok(tree) => Some(ctx.spawn_ui(tree)),
    Err(error) => {
      game_error!("{path}: {error}");
      None
    }
  }
}

// spawn_ui_file(ctx, "assets/ui/pause.ui.ron", include_str!("../assets/ui/pause.ui.ron"))
```

`add_ui` caches the text under `path`, so later loads and imports of that path find it. An embedded file imports other files by their paths too: embed those and `add_ui` them first, or they are read from disk. This is breakout's `look.rs` (`ui_file`, `spawn_ui_file`); any error other than a missing file, such as a mistake in the file on disk, is returned rather than hidden by the embedded copy.

## 16. Virtual Lists

A **virtual list** shows thousands of items for the cost of a screenful. It is a scroll area as long as the whole list that builds only the rows in view, and a few either side (the **overscan**), with spacers standing in for the rest. Both styles have one: `ui.list(count)` in a closure, and `UiNode::list(count, row_height)` (`List` in a file) in a node tree, which keeps a few row entities and reuses them as it scrolls. The `ui` demo's Big list page (`examples/demos/ui/src/scenes/big_list_page.rs`, with `examples/demos/ui/assets/ui/big_list.ui.ron`) shows one 10,000-row leaderboard in both, side by side.

| Use a virtual list for | Use a scroll area (section 6) for |
|---|---|
| Far more rows than fit: leaderboards, logs, save slots, server lists, inventories of hundreds | A few dozen rows, cheap to build every frame |
| Rows of one height (with `columns`, cells of one size) | Rows as tall as what they hold (wrapped text) |

- **Give a list a height:** `height`, `max_height`, `fill` or `fill_height`, or `grow` in a sized parent. A list given none grows to its whole length (10,000 rows of 28 make a view 280,000 tall), so it cannot scroll and overflows its panel. It then builds only the rows that show on screen (or in a scroll area around it), and warns once: "give the list a height, a max_height or fill: without one it grows to its full length".
- **One row height.** The scroll length is exact, `rows × (row_height + gap) − gap` (rows: the items divided by `columns`, rounded up), whatever the rows hold. Row contents taller than `row_height` draw over the next row, so size them to fit: fixed heights, `fill`, text kept to a line (`.max_lines(1)`, `.ellipsis()`). Rows are placed at `f32` positions, so past about 10 million items they lose precision.
- **A list is a scroll area, not a focus scope.** Put it in one (section 7), as a menu's buttons are, so the pad walks it.
- **Bad sizes draw nothing.** A row height that is not a finite number above 0 warns once and draws the list empty, as does a closure list given no `.row_height`. `overscan` is clamped to 1..=64 and `columns` to 1..=256, and a frame builds at most 4,096 items for the view (warned once per list), however short the rows.

### Closure lists: `ui.list`

```rust
let list = ui
  .list(self.scores.len())
  .row_height(28.0) // required
  .gap(2.0)
  .fill()
  .show(|ui, i| {
    // Called for each item built this frame, in a container keyed by `i`
    ui.row().gap(8.0).show(|ui| {
      ui.text(format!("{}", i + 1));
      ui.text(self.scores[i].name.as_str());
    });
  });
// list.id names the list (list_focus, scroll_offset); list.range is the items built for the view
```

- **Options:** `.row_height(h)`, `.columns(n)` (a grid of equal cells, filled row by row), `.gap(g)` (between rows and between cells), `.overscan(rows)` (default 2), `.id(key)`, and the scroll area's size and placement options (`width`, `height`, `fill_width`, `fill_height`, `fill`, `min_width`, `max_width`, `min_height`, `max_height`, `padding`, `margin`, `grow`, `anchor`, `offset`, `at`, `no_bar`). Lists scroll vertically.
- **`.show(|ui, i| ..) -> ListResponse { id, range, response }`.** Each item built is drawn in a container of its own, a row tall (a cell, in a grid), keyed by its index, so the widgets in it keep their ids, and with them their hover, focus and memory, wherever the item scrolls. `range` is the items built for the view this frame (the rows in view and the overscan, not a focused row kept built out of view); `response` is the scroll area's.
- **`ui.list_scroll_to(list.id, index)`** scrolls by the least that shows the item's row. **`ui.list_focus(list.id, index)`** also gives the first focusable widget in the row the focus, up to two frames after the row is first built. An index past the end is the last item; a list not drawn yet ignores both. The scroll area's own calls take the list's id: `ui.scroll_offset(list.id)`.
- The closure runs only for the items built, so it can index the game's data and format their text without costing the whole list.

### Node lists: `UiNode::list`

```rust
fn board() -> UiNode {
  UiNode::list(10_000, 28.0)
    .gap(2.0)
    .fill()
    .row_shape(|| {
      // Required: one row's tree, which every pooled row is spawned from
      UiNode::row()
        .gap(8.0)
        .child(UiNode::text("{rank}").bound())
        .child(UiNode::text("{name}").bound())
    })
    .name("board")
}
```

```ron
// In a file: row_shape names a template (bare or as a string), or is a node
List(count: 10000, row_height: 28, gap: 2, fill: true, row_shape: LeaderRow, name: "board")
```

- **Options:** `.columns(n)`, `.overscan(rows)`, `.gap(g)`, `.no_bar()`, the scroll area's size and placement options, and `.row_shape(f)`. In a file `count` defaults to 0, and `row_height` and `row_shape` are required (`List needs "row_shape"`); in code a list without a row shape warns once and builds no rows.
- **The pool.** The list keeps about a screenful of row entities, plus the overscan, whatever its count. Each carries a `UiListRow` (`index()`: the item it shows). As the view moves, a row whose item went out is given one coming in; rows are spawned from the row shape when more are needed and despawned when fewer are.
- **`UiListBind { list, row, index }`** is sent for every row given an item (new, or reused for another item): fill the row from the game's data.
  - `ctx.ui_set(bind.row, "name", value)`: **each row is its own binding scope**, so this fills that row's `{name}` patterns and bound widgets only;
  - or the row's nodes' components: **each row is its own naming scope** too, so `ctx.find_ui_in(bind.row, "name")` finds the row's node, and `find_ui` and `find_ui_in` from the tree's root never look inside rows.

  Set every value the row shows: a reused row holds its last item's values until it is bound.
- **Every list's binds reach every scene**, so keep to your list's: `if bind.list != board { continue; }`.
- **Bind in `ui`.** The pool follows the scroll once a frame, at the start of the UI pass, before any tree is drawn. A row given an item is hidden that frame and drawn from the next, so the game binds it in between: in the scene's `ui` (recommended: it runs every frame, after the scene's trees are walked) or in an observer on the row (`UiListBind` is also an entity event triggered on the row, so the observer fills it at once; it must not change scenes). Binding in `update` costs a frame: `update` runs at the fixed rate, so when the display runs faster a row can draw unfilled for a frame before a fixed step binds it, and debug builds warn once for each name it shows with no value yet ("no value is set for the UI binding ..").
- **The game API** (on `SceneContext`, and on `UiHarness`):
  - `ctx.ui_list_count(list, n)`: the item count. It is state, like a widget's value: a reload or `update_ui` writes it only when the tree's count changed;
  - `ctx.ui_list_refresh(list)`: bind every pooled row again next frame, after the game's data changed; `ctx.ui_list_refresh_row(list, index)` for one item's row (a row not pooled needs none: it is bound when it shows). A refreshed row keeps drawing what it showed until it is bound, so nothing flashes;
  - `ctx.ui_list_scroll_to(list, index)` and `ctx.ui_list_focus(list, index)`, as the closure calls; the focus goes to the row's first focusable node (depth first, neither hidden nor disabled), up to two frames after the row is bound;
  - `ctx.ui_list_index(node) -> Option<usize>`: the item of the row a node is in, so a `UiEvent` from a row's widget maps to its item.

  On a node that is not a list each warns once and does nothing.
- **Events from rows** are ordinary `UiEvent`s: give the row's widget an `action`, and map `event.node` to its item with `ui_list_index`.
- **A new row shape** (a reload, or `update_ui` with another `.row_shape`) that builds another row brings every pooled row in line where it is and binds it again (hidden until bound, as a new row is). One that builds the same row leaves the rows alone.
- **Rows are drawn in their item's container**, keyed by index as closure rows are, so a reused row entity never carries another item's focus or hover.

### Focus, the pad and jumps

In both styles:

- **The pad walks the whole list.** The overscan keeps the row past each edge of the view built, so a direction steps onto it and the area scrolls it in. In a grid, Left and Right move along a row and Up and Down by a row. At the first and last items the pad goes on as from any widget, to the next one that way in the focus scope.
- **The focused row stays built.** Scrolled away by the wheel or the scrollbar, the row with the focus (and one row each side) stays built, clipped out of view, so it keeps the focus and the pad steps on from it.
- **Jumps never sweep.** A `list_scroll_to` or `list_focus` (`ui_list_*` for a node list) further than a view first moves the list to one view short of the item, then eases the rest, so the rows between are never built.
- **Nor does any far scroll.** When the list is heading further than two views from where it shows (the pad returning to a focused row the wheel left far behind, or a big wheel spin), it first moves to one view short of where it is heading, then eases the rest. A node list's rows come from its pool, given their items that frame, so for that one frame its view is empty; from the next, every frame of the ease is drawn.
- **A shrinking count** clamps the scroll in the frame the list is drawn with it. A focus on an item past the new end moves to the new last item if the list held it, and goes when the list is empty.
- **Where the focus starts.** A closure list's rows are widgets like any others: a scope that appears focuses its `.default_focus()` widget (the first row's, `if i == 0`) or its first. A node list's rows show from its second frame, so other focusables in its focus scope take the scope's default focus (a scope holding only the list gives it to the first row once the rows show). To start on a row, call `ctx.ui_list_focus(list, 0)` after spawning.

### What a row keeps

- **Widget memory goes with the row.** What a row's widgets remember (a text field's caret, an open dropdown, a tooltip timer) is dropped when the row leaves the built rows, as for any widget not drawn. Keep what must last in the game's data: the row's text in a `Vec<String>`, a choice in the game's state.
- **Ids follow the index, not the item.** When the game sorts or filters the list, the focus stays at its index, now another item's; give it the item's new index with `list_focus` to follow the item.

### Recipes

Each compiles as written.

#### A leaderboard (closure)

```rust
/// One player on a leaderboard
struct Entry {
  name: String,
  score: u32,
}

/// A leaderboard of any length as a closure list, with a jump to a rank
struct Leaderboard {
  board: Vec<Entry>,
  /// The jump field's text
  rank: String,
  status: String,
}

impl Leaderboard {
  fn new(board: Vec<Entry>) -> Self {
    Self {
      board,
      rank: String::new(),
      status: "Pick a player".to_string(),
    }
  }
}

impl Scene for Leaderboard {
  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    let muted = ui.theme().palette.muted;
    let board = &self.board;
    let (mut go, mut picked, mut list) = (false, None, None);
    ui.panel()
      .focus_scope()
      .anchor(Anchor::Center)
      .width(440.0)
      .height(Length::Pct(0.8))
      .gap(8.0)
      .show(|ui| {
        ui.row().gap(8.0).align(Align::Center).show(|ui| {
          ui.text("Rank");
          go = ui
            .text_input(&mut self.rank)
            .width(80.0)
            .max_len(6)
            .show()
            .submitted();
          go |= ui.button("Go").clicked();
          ui.text(self.status.as_str()).color(muted);
        });
        let shown = ui
          .list(board.len())
          .row_height(28.0)
          .gap(2.0)
          .fill()
          .show(|ui, i| {
            let Some(entry) = board.get(i) else {
              return;
            };
            // A button across the row takes the focus and the click; the texts sit on it
            ui.stack().fill().show(|ui| {
              let mut pick = ui
                .button("")
                .width(Length::Pct(1.0))
                .height(Length::Pct(1.0));
              if i == 0 {
                pick = pick.default_focus();
              }
              if pick.clicked() {
                picked = Some(i);
              }
              ui.row()
                .fill()
                .padding((12.0, 0.0))
                .gap(12.0)
                .align(Align::Center)
                .show(|ui| {
                  ui.column().width(48.0).show(|ui| {
                    ui.text(format!("{}", i + 1)).color(muted);
                  });
                  ui.column().fill_width().show(|ui| {
                    ui.text(entry.name.as_str());
                  });
                  ui.text(format!("{}", entry.score));
                });
            });
          });
        list = Some(shown.id);
      });

    if let Some(entry) = picked.and_then(|i| self.board.get(i)) {
      self.status = format!("{}: {} points", entry.name, entry.score);
    }
    if go && let (Some(list), Ok(rank)) = (list, self.rank.trim().parse::<usize>()) {
      // Scrolls there (one view short, then eased) and focuses the row
      ui.list_focus(list, rank.saturating_sub(1));
    }
    if ui.back() {
      return SceneAction::Pop;
    }
    SceneAction::Continue
  }
}
```

#### A leaderboard (nodes, from a file)

The same board (`Entry` above) as a node list. The file holds the row as a template; the game gives the list its count and fills each row it binds.

```ron
// assets/ui/leaderboard.ui.ron
(
  templates: {
    // One row: a button across it, under the rank, the name and the score
    "LeaderRow": (
      node: Stack(
        fill: true,
        children: [
          Button(label: "", width: Pct(1), height: Pct(1), action: "pick"),
          Row(
            fill: true, padding: (12, 0), gap: 12, align: Center,
            children: [
              Column(width: 48, children: [Text(text: "{rank}", muted: true)]),
              Column(fill_width: true, children: [Text("{name}")]),
              Text("{score}"),
            ],
          ),
        ],
      ),
    ),
  },
  bindings: { "status": "Pick a player" },
  root: Panel(
    focus_scope: true, anchor: Center, width: 440, height: Pct(0.8), gap: 8,
    children: [
      Text(text: "Leaderboard", heading: true),
      Text(text: "{status}", muted: true),
      List(count: 0, row_height: 28, gap: 2, fill: true, row_shape: LeaderRow, name: "board"),
    ],
  ),
)
```

```rust
const LEADERBOARD: &str = "assets/ui/leaderboard.ui.ron";

/// The leaderboard as a node list from a file: a screenful of pooled rows,
/// each filled from the game's data when it is given an item
struct NodeLeaderboard {
  board: Vec<Entry>,
  root: Option<Entity>,
  list: Option<Entity>,
}

impl NodeLeaderboard {
  fn new(board: Vec<Entry>) -> Self {
    Self {
      board,
      root: None,
      list: None,
    }
  }

  /// Hold the tree and its list, and give the list the board's length
  fn hold(&mut self, ctx: &mut SceneContext, root: Entity) {
    self.root = Some(root);
    self.list = ctx.find_ui_in(root, "board");
    if let Some(list) = self.list {
      ctx.ui_list_count(list, self.board.len());
    }
  }
}

impl Scene for NodeLeaderboard {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    match ctx.spawn_ui_file(LEADERBOARD) {
      Ok(root) => self.hold(ctx, root),
      Err(error) => game_error!("{error}"),
    }
    if let Some(list) = self.list {
      // A node list's rows show from its second frame: start on rank 1
      ctx.ui_list_focus(list, 0);
    }
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
    // A saved file was brought in line before the UI pass: a respawned list
    // starts from the file's count, so hold it again
    for reloaded in ctx.messages::<UiReloaded>() {
      if self
        .root
        .is_some_and(|root| root == reloaded.root || Some(root) == reloaded.replaced)
      {
        self.hold(ctx, reloaded.root);
      }
    }
    // Fill each row given an item this frame: it is drawn from the next
    for bind in ctx.messages::<UiListBind>() {
      // Every list's binds come here: answer only this one's
      if Some(bind.list) != self.list {
        continue;
      }
      let Some(entry) = self.board.get(bind.index) else {
        continue;
      };
      // The row's own values: each row is a binding scope
      ctx.ui_set(bind.row, "rank", bind.index + 1);
      ctx.ui_set(bind.row, "name", entry.name.as_str());
      ctx.ui_set(bind.row, "score", entry.score);
    }
    // A row's button was clicked: the row's item
    for event in ctx.ui_events() {
      if event.kind == UiEventKind::Clicked
        && event.action.as_deref() == Some("pick")
        && let (Some(root), Some(i)) = (self.root, ctx.ui_list_index(event.node))
        && let Some(entry) = self.board.get(i)
      {
        // The root's values: the status line outside the list
        ctx.ui_set(
          root,
          "status",
          format!("{}: {} points", entry.name, entry.score),
        );
      }
    }
    if ui.back() {
      return SceneAction::Pop;
    }
    SceneAction::Continue
  }
}
```

`Text` takes no `width`, so the rank and the name sit in sized columns. When the board changes (a new score), change the data, then `ctx.ui_list_count(list, len)` and `ctx.ui_list_refresh(list)`: the pooled rows are bound again and show the new order.

#### An inventory grid

```rust
/// One stack in the bag
struct Item {
  name: String,
  icon: SpriteHandle,
  count: u32,
}

/// A bag of hundreds of stacks in a grid eight wide: only the rows in view
/// are built
struct Bag {
  items: Vec<Item>,
  selected: Option<usize>,
}

impl Bag {
  fn new(items: Vec<Item>) -> Self {
    Self {
      items,
      selected: None,
    }
  }
}

impl Scene for Bag {
  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    let items = &self.items;
    let selected = self.selected;
    let mut picked = None;
    ui.panel()
      .focus_scope()
      .anchor(Anchor::Center)
      .width(480.0)
      .gap(8.0)
      .show(|ui| {
        ui.text(format!("Bag: {} stacks", items.len()));
        ui.list(items.len())
          .columns(8) // equal cells, filled row by row
          .row_height(52.0)
          .gap(4.0)
          .height(300.0)
          .fill_width()
          .show(|ui, i| {
            let Some(item) = items.get(i) else {
              return;
            };
            ui.column().fill().center().show(|ui| {
              let slot = ui
                .slot()
                .icon(&item.icon)
                .count(item.count)
                .selected(selected == Some(i))
                .tooltip(item.name.as_str())
                .show();
              if slot.clicked() {
                picked = Some(i);
              }
            });
          });
        let detail = match selected.and_then(|i| items.get(i)) {
          Some(item) => format!("{} x{}", item.name, item.count),
          None => "Nothing selected".to_string(),
        };
        ui.text(detail);
      });
    if picked.is_some() {
      self.selected = picked;
    }
    if ui.back() {
      return SceneAction::Pop;
    }
    SceneAction::Continue
  }
}
```

A slot keeps its own size, so each sits centred in its cell. The pad moves along a row with Left and Right and between rows with Up and Down; Confirm picks.

## 17. Testing UI

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
- **Node trees:** `spawn_ui(tree) -> Entity` spawns a tree as a root of the harness's scene, and each `frame` draws the harness's trees before its closure, as a scene's are drawn before its `ui`. `events() -> &[UiEvent]` lists the last frame's node events in walk order. `find_ui(name)` and `ui_id(node)` work as on `SceneContext`, and `world()` / `world_mut()` reach the components. `frame_with_world(|ui, world| ..)` gives the closure the world as well, for `ui.node(root, world)`.

  ```rust
  let mut ui = UiHarness::new(320.0, 240.0);
  ui.spawn_ui(UiNode::column().focus_scope().child(UiNode::checkbox("Sound", false).name("sound")));
  ui.frame(|_| ());
  ui.frame(|_| ());                            // the scope takes focus
  ui.action("ui.confirm");
  ui.frame(|_| ());
  let sound = ui.find_ui("sound").unwrap();
  assert_eq!(ui.world().get::<UiChecked>(sound), Some(&UiChecked(true)));
  assert!(ui.events().iter().any(|e| e.node == sound && e.kind == UiEventKind::Changed));
  ```
- **UI files in a test:** `UiHarness::new(w, h).with_assets(dir)` loads files and images relative to `dir`; `spawn_ui_file(path) -> Result<Entity, UiFileError>` spawns one; `update_ui(node, tree)`, `ui_set(node, name, value)` and `ui_get::<T>(node, name)` work as on `SceneContext`. After editing the file on disk, `reload_ui(path)` makes the next `frame` bring the trees in line, and `messages::<UiReloaded>()` lists the roots it touched. `UiFile::parse(name, text)` checks text alone, for asserting a file's errors.
- **Lists in a test:** `frame` draws a closure list like any widget. A node list's binds are in `messages::<UiListBind>()` after the frame that sent them: fill the rows there with `ui_set`, as a scene's `ui` would, and they are drawn from the next `frame`. `ui_list_count`, `ui_list_refresh`, `ui_list_refresh_row`, `ui_list_scroll_to`, `ui_list_focus` and `ui_list_index` work as on `SceneContext`.

  ```rust
  let mut ui = UiHarness::new(320.0, 240.0);
  ui.spawn_ui(UiNode::column().focus_scope().child(
    UiNode::list(1000, 20.0).height(100.0).row_shape(|| UiNode::button("{name}").bound()).name("list"),
  ));
  let list = ui.find_ui("list").unwrap();
  ui.ui_list_focus(list, 700);
  for _ in 0..4 {
    ui.frame(|_| ());
    let binds: Vec<UiListBind> = ui.messages::<UiListBind>().collect();
    for bind in binds {
      ui.ui_set(bind.row, "name", format!("Item {}", bind.index));
    }
  }
  ui.action("ui.confirm");
  ui.frame(|_| ());
  let click = ui.events().iter().find(|e| e.kind == UiEventKind::Clicked);
  assert_eq!(click.and_then(|e| ui.ui_list_index(e.node)), Some(700));
  ```

- **With the engine**, `HeadlessEngine` (`ryo-game-dev`, Visual Tests) drives whole scenes: `tap_pad(GamepadButton::DPadDown)`, `press_pad`, `release_pad`, `set_stick(Stick::Left, x, y)`, `wheel(dx, dy)`, `tap_key`, `mouse_move`; `ui_focused()` is the focus the last step resolved, `ui_focusables()` what the D-pad can land on now (id and rect), `focus_ui(id)` focuses a widget at the next step. Walking every `ui_focusables()` with the D-pad from each one proves a page has no dead ends.
- **Scripted runs:** `RYO_INPUT` presses pad buttons and holds sticks in dev builds, for captures: `RYO_INPUT="60:PadDPadDown 90:PadSouth 120-180:LeftStick@0x-1 200:Wheel@0x-3"` (`ryo-cli`).
- The console command `debug:ui` outlines every widget with its id, hot, active and focused ones coloured (`ryo-console`).

## 18. Tips

- **Mark every menu `.focus_scope()`**, and give it a `.default_focus()`. Without a scope the pad and the arrows do nothing in it. This holds for node trees too.
- **Node trees: read events in one place.** `ctx.ui_events()` and `ctx.messages::<UiEvent>()` share a cursor, so read in `ui` or in `update`, not both. Change scenes from there, never from an observer.
- **Files: run `ryo2d check-ui` and read the messages.** They carry line, column and the nearest valid name. A template named like a kind, a `Text` with a `width`, a `{` in a label that is not a binding, and a template used from a file it was not imported into are the usual mistakes.
- **Files: the game keeps what the player and it wrote.** A reload or `update_ui` rewrites the tree's settings and leaves values the game and the player set, unless the file changed that value. A bound widget's value is the binding's.
- **Close the innermost thing on `ui.back()`**, then the next: a modal before its menu, a menu before the scene.
- **Use `.id(key)`** for rows of a list that sorts, filters or deletes; call-site ids shift when an earlier call is skipped.
- **Hundreds of rows: a virtual list** (section 16), in either style, rather than a scroll area that builds them all. Give the list a height (`fill`, `height`, `max_height`), give its rows one height and keep their contents inside it; bind a node list's rows in `ui`, and set every value a row shows.
- **Your data, its state.** Widgets write your `bool`/`f32`/`usize`/`String`; an open list, a caret, a scroll offset and focus are the UI's, dropped when the widget is not drawn for a frame (focus memory lasts the scene).
- **Collect, then act.** Set a local in the closure (`clicked = ui.button(..).clicked()`) and change scenes or mutate other state after `.show()` returns.
- **Bind theme values before a closure** (`let heading = ui.theme().text.heading.style();`): `ui.theme()` borrows `ui`.
- **One theme for the game.** The theme is global; a scene that sets its own restores the old one in `on_exit`.
- A widget's `Response` inside a container has last frame's rect (this frame's for a top-level container): use it for anchoring popups, not for placing siblings.
- **Animate looks rather than layout.** Clicks are hit-tested where a widget was drawn last frame: a panel that slides in and settles is fine, a button that never stops moving is hard to click. Fade, tint or pulse instead.
- Many-sprite or world-space visuals belong in `Scene::render` (GPU batches) or in entities, not the UI.
