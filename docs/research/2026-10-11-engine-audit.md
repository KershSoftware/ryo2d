# Ryo2D Engine Audit (2026-10-11)

An audit of the engine core against what a professional 2D engine looks like in 2026. Networking (`src/core/network/`, ~9.9k lines) was excluded on purpose; it will be revisited as a separate project.

## Scope and method

- The engine is ~111k lines of Rust (src), 35 feature demos, 5 games, and a green test suite: 1948 unit, 379 integration and 131 GPU e2e tests pass; clippy on all targets is clean.
- The game loop, scene stack, timing, ECS wrapper, transform hierarchy, physics wrapper, render pipeline, entity draw path, surface/renderer setup and one example game were read in full by the auditor. The remaining subsystems (UI, lighting, particles, trails, tilemap, raycast, skeleton, assets, config, console, dev windows, audio, input, navigation, procgen, inventory, CLI) were summarised by code-reading subagents, and every claim rated High below was then checked against the source.
- Five research passes established the 2026 baseline: Bevy 0.20, Godot 4.x, Unity, Rapier 0.36, wgpu 30, Kira 0.12, glyphon/cosmic-text for text.
- Findings marked *(summary only)* were reported by the summary pass and not re-verified line by line.

## Headline verdict

The engine is a large, well-tested, clippy-clean body of code whose foundations were laid for a CPU software renderer and a "one scene, one world, a few hundred entities" game. Most of the problems below are not bugs in features. They are consequences of six foundational decisions that a professional engine would make differently:

1. The time model (fixed step, interpolation, time scale, physics dt) is inconsistent and in places wrong.
2. The ECS is used as a hash map of components, with a hand-maintained hierarchy that has unit and lifetime bugs.
3. The renderer has no camera matrix and renders at a fixed logical resolution into 8-bit targets, with five parallel render stacks.
4. The physics wrapper hides most of Rapier and defeats its sleeping and filtering.
5. The UI is a CPU rasterizer with an ASCII bitmap font and no widget identity.
6. The asset layer is a path-keyed sprite cache, not an asset system.

Everything else (lighting shadows, tilemap, trails, audio, input, config, console) inherits from those six. The right move is to redesign those six foundations, then rebuild the feature modules on top of them, cutting the ones that do not earn their place in a core.

---

## Part 1: Foundational problems (need redesign, not patches)

### 1. Time, loop, input and events

**Physics runs at the wrong speed when `physics_hz` is not 60.** `RapierPhysicsWorld` uses `IntegrationParameters::default()` (`src/core/physics/rapier.rs:135`) and never sets `dt`. Rapier therefore always integrates 1/60 s per step regardless of how many fixed steps the engine runs per second. Breakout sets `physics_hz(144)` (`examples/games/breakout/main.rs:49`), so its physics advances 2.4x real time. This is the most impactful single bug found.

**Physics ignores time scale and hitstop.** Documented as a decision in `src/core/AGENTS.md` ("Rapier ignores it"). Every professional engine scales physics with game time (Unity `Time.timeScale` scales `fixedDeltaTime`; Godot `Engine.time_scale`; Bevy `Time<Fixed>` derives from `Time<Virtual>`). Slow-mo that does not slow physics bodies, and hitstop that freezes animations but not falling objects, is wrong for a 2D action engine.

**Render interpolation snapshots at the wrong time.** `InterpolationSystem::snapshot` runs once per frame before the fixed loop (`src/core/engine.rs:1131`), not once per fixed step. Two consequences:
- On a frame with zero fixed steps (render rate above `physics_hz`), previous is overwritten with current, so the entity snaps to the latest state instead of continuing to interpolate. This is exactly the case interpolation exists for and it produces judder at 144 Hz render with 60 Hz physics. The examples hide it by setting `target_fps == physics_hz`.
- On a frame with two steps, the lerp spans two steps (S0 to S2).
The standard algorithm (Fiedler, Bevy, Godot) saves previous state inside the step loop.

**"Real dt" is actually the fixed dt.** `let real_dt = time.fixed_dt()` (`engine.rs:1072` and `:1233`). Camera shake, console animation and hitstop decay use it, so they run per frame at the fixed step size. Hitstop and shake durations therefore scale with render frame rate (a 100 ms hitstop lasts about 42 ms at 144 fps).

**Shader time is frame count divided by 60** (`engine.rs:1800-1804`). Any time-driven shader effect runs faster at higher frame rates.

