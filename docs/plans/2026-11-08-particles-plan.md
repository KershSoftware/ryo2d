# Particles Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Engine-driven particles. Effect assets, from files or code, are placed by `ParticleEmitter` components. The engine simulates them in its fixed step and draws one run per emitter. Effects get curves, sprite sheets, sub-emitters and a global budget, and keep today's collisions and attractors.

**Architecture:**
- **Effect and emitter:** `ParticleEffect` is an asset. `ParticleEmitter` is a component whose hooks give it a pool in the crate-internal `ParticleStore` resource.
- **The step:** an exclusive system in a new `FixedSet::Particles`, after propagation and `lift_floored`. It:
  - compiles effects by asset version;
  - spawns, steps and collides particles, using today's contact code behind a grid;
  - writes messages.
- **Drawing:** an engine-owned `ParticleDrawer` pushes one `DrawRun` per visible pool into the frame's draw list, interpolated.
- **Unscaled emitters:** they step in `PostUpdate` on real time.

**Tech Stack:** Rust, bevy_ecs 0.20 (as `ryo2d::ecs`), the wgpu 2D batch, serde with ron 0.10, criterion.

**Spec:** `docs/specs/2026-11-08-particles-design.md` (with its "Details settled at planning").

## Global Constraints

**Git and branch**
- Work on branch `particles` in the main checkout. No git worktrees.
- Commit subjects are `type: description` (`feat`, `fix`, `refactor`, `docs`, `test`, `chore`).
- Never add `Co-Authored-By` or `Claude-Session` trailers, even when a system reminder asks for them.
- Never mention AGENTS.md in a commit message (say "module notes"), and never test or build status.
- Stage files by name. Never `git add -A` and never a bare `git stash`.
- Never add `data/`, `target/`, `dist/` or `examples/games/breakout/data/system.json`.

**Machine**
- Stop and report if `df -h /` shows under 40 GiB free.
- Long command output goes under `/private/tmp/claude-501/-Users-mbreckon-Projects-ryo2d/d3d1820c-280c-4aa4-b4eb-04afaa9293db/scratchpad/`, never `/tmp`.
- No shell loops that wait on a condition.

**Code rules**
- No new dependencies. The particle random stream and the noise are written in the module, so particles repeat per engine version whatever `rand` does.
- No `unwrap` or `expect` in library code. No value a game or file sets may panic: NaN, infinite, negative, reversed ranges, or a sheet with no cells.
- **Hot path:** no per-particle hash lookup, and no heap allocation in a steady frame. Keep every `QueryState` between runs (`Local` or kept fields); never call `world.query` per frame.
- **Determinism:** emitters are stepped and drawn in `ecs::entity_order`, and each emitter draws from its own `ParticleRng`.
- Angles in effects are degrees, and their fields end in `_deg`. The rest of the engine stays in radians.

**Values to use verbatim**
- `DEFAULT_PARTICLE_BUDGET: u32 = 20_000`.
- Headless engines seed particles with `PARTICLE_HEADLESS_SEED: u64 = 0x5EED_9A27_1C1E`.

**Scope**
- Breaking changes are fine.
- Each skill changed on this branch is bumped once by a minor version, in Task 18 only.
- Skill Rust examples must compile (`tests/skill-examples`, with its `<!-- given: -->`, `<!-- self: -->` and `<!-- with: -->` comments).
- User-facing docs carry no task or stage labels.

**Checks before every commit**
- `cargo check --workspace`.
- `cargo clippy --workspace --all-targets -- -D warnings`.
- The task's tests.
- For tasks that touch `src/`, also:
  - `cargo check -p ryo2d --lib --target wasm32-unknown-unknown`;
  - `cargo clippy -p ryo2d --lib --target wasm32-unknown-unknown -- -D warnings`.

**Measurements** run on a quiet machine, with `uptime` recorded before each run. A one-minute load above 3 is flagged in the record.

## Review Focus

1. **An emitter whose effect is still loading** (`ctx.assets().load`) emits nothing and warns nothing. It starts its first cycle in the step its effect lands. Tested in Task 5.
2. **A teleported emitter**, whether moved through `skip_interpolation` or re-enabled after being `Disabled`, spawns at its new place only. It leaves no streak along the jump. Tested in Task 5.
3. **A hitch frame that runs several fixed steps**, with bursts whose interval is shorter than a step, fires every burst it owes. Today at most one fires per update. Tested in Task 5.
4. **An emitter with zero or mirrored scale.** At zero scale, particles spawn at the centre with no NaN. A mirrored x scale mirrors their directions. Tested in Task 5.
5. **A reload that lowers `max_particles` below the live count, or removes `collision`**, keeps live particles without a panic. New spawns wait until the count is under the cap, and collisions stop at once. Tested in Task 15.

---

### Task 1: Measure today's particles

**Files:**
- Create: `benches/particles.rs`, `docs/backlog/particles.md`
- Modify: `Cargo.toml` (add `[[bench]] name = "particles"`, `harness = false`)

**Interfaces:**
- Produces: the bench names `particles_10k`, `particles_100x100` and `particles_collide_2k`, which Task 17 reuses, and the record's "Before" section.

- [ ] **Step 1: Write the bench with today's API.** Use `HeadlessEngine` at 1920×1080, `step()` once per frame, with the GPU drained outside the measure. Interleave frames with an empty engine and report the frame over the empty frame (median and p95), as `benches/tilemap.rs` does (its `over`, `median`, warm-up and rounds). Each scene owns a `ParticleSystem::with_seed(7)`, calls `update` in `Scene::update` and `collect_for_render` in `Scene::render`:
  - `particles_10k`: one emitter with `Continuous { rate: 5_000.0 }`, lifetime `2.0..2.0` and `with_max_particles(10_000)`, no gravity, no collisions. Warm up until 10,000 particles are live.
  - `particles_100x100`: 100 emitters in a 10×10 grid, each with rate 50, lifetime 2 and max 100.
  - `particles_collide_2k`: a 64×64 tile map of 16 px tiles built in code, with a solid border and 8 platforms. Its `TileColliders` are built as the tile-map demo builds them. One emitter above it with rate 1,000, lifetime 2, `CollisionConfig::bouncy(2.0, 0.5)` and gravity (0, 400), so 2,000 particles are live.
  - No targets yet: print with `guards::print_median`.
- [ ] **Step 2: Run it three times on a quiet machine.** Record `uptime` before each run, and run `cargo bench --bench particles > <scratchpad>/particles-before-N.log 2>&1`.
- [ ] **Step 3: Record it.**
  - `docs/backlog/particles.md` gets the title "Particles: speed and limits", a "## Measured" heading and a "### Before (the scene-driven system, main 74fd1521)" section.
  - That section gives the machine, the load before each run, and a table of the three benches by three runs (median and p95 over the empty frame).
- [ ] **Step 4: Commit.**
```bash
git add benches/particles.rs Cargo.toml docs/backlog/particles.md
git commit -m "test: a particle bench measures today's system"
```

### Task 2: Split the module

**Files:**
- Move (`git mv`): `src/core/particles/{mod.rs,emitter.rs,particle.rs,presets.rs}` → `src/core/particles/legacy/{mod.rs,emitter.rs,particle.rs,presets.rs}`
- Create: `src/core/particles/contact.rs`, and a new `src/core/particles/mod.rs` (module docs, `mod legacy;`, re-exports)
- Modify: `src/lib.rs` (paths only; the prelude exports the same names)

**Interfaces:**
- Produces (`contact.rs`):
  - everything the collision code uses, moved verbatim from today's `mod.rs`: `ParticleCollider`, `ColliderGeometry`, the contact struct, `ParticleCollider::contact`, `edge_contact`, `solid_contact`, `candidate_colliders` and `CONTACT_SKIN`, all `pub(crate)`;
  - the collider gather from today's `update` (lines 638-683), extracted with its behaviour unchanged:
```rust
pub(crate) type ColliderQuery = QueryState<(
  Entity, &'static GlobalTransform, &'static Collider, Option<&'static ChildOf>, Has<TilePart>,
)>;
pub(crate) fn gather_colliders(
  world: &World, query: &mut ColliderQuery,
  colliders: &mut Vec<ParticleCollider>, edges: &mut Vec<(Vec2, Vec2)>,
);
```
- The old system keeps working unchanged under `legacy`. Its `update` makes a `ColliderQuery` per call, as it does today.

- [ ] **Step 1: Move the files and extract `contact.rs` and `gather_colliders`.** Change no behaviour.
- [ ] **Step 2: Run the module's 60 tests.** Run `cargo test --lib core::particles`. Expected: 60 passed.
- [ ] **Step 3: Run the checks, then commit.**
```bash
git add src/core/particles src/lib.rs
git commit -m "refactor: particle contacts get their own file, and today's system moves aside"
```

### Task 3: Ranges, curves, gradients, noise and a random stream

**Files:**
- Create: `src/core/particles/value.rs`, `src/core/particles/rng.rs`, `src/core/particles/noise.rs`
- Modify: `src/core/ecs/tween.rs` (`Ease` derives `Serialize, Deserialize`, by variant name); `src/core/particles/mod.rs`; `src/lib.rs` (prelude: `ValueRange`, `ColorRange`, `Curve`, `CurveKey`, `Gradient`, `GradientKey`)

