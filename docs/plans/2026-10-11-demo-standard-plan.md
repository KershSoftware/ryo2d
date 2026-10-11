# Demo Standard Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give every feature demo the same controls, layout and settings panel through a shared `demo-kit` crate, prove it on three pilot demos, and add a headless tour that captures every page.

**Architecture:** `examples/demo-kit` is a workspace library crate. A demo registers page factories with `Demo`; each page runs inside a `PageScene` (an engine `Scene`), and changing page or resetting is a `SceneAction::Switch` to a fresh `PageScene`, so the engine's scene cleanup isolates pages. The shell handles its keys in `update`, applies an immediate-mode `Settings` panel, then calls the page; it draws its chrome after the page's UI. The engine gains `UiContext::wants_keyboard()` so the shell steps aside while a widget is typed into. A `demo-tour` binary in the kit runs each page headlessly (`RYO_HEADLESS`, `RYO_DEMO_PAGE`, `RYO_CAPTURE_*`).

**Tech Stack:** Rust 2024, ryo2d (wgpu 22, winit `KeyCode`), `ryo2d::testing::HeadlessEngine`, serde_json (tour), tempfile (tour tests).

**Spec:** `docs/plans/2026-10-11-demo-standard-design.md`

## Global Constraints

- 2-space indentation, edition 2024. Format only files you create, with `rustfmt --edition 2024 <file>`; in existing files keep your hunks formatted but do not reformat unrelated code (the repo has formatting drift).
- No `unwrap`/`expect` outside tests, in the kit and the tour alike.
- Line endings: run `file <path>` before editing. Many `.rs` files and all skill files are CRLF; edit them byte-preservingly (Python in binary mode, or check `git diff` shows only your lines). New files are LF.
- Skill files exist twice and must stay byte-identical: `src/cli/skills/<name>.md` and `.claude/skills/<name>/SKILL.md`.
- Commit on `main`, format `type: brief description` plus bullets. No `Co-Authored-By`, `Claude-Session` or any other trailer lines. No test-status lines.
- Shell text is ASCII only (the bitmap font covers characters 32-126): no arrows, no middle dots.
- Reserved shell keys, verbatim from the spec: `[`, `]`, PageUp, PageDown, ArrowUp, ArrowDown, ArrowLeft, ArrowRight, `H`, `R`, `Esc`, F1-F5, F9, F10, F12, backquote.
- Hold-to-repeat: 0.35 s delay, then 12 steps per second, timed with `ctx.time.fixed_dt` (unscaled).
- `RYO_DEMO_PAGE` is 1-based, read in dev builds only (`cfg!(debug_assertions)`); unparseable values warn and are ignored; out-of-range values print `[demo] RYO_DEMO_PAGE=<n>, but <title> has <m> pages` (or `1 page`) and exit with code 2 before the engine is built.
- Startup log line, dev and release: `[demo] <title>: <m> pages` (`1 page` when m = 1).
- Window title: `Ryo2D - <demo title>`.
- Tour: capture frame 90, 60 s timeout per page, output `target/demo-tour/<demo>/page-<n>.png`.

## Review Focus

1. A tiny window (160x120) with many settings rows and a long legend must render without panicking, with the panel and legend kept on screen. Test: Task 4 `tiny_screen_with_many_rows_and_a_long_legend_renders`.
2. When a page's rows shrink under the selection (a choice that hides rows), the selection clamps and Left/Right changes the right row. Test: Task 3 `selection_clamps_when_rows_disappear`.
3. An arrow key still held after `R` must not keep adjusting the rebuilt page. Tests: Task 2 `down_without_a_press_never_repeats`, Task 3 `holding_right_through_a_reset_does_not_keep_adjusting`.
4. `Esc` while a dropdown is open must close the dropdown, not quit the demo. Test: Task 3 `escape_with_an_open_dropdown_does_not_quit`.
5. Malformed `RYO_DEMO_PAGE` values (`"0"`, `"-1"`, `" 3 "`, `"abc"`, a number past the end) behave as specified. Test: Task 2 `start_page_handles_every_input`.

---

### Task 1: `UiContext::wants_keyboard`

**Files:**
- Modify: `src/core/ui/immediate/context.rs` (struct field, four constructors, `text_input`, `dropdown`, new method, tests)
- Modify: `src/cli/skills/ryo-ui.md`, `.claude/skills/ryo-ui/SKILL.md`, `src/cli/skills/ryo-api.md`, `.claude/skills/ryo-api/SKILL.md`

**Interfaces:**
- Produces: `pub fn wants_keyboard(&self) -> bool` on `UiContext`.

Ruling recorded from planning: the spec says a dropdown counts when `focused_dropdown() != 0`, but `focused_dropdown_id` is only set on the frame a dropdown is clicked; focus lives in each `DropdownState`. So `dropdown()` records keyboard focus from `state.is_focused() || state.is_open()`, which is the spec's intent ("true while a dropdown has keyboard focus").

- [ ] **Step 1: Write the failing tests** in the `tests` module at the end of `context.rs` (it already has `create_test_context(width, height, frame)`):

```rust
  #[test]
  fn wants_keyboard_is_false_without_widgets() {
    let mut frame = vec![0u8; 200 * 100 * 4];
    let ui = create_test_context(200, 100, &mut frame);
    assert!(!ui.wants_keyboard());
  }

  #[test]
  fn wants_keyboard_while_a_focused_text_input_is_drawn() {
    let mut frame = vec![0u8; 200 * 100 * 4];
    let mut ui = create_test_context(200, 100, &mut frame);
    let mut unfocused = TextInputState::new();
    ui.text_input(&mut unfocused, Rect::new(0.0, 0.0, 100.0, 20.0), "", 0.0);
    assert!(!ui.wants_keyboard());

    let mut focused = TextInputState::new();
    focused.set_focused(true);
    ui.text_input(&mut focused, Rect::new(0.0, 30.0, 100.0, 20.0), "", 0.0);
    assert!(ui.wants_keyboard());
  }

  #[test]
  fn wants_keyboard_while_a_dropdown_is_focused_or_open() {
    let mut frame = vec![0u8; 200 * 200 * 4];
    let mut ui = create_test_context(200, 200, &mut frame);
    let mut closed = DropdownState::with_id(1);
    ui.dropdown(&mut closed, Rect::new(0.0, 0.0, 100.0, 20.0), &["a", "b"]);
    assert!(!ui.wants_keyboard());

    let mut open = DropdownState::with_id(2);
    open.open();
    ui.dropdown(&mut open, Rect::new(0.0, 40.0, 100.0, 20.0), &["a", "b"]);
    assert!(ui.wants_keyboard());
  }

  #[test]
  fn wants_keyboard_resets_with_each_frame() {
    let mut frame = vec![0u8; 200 * 100 * 4];
    let mut focused = TextInputState::new();
    focused.set_focused(true);
    {
      let mut ui = create_test_context(200, 100, &mut frame);
      ui.text_input(&mut focused, Rect::new(0.0, 0.0, 100.0, 20.0), "", 0.0);
      assert!(ui.wants_keyboard());
    }
    // Focus lost before the next frame: the next context starts clear
    focused.set_focused(false);
    let mut ui = create_test_context(200, 100, &mut frame);
    ui.text_input(&mut focused, Rect::new(0.0, 0.0, 100.0, 20.0), "", 0.0);
    assert!(!ui.wants_keyboard());
  }
```

If `TextInputState`/`DropdownState` are not already in scope in the test module, import them from `crate::core::ui` (they are re-exported there; `grep -n "pub use" src/core/ui/mod.rs`).

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib wants_keyboard`
Expected: compile error, `no method named wants_keyboard`.

- [ ] **Step 3: Implement**

1. Add a field after `focused_dropdown_id` in `pub struct UiContext`:

```rust
  /// Whether a text input or dropdown drawn this frame has keyboard focus
  keyboard_focus: bool,
```

2. Initialise it to `false` in all four constructors (`new`, `new_scaled`, `with_theme`, `with_theme_scaled`; each lists `focused_dropdown_id: 0,`; add `keyboard_focus: false,` after it).

3. Add the method next to `focused_dropdown`:

```rust
  /// Whether a widget drawn this frame has keyboard focus: a focused text
  /// input, or a focused or open dropdown.
  ///
  /// Scenes with their own key bindings can check it after drawing their UI
  /// and ignore those keys on the next update, so typing into a field never
  /// triggers them.
  pub fn wants_keyboard(&self) -> bool {
    self.keyboard_focus
  }
```

4. In `text_input`, directly after the block that sets or clears focus on click (the `if clicked { ... } else if state.is_focused() && ... { ... }` chain, including the "Click outside always unfocuses" branch), add:

```rust
    if state.is_focused() {
      self.keyboard_focus = true;
    }