**Two divergent frame loops.** `step_frame` (`engine.rs:937`) and `tick_frame` (`engine.rs:1206`, for unfocused windows) re-implement the fixed loop with different behaviour (no input script, no console, no per-frame input clear in the second).

**Events live one fixed update, not one frame.** `events.clear()` runs at the top of every fixed update (`engine.rs:1901`). The docs in `events/mod.rs` and `scene.rs` say one frame. Events emitted in `render_ui` or in an earlier step of a multi-step frame are lost.

**Input is polled across two clocks with patches.** Per-frame pressed/released sets are only cleared on frames that ran an update (`engine.rs:1194-1196`), but `render_ui` reads input every render frame, so `key_pressed` fires repeatedly between updates. The code works around this with `suppress_oneshots`, `post_render_cleanup`, `skip_next_update` after a scene pop, `consume_key`, `click_consumed` and `escape_consumed` propagation. These are symptoms of not having a frame-scoped input event buffer.

**`Scene::render(ctx, alpha)` is dead.** The trait method exists and `SceneManager::render` exists, but the engine never calls it (only `render_ui` and `render_gpu`).

**`SurfaceError::Timeout` exits the game.** Any surface error other than `Outdated`/`Lost` is returned as `Fatal` (`engine.rs:1860-1863`) and the window loop calls `event_loop.exit()`. Timeout is transient (occluded or minimised windows on some platforms).

**Recommended shape.** One loop. Three clocks, Bevy-style: `Real` (wall clock, for UI/console/shake), `Virtual` (scaled and pausable, drives everything game-visible including physics), `Fixed` (accumulates from `Virtual`). Rapier `dt` equals the fixed step and the step runs inside the fixed loop. Previous-state snapshot inside the step loop; `alpha = accumulator / fixed_dt`. Input delivered as a per-frame event buffer with a consume model (UI consumes first, then gameplay); frame-scoped not update-scoped. Events double-buffered per frame with reader cursors.

### 2. ECS usage, Transform and hierarchy

**Rotation units disagree across the engine.** `WorldTransform.rotation` is documented as degrees and `from_parent_and_local` calls `parent.rotation.to_radians()` for its trig (`src/core/ecs/hierarchy.rs:91,149`). The renderer treats the same value as radians (`src/core/render/entities.rs:497-498`), and tween docs say radians (`ecs/tween.rs:458,487`). A rotated parent orbits its children by the wrong angle. There is no test crossing the hierarchy/renderer boundary.

**Plain `despawn` orphans children.** `World::despawn` (`ecs/world.rs:107`) does no hierarchy cleanup. Orphans keep a stale `Parent`, so the roots query `without::<&Parent>()` (`world.rs:336`) never reaches them and their `WorldTransform` freezes; the renderer prefers `WorldTransform` over `Transform` (`render/entities.rs:440`), so they stop following their own transform. `set_parent` has no cycle check (`world.rs:197-225`).

**Render cache invalidation is incomplete.** `entities_changed` is set only by `spawn`, `spawn_empty`, `despawn` and `clear` (`world.rs:91,100,108,127`). `spawn_child` and `despawn_recursive` call `self.inner` directly and skip it (`world.rs:174,268,272`). `insert_one` (adding a `Renderable` later) and changing `z_order` after spawn never re-sort the cache (`render_cache_dirty` is set only at `engine.rs:1385,1531,1925,2088`). Y-sorting re-sorts the whole cache with a fresh `HashMap` every frame (`engine.rs:2133-2174`).

**`WorldTransform` is a mandatory duplicate with the wrong timing.** `propagate_transforms` (`world.rs:330`) clones every root `Transform` into a `Vec` and `insert_one`s a `WorldTransform` on every entity every fixed step, then clones each `Children` vec per parent. It runs before physics (`engine.rs:2096` vs `:2115`), so physics bodies render one step stale. Physics, trails and skeletons read local `Transform` as if it were world space; interpolation lerps a local `PreviousTransform` toward a world target for children (`render/entities.rs:468-481`).

**The ECS is used as a hash map of components.** The idiom in every engine system is `world.entities_with::<T>()` (allocates a `Vec`) followed by 3-8 random `get`/`has` calls per entity:
- Interpolation: 2 full scans, 5 allocations per frame (`systems/interpolation.rs:28-80`).
- Physics registration: 2 full world scans and 4 `Vec`s per fixed step (`systems/physics.rs:50-99`).
- Entity draw path: 6 lookups per entity per frame (`render/entities.rs:417-490`).
- Particles: 2 lookups per particle per frame (`particles/mod.rs:796-801`, summary only).
- Lights and occluders cloned into `Vec`s every frame including polygon vertices (`engine.rs:1716-1729`).
A single archetype query per system would remove nearly all of this and is the whole point of an archetype ECS.

