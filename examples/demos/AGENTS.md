# AGENTS.md - Feature Demos

Every demo in this folder follows one standard, provided by `examples/demo-kit`. Spec: `docs/specs/2026-10-11-demo-standard-design.md`. Games in `examples/games` keep their own UX.

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
- A page with its own use for Esc right now (closing its dialog) returns true from `DemoPage::wants_escape`: the shell then leaves Esc to the page's `update` instead of quitting (open help still takes it first), drops `Esc quit` from the top bar, and lets the page list Esc in its controls. The ui demo's Clipping & Modals page does this.
- The shell handles its keys in the scene's `ui` (the UI pass, once a frame before the fixed updates) and consumes the ones it acts on, so neither the page's widgets nor its `update` see them. The arrows are consumed while held, not only on the press.
- While help is open the page's `update` sees no input (`Input::block_all`) and its widgets take no clicks or hover (`UiContext::set_mouse_occluded`); a pointer over the settings panel never reaches the page's `update` (`Input::block_pointer`). Both follow what this frame's UI pass drew. The page keeps animating.

## Writing a demo

```rust
use demo_kit::prelude::*;

fn main() {
  Demo::new("Bloom")
    .resolution(960, 640)
    .configure(|b| b.fixed_hz(144)) // any other EngineBuilder setting
    .page(Glow::default)
    .run();
}

#[derive(Default)]
struct Glow {
  intensity: f32,
  bursts: u32,
}

impl DemoPage for Glow {
  fn name(&self) -> &str {
    "Glow"
  }
  fn description(&self) -> &str {
    "Bright pixels bleed light into their neighbours."
  }
  fn settings(&mut self, s: &mut Settings) {
    s.float("Intensity", &mut self.intensity, 0.0..=5.0, 0.1);
  }
  fn controls(&self) -> Vec<Control> {
    vec![Control::key(KeyCode::Space, "Burst")]
  }
  fn enter(&mut self, ctx: &mut SceneContext) {
    let sprite = SpriteHandle::from_sprite(Sprite::solid_rect(32, 32, 255, 240, 200, 255), "glow");
    ctx.world.spawn((Transform::from_xy(480.0, 320.0), Renderable::sprite(sprite)));
  }
  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    ctx.effects().set_bloom(self.intensity);
    if ctx.input.key_pressed(KeyCode::Space) {
      self.bursts += 1;
    }
  }
  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    let area = content_area(ui);
    ui.draw_text(&format!("Bursts: {}", self.bursts), area.x, area.y, Color::WHITE);
  }
}
```

- Add `demo-kit = { path = "../../demo-kit" }` to the demo's `Cargo.toml`; a demo of an optional crate also depends on it (`ryo2d-procgen = { path = "../../../crates/ryo2d-procgen" }`) and imports its prelude beside `demo_kit::prelude::*`.
- Each page is built fresh from its factory when shown or reset, inside its own scene whose cleanup is `SceneCleanup::ALL`: leaving it despawns the page's entities and resets the camera, post-processing effects, lighting settings (ambient, atmosphere, forced on/off), the game clock (speed, pause, hitstop), music and message buffers. Anything else a page changes (cursor, UI scale, render pipeline passes, tilemaps, sound effects, resources it inserted) carries over unless it undoes it in `DemoPage::exit`, or every page sets it in `enter`. Keep feature code plain ryo2d.

| Hook | When | Use |
|---|---|---|
| `enter(ctx)` / `exit(ctx)` | page shown / left or reset | spawn, configure camera, lighting, effects; undo what cleanup does not |
| `settings(s)` | every frame, before `update` | declare the panel's rows; rows may change with other values |
| `update(ctx, dt)` | every fixed step (`dt` is the timestep) | gameplay: read settings fields and `ctx.input` |
| `ui(ui, ctx)` | once a frame, in the UI pass before the fixed steps | page UI under the shell's; read keys for it with `ui.input()`, and `ui.consume_key(key)` keys `update` must not also see |
| `render(gpu)` | once a frame | GPU batches: `particles.collect_for_render(gpu.world, gpu.camera, gpu.batch)` for a page-owned `ParticleSystem` (updated in `update`) |

- `Settings`: `float(label, &mut f32, range, step)`, `int(label, &mut i32, range)`, `choice(label, &mut usize, &[&str])`, `toggle(label, &mut bool)`, `angle(label, &mut f32, step)` (degrees, wrapping round past 0 and 360).
- `Control::key(KeyCode, desc)`, `Control::keys("WASD", &[..], desc)`, `Control::mouse(desc)`.
- Draw page UI inside `content_area(ui)`: below the top bar and description, above the legend. The settings panel covers the right of it when the page has settings, so keep important content left of about 55% of the width on those pages (`settings_panel_height(ui, rows)` says how far down it reaches).
- The fixed rate defaults to 60 Hz; set another with `.configure(|b| b.fixed_hz(..))`.
- Headless tests build the engine from the demo: `HeadlessEngine::new_or_skip(demo.engine_builder())` (see `examples/demo-kit/tests/shell.rs`).

## Layout

Top bar (title, `N/M page`, shell keys), one description line, the settings panel in a right column, and a right-aligned legend at the bottom. The bottom-left corner stays free for the engine's FPS counter.

## Scripting

- `RYO_DEMO_PAGE=<n>` (dev builds, 1-based) starts on page n; an out-of-range page exits with code 2.
- The demo logs `[demo] <title>: <m> pages` at startup.
- Combine with `RYO_HEADLESS=1`, `RYO_INPUT` and `RYO_CAPTURE_*` (see the `ryo-cli` skill).

## Tour

`./dev demos:tour [demo...]` (or `cargo run -p demo-kit --bin demo-tour -- [demo...]`) builds the demos and captures every page headlessly to `target/demo-tour/<demo>/page-<n>.png`, with each run's log beside it. A page that panics, times out (60 s) or captures nothing fails the tour, as does a demo that never prints the kit's `[demo]` startup line; every page still runs, so one tour lists every failure. `RYO_*` variables in your shell are not passed on to the demos. Run it after changing a demo or the renderer, and look at the PNGs.
