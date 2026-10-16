# UI Interaction (4b): Widget Ids, Layout, Focus and Navigation, Theme, Actions - Design

## Overview

Sub-project 4b of the professional-grade overhaul rebuilds how the UI is *used*. 4a rebuilt how it is drawn. 4b covers:

- **Widget identity:** automatic ids by call site, with engine-kept state memory.
- **Interaction:** resolved against last frame's laid-out tree, where the topmost widget under the pointer wins, clicks fire on release, and drags and wheel routing work.
- **Layout:** flexbox and grid through Taffy, behind the immediate API, with content measured (text through 4a's engine).
- **Focus and navigation:** keyboard and gamepad, automatic spatial navigation with overrides, focus scopes and focus memory.
- **An input action map:** named, rebindable, saved actions that also drive the UI.
- **Device awareness:** prompts for the last-used device, and a focus ring that shows only for keyboard and gamepad.
- **A widget set** for four kinds of game UI: menus and pause screens, settings screens, inventories and grids, and dialogue with choices.
- **Drag and drop:** mouse, plus a gamepad equivalent.
- **A theme resource:** per-state styles, variants, nine-slice skins and UI sounds.

The immediate-mode `Scene::ui` call style stays.

Sub-project 4c follows with its own spec: retained UI nodes (entities) on 4b's ids, focus, theme and layout styles.

**Why:** the UI's interaction layer is the one the audit described (`docs/research/2026-10-11-engine-audit.md`, section 5). Verified at main fed18d5:

- **No widget identity.** Widgets are known by the `Rect` passed in and by call order. The first widget called wins a click through consumption (`ui/immediate/context.rs:1006`). Hover is recomputed each frame.
- **Clips are ignored.** Hover and click ignore the clip stack, so a button scrolled out of a scroll view still reacts.
- **Occlusion is ignored.** The slider and the open dropdown list ignore occlusion (`context.rs:1313-1327`, `2200-2220`).
- **Modals don't block.** `draw_modal_overlay` only dims.
- **Activation is on press.** Buttons fire on mouse-down, not release.
- **Focus.** It exists only for text inputs and dropdowns (`retained/focus.rs`). Tab navigation is opt-in and manual, called after drawing. `FocusTrap` is exported and unused.
- **Gamepad.** No widget reads the gamepad. Games hand-roll pad menus in `update` (`examples/games/breakout/src/scenes/pause.rs:85-116`).
- **Theme.** It never reaches a widget: the engine builds `UiContext` with `theme: None` and nothing can set it (`engine/frame.rs:315`). Many theme fields are unused.
- **"Retained mode"** is three caller-owned state structs (`TextInputState`, `DropdownState` with a hand-picked `u32` id, `ScrollViewState`), not a node tree.
- **Layout** is `HBox`/`VBox`/`Stack` rect generators with explicit sizes. Nothing measures content.
- **Nested scroll views:**
  - The outer view consumes the wheel before its content runs (`context.rs:1730`), so the inner one never scrolls.
  - The wheel is a per-frame value with no fixed-step view (`input.rs:702-707`), so gameplay that reads it in fixed steps counts it twice, or drops it.
- **Lower scenes.** They run `ui` with the mouse occluded but the keyboard not, and they run first, so a lower scene can consume the top scene's keys.
- **`context.rs`** is 4,209 lines. Widgets, input and focus are all in one file.
- **Input** has no action maps or rebinding. Only the first gamepad is used, with a hard-coded deadzone.

**The owner's decisions behind this design (2026-10-16):**

- **Scope.** 4b is the interaction core plus immediate layout that measures content. 4c is retained nodes.
- **Target UIs.** All four (menus, settings, inventories, dialogue and choices) become acceptance demos.
- **Ids** are automatic by call site. `.id(key)` covers lists that reorder. UI-only state lives in engine memory, and callers pass only game data.
- **Layout** is flexbox and grid through Taffy, behind the immediate API. Hit tests use last frame's rects.
- **Navigation** is automatic and spatial, with overrides, scopes, focus memory and a default focus.
- **The theme** is an engine-held resource with per-state styles, named variants and nine-slice skins.
- **Extras in 4b:**
  - drag and drop;
  - device-aware prompts;
  - a rebinding widget, which needs an action map;
  - UI sound hooks.
- **The frame model** is a per-frame UI tree resolved after the hook (approach A of three).

## Architecture

`src/core/ui/` is reorganized by responsibility. Today's single `context.rs` is split up.

| Module | Holds |
|---|---|
| `id` | `UiId`, call-site hashing, scopes |
| `memory` | `UiMemory`: per-id records from last frame, hot/active/focus, typed widget state, dropping rules |
| `tree` | This frame's nodes: id, parent, layout style, widget kind, draw recipe, interaction flags |
| `layout` | Our style types and the Taffy glue. Taffy is named only in this module, as Parley is only in `text/` |
| `interact` | Pointer hit testing, hot/active/click/drag, wheel routing, text routing |
| `nav` | Focus, scopes, spatial navigation, default focus, focus memory |
| `widgets/` | One file per widget |
| `containers`, `popup` | Container builders; popups built where they are called and drawn at the scene's end |
| `theme/` | `Theme`, looks, variants, skins, sounds, RON load and hot reload |
| `immediate/` | `UiContext`, the absolute `draw_*` calls and 4a's `UiCommand` recording, unchanged in substance |

The action map lives in `src/core/input/actions.rs`: `src/core/input.rs` becomes `src/core/input/mod.rs`. `src/core/ui/theme.rs` becomes `src/core/ui/theme/`. The old `retained/` module and `immediate/layout.rs` are deleted once nothing uses them.

### One UI pass per frame

The pass runs where today's UI pass runs: after `PreUpdate` and the console tick, before the fixed steps.

1. **Resolve.** Before any scene's `ui` runs, the engine resolves this frame's input against last frame's records (rects, clips, layers, order, flags):
   - The pointer hit test picks the hot widget.
   - A press makes the hot widget active and captures its button; a release resolves the click.
   - The wheel is routed.
   - Navigation actions move focus. Confirm activates the focused widget. Typed text and editing keys go to the focused field.
   - Drag state advances.
   
   The results are stored per id in `UiMemory`.
2. **Record.** Scenes' `ui` hooks run bottom-up. Each widget builder, when it finishes (on `.show()`, a shorthand such as `.clicked()`, or on drop, so `ui.slider(&mut volume, 0.0..=1.0);` works as a statement):
   - registers a node in this frame's tree, with its id, style and draw recipe;
   - reads its id's results;
   - applies value edits to the caller's data immediately (a slider drag, typed text, a toggled bool, a picked index);
   - returns a `Response`.
3. **Lay out and emit.** Each top-level container (one with no open parent) is laid out when its `.show()` returns, inside the hook; a widget called outside any container is its own one-node subtree, laid out at the call:
   - Taffy lays the subtree out. Text is measured through the text engine, and the measured layout is the one drawn.
   - Rects snap to whole render pixels.
   - The draw recipes become 4a's `UiCommand`s at the resolved rects, in submission order.
   - Rects, clips, layers, order, focusables, scrollables and drop targets are saved as next frame's records.
   - State for ids not seen this frame is dropped at the frame's end (see Memory).
   
   Draws are therefore placed exactly in the frame they are recorded, and a custom widget's paint closure runs inside the hook, where it may borrow the scene's data. Only hit testing uses last frame's records.
4. **Consume.** What the UI took is removed from `Input` through the existing consume path before the fixed steps:
   - the pointer, while it is over a blocking UI node or a button is captured;
   - the keyboard, while a field is focused;
   - navigation actions, while a focus scope is shown (tab switching only when a `tabs` widget took it);
   - the wheel, when a scrollable took it.

**Timing:**
- Pointer hits use last frame's rects, which is what was on screen when the player clicked. A widget is hittable from its second frame, and the hit test is one frame behind a layout change.
- Keyboard and gamepad act in the same frame: focus and activation resolve from the focused id, whose rect is known.

### Scenes and layers

- Each scene's UI is a root id scope seeded by the scene's id, so two scenes never share ids. The root is not a layout node: each top-level subtree is laid out with the UI size as its available space and placed by its anchor and offset (`origin = anchor.fraction() * (ui_size - subtree_size) + offset`), with `Anchor::TopLeft` as the default.
- Only the top scene's UI receives input. Lower scenes record and draw, but resolve no pointer, keyboard, navigation or wheel input. This fixes the lower-scene key leak.
- Layers stay as in 4a: page 0, popup 1, overlay 2.
  - Tooltips, open dropdown lists, popups, modals and drag ghosts use the popup layer, drawn after everything their scene drew; a lower scene's popups stay on its page layer.
  - The engine's console and debug overlays stay above the popup layer, on the overlay layer.
- If the UI pass reruns in the same frame, because the top scene changed, the resolve step does not run again and the first pass's records are discarded. `UiMemory` begins one frame per engine frame, and results are delivered once per frame.
- When a scene exits (pop, switch, clear, reset), the engine forgets its memory.

## Identity and memory

### Ids

A `UiId(u64)` is a hash of:
- the enclosing scope's id;
- the call site (file, line and column) from `#[track_caller]` on the method that starts the widget, e.g. `ui.button(..)`;
- the number of times that call site has run in that scope this frame (the occurrence).

`.id(key)`, for any `K: Hash`, replaces the occurrence with the key. Items in a list that reorders, such as inventory slots, keep their state, focus and drag state.

The label is never part of the id. Renaming a button, or animating its text, keeps its focus and state.

**Scopes.** Every container is a scope: its id is the parent for its children's ids. Containers are column, row, grid, stack, panel, scroll, popup, modal and the scene root. `ui.scope(key, |ui| ..)` salts a game's own components.

**Helper functions.** A game's helper function (`fn menu_button(ui: &mut UiContext, ..)`) runs one call site many times, so its widgets are told apart by occurrence. That is stable as long as calls before it in the same scope are not conditional. The fix is to mark the helper `#[track_caller]` (the id then uses the helper's caller), or to use `.id()`/`ui.scope()`. The `ryo-ui` skill says so.