**No scheduler, no change detection, no commands.** System order is hard-coded in `Engine::update` (`engine.rs:1930-2120`). Games cannot register systems; all game logic lives in `Scene::update`. Nothing can react to "this component changed", which is why the render cache, physics registration and collider re-registration are all manual and incomplete. `hecs::CommandBuffer` is never used.

**`Transform` mixes concerns.** Ten fields: position, z, rotation (unit ambiguous), scale, pivot and pixel offset (`ecs/components.rs:41-58`). Pivot and offset are sprite concerns. It is not `Copy`, so propagation clones it.

**Juice systems fight each other and physics.** `Knockback` overwrites `Velocity` and also moves `Transform` directly (`systems/knockback.rs:60-70`); for a Rapier body the velocity is then pushed into Rapier and the body also gets teleported to the moved position, so it moves twice. `SquashStretch` writes absolute scale and caches its base once *(summary only)*. `Tween`, `SquashStretch`, `Knockback` and physics all write `Transform` with no arbitration.

**Pooling.** `acquire`/`release` swap `Active`/`Inactive` marker components, which is an archetype move each time (`ecs/pool.rs:428-437`). Capped pools die after `world.clear()` because `total_spawned` is not reset *(summary only)*.

**Decision to make.** Keep hecs and write a scheduler, change detection and a proper hierarchy on top (roughly 1-2k lines of careful code), or adopt `bevy_ecs` standalone (0.20), which ships schedules with sets and parallelism, `Changed<T>`/`Added<T>`, `Commands`, `ChildOf`/`Children` relationships with maintained invariants, resources, events and observers. The engine currently has home-grown versions of every one of those, and every one of them has a bug in this audit. The recommendation is `bevy_ecs` unless its dependency weight is unacceptable; agents also write correct `bevy_ecs` code far more reliably than code against a bespoke API. Either way: `Transform { translation: Vec2, rotation: f32 (radians), scale: Vec2 }` as `Copy`, a `GlobalTransform` as `Affine2` computed with dirty tracking after simulation, and pivot/anchor moved onto the sprite.

### 3. Renderer core

**No camera matrix.** `Camera` is a CPU struct (`graphics/camera.rs:117`). Every entity is transformed to screen pixels on the CPU with `world_to_screen`, zoom is baked into each sprite's scale (`render/entities.rs:505-506`), and the GPU `view_proj` is a fixed pixel ortho (`graphics/sprite_gpu.rs:258-270`). Consequences: no camera rotation, no multiple cameras or viewports, no render-to-texture with its own camera, no split screen, and the per-sprite CPU transform cost.

**Positions are truncated to integer pixels** ("follows the CPU renderer exactly", `render/entities.rs:1-8,135-144,362-363`). This was parity with the old software renderer. On a GPU it means no sub-pixel motion: slow pans and fractional zooms shimmer for anything that is not pixel art. Pixel snapping should be an option, not the only mode.

**Fixed logical resolution equals render resolution.** The game texture is `config.width x config.height` and the composite pass upscales it (`graphics/renderer.rs:356-372,647-719`). The window is created with `LogicalSize` (`engine.rs:2398`), so on a 2x display the surface is 2560x1440 and the game renders at 1280x720 and is scaled. Fine for pixel art; blurry for everything else. The only native-resolution path is `ResizableStretch`, which changes the coordinate system under the game. There is no separation of logical units (camera, UI layout) from physical pixels.

**8-bit sRGB everywhere.** Game, lit and post-process A/B targets are `Rgba8UnormSrgb` (`renderer.rs:366,435`); only light accumulation is `Rgba16Float`. Bloom thresholds and composites on clamped values. No HDR headroom, no MSAA, no depth.

**Five parallel render stacks.** Sprites (`sprite_gpu.rs`), trails (`trail/renderer.rs`, own vertex type, pipelines, 65536-vertex cap and a second string-keyed texture cache), lighting (`lighting/renderer.rs`, 1.5k lines of descriptor boilerplate), raycast, post-process and the compositor each own pipelines, buffers and bind groups. Textures are keyed three ways: `HashMap<String, CachedTexture>` in the sprite renderer (`sprite_gpu.rs:573`), the atlas, and `TextureManager` with hardware-sRGB textures while the atlas is `Rgba8Unorm` with manual decode. Custom-shader compilation is duplicated between `sprite_gpu.rs:325-549` and `shaders/pipeline.rs:81-348`. The fullscreen vertex shader is pasted into 12 WGSL files.