```

5. In `dropdown`, directly after the "Sync focus: if another dropdown was clicked this frame, unfocus this one" block, add:

```rust
    if state.is_focused() || state.is_open() {
      self.keyboard_focus = true;
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib ui::immediate`
Expected: all pass, including the four new tests.

- [ ] **Step 5: Document it**

In both copies of `ryo-ui` (CRLF), add next to the `text_input` documentation:

```markdown
`ui.wants_keyboard()` is true when a widget drawn this frame has keyboard focus (a focused text input, or a focused or open dropdown). Check it after drawing and ignore your own key bindings on the next update so typing never triggers them.
```

In both copies of `ryo-api` (CRLF), add to the `UiContext` method listing, next to `text_input`:

```rust
    fn wants_keyboard(&self) -> bool;   // a focused text input or a focused/open dropdown was drawn this frame
```

Check: `cmp src/cli/skills/ryo-ui.md .claude/skills/ryo-ui/SKILL.md && cmp src/cli/skills/ryo-api.md .claude/skills/ryo-api/SKILL.md`.

- [ ] **Step 6: Commit**

```bash
git add src/core/ui/immediate/context.rs src/cli/skills/ryo-ui.md .claude/skills/ryo-ui/SKILL.md src/cli/skills/ryo-api.md .claude/skills/ryo-api/SKILL.md
git commit -m "feat(ui): report when a widget wants the keyboard" -m "- UiContext::wants_keyboard is true while a focused text input or a focused or open dropdown is drawn
- Lets scenes ignore their own key bindings while the player types"
```

---

### Task 2: demo-kit crate and its pure building blocks

**Files:**
- Modify: `Cargo.toml` (workspace members)
- Create: `examples/demo-kit/Cargo.toml`
- Create: `examples/demo-kit/src/lib.rs`
- Create: `examples/demo-kit/src/settings.rs`
- Create: `examples/demo-kit/src/hold.rs`
- Create: `examples/demo-kit/src/controls.rs`
- Create: `examples/demo-kit/src/start.rs`

**Interfaces:**
- Produces (crate-public unless noted):
  - `pub struct Settings` with `pub fn float(&mut self, label: &str, value: &mut f32, range: RangeInclusive<f32>, step: f32)`, `pub fn int(&mut self, label: &str, value: &mut i32, range: RangeInclusive<i32>)`, `pub fn choice(&mut self, label: &str, index: &mut usize, options: &[&str])`, `pub fn toggle(&mut self, label: &str, value: &mut bool)`; `pub(crate) fn new(selected: usize, adjust: i32) -> Self`; `pub(crate) fn into_rows(self) -> Vec<Row>`.
  - `pub(crate) struct Row { pub label: String, pub value: String, pub range: String }`.
  - `pub(crate) struct HoldRepeat` with `pub(crate) fn steps(&mut self, pressed: bool, down: bool, dt: f32) -> u32`; `Default`.
  - `pub struct Control` with `pub fn key(key: KeyCode, description: impl Into<String>) -> Self`, `pub fn keys(label: impl Into<String>, keys: &[KeyCode], description: impl Into<String>) -> Self`, `pub fn mouse(description: impl Into<String>) -> Self`; crate-visible fields `label: String`, `keys: Vec<KeyCode>`, `description: String`.
  - `pub(crate) const RESERVED: &[KeyCode]`, `pub(crate) fn reserved_key(controls: &[Control]) -> Option<KeyCode>`, `pub(crate) fn key_label(key: KeyCode) -> String`.
  - `pub(crate) enum StartPageError { Invalid(String), OutOfRange(String) }`, `pub(crate) fn start_page(raw: Option<&str>, title: &str, pages: usize) -> Result<Option<usize>, StartPageError>`, `pub(crate) fn pages_label(pages: usize) -> String`.

- [ ] **Step 1: Create the crate and register it**

Add `"examples/demo-kit",` to `[workspace] members` in the root `Cargo.toml` (after `"ryo2d-derive",`).

`examples/demo-kit/Cargo.toml`:

```toml
[package]
name = "demo-kit"
version = "0.1.0"
edition = "2024"
description = "Shared shell for Ryo2D feature demos: pages, settings panel, help and layout"
publish = false

[dependencies]
ryo2d = { path = "../.." }
```

`examples/demo-kit/src/lib.rs`:

```rust
//! Shared shell for the Ryo2D feature demos.
//!
//! Every demo gets the same controls (`[ ]` pages, arrows for settings, `R`
//! reset, `H` help, `Esc` quit), the same layout and a settings panel. See
//! `examples/demos/AGENTS.md`.

mod controls;
mod hold;
mod settings;
mod start;

pub use controls::Control;
pub use settings::Settings;
```

- [ ] **Step 2: Write the failing tests** (each file's `#[cfg(test)] mod tests`, below its code; write the code files with only their type and function signatures first, bodies `todo!()`-free: return defaults such as `0`, `None`, `Ok(None)` so the tests compile and fail)

`settings.rs` tests:

```rust
#[cfg(test)]
mod tests {
  use super::*;

  fn values(settings: Settings) -> Vec<String> {
    settings.into_rows().into_iter().map(|row| row.value).collect()
  }

  #[test]
  fn float_steps_snaps_and_clamps() {
    let mut level = 0.5;
    let mut s = Settings::new(0, 1);
    s.float("Level", &mut level, 0.0..=1.0, 0.1);
    assert!((level - 0.6).abs() < 1e-6);
    assert_eq!(values(s), ["0.6"]);

    let mut s = Settings::new(0, 10);
    s.float("Level", &mut level, 0.0..=1.0, 0.1);
    assert_eq!(level, 1.0);
  }

  #[test]
  fn unselected_rows_are_clamped_but_not_changed() {
    let mut level = 3.0;
    let mut count = 2;
    let mut s = Settings::new(1, 1);
    s.float("Level", &mut level, 0.0..=1.0, 0.1);
    s.int("Count", &mut count, 1..=5);
    assert_eq!(level, 1.0);
    assert_eq!(count, 3);
  }

  #[test]
  fn int_clamps_to_its_range() {
    let mut count = 4;
    let mut s = Settings::new(0, 5);
    s.int("Count", &mut count, 1..=5);
    assert_eq!(count, 5);
    let mut s = Settings::new(0, -9);
    s.int("Count", &mut count, 1..=5);
    assert_eq!(count, 1);
  }

  #[test]
  fn choice_wraps_both_ways() {
    let mut mode = 0;
    let mut s = Settings::new(0, -1);
    s.choice("Mode", &mut mode, &["A", "B", "C"]);
    assert_eq!(mode, 2);
    let mut s = Settings::new(0, 2);
    s.choice("Mode", &mut mode, &["A", "B", "C"]);
    assert_eq!(mode, 1);
    assert_eq!(values(s), ["B"]);
  }

  #[test]
  fn choice_without_options_is_shown_as_a_dash() {
    let mut mode = 3;
    let mut s = Settings::new(0, 1);
    s.choice("Mode", &mut mode, &[]);
    assert_eq!(values(s), ["-"]);
  }

  #[test]
  fn toggle_flips_once_per_step() {
    let mut on = false;
    let mut s = Settings::new(0, 1);
    s.toggle("On", &mut on);
    assert!(on);
    let mut s = Settings::new(0, 2);
    s.toggle("On", &mut on);
    assert!(on);
    assert_eq!(values(s), ["On"]);
  }

  #[test]
  fn a_selection_past_the_rows_changes_nothing() {
    let mut level = 0.5;
    let mut s = Settings::new(4, 1);
    s.float("Level", &mut level, 0.0..=1.0, 0.1);
    assert_eq!(level, 0.5);
  }

  #[test]
  fn rows_record_labels_and_ranges() {
    let mut level = 0.25;
    let mut s = Settings::new(0, 0);
    s.float("Level", &mut level, 0.0..=1.0, 0.05);
    let rows = s.into_rows();
    assert_eq!(rows[0].label, "Level");
    assert_eq!(rows[0].value, "0.25");
    assert_eq!(rows[0].range, "0.00 - 1.00");
  }
}
```

`hold.rs` tests:

```rust
#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_press_steps_once_then_repeats_after_the_delay() {
    let mut hold = HoldRepeat::default();
    assert_eq!(hold.steps(true, true, 0.0), 1);
    assert_eq!(hold.steps(false, true, 0.3), 0); // held 0.3 s, still inside the delay
    assert_eq!(hold.steps(false, true, 0.2), 2); // held 0.5 s: 0.15 s past the delay
    assert_eq!(hold.steps(false, true, 0.25), 3); // held 0.75 s
  }

  #[test]
  fn releasing_stops_repeats() {
    let mut hold = HoldRepeat::default();
    hold.steps(true, true, 0.0);
    assert_eq!(hold.steps(false, false, 1.0), 0);
  }

  #[test]
  fn down_without_a_press_never_repeats() {
    let mut hold = HoldRepeat::default();
    for _ in 0..100 {
      assert_eq!(hold.steps(false, true, 0.1), 0);
    }
  }
}
```

`controls.rs` tests:

```rust
#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn key_labels_are_short() {
    assert_eq!(key_label(KeyCode::KeyQ), "Q");
    assert_eq!(key_label(KeyCode::Digit3), "3");
    assert_eq!(key_label(KeyCode::Space), "Space");
    assert_eq!(key_label(KeyCode::Enter), "Enter");
  }

  #[test]
  fn reserved_keys_are_found_in_any_control() {
    let fine = [Control::key(KeyCode::Space, "Burst"), Control::mouse("Aim")];
    assert_eq!(reserved_key(&fine), None);
    let clash = [
      Control::key(KeyCode::Space, "Burst"),
      Control::keys("WASD + Up", &[KeyCode::KeyW, KeyCode::ArrowUp], "Move"),
    ];
    assert_eq!(reserved_key(&clash), Some(KeyCode::ArrowUp));
    assert_eq!(reserved_key(&[Control::key(KeyCode::KeyH, "Hop")]), Some(KeyCode::KeyH));
  }
}
```

`start.rs` tests:

```rust
#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn start_page_handles_every_input() {
    assert_eq!(start_page(None, "Bloom", 3), Ok(None));
    assert_eq!(start_page(Some("1"), "Bloom", 3), Ok(Some(0)));
    assert_eq!(start_page(Some(" 3 "), "Bloom", 3), Ok(Some(2)));
    assert!(matches!(start_page(Some("abc"), "Bloom", 3), Err(StartPageError::Invalid(_))));
    assert!(matches!(start_page(Some("-1"), "Bloom", 3), Err(StartPageError::Invalid(_))));
    assert_eq!(
      start_page(Some("4"), "Bloom", 3),
      Err(StartPageError::OutOfRange(
        "[demo] RYO_DEMO_PAGE=4, but Bloom has 3 pages".to_string()
      ))
    );
    assert_eq!(
      start_page(Some("0"), "Fog", 1),
      Err(StartPageError::OutOfRange(
        "[demo] RYO_DEMO_PAGE=0, but Fog has 1 page".to_string()
      ))
    );
  }

  #[test]
  fn pages_are_counted_in_words() {
    assert_eq!(pages_label(1), "1 page");
    assert_eq!(pages_label(6), "6 pages");
  }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p demo-kit`
Expected: the new tests fail on their assertions (stub bodies).

- [ ] **Step 4: Implement**

`settings.rs`:

```rust
//! The settings panel's immediate-mode API

use std::ops::RangeInclusive;

/// One row of the settings panel, as drawn
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Row {
  pub label: String,
  pub value: String,
  /// The range or the options, for the help overlay
  pub range: String,
}

/// Tweakable values for the current page, declared every frame in
/// [`DemoPage::settings`](crate::DemoPage::settings).
///
/// Up/Down select a row and Left/Right change it. Every value is kept within
/// its range, selected or not.
pub struct Settings {
  selected: usize,
  adjust: i32,
  rows: Vec<Row>,
}

impl Settings {
  pub(crate) fn new(selected: usize, adjust: i32) -> Self {
    Self {
      selected,
      adjust,
      rows: Vec::new(),
    }
  }

  pub(crate) fn into_rows(self) -> Vec<Row> {
    self.rows
  }

  /// Steps for the row being declared now
  fn steps(&self) -> i32 {
    if self.rows.len() == self.selected {
      self.adjust
    } else {
      0
    }
  }

  fn push(&mut self, label: &str, value: String, range: String) {
    self.rows.push(Row {
      label: label.to_string(),
      value,
      range,
    });
  }

  /// A number changed by `step` per press and kept within `range`
  pub fn float(&mut self, label: &str, value: &mut f32, range: RangeInclusive<f32>, step: f32) {
    let (lo, hi) = (*range.start(), *range.end());
    let steps = self.steps();
    if steps != 0 {
      let raw = *value + steps as f32 * step;
      *value = if step > 0.0 { (raw / step).round() * step } else { raw };
    }
    // max/min rather than clamp: never panics on an inverted range or NaN
    *value = value.max(lo).min(hi);
    let places = decimals(step);
    self.push(
      label,
      format!("{:.*}", places, *value),
      format!("{:.*} - {:.*}", places, lo, places, hi),
    );
  }

  /// A whole number changed by one per press and kept within `range`
  pub fn int(&mut self, label: &str, value: &mut i32, range: RangeInclusive<i32>) {
    let (lo, hi) = (*range.start(), *range.end());
    *value = value.saturating_add(self.steps()).max(lo).min(hi);
    self.push(label, value.to_string(), format!("{lo} - {hi}"));
  }

  /// One of `options`, cycling past either end
  pub fn choice(&mut self, label: &str, index: &mut usize, options: &[&str]) {
    if options.is_empty() {
      self.push(label, "-".to_string(), String::new());
      return;
    }
    let count = options.len() as i64;
    let current = (*index).min(options.len() - 1) as i64;
    *index = (current + i64::from(self.steps())).rem_euclid(count) as usize;
    self.push(label, options[*index].to_string(), options.join(", "));
  }

  /// On or off; each press flips it
  pub fn toggle(&mut self, label: &str, value: &mut bool) {
    if self.steps() % 2 != 0 {
      *value = !*value;
    }
    let text = if *value { "On" } else { "Off" };
    self.push(label, text.to_string(), "On, Off".to_string());
  }
}

/// Decimal places that show a value changed by `step`
fn decimals(step: f32) -> usize {
  if step >= 1.0 {
    0
  } else if step >= 0.1 {
    1
  } else if step >= 0.01 {
    2
  } else {
    3
  }
}
```

`hold.rs`:

```rust
//! Hold-to-repeat for the settings panel's Left/Right keys

/// Seconds a key is held before it starts repeating
pub(crate) const REPEAT_DELAY: f32 = 0.35;
/// Repeats per second once repeating
pub(crate) const REPEAT_RATE: f32 = 12.0;

/// Turns a key's press and held state into adjustment steps.
///
/// A key only repeats after this tracker has seen it pressed, so a key still
/// held when a page is rebuilt does nothing until pressed again.
#[derive(Debug, Default)]
pub(crate) struct HoldRepeat {
  armed: bool,
  held: f32,
  repeats: u32,
}

impl HoldRepeat {
  /// Steps to apply this update, given whether the key was just pressed,
  /// whether it is down, and the update's unscaled time step.
  pub(crate) fn steps(&mut self, pressed: bool, down: bool, dt: f32) -> u32 {
    if pressed {
      self.armed = true;
      self.held = 0.0;
      self.repeats = 0;
      return 1;
    }
    if !down || !self.armed {
      self.armed = false;
      self.held = 0.0;
      return 0;
    }
    self.held += dt;
    if self.held < REPEAT_DELAY {
      return 0;
    }
    let due = ((self.held - REPEAT_DELAY) * REPEAT_RATE) as u32 + 1;
    let new = due.saturating_sub(self.repeats);
    self.repeats = due;
    new
  }
}
```

`controls.rs`:

```rust
//! Page controls shown in the legend and the help overlay

use ryo2d::prelude::KeyCode;

/// A page's own control, listed in the legend and the help overlay.
#[derive(Debug, Clone, PartialEq)]
pub struct Control {
  pub(crate) label: String,
  pub(crate) keys: Vec<KeyCode>,
  pub(crate) description: String,
}

impl Control {
  /// A single key, labelled from its name (`Space`, `Q`, `1`)
  pub fn key(key: KeyCode, description: impl Into<String>) -> Self {
    Self {
      label: key_label(key),
      keys: vec![key],
      description: description.into(),
    }
  }

  /// Several keys under one label, e.g. `Control::keys("WASD", &[...], "Move")`
  pub fn keys(label: impl Into<String>, keys: &[KeyCode], description: impl Into<String>) -> Self {
    Self {
      label: label.into(),
      keys: keys.to_vec(),
      description: description.into(),
    }
  }

  /// A mouse control
  pub fn mouse(description: impl Into<String>) -> Self {
    Self {
      label: "Mouse".to_string(),
      keys: Vec::new(),
      description: description.into(),
    }
  }
}

/// Keys the shell or the engine owns; pages may not list them
pub(crate) const RESERVED: &[KeyCode] = &[
  KeyCode::BracketLeft,
  KeyCode::BracketRight,
  KeyCode::PageUp,
  KeyCode::PageDown,
  KeyCode::ArrowUp,
  KeyCode::ArrowDown,
  KeyCode::ArrowLeft,
  KeyCode::ArrowRight,
  KeyCode::KeyH,
  KeyCode::KeyR,
  KeyCode::Escape,
  KeyCode::F1,
  KeyCode::F2,
  KeyCode::F3,
  KeyCode::F4,
  KeyCode::F5,
  KeyCode::F9,
  KeyCode::F10,
  KeyCode::F12,
  KeyCode::Backquote,
];

/// The first reserved key any control uses
pub(crate) fn reserved_key(controls: &[Control]) -> Option<KeyCode> {
  controls
    .iter()
    .flat_map(|control| control.keys.iter().copied())
    .find(|key| RESERVED.contains(key))
}

/// A short label for a key: `KeyQ` -> `Q`, `Digit3` -> `3`, `ArrowUp` -> `Up`
pub(crate) fn key_label(key: KeyCode) -> String {
  let name = format!("{key:?}");
  for prefix in ["Key", "Digit", "Arrow"] {
    if let Some(rest) = name.strip_prefix(prefix)
      && !rest.is_empty()
    {
      return rest.to_string();
    }
  }
  name
}
```

`start.rs`:

```rust
//! `RYO_DEMO_PAGE`: the page a demo starts on

/// Why `RYO_DEMO_PAGE` was not used
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StartPageError {
  /// Not a page number: warn and start on page 1
  Invalid(String),
  /// No such page: report and exit with code 2
  OutOfRange(String),
}

/// The 0-based page to start on for a 1-based `RYO_DEMO_PAGE` value, or
/// `Ok(None)` when it is unset.
pub(crate) fn start_page(
  raw: Option<&str>,
  title: &str,
  pages: usize,
) -> Result<Option<usize>, StartPageError> {
  let Some(raw) = raw else {
    return Ok(None);
  };
  let Ok(page) = raw.trim().parse::<usize>() else {
    return Err(StartPageError::Invalid(format!(
      "[demo] RYO_DEMO_PAGE={raw:?} is not a page number; starting on page 1"
    )));
  };
  if page == 0 || page > pages {
    return Err(StartPageError::OutOfRange(format!(
      "[demo] RYO_DEMO_PAGE={page}, but {title} has {}",
      pages_label(pages)
    )));
  }
  Ok(Some(page - 1))
}

/// `1 page`, `6 pages`
pub(crate) fn pages_label(pages: usize) -> String {
  if pages == 1 {
    "1 page".to_string()
  } else {
    format!("{pages} pages")
  }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p demo-kit`
Expected: all pass. Also `cargo clippy -p demo-kit --all-targets` reports nothing for the kit (`dead_code` warnings for items used only by later tasks may be silenced with `#[allow(dead_code)]` on those items, to be removed in Task 3).

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml examples/demo-kit
git commit -m "feat(demos): add demo-kit settings, controls and start page" -m "- Immediate-mode settings rows (float, int, choice, toggle) with clamping
- Hold-to-repeat, page controls with reserved-key checks, and RYO_DEMO_PAGE parsing"
```

---

### Task 3: The shell runtime: pages, switching, settings and keys

**Files:**
- Create: `examples/demo-kit/src/page.rs`, `examples/demo-kit/src/demo.rs`, `examples/demo-kit/src/scene.rs`, `examples/demo-kit/src/chrome.rs` (stub for now)
- Modify: `examples/demo-kit/src/lib.rs`
- Test: `examples/demo-kit/tests/shell.rs`

**Interfaces:**
- Consumes: Task 1 `UiContext::wants_keyboard`; Task 2 `Settings`, `Row`, `HoldRepeat`, `Control`, `reserved_key`, `start_page`, `StartPageError`, `pages_label`.
- Produces:
  - `pub trait DemoPage: 'static` with `fn name(&self) -> &str;` and defaults `fn description(&self) -> &str { "" }`, `fn settings(&mut self, _settings: &mut Settings) {}`, `fn controls(&self) -> Vec<Control> { Vec::new() }`, `fn enter(&mut self, _ctx: &mut SceneContext) {}`, `fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) {}`, `fn render(&mut self, _ctx: &mut SceneContext, _alpha: f32) {}`, `fn render_ui(&mut self, _ui: &mut UiContext) {}`, `fn render_gpu(&mut self, _ctx: &mut GpuRenderContext) {}`.
  - `pub struct Demo` with `pub fn new(title: impl Into<String>) -> Self`, `pub fn resolution(self, width: u32, height: u32) -> Self`, `pub fn configure(self, f: impl FnOnce(EngineBuilder) -> EngineBuilder + 'static) -> Self`, `pub fn page<P: DemoPage, F: Fn() -> P + 'static>(self, factory: F) -> Self`, `pub fn start_page(self, index: usize) -> Self` (0-based), `pub fn engine_builder(self) -> EngineBuilder`, `pub fn run(self)`.
  - `pub mod prelude` re-exporting `ryo2d::prelude::*` and `Control, Demo, DemoPage, Settings, content_area`.
  - `chrome::View<'a>` (fields below) and `chrome::draw(ui, &View)`; Task 4 fills in `draw` and adds `content_area`.

- [ ] **Step 1: Write the failing tests** in `examples/demo-kit/tests/shell.rs`:

```rust
//! Headless tests of the demo shell's keys, pages and settings

use std::sync::{Arc, Mutex};

use demo_kit::prelude::*;
use ryo2d::testing::{HeadlessEngine, StepOutcome};

#[derive(Debug, Default, Clone)]
struct Seen {
  enters: Vec<&'static str>,
  level: f32,
  count: i32,
  mode: usize,
  on: bool,
}

type Shared = Arc<Mutex<Seen>>;

fn seen(shared: &Shared) -> Seen {
  shared.lock().map(|s| s.clone()).unwrap_or_default()
}

/// A page with one row of each kind; `rows` limits how many are declared
struct Probe {
  name: &'static str,
  shared: Shared,
  level: f32,
  count: i32,
  mode: usize,
  on: bool,
  text: Option<TextInputState>,
  dropdown: Option<DropdownState>,
}

impl Probe {
  fn new(name: &'static str, shared: &Shared) -> Self {
    Self {
      name,
      shared: shared.clone(),
      level: 0.5,
      count: 3,
      mode: 0,
      on: false,
      text: None,
      dropdown: None,
    }
  }
}

impl DemoPage for Probe {
  fn name(&self) -> &str {
    self.name
  }

  fn settings(&mut self, s: &mut Settings) {
    s.float("Level", &mut self.level, 0.0..=1.0, 0.1);
    s.int("Count", &mut self.count, 1..=5);
    // Mode "C" hides the toggle row
    s.choice("Mode", &mut self.mode, &["A", "B", "C"]);
    if self.mode != 2 {
      s.toggle("On", &mut self.on);
    }
  }

  fn enter(&mut self, _ctx: &mut SceneContext) {
    if let Ok(mut seen) = self.shared.lock() {
      seen.enters.push(self.name);
    }
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) {
    if let Ok(mut seen) = self.shared.lock() {
      seen.level = self.level;
      seen.count = self.count;
      seen.mode = self.mode;
      seen.on = self.on;
    }
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    if let Some(text) = &mut self.text {
      ui.text_input(text, Rect::new(10.0, 100.0, 120.0, 20.0), "", 0.0);
    }
    if let Some(dropdown) = &mut self.dropdown {
      ui.dropdown(dropdown, Rect::new(10.0, 130.0, 120.0, 20.0), &["x", "y"]);
    }
  }
}

fn three_pages(shared: &Shared) -> Demo {
  let (a, b, c) = (shared.clone(), shared.clone(), shared.clone());
  Demo::new("Probe")
    .resolution(320, 240)
    .page(move || Probe::new("one", &a))
    .page(move || Probe::new("two", &b))
    .page(move || Probe::new("three", &c))
}

fn start(demo: Demo) -> Option<HeadlessEngine> {
  HeadlessEngine::new_or_skip(demo.engine_builder())
}

/// Tap a key, then run two frames so a page switch has entered
fn tap(engine: &mut HeadlessEngine, key: KeyCode) -> StepOutcome {
  engine.tap_key(key);
  let first = engine.step().unwrap_or(StepOutcome::Quit);
  if first == StepOutcome::Quit {
    return first;
  }
  engine.step().unwrap_or(StepOutcome::Quit)
}

#[test]
fn brackets_and_page_keys_change_pages_and_wrap() {
  let shared = Shared::default();
  let Some(mut engine) = start(three_pages(&shared)) else {
    return;
  };
  engine.step_frames(1).unwrap();
  tap(&mut engine, KeyCode::BracketRight);
  tap(&mut engine, KeyCode::BracketLeft);
  tap(&mut engine, KeyCode::BracketLeft);
  tap(&mut engine, KeyCode::PageDown);
  tap(&mut engine, KeyCode::PageUp);
  assert_eq!(seen(&shared).enters, ["one", "two", "one", "three", "one", "three"]);
}

#[test]
fn start_page_opens_that_page() {
  let shared = Shared::default();
  let Some(mut engine) = start(three_pages(&shared).start_page(2)) else {
    return;
  };
  engine.step_frames(1).unwrap();
  assert_eq!(seen(&shared).enters, ["three"]);
}

#[test]
fn arrows_select_rows_and_change_values() {
  let shared = Shared::default();
  let Some(mut engine) = start(three_pages(&shared)) else {
    return;
  };
  engine.step_frames(1).unwrap();
  tap(&mut engine, KeyCode::ArrowRight); // Level 0.5 -> 0.6
  tap(&mut engine, KeyCode::ArrowDown); // Count
  for _ in 0..3 {
    tap(&mut engine, KeyCode::ArrowRight); // 3 -> 5, clamped
  }
  tap(&mut engine, KeyCode::ArrowDown); // Mode
  tap(&mut engine, KeyCode::ArrowLeft); // A -> C (wraps)
  tap(&mut engine, KeyCode::ArrowLeft); // C -> B
  tap(&mut engine, KeyCode::ArrowDown); // On
  tap(&mut engine, KeyCode::ArrowRight); // flips on
  let s = seen(&shared);
  assert!((s.level - 0.6).abs() < 1e-6, "{s:?}");
  assert_eq!((s.count, s.mode, s.on), (5, 1, true), "{s:?}");
}

#[test]
fn holding_right_repeats_and_releasing_stops() {
  let shared = Shared::default();
  let Some(mut engine) = start(three_pages(&shared)) else {
    return;
  };
  engine.step_frames(1).unwrap();
  engine.press_key(KeyCode::ArrowRight);
  engine.step_frames(90).unwrap(); // 1.5 s at the default 60 Hz
  engine.release_key(KeyCode::ArrowRight);
  engine.step_frames(2).unwrap();
  assert_eq!(seen(&shared).level, 1.0);
}

#[test]
fn reset_rebuilds_the_page_with_defaults() {
  let shared = Shared::default();
  let Some(mut engine) = start(three_pages(&shared)) else {
    return;
  };
  engine.step_frames(1).unwrap();
  tap(&mut engine, KeyCode::ArrowRight);
  tap(&mut engine, KeyCode::KeyR);
  let s = seen(&shared);
  assert_eq!(s.enters, ["one", "one"]);
  assert_eq!(s.level, 0.5);
}

#[test]
fn holding_right_through_a_reset_does_not_keep_adjusting() {
  let shared = Shared::default();
  let Some(mut engine) = start(three_pages(&shared)) else {
    return;
  };
  engine.step_frames(1).unwrap();
  engine.press_key(KeyCode::ArrowRight);
  engine.step_frames(2).unwrap();
  tap(&mut engine, KeyCode::KeyR);
  engine.step_frames(60).unwrap(); // still held, 1 s
  assert_eq!(seen(&shared).level, 0.5);
}

#[test]
fn selection_clamps_when_rows_disappear() {
  let shared = Shared::default();
  let Some(mut engine) = start(three_pages(&shared)) else {
    return;
  };
  engine.step_frames(1).unwrap();
  for _ in 0..3 {
    tap(&mut engine, KeyCode::ArrowDown); // On (row 4 of 4)
  }
  tap(&mut engine, KeyCode::ArrowUp); // Mode
  tap(&mut engine, KeyCode::ArrowLeft); // A -> C: the On row disappears
  tap(&mut engine, KeyCode::ArrowDown); // stays on Mode, the last row
  tap(&mut engine, KeyCode::ArrowRight); // C -> A
  let s = seen(&shared);
  assert_eq!(s.mode, 0, "{s:?}");
  assert!(!s.on, "{s:?}");
}

#[test]
fn help_closes_on_escape_before_escape_quits() {
  let shared = Shared::default();
  let Some(mut engine) = start(three_pages(&shared)) else {
    return;
  };
  engine.step_frames(1).unwrap();
  assert_eq!(tap(&mut engine, KeyCode::KeyH), StepOutcome::Running);
  assert_eq!(tap(&mut engine, KeyCode::Escape), StepOutcome::Running);
  assert_eq!(tap(&mut engine, KeyCode::Escape), StepOutcome::Quit);
}

#[test]
fn shell_keys_are_ignored_while_typing() {
  let shared = Shared::default();
  let typing = shared.clone();
  let demo = Demo::new("Typing").resolution(320, 240).page(move || {
    let mut page = Probe::new("typing", &typing);
    let mut text = TextInputState::new();
    text.set_focused(true);
    page.text = Some(text);
    page
  });
  let Some(mut engine) = start(demo) else {
    return;
  };
  engine.step_frames(2).unwrap();
  for key in [
    KeyCode::KeyR,
    KeyCode::BracketRight,
    KeyCode::ArrowRight,
    KeyCode::KeyH,
    KeyCode::Escape,
  ] {
    assert_eq!(tap(&mut engine, key), StepOutcome::Running, "{key:?}");
  }
  let s = seen(&shared);
  assert_eq!(s.enters, ["typing"]);
  assert_eq!(s.level, 0.5);
}

#[test]
fn escape_with_an_open_dropdown_does_not_quit() {
  let shared = Shared::default();
  let picking = shared.clone();
  let demo = Demo::new("Dropdown").resolution(320, 240).page(move || {
    let mut page = Probe::new("dropdown", &picking);
    let mut dropdown = DropdownState::with_id(7);
    dropdown.open();
    page.dropdown = Some(dropdown);
    page
  });
  let Some(mut engine) = start(demo) else {
    return;
  };
  engine.step_frames(2).unwrap();
  assert_eq!(tap(&mut engine, KeyCode::Escape), StepOutcome::Running);
}

struct Greedy;

impl DemoPage for Greedy {
  fn name(&self) -> &str {
    "Greedy"
  }
  fn controls(&self) -> Vec<Control> {
    vec![Control::key(KeyCode::KeyH, "Hop")]
  }
}

#[test]
#[should_panic(expected = "reserved")]
fn a_page_listing_a_reserved_key_is_rejected() {
  let _ = Demo::new("Greedy").page(|| Greedy).engine_builder();
}
```

If `TextInputState` or `DropdownState` are not in `ryo2d::prelude`, import them from where `examples/demos/ui` gets them (`grep -rn "TextInputState" examples/demos/ui/src/scenes/text_input_demo.rs | head -3`).

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p demo-kit --test shell`
Expected: compile errors (`Demo`, `DemoPage`, `prelude` not found).

- [ ] **Step 3: Implement**

`page.rs`:

```rust
//! The trait every demo page implements

use ryo2d::prelude::*;

use crate::{Control, Settings};

/// One page of a demo. Its feature code is plain ryo2d; the shell supplies
/// page switching, the settings panel, help and layout.
///
/// A page is built fresh from its factory whenever it is shown or reset, and
/// runs inside its own engine scene, so the world, camera, effects, lighting,
/// time and events start clean.
pub trait DemoPage: 'static {
  /// Shown in the top bar, e.g. `Glow`
  fn name(&self) -> &str;

  /// What the page shows; the first line appears under the top bar, all of it in help
  fn description(&self) -> &str {
    ""
  }

  /// Declare the settings panel's rows (called every frame, before `update`)
  fn settings(&mut self, _settings: &mut Settings) {}

  /// The page's own keys and mouse controls, for the legend and help.
  /// Reserved shell keys are rejected when the demo starts.
  fn controls(&self) -> Vec<Control> {
    Vec::new()
  }

  /// Set up the page (spawn entities, configure the camera and lighting)
  fn enter(&mut self, _ctx: &mut SceneContext) {}

  /// Fixed-timestep update; read settings fields here
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) {}

  /// Called every frame before rendering (see `Scene::render`)
  fn render(&mut self, _ctx: &mut SceneContext, _alpha: f32) {}

  /// The page's own UI, drawn under the shell's
  fn render_ui(&mut self, _ui: &mut UiContext) {}

  /// GPU drawing (particles, `render_gpu` sprites)
  fn render_gpu(&mut self, _ctx: &mut GpuRenderContext) {}
}
```

`demo.rs`:

```rust
//! The demo builder

