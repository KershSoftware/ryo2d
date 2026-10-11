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
- While a text field has focus or a dropdown is open the shell ignores all of its keys. Esc first leaves the field or closes the dropdown; the next Esc quits. Keys the shell acts on are not typed into a field.
- While help is open the page's `update` sees no input, and a pointer over the settings panel never reaches it (`InputState::block_all` / `block_pointer`); the page keeps animating.

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

## Tour

`./run demos:tour [demo...]` (or `cargo run -p demo-kit --bin demo-tour -- [demo...]`) builds the demos and captures every page headlessly to `target/demo-tour/<demo>/page-<n>.png`, with each run's log beside it. A page that panics, times out (60 s) or captures nothing fails the tour. Demos not yet on demo-kit are captured once and reported as `not on demo-kit`. Run it after changing a demo or the renderer, and look at the PNGs.