**Texture identity by string.** `BatchCollector::add(texture_id: impl AsRef<str>, ..)` (`sprite_gpu.rs:1495`), `has_texture(id: &str)`, `SpriteHandle::id()` allocating a `String` (`asset/manager.rs:132`), `TextureRef = Arc<str>` cloned per draw item. A particle system pays a string hash per particle per frame.

**No blend modes, nearest-only sampling.** Everything is `ALPHA_BLENDING`; no additive/multiply per sprite. Sampler is hard-coded Nearest (`sprite_gpu.rs:637-646`); no mipmaps, so downscaling aliases.

**Per-frame full-frame work.** UI buffer `fill(0)` (`engine.rs:2225`), full-buffer `any()` scan to detect UI (`renderer.rs:736`) and full `write_texture` upload each frame: about 8 MB x3 at 1080p regardless of UI content. A separate clear pass before a `Load` world pass; copies to `scene_copy` (lighting), to a copy texture (transition) and to A (post-process). Bind groups are created every frame in composite, post-process and lighting (`compositor.rs:295-303`, `shaders/pipeline.rs:773-816`, `lighting/renderer.rs:925,994,1107`).

**Portability is desktop only.** `Limits::default()` and `expect` on adapter/device (`renderer.rs:205-229`); storage buffers in vertex/fragment stages (no WebGL2); `pollster::block_on(pop_error_scope())` mid-frame (`sprite_gpu.rs:477`); non-sRGB surface fallback produces dark output (`renderer.rs:240`, `fullscreen_quad.wgsl`).

**Smaller renderer defects.** The letterbox tests exercise a `cfg(test)` copy of `calculate_viewport` (`renderer.rs:889-957`), not the real function, and the same math is duplicated a third time in `engine.rs:371-430`. `DEFAULT_BACKGROUND_COLOR` constants do not match their comment (`renderer.rs:14-23`). `set_effect` reorders the effect chain on every update (`shaders/manager.rs:507-523`). Outline thickness is unclamped and grows per-fragment loop cost quadratically (`sprite.wgsl:189-202`). Atlas eviction can evict entries already emitted this frame under pressure *(summary only, plausible)*.

**What to keep.** The `RenderPipeline` ordered pass list with latest-image tracking (`render/pipeline.rs`, `render/context.rs`) is a reasonable skeleton. The z-sorted `DrawList` with consecutive-run merging and the 2048 atlas pages with extruded borders are the right batching idea (10k sprites on 3 textures in ≤8 draws).

**Recommended shape.** A camera uniform (view-projection) with instances in world space; integer texture handles from one texture registry; one general 2D batch (textured quads plus arbitrary triangle lists, per-batch texture/pipeline/blend/scissor) that sprites, tiles, trails, lines, particles, debug draw and UI all feed; `Rgba16Float` scene target with a tonemap in the composite; logical units separated from physical pixels with native-resolution rendering by default and an opt-in pixel-perfect low-res target; pixel snapping as an option; blend modes and filter modes per material; shared WGSL includes; no per-frame bind group creation.

### 4. Physics wrapper

Beyond the dt bug above, the wrapper exposes a fraction of Rapier and works against it:

- **No static bodies.** `AGENTS.md` note 4: "there is no static body type". Walls and floors are `kinematic_position_based` (`rapier.rs:211-215`). A tilemap of kinematic tiles is the worst case for the broad phase and solver.
- **Rotation locked on every body** (`rapier.rs:214,221`) and never synced. No rolling, tumbling or rotating platforms.
- **Two collider shapes:** `Box` and `Circle` (`physics/collision.rs`). No capsule, convex polygon, polyline/segment (needed for tilemap edges), heightfield, compound or multiple colliders per body (`entity_to_collider: HashMap<Entity, ColliderHandle>`, one each).
- **Collision layers never reach Rapier.** `Collider.layer`/`mask` exist with builders (`collision.rs:197-199,283-295`) but `rapier.rs` never calls `collision_groups`; filtering silently does nothing in the real simulation.
- **`RigidBody.mass` is dead.** Set by the builder (`physics/components.rs:63`), never read at registration.
- **Sleeping is defeated.** Every dynamic body is teleported with `set_translation(.., wake_up=true)` and has its velocity overwritten with `set_linvel(.., true)` on every step (`rapier.rs:546-575`), from values that were themselves just written from Rapier. Islands can never sleep and Rapier's own state is replaced by a pixels-to-meters float round trip each step.
- **No query API.** `QueryPipeline` is unused: no raycast, shape cast, point query or overlap test. (`src/core/raycast/` is a pseudo-3D renderer, not physics ray queries.) `are_intersecting`/`get_current_intersections` use `narrow_phase.contact_pair`, which excludes sensors, so "am I inside this trigger" returns nothing *(summary only)*.
- **No joints**, no CCD configuration beyond a flag, no collider re-registration when scale/collider/body change (dimensions are baked once, `rapier.rs:282-284`), no parented bodies.
- **Manual unit conversion** (`PIXELS_PER_METER` in ~20 places) where Rapier's `IntegrationParameters::length_unit` exists for exactly this.
- **Legacy second physics system** (`physics/spatial.rs`, `check_collision`) is exported from the prelude and used by nothing in `src/` or `examples/`.
- Two fresh unbounded crossbeam channels are allocated per step (`rapier.rs:431-434`).

**Recommended shape.** Rewrite the wrapper on current Rapier (0.36): Static/Kinematic/Dynamic bodies, the full collider set with multiple colliders per body, collision groups, joints, a query API, `length_unit`, change-detected sync in both directions (write to Rapier only when the ECS value changed; read back only awake bodies), Rapier dt equal to the fixed step and scaled by virtual time.

### 5. UI and text

The UI is a CPU software rasterizer writing into an RGBA `Vec<u8>` at game resolution (`ui/immediate/context.rs:515-560`, `engine.rs:2225-2238`), uploaded in full each frame and composited with nearest sampling. The "hybrid immediate plus retained" design in `ui/mod.rs` does not exist: the retained layer is four caller-owned state structs whose logic lives in the immediate context.

- **Text is a placeholder.** The only font is `BitmapFont`: fixed-cell ASCII 32-126 (`ui/immediate/font.rs:36`), glyph lookup by `ch as u8` (non-ASCII renders as the wrong glyph), no TTF, shaping, kerning, fallback, bidi or ellipsis. There is no font crate in `Cargo.toml`. Three separate bitmap fonts exist (UI, debug overlay, dev windows).
- **Alpha compositing is wrong.** The CPU blend produces premultiplied colour (`context.rs:544-552`) but the compositor blends it as straight alpha (`compositor.rs:326-329`), so translucent panels come out darker than specified; glyph pixels force alpha 255 (`font.rs:415,585,767,923`), so faded text renders opaque.
- **No widget identity.** Widgets are identified by the `Rect` passed in and call order; arbitration is "first widget called wins" via `click_consumed`. Hover/held ignore the clip stack and consumption (`context.rs:816-845`), so buttons scrolled out of a scroll view still react, modals cannot block what is under them, and the slider drags whenever the mouse is down near it ignoring occlusion (`context.rs:1054-1079`).
- **Layout** is cursor-advancing `HBox`/`VBox`/`Stack` with explicit sizes; nothing measures content. `ui_scale` scales glyphs but not geometry, and inconsistently (`context.rs:725,739,776-790`).
- **No gamepad navigation, no accessibility hooks, no disabled state, no tooltips** despite theme fields for them. The engine-built `UiContext` has `theme: None`, so scenes never see a theme (`context.rs:140-166`).
- `push_clip`/`pop_clip` `assert!` (`context.rs:364,382`) are user-reachable panics. `context.rs` is 2988 lines mixing rasterizer, widgets, input and focus.

**Recommended shape.** GPU-drawn UI on the shared 2D batch with a scissor stack; text through `cosmic-text` with a glyph atlas (glyphon 0.12 is the practical reference); a hashed widget-id stack with hot/active/focused state in the context; a real layout engine (taffy) behind the immediate API; gamepad/keyboard focus navigation; rendering at window resolution with DPI-aware logical sizing. The immediate-mode call style can stay as the user API; the core under it is what needs replacing.

### 6. Assets

`AssetManager` is two hash maps (sprites and shaders) keyed by canonicalized path (`asset/manager.rs:527-529,606-609`), with a `canonicalize` syscall on every lookup. There is no generic handle type, loader registry, dependency tracking or load state model. Audio, fonts, tilemaps and textures each have their own manager.