use std::rc::Rc;

use ryo2d::prelude::*;

use crate::controls::reserved_key;
use crate::page::DemoPage;
use crate::scene::PageScene;
use crate::start::{StartPageError, pages_label, start_page};

type PageFactory = Box<dyn Fn() -> Box<dyn DemoPage>>;
type Configure = Box<dyn FnOnce(EngineBuilder) -> EngineBuilder>;

/// What every page of one demo shares
pub(crate) struct DemoDef {
  pub title: String,
  pub pages: Vec<PageFactory>,
}

/// A demo: a title, a window size and its pages.
///
/// ```rust,ignore
/// fn main() {
///   Demo::new("Bloom").resolution(960, 640).page(Glow::default).run();
/// }
/// ```
pub struct Demo {
  title: String,
  resolution: Option<(u32, u32)>,
  configure: Vec<Configure>,
  pages: Vec<PageFactory>,
  start: usize,
}

impl Demo {
  pub fn new(title: impl Into<String>) -> Self {
    Self {
      title: title.into(),
      resolution: None,
      configure: Vec::new(),
      pages: Vec::new(),
      start: 0,
    }
  }

  /// Game resolution
  pub fn resolution(mut self, width: u32, height: u32) -> Self {
    self.resolution = Some((width, height));
    self
  }

