# Demo Standard - Design

## Overview

Every feature demo in `examples/demos` shares one set of controls, one screen layout and one settings panel, provided by a small `demo-kit` crate. Owner decisions (2026-10-11): all four goals matter (same controls everywhere, clean reference code, scriptable verification, polished showcase); demos only, the five games keep their own UX; a shell plus a settings panel, not a full framework; the shell lives in a shared examples crate, not the engine.

**Why:** the 34 demos each invent their own keys and layout. Pages switch with arrows in some, digits or Tab in others; four menus are mouse-only; help text overlaps content and the FPS counter; scene-switch digits shadow preset digits (shadows); and verifying a demo means reading its code to find its keys.

**This spec is sub-project 1:** the standard, `demo-kit`, one engine addition (`UiContext::wants_keyboard`), a tour command, docs, and three pilot demos. Sub-project 2 migrates the other 31 demos with its own short spec (a per-demo table of pages, settings and keys) once the pilots have proven the kit.

**Out of scope:** the games; a single gallery binary; mouse interaction with the settings panel; changing what any demo demonstrates.

## The control standard

| Key | Meaning in every demo |
|---|---|
| `[` / `]` (aliases PageUp / PageDown) | Previous / next page, wrapping |
| Up / Down | Select a value in the settings panel |
| Left / Right | Change the selected value (hold to repeat) |
| `R` | Reset the page: values back to defaults, scene rebuilt |
| `H` | Toggle the help overlay |
| `Esc` | Close help if open, otherwise quit |
| `Space` | The page's main action, if it has one (by convention, not owned by the shell) |
| WASD / mouse | Movement, camera and pointing, when a page needs them (by convention) |

- The arrow keys belong to the panel on every page. Demos that move with arrows today (raycasting, y-sorting, tilemap, procgen, anchor-positioning) move to WASD in sub-project 2.
- Digits, other letters and Tab stay with the page and are listed in its legend. Tab stays free for UI focus navigation.
- The shell never uses the engine's keys: F1-F5, F9, F10, F12 and the console's backquote.
- While `UiContext::wants_keyboard()` was true on the previous frame, the shell ignores all of its keys, `Esc` included, so typing into a text field never opens help, resets, changes pages or quits; `Esc` then goes to the widget.
- Presets and variants become `choice` settings (cycled with Left/Right) rather than number keys. Mouse-only menus become pages.

## Screen layout

```
+------------------------------------------------------+
| Bloom - 3/6 Dreamy        [ ] page - H help - Esc quit|  top bar
| Mid threshold, high radius. Soft ethereal look.       |  description (one line)
|                                     +---------------+ |
|                                     | SETTINGS      | |
|        page content (world drawn    |> Intensity 1.2| |  settings panel (right column,
|        full screen underneath)      |  Radius    12 | |  translucent, hidden when the
|                                     |  Preset Dreamy| |  page has no settings)
|                                     +---------------+ |
| (FPS)      Space trigger - R reset - Mouse move light |  legend, right-aligned
+------------------------------------------------------+
```

- Top bar: demo title, `N/M page name` (just the title for single-page demos), and the shell keys on the right (`[ ] page` omitted for single-page demos).
- The description is the first line of the page's description; help shows all of it.
- The legend lists the page's controls plus `R reset`, right-aligned so the bottom-left stays clear for the engine's FPS overlay.
- Help overlay: dims the screen and lists the full description, the shell keys, the page's controls and every setting with its range.
- The shell draws after the page's own UI, using the current UI scale and screen size, so it works from 800x600 to 1920x1080 and under the dpi-scaling demo's scale changes.
- Window title: `Ryo2D - <demo title>`.

## demo-kit

A workspace crate at `examples/demo-kit` (package `demo-kit`), depended on by demos via `demo-kit = { path = "../../demo-kit" }`. Not part of the engine's public API.