- Async loads complete only if the game calls `process_async_loads()` every frame; the engine never does (`asset/AGENTS.md`). Failures become `String`s.
- Unload is manual (`gc_sprites`, never called) and the renderer's own handle clone keeps `ref_count > 1` anyway.
- Hot reload re-reads files synchronously on the main thread with no debounce (`manager.rs:1096-1138`), sprites and shaders only, debug builds only; audio and SVG extensions are watched and dropped.
- `SpriteHandle` is `Arc<RwLock<Sprite>>` with `.expect(..poisoned)` on every accessor (`manager.rs:111-141`). The GPU keys entity sprites by `serial + generation` but `render_gpu` sprites, particles, parallax and the debug font by path string.

**Recommended shape.** `Handle<T>` as id plus generation, an `Asset` trait with typed loaders registered by extension, an engine-pumped async loader with `LoadState { Loading, Loaded, Failed(Arc<Error>) }`, ref-counted lifetimes with weak handles, dependency-aware readiness, and hot reload that updates in place with debounce. One registry for sprites, textures, fonts, audio, shaders and tilemaps.

---

## Part 2: Subsystem findings

Format: what it is, what is wrong, verdict (keep / fix / rewrite / cut from core).

**Lighting (3.7k lines).** Additive light accumulation into `Rgba16Float` plus a compose pass is sound and scales to the 256-light cap. Shadows are CPU shadow volumes subtracted from the shared light buffer by a distance heuristic (`shaders/core/shadow.wgsl`), so one light's shadow darkens other lights and overlapping occluders double-subtract; the CPU loop is O(lights x occluders) with a `Vec` per pair before the cull test (`lighting/shadow.rs:166-200`) and silently stops at 8192 vertices. The raymarched-shadow path (`render_lights_with_shadows`, `render_raymarched_shadow`) has no callers outside its own file, so `Light2D::with_raymarched_shadows` is silently ignored; if wired, its single params buffer would be overwritten per light within one encoder *(summary only)*. Verdict: keep accumulation and compose, rewrite shadows (per-light 1D shadow maps or SDF) with an occluder spatial index, delete the dead raymarch path.

**Trails and lines (5.1k lines, ~1.5k tests).** A parallel render stack with its own vertex type, pipelines, 65536-vertex cap and texture cache (`trail/renderer.rs:14,181-414,496-575`). Trails have no joins (only `Line2D` does), so corners gap or overlap. Debug overlays share the same vertex budget. Verdict: keep `Trail2D` point recording and the effect shaders; rebuild geometry as a ribbon/polyline batch on the shared renderer.

**Particles (3.3k lines).** Documented as a GPU particle system; it is a CPU simulation feeding the sprite batcher (which is fine), driven by the scene, with two hecs lookups per particle per frame, a per-frame `HashMap`, O(particles x attractors) and O(particles x colliders) loops, and user-reachable `gen_range` panics on degenerate ranges *(summary only)*. Verdict: keep the design, fix the hot path, let the engine drive it, add a global cap, fix the docs.

**Tilemap (3.4k lines).** Data model plus a Tiled JSON loader. There is no renderer (`tilemap/AGENTS.md` admits it; the demo draws each tile as an entity). The loader overwrites one `tileset_index` per layer for each tile (`tilemap/tiled.rs:534`), so multi-tileset layers get the last tile's tileset, and it never masks Tiled's flip/rotation GID bits, so flipped tiles resolve to wrong IDs. Collision rects are generated but never become physics colliders; autotiling and tile animation exist as helpers nothing applies. Isometric/hex orientations are dead enum variants. Verdict: rewrite as chunked meshes on the shared batch with per-tile flip bits, autotiling applied on set, animation advanced by the engine, and collision emitted as physics polylines.

**Skeleton (1.2k lines).** FK bones, keyframe tracks, blending and a controller. No IK, no skinning or sprite binding; the only output is debug lines. Scale is keyframed but ignored by `compute_world_transforms` *(summary only)*. Verdict: either add sprite/mesh binding and IK or cut until a renderer exists.

**Raycast pseudo-3D (2.1k lines).** A Wolfenstein-style renderer that replaces the 2D world passes. It bypasses lighting, trails, tilemap and post-processing, has no movement/collision layer, panics on maps over 256x256 (`raycast/gpu.rs:530-536`) and drops the nearest sprites past 1024. It sits on `SceneContext` as a core field. Verdict: move to an optional crate or example; it makes the engine's identity ambiguous.

