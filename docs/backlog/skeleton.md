# Skeleton: speed, allocations and limits

## Measured

What is measured: skeletons against the spec's targets (`docs/specs/2026-11-12-skeleton-design.md`, Targets), by `cargo bench --bench skeleton` (release) and by the allocation test `fifty_animated_skeletons_allocate_nothing_per_frame` in `tests/tilemap_alloc.rs`.

Machine: Apple M5 Pro, 18 cores, shared with other work.

The scene: 1920 x 1080 headless (a GPU), the camera over the view, ambient 0.3 under one point light of 1,400 px, so every skeleton is lit. 200 skeletons in a grid of 20 x 10 cells of 96 x 108 px that fills the view. Each is 30 bones (a spine of 10 and 20 more hanging off it in chains of up to 4) and 25 region slots on distinct bones, each showing a 12 x 12 px region, drawn from one atlas, on an interpolated entity (a `Skeleton` makes its entity `Interpolated`), so every bone is drawn between its last two steps. Two tracks play: track 0 a looping base (`idle` or `walk`, rotating all 30 bones with a bezier on each key, translating or scaling some, tinting five slots) and track 1 a looping layer (`wave` or `aim`, rotating 10 or 12 bones). Each animation has event keys, two a loop, so events fire all the time. The default mix is 1 s. In every frame four calls start on different skeletons (`set_animation` on each track, `add_animation` on the base track, `set_empty` on the layer, one kind each 15 frames a skeleton), so a call and a new mix are inside every figure, and a crossfade is always in progress on both tracks. A frame is 1.3 fixed steps long, so the frame is drawn between two steps (the interpolation factor is never 0). Each case warms 90 frames, then measures 300; each figure is the median of the steps (update) or frames (extract) of one run, and the figure recorded is the median of three runs.

What each figure covers:

- `skeletons_update`: the fixed step's skeleton work (`FixedSet::Skeletons`: `update_skeletons`), read between two probe systems ordered round the set. Per fixed step, 200 skeletons.
- `skeletons_extract`: the entity drawer's `collect` (the quads of every skeleton, their runs in the draw list, the shadows and the culling), `HeadlessEngine::entity_draw_time`, per frame. This is the spec's "drawing work in extract". `skeletons_render` beside it is the render step's CPU time as a whole (extract and the encoding and uploads), for context.
- `skeletons_extract_shadow`: the same with a drop shadow on every skeleton (a second quad for each region).
- `skeletons_extract_offscreen`: the same crowd standing wholly outside the view (10,000 px to the right).
- `skeletons_extract_images`: an image of its own for each of the 25 parts (25 runs a skeleton), where the others draw from one atlas (one run).
- `one_skeleton_update`: the same step with one skeleton. `no_skeleton_update` is the same probes with none, the floor `one_skeleton_update` has in it (two clock reads and a system's run).
- `skeletons_update_sockets` and `skeletons_extract_sockets`: the crowd with a `BoneSocket` on the 30th bone of every skeleton and a child on each socket, so 200 sockets move every step. (The socket's subtree is computed by propagation in the physics step, in the step, by the skeleton walk and in `PostUpdate`; this figure includes the step's and the skeleton walk's.)
- `skeleton_pass_200` and `skeleton_pass_2000`: the frame pass that poses new skeletons (`pose_new_skeletons`) in a steady frame, per call, run outside a frame on a system initialised once (`HeadlessEngine::skeleton_pass_time`), with 200 and with 2,000 skeletons.
- `skeleton_pass_socket_spawn`: the same pass in a frame in which a socketed entity was spawned (the frame the pass looks through every skeleton and socket), with 200 skeletons present and none carrying a socket; `skeleton_pass_socket_spawn_sockets` with 200 skeletons each carrying one. The median of 60 spawns, each undone before the next.

### The record

One-minute load before each run (`uptime`, the first line of each log in the scratchpad's `skeleton9-bench-run-N.log`): run 1 5.85, run 2 5.46, run 3 5.46 (the last two readings were identical: the clock had not moved). Every one is above 3 and so flagged: the machine was shared, and nothing waited for a quiet one. The figures did not move with the load (the spread of the guarded ones is under 5%), and every target is met by a factor of two or more, so a quiet machine is not expected to change a verdict.

Medians in microseconds:

| Bench | Run 1 | Run 2 | Run 3 | Median of runs | Target |
|-------|-------|-------|-------|----------------|--------|
| `skeletons_update` | 498.3 | 500.2 | 513.8 | 500.2 | 1,000 |
| `skeletons_extract` | 164.0 | 161.5 | 168.9 | 164.0 | 600 |
| `skeletons_render` (context) | 331.8 | 326.8 | 389.3 | 331.8 | |
| `skeletons_extract_shadow` | 189.9 | 187.1 | 190.2 | 189.9 | 600 |
| `skeletons_extract_offscreen` | 159.8 | 157.4 | 160.6 | 159.8 | 600 |
| `skeletons_extract_images` | 282.5 | 280.2 | 284.0 | 282.5 | 600 |
| `one_skeleton_update` | 3.0 | 3.0 | 3.1 | 3.0 | 5 |
| `no_skeleton_update` (the floor) | 0.2 | 0.2 | 0.2 | 0.2 | |
| `skeletons_update_sockets` | 511.6 | 506.8 | 508.8 | 508.8 | 1,000 |
| `skeletons_extract_sockets` | 163.9 | 162.0 | 164.4 | 163.9 | 600 |
| `skeleton_pass_200` | 0.07 | 0.07 | 0.07 | 0.07 | |
| `skeleton_pass_2000` | 0.50 | 0.50 | 0.50 | 0.50 | |
| `skeleton_pass_socket_spawn` | 2.6 | 2.6 | 2.6 | 2.6 | |
| `skeleton_pass_socket_spawn_sockets` | 7.8 | 7.9 | 7.7 | 7.8 | |

The update's step of the other cases (shadow, offscreen, images) read 494 to 503 µs, as the crowd's: the update does not depend on how the skeletons are drawn.

Every guard in the bench (`skeletons_update`, the five extract cases, the two socket cases, `one_skeleton_update`) is met in every run, and the bench exits non-zero on a miss (`guards::miss`, `guards::verdict`). No guard was loosened; no optimisation was needed.

### Reading the figures

- **Update:** 2.5 µs a skeleton (500 / 200), half of the 5 µs target; one skeleton alone is 3.0 µs, of which 0.2 is the probes. A socket with a child on each skeleton adds 8.6 µs to the 200 (1.7%).
- **Extract:** 0.82 µs a skeleton, or 33 ns a region (164 / 5,000), with every bone interpolated. A temporary run of the same scene without `Interpolated` read 105 µs, so drawing between steps (the bone blend, in `draw::blend`) costs about 55 µs of the 164, or 11 ns a region. That scene is now a case of the bench, `skeletons_extract_plain`, which takes `Interpolated` off every skeleton (a skeleton adds it, so leaving it out of the spawn no longer measures a plain entity); its figure is not among those recorded above. The drop shadow adds 26 µs (16%), the second quad of each region. The three costs the carried review notes named (a `HashMap` lookup of the attachment by name per region, `sin_cos` per region, and `blend` per slot) are inside the 33 ns and none broke a target, so none was changed.
- **Off-screen:** a crowd wholly outside the view costs 160 µs, 97% of an in-view one: every quad is built before the run is culled by its box. That is within the 600 µs target by a factor of 3.7 and was left; skeleton-level bounds (skip a skeleton whose box misses every camera before the quads) would take most of the 160 off, and are the lever if a game puts several thousand skeletons off screen (at about 0.8 µs each, 1,000 off-screen skeletons cost about 0.8 ms of extract).
- **An image for each part:** 283 µs against 164: 25 runs a skeleton instead of one (the culling, the texture lookup and the pipeline key are paid per run), 73% more, and 47% of the target. One atlas is the way to draw a character.
- **The frame pass:** a steady frame costs 0.07 µs at 200 skeletons and 0.50 µs at 2,000, so the pass's one cost that grows with the skeletons (the change-tick compare of each renderable) is nothing yet. A frame in which a socketed entity is spawned looks through the 200 skeletons and costs 2.6 µs, or 7.8 µs with a socket on each.
- **Frame, whole:** the render step's CPU time is 332 µs for the 200 skeletons, drawing included.

### Allocations

`fifty_animated_skeletons_allocate_nothing_per_frame` (`tests/tilemap_alloc.rs`, a debug-or-release test binary with a counting global allocator): 50 skeletons of 12 bones and 10 region slots, lit, on `Interpolated` entities, each playing two tracks, with events firing (two or three keys a loop), a `BoneSocket` with a child on each, and a mix started in every frame on some (`set_animation` and `add_animation` on the base track, `set_animation` and `set_empty` on the layer; each skeleton makes one such call every third frame and each kind every twelfth). 240 frames of warm-up, then 12 traced frames, each traced from the calls through the step, so a call's allocations count. It is compared with the same entities as plain sprites (a `Renderable::image` where the skeleton's is, and children with no socket) in a second engine, site by site, and asserts no allocation of the engine's own the sprites' frame does not make, none with a function of the skeletons' path on its stack, and none inside the GPU API from that path. The skeletons' path is the skeleton module and the entity drawer's skeleton functions (`collect_skeleton`, `Emitter::draw_runs` and `draw_shaded`), which are outside the module: a buffer made in `collect_skeleton` for each skeleton, planted to try the test, fails it (200 allocations inside the GPU API a frame).

It passed at once (not red first): the skeleton code allocated nothing in steady state, as the module notes say. That it can fail was checked by planting an allocation in `SkeletonAnimation::set_empty`, in `update_skeletons` and in `Skeleton::regions`, one at a time: each was named by the test (4, 1 and 50 allocations a frame at those sites), and each plant was removed.

Both frames make about 229 allocations of the engine's own (390 in all; the rest are inside the GPU API), and the same in both. These are not the skeletons': the entity drawer builds its `QueryState` for the entities every frame (`world.query::<DrawableItem>()` in `EntityDrawer::collect`), which allocates a few dozen times for each archetype it matches (the test's sprite frame has them too). That is true of every scene with a `Renderable`, a sprite scene or a skeleton one, and is older than skeletons; it is listed under Limits.

### Changes the bench made to the engine

Two hooks, `#[doc(hidden)]` in `src/core/testing/headless.rs`: `HeadlessEngine::entity_draw_time` (the last frame's `EntityDrawer::collect` time, written around the call in `src/core/engine/extract.rs`, two clock reads a frame) and `HeadlessEngine::skeleton_pass_time(calls)` (runs `pose_new_skeletons` on a system initialised once, outside a frame).

## Limits

Each was copied from the module notes (`src/core/skeleton/AGENTS.md`) and restated here for a reader of the backlog; the notes' test names stay with the rules there.

### Data

- **At most 255 bones and 255 slots** a skeleton (`BoneId(u8)`, `SlotId(u8)`): `bone_id` and `slot_id` give `None` past the 255th, and a file or builder with more fails to build.
- **A non-finite value in a file fails to load**; in a bone override, a non-finite value is ignored with one warning.
- **An animation has at most one timeline per property of a bone or slot** (two rotate timelines on one bone fail, and so do a combined translate and a translate x), at most one draw-order and one event timeline; a bezier's x controls lie in 0..=1; a draw-order key lists every slot once; a region is not empty and a bounding box has at least three vertices; no animation is named `<empty>`.
- **Spine's model is followed in part**: no path or physics constraints, no clipping attachments, no Spine binary format (the spec's non-goals). The differences in mixing from Spine's documented behaviour (the crossfade does not dip toward the tracks below, `hold_previous` holds time, the queue's delay, `complete` only on the current entry, `clearTrack` returning to setup, the stepped-key thresholds) are listed in the notes ("Known differences"). Not offered: a track end time, queued empty animations (`addEmptyAnimation`), the attachment and draw-order thresholds, `shortestRotation` and `resetRotationDirections`.
- **Inherit modes act in skeleton space**: the entity's transform is applied to every bone after the pass, so turning, scaling or flipping the entity does the same to an only-translation or no-rotation bone, and under "no rotation or reflection" a parent's shear is dropped.

### The animation state

- **Tracks are numbered 0 to 63** (`MAX_TRACKS`); a higher number warns once per skeleton and does nothing.
- **A chain holds at most 16 entries** (`MAX_CHAIN`: the current one and fifteen it is mixing from). A seventeenth ends the oldest whatever weight it has left: 0.7% under a 0.5 s mix when a state machine sets a new animation every step at 60 Hz, about 11% under a 1 s mix. A skeleton whose clock is stopped, with a chain that cannot finish, jumps to the next oldest.
- **Track time is an `f32` that counts every loop**: a looping entry left playing for days moves in coarser steps (2 ms after 4.6 hours, 8 ms after 18 hours, 31 ms after 3 days, where a 60 Hz step rounds to one and the animation runs near twice as fast), and after 6 days a 60 Hz step no longer moves it and the entry stops. Setting the animation again starts its time from 0.
- **The rotation memory winds**: a partial-alpha layer's remembered turn grows by 2π a revolution, so its rotation moves in steps of 0.45° (times its alpha) after 4.5 hours of a turn a second, and its resting angle depends on its history (alpha × k × 360° off a fresh entry's, after k windings). Setting the animation again starts it afresh.
- **One span fires the keys of at most the last 1,000 loops** (`MAX_LOOPS`, 1,001 on a boundary); the loops before are not fired.
- **Events from calls wait in a list of at most 1,024** (`MAX_TOLD`): past that the oldest are dropped, with one warning per skeleton.
- **A track queues at most 64 entries** (`MAX_QUEUE`): `add_animation` past that does nothing and returns `None`, with one warning per skeleton; kept calls that land on a full queue are disposed.
- **`set_empty` every step restarts once a fade**: a call while the track is already fading out changes nothing, so the fade ends on time; once it is over the track is clear, and the next call starts an empty entry afresh that lasts its mix, with a `Start`, an `End` and a `Dispose` (every step only at a mix of 0).
- **Calls before the data lands**: at most 256 are kept (`MAX_KEPT`); one more is ignored and returns `None`, with one warning per skeleton. A kept entry that a later call drops, or whose name the data lacks, ends with a `Dispose` alone.
- **The spare buffers are bounded**: a finished entry's buffers are kept to at most 16 (`MAX_SPARE`), so a steady stream of crossfades allocates nothing after the first few, but a burst of more than 16 in flight at once allocates for the extra.
- **A skeleton remembers at most 64 names it has warned about, and its animation state 64 more** (`MAX_WARNED` in each; past that a missing name is ignored in silence), and a skeleton keeps at most 255 attachments and 255 overrides (one a slot or bone); before the data lands no name can be checked, so more are ignored with one warning.
- **The empty animation sends no `Complete`**: a game that wants "the fade is over" listens for the `End`.
- **A start of whole loops, saved again before its first step, fires the key at 0 twice.** `track_time()` reads the start wrapped once the data is known, so an entry started at a whole number of loops reads back 0.0 until it has stepped; a game that loads such an entry, saves again while no step runs and restores from that save gives a start of 0, which fires the key at 0. Every other entry restored with `start = track_time()` fires no key it had fired. Keeping the start as given until the first step would close it.
- **Skeletons run on game time only**: there is no unscaled mode, so a menu's skeleton stands still while the game clock is paused.
- **A track entry's handle is unique within its own `SkeletonAnimation` only**: two skeletons' handles can be equal, so a game matches a `SkeletonTrackEvent`'s `entity` as well as its `entry`.
- **A reload that changes an animation's length is followed only in part** (development-time only): entries keep their track times, so a looping entry whose animation's duration changed jumps to another phase of it; a non-looping entry already past a shortened duration never sends `Complete` (`is_complete()` turns true); and a queued entry's `delay`, worked out from the old duration when it was queued, is not worked out again.

### In the world

- **A scene's `update` does not see what a skeleton sends during a scene transition.** Skeletons keep animating through one, as particles and physics do, while `Scene::update` waits and messages last two steps. A scene released from under a menu that pops with a fade is the sharp case: its held skeletons are released at the fade's midpoint, and what they send until the fade ends is not seen by `update`. A scene system sees it; so the skill tells a game to read skeleton messages in a scene system, queue what follows with `add_animation`, or ask the state.
- **A steady frame's pass looks through no skeleton, but compares one change tick for each skeleton that has a renderable** (0.07 µs at 200, 0.5 µs at 2,000 above). It looks through every skeleton and socket in a frame in which a `Skeleton` or `BoneSocket` was added or the data's store gave a version.
- **Not covered by the frame's pass**: a skeleton enabled again (`Disabled` taken off) between steps keeps the pose it had until its next step, and so does its flip if its `Renderable` was flipped while it was disabled or taken off while no step runs; a socket given to a skeleton by `ChildOf` alone while no step runs is placed in the next step or the next frame that looks; a `Skeleton` assigned whole through a mutable reference is not an insert (no scene is recorded and no look follows).
- **A moving socket's subtree is computed up to four times a step** (the physics step's propagation, the step's, the skeleton walk, and `PostUpdate`'s). Measured above with one child a socket and 200 sockets: 8.6 µs of 500. Many sockets can tip propagation into its full walk (`propagate.rs`); not measured beyond that.
- **A `Transform` has no shear**, so a skew in a bone's skeleton-space transform (a sheared bone, a turned bone under an unevenly scaled one) is dropped from its socket: the socket points along the bone's x axis, as long as it, and as tall as the bone is across it, mirrored when the bone is. The skeleton entity's own transform is applied whole, so an unevenly scaled and turned entity skews its sockets as it skews its bones. Without `inherit_scale` a socket takes the bone's place and direction only.
- **Read from a scene, a skeleton is one step behind**: `Scene::update` and a scene's own systems run before `FixedSet::Skeletons`, so `bone_world`, `point`, `bounding_box` and `attachment` are the last step's there and an event is read a step after its key. A `BoneSocket`, or a system added with `EngineBuilder::add_systems` after `FixedSet::Skeletons`, is in step; a scene has no hook after the skeletons' step.
- **A skin adds or replaces an attachment and cannot hide one**; a game hides a slot with `set_attachment(slot, None)`.
- **An event's audio is the game's to play**: the event carries a path, a linear volume and a balance, and a game loads the paths and plays them itself; `PlayOptions` has no pan, so `balance` has nowhere to go.
- **A sprite on a socket draws wholly in front of the skeleton or wholly behind it**, being an entity of its own: an item the hand must wrap is a region attachment in a slot.
- **Colliders on a socket follow the bone a physics step late**, since physics runs before propagation. An ordinary child of the skeleton's entity (not a socket, nor below one) is not reached by the entity's `skip_interpolation`.
- **Not taken: the bone's exact matrix in `GlobalTransform`**: propagation re-places a child from its `Transform` whenever its parent moves.
- **A bone the skeleton lacks warns once** per name for a socket and leaves the entity where it is.
- **Skeletons step in entity order**, so one riding another's socket may be posed before its socket moves; it is placed again after every skeleton has stepped.

