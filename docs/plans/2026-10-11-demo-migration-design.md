# Demo Migration - Design

## Overview

Sub-project 2 of the demo standard (`docs/plans/2026-10-11-demo-standard-design.md`): move the 31 feature demos that are not yet on `examples/demo-kit` onto it, so every demo in `examples/demos` shares the same controls, layout and settings panel. The kit, its rules and the three pilots (sprite-shaders, particles, ui) are done; this spec decides each remaining demo's pages, settings and keys. The five games are out of scope.

Owner decisions (2026-10-11): the cross-cutting rules below as written; the console demo's Hub and Workshop become pages and its `game:scene1`/`game:scene2` commands are dropped (no kit change).

## Rules

1. **Arrows belong to the settings panel.** Movement and panning move to WASD; raycasting turns with Q/E; the inventory moves its selection with W/S. Values the arrows changed become settings.
2. **Number keys.** Modes, presets and variants become `choice` settings. Number keys that trigger actions stay page controls.
3. **Scenes switched by keys or menus become pages**; their scene-local toggles and held tweaks become settings.
4. **Reserved-key clashes.** `Esc` always quits (in-page quit dialogs go); other uses of `H` and `R` move to settings or other keys; `R` only means "reset the page" (the shell's rebuild replaces each demo's own reset).
5. **Flows driven by code** (networking's lobby to match) stay inside one page.
6. **What each demo shows does not change.** Only navigation, settings, keys and layout change. Ranges and steps for new settings are the bounds the old key handlers enforced; where the old code had no bound, the spec gives one.
7. **Page UI** sits inside `content_area(ui)`; important content stays left of the settings panel on pages with settings.
8. **READMEs** (anchor-positioning, hierarchy-sprites, lighting, post-processing) fold into page descriptions and help, then are removed.
9. **Descriptions** say what the page shows in one line; keys the shell cannot list (the engine's backquote console, F9/F10 windows) are named in the description.

## Per-demo mapping

"Settings" lists rows in panel order. "Controls" are the page's legend entries (the shell adds `R reset`). Where a setting replaces a key, the old key goes.

| Demo | Pages | Settings | Controls |
|---|---|---|---|
| anchor-positioning | Anchors | Offset X, Offset Y (float, -200..=200, step 2); Info panel (toggle, was Space) | — |
| asset-loading | Loading | — | Mouse: Load / Reset buttons |
| bloom | Bloom | Preset (choice: Off, Subtle, Dreamy, Neon, Explosive, Custom); Intensity, Radius, Threshold (float); Blur passes (int 1..=5). Changing a value switches Preset to Custom, as today | — |
| camera-shake | Trauma | Curve (choice: Linear, InQuad, InCubic); Max offset (float); Decay (float) | 1 / 2 / 3: add trauma 0.2 / 0.5 / 0.8; Space: add 1.0 |
| console | Hub; Workshop | — | Description names the backquote console and the commands (`game:hello` on both pages, `game:paint <color>` on Workshop) |
| controller | Gamepad | — | Gamepad: buttons and sticks |
| cursor | Cursor | Visible (toggle, was V); Confined (toggle, was C) | Mouse: hover the buttons to change the cursor, click the area for sparks |
| dpi-scaling | UI Scale | UI scale (choice: 1x, 2x, 3x, Auto) | — |
| hierarchy-sprites | Solar System | Rotation (toggle, was Space) | — |
| inventory | Inventory | — | 1-8: select slot; W / S: move selection (was Up/Down); Space: equip or unequip; E: equip; D: drop; P: pick up; L: roll loot; U + 1-3: unequip slot |
| knockback | Knockback | Force (float) | 1-4: knock a column; Space: radial; A (hold): rapid fire |
| lighting | Pendulum | Intensity, Radius, Ambient darkness (float; were held arrows and Q/E) | Space: reset the pendulum |
| lighting-volumetric | Volumetric | Atmosphere (choice: Clear, Hazy, Dusty, Foggy, Smoky, Underwater); God rays (toggle, was Space); God ray intensity (float); Light colour (choice, was Tab); Spot direction, Cone angle, Spot position (float; were Q/E, W/S, A/D) | — |
| networking | Netplay (lobby and match in one page) | — | Mouse: lobby buttons and fields; A / D: move; Space or W: jump; L: leave the match (was the Esc dialog) |
| pathfinding | Agents | Smoothing (choice: None, Line of sight, Spline); Avoidance, Grid, Trails (toggles; were A, G, T) | Space: add 10 agents; Mouse: left click sets the destination, right drag adds obstacles, Shift + right click removes |
| post-processing | Effects | Effect (choice: None, Fade, Sepia, Vignette, Screen flash, CRT, Chromatic aberration, Custom scanlines); then the chosen effect's values (Fade amount; Sepia intensity; Vignette radius and softness; Chromatic strength; Scanline strength) | Space: flash |
| procgen | Generators | Algorithm (choice: BSP, Cellular, Noise, Random walk, Poisson, WFC); Grid (toggle, was G) | Space: new seed; WASD: pan (was arrows) |
| profile-and-log | Load | Work multiplier (choice: x0.25, x0.5, x1, x2, x4, x8; was 7/8); Auto-spawn (toggle, was Space) | 1 / 2: add / remove CPU entities; 3 / 4: GPU; 5 / 6: mixed. Description names F9 (logger) and F10 (profiler) |
| raycasting | Maze | — | W / S: move; A / D: strafe; Q / E: turn (were arrows); F: open a door (was E); Mouse: look |
| shadows | Hard; Drop; Blob; Soft; Colored; Contact; Ambient Occlusion; Ray-marched | Hard: Spotlight (toggle, was Space), Spot rotation, Cone (float; were Q/E, W/S). Drop: Style (choice: Default, Below, Diagonal, Squashed, Custom, was Tab), Animation (toggle, was Space), and while Custom: Offset X/Y, Alpha, Scale X/Y (float). Blob: Height scaling (toggle, was H). Soft: Softness, Light radius (float; the Z/X/C presets go). Colored: Shadow colour (choice, was Tab), and while Custom: R, G, B (int 0..=255). Contact: Radius, Intensity, Falloff (float; the Z/X/C presets go). Ambient Occlusion: AO (toggle, was A), Intensity, Radius, Bias (float; were I/K, R/F, B/V). Ray-marched: Samples (int 4..=64) | Hard, Soft, Colored, Ray-marched: Mouse moves the light. Hard, Ray-marched: left click places a box, right click removes one. Blob: Space: jump |
| skeleton | Fighter; Casual; Action RPG | — | Fighter: A / D walk, J punch, K kick, L (hold) block. Casual: A / D walk, 1 wave, 2 jump, 3 dance. Action RPG: A / D run, J attack, K dodge, V victory |
| squash-stretch | Bounce | Intensity (choice of today's three levels, were 1/2/3); Recovery (choice of today's three speeds, were Q/W/E) | Space: drop the balls; 1-5: squash a ball (was Shift + 1-5) |
| state-machine | State Machines | — | A / D: walk (Shift: run); Space: jump; S (hold): crouch; X: attack; P: pause; I: inventory; O: options (Shift + O: video); Backspace: pop; Home: back to the root |
| steering | Boids | Separation, Cohesion, Alignment (float; were 1-9); Trails (toggle, was T) | Space: add 10 boids; Mouse (hold left): predator |
| tilemap | Tilemap | — | WASD: pan (was arrows); Mouse: hover for tile info, left click places, right click clears |
| time-control | Time | Time scale (choice: 1.0x, 0.3x, 0.1x, 2.0x, Paused; were 1-5) | Space: 100 ms hitstop; J: 50 ms hitstop (was H) |
| trails | Presets; Fade Modes; Styles; Caps & Joins; Textured; GPU Effects | Presets: Preset (choice: Bullet, Laser, Swipe, Lightning, Fire, Energy; were 1-6). Fade Modes: Lifetime, Distance (float; were Up/Down). Caps & Joins: Cap, Join (choice; were C, J) | Space: pause (Presets, Fade Modes, Styles, Textured, GPU Effects); Mouse: the cursor trail |
| tweening | Easing; Speed Control; Callbacks; Combined Effects; Sequences; Colour & Alpha; Property Tweens | Easing: Loop mode (choice: Once, Loop, PingPong, Times(3); were Q/W/E/R). Speed Control: Speed (choice: 0.25x, 0.5x, 1x, 2x, 4x; were Q-T), Paused (toggle, was Space) | Space: restart (every page but Speed Control) |
| ui-animations | Easing Showcase; Property Animations; Interactive | Easing Showcase: Auto-restart (toggle, was A) | Easing Showcase, Property Animations: Space restart. Interactive: Mouse |
| validated-animator | Animator | — | 1-5: try a transition (Idle, Walk, Attack, Hurt, Death); Space: force Idle |
| y-sorting | Y-Sort | Depth scaling (toggle, was Space) | WASD: move the player (was arrows) |

Resolutions and engine settings (`physics_hz`, `target_fps`, `vsync`, window modes) carry over through `Demo::resolution` and `configure`.

## Verification

- Each demo builds with no warnings in its own files and its pages pass `./run demos:tour <demo>`.
- Every page's frame is looked at: shell bar, description and legend present; no page text under the shell's bar or behind the settings panel; the page still shows its feature.
- One `RYO_INPUT` run per demo changes a setting or presses a page control and shows the effect.
- At the end, `./run demos:tour` reports every demo on the kit (`N pages ok`), and no demo `not on demo-kit`.

## Out of scope

The games; new demo features; the deferred minor items of sub-project 1 except where a migration touches the same code.