**Audio (0.9k lines).** A thin Kira wrapper: no sub-tracks/buses, no spatial audio, no effects, no clocks. Category volumes are baked into each sound at play time, so volume sliders do not affect playing sounds (`audio.rs:609-627`). No pause, focus-loss or time-scale integration (`engine.rs:674-676` is the only use). Name-keyed cache separate from assets. `stop_music` may not stop playback unless `fade_out_on_drop` was set *(summary only, needs a test)*. Verdict: rewrite on Kira 0.12 tracks: master/music/sfx buses, spatial track with listener, pause/duck on focus loss, handles tied to the asset registry.

**Input (2.3k lines).** Solid raw polling with IME and one-shot suppression, but no action maps, rebinding, buffering, axis abstraction, multi-gamepad, rumble or persistence; hard-coded deadzone 0.15; first gamepad only; a `String` allocated per connected pad per frame (`engine.rs:1032-1038`). Verdict: keep the raw layer, add an action-map layer with rebinding and buffering on top of the new per-frame event buffer.

**Scene and `SceneContext`.** The scene stack, `SceneAction` and `SceneCleanup` are sound. `SceneContext` (`scene.rs:153-238`) is a god object: 14 public `&mut` fields including renderer internals (`lighting`, `raycast`, `device`, `queue`), about 40 methods mixing cursors, window requests, knockback, squash and console registration, constructed by hand in three places in `engine.rs`. `render_ui` receives only `UiContext` and its `SceneAction` is deferred to the next fixed step. `Engine` itself has ~90 fields and takes its renderers out of `Option`s every frame to satisfy the borrow checker (`engine.rs:1661-1665,1870-1874`). Verdict: keep the stack; split the context into focused sub-contexts and give scenes a way to register systems instead of doing everything in `update`.

**Config and save (6.0k lines, ~2.1k tests).** The pure precedence resolver is good. Around it: five parallel layer structs (adding one flag touches ~7 places), two `DebugConfig`s, three `system.json` loaders. Saves and `system.json` are written with `File::create` in place with no temp-file-and-rename (`config/save.rs:1013,1184,1207`), so a crash mid-write corrupts the player's data; `SAVE_VERSION` is checked with strict equality and there is no migration hook (`save.rs:1233`). `WorldState`/`EntitySnapshot` have no users. `config/debug.rs` (1.1k lines) is overlay rendering, not config, with ~240 dead GPU-overlay lines. Verdict: collapse the layers, make writes atomic, add versioned migration, move overlay drawing out.

**Console and dev tools.** The console ships in release behind a runtime flag. `system:ip:wan` does blocking DNS and TCP with 5 s timeouts on the main thread (`console/builtins.rs:415-426`). Command handlers get no world, scene or assets, so game commands cannot do much. Per-frame O(n²) wrapping of up to 1000 lines while visible *(summary only)*. The logger window tails a bounded ring buffer by index and stops updating after 10,000 total entries (`dev_log.rs:160-161,228`, `windows/logger_window.rs:120-172`). The profiler window never removes its puffin sink on close *(summary only)*. `dev_log` is a global `Mutex` with `format!` before the level check and `println!` under the lock, always on in release. Verdict: compile dev tools out of release by feature, remove network calls from the console, give handlers a proper context, and integrate with `log`/`tracing`.

**Events, errors, layering.** `events/builtin.rs` makes `core::events` depend on `inventory` and `network`, and `error.rs` depends on `wgpu`, `image` and `network::replication`: feature modules are reached from the core. At least 14 event types are defined and never emitted (procgen, inventory, state machine, navigation, assets). Error variants are mostly `String` payloads with no `source()` chain. Panics reachable from user code: GPU init (`renderer.rs:205-229`), `TextureManager` asserts, `get_pipeline` unwraps, clip stack asserts, occluder vertex count assert, raycast map size, particle ranges, `Duration::from_secs_f32(dt)` on a negative dt in transitions (`scene.rs:1573`).

**Navigation, procgen, inventory, state machine.** Navigation (3.1k) is a competent grid A* with steering, but nothing drives it; `block_collider` ignores pivot while physics is pivot-aware, so nav grids built from the world are offset for centre-pivot sprites *(summary only)*. Procgen (4.9k) and inventory (1.3k) share almost no types with the engine; inventory `ItemStack` is not serialisable so it cannot be saved. The state machine is small and genuinely shared. Verdict: keep the state machine in core; move procgen and inventory to optional crates; navigation stays only if an engine system drives agents, otherwise it is a library crate too.