```rust
use demo_kit::prelude::*; // re-exports ryo2d::prelude plus the kit

fn main() {
  Demo::new("Bloom")
    .resolution(960, 640)
    .configure(|builder| builder.physics_hz(144)) // optional EngineBuilder access
    .page(Glow::default) // a factory: every visit and every reset starts fresh
    .page(Stacked::default)
    .run();
}

#[derive(Default)]
struct Glow {
  preset: usize,
  intensity: f32,
  passes: i32,
}

impl DemoPage for Glow {
  fn name(&self) -> &str { "Glow" }
  fn description(&self) -> &str { "Bright pixels bleed light into their neighbours." }
  fn settings(&mut self, s: &mut Settings) {
    s.choice("Preset", &mut self.preset, &["Subtle", "Dreamy", "Neon"]);
    s.float("Intensity", &mut self.intensity, 0.0..=5.0, 0.1);
    s.int("Passes", &mut self.passes, 1..=5);
  }
  fn controls(&self) -> Vec<Control> {
    vec![Control::key(KeyCode::Space, "Burst"), Control::mouse("Move light")]
  }
  fn enter(&mut self, ctx: &mut SceneContext) { /* plain ryo2d */ }
  fn update(&mut self, ctx: &mut SceneContext, dt: f32) { /* plain ryo2d */ }
}
```

- `DemoPage` methods: `name`, `description` (default empty), `settings` (default none), `controls` (default none), `enter`, `update`, and optional `render_ui(&mut self, ui: &mut UiContext)` and `render_gpu(&mut self, ctx: &mut GpuRenderContext)`, mirroring `Scene`.
- `Settings` is immediate mode, called every frame before `update`: `float(label, &mut f32, range, step)`, `int(label, &mut i32, range)`, `choice(label, &mut usize, &[&str])` (wraps), `toggle(label, &mut bool)` (Left/Right flip it). The rows may differ from frame to frame (e.g. by the chosen variant); the selection clamps to the rows present. Values are clamped to their ranges.
- Holding Left/Right repeats after 0.35 s at 12 steps per second, timed with real (unscaled) time so time-scale demos do not slow the panel.
- `Control::key(KeyCode, desc)`, `Control::keys(label, &[KeyCode], desc)` (e.g. `"WASD"`), `Control::mouse(desc)`. Key names in the legend come from the `KeyCode` (`Space`, `Q`, `1`).
- `Demo::run` panics before opening a window if any page's controls name a reserved key (`[`, `]`, PageUp, PageDown, the arrows, `H`, `R`, `Esc`, F1-F5, F9, F10, F12, backquote), naming the page and the key.

### Lifecycle

- Each page runs inside a `PageScene` (an engine `Scene`) that holds the shared demo definition (title, page factories), the page index, the live page and the panel state.
- Changing page or pressing `R` returns `SceneAction::Switch` to a new `PageScene` with a freshly built page. The engine's default `SceneCleanup` then resets the world, camera, effects, lighting, time, events and raycasting, so pages cannot leak state into each other.
- Per update: the shell consumes its key presses (`key_pressed_and_consume`) unless `wants_keyboard` was set on the previous frame; it applies panel input to the rows recorded on the previous frame; it calls `page.settings` then `page.update`.
- Per UI frame: `page.render_ui`, then the shell's bar, panel, legend and help; it records `ui.wants_keyboard()` for the next update.
- `Esc` closes help if open, otherwise returns `SceneAction::Quit`.

### Starting page

- `RYO_DEMO_PAGE=<n>` (1-based, dev builds) starts on page n. A value that does not parse is warned about and ignored.
- An out-of-range page prints `[demo] RYO_DEMO_PAGE=<n>, but <title> has <m> pages` and exits with code 2 before building the engine.
- At startup the kit logs `[demo] <title>: <m> pages` (dev builds), which the tour reads.

## Engine addition: `UiContext::wants_keyboard`

- `UiContext::wants_keyboard() -> bool` is true when, this frame, a focused `text_input` was drawn or a dropdown is open (a merely focused dropdown does not count).
- Cleared at the start of each UI frame, set by `text_input` when its state is focused. Documented in the `ryo-ui` and `ryo-api` skills as the way for any scene to stay out of the way of typing.