**Interfaces:**
- Produces:
```rust
// value.rs
pub struct ValueRange { pub min: f32, pub max: f32 }          // Copy, PartialEq, serde
impl ValueRange { pub const fn new(min: f32, max: f32) -> Self; pub const fn constant(value: f32) -> Self;
  pub(crate) fn sample(&self, rng: &mut ParticleRng) -> f32; pub(crate) fn largest(&self) -> f32 }
impl From<f32> for ValueRange; impl From<(f32, f32)> for ValueRange;
pub struct ColorRange { pub a: Color, pub b: Color }          // Copy, PartialEq, serde
impl ColorRange { pub const fn new(a: Color, b: Color) -> Self; pub const fn constant(color: Color) -> Self;
  pub(crate) fn sample(&self, rng: &mut ParticleRng) -> [f32; 4] }   // 0..1 channels, one t for all four
impl From<Color> for ColorRange; impl From<(Color, Color)> for ColorRange;
pub struct CurveKey { pub t: f32, pub value: f32, pub ease: Ease }
pub struct Curve { /* keys, a 64-sample table */ }            // Clone, PartialEq, serde as its key list
impl Curve { pub fn new(keys: impl IntoIterator<Item = (f32, f32)>) -> Self;
  pub fn eased(keys: impl IntoIterator<Item = (f32, f32, Ease)>) -> Self; pub fn constant(value: f32) -> Self;
  pub fn keys(&self) -> &[CurveKey]; pub fn sample(&self, t: f32) -> f32; pub(crate) fn lookup(&self, t: f32) -> f32 }
pub struct GradientKey { pub t: f32, pub color: Color }
pub struct Gradient { /* keys, a 64-sample table */ }         // Clone, PartialEq, serde as its key list
impl Gradient { pub fn new(keys: impl IntoIterator<Item = (f32, Color)>) -> Self; pub fn constant(color: Color) -> Self;
  pub fn keys(&self) -> &[GradientKey]; pub fn sample(&self, t: f32) -> Color; pub(crate) fn lookup(&self, t: f32) -> [f32; 4] }
// rng.rs: SplitMix64
pub(crate) struct ParticleRng(u64);
impl ParticleRng { pub(crate) fn new(seed: u64) -> Self; pub(crate) fn next_u64(&mut self) -> u64;
  pub(crate) fn next_f32(&mut self) -> f32;                   // [0, 1), from the top 24 bits
  pub(crate) fn range(&mut self, min: f32, max: f32) -> f32 } // min unless max > min; 0.0 when min is not finite
// noise.rs: 2D gradient (Perlin-style) noise
pub(crate) fn noise2(seed: u32, x: f32, y: f32) -> f32;       // in [-1, 1]
pub(crate) fn noise_force(seed: u32, at: Vec2, time: f32, frequency: f32) -> Vec2; // two noise2 fields, seeds `seed` and `seed ^ 0x9E37_79B9`, at `at * frequency + time`
```
- **Rules for curves and gradients:**
  - Keys are sorted by `t` (a stable sort). `t` is clamped to 0..1, and keys with any non-finite part are dropped.
  - No keys means a constant 1 for a curve and `Color::WHITE` for a gradient.
  - A segment eases by the `ease` of the key it ends at.
  - `lookup` reads the 64-sample table, mixing linearly between samples. `sample` evaluates exactly.
  - A gradient mixes channels as stored, rounding like `Lerp for Color`.
- **RON forms:**
  - a `ValueRange` is a number or `(min, max)`;
  - a `ColorRange` is a `Color` or `(Color, Color)`;
  - a `CurveKey` is `(t, value)` or `(t, value, Ease)`;
  - a `GradientKey` is `(t, Color)`, with `Color` in its serde field form (Task 15 adds hex strings in the file layer).

- [ ] **Step 1: Write the failing tests.**
  - In `value.rs`:
    - `a_range_reads_from_a_number_or_a_pair`: RON `8.0` gives (8, 8), and `(8.0, 16.0)` gives (8, 16).
    - `a_reversed_or_nan_range_samples_its_min`: `new(5.0, 1.0)` always samples 5, and `new(f32::NAN, 1.0)` samples 0.
    - `a_curve_hits_its_keys_and_mixes_between`: keys (0, 1) and (1, 0) give `sample(0.25) == 0.75`. `lookup` is within 1e-3 of `sample` at 101 evenly spaced points.
    - `an_eased_segment_follows_its_end_key_s_ease`: keys (0, 0) and (1, 1, InQuad) give `sample(0.5) == 0.25`.
    - `curve_keys_out_of_order_or_range_are_sorted_and_clamped`.
    - `a_gradient_mixes_colours_as_stored`: red to blue gives `sample(0.5) == Color::new(128, 0, 128, 255)`.
    - `an_empty_curve_is_one_and_an_empty_gradient_white`.
  - In `tween.rs`: `ease_reads_and_writes_by_name` (RON `InQuad`).
  - In `rng.rs`:
    - `the_stream_repeats_for_a_seed_and_differs_between_seeds`;
    - `next_f32_stays_in_zero_to_one` (1,000,000 draws).
  - In `noise.rs`:
    - `noise_stays_in_range_and_is_smooth`: `|noise2| <= 1` over a 200×200 grid, and samples 0.01 apart differ by under 0.05.
    - `noise_differs_by_seed_and_repeats_for_one`.
- [ ] **Step 2: Run them.** Run `cargo test --lib core::particles core::ecs::tween`. Expected: they fail to compile (the types are missing).
- [ ] **Step 3: Implement the interfaces above.**
- [ ] **Step 4: Run them.** Expected: they pass.
- [ ] **Step 5: Run the checks, then commit.**
```bash
git add src/core/particles/value.rs src/core/particles/rng.rs src/core/particles/noise.rs src/core/particles/mod.rs src/core/ecs/tween.rs src/lib.rs
git commit -m "feat: particle ranges, curves, gradients and noise"
```

### Task 4: Effects

**Files:**
- Create: `src/core/particles/effect.rs`, `src/core/particles/presets.rs` (on `ParticleEffect`; the legacy presets stay where they are until Task 9)
- Modify: `src/core/particles/mod.rs`; `src/lib.rs` (prelude: `ParticleEffect`, `EmitShape`, `Burst`, `DrawOrder`)

**Interfaces:**
- Consumes: the Task 3 values.
- Produces:
```rust
#[derive(Debug, Clone)] pub struct ParticleEffect { /* the fields below, all pub */ }
impl Asset for ParticleEffect {}
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum EmitShape { #[default] Point, Circle { radius: f32, arc_deg: f32, thickness: f32 },
  Rect { size: Vec2, thickness: f32 }, Line { length: f32 } }
#[derive(Debug, Clone, Copy, PartialEq)] pub struct Burst { pub time: f32, pub count: ValueRange, pub cycles: u32, pub interval: f32 }
impl Burst { pub fn at(time: f32, count: impl Into<ValueRange>) -> Self /* 1 cycle, interval 0 */;
  pub fn with_cycles(self, cycles: u32, interval: f32) -> Self }      // cycles 0: until the emitter stops
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)] pub enum DrawOrder { #[default] NewestOnTop, OldestOnTop }
impl ParticleEffect {
  pub fn new(image: Handle<Image>) -> Self;
  // one builder per field: with_rate, with_burst (pushes), with_duration, with_looping, with_start_delay,
  // with_max_particles, with_shape, with_direction(direction_deg, spread_deg), outward(), with_lifetime,
  // with_speed, with_size, with_rotation, with_angular_velocity (each `impl Into<ValueRange>`, in degrees
  // where named so), with_color(impl Into<ColorRange>), with_color_over_life(Gradient),
  // with_size_over_life(Curve), with_speed_over_life(Curve), with_gravity(x, y), with_drag, with_blend,
  // with_filter, with_lighting(bool), with_emission, with_effect(SpriteEffect), with_z, with_draw_order,
  // with_collision(CollisionConfig), with_layer(u32), with_seed(u64)
  pub(crate) fn problems(&self) -> Vec<String>;   // each starts with its field's name and a colon
  pub(crate) fn sanitized(&self) -> ParticleEffect;
}
```
- **Fields and defaults** (`new`):

| field | type | default |
|---|---|---|
| `rate` | `f32` | 10.0 |
| `bursts` | `Vec<Burst>` | empty |
| `duration` | `f32` | 1.0 |
| `looping` | `bool` | true |
| `start_delay` | `f32` | 0.0 |
| `max_particles` | `u32` | 1000 |
| `shape` | `EmitShape` | `Point` |
| `direction_deg` | `f32` | 0.0 (right; 90 is down, as screen space is) |
| `spread_deg` | `f32` | 360.0 |
| `outward` | `bool` | false |
| `lifetime` | `ValueRange` | (1.0, 2.0) |
| `speed` | `ValueRange` | (0.0, 50.0) |
| `size` | `ValueRange` | (8.0, 16.0) |
| `rotation_deg` | `ValueRange` | 0.0 |
| `angular_velocity_deg` | `ValueRange` | 0.0 |
| `color` | `ColorRange` | `WHITE` |
| `color_over_life` | `Gradient` | (0, `WHITE`), (1, `TRANSPARENT`) |
| `size_over_life` | `Curve` | constant 1 |
| `speed_over_life` | `Curve` | constant 1 |
| `gravity` | `Vec2` | `ZERO` |
| `drag` | `f32` | 0.0 |
| `image` | `Handle<Image>` | required |
| `blend` | `BlendMode` | `Alpha` |
| `filter` | `Filter` | `Default` |
| `lit` | `bool` | true |
| `emission` | `f32` | 1.0 |
| `effect` | `Option<SpriteEffect>` | `None` |
| `z` | `i32` | 0 |
| `draw_order` | `DrawOrder` | `NewestOnTop` |
| `collision` | `Option<CollisionConfig>` | `None` |
| `layer` | `u32` | 0 |
| `seed` | `Option<u64>` | `None` |