**CLI.** Hand-parsed args, templates as string literals, `find_workspace_target_dir` copied three times, hand-written unsafe libc/WinAPI FFI for a timezone in `export.rs`, a 715-line `netplay` tunnel installer, 19 embedded skill files that have drifted from `.claude/skills`. Verdict: fine to leave until the engine settles; dedupe when touched.

**Public surface.** The prelude re-exports about 380 names, including procgen, inventory, networking, raycast and the legacy collision system. A professional core exports far less and lets optional crates add the rest.

---

## Part 3: Dependencies (verified against crates.io on 2026-10-11)

| Crate | Pinned | Latest | Gap |
|---|---|---|---|
| wgpu / naga | 22 | 30.0.1 | 8 majors; surface API (29), `request_adapter` returns `Result` (25), buffer-mapping guards (27), vertex layout changes (30) |
| rapier2d | 0.22 | 0.36.1 | 14 breaking minors; BVH broad phase (0.27), solver rewrite (0.29), nalgebra to glam (0.32), soft bodies in `step` (0.36) |
| glam | 0.31 | 0.34.1 | 3 |
| rand | 0.8 | 0.10.3 | 2 (`Rng` renamed `RngExt`, `gen_range` to `random_range`) |
| kira | 0.11 | 0.12.5 | 1 (format feature flags mirror Symphonia) |
| winit | 0.30 | 0.30.13 (0.31 in beta) | 0 |
| hecs | 0.11 | 0.11.2 | 0 |
| notify, resvg, tiny-skia, etagere, puffin | 7 / 0.44 / 0.11 / 0.2 / 0.19 | 8.2 / 0.48 / 0.12 / 0.3 / 0.20 | 1-4 each |
| noise | 0.9 | 0.9 | dormant since 2024 |

The wgpu and Rapier gaps are large enough that the renderer and physics rewrites should target current versions directly rather than migrating the existing code first. Text rendering: `glyphon 0.12` on `cosmic-text 0.19` is the practical wgpu default; `parley` is the ecosystem direction but not drop-in yet.

---

## Part 4: What is good and should be kept

- The test culture: 2,458 passing tests including 131 real GPU pixel tests, `HeadlessEngine`, frame capture, `RYO_INPUT` scripting and the demo tour. This is unusual and valuable. (Caveat: GPU tests skip silently without a GPU, and the letterbox tests exercise a copy of the real function.)
- The render pass list with latest-image tracking, the z-sorted draw list with run merging, and the atlas page design.
- The scene stack, `SceneAction` and `SceneCleanup`.
- The pure settings resolver and its tests.
- The timing accumulator with a step cap and the sleep-plus-spin frame limiter (once wired to the right clock).
- The nested `AGENTS.md` discipline and the skills: this is the agent-first part of the vision working.
- Code hygiene: clippy-clean, one `unsafe` in core, few unwraps in hot paths, consistent formatting.

---

## Part 5: Recommended order and the decisions to make first

The six foundations depend on each other, so order matters:

1. **Decide the ECS** (bevy_ecs standalone vs hecs plus a bespoke scheduler/change-detection/hierarchy layer). Everything below touches it.
2. **Rebuild the loop and clocks**: Real/Virtual/Fixed, single loop, per-step interpolation snapshot, frame-scoped input events with consumption, double-buffered events, Rapier dt from the fixed step. This is small in lines and fixes six verified bugs.
3. **Redesign Transform/GlobalTransform/hierarchy** on the chosen ECS, with propagation after simulation and dirty tracking.
4. **Renderer core** on wgpu 30: camera uniform, texture handles, one 2D batch, HDR target plus tonemap, logical/physical resolution model, blend and filter modes. Lighting, trails, tiles, particles and UI then become clients of that batch.
5. **Physics wrapper** on Rapier 0.36 with the full feature set and change-detected sync.
6. **UI and text**: cosmic-text, widget ids, layout, gamepad focus, GPU drawing.
7. **Asset registry** with typed handles and loaders; audio rewritten on Kira tracks and tied to it.
8. **Feature modules**: tilemap (rewrite), lighting shadows (rewrite), trails (fold in), particles (fix), skeleton (finish or cut). Cut raycast, procgen and inventory to optional crates. Delete the legacy collision system, dead events and dead overlay code.
9. Networking: untouched until the separate rethink.

Decisions the owner should make before step 1 starts: the ECS choice; whether pixel-art-first (integer snapping, low-res target) or resolution-independent is the default; whether the immediate-mode UI API stays as the public surface; whether WebGPU/wasm is a target (it changes storage-buffer and `Send + Sync` choices in the renderer).