  /// Any other engine setting, e.g. `.configure(|b| b.physics_hz(144))`
  pub fn configure(mut self, f: impl FnOnce(EngineBuilder) -> EngineBuilder + 'static) -> Self {
    self.configure.push(Box::new(f));
    self
  }

  /// Add a page; `factory` builds it fresh each time it is shown or reset
  pub fn page<P: DemoPage, F: Fn() -> P + 'static>(mut self, factory: F) -> Self {
    self
      .pages
      .push(Box::new(move || Box::new(factory()) as Box<dyn DemoPage>));
    self
  }

  /// Start on this page (0-based); `RYO_DEMO_PAGE` overrides it in `run`
  pub fn start_page(mut self, index: usize) -> Self {
    self.start = index;
    self
  }

  /// The engine builder for this demo, for `run` and for headless tests.
  ///
  /// Panics if there are no pages, the start page is out of range, or a
  /// page lists a reserved key.
  pub fn engine_builder(self) -> EngineBuilder {
    assert!(!self.pages.is_empty(), "demo \"{}\" has no pages", self.title);
    assert!(
      self.start < self.pages.len(),
      "demo \"{}\": start page {} is out of range",
      self.title,
      self.start
    );
    for factory in &self.pages {
      let page = factory();
      if let Some(key) = reserved_key(&page.controls()) {
        panic!(
          "demo \"{}\", page \"{}\": {key:?} is reserved by the demo shell",
          self.title,
          page.name()
        );
      }
    }

    let def = Rc::new(DemoDef {
      title: self.title,
      pages: self.pages,
    });
    let mut builder = EngineBuilder::new().title(format!("Ryo2D - {}", def.title));
    if let Some((width, height)) = self.resolution {
      builder = builder.resolution(width, height);
    }
    for configure in self.configure {
      builder = configure(builder);
    }
    builder.start_scene(PageScene::new(def, self.start))
  }

  /// Run the demo in a window (or offscreen with `RYO_HEADLESS=1`)
  pub fn run(mut self) {
    let pages = self.pages.len();
    println!("[demo] {}: {}", self.title, pages_label(pages));
    if cfg!(debug_assertions) {
      let raw = std::env::var("RYO_DEMO_PAGE").ok();
      match start_page(raw.as_deref(), &self.title, pages) {
        Ok(Some(index)) => self.start = index,
        Ok(None) => {}
        Err(StartPageError::Invalid(message)) => eprintln!("{message}"),
        Err(StartPageError::OutOfRange(message)) => {
          eprintln!("{message}");
          std::process::exit(2);
        }
      }
    }
    let engine = self.engine_builder().build();
    if let Err(error) = engine.run() {
      eprintln!("Engine error: {error}");
    }
  }
}
```

`scene.rs`:

```rust
//! The engine scene that hosts one page plus the shell

use std::rc::Rc;

use ryo2d::prelude::*;

use crate::chrome::{self, View};
use crate::demo::DemoDef;
use crate::hold::HoldRepeat;
use crate::page::DemoPage;
use crate::settings::{Row, Settings};

/// One page of a demo, with the shell's state
pub(crate) struct PageScene {
  def: Rc<DemoDef>,
  index: usize,
  page: Box<dyn DemoPage>,
  help_open: bool,
  selected: usize,
  rows: Vec<Row>,
  left: HoldRepeat,
  right: HoldRepeat,
  /// A widget had keyboard focus on the last UI frame
  keyboard_wanted: bool,
}

impl PageScene {
  pub(crate) fn new(def: Rc<DemoDef>, index: usize) -> Self {
    let page = (def.pages[index])();
    Self {
      def,
      index,
      page,
      help_open: false,
      selected: 0,
      rows: Vec::new(),
      left: HoldRepeat::default(),
      right: HoldRepeat::default(),
      keyboard_wanted: false,
    }
  }

  /// A fresh scene for page `index`; the engine's scene cleanup runs between
  fn switch_to(&self, index: usize) -> SceneAction {
    SceneAction::Switch(Box::new(PageScene::new(self.def.clone(), index)))
  }

  /// Handle the shell's keys; returns the adjustment steps for the panel, or
  /// a scene action to return instead of updating the page
  fn shell_keys(&mut self, ctx: &mut SceneContext) -> Result<i32, SceneAction> {
    let dt = ctx.time.fixed_dt;
    let input = &mut *ctx.input;

    if input.key_pressed_and_consume(KeyCode::KeyH) {
      self.help_open = !self.help_open;
    }
    if input.key_pressed_and_consume(KeyCode::Escape) {
      if !self.help_open {
        return Err(SceneAction::Quit);
      }
      self.help_open = false;
    }

    let count = self.def.pages.len();
    // `|` so both aliases are consumed
    let next = input.key_pressed_and_consume(KeyCode::BracketRight)
      | input.key_pressed_and_consume(KeyCode::PageDown);
    let prev = input.key_pressed_and_consume(KeyCode::BracketLeft)
      | input.key_pressed_and_consume(KeyCode::PageUp);
    if next {
      return Err(self.switch_to((self.index + 1) % count));
    }
    if prev {
      return Err(self.switch_to((self.index + count - 1) % count));
    }
    if input.key_pressed_and_consume(KeyCode::KeyR) {
      return Err(self.switch_to(self.index));
    }

    if input.key_pressed_and_consume(KeyCode::ArrowDown) && !self.rows.is_empty() {
      self.selected = (self.selected + 1).min(self.rows.len() - 1);
    }
    if input.key_pressed_and_consume(KeyCode::ArrowUp) {
      self.selected = self.selected.saturating_sub(1);
    }
    let right_pressed = input.key_pressed_and_consume(KeyCode::ArrowRight);
    let left_pressed = input.key_pressed_and_consume(KeyCode::ArrowLeft);
    let right = self
      .right
      .steps(right_pressed, input.key_down(KeyCode::ArrowRight), dt);
    let left = self
      .left
      .steps(left_pressed, input.key_down(KeyCode::ArrowLeft), dt);
    Ok(right as i32 - left as i32)
  }
}

impl Scene for PageScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    self.page.enter(ctx);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    let adjust = if self.keyboard_wanted {
      // Typing into a widget: the shell's keys belong to it
      self.left = HoldRepeat::default();
      self.right = HoldRepeat::default();
      0
    } else {
      match self.shell_keys(ctx) {
        Ok(adjust) => adjust,
        Err(action) => return action,
      }
    };

    let mut settings = Settings::new(self.selected, adjust);
    self.page.settings(&mut settings);
    self.rows = settings.into_rows();
    self.selected = self.selected.min(self.rows.len().saturating_sub(1));

    self.page.update(ctx, dt);
    SceneAction::Continue
  }

  fn render(&mut self, ctx: &mut SceneContext, alpha: f32) {
    self.page.render(ctx, alpha);
  }

  fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction {
    self.page.render_ui(ui);
    self.keyboard_wanted = ui.wants_keyboard();
    let controls = self.page.controls();
    let view = View {
      title: &self.def.title,
      index: self.index,
      count: self.def.pages.len(),
      page_name: self.page.name(),
      description: self.page.description(),
      rows: &self.rows,
      selected: self.selected,
      controls: &controls,
      help_open: self.help_open,
    };
    chrome::draw(ui, &view);
    SceneAction::Continue
  }

  fn render_gpu(&mut self, ctx: &mut GpuRenderContext) {
    self.page.render_gpu(ctx);
  }
}
```

`chrome.rs` (stub; Task 4 replaces the body of `draw`):

```rust
//! The shell's drawing: top bar, description, settings panel, legend and help

use ryo2d::prelude::*;

use crate::Control;
use crate::settings::Row;

/// Everything the shell draws for one frame
pub(crate) struct View<'a> {
  pub title: &'a str,
  pub index: usize,
  pub count: usize,
  pub page_name: &'a str,
  pub description: &'a str,
  pub rows: &'a [Row],
  pub selected: usize,
  pub controls: &'a [Control],
  pub help_open: bool,
}

pub(crate) fn draw(_ui: &mut UiContext, _view: &View) {}
```

`lib.rs` (replace the module list and exports):

```rust
mod chrome;
mod controls;
mod demo;
mod hold;
mod page;
mod scene;
mod settings;
mod start;

pub use controls::Control;
pub use demo::Demo;
pub use page::DemoPage;
pub use settings::Settings;

/// Everything a demo needs: ryo2d's prelude plus the kit
pub mod prelude {
  pub use crate::{Control, Demo, DemoPage, Settings};
  pub use ryo2d::prelude::*;
}
```

Remove any `#[allow(dead_code)]` added in Task 2 that is now unnecessary.

If `Scene::render` or `Scene::render_gpu` have different signatures than above, match `src/core/scene.rs` exactly (`grep -n "fn render\|fn render_gpu" src/core/scene.rs`).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p demo-kit`
Expected: all pass (unit and `shell` tests). With no GPU the headless tests print a skip line and return.

- [ ] **Step 5: Commit**

```bash
git add examples/demo-kit
git commit -m "feat(demos): run demo pages as scenes with shared shell keys" -m "- Demo builder with page factories; each page runs in its own scene and [ ], PageUp/PageDown and R switch to a fresh one
- Arrow keys drive the settings panel with hold-to-repeat; H toggles help, Esc closes it or quits
- Shell keys step aside while a widget wants the keyboard; reserved keys in page controls are rejected"
```

---

### Task 4: The shell's drawing and the demo standard doc

**Files:**
- Modify: `examples/demo-kit/src/chrome.rs`, `examples/demo-kit/src/lib.rs`
- Create: `examples/demos/AGENTS.md`
- Test: `examples/demo-kit/tests/chrome.rs`; unit tests in `chrome.rs`

**Interfaces:**
- Consumes: Task 3 `View`, `Row`, `Control` fields (`label`, `keys`, `description`).
- Produces: `pub fn content_area(ui: &UiContext) -> Rect` (re-exported from the crate root and the prelude); crate-private `layout(screen_w, screen_h, line_h) -> Layout` and `wrap_items(widths, gap, available) -> Vec<Vec<usize>>`.

- [ ] **Step 1: Write the failing tests**

Unit tests at the end of `chrome.rs`:

```rust
#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn layout_stacks_bar_description_content_and_legend() {
    let l = layout(800.0, 600.0, 10.0);
    assert_eq!(l.top_bar, Rect::new(0.0, 0.0, 800.0, 20.0));
    assert_eq!(l.description_y, 25.0);
    assert_eq!(l.content.y, 40.0);
    assert_eq!(l.content.x, 5.0);
    // The legend line sits at 600 - 5 - 10 = 585; content ends a pad above it
    assert_eq!(l.content.y + l.content.height, 580.0);
  }

  #[test]
  fn layout_never_goes_negative_on_a_tiny_screen() {
    let l = layout(40.0, 30.0, 10.0);
    assert!(l.content.width >= 0.0 && l.content.height >= 0.0);
  }

  #[test]
  fn items_wrap_into_lines_that_fit() {
    assert_eq!(wrap_items(&[30.0, 30.0, 30.0], 10.0, 100.0), vec![vec![0, 1], vec![2]]);
    assert_eq!(wrap_items(&[30.0, 30.0], 10.0, 70.0), vec![vec![0, 1]]);
    // An item wider than the space still gets a line of its own
    assert_eq!(wrap_items(&[500.0, 10.0], 10.0, 100.0), vec![vec![0], vec![1]]);
    assert!(wrap_items(&[], 10.0, 100.0).is_empty());
  }

  #[test]
  fn truncation_keeps_text_within_the_width() {
    let width = |s: &str| s.len() as f32 * 8.0;
    assert_eq!(truncate_to("short", 100.0, width), "short");
    let cut = truncate_to("a long description line", 80.0, width);
    assert!(width(&cut) <= 80.0, "{cut}");
    assert!(cut.ends_with("..."), "{cut}");
  }
}
```

Headless tests in `examples/demo-kit/tests/chrome.rs`:

```rust
//! Headless smoke tests of what the shell draws