**Duplicates.** Two widgets with the same id in one frame can only come from a repeated explicit key.
- The second one gets a suffix, so state is never silently shared.
- A warning is issued once per id: `engine_warn!(module: "ui", "two widgets share the id key {..} in one scope; use distinct .id() keys")`.

### Memory

`UiMemory` is engine-owned and keyed by `UiId`. It holds, per id:

- **The record** from last frame: rect, clip (the intersection of its ancestors' clips), layer, order, and flags (focusable, blocking, scrollable, drop target, enabled).
- **Typed widget state** (`memory.state::<T>(id)`):
  - scroll offset, target and velocity;
  - the text field's caret, selection, horizontal scroll, preedit and blink timer;
  - the dropdown's open state and highlight;
  - the tooltip timer;
  - hover and press transition timers for animated looks;
  - drag state.
- **Per-scope focus memory:** the last focused child of each focus scope.

**Dropping:**
- A widget's record and typed state are dropped after one frame unseen.
- Focus memory lasts for the scene's lifetime and is cleared when the scene exits. A panel hidden and shown again, or a menu returning from a scene pushed over it, reopens on the button the player left. A pause scene popped and pushed again is a new scene, and starts at its default.

**Game data stays with the game.** Widgets take `&mut bool`, `&mut f32`, `&mut String` or `&mut usize`. `TextInputState`, `DropdownState`, `ScrollViewState`, `Focusable` and `FocusTrap` are removed. Their logic moves into the widgets, including the text-editing code and its tests.

**Control from code.** Every `Response` carries its `id`, and the UI offers:
- `ui.request_focus(id)`;
- `ui.scroll_to(id)`;
- `ui.is_focused(id)`;
- `ui.focused()`.

The console command `debug:ui` (after the console's other `debug:` commands) outlines every widget rect with its id, and marks the hot, active and focused widgets.

## Layout

**Engine.** Taffy does flexbox and grid. Use the current release at planning time; a spike checks it at the start of the plan. The style types are ours (`UiStyle` and its builders), converted to Taffy's at the boundary, so 4c shares the vocabulary and Taffy stays swappable.

**Containers.** Each is a builder that ends in `.show(|ui| ..)` and returns a `Response` for the container:

| Container | Layout |
|---|---|
| `ui.column()` / `ui.row()` | flex, column or row |
| `ui.grid(n)` | CSS grid with `n` equal columns; `.tracks(..)` for explicit tracks |
| `ui.stack()` | children overlap, placed by anchor (a count badge on an icon) |
| `ui.panel()` | a column with the theme's panel look (fill or skin) and padding; blocks the pointer |
| `ui.scroll()` | `.vertical()` (default), `.horizontal()`, `.both()`; a column inside; clipped by a scissor |
| `ui.popup(anchor_id)` / `ui.modal()` | out of flow, on the popup layer; a popup sits below its anchor and traps focus; a modal dims and blocks everything below and traps focus |

**The scene root** places each top-level container by its anchor (default top-left), so `ui.column().anchor(Anchor::Center).show(..)` centres a menu. A HUD can use `.anchor(Anchor::TopRight).offset(-16.0, 16.0)`.

**Style builders** apply to containers and widgets:

| Property | Builders |
|---|---|
| Size | `width`/`height` as `px(f32)`, `pct(f32)`, `fill()` or `fit()` (the default) |
| Min and max | `min_width`, `max_width`, `min_height`, `max_height` |
| Spacing | `padding(..)`, `margin(..)`, `gap(..)` |
| Alignment | `align(Align)` (cross axis), `justify(Justify)` (main axis) |
| Flex | `grow(f32)`, `wrap_items()`, `aspect(f32)` |
| Placement (stacks and root) | `anchor(Anchor)`, `offset(..)` |

All sizes are in UI units (4a's `UiSpace`).

**Absolute placement.** `.at(rect)` takes a widget or container out of layout and places it at a fixed rect. It is for HUD elements and existing code. The rect is known immediately.

**Measuring:**
- **Text leaves** measure through `TextEngine::layout` with the width Taffy offers, so wrapped text fills its column. The `Arc<TextLayout>` measured is the one drawn, so there is no second layout. That fixes 4a's double layout in `draw_text_anchored` and `label`.
- **Images** measure from their sprite size.
- **Widgets** have intrinsic sizes from the theme: padding around their content, minimum heights, slider and toggle sizes.

**Caching.** The Taffy tree is kept between frames, with each node keyed by `UiId`.
- Unchanged nodes are reused; nodes whose style or content changed are marked dirty.
- Nodes not seen this frame are removed.

**Performance targets:**
- A 200-widget settings screen lays out and resolves in under 0.25 ms (release build).
- The HUD frame bench (about 290 µs at fed18d5) does not regress.

**Pixel snapping.** Resolved rects snap to whole render pixels: the origin rounds, and the size rounds through the far edge. Borders stay crisp at fractional scale factors such as 1.25× and 1.5×, which is 4a's follow-up. Text origins already snap.

**Removed:** `HBox`, `VBox`, `Stack`, `HAlign`/`VAlign` and the old rect-taking widget signatures. `Align` and `Justify` replace the alignment enums. `ui.screen_size()` stays.

## Interaction and input routing

**Hit testing.** It uses last frame's records. A point hits a record when it is inside the rect and inside the record's clip. Among hits, the topmost wins: the highest layer, then the latest order.
- Records flagged blocking stop the search for widgets underneath: panels, modals, popups and any container with `.block_pointer()`.
- Plain rows, columns and stacks pass the pointer through.
- Disabled widgets are never hot.

**States.** A `Response` reports these:

| State | Meaning |
|---|---|
| `hovered()` | hot: topmost under the pointer, not blocked |
| `pressed()` | active: the primary press landed on it, and it holds the button until release (the existing engine capture keeps that button from gameplay) |
| `clicked()` | primary release over the same widget it was pressed on; dragging off before release cancels |
| `secondary_clicked()` | the same for the secondary button |
| `double_clicked()` | two clicks within 0.3 s on the same widget |
| `drag_started()`, `dragging()`, `drag_released()` | while active, the pointer moved more than 4 UI units; `drag_delta()` gives the movement |
| `changed()` | the widget changed the caller's value this frame |
| `focused()`, `gained_focus()`, `lost_focus()` | keyboard/pad focus |
| `submitted()` | Enter in a text field |

**Activation.** Gamepad and keyboard Confirm on the focused widget activates on press, as games expect. The widget reports `clicked()`.

**The wheel:**
- **Routing.** It goes to the innermost scrollable under the pointer that can still scroll in the wheel's direction. When that one is at its edge, the wheel bubbles to the next scrollable ancestor.
- **Direction.** The horizontal wheel, or Shift with the vertical wheel, drives horizontal scrolling.
- **Speed.** Distance per wheel line and the smoothing come from the theme. The defaults are 40 UI units per line and a short ease.
- **Gamepad.** The right stick scrolls the scrollable that contains the focused widget. Moving focus to a widget outside the visible area scrolls it into view.
- **Dropped.** Content-drag scrolling with momentum is removed: it fought sliders and drag-and-drop inside lists. The scrollbar thumb still drags.

**The wheel in fixed steps.** The wheel gets the one-shot treatment that key presses have (`Input::set_fixed_step`/`end_fixed_steps`). Inside the fixed steps, where `Scene::update` and game systems run, `scroll_delta()` returns the frame's wheel in the first fixed step and zero in later ones. A frame that runs no fixed step carries its wheel to the next frame's first step. The exception is a pause: a press nobody stepped over is dropped there, and so is the wheel. Outside the fixed steps (the UI pass, the console), `scroll_delta()` is the frame's value as now.

**Text.** The focused text field receives typed text, IME preedit and commit, and editing keys. It consumes the keyboard, except:
- Escape, which leaves the field;
- Tab and Shift+Tab, which move focus.

**Disabled.** `.enabled(false)` means:
- the widget is never hot, active or focused;
- navigation skips it;
- it draws with the disabled look.

**Tooltips.** `.tooltip(text)` takes text or `RichText`. The tooltip appears on hover after the theme's delay (0.5 s by default), or on focus when the last device is the keyboard or gamepad. It sits on the popup layer below the widget, flips above the widget or moves sideways to stay on screen, and hides on press.

**Game input.**
- `ui.wants_pointer()` is true while the pointer is over a blocking UI node or captured.
- `ui.wants_keyboard()` is true while a text field is focused or a list or popup is open (a trapping scope that is not a modal). A modal does not make it true: the game decides when its modal closes, typically on `ui.back()`.
- Gameplay never sees what the UI took (see step 4).

## Focus and navigation

**Focus.** The top scene's UI has at most one focused widget.

**Focusable widgets:**
- buttons, checkboxes, toggles and sliders;
- dropdowns and text fields;
- radio items, tabs, choice items and slots;
- rebind rows;
- any custom widget with `.focusable(true)`.

**Scopes.** Keyboard and gamepad navigation, Tab and Confirm work only inside focus scopes: containers marked `.focus_scope()`, popups (dropdown lists, `ui.popup`) and modals (including `ui.confirm`). Panels are not scopes unless marked, and the scene root is never one. So a HUD panel with a button in a gameplay scene is mouse-only and never takes the arrows, Space or A from gameplay; a menu marks its container `.focus_scope()`. Text fields outside a scope still take focus by click.
- Each scope remembers its last focused child, for the scene's lifetime.
- Entering a scope restores that child. Otherwise focus goes to the child marked `.default_focus()`, otherwise to the first focusable child in order.
- `.trap()` keeps navigation inside the scope; modals and popups trap. A trapping scope with nothing focusable keeps focus empty: the page under it is never a target.
- `.wrap_focus()` makes navigation wrap at the scope's edges; vertical menus usually want it.

**Default focus.** When a scope first appears and nothing is focused, focus goes to its default; when a modal or popup opens, focus moves into it. When it closes, focus returns to where it was. Godot, Unity and Bevy all require code for this; here it is automatic, and `ui.request_focus(id)` overrides it.

**Spatial navigation.** For a direction `d`, a candidate qualifies when its centre lies beyond the focused rect's centre along `d`. Each qualifying candidate is scored:
- `p` is the gap along `d` between the focused rect's leading edge and the candidate's near edge, or 0 if they overlap.
- `s` is the gap between the two rects on the perpendicular axis, or 0 if their ranges overlap.
- The score is `p + 2·s + 0.1·(perpendicular centre distance)`.
- Candidates with `s == 0` (aligned) beat all non-aligned candidates. A non-aligned candidate is chosen only if no aligned one exists.

The search covers the current scope first. If nothing qualifies, it steps out to the parent scope, unless the scope traps or wraps.

Candidates are the visible focusables: never disabled, and never clipped out. A widget inside a scroll area counts as visible when its scroll area is, so the pad can walk a long list, and moving focus to it scrolls it into view.

**Overrides:**
- `.nav_up(id)`, `.nav_down(id)`, `.nav_left(id)` and `.nav_right(id)` set explicit neighbours.
- `.nav_skip()` keeps a focusable widget out of directional navigation.
- Tab (`ui.next`) follows submission order, and goes backwards while Shift is held.

**Widgets that take directions:**
- A focused slider takes Left and Right to step its value.
- An open dropdown list traps focus, so Up and Down move between its options.
- A focused text field takes Left and Right as caret movement.
- A custom widget can take either axis (`.takes_horizontal()`, `.takes_vertical()`) and read `ui.nav_step(id)`.
- A rebind row that is capturing takes everything, so arrows and Confirm can be bound.

Directions a widget doesn't take go to navigation.

**Back.** `ui.back()` is true when the Back action was pressed this frame and no focused widget used it (closing a dropdown, leaving a text field, cancelling a drag). A menu writes `if ui.back() { return SceneAction::Pop; }`.

**Held directions** repeat after 0.35 s, then every 0.1 s. A repeat is armed only by a press, so a direction still held when the top scene changes does nothing until pressed again.

**The device and the focus ring.** The engine tracks the last-used device (`InputDevice::{Mouse, Keyboard, Gamepad}`, `ctx.input.last_device()`):
- pointer motion, clicks and the wheel mean the mouse;
- keys mean the keyboard;
- pad buttons, and sticks past their deadzone, mean the gamepad.

Moving the pointer onto a focusable widget moves focus to it, so there is one highlight, and switching to the pad continues from where the mouse was. The focused look and the focus ring (theme) show only when the last device is the keyboard or the gamepad (Xbox Accessibility Guideline 113: clear, visible focus).

## Actions, prompts and rebinding

**Actions** (`src/core/input/actions.rs`). An `Actions` resource maps named actions to bindings, read through `Input`:

```rust
pub enum Binding {
  Key(KeyCode),
  Mouse(MouseButton),
  Pad(GamepadButton),
  Stick(StickDirection),
}

ctx.actions().bind("jump", [Binding::Key(KeyCode::Space), Binding::Pad(GamepadButton::South)]);
ctx.input.action_pressed("jump");
ctx.input.action_held("jump");
ctx.input.action_released("jump");
```

- `StickDirection` is a left- or right-stick direction, pressed past half deflection and released below 40%.
- Actions follow the same fixed-step one-shot rule as keys.
- Bindings save in the player settings (`SystemState`, `system.json`, under `bindings`). A game that enables `UserPreference::Bindings` (`EngineBuilder::with_user_preferences(..)`) has them loaded at startup and saved whenever they change; otherwise they last for the session.
- `Actions::reset_defaults()` restores what the game bound in code.
- Analog axes stay on the existing raw API.
- It is still the first gamepad only.

**UI actions.** The engine binds these, and games can rebind them:

| Action | Default bindings |
|---|---|
| `ui.up` / `ui.down` / `ui.left` / `ui.right` | arrow keys, D-pad, left stick |
| `ui.confirm` | Enter, Space, pad South |
| `ui.back` | Escape, pad East |
| `ui.next` | Tab (backwards while Shift is held: bindings carry no modifiers) |
| `ui.tab_next` / `ui.tab_prev` | E / Q, pad RightBumper / LeftBumper (demo-kit reserves PageUp/PageDown) |

Held directions repeat: the first repeat after 0.35 s, then every 0.1 s, set in the theme. The stick repeats the same way.

**Scripted and headless pad input.** gilrs gamepad ids cannot be constructed, so scripted and headless pad input uses virtual events (`InputEvent::PadButton`, `InputEvent::PadStick`) applied to the tracked pad, or to a virtual pad when none is connected.

**Prompts:**
- `ui.prompt("ui.confirm")`, and the markup tag `[prompt=jump]` in any `RichText`, draw the binding of an action for the last-used device.
- **Built-in look.** Key-cap chips: a rounded rect with the key's or button's label in the theme's caption style.
- **Pad labels by family.** The family comes from the gamepad name gilrs reports:
  - Xbox labels by default;
  - names containing "DualSense", "DualShock", "PlayStation", "Sony", "PS4" or "PS5" get Cross/Circle/Square/Triangle (a bare "PS" matched unrelated names);
  - names containing "Nintendo", "Switch", "Pro Controller" or "Joy-Con" get Nintendo labels, with South labelled B.
- **Glyph sprites.** A game registers glyph sprites per binding in `TextIcons`, named with the `KeyCode` or `GamepadButton` variant (for example `prompt.pad.xbox.South` or `prompt.key.Enter`), and these replace the chips.
- **In markup,** `[prompt=jump]` is resolved before layout against the actions, the last-used device and `TextIcons`: a registered glyph becomes an inline icon, otherwise a short label such as `[A]` in the accent colour. Chips are drawn by the `ui.prompt(action)` widget.

**Rebinding.** `ui.rebind("jump")` is a settings row: the action's display name and its current bindings as prompts.
- Confirm or click starts capture ("Press a key or button…"). The next key or pad button binds it, East included, so Back is not a cancel. Escape cancels, and capture times out after 5 s. While capturing, navigation stands aside, so arrows and Confirm can be bound.
- If the new binding already belongs to another action in the same group (`ui.*` or the game's, from the action's name), the two are swapped and the row shows a short note.
- Changes save immediately (when saved bindings are enabled).
- `.slot(n)` chooses which of several bindings is edited.

## Widgets

Every widget is a builder that returns a `Response` and records itself on drop when `.show()` is not called. Every widget builder has:
- `.id(key)`, `.enabled(bool)`, `.tooltip(..)`;
- `.variant(name)`, `.style(LookPatch)` (only the fields it names);
- `.default_focus()`, `.nav_*()`;
- the size and placement builders, including `.at(rect)`;
- `.drag_source(payload)` and `.drop_target::<T>()`.

| Widget | Signature | Notes |
|---|---|---|
| Text | `ui.text(text)` | 4a's builder, now a layout leaf; `.at()` keeps absolute placement; `.typewriter(&mut TextReveal)` as in 4a |
| Image | `ui.image(&sprite)` | `.tint`, `.size`, `.uv`, `.filter(Filter)`; sprites carry no filter in this engine, so the filter is chosen here (default `Filter::Default`), which is 4a's pixel-art follow-up |
| Button | `ui.button(label)` | `.icon(&sprite)`; text, icon or both |
| Checkbox | `ui.checkbox(&mut bool, label)` | |
| Toggle | `ui.toggle(&mut bool, label)` | |
| Slider | `ui.slider(&mut f32, range)` | `.step(f32)`, `.show_value()`, `.label(..)`; Left/Right step when focused |
| Text field | `ui.text_input(&mut String)` | `.placeholder`, `.max_len`, `.password`; single-line |
| Dropdown | `ui.dropdown(&mut usize, &[&str])` | |
| Radio | `ui.radio(&mut T, value, label)` | `T: PartialEq + Copy` |
| Tabs | `ui.tabs(&mut usize, &[&str])` | the bumpers and Q/E switch the first tabs drawn in the top scene; with no tabs on screen those keys stay the game's |
| Choice list | `ui.choice_list(&[&str]).show() -> Option<usize>` | dialogue choices; number keys 1-9 pick directly |
| Progress | `ui.progress(fraction)` | `.label(..)`; not focusable |
| Separator, spacer | `ui.separator()`, `ui.spacer()` | |
| Slot | `ui.slot()` | `.icon(&sprite)`, `.count(n)`, `.selected(bool)`, `.filter(Filter)`; inventory cell, empty without an icon |
| Prompt | `ui.prompt(action)` | see Actions |
| Rebind row | `ui.rebind(action)` | see Actions |
| Modal | `ui.modal().show(..)` | dims, blocks, traps focus |
| Confirm dialog | `ui.confirm(title, message) -> Option<bool>` | a modal with Yes/No; Back means No |
| Custom | `ui.custom(width, height)` | `.focusable(true)`, `.sense(Sense)`, `.takes_horizontal()`/`.takes_vertical()`, `.paint(\|painter, response\| ..)`; for game-made widgets, drawn with a `Painter` in the widget's own units |

The `draw_text*`, `draw_rect*`, `draw_sprite*`, `draw_nine_slice` and `draw_line` calls stay. They are absolute drawing that does not take part in layout or interaction, for HUDs.

**Drag and drop:**
- **Sources and targets.** Any widget or container can be a drag source with `.drag_source(payload)`, for any `T: Clone + 'static`. A drop target reads the drop with `if let Some(item) = resp.dropped::<ItemId>() { .. }`. `resp.drag_hover::<T>()` tells a target that a compatible payload is over it, for highlighting.
- **The ghost.** While dragging, a ghost follows the pointer on the popup layer, above the popups, at 80% opacity: the `.drag_ghost(&sprite)` sprite, else the slot's icon, else a box in the accent colour.
- **Gamepad:**
  1. Confirm on a focused drag source picks the payload up, and the ghost rides the focus.
  2. The player navigates.
  3. Confirm on a focused drop target that accepts the type drops it.
  4. Back cancels.
- **What a drop means** is the game's call. The UI delivers the payload and the target, and nothing moves by itself.

## Theme, skins and sounds

**The resource.** `UiTheme` is a world resource holding an `Arc<Theme>`. Every widget reads it; the UI context takes the `Arc` at the start of each pass, so a change applies from the next frame. Set it with `EngineBuilder::ui_theme(Theme)` or `ctx.ui_theme()` (`get`, `set`, `modify`).

```text
Theme
  palette:   background, surface, text, muted, accent, danger, success, warning
  metrics:   radius, spacing, border, focus_ring (width, offset, colour), scroll (line, smoothing),
             nav (repeat_delay, repeat_rate), tooltip_delay, drag_threshold
  text:      TextLook per role (size, colour, font, bold): body, heading, button, caption, mono
  widgets:   per WidgetKind: Looks { normal, hovered, pressed, focused, disabled } plus kind metrics
             (slider track/handle, toggle size, checkbox box, scrollbar width, slot size)
  variants:  name -> { base: WidgetKind, overrides: Looks }
  skins:     name -> { sprite path, insets [l, t, r, b], filter }
  sounds:    focus, confirm, back, tick, error: optional sound paths; volume
```

**`Look`:**
- fill, border colour and width, radius;
- text colour;
- padding;
- pressed offset;
- `skin: Option<String>`, a nine-slice used in place of fill and border.

Every state's look is a `LookPatch` over `normal`, applied field by field, so a theme writes only the differences. A variant patches its base kind's looks. Looks switch per state without transition timers. Skins name a sprite path and their sampling filter.

**Built-in themes:**
- `Theme::dark()` is the default and matches today's look.
- `Theme::light()`.
- `Theme::pixel()`: m5x7 text, square corners, nearest-filtered skins drawn in code, for pixel-art games.

**Files.** Themes load from RON (`Theme::load(path)`; `ron` is already a dependency). The skills already describe RON files, and today's code writes JSON, which is replaced. `Theme::load` remembers the path, and in debug builds the existing hot reload re-applies the file when it changes.

**Sounds.** At the end of its pass, the UI plays at most one sound per kind per frame through the engine's audio system, at the theme volume:
- focus moves;
- confirm activations;
- back;
- slider ticks (each step, or at most every 0.05 s);
- errors (a refused drop, a cancelled rebind).

No sound plays when none are named.

## Public API changes

**Added:**
- **Core types:** `Response`, `UiId`, `Sense`, `Align`, `Justify`, `UiStyle` builders, `InputDevice`.
- **Actions:** `Actions`, `Binding`, `StickDirection`; `Input::{action_pressed, action_held, action_released, last_device}`; `SceneContext::{actions, ui_theme}`.
- **Theme:** the new `Theme` structure with `Look`, `Skin` and `UiSounds`; `EngineBuilder::ui_theme`.
- **Widgets and containers:** every widget and container in the tables above.
- **`UiContext` methods:** `scope`, `request_focus`, `scroll_to`, `is_focused`, `focused`, `focus_visible`, `nav_step`, `back`, `wants_pointer`, `wants_keyboard`, `prompt`.
- **Testing:** `ryo2d::testing::UiHarness` (UI frames over scripted input, no GPU) and `HeadlessEngine::{ui_focused, ui_focusables, focus_ui}` and pad helpers.

**Changed:**
- Every interactive widget drops its rect argument (use layout or `.at(rect)`) and returns `Response` instead of `bool`.
- `text_input` takes `&mut String`; `dropdown` takes `&mut usize`.
- `ui.text(..)` takes part in layout unless `.at()` is used.

**Removed:**
- **State and focus types:** `TextInputState`, `DropdownState`, `ScrollViewState`, `Focusable`, `FocusTrap`.
- **Layout helpers:** `HBox`, `VBox`, `Stack`, `HAlign`, `VAlign`.
- **Old widget calls:** `button_styled` (use variants), `scroll_view`/`scroll_view_horizontal` (use `ui.scroll()`).
- **Old focus and theme calls:** `handle_tab_navigation`, `handle_dropdown_tab_navigation`, `draw_focus_ring*`, `draw_themed_*`, `UiContext::with_theme`.
- **Old hit and occlusion calls:**
  - `is_clicked`/`is_hovered`/`is_held` (use `ui.custom(..)`);
  - `set_mouse_occluded`/`is_mouse_occluded` (layering is automatic; a scene that needs to block uses a modal);
  - `consume_click`/`click_consumed`/`escape_consumed`/`consume_escape` (use `ui.back()` and the blocking flags).
- **Theme types:** the old flat `ButtonStyle`/`PanelStyle`/… types.

## Migration (in place, every step green)

1. **Input groundwork:**
   - the one-shot wheel in fixed steps;
   - device tracking;
   - the action map with save and load;
   - `RYO_INPUT` gains wheel steps (`N:Wheel@DXxDY`, in lines), pad buttons (`N:PadSouth`, `A-B:PadDPadDown`: `Pad` plus a `GamepadButton` variant) and held stick positions (`A-B:LeftStick@XxY`, `A-B:RightStick@XxY`, back to centre at `B`).
2. **The new core beside the old widgets:**
   - ids, memory, the tree;
   - the resolve and lay-out steps;
   - Taffy behind `layout`.
   
   Old rect-based widgets keep working while the new builders land.
3. **Navigation, theme, widgets.** Focus and navigation. The new theme resource reaches every widget. Then the widgets, family by family:
   - buttons, toggles, checkboxes, sliders;
   - text input;
   - dropdown, radio, tabs and choice list;
   - scroll;
   - tooltips;
   - modal and confirm;
   - slots and drag-and-drop;
   - prompts and rebinding;
   - sounds.
4. **Port, then delete.** Each widget-family step replaces the old widget of that name and ports its call sites mechanically (`ui.button("X", rect)` becomes `ui.button("X").at(rect).clicked()`), so every step builds and passes. The games, demo-kit and the `ui` demo are then restructured onto layout and navigation (below), and the remaining old API is deleted.
5. **Docs and benches.**

## Games and demos

- **Games:** their menus move onto the widgets and the actions, and the hand-rolled gamepad menu code in `update` is deleted.
  - **breakout:** menu, pause (three volume sliders), quit confirm.
  - **bullet-hell:** pause (`button_styled` becomes variants), menu, game over.
  - **flappy:** menu and game over.
  - **leap-king:** the menu (text fields), and the leave-confirm dialog in game.
- **Demos:**
  - **networking:** the menu.
  - **asset-loading:** its buttons.
  - **ui-animations:** `is_hovered` becomes responses.
  - **text:** its scroll view.
- **demo-kit:**
  - The settings panel becomes a focus scope holding one focusable widget that takes all four directions (so its own row window keeps working): the same keys, plus the D-pad and stick, and a click selects a row.
  - Pages with their own focus scope keep the arrows, and reach the panel with Tab or a click.
  - Help becomes a modal.
  - The reserved-key rules and `tests/shell.rs` still hold.
- **The `ui` demo is the acceptance showcase,** with four new pages:
  - **Menu:** a title menu, then a pause flow, then a confirm dialog.
  - **Settings:** tabs (Video, Audio, Controls), scroll, sliders, toggles, dropdowns, and rebind rows with prompts.
  - **Inventory:** a 6×4 grid of slots, drag and drop with the mouse and the pad, tooltips, and an equipment row.
  - **Dialogue:** a typewriter line in a box, a choice list, and a HUD with a health bar and counters that size to their text.
  
  The acceptance pages declare no settings rows, so every widget on them is reachable by the D-pad alone. The existing pages are updated: Layout and Clipping are rebuilt on containers, scroll areas and modals.
- **Captures.** The acceptance pages are captured with scripted pad input (`RYO_INPUT`). The demo tour itself stays input-free.

## Testing

- **Unit tests:**
  - **Ids:** call-site stability across frames; occurrence counting; `.id(key)` surviving a reorder; duplicate-key suffixing and its warning.
  - **Memory:** dropping after one frame unseen; focus memory that lasts the scene's lifetime.
  - **Hit testing:** layers, clips and blocking; disabled widgets never hot.
  - **Pointer states:** click on release, cancel by dragging off, the drag threshold, double click.
  - **The wheel:** nested routing, including bubbling at an edge; the fixed-step one-shot wheel (2 steps count once; 0 steps carry over).
  - **Spatial navigation:** a list, a 6×4 grid, side-by-side buttons, a diagonal layout (aligned beats nearer-diagonal), scope stepping, traps, wrap, overrides.
  - **Focus:** default focus on appear; a modal returning focus; device tracking; ring visibility.
  - **Actions:** bind, rebind with a conflict swap, save and load round trip, the fixed-step one-shot rule.
  - **Theme:** state inheritance; variants; RON round trip; hot-reload re-apply.
  - **Text editing:** the existing text-editing tests, kept on the new field.
- **A headless UI driver** (`ui::test_util`). It runs a scene's `ui` closure for N frames with scripted pointer, keyboard, pad and wheel input. It asserts responses, focus and values, with no GPU. A public subset, `ryo2d::testing::UiHarness`, lets games test their UI the same way and carries the benchmarks.
- **Pinned without a GPU** through the driver: a grid that sizes to text, crisp rects at 1.25×, the focus ring shown for the pad and hidden for the mouse, a skinned panel, a modal blocking a button under it, a scroll view clipping hits.
- **e2e (GPU) tests:**
  - a centred panel;
  - the console blocking the pointer;
  - a lower scene's button ignoring the pointer;
  - the `debug:ui` overlay;
  - reading and setting UI focus headlessly.
- **Headless engine tests:**
  - an existing game's pause menu navigated by pad from start to quit;
  - every widget on each acceptance page reachable by the D-pad;
  - demo-kit `shell.rs` still green.
- **Benches:** a 200-widget settings screen (layout plus resolve) under 0.25 ms; a 1,000-row list; the HUD frame not regressed.

## Documentation

- **`ryo-ui` is rewritten:**
  - ids, including the helper-function caveat;
  - layout containers and styles;
  - responses;
  - focus, scopes and default focus;
  - `ui.back()`;
  - theme and skins;
  - drag and drop;
  - prompts and rebinding;
  - recipes for the four acceptance UIs.
- **`ryo-api`** gets the new surface.
- **`ryo-game-dev`** gets actions.
- **`ryo-text`** gets the `[prompt=..]` markup tag.
- **`ryo-config-save`** gets saved bindings.
- **Every skill copy** is kept byte-identical with its `src/cli/skills/` twin.
- **AGENTS.md:** `src/core/ui/AGENTS.md` and the new module AGENTS files, and `src/core/input/AGENTS.md` (actions, devices, the wheel rule).
- **The architecture overview:** the "Text and UI" section gains interaction.

## Design decisions

- **The frame tree is resolved after the hook (approach A).** It removes the order dependence behind most of today's input bugs: topmost wins, clips and modals block, nested scrolls route, and lower scenes stay quiet. The cost is a one-frame pointer lag after layout changes, the same lag egui and Clay ship with. Approach B (eager placement with cached sizes) kept order-dependent arbitration and needed a second layout system for 4c. Approach C (a retained-first reconciler) pulled 4c into 4b.
- **Ids are automatic by call site.** There are no ids for agents or humans to write, labels can change freely, and only lists that reorder need keys. Label-hashed ids (egui) clash on repeated labels and reset on renames. Explicit ids everywhere are boilerplate.
- **Taffy, behind our own style types.** It gives full flexbox and grid with size-to-content through a measure callback, shared with 4c, and is swappable if needed.
- **Clicks fire on release; Confirm fires on press.** Desktop conventions for the mouse, game conventions for the pad.
- **Spatial navigation by default, with automatic default focus.** Menus, settings and grids work with a pad without extra code. Overrides exist where geometry guesses wrong.
- **Focus follows the mouse; the ring shows only for the keyboard and pad.** There is one highlight, and switching devices is seamless. It follows XAG 113.
- **Content-drag scrolling is dropped.** It conflicted with sliders and drag-and-drop inside lists. Touch input is out of scope.
- **The action map is minimal:** digital actions, rebinding and persistence. Analog axes, multiple gamepads and touch wait for a later input sub-project.
- **RON themes.** It is already a dependency, it is readable, and the skills already promise it.

## Risks

- **Migration size.** Roughly 100 widget call sites across 5 games, about 10 demos and demo-kit change shape. The plan ports family by family, with the old API alive until the last port.
- **`#[track_caller]` through builders.** The location must be captured where the widget starts (`ui.button(..)`), not in `.show()` or the builder's terminal method. A test pins that two buttons on two lines get two ids, and that one helper called twice gets two ids by occurrence.
- **Taffy cost and size.** The bench target (0.25 ms for 200 widgets) and the binary size are checked in the plan's first spike. Taffy is pure Rust and builds for wasm.
- **Lag artifacts.** A widget that moves every frame (an animated menu) is hit-tested at last frame's position. That is acceptable, since it is what was drawn. Animated widgets should animate their look, not their layout, and the skill says so.
- **demo-kit's shell** has reserved keys and its own input rules. Porting its settings panel must keep `tests/shell.rs` green.
- **Gamepad family detection by name** is heuristic. A wrong family shows the Xbox labels.

## Out of scope

- Retained UI nodes (4c).
- Touch input.
- Analog action axes; more than one gamepad.
- Accessibility screen readers (AccessKit).
- Right-to-left layout.
- Rich text editing (bold or colour inside a text field).
- Windows that the user drags or resizes.
- Networking.
