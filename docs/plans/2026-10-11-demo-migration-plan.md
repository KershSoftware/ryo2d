# Demo Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move the 31 feature demos not yet on `examples/demo-kit` onto it, so every demo shares the same controls, layout and settings panel.

**Architecture:** Each demo becomes one `Demo` with one or more `DemoPage`s. Each page's feature code stays plain ryo2d; the demo's own navigation, `Esc` handling, titles, key hints and per-demo reset go to the shell. Values once changed with arrows or number keys become `Settings` rows. Demos are grouped into seven batches of similar shape; each batch is verified with the tour, by looking at every page, and with one `RYO_INPUT` run per demo.

**Tech Stack:** Rust 2024, ryo2d, `examples/demo-kit` (`Demo`, `DemoPage`, `Settings`, `Control`, `content_area`), `./run demos:tour`, `RYO_HEADLESS`, `RYO_INPUT`, `RYO_CAPTURE_*`, `RYO_DEMO_PAGE`.

**Spec:** `docs/plans/2026-10-11-demo-migration-design.md` (its per-demo table is binding), building on `docs/plans/2026-10-11-demo-standard-design.md`.

## Global Constraints

- The spec's per-demo table decides each demo's pages, settings (names, kinds, ranges) and controls. Where it says "today's" bounds or levels, read them from the old key-handling code.
- What each demo shows does not change; only navigation, settings, keys and layout.
- Pages never read the reserved keys for their own purposes: `[`, `]`, PageUp, PageDown, the four arrows, `H`, `R`, `Esc`, F1-F5, F9, F10, F12, backquote. `Demo::run` panics if a page's controls list one.
- 2-space indentation, edition 2024; keep your hunks formatted, do not reformat unrelated code. Run `file <path>` before editing; preserve CRLF files byte-for-byte. New files are LF.
- No `unwrap`/`expect` added outside tests.
- Commit on `main`, format `type: brief description` plus bullets; no trailer lines of any kind; no test-status lines. If `git commit` is refused by a permission check, stop and report BLOCKED with the error; never ask anyone else to commit for you.
- Each demo's `Cargo.toml` gains `demo-kit = { path = "../../demo-kit" }`; its sources use `use demo_kit::prelude::*;` in place of `use ryo2d::prelude::*;` (keeping both warns).

## Migration recipe (every task)

Worked examples to read first: `examples/demos/sprite-shaders/main.rs` (one page, settings that depend on a choice), `examples/demos/particles/main.rs` (one page type built per scene), `examples/demos/ui/src` (several scene types as pages), and `examples/demos/AGENTS.md`.

For each demo in the task:

1. **Map.** Read the demo's current code and its row in the spec table. Note every key it reads and what replaces it.
2. **Pages.** Each page becomes a type implementing `DemoPage`: `on_enter` becomes `enter`; `update(..) -> SceneAction` becomes `update(..)` with no return; `render_ui(..) -> SceneAction` becomes `render_ui(..)` with no return; `render_gpu` and `render` carry over. A page type used for several pages takes its variant in `new(..)` and is registered once per variant (`demo.page(move || Page::new(v))`). Code that switched scenes on keys, cooldowns for that switching, `Esc`/quit handling, the demo's own reset key and reset functions, title lines, page counters and key-hint text all go.
3. **Settings.** Add `fn settings(&mut self, s: &mut Settings)` with the spec's rows, in order, bound to the fields the old keys changed. `int` takes `&mut i32` (convert field types where needed). `update` applies the current values every frame (or whenever they differ from what was last applied).
4. **Controls.** `fn controls(&self) -> Vec<Control>` returns the spec's legend entries: `Control::key(KeyCode::X, "...")`, `Control::keys("WASD", &[KeyCode::KeyW, KeyCode::KeyA, KeyCode::KeyS, KeyCode::KeyD], "...")`, `Control::mouse("...")`. Rebind keys the spec moves (arrows to WASD, H to J, E to F, ...).
5. **Global state.** Engine state that `SceneCleanup` does not reset (UI scale, cursor visibility and grab, window requests, render-pipeline passes) is set from the page's settings in `enter` and kept in step in `update`, so `R` and page changes restore the defaults.
6. **Layout.** Page text and widgets sit inside `content_area(ui)`; nothing under the shell's top bar, description line or legend, and nothing important behind the settings panel on pages with settings.
7. **main.** `Demo::new("<Title>").resolution(w, h)` plus `.configure(|b| ...)` for any other builder setting the demo used (`target_fps`, `physics_hz`, `vsync`, `window_mode`, ...), then `.page(...)` per page, then `.run()`.
8. **README.** If the demo has a `README.md`, its content goes into the description and help (`description()` may be several lines; the first is shown under the bar) and the file is deleted.
9. **Doc comment.** The module doc says what the demo shows and that it runs on the demo-kit shell; no stale key lists.