### Drawing

- **A skeleton's entity is interpolated** (`Skeleton` adds `Interpolated`), its own place with its bones; a game removes the component for a skeleton that animates at the step rate, and a bundle cannot leave it out. The blend is of each bone's transform in skeleton space, not of the local poses, so a child of a bone turning fast draws a chord's sagitta off its parent's tip mid-step (a 60 px bone turning 10 degrees a step: about 0.2 px).
- **Every quad is built before its run is culled** (the off-screen figure above). There are no skeleton-level bounds; a crowd off screen costs nearly what one in view does, about 0.8 µs a skeleton.
- **A run is a stretch of consecutive quads of one image and one blend**, so a skeleton of an image a part draws one run a part (283 against 164 µs above): use an atlas. With a game shader on the entity each quad is a sprite item of its own instead.
- **Where parts overlap, a drop shadow is darker than a sprite's uniform one**, each region's shadow laid over the last (a half black shadow leaves 187 of a white floor once and 136 where two parts overlap). A uniform silhouette needs the skeleton drawn offscreen first, which is not built; a character should use a blob or contact shadow. The blob and contact shadows stand where the entity does, not following the pose's bounds. `Renderable`'s `image`, `anchor` and `target` are not used by a skeleton, and a `YSort` sorts it by its entity's y plus `sort_offset` (`foot_offset` plays no part).
- **A skeleton with no pose for its data draws nothing, shadows included**: reached only by the case the frame's pass does not cover (enabled again between steps after its data changed).
- **A game shader's `in.uv` runs over the part's own rectangle of its image**, not the figure.