use demo_kit::prelude::*;
use ryo2d::testing::HeadlessEngine;

const RED: Color = Color::new(200, 60, 60, 255);

/// Fills the screen red; `rows` settings rows and `controls` legend items
struct Backdrop {
  rows: usize,
  controls: usize,
  values: Vec<f32>,
}

impl Backdrop {
  fn new(rows: usize, controls: usize) -> Self {
    Self {
      rows,
      controls,
      values: vec![0.5; rows],
    }
  }
}

impl DemoPage for Backdrop {
  fn name(&self) -> &str {
    "Backdrop"
  }
  fn description(&self) -> &str {
    "A page that fills the screen so the shell's drawing can be seen against it."
  }
  fn settings(&mut self, s: &mut Settings) {
    for (i, value) in self.values.iter_mut().enumerate().take(self.rows) {
      s.float(&format!("Value {i}"), value, 0.0..=1.0, 0.1);
    }
  }
  fn controls(&self) -> Vec<Control> {
    (0..self.controls)
      .map(|i| Control::key(KeyCode::Digit1, format!("A rather long control description {i}")))
      .collect()
  }
  fn render_ui(&mut self, ui: &mut UiContext) {
    let (w, h) = ui.screen_size();
    ui.draw_rect(Rect::new(0.0, 0.0, w as f32, h as f32), RED);
  }
}

fn frame_of(page: Backdrop, size: (u32, u32), keys: &[KeyCode]) -> Option<ryo2d::image::RgbaImage> {
  let rows = page.rows;
  let controls = page.controls;
  let demo = Demo::new("Chrome")
    .resolution(size.0, size.1)
    .page(move || Backdrop::new(rows, controls));
  let mut engine = HeadlessEngine::new_or_skip(demo.engine_builder())?;
  engine.step_frames(2).ok()?;
  for key in keys {
    engine.tap_key(*key);
    engine.step_frames(2).ok()?;
  }
  engine.frame().ok()
}

fn red_at(image: &ryo2d::image::RgbaImage, x: u32, y: u32) -> u8 {
  image.get_pixel(x, y).0[0]
}

#[test]
fn the_top_bar_darkens_the_top_of_the_screen() {
  let Some(image) = frame_of(Backdrop::new(0, 0), (320, 240), &[]) else {
    return;
  };
  assert!(red_at(&image, 160, 1) < 80);
  assert!(red_at(&image, 160, 120) > 180, "no settings: the middle stays clear");
}

#[test]
fn the_settings_panel_appears_only_with_settings() {
  let Some(with) = frame_of(Backdrop::new(10, 0), (320, 240), &[]) else {
    return;
  };
  let Some(without) = frame_of(Backdrop::new(0, 0), (320, 240), &[]) else {
    return;
  };
  assert!(red_at(&with, 312, 120) < 120, "panel in the right column");
  assert!(red_at(&without, 312, 120) > 180);
}

#[test]
fn help_dims_the_screen() {
  let Some(image) = frame_of(Backdrop::new(0, 0), (320, 240), &[KeyCode::KeyH]) else {
    return;
  };
  assert!(red_at(&image, 160, 120) < 150);
}