## Tour command

`./dev demos:tour [demo...]` in `dev.sh` and `dev.ps1`:

- Builds the demo binaries, then for each demo (all of `examples/demos`, or the ones named) runs page 1, 2, ... with `RYO_HEADLESS=1 RYO_DEMO_PAGE=<n> RYO_CAPTURE_FRAMES=90 RYO_CAPTURE_EXIT=1 RYO_CAPTURE_DIR=target/demo-tour/<demo>` from the demo's folder.
- The kit logs `[demo] <title>: <m> pages` at startup; the tour reads the page count from page 1's run. A demo whose page 1 run has no such line is not on the kit yet: it is captured once and marked `not on demo-kit`.
- A run that panics, times out (60 s) or writes no PNG is a failure; frames are renamed `page-<n>.png`.
- Prints one line per demo (`bloom: 6 pages ok`) and exits non-zero if any page failed.

## Pilot demos

| Demo | Pages | Settings | Page controls |
|---|---|---|---|
| sprite-shaders | 1 "Effects" | Effect (choice of the 10 effects); then the chosen effect's value that Up/Down adjusts today, with today's range, and the one `C` changes today (a colour choice or a second value) | Space trigger (flash, dissolve animation) |
| particles | 10, one per scene (Fire, Smoke, Explosions, Magic, Weather, Impacts, Celebration, Trails, Attractors, Combined) | none | Mouse click / right-click / movement where a page uses it today |
| ui | 7, one per sub-demo (Widgets & Themes, Layout, Clipping, Text Wrapping, Text Input, Scroll Views, Dropdowns); the menu goes away | Text Wrapping: Sample (choice), Wrap width (int); Scroll Views: Item count (int) | Theme: T variant, I instructions; Scroll Views: Home/End |

Each pilot keeps what it demonstrates; only navigation, settings and layout change. Keys that collide with the standard move (e.g. the text demo's arrows become settings; the clipping demo's arrow scrolling becomes mouse-wheel only).

## Documentation

- `examples/demos/AGENTS.md` (new): the standard, the layout, how to write a page, and the tour.
- `ryo-engine-dev` skill: a short "Demos" section pointing to it and to `./dev demos:tour`.
- `ryo-ui` and `ryo-api` skills: `wants_keyboard`.
- `examples/demos/sprite-shaders/README.md`: its content moves into the page description and help, and the README is removed. The other demo READMEs are handled in sub-project 2.

## Testing

- **demo-kit (headless, in the crate's tests):** `]`/`[` and PageUp/PageDown change pages and wrap; `R` rebuilds the page with default values; Up/Down select rows and clamp to the rows present; Left/Right change floats and ints within their ranges, cycle choices and flip toggles; holding repeats after the delay; `H` toggles help and `Esc` closes help before quitting; with `wants_keyboard` set, `H`, `R`, `[`, `]` and the arrows do nothing; a reserved key in a page's controls panics with the page and key named; `RYO_DEMO_PAGE` parsing (valid, invalid, out of range).
- **Engine:** `wants_keyboard` is false with no widgets, true while a focused text input is drawn, true while a dropdown is open, false for a focused but closed dropdown, and false again the frame after focus is lost.
- **Tour:** run on the three pilots and every other demo; the pilots' pages all render without panic and the others are reported as `not on demo-kit`. Reviewed by eye from the PNGs.

## Design decisions

- **Shared crate, not engine API:** the shell can evolve with the demos without public-API cost; each page's feature code is still plain `ryo2d`.
- **Pages are scenes rebuilt from factories:** reuses the engine's scene cleanup, so isolation between pages is guaranteed rather than hand-maintained; `R` is the same mechanism.
- **Immediate-mode settings:** matches the engine's immediate UI and Rust borrowing; values stay ordinary fields that `update` reads.
- **Reserved keys are few and checked:** `[ ] H R Esc` plus the arrows; a page that claims one fails at startup, not in a user's hands.
- **One run per page for the tour:** independent of each demo's timing and cooldowns, and page counts are discovered rather than maintained.