Verification for each demo (results go in the report):

- `cargo build -p <package>` and `cargo clippy -p <package>`: no warnings in the demo's files.
- `grep -n "KeyCode::\(ArrowUp\|ArrowDown\|ArrowLeft\|ArrowRight\|KeyH\|KeyR\|Escape\|BracketLeft\|BracketRight\|PageUp\|PageDown\|Backquote\)" <demo sources>` finds nothing outside comments.
- `./run.sh demos:tour <demo-dir-name>` prints `<demo>: N page(s) ok` with N as in the spec; look at every `target/demo-tour/<demo>/page-N.png` (top bar, description, legend, settings panel where the spec has settings, no overlaps, the feature visible). Pages driven by the mouse may look empty with the mouse at (0, 0); use the `RYO_INPUT` run to see them.
- One `RYO_INPUT` run per demo (from the demo's folder, into a scratch directory) that changes a setting or presses a page control, with a capture showing the effect, e.g.
  `RYO_HEADLESS=1 RYO_DEMO_PAGE=1 RYO_INPUT="20:ArrowDown 30:ArrowRight" RYO_CAPTURE_FRAMES=60 RYO_CAPTURE_EXIT=1 RYO_CAPTURE_DIR=<scratch> ../../../target/debug/<bin>`.
  `N:Mouse@XxY` and `N:Click@XxY` drive the mouse in game coordinates.

## Review Focus

1. A page that still reads a reserved key (an arrow, `H`, `R`, `Esc`) for its own purpose acts twice per press. Check: each task's grep step finds nothing.
2. Engine state outside `SceneCleanup` (UI scale in dpi-scaling, cursor visibility and confinement in cursor) must return to the page's defaults after `R`. Test: Task 1 (dpi-scaling) and Task 3 (cursor) each include an `RYO_INPUT` run that changes the value, presses `R`, and shows the default restored.
3. Shell keys and settings must keep working while the game is paused or slowed (time-control's Paused). Test: Task 1 includes an `RYO_INPUT` run that selects Paused, then 1.0x, and shows the balls moving again.
4. Text fields must still receive typing (networking's lobby). Test: Task 7 checks that with the room-code field focused, `[`, `]`, `H` and `R` do nothing.
5. A page that sets up its world for a specific camera or screen origin must still look right after `R` (fresh scene, default camera). Test: every task's tour pass plus one `R` press in each `RYO_INPUT` run on a page with a camera set in `enter`.

---

### Task 1: Small single-page demos

**Files:** `Cargo.toml` and sources of `examples/demos/anchor-positioning`, `asset-loading`, `controller`, `dpi-scaling`, `hierarchy-sprites`, `time-control`, `validated-animator`, `y-sorting`; delete `anchor-positioning/README.md` and `hierarchy-sprites/README.md`.

**Interfaces:**
- Consumes: `demo_kit::prelude::{Demo, DemoPage, Settings, Control, content_area}`; `Settings::{float, int, choice, toggle}`.

Per demo (from the spec table):

| Demo | Pages | Settings | Controls |
|---|---|---|---|
| anchor-positioning | Anchors | Offset X, Offset Y (float, -200..=200, step 2); Info panel (toggle, was Space) | — |
| asset-loading | Loading | — | `Control::mouse("Load / Reset buttons")` |
| controller | Gamepad | — | `Control::keys("Gamepad", &[], "Buttons and sticks")` |
| dpi-scaling | UI Scale | UI scale (choice: 1x, 2x, 3x, Auto) | — |
| hierarchy-sprites | Solar System | Rotation (toggle, was Space) | — |
| time-control | Time | Time scale (choice: 1.0x, 0.3x, 0.1x, 2.0x, Paused; were 1-5) | Space: 100 ms hitstop; J: 50 ms hitstop (was H) |
| validated-animator | Animator | — | 1-5: try a transition (Idle, Walk, Attack, Hurt, Death); Space: force Idle |
| y-sorting | Y-Sort | Depth scaling (toggle, was Space) | WASD: move the player (was arrows) |

- [ ] **Step 1: Migrate** each demo with the Migration recipe. dpi-scaling applies the chosen UI scale in `enter` and whenever the setting changes (Auto = the engine's automatic scale as today). time-control applies the chosen time scale every update.
- [ ] **Step 2: Verify** each demo with the recipe's checks. Extra checks:
  - dpi-scaling: `RYO_INPUT="20:ArrowRight 40:KeyR"` with captures at 30 and 60: frame 30 at 2x, frame 60 back at 1x.
  - time-control: `RYO_INPUT="20:ArrowLeft 60:ArrowRight"` (1.0x -> Paused -> 1.0x) with captures at 40, 55 and 100: balls identical at 40 and 55, moved by 100.
- [ ] **Step 3: Commit**

```bash
git add examples/demos/anchor-positioning examples/demos/asset-loading examples/demos/controller examples/demos/dpi-scaling examples/demos/hierarchy-sprites examples/demos/time-control examples/demos/validated-animator examples/demos/y-sorting
git commit -m "refactor(demos): move eight single-page demos onto demo-kit" -m "- anchor-positioning, asset-loading, controller, dpi-scaling, hierarchy-sprites, time-control, validated-animator, y-sorting
- Arrow and number-key values become settings; y-sorting moves with WASD; time-control's 50 ms hitstop moves to J"
```

---

### Task 2: Effects and lighting demos

**Files:** `Cargo.toml` and sources of `examples/demos/bloom`, `post-processing`, `camera-shake`, `lighting`, `lighting-volumetric`; delete `post-processing/README.md` and `lighting/README.md`.

**Interfaces:**
- Consumes: `demo_kit::prelude::{Demo, DemoPage, Settings, Control, content_area}`.

| Demo | Pages | Settings | Controls |
|---|---|---|---|
| bloom | Bloom | Preset (choice: Off, Subtle, Dreamy, Neon, Explosive, Custom); Intensity, Radius, Threshold (float); Blur passes (int 1..=5). Changing a value switches Preset to Custom, as today | — |
| post-processing | Effects | Effect (choice: None, Fade, Sepia, Vignette, Screen flash, CRT, Chromatic aberration, Custom scanlines); then the chosen effect's values: Fade amount; Sepia intensity; Vignette radius and softness; Chromatic strength; Scanline strength | Space: flash |
| camera-shake | Trauma | Curve (choice: Linear, InQuad, InCubic); Max offset (float); Decay (float) | 1 / 2 / 3: add trauma 0.2 / 0.5 / 0.8; Space: add 1.0 |
| lighting | Pendulum | Intensity, Radius, Ambient darkness (float; were held arrows and Q/E) | Space: reset the pendulum |
| lighting-volumetric | Volumetric | Atmosphere (choice: Clear, Hazy, Dusty, Foggy, Smoky, Underwater); God rays (toggle, was Space); God ray intensity (float); Light colour (choice, was Tab); Spot direction, Cone angle, Spot position (float; were Q/E, W/S, A/D) | — |

- [ ] **Step 1: Migrate** each demo with the Migration recipe. Bloom: when a preset is chosen, its values are copied into the Intensity/Radius/Threshold/Blur-passes fields; when any of those changes while a preset other than Custom is chosen, the preset becomes Custom (keep the previous values to detect the change). Post-processing follows sprite-shaders' pattern (rows depend on the chosen effect).
- [ ] **Step 2: Verify** with the recipe's checks. Extra check for bloom: `RYO_INPUT="20:ArrowRight 30:ArrowDown 40:ArrowRight"` (Preset -> Subtle, then Intensity up) with a capture at 60 shows Preset `Custom`.
- [ ] **Step 3: Commit**

```bash
git add examples/demos/bloom examples/demos/post-processing examples/demos/camera-shake examples/demos/lighting examples/demos/lighting-volumetric
git commit -m "refactor(demos): move the effects and lighting demos onto demo-kit" -m "- bloom, post-processing, camera-shake, lighting, lighting-volumetric
- Presets, effects and atmospheres become choice settings; held-key tweaks become settings rows"
```

---

### Task 3: Interactive single-page demos

**Files:** `Cargo.toml` and sources of `examples/demos/cursor`, `inventory`, `knockback`, `profile-and-log`, `squash-stretch`, `state-machine`, `steering`, `pathfinding`.

**Interfaces:**
- Consumes: `demo_kit::prelude::{Demo, DemoPage, Settings, Control, content_area}`.

| Demo | Pages | Settings | Controls |
|---|---|---|---|
| cursor | Cursor | Visible (toggle, was V); Confined (toggle, was C) | Mouse: hover the buttons to change the cursor, click the area for sparks |
| inventory | Inventory | — | 1-8: select slot; W / S: move selection (was Up/Down); Space: equip or unequip; E: equip; D: drop; P: pick up; L: roll loot; U + 1-3: unequip slot |
| knockback | Knockback | Force (float) | 1-4: knock a column; Space: radial; A (hold): rapid fire |
| profile-and-log | Load | Work multiplier (choice: x0.25, x0.5, x1, x2, x4, x8; was 7/8); Auto-spawn (toggle, was Space) | 1 / 2: add / remove CPU entities; 3 / 4: GPU; 5 / 6: mixed. Description names F9 (logger) and F10 (profiler) |
| squash-stretch | Bounce | Intensity (choice of today's three levels, were 1/2/3); Recovery (choice of today's three speeds, were Q/W/E) | Space: drop the balls; 1-5: squash a ball (was Shift + 1-5) |
| state-machine | State Machines | — | A / D: walk (Shift: run); Space: jump; S (hold): crouch; X: attack; P: pause; I: inventory; O: options (Shift + O: video); Backspace: pop; Home: back to the root |
| steering | Boids | Separation, Cohesion, Alignment (float; were 1-9); Trails (toggle, was T) | Space: add 10 boids; Mouse (hold left): predator |
| pathfinding | Agents | Smoothing (choice: None, Line of sight, Spline); Avoidance, Grid, Trails (toggles; were A, G, T) | Space: add 10 agents; Mouse: left click sets the destination, right drag adds obstacles, Shift + right click removes |

- [ ] **Step 1: Migrate** each demo with the Migration recipe. Cursor applies Visible and Confined in `enter` and when they change (and shows the cursor and releases confinement on defaults), so `R` restores them. Profile-and-log's work multiplier keeps today's starting value as the default choice.
- [ ] **Step 2: Verify** with the recipe's checks. Extra check for cursor: `RYO_INPUT="20:ArrowRight 40:KeyR"` and confirm from the frames (status text) that Visible is Off after frame 20 and On again after `R`.
- [ ] **Step 3: Commit**

```bash
git add examples/demos/cursor examples/demos/inventory examples/demos/knockback examples/demos/profile-and-log examples/demos/squash-stretch examples/demos/state-machine examples/demos/steering examples/demos/pathfinding
git commit -m "refactor(demos): move the interactive single-page demos onto demo-kit" -m "- cursor, inventory, knockback, profile-and-log, squash-stretch, state-machine, steering, pathfinding
- Toggles and weights become settings; inventory selection moves to W/S; ball squashes use plain 1-5"
```

---

### Task 4: Demos that move with the keyboard

**Files:** `Cargo.toml` and sources of `examples/demos/procgen`, `raycasting`, `tilemap`.

**Interfaces:**
- Consumes: `demo_kit::prelude::{Demo, DemoPage, Settings, Control, content_area}`.

| Demo | Pages | Settings | Controls |
|---|---|---|---|
| procgen | Generators | Algorithm (choice: BSP, Cellular, Noise, Random walk, Poisson, WFC); Grid (toggle, was G) | Space: new seed; WASD: pan (was arrows) |
| raycasting | Maze | — | W / S: move; A / D: strafe; Q / E: turn (were arrows); F: open a door (was E); Mouse: look |
| tilemap | Tilemap | — | WASD: pan (was arrows); Mouse: hover for tile info, left click places, right click clears |

- [ ] **Step 1: Migrate** each demo with the Migration recipe. Procgen regenerates when the Algorithm setting changes (as the digit keys did).
- [ ] **Step 2: Verify** with the recipe's checks. Extra checks: raycasting `RYO_INPUT="20-60:KeyW 60-80:KeyQ"` shows the view moved and turned; procgen `RYO_INPUT="20:ArrowRight"` shows the Cellular map.
- [ ] **Step 3: Commit**

```bash
git add examples/demos/procgen examples/demos/raycasting examples/demos/tilemap
git commit -m "refactor(demos): move procgen, raycasting and tilemap onto demo-kit" -m "- Panning and movement use WASD; raycasting turns with Q/E and opens doors with F
- Procgen's algorithm and grid become settings"
```

---

### Task 5: Multi-page demos

**Files:** `Cargo.toml` and sources of `examples/demos/skeleton`, `tweening`, `trails`, `ui-animations` (delete ui-animations' main-menu scene).

**Interfaces:**
- Consumes: `demo_kit::prelude::{Demo, DemoPage, Settings, Control, content_area}`.

| Demo | Pages | Settings | Controls |
|---|---|---|---|
| skeleton | Fighter; Casual; Action RPG | — | Fighter: A / D walk, J punch, K kick, L (hold) block. Casual: A / D walk, 1 wave, 2 jump, 3 dance. Action RPG: A / D run, J attack, K dodge, V victory |
| tweening | Easing; Speed Control; Callbacks; Combined Effects; Sequences; Colour & Alpha; Property Tweens | Easing: Loop mode (choice: Once, Loop, PingPong, Times(3); were Q/W/E/R). Speed Control: Speed (choice: 0.25x, 0.5x, 1x, 2x, 4x; were Q-T), Paused (toggle, was Space) | Space: restart (every page but Speed Control) |
| trails | Presets; Fade Modes; Styles; Caps & Joins; Textured; GPU Effects | Presets: Preset (choice: Bullet, Laser, Swipe, Lightning, Fire, Energy; were 1-6). Fade Modes: Lifetime, Distance (float; were Up/Down). Caps & Joins: Cap, Join (choice; were C, J) | Space: pause (Presets, Fade Modes, Styles, Textured, GPU Effects); Mouse: the cursor trail |
| ui-animations | Easing Showcase; Property Animations; Interactive | Easing Showcase: Auto-restart (toggle, was A) | Easing Showcase, Property Animations: Space restart. Interactive: Mouse |

- [ ] **Step 1: Migrate** each demo with the Migration recipe. Skeleton's shared scene code (`scenes/mod.rs` base) stays shared between the three page types. Tweening's seven scene types each become a page type. Trails' page enum becomes one page type built per variant (as particles does). ui-animations' menu goes; its three scenes become pages and lose their `Esc`/Backspace pop.
- [ ] **Step 2: Verify** with the recipe's checks (tours: skeleton 3 pages, tweening 7, trails 6, ui-animations 3). Extra check: trails `RYO_DEMO_PAGE=4 RYO_INPUT="20:ArrowRight"` shows the next cap style.
- [ ] **Step 3: Commit**

```bash
git add examples/demos/skeleton examples/demos/tweening examples/demos/trails examples/demos/ui-animations
git commit -m "refactor(demos): move skeleton, tweening, trails and ui-animations onto demo-kit" -m "- Their scenes and pages become demo-kit pages; ui-animations' mouse-only menu goes
- Loop modes, speeds, presets, fade values, caps and joins become settings"
```

---

### Task 6: Shadows

**Files:** `examples/demos/shadows/Cargo.toml`, `main.rs`, `scenes/*.rs`.

**Interfaces:**
- Consumes: `demo_kit::prelude::{Demo, DemoPage, Settings, Control, content_area}`.

Eight pages, one per scene, in this order: Hard, Drop, Blob, Soft, Colored, Contact, Ambient Occlusion, Ray-marched. Remove every scene's digit scene-switching, its `Esc` handling, its `[1] Hard [2] Drop ...` header line and its own control text.

| Page | Settings | Controls |
|---|---|---|
| Hard | Spotlight (toggle, was Space); Spot rotation, Cone (float; were Q/E, W/S) | Mouse: moves the light; left click places a box, right click removes one |
| Drop | Style (choice: Default, Below, Diagonal, Squashed, Custom; was Tab); Animation (toggle, was Space); while Custom: Offset X, Offset Y, Alpha, Scale X, Scale Y (float; were A/D, W/S, Q/E, Z/C, X/V) | — |
| Blob | Height scaling (toggle, was H) | Space: jump |
| Soft | Softness, Light radius (float; were W/S, Q/E; the Z/X/C presets go) | Mouse: moves the light |
| Colored | Shadow colour (choice of today's presets, was Tab); while Custom: R, G, B (int 0..=255; were Q/A, W/S, E/D) | Mouse: moves the light |
| Contact | Radius, Intensity, Falloff (float; were Q/A, W/S, E/D; the Z/X/C presets go) | — |
| Ambient Occlusion | AO (toggle, was A); Intensity, Radius, Bias (float; were I/K, R/F, B/V) | — |
| Ray-marched | Samples (int 4..=64; were W/S) | Mouse: moves the light; left click places a box, right click removes one |

- [ ] **Step 1: Migrate** with the Migration recipe. The light markers added recently (they follow the light) stay.
- [ ] **Step 2: Verify** with the recipe's checks: `./run.sh demos:tour shadows` prints `shadows: 8 pages ok`. Mouse-lit pages look dark with the mouse at (0, 0); for Hard, Soft, Colored and Ray-marched run `RYO_DEMO_PAGE=<n> RYO_INPUT="5:Mouse@460x300"` and look at frame 60. Extra check: Ambient Occlusion `RYO_INPUT="20:ArrowRight"` turns AO off (frame visibly flatter).
- [ ] **Step 3: Commit**

```bash
git add examples/demos/shadows
git commit -m "refactor(demos): move the shadows demo onto demo-kit" -m "- Its eight scenes become pages; per-scene held keys and presets become settings
- The light-following mouse and box placement stay as page controls"
```

---

### Task 7: Console and networking, and the full tour

**Files:** `examples/demos/console/Cargo.toml`, `main.rs`; `examples/demos/networking/Cargo.toml`, `main.rs`, `src/**`.

**Interfaces:**
- Consumes: `demo_kit::prelude::{Demo, DemoPage, Settings, Control, content_area}`.

| Demo | Pages | Settings | Controls |
|---|---|---|---|
| console | Hub; Workshop | — | Description names the backquote console and the commands (`game:hello` on both pages, `game:paint <color>` on Workshop) |
| networking | Netplay (lobby and match in one page) | — | Mouse: lobby buttons and fields; A / D: move; Space or W: jump; L: leave the match (was the Esc dialog) |

- [ ] **Step 1: Migrate** with the Migration recipe.
  - Console: `HubScene` and `WorkshopScene` become pages; the `game:scene1`/`game:scene2` commands and the `SCENE_REQUEST` switch go; each page registers its commands in `enter` as today.
  - Networking: one page type holding today's lobby and match as an internal state (an enum of the two existing scene structs, or equivalent), switching between them where the scenes returned `SceneAction::Switch`. The quit dialog goes; `L` leaves the match (disconnect, back to the lobby). Text fields keep working because the shell steps aside while a field has focus.
- [ ] **Step 2: Verify** with the recipe's checks (console 2 pages, networking 1). Extra checks: networking `RYO_INPUT` clicks the room-code field (coordinates from the page-1 frame), then presses `20:KeyH 25:KeyR 30:BracketRight` (shifted to after the click): the frame still shows the lobby with no help overlay. Console: `RYO_DEMO_PAGE=2` frame shows the Workshop.
- [ ] **Step 3: Full tour.** Run `./run.sh demos:tour`. Expected: every demo `N page(s) ok`, none `not on demo-kit`, exit 0.
- [ ] **Step 4: Commit**

```bash
git add examples/demos/console examples/demos/networking
git commit -m "refactor(demos): move console and networking onto demo-kit" -m "- Console's Hub and Workshop are pages; the scene-switch commands go
- Networking's lobby and match live in one page; L leaves a match instead of the Esc dialog"
```