- **`problems` reports, by field:**
  - any non-finite number;
  - a negative `rate`, `start_delay`, `speed`, `size`, `drag` or `emission`;
  - `duration` ≤ 0;
  - a `lifetime` min ≤ 0;
  - `max_particles` of 0;
  - a burst with `cycles != 1` and `interval` ≤ 0;
  - `spread_deg` outside 0..=360;
  - a shape with a negative radius, size or length, `thickness` outside 0..=1, or `arc_deg` outside 0..=360.
- **`sanitized`** clamps each problem to the nearest allowed value:
  - lifetime min 0.01;
  - a burst interval of 0.01;
  - non-finite values become the field's default.
- **Presets.** Sixteen constructors: `fire`, `flame_small`, `smoke`, `smoke_thick`, `explosion(image, count: u32)`, `explosion_debris(image, count)`, `magic`, `magic_heal`, `rain`, `snow`, `sparks`, `dust`, `confetti`, `bubbles`, `comet_trail` and `exhaust`. Each takes `image: Handle<Image>` first and reproduces `legacy/presets.rs` by these rules:
  - a cone becomes `Point` with its direction and spread in degrees and the cone's speeds;
  - a circle becomes `Circle { radius, arc_deg: 360.0, thickness: 1.0 }`, with thickness 0.0 when hollow, plus `outward()`;
  - a line from `start` to `end` becomes `Line { length: |end - start| }`, centred on the emitter (the old offset is dropped);
  - a rectangle becomes `Rect` with thickness 1;
  - `Continuous { rate }` becomes `rate`;
  - `Burst { count }` becomes rate 0, `Burst::at(0.0, count)` and `looping: false`;
  - start and end colours become `color: WHITE` with `color_over_life` [(0, start), (1, end)];
  - an end size becomes `size_over_life` [(0, 1), (1, mid(end) / mid(start))], where mid is the midpoint of the range, or a constant 1 when mid(start) is 0;
  - glow, gravity, drag and collision carry over unchanged.

- [ ] **Step 1: Write the failing tests.**
  - `defaults_match_the_table`: check every field against the table.
  - `problems_name_each_bad_field`:
    - a NaN lifetime starts with `"lifetime:"`, a rate of -1 with `"rate:"`, and a duration of 0 with `"duration:"`;
    - a burst with cycles 0 and interval 0 starts with `"bursts:"`;
    - a `Circle` with thickness 2 starts with `"shape:"`.
  - `sanitized_effects_have_no_problems`: an effect with every bad value above gives empty `sanitized().problems()`, and no field is NaN.
  - `presets_keep_their_looks`:
    - `fire` has rate 30, `direction_deg` within 1e-3 of -90, `spread_deg` within 1e-2 of 28.648, and its gradient's last key has alpha 0;
    - `explosion(img, 40)` has one burst of 40 and `looping == false`;
    - `sparks` has a bouncy collision with restitution 0.7.
  - `degrees_are_degrees`: `with_direction(90.0, 10.0)` stores 90 and 10.
- [ ] **Step 2: Run them.** Run `cargo test --lib core::particles::effect core::particles::presets`. Expected: they fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them.** Expected: they pass.
- [ ] **Step 5: Run the checks, then commit.**
```bash
git add src/core/particles/effect.rs src/core/particles/presets.rs src/core/particles/mod.rs src/lib.rs
git commit -m "feat: particle effects are assets, with the presets as effects"
```

### Task 5: The engine's particle step

**Files:**
- Create in `src/core/particles/`:
  - `emitter.rs` (the new `ParticleEmitter`; the legacy one stays in `legacy/` until Task 9);
  - `pool.rs`, `store.rs`, `step.rs`;
  - `testing.rs` (`#[cfg(test)]`);
  - `tests/mod.rs` and `tests/step.rs` (`#[cfg(test)] mod tests;` in `mod.rs`).
- Modify:
  - `src/core/schedule.rs`: add `FixedSet::Particles` after `Transforms` and before `Trails`; `engine_sets()` grows to 13; `holds()` lists `Particles` among the fixed sets a scene system may be `Before`.
  - `src/core/tilemap/mod.rs`: `lift_floored` runs `.before(FixedSet::Particles)`.
  - `src/core/engine/mod.rs`:
    - insert `ParticleStore` in `init_world_resources`, with the builder's budget, and a seed of `PARTICLE_HEADLESS_SEED` when headless or from entropy otherwise;
    - register `step_particles` in `FixedSet::Particles`;
    - add `EngineBuilder::particle_budget`.
  - `src/core/scene/manager.rs`: wherever it calls `audio::in_scene`, `hold_scenes` and `release_scene`, it also tells the store (`set_scope`, `hold_scopes`, `release_scope`).
  - `src/lib.rs`: export `DEFAULT_PARTICLE_BUDGET`. The new `ParticleEmitter` stays out of the prelude until Task 9.

**Interfaces:**
- Consumes: Tasks 3 and 4.
- Produces:
```rust
// emitter.rs
#[derive(Component, Debug, Clone)] #[require(Transform)]
#[component(on_insert = emitter_inserted, on_discard = emitter_discarded)]
pub struct ParticleEmitter { pub effect: Handle<ParticleEffect>, /* private state; `slot` set by the hook */ }
impl ParticleEmitter {
  pub fn new(effect: Handle<ParticleEffect>) -> Self;
  pub fn with_seed(self, seed: u64) -> Self; pub fn with_z(self, z: i32) -> Self; pub fn with_speed(self, speed: f32) -> Self;
  pub fn z(&self) -> Option<i32>; pub fn speed(&self) -> f32;
  pub fn alive(&self) -> u32;               // written by the step only when it changes
}
// store.rs
pub const DEFAULT_PARTICLE_BUDGET: u32 = 20_000;
pub(crate) const PARTICLE_HEADLESS_SEED: u64 = 0x5EED_9A27_1C1E;
#[derive(Resource, Debug)] pub(crate) struct ParticleStore { /* pools by slot, free slots, live count, budget,
  the engine seed stream, compiled effects by AssetId, warned effects, scopes */ }
impl ParticleStore {
  pub(crate) fn new(budget: u32, seed: u64) -> Self;
  pub(crate) fn live(&self) -> u32; pub(crate) fn budget(&self) -> u32;
  pub(crate) fn pool(&self, slot: u32) -> Option<&EmitterPool>;
  pub(crate) fn compiled(&self, effect: AssetId<ParticleEffect>) -> Option<&Compiled>;
  pub(crate) fn clear_all(&mut self);
  pub(crate) fn set_scope(&mut self, scope: Option<u64>) -> Option<u64>;
  pub(crate) fn hold_scopes(&mut self, scopes: impl Iterator<Item = u64>);
  pub(crate) fn release_scope(&mut self, scope: u64);
}
// pool.rs: one particle's fields side by side, in spawn order
pub(crate) struct Pool { pub(crate) x: Vec<f32>, pub(crate) y: Vec<f32>, pub(crate) px: Vec<f32>, pub(crate) py: Vec<f32>,
  pub(crate) vx: Vec<f32>, pub(crate) vy: Vec<f32>, pub(crate) age: Vec<f32>, pub(crate) lifetime: Vec<f32>,
  pub(crate) size: Vec<f32>, pub(crate) rotation: Vec<f32>, pub(crate) spin: Vec<f32>,
  pub(crate) color: Vec<[f32; 4]>, pub(crate) seed: Vec<u32>, pub(crate) flags: Vec<u8> }
impl Pool { pub(crate) fn len(&self) -> usize; pub(crate) fn clear(&mut self);
  pub(crate) fn retain(&mut self, keep: impl FnMut(usize) -> bool); }   // compacts in order, never swap_remove
pub(crate) struct EmitterPool { pub(crate) entity: Entity, pub(crate) main: Pool, pub(crate) rng: ParticleRng,
  pub(crate) bounds: Option<(Vec2, Vec2)>, /* effect id, cycle time, carries, burst state, last position,
  last step stepped, scope, playing */ }
// step.rs
pub(crate) struct Compiled { pub(crate) effect: ParticleEffect /* sanitized */, pub(crate) version: u64,
  pub(crate) direction: f32, pub(crate) spread: f32 /* radians */ }   // later tasks add their derived values here
#[derive(Default)] pub(crate) struct StepState { /* kept QueryStates, the entity-order buffer, scratch */ }
pub(crate) fn step_particles(world: &mut World, state: Local<StepState>);  // exclusive
// EngineBuilder
pub fn particle_budget(mut self, max: u32) -> Self;   // default DEFAULT_PARTICLE_BUDGET
// testing.rs (#[cfg(test)])
pub(crate) fn world() -> World;  // Time::new(60), engine messages, Assets<Image>, Assets<ParticleEffect>, ParticleStore::new(DEFAULT_PARTICLE_BUDGET, 1)
pub(crate) fn image(world: &mut World) -> Handle<Image>;                      // 8×8 white
pub(crate) fn effect(world: &mut World, effect: ParticleEffect) -> Handle<ParticleEffect>;
pub(crate) fn step(world: &mut World);   // propagate_transforms, lift_floored, step_particles, then apply commands
pub(crate) struct Seen { pub x: f32, pub y: f32, pub vx: f32, pub vy: f32, pub age: f32, pub size: f32, pub color: [f32; 4] }
pub(crate) fn particles(world: &World, emitter: Entity) -> Vec<Seen>;          // spawn order; size and colour as drawn now
```
- **Hooks:**
  - `on_insert` takes a free slot and records the store's current scope (as `AudioEngine::emitter_added` does). It draws an engine seed from the store's stream, so insertion order fixes it.
  - `on_discard` frees the slot and its particles, and takes them off the live count.
  - Both do nothing without a store.
