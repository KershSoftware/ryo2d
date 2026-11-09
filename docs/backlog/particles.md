# Particles: speed and limits

## Measured

### Before (the scene-driven system, main 74fd1521)

Machine: Apple M5 Pro, 18 cores, shared with other work. Bench: `cargo bench --bench particles`, 1920x1080 headless, 1,000 interleaved frames over an empty frame, the GPU drained outside the measure, each scene warmed up 240 frames first. The scenes: `particles_10k` is one emitter holding 9,916 particles (cap 10,000, rate 5,000, lifetime 2); `particles_100x100` is 100 emitters holding 9,900 (rate 50, max 100 each); `particles_collide_2k` is one emitter holding 1,983 bouncing on a 64x64 tile map (16 px tiles, border and 8 platforms; its cap is raised to 2,000, as the default 1,000 would hold half).

One-minute load before each run (`uptime`), every one above 3 and so flagged: run 1 8.18, run 2 6.07, run 3 4.84. The load fell from run to run and the figures did not move with it.

Each cell is the step's median and 95th percentile over the empty frame's median, in microseconds (an empty frame's step: about 104).

| Bench | Run 1 | Run 2 | Run 3 |
|-------|-------|-------|-------|
| `particles_10k` | 675.8 / 741.8 | 668.7 / 749.6 | 678.3 / 738.2 |
| `particles_100x100` | 704.2 / 775.1 | 704.3 / 790.0 | 714.0 / 783.1 |
| `particles_collide_2k` | 287.2 / 329.7 | 286.6 / 332.0 | 290.2 / 328.3 |

### After (the engine-driven system, this branch)

Machine: the same Apple M5 Pro, 18 cores, shared with other work. Bench: `cargo bench --bench particles`, the same three scenes on the new API (live at the end, as in Before: `particles_10k` 9,916, `particles_100x100` 9,900, `particles_collide_2k` 1,983), 1920x1080 headless, 1,000 interleaved frames over an empty frame, the GPU drained outside the measure, each scene warmed up 240 frames first.

Before and After were measured in one session, as the Before table above ran at a higher load: the scene-driven system's bench (at `d127c8ab`, built from a source export into its own target directory) and this one, three rounds, run alternately (old then new, new then old, old then new), so a drifting load reaches both alike. One-minute load before each run (`uptime`): round 1 old 10.47, new 7.18; round 2 new 5.77, old 5.15; round 3 old 4.80, new 4.13. Every one is above 3 and so flagged; nothing waited for a quiet machine.

Each cell is the step's median and 95th percentile over its own engine's empty frame's median, in microseconds (an empty frame's step: old 105.8, 108.5 and 109.2; new 116.1, 101.3 and 102.4).

| Bench | Before (rounds 1 to 3) | After (rounds 1 to 3) | Median of rounds, Before to After |
|-------|------------------------|-----------------------|-----------------------------------|
| `particles_10k` | 666.6 / 741.2, 669.3 / 754.3, 682.8 / 821.2 | 415.8 / 603.8, 399.1 / 476.8, 411.3 / 522.0 | 669.3 to 411.3 (-258.0, -38.5%) |
| `particles_100x100` | 690.8 / 779.5, 704.8 / 796.8, 722.2 / 873.9 | 373.8 / 587.5, 357.4 / 443.1, 365.0 / 457.2 | 704.8 to 365.0 (-339.8, -48.2%) |
| `particles_collide_2k` | 283.7 / 335.6, 285.3 / 339.2, 290.6 / 389.1 | 215.8 / 368.2, 208.9 / 263.5, 215.1 / 283.8 | 285.3 to 215.1 (-70.2, -24.6%) |

The spec's targets, on the medians of the three rounds, and the same frames against the scene-driven system's. Beside them, with no target, the particle systems' own time in the frame (`HeadlessEngine::particle_systems_time`: the profiler's `cpu:particles`, the fixed step and the unscaled pass, and `render:particles`, the drawer's `collect`) and the frame's render step over the empty frame's (medians of the round medians):

| Bench | Target | After | Spec's target | Against the scene-driven system | `cpu:particles` | `render:particles` | Render step over empty |
|-------|--------|-------|----------------|----------------------------------|-----------------|---------------------|------------------------|
| `particles_10k` | 1,000 µs | 411.3 µs | met | met (-38.5%) | 151.6 µs | 97.5 µs | +259.9 µs |
| `particles_100x100` | 1,200 µs | 365.0 µs | met | met (-48.2%) | 106.2 µs | 101.2 µs | +264.7 µs |
| `particles_collide_2k` | 1,000 µs | 215.1 µs | met | met (-24.6%) | 147.1 µs | 19.1 µs | +66.6 µs |