### Found while measuring, not skeleton's

- **The entity drawer builds a `QueryState` every frame** (`world.query::<DrawableItem>()` and the composites' in `EntityDrawer::collect`), which allocates a few dozen times a frame for any scene with a `Renderable`, more with each archetype that holds one (about 229 allocations of the engine's own in the allocation test's frames, skeletons or sprites alike). It is older than skeletons and the same for sprites; keeping the states in the drawer would remove it.

## Later

- **Holding a skeleton while its scene's `update` waits on a transition.** The other answer to the limit above (In the world): a skeleton whose scene is in a transition would be held, as it is under a scene that stops its own, so that nothing is sent while `update` cannot hear it. It was not taken because it changes how every fade looks (characters stand frozen under it) and parts skeletons from particles and physics, which run on through a transition; holding all three, or letting `update` run during a transition, is an engine-wide decision. The owner's call.
- **A map from (target, property) to timeline for each animation, and an index of its event timeline.** Two searches are linear in an animation's timelines. When a mix starts, each timeline of the outgoing entry looks for the incoming animation's timeline that keys the same properties (`apply_track`'s cover), which is quadratic in timelines, once an entry a mix. And `events_crossed` scans the animation's timelines for its event timeline, every entry every step. Both are negligible for the bench's rig (about 40 timelines an animation). Inferred, not measured: 200 skeletons whose animations have 150 timelines each, all switching animation in one step, would spend about 7 ms on the cover search in that step. A per-animation map built with the data (and the event timeline's index kept beside it) makes both a lookup.
- **What the data cannot yet express, for an importer of another tool's files:** a slot's colour keyed as separate rgb and alpha timelines (one `Color` timeline keys all four here); a curve for each channel of a colour key (one curve a key here); and atlas regions packed rotated (a region here is an upright rectangle of its image). Check each against the tool's public format documentation when that stage is built, not against a runtime's source.
- **A uniform drop shadow, from an offscreen pass.** A skeleton's drop shadow is each region's own silhouette laid over the last, so a translucent one is darker where parts overlap (Limits, Drawing). An even silhouette, as a sprite casts, needs the skeleton drawn offscreen first: its regions into an image (or a mask) of their own, sized by the pose's bounds, which is then drawn once as the shadow. That is new renderer work (a target or an atlas cell for each shadowed skeleton a camera sees, redrawn as the pose changes, and a pass before the world's), with a cost for each shadowed skeleton, so it was left out and characters use blob or contact shadows, which match a sprite's. The same image would give a figure-wide outline or glow, where a sprite shader applies part by part.