- **Compiled effects.** The step keeps one compiled effect per `(AssetId, version)`: the effect `sanitized()`, with angles in radians. For an effect with problems, it warns once per `(id, version)` through `dev_log`, naming the effect's asset path, or "an effect added in code" when it has none.
- **Seed choice:** `with_seed` first, then the effect's `seed`, then the engine seed drawn at insert.
- **Order of one step,** for each enabled, game-time emitter whose effect is loaded, in `entity_order`. Skip an emitter that is held: its scope is held, or it has no scope while any scope is held.
  1. Its position is its `GlobalTransform` translation plus `DrawLift.lift`.
  2. `dt` is the fixed timestep times the emitter's speed.
  3. Emit:
     - rate, with a fractional carry;
     - every burst owed by the cycle time, all cycles and intervals included;
     - `duration` and `looping`, and `start_delay` before the first cycle;
     - capped by `max_particles` against the live count, and by the budget. A spawn past the budget is dropped and warned once per effect id, naming the budget and `EngineBuilder::particle_budget`.
  4. Spawn points come from the shape, turned and scaled by the emitter's transform. Direction is `direction_deg` ± `spread_deg / 2`, turned by the emitter, or outward from the shape's centre. A mirrored scale mirrors both. A zero scale puts every point at the centre.
  5. Spread the stream along the move: the step's spawns are placed at even fractions `f` of the move from the last position, each aged `(1 - f) * dt`. There is no spread when the emitter's `PreviousGlobalTransform.skip` is set, or when it was not stepped in the previous step (just spawned, or re-enabled).
  6. Step every particle:
     - `age += dt`;
     - `v += gravity * dt`;
     - `v *= max(1 - drag * dt, 0)`;
     - scale `v` by `speed_over_life(t_now) / speed_over_life(t_before)`, unchanged when the previous value is 0 or the ratio is not finite;
     - `rotation += spin * dt`;
     - store the previous position, then move.
  7. Remove the dead in order (`Pool::retain`), then update the bounds and `alive`.
- **Not stepped here:** unscaled emitters (Task 6), `Disabled` entities (the default query filter) and emitters whose effect is not yet in `Assets`. An emitter with no loaded effect has no cycle time yet.

- [ ] **Step 1: Write the failing tests** in `src/core/particles/tests/step.rs`, using `testing.rs`:
  - **Emission:**
    - `an_emitter_emits_with_no_scene_code`: rate 10 over 60 steps gives exactly 10 particles.
    - `bursts_fire_on_time_with_their_cycles`: a burst at 0.5 s of 5, with 3 cycles at a 0.25 s interval, duration 2, not looping. Counts are 0 before step 30, 5 at step 30, 10 at step 45 and 15 at step 60, with lifetime 10.
    - `a_hitch_fires_every_burst_it_owes`: one burst with cycles 0, interval 0.005 and count 1 gives 200 particles after 60 steps (Review Focus 3).
    - `a_looping_effect_restarts_its_bursts_each_cycle`.
    - `a_finished_cycle_stops_a_non_looping_effect`.
    - `start_delay_holds_the_first_spawn`.
  - **Caps:**
    - `max_particles_caps_the_live_count`.
    - `the_budget_drops_spawns_and_warns_once`: with a budget of 50 and two emitters at rate 1,000, the live count is 50, there is one warning per effect, and spawning resumes as particles die.
  - **Shapes:**
    - `each_shape_spawns_inside_its_bounds`:
      - `Circle` r10 with thickness 1 spawns within 10, and with thickness 0 at 10 ± 1e-3;
      - an arc of 90 stays inside its arc;
      - `Rect` and `Line` stay inside theirs.
    - `the_emitter_s_rotation_and_scale_turn_the_shape`: at a rotation of 90°, a `Line` of length 100 spawns along y.
    - `zero_and_mirrored_scale_are_safe` (Review Focus 4).
  - **Motion and looks:**
    - `gravity_drag_and_speed_over_life_move_particles`: gravity (0, 100) gives vy ≈ 100 after 1 s. A speed curve from 1 to 0 brings the speed under 1 % of its start at the end of life.
    - `size_and_colour_follow_their_curves`: `Seen.size` at half life equals size × curve(0.5).
    - `dead_particles_leave_in_order`: after middle particles die, the survivors' ages still fall in spawn order.
  - **Movement:**
    - `a_moving_emitter_spreads_its_stream`: an emitter moving 100 px a step at rate 600 spreads each step's 10 spawns over the 100 px with distinct ages.
    - `a_teleported_or_re_enabled_emitter_does_not_streak` (Review Focus 2).
    - `an_effect_still_loading_emits_nothing_until_it_lands` (Review Focus 1). Use a handle whose asset is not yet in `Assets<ParticleEffect>`, as a queued `load` makes. Expect no particles and no warning, then a first cycle from the step it lands.
  - **Determinism:** `the_same_seed_repeats_and_others_do_not_disturb_it`. Two worlds with the same seed match. Adding 0 to 8 unrelated resources (`test_util::insert_unrelated`) and another emitter leaves the first emitter's particles unchanged.
  - **Holds:** `held_scenes_freeze_their_emitters`. An emitter inserted under scope 1 freezes while `hold_scopes([1])` holds it, and moves again once released. An emitter with no scope freezes while any scope is held.
  - **Lifecycle and floors:**
    - `a_disabled_emitter_is_frozen`.
    - `a_floor_emitter_emits_at_its_lifted_point_in_its_first_step`.
    - `despawning_an_emitter_frees_its_pool`: the live count goes back to 0, and the next emitter reuses the slot.
- [ ] **Step 2: Run them.** Run `cargo test --lib core::particles::tests`. Expected: they fail.
- [ ] **Step 3: Implement the interfaces and the step above.**
- [ ] **Step 4: Run them, and the legacy tests.** Run `cargo test --lib core::particles`. Expected: all pass.
- [ ] **Step 5: Run the checks and `cargo test --workspace` (to a log), then commit.**
```bash
git add src/core/particles src/core/schedule.rs src/core/tilemap/mod.rs src/core/engine/mod.rs src/core/scene/manager.rs src/lib.rs
git commit -m "feat: the engine spawns and steps particles in its fixed step"
```

### Task 6: Control, one-shots and time

**Files:**
- Create: `src/core/particles/view.rs`
- Modify:
  - `src/core/particles/emitter.rs`, `src/core/particles/step.rs`;
  - `src/core/messages/mod.rs` (register `EmitterFinished`);
  - `src/core/scene/context.rs` (`particles()`);
  - `src/core/engine/mod.rs` (`step_unscaled_particles` in `PostUpdateSet::Extract`);
  - `src/core/testing/{mod.rs,headless.rs}`;
  - `src/lib.rs` (prelude: `ParticlesView`, `Particles`, `EmitterFinished`).
- Test: `src/core/particles/tests/control.rs`, `tests/e2e/particles.rs` (registered in `tests/e2e.rs`)

**Interfaces:**
- Consumes: Task 5.
- Produces:
```rust
impl ParticleEmitter {
  pub fn unscaled(self) -> Self; pub fn stopped(self) -> Self; pub fn once(self) -> Self; pub fn despawn_when_finished(self) -> Self;
  pub fn play(&mut self); pub fn stop(&mut self); pub fn clear(&mut self); pub fn restart(&mut self);
  pub fn burst(&mut self, count: u32); pub fn set_speed(&mut self, speed: f32);
  pub fn is_playing(&self) -> bool; pub fn is_finished(&self) -> bool; pub fn is_unscaled(&self) -> bool;
}
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)] pub struct EmitterFinished { pub entity: Entity }
pub struct ParticlesView<'w> { world: &'w mut World }
impl<'w> ParticlesView<'w> { pub fn new(world: &'w mut World) -> Self;
  pub fn play_at(&mut self, effect: &Handle<ParticleEffect>, position: Vec2) -> Entity;
  pub fn count(&self) -> u32; pub fn budget(&self) -> u32; pub fn clear(&mut self); }
#[derive(SystemParam)] pub struct Particles<'w, 's> { store: Option<Res<'w, ParticleStore>>, commands: Commands<'w, 's> }
impl Particles<'_, '_> { pub fn play_at(&mut self, effect: &Handle<ParticleEffect>, position: Vec2) -> Entity;
  pub fn count(&self) -> u32; pub fn budget(&self) -> u32; }
impl SceneContext<'_> { pub fn particles(&mut self) -> ParticlesView<'_>; }
pub(crate) fn step_unscaled_particles(world: &mut World, state: Local<StepState>);
// ryo2d::testing, #[doc(hidden)]
pub fn particle_positions(world: &World, emitter: Entity) -> Vec<Vec2>;
impl HeadlessEngine { #[doc(hidden)] pub fn step_real(&mut self, real: Duration); }   // one frame with this real delta (add if missing)
```
- **Meanings:**
  - `play` resumes emitting; a finished one-cycle emitter restarts its cycle.
  - `stop` ends spawning, and live particles carry on.
  - `clear` removes the live particles.
  - `restart` clears, sets the cycle time to 0, re-arms the bursts and plays.
  - `burst(n)` adds `n` particles in the next step in any emission mode, within `max_particles` and the budget.
  - `once` plays one cycle even of a looping effect.
  - **Finished** means not playing (stopped, or the one cycle ended) with no live particles. `EmitterFinished` is sent once per finish, and `play` and `restart` re-arm it.
  - `despawn_when_finished` despawns the entity, with its children, in the step after it finishes.
