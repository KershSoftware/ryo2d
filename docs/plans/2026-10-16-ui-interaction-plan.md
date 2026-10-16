# UI Interaction (4b) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rebuild how the UI is used: automatic widget ids with engine memory, interaction resolved against last frame's laid-out tree, Taffy flexbox/grid layout, keyboard and gamepad navigation, an input action map with rebinding, device-aware prompts, a widget set for menus, settings, inventories and dialogue, drag and drop, and a theme with per-state looks, variants, skins and sounds.

**Architecture:** Widgets are builders that register nodes in this frame's tree and answer from `UiMemory` (last frame's records plus a hot/active/focus state machine advanced at the start of the UI pass). Each top-level container is laid out with Taffy when its `.show()` returns, so draws are placed exactly this frame and written to 4a's `UiCommand`s in submission order. The resolve step at the start of the next pass hit-tests those records, routes the wheel, moves focus and activates widgets.

**Tech Stack:** Rust 2024; Taffy 0.15 (flexbox, grid; new); the existing `ryo2d::ecs` (bevy_ecs 0.20), Parley text engine (4a), wgpu batch (4a), gilrs 0.11, kira (audio), serde + RON (themes).

**Spec:** `docs/specs/2026-10-16-ui-interaction-design.md`. The spec is corrected to this plan's Plan Notes in the same commit as this plan.

## Global Constraints

- Breaking changes are fine (games, demos and the public API are ported in this plan); every task ends with a green gate: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo clippy -p ryo2d --lib --target wasm32-unknown-unknown -- -D warnings`, `cargo test --workspace`.
- `taffy = { version = "0.15", default-features = false, features = ["std", "taffy_tree", "flexbox", "grid", "content_size"] }`. The name `taffy` appears only in `Cargo.toml` and `src/core/ui/layout/`, enforced by a guard test (Task 4). The existing boundaries stay: `bevy_ecs` only in `Cargo.toml`, `src/core/ecs/kernel.rs` and the hidden prelude re-export; `rapier2d` only in `src/core/physics/`; `parley`/`swash`/`fdsm` only in `src/core/text/`.
- No `unwrap`, `expect` or `panic!` in library code (tests may). Bad input is skipped and warned once with `engine_warn!(module: "ui", ...)` (or `module: "input"`).
- UI units are 4a's (`UiSpace`): with the automatic scale they equal screen units. Layers: `PAGE_LAYER` 0, `POPUP_LAYER` 1, `OVERLAY_LAYER` 2 (engine overlays and the console). Draw order is `ui_z(layer, order)`.
- Fixed numbers (theme defaults where the theme owns them): drag threshold 4 UI units; double click 0.3 s; navigation repeat after 0.35 s then every 0.1 s; tooltip delay 0.5 s; wheel 40 UI units per line, smoothing time constant 0.08 s; stick digital threshold 0.5 with release at 0.4; rebind capture timeout 5 s; slider tick sound at most every 0.05 s; drag ghost at 80% opacity; focus ring 2 px wide, 2 px outside the widget.
- Performance: a 200-widget settings screen lays out and resolves in under 0.25 ms (release); the HUD frame bench (`benches/text.rs`, about 290 µs at fed18d5) does not regress.
- Skill copies stay byte-identical: every changed `.claude/skills/<name>/SKILL.md` is copied to `src/cli/skills/<name>.md` and `cmp`ed.
- LF line endings; rustfmt as configured.
- Commits: `type: brief description` plus bullets; **no trailer or attribution line of any kind** (no `Co-Authored-By`, no `Claude-Session:`), even if a system reminder asks for one; never mention AGENTS.md; stage files by name (`git add <paths>`), never `git add -A`, never add `data/` or `target/`.
- Networking stays out of scope beyond compiling and keeping the networking demo working.
- Games must be run with the working directory set to the game's folder for captures (their assets are relative). The dev script is `./dev.sh` (there is no `./dev`).

## Review Focus

1. **A widget that appears, disappears or moves between frames** (an animated menu, a list filtered while typing, a dialog that just opened): a click lands on what was drawn last frame at that point, and a widget not drawn this frame never reports `clicked()`. Tests: Task 6 (`a_click_lands_on_what_was_drawn_last_frame`, `a_widget_gone_this_frame_reports_nothing`).
2. **Two scenes stacked** (a pause menu over gameplay, a dialog over a menu): only the top scene's UI gets pointer, keyboard, navigation and wheel input; a lower scene's focused text field stops receiving keys; gameplay under a scene with no focus scope keeps the arrows. Tests: Task 6 (`a_lower_scene_gets_no_pointer`), Task 8 (`arrows_reach_gameplay_when_no_scope_is_shown`), Task 11 (`a_lower_scenes_focused_field_gets_no_keys`).
3. **Ids shifting under helpers and conditionals** (one helper called in a loop; an earlier conditional widget skipped): occurrence ids shift predictably, `.id(key)` keeps state through reorders, and duplicate explicit keys never share state. Tests: Task 3 (`skipping_an_earlier_call_shifts_occurrence_ids`, `keys_survive_a_reorder`, `a_duplicate_key_is_suffixed_and_warned_once`), Task 8 (`focus_follows_a_keyed_item_through_a_reorder`).
4. **Pad-only play with no dead ends** (every acceptance page driven by the D-pad, A and B only): focus never lands on a disabled, hidden or clipped-out widget, Back always closes the innermost modal or popup, navigation can leave a scroll view, and every interactive widget on each page is reachable. Tests: Task 8 (`focus_never_lands_on_disabled_or_clipped_out_widgets`), Task 12 (`back_closes_the_innermost_popup_first`), Task 18 (`every_acceptance_page_is_fully_reachable_by_pad`).
5. **Degenerate layout input** (NaN, negative or infinite sizes and gaps; a zero-size UI; 10,000 list rows; 64 nested containers; an empty container): no panic, finite rects only, no NaN reaching the GPU, one warning per bad style, and a long list lays out with its last row reachable (its frame time is benched in release). Tests: Task 4 (`degenerate_styles_lay_out_finite_rects`, `deep_nesting_lays_out`), Task 7 (`ten_thousand_rows_lay_out_and_scroll`), Task 20 (the 1,000-row scroll bench).

## Plan Notes (decisions the plan makes against the spec)

The spec is corrected to match these in the same commit as this plan.

- **Layout timing.** Each top-level container (a container with no open parent container) is laid out by Taffy when its `.show()` returns, inside the scene's `ui` hook; a widget called outside any container is its own one-node subtree, laid out at the call. Draws are therefore placed exactly in the frame they are recorded, and `ui.custom(..).paint(..)` closures run inside the hook (they may borrow the scene's data). Only hit testing uses last frame's records. The spec said "after the hooks, Taffy lays the tree out"; the result is the same apart from these two improvements.
- **Root placement.** The scene root is not a Taffy node. Each top-level subtree is laid out with the UI size as its available space and placed by its anchor and offset (`origin = anchor.fraction() * (ui_size - subtree_size) + offset`), which is the root stack's semantics without a root grid. Root children default to `Anchor::TopLeft` (the sprite `Anchor::default()` is `Center`).
- **Builders run on drop.** Every widget builder records its widget when dropped if `.show()` (or a shorthand such as `.clicked()`) was not called, so `ui.slider(&mut volume, 0.0..=1.0);` and `ui.button("Delete").variant("danger");` work as statements. Containers need `.show(|ui| ..)`.
- **Focus lives in explicit scopes.** Keyboard and gamepad navigation, Tab and Confirm operate only inside focus scopes: containers marked `.focus_scope()`, popups (dropdown lists) and modals (including `ui.confirm`). Panels are not scopes unless marked, and the scene root is never one. So a HUD panel with a button in a gameplay scene is mouse-only and never takes the arrows, Space or A from gameplay; a menu marks its container `.focus_scope()` (the skill's recipes always do). When a scope first appears and nothing is focused, focus goes to its default. Text fields outside a scope still take focus by click. The spec's "panels, popups and modals are scopes by default" is corrected.
- **Focus memory.** It lasts for the scene's lifetime and is forgotten when the scene exits. A panel that is hidden and shown again, or a menu returning from a scene pushed over it, restores its last focus. (A popped and re-pushed pause scene is a new scene instance and starts at its default.)
- **Images and skins take an explicit filter.** Sprites carry no filter in this engine, so `ui.image(&sprite).filter(Filter::Nearest)` and `Skin::filter` choose it (default `Filter::Default`: nearest in pixel-art presentation, linear otherwise). `UiCommand::Sprite` gains a `filter`.
- **Saved bindings.** `SystemState` gains a `bindings` key and `UserPreference::Bindings`. A game that enables user preferences including `Bindings` (`EngineBuilder::with_user_preferences(..)`) gets its bindings loaded at startup and saved by the engine whenever they change. Without it, bindings live for the session.
- **Virtual gamepad events.** gilrs `GamepadId`s cannot be constructed, so scripted and headless pad input uses new `InputEvent::PadButton { button, pressed }` and `InputEvent::PadStick { stick, x, y }` variants, applied to the tracked pad (or a virtual pad named "Virtual pad" when none is connected).
- **`RYO_INPUT` additions:** `N:Wheel@DXxDY` (a wheel step in lines), `N:PadSouth` / `A-B:PadDPadDown` (pad buttons named `Pad` + a `GamepadButton` variant), `A-B:LeftStick@XxY` / `A-B:RightStick@XxY` (a held stick position, released to 0 at `B`).
- **The theme in the world.** `UiTheme` is a world resource holding an `Arc<Theme>`, reached with `ctx.ui_theme()` (`get`, `set`, `modify`); `UiContext` clones the `Arc` at the start of the pass, so a change applies from the next frame.
- **Prompts in markup** (`[prompt=jump]`) are resolved before layout against the actions, the last-used device and `TextIcons`: a registered glyph becomes an inline icon, otherwise a short label such as `[A]` in the accent colour. Chips (rounded key caps) are drawn by the `ui.prompt(action)` widget.
- **Module moves.** `src/core/input.rs` becomes `src/core/input/mod.rs` with `actions.rs` beside it. `src/core/ui/theme.rs` becomes `src/core/ui/theme/`. The old `src/core/ui/retained/` and `src/core/ui/immediate/layout.rs` are deleted once nothing uses them.
- **Scene exits.** `SceneManager` records the scope ids of scenes that exit (pop, switch, clear, reset); the engine drains them each frame and calls `UiMemory::forget_scene`.
- **Builder names.** Containers use `.wrap_items()` for flex wrapping and `.wrap_focus()` for navigation that wraps at a scope's edges (text keeps 4a's `.wrap(width)`). The spec's `.wrap()` (both meanings) is corrected.
- **One memory frame per engine frame.** The UI pass can run twice in a frame (a scene switch from `ui`). `UiMemory` begins its frame once per engine frame; a rerun discards the first pass's records (`restart_pass`) and the resolve step does not run again.
- **Shift+Tab.** Bindings carry no modifiers, so one `ui.next` action on Tab moves forwards, and backwards while Shift is held. Tab switching uses Q/E and the shoulder buttons (demo-kit reserves PageUp/PageDown). The spec's `ui.next`/`ui.prev` and PageUp/PageDown rows are corrected.
- **Rebind capture.** Escape (or a 5 s timeout) cancels a capture, and every pad button can be bound, East included. The spec's "Escape or Back cancels" would make B unbindable. While a row captures, navigation stands aside (a `CAPTURING` record flag), so arrows and Confirm can be captured too.
- **`ui.choice_list`** is a builder whose `show()` returns `Option<usize>` (the spec wrote the call returning it directly).
- **Tab keys stay gameplay's unless tabs take them.** Q, E and the shoulder buttons switch the first `tabs` drawn in the top scene; with no tabs on screen they are not consumed, even inside a focus scope.
- **`wants_keyboard()`** is true for a focused text field or an open list or popup (a trapping scope that is not a modal). A modal does not make it true: the game closes its modal on `ui.back()` or a button. The spec's "or a modal is open" is corrected.
- **Custom widgets paint with a `Painter`** in their own units: `ui.custom(w, h).sense(..).paint(|painter, response| ..)`. The spec's `.show(|ui, rect, response| ..)` with `draw_*` calls is corrected: the builder holds the context, and local units survive layout.
- **Smaller widget surfaces.**
  - Text fields are single-line; `.multiline` is not added.
  - Dropdown, tab and choice options are `&str`.
  - Slots have no `.empty()`: a slot with no icon is empty.
  - Rebind rows have no `.group(..)`: the group comes from the action's name (`ui.*` or the game's).
  - Per-call styles are `.style(LookPatch)`, which changes only what it names.
  - Theme text roles are `TextLook` (size, colour, font, bold) rather than `TextStyle`, so a RON file can state them.
  - Looks switch per state without transition timers.
- **Drag ghosts** are a sprite (`.drag_ghost(&sprite)`, else the slot's icon, else a box), drawn on the popup layer above the popups. The overlay layer stays the engine's (console and debug overlays). The spec's closure ghost on the overlay layer is corrected.
- **Pad families and glyph names.** PlayStation names match "dualsense", "dualshock", "playstation", "sony", "ps4" or "ps5" (a bare "PS" matched unrelated names). Glyph names use the `GamepadButton` variant names, as in `prompt.pad.xbox.South`.
- **Scrolled-out rows stay reachable.** A record inside a scroll area counts as visible to navigation when its scroll area is, so the pad can walk a long list; navigation scrolls to it. "Clipped out" means clipped by anything else.
- **The UI debug overlay** is the console command `debug:ui`, after the console's other `debug:` commands (the spec's `ui_debug`).
- **The demo shell.**
  - The settings panel is one focusable widget that takes all four directions, so its own row window keeps working. A click on a row selects it.
  - Pages with their own focus scope keep the arrows, and reach the panel with Tab or a click.
  - The acceptance pages declare no settings rows.
- **Captures.** The acceptance pages are captured with hand-written `RYO_INPUT` pad scripts in Task 18. The demo tour itself stays input-free.
- **Testing.** Most of the spec's e2e list is pinned without a GPU through the UI driver: a grid sizing to text, crisp rects at 1.25×, ring visibility, a modal blocking, scroll clipping. GPU tests cover the centred panel, the console blocking, a lower scene's pointer, the debug overlay and headless focus. A public `ryo2d::testing::UiHarness` (Task 20) lets games test UI without a GPU and carries the benchmarks.
- **Migration.** Each widget-family task replaces the old widget of that name and ports its call sites mechanically (`ui.button("X", rect)` becomes `ui.button("X").at(rect).clicked()`), so every step builds and passes. The games, demo-kit and the `ui` demo are restructured onto layout and navigation in Tasks 16-18, and the remaining old API is deleted in Task 19.

---

## File Structure

| Path | Responsibility | Task |
|---|---|---|
| `src/core/input/mod.rs` (moved from `input.rs`) | `Input`; adds `InputDevice` tracking, the one-shot wheel, virtual pad events, stick directions, action reads | 1, 2 |
| `src/core/input/actions.rs` | `Actions`, `Binding`, `StickDirection`, `Direction`, `Stick`; parsing, groups, rebind with swap, saved overrides | 2 |
| `src/core/input/AGENTS.md` | Devices, the wheel rule, actions, virtual pad events | 1, 2 |
| `src/core/input_script.rs` | `RYO_INPUT` wheel, pad button and stick entries | 1 |
| `src/core/testing/headless.rs` | `press_pad`, `release_pad`, `tap_pad`, `set_stick`, `wheel` | 1 |
| `src/core/config/save.rs` | `SystemState::bindings`, `UserPreference::Bindings` | 2 |
| `src/core/ui/id.rs` | `UiId`, call-site hashing, scopes, occurrence counting, duplicate keys | 3 |
| `src/core/ui/memory.rs` | `UiMemory`: records (last frame / this frame), typed widget state, focus memory, interaction state | 3, 6, 8 |
| `src/core/ui/layout/mod.rs` | Public style vocabulary: `Length`, `Edges`, `Align`, `Justify`; crate `UiStyle`, `Flow` | 4 |
| `src/core/ui/layout/engine.rs` | `LayoutEngine` (the only Taffy code): persistent tree keyed by `UiId`, measure callback, unrounded results | 4 |
| `src/core/ui/tree.rs` | The subtree being built: nodes, draws with placements, records; resolution into `UiCommand`s and `Record`s | 5 |
| `src/core/ui/containers.rs` | `column`, `row`, `grid`, `stack`, `panel` builders | 5 |
| `src/core/ui/response.rs` | `Response`, `Sense` | 6 |
| `src/core/ui/interact.rs` | The resolve step: hit testing, hot/active/click/drag, wheel routing, activation | 6, 7, 8 |
| `src/core/ui/nav.rs` | Spatial navigation, scopes, Tab order, default focus | 8 |
| `src/core/ui/widgets/mod.rs`, `common.rs` | `WidgetOpts` and the shared builder methods (macro) | 6 |
| `src/core/ui/widgets/{button, custom, scroll, checkbox, toggle, slider, radio, progress, image, separator, tooltip, text_input, text_edit, dropdown, tabs, choice_list, modal, slot, drag, prompt, rebind}.rs` | One widget each | 6-14 |
| `src/core/ui/theme/{mod.rs, look.rs, builtin.rs, file.rs}` | `Theme`, `UiTheme`, looks and patches, variants, skins, sounds, RON | 9, 15 |
| `src/core/ui/immediate/context.rs` | `UiContext`: holds memory, layout, scopes, the subtree, the theme; absolute `draw_*` calls; scenes | 5-19 |
| `src/core/ui/immediate/commands.rs` | `UiCommand` gains `order` (Task 5) and a sprite `filter` (Task 10) | 5, 10 |
| `src/core/render/ui.rs` | `emit_ui` uses `ui_z(layer, order)`; `snapped` shared with layout | 5, 10 |
| `src/core/engine/{mod.rs, frame.rs}` | Owns `UiMemory`, `LayoutEngine`; resolve before scenes; consumption; theme and sounds; saves bindings | 2, 5-9, 15 |
| `src/core/scene/{manager.rs, context.rs}` | `begin_scene`/`end_scene`; exited scopes; `ctx.actions()`, `ctx.ui_theme()` | 2, 5, 9, 12 |
| `src/core/ui/test_util.rs` | Recording contexts and a headless UI driver (scripted input over frames) | 5, 6 |
| `examples/**` | Ports (mechanical in 6-12; restructured in 16-18) | 6-18 |
| `src/core/testing/ui.rs` | `UiHarness`: public GPU-free UI frames over scripted input | 20 |
| `benches/ui.rs` | The 200-widget settings screen, 1,000-row list and empty-frame benches | 20 |

## Task Order

1. Input groundwork: devices, the one-shot wheel, virtual pad events, `RYO_INPUT`, headless helpers
2. Actions: bindings, stick directions, UI defaults, rebind and saved bindings
3. Ids and memory
4. The layout engine (Taffy)
5. The frame pipeline: subtrees, containers, `ui.text` as a leaf, scenes
6. Interaction, responses, custom widgets and the button
7. Scroll containers and wheel routing
8. Focus and navigation
9. The theme
10. Checkbox, toggle, slider, radio, progress, image, separator, spacer, tooltips, disabled
11. The text field
12. Dropdown, tabs, choice list, modal, confirm, popups
13. Slots and drag and drop
14. Prompts and rebinding
15. UI sounds
16. The games and demos on layout and navigation
17. The demo shell on UI focus
18. The `ui` demo: the acceptance showcase
19. Removing the old API, and the UI debug overlay
20. A public UI harness, benchmarks, docs and skills

Each task depends on the ones before it. Tasks 10-15 depend on 6-9 but not on each other beyond what their Interfaces blocks list.

---

### Task 1: Input groundwork: devices, the one-shot wheel, virtual pad events, `RYO_INPUT`, headless helpers

**Files:**
- Move: `src/core/input.rs` → `src/core/input/mod.rs` (`git mv`; no content change in the move itself)
- Create: `src/core/input/AGENTS.md`
- Modify: `src/core/input/mod.rs`, `src/core/input_script.rs`, `src/core/testing/headless.rs`, `src/lib.rs` (prelude: `InputDevice`, `Stick`)
- Test: `src/core/input/mod.rs` (tests module), `src/core/input_script.rs` (tests module), `tests/e2e/headless_engine.rs`

**Interfaces:**
- Consumes: the existing `Input` (`queue`, `begin_frame`, `set_fixed_step`, `end_fixed_steps`, `scroll_delta`, `consume_scroll`, the `handle_gamepad_*` handlers, `OneShots`), `InputScript`, `HeadlessEngine`.
- Produces:
  - `pub enum InputDevice { Mouse, Keyboard, Gamepad }` (`Debug, Clone, Copy, PartialEq, Eq, Hash, Default`; default `Mouse`) and `Input::last_device(&self) -> InputDevice`.
  - `pub enum Stick { Left, Right }` (`Debug, Clone, Copy, PartialEq, Eq, Hash`).
  - `InputEvent::PadButton { button: GamepadButton, pressed: bool }` and `InputEvent::PadStick { stick: Stick, x: f32, y: f32 }`.
  - `Input::scroll_delta()` with the fixed-step one-shot rule (below).
  - `pub(crate) fn gamepad_press_pending(&self, button: GamepadButton) -> bool` (reads the fixed set, like `key_press_pending`).
  - `HeadlessEngine::{press_pad(GamepadButton), release_pad(GamepadButton), tap_pad(GamepadButton), set_stick(Stick, f32, f32), wheel(f32, f32)}`.
  - `RYO_INPUT` entries `N:Wheel@DXxDY`, `N:Pad<Button>`, `A-B:Pad<Button>`, `N:LeftStick@XxY`, `A-B:RightStick@XxY`.

**The rules:**
- *Last device.* A key press (not a release or an OS repeat) makes it `Keyboard`. A mouse button press, any wheel step, or pointer motion of at least 2 screen units from the last position makes it `Mouse` (small jitter does not). A gamepad button press (real or virtual), or a stick whose deadzoned magnitude passes 0.5, makes it `Gamepad`. A gamepad disconnect leaves it as it was.
- *The wheel.* `handle_scroll` adds each step to the frame accumulators (as now) and to new fixed accumulators `scroll_fixed_x`, `scroll_fixed_y`. `begin_frame` zeroes the frame accumulators only. `scroll_delta()` returns `(0, 0)` when the pointer is blocked; otherwise, by `oneshots()`: `Frame` gives the frame accumulators, `Fixed` gives the fixed accumulators, `Suppressed` gives `(0, 0)`. `end_fixed_steps(ran_any)` zeroes the fixed accumulators when `ran_any` (the engine passes `step > 0 || paused`, so a paused frame drops the wheel, as it drops presses). `consume_scroll` zeroes both pairs.
- *Virtual pad events.* `PadButton` and `PadStick` apply to the tracked pad without an id check. If no pad is connected, the first virtual event connects one: `gamepad_connected = true`, `gamepad_name = "Virtual pad"`, `gamepad_id = None`. Refactor the bodies of `handle_gamepad_button` and `handle_gamepad_axis` into `fn set_gamepad_button(&mut self, button: GamepadButton, pressed: bool)` and `fn set_stick_raw(&mut self, stick: Stick, x: f32, y: f32)`, used by both the gilrs handlers (after their id check and button mapping) and the virtual events, so presses go into the frame and fixed sets and sticks are deadzoned identically.

- [ ] **Step 1: Move the file**

```bash
git mv src/core/input.rs src/core/input/mod.rs
cargo check -p ryo2d
```
Expected: builds unchanged (the module path `crate::core::input` is the same).

- [ ] **Step 2: Write the failing tests**

In `src/core/input/mod.rs`'s tests module (it already builds `Input::new()` and calls `queue`/`begin_frame`):

```rust
  fn frame(input: &mut Input, events: Vec<InputEvent>) {
    for event in events {
      input.queue(event);
    }
    input.begin_frame();
  }

  #[test]
  fn the_last_device_follows_the_newest_input() {
    let mut input = Input::new();
    assert_eq!(input.last_device(), InputDevice::Mouse);
    frame(&mut input, vec![InputEvent::Key { key: KeyCode::KeyA, pressed: true }]);
    assert_eq!(input.last_device(), InputDevice::Keyboard);
    frame(&mut input, vec![InputEvent::PadButton { button: GamepadButton::South, pressed: true }]);
    assert_eq!(input.last_device(), InputDevice::Gamepad);
    // A one-unit nudge is jitter, not the mouse
    frame(&mut input, vec![InputEvent::MouseMove { x: 1.0, y: 0.0 }]);
    assert_eq!(input.last_device(), InputDevice::Gamepad);
    frame(&mut input, vec![InputEvent::MouseMove { x: 40.0, y: 30.0 }]);
    assert_eq!(input.last_device(), InputDevice::Mouse);
    frame(&mut input, vec![InputEvent::PadStick { stick: Stick::Left, x: 0.9, y: 0.0 }]);
    assert_eq!(input.last_device(), InputDevice::Gamepad);
    frame(&mut input, vec![InputEvent::Scroll(MouseScrollDelta::LineDelta(0.0, 1.0))]);
    assert_eq!(input.last_device(), InputDevice::Mouse);
  }

  #[test]
  fn the_wheel_shows_in_the_first_fixed_step_only() {
    let mut input = Input::new();
    frame(&mut input, vec![InputEvent::Scroll(MouseScrollDelta::LineDelta(0.0, 2.0))]);
    assert_eq!(input.scroll_delta(), (0.0, 2.0), "the frame view, as the UI pass reads it");
    input.set_fixed_step(0);
    assert_eq!(input.scroll_delta(), (0.0, 2.0));
    input.set_fixed_step(1);
    assert_eq!(input.scroll_delta(), (0.0, 0.0), "a second step must not count it again");
    input.end_fixed_steps(true);
    input.begin_frame();
    input.set_fixed_step(0);
    assert_eq!(input.scroll_delta(), (0.0, 0.0), "seen once, gone");
  }

  #[test]
  fn a_wheel_with_no_fixed_step_waits_for_the_next_frame() {
    let mut input = Input::new();
    frame(&mut input, vec![InputEvent::Scroll(MouseScrollDelta::LineDelta(0.0, -1.0))]);
    input.end_fixed_steps(false);
    frame(&mut input, vec![InputEvent::Scroll(MouseScrollDelta::LineDelta(0.0, -1.0))]);
    input.set_fixed_step(0);
    assert_eq!(input.scroll_delta(), (0.0, -2.0), "both frames' steps reach the first step that runs");
  }

  #[test]
  fn a_paused_frame_drops_the_wheel() {
    let mut input = Input::new();
    frame(&mut input, vec![InputEvent::Scroll(MouseScrollDelta::LineDelta(0.0, 3.0))]);
    // The engine passes `step > 0 || paused`: a pause clears what no step saw
    input.end_fixed_steps(true);
    input.begin_frame();
    input.set_fixed_step(0);
    assert_eq!(input.scroll_delta(), (0.0, 0.0));
  }

  #[test]
  fn consuming_the_wheel_hides_it_from_the_fixed_steps() {
    let mut input = Input::new();
    frame(&mut input, vec![InputEvent::Scroll(MouseScrollDelta::LineDelta(0.0, 1.0))]);
    input.consume_scroll();
    input.set_fixed_step(0);
    assert_eq!(input.scroll_delta(), (0.0, 0.0));
  }

  #[test]
  fn a_virtual_pad_button_reads_like_a_real_one() {
    let mut input = Input::new();
    frame(&mut input, vec![InputEvent::PadButton { button: GamepadButton::South, pressed: true }]);
    assert!(input.gamepad_connected());
    assert_eq!(input.gamepad_name(), "Virtual pad");
    assert!(input.gamepad_button_pressed(GamepadButton::South));
    assert!(input.gamepad_button_down(GamepadButton::South));
    input.set_fixed_step(0);
    assert!(input.gamepad_button_pressed(GamepadButton::South), "the fixed view sees the press too");
    input.end_fixed_steps(true);
    frame(&mut input, vec![InputEvent::PadButton { button: GamepadButton::South, pressed: false }]);
    assert!(input.gamepad_button_released(GamepadButton::South));
    assert!(!input.gamepad_button_down(GamepadButton::South));
  }

  #[test]
  fn a_virtual_stick_is_deadzoned() {
    let mut input = Input::new();
    frame(&mut input, vec![InputEvent::PadStick { stick: Stick::Right, x: 0.1, y: 0.05 }]);
    assert_eq!(input.right_stick(), (0.0, 0.0));
    frame(&mut input, vec![InputEvent::PadStick { stick: Stick::Right, x: 1.0, y: 0.0 }]);
    let (x, y) = input.right_stick();
    assert!((x - 1.0).abs() < 1e-5 && y.abs() < 1e-5);
    assert_eq!(input.right_stick_raw(), (1.0, 0.0));
  }
```

In `src/core/input_script.rs`'s tests (use its `script`, `start`, `finish` helpers):

```rust
  #[test]
  fn wheel_entries_scroll_on_their_frame() {
    let mut s = script("3:Wheel@0x-2");
    let mut input = Input::new();
    start(&mut s, &mut input, 2);
    assert_eq!(input.scroll_delta(), (0.0, 0.0));
    finish(&mut s, &mut input, true);
    start(&mut s, &mut input, 3);
    assert_eq!(input.scroll_delta(), (0.0, -2.0));
  }

  #[test]
  fn pad_taps_and_holds_press_the_virtual_pad() {
    let mut s = script("1:PadSouth 2-4:PadDPadDown");
    let mut input = Input::new();
    start(&mut s, &mut input, 1);
    assert!(input.gamepad_button_pressed(GamepadButton::South));
    finish(&mut s, &mut input, true);
    start(&mut s, &mut input, 2);
    assert!(!input.gamepad_button_down(GamepadButton::South), "a tap comes up once an update saw it");
    assert!(input.gamepad_button_down(GamepadButton::DPadDown));
    finish(&mut s, &mut input, true);
    start(&mut s, &mut input, 3);
    assert!(input.gamepad_button_down(GamepadButton::DPadDown));
    finish(&mut s, &mut input, true);
    start(&mut s, &mut input, 4);
    assert!(!input.gamepad_button_down(GamepadButton::DPadDown), "held until just before frame 4");
  }

  #[test]
  fn stick_ranges_hold_a_position_then_centre() {
    let mut s = script("1-3:LeftStick@0x1");
    let mut input = Input::new();
    start(&mut s, &mut input, 1);
    assert_eq!(input.left_stick_raw(), (0.0, 1.0));
    finish(&mut s, &mut input, true);
    start(&mut s, &mut input, 3);
    assert_eq!(input.left_stick_raw(), (0.0, 0.0));
  }

  #[test]
  fn pad_and_wheel_names_are_checked() {
    for bad in ["1:PadNope", "1:Wheel", "1-3:Wheel@0x1", "1:LeftStick", "1:PadSouth@1x1"] {
      assert!(InputScript::parse(bad).is_err(), "{bad} must be rejected");
    }
    for good in ["1:padsouth", "1:PadDPadUp", "1:Wheel@1x0", "1:RightStick@-1x0"] {
      assert!(InputScript::parse(good).is_ok(), "{good} must parse");
    }
  }
```

In `tests/e2e/headless_engine.rs`:

```rust
#[test]
fn pad_and_wheel_helpers_reach_input() {
  #[derive(Default)]
  struct Watch;
  impl Scene for Watch {
    fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
      SceneAction::Continue
    }
  }
  let Some(mut engine) = HeadlessEngine::new_or_skip(EngineBuilder::new().resolution(64, 64).start_scene(Watch))
  else {
    return;
  };
  engine.tap_pad(GamepadButton::North);
  engine.set_stick(Stick::Left, 0.0, 1.0);
  engine.wheel(0.0, 1.0);
  engine.step().unwrap();
  let input = engine.world().resource::<Input>();
  assert!(input.gamepad_button_down(GamepadButton::North));
  assert_eq!(input.left_stick_raw(), (0.0, 1.0));
  assert_eq!(input.last_device(), InputDevice::Mouse, "the wheel came last");
  engine.step().unwrap();
  assert!(!engine.world().resource::<Input>().gamepad_button_down(GamepadButton::North), "a tap is released after one step");
}
```

- [ ] **Step 3: Run them to see them fail**

Run: `cargo test -p ryo2d --lib input::` and `cargo test --test e2e pad_and_wheel_helpers_reach_input`
Expected: compile errors (`InputDevice`, `Stick`, `PadButton`, `PadStick`, `tap_pad`, ... not found).

- [ ] **Step 4: Implement**

In `src/core/input/mod.rs`:

```rust
/// The kind of device the player used last: prompts and the focus ring follow it
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum InputDevice {
  #[default]
  Mouse,
  Keyboard,
  Gamepad,
}

/// A gamepad stick
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stick {
  Left,
  Right,
}
```

Add to `InputEvent`, with doc comments:

```rust
  /// A gamepad button with no gilrs id: scripted and headless input. Applies
  /// to the tracked pad, or connects a virtual one
  PadButton { button: GamepadButton, pressed: bool },
  /// A stick position (-1..1 each axis, +y up) with no gilrs id
  PadStick { stick: Stick, x: f32, y: f32 },
```

Add the fields `last_device: InputDevice`, `scroll_fixed_x: f32`, `scroll_fixed_y: f32` to `Input`. Then:

- In `apply`, route the two new variants: `PadButton` → `self.connect_virtual_pad(); self.set_gamepad_button(button, pressed)`, and `PadStick` → `self.connect_virtual_pad(); self.set_stick_raw(stick, x, y)`. Here `connect_virtual_pad` sets `gamepad_connected`, sets `gamepad_name = "Virtual pad".to_string()` and leaves `gamepad_id` as `None`, only when no pad is connected.
- Set `last_device`:
  - in `handle_keyboard` on `ElementState::Pressed` when the key was not already down;
  - in `handle_mouse_button` on press;
  - in `handle_scroll`;
  - in `handle_mouse_move` when `(x - self.mouse_x).hypot(y - self.mouse_y) >= 2.0`, computed before the position is stored;
  - in `set_gamepad_button` on press;
  - in `set_stick_raw` when the deadzoned magnitude of that stick is > 0.5.
- `handle_scroll` adds each step to the fixed accumulators as well.
- `scroll_delta`, `end_fixed_steps` and `consume_scroll` follow the rules above.

```rust
  pub fn scroll_delta(&self) -> (f32, f32) {
    if self.pointer_blocked() {
      return (0.0, 0.0);
    }
    match self.oneshots() {
      OneShots::Frame => (self.scroll_delta_x, self.scroll_delta_y),
      OneShots::Fixed => (self.scroll_fixed_x, self.scroll_fixed_y),
      OneShots::Suppressed => (0.0, 0.0),
    }
  }

  /// The player's last-used device (keyboard, mouse or gamepad)
  pub fn last_device(&self) -> InputDevice {
    self.last_device
  }

  /// A pad press waiting for the next fixed step (scripted input)
  pub(crate) fn gamepad_press_pending(&self, button: GamepadButton) -> bool {
    self.gamepad_buttons_pressed_fixed.contains(&button)
  }
```

Update the doc comment of `scroll_delta` to say:
- inside the fixed steps, the first step sees the frame's wheel and later steps see none;
- a frame with no step carries its wheel to the next first step;
- a pause drops it.

In `src/core/input_script.rs`:
- Extend `Control` with `Pad(GamepadButton)` and `Stick(Stick, f32, f32)`, and add an `Action::Wheel(f32, f32)` variant queued as `InputEvent::Scroll(MouseScrollDelta::LineDelta(dx, dy))`.
- In `parse_entry`, handle the new forms:
  - `Wheel` requires `@` and rejects a range.
  - A name starting with `Pad` (case-insensitive) maps the rest through a `PAD_BUTTONS: &[(&str, GamepadButton)]` table of every variant name (`South`, `East`, `West`, `North`, `LeftBumper`, `RightBumper`, `LeftTrigger`, `RightTrigger`, `LeftStick`, `RightStick`, `Select`, `Start`, `Guide`, `DPadUp`, `DPadDown`, `DPadLeft`, `DPadRight`, case-insensitive), and rejects `@`.
  - `LeftStick`/`RightStick` require `@`.
- `set(input, Control::Pad(b), state)` queues `PadButton`. `Control::Stick` presses queue `PadStick { x, y }` and releases queue `PadStick { 0, 0 }`. A stick tap releases before the next frame (no fixed-step wait).
- `press_pending` uses `gamepad_press_pending` for pads.
- Update the module doc comment's bullet list and the error message so they list the new forms: `Wheel@DXxDY`, `Pad<Button>` (e.g. `PadSouth`, `PadDPadDown`), `LeftStick@XxY`, `RightStick@XxY`.

In `src/core/testing/headless.rs`:
- `press_pad`, `release_pad` and `set_stick` queue the virtual events.
- `tap_pad` presses and records a pending pad release, released after the next step, as `tap_key` does with `pending_releases`. Add a `pending_pad_releases: Vec<GamepadButton>` field.
- `wheel(dx, dy)` queues `InputEvent::Scroll(MouseScrollDelta::LineDelta(dx, dy))`.
- Each method gets a one-line doc comment.

In `src/lib.rs`, add `InputDevice` and `Stick` to the prelude's input line.

Create `src/core/input/AGENTS.md`:

```markdown
# Input

- `Input` is a world resource fed by queued `InputEvent`s, published in `First` by `begin_frame`.
- One-shots (presses, releases, the wheel) read the frame view outside the fixed steps, the first fixed step's view inside them, and nothing in later steps. A frame with no fixed step carries them to the next first step; a pause drops them (`end_fixed_steps(step > 0 || paused)`).
- `last_device()` follows the newest key press, mouse press/wheel/motion (2+ units), or pad press/stick (past 0.5): prompts and the UI's focus ring use it.
- Only the first connected gamepad is tracked. `InputEvent::PadButton`/`PadStick` are id-less pad events for scripted and headless input; they connect a "Virtual pad" when none is connected.
- `RYO_INPUT` (`input_script.rs`) scripts keys, the mouse, the wheel, pad buttons and sticks for dev captures.
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p ryo2d --lib input::` and `cargo test -p ryo2d --lib input_script` and `cargo test --test e2e pad_and_wheel_helpers_reach_input`
Expected: pass (the e2e test skips without a GPU, but must run here).

Then run `cargo test --workspace`. Expected: everything that passed before still passes. The existing scroll tests (`scroll_accumulates_within_frame` and the UI scroll-view tests) read the frame view, which is unchanged.

- [ ] **Step 6: Gate and commit**

Run the gate (Global Constraints).

```bash
git add src/core/input/mod.rs src/core/input/AGENTS.md src/core/input_script.rs src/core/testing/headless.rs src/lib.rs tests/e2e/headless_engine.rs
git commit -m "feat: input devices, a one-shot wheel and virtual pad input" -m "- the last-used device (keyboard, mouse or gamepad) is tracked
- the wheel shows in the first fixed step only, like presses
- id-less pad events drive a virtual pad for scripts and headless tests; RYO_INPUT gains wheel, pad and stick entries"
```

### Task 2: Actions: bindings, stick directions, UI defaults, rebind and saved bindings

**Files:**
- Create: `src/core/input/actions.rs`
- Modify:
  - `src/core/input/mod.rs`: the `Actions` field, stick directions, action reads, `consume_binding`.
  - `src/core/input_script.rs`: share its key-name table.
  - `src/core/scene/context.rs`: `ctx.actions()`.
  - `src/core/config/save.rs`: `SystemState::bindings`, `UserPreference::Bindings`.
  - `src/core/config/resolve.rs`: `StoredPreferences::bindings`.
  - `src/core/engine/mod.rs`: apply saved bindings at startup; save on change.
  - `src/lib.rs`: prelude.
  - `src/core/input/AGENTS.md`.
- Test: `src/core/input/actions.rs`, `src/core/input/mod.rs`, `src/core/config/save.rs` (tests modules)

**Interfaces:**
- Consumes: Task 1's `Stick`, the virtual pad events, the one-shot sets and `OneShots`.
- Produces (all `pub` unless noted; `Debug, Clone, Copy, PartialEq, Eq, Hash` on the small enums):

```rust
pub enum Direction { Up, Down, Left, Right }
pub struct StickDirection { pub stick: Stick, pub direction: Direction }
pub enum Binding {
  Key(KeyCode),
  Mouse(MouseButton),
  Pad(GamepadButton),
  Stick(StickDirection),
}
impl Binding {
  /// "Key:Space", "Mouse:Left", "Pad:South", "Stick:LeftUp"
  pub fn parse(text: &str) -> Option<Binding>;
  pub fn to_config(&self) -> String;
}
pub struct Actions { /* ordered entries, saved overrides, dirty flag */ }
impl Actions {
  pub fn bind(&mut self, name: &str, bindings: impl IntoIterator<Item = Binding>) -> &mut Self;
  pub fn bindings(&self, name: &str) -> &[Binding];          // empty for an unknown action
  pub fn set_bindings(&mut self, name: &str, bindings: Vec<Binding>);
  /// Bind `binding` in `slot` (appending past the end). If another action in
  /// the same group has it, the two swap; returns that action's name
  pub fn rebind(&mut self, name: &str, slot: usize, binding: Binding) -> Option<String>;
  pub fn reset_defaults(&mut self);
  pub fn names(&self) -> impl Iterator<Item = &str>;
  /// "ui" for `ui.*` actions, "game" for the rest
  pub fn group(name: &str) -> &str;
  pub(crate) fn with_ui_defaults() -> Actions;
  pub(crate) fn load_saved(&mut self, saved: &BTreeMap<String, Vec<String>>);
  pub(crate) fn saved_map(&self) -> BTreeMap<String, Vec<String>>;  // actions whose bindings differ from their defaults
  pub(crate) fn take_dirty(&mut self) -> bool;
}
// on Input:
pub fn actions(&self) -> &Actions;
pub fn actions_mut(&mut self) -> &mut Actions;
pub fn action_pressed(&self, name: &str) -> bool;
pub fn action_held(&self, name: &str) -> bool;
pub fn action_released(&self, name: &str) -> bool;
pub fn stick_direction_down(&self, direction: StickDirection) -> bool;
pub fn stick_direction_pressed(&self, direction: StickDirection) -> bool;
pub(crate) fn binding_pressed(&self, binding: &Binding) -> bool;
pub(crate) fn binding_held(&self, binding: &Binding) -> bool;
pub(crate) fn consume_binding(&mut self, binding: &Binding);
// on SceneContext:
pub fn actions(&mut self) -> &mut Actions;
// in config:
SystemState::bindings: BTreeMap<String, Vec<String>>;   // serialized as "bindings" only when non-empty
UserPreference::Bindings;
```

**The UI defaults** (`Actions::with_ui_defaults`, bound by `Input::new`):

| Action | Bindings |
|---|---|
| `ui.up` | `Key(ArrowUp)`, `Pad(DPadUp)`, `Stick(Left, Up)` |
| `ui.down` | `Key(ArrowDown)`, `Pad(DPadDown)`, `Stick(Left, Down)` |
| `ui.left` | `Key(ArrowLeft)`, `Pad(DPadLeft)`, `Stick(Left, Left)` |
| `ui.right` | `Key(ArrowRight)`, `Pad(DPadRight)`, `Stick(Left, Right)` |
| `ui.confirm` | `Key(Enter)`, `Key(NumpadEnter)`, `Key(Space)`, `Pad(South)` |
| `ui.back` | `Key(Escape)`, `Pad(East)` |
| `ui.next` | `Key(Tab)` (with Shift held it moves backwards; navigation reads Shift) |
| `ui.tab_next` / `ui.tab_prev` | `Key(KeyE)`, `Pad(RightBumper)` / `Key(KeyQ)`, `Pad(LeftBumper)` |

The spec listed `ui.next` and `ui.prev` on Tab and Shift+Tab, and PageUp/PageDown for tabs.
- Bindings carry no modifiers, so one `ui.next` on Tab serves both directions.
- demo-kit reserves PageUp and PageDown for page switching, so tab switching uses Q and E, the common game convention.

The spec's `ui.next`/`ui.prev` row is corrected.

**Stick directions.** In `set_stick_raw`, after the deadzone, the four directions of that stick update a digital state. The component is `y` for Up, `-y` for Down, `x` for Right and `-x` for Left (+y is up).
- A direction goes down when its component passes 0.5, and comes up when it falls below 0.4.
- Going down inserts into `stick_dirs_pressed` and `stick_dirs_pressed_fixed`, and coming up into the released sets, with the same frame and fixed rules as buttons.
- `begin_frame` clears the frame sets, and `end_fixed_steps(true)` clears the fixed sets.
- Consumption uses a `consumed_stick_dirs` set, cleared in `begin_frame`.

- [ ] **Step 1: Write the failing tests**

`src/core/input/actions.rs`:

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use winit::keyboard::KeyCode;

  #[test]
  fn bindings_print_and_parse_back() {
    for binding in [
      Binding::Key(KeyCode::Space),
      Binding::Mouse(MouseButton::Right),
      Binding::Pad(GamepadButton::DPadUp),
      Binding::Stick(StickDirection { stick: Stick::Right, direction: Direction::Left }),
    ] {
      assert_eq!(Binding::parse(&binding.to_config()), Some(binding));
    }
    assert_eq!(Binding::Key(KeyCode::Space).to_config(), "Key:Space");
    assert_eq!(Binding::parse("Stick:LeftUp"), Some(Binding::Stick(StickDirection { stick: Stick::Left, direction: Direction::Up })));
    for bad in ["", "Key:", "Key:Nope", "Pad:Nope", "Joystick:Up", "Stick:Middle"] {
      assert_eq!(Binding::parse(bad), None, "{bad}");
    }
  }

  #[test]
  fn rebinding_to_a_key_another_action_has_swaps_them() {
    let mut actions = Actions::default();
    actions.bind("jump", [Binding::Key(KeyCode::Space)]);
    actions.bind("dash", [Binding::Key(KeyCode::ShiftLeft)]);
    let swapped = actions.rebind("jump", 0, Binding::Key(KeyCode::ShiftLeft));
    assert_eq!(swapped.as_deref(), Some("dash"));
    assert_eq!(actions.bindings("jump"), &[Binding::Key(KeyCode::ShiftLeft)]);
    assert_eq!(actions.bindings("dash"), &[Binding::Key(KeyCode::Space)]);
    assert!(actions.take_dirty());
    assert!(!actions.take_dirty());
  }

  #[test]
  fn groups_keep_ui_and_game_bindings_apart() {
    let mut actions = Actions::with_ui_defaults();
    actions.bind("jump", [Binding::Key(KeyCode::KeyZ)]);
    // Space is ui.confirm's, but the UI and the game are separate groups
    assert_eq!(actions.rebind("jump", 0, Binding::Key(KeyCode::Space)), None);
    assert!(actions.bindings("ui.confirm").contains(&Binding::Key(KeyCode::Space)));
    assert_eq!(Actions::group("ui.confirm"), "ui");
    assert_eq!(Actions::group("jump"), "game");
  }

  #[test]
  fn saved_bindings_override_what_the_game_binds() {
    let mut actions = Actions::default();
    let mut saved = BTreeMap::new();
    saved.insert("jump".to_string(), vec!["Key:KeyJ".to_string(), "Nope".to_string()]);
    actions.load_saved(&saved);
    actions.bind("jump", [Binding::Key(KeyCode::Space)]);
    assert_eq!(actions.bindings("jump"), &[Binding::Key(KeyCode::KeyJ)], "unparsable entries are skipped");
    assert_eq!(actions.saved_map().get("jump"), Some(&vec!["Key:KeyJ".to_string()]));
    actions.reset_defaults();
    assert_eq!(actions.bindings("jump"), &[Binding::Key(KeyCode::Space)]);
    assert!(actions.saved_map().is_empty(), "defaults are not saved");
  }

  #[test]
  fn rebinding_past_the_end_adds_a_binding() {
    let mut actions = Actions::default();
    actions.bind("jump", [Binding::Key(KeyCode::Space)]);
    actions.rebind("jump", 5, Binding::Pad(GamepadButton::South));
    assert_eq!(actions.bindings("jump"), &[Binding::Key(KeyCode::Space), Binding::Pad(GamepadButton::South)]);
  }
}
```

In `src/core/input/mod.rs`'s tests (reuse Task 1's `frame` helper):

```rust
  #[test]
  fn actions_read_through_their_bindings() {
    let mut input = Input::new();
    input.actions_mut().bind("jump", [Binding::Key(KeyCode::Space), Binding::Pad(GamepadButton::South)]);
    frame(&mut input, vec![InputEvent::PadButton { button: GamepadButton::South, pressed: true }]);
    assert!(input.action_pressed("jump"));
    assert!(input.action_held("jump"));
    input.set_fixed_step(0);
    assert!(input.action_pressed("jump"), "the first fixed step sees it");
    input.set_fixed_step(1);
    assert!(!input.action_pressed("jump"));
    assert!(input.action_held("jump"), "held is not a one-shot");
    input.end_fixed_steps(true);
    frame(&mut input, vec![InputEvent::PadButton { button: GamepadButton::South, pressed: false }]);
    assert!(input.action_released("jump"));
    assert!(!input.action_pressed("nothing bound"));
  }

  #[test]
  fn stick_directions_press_past_half_and_release_below_forty_percent() {
    let mut input = Input::new();
    let up = StickDirection { stick: Stick::Left, direction: Direction::Up };
    frame(&mut input, vec![InputEvent::PadStick { stick: Stick::Left, x: 0.0, y: 0.45 }]);
    assert!(!input.stick_direction_down(up));
    frame(&mut input, vec![InputEvent::PadStick { stick: Stick::Left, x: 0.0, y: 0.7 }]);
    assert!(input.stick_direction_pressed(up) && input.stick_direction_down(up));
    frame(&mut input, vec![InputEvent::PadStick { stick: Stick::Left, x: 0.0, y: 0.45 }]);
    assert!(input.stick_direction_down(up), "hysteresis: still down above 0.4");
    assert!(!input.stick_direction_pressed(up));
    frame(&mut input, vec![InputEvent::PadStick { stick: Stick::Left, x: 0.0, y: 0.3 }]);
    assert!(!input.stick_direction_down(up));
    assert!(!input.action_held("ui.up"));
  }

  #[test]
  fn the_ui_actions_are_bound_from_the_start() {
    let mut input = Input::new();
    frame(&mut input, vec![InputEvent::Key { key: KeyCode::Enter, pressed: true }]);
    assert!(input.action_pressed("ui.confirm"));
    frame(&mut input, vec![InputEvent::PadButton { button: GamepadButton::East, pressed: true }]);
    assert!(input.action_pressed("ui.back"));
  }

  #[test]
  fn consuming_a_binding_hides_it() {
    let mut input = Input::new();
    frame(&mut input, vec![InputEvent::Key { key: KeyCode::ArrowDown, pressed: true }]);
    input.consume_binding(&Binding::Key(KeyCode::ArrowDown));
    assert!(!input.action_pressed("ui.down"));
    assert!(!input.key_pressed(KeyCode::ArrowDown));
  }
```

In `src/core/config/save.rs`'s tests:

```rust
  #[test]
  fn bindings_round_trip_through_the_system_file() {
    let dir = tempfile::tempdir().unwrap();
    let manager = SaveManager::new(dir.path()).unwrap();
    let mut state = SystemState::new();
    state.bindings.insert("jump".to_string(), vec!["Key:KeyJ".to_string(), "Pad:South".to_string()]);
    manager.save_system(&state).unwrap();
    let loaded = manager.load_system().unwrap();
    assert_eq!(loaded.bindings, state.bindings);
    let text = std::fs::read_to_string(dir.path().join("system.json")).unwrap();
    assert!(text.contains("\"bindings\""));
  }

  #[test]
  fn an_empty_binding_table_is_not_written() {
    let dir = tempfile::tempdir().unwrap();
    let manager = SaveManager::new(dir.path()).unwrap();
    manager.save_system(&SystemState::new()).unwrap();
    let text = std::fs::read_to_string(dir.path().join("system.json")).unwrap();
    assert!(!text.contains("bindings"));
  }
```

Adapt the `SaveManager`/`system.json` names if the module's tests use other helpers. Read the existing tests first.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p ryo2d --lib input::` and `cargo test -p ryo2d --lib config::save`
Expected: compile errors (`Actions`, `Binding`, `SystemState::bindings` not found).

- [ ] **Step 3: Write `actions.rs`**

```rust
//! Named, rebindable actions: a game binds "jump" to keys, mouse buttons,
//! pad buttons and stick directions, and reads `input.action_pressed("jump")`.
//! The UI's own actions (`ui.*`) are bound here too.

use std::collections::BTreeMap;

use winit::event::MouseButton;
use winit::keyboard::KeyCode;

use super::{GamepadButton, Stick};

/// A direction on the screen or a stick
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
  Up,
  Down,
  Left,
  Right,
}

/// One stick pushed one way, read as a button past half deflection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StickDirection {
  pub stick: Stick,
  pub direction: Direction,
}

/// What can trigger an action
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Binding {
  Key(KeyCode),
  Mouse(MouseButton),
  Pad(GamepadButton),
  Stick(StickDirection),
}

#[derive(Debug, Clone)]
struct Entry {
  name: String,
  defaults: Vec<Binding>,
  current: Vec<Binding>,
}

/// The game's actions and their bindings. Lives in `Input`; reach it with
/// `ctx.actions()`
#[derive(Debug, Clone, Default)]
pub struct Actions {
  entries: Vec<Entry>,
  saved: BTreeMap<String, Vec<Binding>>,
  dirty: bool,
}
```

Then implement the methods as specified:

- **`Binding::parse` and `to_config`.**
  - Keys use the key-name table that `input_script.rs` already has. Move that table into `actions.rs` as `pub(crate) const KEY_NAMES` with `key_from_name` and `key_name` helpers, and have `input_script.rs` use them.
  - Mouse names are `Left`, `Right`, `Middle`, `Back` and `Forward`.
  - Pad names are the `GamepadButton` variant names. Move the table Task 1 added to `input_script.rs` here as `PAD_NAMES`, and share it.
  - Stick names are `LeftUp`, `LeftDown`, `LeftLeft`, `LeftRight`, then the same four for `Right`.
- **`bind`** replaces an existing entry's defaults (keeping the order of first binding). Its current bindings are the saved override if `saved` has one, otherwise the defaults.
- **`rebind`.** Find another entry in the same group whose `current` contains `binding`, and remove it from there. If `name` had a binding at `slot`, put that old binding where the removed one was, so the two swap. Then set `name`'s slot (append if `slot >= len`), mark dirty, and return the other entry's name.
- **`set_bindings`** marks dirty.
- **`reset_defaults`** sets every `current` to its `defaults`, clears `saved` and marks dirty.
- **`load_saved`** parses each string with `Binding::parse`. An unparsable string is skipped with `engine_warn!(module: "input", "saved binding {s:?} for {name:?} is not a binding; skipped")`. Entries already bound take the override at once.
- **`saved_map`** lists the entries whose `current != defaults`.
- **`group(name)`** is the text before the first `.` when that text is `ui`, otherwise `"game"`.

In `src/core/input/mod.rs`:
- Add `mod actions; pub use actions::{Actions, Binding, Direction, StickDirection};`.
- Add the `actions: Actions` field, set from `Actions::with_ui_defaults()` in `Input::new`.
- Add the stick-direction sets and the methods listed in Interfaces.
- `binding_pressed` matches on the binding and calls `key_pressed`, `mouse_pressed`, `gamepad_button_pressed` or `stick_direction_pressed`, so it inherits blocking, consumption and the one-shot rule.
- `binding_held` does the same with the `*_down` reads.
- `action_released` uses the `*_released` reads.
- `consume_binding` calls `consume_key`, `consume_mouse` or `consume_gamepad_button`, or inserts into `consumed_stick_dirs`.

In `src/core/scene/context.rs`:

```rust
  /// The game's actions and their bindings ([`Actions`]): bind "jump" here and
  /// read it with `ctx.input.action_pressed("jump")`
  pub fn actions(&mut self) -> &mut Actions {
    self.input.actions_mut()
  }
```

In config:
- Add `bindings: BTreeMap<String, Vec<String>>` to `SystemState`. Wire it through `Serialize` (write the `"bindings"` key only when non-empty), `RawSystemFile` (optional), `into_state`, `Default`, `from_defaults`, `UserPreference::Bindings` (and `all()`), `StoredPreferences::bindings: Option<BTreeMap<..>>`, and `write_filtered_system` (keep the key when `Bindings` is enabled).
- In `engine/mod.rs`, where the stored preferences are applied at startup, call `world.resource_mut::<Input>().actions_mut().load_saved(&bindings)` when they carry bindings.
- When the filter includes `Bindings`, keep `bindings_saver: Option<SaveManager>` on the `Engine` (from `workspace.create_save_manager()`).
- At the end of `Engine::frame` (after the fixed steps), if `input.actions_mut().take_dirty()` and the saver exists, run `let mut state = saver.load_system()?; state.bindings = actions.saved_map(); saver.save_system(&state)`. A failure warns once with `engine_warn!(module: "input", "could not save bindings: {e}")`.
- Headless and web builds have no saver.

In `src/lib.rs`, add `Actions`, `Binding`, `Direction`, `StickDirection` to the prelude's input line.

Append to `src/core/input/AGENTS.md`:

```markdown
- Actions (`actions.rs`): named bindings to keys, mouse buttons, pad buttons and stick directions (digital past 0.5, released below 0.4). Reads follow the one-shot rule of what they bind. Groups: `ui.*` and the game's; a rebind that collides inside a group swaps the two bindings.
- The UI's actions (`ui.up/down/left/right/confirm/back/next/tab_next/tab_prev`) are bound in `Input::new`; games may rebind them.
- Saved bindings: with `UserPreference::Bindings` enabled, `system.json` keeps the bindings that differ from the defaults, loaded at startup and saved when they change.
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ryo2d --lib input::` and `cargo test -p ryo2d --lib config::` and `cargo test --workspace`
Expected: pass.

- [ ] **Step 5: Gate and commit**

```bash
git add src/core/input/actions.rs src/core/input/mod.rs src/core/input/AGENTS.md src/core/input_script.rs src/core/scene/context.rs src/core/config/save.rs src/core/config/resolve.rs src/core/engine/mod.rs src/lib.rs
git commit -m "feat: input actions with rebinding and saved bindings" -m "- actions bind keys, mouse and pad buttons and stick directions; reads follow the one-shot rule
- the UI's navigation actions are bound by default
- a rebind that collides swaps; player bindings save to system.json when enabled"
```

### Task 3: Ids and memory

**Files:**
- Create: `src/core/ui/id.rs`, `src/core/ui/memory.rs`
- Modify: `src/core/ui/mod.rs` (modules; `pub use id::UiId`), `src/lib.rs` (prelude: `UiId`)
- Test: `src/core/ui/id.rs`, `src/core/ui/memory.rs` (tests modules)

**Interfaces:**
- Consumes: `Rect` (`ui/types.rs`), `engine_warn!`.
- Produces:

```rust
pub struct UiId(pub(crate) u64);                     // Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default
impl UiId {
  pub const NONE: UiId;
  pub(crate) fn root(scene_scope: u64) -> UiId;
}
pub(crate) struct IdScopes;                           // Default
impl IdScopes {
  pub(crate) fn begin_frame(&mut self);
  pub(crate) fn enter_root(&mut self, root: UiId);
  pub(crate) fn current(&self) -> UiId;
  pub(crate) fn push(&mut self, scope: UiId);
  pub(crate) fn pop(&mut self);
  pub(crate) fn make(&mut self, site: &'static std::panic::Location<'static>, key: Option<u64>) -> UiId;
}
pub(crate) fn key_hash<K: std::hash::Hash + ?Sized>(key: &K) -> u64;

pub(crate) struct Flags(u32);                         // Debug, Clone, Copy, PartialEq, Eq, Default; BitOr, BitOrAssign
impl Flags {
  pub(crate) const CLICK, DRAG, FOCUSABLE, BLOCKING, SCROLLABLE, DROP_TARGET, DISABLED,
    TAKES_HORIZONTAL, TAKES_VERTICAL, FOCUS_SCOPE, TRAP, WRAP, MODAL, TEXT_FIELD,
    DEFAULT_FOCUS, USES_BACK, DRAG_SOURCE, NAV_SKIP: Flags;
  pub(crate) fn contains(self, other: Flags) -> bool;
  pub(crate) fn intersects(self, other: Flags) -> bool;
}
pub(crate) struct Record {                            // Debug, Clone, Copy, PartialEq
  pub(crate) rect: Rect, pub(crate) clip: Option<Rect>, pub(crate) layer: u8, pub(crate) order: u32,
  pub(crate) depth: u16, pub(crate) scene: u64, pub(crate) flags: Flags,
  pub(crate) scope: UiId,     // nearest focus-scope ancestor (NONE outside scopes)
  pub(crate) scroll: UiId,    // nearest scrollable ancestor (NONE)
  pub(crate) content: glam::Vec2,  // scrollables: content size
  pub(crate) nav: [UiId; 4],  // explicit Up, Down, Left, Right neighbours (NONE)
}
pub(crate) struct UiMemory;
impl UiMemory {
  pub(crate) fn new() -> UiMemory;
  pub(crate) fn begin_frame(&mut self);
  pub(crate) fn end_frame(&mut self);
  pub(crate) fn frame(&self) -> u64;
  pub(crate) fn record(&mut self, id: UiId, record: Record);
  pub(crate) fn prev(&self, id: UiId) -> Option<&Record>;
  pub(crate) fn prev_records(&self) -> impl Iterator<Item = (UiId, &Record)>;
  pub(crate) fn current(&self, id: UiId) -> Option<&Record>;
  /// Always `Some`; the `Option` keeps library code panic-free
  pub(crate) fn state<T: Default + 'static>(&mut self, id: UiId, scene: u64) -> Option<&mut T>;
  pub(crate) fn peek_state<T: 'static>(&self, id: UiId) -> Option<&T>;
  pub(crate) fn remember_focus(&mut self, scope: UiId, scene: u64, focused: UiId);
  pub(crate) fn recall_focus(&self, scope: UiId) -> Option<UiId>;
  pub(crate) fn forget_scene(&mut self, scene: u64);
  pub(crate) scopes: IdScopes,                        // public field for UiContext
}
```

- [ ] **Step 1: Write the failing tests**

`src/core/ui/id.rs`:

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use std::panic::Location;

  #[track_caller]
  fn here() -> &'static Location<'static> {
    Location::caller()
  }

  fn scopes() -> IdScopes {
    let mut scopes = IdScopes::default();
    scopes.begin_frame();
    scopes.enter_root(UiId::root(1));
    scopes
  }

  #[test]
  fn one_site_run_twice_gives_two_ids() {
    let mut s = scopes();
    let site = here();
    let a = s.make(site, None);
    let b = s.make(site, None);
    assert_ne!(a, b);
    assert_ne!(a, UiId::NONE);
  }

  #[test]
  fn ids_are_stable_across_frames() {
    let site_a = here();
    let site_b = here();
    let mut s = scopes();
    let first = (s.make(site_a, None), s.make(site_b, None));
    s.begin_frame();
    s.enter_root(UiId::root(1));
    let second = (s.make(site_a, None), s.make(site_b, None));
    assert_eq!(first, second);
    assert_ne!(first.0, first.1, "two lines, two ids");
  }

  #[test]
  fn keys_survive_a_reorder() {
    let site = here();
    let mut s = scopes();
    let a1 = s.make(site, Some(key_hash(&"sword")));
    let b1 = s.make(site, Some(key_hash(&"shield")));
    s.begin_frame();
    s.enter_root(UiId::root(1));
    let b2 = s.make(site, Some(key_hash(&"shield")));
    let a2 = s.make(site, Some(key_hash(&"sword")));
    assert_eq!((a1, b1), (a2, b2));
  }

  #[test]
  fn skipping_an_earlier_call_shifts_occurrence_ids() {
    // Documents the rule: a helper's widgets are told apart by order
    let site = here();
    let mut s = scopes();
    let _first = s.make(site, None);
    let second = s.make(site, None);
    s.begin_frame();
    s.enter_root(UiId::root(1));
    let now_first = s.make(site, None);
    assert_ne!(now_first, second, "the second widget now gets the first slot's id");
  }

  #[test]
  fn a_duplicate_key_is_suffixed_and_warned_once() {
    let site = here();
    let mut s = scopes();
    let a = s.make(site, Some(7));
    let b = s.make(site, Some(7));
    let c = s.make(site, Some(7));
    assert!(a != b && b != c && a != c);
    let warnings = crate::core::dev_log::get_entries()
      .iter()
      .filter(|e| e.message.contains("share the id key"))
      .count();
    assert_eq!(warnings, 1);
  }

  #[test]
  fn scopes_separate_the_same_site() {
    let site = here();
    let mut s = scopes();
    let outer = s.make(site, None);
    s.push(outer);
    let inner = s.make(site, None);
    s.pop();
    assert_ne!(outer, inner);
    assert_ne!(UiId::root(1), UiId::root(2));
  }
}
```

`src/core/ui/memory.rs`:

```rust
#[cfg(test)]
mod tests {
  use super::*;

  fn record(scene: u64) -> Record {
    Record {
      rect: Rect::new(0.0, 0.0, 10.0, 10.0),
      clip: None,
      layer: 0,
      order: 0,
      depth: 0,
      scene,
      flags: Flags::CLICK,
      scope: UiId::NONE,
      scroll: UiId::NONE,
      content: glam::Vec2::ZERO,
      nav: [UiId::NONE; 4],
    }
  }

  #[test]
  fn records_from_last_frame_are_read_this_frame() {
    let mut memory = UiMemory::new();
    memory.begin_frame();
    memory.record(UiId(5), record(1));
    assert!(memory.prev(UiId(5)).is_none(), "not until the next frame");
    memory.end_frame();
    memory.begin_frame();
    assert!(memory.prev(UiId(5)).is_some());
    memory.end_frame();
    memory.begin_frame();
    assert!(memory.prev(UiId(5)).is_none(), "gone once a frame did not record it");
  }

  #[test]
  fn widget_state_is_dropped_after_a_frame_unseen() {
    let mut memory = UiMemory::new();
    memory.begin_frame();
    *memory.state::<u32>(UiId(9), 1).unwrap() = 42;
    memory.end_frame();
    memory.begin_frame();
    assert_eq!(*memory.state::<u32>(UiId(9), 1).unwrap(), 42, "seen again: kept");
    memory.end_frame();
    memory.begin_frame();
    memory.end_frame();
    memory.begin_frame();
    assert_eq!(*memory.state::<u32>(UiId(9), 1).unwrap(), 0, "a frame unseen: dropped");
  }

  #[test]
  fn focus_memory_lasts_until_the_scene_is_forgotten() {
    let mut memory = UiMemory::new();
    memory.remember_focus(UiId(100), 3, UiId(7));
    for _ in 0..5 {
      memory.begin_frame();
      memory.end_frame();
    }
    assert_eq!(memory.recall_focus(UiId(100)), Some(UiId(7)));
    memory.forget_scene(3);
    assert_eq!(memory.recall_focus(UiId(100)), None);
  }
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p ryo2d --lib ui::id ui::memory`
Expected: compile errors (modules missing).

- [ ] **Step 3: Write `id.rs`**

```rust
//! Widget ids: a hash of the enclosing scope, the call site, and how often
//! that site ran in the scope this frame (or an explicit key in its place)

use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::panic::Location;

/// A widget's identity across frames. Made from where the widget is called;
/// `.id(key)` makes it from a key instead, for lists that reorder
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct UiId(pub(crate) u64);

impl UiId {
  /// No widget
  pub const NONE: UiId = UiId(0);

  /// The root scope of a scene's UI
  pub(crate) fn root(scene_scope: u64) -> UiId {
    UiId(mix(FNV_OFFSET, &[0x00ec_0e00, scene_scope]))
  }
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
const KEYED: u64 = 0x006b_6579;

/// FNV-1a over little-endian words; never returns 0 (`UiId::NONE`)
fn mix(mut hash: u64, words: &[u64]) -> u64 {
  for word in words {
    for byte in word.to_le_bytes() {
      hash ^= u64::from(byte);
      hash = hash.wrapping_mul(FNV_PRIME);
    }
  }
  hash.max(1)
}

/// A call site: the static file name's address, its length, line and column
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Site([u64; 4]);

impl Site {
  fn of(location: &'static Location<'static>) -> Site {
    let file = location.file();
    Site([file.as_ptr() as u64, file.len() as u64, u64::from(location.line()), u64::from(location.column())])
  }
}

/// The scopes and per-site counts of the frame being recorded
#[derive(Debug, Default)]
pub(crate) struct IdScopes {
  stack: Vec<UiId>,
  counts: HashMap<(UiId, Site), u64>,
  used: HashSet<UiId>,
  warned: HashSet<UiId>,
}

impl IdScopes {
  pub(crate) fn begin_frame(&mut self) {
    self.stack.clear();
    self.counts.clear();
    self.used.clear();
  }

  /// Start a scene's UI: `root` is the only open scope
  pub(crate) fn enter_root(&mut self, root: UiId) {
    self.stack.clear();
    self.stack.push(root);
  }

  pub(crate) fn current(&self) -> UiId {
    self.stack.last().copied().unwrap_or(UiId::NONE)
  }

  pub(crate) fn push(&mut self, scope: UiId) {
    self.stack.push(scope);
  }

  pub(crate) fn pop(&mut self) {
    // The root stays
    if self.stack.len() > 1 {
      self.stack.pop();
    }
  }

  /// The id of a widget started at `site` in the current scope
  pub(crate) fn make(&mut self, site: &'static Location<'static>, key: Option<u64>) -> UiId {
    let parent = self.current();
    let site = Site::of(site);
    let id = match key {
      Some(key) => UiId(mix(parent.0, &[site.0[0], site.0[1], site.0[2], site.0[3], KEYED, key])),
      None => {
        let count = self.counts.entry((parent, site)).or_insert(0);
        let id = UiId(mix(parent.0, &[site.0[0], site.0[1], site.0[2], site.0[3], *count]));
        *count += 1;
        id
      }
    };
    if self.used.insert(id) {
      return id;
    }
    // Only repeated explicit keys collide; never share their state
    if self.warned.insert(id) {
      crate::engine_warn!(module: "ui", "two widgets share the id key in one scope; use distinct .id() keys");
    }
    let mut n = 1;
    loop {
      let candidate = UiId(mix(id.0, &[n]));
      if self.used.insert(candidate) {
        return candidate;
      }
      n += 1;
    }
  }
}

/// A key's hash for `.id(key)`
pub(crate) fn key_hash<K: Hash + ?Sized>(key: &K) -> u64 {
  let mut hasher = DefaultHasher::new();
  key.hash(&mut hasher);
  hasher.finish()
}
```

- [ ] **Step 4: Write `memory.rs`**

Write the module doc, then `Flags`, `Record` and `UiMemory` as in Interfaces. The flag values are powers of two in the listed order (`CLICK = 1 << 0` … `NAV_SKIP = 1 << 17`). `Flags` gets `BitOr` and `BitOrAssign` impls, and `contains(self, f) = self.0 & f.0 == f.0`.

`UiMemory`:

```rust
struct Slot {
  scene: u64,
  seen: u64,
  value: Box<dyn std::any::Any>,
}

/// What the UI remembers between frames: last frame's laid-out records,
/// widgets' own state (scroll offsets, carets, open lists) and focus
pub(crate) struct UiMemory {
  prev: HashMap<UiId, Record>,
  next: HashMap<UiId, Record>,
  states: HashMap<UiId, Slot>,
  focus_memory: HashMap<UiId, (u64, UiId)>,
  frame: u64,
  pub(crate) scopes: IdScopes,
}
```

Method semantics:
- `begin_frame` swaps `next` into `prev`, clears `next`, increments `frame` and calls `scopes.begin_frame()`.
- `end_frame` retains the states whose `seen == frame`.
- `state::<T>` inserts `T::default()` if absent or of another type, sets `seen = frame`, then returns `downcast_mut`.
- `forget_scene` removes that scene's states, focus memory and records from both `prev` and `next`.
- `current(id)` reads `next`.

In `src/core/ui/mod.rs`:
- Add `pub(crate) mod id;` and `pub(crate) mod memory;`.
- Add `pub use id::UiId;`.
- Add `UiId` to the prelude's UI line in `src/lib.rs`.
- Silence dead code narrowly with `#[cfg_attr(not(test), allow(dead_code))]` on the two modules, until Task 5 uses them.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p ryo2d --lib ui::id ui::memory`
Expected: 9 pass.

- [ ] **Step 6: Gate and commit**

```bash
git add src/core/ui/id.rs src/core/ui/memory.rs src/core/ui/mod.rs src/lib.rs
git commit -m "feat: UI widget ids and memory" -m "- ids hash the scope, the call site and its occurrence, or an explicit key
- memory keeps last frame's records, widget state and per-scope focus"
```

### Task 4: The layout engine (Taffy)

**Files:**
- Create:
  - `src/core/ui/layout/mod.rs`: the style vocabulary and the guard test.
  - `src/core/ui/layout/engine.rs`: the Taffy glue, the only file that names `taffy`.
- Modify: `Cargo.toml` (the `taffy` line from Global Constraints), `src/core/ui/mod.rs`, `src/lib.rs` (prelude: `Length`, `Edges`, `Align`, `Justify`)
- Test: both new files' tests modules

**Interfaces:**
- Consumes: `UiId` (Task 3), `Anchor` (`crate::core::ecs::Anchor`, re-exported as `ui::Anchor`), `Rect`, `glam::Vec2`.
- Produces:

```rust
// public vocabulary (prelude)
pub enum Length { Px(f32), Pct(f32), Fill, Fit }       // Debug, Clone, Copy, PartialEq; Default = Fit; From<f32> = Px
pub struct Edges { pub left: f32, pub top: f32, pub right: f32, pub bottom: f32 }   // Debug, Clone, Copy, PartialEq, Default; From<f32> = all
impl Edges { pub const ZERO: Edges; pub fn all(v: f32) -> Edges; pub fn xy(x: f32, y: f32) -> Edges; pub fn new(left: f32, top: f32, right: f32, bottom: f32) -> Edges; pub fn width(&self) -> f32; pub fn height(&self) -> f32 }
pub enum Align { Start, Center, End, Stretch }          // Default = Stretch
pub enum Justify { Start, Center, End, SpaceBetween, SpaceAround, SpaceEvenly }   // Default = Start
// crate
pub(crate) enum Flow { Column, Row, Grid(Tracks), Stack }   // Default = Column
pub(crate) enum Tracks { Equal(u16), Fixed(Vec<f32>) }
pub(crate) struct UiStyle {                              // Debug, Clone, PartialEq; Default
  pub(crate) flow: Flow,
  pub(crate) width: Length, pub(crate) height: Length,
  pub(crate) min_width: Option<f32>, pub(crate) max_width: Option<f32>,
  pub(crate) min_height: Option<f32>, pub(crate) max_height: Option<f32>,
  pub(crate) padding: Edges, pub(crate) margin: Edges, pub(crate) gap: f32,
  pub(crate) align: Align, pub(crate) justify: Justify, pub(crate) align_self: Option<Align>,
  pub(crate) grow: f32, pub(crate) wrap: bool, pub(crate) aspect: Option<f32>,
  pub(crate) scroll_x: bool, pub(crate) scroll_y: bool,
  pub(crate) anchor: Anchor,   // default TopLeft: placement in a stack or the scene root
  pub(crate) offset: Vec2,     // applied after layout, in a stack or the scene root
}
pub(crate) enum Measure { None, Fixed(Vec2), Text }   // Text: sized by the callback
pub(crate) struct NodeDesc<'s> { pub(crate) id: UiId, pub(crate) style: &'s UiStyle, pub(crate) measure: Measure, pub(crate) content_key: u64, pub(crate) children: &'s [usize] }
pub(crate) enum Avail { Definite(f32), MinContent, MaxContent }
pub(crate) struct Placed { pub(crate) rect: Rect, pub(crate) content: Vec2 }   // rect relative to node 0's origin
pub(crate) struct LayoutEngine;
impl LayoutEngine {
  pub(crate) fn new() -> LayoutEngine;
  pub(crate) fn begin_frame(&mut self);
  /// Lay out `nodes` (node 0 is the subtree's root) in `available` UI units
  pub(crate) fn layout(
    &mut self,
    nodes: &[NodeDesc<'_>],
    available: Vec2,
    measure: &mut dyn FnMut(usize, (Option<f32>, Option<f32>), (Avail, Avail)) -> Vec2,
  ) -> Vec<Placed>;
  pub(crate) fn end_frame(&mut self);   // removes nodes not laid out this frame
}
```

**Style conversion** (`fn to_taffy(style: &UiStyle, parent: Option<&Flow>, measure: &Measure) -> taffy::Style`), following the spike:

- **Display and flow:**
  - `Column` and `Row` are `Display::Flex` with `FlexDirection::Column` or `Row`.
  - `Grid(Equal(n))` is `Display::Grid` with `grid_template_columns: vec![repeat(n, vec![fr(1.0)])]`; `Grid(Fixed(ws))` uses one `length(w)` per track.
  - `Stack` is `Display::Grid` with one `fr(1.0)` column and row, and every child placed at `line(1)`/`line(1)` with `justify_self`/`align_self` from the child's `anchor.fraction()`: 0 → `START`, 0.5 → `CENTER`, 1 → `END`.
- **Sizes:**
  - `Px(v)` → `length(v)`; `Pct(p)` → `percent(p)`; `Fit` → `auto()`.
  - `Fill` depends on the parent's flow:
    - on the parent's main axis (height in a `Column`, width in a `Row`), `flex_grow: 1.0` with `flex_basis: length(0.0)` and the size `auto`;
    - on the cross axis, `align_self: STRETCH` with the size `auto`;
    - in a `Grid` or `Stack`, `justify_self`/`align_self` `STRETCH`;
    - at the subtree root, `percent(1.0)`.
  - `Measure::Fixed(s)` sets `size` to `length(s.x)` and `length(s.y)` for any axis the style leaves `Fit`.
- **Spacing and alignment:**
  - Padding, margin and gap are lengths; `min_*`/`max_*` set `min_size`/`max_size`.
  - `align` maps to `align_items`, `justify` to `justify_content`, and `align_self` to `align_self`.
  - `grow` sets `flex_grow`; `wrap` sets `flex_wrap: Wrap`; `aspect` sets `aspect_ratio`.
- **Shrinking:** every node gets `flex_shrink: 0.0`, except text leaves (`Measure::Text`), which get `flex_shrink: 1.0` and `min_size.width: length(0.0)` so they can wrap.
- **Scroll:** `scroll_x`/`scroll_y` set `overflow` to `Overflow::Scroll` on that axis and `scrollbar_width: 0.0`.
- **`anchor` and `offset`** are not Taffy's: Task 5 applies them after layout.

**Sanitising.** Before conversion, each style is checked:
- A non-finite or negative `Px`, `Pct`, min/max, padding, margin, gap or `grow` is replaced (`Fit`, `None` or 0).
- A non-finite or non-positive `aspect` becomes `None`.
- A replacement warns once per field name, with `engine_warn!(module: "ui", "a UI style has a non-finite or negative {field}; ignored")`.
- A non-finite or negative `available` axis is 0.

**The tree.**
- The `TaffyTree<UiId>` keeps one node per `UiId`, created with the id as its context: setting a context dirties a node, so the context never changes.
- Each `layout` call walks the nodes children first and syncs them:
  - `set_style` only when the converted style differs from the stored one, comparing our `UiStyle` plus the parent's `Flow`;
  - `set_children` only when the child `NodeId`s differ;
  - `mark_dirty` when `content_key` changes.
- `disable_rounding()` is called once in `new`. Task 5 snaps to render pixels itself.
- Results are accumulated from the parent-relative `location`s, with node 0 at (0, 0).
- `Placed::content` for a scroll node is `(scrollable_overflow_rect.right, scrollable_overflow_rect.bottom)`, at least the node's size. For other nodes it is the node's size.
- `end_frame` removes the entries not laid out since the last `begin_frame`, one `taffy.remove` per node. Children are listed separately and removed in their own turn, so none are orphaned.

- [ ] **Step 1: Write the failing tests**

`src/core/ui/layout/engine.rs`:

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ui::layout::{Align, Edges, Flow, Length, Measure, Tracks, UiStyle};
  use crate::core::ui::UiId;
  use glam::Vec2;

  /// The spike's text: `chars` 7-unit glyphs, 17 units a line, wrapped to the width offered
  fn measure_text(chars: f32) -> impl FnMut(usize, (Option<f32>, Option<f32>), (Avail, Avail)) -> Vec2 {
    move |_, known, avail| {
      let full = chars * 7.0;
      let width = match (known.0, avail.0) {
        (Some(w), _) => w,
        (None, Avail::Definite(w)) => w.min(full),
        (None, Avail::MinContent) => 7.0,
        (None, Avail::MaxContent) => full,
      };
      Vec2::new(width, (full / width.max(1.0)).ceil().max(1.0) * 17.0)
    }
  }

  fn node<'s>(id: u64, style: &'s UiStyle, measure: Measure, children: &'s [usize]) -> NodeDesc<'s> {
    NodeDesc { id: UiId(id), style, measure, content_key: 0, children }
  }

  #[test]
  fn a_column_stacks_children_with_gap_and_padding() {
    let column = UiStyle { padding: Edges::all(10.0), gap: 8.0, width: Length::Px(200.0), ..UiStyle::default() };
    let leaf = UiStyle::default();
    let nodes = [
      node(1, &column, Measure::None, &[1, 2]),
      node(2, &leaf, Measure::Text, &[]),
      node(3, &leaf, Measure::Fixed(Vec2::new(60.0, 20.0)), &[]),
    ];
    let mut engine = LayoutEngine::new();
    engine.begin_frame();
    let placed = engine.layout(&nodes, Vec2::new(800.0, 600.0), &mut measure_text(100.0));
    assert_eq!(placed[0].rect, Rect::new(0.0, 0.0, 200.0, 116.0));
    assert_eq!(placed[1].rect, Rect::new(10.0, 10.0, 180.0, 68.0), "4 lines of 17 at 180 wide");
    assert_eq!(placed[2].rect, Rect::new(10.0, 86.0, 60.0, 20.0), "a fixed leaf is not stretched");
  }

  #[test]
  fn fill_takes_what_a_row_has_left() {
    let row = UiStyle { flow: Flow::Row, width: Length::Px(300.0), ..UiStyle::default() };
    let fill = UiStyle { width: Length::Fill, ..UiStyle::default() };
    let leaf = UiStyle::default();
    let nodes = [
      node(1, &row, Measure::None, &[1, 2, 3]),
      node(2, &leaf, Measure::Fixed(Vec2::new(50.0, 10.0)), &[]),
      node(3, &leaf, Measure::Fixed(Vec2::new(30.0, 10.0)), &[]),
      node(4, &fill, Measure::Fixed(Vec2::new(0.0, 10.0)), &[]),
    ];
    let mut engine = LayoutEngine::new();
    engine.begin_frame();
    let placed = engine.layout(&nodes, Vec2::new(800.0, 600.0), &mut |_, _, _| Vec2::ZERO);
    assert_eq!(placed[3].rect.x, 80.0);
    assert_eq!(placed[3].rect.width, 220.0);
  }

  #[test]
  fn a_six_column_grid_of_48_unit_cells() {
    let grid = UiStyle { flow: Flow::Grid(Tracks::Fixed(vec![48.0; 6])), gap: 4.0, ..UiStyle::default() };
    let cell = UiStyle::default();
    let children: Vec<usize> = (1..=14).collect();
    let mut nodes = vec![node(1, &grid, Measure::None, &children)];
    for i in 0..14 {
      nodes.push(node(10 + i, &cell, Measure::Fixed(Vec2::new(48.0, 48.0)), &[]));
    }
    let mut engine = LayoutEngine::new();
    engine.begin_frame();
    let placed = engine.layout(&nodes, Vec2::new(800.0, 600.0), &mut |_, _, _| Vec2::ZERO);
    assert_eq!((placed[0].rect.width, placed[0].rect.height), (308.0, 152.0));
    assert_eq!((placed[7].rect.x, placed[7].rect.y), (0.0, 52.0), "cell 6 starts the second row");
    assert_eq!((placed[14].rect.x, placed[14].rect.y), (52.0, 104.0));
  }

  #[test]
  fn a_stack_overlaps_its_children_by_anchor() {
    let stack = UiStyle { flow: Flow::Stack, width: Length::Px(100.0), height: Length::Px(50.0), ..UiStyle::default() };
    let badge = UiStyle { anchor: Anchor::TopRight, ..UiStyle::default() };
    let icon = UiStyle::default();
    let nodes = [
      node(1, &stack, Measure::None, &[1, 2]),
      node(2, &icon, Measure::Fixed(Vec2::new(100.0, 50.0)), &[]),
      node(3, &badge, Measure::Fixed(Vec2::new(20.0, 20.0)), &[]),
    ];
    let mut engine = LayoutEngine::new();
    engine.begin_frame();
    let placed = engine.layout(&nodes, Vec2::new(800.0, 600.0), &mut |_, _, _| Vec2::ZERO);
    assert_eq!(placed[1].rect, Rect::new(0.0, 0.0, 100.0, 50.0));
    assert_eq!(placed[2].rect, Rect::new(80.0, 0.0, 20.0, 20.0));
  }

  #[test]
  fn a_scroll_reports_its_content() {
    let scroll = UiStyle { width: Length::Px(200.0), height: Length::Px(150.0), gap: 4.0, scroll_y: true, ..UiStyle::default() };
    let row = UiStyle::default();
    let children: Vec<usize> = (1..=10).collect();
    let mut nodes = vec![node(1, &scroll, Measure::None, &children)];
    for i in 0..10 {
      nodes.push(node(10 + i, &row, Measure::Fixed(Vec2::new(180.0, 40.0)), &[]));
    }
    let mut engine = LayoutEngine::new();
    engine.begin_frame();
    let placed = engine.layout(&nodes, Vec2::new(800.0, 600.0), &mut |_, _, _| Vec2::ZERO);
    assert_eq!(placed[0].rect.height, 150.0);
    assert_eq!(placed[0].content.y, 436.0, "10 rows of 40 and 9 gaps of 4");
    assert_eq!(placed[10].rect.height, 40.0, "scroll content does not shrink");
  }

  #[test]
  fn a_second_layout_with_nothing_changed_measures_nothing() {
    let column = UiStyle { width: Length::Px(120.0), ..UiStyle::default() };
    let leaf = UiStyle::default();
    let nodes = [node(1, &column, Measure::None, &[1]), node(2, &leaf, Measure::Text, &[])];
    let mut engine = LayoutEngine::new();
    let mut calls = 0;
    let mut text = measure_text(40.0);
    let mut counting = |i, k, a| {
      calls += 1;
      text(i, k, a)
    };
    engine.begin_frame();
    let first = engine.layout(&nodes, Vec2::new(800.0, 600.0), &mut counting);
    let after_first = calls;
    engine.end_frame();
    engine.begin_frame();
    let second = engine.layout(&nodes, Vec2::new(800.0, 600.0), &mut counting);
    assert!(after_first > 0);
    assert_eq!(calls, after_first, "cached");
    assert_eq!(first[1].rect, second[1].rect);
    assert!(second[1].rect.height > 17.0, "wrapped to the 120-wide column");
  }

  #[test]
  fn nodes_not_laid_out_are_removed_at_frame_end() {
    let leaf = UiStyle::default();
    let mut engine = LayoutEngine::new();
    engine.begin_frame();
    engine.layout(&[node(1, &leaf, Measure::Fixed(Vec2::ONE), &[])], Vec2::splat(10.0), &mut |_, _, _| Vec2::ZERO);
    engine.end_frame();
    assert_eq!(engine.node_count(), 1);
    engine.begin_frame();
    engine.end_frame();
    assert_eq!(engine.node_count(), 0);
  }

  #[test]
  fn degenerate_styles_lay_out_finite_rects() {
    let bad = UiStyle {
      width: Length::Px(f32::NAN),
      height: Length::Pct(-1.0),
      gap: -5.0,
      padding: Edges::all(f32::INFINITY),
      min_width: Some(f32::NAN),
      aspect: Some(0.0),
      grow: f32::NEG_INFINITY,
      ..UiStyle::default()
    };
    let leaf = UiStyle::default();
    let nodes = [node(1, &bad, Measure::None, &[1]), node(2, &leaf, Measure::Fixed(Vec2::new(5.0, 5.0)), &[])];
    let mut engine = LayoutEngine::new();
    for available in [Vec2::new(100.0, 100.0), Vec2::ZERO, Vec2::new(f32::NAN, -3.0)] {
      engine.begin_frame();
      let placed = engine.layout(&nodes, available, &mut |_, _, _| Vec2::ZERO);
      for p in &placed {
        assert!(p.rect.x.is_finite() && p.rect.y.is_finite() && p.rect.width.is_finite() && p.rect.height.is_finite(), "{p:?}");
      }
      engine.end_frame();
    }
    let warnings = crate::core::dev_log::get_entries().iter().filter(|e| e.message.contains("non-finite or negative")).count();
    assert!((1..=7).contains(&warnings), "each bad field warned once, got {warnings}");
  }

  #[test]
  fn deep_nesting_lays_out() {
    let column = UiStyle::default();
    let children: Vec<Vec<usize>> = (0..64).map(|i| if i < 63 { vec![i + 1] } else { vec![] }).collect();
    let nodes: Vec<NodeDesc<'_>> = (0..64)
      .map(|i| node(100 + i as u64, &column, if i == 63 { Measure::Fixed(Vec2::new(10.0, 10.0)) } else { Measure::None }, &children[i]))
      .collect();
    let mut engine = LayoutEngine::new();
    engine.begin_frame();
    let placed = engine.layout(&nodes, Vec2::new(800.0, 600.0), &mut |_, _, _| Vec2::ZERO);
    assert_eq!(placed[63].rect, Rect::new(0.0, 0.0, 10.0, 10.0));
    let _ = Align::Stretch;
  }
}
```

`node_count(&self) -> usize` is a `#[cfg(test)]` helper on `LayoutEngine` that returns the number of stored entries.

In `src/core/ui/layout/mod.rs`, add the guard test, a copy of the text module's guard. It scans `src/` for `taffy::` outside `src/core/ui/layout/`, asserts `scanned > 20`, and is named `taffy_is_named_only_in_the_layout_module`.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p ryo2d --lib ui::layout`
Expected: compile errors (the module and the `taffy` crate are missing).

- [ ] **Step 3: Write `layout/mod.rs`**

Write the types from Interfaces, with doc comments on every public item:
- **Defaults:** `UiStyle::default()` is a `Column` of `Fit` × `Fit`, `align: Stretch`, `justify: Start`, `anchor: Anchor::TopLeft`, everything else zero, `None` or `false`.
- **`Length`:** implement `From<f32> for Length` (`Px`).
- **`Edges`:** implement `From<f32> for Edges` (`all`) and `From<(f32, f32)> for Edges` (`xy`).
- **Re-exports:** `pub use` the four public types from `ui/mod.rs`, and add them to the prelude.
- **Module declarations:** `pub(crate) mod engine;` here, and `pub(crate) mod layout;` in `ui/mod.rs`. The crate items get `#[cfg_attr(not(test), allow(dead_code))]` until Task 5 uses them.

- [ ] **Step 4: Write `layout/engine.rs`**

```rust
//! The layout engine: Taffy flexbox and grid over the UI's nodes, kept
//! between frames by widget id. The only file that names Taffy.

use std::collections::{HashMap, HashSet};

use glam::Vec2;
use taffy::prelude::*;
use taffy::{Overflow, Point, compute_leaf_layout};

use super::{Align, Flow, Justify, Length, Measure, Tracks, UiStyle};
use crate::core::ui::{Rect, UiId};

struct Entry {
  node: NodeId,
  style: UiStyle,
  parent_flow: Option<Flow>,
  children: Vec<NodeId>,
  content_key: u64,
  frame: u64,
}

/// Flexbox and grid layout over UI nodes, cached by widget id
pub(crate) struct LayoutEngine {
  taffy: TaffyTree<UiId>,
  entries: HashMap<UiId, Entry>,
  frame: u64,
  warned: HashSet<&'static str>,
}
```

- `layout` builds `index_of: HashMap<UiId, usize>` and `parents: Vec<Option<usize>>` from the descs.
- It syncs the nodes in post-order (children first) by the rules above. A node whose id is not yet stored is created with `new_leaf_with_context(style, id)`, then `set_children` when it has children.
- It calls `self.taffy.compute_layout_with_measure(root, Size { width: AvailableSpace::Definite(w), height: AvailableSpace::Definite(h) }, |inputs, _node, ctx, style| ...)`. The closure wraps `compute_leaf_layout(inputs, style, |_, _| 0.0, |known, avail| ...)`, which maps the context id to the desc index through `index_of` and calls `measure` only for `Measure::Text` (`Size::ZERO` otherwise), converting `AvailableSpace` to `Avail`.
- It reads `self.taffy.layout(node)` for each desc, accumulating absolute positions from the root.
- An error from Taffy (a `TaffyResult`) is warned once (`engine_warn!(module: "ui", "layout failed: {e}")`) and yields zero rects. It never panics.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p ryo2d --lib ui::layout`
Expected: 10 pass.
Run: `cargo build -p ryo2d --target wasm32-unknown-unknown`
Expected: builds; the spike built Taffy for wasm unchanged.

- [ ] **Step 6: Gate and commit**

```bash
git add Cargo.toml Cargo.lock src/core/ui/layout/mod.rs src/core/ui/layout/engine.rs src/core/ui/mod.rs src/lib.rs
git commit -m "feat: UI layout on Taffy flexbox and grid" -m "- columns, rows, grids and stacks with fill, fit, gaps, padding and alignment
- the tree is kept between frames by widget id; unchanged nodes are not measured again
- bad style values are replaced and warned once"
```

### Task 5: The frame pipeline: subtrees, containers, `ui.text` as a leaf, scenes

**Files:**
- Create:
  - `src/core/ui/tree.rs`: the subtree being built, its draws, and its resolution into commands and records.
  - `src/core/ui/containers.rs`: the `column`, `row`, `grid`, `stack` and `panel` builders.
  - `src/core/ui/response.rs`: `Response` with `id` and `rect` (Task 6 adds the interaction states).
- Modify:
  - `src/core/ui/immediate/context.rs`: memory, layout, subtree and scene fields; absolute draws take an order; `UiText` becomes a leaf and runs on drop.
  - `src/core/ui/immediate/commands.rs`: commands carry an `order`.
  - `src/core/render/ui.rs`: `ui_z(layer, order)`; `snapped` becomes `pub(crate) fn snap_rect`.
  - `src/core/engine/mod.rs`, `src/core/engine/frame.rs`: `UiMemory` and `LayoutEngine` fields, and frame bracketing.
  - `src/core/scene/manager.rs`: `begin_scene`/`end_scene`; exited scopes.
  - `src/core/ui/test_util.rs`, `src/core/ui/mod.rs`, `src/lib.rs` (prelude: `Response`).
- Test: `src/core/ui/containers.rs` and `src/core/ui/tree.rs` (tests modules), `tests/e2e/ui_drawing.rs`

**Interfaces:**
- Consumes: `UiId`, `IdScopes`, `UiMemory`, `Record`, `Flags` (Task 3); `LayoutEngine`, `UiStyle`, `Flow`, `Tracks`, `Measure`, `NodeDesc`, `Avail`, `Placed`, `Length`, `Edges`, `Align`, `Justify` (Task 4); `TextEngine::{layout, measure}`; `UiCommand`; `UiSpace`.
- Produces:

```rust
pub struct Response { pub id: UiId, pub rect: Rect }          // Debug, Clone, Default; Task 6 extends it
// containers (each #[track_caller] on UiContext; builders are #[must_use])
pub fn column<'u>(&'u mut self) -> Container<'u, 'a>;
pub fn row<'u>(&'u mut self) -> Container<'u, 'a>;
pub fn grid<'u>(&'u mut self, columns: u16) -> Container<'u, 'a>;
pub fn stack<'u>(&'u mut self) -> Container<'u, 'a>;
pub fn panel<'u>(&'u mut self) -> Container<'u, 'a>;      // a column with the panel look and padding; blocks the pointer
impl Container<'_, '_> {
  // layout
  pub fn width(self, w: impl Into<Length>) -> Self;  pub fn height(self, h: impl Into<Length>) -> Self;
  pub fn fill_width(self) -> Self;  pub fn fill_height(self) -> Self;  pub fn fill(self) -> Self;
  pub fn min_width(self, w: f32) -> Self;  pub fn max_width(self, w: f32) -> Self;  pub fn min_height(self, h: f32) -> Self;  pub fn max_height(self, h: f32) -> Self;
  pub fn padding(self, p: impl Into<Edges>) -> Self;  pub fn margin(self, m: impl Into<Edges>) -> Self;  pub fn gap(self, g: f32) -> Self;
  pub fn align(self, a: Align) -> Self;  pub fn justify(self, j: Justify) -> Self;  pub fn center(self) -> Self;   // align + justify Center
  pub fn grow(self, g: f32) -> Self;  pub fn wrap_items(self) -> Self;  pub fn aspect(self, ratio: f32) -> Self;
  pub fn tracks(self, widths: &[f32]) -> Self;   // grid with explicit column widths
  pub fn anchor(self, a: Anchor) -> Self;  pub fn offset(self, x: f32, y: f32) -> Self;
  pub fn at(self, rect: Rect) -> Self;
  // identity and behaviour
  pub fn id(self, key: impl std::hash::Hash) -> Self;
  pub fn focus_scope(self) -> Self;  pub fn trap(self) -> Self;  pub fn wrap_focus(self) -> Self;
  pub fn block_pointer(self) -> Self;
  pub fn show(self, contents: impl FnOnce(&mut UiContext<'a>)) -> Response;
}
// UiContext (public)
/// Run `contents` with ids keyed by `key` at this call site: a game's own component salts its widgets' ids.
/// Adds no node, so layout is unaffected. #[track_caller]
pub fn scope<R>(&mut self, key: impl std::hash::Hash, contents: impl FnOnce(&mut UiContext<'a>) -> R) -> R;
// UiContext (crate)
pub(crate) fn new(commands: &'a mut UiCommands, text: &'a mut TextEngine, memory: &'a mut UiMemory, layout: &'a mut LayoutEngine, space: UiSpace, input: UiInput) -> UiContext<'a>;
pub(crate) fn begin_scene(&mut self, scope: u64, top: bool);
pub(crate) fn end_scene(&mut self);
pub(crate) fn next_order(&mut self) -> u32;
pub(crate) fn scene(&self) -> (u64, bool);
// tree.rs (crate)
pub(crate) struct Subtree { pub(crate) nodes: Vec<Node>, pub(crate) draws: Vec<Draw>, pub(crate) at: Option<Rect>, pub(crate) clip: Option<Rect> }
pub(crate) struct Node { pub(crate) id: UiId, pub(crate) parent: Option<usize>, pub(crate) style: UiStyle, pub(crate) measure: Measure,
  pub(crate) text: Option<TextLeaf>, pub(crate) content_key: u64, pub(crate) children: Vec<usize>,
  pub(crate) record: Option<RecordSpec>, pub(crate) layer: u8, pub(crate) order: u32, pub(crate) depth: u16,
  pub(crate) scope: UiId, pub(crate) scroll: UiId, pub(crate) clips: bool }
pub(crate) struct RecordSpec { pub(crate) flags: Flags, pub(crate) nav: [UiId; 4] }
pub(crate) struct TextLeaf { pub(crate) text: RichText, pub(crate) style: TextStyle, pub(crate) options: TextLayoutOptions }
pub(crate) struct Draw { pub(crate) order: u32, pub(crate) layer: u8, pub(crate) node: usize, pub(crate) kind: DrawKind }
pub(crate) enum DrawKind {
  Fill { inset: Edges, fill: Color, radius: f32, border: Option<(f32, Color)> },
  Box { inset: Edges, frac: Vec2, size: Vec2, fill: Color, radius: f32, border: Option<(f32, Color)> },
  Text { leaf: TextLeaf, frac: Vec2, wrap: bool, opacity: f32, reveal: Option<(f32, f32)>, seed: u64 },
  Sprite { inset: Edges, sprite: SpriteHandle, uv: [f32; 4], tint: Color },
  Local(UiCommand),   // geometry in node-local units, translated after layout
}
pub(crate) struct Resolved { pub(crate) rects: Vec<Rect>, pub(crate) content: Vec<Vec2> }
// UiCommands
pub(crate) fn push(&mut self, order: u32, command: UiCommand);
pub(crate) fn iter(&self) -> impl Iterator<Item = (u32, &UiCommand)>;
// SceneManager
pub(crate) fn take_exited(&mut self) -> Vec<u64>;
// UiMemory (adds)
pub(crate) fn restart_pass(&mut self);    // discard this frame's records so far (a rerun UI pass)
```

**How a frame's UI is built:**
- **Builder ids.** Every builder captures `std::panic::Location::caller()` in its `#[track_caller]` constructor. When it runs (on `show` or on drop), it calls `ui.memory.scopes.make(site, key)`.
- **`ui.scope(key, contents)`** makes an id from its call site and `key` (as `.id(key)` does), pushes it as the current id scope while `contents` runs, and pops it.
- **Opening a container** pushes a `Node` onto the open subtree. If no subtree is open, it starts one, remembering `at` and the current clip. The container's id becomes the current id scope while `contents` runs, then is popped.
- **Closing a container** that has no open parent finishes the subtree (below). A leaf widget recorded while no container is open is its own one-node subtree, finished at once.
- **Order.** Every draw takes `ui.next_order()` when it is recorded, inside a subtree or not. The commands' z is `ui_z(layer, order)`, so draws keep submission order however late their geometry is resolved. `UiCommands::push(order, command)` stores the pair, and `emit_ui` uses the stored order in place of the index.
- **Absolute draws** (`draw_rect`, `draw_text*`, `draw_sprite*`, `draw_line`, `draw_nine_slice`, and `ui.text(..).at(..)`) are recorded at once with their own geometry, as now.

**Finishing a subtree** (`fn finish_subtree(&mut self, subtree: Subtree)`):
1. **Lay out.** Build the `NodeDesc`s and call `self.layout.layout(&descs, available, measure)`.
   - `available` is the UI size, or the `at` rect's size, which is also forced onto the root's style as `Px` width and height.
   - The measure closure handles text nodes: it measures with `self.text.measure(&leaf.text, &leaf.style, &options, px_scale)`.
     - The wrap width is the definite width offered (`options.wrap(w)`).
     - `MaxContent` measures without a wrap.
     - `MinContent` measures with `wrap(0.0)`, which gives the longest word.
   - `content_key` is the hash of the leaf's text, style and options, from 4a's `text_seed`, so changed text re-measures.
2. **Place the root.** At `at.min` when `at` is set, otherwise at `root.style.anchor.fraction() * (ui_size - root_size) + root.style.offset`.
3. **Absolute rects.** Walk the nodes from the root:
   - A node's absolute origin is its parent's absolute origin plus its placed offset.
   - A child of a `Stack` also adds its own `offset`.
   - A child of a scroll node subtracts that node's scroll offset, read from `memory.peek_state::<ScrollState>(scroll_id)` (zero until Task 7).
   - Then `snap_rect(rect, px_scale)`.
4. **Clips.** The root's clip is the subtree's starting clip. A child's clip is its parent's clip intersected with the parent's rect when the parent `clips` (scroll nodes, Task 7).
5. **Records.** For each node with a `RecordSpec`, write `memory.record(id, Record { rect, clip, layer, order, depth, scene, flags, scope, scroll, content, nav })` using the current scene's scope id.
6. **Draws.** Turn each draw into a `UiCommand` with the node's clip and the draw's layer, pushed with the draw's order:
   - **`Fill`**: the node rect shrunk by `inset`.
   - **`Box`**: `size` at `inset_rect.min + frac * (inset_rect.size - size)`.
   - **`Text`**: lay out with `options.wrap(content_width)` when `wrap`, else as is. `content_width` is the node width minus its padding. Then place at `content.min + frac * (content.size - text_size)`.
   - **`Sprite`**: the inset rect.
   - **`Local`**: the command's rects, origins and points translated by the node's origin.

**Engine and scenes:**
- `Engine` gains `ui_memory: UiMemory` and `ui_layout: LayoutEngine`.
- In `draw_ui`:
  1. Drain `scenes.take_exited()` into `ui_memory.forget_scene`.
  2. On the frame's first pass, call `ui_memory.begin_frame()` and `ui_layout.begin_frame()`. On a rerun pass, call `ui_memory.restart_pass()` instead. Track this with an `ui_pass_index` reset in `frame()`.
  3. Build the context with the memory and the layout, and call `begin_scene(0, false)` before the overlays and the console.
  4. After the pass, call `ui_memory.end_frame()` and `ui_layout.end_frame()` once per frame, after the last pass.
- `SceneManager::ui_pass` calls `ui.begin_scene(slot.scope_id, i == top)` before each scene's `ui`, and `ui.end_scene()` after it, before the existing `finish_dropdowns*` calls (Task 12 folds those in).
- `pop`, `switch`, `clear` and `reset` push the exiting slot's `scope_id` onto `exited`.
- `end_scene` finishes any subtree left open (a builder that panicked or was leaked), warns once, and restores `scopes.enter_root(..)`.

**`ui.text(..)`:**
- **Absolute or leaf.** It stays absolute when `.at(..)` was called. Otherwise it becomes a text leaf: `Measure::Text`, `UiStyle { width: Fit, height: Fit }`, and a `Text` draw with `frac = (align, 0)` (Start 0, Center 0.5, End 1) and `wrap: true`.
- **Size.** `draw()` returns the size: the laid-out size for a leaf at top level, and its unwrapped measured size inside a container.
- **Drop.** `UiText` now draws on drop if `draw()` was not called. The `#[must_use]` attribute goes.

**`test_util::context`** keeps its signature. It leaks a fresh `UiMemory` and `LayoutEngine` per call (tests only: `Box::leak(Box::new(UiMemory::new()))`) and calls `begin_scene(1, true)`.

**Panel look.**
- Until the theme (Task 9), `panel()` draws a `Fill` with the old panel defaults: fill `Color::new(20, 20, 30, 200)`, border `(1.0, Color::rgb(80, 80, 100))`, radius 4.
- Its padding is 8.
- It records `Flags::BLOCKING`. `.focus_scope()` adds `FOCUS_SCOPE` (on any container), `.trap()` adds `TRAP` and `.wrap_focus()` adds `WRAP`.

- [ ] **Step 1: Write the failing tests**

`src/core/ui/containers.rs`:

```rust
#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use super::*;
  use crate::core::ui::Anchor;
  use crate::core::ui::immediate::{UiCommand, UiCommands};
  use crate::core::ui::test_util::{context, text_engine};

  fn texts(commands: &UiCommands) -> Vec<(u32, Vec2)> {
    commands
      .iter()
      .filter_map(|(order, c)| match c {
        UiCommand::Text { origin, .. } => Some((order, *origin)),
        _ => None,
      })
      .collect()
  }

  #[test]
  fn a_column_places_texts_top_to_bottom_with_its_gap() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text, (400.0, 300.0));
    let line = ui.line_height(&TextStyle::default());
    ui.column().gap(10.0).show(|ui| {
      ui.text("first");
      ui.text("second");
    });
    drop(ui);
    let t = texts(&commands);
    assert_eq!(t.len(), 2);
    assert_eq!(t[0].1, Vec2::ZERO);
    assert!((t[1].1.y - (line + 10.0)).abs() < 1.0, "{t:?}");
  }

  #[test]
  fn a_centred_panel_sits_in_the_middle() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text, (400.0, 300.0));
    let response = ui.panel().width(100.0).height(40.0).anchor(Anchor::Center).show(|_| {});
    drop(ui);
    assert_eq!(response.rect, Rect::new(150.0, 130.0, 100.0, 40.0));
    let rect = commands.iter().find_map(|(_, c)| match c {
      UiCommand::Rect { rect, .. } => Some(*rect),
      _ => None,
    });
    assert_eq!(rect, Some(Rect::new(150.0, 130.0, 100.0, 40.0)));
  }

  #[test]
  fn an_offset_moves_an_anchored_block() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text, (400.0, 300.0));
    let response = ui.panel().width(50.0).height(20.0).anchor(Anchor::TopRight).offset(-16.0, 16.0).show(|_| {});
    assert_eq!(response.rect, Rect::new(334.0, 16.0, 50.0, 20.0));
  }

  #[test]
  fn a_grid_places_cells_in_rows() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text, (400.0, 300.0));
    let mut ids = Vec::new();
    ui.grid(3).tracks(&[40.0, 40.0, 40.0]).gap(5.0).show(|ui| {
      for i in 0..5 {
        ids.push(ui.panel().width(40.0).height(30.0).id(i).show(|_| {}).id);
      }
    });
    // Inside a container a response carries last frame's rect; this frame's
    // records are written when the grid closes
    let rect = |id| ui.memory_for_test().current(id).map(|r| r.rect).unwrap();
    assert_eq!(rect(ids[0]), Rect::new(0.0, 0.0, 40.0, 30.0));
    assert_eq!(rect(ids[1]), Rect::new(45.0, 0.0, 40.0, 30.0));
    assert_eq!(rect(ids[2]), Rect::new(90.0, 0.0, 40.0, 30.0));
    assert_eq!(rect(ids[3]), Rect::new(0.0, 35.0, 40.0, 30.0));
  }

  #[test]
  fn draws_keep_submission_order_across_absolute_draws() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text, (400.0, 300.0));
    ui.column().show(|ui| {
      ui.text("a");
      ui.draw_rect(Rect::new(0.0, 0.0, 5.0, 5.0), Color::RED);
      ui.text("b");
    });
    drop(ui);
    let mut kinds: Vec<(u32, &'static str)> = commands.iter().map(|(order, c)| (order, c.kind_name())).collect();
    kinds.sort();
    assert_eq!(kinds.iter().map(|k| k.1).collect::<Vec<_>>(), vec!["text", "rect", "text"]);
  }

  #[test]
  fn text_in_a_narrow_column_wraps() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text, (400.0, 300.0));
    ui.column().width(60.0).show(|ui| {
      ui.text("several words that cannot fit on one line");
    });
    drop(ui);
    let lines = commands.iter().find_map(|(_, c)| match c {
      UiCommand::Text { layout, .. } => Some(layout.lines.len()),
      _ => None,
    });
    assert!(lines.unwrap_or(0) > 2);
  }

  #[test]
  fn an_at_rect_container_is_placed_there() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text, (400.0, 300.0));
    let response = ui.panel().at(Rect::new(10.0, 20.0, 80.0, 60.0)).show(|_| {});
    assert_eq!(response.rect, Rect::new(10.0, 20.0, 80.0, 60.0));
  }

  #[test]
  fn panels_block_and_scopes_are_explicit() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text, (400.0, 300.0));
    let panel = ui.panel().width(10.0).height(10.0).show(|_| {});
    let menu = ui.column().focus_scope().trap().show(|_| {});
    let record = |id| ui.memory_for_test().current(id).copied().unwrap();
    assert!(record(panel.id).flags.contains(Flags::BLOCKING));
    assert!(!record(panel.id).flags.contains(Flags::FOCUS_SCOPE), "panels are not scopes unless marked");
    assert!(record(menu.id).flags.contains(Flags::FOCUS_SCOPE | Flags::TRAP));
  }

  #[test]
  fn a_scope_keys_the_ids_inside_it() {
    fn panel_in(ui: &mut UiContext<'_>, key: &str) -> crate::core::ui::UiId {
      ui.scope(key, |ui| ui.panel().show(|_| {}).id)
    }
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text, (400.0, 300.0));
    let (a, b) = (panel_in(&mut ui, "a"), panel_in(&mut ui, "b"));
    drop(ui);
    let mut ui = context(&mut commands, &mut text, (400.0, 300.0));
    let (b2, a2) = (panel_in(&mut ui, "b"), panel_in(&mut ui, "a"));
    assert_ne!(a, b);
    assert_eq!((a, b), (a2, b2), "the key, not the call order, names the scope");
  }

  #[test]
  fn the_same_code_in_two_scenes_gets_two_ids() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text, (400.0, 300.0));
    let mut ids = Vec::new();
    for scene in [1, 2] {
      ui.begin_scene(scene, scene == 2);
      ids.push(ui.panel().show(|_| {}).id);
      ui.end_scene();
    }
    assert_ne!(ids[0], ids[1]);
  }

  #[test]
  fn rects_snap_to_render_pixels_at_fractional_scale() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = crate::core::ui::test_util::context_scaled(&mut commands, &mut text, (400.0, 300.0), 1.25);
    let rect = ui.panel().width(33.3).height(10.1).offset(0.3, 0.3).show(|_| {}).rect;
    for edge in [rect.x, rect.y, rect.right(), rect.bottom()] {
      let px = edge * 1.25;
      assert!((px - px.round()).abs() < 1e-3, "{edge} is not on a render pixel");
    }
  }
}
```

The test helpers:
- `memory_for_test(&self) -> &UiMemory` is a `#[cfg(test)]` accessor on `UiContext`.
- `test_util::context_scaled(commands, text, size, px_scale)` is `context` with `px_scale` and `image = size * px_scale`.


In `tests/e2e/ui_drawing.rs`:

```rust
#[test]
fn a_centred_panel_draws_in_the_middle() {
  let scene = Ui {
    draw: |ui: &mut UiContext| {
      ui.panel().width(40.0).height(30.0).anchor(Anchor::Center).show(|_| {});
    },
    world: None,
  };
  let Some(frame) = frame(None, |b| b, scene) else {
    return;
  };
  // 128x96: the panel covers 44..84 x 33..63
  let centre = frame.get_pixel(64, 48).0;
  assert!(centre[0] > 0 || centre[1] > 0 || centre[2] > 0, "panel fill at the centre: {centre:?}");
  assert_eq!(frame.get_pixel(10, 10).0, [0, 0, 0, 255], "black outside");
  assert_eq!(frame.get_pixel(40, 48).0, [0, 0, 0, 255], "left of the panel");
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p ryo2d --lib ui::containers` and `cargo test --test e2e a_centred_panel_draws_in_the_middle`
Expected: compile errors.

- [ ] **Step 3: Implement**

Write `tree.rs`, `containers.rs` and `response.rs` with the types from Interfaces, and the `UiContext` changes. The container builder:

```rust
/// A container being set up: `.show(|ui| ..)` lays its contents out
#[must_use = "call .show(|ui| ..) to draw the container"]
pub struct Container<'u, 'a> {
  ui: &'u mut UiContext<'a>,
  site: &'static Location<'static>,
  kind: ContainerKind,
  style: UiStyle,
  key: Option<u64>,
  flags: Flags,
  at: Option<Rect>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContainerKind {
  Plain,
  Panel,
}

impl<'u, 'a> Container<'u, 'a> {
  pub fn show(self, contents: impl FnOnce(&mut UiContext<'a>)) -> Response {
    let Container { ui, site, kind, style, key, flags, at } = self;
    let id = ui.memory.scopes.make(site, key);
    let node = ui.open_node(id, style, at, flags, kind == ContainerKind::Panel);
    if kind == ContainerKind::Panel {
      ui.panel_fill(node);
    }
    ui.memory.scopes.push(id);
    contents(ui);
    ui.memory.scopes.pop();
    ui.close_node(node)
  }
}
```

`open_node`, `close_node` and `panel_fill` are crate-private `UiContext` methods:
- `open_node` returns the node's index in the open subtree, starting one if needed.
- `close_node` finishes the subtree when the node is its root, and returns the node's `Response`. This frame's rect is in `memory.current(id)` when the subtree is finished, otherwise last frame's from `memory.prev(id)`. Containers always record when they have flags; plain containers record a `Record` with empty flags so their rect is available.
- `panel_fill` pushes the `Fill` draw.

Write the `finish_subtree` steps as specified. `snap_rect` is the old `snapped` helper from `render/ui.rs`, made `pub(crate)` and applied to every node (not only in pixel-art).

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ryo2d --lib ui::` and `cargo test --test e2e ui_drawing`
Expected: pass. Every existing UI test still passes; absolute draws are unchanged apart from the explicit order.
Run: `cargo test --workspace`
Expected: pass.

- [ ] **Step 5: Docs**

In `src/core/ui/AGENTS.md`, add a section "Layout and ids":
- Ids come from call sites, scopes and occurrences, with `.id(key)` for lists.
- Each top-level container lays out when its `.show()` returns.
- Draws keep submission order through `order`.
- Records are written for next frame's hit tests.
- Scenes are root scopes.

- [ ] **Step 6: Gate and commit**

```bash
git add src/core/ui/tree.rs src/core/ui/containers.rs src/core/ui/response.rs src/core/ui/immediate/context.rs src/core/ui/immediate/commands.rs src/core/ui/immediate/mod.rs src/core/ui/mod.rs src/core/ui/test_util.rs src/core/ui/AGENTS.md src/core/render/ui.rs src/core/engine/mod.rs src/core/engine/frame.rs src/core/scene/manager.rs src/lib.rs tests/e2e/ui_drawing.rs
git commit -m "feat: UI containers laid out by Taffy" -m "- column, row, grid, stack and panel containers that size to their content
- each top-level container lays out when it closes; draws keep submission order
- ui.text joins layouts as a leaf; scenes are root id scopes"
```

### Task 6: Interaction, responses, custom widgets and the button

**Files:**
- Create:
  - `src/core/ui/interact.rs`: the pointer resolve.
  - `src/core/ui/widgets/mod.rs`
  - `src/core/ui/widgets/common.rs`: `WidgetOpts`, `Sense`, and the `widget_builders!` macro.
  - `src/core/ui/widgets/button.rs`
  - `src/core/ui/widgets/custom.rs`: `ui.custom` and `Painter`.
- Modify:
  - `src/core/ui/response.rs`: the states.
  - `src/core/ui/memory.rs`: `Interaction`.
  - `src/core/ui/immediate/context.rs`: `response`, `wants_pointer`, and the removal of the old `button`.
  - `src/core/ui/test_util.rs`: `UiDriver`.
  - `src/core/engine/frame.rs`: resolve before the scenes, consumption, and the console as a blocker.
  - `src/core/scene/manager.rs`: tests.
  - Every `ui.button(` call site, ported mechanically:
    - games: `examples/games/breakout/src/scenes/{menu,pause,quit_confirm}.rs`, `examples/games/flappy/src/scenes/{menu,game_over}.rs`, `examples/games/leap-king/src/scenes/{menu,game}.rs`;
    - demos: `examples/demos/networking/src/scenes/menu.rs`, `examples/demos/asset-loading/main.rs`, `examples/demos/ui/src/scenes/{theme_demo,layout_demo,clipping_demo}.rs`;
    - tests: `examples/demo-kit/tests/shell.rs`, `tests/e2e/ui_drawing.rs`, and the in-crate tests that call `button`.
  - `src/lib.rs`: prelude gains `Sense`, `Painter`.
- Test: `src/core/ui/interact.rs`, `src/core/ui/widgets/button.rs`, `src/core/ui/widgets/custom.rs` (tests modules); `src/core/engine/frame.rs` tests

**Interfaces:**
- Consumes: Task 5's subtree API (`open_node`, `close_node`, draws, `Response { id, rect }`), `UiMemory`, `Record`, `Flags`, `UiInput`.
- Produces:

```rust
// response.rs
pub struct Response { pub id: UiId, pub rect: Rect, /* pub(crate) state bits, drag_delta, pointer */ }
impl Response {
  pub fn hovered(&self) -> bool; pub fn pressed(&self) -> bool; pub fn clicked(&self) -> bool;
  pub fn secondary_clicked(&self) -> bool; pub fn double_clicked(&self) -> bool;
  pub fn drag_started(&self) -> bool; pub fn dragging(&self) -> bool; pub fn drag_released(&self) -> bool;
  pub fn drag_delta(&self) -> Vec2; pub fn changed(&self) -> bool;
  pub fn focused(&self) -> bool; pub fn gained_focus(&self) -> bool; pub fn lost_focus(&self) -> bool;
  pub fn submitted(&self) -> bool;
  /// The pointer in UI units while this widget is hot or active
  pub fn pointer(&self) -> Option<Vec2>;
  pub(crate) fn mark_changed(&mut self);
  pub(crate) fn mark_submitted(&mut self);
}
pub struct Sense { /* click, drag */ }   // Debug, Clone, Copy, PartialEq, Eq
impl Sense { pub fn hover() -> Sense; pub fn click() -> Sense; pub fn drag() -> Sense; pub fn click_and_drag() -> Sense }
// memory.rs
pub(crate) struct Interaction { pub(crate) hot: UiId, pub(crate) active: UiId, pub(crate) active_button: u8,
  pub(crate) press_pos: Vec2, pub(crate) pointer: Vec2, pub(crate) prev_pointer: Vec2, pub(crate) dragging: bool,
  pub(crate) clicked: UiId, pub(crate) secondary: UiId, pub(crate) double: UiId, pub(crate) last_click: (UiId, f64),
  pub(crate) drag_started: UiId, pub(crate) drag_released: UiId, pub(crate) over_ui: bool }
UiMemory::interact: Interaction   // pub(crate) field
// interact.rs
pub(crate) struct PointerInput { pub(crate) position: Vec2, pub(crate) pressed: [bool; 2], pub(crate) released: [bool; 2], pub(crate) down: [bool; 2], pub(crate) time: f64 }
pub(crate) struct PointerOutcome { pub(crate) consume_press: [bool; 2] }
pub(crate) fn resolve_pointer(memory: &mut UiMemory, pointer: &PointerInput, top_scene: Option<u64>, blockers: &[Rect], drag_threshold: f32) -> PointerOutcome;
// widgets
pub fn button<'u>(&'u mut self, label: impl Into<RichText>) -> Button<'u, 'a>;      // #[track_caller]
impl Button<'_, '_> { /* widget_builders! */ pub fn icon(self, sprite: &SpriteHandle) -> Self; pub fn show(self) -> Response; pub fn clicked(self) -> bool }
pub fn custom<'u>(&'u mut self, width: f32, height: f32) -> Custom<'u, 'a>;         // #[track_caller]
impl Custom<'_, '_> { pub fn sense(self, sense: Sense) -> Self; pub fn paint(self, paint: impl FnOnce(&mut Painter<'_>, &Response)) -> Response; pub fn show(self) -> Response }
pub struct Painter<'p>;    // draws in the widget's own units: (0, 0) is its top-left
impl Painter<'_> {
  pub fn size(&self) -> Vec2;
  pub fn rect(&mut self, rect: Rect, color: Color);
  pub fn rounded_rect(&mut self, rect: Rect, radius: f32, color: Color);
  pub fn bordered_rect(&mut self, rect: Rect, fill: Color, radius: f32, border_width: f32, border_color: Color);
  pub fn text(&mut self, text: impl Into<RichText>, x: f32, y: f32, style: &TextStyle);
  pub fn sprite(&mut self, sprite: &SpriteHandle, rect: Rect, tint: Color);
  pub fn line(&mut self, points: &[Vec2], width: f32, color: Color);
}
// UiContext
pub fn wants_pointer(&self) -> bool;
pub(crate) fn response(&mut self, id: UiId, sense: Sense, disabled: bool) -> Response;
// common.rs
pub(crate) struct WidgetOpts { pub(crate) key: Option<u64>, pub(crate) disabled: bool, pub(crate) style: UiStyle, pub(crate) at: Option<Rect> }
macro_rules! widget_builders   // id, enabled, at, width, height, fill_width, margin, anchor, offset, grow, min_width, max_width
```

**The resolve step** (`resolve_pointer`) runs once per engine frame before any scene's `ui`, with the pointer in UI units (`mouse * mouse_scale`) and the top scene's scope id:

1. **Hit test.** The candidates are last frame's records of the top scene whose `rect` contains the pointer, are inside their `clip`, and whose flags intersect `CLICK | DRAG | BLOCKING | SCROLLABLE | DROP_TARGET`. Order them by `layer`, then `order`, highest first. The first one decides:
   - if it senses (`CLICK` or `DRAG`) and is not `DISABLED`, it is `hot`;
   - otherwise nothing is hot: a panel, a scroll area or a disabled widget blocks what is under it.
   
   `over_ui` is whether any candidate contains the pointer. A pointer inside a `blockers` rect (the console window) makes nothing hot and leaves `over_ui` false (the console takes its own input).
2. **Press.** A press of the left (index 0) or right (1) button with something hot makes it `active` with that button and records `press_pos`. With `over_ui`, `consume_press[button]` is set. A press with nothing hot over a blocking record is consumed too.
3. **Drag.** While `active`, moving more than `drag_threshold` from `press_pos` sets `dragging`. The first such frame sets `drag_started = active`. If the active record no longer exists in last frame's records, active, dragging and the drag clear.
4. **Release** of the active button:
   - If dragging, `drag_released = active`.
   - Else if `hot == active`, the left button gives `clicked`, and a second click on the same id within 0.3 s gives `double` too (then `last_click` resets). The right button gives `secondary`.
   - `active` then clears.
5. The one-frame ids (`clicked`, `secondary`, `double`, `drag_started`, `drag_released`) are cleared at the start of the next resolve.

**`UiContext::response(id, sense, disabled)`** builds a `Response` from `memory.interact`. All states are false when:
- the context's scene is not the top scene;
- the deprecated `mouse_occluded` is set (until Task 19);
- `disabled` is set.

Its `rect` is `memory.current(id)`'s rect if this frame already resolved it, else `memory.prev(id)`'s.

**The engine** (`draw_ui`), on the frame's first pass:
1. Build the `PointerInput` from the `Input` snapshot.
2. Get the top scope with `scenes.top_scope()`.
3. Pass the console window rect as a blocker when the console is visible.
4. Call `resolve_pointer` with a drag threshold of 4.0.
5. For each `consume_press[b]`, call `snapshot.consume_mouse(button)` before building the context.

The existing `apply_consumed` and `ui_mouse_capture` then keep a press the UI took from gameplay until its release. `ui.wants_pointer()` is `over_ui || active != NONE`.

**The button.** It runs on `show()`, on `clicked()`, or on drop.
- **Id and size.** The id comes from the site and key. Its intrinsic size is the label measured in the default style, plus padding 12 × 6, with a minimum height of 28.
- **Node.** A leaf with `Measure::Fixed(size)`, or placed `at` a rect, recording `Flags::CLICK` (plus `DISABLED`).
- **Draws.**
  - A `Fill`, radius 4, with a 1-unit border.
  - The label as a `Text` draw centred at frac (0.5, 0.5), with `wrap: false`.
  - With `.icon`, a `Sprite` of the line height square, left of the label (its intrinsic width grows by the icon and a 6-unit gap).
- **Colours** until Task 9 (the old fallbacks):

| State | Fill | Border |
|---|---|---|
| Normal | `rgb(40,40,60)` | `rgb(80,80,100)` |
| Hovered | `rgb(50,50,70)` | `rgb(100,100,130)` |
| Pressed | `rgb(60,60,80)` | `rgb(100,100,130)` |
| Disabled | `rgba(40,40,60,120)` | `rgba(80,80,100,120)` |

  The label is white, or `rgba(255,255,255,120)` when disabled.
- **Port.** The old `button(&mut self, &str, Rect) -> bool` is removed. Port every call site to `ui.button(label).at(rect).clicked()` (keep `let clicked = ..` shapes as they are).

**`ui.custom(w, h)`:**
- It is a leaf of fixed size `(w, h)` (a style may make it fill), with the chosen `Sense` recorded as `CLICK`/`DRAG`.
- `paint` runs the closure at once with a `Painter`. Its `size()` is `(w, h)`, or last frame's laid-out size when the style fills.
- Each painter call pushes a `DrawKind::Local` draw, translated to the widget's final position when its subtree is laid out.
- It returns the widget's `Response`.

- [ ] **Step 1: Write the failing tests**

`src/core/ui/interact.rs`:

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ui::memory::{Flags, Record, UiMemory};
  use crate::core::ui::{Rect, UiId};
  use glam::Vec2;

  fn rec(rect: Rect, layer: u8, order: u32, flags: Flags, scene: u64) -> Record {
    Record { rect, clip: None, layer, order, depth: 0, scene, flags, scope: UiId::NONE, scroll: UiId::NONE, content: Vec2::ZERO, nav: [UiId::NONE; 4] }
  }

  /// Records drawn "last frame", then a fresh frame to resolve against
  fn memory(records: &[(u64, Record)]) -> UiMemory {
    let mut memory = UiMemory::new();
    memory.begin_frame();
    for (id, r) in records {
      memory.record(UiId(*id), *r);
    }
    memory.end_frame();
    memory.begin_frame();
    memory
  }

  fn at(x: f32, y: f32) -> PointerInput {
    PointerInput { position: Vec2::new(x, y), pressed: [false; 2], released: [false; 2], down: [false; 2], time: 0.0 }
  }

  fn press(x: f32, y: f32) -> PointerInput {
    PointerInput { pressed: [true, false], down: [true, false], ..at(x, y) }
  }

  fn release(x: f32, y: f32) -> PointerInput {
    PointerInput { released: [true, false], ..at(x, y) }
  }

  const BUTTON: Rect = Rect::new(10.0, 10.0, 50.0, 20.0);

  #[test]
  fn the_topmost_widget_under_the_pointer_is_hot() {
    let mut m = memory(&[(1, rec(BUTTON, 0, 1, Flags::CLICK, 1)), (2, rec(BUTTON, 0, 2, Flags::CLICK, 1))]);
    resolve_pointer(&mut m, &at(20.0, 20.0), Some(1), &[], 4.0);
    assert_eq!(m.interact.hot, UiId(2));
  }

  #[test]
  fn a_blocking_panel_hides_widgets_under_it() {
    let mut m = memory(&[(1, rec(BUTTON, 0, 1, Flags::CLICK, 1)), (2, rec(Rect::new(0.0, 0.0, 100.0, 100.0), 0, 2, Flags::BLOCKING, 1))]);
    let out = resolve_pointer(&mut m, &press(20.0, 20.0), Some(1), &[], 4.0);
    assert_eq!(m.interact.hot, UiId::NONE);
    assert!(m.interact.over_ui);
    assert!(out.consume_press[0], "a press on a panel is the UI's");
  }

  #[test]
  fn a_clipped_out_widget_is_not_hit() {
    let mut clipped = rec(BUTTON, 0, 1, Flags::CLICK, 1);
    clipped.clip = Some(Rect::new(0.0, 0.0, 15.0, 15.0));
    let mut m = memory(&[(1, clipped)]);
    resolve_pointer(&mut m, &at(40.0, 20.0), Some(1), &[], 4.0);
    assert_eq!(m.interact.hot, UiId::NONE);
    resolve_pointer(&mut m, &at(12.0, 12.0), Some(1), &[], 4.0);
    assert_eq!(m.interact.hot, UiId(1));
  }

  #[test]
  fn a_disabled_widget_is_never_hot_and_blocks() {
    let mut m = memory(&[(1, rec(BUTTON, 0, 1, Flags::CLICK, 1)), (2, rec(BUTTON, 0, 2, Flags::CLICK | Flags::DISABLED, 1))]);
    resolve_pointer(&mut m, &at(20.0, 20.0), Some(1), &[], 4.0);
    assert_eq!(m.interact.hot, UiId::NONE);
  }

  #[test]
  fn a_click_fires_on_release_over_the_pressed_widget() {
    let mut m = memory(&[(1, rec(BUTTON, 0, 1, Flags::CLICK, 1))]);
    resolve_pointer(&mut m, &press(20.0, 20.0), Some(1), &[], 4.0);
    assert_eq!(m.interact.active, UiId(1));
    assert_eq!(m.interact.clicked, UiId::NONE, "not on press");
    m.end_frame();
    m.begin_frame();
    for (id, r) in [(1, rec(BUTTON, 0, 1, Flags::CLICK, 1))] {
      m.record(UiId(id), r);
    }
    m.end_frame();
    m.begin_frame();
    resolve_pointer(&mut m, &release(21.0, 20.0), Some(1), &[], 4.0);
    assert_eq!(m.interact.clicked, UiId(1));
    assert_eq!(m.interact.active, UiId::NONE);
  }

  #[test]
  fn dragging_off_before_release_cancels_the_click() {
    let mut m = memory(&[(1, rec(BUTTON, 0, 1, Flags::CLICK, 1))]);
    resolve_pointer(&mut m, &press(20.0, 20.0), Some(1), &[], 4.0);
    // Still recorded next frame
    let r = *m.prev(UiId(1)).unwrap();
    m.record(UiId(1), r);
    m.end_frame();
    m.begin_frame();
    resolve_pointer(&mut m, &release(200.0, 200.0), Some(1), &[], 4.0);
    assert_eq!(m.interact.clicked, UiId::NONE);
  }

  #[test]
  fn a_drag_starts_past_four_units() {
    let mut m = memory(&[(1, rec(BUTTON, 0, 1, Flags::DRAG, 1))]);
    resolve_pointer(&mut m, &press(20.0, 20.0), Some(1), &[], 4.0);
    let held = |x| PointerInput { down: [true, false], ..at(x, 20.0) };
    resolve_pointer(&mut m, &held(23.0), Some(1), &[], 4.0);
    assert!(!m.interact.dragging);
    resolve_pointer(&mut m, &held(25.0), Some(1), &[], 4.0);
    assert!(m.interact.dragging);
    assert_eq!(m.interact.drag_started, UiId(1));
  }

  #[test]
  fn two_quick_clicks_are_a_double_click() {
    let mut m = memory(&[(1, rec(BUTTON, 0, 1, Flags::CLICK, 1))]);
    for (t, double) in [(0.0, false), (0.2, true)] {
      resolve_pointer(&mut m, &PointerInput { time: t, ..press(20.0, 20.0) }, Some(1), &[], 4.0);
      resolve_pointer(&mut m, &PointerInput { time: t + 0.05, ..release(20.0, 20.0) }, Some(1), &[], 4.0);
      assert_eq!(m.interact.double == UiId(1), double);
    }
  }

  #[test]
  fn a_secondary_click_is_its_own() {
    let mut m = memory(&[(1, rec(BUTTON, 0, 1, Flags::CLICK, 1))]);
    resolve_pointer(&mut m, &PointerInput { pressed: [false, true], down: [false, true], ..at(20.0, 20.0) }, Some(1), &[], 4.0);
    resolve_pointer(&mut m, &PointerInput { released: [false, true], ..at(20.0, 20.0) }, Some(1), &[], 4.0);
    assert_eq!(m.interact.secondary, UiId(1));
    assert_eq!(m.interact.clicked, UiId::NONE);
  }

  #[test]
  fn a_lower_scene_gets_no_pointer() {
    let mut m = memory(&[(1, rec(BUTTON, 0, 1, Flags::CLICK, 7))]);
    resolve_pointer(&mut m, &at(20.0, 20.0), Some(8), &[], 4.0);
    assert_eq!(m.interact.hot, UiId::NONE);
  }

  #[test]
  fn a_widget_gone_this_frame_reports_nothing() {
    let mut m = memory(&[(1, rec(BUTTON, 0, 1, Flags::CLICK, 1))]);
    resolve_pointer(&mut m, &press(20.0, 20.0), Some(1), &[], 4.0);
    m.end_frame();
    m.begin_frame(); // the button was not recorded: it is gone
    resolve_pointer(&mut m, &release(20.0, 20.0), Some(1), &[], 4.0);
    assert_eq!(m.interact.clicked, UiId::NONE);
    assert_eq!(m.interact.active, UiId::NONE);
  }

  #[test]
  fn a_click_lands_on_what_was_drawn_last_frame() {
    // The button moved this frame, but the player clicked where they saw it
    let mut m = memory(&[(1, rec(BUTTON, 0, 1, Flags::CLICK, 1))]);
    resolve_pointer(&mut m, &press(12.0, 12.0), Some(1), &[], 4.0);
    m.record(UiId(1), rec(Rect::new(200.0, 200.0, 50.0, 20.0), 0, 1, Flags::CLICK, 1));
    m.end_frame();
    m.begin_frame();
    m.record(UiId(1), rec(BUTTON, 0, 1, Flags::CLICK, 1));
    m.end_frame();
    m.begin_frame();
    resolve_pointer(&mut m, &release(12.0, 12.0), Some(1), &[], 4.0);
    assert_eq!(m.interact.clicked, UiId(1));
  }

  #[test]
  fn the_console_blocks_the_pointer() {
    let mut m = memory(&[(1, rec(BUTTON, 0, 1, Flags::CLICK, 1))]);
    let out = resolve_pointer(&mut m, &press(20.0, 20.0), Some(1), &[Rect::new(0.0, 0.0, 100.0, 100.0)], 4.0);
    assert_eq!(m.interact.hot, UiId::NONE);
    assert!(!out.consume_press[0], "the console takes its own clicks");
  }
}
```

Add `UiDriver` to `src/core/ui/test_util.rs`: a headless multi-frame UI. It owns a memory, a layout, a text engine and commands; it scripts the pointer and calls `resolve_pointer` each frame.

```rust
pub(crate) struct UiDriver {
  pub(crate) memory: UiMemory,
  pub(crate) layout: LayoutEngine,
  pub(crate) text: TextEngine,
  pub(crate) commands: UiCommands,
  size: (f32, f32),
  pointer: PointerInput,
  time: f64,
}

impl UiDriver {
  pub(crate) fn new(size: (f32, f32)) -> UiDriver;
  pub(crate) fn move_to(&mut self, x: f32, y: f32);
  pub(crate) fn press(&mut self);    // left button, this frame
  pub(crate) fn release(&mut self);
  /// One frame: resolve the scripted pointer, run `ui` as the top scene (scope 1)
  pub(crate) fn frame<R>(&mut self, ui: impl FnOnce(&mut UiContext<'_>) -> R) -> R;
}
```

`frame` advances `time` by 1/60 s, calls `memory.begin_frame()`, `layout.begin_frame()`, `resolve_pointer(..)` and `begin_scene(1, true)`. It then runs `ui`, calls `end_scene()`, and ends both frames. Press and release last one frame.

`src/core/ui/widgets/button.rs`:

```rust
#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use crate::core::ui::Rect;
  use crate::core::ui::test_util::UiDriver;

  #[test]
  fn a_button_clicks_on_release() {
    let mut d = UiDriver::new((200.0, 100.0));
    let at = Rect::new(10.0, 10.0, 80.0, 30.0);
    d.frame(|ui| ui.button("Go").at(at).clicked());
    d.move_to(20.0, 20.0);
    d.press();
    assert!(!d.frame(|ui| ui.button("Go").at(at).clicked()));
    d.release();
    assert!(d.frame(|ui| ui.button("Go").at(at).clicked()));
    assert!(!d.frame(|ui| ui.button("Go").at(at).clicked()), "once");
  }

  #[test]
  fn a_panel_drawn_over_a_button_takes_its_clicks() {
    let mut d = UiDriver::new((200.0, 100.0));
    let at = Rect::new(10.0, 10.0, 80.0, 30.0);
    let draw = |ui: &mut crate::core::ui::UiContext<'_>| {
      let clicked = ui.button("Under").at(at).clicked();
      ui.panel().at(Rect::new(0.0, 0.0, 200.0, 100.0)).show(|_| {});
      clicked
    };
    d.frame(draw);
    d.move_to(20.0, 20.0);
    d.press();
    d.frame(draw);
    d.release();
    assert!(!d.frame(draw));
  }

  #[test]
  fn a_button_in_a_column_is_hit_where_it_was_laid_out() {
    let mut d = UiDriver::new((200.0, 100.0));
    let draw = |ui: &mut crate::core::ui::UiContext<'_>| {
      let mut clicked = false;
      ui.column().offset(50.0, 40.0).show(|ui| clicked = ui.button("In").clicked());
      clicked
    };
    d.frame(draw);
    d.move_to(60.0, 50.0);
    d.press();
    d.frame(draw);
    d.release();
    assert!(d.frame(draw));
  }

  #[test]
  fn a_disabled_button_never_clicks() {
    let mut d = UiDriver::new((200.0, 100.0));
    let at = Rect::new(10.0, 10.0, 80.0, 30.0);
    d.frame(|ui| ui.button("No").at(at).enabled(false).clicked());
    d.move_to(20.0, 20.0);
    d.press();
    d.frame(|ui| ui.button("No").at(at).enabled(false).clicked());
    d.release();
    assert!(!d.frame(|ui| ui.button("No").at(at).enabled(false).clicked()));
  }
}
```

`src/core/ui/widgets/custom.rs`:

```rust
#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use crate::core::ui::immediate::UiCommand;
  use crate::core::ui::test_util::UiDriver;
  use crate::core::ui::{Color, Rect, Sense};

  #[test]
  fn painting_in_a_column_lands_at_the_widget() {
    let mut d = UiDriver::new((200.0, 100.0));
    d.frame(|ui| {
      ui.column().offset(30.0, 20.0).show(|ui| {
        ui.custom(16.0, 16.0).sense(Sense::click()).paint(|p, _| p.rect(Rect::new(2.0, 2.0, 4.0, 4.0), Color::RED));
      });
    });
    let rect = d.commands.iter().find_map(|(_, c)| match c {
      UiCommand::Rect { rect, .. } => Some(*rect),
      _ => None,
    });
    assert_eq!(rect, Some(Rect::new(32.0, 22.0, 4.0, 4.0)));
  }
}
```

In `src/core/engine/frame.rs` tests, update the existing tests that draw buttons to the new call. Then add:

```rust
  #[test]
  fn a_ui_button_clicks_on_release_and_the_press_never_reaches_gameplay() {
    // As `a_press_a_button_takes_stays_the_ui_s_until_released`, with the click counted at release
  }
```

Write it with the module's `gameplay_under` helper and a `Cell` click counter. Assert:
- the gameplay count stays 0 throughout;
- the click count is 0 after the press frame and 1 after the release frame.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p ryo2d --lib ui::interact ui::widgets`
Expected: compile errors.

- [ ] **Step 3: Implement**

Write the modules from Interfaces and the rules above.

`widget_builders!` generates the shared builder methods on each widget type from a `opts: WidgetOpts` field:
- `id(key)` stores `key_hash(&key)`;
- `enabled(bool)`;
- `at(rect)`;
- `width`, `height`, `fill_width`, `margin`, `anchor`, `offset`, `grow`, `min_width`, `max_width`, writing into `opts.style`.

Each widget runs on drop. `Drop` takes the context out of an `Option<&'u mut UiContext<'a>>` field, so `show()` and `Drop` share one `fn run(&mut self) -> Option<Response>`, and there is no `unwrap`.

Port the call sites. Every `ui.button("X", rect)` becomes `ui.button("X").at(rect).clicked()`; the scenes' layouts are reworked in Task 16. The code that read the old `bool` keeps working.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ryo2d --lib ui::` and `cargo test -p ryo2d --lib engine::` and `cargo test --test e2e ui_drawing` and `cargo test -p demo-kit` and `cargo build --workspace --all-targets`
Expected: pass. `ui_units_and_mouse_hits_hold_at_scale_factor_two` and demo-kit's `page_widgets_take_no_clicks_under_help` still pass: they press and release over steps, and help still occludes through the deprecated flag.

- [ ] **Step 5: Docs**

In `src/core/ui/AGENTS.md`, add "Interaction":
- the resolve step against last frame's records;
- topmost wins, with blocking, clips and disabled;
- click on release, drag past 4 units, double click within 0.3 s;
- only the top scene;
- presses over the UI are captured from gameplay until release.

- [ ] **Step 6: Gate and commit**

Stage `src/core/ui/`, `src/core/engine/frame.rs`, `src/core/scene/manager.rs`, `src/lib.rs`, `tests/e2e/ui_drawing.rs`, `examples/demo-kit/tests/shell.rs` and each ported example file by name.

```bash
git commit -m "feat: UI interaction resolved against last frame's layout" -m "- the topmost widget under the pointer wins; panels, clips and disabled widgets block
- clicks fire on release; drags start past a threshold; double and secondary clicks
- ui.button and ui.custom return responses; the old rect-taking button is gone"
```

### Task 7: Scroll containers and wheel routing

**Files:**
- Create: `src/core/ui/widgets/scroll.rs` (`ui.scroll()`, `ScrollState`, the scrollbar)
- Modify:
  - `src/core/ui/interact.rs`: `route_wheel` and scroll requests.
  - `src/core/ui/tree.rs`: scroll offsets and clips applied when resolving.
  - `src/core/ui/memory.rs`: pending scroll requests.
  - `src/core/ui/immediate/context.rs`: remove `scroll_view`, `scroll_view_horizontal`, `take_scroll_input`; add `scroll_to`, `scroll_offset`, `set_scroll_offset`.
  - `src/core/engine/frame.rs`: route the wheel after the pointer resolve.
  - `src/core/ui/retained/scroll_view.rs`: deleted.
  - `src/core/ui/retained/mod.rs`, `src/core/ui/mod.rs`, `src/lib.rs`: drop `ScrollViewState`.
  - Ports:
    - `examples/demos/text/main.rs`: the DrawingPage list;
    - `examples/demos/ui/src/scenes/scroll_view_demo.rs`.
- Test: `src/core/ui/interact.rs`, `src/core/ui/widgets/scroll.rs` (tests modules)

**Interfaces:**
- Consumes: Task 5's subtree resolution (`Node::clips`, `Node::scroll`, `Placed::content`), Task 6's resolve, `Response`, `UiDriver`, Task 1's one-shot wheel.
- Produces:

```rust
pub fn scroll<'u>(&'u mut self) -> Scroll<'u, 'a>;     // #[track_caller]
impl Scroll<'_, '_> {
  pub fn vertical(self) -> Self;     // the default
  pub fn horizontal(self) -> Self;
  pub fn both(self) -> Self;
  pub fn no_bar(self) -> Self;
  // plus the container layout builders: width, height, fill_width, fill_height, fill, min/max, padding, gap, margin, grow, anchor, offset, at, id
  pub fn show(self, contents: impl FnOnce(&mut UiContext<'a>)) -> Response;
}
pub fn scroll_to(&mut self, id: UiId);                 // brings a widget into view next frame
pub fn scroll_offset(&self, scroll: UiId) -> Vec2;
pub fn set_scroll_offset(&mut self, scroll: UiId, offset: Vec2);
pub(crate) struct ScrollState { pub(crate) offset: Vec2, pub(crate) target: Vec2, pub(crate) content: Vec2, pub(crate) view: Vec2 }   // Default
pub(crate) fn route_wheel(memory: &mut UiMemory, pointer: Vec2, wheel: Vec2, shift: bool, top_scene: Option<u64>, line: f32) -> bool;
pub(crate) fn apply_scroll_requests(memory: &mut UiMemory);
```

**The rules:**
- **The wheel.**
  - **Delta.** `wheel` is in lines, with +y meaning up. Shift with a vertical-only wheel turns it into horizontal. The offset moves by `-wheel * line`, and `line` is 40.
  - **Candidates.** Last frame's records of the top scene with `SCROLLABLE` that contain the pointer inside their clip, innermost first (highest `depth`).
  - **Routing, per axis.** The first candidate that can still move toward the wheel takes it: `target` is above 0 for up or left, or below `content - view` for down or right. Its `target` changes by the delta, clamped to `[0, max]`, and that axis is consumed. A candidate at its edge passes the wheel outward.
  - **The engine.** If anything took the wheel, `snapshot.consume_scroll()`, so gameplay never sees it.
- **Easing.** The scroll widget eases `offset` toward `target` each frame: `offset += (target - offset) * (1 - exp(-dt / 0.08))`, snapping when within 0.5. It clamps both to `[0, max]`, where `max = (content - view).max(0)`.
- **The node.**
  - The scroll node has `scroll_x`/`scroll_y` set, `clips = true` and the flags `SCROLLABLE | DRAG`.
  - Its children are translated by `-offset` and clipped to its rect (Task 5's resolution reads `ScrollState`).
  - After its subtree is laid out, `content` and `view` are stored from this frame's record when known, otherwise from last frame's.
- **The scrollbar:**
  - It is drawn inside the scroll rect along its right (or bottom) edge when `content > view` on that axis: a track 10 wide, and a thumb of length `max(20, view / content * track)` at `offset / max * (track - thumb)`. Its colours are the old scrollbar defaults (track `rgba(40,40,50,180)`, thumb `rgb(90,90,110)`, hovered `rgb(120,120,140)`) until the theme.
  - A press inside the bar's column, read from `press_pos` against last frame's rect, makes the scroll active. While dragging, the target follows the pointer: `target = (pointer - grab) / (track - thumb) * max`.
  - A press on the content background does nothing; content-drag scrolling is dropped.
- **Scroll requests.** `ui.scroll_to(id)` stores the request in memory. `apply_scroll_requests` runs in the next resolve. It finds `id`'s record and its `scroll` ancestor, then sets that scroll's `target` so the record's rect lies within the view, scrolling as little as possible.

- [ ] **Step 1: Write the failing tests**

`src/core/ui/interact.rs` (add):

```rust
  fn scrollable(rect: Rect, depth: u16, id: u64, m: &mut UiMemory, content: Vec2) {
    let mut r = rec(rect, 0, id as u32, Flags::SCROLLABLE, 1);
    r.depth = depth;
    r.content = content;
    m.record(UiId(id), r);
    let state = m.state::<crate::core::ui::widgets::scroll::ScrollState>(UiId(id), 1).unwrap();
    state.view = Vec2::new(rect.width, rect.height);
    state.content = content;
  }

  #[test]
  fn the_innermost_scrollable_takes_the_wheel() {
    let mut m = UiMemory::new();
    m.begin_frame();
    scrollable(Rect::new(0.0, 0.0, 200.0, 200.0), 0, 1, &mut m, Vec2::new(200.0, 1000.0));
    scrollable(Rect::new(10.0, 10.0, 100.0, 100.0), 3, 2, &mut m, Vec2::new(100.0, 500.0));
    m.end_frame();
    m.begin_frame();
    assert!(route_wheel(&mut m, Vec2::new(20.0, 20.0), Vec2::new(0.0, -1.0), false, Some(1), 40.0));
    let inner = m.peek_state::<crate::core::ui::widgets::scroll::ScrollState>(UiId(2)).unwrap().target;
    let outer = m.peek_state::<crate::core::ui::widgets::scroll::ScrollState>(UiId(1)).unwrap().target;
    assert_eq!((inner.y, outer.y), (40.0, 0.0));
  }

  #[test]
  fn a_scrollable_at_its_edge_passes_the_wheel_out() {
    let mut m = UiMemory::new();
    m.begin_frame();
    scrollable(Rect::new(0.0, 0.0, 200.0, 200.0), 0, 1, &mut m, Vec2::new(200.0, 1000.0));
    scrollable(Rect::new(10.0, 10.0, 100.0, 100.0), 3, 2, &mut m, Vec2::new(100.0, 500.0));
    m.end_frame();
    m.begin_frame();
    // The inner one is at its top; scrolling up goes to the outer one, which is at its top too
    assert!(!route_wheel(&mut m, Vec2::new(20.0, 20.0), Vec2::new(0.0, 1.0), false, Some(1), 40.0));
    m.state::<crate::core::ui::widgets::scroll::ScrollState>(UiId(1), 1).unwrap().target.y = 100.0;
    assert!(route_wheel(&mut m, Vec2::new(20.0, 20.0), Vec2::new(0.0, 1.0), false, Some(1), 40.0));
    assert_eq!(m.peek_state::<crate::core::ui::widgets::scroll::ScrollState>(UiId(1)).unwrap().target.y, 60.0);
  }

  #[test]
  fn a_scrollable_whose_content_fits_leaves_the_wheel() {
    let mut m = UiMemory::new();
    m.begin_frame();
    scrollable(Rect::new(0.0, 0.0, 200.0, 200.0), 0, 1, &mut m, Vec2::new(200.0, 150.0));
    m.end_frame();
    m.begin_frame();
    assert!(!route_wheel(&mut m, Vec2::new(20.0, 20.0), Vec2::new(0.0, -1.0), false, Some(1), 40.0));
  }

  #[test]
  fn shift_turns_the_wheel_sideways() {
    let mut m = UiMemory::new();
    m.begin_frame();
    scrollable(Rect::new(0.0, 0.0, 200.0, 50.0), 0, 1, &mut m, Vec2::new(800.0, 50.0));
    m.end_frame();
    m.begin_frame();
    assert!(route_wheel(&mut m, Vec2::new(20.0, 20.0), Vec2::new(0.0, -2.0), true, Some(1), 40.0));
    assert_eq!(m.peek_state::<crate::core::ui::widgets::scroll::ScrollState>(UiId(1)).unwrap().target.x, 80.0);
  }
```

`src/core/ui/widgets/scroll.rs`:

```rust
#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use crate::core::ui::test_util::UiDriver;
  use crate::core::ui::{Rect, UiContext, UiId};

  fn list(ui: &mut UiContext<'_>, rows: usize, ids: &mut Vec<UiId>, clicked: &mut Option<usize>) -> UiId {
    ui.scroll().at(Rect::new(0.0, 0.0, 120.0, 100.0)).show(|ui| {
      for i in 0..rows {
        let r = ui.button(format!("Row {i}")).id(i).height(30.0).fill_width().show();
        if r.clicked() {
          *clicked = Some(i);
        }
        ids.push(r.id);
      }
    }).id
  }

  #[test]
  fn the_wheel_scrolls_rows_and_clipped_rows_take_no_clicks() {
    let mut d = UiDriver::new((200.0, 200.0));
    let mut scroll = UiId::NONE;
    for _ in 0..3 {
      d.frame(|ui| scroll = list(ui, 10, &mut Vec::new(), &mut None));
    }
    d.move_to(50.0, 50.0);
    d.wheel(0.0, -1.0);
    for _ in 0..30 {
      d.frame(|ui| scroll = list(ui, 10, &mut Vec::new(), &mut None));
    }
    let offset = d.frame(|ui| ui.scroll_offset(scroll));
    assert!((offset.y - 40.0).abs() < 0.5, "eased to the target: {offset:?}");
    // Row 0 is above the view now; a click at the very top (y 2) is on row 1
    d.move_to(50.0, 2.0);
    d.press();
    d.frame(|ui| list(ui, 10, &mut Vec::new(), &mut None));
    d.release();
    let mut clicked = None;
    d.frame(|ui| list(ui, 10, &mut Vec::new(), &mut clicked));
    assert_eq!(clicked, Some(1));
    // Below the scroll area (y 150) nothing is clicked, though row 6 lies there unclipped
    d.move_to(50.0, 150.0);
    d.press();
    d.frame(|ui| list(ui, 10, &mut Vec::new(), &mut None));
    d.release();
    let mut clicked = None;
    d.frame(|ui| list(ui, 10, &mut Vec::new(), &mut clicked));
    assert_eq!(clicked, None);
  }

  #[test]
  fn scroll_to_brings_a_row_into_view() {
    let mut d = UiDriver::new((200.0, 200.0));
    let mut ids = Vec::new();
    let mut scroll = UiId::NONE;
    d.frame(|ui| scroll = list(ui, 10, &mut ids, &mut None));
    let target = ids[8];
    // The request is read against this frame's records, so the list is drawn too
    d.frame(|ui| {
      scroll = list(ui, 10, &mut Vec::new(), &mut None);
      ui.scroll_to(target);
    });
    for _ in 0..30 {
      d.frame(|ui| scroll = list(ui, 10, &mut Vec::new(), &mut None));
    }
    let offset = d.frame(|ui| ui.scroll_offset(scroll));
    // Row 8 spans 240..270; the view is 100 tall
    assert!((offset.y - 170.0).abs() < 0.5, "{offset:?}");
  }

  #[test]
  fn ten_thousand_rows_lay_out_and_scroll() {
    let mut d = UiDriver::new((200.0, 200.0));
    let mut ids = Vec::new();
    d.frame(|ui| {
      list(ui, 10_000, &mut ids, &mut None);
    });
    let last = *ids.last().unwrap();
    let mut scroll = UiId::NONE;
    d.frame(|ui| {
      scroll = list(ui, 10_000, &mut Vec::new(), &mut None);
      ui.scroll_to(last);
    });
    for _ in 0..40 {
      d.frame(|ui| scroll = list(ui, 10_000, &mut Vec::new(), &mut None));
    }
    let offset = d.frame(|ui| ui.scroll_offset(scroll));
    assert!((offset.y - (10_000.0 * 30.0 - 100.0)).abs() < 1.0, "{offset:?}");
  }
}
```

`UiDriver` gains `wheel(dx, dy)` (a wheel step this frame, routed with `shift: false`).

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p ryo2d --lib ui::interact ui::widgets::scroll`
Expected: compile errors.

- [ ] **Step 3: Implement**

Write `scroll.rs`, `route_wheel` and `apply_scroll_requests` by the rules above. The engine routes the wheel on the frame's first pass, right after `resolve_pointer`:
- read the wheel from the snapshot (`ui_view().scroll`) and Shift (`key_down(ShiftLeft) || key_down(ShiftRight)`);
- consume it when it was taken.

Delete the old scroll views and `ScrollViewState`, and port the two demos:
- **Text demo DrawingPage list.** Use `ui.scroll().at(view).show(|ui| for row in 0..30 { ui.custom(300.0, ROW_HEIGHT).id(row).paint(|p, _| ..the row's drawing.. ); })`. The old closure's `scroll_y` parameter is gone, because rows are laid out and translated by the scroll.
- **Scroll view demo.** Three `ui.scroll()` areas, each `.at(rect)`. Lorem text becomes `ui.text(..)` leaves. The item list becomes a column of `ui.custom` rows. The horizontal card strip becomes a `ui.scroll().horizontal()` row of 150-wide cards.
  - Home calls `ui.set_scroll_offset(id, Vec2::ZERO)`.
  - End sets a large offset (it is clamped).
  - The readouts read `ui.scroll_offset(id)`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ryo2d --lib ui::` and `cargo test --workspace` and `cargo build --workspace --all-targets`
Expected: pass.

- [ ] **Step 5: Docs**

In `src/core/ui/AGENTS.md`, add "Scrolling":
- the innermost scrollable under the pointer that can move takes the wheel, and the wheel bubbles out at an edge;
- Shift makes it horizontal;
- offsets ease, and children are translated and clipped;
- `scroll_to`;
- no content dragging.

- [ ] **Step 6: Gate and commit**

Stage the files listed above by name, including the deletion of `src/core/ui/retained/scroll_view.rs` (`git rm`).

```bash
git commit -m "feat: UI scroll areas with nested wheel routing" -m "- the innermost scroll area under the pointer that can move takes the wheel; it bubbles out at an edge
- offsets ease; rows are clipped for hits as well as drawing; scroll_to brings a widget into view
- the old scroll views and ScrollViewState are replaced"
```

### Task 8: Focus and navigation

**Files:**
- Create: `src/core/ui/nav.rs` (the pure navigation functions and `resolve_focus`)
- Modify:
  - `src/core/ui/memory.rs`: focus fields on `Interaction`, pending focus requests.
  - `src/core/ui/response.rs`: focus states; `clicked()` includes activation.
  - `src/core/ui/tree.rs`: nodes inherit their nearest scope.
  - `src/core/ui/widgets/common.rs`: `default_focus`, `nav_up/down/left/right`, `nav_skip`, `focusable`.
  - `src/core/ui/widgets/button.rs`: focusable, focus ring.
  - `src/core/ui/immediate/context.rs`: `request_focus`, `is_focused`, `focused`, `back`, `focus_visible`; the focus ring helper.
  - `src/core/engine/frame.rs`: the navigation input from actions; consume the used actions.
  - `src/core/ui/test_util.rs`: `UiDriver` navigation.
- Test: `src/core/ui/nav.rs` (tests module), `src/core/ui/widgets/button.rs`

**Interfaces:**
- Consumes:
  - Task 2's actions (`ui.up/down/left/right/confirm/back/next/tab_next/tab_prev`, `Input::{action_pressed, action_held, actions, consume_binding}`), `Direction`, `InputDevice`.
  - Task 6's resolve and `Response`.
  - Task 7's `scroll_to` requests.
- Produces:

```rust
// nav.rs (crate)
pub(crate) struct NavInput {
  pub(crate) pressed: [bool; 4],   // Up, Down, Left, Right this frame
  pub(crate) held: [bool; 4],
  pub(crate) confirm: bool, pub(crate) back: bool, pub(crate) next: bool, pub(crate) shift: bool,
  pub(crate) tab_next: bool, pub(crate) tab_prev: bool,
  pub(crate) right_stick: Vec2, pub(crate) device: InputDevice, pub(crate) dt: f32,
}
pub(crate) struct NavOutcome { pub(crate) used: Vec<&'static str> }   // action names whose bindings the UI took
pub(crate) fn resolve_focus(memory: &mut UiMemory, input: &NavInput, top_scene: Option<u64>, pointer_moved: bool) -> NavOutcome;
pub(crate) fn spatial(from: Rect, direction: Direction, candidates: impl Iterator<Item = (UiId, Rect)>) -> Option<UiId>;
pub(crate) fn wrap_target(from: Rect, direction: Direction, candidates: impl Iterator<Item = (UiId, Rect)>) -> Option<UiId>;
// memory.rs: Interaction gains
pub(crate) focused: UiId, pub(crate) gained: UiId, pub(crate) lost: UiId, pub(crate) activated: UiId,
pub(crate) nav_step: Option<(UiId, Direction)>,   // a direction the focused widget takes (slider, open list)
pub(crate) back_pressed: bool, pub(crate) back_used: bool, pub(crate) back_taken: bool,
pub(crate) focus_visible: bool, pub(crate) tab_step: i8,   // -1/0/1 from tab_prev/tab_next
pub(crate) tab_taken: bool,   // a tabs widget took tab_step this frame (Task 12)
// public UiContext
pub fn request_focus(&mut self, id: UiId);
pub fn is_focused(&self, id: UiId) -> bool;
pub fn focused(&self) -> Option<UiId>;
pub fn back(&mut self) -> bool;          // Back pressed and no widget used it; takes it from gameplay
pub fn focus_visible(&self) -> bool;
// widget builders (widget_builders! gains)
pub fn default_focus(self) -> Self;  pub fn nav_up(self, id: UiId) -> Self;  pub fn nav_down(self, id: UiId) -> Self;
pub fn nav_left(self, id: UiId) -> Self;  pub fn nav_right(self, id: UiId) -> Self;  pub fn nav_skip(self) -> Self;
pub fn focusable(self, focusable: bool) -> Self;
// crate
pub(crate) fn nav_step_for(&self, id: UiId) -> Option<Direction>;   // for widgets that take directions
pub(crate) fn draw_focus_ring(&mut self, node: usize, radius: f32);
```

**`spatial(from, d, candidates)`.** A candidate qualifies when its centre lies beyond `from`'s centre along `d`. For each one:
- `p` is the gap along `d` from `from`'s leading edge to the candidate's near edge (0 if they overlap);
- `s` is the gap between the two rects on the perpendicular axis (0 if their ranges overlap);
- its score is `p + 2·s + 0.1·|perpendicular centre distance|`.

Aligned candidates (`s == 0`) beat every non-aligned one. The lowest score wins, with ties going to the lower `UiId`, so the result is deterministic.

**`wrap_target`** picks, among the candidates on the far side (beyond `from`'s centre against `d`), the one farthest against `d`, preferring aligned ones and then the nearest on the perpendicular axis.

**`resolve_focus`** runs after `resolve_pointer` and `route_wheel`, once per engine frame, against last frame's records of the top scene:

1. **Focusables.** Records with `FOCUSABLE`, not `DISABLED`, whose rect overlaps their clip with positive area. A record inside a scroll area (`scroll != NONE`) is tested by its scroll area instead: it counts as visible when the scroll area's record overlaps that record's own clip, so navigation reaches rows scrolled out of view and scrolls to them (rule 11). Directional navigation also skips `NAV_SKIP`. A record outside any scope (`scope == NONE`) is focusable only by click, unless it is a `TEXT_FIELD`.
2. **Validate.** If `focused` is no longer a focusable record, it becomes `NONE`, with `lost` set to the old id.
3. **Scopes.** Collect the scope records, those with `FOCUS_SCOPE`.
   - **Scopes that just appeared** (not in `known_scopes`), in order:
     - one with `MODAL` or `TRAP` pushes `(scope, focused)` onto `modal_stack` and focuses `default_in(scope)`, or nothing when it has no focusable member;
     - otherwise, if nothing is focused, focus `default_in(scope)`.
   - **`default_in(scope)`** is the first of these that is a focusable member of the scope (its record's scope chain contains it): the recalled memory; a member with `DEFAULT_FOCUS`; the member with the lowest `order`.
   - **A modal that disappeared.** When the top of `modal_stack` is gone, pop it and restore its saved focus if that is still focusable.
   - Finally, `known_scopes` becomes the current set.
4. **Requests.** A pending `request_focus(id)` focuses `id` if it is focusable.
5. **Pointer.**
   - On a frame where the pointer moved and `hot` is focusable, focus `hot` (focus follows the mouse).
   - A press on a focusable widget focuses it.
   - A press elsewhere, while a `TEXT_FIELD` is focused, unfocuses it.
6. **Directions.**
   - **Repeat.** A direction fires on press, then while held after 0.35 s and every 0.1 s, with per-direction timers in `Interaction`. The timing comes from the theme in Task 9; use constants until then. A timer is armed only by a press: when the top scene changes, every timer is disarmed, so a direction still held from the old scene does nothing until it is pressed again.
   - **Directions a widget takes.** When the focused record has `TAKES_HORIZONTAL` (for Left/Right) or `TAKES_VERTICAL` (for Up/Down), set `nav_step = (focused, d)`.
   - **Otherwise, with something focused,** the target is the first of these that finds one:
     - an explicit `nav[d]` that is focusable;
     - `spatial` among the members of the focused record's scope;
     - `wrap_target` within that scope if it is `WRAP`;
     - if the scope is not `TRAP`, the same search in each ancestor scope in turn, through each scope record's own `scope` field.
     
     If none is found, focus stays.
   - **With nothing focused,** focus `default_in` the innermost scope on `modal_stack`, or the first scope by order when `modal_stack` is empty. A modal or trapped scope with nothing focusable keeps focus empty: the page under it is never a target.
   - A firing direction marks its action used whenever something is focused or a scope exists.
7. **Tab.** `next` moves to the next focusable by `order`, or the previous one with Shift. It wraps, stays within the innermost `TRAP` scope on `modal_stack` when one exists, and marks `ui.next` used.
8. **Confirm.** With something focused, `confirm` sets `activated = focused` and marks `ui.confirm` used.
9. **Back.** `back_pressed = back`. `back_used` is true when the focused record has `USES_BACK`, or a drag is active (Task 13). When it is used, mark `ui.back` used.
10. **Tab switching.** `tab_step` is `+1`/`-1` from `tab_next`/`tab_prev` while a scope exists. It marks nothing used: a `tabs` widget that takes the step sets `tab_taken` (Task 12), and the engine consumes the two actions after the pass. With no tabs on screen, Q, E and the shoulder buttons stay the game's.
11. **After focus changes.**
    - `gained` and `lost` are set.
    - `remember_focus(scope, scene, focused)` is called when the focused record has a scope.
    - A change made by navigation (not by the pointer) pushes a `scroll_to(focused)` request.
12. **Right stick.** When the focused record has a `scroll` ancestor and the right stick is past 0.2, that scroll's `target.y` moves by `-stick.y * 600 * dt` (`target.x` likewise), clamped.
13. **Ring visibility.** `focus_visible` is whether `device` is `Keyboard` or `Gamepad`.

**The engine** builds `NavInput` from `Input`:
- `action_pressed`/`action_held` of the UI actions;
- Shift from `key_down(ShiftLeft | ShiftRight)`;
- `right_stick()`, `last_device()`, and the real frame `dt`.

It then calls `resolve_focus` on the first pass and consumes the bindings of every used action from `Input` before the fixed steps. After the pass, if `back_taken` (a scene called `ui.back()` and got `true`), it consumes `ui.back`'s bindings as well, and if `tab_taken`, those of `ui.tab_next` and `ui.tab_prev`. Both flags clear at the next resolve.

**`Response`:**
- `focused()` is `focused == id`, and `gained_focus()`/`lost_focus()` follow `gained`/`lost`.
- `clicked()` is `clicked == id || activated == id`.
- Focus states are false outside the top scene.

**Widgets:**
- The button is `FOCUSABLE`, and `DEFAULT_FOCUS` with `.default_focus()`.
- `focusable(false)` removes `FOCUSABLE`.
- `nav_*` fill the record's `nav`, and `nav_skip` adds `NAV_SKIP`.
- A focused widget whose ring is visible calls `draw_focus_ring(node, radius)`. That pushes a `Fill` with `inset: Edges::all(-4.0)`, a transparent fill, a 2-unit border in `rgb(100,140,200)` (the theme's in Task 9), and radius `radius + 4`.

- [ ] **Step 1: Write the failing tests**

`src/core/ui/nav.rs`:

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::input::Direction;
  use crate::core::ui::{Rect, UiId};

  fn cands(rects: &[(u64, Rect)]) -> impl Iterator<Item = (UiId, Rect)> + '_ {
    rects.iter().map(|(id, r)| (UiId(*id), *r))
  }

  const ROW: f32 = 40.0;

  fn list() -> Vec<(u64, Rect)> {
    (0..4).map(|i| (i + 1, Rect::new(0.0, i as f32 * ROW, 100.0, 30.0))).collect()
  }

  #[test]
  fn down_in_a_list_picks_the_next_row() {
    let rows = list();
    assert_eq!(spatial(rows[0].1, Direction::Down, cands(&rows[1..])), Some(UiId(2)));
    assert_eq!(spatial(rows[2].1, Direction::Up, cands(&rows)), Some(UiId(2)));
    assert_eq!(spatial(rows[3].1, Direction::Down, cands(&rows)), None, "nothing beyond");
  }

  #[test]
  fn right_in_a_grid_stays_on_the_row() {
    let cells: Vec<(u64, Rect)> = (0..12).map(|i| (i + 1, Rect::new((i % 4) as f32 * 52.0, (i / 4) as f32 * 52.0, 48.0, 48.0))).collect();
    assert_eq!(spatial(cells[5].1, Direction::Right, cands(&cells)), Some(UiId(7)));
    assert_eq!(spatial(cells[5].1, Direction::Down, cands(&cells)), Some(UiId(10)));
    assert_eq!(spatial(cells[4].1, Direction::Left, cands(&cells)), None);
  }

  #[test]
  fn an_aligned_candidate_beats_a_nearer_diagonal() {
    let from = Rect::new(0.0, 0.0, 50.0, 30.0);
    let far_aligned = (1, Rect::new(0.0, 200.0, 50.0, 30.0));
    let near_diagonal = (2, Rect::new(60.0, 40.0, 50.0, 30.0));
    assert_eq!(spatial(from, Direction::Down, cands(&[far_aligned, near_diagonal])), Some(UiId(1)));
  }

  #[test]
  fn wrapping_goes_to_the_far_end() {
    let rows = list();
    assert_eq!(wrap_target(rows[3].1, Direction::Down, cands(&rows)), Some(UiId(1)));
    assert_eq!(wrap_target(rows[0].1, Direction::Up, cands(&rows)), Some(UiId(4)));
  }
}
```

`src/core/ui/widgets/button.rs` (add; `UiDriver` gains `nav(Direction)`, `hold(Direction)`, `release_nav()`, `confirm()`, `back()`, `tab(shift)`, `stick(Vec2)`, `device(InputDevice)`, `set_scene(u64)` (the scene id `frame` runs as the top scene, 1 by default), and `last_outcome()` returning the frame's `NavOutcome`):

```rust
  use crate::core::input::{Direction, InputDevice};
  use crate::core::ui::{UiContext, UiId};

  fn menu(ui: &mut UiContext<'_>, items: &[&str], clicked: &mut Option<usize>) -> Vec<UiId> {
    let mut ids = Vec::new();
    ui.column().focus_scope().wrap_focus().show(|ui| {
      for (i, item) in items.iter().enumerate() {
        let mut b = ui.button(*item).id(i).width(120.0);
        if i == 1 {
          b = b.default_focus();
        }
        let r = b.show();
        if r.clicked() {
          *clicked = Some(i);
        }
        ids.push(r.id);
      }
    });
    ids
  }

  #[test]
  fn a_menu_opens_on_its_default_focus() {
    let mut d = UiDriver::new((300.0, 300.0));
    let ids = d.frame(|ui| menu(ui, &["Resume", "Settings", "Quit"], &mut None));
    d.frame(|ui| menu(ui, &["Resume", "Settings", "Quit"], &mut None));
    assert_eq!(d.frame(|ui| ui.focused()), Some(ids[1]));
  }

  #[test]
  fn the_d_pad_moves_down_a_menu_and_confirm_clicks() {
    let mut d = UiDriver::new((300.0, 300.0));
    let items = ["Resume", "Settings", "Quit"];
    for _ in 0..2 {
      d.frame(|ui| menu(ui, &items, &mut None));
    }
    d.device(InputDevice::Gamepad);
    d.nav(Direction::Down);
    d.frame(|ui| menu(ui, &items, &mut None));
    d.confirm();
    let mut clicked = None;
    d.frame(|ui| menu(ui, &items, &mut clicked));
    assert_eq!(clicked, Some(2));
    assert!(d.last_outcome().used.contains(&"ui.confirm"));
  }

  #[test]
  fn wrap_focus_loops_a_menu() {
    let mut d = UiDriver::new((300.0, 300.0));
    let items = ["A", "B", "C"];
    let ids = d.frame(|ui| menu(ui, &items, &mut None));
    d.frame(|ui| menu(ui, &items, &mut None));
    for _ in 0..2 {
      d.nav(Direction::Down);
      d.frame(|ui| menu(ui, &items, &mut None));
    }
    assert_eq!(d.frame(|ui| ui.focused()), Some(ids[0]), "B, C, then round to A");
  }

  #[test]
  fn a_held_direction_repeats_after_a_delay() {
    let mut d = UiDriver::new((300.0, 600.0));
    let items = ["0", "1", "2", "3", "4", "5", "6", "7"];
    let ids = d.frame(|ui| menu(ui, &items, &mut None));
    d.frame(|ui| menu(ui, &items, &mut None));
    d.hold(Direction::Down);
    // 0.35 s at 60 fps is 21 frames: one press, then repeats every 6 frames
    for _ in 0..28 {
      d.frame(|ui| menu(ui, &items, &mut None));
    }
    d.release_nav();
    let at = ids.iter().position(|id| Some(*id) == d.frame(|ui| ui.focused())).unwrap();
    assert_eq!(at, 1 + 1 + 2, "default 1, the press, then two repeats");
  }

  #[test]
  fn focus_never_lands_on_disabled_or_clipped_out_widgets() {
    let mut d = UiDriver::new((300.0, 300.0));
    // A clip 100 tall: C (80-120) shows in part, D (120-160) not at all
    let draw = |ui: &mut UiContext<'_>| {
      let mut ids = Vec::new();
      ui.push_clip(crate::core::ui::Rect::new(0.0, 0.0, 300.0, 100.0));
      ui.column().focus_scope().show(|ui| {
        ids.push(ui.button("A").id(0).height(40.0).default_focus().show().id);
        ids.push(ui.button("Off").id(1).height(40.0).enabled(false).show().id);
        ids.push(ui.button("C").id(2).height(40.0).show().id);
        ids.push(ui.button("D").id(3).height(40.0).show().id);
      });
      ui.pop_clip();
      ids
    };
    let ids = d.frame(draw);
    d.frame(draw);
    d.nav(Direction::Down);
    d.frame(draw);
    assert_eq!(d.frame(|ui| { draw(ui); ui.focused() }), Some(ids[2]), "skips the disabled one");
    d.nav(Direction::Down);
    d.frame(draw);
    assert_eq!(d.frame(|ui| { draw(ui); ui.focused() }), Some(ids[2]), "D is clipped out");
  }

  #[test]
  fn navigation_reaches_rows_scrolled_out_of_view() {
    let mut d = UiDriver::new((300.0, 300.0));
    // A view of two 50-tall rows over ten: row 2 starts fully hidden
    let draw = |ui: &mut UiContext<'_>| {
      let mut ids = Vec::new();
      ui.column().focus_scope().show(|ui| {
        ui.scroll().height(100.0).show(|ui| {
          for i in 0..10 {
            let b = ui.button("Row").id(i).height(50.0);
            let b = if i == 0 { b.default_focus() } else { b };
            ids.push(b.show().id);
          }
        });
      });
      ids
    };
    let ids = d.frame(draw);
    d.frame(draw);
    for _ in 0..3 {
      d.nav(Direction::Down);
      d.frame(draw);
    }
    assert_eq!(d.frame(|ui| { draw(ui); ui.focused() }), Some(ids[3]));
  }

  #[test]
  fn an_empty_trapped_scope_keeps_focus_off_the_page() {
    let mut d = UiDriver::new((300.0, 300.0));
    let items = ["A", "B"];
    let draw = |ui: &mut UiContext<'_>, open: bool| {
      menu(ui, &items, &mut None);
      if open {
        ui.column().focus_scope().trap().show(|ui| {
          ui.text("Nothing to press");
        });
      }
    };
    for _ in 0..2 {
      d.frame(|ui| draw(ui, false));
    }
    d.frame(|ui| draw(ui, true));
    assert_eq!(d.frame(|ui| { draw(ui, true); ui.focused() }), None, "the trap took focus");
    d.nav(Direction::Down);
    d.frame(|ui| draw(ui, true));
    assert_eq!(d.frame(|ui| { draw(ui, true); ui.focused() }), None, "the page under the trap is not a target");
    assert!(d.last_outcome().used.contains(&"ui.down"), "the arrow is still the UI's");
  }

  #[test]
  fn a_held_direction_does_not_repeat_into_a_new_scene() {
    let mut d = UiDriver::new((300.0, 600.0));
    let items = ["0", "1", "2", "3", "4", "5"];
    d.frame(|ui| menu(ui, &items, &mut None));
    d.frame(|ui| menu(ui, &items, &mut None));
    d.hold(Direction::Down);
    d.frame(|ui| menu(ui, &items, &mut None));
    d.set_scene(2); // the menu's scene was replaced while Down is held
    let ids = d.frame(|ui| menu(ui, &items, &mut None));
    for _ in 0..40 {
      d.frame(|ui| menu(ui, &items, &mut None));
    }
    assert_eq!(d.frame(|ui| { menu(ui, &items, &mut None); ui.focused() }), Some(ids[1]), "still on the default");
  }

  #[test]
  fn a_trapped_scope_keeps_focus_in_and_an_open_one_lets_it_out() {
    let draw = |trap: bool| {
      move |ui: &mut UiContext<'_>| {
        let mut ids = Vec::new();
        ui.row().gap(20.0).show(|ui| {
          let mut left = ui.column().focus_scope();
          if trap {
            left = left.trap();
          }
          left.show(|ui| ids.push(ui.button("L").id(0).default_focus().show().id));
          ui.column().focus_scope().show(|ui| ids.push(ui.button("R").id(1).show().id));
        });
        ids
      }
    };
    for trap in [true, false] {
      let mut d = UiDriver::new((400.0, 300.0));
      let ids = d.frame(draw(trap));
      d.frame(draw(trap));
      d.nav(Direction::Right);
      d.frame(draw(trap));
      let expect = if trap { ids[0] } else { ids[1] };
      assert_eq!(d.frame(|ui| ui.focused()), Some(expect), "trap: {trap}");
    }
  }

  #[test]
  fn explicit_neighbours_override_geometry() {
    let mut d = UiDriver::new((300.0, 300.0));
    let mut quit = UiId::NONE;
    let draw = |quit_id: UiId| {
      move |ui: &mut UiContext<'_>| {
        let mut ids = Vec::new();
        ui.column().focus_scope().show(|ui| {
          ids.push(ui.button("A").id(0).default_focus().nav_down(quit_id).show().id);
          ids.push(ui.button("B").id(1).show().id);
          ids.push(ui.button("Quit").id(2).show().id);
        });
        ids
      }
    };
    let ids = d.frame(draw(quit));
    quit = ids[2];
    d.frame(draw(quit));
    d.nav(Direction::Down);
    d.frame(draw(quit));
    assert_eq!(d.frame(|ui| ui.focused()), Some(quit));
  }

  #[test]
  fn focus_memory_restores_a_hidden_panel() {
    let mut d = UiDriver::new((300.0, 300.0));
    let items = ["A", "B", "C"];
    let ids = d.frame(|ui| menu(ui, &items, &mut None));
    d.frame(|ui| menu(ui, &items, &mut None));
    d.nav(Direction::Down);
    d.frame(|ui| menu(ui, &items, &mut None));
    for _ in 0..3 {
      d.frame(|_| ()); // the menu is hidden
    }
    d.frame(|ui| menu(ui, &items, &mut None));
    d.frame(|ui| menu(ui, &items, &mut None));
    assert_eq!(d.frame(|ui| ui.focused()), Some(ids[2]), "back on C, not the default B");
  }

  #[test]
  fn focus_follows_the_mouse_but_the_ring_shows_only_for_keys_and_pads() {
    let mut d = UiDriver::new((300.0, 300.0));
    let items = ["A", "B", "C"];
    let ids = d.frame(|ui| menu(ui, &items, &mut None));
    d.frame(|ui| menu(ui, &items, &mut None));
    d.device(InputDevice::Mouse);
    d.move_to(10.0, 5.0);
    d.frame(|ui| menu(ui, &items, &mut None));
    assert_eq!(d.frame(|ui| ui.focused()), Some(ids[0]));
    assert!(!d.frame(|ui| ui.focus_visible()));
    d.device(InputDevice::Keyboard);
    assert!(d.frame(|ui| ui.focus_visible()));
  }

  #[test]
  fn focus_follows_a_keyed_item_through_a_reorder() {
    let mut d = UiDriver::new((300.0, 300.0));
    let draw = |order: [usize; 3]| {
      move |ui: &mut UiContext<'_>| {
        ui.column().focus_scope().show(|ui| {
          for key in order {
            let mut b = ui.button(format!("Item {key}")).id(key);
            if key == 2 {
              b = b.default_focus();
            }
            b.show();
          }
        });
      }
    };
    d.frame(draw([0, 1, 2]));
    d.frame(draw([0, 1, 2]));
    let focused = d.frame(|ui| ui.focused());
    d.frame(draw([2, 0, 1]));
    d.frame(draw([2, 0, 1]));
    assert_eq!(d.frame(|ui| ui.focused()), focused);
  }

  #[test]
  fn tab_walks_in_order_and_shift_goes_back() {
    let mut d = UiDriver::new((300.0, 300.0));
    let items = ["A", "B", "C"];
    let ids = d.frame(|ui| menu(ui, &items, &mut None));
    d.frame(|ui| menu(ui, &items, &mut None));
    d.tab(false);
    d.frame(|ui| menu(ui, &items, &mut None));
    assert_eq!(d.frame(|ui| ui.focused()), Some(ids[2]));
    d.tab(true);
    d.frame(|ui| menu(ui, &items, &mut None));
    assert_eq!(d.frame(|ui| ui.focused()), Some(ids[1]));
  }

  #[test]
  fn back_is_free_when_no_widget_uses_it() {
    let mut d = UiDriver::new((300.0, 300.0));
    d.frame(|ui| menu(ui, &["A"], &mut None));
    d.back();
    assert!(d.frame(|ui| {
      menu(ui, &["A"], &mut None);
      ui.back()
    }));
  }

  #[test]
  fn arrows_reach_gameplay_when_no_scope_is_shown() {
    let mut d = UiDriver::new((300.0, 300.0));
    // A HUD button outside any focus scope
    let hud = |ui: &mut UiContext<'_>| {
      ui.button("Map").at(crate::core::ui::Rect::new(0.0, 0.0, 60.0, 20.0)).show();
    };
    d.frame(hud);
    d.nav(Direction::Down);
    d.confirm();
    d.frame(hud);
    assert!(d.last_outcome().used.is_empty(), "{:?}", d.last_outcome().used);
    assert_eq!(d.frame(|ui| ui.focused()), None);
  }
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p ryo2d --lib ui::nav ui::widgets::button`
Expected: compile errors.

- [ ] **Step 3: Implement**

Write `nav.rs` and the rest by the rules above. In `tree.rs`:
- `open_node` sets each node's `scope` to the nearest open node with `FOCUS_SCOPE`, or the subtree's starting scope (the `UiContext`'s current scope when the subtree started), or `NONE`.
- A scope node's own `scope` field is its parent scope, which gives the scope chain.

`UiDriver` passes its scripted `NavInput` to `resolve_focus`, after `resolve_pointer` and `route_wheel`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ryo2d --lib ui::` and `cargo test --workspace`
Expected: pass. The games' gameplay reads the arrows and pad buttons in fixed steps; their scenes have no focus scopes yet, so nothing is consumed.

- [ ] **Step 5: Docs**

In `src/core/ui/AGENTS.md`, add "Focus and navigation":
- **Scopes.** Explicit scopes (`.focus_scope()`), popups and modals; default focus on appearance; focus memory per scope for the scene's lifetime; trap and wrap.
- **Movement.** The spatial rule; Tab order; overrides.
- **Pointer.** Focus follows the pointer; a ring only for keys and pads.
- **Actions.** Directions a widget takes; the actions the UI consumes when it uses them.

- [ ] **Step 6: Gate and commit**

```bash
git add src/core/ui/nav.rs src/core/ui/memory.rs src/core/ui/response.rs src/core/ui/tree.rs src/core/ui/widgets/common.rs src/core/ui/widgets/button.rs src/core/ui/immediate/context.rs src/core/ui/test_util.rs src/core/ui/AGENTS.md src/core/engine/frame.rs
git commit -m "feat: keyboard and gamepad UI navigation" -m "- focus scopes with default focus, memory, traps and wrapping; spatial moves with overrides; Tab order
- Confirm activates and Back is offered to widgets first; the UI takes only the actions it used
- focus follows the pointer; the focus ring shows for keyboard and gamepad only"
```

### Task 9: The theme

**Files:**
- Delete: `src/core/ui/theme.rs` (replaced)
- Create:
  - `src/core/ui/theme/mod.rs`: `Theme`, `UiTheme`, palette, metrics, text roles, skins, sounds.
  - `src/core/ui/theme/look.rs`: `Look`, `LookPatch`, `Looks`, `Variant`, `WidgetKind`, `WidgetState`.
  - `src/core/ui/theme/builtin.rs`: `dark`, `light`, `pixel`.
  - `src/core/ui/theme/file.rs`: RON load and save, and `ThemeError`.
- Modify:
  - Serde derives: `src/core/ui/layout/mod.rs` (on `Edges`) and `src/core/render/batch/key.rs` (on `Filter`).
  - `src/core/ui/immediate/context.rs`: an `Arc<Theme>` field; `theme()`; `with_theme`, `button_styled`, `draw_themed_panel`, `draw_themed_progress_bar` and `draw_focus_ring_themed` removed; old widgets keep their fallback colours.
  - Widgets reading the theme: `src/core/ui/widgets/{button,scroll}.rs`, `src/core/ui/containers.rs` (panel), the focus ring.
  - Timings from the theme's metrics: `src/core/ui/{interact,nav}.rs`.
  - Engine:
    - `src/core/engine/mod.rs`: `UiTheme` resource, `EngineBuilder::ui_theme`, skin sprite loading, hot reload;
    - `src/core/engine/frame.rs`;
    - `src/core/scene/context.rs`: `ctx.ui_theme()`.
  - Exports: `src/core/ui/mod.rs`, `src/lib.rs`.
  - Ports: `examples/games/bullet-hell/src/scenes/pause.rs` (`button_styled` → variants), `examples/demos/ui/src/scenes/theme_demo.rs`, `examples/demos/ui/assets/ui/theme.json` → `theme.ron`.
- Test: the theme modules' tests; `src/core/ui/widgets/button.rs`; `src/core/ui/containers.rs`

**Interfaces:**
- Consumes: the widgets and containers from Tasks 5-8, `Filter`, `SpriteHandle`, `TextStyle`, `FontRole`, `Color`, `Edges`.
- Produces (all `pub`, documented; `Debug, Clone, PartialEq, Serialize, Deserialize` with `#[serde(default)]` on every struct, so a RON file states only what it changes):

```rust
pub struct Theme { pub name: String, pub palette: Palette, pub metrics: Metrics, pub text: TextRoles, pub widgets: WidgetLooks,
  pub variants: BTreeMap<String, Variant>, pub skins: BTreeMap<String, Skin>, pub sounds: UiSounds, #[serde(skip)] source: Option<PathBuf> }
impl Theme {
  pub fn dark() -> Theme; pub fn light() -> Theme; pub fn pixel() -> Theme;   // Default = dark
  pub fn load(path: impl AsRef<Path>) -> Result<Theme, ThemeError>; pub fn save(&self, path: impl AsRef<Path>) -> Result<(), ThemeError>;
  pub fn look(&self, kind: WidgetKind, variant: Option<&str>, state: WidgetState) -> Look;
  pub fn variant(mut self, name: &str, variant: Variant) -> Theme;   // builder
  pub fn skin(mut self, name: &str, skin: Skin) -> Theme;            // builder
  pub fn source(&self) -> Option<&Path>;
}
pub struct Palette { pub background: Color, pub surface: Color, pub text: Color, pub muted: Color, pub accent: Color, pub danger: Color, pub success: Color, pub warning: Color }
pub struct Metrics { pub radius: f32, pub spacing: f32, pub border: f32, pub focus_ring: FocusRing, pub scroll_line: f32, pub scroll_smoothing: f32,
  pub repeat_delay: f32, pub repeat_rate: f32, pub tooltip_delay: f32, pub drag_threshold: f32, pub double_click: f32, pub sizes: Sizes }
pub struct FocusRing { pub width: f32, pub offset: f32, pub color: Color }
pub struct Sizes { pub button_padding: [f32; 2], pub button_min_height: f32, pub panel_padding: f32, pub slider: [f32; 2], pub slider_handle: [f32; 2],
  pub toggle: [f32; 2], pub checkbox: f32, pub scrollbar: f32, pub slot: f32, pub text_input: [f32; 2] }
pub struct TextRoles { pub body: TextLook, pub heading: TextLook, pub button: TextLook, pub caption: TextLook, pub mono: TextLook }
pub struct TextLook { pub size: f32, pub color: Color, pub font: ThemeFont, pub bold: bool }
impl TextLook { pub fn style(&self) -> TextStyle }
pub enum ThemeFont { Ui, Mono, Pixel }
pub enum WidgetKind { Button, Panel, Checkbox, Toggle, Radio, Slider, TextInput, Dropdown, Tab, Choice, Tooltip, Scroll, Slot, Progress, Rebind, Modal, Prompt }
pub enum WidgetState { Normal, Hovered, Pressed, Focused, Disabled }
pub struct Look { pub fill: Color, pub border: Color, pub border_width: f32, pub radius: f32, pub text: Color, pub padding: Edges, pub offset: [f32; 2], pub skin: Option<String> }
pub struct LookPatch { pub fill: Option<Color>, pub border: Option<Color>, pub border_width: Option<f32>, pub radius: Option<f32>, pub text: Option<Color>,
  pub padding: Option<Edges>, pub offset: Option<[f32; 2]>, pub skin: Option<String> }
impl LookPatch { pub fn fill(self, c: Color) -> Self; pub fn border(self, c: Color) -> Self; pub fn text(self, c: Color) -> Self; pub fn radius(self, r: f32) -> Self; pub fn skin(self, name: &str) -> Self }
pub struct Looks { pub normal: Look, pub hovered: LookPatch, pub pressed: LookPatch, pub focused: LookPatch, pub disabled: LookPatch }
impl Looks { pub fn resolve(&self, state: WidgetState) -> Look }
pub struct WidgetLooks { pub button: Looks, pub panel: Looks, pub checkbox: Looks, pub toggle: Looks, pub radio: Looks, pub slider: Looks, pub text_input: Looks,
  pub dropdown: Looks, pub tab: Looks, pub choice: Looks, pub tooltip: Looks, pub scroll: Looks, pub slot: Looks, pub progress: Looks, pub rebind: Looks, pub modal: Looks, pub prompt: Looks }
impl WidgetLooks { pub fn get(&self, kind: WidgetKind) -> &Looks }
pub struct Variant { pub base: WidgetKind, pub normal: LookPatch, pub hovered: LookPatch, pub pressed: LookPatch, pub focused: LookPatch, pub disabled: LookPatch }
impl Variant { pub fn of(base: WidgetKind) -> Variant; pub fn normal(self, p: LookPatch) -> Self; pub fn hovered(self, p: LookPatch) -> Self; pub fn pressed(self, p: LookPatch) -> Self; pub fn focused(self, p: LookPatch) -> Self }
pub struct Skin { pub path: Option<String>, pub insets: [f32; 4], pub filter: Filter, #[serde(skip)] pub sprite: Option<SpriteHandle> }
impl Skin { pub fn from_sprite(sprite: SpriteHandle, insets: [f32; 4], filter: Filter) -> Skin; pub fn from_path(path: &str, insets: [f32; 4], filter: Filter) -> Skin }
pub struct UiSounds { pub focus: Option<String>, pub confirm: Option<String>, pub back: Option<String>, pub tick: Option<String>, pub error: Option<String>, pub volume: f32 }
pub enum ThemeError { Io { path: PathBuf, error: String }, Parse { path: PathBuf, error: String }, Serialize { error: String } }   // Display, Error
/// Debug-build hot reload of the theme's source file (file.rs)
#[derive(Default)] pub(crate) struct ThemeWatch { /* last check time, the file's last modified time, the change last warned about */ }
impl ThemeWatch { pub(crate) fn poll(&mut self, theme: &mut UiTheme, now: f64); }
#[derive(Resource)] pub struct UiTheme;   // Default: Theme::dark()
impl UiTheme { pub fn new(theme: Theme) -> UiTheme; pub fn get(&self) -> &Theme; pub fn set(&mut self, theme: Theme); pub fn modify(&mut self, change: impl FnOnce(&mut Theme)); pub(crate) fn shared(&self) -> Arc<Theme>; pub(crate) fn generation(&self) -> u64 }
// engine and scenes
impl EngineBuilder { pub fn ui_theme(self, theme: Theme) -> Self }
impl SceneContext<'_> { pub fn ui_theme(&mut self) -> Mut<'_, UiTheme> }
impl UiContext<'_> { pub fn theme(&self) -> &Theme }
// widget builders (widget_builders! gains)
pub fn variant(self, name: &str) -> Self;  pub fn style(self, patch: LookPatch) -> Self;
```

**The rules:**
- **Resolving a look.**
  - `Looks::resolve(state)` starts from `normal` and applies the state's patch field by field (`Some` replaces).
  - `Pressed` also applies `hovered` first, and `Focused` applies only its own patch over `normal`.
  - `Theme::look(kind, variant, state)` resolves the kind's look, then applies the variant's `normal` and state patches over it.
  - An unknown variant name warns once (`engine_warn!(module: "ui", "no UI variant named {name:?}; using the plain look")`) and uses the kind's look.
  - A per-call `.style(patch)` applies last, in every state.
- **Which state a widget is in.** `Disabled` when disabled, else `Pressed` when pressed, else `Hovered` when hovered, else `Focused` when focused and the ring is visible, else `Normal`.
- **Skins.**
  - A look with `skin: Some(name)` draws its skin's sprite as a nine-slice in place of its fill and border, through a new `DrawKind::NineSlice { inset, sprite, insets, tint }`.
  - The tint is the look's `fill`, applied to the sprite: white keeps its colours.
  - The skin's `filter` is used; Task 10 adds `filter` to `UiCommand::Sprite`, so until then nine-slices use `Filter::Default`.
  - The engine loads skin sprites from `path` through `AssetManager::load_sprite` whenever the theme's generation changes, before the pass. It stores the results in its working copy (`UiTheme` keeps the resolved `Arc`).
  - A skin with neither a path nor a sprite draws the plain look and warns once.
- **Text.** Widgets draw their labels with `theme.text.button.style()`, or `body` for body text. The text colour comes from the look. Plain `ui.text(..)` and the `draw_text*` shorthands keep 4a's default style.
- **Metrics** replace Tasks 6-8's constants:
  - drag threshold, double click, repeat delay and rate;
  - tooltip delay (Task 10);
  - scroll line and smoothing;
  - the focus ring's width, offset and colour;
  - every widget size listed in `Sizes`.
- **`dark()`** keeps today's colours:
  - **Button:** normal fill `rgb(40,40,60)`, border `rgb(80,80,100)`; hovered fill `rgb(50,50,70)`, border `rgb(100,100,130)`; pressed fill `rgb(60,60,80)`; disabled fill `rgba(40,40,60,120)` and text `rgba(255,255,255,120)`; text white; radius 4; border width 1; padding `[12, 6]`.
  - **Panel:** fill `rgba(20,20,30,200)`, border `rgb(80,80,100)`.
  - **Focus ring:** `rgb(100,140,200)`, 2 wide, offset 2.
  - **The rest:** every other kind takes its colours from the matching old style struct's `Default` in `theme.rs` as it is before this task. For example, the slider track is `rgb(60,60,80)` with fill `rgb(80,150,255)`, and the text input's focused border is `rgb(100,140,200)`.
  - **Palette:** `accent` = `rgb(80,150,255)`, `danger` = `rgb(220,70,70)`, `success` = `rgb(80,190,110)`, `warning` = `rgb(230,180,60)`.
  - **Variants:** `"danger"` (a danger-filled button), `"accent"` and `"tab"`.
- **`light()`** is the light counterpart: today's `Theme::light()` colours.
- **`pixel()`:**
  - text roles on `ThemeFont::Pixel` at 16;
  - radius 0;
  - a button skin and a panel skin made in code: 12×12 RGBA, a 1-pixel dark border with a lighter top-left bevel, insets `[4, 4, 4, 4]`, `Filter::Nearest`. Build them with `SpriteHandle::from_sprite(Sprite::from_rgba(..), "ryo2d-pixel-button")`.
- **Files.** RON through `ron::de::from_str` and `ron::ser::to_string_pretty`. `load` stores `source`.
- **Hot reload.** `ThemeWatch::poll` checks the theme's `source` modification time at most once a second (by `now`, in seconds of real time). The first poll only remembers the time. When the time changes, it reloads the file and `set`s the theme. A load error warns once per change (`engine_warn!(module: "ui", "could not reload the UI theme {path}: {error}; keeping the old one")`) and keeps the old theme. The engine owns a `ThemeWatch` and polls it each frame in debug builds only.
- **Names.** The built-ins are named "dark", "light" and "pixel".
- **The `UiContext`** takes `Arc<Theme>` from `UiTheme::shared()` at the start of each pass. `UiTheme` is inserted by `init_world_resources` with `Theme::dark()`, and replaced by `EngineBuilder::ui_theme`.

- [ ] **Step 1: Write the failing tests**

`src/core/ui/theme/look.rs`:

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ui::Color;

  fn looks() -> Looks {
    Looks {
      normal: Look { fill: Color::rgb(10, 10, 10), border: Color::rgb(20, 20, 20), text: Color::WHITE, ..Look::default() },
      hovered: LookPatch::default().fill(Color::rgb(30, 30, 30)),
      pressed: LookPatch::default().border(Color::rgb(90, 90, 90)),
      focused: LookPatch::default(),
      disabled: LookPatch::default().text(Color::GRAY),
    }
  }

  #[test]
  fn a_state_patch_overrides_only_its_fields() {
    let looks = looks();
    let hovered = looks.resolve(WidgetState::Hovered);
    assert_eq!((hovered.fill, hovered.border), (Color::rgb(30, 30, 30), Color::rgb(20, 20, 20)));
    let pressed = looks.resolve(WidgetState::Pressed);
    assert_eq!((pressed.fill, pressed.border), (Color::rgb(30, 30, 30), Color::rgb(90, 90, 90)), "pressed builds on hovered");
    assert_eq!(looks.resolve(WidgetState::Disabled).text, Color::GRAY);
  }

  #[test]
  fn a_variant_extends_its_base() {
    let theme = Theme::dark().variant("gold", Variant::of(WidgetKind::Button).normal(LookPatch::default().fill(Color::YELLOW)));
    let gold = theme.look(WidgetKind::Button, Some("gold"), WidgetState::Normal);
    let plain = theme.look(WidgetKind::Button, None, WidgetState::Normal);
    assert_eq!(gold.fill, Color::YELLOW);
    assert_eq!(gold.border, plain.border, "the rest is the base's");
  }

  #[test]
  fn an_unknown_variant_warns_once_and_uses_the_plain_look() {
    let theme = Theme::dark();
    let plain = theme.look(WidgetKind::Button, None, WidgetState::Normal);
    for _ in 0..3 {
      assert_eq!(theme.look(WidgetKind::Button, Some("nope-variant"), WidgetState::Normal), plain);
    }
    let warnings = crate::core::dev_log::get_entries().iter().filter(|e| e.message.contains("nope-variant")).count();
    assert_eq!(warnings, 1);
  }
}
```

`src/core/ui/theme/file.rs`:

```rust
#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_theme_round_trips_through_ron() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("theme.ron");
    let theme = Theme::light();
    theme.save(&path).unwrap();
    let loaded = Theme::load(&path).unwrap();
    assert_eq!(loaded.palette, theme.palette);
    assert_eq!(loaded.widgets, theme.widgets);
    assert_eq!(loaded.source(), Some(path.as_path()));
  }

  #[test]
  fn a_partial_file_fills_the_rest_from_dark() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("theme.ron");
    std::fs::write(&path, "(name: \"mine\", palette: (accent: (r: 1, g: 2, b: 3, a: 255)))").unwrap();
    let theme = Theme::load(&path).unwrap();
    assert_eq!(theme.name, "mine");
    assert_eq!(theme.palette.accent, Color::new(1, 2, 3, 255));
    assert_eq!(theme.widgets, Theme::dark().widgets);
  }

  #[test]
  fn a_bad_file_is_an_error_naming_the_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("broken.ron");
    std::fs::write(&path, "(palette: (accent: nonsense").unwrap();
    let error = Theme::load(&path).unwrap_err();
    assert!(error.to_string().contains("broken.ron"), "{error}");
  }

  /// Write `text` and move the file's time forward, so the change shows on any filesystem
  fn edit(path: &std::path::Path, text: &str, seconds: u64) {
    std::fs::write(path, text).unwrap();
    let file = std::fs::File::options().write(true).open(path).unwrap();
    file.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(seconds)).unwrap();
  }

  #[test]
  fn a_changed_file_reloads_at_most_once_a_second() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("theme.ron");
    Theme::dark().save(&path).unwrap();
    let mut theme = UiTheme::new(Theme::load(&path).unwrap());
    let mut watch = ThemeWatch::default();
    watch.poll(&mut theme, 0.0);
    edit(&path, "(name: \"edited\")", 5);
    watch.poll(&mut theme, 0.5);
    assert_eq!(theme.get().name, "dark", "checked at most once a second");
    watch.poll(&mut theme, 1.5);
    assert_eq!(theme.get().name, "edited");
    assert_eq!(theme.get().source(), Some(path.as_path()), "the reloaded theme still watches its file");
  }

  #[test]
  fn a_broken_edit_keeps_the_old_theme_and_warns_once() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("watched-broken.ron");
    Theme::light().save(&path).unwrap();
    let mut theme = UiTheme::new(Theme::load(&path).unwrap());
    let mut watch = ThemeWatch::default();
    watch.poll(&mut theme, 0.0);
    edit(&path, "(palette: nonsense", 5);
    for second in 1..5 {
      watch.poll(&mut theme, f64::from(second) + 0.5);
    }
    assert_eq!(theme.get().name, "light");
    let warnings = crate::core::dev_log::get_entries().iter().filter(|e| e.message.contains("watched-broken.ron")).count();
    assert_eq!(warnings, 1, "one warning for one bad change");
  }
}
```

`src/core/ui/containers.rs` (add to its tests):

```rust
  #[test]
  fn a_skinned_panel_draws_a_nine_slice_in_place_of_its_fill() {
    use crate::core::asset::SpriteHandle;
    use crate::core::graphics::Sprite;
    use crate::core::render::batch::Filter;
    use crate::core::ui::test_util::UiDriver;
    use crate::core::ui::theme::{Skin, Theme};
    let sprite = SpriteHandle::from_sprite(Sprite::solid_rect(12, 12, 255, 255, 255, 255), "test-skin");
    let mut theme = Theme::dark().skin("frame", Skin::from_sprite(sprite, [4.0; 4], Filter::Nearest));
    theme.widgets.panel.normal.skin = Some("frame".to_string());
    let mut d = UiDriver::new((200.0, 100.0)).with_theme(theme);
    d.frame(|ui| {
      ui.panel().at(Rect::new(10.0, 10.0, 80.0, 40.0)).show(|_| {});
    });
    let sprites = d.commands.iter().filter(|(_, c)| matches!(c, UiCommand::Sprite { .. })).count();
    let rects = d.commands.iter().filter(|(_, c)| matches!(c, UiCommand::Rect { .. })).count();
    assert_eq!((sprites, rects), (9, 0), "nine sprite pieces and no plain fill");
  }
```

`src/core/ui/widgets/button.rs` (add; `UiDriver::with_theme(Theme)` sets the driver's theme):

```rust
  #[test]
  fn a_button_takes_its_colours_from_the_theme() {
    use crate::core::ui::immediate::UiCommand;
    use crate::core::ui::theme::{LookPatch, Theme, Variant, WidgetKind};
    use crate::core::ui::Color;
    let theme = Theme::dark().variant("go", Variant::of(WidgetKind::Button).normal(LookPatch::default().fill(Color::GREEN)));
    let mut d = UiDriver::new((200.0, 100.0)).with_theme(theme.clone());
    let at = crate::core::ui::Rect::new(10.0, 10.0, 80.0, 30.0);
    d.frame(|ui| {
      ui.button("Plain").at(at).show();
      ui.button("Go").at(at).variant("go").show();
    });
    let fills: Vec<Color> = d
      .commands
      .iter()
      .filter_map(|(_, c)| match c {
        UiCommand::Rect { fill, .. } => Some(*fill),
        _ => None,
      })
      .collect();
    let plain = theme.look(WidgetKind::Button, None, crate::core::ui::theme::WidgetState::Normal).fill;
    assert_eq!(fills, vec![plain, Color::GREEN]);
  }
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p ryo2d --lib ui::theme ui::widgets`
Expected: compile errors.

- [ ] **Step 3: Implement**

Write the theme modules and wire the theme through the engine, `SceneContext` and `UiContext`.

**Widgets that read the theme:**
- The button: the button looks and text role, and `Sizes::button_*`.
- The panel: the panel looks, with `Sizes::panel_padding` as its default padding.
- The scrollbar: the scroll looks (`fill` is the track, `border` the thumb, and hovered/pressed the thumb's states), with `Sizes::scrollbar` as its width.
- The focus ring: `metrics.focus_ring`.

**Old widgets.** The old checkbox, toggle, slider, text input and dropdown lose their `if let Some(theme)` branches and keep their fallback colours until their tasks.

**Ports:**
- **bullet-hell pause.** Its `get_btn_colors` closure becomes two variants, set on the theme in `on_enter` with `ctx.ui_theme().modify(..)`. `"pause-item"` is the normal and hovered colours; `"pause-selected"` is the selected (green) mode. Each `button_styled(label, rect, ..)` becomes `ui.button(label).at(rect).variant(..).clicked()`. The hand-rolled focus rectangles stay until Task 16.
- **The theme demo.** It cycles `ctx.ui_theme().set(..)` through Dark, Light, Neon and a file theme. Neon is built in code with `Theme::dark()` plus palette changes and variants. The file theme is `Theme::load("assets/ui/theme.ron")`: convert `assets/ui/theme.json` to RON in the new shape, and delete the JSON. The T key cycles themes; I toggles the instructions, as now.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ryo2d --lib ui::` and `cargo test --workspace` and `cargo build --workspace --all-targets`
Expected: pass.

- [ ] **Step 5: Docs**

In `src/core/ui/AGENTS.md`, replace the theme notes with "Theme":
- `UiTheme` in the world, read once per pass;
- looks per widget kind and state, inheriting from normal;
- variants, skins, metrics and sounds (sounds land in Task 15);
- RON files with hot reload in debug builds.

- [ ] **Step 6: Gate and commit**

Stage the new and changed files by name, including `git rm src/core/ui/theme.rs` and the JSON theme.

```bash
git commit -m "feat: a UI theme with per-state looks, variants and skins" -m "- the theme is a world resource every widget reads; looks per widget kind and state inherit from normal
- named variants and per-call styles; nine-slice skins; dark, light and pixel themes
- RON theme files, hot-reloaded in debug builds"
```

### Task 10: Checkbox, toggle, slider, radio, progress, image, separator, spacer, tooltips, disabled

**Files:**
- Create: `src/core/ui/widgets/{checkbox,toggle,slider,radio,progress,image,separator,tooltip}.rs`
- Modify:
  - `src/core/ui/tree.rs`: `DrawKind::Fraction`; `DrawKind::Sprite` and `NineSlice` gain `filter`.
  - `src/core/ui/immediate/commands.rs`: `UiCommand::Sprite` gains `filter`.
  - `src/core/render/ui.rs`: the sprite arm uses it.
  - `src/core/ui/widgets/common.rs`: `.tooltip(..)`.
  - `src/core/ui/immediate/context.rs`: the old `slider`, `checkbox` and `toggle` are removed; tooltips flush in `end_scene`.
  - `src/core/text/`: a crate `TextLayout::text()` accessor (the source text it laid out), for tests.
  - Ports:
    - `examples/games/breakout/src/scenes/pause.rs`: the 3 sliders;
    - `examples/demos/ui/src/scenes/{theme_demo,layout_demo}.rs`.
- Test: each new widget file's tests module

**Interfaces:**
- Consumes:
  - Tasks 5-9: the subtree and draws, `Response`, `UiDriver`, `nav_step_for`, `WidgetKind`/`WidgetState`/`Theme::look`, `Sizes`, `metrics.tooltip_delay`.
  - `widget_builders!`.
- Produces:

```rust
pub fn checkbox<'u>(&'u mut self, checked: &'u mut bool, label: impl Into<RichText>) -> Checkbox<'u, 'a>;
pub fn toggle<'u>(&'u mut self, on: &'u mut bool, label: impl Into<RichText>) -> Toggle<'u, 'a>;
pub fn slider<'u>(&'u mut self, value: &'u mut f32, range: RangeInclusive<f32>) -> Slider<'u, 'a>;
impl Slider<'_, '_> { pub fn step(self, step: f32) -> Self; pub fn show_value(self) -> Self; pub fn label(self, label: impl Into<RichText>) -> Self }
pub fn radio<'u, T: PartialEq + Copy>(&'u mut self, current: &'u mut T, value: T, label: impl Into<RichText>) -> Radio<'u, 'a, T>;
pub fn progress<'u>(&'u mut self, fraction: f32) -> Progress<'u, 'a>;
impl Progress<'_, '_> { pub fn label(self, label: impl Into<RichText>) -> Self }
pub fn image<'u>(&'u mut self, sprite: &SpriteHandle) -> Image<'u, 'a>;
impl Image<'_, '_> { pub fn size(self, width: f32, height: f32) -> Self; pub fn tint(self, color: Color) -> Self; pub fn filter(self, filter: Filter) -> Self; pub fn uv(self, uv: [f32; 4]) -> Self }
pub fn separator(&mut self) -> Response;
pub fn spacer(&mut self) -> Response;
// every builder: .tooltip(text: impl Into<RichText>)
// every widget builder has show() -> Response, runs on drop, and gets widget_builders!
pub(crate) enum DrawKind { /* adds */ Fraction { inset: Edges, to: f32, fill: Color, radius: f32 } }
```

**The rules:**
- **Checkbox and toggle.**
  - Each is a row: the box (`Sizes::checkbox` square, or the `Sizes::toggle` track with a round knob), a gap of `metrics.spacing`, and the label in the body text role.
  - The row records `CLICK | FOCUSABLE`.
  - A click or a Confirm flips the bool and marks `changed`.
  - The box draws the checkbox (or toggle) looks; checked uses `palette.accent` as its fill, with a check mark drawn as two `Local` line segments (or a knob moved to the right).
- **Slider.**
  - It is a row of an optional label, the track, and an optional value text (`format!("{:.*}", decimals, value)`, where `decimals` comes from the step: 0 when the step ≥ 1, else enough to show it).
  - **The track node** is `Sizes::slider` (width × height), records `CLICK | DRAG | FOCUSABLE | TAKES_HORIZONTAL`, and draws:
    - a `Fill`, inset vertically to a 6-unit track, in the look's `fill`;
    - a `Fraction` from 0 to `t`, inset by half the handle width on each side, in `palette.accent`;
    - a handle `Box` of `Sizes::slider_handle` at frac `(t, 0.5)` over the same inset, in the look's `border` (hovered and pressed patches apply).
  - **Dragging.** While the track is active, `t = ((pointer.x - (rect.x + hw / 2)) / (rect.width - hw)).clamp(0, 1)`, where `rect` is last frame's track rect and `hw` the handle width. A press on the track jumps there.
  - **Keys and pad.** `nav_step_for(id)` Left/Right moves the value by the step. The default step is `(max - min) / 20`.
  - **Writing the value.** It is snapped to `min + round((v - min) / step) * step` when a step is set, clamped, and written to `*value`. Any change marks `changed`.
  - **Bad input.** A range with `min > max` is swapped. A non-finite bound makes a 0..1 range and warns once. A non-finite `*value` becomes `min`.
- **Radio.** A circle of `Sizes::checkbox`, a filled dot when `*current == value`, and the label. A click or Confirm sets `*current = value` and marks `changed` if it differed.
- **Progress.**
  - A track (160 × 12 by default; `.width()`/`.height()` override it) with a `Fraction` fill in `palette.accent`, and an optional label centred over it.
  - The fraction is clamped to `[0, 1]`, with NaN as 0.
  - Not focusable; no record unless it has a tooltip.
- **Image.**
  - A leaf of the sprite's size (or `.size`), with a `Sprite` draw: the tint (default white), the UV (default `[0, 0, 1, 1]`) and the filter (default `Filter::Default`).
  - `UiCommand::Sprite` gains `filter: Filter`, which the `emit_ui` sprite arm uses in its `SpriteLook`. Absolute `draw_sprite*` keep `Filter::Default`.
- **Separator.** A 1-unit line in `palette.muted` at 50% alpha. It fills the parent's cross axis: horizontal in a column, vertical in a row. The open parent's `Flow` is known at `open_node`.
- **Spacer.** A leaf with `grow: 1.0`.
- **Tooltips.**
  - Timing:
    - A widget with `.tooltip(t)` keeps a `TooltipTimer` state that adds `dt` while it is hovered, or focused with the ring visible.
    - Once `metrics.tooltip_delay` is reached, it sets `ui.pending_tooltip = Some((t, rect))`.
    - A press, or the widget losing hover and focus, resets it.
  - Drawing: `end_scene` draws the pending tooltip on `POPUP_LAYER` as its own top-level subtree.
    - It is a column with the tooltip look and `TextRoles::caption` text, `max_width` 280.
    - It sits below the widget's rect (`rect.bottom() + 6`), flipped above when it would leave the UI's bottom, and moved left to stay on screen.
    - Only the last pending tooltip of the top scene draws.
- **Disabled.** Every widget in this task honours `.enabled(false)`: the `Disabled` look, the `DISABLED` flag, no value changes and no focus.

- [ ] **Step 1: Write the failing tests**

`src/core/ui/widgets/slider.rs`:

```rust
#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use crate::core::input::Direction;
  use crate::core::ui::test_util::UiDriver;
  use crate::core::ui::{Rect, UiContext};

  const TRACK: Rect = Rect::new(20.0, 20.0, 112.0, 20.0);

  fn slider(ui: &mut UiContext<'_>, value: &mut f32) -> bool {
    let mut changed = false;
    ui.column().focus_scope().show(|ui| changed = ui.slider(value, 0.0..=1.0).at(TRACK).default_focus().show().changed());
    changed
  }

  #[test]
  fn dragging_a_slider_sets_its_value() {
    let mut d = UiDriver::new((200.0, 100.0));
    let mut v = 0.0;
    d.frame(|ui| slider(ui, &mut v));
    // The handle is 12 wide: x 26 is t = 0, x 126 is t = 1
    d.move_to(76.0, 30.0);
    d.press();
    assert!(d.frame(|ui| slider(ui, &mut v)));
    assert!((v - 0.5).abs() < 0.01, "{v}");
    d.move_to(500.0, 30.0);
    d.frame(|ui| slider(ui, &mut v));
    assert_eq!(v, 1.0, "clamped");
  }

  #[test]
  fn left_and_right_step_a_focused_slider() {
    let mut d = UiDriver::new((200.0, 100.0));
    let mut v = 0.5;
    d.frame(|ui| slider(ui, &mut v));
    d.frame(|ui| slider(ui, &mut v));
    d.nav(Direction::Right);
    d.frame(|ui| slider(ui, &mut v));
    assert!((v - 0.55).abs() < 1e-5, "a twentieth: {v}");
    d.nav(Direction::Left);
    d.frame(|ui| slider(ui, &mut v));
    assert!((v - 0.5).abs() < 1e-5);
    assert!(d.last_outcome().used.contains(&"ui.left"), "the slider took Left, so gameplay does not see it");
  }

  #[test]
  fn a_slider_snaps_to_its_step() {
    let mut d = UiDriver::new((200.0, 100.0));
    let mut v = 0.0;
    let draw = |ui: &mut UiContext<'_>, v: &mut f32| {
      ui.slider(v, 0.0..=10.0).step(2.5).at(TRACK).show();
    };
    d.frame(|ui| draw(ui, &mut v));
    d.move_to(60.0, 30.0); // t = 0.34
    d.press();
    d.frame(|ui| draw(ui, &mut v));
    assert_eq!(v, 2.5);
  }

  #[test]
  fn a_bad_range_or_value_is_made_safe() {
    let mut d = UiDriver::new((200.0, 100.0));
    let mut v = f32::NAN;
    d.frame(|ui| {
      ui.slider(&mut v, 5.0..=1.0).at(TRACK).show();
    });
    assert_eq!(v, 1.0, "NaN becomes the minimum of the swapped range");
  }

  #[test]
  fn a_disabled_slider_ignores_input() {
    let mut d = UiDriver::new((200.0, 100.0));
    let mut v = 0.2;
    let draw = |ui: &mut UiContext<'_>, v: &mut f32| {
      ui.slider(v, 0.0..=1.0).at(TRACK).enabled(false).show();
    };
    d.frame(|ui| draw(ui, &mut v));
    d.move_to(100.0, 30.0);
    d.press();
    d.frame(|ui| draw(ui, &mut v));
    assert_eq!(v, 0.2);
  }
}
```

`src/core/ui/widgets/checkbox.rs`:

```rust
#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use crate::core::ui::test_util::UiDriver;
  use crate::core::ui::{Rect, UiContext};

  #[test]
  fn clicking_a_checkbox_toggles_it_and_reports_changed() {
    let mut d = UiDriver::new((200.0, 100.0));
    let mut on = false;
    let draw = |ui: &mut UiContext<'_>, on: &mut bool| ui.checkbox(on, "Sound").at(Rect::new(10.0, 10.0, 120.0, 24.0)).show().changed();
    d.frame(|ui| draw(ui, &mut on));
    d.move_to(15.0, 20.0);
    d.press();
    d.frame(|ui| draw(ui, &mut on));
    d.release();
    assert!(d.frame(|ui| draw(ui, &mut on)));
    assert!(on);
  }

  #[test]
  fn confirm_toggles_a_focused_checkbox() {
    let mut d = UiDriver::new((200.0, 100.0));
    let mut on = true;
    let draw = |ui: &mut UiContext<'_>, on: &mut bool| {
      ui.column().focus_scope().show(|ui| {
        ui.checkbox(on, "Music").default_focus().show();
      });
    };
    d.frame(|ui| draw(ui, &mut on));
    d.frame(|ui| draw(ui, &mut on));
    d.confirm();
    d.frame(|ui| draw(ui, &mut on));
    assert!(!on);
  }
}
```

`src/core/ui/widgets/toggle.rs`:

```rust
#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use crate::core::ui::test_util::UiDriver;
  use crate::core::ui::{Rect, UiContext};

  fn click(d: &mut UiDriver, on: &mut bool) -> bool {
    let draw = |ui: &mut UiContext<'_>, on: &mut bool| ui.toggle(on, "Music").at(Rect::new(10.0, 10.0, 120.0, 24.0)).show().changed();
    d.move_to(20.0, 20.0);
    d.press();
    d.frame(|ui| draw(ui, on));
    d.release();
    d.frame(|ui| draw(ui, on))
  }

  #[test]
  fn a_toggle_flips_on_click() {
    let mut d = UiDriver::new((200.0, 100.0));
    let mut on = false;
    d.frame(|ui| {
      ui.toggle(&mut on, "Music").at(Rect::new(10.0, 10.0, 120.0, 24.0)).show();
    });
    assert!(click(&mut d, &mut on));
    assert!(on);
    assert!(click(&mut d, &mut on));
    assert!(!on);
  }
}
```

`src/core/ui/widgets/radio.rs`:

```rust
#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use crate::core::ui::test_util::UiDriver;
  use crate::core::ui::{Rect, UiContext};

  #[derive(Debug, Clone, Copy, PartialEq)]
  enum Size {
    Small,
    Medium,
    Large,
  }

  /// Three 30-tall radios from y 0; whether any changed
  fn radios(ui: &mut UiContext<'_>, size: &mut Size) -> bool {
    let mut changed = false;
    ui.column().gap(0.0).at(Rect::new(0.0, 0.0, 200.0, 90.0)).show(|ui| {
      for (value, label) in [(Size::Small, "Small"), (Size::Medium, "Medium"), (Size::Large, "Large")] {
        changed |= ui.radio(size, value, label).id(label).height(30.0).show().changed();
      }
    });
    changed
  }

  fn click_third(d: &mut UiDriver, size: &mut Size) -> bool {
    d.move_to(10.0, 75.0);
    d.press();
    d.frame(|ui| radios(ui, size));
    d.release();
    d.frame(|ui| radios(ui, size))
  }

  #[test]
  fn a_radio_sets_its_value() {
    let mut d = UiDriver::new((200.0, 100.0));
    let mut size = Size::Small;
    d.frame(|ui| radios(ui, &mut size));
    assert!(click_third(&mut d, &mut size));
    assert_eq!(size, Size::Large);
    assert!(!click_third(&mut d, &mut size), "already Large: no change");
  }
}
```

`src/core/ui/widgets/progress.rs`:

```rust
#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use crate::core::ui::immediate::UiCommand;
  use crate::core::ui::test_util::UiDriver;
  use crate::core::ui::Rect;

  /// The widths of the rects a 160-wide bar draws at `fraction`: the track, then the fill
  fn widths(fraction: f32) -> Vec<f32> {
    let mut d = UiDriver::new((300.0, 100.0));
    d.frame(|ui| {
      ui.progress(fraction).at(Rect::new(0.0, 0.0, 160.0, 12.0)).show();
    });
    d.commands
      .iter()
      .filter_map(|(_, c)| match c {
        UiCommand::Rect { rect, .. } => Some(rect.width),
        _ => None,
      })
      .collect()
  }

  #[test]
  fn progress_clamps_its_fraction() {
    assert_eq!(widths(0.5), vec![160.0, 80.0]);
    assert_eq!(widths(2.0), vec![160.0, 160.0]);
    assert_eq!(widths(f32::NAN), vec![160.0], "NaN is 0, and an empty fill draws nothing");
  }
}
```

`src/core/ui/widgets/image.rs`:

```rust
#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use crate::core::asset::SpriteHandle;
  use crate::core::graphics::Sprite;
  use crate::core::render::batch::Filter;
  use crate::core::ui::immediate::UiCommand;
  use crate::core::ui::test_util::UiDriver;
  use crate::core::ui::Rect;

  #[test]
  fn an_image_draws_its_sprite_with_its_filter() {
    let sprite = SpriteHandle::from_sprite(Sprite::solid_rect(8, 8, 255, 255, 255, 255), "test-image");
    let mut d = UiDriver::new((100.0, 100.0));
    d.frame(|ui| {
      ui.image(&sprite).filter(Filter::Nearest).at(Rect::new(4.0, 4.0, 32.0, 32.0)).show();
    });
    let drawn: Vec<(Rect, Filter)> = d
      .commands
      .iter()
      .filter_map(|(_, c)| match c {
        UiCommand::Sprite { rect, filter, .. } => Some((*rect, *filter)),
        _ => None,
      })
      .collect();
    assert_eq!(drawn, vec![(Rect::new(4.0, 4.0, 32.0, 32.0), Filter::Nearest)]);
  }
}
```

`src/core/ui/widgets/tooltip.rs`:

```rust
#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use crate::core::ui::immediate::{POPUP_LAYER, UiCommand};
  use crate::core::ui::test_util::UiDriver;
  use crate::core::ui::{Rect, UiContext};

  fn popup_texts(d: &UiDriver) -> Vec<String> {
    d.commands
      .iter()
      .filter_map(|(_, c)| match c {
        UiCommand::Text { layout, layer, .. } if *layer == POPUP_LAYER => Some(layout.text().to_string()),
        _ => None,
      })
      .collect()
  }

  fn draw(ui: &mut UiContext<'_>) {
    ui.button("Save").at(Rect::new(10.0, 10.0, 80.0, 30.0)).tooltip("Saves the game").show();
  }

  #[test]
  fn a_tooltip_appears_after_the_delay_and_hides_on_press() {
    let mut d = UiDriver::new((300.0, 200.0));
    d.frame(draw);
    d.move_to(20.0, 20.0);
    for _ in 0..28 {
      d.frame(draw);
    }
    assert!(popup_texts(&d).is_empty(), "not before 0.5 s");
    for _ in 0..4 {
      d.frame(draw);
    }
    assert_eq!(popup_texts(&d), vec!["Saves the game".to_string()]);
    d.press();
    d.frame(draw);
    d.release();
    d.frame(draw);
    assert!(popup_texts(&d).is_empty(), "a press hides it and restarts the delay");
  }
}
```

The tooltip test reads `layout.text()`. Add it here as a crate accessor on `TextLayout`, returning the source text it laid out (keep the text in the layout if it is not kept already). Task 11 uses it too.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p ryo2d --lib ui::widgets`
Expected: compile errors.

- [ ] **Step 3: Implement**

Write the widgets by the rules above. Remove the old `slider`, `checkbox` and `toggle` from `context.rs`.

Port the call sites to `.at(rect)` forms:
- breakout pause: `ui.slider(&mut settings.master_volume, 0.0..=1.0).at(rect).show()`, with the 0.05 rounding replaced by `.step(0.05)`;
- the `ui` demo's theme and layout pages.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ryo2d --lib ui::` and `cargo test --workspace` and `cargo build --workspace --all-targets`
Expected: pass.

- [ ] **Step 5: Gate and commit**

Stage the new widget files and the ported files by name.

```bash
git commit -m "feat: checkbox, toggle, slider, radio, progress, image and tooltips" -m "- value widgets write the caller's data and report changed; sliders step with Left/Right and snap to a step
- images take a filter; separators and spacers for layouts
- tooltips appear after the theme's delay on hover or focus"
```

### Task 11: The text field

**Files:**
- Create:
  - `src/core/ui/widgets/text_edit.rs`: the editing logic of `TextInputState`, moved and reshaped to edit a caller's `String`, with its tests.
  - `src/core/ui/widgets/text_input.rs`: the widget.
- Delete: `src/core/ui/retained/text_input.rs`, `src/core/ui/retained/focus.rs`
- Modify:
  - `src/core/ui/immediate/context.rs`: remove `text_input`, `handle_tab_navigation` and the `Focusable` uses; `wants_keyboard` reads memory.
  - `src/core/ui/immediate/input.rs`: `UiInput::from_view` (crate), for the driver.
  - `src/core/ui/retained/mod.rs`, `src/core/ui/mod.rs`, `src/lib.rs`: drop `TextInputState`, `Focusable`, `FocusTrap`.
  - `src/core/ui/test_util.rs`: `UiDriver` keyboard and text.
  - `src/core/engine/frame.rs`: the typing test ported.
  - Ports:
    - games and demos: `examples/games/leap-king/src/scenes/menu.rs`, `examples/demos/networking/src/scenes/menu.rs`, `examples/demos/ui/src/scenes/text_input_demo.rs`;
    - demo-kit: `examples/demo-kit/tests/shell.rs` (the `Probe` and `Typing` pages), and `examples/demo-kit/src/scene.rs` if it names `TextInputState`.
- Test: `src/core/ui/widgets/text_edit.rs`, `src/core/ui/widgets/text_input.rs` (tests modules)

**Interfaces:**
- Consumes:
  - Tasks 5-10: subtrees, `Local` draws, `Response`, focus and `resolve_focus` (the `TEXT_FIELD`, `USES_BACK` and `TAKES_HORIZONTAL` flags), the theme's `text_input` looks and `Sizes::text_input`.
  - `TextEngine::{caret_x, byte_at, selection_x}` through `UiContext`'s private helpers.
  - `UiInput` (text, IME, keys, `consume_keyboard`).
- Produces:

```rust
pub fn text_input<'u>(&'u mut self, text: &'u mut String) -> TextInput<'u, 'a>;   // #[track_caller]
impl TextInput<'_, '_> {
  pub fn placeholder(self, placeholder: impl Into<String>) -> Self;
  pub fn max_len(self, chars: usize) -> Self;
  pub fn password(self) -> Self;
  pub fn show(self) -> Response;   // changed(), submitted() (Enter), focused()
  // + widget_builders!
}
pub fn wants_keyboard(&self) -> bool;   // a text field in the top scene is focused
pub(crate) struct TextEdit { /* caret, anchor, scroll, blink, paste guard */ }   // Default
impl TextEdit {
  pub(crate) fn handle_input(&mut self, text: &mut String, input: &UiInput, max_len: Option<usize>) -> EditOutcome;
  pub(crate) fn clamp_to(&mut self, text: &str);       // after the caller changed the string
  pub(crate) fn set_cursor(&mut self, text: &str, byte: usize, select: bool);
  pub(crate) fn cursor(&self) -> usize; pub(crate) fn selection(&self) -> Option<std::ops::Range<usize>>;
  pub(crate) fn ensure_caret_visible(&mut self, caret_x: f32, width: f32);
}
pub(crate) struct EditOutcome { pub(crate) changed: bool, pub(crate) submitted: bool }
pub(crate) fn from_view(view: InputView) -> UiInput;   // on UiInput
```

**The rules:**
- **The record.**
  - The field records `CLICK | DRAG | FOCUSABLE | TEXT_FIELD | TAKES_HORIZONTAL`, plus `USES_BACK` while focused.
  - Its default size is `Sizes::text_input`. `.width()`/`.fill_width()` override it.
- **When the response is focused** (focus only exists in the top scene):
  - It takes the `TextEdit` from `memory.state::<TextEdit>(id, scene)` and calls `clamp_to(text)`.
  - It calls `handle_input(text, &ui.input, max_len)`: the moved `TextInputState::handle_input` logic, editing `text` in place, with the same shortcuts.
  - It consumes the keyboard: `ui.input.consume_keyboard()`.
  - It marks `changed`/`submitted` from the outcome.
  - Escape unfocuses the field (`ui.release_focus(id)`, crate) and is consumed; with `USES_BACK`, `ui.back()` is false that frame.
  - Tab is left to navigation, which moves focus within the scope's submission order.
- **Mouse.** A press in the field focuses it, through `resolve_focus` step 5, and sets the caret at `byte_at(display, pointer.x - text_x + scroll)`. A drag extends the selection.
- **`max_len`** counts chars. Inserting past it truncates the insertion.
- **Password fields** display `'•'` (U+2022, in the built-in subsets) once per char. Carets, selections and clicks map between display and real byte indices by char index.
- **Drawing,** in node-local units, using the field's known width (`Sizes::text_input[0]`, `.width`, or last frame's laid-out width when it fills):
  - the box (`Fill`, the `text_input` looks; the focused look while focused);
  - the selection rect;
  - the text, or the placeholder in `palette.muted` when the text is empty and the field unfocused, clipped to the field's inner rect and scrolled by the edit's horizontal scroll;
  - the caret (1.5 wide, blinking at 1 Hz) and the IME preedit underlined at the caret, while focused.
- **`wants_keyboard()`** is true when `memory.interact.focused` is a `TEXT_FIELD` record of the top scene.

- [ ] **Step 1: Move the editing logic and its tests**

Move `TextInputState`'s editing core into `TextEdit`, with the text held by the caller:
- `TextEdit::handle_input` takes `&mut String`.
- Every test in `retained/text_input.rs` moves with it and changes from `state.text()` to the external string, keeping its assertions. They must pass unchanged in meaning.

Run: `cargo test -p ryo2d --lib ui::widgets::text_edit`
Expected: the moved tests pass.

- [ ] **Step 2: Write the failing widget tests**

`UiDriver` gains:
- `key(KeyCode)`, a press this frame;
- `type_text(&str)`, text this frame;
- `shift(bool)`.

Each frame it builds the context's `UiInput` with `UiInput::from_view(view)`, from the scripted keys, text and pointer.

`src/core/ui/widgets/text_input.rs`:

```rust
#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use crate::core::ui::test_util::UiDriver;
  use crate::core::ui::{Rect, UiContext};
  use winit::keyboard::KeyCode;

  const FIELD: Rect = Rect::new(10.0, 10.0, 160.0, 28.0);

  fn field(ui: &mut UiContext<'_>, text: &mut String) -> crate::core::ui::Response {
    ui.text_input(text).at(FIELD).placeholder("Name").max_len(8).show()
  }

  fn focus(d: &mut UiDriver, text: &mut String) {
    d.frame(|ui| field(ui, text));
    d.move_to(20.0, 20.0);
    d.press();
    d.frame(|ui| field(ui, text));
    d.release();
    d.frame(|ui| field(ui, text));
  }

  #[test]
  fn a_field_takes_focus_on_click_outside_any_scope_and_types() {
    let mut d = UiDriver::new((300.0, 100.0));
    let mut name = String::new();
    focus(&mut d, &mut name);
    assert!(d.frame(|ui| {
      field(ui, &mut name);
      ui.wants_keyboard()
    }));
    d.type_text("Ryo");
    assert!(d.frame(|ui| field(ui, &mut name)).changed());
    assert_eq!(name, "Ryo");
  }

  #[test]
  fn max_len_stops_typing() {
    let mut d = UiDriver::new((300.0, 100.0));
    let mut name = String::new();
    focus(&mut d, &mut name);
    d.type_text("abcdefghijk");
    d.frame(|ui| field(ui, &mut name));
    assert_eq!(name, "abcdefgh");
  }

  #[test]
  fn escape_leaves_the_field_and_back_is_not_offered() {
    let mut d = UiDriver::new((300.0, 100.0));
    let mut name = String::from("x");
    focus(&mut d, &mut name);
    d.key(KeyCode::Escape);
    d.back();
    let back = d.frame(|ui| {
      field(ui, &mut name);
      ui.back()
    });
    assert!(!back, "the field used Escape");
    assert!(!d.frame(|ui| {
      field(ui, &mut name);
      ui.wants_keyboard()
    }));
  }

  #[test]
  fn enter_submits_and_keeps_focus() {
    let mut d = UiDriver::new((300.0, 100.0));
    let mut name = String::from("ok");
    focus(&mut d, &mut name);
    d.key(KeyCode::Enter);
    assert!(d.frame(|ui| field(ui, &mut name)).submitted());
    assert!(d.frame(|ui| field(ui, &mut name)).focused());
  }

  #[test]
  fn tab_moves_to_the_next_field_in_the_scope() {
    let mut d = UiDriver::new((300.0, 200.0));
    let (mut a, mut b) = (String::new(), String::new());
    let draw = |ui: &mut UiContext<'_>, a: &mut String, b: &mut String| {
      let mut ids = Vec::new();
      ui.column().focus_scope().show(|ui| {
        ids.push(ui.text_input(a).default_focus().show().id);
        ids.push(ui.text_input(b).show().id);
      });
      ids
    };
    let ids = d.frame(|ui| draw(ui, &mut a, &mut b));
    d.frame(|ui| draw(ui, &mut a, &mut b));
    d.tab(false);
    d.frame(|ui| draw(ui, &mut a, &mut b));
    assert_eq!(d.frame(|ui| ui.focused()), Some(ids[1]));
    d.type_text("2");
    d.frame(|ui| draw(ui, &mut a, &mut b));
    assert_eq!((a.as_str(), b.as_str()), ("", "2"));
  }

  #[test]
  fn a_password_field_draws_bullets() {
    use crate::core::ui::immediate::UiCommand;
    let mut d = UiDriver::new((300.0, 100.0));
    let mut secret = String::from("hunter");
    d.frame(|ui| ui.text_input(&mut secret).at(FIELD).password().show());
    let shown: Vec<String> = d
      .commands
      .iter()
      .filter_map(|(_, c)| match c {
        UiCommand::Text { layout, .. } => Some(layout.text().to_string()),
        _ => None,
      })
      .collect();
    assert!(shown.iter().any(|t| t == "••••••"), "{shown:?}");
    assert!(!shown.iter().any(|t| t.contains("hunter")));
  }

  #[test]
  fn a_lower_scenes_focused_field_gets_no_keys() {
    let mut d = UiDriver::new((300.0, 100.0));
    let mut name = String::new();
    focus(&mut d, &mut name);
    d.type_text("z");
    d.frame(|ui| {
      ui.end_scene();
      ui.begin_scene(1, false); // the field's scene is no longer the top one
      field(ui, &mut name);
      ui.end_scene();
      ui.begin_scene(2, true);
    });
    assert_eq!(name, "", "a scene under the top one does not type");
  }
}
```

`layout.text()` is Task 10's crate accessor on `TextLayout`.

- [ ] **Step 3: Run them to see them fail**

Run: `cargo test -p ryo2d --lib ui::widgets::text_input`
Expected: compile errors.

- [ ] **Step 4: Implement**

Write the widget by the rules above. Delete the old `text_input`, `handle_tab_navigation`, `TextInputState`, `Focusable` and `FocusTrap`.

Port:
- **leap-king menu:**
  - The three `TextInputState`s become `String` fields.
  - The `is_focused()` checks become `ui.wants_keyboard()` (stored by the `ui` hook for `update`), or the fields' responses.
  - Enter in a field becomes `resp.submitted()`.
  - Keep its rects with `.at(..)`; Task 16 restructures it.
- **networking lobby:** two `String`s; Enter becomes `submitted()`.
- **ui `text_input_demo`:**
  - Its four fields go in a `ui.column().focus_scope()` at their current rects, through `.at(..)` inside the column.
  - The password field gets `.password()`; update its "can't mask yet" copy.
  - Enter on the username becomes `submitted()`.
  - Remove the `handle_tab_navigation` call.
- **demo-kit `tests/shell.rs` pages:** a `String` field drawn with `ui.text_input(&mut self.text).at(Rect::new(10.0, 100.0, 120.0, 20.0)).show()`, and the tests' focus set-up done by clicking the field. Keep every assertion.
- **engine:** port `frame.rs`'s `typing_into_a_focused_field_is_the_ui_s`.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p ryo2d --lib ui::` and `cargo test -p demo-kit` and `cargo test --workspace` and `cargo build --workspace --all-targets`
Expected: pass.

- [ ] **Step 6: Gate and commit**

Stage the files by name, including the `git rm`s.

```bash
git commit -m "feat: a text field that edits the caller's string" -m "- focus by click anywhere or by navigation in a scope; Tab moves on; Escape leaves; Enter submits
- max length, password masking, selection and IME as before
- TextInputState, Focusable and FocusTrap are gone"
```

### Task 12: Dropdown, tabs, choice list, modal, confirm, popups

**Files:**
- Create: `src/core/ui/widgets/{dropdown,tabs,choice_list,modal}.rs`, `src/core/ui/popup.rs` (deferred popup subtrees)
- Delete: `src/core/ui/retained/` (its last file, `dropdown.rs`, and `mod.rs`)
- Modify:
  - `src/core/ui/immediate/context.rs`: a subtree stack; popups flushed in `end_scene`; remove `dropdown`, `handle_dropdown_tab_navigation`, `PendingDropdown`, `finish_dropdowns`, `finish_dropdowns_on`, `any_dropdown_open`, `focused_dropdown`, `set_focused_dropdown`; `wants_keyboard` also covers open popups and modals.
  - `src/core/ui/memory.rs`: `press_target` (what a press landed on this frame).
  - `src/core/ui/theme/builtin.rs`: the Tab `selected` and the `choice` variants.
  - `src/core/scene/manager.rs`: no `finish_dropdowns*` calls; `end_scene` does it.
  - `src/core/ui/mod.rs`, `src/lib.rs`.
  - Ports: `examples/demos/ui/src/scenes/dropdown_demo.rs`, `examples/demo-kit/tests/shell.rs` (the `Probe` dropdown).
- Test: each new file's tests module; `src/core/scene/manager.rs` (the popup-layer tests)

**Interfaces:**
- Consumes:
  - Tasks 5-11: subtrees, focus (`TRAP` and `MODAL` scopes push onto `modal_stack` in `resolve_focus`; `USES_BACK` makes `back_used`), `tab_step`, the theme.
  - `UiInput::key_pressed` for number keys.
- Produces:

```rust
pub fn dropdown<'u>(&'u mut self, selected: &'u mut usize, options: &'u [&'u str]) -> Dropdown<'u, 'a>;   // #[track_caller]; show() -> Response (changed when picked)
pub fn tabs<'u>(&'u mut self, selected: &'u mut usize, labels: &'u [&'u str]) -> Tabs<'u, 'a>;          // show() -> Response
pub fn choice_list<'u>(&'u mut self, options: &'u [&'u str]) -> ChoiceList<'u, 'a>;                     // show() -> Option<usize>
pub fn modal<'u>(&'u mut self) -> Modal<'u, 'a>;    // container builders; show(|ui| ..) -> Response
/// Out of flow below `anchor`'s last-frame rect (centred when unknown), on the popup layer: a column with
/// the panel look that traps focus. The game decides when it shows, typically closing it on ui.back(). #[track_caller]
pub fn popup<'u>(&'u mut self, anchor: UiId) -> Container<'u, 'a>;
pub fn confirm(&mut self, title: &str, message: &str) -> Option<bool>;                                   // #[track_caller]
pub fn confirm_with(&mut self, title: &str, message: &str, yes: &str, no: &str) -> Option<bool>;        // #[track_caller]
pub(crate) struct Popup { pub(crate) subtree: Subtree, pub(crate) anchor: Option<Rect>, pub(crate) modal: bool }
pub(crate) fn begin_popup(&mut self, anchor: Option<Rect>, modal: bool);   // nodes go to a new subtree
pub(crate) fn end_popup(&mut self);                                        // queued until end_scene
```

The spec wrote `ui.choice_list(&[..]) -> Option<usize>`. It is a builder whose `show()` returns the `Option<usize>`, so it takes the layout builders too.

**The rules:**
- **Popups.**
  - `begin_popup` pushes a new subtree onto the context's subtree stack; widgets record into the top subtree. `end_popup` pops it onto `popups`.
  - `end_scene` flushes the queued popups in order. Each one's draws are renumbered with fresh `next_order()` values, so they follow everything the scene drew, and are placed on `POPUP_LAYER` for the top scene, or `PAGE_LAYER` for a lower one, as Task 9 of 4a ruled. Then it is finished like any subtree.
  - Placement: a modal is centred on the UI. An anchored popup goes below its anchor rect (`anchor.bottom() + 2`), flips above when it would leave the UI, is shifted to stay inside horizontally, and is at least as wide as the anchor.
  - Ids inside a popup are scoped under the popup's owner, so a dropdown's options belong to that dropdown.
- **`ui.popup(anchor)`** builds an anchored popup from `memory.prev(anchor)`'s rect: a column with the panel look and `FOCUS_SCOPE | TRAP`, placed like a dropdown list. A context menu is a `ui.popup(button.id)` shown while the game's flag is set.
- **Dropdown.**
  - The trigger is a button-like widget showing `options[*selected]` and a chevron (`Local` line segments). It records `CLICK | FOCUSABLE` and uses the dropdown looks.
  - State `DropdownState { open: bool }` lives in memory. A click or Confirm on the trigger toggles it.
  - While open, the trigger records `USES_BACK`, and the list is built as an anchored popup:
    - a column with `FOCUS_SCOPE | TRAP`;
    - one focusable option per entry (the `choice` look; `USES_BACK`), with the selected one `default_focus`.
  - Picking (a click or Confirm on an option) sets `*selected`, closes the list and marks `changed`. Focus returns to the trigger through `modal_stack`.
  - It closes on Back while open (Back is used, so `ui.back()` is false), and on a press whose `press_target` is neither the trigger nor an option.
  - Out-of-range values: `*selected` is clamped into range on draw; an empty `options` draws "-" and never opens.
- **Tabs.**
  - A row of focusable tab buttons that share the row's width equally (`grow: 1`). The selected one uses `Theme::look(WidgetKind::Tab, Some("selected"), state)`, the others the plain Tab look.
  - A click or Confirm selects one.
  - `memory.interact.tab_step` (from `ui.tab_next`/`ui.tab_prev`, the shoulder buttons and Q/E) moves the selection with wrap-around. The first `tabs` drawn in the top scene that frame takes it: it zeroes `tab_step` and sets `tab_taken`, so the engine consumes the actions.
  - A selection moved by `tab_step` requests focus on the newly selected tab button, so the bumpers leave focus on the tab bar rather than nowhere (the old tab's contents are gone).
  - `changed` when the selection moves.
- **Choice list.**
  - A column with `FOCUS_SCOPE` of focusable choices (`Theme::look(WidgetKind::Choice, ..)`), the first being `default_focus` unless `.default(i)`.
  - `show()` returns `Some(i)` on a click or Confirm.
  - In the top scene, `Digit1`-`Digit9` and `Numpad1`-`Numpad9` pick choice `n - 1` directly when it exists, and that key is consumed.
- **Modal.**
  - `ui.modal().show(|ui| ..)` builds a popup with `modal: true`:
    - a full-UI dim (`Theme::look(WidgetKind::Modal, ..)`'s `fill`, by default `rgba(0,0,0,160)`), recorded `BLOCKING`, so nothing under it is hot;
    - a centred column with the panel look and `FOCUS_SCOPE | TRAP | MODAL`.
  - Focus moves in when it appears and returns when it goes (`modal_stack`).
  - It does not close itself: the game decides, typically on `ui.back()` or a button.
- **Confirm.**
  - `ui.confirm_with(title, message, yes, no)` is a modal holding the title (heading role), the message (body role, wrapped at 320), and a row of two buttons, with `no` the default focus.
  - It returns `Some(true)`/`Some(false)` when one is chosen, `Some(false)` on Back, and `None` otherwise.
  - `ui.confirm(title, message)` uses "Yes" and "No".
- **`wants_keyboard()`** is also true while the top scene has an open dropdown list: an entry on `modal_stack` whose scope record has `TRAP` but not `MODAL`. demo-kit uses it to leave its keys, Escape included, to the list. A modal does not make it true: the game decides when its modal closes (on `ui.back()` or a button), and a demo page showing one says so with `wants_escape`.

- [ ] **Step 1: Write the failing tests**

`src/core/ui/widgets/dropdown.rs`:

```rust
#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use crate::core::input::Direction;
  use crate::core::ui::test_util::UiDriver;
  use crate::core::ui::{Rect, UiContext};

  const OPTIONS: [&str; 3] = ["Easy", "Medium", "Hard"];
  const TRIGGER: Rect = Rect::new(10.0, 10.0, 120.0, 28.0);

  fn drop_down(ui: &mut UiContext<'_>, sel: &mut usize) -> bool {
    let mut changed = false;
    ui.column().focus_scope().show(|ui| changed = ui.dropdown(sel, &OPTIONS).at(TRIGGER).default_focus().show().changed());
    changed
  }

  fn click(d: &mut UiDriver, x: f32, y: f32, sel: &mut usize) -> bool {
    d.move_to(x, y);
    d.press();
    d.frame(|ui| drop_down(ui, sel));
    d.release();
    d.frame(|ui| drop_down(ui, sel))
  }

  #[test]
  fn a_dropdown_opens_picks_and_closes() {
    let mut d = UiDriver::new((300.0, 200.0));
    let mut sel = 0;
    d.frame(|ui| drop_down(ui, &mut sel));
    click(&mut d, 20.0, 20.0, &mut sel);
    d.frame(|ui| drop_down(ui, &mut sel));
    // The list opens below the trigger: options of about 28 each from y 40
    assert!(click(&mut d, 20.0, 40.0 + 28.0 * 2.0 + 10.0, &mut sel));
    assert_eq!(sel, 2);
    d.frame(|ui| drop_down(ui, &mut sel));
    assert!(!d.frame(|ui| {
      drop_down(ui, &mut sel);
      ui.wants_keyboard()
    }), "closed");
  }

  #[test]
  fn confirm_opens_a_focused_dropdown_and_the_d_pad_picks() {
    let mut d = UiDriver::new((300.0, 200.0));
    let mut sel = 0;
    for _ in 0..2 {
      d.frame(|ui| drop_down(ui, &mut sel));
    }
    d.confirm();
    d.frame(|ui| drop_down(ui, &mut sel));
    d.frame(|ui| drop_down(ui, &mut sel));
    d.nav(Direction::Down);
    d.frame(|ui| drop_down(ui, &mut sel));
    d.confirm();
    d.frame(|ui| drop_down(ui, &mut sel));
    assert_eq!(sel, 1);
  }

  #[test]
  fn back_closes_an_open_dropdown_before_anything_else() {
    let mut d = UiDriver::new((300.0, 200.0));
    let mut sel = 1;
    d.frame(|ui| drop_down(ui, &mut sel));
    click(&mut d, 20.0, 20.0, &mut sel);
    d.frame(|ui| drop_down(ui, &mut sel));
    d.back();
    let back = d.frame(|ui| {
      drop_down(ui, &mut sel);
      ui.back()
    });
    assert!(!back, "the list used Back");
    assert_eq!(sel, 1);
    d.back();
    assert!(d.frame(|ui| {
      drop_down(ui, &mut sel);
      ui.back()
    }), "closed now: Back is free");
  }

  #[test]
  fn the_open_list_draws_over_the_page() {
    let mut d = UiDriver::new((300.0, 200.0));
    let mut sel = 0;
    let draw = |ui: &mut UiContext<'_>, sel: &mut usize| {
      drop_down(ui, sel);
      ui.draw_rect(Rect::new(0.0, 0.0, 300.0, 200.0), crate::core::ui::Color::BLUE);
    };
    d.frame(|ui| draw(ui, &mut sel));
    d.move_to(20.0, 20.0);
    d.press();
    d.frame(|ui| draw(ui, &mut sel));
    d.release();
    d.frame(|ui| draw(ui, &mut sel));
    d.frame(|ui| draw(ui, &mut sel));
    let layers: Vec<u8> = d.commands.iter().map(|(_, c)| c.draw_layer()).collect();
    assert!(layers.contains(&1), "the list is on the popup layer");
  }
}
```

`src/core/ui/widgets/modal.rs`:

```rust
#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use crate::core::input::Direction;
  use crate::core::ui::test_util::UiDriver;
  use crate::core::ui::{Rect, UiContext};

  #[test]
  fn a_popup_sits_below_its_anchor_and_traps_focus() {
    let mut d = UiDriver::new((300.0, 200.0));
    let draw = |ui: &mut UiContext<'_>| {
      let mut inner = None;
      let anchor = ui.button("More").at(Rect::new(10.0, 10.0, 80.0, 24.0)).show();
      ui.popup(anchor.id).show(|ui| inner = Some(ui.button("Rename").show()));
      inner
    };
    d.frame(draw);
    d.frame(draw);
    let Some(inner) = d.frame(draw) else {
      panic!("the popup drew no button");
    };
    assert!(inner.rect.y >= 34.0, "below the anchor: {:?}", inner.rect);
    assert_eq!(d.frame(|ui| { draw(ui); ui.focused() }), Some(inner.id), "the popup took focus");
  }

  #[test]
  fn a_modal_blocks_the_page_under_it() {
    let mut d = UiDriver::new((300.0, 200.0));
    let page = Rect::new(10.0, 10.0, 80.0, 30.0);
    let draw = |ui: &mut UiContext<'_>| {
      let clicked = ui.button("Page").at(page).clicked();
      ui.modal().show(|ui| {
        ui.text("Paused");
      });
      clicked
    };
    d.frame(draw);
    d.move_to(20.0, 20.0);
    d.press();
    d.frame(draw);
    d.release();
    assert!(!d.frame(draw));
  }

  #[test]
  fn a_modal_traps_focus_and_returns_it_when_closed() {
    let mut d = UiDriver::new((300.0, 200.0));
    let mut open = false;
    let draw = |ui: &mut UiContext<'_>, open: bool| {
      let mut page = None;
      ui.column().focus_scope().show(|ui| page = Some(ui.button("Settings").default_focus().show().id));
      let mut inner = None;
      if open {
        ui.modal().show(|ui| inner = Some(ui.button("Close").show().id));
      }
      (page, inner)
    };
    let (page, _) = d.frame(|ui| draw(ui, open));
    d.frame(|ui| draw(ui, open));
    open = true;
    let (_, inner) = d.frame(|ui| draw(ui, open));
    d.frame(|ui| draw(ui, open));
    assert_eq!(d.frame(|ui| ui.focused()), inner);
    d.nav(Direction::Up);
    d.frame(|ui| draw(ui, open));
    assert_eq!(d.frame(|ui| ui.focused()), inner, "trapped");
    open = false;
    d.frame(|ui| draw(ui, open));
    d.frame(|ui| draw(ui, open));
    assert_eq!(d.frame(|ui| ui.focused()), page);
  }

  #[test]
  fn back_closes_the_innermost_popup_first() {
    let mut d = UiDriver::new((400.0, 300.0));
    let mut sel = 0;
    let draw = |ui: &mut UiContext<'_>, sel: &mut usize| {
      let mut back = false;
      ui.modal().show(|ui| {
        ui.dropdown(sel, &["a", "b"]).default_focus().show();
      });
      back |= ui.back();
      back
    };
    for _ in 0..2 {
      d.frame(|ui| draw(ui, &mut sel));
    }
    d.confirm(); // opens the focused dropdown
    d.frame(|ui| draw(ui, &mut sel));
    d.frame(|ui| draw(ui, &mut sel));
    d.back();
    assert!(!d.frame(|ui| draw(ui, &mut sel)), "the list took the first Back");
    d.back();
    assert!(d.frame(|ui| draw(ui, &mut sel)), "the game gets the second, to close the modal");
  }

  #[test]
  fn confirm_returns_the_choice_and_back_means_no() {
    let mut d = UiDriver::new((400.0, 300.0));
    for _ in 0..2 {
      d.frame(|ui| ui.confirm("Quit?", "Unsaved progress is lost."));
    }
    d.back();
    assert_eq!(d.frame(|ui| ui.confirm("Quit?", "Unsaved progress is lost.")), Some(false));
    for _ in 0..2 {
      d.frame(|ui| ui.confirm("Quit?", "Unsaved progress is lost."));
    }
    d.nav(Direction::Left); // from No to Yes
    d.frame(|ui| ui.confirm("Quit?", "Unsaved progress is lost."));
    d.confirm();
    assert_eq!(d.frame(|ui| ui.confirm("Quit?", "Unsaved progress is lost.")), Some(true));
  }
}
```

`src/core/ui/widgets/tabs.rs` (`UiDriver` gains `tab_next()`, pressing `ui.tab_next` next frame):

```rust
#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use crate::core::ui::test_util::UiDriver;
  use crate::core::ui::{Rect, UiContext};

  const LABELS: [&str; 3] = ["Audio", "Video", "Controls"];

  fn draw(ui: &mut UiContext<'_>, sel: &mut usize) {
    ui.column().focus_scope().show(|ui| {
      ui.tabs(sel, &LABELS).at(Rect::new(0.0, 0.0, 300.0, 30.0)).show();
      ui.button("Inside").default_focus().show();
    });
  }

  /// The focused widget's rect, as last drawn
  fn focus_rect(d: &UiDriver) -> Option<Rect> {
    d.memory.prev(d.memory.interact.focused).map(|r| r.rect)
  }

  #[test]
  fn shoulder_buttons_switch_tabs_and_clicks_select() {
    let mut d = UiDriver::new((300.0, 200.0));
    let mut sel = 0;
    for _ in 0..2 {
      d.frame(|ui| draw(ui, &mut sel));
    }
    for expected in [1, 2, 0] {
      d.tab_next();
      d.frame(|ui| draw(ui, &mut sel));
      assert_eq!(sel, expected);
      d.frame(|ui| draw(ui, &mut sel));
      let third = Rect::new(100.0 * expected as f32, 0.0, 100.0, 30.0);
      let focus = focus_rect(&d);
      assert!(
        focus.is_some_and(|f| {
          let (x, y) = f.center();
          third.contains(x, y)
        }),
        "focus is on tab {expected}: {focus:?}"
      );
    }
    d.move_to(150.0, 15.0);
    d.press();
    d.frame(|ui| draw(ui, &mut sel));
    d.release();
    d.frame(|ui| draw(ui, &mut sel));
    assert_eq!(sel, 1, "a click selects");
  }

  #[test]
  fn tab_keys_are_left_alone_without_tabs() {
    let mut d = UiDriver::new((300.0, 200.0));
    let draw = |ui: &mut UiContext<'_>| {
      ui.column().focus_scope().show(|ui| {
        ui.button("Only").default_focus().show();
      });
    };
    for _ in 0..2 {
      d.frame(draw);
    }
    d.tab_next();
    d.frame(draw);
    assert!(!d.memory.interact.tab_taken);
    assert!(!d.last_outcome().used.contains(&"ui.tab_next"));
  }
}
```

`src/core/ui/widgets/choice_list.rs`:

```rust
#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use crate::core::input::Direction;
  use crate::core::ui::test_util::UiDriver;
  use crate::core::ui::{Rect, UiContext};
  use winit::keyboard::KeyCode;

  fn choices(ui: &mut UiContext<'_>) -> Option<usize> {
    ui.choice_list(&["Buy", "Sell", "Leave"]).at(Rect::new(0.0, 0.0, 200.0, 90.0)).show()
  }

  #[test]
  fn number_keys_pick_a_choice() {
    let mut d = UiDriver::new((300.0, 200.0));
    for _ in 0..2 {
      d.frame(choices);
    }
    d.key(KeyCode::Digit2);
    assert_eq!(d.frame(choices), Some(1));
    d.key(KeyCode::Digit9);
    assert_eq!(d.frame(choices), None, "there is no ninth choice");
  }

  #[test]
  fn confirm_picks_the_focused_choice() {
    let mut d = UiDriver::new((300.0, 200.0));
    for _ in 0..2 {
      d.frame(choices);
    }
    d.nav(Direction::Down);
    d.frame(choices);
    d.confirm();
    assert_eq!(d.frame(choices), Some(1));
  }
}
```

`src/core/scene/manager.rs`: port its two existing popup-layer tests to the new flush, keeping what they assert. A top scene's open dropdown list draws on layer 1. A lower scene's draws on layer 0, under the upper scene's rect.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p ryo2d --lib ui::widgets ui::popup scene::manager`
Expected: compile errors.

- [ ] **Step 3: Implement**

Write the popup machinery and the widgets by the rules above, and delete the old dropdown code and `retained/`.

Port:
- **The dropdown demo.** Four `usize` fields; its column of dropdowns goes in a `.focus_scope()` column (keep the rects with `.at`); remove `handle_dropdown_tab_navigation`; read the values directly.
- **demo-kit's `Probe` page.** Draws `ui.dropdown(&mut self.choice, &["x", "y"]).at(Rect::new(10.0, 130.0, 120.0, 20.0)).show()`.
  - The tests that open it now click it.
  - `escape_with_an_open_dropdown_does_not_quit` holds: `wants_keyboard()` is true while the list is open, so the shell leaves Escape, and the list uses Back.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ryo2d --lib ui:: scene::` and `cargo test -p demo-kit` and `cargo test --workspace` and `cargo build --workspace --all-targets`
Expected: pass.

- [ ] **Step 5: Docs**

In `src/core/ui/AGENTS.md`, add "Popups and dialogs":
- popups are built during the scene and drawn at its end, above its page;
- dropdown lists trap focus and use Back;
- modals block and trap, and return focus;
- `ui.confirm`;
- tabs follow the shoulder buttons;
- choice lists take number keys.

- [ ] **Step 6: Gate and commit**

Stage the new and changed files by name, including `git rm -r src/core/ui/retained`.

```bash
git commit -m "feat: dropdowns, tabs, choice lists, modals and confirm dialogs" -m "- popups are built where they are called and drawn above their scene's page
- dropdown lists and modals trap focus and give it back; Back closes the innermost first
- tabs switch on the shoulder buttons; choice lists take number keys; ui.confirm for yes/no"
```

### Task 13: Slots and drag and drop

**Files:**
- Create: `src/core/ui/widgets/slot.rs`, `src/core/ui/widgets/drag.rs` (drag state, ghost, the shared builder methods)
- Modify:
  - `src/core/ui/memory.rs`: `Record::accepts`, `Interaction::{drag, dropped}`.
  - `src/core/ui/interact.rs`: a mouse drop on release.
  - `src/core/ui/nav.rs`: pad pick-up and put-down; Back cancels.
  - `src/core/ui/response.rs`: `dropped`, `drag_hover`.
  - `src/core/ui/widgets/common.rs`: `drag_source`, `drop_target`, `drag_ghost`.
  - `src/core/ui/immediate/context.rs`: the ghost drawn in `end_scene`.
  - `src/lib.rs`.
- Test: `src/core/ui/widgets/slot.rs`, `src/core/ui/widgets/drag.rs` (tests modules)

**Interfaces:**
- Consumes: Tasks 5-12 (resolve, focus, activation, `USES_BACK`, popups, the theme's `slot` looks and `Sizes::slot`, `palette.accent`).
- Produces:

```rust
pub fn slot<'u>(&'u mut self) -> Slot<'u, 'a>;     // #[track_caller]
impl Slot<'_, '_> { pub fn icon(self, sprite: &SpriteHandle) -> Self; pub fn count(self, n: u32) -> Self; pub fn selected(self, on: bool) -> Self; pub fn filter(self, filter: Filter) -> Self; pub fn show(self) -> Response }
// on every widget and container builder:
pub fn drag_source<T: Clone + Send + Sync + 'static>(self, payload: T) -> Self;
pub fn drop_target<T: 'static>(self) -> Self;
pub fn drag_ghost(self, sprite: &SpriteHandle) -> Self;
// Response
pub fn dropped<T: Clone + 'static>(&self) -> Option<T>;   // the payload dropped on this target this frame
pub fn drag_hover<T: 'static>(&self) -> bool;             // a compatible payload is over this target now
pub fn drag_started_here(&self) -> bool;                  // this widget's payload was picked up this frame
// memory (crate)
Record::accepts: Option<std::any::TypeId>
pub(crate) struct DragState { pub(crate) source: UiId, pub(crate) payload: Arc<dyn Any + Send + Sync>, pub(crate) type_id: TypeId,
  pub(crate) ghost: Ghost, pub(crate) pad: bool, pub(crate) grab: Vec2, pub(crate) size: Vec2 }
pub(crate) enum Ghost { Sprite(SpriteHandle, Filter), Box }
Interaction::drag: Option<DragState>, Interaction::dropped: Option<(UiId, Arc<dyn Any + Send + Sync>)>
```

**The rules:**
- **The slot.**
  - It is a `Sizes::slot` square leaf recording `CLICK | FOCUSABLE`.
  - It draws the slot looks: selected uses a 2-unit `palette.accent` border.
  - The icon is a `Sprite` inset 6 with the slot's filter (default `Filter::Default`).
  - When `count > 1`, the count draws in the caption role at the bottom-right, inset 3.
- **Sources and targets.**
  - `drag_source` adds `DRAG_SOURCE | DRAG` to the record.
  - `drop_target::<T>()` adds `DROP_TARGET` and sets `accepts = Some(TypeId::of::<T>())`.
- **Mouse.**
  - When a drag source's response has `drag_started()` and no drag is active, the widget sets `interact.drag`. The ghost is the `drag_ghost` sprite or the slot's icon, else `Ghost::Box`. `grab` is the pointer's offset in the rect, and `size` is the rect's size.
  - In `resolve_pointer`, a release while a mouse drag is active looks for the topmost top-scene record under the pointer with `DROP_TARGET` and `accepts == drag.type_id`, inside its clip.
    - If one is found, `dropped = Some((target, payload))`.
    - Either way, the drag ends.
    - A release over no target ends it with no drop.
  - The source does not report `clicked`.
- **Pad.**
  - In `resolve_focus`, Confirm while a pad drag is active and the focused record accepts the payload's type sets `dropped = Some((focused, payload))`, ends the drag, and is not an activation. Otherwise, with a drag active, Confirm does nothing.
  - With no drag active, Confirm on a focused `DRAG_SOURCE` is delivered as `activated`. The source widget turns it into a pick-up: it sets `interact.drag` with `pad: true` and reports `drag_started_here()`, not `clicked()`.
  - Back while a drag is active cancels it: `back_used`.
- **The ghost.** `end_scene` of the top scene draws it, while a drag is active, on `POPUP_LAYER` after the popups, at 80% opacity (alpha 204):
  - under the mouse at `pointer - grab`;
  - under the pad at the focused record's rect, offset (-8, -8).
  
  `Ghost::Sprite` draws the sprite at the source's size; `Ghost::Box` draws a rounded rect in `palette.accent`.
- **Responses.**
  - `dropped::<T>()` is the payload when `dropped`'s target is this id and the payload downcasts to `T`. It is cloned out of the `Arc` and cleared at the next resolve.
  - `drag_hover::<T>()` is true while a drag of type `T` is active and this target is hot (mouse) or focused (pad).

- [ ] **Step 1: Write the failing tests**

`src/core/ui/widgets/drag.rs`:

```rust
#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use crate::core::input::Direction;
  use crate::core::ui::test_util::UiDriver;
  use crate::core::ui::{Rect, UiContext};

  #[derive(Debug, Clone, Copy, PartialEq)]
  struct Item(u32);

  /// Two slots side by side: 0 holds Item(7); returns what each received
  fn slots(ui: &mut UiContext<'_>) -> [Option<Item>; 2] {
    let mut got = [None, None];
    ui.row().gap(4.0).focus_scope().show(|ui| {
      for i in 0..2 {
        let mut slot = ui.slot().id(i).drop_target::<Item>();
        if i == 0 {
          slot = slot.drag_source(Item(7)).default_focus();
        }
        got[i] = slot.show().dropped::<Item>();
      }
    });
    got
  }

  #[test]
  fn dragging_a_slot_onto_another_drops_its_payload() {
    let mut d = UiDriver::new((200.0, 100.0));
    d.frame(slots);
    d.move_to(10.0, 10.0);
    d.press();
    d.frame(slots);
    d.move_to(30.0, 10.0);
    d.frame(slots); // past the threshold: the drag starts
    d.move_to(70.0, 20.0); // over slot 1 (48 + 4 from x 0)
    d.frame(slots);
    d.release();
    assert_eq!(d.frame(slots), [None, Some(Item(7))]);
    assert_eq!(d.frame(slots), [None, None], "one frame only");
  }

  #[test]
  fn releasing_over_nothing_cancels_the_drag() {
    let mut d = UiDriver::new((200.0, 100.0));
    d.frame(slots);
    d.move_to(10.0, 10.0);
    d.press();
    d.frame(slots);
    d.move_to(150.0, 80.0);
    d.frame(slots);
    d.release();
    assert_eq!(d.frame(slots), [None, None]);
  }

  #[test]
  fn a_target_of_another_type_ignores_the_drop() {
    let mut d = UiDriver::new((200.0, 100.0));
    let draw = |ui: &mut UiContext<'_>| {
      ui.slot().at(Rect::new(0.0, 0.0, 48.0, 48.0)).drag_source(Item(1)).show();
      ui.slot().at(Rect::new(60.0, 0.0, 48.0, 48.0)).drop_target::<u8>().show().dropped::<u8>()
    };
    d.frame(draw);
    d.move_to(10.0, 10.0);
    d.press();
    d.frame(draw);
    d.move_to(80.0, 10.0);
    d.frame(draw);
    d.release();
    assert_eq!(d.frame(draw), None);
  }

  #[test]
  fn the_pad_picks_up_moves_and_puts_down() {
    let mut d = UiDriver::new((200.0, 100.0));
    for _ in 0..2 {
      d.frame(slots);
    }
    d.confirm(); // pick up slot 0
    assert_eq!(d.frame(slots), [None, None]);
    d.nav(Direction::Right);
    d.frame(slots);
    d.confirm(); // put down on slot 1
    assert_eq!(d.frame(slots), [None, Some(Item(7))]);
  }

  #[test]
  fn back_cancels_a_pad_drag() {
    let mut d = UiDriver::new((200.0, 100.0));
    for _ in 0..2 {
      d.frame(slots);
    }
    d.confirm();
    d.frame(slots);
    d.back();
    let back = d.frame(|ui| {
      slots(ui);
      ui.back()
    });
    assert!(!back, "the drag used Back");
    d.nav(Direction::Right);
    d.frame(slots);
    d.confirm();
    assert_eq!(d.frame(slots), [None, None], "nothing was held any more");
  }
}
```

In `drag.rs`'s tests, add:

```rust
  #[test]
  fn the_ghost_follows_the_pointer_at_80_percent() {
    use crate::core::asset::SpriteHandle;
    use crate::core::graphics::Sprite;
    use crate::core::ui::immediate::{POPUP_LAYER, UiCommand};
    let icon = SpriteHandle::from_sprite(Sprite::solid_rect(16, 16, 255, 255, 255, 255), "ghost-icon");
    let mut d = UiDriver::new((200.0, 100.0));
    let draw = |ui: &mut UiContext<'_>| {
      ui.slot().at(Rect::new(0.0, 0.0, 48.0, 48.0)).icon(&icon).drag_source(Item(3)).show();
    };
    d.frame(draw);
    d.move_to(10.0, 10.0); // grabbed 10, 10 into the slot
    d.press();
    d.frame(draw);
    d.move_to(100.0, 50.0);
    d.frame(draw); // past the threshold: the drag starts
    d.frame(draw);
    let ghost: Vec<(Rect, u8)> = d
      .commands
      .iter()
      .filter_map(|(_, c)| match c {
        UiCommand::Sprite { rect, tint, layer, .. } if *layer == POPUP_LAYER => Some((*rect, tint.a)),
        _ => None,
      })
      .collect();
    assert_eq!(ghost, vec![(Rect::new(90.0, 40.0, 48.0, 48.0), 204)]);
  }
```

`src/core/ui/widgets/slot.rs`:

```rust
#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use crate::core::asset::SpriteHandle;
  use crate::core::graphics::Sprite;
  use crate::core::ui::immediate::UiCommand;
  use crate::core::ui::test_util::UiDriver;
  use crate::core::ui::Rect;

  #[test]
  fn a_slot_draws_its_icon_and_count() {
    let icon = SpriteHandle::from_sprite(Sprite::solid_rect(16, 16, 200, 50, 50, 255), "test-icon");
    let mut d = UiDriver::new((100.0, 100.0));
    d.frame(|ui| {
      ui.slot().at(Rect::new(10.0, 10.0, 48.0, 48.0)).icon(&icon).count(12).show();
    });
    let sprites: Vec<Rect> = d
      .commands
      .iter()
      .filter_map(|(_, c)| match c {
        UiCommand::Sprite { rect, .. } => Some(*rect),
        _ => None,
      })
      .collect();
    assert_eq!(sprites, vec![Rect::new(16.0, 16.0, 36.0, 36.0)], "the icon, inset 6");
    let texts: Vec<String> = d
      .commands
      .iter()
      .filter_map(|(_, c)| match c {
        UiCommand::Text { layout, .. } => Some(layout.text().to_string()),
        _ => None,
      })
      .collect();
    assert_eq!(texts, vec!["12".to_string()]);
  }
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p ryo2d --lib ui::widgets::slot ui::widgets::drag`
Expected: compile errors.

- [ ] **Step 3: Implement** by the rules above.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ryo2d --lib ui::` and `cargo test --workspace`
Expected: pass.

- [ ] **Step 5: Docs**

In `src/core/ui/AGENTS.md`, add "Drag and drop":
- typed payloads; sources and targets on any widget;
- a mouse drop on release over a matching target;
- the pad picks up and puts down with Confirm, and Back cancels;
- a ghost at 80%;
- the game decides what a drop means.

- [ ] **Step 6: Gate and commit**

Stage the files by name.

```bash
git commit -m "feat: inventory slots and drag and drop" -m "- any widget can be a typed drag source or drop target; drops arrive on the target's response
- the mouse drags with a ghost; the gamepad picks up and puts down with Confirm, Back cancels"
```

### Task 14: Prompts and rebinding

**Files:**
- Create:
  - `src/core/input/labels.rs`: `PadFamily`, `Binding::label`.
  - `src/core/ui/widgets/prompt.rs`: `ui.prompt`, prompt resolution for markup.
  - `src/core/ui/widgets/rebind.rs`.
- Modify:
  - `src/core/input/mod.rs`: `pad_family()`.
  - `src/core/ui/immediate/input.rs`: gamepad presses in the view; `consume_gamepad`; `apply_consumed` applies them.
  - `src/core/ui/memory.rs`: `Flags::CAPTURING`.
  - `src/core/ui/nav.rs`: stands aside while capturing.
  - `src/core/text/markup.rs`, `src/core/text/rich.rs`: the `[prompt=..]` tag, prompt marks, `insert_text`.
  - `src/core/text/world.rs`, `src/core/engine/extract.rs`: prompts in world text.
  - `src/core/ui/immediate/context.rs`: `PromptContext`; text is prepared before layout; `pending_rebinds`.
  - `src/core/engine/frame.rs`: build the prompt context; apply the rebinds after the pass.
  - `src/lib.rs`: prelude gains `PadFamily`.
- Test: `src/core/input/labels.rs`, `src/core/ui/widgets/prompt.rs`, `src/core/ui/widgets/rebind.rs`, `src/core/text/markup.rs` (tests modules)

**Interfaces:**
- Consumes: Task 2's `Actions`, `Binding`, `rebind`; Task 1's `last_device`, `gamepad_name`; `TextIcons`; the markup parser and `RichText` marks; the theme's `prompt` and `rebind` looks.
- Produces:

```rust
pub enum PadFamily { Xbox, PlayStation, Nintendo }   // Debug, Clone, Copy, PartialEq, Eq, Hash, Default (Xbox)
impl PadFamily { pub fn from_name(gamepad_name: &str) -> PadFamily }
impl Input { pub fn pad_family(&self) -> PadFamily }
impl Binding { pub fn label(&self, family: PadFamily) -> String; pub(crate) fn glyph_name(&self, family: PadFamily) -> String }
pub fn prompt<'u>(&'u mut self, action: &str) -> Prompt<'u, 'a>;          // show() -> Response
pub fn rebind<'u>(&'u mut self, action: &str) -> Rebind<'u, 'a>;
impl Rebind<'_, '_> { pub fn label(self, label: impl Into<RichText>) -> Self; pub fn slot(self, slot: usize) -> Self; pub fn show(self) -> Response }  // changed when rebound
pub(crate) struct PromptContext { pub(crate) device: InputDevice, pub(crate) family: PadFamily, pub(crate) bindings: BTreeMap<String, Vec<Binding>>, pub(crate) icons: TextIcons }
pub(crate) fn resolve_prompts(text: &RichText, prompts: &PromptContext, accent: Color) -> Cow<'_, RichText>;
pub(crate) fn binding_for_device<'b>(bindings: &'b [Binding], device: InputDevice) -> Option<&'b Binding>;
// UiInput: pub(crate) fn gamepad_pressed_any(&self) -> Option<GamepadButton>; pub fn consume_gamepad(&mut self, button: GamepadButton)
// rich.rs: pub(crate) prompts: Vec<(usize, String)>; pub fn prompt(self, action: &str) -> RichText; pub(crate) fn insert_text(&mut self, at: usize, text: &str, span: Span)
// markup: [prompt=jump]
```

**The rules:**
- **`PadFamily::from_name`.**
  - It lowercases the name. A name containing "dualsense", "dualshock", "playstation", "sony" or "ps4"/"ps5" is `PlayStation`. One containing "nintendo", "switch", "pro controller" or "joy-con" is `Nintendo`. Anything else is `Xbox`.
  - `Input::pad_family()` applies it to `gamepad_name()`.
- **`Binding::label(family)`:**
  - **Keys:**
    - letters and digits as themselves;
    - `Enter`, `Space`, `Esc`, `Tab`, `Shift`, `Ctrl`, `Alt`, `Bksp` and `F1`-`F12`;
    - the arrows as `↑ ↓ ← →` (U+2190-2193, in the subsets);
    - punctuation as its character;
    - otherwise the `KeyCode` name.
  - **Mouse:** `LMB`, `RMB`, `MMB`, `Back`, `Fwd`.
  - **Pad, by family:**

| Button | Xbox | PlayStation | Nintendo |
|---|---|---|---|
| South / East / West / North | A / B / X / Y | × / ○ / □ / △ | B / A / Y / X |
| LeftBumper / RightBumper | LB / RB | L1 / R1 | L / R |
| LeftTrigger / RightTrigger | LT / RT | L2 / R2 | ZL / ZR |
| Start / Select / Guide | Menu / View / Home | Options / Share / PS | + / − / Home |
| LeftStick / RightStick (clicks) | LS / RS | L3 / R3 | LS / RS |
| DPadUp/Down/Left/Right | D↑ / D↓ / D← / D→ | same | same |

  - **Sticks:** `L↑` and the like.
  - Every character above is in the built-in subsets (× U+00D7; ○ □ △ in U+25A0-25FF; − U+2212).
- **`glyph_name`** is the `TextIcons` name a game can register: `prompt.key.<KeyCode name>`, `prompt.mouse.<Left|Right|..>`, `prompt.pad.<xbox|playstation|nintendo>.<button>`, `prompt.stick.<LeftUp|..>`.
- **`binding_for_device`:**
  - On a keyboard or mouse device, the first `Key` or `Mouse` binding.
  - On a gamepad, the first `Pad` or `Stick` binding.
  - Otherwise the first binding.
- **`ui.prompt(action)`.**
  - A leaf showing the action's binding for the device. A glyph registered in `PromptContext.icons` under `glyph_name` draws as an image of the caption line height. Otherwise a chip: a `Fill` with the `prompt` look (radius 4, padding 4 × 1) and the label in the caption role.
  - An action with no binding draws "?" and warns once per name.
- **`[prompt=jump]` markup.** It adds a prompt mark at its byte, like `[icon=..]`. `RichText::prompt(action)` does the same in code.
  - Before any UI text is laid out (`ui.text`, widget labels, `draw_text*`), `resolve_prompts` replaces each mark with an inline `Icon::Named(glyph_name)` when that glyph is registered. Otherwise it inserts the text `[<label>]` as a bold `Span` in `palette.accent`.
  - `RichText::hash_into` hashes the prompt marks, so the cache keys the unresolved text. The resolved text differs by device, which gives it a different layout.
  - World text resolves the same way in `layout_world_text`, from a `PromptContext` the engine builds from `Input` and `TextIcons`.
- **The engine** builds the `PromptContext` each frame (device, family, `actions.names()` with their bindings, and a `TextIcons` clone refreshed when that resource changes), and hands it to the `UiContext`.
- **`ui.rebind(action)`:**
  - **The row.** A focusable row recording `CLICK | FOCUSABLE`: the label (default: the action's name) on the left (`grow`), then the bindings as prompt chips, or while capturing "Press a key or button…".
  - **State.** `RebindState { capturing: bool, since: f64, note: Option<(String, f64)> }` lives in memory.
  - **Starting a capture.** A click, or a Confirm, sets `capturing`. While capturing the record adds `CAPTURING | USES_BACK`.
  - **`resolve_focus` stands aside while the focused record has `CAPTURING`.** Directions, Tab, Confirm and Back do nothing and mark nothing used, so the widget can capture them.
  - **The next press, in the widget's run.**
    - Escape cancels.
    - Any other key gives `Binding::Key`.
    - Any pad button, East included, gives `Binding::Pad`.
    - The press is consumed (`consume_key` or `consume_gamepad`).
    - The widget pushes `(action, slot, binding)` onto `ui.pending_rebinds` and marks `changed`.
    - Mouse buttons are not captured.
  - **Timeout.** After 5 s with no press, the capture cancels.
  - **Applying it.** After the pass, the engine applies each pending rebind with `actions_mut().rebind(..)`. When that returns another action's name, the row's state gets `note = Some(("Swapped with <name>", now))`, shown under the row for 2 s.
  - **Saving.** Task 2's dirty flag saves it.

- [ ] **Step 1: Write the failing tests**

`src/core/input/labels.rs`:

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::input::{Binding, GamepadButton, StickDirection, Stick, Direction};
  use winit::keyboard::KeyCode;

  #[test]
  fn pad_families_come_from_the_name() {
    assert_eq!(PadFamily::from_name("Sony DualSense Wireless Controller"), PadFamily::PlayStation);
    assert_eq!(PadFamily::from_name("Nintendo Switch Pro Controller"), PadFamily::Nintendo);
    assert_eq!(PadFamily::from_name("Xbox Wireless Controller"), PadFamily::Xbox);
    assert_eq!(PadFamily::from_name("Virtual pad"), PadFamily::Xbox);
  }

  #[test]
  fn binding_labels_follow_the_pad_family() {
    let south = Binding::Pad(GamepadButton::South);
    assert_eq!(south.label(PadFamily::Xbox), "A");
    assert_eq!(south.label(PadFamily::PlayStation), "×");
    assert_eq!(south.label(PadFamily::Nintendo), "B");
    assert_eq!(Binding::Key(KeyCode::Enter).label(PadFamily::Xbox), "Enter");
    assert_eq!(Binding::Key(KeyCode::ArrowUp).label(PadFamily::Xbox), "↑");
    assert_eq!(Binding::Key(KeyCode::KeyJ).label(PadFamily::Xbox), "J");
    assert_eq!(Binding::Stick(StickDirection { stick: Stick::Left, direction: Direction::Up }).label(PadFamily::Xbox), "L↑");
    assert_eq!(Binding::Pad(GamepadButton::RightBumper).glyph_name(PadFamily::PlayStation), "prompt.pad.playstation.RightBumper");
  }
}
```

`src/core/ui/widgets/prompt.rs` (`UiDriver::prompts(PromptContext)` sets the driver's prompt context; a helper `prompts_with(device, &[("ui.confirm", vec![Key(Enter), Pad(South)])])` builds one):

```rust
#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use super::*;
  use crate::core::input::{Binding, GamepadButton, InputDevice};
  use crate::core::ui::immediate::UiCommand;
  use crate::core::ui::test_util::{UiDriver, prompts_with};
  use winit::keyboard::KeyCode;

  fn shown(d: &UiDriver) -> Vec<String> {
    d.commands.iter().filter_map(|(_, c)| match c {
      UiCommand::Text { layout, .. } => Some(layout.text().to_string()),
      _ => None,
    }).collect()
  }

  #[test]
  fn a_prompt_shows_the_binding_for_the_last_device() {
    let confirm = ("ui.confirm", vec![Binding::Key(KeyCode::Enter), Binding::Pad(GamepadButton::South)]);
    let mut d = UiDriver::new((200.0, 100.0)).prompts(prompts_with(InputDevice::Keyboard, &[confirm.clone()]));
    d.frame(|ui| ui.prompt("ui.confirm").show());
    assert!(shown(&d).contains(&"Enter".to_string()));
    let mut d = UiDriver::new((200.0, 100.0)).prompts(prompts_with(InputDevice::Gamepad, &[confirm]));
    d.frame(|ui| ui.prompt("ui.confirm").show());
    assert!(shown(&d).contains(&"A".to_string()));
  }

  #[test]
  fn markup_prompts_become_labels_or_registered_glyphs() {
    let jump = ("jump", vec![Binding::Key(KeyCode::Space), Binding::Pad(GamepadButton::South)]);
    let mut context = prompts_with(InputDevice::Gamepad, &[jump]);
    let text = RichText::markup("Press [prompt=jump] to jump");
    let resolved = resolve_prompts(&text, &context, Color::YELLOW);
    assert_eq!(resolved.as_str(), "Press [A] to jump");
    let sprite = crate::core::asset::SpriteHandle::from_sprite(crate::core::graphics::Sprite::solid_rect(4, 4, 255, 255, 255, 255), "a-glyph");
    context.icons.insert("prompt.pad.xbox.South", sprite);
    let resolved = resolve_prompts(&text, &context, Color::YELLOW);
    assert_eq!(resolved.as_str(), "Press  to jump", "the glyph replaces the label");
    assert_eq!(resolved.icons.len(), 1);
  }
}
```

`src/core/ui/widgets/rebind.rs`:

```rust
#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use crate::core::input::{Binding, Direction, GamepadButton, InputDevice};
  use crate::core::ui::test_util::{UiDriver, prompts_with};
  use crate::core::ui::UiContext;
  use winit::keyboard::KeyCode;

  fn row(ui: &mut UiContext<'_>) -> bool {
    let mut changed = false;
    ui.column().focus_scope().show(|ui| changed = ui.rebind("jump").label("Jump").default_focus().show().changed());
    changed
  }

  fn driver() -> UiDriver {
    UiDriver::new((300.0, 100.0)).prompts(prompts_with(InputDevice::Keyboard, &[("jump", vec![Binding::Key(KeyCode::Space)])]))
  }

  #[test]
  fn rebinding_captures_the_next_key() {
    let mut d = driver();
    for _ in 0..2 {
      d.frame(row);
    }
    d.confirm();
    d.frame(row);
    d.key(KeyCode::KeyJ);
    assert!(d.frame(row));
    assert_eq!(d.pending_rebinds(), vec![("jump".to_string(), 0, Binding::Key(KeyCode::KeyJ))]);
  }

  #[test]
  fn arrows_and_the_east_button_are_captured_not_navigated() {
    let mut d = driver();
    for _ in 0..2 {
      d.frame(row);
    }
    d.confirm();
    d.frame(row);
    d.nav(Direction::Down);
    d.key(KeyCode::ArrowDown);
    d.frame(row);
    assert_eq!(d.pending_rebinds().last().map(|r| r.2), Some(Binding::Key(KeyCode::ArrowDown)));
    d.confirm();
    d.frame(row);
    d.pad(GamepadButton::East);
    d.back();
    d.frame(row);
    assert_eq!(d.pending_rebinds().last().map(|r| r.2), Some(Binding::Pad(GamepadButton::East)));
  }

  #[test]
  fn escape_cancels_and_a_capture_times_out() {
    let mut d = driver();
    for _ in 0..2 {
      d.frame(row);
    }
    d.confirm();
    d.frame(row);
    d.key(KeyCode::Escape);
    assert!(!d.frame(row));
    assert!(d.pending_rebinds().is_empty());
    d.confirm();
    d.frame(row);
    for _ in 0..(5 * 60 + 2) {
      d.frame(row);
    }
    d.key(KeyCode::KeyK);
    d.frame(row);
    assert!(d.pending_rebinds().is_empty(), "timed out before K");
  }
}
```

`UiDriver` gains `pad(GamepadButton)` (a pad press this frame in the view) and `pending_rebinds()` (the last frame's list).

In `src/core/text/markup.rs`'s tests, add:

```rust
  #[test]
  fn a_prompt_tag_marks_its_byte() {
    let text = RichText::markup("Press [prompt=jump] to jump");
    assert_eq!(text.as_str(), "Press  to jump");
    assert_eq!(text.prompts, vec![(6, "jump".to_string())]);
  }
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p ryo2d --lib input::labels ui::widgets::prompt ui::widgets::rebind text::markup`
Expected: compile errors.

- [ ] **Step 3: Implement** by the rules above.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ryo2d --lib` and `cargo test --workspace`
Expected: pass.

- [ ] **Step 5: Docs**

- `src/core/ui/AGENTS.md`, "Prompts and rebinding": prompts follow the last device and the pad family; registered glyphs replace chips; markup prompts are resolved before layout; rebind rows capture the next key or pad button, Escape cancels, and a collision swaps.
- `src/core/input/AGENTS.md`: `PadFamily` and labels.

- [ ] **Step 6: Gate and commit**

Stage the files by name.

```bash
git commit -m "feat: input prompts and a rebinding row" -m "- prompts show the action's key or pad button for the last device, labelled for the pad family; games can register glyphs
- [prompt=action] in markup, in UI and world text
- rebind rows capture the next key or pad button; a collision swaps; bindings save"
```

### Task 15: UI sounds

**Files:**
- Modify:
  - `src/core/ui/memory.rs`: `Interaction::sounds`.
  - `src/core/ui/nav.rs`: focus moves, confirm, back.
  - `src/core/ui/widgets/{slider,drag,rebind}.rs`: ticks and errors.
  - `src/core/engine/frame.rs`: load and play.
  - `src/core/ui/theme/mod.rs`: `UiSounds` docs.
- Test: `src/core/ui/nav.rs`, `src/core/ui/widgets/slider.rs`, `src/core/engine/frame.rs` (tests)

**Interfaces:**
- Consumes: Task 9's `UiSounds`, `Audio::{load_sound, play_sound_with, is_available}`, `SoundOptions`, `SceneHost::audio`.
- Produces:

```rust
pub(crate) enum UiSound { Focus, Confirm, Back, Tick, Error }   // Debug, Clone, Copy, PartialEq, Eq, Hash
Interaction::sounds: Vec<UiSound>      // this frame's events, at most one of each kind
pub(crate) fn play(&mut self, sound: UiSound);   // on UiContext and in resolve; dedups per frame
```

**The rules:**
- **Events.**
  - `Focus`: focus moved by navigation (not by the pointer).
  - `Confirm`: an activation by Confirm, or a click on a button, tab, choice or option.
  - `Back`: Back used by a widget, or `ui.back()` returning true.
  - `Tick`: a slider changed by a step. Rate-limited to one every 0.05 s by a timestamp in `Interaction`.
  - `Error`: a drop released over a target that refuses its type, or a cancelled rebind.
- **Loading.** When the theme's generation changes, the engine loads each named sound with `audio.load_sound("ryo2d.ui.<kind>", path)`. A failure, including no audio device or the web, warns once per path (`"could not load UI sound {path}: {error}"`) and leaves that sound silent.
- **Playing.** After the UI pass, each event whose sound is named is played with `audio.play_sound_with("ryo2d.ui.<kind>", SoundOptions::new().volume(theme.sounds.volume))`, and errors are ignored. `UiSounds::default()` names none, so games are silent until they set sounds.

- [ ] **Step 1: Write the failing tests**

In `src/core/ui/nav.rs`'s tests, or `button.rs` with the `UiDriver` (whose `last_sounds()` returns the frame's `Interaction::sounds`):

```rust
  #[test]
  fn navigation_and_activation_raise_sounds_once_a_frame() {
    let mut d = UiDriver::new((300.0, 300.0));
    let items = ["A", "B", "C"];
    for _ in 0..2 {
      d.frame(|ui| menu(ui, &items, &mut None));
    }
    d.nav(Direction::Down);
    d.frame(|ui| menu(ui, &items, &mut None));
    assert_eq!(d.last_sounds(), vec![UiSound::Focus]);
    d.confirm();
    d.frame(|ui| menu(ui, &items, &mut None));
    assert_eq!(d.last_sounds(), vec![UiSound::Confirm]);
    d.move_to(10.0, 5.0);
    d.frame(|ui| menu(ui, &items, &mut None));
    assert!(d.last_sounds().is_empty(), "focus that follows the mouse is silent");
  }
```

In `slider.rs`'s tests (its `slider` helper focuses one slider in a scope):

```rust
  #[test]
  fn slider_ticks_are_rate_limited() {
    use crate::core::ui::memory::UiSound;
    let mut d = UiDriver::new((200.0, 100.0));
    let mut v = 0.0;
    d.frame(|ui| slider(ui, &mut v));
    d.frame(|ui| slider(ui, &mut v));
    let mut ticks = 0;
    for _ in 0..20 {
      d.nav(Direction::Right);
      d.frame(|ui| slider(ui, &mut v));
      ticks += usize::from(d.last_sounds().contains(&UiSound::Tick));
    }
    assert!((v - 1.0).abs() < 1e-5, "every press stepped: {v}");
    assert!((1..=7).contains(&ticks), "one tick per 0.05 s at most, every third frame: {ticks}");
  }
```

In `src/core/engine/frame.rs`'s tests:

```rust
  #[test]
  fn a_ui_sound_that_cannot_load_warns_once_and_the_frames_run() {
    use crate::core::ui::theme::{Theme, UiTheme};
    let mut e = engine();
    let mut theme = Theme::dark();
    theme.sounds.confirm = Some("sounds/missing-confirm.ogg".to_string());
    e.world_mut().resource_mut::<UiTheme>().set(theme);
    for _ in 0..3 {
      e.frame(Some(Duration::from_millis(16)));
    }
    let warnings = crate::core::dev_log::get_entries()
      .iter()
      .filter(|w| w.message.contains("missing-confirm.ogg"))
      .count();
    assert_eq!(warnings, 1, "a warning naming the sound, once");
  }
```

Headless, audio is unavailable, so the load fails. Warn with `engine_warn!(module: "ui", "could not load UI sound {path}: {error}")`.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p ryo2d --lib ui:: engine::`
Expected: compile errors.

- [ ] **Step 3: Implement** by the rules above.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ryo2d --lib` and `cargo test --workspace`
Expected: pass.

- [ ] **Step 5: Gate and commit**

```bash
git add src/core/ui/memory.rs src/core/ui/nav.rs src/core/ui/widgets/slider.rs src/core/ui/widgets/drag.rs src/core/ui/widgets/rebind.rs src/core/ui/widgets/button.rs src/core/ui/test_util.rs src/core/ui/theme/mod.rs src/core/engine/frame.rs
git commit -m "feat: UI sounds from the theme" -m "- focus moves, confirm, back, slider ticks and errors play the theme's sounds at its volume
- silent unless the theme names sounds; missing audio is skipped"
```

### Task 16: The games and demos on layout and navigation

**Files:**
- Modify:
  - Games:
    - `examples/games/breakout/src/scenes/{menu,pause,quit_confirm}.rs`
    - `examples/games/bullet-hell/src/scenes/{pause,menu,game_over}.rs`
    - `examples/games/flappy/src/scenes/{menu,game_over}.rs`
    - `examples/games/leap-king/src/scenes/{menu,game}.rs`
  - Demos:
    - `examples/demos/networking/src/scenes/menu.rs`
    - `examples/demos/asset-loading/main.rs`
    - `examples/demos/ui-animations/src/scenes/interactive_demo.rs`
- Create: `examples/games/breakout/tests/menus.rs`, `examples/games/flappy/tests/menus.rs`
- Modify (tests): `examples/games/bullet-hell/tests/headless.rs`, `examples/games/leap-king/tests/headless.rs`

**Interfaces:**
- Consumes: every widget and container from Tasks 5-15; `ui.back()`, `ui.confirm_with`, `ui.radio`, `ui.text_input`; `Input::action_pressed("ui.confirm")`; `HeadlessEngine` pad helpers (Task 1).
- Produces: no API. Every menu is navigable by mouse, keyboard and pad through the UI, and the hand-rolled menu navigation in `update` is gone. Gameplay pad code is untouched: breakout's paddle and launch, bullet-hell's pause trigger, leap-king's aim and respawn.

**The ports.** The positions follow today's layout, read from `plan`-time call-site notes; each screen stays where it was, now as layout.

- **breakout, `menu.rs`.**
  - The START GAME button goes in `ui.column().focus_scope().anchor(Anchor::TopCenter).offset(0.0, 350.0)`, with `.width(200.0).height(50.0).default_focus()`.
  - The title texts stay absolute.
  - Delete the Enter/Space/pad South/Start start code (lines ~84-88). Confirm on the focused button starts the game.
  - Replace the Esc/pad East/Select quit-confirm code (lines ~76-81, 107-111) with `if ui.back() { push QuitConfirmScene }` in `ui`, which needs no `escape_was_released` gate.
- **breakout, `pause.rs`.**
  - A centred `ui.panel().focus_scope().width(340.0)` column holds:
    - the PAUSED title and score;
    - three rows (`ui.row()`) of a label, `ui.slider(&mut settings.x_volume, 0.0..=1.0).step(0.05).fill_width()` and a percentage text;
    - RESUME (`default_focus`), QUIT TO MENU and QUIT TO DESKTOP, each `fill_width`.
  - A slider's `.changed()` applies its volume, replacing the `prev_*_volume` diffing.
  - `ui.back()` resumes (Pop).
  - Keep `Q` as a keyboard shortcut to the menu. Delete the pad Select and the Enter/South/Start/East handling.
- **breakout, `quit_confirm.rs`.** `match ui.confirm_with("Quit to Desktop?", "", "YES", "NO") { Some(true) => Quit, Some(false) => Pop, None => Continue }`. Delete the Y/N/Esc/pad code.
- **bullet-hell, `pause.rs`.**
  - The main menu is a centred `ui.panel().focus_scope().wrap_focus()` with Resume (`default_focus`), Settings, Quit to Menu and Quit to Desktop.
  - Settings opens a `ui.modal()` with "Display Mode", three `ui.radio(&mut self.selected_display_mode_idx, i, mode.display_name())`, and Apply and Back buttons. `ui.back()` closes the modal when it is open, else resumes.
  - Delete `pause_menu_focus`, `settings_menu_focus`, their outline drawing and the whole nav block (~98-226).
  - The `"pause-*"` variants from Task 9 stay for colour.
- **bullet-hell, `menu.rs` and `game_over.rs`.** Text-only screens. Their Space/Enter/pad South/Start checks become `ctx.input.action_pressed("ui.confirm")`, plus `GamepadButton::Start` kept as an extra start key. Pad East becomes `action_pressed("ui.back")`.
- **flappy, `menu.rs` and `game_over.rs`.** The buttons go in `ui.column().focus_scope()` columns at their current places, with START (or RESTART) as `default_focus`. Space/Enter start comes through Confirm; delete the `update` code for it. Keep `D` (debug), and use `ui.back()` for Esc on the game-over screen.
- **leap-king, `menu.rs`.** Rebuilt as a centred column, `ui.column().focus_scope().anchor(Anchor::Center).gap(10.0)`, of:
  1. title and subtitle;
  2. Single Player (`default_focus`);
  3. a row of `ui.radio(&mut self.host_mode, HostMode::Lan, "LAN")` and `ui.radio(.., HostMode::Internet, "Internet")`;
  4. the signal URL field (Internet only, `fill_width`);
  5. Create Room;
  6. "- or join -";
  7. a row of the room-code field (`max_len(ROOM_CODE_LENGTH)`) and the address field;
  8. Join Room;
  9. the hint and error texts.

  Delete `selected: MenuItem`, `input_cooldown`, the highlight rects, the `>` labels and the nav block (lines 345-412). `submitted()` on either join field tries to join. `ui.back()` quits.
- **leap-king, `game.rs`.** While `show_quit_dialog`, run `match ui.confirm_with("Leave Game?", "", "Yes, Leave", "Cancel")`: `Some(true)` sets `pending_quit`, `Some(false)` closes the dialog. Delete `quit_dialog_selection`, the cooldowns, and the dialog's arrow, East and Enter handling (~3311-3337); keep Esc/pad Start opening the dialog.
- **networking, `menu.rs` (Lobby).** A centred `ui.column().focus_scope()` with Create Room (`default_focus`), the two fields in a row, and Join Room. `submitted()` joins.
- **asset-loading.** Put its two buttons in `ui.column().focus_scope()` columns at their places (Load Assets with `default_focus`).
- **ui-animations, `interactive_demo.rs`.**
  - Each `AnimatedButton` draws through `ui.custom(w, h).at(rect).sense(Sense::click()).paint(..)`, and its hover, press and click animations read the `Response`.
  - Delete its own hit testing, the `ui.input()` mouse reads, and the `is_hovered(full screen)` gate: a help overlay that blocks now blocks the custom widgets too.

- [ ] **Step 1: Write the failing tests**

`examples/games/breakout/tests/menus.rs`:

```rust
use breakout::scenes::PauseScene;
use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

#[test]
fn the_pause_menu_is_navigable_by_pad() {
  let Some(mut engine) = HeadlessEngine::new_or_skip(EngineBuilder::new().resolution(800, 600).start_scene(PauseScene::new(0, 0)))
  else {
    return;
  };
  engine.step_frames(3).unwrap();
  // Resume is the default; down twice reaches Quit to Desktop
  for _ in 0..2 {
    engine.tap_pad(GamepadButton::DPadDown);
    engine.step_frames(2).unwrap();
  }
  engine.tap_pad(GamepadButton::South);
  let outcome = engine.step_frames(3).unwrap();
  assert_eq!(outcome, ryo2d::testing::StepOutcome::Quit);
}
```

`examples/games/flappy/tests/menus.rs`:

```rust
use flappy::components::Bird;
use flappy::scenes::MenuScene;
use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

fn birds(engine: &mut HeadlessEngine) -> usize {
  let world = engine.world_mut();
  world.query::<&Bird>().iter(world).count()
}

#[test]
fn confirm_starts_from_the_menu() {
  let Some(mut engine) = HeadlessEngine::new_or_skip(EngineBuilder::new().resolution(800, 600).start_scene(MenuScene::new())) else {
    return;
  };
  engine.step_frames(3).unwrap();
  assert_eq!(birds(&mut engine), 0, "the menu has no bird");
  // START is the menu's default focus, so Confirm presses it
  engine.tap_key(KeyCode::Enter);
  engine.step_frames(60).unwrap(); // past the fade
  assert_eq!(birds(&mut engine), 1, "the game scene spawned its bird");
}
```

The menu's start fade is 300 ms, so 60 frames is past it.

`examples/games/bullet-hell/tests/headless.rs` (add):

```rust
#[test]
fn the_pause_menu_resumes_by_pad() {
  use ryo2d::core::scene::PhysicsPaused;
  let builder = EngineBuilder::new().pixel_art(1920, 1080).fixed_hz(144).start_scene(GameScene::new());
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  engine.step_frames(10).expect("frames");
  let paused = |engine: &HeadlessEngine| engine.world().resource::<PhysicsPaused>().0;
  assert!(!paused(&engine));
  engine.tap_pad(GamepadButton::Start);
  engine.step_frames(4).expect("frames");
  assert!(paused(&engine), "Start pushed the pause menu");
  // Resume is the default focus
  engine.tap_pad(GamepadButton::South);
  engine.step_frames(4).expect("frames");
  assert!(!paused(&engine), "South on Resume popped it");
}
```

`examples/games/leap-king/tests/headless.rs` (add; `engine()` and `player()` are the file's helpers):

```rust
#[test]
fn the_leave_dialog_cancels_with_back_and_defaults_to_cancel() {
  let Some(mut engine) = engine() else {
    return;
  };
  engine.step_frames(30).expect("frames");
  let before = player(&mut engine).entity;
  engine.tap_key(KeyCode::Escape); // open the dialog
  engine.step_frames(3).expect("frames");
  engine.tap_pad(GamepadButton::East); // Back: No
  engine.step_frames(3).expect("frames");
  engine.tap_key(KeyCode::Escape); // open it again
  engine.step_frames(3).expect("frames");
  engine.tap_pad(GamepadButton::South); // Confirm on the default, Cancel
  engine.step_frames(10).expect("frames");
  assert_eq!(player(&mut engine).entity, before, "still in the same game");
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p breakout -p flappy -p bullet-hell -p leap-king`
Expected: failures. Pad navigation does nothing in today's widget-less pause menus, or the hand-rolled code reacts differently.

- [ ] **Step 3: Port** each file as described.

- [ ] **Step 4: Run and look**

Run: `cargo test --workspace` and `cargo build --workspace --all-targets`
Expected: pass.

Capture every ported menu headlessly from the game's folder:
- the games' menus, pause and confirm screens;
- the networking lobby;
- asset-loading;
- the interactive demo.

For example: `(cd examples/games/breakout && RYO_HEADLESS=1 RYO_INPUT="20:PadDPadDown 40:PadDPadDown" RYO_CAPTURE_FRAMES=30,60 RYO_CAPTURE_EXIT=1 RYO_CAPTURE_DIR=$PWD/../../../target/ui-4b/breakout cargo run --release -p breakout)`. Reach the pause screens with their keys.

Look at every capture. Check that:
- each screen sits where it did;
- focus rings show after pad input;
- nothing overlaps or is clipped.

List the paths and what you saw in the report.

- [ ] **Step 5: Gate and commit**

Stage each changed file by name.

```bash
git commit -m "feat: game menus on layout and gamepad navigation" -m "- breakout, bullet-hell, flappy, leap-king and the networking lobby menus are focus-scoped layouts
- pause, quit and leave dialogs use sliders, radios, modals and ui.confirm
- the hand-rolled pad navigation in update is gone"
```

### Task 17: The demo shell on UI focus

**Files:**
- Modify (engine):
  - `src/core/ui/widgets/custom.rs`: `takes_horizontal`, `takes_vertical`.
  - `src/core/ui/immediate/context.rs`: public `nav_step`.
- Modify (demo-kit):
  - `examples/demo-kit/src/scene.rs`: the arrows leave `shell_keys`; the panel's directions come from `ui.nav_step`; no `set_mouse_occluded`.
  - `examples/demo-kit/src/chrome.rs`: the panel is a focusable widget in a focus scope; help is a modal; `Drawn::panel_id`.
  - `examples/demo-kit/src/lib.rs`: drop `mod hold`.
- Delete: `examples/demo-kit/src/hold.rs` (the UI's repeat replaces it)
- Modify (tests and docs): `examples/demo-kit/tests/shell.rs`, `examples/demos/AGENTS.md`
- Test: `src/core/ui/widgets/custom.rs` (tests module), `examples/demo-kit/tests/shell.rs`

**Interfaces:**
- Consumes:
  - Task 6: `ui.custom`, `Sense`.
  - Task 8: focus, `nav_step_for`, `TAKES_HORIZONTAL`/`TAKES_VERTICAL`, repeat with disarm on a scene change, `modal_stack`.
  - Task 12: `ui.modal()`, the narrowed `wants_keyboard`.
  - Task 1: `HeadlessEngine::tap_pad`.
- Produces:

```rust
impl Custom<'_, '_> {
  /// Left and Right go to this widget while it is focused, instead of moving focus
  pub fn takes_horizontal(self) -> Self;
  /// Up and Down go to this widget while it is focused, instead of moving focus
  pub fn takes_vertical(self) -> Self;
}
impl UiContext<'_> {
  /// The direction the focused widget `id` took this frame (with the UI's hold-to-repeat), if any
  pub fn nav_step(&self, id: UiId) -> Option<Direction>;
}
// demo-kit, crate-private
pub(crate) struct Drawn { pub panel: Option<Rect>, pub scroll: usize, pub panel_id: Option<UiId>, pub clicked_row: Option<usize> }
/// The row whose rect holds (x, y), from the visible rows' (index, rect)
pub(crate) fn row_at(rows: &[(usize, Rect)], x: f32, y: f32) -> Option<usize>;
```

**The rules:**
- **The panel.** It is one focusable widget covering the panel rect, made with `ui.custom(w, h).id(0).sense(Sense::click()).focusable(true).takes_horizontal().takes_vertical().default_focus()`. It sits in `ui.stack().focus_scope().at(panel)`, drawn after the panel's absolute draws, and draws nothing itself: the rows, the highlight and the scroll window stay as they are.
  - **Directions.** The shell reads `ui.nav_step(panel_id)` with the id from the last frame. Up and Down move `selected`, and Right and Left are the `+1`/`-1` adjustment for `read_rows`.
  - **Repeat.** It comes from the UI (0.35 s, then every 0.1 s), so `HoldRepeat` goes. A held arrow does nothing on a rebuilt page until pressed again, by Task 8's disarm.
  - **The D-pad and left stick** drive the panel too, through the same actions.
  - **Pages without their own scope.** On a page with no `.focus_scope()`, the panel is the only scope, takes focus when it appears, and the arrows work as before.
  - **Pages with their own scope.** On a page with one (the ui demo's menus), the page's scope comes first in order and keeps the arrows. Tab (`ui.next`) moves focus to the panel and back, and a click on the panel focuses it.
  - **No rows.** A page with no rows has no panel and no scope from the shell, so the shell takes no arrows.
  - **The mouse.** A click on the panel focuses it, and selects the row under the pointer. `draw_panel` keeps each visible row's highlight rect with its index. When the panel's response is `clicked()`, it maps `resp.pointer()` through `row_at` into `Drawn::clicked_row`, which the shell makes `selected` for the next frame. Values still change with Left and Right.
- **Help.**
  - **Drawing.** It is drawn in `ui.modal().padding(0.0)`, holding one `ui.custom(box_w, box_h)`. Its painter draws today's box and lines in the box's own units. The lines, truncation and the "more" lines are measured before the modal is opened, since the painter has no measuring.
  - **What the modal replaces.** Its dim replaces `draw_modal_overlay`, and its blocking replaces `set_mouse_occluded()`.
  - **Focus and keys.** The modal has nothing focusable, so focus empties while it is open, and the arrows go nowhere (Task 8). When it closes, focus returns to what it was.
  - **The page's update.** `update` keeps `input.block_all()` while help is open: gameplay input is not the UI's.
- **The shell's own keys** (H, Esc, `[`, `]`, PageUp, PageDown, R) are taken as before, and skipped while `keyboard_wanted`, which is a focused field or an open list (Task 12).
- **Help text.** The help's demo-keys line reads "Up/Down  pick a setting   Left/Right  change it   Tab  to the page and back".

- [ ] **Step 1: Write the failing tests**

`src/core/ui/widgets/custom.rs` (add):

```rust
  #[test]
  fn a_custom_widget_can_take_directions() {
    use crate::core::input::Direction;
    use crate::core::ui::UiId;
    let mut d = UiDriver::new((200.0, 100.0));
    let draw = |ui: &mut UiContext<'_>| {
      let mut id = UiId::NONE;
      ui.column().focus_scope().show(|ui| {
        id = ui.custom(80.0, 40.0).focusable(true).takes_horizontal().takes_vertical().default_focus().show().id;
      });
      id
    };
    let id = d.frame(draw);
    d.frame(draw);
    d.nav(Direction::Up);
    assert_eq!(d.frame(|ui| ui.nav_step(draw(ui))), Some(Direction::Up));
    d.nav(Direction::Right);
    assert_eq!(d.frame(|ui| ui.nav_step(draw(ui))), Some(Direction::Right));
    assert_eq!(d.frame(|ui| { draw(ui); ui.focused() }), Some(id));
  }
```

`examples/demo-kit/src/chrome.rs` (add to its tests module):

```rust
  #[test]
  fn row_at_finds_the_row_under_a_point() {
    let rows = [(3, Rect::new(0.0, 20.0, 100.0, 18.0)), (4, Rect::new(0.0, 38.0, 100.0, 18.0))];
    assert_eq!(row_at(&rows, 10.0, 40.0), Some(4));
    assert_eq!(row_at(&rows, 10.0, 21.0), Some(3));
    assert_eq!(row_at(&rows, 10.0, 80.0), None);
    assert_eq!(row_at(&[], 10.0, 40.0), None);
  }
```

`examples/demo-kit/tests/shell.rs`:
- **New `Seen` field:** `menu_clicks: Vec<usize>`.
- **Add:**

```rust
/// A page with its own menu (a focus scope of two buttons) and one setting
struct MenuProbe {
  shared: Shared,
  level: f32,
}

impl DemoPage for MenuProbe {
  fn name(&self) -> &str {
    "menu"
  }

  fn settings(&mut self, s: &mut Settings) {
    s.float("Level", &mut self.level, 0.0..=1.0, 0.1);
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) {
    if let Ok(mut seen) = self.shared.lock() {
      seen.level = self.level;
    }
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    let area = content_area(ui);
    let mut clicked = None;
    ui.column().focus_scope().at(Rect::new(area.x + 10.0, area.y + 10.0, 100.0, 80.0)).show(|ui| {
      if ui.button("First").default_focus().clicked() {
        clicked = Some(0);
      }
      if ui.button("Second").clicked() {
        clicked = Some(1);
      }
    });
    if let (Some(i), Ok(mut seen)) = (clicked, self.shared.lock()) {
      seen.menu_clicks.push(i);
    }
  }
}

fn menu_page(shared: &Shared) -> Demo {
  let shared = shared.clone();
  Demo::new("Menu").resolution(320, 240).page(move || MenuProbe { shared: shared.clone(), level: 0.5 })
}

/// Tap a pad button, then run two frames
fn tap_pad(engine: &mut HeadlessEngine, button: GamepadButton) {
  engine.tap_pad(button);
  let _ = engine.step();
  let _ = engine.step();
}

#[test]
fn the_d_pad_drives_the_settings_panel() {
  let shared = Shared::default();
  let Some(mut engine) = start(three_pages(&shared)) else {
    return;
  };
  engine.step_frames(2).unwrap();
  tap_pad(&mut engine, GamepadButton::DPadRight); // Level 0.5 -> 0.6
  tap_pad(&mut engine, GamepadButton::DPadDown); // Count
  tap_pad(&mut engine, GamepadButton::DPadRight); // 3 -> 4
  let s = seen(&shared);
  assert!((s.level - 0.6).abs() < 1e-6, "{s:?}");
  assert_eq!(s.count, 4, "{s:?}");
}

#[test]
fn a_page_with_its_own_menu_keeps_the_arrows_and_tab_reaches_the_panel() {
  let shared = Shared::default();
  let Some(mut engine) = start(menu_page(&shared)) else {
    return;
  };
  engine.step_frames(2).unwrap();
  tap(&mut engine, KeyCode::ArrowDown); // the menu: First -> Second
  tap(&mut engine, KeyCode::ArrowRight); // nothing to the right in the menu
  tap(&mut engine, KeyCode::Enter);
  let s = seen(&shared);
  assert_eq!(s.menu_clicks, vec![1], "{s:?}");
  assert_eq!(s.level, 0.5, "the panel did not see the arrows");
  tap(&mut engine, KeyCode::Tab); // Second -> the panel
  tap(&mut engine, KeyCode::ArrowRight);
  assert!((seen(&shared).level - 0.6).abs() < 1e-6);
}

#[test]
fn closing_help_gives_the_panel_its_arrows_back() {
  let shared = Shared::default();
  let Some(mut engine) = start(three_pages(&shared)) else {
    return;
  };
  engine.step_frames(2).unwrap();
  tap(&mut engine, KeyCode::KeyH);
  tap(&mut engine, KeyCode::ArrowRight);
  assert_eq!(seen(&shared).level, 0.5, "help is modal");
  tap(&mut engine, KeyCode::KeyH);
  tap(&mut engine, KeyCode::ArrowRight);
  assert!((seen(&shared).level - 0.6).abs() < 1e-6);
}
```

Keep every existing test, unchanged in what it asserts:
- `arrows_select_rows_and_change_values`;
- `holding_right_repeats_and_releasing_stops`: the UI repeats every 0.1 s, so 36 frames give 0.9, which is still inside its bounds;
- `holding_right_through_a_reset_does_not_keep_adjusting`;
- `selection_clamps_when_rows_disappear`;
- `arrows_do_nothing_while_help_is_open`;
- the help and Escape tests;
- `a_click_on_the_settings_panel_does_not_reach_the_page`;
- `page_widgets_take_no_clicks_under_help`.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p ryo2d --lib ui::widgets::custom` and `cargo test -p demo-kit`
Expected: compile errors for `takes_horizontal`/`nav_step`. Once those compile, `the_d_pad_drives_the_settings_panel` and the menu test fail, because the shell still takes raw arrows and ignores the pad.

- [ ] **Step 3: Implement**

Add the two `Custom` builders (they set `TAKES_HORIZONTAL`/`TAKES_VERTICAL` on the record) and `UiContext::nav_step` (public, returning `nav_step_for(id)`).

`examples/demo-kit/src/scene.rs`:
- `shell_keys` returns `Result<(), SceneAction>` and loses the arrow and hold handling.
- `PageScene` loses `left`/`right` and gains `panel_id: Option<UiId>`.
- `ui` becomes:

```rust
  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
    // Typing into a widget, or picking from an open list: the shell's keys belong to it
    if !self.keyboard_wanted
      && let Err(action) = self.shell_keys(ui)
    {
      return action;
    }
    // The panel takes the arrows and the D-pad through UI focus, with the UI's hold-to-repeat
    let last = self.rows.len().saturating_sub(1);
    let adjust = match self.panel_id.and_then(|id| ui.nav_step(id)) {
      Some(Direction::Down) => {
        self.selected = (self.selected + 1).min(last);
        0
      }
      Some(Direction::Up) => {
        self.selected = self.selected.saturating_sub(1);
        0
      }
      Some(Direction::Right) => 1,
      Some(Direction::Left) => -1,
      None => 0,
    };
    self.read_rows(adjust);
    self.page.ui(ui, ctx);
    self.keyboard_wanted = ui.wants_keyboard();
    // (the reserved-key check and the View, as now)
    let drawn = chrome::draw(ui, &view);
    self.panel = drawn.panel;
    self.panel_id = drawn.panel_id;
    self.scroll = drawn.scroll;
    if let Some(row) = drawn.clicked_row {
      self.selected = row;
    }
    SceneAction::Continue
  }
```

`examples/demo-kit/src/chrome.rs`:
- `draw_panel` returns the panel's id with its rect, from:

```rust
  let mut id = None;
  ui.stack().focus_scope().at(panel).show(|ui| {
    id = Some(
      ui.custom(panel.width, panel.height)
        .id(0)
        .sense(Sense::click())
        .focusable(true)
        .takes_horizontal()
        .takes_vertical()
        .default_focus()
        .show()
        .id,
    );
  });
```

- `draw_help` measures and truncates its lines first (as now, into `(text, colour, x, y)` in box units), then draws inside `ui.modal().padding(0.0).show(|ui| { ui.custom(box_w, box_h).paint(|p, _| { .. }) })`, with `p.rect` for the box and `p.text` for each line in the shell's text style.

Delete `hold.rs` and its `mod`.

- [ ] **Step 4: Run the tests and look**

Run: `cargo test -p demo-kit` and `cargo test --workspace` and `cargo build --workspace --all-targets`
Expected: pass.

Capture:
- a demo page with settings and its help, e.g. `(cd examples/demos/particles && RYO_HEADLESS=1 RYO_INPUT="20:KeyH" RYO_CAPTURE_FRAMES=10,40 RYO_CAPTURE_EXIT=1 RYO_CAPTURE_DIR=$PWD/../../../target/ui-4b/shell cargo run --release)`;
- `./dev demos:tour particles ui`.

Look at the PNGs: the panel and the help match the captures from before the task, apart from the help's dim, which is now the theme's modal dim.

- [ ] **Step 5: Docs**

In `examples/demos/AGENTS.md`, rewrite the shell-keys bullets:
- the settings panel is a focus scope, so the arrows, the D-pad and the left stick reach it through UI navigation;
- a page with its own `.focus_scope()` keeps the arrows, and Tab moves between the page and the panel;
- help is a modal;
- while a text field is focused or a list is open, the shell ignores its keys;
- the arrows stay reserved: pages use them only through UI focus.

Also update the `ui(ui, ctx)` row: read keys with `ui.input()` or actions, and use `ui.back()` for a page's own Back.

- [ ] **Step 6: Gate and commit**

```bash
git add src/core/ui/widgets/custom.rs src/core/ui/immediate/context.rs examples/demo-kit/src/scene.rs examples/demo-kit/src/chrome.rs examples/demo-kit/src/lib.rs examples/demo-kit/tests/shell.rs examples/demos/AGENTS.md
git rm examples/demo-kit/src/hold.rs
git commit -m "feat: the demo shell's settings panel on UI focus" -m "- the panel is a focus scope: arrows, the D-pad and the stick drive it with the UI's repeat
- pages with their own menus keep the arrows; Tab moves between page and panel
- help is a modal that traps focus and blocks the page"
```

### Task 18: The `ui` demo: the acceptance showcase

**Files:**
- Modify (engine):
  - `src/core/testing/headless.rs`: `ui_focused`, `ui_focusables`, `focus_ui`.
  - `src/core/ui/nav.rs`: `nav_targets`, shared by `resolve_focus` and the headless reads.
  - `src/core/engine/mod.rs`: crate accessors for the memory.
  - `src/core/text/world.rs`: `TextReveal::finish`.
- Create:
  - `examples/demos/ui/src/lib.rs`: `pub mod scenes; pub fn demo() -> Demo`.
  - `examples/demos/ui/src/scenes/{menu_page,settings_page,inventory_page,dialogue_page,art}.rs`.
  - `examples/demos/ui/tests/acceptance.rs`.
- Modify (demo):
  - `examples/demos/ui/Cargo.toml`: `[lib] name = "ui_demo"` and `[[bin]] name = "ui"`.
  - `examples/demos/ui/src/main.rs`: `ui_demo::demo().run()`.
  - `examples/demos/ui/src/scenes/{mod,layout_demo,clipping_demo,theme_demo}.rs`.
- Test: `tests/e2e/headless_engine.rs`, `examples/demos/ui/tests/acceptance.rs`

**Interfaces:**
- Consumes: every widget from Tasks 6-15; `Demo::start_page`; `HeadlessEngine::{tap_pad, tap_key, step_frames}`; `ctx.actions()`; `ctx.ui_theme()`; `Theme::pixel()`.
- Produces:

```rust
// nav.rs (crate): what directional navigation can target now in the top scene — rule 1's focusables
// minus NAV_SKIP, restricted to the innermost scope on modal_stack when one is open, in order
pub(crate) fn nav_targets(memory: &UiMemory, top_scene: Option<u64>) -> Vec<(UiId, Rect)>;
// HeadlessEngine
pub fn ui_focused(&self) -> Option<UiId>;
pub fn ui_focusables(&self) -> Vec<(UiId, Rect)>;   // nav_targets for the current top scene, as last drawn
pub fn focus_ui(&mut self, id: UiId);               // a request_focus applied at the next frame's resolve
// text
impl TextReveal { /// Show the rest of the text at the next advance, and be done
  pub fn finish(&mut self); }
// ui demo
pub fn demo() -> Demo;   // the pages below, in order
```

**The pages, in order:**
1. Menus
2. Settings
3. Inventory
4. Dialogue
5. Theme
6. Layout
7. Clipping & Modals
8. Text
9. Text Input
10. Scroll Views
11. Dropdowns

The first four are the acceptance pages. They declare no settings rows: the panel is reached by Tab, not the D-pad, and on these pages every focusable must be reachable by the D-pad alone. Each sets `wants_escape` while one of its modals is open, so Esc reaches its `ui.back()`.

- **Menus** (`menu_page.rs`), a title screen.
  - **Layout.** A title ("RYO QUEST", the heading role at 48) at `Anchor::TopCenter` with offset 80. Below it, `ui.panel().focus_scope().wrap_focus().anchor(Anchor::Center).width(260.0)` holds `fill_width` buttons:
    - **Continue,** `.enabled(self.has_save)`; `has_save` starts false.
    - **New Game,** `default_focus`. It sets `has_save = true` and the status to "A new game begins."
    - **Options.**
    - **Credits.**
    - **Quit.**
  - **Options** opens a modal: the heading "Options", `ui.slider(&mut self.volume, 0.0..=1.0).step(0.05).label("Volume").show_value()`, `ui.toggle(&mut self.fullscreen, "Fullscreen")`, `ui.checkbox(&mut self.subtitles, "Subtitles")` and Done (`default_focus`). Done or `ui.back()` closes it.
  - **Credits** opens a modal with a `ui.scroll().height(200.0)` of 30 credit lines and Close (`default_focus`). The right stick scrolls the lines (Task 8).
  - **Quit** runs `ui.confirm("Quit to desktop?", "Your progress is saved.")`. Yes sets the status to "Quit chosen (the demo stays open)." Both answers close it.
  - **Footer.** `RichText::markup("[prompt=ui.confirm] Select    [prompt=ui.back] Back")` at `Anchor::BottomCenter` with offset -40, and the status line above it.
- **Settings** (`settings_page.rs`).
  - **Layout.** A centred `ui.panel().width(600.0).height(460.0)` column: the heading "Settings"; `ui.tabs(&mut self.tab, &["Audio", "Video", "Controls"])`; a `ui.scroll().fill_width().grow(1.0)` of the tab's rows; and a footer row.
  - **Rows.** Each row is `ui.row().gap(12.0)` of a label (`grow(1.0)`) and its widget, inside the panel's `.focus_scope()`.
  - **Audio:**
    - sliders Master, Music, Effects and Voice (`0.0..=1.0`, `step(0.05)`, `show_value`, width 220);
    - `ui.checkbox(&mut self.mute_in_background, "Mute in background")`;
    - a dropdown "Output" over `["Default", "Headphones", "Speakers"]`.
  - **Video:**
    - dropdowns "Window" (`["Windowed", "Borderless", "Fullscreen"]`) and "Resolution" (`["1280×720", "1600×900", "1920×1080", "2560×1440"]`);
    - `ui.toggle(&mut self.vsync, "VSync")`;
    - "UI scale" as three radios in a row (Small, Medium, Large over a `UiScale` enum);
    - a slider "Brightness" (`0.5..=1.5`, `step(0.05)`);
    - `ui.checkbox(&mut self.show_fps, "Show FPS")`.
  - **Controls:**
    - one `ui.rebind(name).label(label)` per page action: `move_left` "Move left", `move_right` "Move right", `jump` "Jump", `attack` "Attack", `interact` "Interact", `inventory` "Inventory", `pause` "Pause";
    - a "Reset controls" button calling `ctx.actions().reset_defaults()` from `update` (set a flag in `ui`).
    - The page binds its actions in `enter`:
      - `move_left`: `A`, `ArrowLeft`, `Stick:LeftLeft`;
      - `move_right`: `D`, `ArrowRight`, `Stick:LeftRight`;
      - `jump`: `Space`, `Pad:South`;
      - `attack`: `J`, `Pad:West`;
      - `interact`: `E`, `Pad:North`;
      - `inventory`: `I`, `Pad:Select`;
      - `pause`: `P`, `Pad:Start`.
  - **Footer:** `RichText::markup("[prompt=ui.tab_prev] [prompt=ui.tab_next] Switch tab    [prompt=ui.back] Back")`.
  - **Values** are the page's fields. Only bindings persist (Task 2).
- **Inventory** (`inventory_page.rs`).
  - **Items.** An `Item { kind: ItemKind, count: u32 }` (`Clone, Copy, PartialEq, Debug`), with `ItemKind`:
    - Sword (Weapon), Shield (Weapon), Helmet (Head), Armor (Body);
    - Potion, Gem, Key, Bread (none of these equip);
    - each with a name and a one-line description.
  - **Icons.** `art.rs` draws each 16×16 icon in code, the `examples/demos/text/main.rs` `Canvas` way: `SpriteHandle::from_sprite(Sprite::from_rgba(..), "ui-demo-sword")`. They draw at 32 in a 48 slot with `Filter::Nearest`.
  - **The bag** is 24 slots, starting with Sword, Helmet, Armor, Potion ×5, Gem ×12, Key, Bread ×3 and the rest empty.
  - **The equipment** is Head, Body and Weapon, empty.
  - **Layout.**
    - A centred row: an "Equipment" `ui.panel()` column of three rows, each a label and a slot (Head, Body, Weapon from the top), and a "Backpack" panel with `ui.grid(6).gap(6.0)` of the 24 slots (`.id(i)`). The equipment rows use the grid's gap and the two panels the same heading, so Head, Body and Weapon line up with the bag's first three rows, and Left from the bag's first column reaches them.
    - The bag's first slot is `default_focus`.
    - Under the row, a details panel: the name and description of the focused or hovered slot's item.
    - A row of "Sort" (by kind, merging stacks) and a "Discard" slot (a drop target that deletes the item).
    - The status line, and `RichText::markup("[prompt=ui.confirm] Pick up / put down    [prompt=ui.back] Cancel")`.
    - The whole page is one `.focus_scope()` column.
  - **Moving items.**
    - The payload is `SlotRef { Bag(usize), Equip(EquipSlot) }`, `Clone, Copy`. A non-empty slot is `drag_source(SlotRef::..)`, every slot is `drop_target::<SlotRef>()`, and each slot has `.tooltip(name)` and `.count(n)`.
    - A drop moves the item, or swaps when the target holds one.
    - An equipment slot refuses an item of the wrong kind: the status reads "A Potion doesn't go in the Head slot." and nothing moves.
- **Dialogue** (`dialogue_page.rs`).
  - **The tree.** A five-node conversation with a shopkeeper, as data in the page:
    - `struct Line { speaker: &'static str, text: &'static str, choices: &'static [(&'static str, Next)] }`, with `Next::{To(usize), End}`;
    - the texts in markup, with colour tags for item names;
    - one node with no choices, which continues.
  - **Layout.**
    - An `Anchor::BottomCenter` `ui.panel().width(760.0)` row: a 96×96 portrait (`ui.image(&self.portrait).size(96.0, 96.0).filter(Filter::Nearest)`, a 24×24 face drawn in `art.rs`), and a column of:
      - the speaker, in the heading role;
      - the line, as `ui.text(RichText::markup(line.text)).typewriter(&mut self.reveal)` inside `ui.column().max_width(600.0)`, with `TextReveal::new(40.0).with_punctuation_pause(0.15)` restarted on each new line;
      - while the line is still revealing, a Skip button (`default_focus`) that calls `self.reveal.finish()`;
      - once it is done, either `ui.choice_list(&labels).show()` or a Continue button (`default_focus`).
    - The panel is a `.focus_scope()`.
    - **A HUD** at `Anchor::TopLeft` with offset `(16, 16)`: `ui.row().gap(12.0)` (no scope, so it is mouse-only and takes no keys) of:
      - `ui.progress(self.health).label("HP").width(160.0)`;
      - two `ui.panel().padding(6.0)` counters holding `ui.text(format!("Gold {}", self.gold))` and `ui.text(format!("Gems {}", self.gems))`. They size to their text, so buying from the shopkeeper (a choice that spends gold for a gem or a potion that restores health) visibly resizes them.
  - **Choosing.** A choice moves to its `Next`, and number keys pick choices (Task 12). At the end the page shows "The shopkeeper waves goodbye. R starts again."
  - **Back.** `ui.back()` during the conversation opens `ui.confirm("Leave the conversation?", "The shopkeeper will remember.")`. Yes ends it, and No or Back returns to it. `wants_escape` is true until the conversation ends.
- **Theme** (updated): it cycles Dark, Light, Pixel, Neon and the file theme. `exit` restores `Theme::dark()`.
- **Layout** (rewritten on containers; the old `HBox`/`VBox`/`Stack` go in Task 19). Five labelled sections in a `ui.scroll()` column:
  1. a row of buttons A-D with `gap(self.gap)`;
  2. a 3×3 grid of tiles (`ui.grid(3)`), and a second grid with `.tracks(&[80.0, 160.0, 80.0])`;
  3. a 300×160 stack with nine anchored labels (TopLeft … BottomRight);
  4. a row with a fixed button, a `ui.spacer()`, and an end button, showing grow;
  5. a `.wrap_items()` row of 14 chips that wraps at the page's width.

  Its settings rows are Gap (`0..=32`, step 2) and Padding (`0..=32`, step 2). It keeps arrow navigation through its own `.focus_scope()`.
- **Clipping & Modals** (rewritten). It keeps its nested `push_clip` panels for absolute drawing, and replaces:
  - the hand-rolled scroll list, its scrollbar and its momentum, with a `ui.scroll()` of 40 rows;
  - its own focus, with a `.focus_scope()`.

  Inside the list it nests a horizontal `ui.scroll()` of 12 cards, to show the innermost scroll taking the wheel. "Show Modal Dialog" opens `ui.modal()` (text, Confirm, Cancel), and "Ask" runs `ui.confirm`. `wants_escape` is true while either is open, and the modal closes on `ui.back()`. `is_hovered`, `is_clicked` and `draw_modal_overlay` leave the page.

- [ ] **Step 1: Write the failing tests**

`tests/e2e/headless_engine.rs` (add):

```rust
#[test]
fn ui_focus_can_be_read_and_set_headlessly() {
  struct Menu;
  impl Scene for Menu {
    fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
      ui.column().focus_scope().show(|ui| {
        ui.button("A").default_focus().show();
        ui.button("B").show();
      });
      SceneAction::Continue
    }
  }
  let Some(mut engine) = HeadlessEngine::new_or_skip(EngineBuilder::new().resolution(200, 150).start_scene(Menu)) else {
    return;
  };
  engine.step_frames(3).unwrap();
  let targets = engine.ui_focusables();
  assert_eq!(targets.len(), 2);
  assert_eq!(engine.ui_focused(), Some(targets[0].0));
  engine.focus_ui(targets[1].0);
  engine.step_frames(1).unwrap();
  assert_eq!(engine.ui_focused(), Some(targets[1].0));
}
```

Beside the existing UI typewriter tests (find them with `grep -rn "typewriter(" src/core`), add:

```rust
  #[test]
  fn finishing_a_reveal_shows_the_whole_line() {
    let mut d = crate::core::ui::test_util::UiDriver::new((300.0, 100.0));
    let mut reveal = crate::core::text::TextReveal::new(1.0);
    d.frame(|ui| {
      ui.text("A long line to reveal").typewriter(&mut reveal).draw();
    });
    assert!(!reveal.is_done());
    reveal.finish();
    d.frame(|ui| {
      ui.text("A long line to reveal").typewriter(&mut reveal).draw();
    });
    assert!(reveal.is_done());
  }
```

`examples/demos/ui/tests/acceptance.rs`:

```rust
//! The acceptance pages, driven by a gamepad alone

use std::collections::BTreeSet;

use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

const DPAD: [GamepadButton; 4] = [GamepadButton::DPadUp, GamepadButton::DPadDown, GamepadButton::DPadLeft, GamepadButton::DPadRight];

fn start(page: usize) -> Option<HeadlessEngine> {
  let mut engine = HeadlessEngine::new_or_skip(ui_demo::demo().start_page(page).engine_builder())?;
  engine.step_frames(4).ok()?;
  Some(engine)
}

fn pad(engine: &mut HeadlessEngine, button: GamepadButton) {
  engine.tap_pad(button);
  engine.step_frames(3).unwrap();
}

/// The focusables reached from the current focus by D-pad presses alone
fn reachable(engine: &mut HeadlessEngine) -> BTreeSet<UiId> {
  let Some(start) = engine.ui_focused() else {
    panic!("the page focused nothing by itself");
  };
  let mut seen = BTreeSet::from([start]);
  let mut queue = vec![start];
  while let Some(from) = queue.pop() {
    for button in DPAD {
      engine.focus_ui(from);
      engine.step_frames(2).unwrap();
      pad(engine, button);
      if let Some(to) = engine.ui_focused()
        && seen.insert(to)
      {
        queue.push(to);
      }
    }
  }
  seen
}

fn assert_all_reachable(engine: &mut HeadlessEngine, what: &str) {
  let all: BTreeSet<UiId> = engine.ui_focusables().into_iter().map(|(id, _)| id).collect();
  assert!(!all.is_empty(), "{what}: nothing to focus");
  let reached = reachable(engine);
  let missed: Vec<Rect> = engine.ui_focusables().into_iter().filter(|(id, _)| !reached.contains(id)).map(|(_, r)| r).collect();
  assert!(missed.is_empty(), "{what}: the D-pad never reaches the widgets at {missed:?}");
}

#[test]
fn every_acceptance_page_is_fully_reachable_by_pad() {
  // Menus, Inventory and Dialogue as they open
  for (page, name) in [(0, "Menus"), (2, "Inventory"), (3, "Dialogue")] {
    let Some(mut engine) = start(page) else {
      return;
    };
    assert_all_reachable(&mut engine, name);
  }
  // Settings, on each tab in turn
  let Some(mut engine) = start(1) else {
    return;
  };
  for tab in ["Audio", "Video", "Controls"] {
    assert_all_reachable(&mut engine, &format!("Settings, {tab}"));
    pad(&mut engine, GamepadButton::RightBumper);
  }
}

#[test]
fn every_menu_dialog_is_reachable_and_closes_on_back() {
  let Some(mut engine) = start(0) else {
    return;
  };
  let ids = |engine: &HeadlessEngine| -> BTreeSet<UiId> { engine.ui_focusables().into_iter().map(|(id, _)| id).collect() };
  let page = ids(&engine);
  let Some(new_game) = engine.ui_focused() else {
    panic!("the menu focused nothing");
  };
  // Continue starts disabled, so New Game is first: Options is one down, Credits two, Quit three
  for (downs, name) in [(1, "Options"), (2, "Credits"), (3, "Quit")] {
    engine.focus_ui(new_game);
    engine.step_frames(2).unwrap();
    for _ in 0..downs {
      pad(&mut engine, GamepadButton::DPadDown);
    }
    pad(&mut engine, GamepadButton::South);
    assert!(ids(&engine).is_disjoint(&page), "{name} opened a modal over the menu");
    assert_all_reachable(&mut engine, name);
    pad(&mut engine, GamepadButton::East);
    assert_eq!(ids(&engine), page, "{name} closed on Back");
  }
}

#[test]
fn the_pad_moves_an_item_into_its_equipment_slot() {
  let Some(mut engine) = start(2) else {
    return;
  };
  let rect_of = |engine: &HeadlessEngine, id: UiId| engine.ui_focusables().into_iter().find(|(t, _)| *t == id).map(|(_, r)| r);
  let route = [GamepadButton::DPadLeft, GamepadButton::DPadDown, GamepadButton::DPadDown];
  // The Sword's bag slot has the first focus; Left reaches Head, then Body and Weapon below it
  let Some(sword) = engine.ui_focused() else {
    panic!("nothing focused");
  };
  for button in route {
    pad(&mut engine, button);
  }
  let Some(weapon) = engine.ui_focused() else {
    panic!("focus left the page");
  };
  let (Some(sword_rect), Some(weapon_rect)) = (rect_of(&engine, sword), rect_of(&engine, weapon)) else {
    panic!("the slots are not focusable");
  };
  let before = engine.frame().unwrap();
  engine.focus_ui(sword);
  engine.step_frames(2).unwrap();
  pad(&mut engine, GamepadButton::South); // pick up the Sword
  for button in route {
    pad(&mut engine, button);
  }
  pad(&mut engine, GamepadButton::South); // put it down on Weapon
  let after = engine.frame().unwrap();
  let differs = |r: Rect| {
    (r.x as u32..r.right() as u32).any(|x| (r.y as u32..r.bottom() as u32).any(|y| before.get_pixel(x, y) != after.get_pixel(x, y)))
  };
  assert!(differs(weapon_rect), "the Weapon slot shows the Sword now");
  assert!(differs(sword_rect), "the Sword's bag slot is empty now");
}
```

The engine frame is in physical pixels. The demo's 1024×768 headless window has scale 1, so UI units are pixels. Both frames have the focus on the Weapon slot and no tooltip yet (fewer than 30 frames of focus), so only the moved item differs.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --test e2e ui_focus_can_be_read` and `cargo test -p ui`
Expected: compile errors: no `ui_focusables`, no `ui_demo` library, no pages.

- [ ] **Step 3: Implement**

Engine:
- Add `nav_targets`, and make `resolve_focus` use it for its directional candidates.
- Add the three `HeadlessEngine` methods. They read the engine's `UiMemory` and the scene manager's top scope through crate accessors; `focus_ui` pushes a focus request.

Demo:
- Split the crate into `lib.rs` (pages and `demo()`) and `main.rs`.
- Write the pages by the descriptions above, and `art.rs` with a small canvas helper: `new(w, h)`, `set(x, y, rgba)`, `fill_rect`, `into_sprite(name)`.

- [ ] **Step 4: Run the tests and look**

Run: `cargo test -p ui` and `cargo test --workspace` and `cargo build --workspace --all-targets`
Expected: pass.

Capture each acceptance page, driven by pad scripts from `examples/demos/ui`. Each runs with `RYO_HEADLESS=1 RYO_CAPTURE_EXIT=1` and `RYO_CAPTURE_DIR=$PWD/../../../target/ui-4b/ui-<name>`, then `cargo run --release`:
- **Menus:** `RYO_DEMO_PAGE=1 RYO_INPUT="20:PadDPadDown 30:PadDPadDown 40:PadSouth" RYO_CAPTURE_FRAMES=10,60`: the menu, then Options.
- **Settings:** `RYO_DEMO_PAGE=2 RYO_INPUT="20:PadRightBumper 40:PadDPadDown 60:PadRightBumper" RYO_CAPTURE_FRAMES=10,50,80`.
- **Inventory:** `RYO_DEMO_PAGE=3 RYO_INPUT="20:PadSouth 30:PadDPadRight 40:PadDPadRight" RYO_CAPTURE_FRAMES=10,50`: a pad drag and its ghost.
- **Dialogue:** `RYO_DEMO_PAGE=4 RYO_INPUT="20:PadSouth 40:PadDPadDown" RYO_CAPTURE_FRAMES=10,60`.
- **Prompts on the keyboard:** one keyboard-driven capture of Menus (`RYO_INPUT="20:ArrowDown"`), showing the prompts as key labels rather than pad labels.

Then run `./dev demos:tour ui`.

Look at every PNG, and in the report list each path with what it shows. Check that:
- the focus ring is visible after pad input;
- the prompts show A and B on the pad captures and Enter and Esc on the keyboard one;
- nothing overlaps or is clipped;
- the pixel icons are crisp.

- [ ] **Step 5: Gate and commit**

Stage the new and changed files by name.

```bash
git commit -m "feat: the ui demo's menu, settings, inventory and dialogue pages" -m "- four acceptance pages built from the new widgets, every widget reachable by D-pad
- the layout and clipping pages rebuilt on containers, scroll areas and modals
- headless tests can read and set UI focus"
```

### Task 19: Removing the old API, and the UI debug overlay

**Files:**
- Delete: `src/core/ui/immediate/layout.rs` (`HBox`, `VBox`, `Stack`, `HAlign`, `VAlign`)
- Modify:
  - `src/core/ui/immediate/context.rs`:
    - remove `is_clicked`, `is_hovered`, `is_held`, the `mouse_occluded` field and `set_mouse_occluded`/`is_mouse_occluded`/`restore_mouse_occluded`;
    - remove `consume_click`, `click_consumed`, `escape_consumed`, `consume_escape` and `draw_focus_ring`;
    - remove their tests (`mouse_occluded_*`, `is_clicked_*`, `is_hovered_*`, `is_held_*` and the like).
  - `src/core/ui/immediate/mod.rs`, `src/core/ui/mod.rs` (exports and the module docs), `src/lib.rs` (prelude).
  - `src/core/scene/manager.rs`: the occlusion loop in `ui_pass`.
  - `src/core/engine/frame.rs`: the console block.
  - `src/core/console/mod.rs`: the close button's hover.
  - `src/core/ui/immediate/AGENTS.md`, `src/core/ui/AGENTS.md`.
  - For the overlay (second commit): `src/core/config/debug.rs` (`show_ui_debug`), `src/core/console/builtins.rs` (`debug:ui`), a new `src/core/ui/debug.rs` (`pub(crate) fn draw_debug_overlay(ui: &mut UiContext)`, reading this frame's records from the context's memory) with its `mod` line in `src/core/ui/mod.rs`, and `src/core/engine/frame.rs` (calling it), `.claude/skills/ryo-console/SKILL.md` and its CLI copy.
- Test: `src/core/engine/frame.rs` (tests module), `tests/e2e/headless_engine.rs`, `src/core/console/builtins.rs` (tests module)

**Interfaces:**
- Consumes: Task 6's `top_scene` gating in `UiContext::response` and the console blocker in `resolve_pointer`; Task 5's `begin_scene`/`end_scene`.
- Produces: no new API. After this task the UI's public surface is:
  - the builders (widgets and containers);
  - `Response`;
  - the absolute `draw_*` calls and `ui.text(..)`;
  - `ui.input()`, `consume_key`, `consume_scroll` and `wants_pointer`/`wants_keyboard`;
  - focus (`request_focus`, `focused`, `is_focused`, `focus_visible`, `back`, `nav_step`), `scroll_to`/`scroll_offset`/`set_scroll_offset` and the clips;
  - the theme;
  - the layer and clip controls the engine uses.

This task deletes API, so the compiler is its failing test: Step 1 deletes, and the build errors list every remaining call site.

The spec's `ui_debug` console command is added here as a second commit, named `debug:ui` after the console's other `debug:` commands:
- `DebugConfig` gains `show_ui_debug: bool` (default false; read from `Ryo.toml`'s debug table like the other flags, if that table is read field by field).
- `debug:ui` flips it and prints "UI debug overlay enabled" or "disabled".
- While it is on, `draw_ui` draws, after the scenes and before the console, on `OVERLAY_LAYER` with no clip, for every record written this frame (`UiMemory::current` records):
  - a 1-unit `draw_rect_outline` in `rgba(80, 200, 255, 200)` for the top scene's records and `rgba(140, 140, 140, 160)` for a lower scene's;
  - instead, in yellow for hot, orange `rgb(255, 150, 40)` for active and green `rgb(80, 220, 120)` for focused;
  - the id as `format!("{:04x}", id.0 & 0xffff)` in 8-unit text at the rect's top-left plus `(1, 1)`.

**The rules:**
- **Lower scenes.** `SceneManager::ui_pass` stops setting and restoring occlusion. A lower scene's widgets already answer nothing because `response()` checks the top scene (Task 6); the loop keeps only `begin_scene`/`end_scene`.
- **The console.**
  - **Pointer.** `draw_ui` drops `set_mouse_occluded()` and `consume_click()`. When the console is visible and the pointer is inside its window, the engine calls `snapshot.consume_mouse(MouseButton::Left)` before building the context, so gameplay never sees a click on the console. The console's own handling reads `Input` as it does now.
  - **Widgets under it.** The console's rect is already a `resolve_pointer` blocker, so no widget under it becomes hot.
  - **The close button's hover** becomes `close_rect.contains(mx, my)` from `ui.input().mouse_position()`: the console is a blocker, so its own parts are not hit-tested by the UI.
- **Module docs.** The `ui/mod.rs` header diagram and its "When to Use Each Mode" section are replaced by a short description:
  - widgets are builders answering from memory;
  - containers are laid out by Taffy;
  - focus lives in scopes;
  - absolute `draw_*` calls remain for HUDs and custom drawing;
  - the theme is a world resource.

- [ ] **Step 1: Delete, and read the build errors**

Delete the items listed above. Run: `cargo build --workspace --all-targets 2>&1 | grep -E "^error" -A5 | head -80`
Expected: errors only at the call sites named in this task's rules (`manager.rs`, `frame.rs`, `console/mod.rs`) and in the removed items' own tests. Any other site is a port an earlier task missed: port it here the way that task's rules say, and say so in the report.

- [ ] **Step 2: Write the tests that pin the replacements**

`src/core/engine/frame.rs` (add to the tests module):

```rust
  #[test]
  fn a_click_on_the_console_reaches_neither_widgets_nor_gameplay() {
    use crate::core::input::{Input, InputEvent};
    use crate::core::ui::{Rect, UiContext};
    use winit::event::MouseButton;

    #[derive(Resource, Default)]
    struct Seen {
      at: Rect,
      button: u32,
      gameplay: u32,
    }
    struct UnderConsole;
    impl Scene for UnderConsole {
      fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
        let at = ctx.world.resource::<Seen>().at;
        if ui.button("Under").at(at).clicked() {
          ctx.world.resource_mut::<Seen>().button += 1;
        }
        SceneAction::Continue
      }
      fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
        let pressed = ctx.input.mouse_pressed(MouseButton::Left);
        ctx.world.resource_mut::<Seen>().gameplay += u32::from(pressed);
        SceneAction::Continue
      }
    }
    fn click(e: &mut Engine, x: f32, y: f32) {
      let frames = [
        vec![InputEvent::MouseMove { x: f64::from(x), y: f64::from(y) }],
        vec![InputEvent::MouseButton { button: MouseButton::Left, pressed: true }],
        vec![InputEvent::MouseButton { button: MouseButton::Left, pressed: false }],
        vec![],
      ];
      for events in frames {
        for event in events {
          e.world_mut().resource_mut::<Input>().queue(event);
        }
        e.frame(Some(Duration::from_micros(16_667)));
      }
    }

    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let mut e = EngineBuilder::new()
      .resolution(320, 240)
      .fixed_hz(60)
      .insert_resource(Seen::default())
      .start_scene(UnderConsole)
      .build_headless();
    e.init_gpu(WgpuRenderer::new_offscreen(device, queue, 320, 240), 1.0);
    e.console_state.toggle();
    // Let the console finish opening, then put the button under its window
    for _ in 0..60 {
      e.frame(Some(Duration::from_micros(16_667)));
    }
    let window = e.console_state.window_rect();
    let at = Rect::new(window.x + 20.0, window.y + 40.0, 60.0, 20.0);
    e.world_mut().resource_mut::<Seen>().at = at;
    e.frame(Some(Duration::from_micros(16_667)));
    click(&mut e, at.x + 10.0, at.y + 10.0);
    let seen = e.world().resource::<Seen>();
    assert_eq!((seen.button, seen.gameplay), (0, 0), "the console took the click");
    // With the console closed, the same click is the button's
    e.console_state.toggle();
    for _ in 0..60 {
      e.frame(Some(Duration::from_micros(16_667)));
    }
    click(&mut e, at.x + 10.0, at.y + 10.0);
    assert_eq!(e.world().resource::<Seen>().button, 1);
  }
```

`tests/e2e/headless_engine.rs` (add):

```rust
#[test]
fn a_lower_scenes_button_ignores_the_pointer() {
  use std::sync::{Arc, Mutex};
  type Clicks = Arc<Mutex<u32>>;

  struct Lower {
    clicks: Clicks,
    pushed: bool,
  }
  impl Scene for Lower {
    fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
      if self.pushed {
        return SceneAction::Continue;
      }
      self.pushed = true;
      SceneAction::Push(Box::new(Upper))
    }
    fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
      if ui.button("Under").at(Rect::new(10.0, 10.0, 80.0, 30.0)).clicked()
        && let Ok(mut n) = self.clicks.lock()
      {
        *n += 1;
      }
      SceneAction::Continue
    }
  }
  /// Transparent, so the scene under it still draws; P pops it
  struct Upper;
  impl Scene for Upper {
    fn is_transparent(&self) -> bool {
      true
    }
    fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
      if ctx.input.key_pressed(KeyCode::KeyP) {
        SceneAction::Pop
      } else {
        SceneAction::Continue
      }
    }
  }
  fn click(engine: &mut HeadlessEngine) {
    engine.mouse_move(20.0, 20.0);
    engine.press_mouse(MouseButton::Left);
    engine.step().unwrap();
    engine.release_mouse(MouseButton::Left);
    engine.step_frames(2).unwrap();
  }

  let clicks = Clicks::default();
  let lower = Lower { clicks: clicks.clone(), pushed: false };
  let Some(mut engine) = HeadlessEngine::new_or_skip(EngineBuilder::new().resolution(200, 150).start_scene(lower)) else {
    return;
  };
  engine.step_frames(3).unwrap();
  click(&mut engine);
  click(&mut engine);
  assert_eq!(*clicks.lock().unwrap(), 0, "the scene above has the pointer");
  engine.tap_key(KeyCode::KeyP);
  engine.step_frames(3).unwrap();
  click(&mut engine);
  assert_eq!(*clicks.lock().unwrap(), 1, "on top again, the button works");
}
```

- [ ] **Step 3: Port the remaining sites and run**

Port `manager.rs`, `frame.rs` and `console/mod.rs` by the rules.

Run: `cargo test --workspace` and `cargo build --workspace --all-targets`
Expected: pass. Both new tests pass. They pin behaviour the old occlusion gave, so they would have passed before Step 1 too: they guard the replacements, not the deletions.

- [ ] **Step 4: Check that nothing old is left**

First rename `examples/demos/cursor/main.rs`'s local variable `is_hovered` to `hovered`, so the grep stays clean.

Run: `grep -rnE "HBox|VBox|HAlign|VAlign|is_clicked|is_hovered|is_held|mouse_occluded|consume_click|click_consumed|escape_consumed|consume_escape|draw_focus_ring|TextInputState|DropdownState|ScrollViewState|FocusTrap|Focusable|HotReloadTheme|button_styled|finish_dropdowns" --include='*.rs' src examples tests crates`
Expected: no matches.

Then run the same pattern over `--include='*.md' src .claude/skills docs/architecture examples`. Matches there are expected: Task 20 rewrites those docs. List them in the report, for Task 20, without editing them here.

- [ ] **Step 5: Docs**

- `src/core/ui/immediate/AGENTS.md`: drop `layout.rs`.
- `src/core/ui/AGENTS.md`: the Input bullet's consume list becomes `consume_key`, `consume_scroll`, widget responses, the wheel over a scroll area, typed text and the actions navigation used.

- [ ] **Step 6: Gate and commit**

Stage the files by name, including `git rm src/core/ui/immediate/layout.rs`.

```bash
git commit -m "refactor: remove the old UI layout helpers and pointer checks" -m "- HBox, VBox, Stack, rect hit tests, mouse occlusion and the click and escape consumption helpers are gone
- lower scenes and the console rely on the resolve step's top-scene and blocker rules"
```

- [ ] **Step 7: Write the failing overlay tests**

`src/core/console/builtins.rs` (add, in the style of its other tests):

```rust
  #[test]
  fn debug_ui_toggles_the_overlay() {
    let mut registry = CommandRegistry::new();
    register_builtins(&mut registry);
    let mut console = super::super::ConsoleState::new();
    let mut debug_config = DebugConfig::default();
    let mut time_scale = 1.0;
    let mut ctx = make_test_context(&mut console, &mut debug_config, &mut time_scale);
    assert_eq!(registry.execute("debug:ui", &mut ctx), CommandResult::Ok);
    assert!(ctx.debug_config.show_ui_debug);
    registry.execute("debug:ui", &mut ctx);
    assert!(!ctx.debug_config.show_ui_debug);
  }
```

`src/core/engine/frame.rs` (add):

```rust
  #[test]
  fn the_ui_debug_overlay_outlines_each_widget() {
    use crate::core::ui::immediate::{OVERLAY_LAYER, UiCommand};
    use crate::core::ui::{Rect, UiContext};
    const AT: Rect = Rect::new(20.0, 20.0, 80.0, 30.0);
    struct OneButton;
    impl Scene for OneButton {
      fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
        ui.button("Outlined").at(AT).show();
        SceneAction::Continue
      }
    }
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let mut e = EngineBuilder::new().resolution(320, 240).start_scene(OneButton).build_headless();
    e.init_gpu(WgpuRenderer::new_offscreen(device, queue, 320, 240), 1.0);
    let outlined = |e: &Engine| {
      e.ui_commands.iter().any(|(_, c)| {
        matches!(c, UiCommand::Rect { rect, layer, border: Some(_), .. } if *layer == OVERLAY_LAYER && *rect == AT)
      })
    };
    e.frame(Some(Duration::from_millis(16)));
    assert!(!outlined(&e), "off by default");
    e.config.debug.show_ui_debug = true;
    e.frame(Some(Duration::from_millis(16)));
    assert!(outlined(&e), "the button's rect is outlined on the overlay layer");
  }
```

`ui_commands.iter()` yields `(order, &UiCommand)` pairs since Task 5.

- [ ] **Step 8: Run them to see them fail**

Run: `cargo test -p ryo2d --lib console::builtins engine::frame::tests::the_ui_debug`
Expected: compile errors (no `show_ui_debug`).

- [ ] **Step 9: Implement and run**

Add the field, the command and the drawing by the rules above. List `debug:ui` in the `ryo-console` skill's command table, then copy the skill to its CLI twin and `cmp` the two.

Run: `cargo test -p ryo2d --lib` and `cargo test --workspace`
Expected: pass.

- [ ] **Step 10: Gate and commit**

```bash
git add src/core/config/debug.rs src/core/console/builtins.rs src/core/ui/debug.rs src/core/ui/mod.rs src/core/engine/frame.rs .claude/skills/ryo-console/SKILL.md src/cli/skills/ryo-console.md
git commit -m "feat: a debug:ui console overlay of widget rects and ids" -m "- outlines every widget the UI recorded this frame, with its id, marking hot, active and focused"
```

### Task 20: A public UI harness, benchmarks, docs and skills

**Files:**
- Create:
  - `src/core/testing/ui.rs`: `UiHarness`.
  - `benches/ui.rs`.
  - `tests/integration/ui_harness.rs`.
- Modify (engine and build):
  - `src/core/testing/mod.rs`: `pub use ui::UiHarness`.
  - `src/core/ui/test_util.rs`: `UiDriver` holds a `UiHarness`.
  - `Cargo.toml`: `[[bench]] name = "ui"`.
  - `tests/integration.rs`: the new module.
- Modify (skills; each `.claude/skills/<name>/SKILL.md` and its byte-identical `src/cli/skills/<name>.md`): `ryo-ui` (rewritten), `ryo-api`, `ryo-game-dev`, `ryo-controller`, `ryo-text`, `ryo-config-save`, `ryo-cli`, `ryo-engine-dev`.
- Modify (docs):
  - `docs/architecture/overview.md`;
  - `src/core/ui/AGENTS.md`, `src/core/AGENTS.md`, `src/core/input/AGENTS.md`, `src/core/testing/AGENTS.md`;
  - `src/core/ui/mod.rs` (module docs);
  - `docs/backlog/` (a 4c note).
- Test: `tests/integration/ui_harness.rs`; the skill copies' existing byte-identity test in `src/cli`

**Interfaces:**
- Consumes: everything from Tasks 1-19.
- Produces:

```rust
/// A UI with no window or GPU: memory, layout, text and the resolve step over scripted
/// input, one call per frame. For unit-testing game UI and for benchmarks.
pub struct UiHarness;
impl UiHarness {
  pub fn new(width: f32, height: f32) -> UiHarness;            // the built-in fonts, scale 1, the dark theme
  pub fn with_theme(self, theme: Theme) -> UiHarness;
  /// One frame at 60 fps: resolve the scripted input, run `ui` as the top scene, finish the frame
  pub fn frame<R>(&mut self, ui: impl FnOnce(&mut UiContext<'_>) -> R) -> R;
  pub fn move_to(&mut self, x: f32, y: f32);
  pub fn press(&mut self);                                      // the left button, next frame
  pub fn release(&mut self);
  /// Press a UI action next frame: "ui.up", "ui.down", "ui.left", "ui.right", "ui.confirm",
  /// "ui.back", "ui.next", "ui.tab_next", "ui.tab_prev"; another name warns once and does nothing
  pub fn action(&mut self, name: &str);
  pub fn wheel(&mut self, dx: f32, dy: f32);                    // lines, next frame
  pub fn focused(&self) -> Option<UiId>;
  /// The UI commands the last frame recorded
  pub fn command_count(&self) -> usize;
}
```

**The rules:**
- **`UiHarness` is `UiDriver`'s machinery made public.** Move the fields and `frame` from `UiDriver` into `UiHarness`. `UiDriver` (crate, test-only) then holds a `UiHarness`. It keeps every helper it has, delegating those `UiHarness` also has (`move_to`, `press`, `release`, `wheel`) and keeping the crate-only ones on top: `commands`, `memory`, `nav`, `hold`, `release_nav`, `confirm`, `back`, `tab`, `tab_next`, `key`, `type_text`, `shift`, `pad`, `stick`, `device`, `prompts`, `pending_rebinds`, `last_outcome`, `last_sounds`, `set_scene` and `with_theme`. Every existing `UiDriver` test passes unchanged.
- **Benchmarks** (`benches/ui.rs`, criterion, `harness = false`, on `UiHarness`, no GPU):
  1. **"ui frame, a 200-widget settings screen".** One `ui.panel().focus_scope()` holding `ui.tabs` and a `ui.scroll()` of 50 rows. Each row is `ui.row()` of a label and one of a slider, a checkbox, a dropdown or a toggle (cycling), plus a value text: 200 widgets. Each iteration is one `frame`, with a `ui.down` every 10th iteration so navigation is in the cost.
  2. **"ui frame, a 1,000-row scroll list".** A `ui.scroll().height(400.0)` of 1,000 buttons, wheel-scrolled to the middle during warm-up.
  3. **"ui frame, an empty frame".** The baseline.
- **The budget.** Run `cargo bench --bench ui`. Benchmark 1 must be under 0.25 ms. If it is over, profile it (`cargo flamegraph --bench ui` or puffin scopes) and fix it in this task. The likely costs are:
  - Taffy re-dirtying, when styles are not diffed;
  - text measuring that misses the layout cache;
  - allocations per node in the subtree;
  - record hashing.

  Report the numbers before and after. Then run `cargo bench --bench text`: the HUD frame stays within 5% of about 290 µs.
- **Skills.** Every changed skill is copied, byte for byte, to its CLI copy and checked with `cmp`.
  - **`ryo-ui`, rewritten.** It is the agent's guide to building game UI with this engine, with these sections:
    1. **The `ui` hook** and the top-scene rule.
    2. **UI units.**
    3. **Widgets are builders:** ids by call site, `.id(key)` in loops, recording on drop, and `Response`.
    4. **Layout:** containers, `Length`, `anchor`/`offset`/`at`, grid and `tracks`, `wrap_items`, `stack`, `spacer`/`separator`.
    5. **The widget catalogue,** each with a one-line example: button, text, image, checkbox, toggle, radio, slider, progress, text input, dropdown, tabs, choice list, slot, prompt, rebind, custom with `Painter`, tooltips, `enabled`.
    6. **Scroll areas.**
    7. **Focus and navigation:**
       - explicit scopes, so gameplay keeps the arrows without one;
       - `default_focus`, `wrap_focus`, `trap`, the `nav_*` overrides;
       - `ui.back()`, `focus_visible`;
       - the Q/E and shoulder tab keys, taken only by drawn tabs.
    8. **Popups, modals and `ui.confirm`.**
    9. **Drag and drop.**
    10. **Theme:** `ctx.ui_theme()`, looks, variants, skins, RON, hot reload, sounds.
    11. **Input and consumption:** `wants_pointer`, `wants_keyboard`, used actions.
    12. **Absolute drawing and clips.**
    13. **UI tweens.**
    14. **Recipes,** each complete and compilable: main menu, pause menu with a settings modal, tabbed settings screen with rebinding, inventory grid with drag and drop and equipment, dialogue with choices and a leave confirmation, HUD with no scope, quit confirmation.
    15. **Testing UI:** `UiHarness`, `HeadlessEngine::{ui_focused, ui_focusables, focus_ui, tap_pad}`, `RYO_INPUT` pad scripts.
    16. **Tips.**

    Take the recipes from the `ui` demo's acceptance pages and the ported games, and check each compiles. The quickest way: paste each recipe into a scratch `examples/` file, `cargo build` it, then delete the file.
  - **`ryo-api`.** Replace the UI section with the complete signatures from Tasks 5-15 and 17. Add actions, `InputDevice`, the virtual pad events and `PadFamily` to Input. Add `UserPreference::Bindings` to Config, `[prompt=..]` to Text, and `UiHarness` plus the headless focus methods to Testing.
  - **`ryo-game-dev`.** The Input section: actions over raw keys (`ctx.actions().bind(..)`, `ctx.input.action_pressed(..)`), and menus through `ryo-ui`'s recipes.
  - **`ryo-controller`.** Actions and stick directions, `PadFamily` and prompts, `last_device`, and headless pad input.
  - **`ryo-text`.** `[prompt=action]` and `RichText::prompt`.
  - **`ryo-config-save`.** Saved bindings.
  - **`ryo-cli`.** The `RYO_INPUT` `Wheel@`, `Pad*` and `LeftStick@`/`RightStick@` entries.
  - **`ryo-engine-dev`.** The module table's `ui/` and `input/` rows.
- **Docs.**
  - **`docs/architecture/overview.md`.** Rewrite the UI section: the frame (resolve → hooks → layout per container → commands), memory, focus scopes, the theme, actions.
  - **`src/core/ui/AGENTS.md`.** One consistent pass over the sections added by Tasks 5-19, with no immediate/retained split left.
  - **`src/core/testing/AGENTS.md`.** `UiHarness`.
  - **`docs/backlog/ui-retained-nodes.md`.** 4c: retained nodes over the same memory and layout, for UIs authored as data, with what 4b leaves for it.

- [ ] **Step 1: Write the failing test**

`tests/integration/ui_harness.rs`:

```rust
use ryo2d::prelude::*;
use ryo2d::testing::UiHarness;

fn menu(ui: &mut UiContext<'_>, picked: &mut Option<usize>) {
  ui.column().focus_scope().show(|ui| {
    for (i, label) in ["Play", "Options", "Quit"].into_iter().enumerate() {
      let button = ui.button(label).id(i);
      let button = if i == 0 { button.default_focus() } else { button };
      if button.clicked() {
        *picked = Some(i);
      }
    }
  });
}

#[test]
fn a_menu_is_driven_without_a_gpu() {
  let mut ui = UiHarness::new(320.0, 240.0);
  let mut picked = None;
  ui.frame(|u| menu(u, &mut picked));
  ui.frame(|u| menu(u, &mut picked));
  assert!(ui.focused().is_some(), "the menu took focus");
  ui.action("ui.down");
  ui.frame(|u| menu(u, &mut picked));
  ui.action("ui.confirm");
  ui.frame(|u| menu(u, &mut picked));
  assert_eq!(picked, Some(1));
  assert!(ui.command_count() > 0);
}

#[test]
fn an_unknown_action_does_nothing() {
  let mut ui = UiHarness::new(320.0, 240.0);
  let mut picked = None;
  ui.frame(|u| menu(u, &mut picked));
  ui.action("jump");
  ui.frame(|u| menu(u, &mut picked));
  assert_eq!(picked, None);
}
```

Register it in `tests/integration.rs` (`#[path = "integration/ui_harness.rs"] mod ui_harness;`).

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test --test integration ui_harness`
Expected: compile error: no `ryo2d::testing::UiHarness`.

- [ ] **Step 3: Implement the harness and move `UiDriver` onto it**

Run: `cargo test --test integration ui_harness` and `cargo test -p ryo2d --lib ui::`
Expected: pass. Every `UiDriver` test is unchanged.

- [ ] **Step 4: The benchmarks**

Write `benches/ui.rs` by the rules, in `benches/text.rs`'s style, and add the `[[bench]]` entry.

Run: `cargo bench --bench ui` and `cargo bench --bench text`
Expected: the settings screen under 0.25 ms, and the HUD within 5% of its old number. Put the three ui numbers and the HUD number in the report.

Gate, then commit the harness and the benchmarks:

```bash
git add src/core/testing/ui.rs src/core/testing/mod.rs src/core/ui/test_util.rs tests/integration/ui_harness.rs tests/integration.rs benches/ui.rs Cargo.toml
git commit -m "feat: a public UI harness and UI benchmarks" -m "- UiHarness runs UI frames over scripted input without a window or GPU, for game UI tests
- benchmarks for a 200-widget settings screen, a 1,000-row list and an empty frame"
```

Include any performance fixes in this commit, or in their own `fix:` commits before it (the commit types are feat, fix, refactor, docs, test and chore).

- [ ] **Step 5: Skills and docs**

Write them by the rules. For each changed skill, run `cp .claude/skills/<name>/SKILL.md src/cli/skills/<name>.md && cmp .claude/skills/<name>/SKILL.md src/cli/skills/<name>.md`.

Run: `grep -rnE "HBox|VBox|is_clicked|is_hovered|TextInputState|DropdownState|ScrollViewState|FocusTrap|HotReloadTheme|button_styled|set_mouse_occluded|scroll_view\(" --include='*.md' src .claude/skills docs/architecture examples`
Expected: no matches.

- [ ] **Step 6: The final gate**

Run the full gate from Global Constraints, then `./dev.sh demos:tour` and look at the tour's PNGs for the UI-heavy demos (ui, ui-animations, text, asset-loading, networking, controller). The ported games build and their headless tests pass.

- [ ] **Step 7: Commit**

Stage the files by name.

```bash
git commit -m "docs: UI interaction skills and architecture" -m "- ryo-ui rewritten around builders, layout, focus scopes, popups, drag and drop and the theme, with recipes
- actions, prompts, saved bindings, pad scripting and the UI harness in the API, controller, text, config and CLI skills
- the architecture overview and module notes describe the new UI frame"
```