#[test]
fn tiny_screen_with_many_rows_and_a_long_legend_renders() {
  let Some(image) = frame_of(Backdrop::new(30, 12), (160, 120), &[KeyCode::KeyH]) else {
    return;
  };
  assert_eq!((image.width(), image.height()), (160, 120));
}
```

Before relying on `ryo2d::image::RgbaImage` and `Color::new` being `const`, check them: `grep -n "pub use image\|pub extern crate image\|image::" src/lib.rs | head` and `grep -n "pub const fn new" src/core/ui/types.rs src/core/ui/*.rs | head`. If `image` is not re-exported, add `image = "0.25"` to `[dev-dependencies]` of the kit (the version the engine uses: `grep -n "^image" Cargo.toml`) and use `image::RgbaImage`; if `Color::new` is not `const`, build `RED` inside a `fn red() -> Color`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p demo-kit`
Expected: unit tests fail to compile (`layout`, `wrap_items`, `truncate_to` missing); headless tests fail their assertions (nothing is drawn).

- [ ] **Step 3: Implement** — replace `chrome.rs` above the tests with:

```rust
//! The shell's drawing: top bar, description, settings panel, legend and help
//!
//! Text is ASCII only: the bitmap font covers characters 32-126.

use ryo2d::prelude::*;

use crate::Control;
use crate::settings::Row;

/// Everything the shell draws for one frame
pub(crate) struct View<'a> {
  pub title: &'a str,
  pub index: usize,
  pub count: usize,
  pub page_name: &'a str,
  pub description: &'a str,
  pub rows: &'a [Row],
  pub selected: usize,
  pub controls: &'a [Control],
  pub help_open: bool,
}

/// Where the shell's parts go on a screen
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Layout {
  pub line_h: f32,
  pub pad: f32,
  pub top_bar: Rect,
  pub description_y: f32,
  /// Below the description, above the legend, full width
  pub content: Rect,
}

pub(crate) fn layout(screen_w: f32, screen_h: f32, line_h: f32) -> Layout {
  let pad = (line_h * 0.5).round().max(2.0);
  let bar_h = line_h + 2.0 * pad;
  let description_y = bar_h + pad;
  let content_y = description_y + line_h + pad;
  let legend_y = screen_h - pad - line_h;
  Layout {
    line_h,
    pad,
    top_bar: Rect::new(0.0, 0.0, screen_w, bar_h),
    description_y,
    content: Rect::new(
      pad,
      content_y,
      (screen_w - 2.0 * pad).max(0.0),
      (legend_y - pad - content_y).max(0.0),
    ),
  }
}

/// The area a page can use: below the shell's top bar and description and
/// above its legend. The settings panel, when shown, covers its right side.
pub fn content_area(ui: &UiContext) -> Rect {
  let (w, h) = ui.screen_size();
  layout(w as f32, h as f32, line_height(ui)).content
}

fn line_height(ui: &UiContext) -> f32 {
  ui.text_size("Ag").1 as f32
}

fn text_w(ui: &UiContext, text: &str) -> f32 {
  ui.text_size(text).0 as f32
}

/// Group items, in order, into lines no wider than `available`
pub(crate) fn wrap_items(widths: &[f32], gap: f32, available: f32) -> Vec<Vec<usize>> {
  let mut lines: Vec<Vec<usize>> = Vec::new();
  let mut used = 0.0;
  for (i, &width) in widths.iter().enumerate() {
    match lines.last_mut() {
      Some(line) if used + gap + width <= available => {
        line.push(i);
        used += gap + width;
      }
      _ => {
        lines.push(vec![i]);
        used = width;
      }
    }
  }
  lines
}

/// `text`, shortened with `...` until `width` says it fits in `max`
pub(crate) fn truncate_to(text: &str, max: f32, width: impl Fn(&str) -> f32) -> String {
  if width(text) <= max {
    return text.to_string();
  }
  let mut chars: Vec<char> = text.chars().collect();
  while !chars.is_empty() {
    chars.pop();
    let candidate: String = chars.iter().collect::<String>() + "...";
    if width(&candidate) <= max {
      return candidate;
    }
  }
  String::new()
}

fn bar_color() -> Color {
  Color::new(12, 12, 20, 225)
}
fn panel_color() -> Color {
  Color::new(12, 12, 20, 205)
}
fn highlight_color() -> Color {
  Color::new(60, 60, 95, 230)
}
fn dim_text() -> Color {
  Color::new(170, 170, 185, 255)
}
fn key_text() -> Color {
  Color::YELLOW
}

pub(crate) fn draw(ui: &mut UiContext, view: &View) {
  let (w, h) = ui.screen_size();
  let (w, h) = (w as f32, h as f32);
  let l = layout(w, h, line_height(ui));
  draw_top_bar(ui, view, &l, w);
  draw_description(ui, view, &l, w);
  draw_panel(ui, view, &l, w);
  draw_legend(ui, view, &l, w, h);
  if view.help_open {
    draw_help(ui, view, &l, w, h);
  }
}

fn draw_top_bar(ui: &mut UiContext, view: &View, l: &Layout, w: f32) {
  ui.draw_rect(l.top_bar, bar_color());
  let left = if view.count > 1 {
    format!("{} - {}/{} {}", view.title, view.index + 1, view.count, view.page_name)
  } else {
    view.title.to_string()
  };
  let right = if view.count > 1 {
    "[ ] page   H help   Esc quit"
  } else {
    "H help   Esc quit"
  };
  let y = l.pad;
  ui.draw_text(&left, l.pad, y, Color::WHITE);
  let right_w = text_w(ui, right);
  let right_x = w - l.pad - right_w;
  // Only when it clears the left text
  if right_x > l.pad + text_w(ui, &left) + l.pad {
    ui.draw_text(right, right_x, y, dim_text());
  }
}

fn draw_description(ui: &mut UiContext, view: &View, l: &Layout, w: f32) {
  let first = view.description.lines().next().unwrap_or("");
  if first.is_empty() {
    return;
  }
  let line = truncate_to(first, w - 2.0 * l.pad, |s| text_w(ui, s));
  ui.draw_text(&line, l.pad, l.description_y, dim_text());
}

fn draw_panel(ui: &mut UiContext, view: &View, l: &Layout, w: f32) {
  if view.rows.is_empty() {
    return;
  }
  let gap = l.pad * 2.0;
  let hint = "Up/Down pick  Left/Right set";
  let widest_row = view
    .rows
    .iter()
    .map(|row| text_w(ui, "> ") + text_w(ui, &row.label) + gap + text_w(ui, &row.value))
    .fold(0.0, f32::max);
  let panel_w = (widest_row.max(text_w(ui, hint)) + 2.0 * l.pad).min(w * 0.45);
  let row_h = l.line_h + l.pad * 0.5;
  // Header + rows + hint, within the content area
  let wanted_h = (view.rows.len() as f32 + 2.0) * row_h + 2.0 * l.pad;
  let panel_h = wanted_h.min(l.content.height);
  let x = w - l.pad - panel_w;
  let y = l.content.y;
  if panel_h < row_h * 2.0 || panel_w <= 0.0 {
    return;
  }
  ui.draw_rect(Rect::new(x, y, panel_w, panel_h), panel_color());
  ui.draw_text("SETTINGS", x + l.pad, y + l.pad, Color::WHITE);

  let rows_fit = ((panel_h - 2.0 * l.pad) / row_h).floor() as usize;
  let visible = rows_fit.saturating_sub(2).min(view.rows.len());
  // Keep the selected row in view
  let first = view.selected.saturating_sub(visible.saturating_sub(1));
  let inner = panel_w - 2.0 * l.pad;
  for (slot, (i, row)) in view.rows.iter().enumerate().skip(first).take(visible).enumerate() {
    let row_y = y + l.pad + row_h * (slot as f32 + 1.0);
    let selected = i == view.selected;
    if selected {
      ui.draw_rect(Rect::new(x + 2.0, row_y - l.pad * 0.25, panel_w - 4.0, row_h), highlight_color());
    }
    let marker = if selected { "> " } else { "  " };
    let label = truncate_to(&format!("{marker}{}", row.label), inner * 0.6, |s| text_w(ui, s));
    ui.draw_text(&label, x + l.pad, row_y, if selected { key_text() } else { Color::WHITE });
    let value = truncate_to(&row.value, inner * 0.4, |s| text_w(ui, s));
    let value_x = x + panel_w - l.pad - text_w(ui, &value);
    ui.draw_text(&value, value_x, row_y, Color::WHITE);
  }
  let hint_y = y + panel_h - l.pad - l.line_h;
  let hint = truncate_to(hint, inner, |s| text_w(ui, s));
  ui.draw_text(&hint, x + l.pad, hint_y, dim_text());
}

/// Legend items: the page's controls, then `R reset`
fn legend_items(view: &View) -> Vec<(String, String)> {
  let mut items: Vec<(String, String)> = view
    .controls
    .iter()
    .map(|control| (control.label.clone(), control.description.clone()))
    .collect();
  items.push(("R".to_string(), "reset".to_string()));
  items
}

fn draw_legend(ui: &mut UiContext, view: &View, l: &Layout, w: f32, h: f32) {
  let items = legend_items(view);
  let gap = l.pad * 3.0;
  let widths: Vec<f32> = items
    .iter()
    .map(|(key, desc)| text_w(ui, key) + text_w(ui, " ") + text_w(ui, desc))
    .collect();
  // Leave the bottom-left corner to the engine's FPS counter
  let fps_reserve = text_w(ui, "FPS: 0000") + 2.0 * l.pad;
  let available = (w - 2.0 * l.pad - fps_reserve).max(1.0);
  let lines = wrap_items(&widths, gap, available);
  let line_step = l.line_h + l.pad * 0.5;
  for (n, line) in lines.iter().enumerate() {
    let from_bottom = (lines.len() - 1 - n) as f32;
    let y = h - l.pad - l.line_h - from_bottom * line_step;
    if y < l.content.y {
      continue;
    }
    let line_w: f32 =
      line.iter().map(|&i| widths[i]).sum::<f32>() + gap * line.len().saturating_sub(1) as f32;
    let mut x = (w - l.pad - line_w).max(l.pad);
    for &i in line {
      let (key, desc) = &items[i];
      ui.draw_text(key, x, y, key_text());
      let desc_x = x + text_w(ui, key) + text_w(ui, " ");
      ui.draw_text(desc, desc_x, y, dim_text());
      x += widths[i] + gap;
    }
  }
}

fn draw_help(ui: &mut UiContext, view: &View, l: &Layout, w: f32, h: f32) {
  ui.draw_modal_overlay(Color::new(0, 0, 0, 170));
  let box_w = (w - 4.0 * l.pad).max(0.0);
  let box_h = (h - 4.0 * l.pad).max(0.0);
  let (x, y) = (2.0 * l.pad, 2.0 * l.pad);
  ui.draw_rect(Rect::new(x, y, box_w, box_h), panel_color());

  let mut lines: Vec<(String, Color)> = Vec::new();
  let page = if view.count > 1 {
    format!("{} - page {}/{}: {}", view.title, view.index + 1, view.count, view.page_name)
  } else {
    format!("{}: {}", view.title, view.page_name)
  };
  lines.push((page, Color::WHITE));
  for line in view.description.lines() {
    lines.push((line.to_string(), dim_text()));
  }
  lines.push((String::new(), Color::WHITE));
  lines.push(("Demo keys".to_string(), key_text()));
  if view.count > 1 {
    lines.push(("  [ ]  previous / next page".to_string(), Color::WHITE));
  }
  lines.push(("  Up/Down  pick a setting   Left/Right  change it".to_string(), Color::WHITE));
  lines.push(("  R  reset this page   H  help   Esc  close help / quit".to_string(), Color::WHITE));
  if !view.controls.is_empty() {
    lines.push((String::new(), Color::WHITE));
    lines.push(("This page".to_string(), key_text()));
    for control in view.controls {
      lines.push((format!("  {}  {}", control.label, control.description), Color::WHITE));
    }
  }
  if !view.rows.is_empty() {
    lines.push((String::new(), Color::WHITE));
    lines.push(("Settings".to_string(), key_text()));
    for row in view.rows {
      lines.push((format!("  {}  {}", row.label, row.range), Color::WHITE));
    }
  }

  let inner = box_w - 2.0 * l.pad;
  let step = l.line_h + l.pad * 0.5;
  let mut line_y = y + l.pad;
  for (text, color) in lines {
    if line_y + l.line_h > y + box_h - l.pad {
      break;
    }
    let text = truncate_to(&text, inner, |s| text_w(ui, s));
    ui.draw_text(&text, x + l.pad, line_y, color);
    line_y += step;
  }
}
```

Export `content_area`: in `lib.rs` add `pub use chrome::content_area;` and add `content_area` to the prelude's `pub use crate::{...}`.

If `Rect` does not implement `PartialEq`/`Debug` (needed by `layout`'s derive and the unit test), compare fields in the test instead and drop `PartialEq` from `Layout`'s derive. If `draw_text` takes a different text type or the coordinates are the top-left of the text box, keep the calls and adjust only the offsets.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p demo-kit`
Expected: all pass.

- [ ] **Step 5: Write `examples/demos/AGENTS.md`** (LF):

````markdown
# AGENTS.md - Feature Demos

Every demo in this folder follows one standard, provided by `examples/demo-kit`. Spec: `docs/plans/2026-10-11-demo-standard-design.md`. Games in `examples/games` keep their own UX.

## Controls (every demo)

| Key | Meaning |
|---|---|
| `[` / `]` (PageUp / PageDown) | Previous / next page, wrapping |
| Up / Down | Pick a row in the settings panel |
| Left / Right | Change it (hold to repeat) |
| `R` | Reset the page (values back to defaults, scene rebuilt) |
| `H` | Help: the page's description, every control and setting |
| `Esc` | Close help, otherwise quit |
| `Space` | The page's main action, if any (convention) |
| WASD / mouse | Movement, camera, pointing (convention) |

- Arrow keys always belong to the panel; pages move with WASD.
- Presets and variants are `choice` settings, not number keys. Digits, other letters and Tab are free for the page and listed in its controls.
- A page listing a reserved key (`[ ] PageUp PageDown`, arrows, `H R Esc`, F1-F5, F9, F10, F12, backquote) makes the demo panic at startup.
- While a text field or dropdown has keyboard focus the shell ignores all of its keys.

## Writing a demo

```rust
use demo_kit::prelude::*;

fn main() {
  Demo::new("Bloom")
    .resolution(960, 640)
    .configure(|b| b.physics_hz(144)) // any other EngineBuilder setting
    .page(Glow::default)
    .run();
}

#[derive(Default)]
struct Glow {
  intensity: f32,
}

impl DemoPage for Glow {
  fn name(&self) -> &str { "Glow" }
  fn description(&self) -> &str { "Bright pixels bleed light into their neighbours." }
  fn settings(&mut self, s: &mut Settings) {
    s.float("Intensity", &mut self.intensity, 0.0..=5.0, 0.1);
  }
  fn controls(&self) -> Vec<Control> {
    vec![Control::key(KeyCode::Space, "Burst")]
  }
  fn enter(&mut self, ctx: &mut SceneContext) { /* spawn */ }
  fn update(&mut self, ctx: &mut SceneContext, dt: f32) { /* read self.intensity */ }
}
```

- Add `demo-kit = { path = "../../demo-kit" }` to the demo's `Cargo.toml`.
- Each page is built fresh from its factory when shown or reset, inside its own scene: the engine's scene cleanup resets the world, camera, effects, lighting, time, events and raycasting between pages. Keep feature code plain ryo2d.
- `Settings`: `float(label, &mut f32, range, step)`, `int(label, &mut i32, range)`, `choice(label, &mut usize, &[&str])`, `toggle(label, &mut bool)`. Called every frame before `update`; rows may change with other values.
- `Control::key(KeyCode, desc)`, `Control::keys("WASD", &[..], desc)`, `Control::mouse(desc)`.
- Draw page UI inside `content_area(ui)`: below the top bar and description, above the legend. The settings panel covers the right of it when the page has settings, so keep important content left of about 55% of the width on those pages.

## Layout

Top bar (title, `N/M page`, shell keys), one description line, the settings panel in a right column, and a right-aligned legend at the bottom. The bottom-left corner stays free for the engine's FPS counter.

## Scripting

- `RYO_DEMO_PAGE=<n>` (dev builds, 1-based) starts on page n; an out-of-range page exits with code 2.
- The demo logs `[demo] <title>: <m> pages` at startup.
- Combine with `RYO_HEADLESS=1`, `RYO_INPUT` and `RYO_CAPTURE_*` (see the `ryo-cli` skill).
````

- [ ] **Step 6: Commit**

```bash
git add examples/demo-kit examples/demos/AGENTS.md
git commit -m "feat(demos): draw the demo shell's bar, settings panel, legend and help" -m "- Top bar with page and shell keys, one description line, right settings column, right-aligned legend clear of the FPS counter
- Help overlay listing the description, shell keys, page controls and settings
- content_area for page UI; document the demo standard in examples/demos/AGENTS.md"
```

---

### Task 5: The demo tour

**Files:**
- Modify: `examples/demo-kit/Cargo.toml` (bin target, serde_json, tempfile)
- Create: `examples/demo-kit/src/bin/demo-tour.rs`
- Modify: `run.sh`, `run.ps1`
- Modify: `examples/demos/AGENTS.md` (Tour section)
- Modify: `src/cli/skills/ryo-engine-dev.md`, `.claude/skills/ryo-engine-dev/SKILL.md` (CRLF)

**Interfaces:**
- Consumes: the startup line `[demo] <title>: <m> pages` (or `1 page`) and the exit-code-2 contract from Task 3; `RYO_HEADLESS`, `RYO_CAPTURE_FRAMES`, `RYO_CAPTURE_EXIT`, `RYO_CAPTURE_DIR` from the engine; capture files are named `frame-NNNNNN.png` (6 digits).
- Produces: `cargo run -p demo-kit --bin demo-tour -- [demo ...]`; `./run demos:tour [demo ...]`.

Planning decision: the spec puts the tour behind `./run demos:tour` in both scripts. The logic lives in one Rust binary that both scripts call, so there is one tested implementation instead of a bash and a PowerShell copy.

- [ ] **Step 1: Write the failing tests** at the end of `src/bin/demo-tour.rs` (create the file with the functions below as stubs returning `None`/`Ok(())`/empty first):

```rust
#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn page_count_comes_from_the_startup_line() {
    assert_eq!(parse_page_count("noise\n[demo] Bloom: 6 pages\n"), Some(6));
    assert_eq!(parse_page_count("[demo] Fog: 1 page"), Some(1));
    assert_eq!(parse_page_count("[demo] RYO_DEMO_PAGE=7, but Bloom has 6 pages"), None);
    assert_eq!(parse_page_count("Particle System Demo"), None);
  }

  #[test]
  fn runs_are_judged_on_panics_exit_and_frame() {
    assert_eq!(judge(Exit::Code(0), "fine", true), Ok(()));
    assert!(judge(Exit::Code(0), "thread 'main' panicked at x.rs:1", true)
      .is_err_and(|e| e.contains("panicked")));
    assert!(judge(Exit::Code(1), "", true).is_err_and(|e| e.contains("code 1")));
    assert!(judge(Exit::TimedOut, "", false).is_err_and(|e| e.contains("timed out")));
    assert!(judge(Exit::Signal, "", true).is_err());
    assert!(judge(Exit::Code(0), "", false).is_err_and(|e| e.contains("no frame")));
  }

  #[test]
  fn demo_dirs_are_sorted_and_filtered() {
    let root = tempfile::tempdir().unwrap();
    for name in ["bloom", "trails", "notes"] {
      std::fs::create_dir(root.path().join(name)).unwrap();
    }
    std::fs::write(root.path().join("bloom/Cargo.toml"), "").unwrap();
    std::fs::write(root.path().join("trails/Cargo.toml"), "").unwrap();

    let all = demo_dirs(root.path(), &[]).unwrap();
    let names: Vec<_> = all.iter().map(|d| d.file_name().unwrap().to_owned()).collect();
    assert_eq!(names, ["bloom", "trails"]);

    let one = demo_dirs(root.path(), &["trails".to_string()]).unwrap();
    assert_eq!(one.len(), 1);
    assert!(demo_dirs(root.path(), &["nope".to_string()]).is_err());
  }

  #[test]
  fn the_binary_is_found_from_cargo_metadata() {
    let dir = Path::new("/repo/examples/demos/bloom");
    let metadata = serde_json::json!({
      "packages": [
        { "name": "other", "manifest_path": "/repo/Cargo.toml",
          "targets": [{ "name": "other", "kind": ["lib"] }] },
        { "name": "bloom-demo", "manifest_path": "/repo/examples/demos/bloom/Cargo.toml",
          "targets": [{ "name": "bloom-demo", "kind": ["bin"] }] }
      ]
    });
    assert_eq!(
      bin_for(&metadata, dir),
      Some(("bloom-demo".to_string(), "bloom-demo".to_string()))
    );
    assert_eq!(bin_for(&metadata, Path::new("/repo/examples/demos/fog")), None);
  }
}
```

Cargo additions (`examples/demo-kit/Cargo.toml`):

```toml
[dependencies]
ryo2d = { path = "../.." }
serde_json = "1"

[dev-dependencies]
tempfile = "3"

[[bin]]
name = "demo-tour"
path = "src/bin/demo-tour.rs"
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p demo-kit --bin demo-tour`
Expected: assertion failures from the stubs.

- [ ] **Step 3: Implement** — `src/bin/demo-tour.rs` above the tests:

```rust
//! Capture every page of every demo without a window.
//!
//! `cargo run -p demo-kit --bin demo-tour -- [demo ...]` (or
//! `./run demos:tour [demo ...]`) builds the demos, runs each page with
//! `RYO_HEADLESS=1` and `RYO_DEMO_PAGE`, and saves
//! `target/demo-tour/<demo>/page-<n>.png`. Exits non-zero if any page
//! panics, times out or captures nothing.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

const CAPTURE_FRAME: u32 = 90;
const TIMEOUT: Duration = Duration::from_secs(60);

struct DemoBin {
  name: String,
  dir: PathBuf,
  package: String,
  bin: String,
}

/// How a page's run ended
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Exit {
  Code(i32),
  Signal,
  TimedOut,
}

fn main() -> ExitCode {
  let wanted: Vec<String> = std::env::args().skip(1).collect();
  match tour(&wanted) {
    Ok(true) => ExitCode::SUCCESS,
    Ok(false) => ExitCode::FAILURE,
    Err(error) => {
      eprintln!("demo-tour: {error}");
      ExitCode::FAILURE
    }
  }
}

fn tour(wanted: &[String]) -> Result<bool, String> {
  let root = fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
    .map_err(|e| format!("cannot find the repository root: {e}"))?;
  let metadata = cargo_metadata(&root)?;
  let target_dir = PathBuf::from(
    metadata["target_directory"]
      .as_str()
      .ok_or("cargo metadata has no target_directory")?,
  );

  let mut demos = Vec::new();
  for dir in demo_dirs(&root.join("examples").join("demos"), wanted)? {
    let (package, bin) =
      bin_for(&metadata, &dir).ok_or_else(|| format!("no binary for {}", dir.display()))?;
    let name = dir
      .file_name()
      .and_then(|n| n.to_str())
      .unwrap_or_default()
      .to_string();
    demos.push(DemoBin {
      name,
      dir,
      package,
      bin,
    });
  }
  build(&root, &demos)?;

  let out_root = target_dir.join("demo-tour");
  let mut all_ok = true;
  for demo in &demos {
    let bin = target_dir
      .join("debug")
      .join(format!("{}{}", demo.bin, std::env::consts::EXE_SUFFIX));
    match tour_demo(demo, &bin, &out_root.join(&demo.name)) {
      Ok(summary) => println!("{}: {summary}", demo.name),
      Err(error) => {
        all_ok = false;
        println!("{}: FAILED {error}", demo.name);
      }
    }
  }
  println!("Frames: {}", out_root.display());
  Ok(all_ok)
}