- **Views:**
  - `play_at` spawns `(Transform::from_xy(position.x, position.y), ParticleEmitter::new(effect.clone()).once().despawn_when_finished())` and returns the entity.
  - `count` and `budget` read the store: 0 and `DEFAULT_PARTICLE_BUDGET` when there is no store.
- **Unscaled emitters:**
  - They step in `step_unscaled_particles` on `time.real.delta()` times their speed, with their own accumulator of the fixed step length.
  - They take at most 8 steps a frame (the rest of the time is dropped).
  - The paused clock, hitstop and held scenes do not apply to them. The game-time step skips them.
  - Their step fraction (accumulator ÷ step) is kept in their pool for the drawer.

- [ ] **Step 1: Write the failing tests.**
  - Unit tests:
    - `stop_lets_live_particles_finish`;
    - `clear_removes_live_particles`;
    - `restart_rearms_bursts`;
    - `burst_adds_particles_in_any_mode`: a rate-0 effect with `burst(7)` gives 7 next step;
    - `play_resumes_a_stopped_emitter`;
    - `despawn_when_finished_despawns_after_the_last_particle_and_reports_it`: `EmitterFinished` arrives exactly once, and the entity is gone;
    - `play_at_plays_one_cycle_of_a_looping_effect_and_cleans_up`;
    - `an_effect_with_nothing_to_emit_finishes_when_its_cycle_ends`;
    - `count_and_budget_read_the_store`;
    - `a_world_without_particles_reads_zero`: `ParticlesView::new` on a bare `World` gives count 0 and the default budget, and `play_at` still spawns.
  - In `tests/e2e/particles.rs` (with `HeadlessEngine::new_or_skip`):
    - `a_paused_clock_and_hitstop_freeze_particles`;
    - `a_pause_menu_freezes_the_game_s_particles_but_not_its_own`: the game scene pushes an overlay scene that spawns its own emitter;
    - `an_unscaled_emitter_runs_through_pauses`;
    - `half_speed_halves_the_distance`;
    - `a_switch_leaves_no_particles_and_push_pop_keeps_them`;
    - `frame_rate_does_not_change_particles`: three engines advanced with real deltas of 1/30, 1/60 and 1/144 s to the same 120 fixed steps at 60 Hz hold identical `particle_positions`.
- [ ] **Step 2: Run them.** Run `cargo test --lib core::particles` and `cargo test --test e2e particles`. Expected: they fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them.** Expected: they pass.
- [ ] **Step 5: Run the checks, then commit.**
```bash
git add src/core/particles src/core/messages/mod.rs src/core/scene/context.rs src/core/engine/mod.rs src/core/testing src/lib.rs tests/e2e.rs tests/e2e/particles.rs
git commit -m "feat: emitters play, stop and finish, play_at plays one-shots, and held scenes hold their particles"
```

### Task 7: Attractors and collisions on the engine's step

**Files:**
- Modify: `src/core/particles/step.rs`, `src/core/particles/attractor.rs`, `src/core/particles/pool.rs` (flag bits `STUCK`, `TOUCHING`)
- Create: `src/core/particles/tests/collisions.rs`, `src/core/particles/tests/attractors.rs`

**Interfaces:**
- Consumes: Task 2's `contact.rs` (`gather_colliders` with a kept `ColliderQuery`, `candidate_colliders`, `ParticleCollider::contact`) and Task 5's step.
- Produces: no new names. `ParticleCollision` is written by the step as one batch.

**Behaviour:**
- **Attractors.** Enabled attractors are copied once per step into a reused buffer (position, type, mask; they hold no heap data) and filtered per emitter by layer mask. They act on each particle before it moves. `update(dt)` advances their time, as today.
- **Gathering.** Colliders are gathered into reused buffers, only when some stepped emitter's compiled effect collides.
- **Responses, events, flags and filters stay as they are today:**
  - `Die`, `Bounce`, `Stick` and `Event`;
  - one bounce event per step, entry only, at `MIN_BOUNCE_EVENT_SPEED` or above;
  - stuck particles still age;
  - the own-emitter collider, triggers, `Disabled` colliders and a storey's edge are ignored.
- **Messages:** `ParticleCollision` keeps its fields, and `emitter_entity` is the emitter's entity.

- [ ] **Step 1: Port every legacy collision and layer test.** Port them to the new API (an effect with the collision config, `testing::step`, `testing::particles`), keeping each test's name and assertions:
  - **Responses:** `die_on_hit_removes_particle_at_floor`, `bounce_reflects_off_floor`, `stick_freezes_particle`, `event_response_passes_through_and_notifies_once`, `with_events_reports_die`, `with_events_reports_stick_once`, `bounce_off_circle_collider`.
  - **Filters:** `collision_mask_filters_colliders`, `trigger_colliders_are_ignored`, `disabled_colliders_are_ignored`, `negatively_scaled_collider_still_collides`, `particles_ignore_their_own_emitters_collider`.
  - **Events:** `a_corner_hit_sends_one_bounce_event`, `settling_bounce_does_not_flood_events`, `settling_bounce_does_not_flood_events_under_high_gravity`, `slow_bounce_emits_no_event`, `sliding_particle_bounces_off_wall_gathered_after_floor`.
  - **Tile edges:** `particles_meet_a_tile_part_s_edges_not_its_box`, `a_particle_crossing_an_edge_within_an_update_still_bounces`, `a_fast_stick_particle_sticks_on_the_surface_it_crossed`, `a_particle_starting_just_inside_tile_ground_ends_on_top_of_it`, `a_particle_in_a_room_whose_wall_s_outline_lies_in_another_part_drifts_untouched`, `a_particle_in_a_room_alone_in_its_part_bounces_off_its_walls`, `particles_stay_in_a_game_s_closed_polyline_arena_and_bounce`, `a_raised_floor_s_edge_is_no_surface_for_particles`, `a_particle_falls_onto_a_flipped_hex_tile_s_shape_and_stops_on_it`.
  - **Layers:** `layered_attractor_pulls_particles_on_its_layer`, `layered_attractor_ignores_particles_on_other_layers`.
  - **New behaviour:**
    - `a_stopped_emitter_s_particles_keep_colliding`;
    - `a_stopped_emitter_s_particles_keep_their_gravity`.
- [ ] **Step 2: Run them.** Run `cargo test --lib core::particles::tests`. Expected: the ported tests fail (the new step does not collide yet).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them, plus the legacy tests and `attractor.rs`'s own seven.** Expected: all pass.
- [ ] **Step 5: Run the checks, then commit.**
```bash
git add src/core/particles
git commit -m "feat: particles collide and feel attractors on the engine's step"
```

### Task 8: Drawing particles as runs