Criterion's back-to-back medians (the three rounds), with the GPU paced rather than drained, so these move little with the CPU's work (the 10,000 quads are drawn either way, and `particles_10k` reads a little over the old system's here, as it did before the change to `retain`), and the guard above is the figure held: `particles_10k` old 996.5, 995.2 and 990.7 µs, new 1,038.5, 1,036.1 and 1,038.3; `particles_100x100` old 856.3, 852.4 and 860.0 µs, new 491.9, 488.2 and 487.9; `particles_collide_2k` old 415.6, 414.1 and 414.6 µs, new 335.1, 332.0 and 332.2.

The first measure of this system put `particles_10k` over the scene-driven system: with `Pool::retain` copying every survivor particle by particle, 779.8 µs against 688.6 (+13.2%, worse in every round), `cpu:particles` 522 µs. A temporary change to the bench raising the particles' lifetime to 1,000 s, so that none die and none spawn once the 10,000 are out, took `cpu:particles` to about 151 µs. The oldest die first, so the dead sit at the front of the pool and the in-order compaction moved every survivor, fourteen fields each, on every step that lost any. In a pool of 4,096 particles or more, `Pool::retain` now moves each run of at least 8 survivors with `copy_within` on every field array, and a shorter run particle by particle; a run with nothing dead before it is not touched. It needs no new buffer, and the survivors keep their spawn order.

Measured alone, on the pool's exact fields (a standalone micro-benchmark, not kept), median time of one `retain` at 10,000 particles, particle by particle against now: the dead at the front (100 of them) 376 µs against 8.5; 1% scattered 348 against 17; 10% scattered 326 against 104; 33% scattered 246 against 181; every other one dead 178 against 165; nothing dead 4.2 against 1.4. Below 4,096 particles the loop stays particle by particle, because it was the faster when many die at once (1,000 particles with every other one dead, 1.9 µs against 5.7; 3,000, 5.7 against 16.8) and costs only a few microseconds when they do not (3,000 particles, 1% at the front, 9.8 µs). The particle-by-particle loop's cost per particle was not smooth in the size: about 3 ns at 3,000 and 6,000 particles, about 36 ns at 4,000, 10,000 and 20,000, for a reason not investigated (the fourteen arrays' placement is the likely one). So at 6,000 particles a step in which many die at once is slower by runs (33% scattered 14 µs against 26; every other one dead 11 against 34), a cost of that one step; with few dead, runs are faster there too (1% at the front, 19 µs against 5).

No emitters (`particles_idle`): the particle systems' own work with no emitters (the fixed step, the unscaled pass and the drawer, run outside a frame by `HeadlessEngine::idle_particle_systems`, 100 batches of 10,000 runs each, the first left out), the median batch's time a run, 74.0, 73.0 and 75.0 ns in the three rounds (each round's batches between 71.0 and 80.0 ns), against the target of 1 µs: met. (Read inside a real empty frame, the same work with the systems' clock reads around it is about 0.42 to 0.50 µs; the idle measure leaves those reads out. A criterion run of the same call made singly, its own clock reads in it, was 99 to 100 ns.)

Allocations (`tests/particles_alloc.rs`, 20 steady emitters, 18 plain with an `on_death` sub-effect and 2 colliding over a tile map's parts with `on_collision`/`on_death`, and a burst effect played with `play_at` every 10 frames, 120 frames warmed up, 120 measured): the measured frames' 418 allocations each are the same 241 the empty frame makes of the engine's own, site by site, and 177 inside the GPU API (asserted none from the particle code's own path); none of the engine's own came from the particle code, and none from the ECS's spawn or despawn code (the one-shots' entities coming and going, printed and not asserted, made none at all). The test names the site of any it finds (checked by planting one in `Pass::step_emitter` and one in the `emitter_inserted` hook: each was named). All of this is in a debug build, in the test harness; the frame targets above are release.

Recorded, with no target:

- The prewarm of a 1,000-particle effect (rate 500, lifetime 2, its cycle 2 s, 993 alive after it): `cpu:particles` in the frame it starts, 424.7, 436.2 and 434.9 µs (median over 30 starts each round, the first 3 left out; 95th percentile 438.2 to 442.6), and the whole frame over an ordinary one 467.5, 480.4 and 479.8 µs.
- A sample effect file's reload (`examples/demos/particles/assets/fx/sparks.particles.ron`, 619 B of RON, its rate changed each time; no engine, a world with the asset registry): from the reload to the new version in the store, the main thread pumping as fast as it can (so the worker's latency, not a frame's), 11.3, 12.0 and 11.2 µs in the three rounds (median over 30 reloads, the first 3 left out; 95th percentile 16.5 to 28.8 µs).

Each guard prints its verdict and fails the run on a miss (`guards::miss`, `guards::verdict`): the three frame targets and the no-emitters target. Against the scene-driven system is recorded here only: that bench no longer exists on this branch, so it cannot be a run-to-run guard.

### Changes the bench made to the engine

Two hooks, `#[doc(hidden)]` in `src/core/testing/headless.rs`: `HeadlessEngine::particle_systems_time` (the last frame's `cpu:particles` and `render:particles`, from `ParticleTimes` in the store, written by the two systems and around the drawer's `collect`) and `HeadlessEngine::idle_particle_systems(calls)`, which runs the systems' bodies (`run_particle_step`, `run_unscaled_step`, split out of `step_particles` and `step_unscaled_particles`) and the drawer's `collect` outside a frame.

The timers cost the frame two clock reads around each of the two systems and around the drawer, and a store write for each; they are the only cost the systems and the drawer carry for this, and the frame's figures above include it.

## Limits

Each was checked against the code or the module notes (`src/core/particles/AGENTS.md`) before it was written here.

- **The simulation is on the CPU**, one emitter after another on the main thread, in the fixed step: no compute shader (WebGL2 has none, and the web is a target) and no threads. The budget (20,000 live particles by default, `EngineBuilder::particle_budget`) bounds it; the figures above are what it costs on this machine.
- **Particles collide in ground space.** Colliders are where their entities stand, so an emitter on a storey emits where it draws (lifted) but its particles fly, collide and are attracted in ground space, meeting colliders a storey's height below the drawn surface. A storey's edge is no surface (the module notes, "On a floor").
- **`Local` effects do not collide.** `ParticleEffect::problems` reports it and `sanitized` drops the collision, so a `Local` main effect's particles never collide and it never fires `on_collision` (its `on_death` still plays), and a `Local` sub-effect does not collide either.
- **Sub-emitters go one level deep.** A sub-effect's own sub-emitters are ignored, with a warning once per compiled version; a file naming such a sub-effect also warns when it loads.
- **Each particle is clipped by the GPU, not culled on the CPU.** The drawer culls whole runs: an emitter whose box (or a sub-effect's, its own) meets a camera's view on its layers submits every particle it holds, so a large emitter mostly off screen still pays for every instance and the overdraw of what is on screen. There is no per-particle cull.
- **The cull box is grown only by the largest a particle can draw now:** its effect's largest size times its size curve's largest value. Glow and outline padding (`with_effect` sprite effects drawn past the quad) are not counted, and neither are particles spawned larger under an earlier compiled version of the effect, so their edges can be cut at the view's edge.
- **A `Local` emitter not stepped can be culled on screen.** Its box is rebuilt only when it steps, so one held by a scene, or at speed 0, whose entity moves away draws through where it stands (the local frame) but is culled by the box from its last step.
- **Prewarm is paid at once.** A prewarmed effect runs `min(duration, 10)` s of steps (at most 600, longer ones past that) inside the first step of its emitter, so the frame it appears in pays its whole prewarm (about 430 µs for 1,000 particles over a 2 s cycle, recorded above; it scales with the particles and the steps).
- **Effect files on the web must be embedded.** The web has no asset folder and no hot reload: `.particles.ron` files reach the game only through a source of embedded bytes, as the particles demo's `embedded.rs` does, and that list is kept by hand.
- **An unscaled emitter taking several steps in one frame gets a lumpy inherited velocity:** the whole frame's move lands in its first step and the others see none (`Frame::inherited`).
- **A mirrored emitter that turns or scales between steps draws its `Local` particles unmirrored between steps** (`GlobalTransform::lerp`, which `render_global` uses, drops the mirror when the two steps' matrices differ; each step's end is right).
- **One distant or huge collider coarsens the collision grid.** The grid's cell side follows the mean edge length (or collider size) and doubles until at most 65,536 cells cover every gathered collider's span, so a single far-off collider widens every cell of the pass and each particle's query finds more candidates. Results stay exact; only the cost moves.
- **Smaller ones.** A `play_at` whose effect never loads leaves its emitter entity waiting (no warning, no despawn). `ParticlesView::clear` leaves a held emitter's `alive` stale until it next steps. An unscaled `Interpolated` emitter's stream keeps a teleport's "no spread" while the game clock is paused, because the fixed step's snapshot that clears `PreviousGlobalTransform.skip` does not run. A turbulence attractor's time advances with the game-time pass only (when it has an emitter to step), so an unscaled emitter's turbulence freezes while paused or with no game-time emitter stepping. A sub pool names its effect by id, not by a kept handle, so the particles a parent played last keep going after it is repointed only while something else still holds that effect.