fn cargo_metadata(root: &Path) -> Result<serde_json::Value, String> {
  let output = Command::new("cargo")
    .args(["metadata", "--no-deps", "--format-version", "1"])
    .current_dir(root)
    .output()
    .map_err(|e| format!("cargo metadata: {e}"))?;
  if !output.status.success() {
    return Err(format!(
      "cargo metadata failed: {}",
      String::from_utf8_lossy(&output.stderr)
    ));
  }
  serde_json::from_slice(&output.stdout).map_err(|e| format!("cargo metadata: {e}"))
}

fn build(root: &Path, demos: &[DemoBin]) -> Result<(), String> {
  let mut command = Command::new("cargo");
  command.arg("build").arg("--bins").current_dir(root);
  for demo in demos {
    command.args(["-p", &demo.package]);
  }
  let status = command.status().map_err(|e| format!("cargo build: {e}"))?;
  if status.success() {
    Ok(())
  } else {
    Err("cargo build failed".to_string())
  }
}

/// Demo folders (those with a `Cargo.toml`), sorted; only `wanted` if given
fn demo_dirs(demos_dir: &Path, wanted: &[String]) -> Result<Vec<PathBuf>, String> {
  let entries = fs::read_dir(demos_dir).map_err(|e| format!("{}: {e}", demos_dir.display()))?;
  let mut dirs: Vec<PathBuf> = entries
    .filter_map(|entry| entry.ok())
    .map(|entry| entry.path())
    .filter(|path| path.join("Cargo.toml").is_file())
    .collect();
  dirs.sort();
  if wanted.is_empty() {
    return Ok(dirs);
  }
  wanted
    .iter()
    .map(|name| {
      dirs
        .iter()
        .find(|dir| dir.file_name().is_some_and(|n| n == name.as_str()))
        .cloned()
        .ok_or_else(|| format!("no demo named {name:?} in {}", demos_dir.display()))
    })
    .collect()
}

/// The package and binary target whose manifest is in `dir`
fn bin_for(metadata: &serde_json::Value, dir: &Path) -> Option<(String, String)> {
  metadata["packages"].as_array()?.iter().find_map(|package| {
    let manifest = Path::new(package["manifest_path"].as_str()?);
    if manifest.parent()? != dir {
      return None;
    }
    let bin = package["targets"].as_array()?.iter().find(|target| {
      target["kind"]
        .as_array()
        .is_some_and(|kinds| kinds.iter().any(|kind| kind == "bin"))
    })?;
    Some((
      package["name"].as_str()?.to_string(),
      bin["name"].as_str()?.to_string(),
    ))
  })
}

fn tour_demo(demo: &DemoBin, bin: &Path, out_dir: &Path) -> Result<String, String> {
  let _ = fs::remove_dir_all(out_dir);
  fs::create_dir_all(out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
  let first = run_page(demo, bin, out_dir, 1)?;
  let Some(pages) = parse_page_count(&first) else {
    return Ok("1 capture (not on demo-kit)".to_string());
  };
  for page in 2..=pages {
    run_page(demo, bin, out_dir, page)?;
  }
  Ok(format!(
    "{pages} {} ok",
    if pages == 1 { "page" } else { "pages" }
  ))
}

/// Run one page, keep its frame as `page-<n>.png`, and return its output
fn run_page(demo: &DemoBin, bin: &Path, out_dir: &Path, page: usize) -> Result<String, String> {
  let capture_dir = out_dir.join(format!("page-{page}.capture"));
  let log_path = out_dir.join(format!("page-{page}.log"));
  let log = File::create(&log_path).map_err(|e| format!("{}: {e}", log_path.display()))?;
  let log_err = log.try_clone().map_err(|e| e.to_string())?;
  let mut child = Command::new(bin)
    .current_dir(&demo.dir)
    .env("RYO_HEADLESS", "1")
    .env("RYO_DEMO_PAGE", page.to_string())
    .env("RYO_CAPTURE_FRAMES", CAPTURE_FRAME.to_string())
    .env("RYO_CAPTURE_EXIT", "1")
    .env("RYO_CAPTURE_DIR", &capture_dir)
    .stdout(Stdio::from(log))
    .stderr(Stdio::from(log_err))
    .spawn()
    .map_err(|e| format!("page {page}: {e}"))?;

  let started = Instant::now();
  let exit = loop {
    match child.try_wait() {
      Ok(Some(status)) => break status.code().map_or(Exit::Signal, Exit::Code),
      Ok(None) if started.elapsed() > TIMEOUT => {
        let _ = child.kill();
        let _ = child.wait();
        break Exit::TimedOut;
      }
      Ok(None) => sleep(Duration::from_millis(50)),
      Err(e) => return Err(format!("page {page}: {e}")),
    }
  };

  let output = fs::read_to_string(&log_path).unwrap_or_default();
  let frame = capture_dir.join(format!("frame-{CAPTURE_FRAME:06}.png"));
  judge(exit, &output, frame.is_file())
    .map_err(|e| format!("page {page}: {e} (log: {})", log_path.display()))?;
  fs::rename(&frame, out_dir.join(format!("page-{page}.png")))
    .map_err(|e| format!("page {page}: {e}"))?;
  let _ = fs::remove_dir_all(&capture_dir);
  Ok(output)
}

/// The page count from the kit's `[demo] <title>: <m> pages` line
fn parse_page_count(output: &str) -> Option<usize> {
  output.lines().find_map(|line| {
    let rest = line.trim().strip_prefix("[demo] ")?;
    let (_, count) = rest.rsplit_once(": ")?;
    let number = count
      .strip_suffix(" pages")
      .or_else(|| count.strip_suffix(" page"))?;
    number.parse().ok()
  })
}

/// Whether a page's run succeeded
fn judge(exit: Exit, output: &str, captured: bool) -> Result<(), String> {
  if let Some(line) = output.lines().find(|line| line.contains("panicked")) {
    return Err(format!("panicked: {}", line.trim()));
  }
  match exit {
    Exit::TimedOut => return Err(format!("timed out after {} s", TIMEOUT.as_secs())),
    Exit::Signal => return Err("killed by a signal".to_string()),
    Exit::Code(0) => {}
    Exit::Code(code) => return Err(format!("exited with code {code}")),
  }
  if !captured {
    return Err("no frame captured".to_string());
  }
  Ok(())
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p demo-kit --bin demo-tour`
Expected: 4 passed.

- [ ] **Step 5: Wire up `./run demos:tour`**

`run.sh` (LF): add to the header comment after the `demos:kill-all` line:

```bash
#     ./run.sh demos:tour [demo...] - Capture every page of every demo headlessly (target/demo-tour)
```

and a case before `check)`:

```bash
    demos:tour)
        TOUR_ARGS=()
        for arg in "$@"; do
            if [[ "$arg" != "demos:tour" && "$arg" != -* ]]; then
                TOUR_ARGS+=("$arg")
            fi
        done
        cargo run -q -p demo-kit --bin demo-tour -- "${TOUR_ARGS[@]}"
        ;;
```

and to its `help)` case output the same line as the header comment.

`run.ps1` (check `file run.ps1` first): add to the header comment `#   .\run demos:tour [demo...] - Capture every page of every demo headlessly (target/demo-tour)`, a case before `"check"` in the `switch` that dispatches commands:

```powershell
    "demos:tour" {
        $tourArgs = @()
        if ($Arg2) { $tourArgs += $Arg2 }
        $tourArgs += $args
        cargo run -q -p demo-kit --bin demo-tour -- @tourArgs
        exit $LASTEXITCODE
    }
```

and to its `"help"` case: `Write-Host "  ./run demos:tour [demo...] - Capture every page of every demo headlessly"`. PowerShell is not installed on the development machine; keep the PowerShell case to the lines above.

- [ ] **Step 6: Run the tour for real**

Run: `./run.sh demos:tour bloom trails`
Expected: `bloom: 1 capture (not on demo-kit)` and `trails: 1 capture (not on demo-kit)` (no demo is on the kit yet), exit 0, PNGs in `target/demo-tour/bloom/page-1.png` and `target/demo-tour/trails/page-1.png`. Open one PNG to confirm it is a frame of the demo.

- [ ] **Step 7: Document**

Append to `examples/demos/AGENTS.md`:

```markdown
## Tour

`./run demos:tour [demo...]` (or `cargo run -p demo-kit --bin demo-tour -- [demo...]`) builds the demos and captures every page headlessly to `target/demo-tour/<demo>/page-<n>.png`, with each run's log beside it. A page that panics, times out (60 s) or captures nothing fails the tour. Demos not yet on demo-kit are captured once and reported as `not on demo-kit`. Run it after changing a demo or the renderer, and look at the PNGs.
```

In both copies of `ryo-engine-dev` (CRLF; keep them identical), add after the "Verifying Rendering Changes" section:

```markdown
## Demos

Feature demos follow one standard (shared keys, settings panel, layout) through `examples/demo-kit`; see `examples/demos/AGENTS.md`. `./run demos:tour` captures every page of every demo headlessly to `target/demo-tour/`.
```

- [ ] **Step 8: Commit**

```bash
git add examples/demo-kit run.sh run.ps1 examples/demos/AGENTS.md src/cli/skills/ryo-engine-dev.md .claude/skills/ryo-engine-dev/SKILL.md
git commit -m "feat(demos): capture every demo page with ./run demos:tour" -m "- demo-tour runs each page headlessly with RYO_DEMO_PAGE and saves target/demo-tour/<demo>/page-<n>.png
- Fails on panics, timeouts or missing frames; demos not on demo-kit are captured once"
```

---

### Task 6: Pilot: sprite-shaders

**Files:**
- Modify: `examples/demos/sprite-shaders/Cargo.toml`, `examples/demos/sprite-shaders/main.rs`
- Delete: `examples/demos/sprite-shaders/README.md`

**Interfaces:**
- Consumes: `demo_kit::prelude::*` (`Demo`, `DemoPage`, `Settings`, `Control`, `content_area`).

Read the current `main.rs` first; this task restructures it, keeping what it demonstrates: three sprites (plain, custom WGSL dissolve from `assets/shaders/dissolve.wgsl`, and the chosen built-in effect), ten built-in effects, flash and dissolve triggers on Space.

- [ ] **Step 1: Restructure**

1. `Cargo.toml`: add `demo-kit = { path = "../../demo-kit" }` under `[dependencies]`.
2. `use demo_kit::prelude::*;` replaces `use ryo2d::prelude::*;`.
3. Keep `DemoEffect` (`ALL`, `name`, `convenience_method`); replace `description` texts with key-free ones:
   Outline "Draw a coloured border around the sprite.", Glow "Add a soft halo around the sprite.", Flash "Blend with a solid colour (damage flash).", ColorShift "Shift colours toward a target colour.", Silhouette "Replace every colour (shadow mode).", Pixelate "Group pixels into blocks.", Wave "Wavy distortion over time.", Dissolve "Noise-based pixel dissolve.", Invert "Invert the RGB colours.", Grayscale "Convert to grayscale.".
4. Replace `SpriteShaderDemo` with `EffectsPage` (implement `Default` with today's defaults; `silhouette_alpha` becomes an `f32` 0-1 defaulting to `0.5`; `outline_thickness` and `pixelate_size` become `i32`):

```rust
const COLOR_NAMES: [&str; 6] = ["White", "Red", "Cyan", "Yellow", "Magenta", "Green"];

fn palette(index: usize) -> Color {
  [Color::WHITE, Color::RED, Color::CYAN, Color::YELLOW, Color::MAGENTA, Color::GREEN]
    .get(index)
    .copied()
    .unwrap_or(Color::WHITE)
}

/// Left edges of the plain, custom-shader and effect sprites (96 px wide),
/// left of the settings panel
const SPRITE_X: [f32; 3] = [80.0, 230.0, 380.0];

struct EffectsPage {
  effect: usize,
  color: usize,
  outline_thickness: i32,
  glow_radius: f32,
  glow_intensity: f32,
  flash_intensity: f32,
  color_shift_amount: f32,
  silhouette_alpha: f32,
  pixelate_size: i32,
  wave_amplitude: f32,
  wave_speed: f32,
  dissolve_progress: f32,
  grayscale_intensity: f32,
  time: f32,
  flash_timer: f32,
  dissolve_animating: bool,
  normal_sprite: Option<Entity>,
  effect_sprite: Option<Entity>,
  custom_sprite: Option<Entity>,
}
```

Settings mapping (the value Up/Down adjusted, with today's range, then the value `C` changed):

```rust
  fn settings(&mut self, s: &mut Settings) {
    let names: Vec<&str> = DemoEffect::ALL.iter().map(|e| e.name()).collect();
    s.choice("Effect", &mut self.effect, &names);
    match self.current() {
      DemoEffect::Outline => {
        s.int("Thickness", &mut self.outline_thickness, 1..=8);
        s.choice("Color", &mut self.color, &COLOR_NAMES);
      }
      DemoEffect::Glow => {
        s.float("Radius", &mut self.glow_radius, 1.0..=32.0, 1.0);
        s.choice("Color", &mut self.color, &COLOR_NAMES);
      }
      DemoEffect::Flash => {
        s.float("Intensity", &mut self.flash_intensity, 0.0..=1.0, 0.1);
        s.choice("Color", &mut self.color, &COLOR_NAMES);
      }
      DemoEffect::ColorShift => {
        s.float("Amount", &mut self.color_shift_amount, 0.0..=1.0, 0.1);
        s.choice("Color", &mut self.color, &COLOR_NAMES);
      }
      DemoEffect::Silhouette => {
        s.float("Alpha", &mut self.silhouette_alpha, 0.0..=1.0, 0.1);
        s.choice("Color", &mut self.color, &COLOR_NAMES);
      }
      DemoEffect::Pixelate => s.int("Block size", &mut self.pixelate_size, 2..=16),
      DemoEffect::Wave => {
        s.float("Amplitude", &mut self.wave_amplitude, 0.5..=10.0, 0.5);
        s.float("Speed", &mut self.wave_speed, 0.0..=5.0, 0.5);
      }
      DemoEffect::Dissolve => s.float("Progress", &mut self.dissolve_progress, 0.0..=1.0, 0.1),
      DemoEffect::Invert => {}
      DemoEffect::Grayscale => s.float("Intensity", &mut self.grayscale_intensity, 0.0..=1.0, 0.1),
    }
  }

  fn controls(&self) -> Vec<Control> {
    match self.current() {
      DemoEffect::Flash => vec![Control::key(KeyCode::Space, "Flash")],
      DemoEffect::Dissolve => vec![Control::key(KeyCode::Space, "Animate the dissolve")],
      _ => Vec::new(),
    }
  }
```

   Add `fn current(&self) -> DemoEffect { DemoEffect::ALL[self.effect.min(DemoEffect::ALL.len() - 1)] }` on `EffectsPage`.
   `name()` is `"Effects"`; `description()` is `"Plain sprite (left), custom WGSL dissolve shader (middle), the chosen built-in effect (right)."`.
5. `enter` is today's `on_enter`, with the three sprites placed at `Transform::new(SPRITE_X[i], cy)` instead of `cx - cx / 2.0`, `cx`, `cx + cx / 2.0`.
6. `update` keeps today's animation code in this order: `self.time += dt`; flash timer; dissolve animation; custom-shader `amount` animation; then `self.update_effect_sprite(ctx.world)` every update (settings may change any frame); then the wave-time block (it must run after `update_effect_sprite`, which rebuilds the effect); then `if ctx.input.key_pressed(KeyCode::Space) { self.trigger_action(); }`. Delete navigation, `nav_cooldown`, Up/Down, `C`, `R` and `Escape` handling, `adjust_param`, `adjust_secondary_param`, `reset_params` (the shell's `R` rebuilds the page) and `effect_colors`/`current_color_index` (use `palette(self.color)`; silhouette alpha is `(self.silhouette_alpha * 255.0).round() as u8`).
7. `render_ui` draws only, at `let area = content_area(ui);`: the effect's description at `(area.x, area.y)`, `Code: <convenience_method>` one line below, and the three sprite labels centred under `SPRITE_X` (today's `label` helper with `label(SPRITE_X[i], 432.0)`). Remove the title, effect counter, current values, colour line and controls reminder.
8. `main`:

```rust
fn main() {
  Demo::new("Sprite Shaders")
    .resolution(800, 600)
    .page(EffectsPage::default)
    .run();
}
```

9. Delete `README.md` (its content is in the description and help now).

- [ ] **Step 2: Build and check warnings**

Run: `cargo build -p sprite-shaders-demo && cargo clippy -p sprite-shaders-demo`
Expected: builds; no warnings from `examples/demos/sprite-shaders`.

- [ ] **Step 3: Tour and look**

Run: `./run.sh demos:tour sprite-shaders`
Expected: `sprite-shaders: 1 page ok`. Open `target/demo-tour/sprite-shaders/page-1.png`: top bar `Sprite Shaders`, the description line, three sprites left of the settings panel, panel rows `Effect Outline`, `Thickness 2`, `Color White`, legend `R reset`, no overlaps.

Then drive the panel and check the effect changes:

```bash
cd examples/demos/sprite-shaders && RYO_HEADLESS=1 RYO_INPUT="20:ArrowRight 40:ArrowDown 50:ArrowRight 60:ArrowRight" RYO_CAPTURE_FRAMES=90 RYO_CAPTURE_EXIT=1 RYO_CAPTURE_DIR=/tmp/ss-check ../../../target/debug/sprite-shaders-demo
```

Expected: the frame shows `Effect Glow`, `Radius 10` and a glow on the right sprite. Use a scratch directory of your own instead of `/tmp/ss-check` if your environment provides one.

- [ ] **Step 4: Commit**

```bash
git add examples/demos/sprite-shaders
git commit -m "refactor(demos): move sprite-shaders onto demo-kit" -m "- One Effects page; the effect and its values are settings rows instead of arrow keys and C
- Sprites sit left of the settings panel; the README's content moves into the page description and help"
```

---

### Task 7: Pilot: particles

**Files:**
- Modify: `examples/demos/particles/Cargo.toml`, `examples/demos/particles/main.rs`

**Interfaces:**
- Consumes: `demo_kit::prelude::*`.

Read the current `main.rs` first. Each of the ten `DemoScene` variants becomes a page built from one page type.

- [ ] **Step 1: Restructure**

1. `Cargo.toml`: add `demo-kit = { path = "../../demo-kit" }`. `use demo_kit::prelude::*;`.
2. Rename `ParticlesDemo` to `ParticlesPage`; `fn new(scene: DemoScene) -> Self` sets `current_scene: scene`. Remove `nav_cooldown`, `clear_scene` (a fresh page and the engine's scene cleanup replace it), and `DemoScene::next`/`prev`/`from_index`/`index` if nothing else uses them. `DemoScene` must derive `Clone, Copy` (add them if missing).
3. `impl DemoPage for ParticlesPage`:
   - `name()` -> `self.current_scene.name()`; `description()` -> `self.current_scene.description()`.
   - `controls()`:

```rust
  fn controls(&self) -> Vec<Control> {
    match self.current_scene {
      DemoScene::Explosions => vec![Control::mouse("Left-click explosion, right-click debris")],
      DemoScene::Impacts => vec![Control::mouse("Left-click sparks, right-click dust")],
      DemoScene::Trails => vec![Control::mouse("Move to steer the comet")],
      _ => Vec::new(),
    }
  }
```

   - `enter` is today's `on_enter` without the `println!` lines.
   - `update` is today's `update` without scene navigation and `Escape`, returning nothing.
   - `render_gpu` unchanged.
   - `render_ui` keeps only the particle count, drawn at the top right of `content_area(ui)` (`area.x + area.width - text width`, `area.y`), and the attractor labels. Remove the scene title, description, navigation hint and the per-scene hint lines (they are the description and legend now).
4. `main`:

```rust
fn main() {
  let mut demo = Demo::new("Particles")
    .resolution(1280, 720)
    .configure(|b| b.target_fps(144).physics_hz(144).vsync(false));
  for scene in DemoScene::ALL {
    demo = demo.page(move || ParticlesPage::new(scene));
  }
  demo.run();
}
```

- [ ] **Step 2: Build and check warnings**

Run: `cargo build -p particles-demo && cargo clippy -p particles-demo`
Expected: builds; no warnings from `examples/demos/particles`.

- [ ] **Step 3: Tour and look**

Run: `./run.sh demos:tour particles`
Expected: `particles: 10 pages ok`. Open the ten PNGs: each shows `Particles - N/10 <name>`, its description, its particles, and a legend (`Mouse ...` on Explosions, Impacts and Trails). Explosions may show few particles at frame 90 (its first bursts come after 2.5 s); that is expected.

- [ ] **Step 4: Commit**

```bash
git add examples/demos/particles
git commit -m "refactor(demos): move particles onto demo-kit" -m "- Each of the ten scenes is a page; navigation, Escape and the per-scene hint text move to the shell
- Mouse interactions are listed in the legend"
```

---

### Task 8: Pilot: ui

**Files:**
- Modify: `examples/demos/ui/Cargo.toml`, `examples/demos/ui/src/main.rs`, `examples/demos/ui/src/scenes/mod.rs`, and `theme_demo.rs`, `layout_demo.rs`, `clipping_demo.rs`, `text_demo.rs`, `text_input_demo.rs`, `scroll_view_demo.rs`, `dropdown_demo.rs` in `examples/demos/ui/src/scenes/`
- Delete: `examples/demos/ui/src/scenes/main_menu.rs`

**Interfaces:**
- Consumes: `demo_kit::prelude::*`; Task 1's `wants_keyboard` (the shell relies on it for the text input and dropdown pages).

Read each scene file first. The menu goes away; the seven sub-demos become seven pages in the menu's order.

- [ ] **Step 1: Restructure**

1. `Cargo.toml`: add `demo-kit = { path = "../../demo-kit" }`; each file `use demo_kit::prelude::*;` in place of `use ryo2d::prelude::*;`.
2. `main.rs`:

```rust
//! UI Demo - Ryo2D's immediate-mode UI, one feature per page

mod scenes;

use demo_kit::prelude::*;
use scenes::{
  ClippingDemo, DropdownDemo, LayoutDemo, ScrollViewDemo, TextDemo, TextInputDemo, ThemeDemo,
};

fn main() {
  Demo::new("UI")
    .resolution(1024, 768)
    .configure(|b| b.window_mode(WindowMode::ResizableAspectRatio))
    .page(ThemeDemo::new)
    .page(LayoutDemo::new)
    .page(ClippingDemo::new)
    .page(TextDemo::new)
    .page(TextInputDemo::new)
    .page(ScrollViewDemo::new)
    .page(DropdownDemo::new)
    .run();
}
```

3. Delete `main_menu.rs` and its `mod`/`pub use` lines in `mod.rs`.
4. In every scene: `impl Scene for X` becomes `impl DemoPage for X`; `on_enter` becomes `enter`; `update(...) -> SceneAction` becomes `update(...)` with no return value; `render_ui(...) -> SceneAction` becomes `render_ui(...)` with no return value; delete every `Escape` / `SceneAction::Pop` branch. Add `name` and `description` from the old menu:

| Page | `name()` | `description()` |
|---|---|---|
| `ThemeDemo` | `Widgets & Themes` | `Buttons, sliders, toggles and a hot-reloaded theme (assets/ui/theme.json).` |
| `LayoutDemo` | `Layout` | `HBox, VBox and Stack positioning.` |
| `ClippingDemo` | `Clipping & Modals` | `Scissor clipping for scroll regions, and modal overlays.` |
| `TextDemo` | `Text Wrapping` | `Word wrap and text layout.` |
| `TextInputDemo` | `Text Input` | `Editable text fields with a cursor and selection.` |
| `ScrollViewDemo` | `Scroll Views` | `Scrollable content in both directions.` |
| `DropdownDemo` | `Dropdowns` | `Pick from a list of options with mouse or keyboard.` |

5. Remove each page's own title line and any "Press ESC ..." or navigation hint the shell now shows. If a page's content starts above `content_area(ui).y`, shift it down by the difference so nothing sits under the top bar or description.
6. Per-page keys:
   - `ThemeDemo`: keep `T` and `I`; `controls()` -> `[Control::key(KeyCode::KeyT, "Next theme variant"), Control::key(KeyCode::KeyI, "Show or hide instructions")]`.
   - `TextDemo`: replace Up/Down (sample) and held Left/Right (wrap width) with settings: `s.choice("Sample", &mut self.sample_index, &names)` where `names` are the samples' existing labels, and `s.int("Wrap width", &mut self.wrap_width, MIN..=MAX)` using the bounds the arrow code enforces today (store `wrap_width` as `i32`, converting where it is used).
   - `ClippingDemo`: delete the arrow-key scrolling (mouse wheel scrolling stays). The modal closes with its own button; `Esc` is the shell's.
   - `TextInputDemo`: keep `Enter` submit and Tab navigation; `controls()` -> `[Control::key(KeyCode::Enter, "Submit the username"), Control::key(KeyCode::Tab, "Next field")]`.
   - `ScrollViewDemo`: replace `+`/`-` (Equal/NumpadAdd, Minus/NumpadSubtract) with `s.int("Items", &mut self.item_count, MIN..=MAX)` using today's bounds (store as `i32`); keep `Home`/`End`; `controls()` -> `[Control::key(KeyCode::Home, "Scroll to the top"), Control::key(KeyCode::End, "Scroll to the bottom")]`.
   - `DropdownDemo`: `controls()` -> `[Control::key(KeyCode::Tab, "Next dropdown"), Control::mouse("Click a dropdown to open it")]`.
   - `LayoutDemo`: no controls.

- [ ] **Step 2: Build and check warnings**

Run: `cargo build -p ui && cargo clippy -p ui`
Expected: builds; no warnings from `examples/demos/ui`.

- [ ] **Step 3: Tour and look**

Run: `./run.sh demos:tour ui`
Expected: `ui: 7 pages ok`. Open the PNGs: every page has the shell's top bar `UI - N/7 <name>` and description, its content below them, no overlapping text; Text Wrapping and Scroll Views show their settings panels.

Check typing does not trigger shell keys: on the Text Input page, click the username field and type `hr[` with `RYO_INPUT`:

```bash
cd examples/demos/ui && RYO_HEADLESS=1 RYO_DEMO_PAGE=5 RYO_INPUT="20:Click@<x>x<y> 30:KeyH 35:KeyR 40:BracketRight" RYO_CAPTURE_FRAMES=60 RYO_CAPTURE_EXIT=1 RYO_CAPTURE_DIR=<scratch dir> ../../../target/debug/ui
```

with `<x>x<y>` the username field's centre (from the page-5 PNG). Expected: the frame is still page 5 (`UI - 5/7 Text Input`), the help overlay is closed, and the field contains `hr[`.

Finally run the whole tour: `./run.sh demos:tour`. Expected: `sprite-shaders: 1 page ok`, `particles: 10 pages ok`, `ui: 7 pages ok`, every other demo `1 capture (not on demo-kit)`, exit 0.

- [ ] **Step 4: Commit**

```bash
git add -A examples/demos/ui
git commit -m "refactor(demos): move the UI demo onto demo-kit" -m "- The seven sub-demos are pages; the mouse-only menu and per-page Escape handling go
- Text sample, wrap width and item count become settings; the shell steps aside while typing"
```