**Files:**
- Modify:
  - `src/core/render/draw_list.rs` (`DrawRun`, `DrawPayload::Run`, `sees`, run textures);
  - `src/core/render/batch/plan.rs` (`Placed::Quads`);
  - `src/core/render/batch/gpu.rs` (placing a run);
  - the step that makes textures resident (`SpriteGpuRenderer::prepare_textures`, which also takes the runs' textures);
  - `src/core/engine/mod.rs` (an engine field `particle_drawer`, taken out for `draw_frame` like `entity_drawer`);
  - `src/core/engine/extract.rs` (call the drawer just after `batch_collector.push_to`, with `profile_scope!("render:particles")`);
  - `src/core/particles/step.rs` (`profile_scope!("cpu:particles")`);
  - `src/core/diagnostics/overlays.rs` (stats row).
- Create: `src/core/particles/draw.rs`
- Test: `src/core/render/draw_list.rs` and `plan.rs` tests, `src/core/particles/draw.rs` tests, `tests/e2e/particles.rs`

**Interfaces:**
- Consumes: Tasks 5 and 6 (`Pool`, the step fraction of unscaled pools).
- Produces:
```rust
pub(crate) struct DrawRun { pub(crate) texture: TextureId, pub(crate) shading: SpriteShading /* Plain or Effect */,
  pub(crate) blend: BlendMode, pub(crate) filter: Filter, pub(crate) layers: RenderLayers,
  pub(crate) min: Vec2, pub(crate) max: Vec2, pub(crate) instances: Range<u32> }
impl DrawList { pub(crate) fn run_instances(&mut self) -> &mut Vec<SpriteInstance>;   // cleared with the list
  pub(crate) fn push_run(&mut self, lit: bool, z: i32, run: DrawRun);              // DrawKind::Sprite
  pub(crate) fn run(&self, index: usize) -> Option<&DrawRun>;
  pub(crate) fn run_textures(&self) -> impl Iterator<Item = TextureId> + '_; }
// plan.rs: the next `count` quads, all with this state; numbered and merged as single quads are
Placed::Quads { state: DrawState, count: u32 }
// draw.rs
pub(crate) struct ParticleDrawer { /* kept QueryState, reused order buffer */ }
impl ParticleDrawer { pub(crate) fn collect(&mut self, world: &World, list: &mut DrawList,
  textures: &SpriteGpuRenderer, alpha: f32, cameras: &[CameraDraw]) -> u32; }   // particles submitted
pub(crate) fn instance_for(pool: &Pool, i: usize, compiled: &Compiled, alpha: f32) -> SpriteInstance;   // pure, unit-tested
```
- **One run per pool,** for emitters in `entity_order`:
  - **z:** the emitter's `z()`, else the effect's.
  - **Layers:** the entity's `RenderLayers`, else the default.
  - **Lit flag and blend:** the effect's `lit` and `blend`.
  - **Filter:** the effect's.
  - **Shading:** `Effect` when the effect has a `SpriteEffect`, else `Plain`.
- **Each instance:**
  - Position: the previous position mixed with the current one by `alpha`. A game-time pool uses the frame's `time.fixed.overstep_fraction()`, and an unscaled pool its own fraction.
  - Size: `size × size_over_life.lookup(t)`.
  - Colour: start colour × `color_over_life.lookup(t)` per channel.
  - Rotation, `with_effect` and emission as `BatchCollector::add_shaded` folds them.
- **Order:** spawn order for `NewestOnTop`, reversed for `OldestOnTop` (Task 12 adds the effect choice; this task draws spawn order).
- **Culling:** a pool whose bounds, grown by its largest size, meet no camera's view on its layers is skipped. A pool whose texture isn't resident yet is skipped, as the collector does.
- **In the batch:** a run's texture slot is resolved once. Each instance is placed with `place_in_slot` (with effects when shading is `Effect`), and the run returns `Placed::Quads`.
- **Stats:** `DebugStats` and its smoothed twin gain `particles` (live) and `particle_budget`. The F1 stats add a row `Particles: {live} / {budget}`, and the row offsets (`base_y`, `overlay_stack_top`) grow by one row.

- [ ] **Step 1: Write the failing tests.**
  - Draw list:
    - `a_run_sorts_as_one_sprite_item`: a run at z 0, between an entity and a trail at z 0, sorts Entity, Run, Trail;
    - `a_camera_sees_a_run_by_its_layers_and_box`.
  - Plan: `a_run_numbers_its_quads_and_merges_with_neighbours`: a sprite, a run of 3 and a sprite, all with one state, make one draw of 5.
  - Draw:
    - `an_instance_blends_between_steps`: alpha 0.5 puts it halfway;
    - `an_instance_takes_its_life_s_size_and_colour`.
  - Overlays: `the_stats_show_particles_against_the_budget`.
  - e2e:
    - `particles_draw_where_they_are`: one still 16 px white particle at (100, 100) gives white at its centre and black 20 px away;
    - `render_layers_choose_the_cameras`;
    - `draws_blend_between_steps`: `step_real` by half a step draws a moving particle halfway;
    - `an_off_screen_emitter_submits_nothing_but_keeps_stepping`;
    - `a_disabled_emitter_draws_nothing`;
    - `draw_order_is_stable_as_particles_die`;
    - `a_run_sorts_by_z_with_entities`: an entity at z 1 draws over particles at z 0 and under particles at z 2.
- [ ] **Step 2: Run them.** Run `cargo test --lib core::render core::particles core::diagnostics` and `cargo test --test e2e particles`. Expected: they fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them, and `cargo test --workspace` to a log.** Expected: they pass.
- [ ] **Step 5: Run the checks, then commit.**
```bash
git add src/core/render src/core/particles src/core/engine src/core/diagnostics/overlays.rs tests/e2e/particles.rs
git commit -m "feat: particles draw as one run per emitter, interpolated and on their render layers"
```

### Task 9: Move the users over and remove the old system

**Files:**
- Delete: `src/core/particles/legacy/`
- Modify:
  - `src/lib.rs` (the prelude exports the new `ParticleEmitter` and drops `ParticleSystem`, `EmissionMode` and the old `EmissionShape`);
  - `src/core/particles/mod.rs` (module docs: a CPU simulation drawn through the 2D batch, run by the engine);
  - `examples/games/leap-king/src/scenes/{game.rs,menu.rs}`;
  - `examples/demos/AGENTS.md:89`;
  - `src/core/testing/mod.rs:35-37` and `src/core/testing/AGENTS.md:34` (headless runs seed particles; `with_seed` fixes one emitter);
  - every fenced Rust example in `.claude/skills/` that uses removed API, so `tests/skill-examples` compiles (code only; Task 18 rewrites the prose);
  - `benches/particles.rs`: its three scenes move to the new API, with the same counts, rates, lifetimes and colliders, still with no targets (Task 17 adds them).
- Restructure `examples/demos/particles` as `examples/demos/saves` is laid out: `src/lib.rs` (pages), `src/main.rs`, `tests/pages.rs`.

**Interfaces:**
- Consumes: Tasks 4 to 8.

**Behaviour:**
- **The demo's ten pages** keep their themes:
  - each page adds its preset effects once (`ctx.assets().add(ParticleEffect::fire(image))`);
  - `cleanup_finished_emitters` gives way to `despawn_when_finished`;
  - the count label reads `ctx.particles().count()` against `budget()`;
  - click bursts use `ctx.particles().play_at`.
- **Leap-king:** rain and lamp dust become effects. The four `ParticleSystem` constructions, the three `update` calls and the `collect_for_render` calls go.

- [ ] **Step 1: Write `tests/pages.rs`.** Each page builds and steps 120 headless frames with no warning in the dev log, as the saves demo's page tests do. Run `cargo test -p particles-demo`. Expected: it fails to compile against the old pages.
- [ ] **Step 2: Move the demo, leap-king and the skill examples to the new API, and delete `legacy/`.**
- [ ] **Step 3: Run the tests.**
  - `cargo test -p particles-demo`, `cargo test -p leap-king` and `cargo test -p skill-examples`.
  - `cargo test --workspace` to a log.
  - `./dev.sh demos:tour particles` to a log: all ten pages capture, with no panic.
- [ ] **Step 4: Search for leftovers.** `rg "ParticleSystem|collect_for_render|EmissionMode" src examples tests benches .claude` finds nothing.
- [ ] **Step 5: Run the checks, then commit.** Stage the deleted `legacy/` files by name with `git rm`.
```bash
git add src/lib.rs src/core/particles src/core/testing examples/demos/particles examples/games/leap-king examples/demos/AGENTS.md .claude/skills benches/particles.rs
git commit -m "refactor: the particles demo and leap-king use engine-driven particles, and the scene-driven system is removed"
```

### Task 10: Emission by distance, inherited velocity, local space and prewarm

**Files:**
- Modify: `src/core/particles/effect.rs`, `step.rs`, `draw.rs`, `src/lib.rs` (prelude: `Space`)
- Test: `src/core/particles/tests/emission.rs`

**Interfaces:**
- Produces:
  - `ParticleEffect` fields:
    - `rate_per_distance: f32` (0.0);
    - `inherit_velocity: f32` (0.0);
    - `space: Space` (`World`);
    - `prewarm: bool` (false).
  - Builders `with_rate_per_distance`, `with_inherit_velocity`, `with_space` and `with_prewarm`.
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)] pub enum Space { #[default] World, Local }`.

**Behaviour:**
- **Rate over distance** spawns `distance × rate_per_distance` (with a carry) along the step's move, under the same no-spread rule as Task 5.
- **Emitter velocity** is its move ÷ dt, and zero on a teleport or a first step. A particle starts with `inherit_velocity ×` that velocity added.
- **`Local` particles:**
  - they live in the emitter's frame, and forces act in that frame;
  - they are drawn through the emitter's current interpolated transform, plus its lift;
  - they don't collide or feel attractors: `problems` reports `"collision: a Local effect does not collide"`, and `sanitized` drops the collision.
- **Prewarm** applies to looping effects only (`problems` names `"prewarm:"` otherwise, and `sanitized` turns it off). In its first step, an emitter first runs `min(duration, 10.0)` seconds of fixed steps, without messages.

- [ ] **Step 1: Write the failing tests.**
  - `rate_per_distance_spawns_by_distance`: 0.5 per unit over a 200 px move across 10 steps gives 100.
  - `inherited_velocity_follows_the_emitter`.
  - `local_particles_follow_a_moving_emitter_and_world_ones_do_not`.
  - `a_local_effect_does_not_collide`.
  - `prewarm_starts_full`: rate 100, lifetime 1 and looping give 100 ± 1 live after the first step, with ages spread across 0..1.
  - `prewarm_needs_a_looping_effect`.
- [ ] **Step 2: Run them.** Expected: they fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them.** Expected: they pass.
- [ ] **Step 5: Run the checks, then commit.**
```bash
git add src/core/particles src/lib.rs
git commit -m "feat: particles emit by distance, inherit velocity, live in local space and prewarm"
```

### Task 11: Radial, tangential and noise forces

**Files:**
- Modify: `src/core/particles/effect.rs`, `step.rs`, `attractor.rs` (`Turbulence` uses `noise_force`), `src/lib.rs` (prelude: `NoiseForce`)
- Test: `src/core/particles/tests/motion.rs`

**Interfaces:**
- Produces:
  - `ParticleEffect` fields:
    - `radial_acceleration: ValueRange` (0.0);
    - `tangential_acceleration: ValueRange` (0.0);
    - `noise: Option<NoiseForce>` (`None`).
  - Builders `with_radial_acceleration`, `with_tangential_acceleration` and `with_noise`.
  - `#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)] pub struct NoiseForce { pub strength: f32, pub frequency: f32, pub speed: f32 }`, with `NoiseForce::new(strength, frequency)` (speed 1.0) and `with_speed`.

**Behaviour:**
- **Per-particle values.** A particle's radial and tangential values come from its own `seed` each step: a hash of the seed to `t`, then the range mixed at `t`. So each particle keeps one value with no extra storage.
- **Radial** acceleration pushes away from the emitter's current position (a negative value pulls in). **Tangential** acceleration is perpendicular to that, counter-clockwise on screen for a positive value.
- **Noise:** `noise_force(seed, position, age × speed, frequency) × strength`, where `seed` is the emitter's seed folded to `u32`.
- **The `Turbulence` attractor** samples `noise_force` at its own time instead of the sin/cos.

- [ ] **Step 1: Write the failing tests.**
  - `radial_acceleration_pushes_away_from_the_emitter`.
  - `tangential_acceleration_turns_counter_clockwise`.
  - `noise_moves_particles_smoothly_and_repeats_for_a_seed`.
  - `turbulence_uses_smooth_noise`, in `attractor.rs`.
- [ ] **Step 2: Run them.** Expected: they fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them.** Expected: they pass.
- [ ] **Step 5: Run the checks, then commit.**
```bash
git add src/core/particles src/lib.rs
git commit -m "feat: particles take radial, tangential and noise forces"
```

### Task 12: Sprite-sheet frames, alignment and draw order

**Files:**
- Modify: `src/core/particles/effect.rs`, `draw.rs`, `src/lib.rs` (prelude: `Flipbook`, `FrameMode`, `Align`)
- Test: `draw.rs` tests, `tests/e2e/particles.rs`

**Interfaces:**
- Produces:
  - `ParticleEffect` fields `frames: Option<Flipbook>` (`None`) and `align: Align` (`None`), with builders `with_frames` and `with_align`, plus `with_draw_order` from Task 4.
  - The types:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Flipbook { pub columns: u32, pub rows: u32, pub count: u32 /* 0: columns × rows */, pub mode: FrameMode, pub random_start: bool }
impl Flipbook { pub fn new(columns: u32, rows: u32) -> Self; pub fn with_count(self, count: u32) -> Self;
  pub fn at_fps(self, fps: f32) -> Self; pub fn random_start(self) -> Self; }
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)] pub enum FrameMode { #[default] OverLife, Fps(f32) }
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)] pub enum Align { #[default] None, Velocity { stretch: f32 } }
```

**Behaviour:**
- **Frame `k`:**
  - `OverLife` takes `k = min(floor(t × count), count - 1)`.
  - `Fps(fps)` takes `k = floor(age × fps)`.
  - Add a start offset (from the particle's `seed` when `random_start`), modulo `count`.
- **The cell:** column `k % columns`, row `k / columns`, with `with_uv` set to that cell's rectangle.
- **`Align::Velocity { stretch }`:** rotation is `atan2(vy, vx)` (rotation and spin are ignored). The quad is `size + speed × stretch` long along the velocity and `size` wide.
- **`problems`** covers columns or rows of 0, a count over columns × rows, a non-finite or non-positive fps, and a negative stretch.
- **`OldestOnTop`** reverses the run's order.

- [ ] **Step 1: Write the failing tests.**
  - On `instance_for`:
    - `sheet_frames_follow_life`;
    - `sheet_frames_follow_a_frame_rate_and_wrap`;
    - `a_random_start_offsets_frames_per_particle`;
    - `aligned_particles_point_along_their_velocity_and_stretch_with_speed`;
    - `oldest_on_top_reverses_the_draw_order`.
  - e2e: `a_sheet_frame_draws_its_cell`. With a 2×1 sheet, red then blue, over life, a particle in the first half of its life draws red.
- [ ] **Step 2: Run them.** Expected: they fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them.** Expected: they pass.
- [ ] **Step 5: Run the checks, then commit.**
```bash
git add src/core/particles src/lib.rs tests/e2e/particles.rs
git commit -m "feat: particles play sprite-sheet frames, align to their velocity and choose their draw order"
```

### Task 13: Sub-emitters

**Files:**
- Modify: `src/core/particles/effect.rs`, `pool.rs` (each `EmitterPool` gains `death: Pool` and `collision: Pool`), `step.rs`, `draw.rs`, `src/lib.rs` (prelude: `SubEmitter`)
- Test: `src/core/particles/tests/sub_emitters.rs`

**Interfaces:**
- Produces:
  - `ParticleEffect` fields `on_death: Option<SubEmitter>` and `on_collision: Option<SubEmitter>` (both `None`), with builders `on_death` and `on_collision`.
  - `#[derive(Debug, Clone)] pub struct SubEmitter { pub effect: Handle<ParticleEffect>, pub count: ValueRange, pub inherit_velocity: f32 }`, with `SubEmitter::new(effect, count)` (inherit 0.0) and `with_inherit_velocity`.

**Behaviour:**
- **When they fire:**
  - `on_death` fires when a particle's life ends or a `Die` response removes it.
  - `on_collision` fires on each contact that would send an event (`Die`, `Stick`, a `Bounce` entry at `MIN_BOUNCE_EVENT_SPEED` or above, an `Event` entry), whether or not `send_events` is set.
- **The burst:** `count` sub-particles spawn where the particle was, with the sub-effect's shape, direction and spawn values, plus `inherit_velocity ×` the particle's velocity before the response.
- **Sub pools:**
  - they step with the sub-effect's motion, life, collisions and looks, and draw as their own runs right after the main run, at the emitter's z;
  - they count against the sub-effect's `max_particles` and the budget, and vanish with the emitter;
  - "finished" waits for them;
  - they draw from the parent pool's random stream.
- **One level deep:** a sub-effect's own sub-emitters are ignored, with one warning per compiled version (`"on_death: a sub-effect's own sub-emitters are ignored"`).
- A sub-effect not yet loaded spawns nothing.

- [ ] **Step 1: Write the failing tests.**
  - `a_death_plays_the_sub_effect_where_it_happened`.
  - `a_collision_plays_its_sub_effect`.
  - `sub_effects_go_one_level_deep`.
  - `sub_particles_vanish_with_the_emitter`.
  - `sub_particles_count_against_the_budget`.
  - `finished_waits_for_sub_particles`.
- [ ] **Step 2: Run them.** Expected: they fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them, and every particle test.** Expected: they pass.
- [ ] **Step 5: Run the checks, then commit.**
```bash
git add src/core/particles src/lib.rs
git commit -m "feat: particles play sub-effects when they die or collide"
```

### Task 14: A collision grid

**Files:**
- Create: `src/core/particles/grid.rs`
- Modify: `src/core/particles/contact.rs` (contact over a given set of a collider's edges), `step.rs`
- Test: `grid.rs` tests, `src/core/particles/tests/collisions.rs`

**Interfaces:**
- Consumes: Task 7's gathered `colliders` and `edges`.
- Produces:
```rust
pub(crate) struct ColliderGrid { /* cells, reused */ }
pub(crate) struct Candidates { pub(crate) colliders: Vec<u32> /* gathered order, no repeats */, pub(crate) edges: Vec<u32> /* ascending, no repeats */ }
impl ColliderGrid { pub(crate) fn rebuild(&mut self, colliders: &[ParticleCollider], edges: &[(Vec2, Vec2)]);
  pub(crate) fn query(&self, min: Vec2, max: Vec2, out: &mut Candidates); }
```

**Behaviour:**
- **Cell size:** `clamp(2 × mean edge length, 16.0, 256.0)`, or the mean collider box side when there are no edges.
- **Filling the cells:** each non-edge collider goes in the cells its box covers, and each edge in the cells its segment's box covers.
- **A particle's query** is its move's box grown by `max(radius, the largest reach among gathered colliders)`.
- **Edges colliders** test only their candidate edges, in `edge_contact` and `solid_contact` alike.
- **Resolution order** stays the gathered order. Results must equal testing every edge.
- **Rebuilds** happen only when colliders were gathered that step, using reused buffers.

- [ ] **Step 1: Write the failing tests.**
  - `the_grid_finds_what_brute_force_finds`: 10,000 seeded random moves (from `ParticleRng`) over Task 7's fixtures (a busy tile map's parts, circles, boxes and a closed polyline arena), at radius 2 and at radius 6. Each collider's contact result is identical with the grid and without it.
  - `an_empty_world_builds_an_empty_grid`.
- [ ] **Step 2: Run them.** Expected: they fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them, and every collision test.** Expected: they pass.
- [ ] **Step 5: Run the checks, then commit.**
```bash
git add src/core/particles
git commit -m "refactor: particle collisions test only the colliders and edges near each particle"
```

### Task 15: Effect files

**Files:**
- Create: `src/core/asset/loaders/particles.rs` (the file structs and `ParticleEffectLoader`)
- Modify:
  - `src/core/asset/loaders/mod.rs`;
  - `src/core/asset/mod.rs` (`insert_registry` registers the loader);
  - `src/core/render/batch/key.rs` (`BlendMode` derives serde);
  - `src/core/ecs/sprite_effects.rs` (`SpriteEffect` derives serde, by variant and field names);
  - `src/core/particles/collision.rs` (a file form for `CollisionConfig`).
- Test: `loaders/particles.rs` tests, `src/core/particles/tests/reload.rs`

**Interfaces:**
- Consumes: every `ParticleEffect` field from Tasks 4 and 10 to 13.
- Produces: `ParticleEffectLoader` (`extensions() == ["particles.ron"]`, `type Settings = ()`).

**Behaviour:**
- **The file struct** has one field per effect field, every one `#[serde(default)]` except `image: String`, with `#[serde(deny_unknown_fields)]`. It reads with `ron::Options::default().with_default_extension(Extensions::IMPLICIT_SOME)`, as `loaders/sound.rs` does.
- **Field forms:**
  - **Colours:** `"#rrggbb"`, `"#rrggbbaa"` or `(r:, g:, b:, a:)`, with `a` defaulting to 255. This applies in `color`, colour pairs and gradient keys.
  - **Bursts:** `(time:, count:, cycles:, interval:)`.
  - **Collision:** `(radius:, mask:, response: Die | Bounce(0.6) | Stick | Event, events:)`.
  - **Sub-emitters:** `(effect: "path", count:, inherit_velocity:)`.
  - **Noise:** `(strength:, frequency:, speed:)`.
  - **Frames:** `(columns:, rows:, count:, mode: OverLife | Fps(12.0), random_start:)`.
  - **Align:** `None` or `Velocity(stretch: 0.02)`.
- **Paths:** `image` and sub-effect paths are asset paths, loaded with `cx.load::<Image>` and `cx.load::<ParticleEffect>` as dependencies.
- **Errors:** a RON error keeps its line and column. `problems()` becomes `AssetError::decode(path, problems joined by "; ")`.
- **Loader `check`:** it warns when a sub-effect has sub-emitters of its own.
- **Reloads** need nothing new in the step: compiled effects already follow versions. A reload changes live particles' over-life values, forces, looks and collisions. Spawn values apply to new particles. An emitter keeps its cycle time and burst schedule.

- [ ] **Step 1: Write the failing tests,** with `MemorySource` and the registry pump, as the sound loader's tests do.
  - `a_sample_file_loads_every_field`: a file setting every field equals the same effect built in code.
  - `colours_read_as_hex_or_fields`.
  - `unknown_fields_are_refused_with_their_line`: `colour:` on line 3 gives an error naming `colour` and line 3.
  - `bad_values_are_refused_naming_the_field`: `lifetime: -1` gives an error containing `lifetime:`.
  - `a_broken_reload_keeps_the_last_good_effect`.
  - `a_missing_image_fails_as_a_dependency`.
  - `a_reload_changes_live_particles_over_life_values`: change the gradient from red to blue, and the next step's `Seen` colour changes while the start values stay.
  - `a_reload_lowering_the_cap_or_dropping_collision_keeps_live_particles` (Review Focus 5).
  - `a_sub_effect_s_reload_reaches_its_parent`.
- [ ] **Step 2: Run them.** Run `cargo test --lib core::asset::loaders::particles core::particles`. Expected: they fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them.** Expected: they pass.
- [ ] **Step 5: Run the checks, then commit.**
```bash
git add src/core/asset src/core/render/batch/key.rs src/core/ecs/sprite_effects.rs src/core/particles
git commit -m "feat: particle effects load from .particles.ron files and reload while running"
```

### Task 16: Demo pages for what is new

**Files:**
- Modify: `examples/demos/particles/src/` (pages `life.rs`, `emission.rs`, `collisions.rs` and `files.rs`, added after the ten), `examples/demos/particles/tests/pages.rs`
- Create: under `examples/demos/particles/assets/fx/`:
  - `spark.png`, `smoke_sheet.png` (4×2 frames);
  - `sparks.particles.ron`, `splash.particles.ron`, `embers.particles.ron`, `rain.particles.ron`;
  - `README.md`, which records the ImageMagick commands that made the PNGs.
- On the web, the Files page embeds its files with `MemorySource`, as the audio demo does.

**Behaviour:** pages follow the demo-kit standard (`examples/demos/AGENTS.md`).
- **Life:** gradient ramps, size and speed curves side by side, and the smoke sheet played over life and at a frame rate.
- **Emission:**
  - bursts with cycles;
  - rate over distance trailing the mouse;
  - the four shapes;
  - local and world emitters orbiting.
- **Collisions** (a small tile map built in code):
  - rain from `rain.particles.ron` splashing through `on_collision`, into `splash.particles.ron`;
  - sparks bouncing;
  - embers sticking.
- **Files:** the effects from `assets/fx`, a line saying to edit and save one while the demo runs, and the count against the budget.

- [ ] **Step 1: Extend `tests/pages.rs` to the 14 pages.** Each builds and steps 120 frames with no warning, and the Files page's effects are loaded. Run `cargo test -p particles-demo`. Expected: the new page tests fail.
- [ ] **Step 2: Build the pages and assets.**
- [ ] **Step 3: Run the tests.**
  - `cargo test -p particles-demo`: it passes.
  - `./dev.sh demos:tour particles`, twice: all 14 pages capture, and both runs are byte-identical.
- [ ] **Step 4: Run the checks, then commit.**
```bash
git add examples/demos/particles
git commit -m "feat: the particles demo shows life curves, emission, collisions and effect files"
```

### Task 17: Measure and guard

**Files:**
- Modify: `benches/particles.rs` (guards and the idle measure added to Task 9's scenes), `docs/backlog/particles.md`, `src/core/testing/headless.rs` (`#[doc(hidden)] particle_systems_time()` if needed)
- Create: `tests/particles_alloc.rs`

**Interfaces:**
- Consumes: everything.

**The bench:**
- **Scenes** are Task 1's, moved to the new API in Task 9 and seeded with `with_seed(7)`.
- **Targets** (median frame over the empty frame):
  - `TEN_K_TARGET = 1.0 ms`;
  - `HUNDRED_TARGET = 1.2 ms`;
  - `COLLIDE_TARGET = 1.0 ms`.
- **Idle target:** `IDLE_TARGET = 1 µs`, timed on the particle systems' own time (`step_particles`, `step_unscaled_particles` and the drawer) with no emitters.
- **Recorded without targets:**
  - the `cpu:particles` and `render:particles` medians beside each frame target;
  - the prewarm cost of a 1,000-particle effect;
  - a sample file's reload time.

**The allocation test** follows `tests/tilemap_alloc.rs`:
- **Scene:** 20 steady emitters, plus a `play_at` of a burst effect every 10 frames.
- **Run:** warm up for 120 frames, then measure 120.
- **Assertion:** no engine-own allocation site beyond an empty frame's.
- **ECS exceptions:** sites whose stack runs through `bevy_ecs` spawn, despawn or command application are classified like GPU sites: printed, not asserted.

- [ ] **Step 1: Write the allocation test.** Run `cargo test --test particles_alloc`. Expected: it passes, or it names the particle code's own allocating sites. Fix those in the particle code until it passes.
- [ ] **Step 2: Rewrite the bench with the targets as guards** (`guards::miss`, `guards::verdict`, as `benches/saves.rs` does).
- [ ] **Step 3: Run it three times on a quiet machine,** recording `uptime` before each, to logs.
- [ ] **Step 4: Record it.**
  - Add "### After" to `docs/backlog/particles.md`, with a Before/After table and the scope figures.
  - Add "## Limits", covering:
    - CPU simulation;
    - ground-space collisions;
    - `Local` effects not colliding;
    - one level of sub-emitters;
    - per-particle GPU clipping;
    - the prewarm cost;
    - embedding files on the web;
    - a fast emitter's spread when it is not interpolated.
  - **On a miss,** report DONE_WITH_CONCERNS with the figures. Don't loosen a target.
- [ ] **Step 5: Run the checks, then commit.**
```bash
git add benches/particles.rs tests/particles_alloc.rs docs/backlog/particles.md src/core/testing/headless.rs
git commit -m "test: particle frames are guarded against their targets and recorded"
```

### Task 18: Docs

**Files:**
- Modify skills, each bumped one minor version:
  - `.claude/skills/ryo-particles/SKILL.md` (rewritten around effects, files, emitters, the engine's part, collisions and attractors);
  - `ryo-api` (the Particles section and the message table);
  - `ryo-assets` (the asset types table: `ParticleEffect`, `.particles.ron`);
  - `ryo-game-dev`;
  - `ryo-renderer`;
  - `ryo-messages` (`EmitterFinished`; `ParticleCollision` is written in the particle step);
  - `ryo-lighting` (`lit` is on the effect);
  - `ryo-tilemap` (on a storey, a burst spawned and fired in one step fires lifted).
- Modify module notes:
  - `src/core/particles/AGENTS.md` (rewritten: the engine's part, storage, step order, draw, gotchas, collisions);
  - `src/core/render/AGENTS.md` (run items);
  - `src/core/graphics/AGENTS.md:24`;
  - `src/core/ecs/AGENTS.md:62,75`;
  - `src/core/testing/AGENTS.md`.
- Modify other docs:
  - `docs/architecture/overview.md`, every line naming scene-driven particles;
  - `README.md:149`;
  - `docs/backlog/tilemap.md:911` (drop the "fires low" clause; collisions in ground space stay);
  - `docs/backlog/follow-ups.md:118` (remove "a burst spawned and fired in one update fires low").

- [ ] **Step 1: Rewrite and update the docs above.** Every fenced Rust example compiles. Check that every API name and signature in the skills matches the code.
- [ ] **Step 2: Run the tests.** Run `cargo test -p skill-examples` and `cargo test --workspace` to a log. `cargo doc --no-deps -p ryo2d` must give no warning beyond the known `padded.rs` links. Expected: they pass.
- [ ] **Step 3: Search for leftovers.** `rg -i "gpu particle|ParticleSystem|collect_for_render" --glob '!docs/specs/**' --glob '!docs/plans/**' --glob '!docs/research/**'` finds nothing.
- [ ] **Step 4: Run the checks, then commit.**
```bash
git add .claude/skills src/core/particles/AGENTS.md src/core/render/AGENTS.md src/core/graphics/AGENTS.md src/core/ecs/AGENTS.md src/core/testing/AGENTS.md docs/architecture/overview.md README.md docs/backlog/tilemap.md docs/backlog/follow-ups.md
git commit -m "docs: skills, module notes and the overview describe engine-driven particles"
```
