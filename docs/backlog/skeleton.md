# Skeleton: speed, allocations and limits

## Measured

What is measured: skeletons against the spec's targets (`docs/specs/2026-11-12-skeleton-design.md`, Targets), by `cargo bench --bench skeleton` (release) and by the allocation tests `fifty_animated_skeletons_allocate_nothing_per_frame`, `fifty_skeletons_with_a_deforming_mesh_allocate_nothing_per_frame`, `fifty_skeletons_with_constraints_allocate_nothing_per_frame` and `fifty_skeletons_with_springs_sounds_and_the_ik_changes_allocate_nothing_per_frame` in `tests/tilemap_alloc.rs`. Meshes, constraints, and springs with culling and the IK changes have sections of their own below (Meshes, Constraints, Springs, culling and the IK changes), each with the comparison of the figures without them against the build before them.

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

### A second record, on a quiet machine

Three more runs, on the same machine with the branch complete (`cargo bench --bench skeleton`, release). One-minute load before each run: 1.83, 2.02 and 2.10, none above 3, so none is flagged. Medians in microseconds:

| Bench | Run 1 | Run 2 | Run 3 | Median of runs | Target |
|-------|-------|-------|-------|----------------|--------|
| `skeletons_update` | 490.1 | 481.4 | 479.8 | 481.4 | 1,000 |
| `skeletons_extract` | 162.8 | 150.3 | 150.3 | 150.3 | 600 |
| `skeletons_render` (context) | 365.2 | 316.4 | 318.0 | 318.0 | |
| `skeletons_extract_shadow` | 185.8 | 176.3 | 175.0 | 176.3 | 600 |
| `skeletons_extract_offscreen` | 155.5 | 145.8 | 145.6 | 145.8 | 600 |
| `skeletons_extract_images` | 282.9 | 262.9 | 279.7 | 279.7 | 600 |
| `skeletons_extract_plain` (no `Interpolated`) | 101.5 | 92.5 | 99.0 | 99.0 | |
| `one_skeleton_update` | 3.7 | 3.8 | 3.5 | 3.7 | 5 |
| `no_skeleton_update` (the floor) | 0.3 | 0.3 | 0.2 | 0.3 | |
| `skeletons_update_sockets` | 496.5 | 492.9 | 524.2 | 496.5 | 1,000 |
| `skeletons_extract_sockets` | 151.4 | 150.5 | 160.4 | 151.4 | 600 |
| `skeleton_pass_200` | 0.07 | 0.07 | 0.07 | 0.07 | |
| `skeleton_pass_2000` | 0.48 | 0.47 | 0.50 | 0.48 | |
| `skeleton_pass_socket_spawn` | 2.7 | 2.6 | 2.5 | 2.6 | |
| `skeleton_pass_socket_spawn_sockets` | 7.5 | 8.2 | 7.9 | 7.9 | |

Every guard is met in every run. Against the first record, the crowd's update and extract read a little lower, and `one_skeleton_update` reads 3.7 µs where it read 3.0: the code changed between the two records (queue and warning bounds in the animation state, a start offset, readbacks), and which change accounts for the 0.7 µs was not measured. `skeletons_extract_plain` is the figure the first record lacked: without `Interpolated` the drawing work is about 99 µs against 150 µs with it, so the bone blend costs about 51 µs of a frame for 200 skeletons.

### Meshes

The scene is the crowd's, each skeleton with one mesh more: a ribbon of 60 vertices (30 rows of two, 58 triangles) along the spine, each vertex held by two bones (two influences a vertex, so 120 positions), in a slot of its own after the 13th region. So a skeleton draws a run of 13 quads, the mesh, and a run of 12 quads: three items where a skeleton of regions is one, and the meshes cannot merge with anything. Both of the base track's animations key every position of the mesh (three full keys each, a bezier on each, by different amounts), so a deform is applied in every step and two are blended through every crossfade of that track, which is always in progress. The bench checks the case is the one it says, and counts a miss if not: the frame's draw calls (two a mesh at least) and a deform moving the mesh of three skeletons in four at least (191 of 200 stood off their setup shape when it was read).

What each figure covers:

- `skeletons_update_mesh` and `skeletons_extract_mesh`: the crowd's two figures with the mesh, guarded at the same targets (1,000 and 600 µs). The update has the deform keys' blending in it; the extract has the skinning (each bone the mesh uses placed once a skeleton, 120 weighted positions summed, each moved by its offset first) and the mesh stream.
- `skeletons_extract_mesh_shadow`: the same with a drop shadow on every skeleton, so two mesh items a skeleton, the shadow's and the body's. It is held to the extract's 600 µs too, as the shadow case of regions is.
- `skeletons_prepare*`: `Batch2D::prepare` for the frame (`HeadlessEngine::batch_prepare_time`): every instance placed in its atlas slot, the draws planned for the camera, and the instances and the mesh stream uploaded.
- `skeletons_world_pass*`: the camera's world pass as it is recorded (`begin_render_pass`, the `Batch2D::draw` loop and the pass's end), between two probe passes the bench puts round it. wgpu only records there: it checks and encodes the pass's commands at the encoder's `finish`, so that part of a mesh's cost is in `skeletons_render*` alone, and shows as "the rest" below (the render step less the extract, the prepare and the recording).
- `skeletons_draws*`: the frame's draw calls, the light's one among them.
- `skeleton_mesh_cost` and `skeleton_mesh_cost_shadow`: what one mesh draw adds to each of those, from the mesh case less its regions-only twin, over 200 and over 400 mesh draws.
- `skeleton_first_shader_frame` and `skeleton_first_shader_frame_mesh`: the render step of the frame in which the whole crowd first draws through a game sprite shader, of regions alone and with the mesh. Each is the median over five shaders, set one after another, of the longest of the 12 frames after each is set: first with sources made afresh each run (new to the machine, so its GPU shader cache does not hold them), then with the same sources a second time (cached), against the median frame once the shader draws.

#### The mesh record

Three runs of the bench (`cargo bench --bench skeleton`, release), each run after a run of the bench of the build before meshes (the comparison below). One-minute load before each run: 5.29, 5.03 and 4.48, every one above 3 and so flagged: the machine was in use and would not go quiet all afternoon. Medians in microseconds (draw calls for the last four rows):

| Bench | Run 1 | Run 2 | Run 3 | Median of runs | Target |
|-------|-------|-------|-------|----------------|--------|
| `skeletons_update_mesh` | 594.2 | 595.7 | 600.8 | 595.7 | 1,000 |
| `skeletons_extract_mesh` | 282.4 | 285.1 | 286.9 | 285.1 | 600 |
| `skeletons_extract_mesh_shadow` | 363.6 | 364.3 | 362.1 | 363.6 | 600 |
| `skeletons_render` (context) | 330.9 | 339.3 | 330.9 | 330.9 | |
| `skeletons_render_mesh` | 643.8 | 647.3 | 646.3 | 646.3 | |
| `skeletons_render_shadow` | 436.7 | 430.3 | 429.5 | 430.3 | |
| `skeletons_render_mesh_shadow` | 966.7 | 968.5 | 949.5 | 966.7 | |
| `skeletons_prepare` | 76.7 | 77.6 | 76.4 | 76.7 | |
| `skeletons_prepare_mesh` | 139.2 | 140.0 | 136.8 | 139.2 | |
| `skeletons_prepare_shadow` | 149.5 | 146.2 | 145.0 | 146.2 | |
| `skeletons_prepare_mesh_shadow` | 251.7 | 255.9 | 251.6 | 251.7 | |
| `skeletons_world_pass` | 0.8 | 0.8 | 0.8 | 0.8 | |
| `skeletons_world_pass_mesh` | 24.8 | 25.6 | 25.6 | 25.6 | |
| `skeletons_world_pass_shadow` | 0.8 | 0.7 | 0.8 | 0.8 | |
| `skeletons_world_pass_mesh_shadow` | 49.9 | 49.0 | 47.2 | 49.0 | |
| `skeletons_draws` | 2 | 2 | 2 | 2 | |
| `skeletons_draws_mesh` | 402 | 402 | 402 | 402 | |
| `skeletons_draws_shadow` | 2 | 2 | 2 | 2 | |
| `skeletons_draws_mesh_shadow` | 802 | 802 | 802 | 802 | |

The three guards are met in every run, by 40%, 52% and 39%, and so were they in the 25 other runs of the bench with these cases made that day, before the change below and after it, at loads of 2.8 to 7 (update 572 to 640 µs, extract 278 to 302 µs, the shadowed extract 352 to 395 µs); every other guard of the bench was met in each of those too. The shadowed case was given its guard after the record was read; the two runs made with it in the bench read 375.7 µs at a load of 6.2 and 357.0 µs at a load of 2.8. That second run is the one run of the day whose load was under 3 before it and after it (2.80 and 2.90), so it is the one not flagged: `skeletons_update_mesh` 572.1 µs, `skeletons_extract_mesh` 281.3 µs, `skeletons_extract_mesh_shadow` 357.0 µs, and for regions alone `skeletons_update` 461.7 µs and `skeletons_extract` 158.8 µs. Guards were missed only in runs made while a build was running or had just ended: `one_skeleton_update` read 6.5 and 11.5 µs against its 5 at loads of 11 and 13, and with every core building it read 13.6 and `skeletons_update_mesh` 1,090 against its 1,000. That is the load's doing, not a miss on a quiet machine. No guard was loosened.

#### What a mesh costs

From the medians above, a mesh case less its regions-only twin, in microseconds a mesh draw:

| | Extract | Prepare | World pass, recorded | The rest of the render step | Render step |
|---|---|---|---|---|---|
| A mesh between two runs of quads (200 a frame) | 0.61 | 0.31 | 0.12 | 0.53 | 1.58 |
| A mesh draw under a drop shadow (400 a frame, two a skeleton) | 0.44 | 0.26 | 0.12 | 0.52 | 1.34 |

- **A mesh of 60 weighted vertices between two runs costs about 1.6 µs of a frame's render step** (1.5 to 1.8 over the runs), and its deform about 0.55 µs of a step's update for a skeleton (596 against 489 µs for the 200, with two deforms blended through the crossfade). A character of three such meshes is about 5 µs a frame; a hundred of them half a millisecond.
- **Extract, 0.6 µs**: the skinning (120 influence positions, each moved by its blended offset, placed by its bone and weighted), the 60 vertices and 174 indices read into the mesh stream, the culling box, and the two runs the mesh splits one run into. The shadow's mesh is cheaper (0.44): it is skinned once and cast.
- **Prepare, 0.3 µs**: the mesh's instance placed, three draws planned where one was, and the stream's upload, 1.7 kB a mesh (200 meshes are 330 kB a frame through two `write_buffer` calls).
- **The world pass, 0.12 µs recorded and about 0.5 µs more at the encoder's finish**: a mesh between two runs is 402 draws where the crowd of regions is 2 (the skeletons' one and the light's). Recorded, a mesh and the quads after it are two pipeline sets, the stream's vertex and index buffers and the quads' vertex buffer again, the two instance slices and two draws. "The rest" is that part of the render step the three figures before it do not cover; what a mesh adds to it is wgpu checking and encoding those commands, and the Metal backend's four allocations for the two pipeline sets (Allocations, below).
- **Keeping bind groups across the two pipelines, and setting the index buffer once, was measured and is not worth a rule of the renderer**: with both, the recording read 23.5 µs against 25.9 for the 200 meshes (three runs each, alternately, loads 3.3 to 3.6), and 43.8 against 49.2 for the 400 under a shadow: 0.012 µs a mesh, under 1% of what it costs, and nothing that showed in the render step. The pass sets its bind groups again after every pipeline change (render module notes), but wgpu drops a bind-group set that repeats the last one at its index as it records it (`set_and_check_redundant` in `wgpu_core::command::render`), so the four repeats a mesh are a call and a compare each and are never checked or encoded.
- **The lever that is left is one pipeline for quads and meshes** (Later): it would take the two pipeline sets and their allocations out of every mesh, at the cost of a vertex stage that serves both.

**The frame a game shader first draws through.** In milliseconds, the medians of three runs (the same three):

| | A shader new to the machine | A shader its GPU cache holds | A frame once it draws |
|---|---|---|---|
| The crowd of regions | 87.0 | 2.28 | 0.62 |
| The crowd with the mesh | 90.6 | 3.97 | 0.94 |

A mesh has a pipeline of its own for a game shader (the shader is checked once for quads and once for meshes), made in the frame it first draws. That costs about 1.7 ms more when the machine has compiled the shader before, and nothing that can be told from the noise when it has not, since the two pipelines share the fragment stage, which is what takes the time. The 87 ms is not the meshes': it is the first use of any game sprite shader on a machine, compiled on the frame's own thread (Found while measuring, below).

#### Against the build before meshes

The region-only cases must not cost more than 5% over what they cost before meshes. A stored figure cannot show that (the second record's own runs spread 8%), so it is measured in one sitting: the tree before meshes (main at `eeef871c`) exported and its bench built apart, the two benches run alternately, three each, and the medians compared.

**Before any change**, two sets, both flagged. The first at loads of 2.9 to 3.1 (3.06, 3.05 and 2.88 before the three runs of the build before meshes; 3.05, 3.05 and 3.13 before this tree's: five of the six above 3), the second at 4.0 to 4.5:

| Bench | Before meshes | With meshes | Difference | Before meshes | With meshes | Difference |
|-------|---------------|-------------|------------|---------------|-------------|------------|
| `skeletons_update` | 473.8 | 468.9 | -1.0% | 486.0 | 462.3 | -4.9% |
| `skeletons_extract` | 153.5 | 165.5 | +7.8% | 155.5 | 173.5 | +11.6% |
| `skeletons_extract_shadow` | 177.4 | 191.8 | +8.1% | 176.0 | 201.1 | +14.3% |
| `skeletons_extract_offscreen` | 148.7 | 160.4 | +7.9% | 148.3 | 168.1 | +13.4% |
| `skeletons_extract_images` | 266.4 | 271.4 | +1.9% | 269.0 | 285.5 | +6.1% |
| `skeletons_extract_plain` | 92.8 | 106.1 | +14.3% | 95.2 | 113.1 | +18.8% |
| `skeletons_extract_sockets` | 152.7 | 165.9 | +8.6% | 152.8 | 172.2 | +12.7% |

The update was unchanged (both readings are inside the noise); the extract was about 12 µs dearer for a crowd of regions drawn from one atlas, whatever else the case did, and 5 µs dearer in the first set with an image a part, where each quad is a run of its own. A timer put round the two halves of a skeleton's drawing, in both builds, found all of it in the second: the walk of the pose read 120.7 µs a frame before meshes and 121.4 with them, and turning its parts into runs 35.4 and 47.3. The drawer took the parts as an iterator of ready-made parts of either kind (a quad as drawn, or a mesh), and the compiler did not inline that iterator's step: every quad was built behind a call and copied out of it again through the two-kind type.

**What was changed**, in two steps, each measured:

1. `Emitter::draw_parts` takes the walk's own list and two functions, how a region and how a mesh of it is drawn, and builds each quad where it uses it. The type that held either kind is gone.
2. The mesh's skinning (`Parts::skin`) and its drawing (`Emitter::draw_skinned`) are never inlined, so the two loops over a skeleton's regions are not compiled round code that only a mesh runs. Over sixteen-run medians the shadow case read 3.9% over with the attributes where it read 6.4% without, and the crowd 4.3% where it read 6.2%. That is two points, and the same engine code moves by as much between two builds (below), so the gain is not shown to be the attributes'. They stay because they cannot hurt: one call a mesh, none for a skeleton of regions alone, and both functions keep their names on a stack, which is how the allocation test finds them.

**With both**, three sets of three runs each, alternately, from the binaries the tree builds. Loads 4.5 to 5.7 for the first (the set of the mesh record above), 3.6 to 7.0 for the second and 4.7 to 5.7 for the third: all flagged. The first set's medians, and each set's difference:

| Bench | Before meshes | With meshes | Difference | Second set | Third set |
|-------|---------------|-------------|------------|------------|-----------|
| `skeletons_update` | 479.2 | 489.3 | +2.1% | +2.4% | +4.4% |
| `skeletons_extract` | 159.2 | 163.2 | +2.5% | +1.3% | +2.3% |
| `skeletons_extract_shadow` | 181.7 | 189.6 | +4.3% | +6.5% | +6.7% |
| `skeletons_extract_offscreen` | 154.8 | 159.2 | +2.8% | +2.2% | +4.6% |
| `skeletons_extract_images` | 275.8 | 284.0 | +3.0% | +2.4% | +2.5% |
| `skeletons_extract_plain` | 97.3 | 101.9 | +4.7% | +3.9% | +3.0% |
| `skeletons_extract_sockets` | 157.0 | 160.5 | +2.2% | +4.4% | +5.7% |

So the extract of a crowd of regions is now 1 to 7% over what it was before meshes, 3.6% on average over the six cases and three sets, where it was 8% and 13% over on average in the two sets before the change. Fifteen of the eighteen readings are inside the 5%. The three that are not are the shadow case in the second and third sets (4.3% in the first) and the sockets case in the third (2.2% and 4.4% before it), and no set was read on a quiet machine, so by these figures the shadow case is not shown to be inside the bound. It runs the loop over the quads twice a skeleton, so what is left shows most in it; its sixteen-run medians, under a bench built for that reading, were 3.8% and 3.9% over. The update did not change in the code and reads 2 to 4% over in these sets after reading 1% and 5% under in the two before: that is the size of the noise.

**What is left, and what did not help.** About 2 to 12 µs of 100 to 200 remain, most in the shadow case, and they could not be placed:

- The regions' quads kept in a list of their own, each stretch of them with no mesh among it one part, makes both loops the same as before meshes. Built twice, with and without the second step above, it gained nothing on the crowd or the shadow case and made the plain case worse (9.5% and 11% over), so it was taken out.
- `draw_parts` as a function of its own changed nothing.
- The same engine code under a bench whose `main` differed read 2.4%, 2.9% and 6.2% over on the crowd, and timers in the two halves moved the walk by 5 µs between two builds whose walk is the same code. A difference of this size moves with how the binary is laid out, so no further change was made on a guess.
- No session that day was quiet: the sets were read at loads from 2.9 to 7. The crowd's extract in the build before meshes itself read 153.5 µs at noon and 159 to 164 in the afternoon.
- So the shadow case's 4 to 7% is the open figure: a comparison on a quiet machine would settle whether it is inside the 5%, and the two benches build from `main` and from this tree as they stand.

**A later sitting**, on the finished tree, by the same method: three sets of three runs each, alternately. Loads 3.2 to 3.5 for the first, 3.0 to 3.7 for the second and 3.5 to 4.1 for the third, all flagged: the machine was in use by other programs throughout, one of them on the GPU. The first set's medians, and each set's difference:

| Bench | Before meshes | With meshes | Difference | Second set | Third set |
|-------|---------------|-------------|------------|------------|-----------|
| `skeletons_update` | 503.9 | 516.3 | +2.5% | +6.3% | +6.1% |
| `skeletons_extract` | 164.9 | 172.3 | +4.5% | +6.7% | +5.0% |
| `skeletons_extract_shadow` | 191.1 | 198.7 | +4.0% | +6.6% | +8.1% |
| `skeletons_extract_offscreen` | 160.2 | 164.2 | +2.5% | +4.9% | +3.9% |
| `skeletons_extract_images` | 285.1 | 291.7 | +2.3% | +2.9% | +2.6% |
| `skeletons_extract_plain` | 101.4 | 104.4 | +3.0% | +6.9% | +2.9% |
| `skeletons_extract_sockets` | 167.2 | 171.2 | +2.4% | +3.8% | +4.4% |

Over the six extract cases the sets read 3.1%, 5.3% and 4.5% over on average, 4.3% together, where the three sets above read 3.6%. Thirteen of the eighteen readings are inside the 5%; the five that are not are the crowd, the shadow case and the plain case in the second set, and the crowd (by a hair, 5.03%) and the shadow case in the third. In time that is 3 to 16 µs a frame for 200 skeletons, most in the shadow case as before. The update read 12 to 32 µs over (2.5 to 6.3%), with this tree's own three runs spreading 12% in the second set.

So the finished tree agrees with the sets above: a crowd of regions costs about 4% more to draw than it did before meshes, at the 5% bound and not shown to be inside it, and the shadow case is the dearest. No sitting on a quiet machine has been had. The whole render step is not compared between the two: this tree's bench puts two probe passes round the world pass, which the bench before meshes does not have.

**Placing a sprite.** The comparison above covers the update and the entity drawer. `Batch2D::prepare` places every sprite a frame draws, and there the shader a sprite draws with was asked through a function the compiler kept out of line: a call for each sprite, where before meshes the answer for a sprite sharing the last one's key was a compare in the loop. It is a compare there again (`sprite_shader` is always inlined, and only a new key calls out, to `ensure_sprite_shader`). Measured on a whole frame of 10,000 sprites (`cargo bench --bench foundation`, "headless frame, 10k sprites", criterion's median), the three builds run alternately, three each, in two sets at loads of 5.0 to 7.0 and 4.3 to 6.2 (flagged). The median of each build's three runs, in microseconds:

| | Before meshes | With the call | With the compare |
|---|---|---|---|
| First set | 1,105.5 | 1,091.4 (-1.3%) | 1,097.8 (-0.7%) |
| Second set | 1,132.5 | 1,129.2 (-0.3%) | 1,116.4 (-1.4%) |

The three do not differ by more than the build before meshes spreads over its own three runs (1.6% in the first set, 6.8% in the second): ten thousand calls of a few instructions each do not show in a frame of 1.1 ms. The change is made for the shape of the loop every sprite passes through, not for a figure.

### Constraints

The scene is the crowd's, each skeleton with four constraints on bones among its 30: two limbs of two bones (parts of 4 and 4, hanging from the spine) reaching by IK, a bone pointed by a one-bone IK constraint, and then a transform constraint copying the turn of the first limb's second bone, in the world, onto another bone. Before every fixed step a system of the bench puts the three targets (leaf bones no constraint moves) at new places by world-space bone overrides, about where each limb's first bone is going (where it is, moved on as far as it moved in the step before: the animation swings it by up to 5 a step), 2.5 to 3.5 from it, within the limbs' reach of 8. After every step another checks each target is where it was put and each tip is on its target (the pointed bone's axis through it; a target the animation has carried out of reach is reached for, the tip at the full reach on the line to it), and a step in which one is off by 0.01 or more counts as a miss, so the constraints are known to act in every step measured. Both systems are outside the two probes.

- `skeletons_update_constraints`: the update (`FixedSet::Skeletons`) per fixed step, guarded at the update's 1,000 µs. It holds the four solves, the bones placed again below each, and the three world-space overrides a skeleton.

#### The constraints record

Three runs (`skeleton` bench binary, release), each after a run of the build before constraints (the comparison below). One-minute load before each run: 7.44, 6.09 and 5.04, every one above 3 and so flagged: the machine was in use throughout, and nothing waited for a quiet one. Medians in microseconds:

| Bench | Run 1 | Run 2 | Run 3 | Median of runs | Target |
|-------|-------|-------|-------|----------------|--------|
| `skeletons_update_constraints` | 586.1 | 546.9 | 549.2 | 549.2 | 1,000 |
| `skeletons_update` (the same crowd without) | 535.7 | 496.2 | 491.3 | 496.2 | 1,000 |

In every run all 390 measured steps had every tip on its target, at most 0.0004 from it. The guard is met by 45%. A fourth run, after a change to the check alone (outside the probes: a distance that is not a number now counts as a miss), read 549.0 µs at a load of 7.50. The four constraints and three overrides cost about 53 µs a step for the 200 skeletons, 0.27 µs a skeleton. An earlier run of the day, at a load of 9.8 and before the targets were placed ahead of the limbs, read 568.0 µs (its check missed: the animation had carried 227 of its 390 steps' targets out of reach, which is why the check now reaches for the line).

#### Against the build before constraints

The cases without constraints must not cost more than 5% over what they cost before constraints, measured in one sitting: main at `06b68319` exported with `git archive` and its bench built apart (its own target directory), the two benches run alternately, main first, three each, and the medians compared. Loads before the six runs, main then this tree: 8.08, 7.44, 6.93, 6.09, 5.30 and 5.04, all flagged (the load fell through the sitting, so each tree run had a lower load than the main run before it). Medians of three in microseconds:

| Bench | Before constraints | With constraints | Difference |
|-------|--------------------|------------------|------------|
| `skeletons_update` | 503.5 | 496.2 | -1.4% |
| `skeletons_extract` | 167.5 | 169.0 | +0.9% |
| `skeletons_extract_shadow` | 199.1 | 199.0 | -0.1% |
| `skeletons_extract_shadow_update` | 521.2 | 496.8 | -4.7% |
| `skeletons_extract_offscreen` | 161.9 | 164.5 | +1.6% |
| `skeletons_extract_offscreen_update` | 495.7 | 495.0 | -0.1% |
| `skeletons_extract_images` | 284.2 | 288.7 | +1.6% |
| `skeletons_extract_images_update` | 495.9 | 505.4 | +1.9% |
| `skeletons_extract_plain` | 102.7 | 105.6 | +2.8% |
| `skeletons_extract_plain_update` | 496.8 | 494.7 | -0.4% |
| `skeletons_update_sockets` | 505.9 | 507.6 | +0.3% |
| `skeletons_extract_sockets` | 163.8 | 169.3 | +3.4% |
| `one_skeleton_update` | 3.5 | 3.6 | +2.9% |
| `skeleton_pass_socket_spawn` | 2.6 | 2.6 | 0.0% |
| `skeleton_pass_socket_spawn_sockets` | 8.5 | 8.9 | +4.7% |
| `skeletons_update_mesh` | 605.3 | 607.5 | +0.4% |
| `skeletons_extract_mesh` | 287.9 | 294.0 | +2.1% |
| `skeletons_extract_mesh_shadow` | 369.8 | 369.8 | 0.0% |

Every case is inside the 5%; the nearest are the socket-spawn pass with a socket on each skeleton (+0.4 µs) and the shadow case's update, which reads under. `skeleton_pass_200` and `skeleton_pass_2000` read 0.08 and 0.50 µs in both. The render step and its parts (`skeletons_render*`, `skeletons_prepare*`, `skeletons_world_pass*`) moved by up to 16% both ways between runs of either build, with the first run of each the highest, and are context, not compared. Skeletons without constraints run the code they ran before (`Skeleton::place` branches on the data's constraints being empty), so no difference was expected, and nothing was changed for speed. The sitting was not on a quiet machine: a quiet one would settle the two readings near 5%, though both are fractions of a microsecond and one is not in a step at all.

**Allocations.** `fifty_skeletons_with_constraints_allocate_nothing_per_frame` (`tests/tilemap_alloc.rs`) is the allocation crowd with constraints of every kind on bones beside its chain: two limbs of two bones by IK (the first one's mix keyed by both of the base track's animations, so blended through every crossfade), a chain of four bones by IK, and a transform constraint in each of the four modes (world, world relative, own values, own values relative). Before every frame the game puts the three targets at new places by world-space bone overrides, and every 30 frames a skeleton changes its overrides of the first limb (its mix and its bend) and of the world-mode transform constraint (its rotate mix), so the twelve traced frames hold changes on several skeletons. Before the traced frames and after each, the test asserts every skeleton's second limb and chain have their tips on their targets, within 0.01, where the game put the targets in that step. The comparison with the sprites' frame is the other two tests': 390 allocations a frame (229 the engine's own) against the sprites' 392 (231), none from the skeletons' path.

It passed at once. Planted allocations, one at a time, each named by the test and removed: one in `constrain` fails with 50 a frame at `<alloc::vec::Vec<u8>>::with_capacity <- ryo2d::core::skeleton::constrain::constrain` in a debug build, and in a release build at `<ryo2d::core::skeleton::component::Skeleton>::place`, where the compiler inlines `constrain` (the stack names only the caller); one in `ik::chain` fails with 50 a frame at `<alloc::vec::Vec<u8>>::with_capacity <- ryo2d::core::skeleton::ik::chain` in both.

### Springs, culling and the IK changes

The scene is the crowd's. The new case:

- `skeletons_update_springs`: the crowd with two springs a skeleton, on the spine's last bone and the bone that hangs from it (each moving its place by half its lag and turning fully toward its lagging tip), every entity moving on an ellipse 24 across and 12 high round its place, about once every two seconds, so the springs follow the entity as well as the animation. Guarded at the update's 1,000 µs. After the measured steps the bench counts, for each spring, the skeletons whose bone stands off the place its parent gives it, which no animation moves: those whose spring is swaying it. Fewer than three in four for either is a miss; every run had all 200 for the first, and the run after the second was checked too had 200 for both.

The offscreen guard (`skeletons_extract_offscreen` held to a tenth of the crowd's extract in the same run) stays in, and held in every run.

#### Against the build before springs

The cases that exist in both benches, measured in one sitting: `main` at `bf1a5986` exported with `git archive` and its bench built apart (its own target directory), the two benches run alternately, main first. Two more builds ran in the same rotation to find where the step's cost goes (below): `bee38c63`, exported the same way, and this tree's bench with two temporary run-time switches, one that skips finding the box and one that skips the sounds system (removed after; no switch is in the code). Each run is about nine seconds, so nine rounds were run, not three, and every figure is the median of nine. The one-minute load before the first round's runs read 10.36, 8.29, 7.24, 6.28 and 5.77, the second's 5.55 to 3.60 and the third's 3.48 to 2.62, all above 3 and so flagged; rounds four to nine read 1.43 to 2.07, none flagged. Medians in microseconds:

| Bench | Main | This tree | Difference |
|-------|------|-----------|------------|
| `skeletons_update` | 483.5 | 493.1 | +2.0% |
| `skeletons_extract` | 161.9 | 162.4 | +0.3% |
| `skeletons_extract_shadow` | 190.1 | 188.6 | -0.8% |
| `skeletons_extract_shadow_update` | 483.0 | 495.7 | +2.6% |
| `skeletons_extract_offscreen` | 156.3 | 5.4 | -96.5% |
| `skeletons_extract_offscreen_update` | 480.2 | 492.2 | +2.5% |
| `skeletons_extract_images` | 269.2 | 276.3 | +2.6% |
| `skeletons_extract_images_update` | 483.2 | 494.8 | +2.4% |
| `skeletons_extract_plain` | 99.6 | 101.6 | +2.0% |
| `skeletons_extract_plain_update` | 481.8 | 493.3 | +2.4% |
| `skeletons_update_sockets` | 492.1 | 504.5 | +2.5% |
| `skeletons_extract_sockets` | 161.9 | 160.8 | -0.7% |
| `skeletons_update_constraints` | 533.5 | 547.6 | +2.6% |
| `one_skeleton_update` | 3.7 | 3.8 | +2.7% |
| `skeleton_pass_socket_spawn` | 2.7 | 2.6 | -3.7% |
| `skeleton_pass_socket_spawn_sockets` | 8.5 | 8.6 | +1.2% |
| `skeletons_update_mesh` | 589.4 | 609.1 | +3.3% |
| `skeletons_extract_mesh` | 283.4 | 280.3 | -1.1% |
| `skeletons_extract_mesh_shadow` | 353.6 | 355.3 | +0.5% |
| `skeletons_update_springs` (this tree only) | | 531.6 | |

Every case is inside the 5%. The nearest is `skeletons_update_mesh`, +3.3%. `skeleton_pass_200` and `skeleton_pass_2000` read 0.1 and 0.5 µs in both. The render step and its parts are context, not compared, and moved under 4% either way. Two springs a skeleton cost at most about 38 µs a step for the 200 skeletons over the crowd without them (531.6 against 493.1), about 0.1 µs a spring. That is an upper bound: the springs case also moves its 200 entities every step (the bench's `drift` system runs outside the probes, but the step then places each skeleton under a transform that changed), which the crowd without them does not.

**Where the update's cost goes.** The step reads about 10 µs (2%) over main for skeletons without the new features. The two switches put it:

| Build | `skeletons_update` | `skeletons_update_mesh` |
|-------|--------------------|-------------------------|
| `main` (`bf1a5986`) | 483.5 | 589.4 |
| `bee38c63` | 476.3 (-1.5%) | 582.1 (-1.2%) |
| This tree, the box and the sounds system skipped | 485.8 (+0.5%) | 594.0 (+0.8%) |
| This tree, the box skipped | 486.2 (+0.6%) | 593.9 (+0.8%) |
| This tree | 493.1 (+2.0%) | 609.1 (+3.3%) |

- **The box** is most of it: about 7 µs a step for the 200 skeletons (0.035 µs a skeleton), and about 15 µs with a mesh on each, where the box also reads the deform offsets. It is the one change every skeleton gets, and it is what takes the off-screen crowd's drawing from 156 µs to 5.4.
- **The sounds system**, with no `SkeletonSounds` in the world, is within the noise: switched off it moved the step by 0.4 µs, against a spread of several µs between runs of one build. Its own measure, about 0.1 µs a step, stands.
- **The rest**, about 2 to 3 µs over main with both skipped, is not pinned. It is under 1% of the step, inside what two builds of the same engine code read apart: `bee38c63`, which has the springs and the IK changes and neither the box nor the sounds system, reads 7 µs *under* main in every one of the nine rounds (474.5 to 479.2 against 473.0 to 486.0), though it does more than main, not less.
- So the 15 µs over `bee38c63` that the box's own measure left unexplained is the box's remaining cost after it was made cheaper (about 7 µs here), a few µs not pinned, and `bee38c63`'s own low reading, which this sitting puts about 7 µs below main. Nothing was changed for speed.

**Allocations.** `fifty_skeletons_with_springs_sounds_and_the_ik_changes_allocate_nothing_per_frame` (`tests/tilemap_alloc.rs`) is the allocation crowd with what a rig is now authored with, on bones beside its chain: a strand of two bones on the chain's last, each with a spring (a gust through the first, its wind keyed 0 to 400 and back by both of the base track's animations, and every 30 frames a skeleton changes its override of the second, its gravity and its mix); a chain of four parts kept to its positive side; an arm of two bones whose stretch is limited to 1.2 times its reach, its target moved every frame from inside its reach to past the limit; a leg whose first bone is sheared and whose second stands off its axis, its target held while the same animations stretch the first bone across to twice and back every 0.2 s, so its knee follows its last bend and swings where the branch is lost; and every other skeleton with `SkeletonSounds`, its step event naming a loaded clip. Before the traced frames and after each it asserts that each acts: both springs have turned their bones off their animated turn in four skeletons in five at the least (all 50 in every check); the chain's three joints turn the positive way, each as much as the others, and its tip is on its target; the arm's tip is on its target inside the limit and, past it, at the limit on the line to the target, both seen in every check (about 24 capped and 26 inside); over the checks a leg on its target and a leg swinging past it are both seen (12 on it and 4 past it in each); and the step sound's voices went on starting through the traced frames, by the audio log. After the traced frames three skeletons are taken out of the game's mixes and overrides: the first plays `idle`, the second `still` (`idle` without the gust) and the third `idle`, the first and third with the second spring's gravity overridden to 20 and to 400. Five seconds on, when the springs have forgotten how they came, the first spring's bone turns apart between the first two by up to 0.67 rad over a loop, and the second spring's between the first and third by up to 0.52; the test asks for 0.2 and 0.15. With the gust's keys skipped, or the override's values not written, by a run-time switch (one build, removed after), those read 0.00004 and 0.0001 rad, and the test fails on each.

Its frames make 408 to 410 allocations against the sprites' 392, and the engine's own are the same 229 (the sprites' 231). The 16 to 18 more are the audio engine's: a play of the step sound starts a voice in the audio backend and logs its start, or its refusal, with the sound's path (the headless engine's audio log is on from the start). They are counted apart, as a stack with a function of the audio module on it, in this test alone (every other test counts them as any other), printed beside the others, and bounded: at most 10 a play of the step sound, started or refused, as the frame's audio log counts them (three or four plays a frame, at most 6 allocations a play measured); none is made by the skeletons' path, the sounds system's own code included. An allocation planted once in the spring's arm of the constraint pass fails it at 100 a frame (two springs on each of 50 skeletons), at `<alloc::vec::Vec<u8>>::with_capacity <- ryo2d::core::skeleton::constrain::constrain`, release; the plant was removed.

### Reading the figures

- **Update:** 2.5 µs a skeleton (500 / 200), half of the 5 µs target; one skeleton alone is 3.0 µs, of which 0.2 is the probes. A socket with a child on each skeleton adds 8.6 µs to the 200 (1.7%).
- **Extract:** 0.82 µs a skeleton, or 33 ns a region (164 / 5,000), with every bone interpolated. A temporary run of the same scene without `Interpolated` read 105 µs, so drawing between steps (the bone blend, in `draw::blend`) costs about 55 µs of the 164, or 11 ns a region. That scene is now a case of the bench, `skeletons_extract_plain`, which takes `Interpolated` off every skeleton (a skeleton adds it, so leaving it out of the spawn no longer measures a plain entity); its figure is not among those recorded above. The drop shadow adds 26 µs (16%), the second quad of each region. The three costs the carried review notes named (a `HashMap` lookup of the attachment by name per region, `sin_cos` per region, and `blend` per slot) are inside the 33 ns and none broke a target, so none was changed.
- **Off-screen:** a crowd wholly outside the view cost 160 µs, 97% of an in-view one, when every quad was built before the run was culled by its box. Skeleton-level bounds have since been built (a skeleton whose box misses every camera is not walked: Springs, culling and the IK changes, above), and the same crowd now costs 5.4 µs.
- **An image for each part:** 283 µs against 164: 25 runs a skeleton instead of one (the culling, the texture lookup and the pipeline key are paid per run), 73% more, and 47% of the target. One atlas is the way to draw a character.
- **The frame pass:** a steady frame costs 0.07 µs at 200 skeletons and 0.50 µs at 2,000, so the pass's one cost that grows with the skeletons (the change-tick compare of each renderable) is nothing yet. A frame in which a socketed entity is spawned looks through the 200 skeletons and costs 2.6 µs, or 7.8 µs with a socket on each.
- **Frame, whole:** the render step's CPU time is 332 µs for the 200 skeletons, drawing included.

### Allocations

`fifty_animated_skeletons_allocate_nothing_per_frame` (`tests/tilemap_alloc.rs`, a debug-or-release test binary with a counting global allocator): 50 skeletons of 12 bones and 10 region slots, lit, on `Interpolated` entities, each playing two tracks, with events firing (two or three keys a loop), a `BoneSocket` with a child on each, and a mix started in every frame on some (`set_animation` and `add_animation` on the base track, `set_animation` and `set_empty` on the layer; each skeleton makes one such call every third frame and each kind every twelfth). 240 frames of warm-up, then 12 traced frames, each traced from the calls through the step, so a call's allocations count. It is compared with the same entities as plain sprites (a `Renderable::image` where the skeleton's is, and children with no socket) in a second engine, site by site, and asserts no allocation of the engine's own the sprites' frame does not make, none with a function of the skeletons' path on its stack, and none inside the GPU API from that path. The skeletons' path is the skeleton module, the entity drawer's skeleton functions (`collect_skeleton`, `Emitter::draw_parts`, `draw_shaded`, `draw_skinned` and `draw_mesh`) and the frame's mesh stream (`render/batch/mesh.rs`), which are outside the module: a buffer made in `collect_skeleton` for each skeleton, planted to try the test, fails it (200 allocations inside the GPU API a frame).

It passed at once (not red first): the skeleton code allocated nothing in steady state, as the module notes say. That it can fail was checked by planting an allocation in `SkeletonAnimation::set_empty`, in `update_skeletons` and in `Skeleton::parts`, one at a time: each was named by the test (4, 1 and 50 allocations a frame at those sites), and each plant was removed.

Both frames make about 229 allocations of the engine's own (390 in all; the rest are inside the GPU API), and the same in both. These are not the skeletons': the entity drawer builds its `QueryState` for the entities every frame (`world.query::<DrawableItem>()` in `EntityDrawer::collect`), which allocates a few dozen times for each archetype it matches (the test's sprite frame has them too). That is true of every scene with a `Renderable`, a sprite scene or a skeleton one, and is older than skeletons; it is listed under Limits.

**With a mesh.** `fifty_skeletons_with_a_deforming_mesh_allocate_nothing_per_frame` is the same crowd with a weighted mesh among each skeleton's regions: a ribbon of 60 vertices (30 rows of two, 58 triangles), each vertex held by two bones, so 120 positions, in a slot of its own after the fifth region, so a skeleton draws five regions, the mesh and five regions more. Both of the base track's animations key every position of it (three keys each, by different amounts), so a deform is applied in every step and two are blended through every crossfade. Before its traced frames the test asserts that the case is the one it says: more than 100 draws a frame (50 meshes among 51 runs of quads), and at least 40 of the 50 meshes standing off where their bones alone put them (with the deform keys taken out of the rig all 50 read 0.0, and the test fails there). Then the same comparison with the sprites' frame and the same three findings, with a fourth for both tests (the GPU API's allocations compared site by site, below), all told together in one message when any is not empty. After the last traced frame the deform is asserted again, so the frames that were traced drew a mesh a deform was moving (tried with every skeleton fading, across the traced frames, to a clip that keys no deform: the first assertion and all twelve frames pass, and this one fails with 32 of 50 moved).

It passed at once too. Planted allocations, a build each, each named by the test and then removed:

- in `Parts::skin` (the skinning): 50 a frame, at `<alloc::vec::Vec<u8>>::with_capacity <- <ryo2d::core::skeleton::draw::Parts>::skin`;
- in `Emitter::draw_mesh`: 50 a frame, the nearest function of the skeletons' path being `<ryo2d::core::render::entities::Emitter>::draw_mesh::<..>`;
- in `Emitter::draw_skinned` and in `MeshStream::add_whole` together: 50 each, named `<..::Emitter>::draw_skinned` and `<ryo2d::core::render::batch::mesh::MeshStream>::add_whole::<..>`;
- in the two functions `draw_parts` is handed, how a mesh is drawn and how a region is (they are closures of `collect_skeleton`): 50 a frame at `ryo2d::core::render::entities::collect_skeleton::{closure#7}` for the mesh's, and 500 at `{closure#6}` for the region's, in both tests; the `collect_skeleton` name finds them.

So each name the test lists for the mesh path (`draw_skinned`, `draw_mesh`, `batch::mesh::`) matches a function a mesh's frame really runs.

The meshes' frame makes 605 allocations where the sprites' makes 392. The engine's own are the same 229 (the sprites' 231); the 215 more it makes are inside the GPU API, and none of them from the skeletons' path. Found by their stacks: 204 against the sprites' 4 are copies of a pipeline's vertex buffer layouts (`naga::back::msl::VertexBufferMapping::clone`) made in wgpu's Metal backend at `CommandEncoder::finish`, where wgpu checks and encodes what the passes recorded. By their count they are two for each pipeline the frame's passes set, a pipeline having two vertex buffers: 102 sets a frame against the sprites' 2. A mesh between two runs of quads is two pipeline changes, so four allocations a mesh a frame; a few more are the mesh stream's two buffer writes. They belong to the pipeline changes, not to anything the skeleton code does, and the regions-only frame, one draw, makes none. One pipeline for quads and meshes would remove them (Later).

**The GPU API's allocations, site by site.** An allocation inside the GPU API counts as the GPU's unless a function of the skeletons' path is on its stack, and that rule alone would pass a buffer made, a buffer written or a bind group made for each mesh by code outside that path: `Batch2D::prepare` and the planning of the draws, where a mesh's instance is placed, or the recording of the pass. So both skeleton tests also compare the GPU API's allocations with the sprites' frame site by site, a site being the GPU API's function and the engine's that called it, and fail on any site where the skeletons' frame makes over 24 more (`GPU_ROOM`, `more_gpu`). The room and every count here were measured on Metal only; another backend's sites may differ by more, and the failure names the site and both counts. What the sites differ by is a few: the mesh stream's two buffer writes (5 allocations, 9 in a debug build, where the sprites' frame has none), a longer list of the pass's commands (1 or 2 at four sites) and what moves with the GPU's timing (1 either way). A resource for each of the fifty meshes is fifty at the least: a buffer made for each mesh in `prepare`, planted to try it, fails with `200 (the other frame 0) at <wgpu::api::device::Device>::create_buffer <- <ryo2d::core::render::batch::gpu::Batch2D>::prepare::{closure#2}` while the three other findings stay empty, so it is a case the old rule let through.

One site is left out, the encoder's `finish`: no code of the engine's runs there, and what it allocates is wgpu's own handling of the commands the passes recorded. Leaving it out lets growth in wgpu's own encoding through: if a later wgpu allocates more there for each pipeline change or draw, these two tests will not say so, and the bench's `skeleton_mesh_cost` is where it would show. That is where the four a mesh are (279 against the sprites' 75; in a debug build 637 against 133, about ten a mesh, the Metal bindings checking each message they send). A bound on the frame's total of so many a mesh was not taken for that reason: the number is one backend's in one build profile (the frame's total is 4.3 a mesh over the sprites' on Metal in release and 10.4 in debug, and was not measured on the others), where the comparison by site needs no such number. A skeleton of regions makes exactly the sprites' count inside the GPU API, 161 in release and 229 in debug, 75 and 133 of them at the finish.

### Changes the bench made to the engine

Three hooks, `#[doc(hidden)]` in `src/core/testing/headless.rs`: `HeadlessEngine::entity_draw_time` (the last frame's `EntityDrawer::collect` time, written around the call in `src/core/engine/extract.rs`, two clock reads a frame), `HeadlessEngine::batch_prepare_time` (the last frame's `Batch2D::prepare` time, written the same way, two more clock reads a frame) and `HeadlessEngine::skeleton_pass_time(calls)` (runs `pose_new_skeletons` on a system initialised once, outside a frame). The world pass needs none: the bench puts two probe passes of its own round it in the render pipeline, as a game may.

## Limits

Each was copied from the module notes (`src/core/skeleton/AGENTS.md`) and restated here for a reader of the backlog; the notes' test names stay with the rules there.

### Data

- **At most 255 bones and 255 slots** a skeleton (`BoneId(u8)`, `SlotId(u8)`): `bone_id` and `slot_id` give `None` past the 255th, and a file or builder with more fails to build.
- **A non-finite value in a file fails to load**; in a bone override, a non-finite value is ignored with one warning.
- **An animation has at most one timeline per property of a bone or slot** (two rotate timelines on one bone fail, and so do a combined translate and a translate x), at most one draw-order and one event timeline; a bezier's x controls lie in 0..=1; a draw-order key lists every slot once; a region is not empty and a bounding box has at least three vertices; no animation is named `<empty>`.
- **Spine's model is followed in part**: no path constraints and no physics constraints (the engine's own springs stand in for them, with a file format of its own), no clipping attachments, no linked meshes (a mesh that borrows another's vertices and deform keys), no Spine binary format (the spec's non-goals). The differences in mixing from Spine's documented behaviour (the crossfade does not dip toward the tracks below, `hold_previous` holds time, the queue's delay, `complete` only on the current entry, `clearTrack` returning to setup, the stepped-key thresholds) are listed in the notes ("Known differences"). Not offered: a track end time, queued empty animations (`addEmptyAnimation`), the attachment and draw-order thresholds, `shortestRotation` and `resetRotationDirections`.
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
- **Event sounds play from the fixed step**, so up to a step late, as every skeleton event is, and only for a skeleton with `SkeletonSounds` (a game can still play an event's audio itself, its balance as `PlayOptions::pan`). The sounds load asynchronously when the data lands, so the first events may be silent. A voice is not tied to its skeleton: despawning it does not stop a sound already playing. A sound played at a place stays where it started, not following the skeleton or the bone. In a crowd sharing one sound, the sound's `per_frame` limit keeps the first skeleton in entity order whose event fired that frame, not the nearest to the listener. A looping sound named by an event starts another loop at each firing, and only its scene's exit stops them.
- **A sprite on a socket draws wholly in front of the skeleton or wholly behind it**, being an entity of its own: an item the hand must wrap is a region attachment in a slot.
- **Colliders on a socket follow the bone a physics step late**, since physics runs before propagation. An ordinary child of the skeleton's entity (not a socket, nor below one) is not reached by the entity's `skip_interpolation`.
- **Not taken: the bone's exact matrix in `GlobalTransform`**: propagation re-places a child from its `Transform` whenever its parent moves.
- **A bone the skeleton lacks warns once** per name for a socket and leaves the entity where it is.
- **Skeletons step in entity order**, so one riding another's socket may be posed before its socket moves; it is placed again after every skeleton has stepped.

### Drawing

- **A skeleton's entity is interpolated** (`Skeleton` adds `Interpolated`), its own place with its bones; a game removes the component for a skeleton that animates at the step rate, and a bundle cannot leave it out. The blend is of each bone's transform in skeleton space, not of the local poses, so a child of a bone turning fast draws a chord's sagitta off its parent's tip mid-step (a 60 px bone turning 10 degrees a step: about 0.2 px).
- **A skeleton no camera can see is not walked**, but its box is conservative (each bone's furthest attachment of any skin, under its largest stretch, joined over this pose and the last), so a skeleton just out of view may still be walked, its runs then culled one by one. It is still stepped. Finding the box costs about 0.035 µs a skeleton a step (0.075 with a mesh). The padding assumes an image draws at its own size in its slot: an image reloaded smaller than a region's rectangle is a data error, and an outline on that region can then reach past the box.
- **A run is a stretch of consecutive quads of one image and one blend**, so a skeleton of an image a part draws one run a part (283 against 164 µs above): use an atlas. With a game shader on the entity each quad is a sprite item of its own instead.
- **Where parts overlap, a drop shadow is darker than a sprite's uniform one**, each region's shadow laid over the last (a half black shadow leaves 187 of a white floor once and 136 where two parts overlap). A uniform silhouette needs the skeleton drawn offscreen first, which is not built; a character should use a blob or contact shadow. The blob and contact shadows stand where the entity does, not following the pose's bounds. `Renderable`'s `image`, `anchor` and `target` are not used by a skeleton, and a `YSort` sorts it by its entity's y plus `sort_offset` (`foot_offset` plays no part).
- **A skeleton with no pose for its data draws nothing, shadows included**: reached only by the case the frame's pass does not cover (enabled again between steps after its data changed).
- **A game shader's `in.uv` runs over the part's own rectangle of its image**, not the figure.
- **A mesh has at most 65,535 vertices** (its triangle indices are `u16`), and a deform key is whole or empty: an x and a y for every position of the mesh, or nothing (the file cannot leave a key's leading or trailing zeros out). A deform is keyed by its slot and attachment name, for whichever mesh of that count the skins worn show there.
- **An effect that grows its sprite's quad leaves a mesh plain**: an outline, a glow or a directional outline draws round the part's rectangle, which a quad is enlarged to hold and a mesh has no rectangle for. The regions take it and the meshes draw without it, with one warning a run naming the first such entity. The built-in effects that stay inside the sprite, and a game's own sprite shader, reach a mesh as they reach a region.
- **Pixel-art snapping does not apply to a mesh's vertices**: a region's rectangle is snapped to whole pixels on a pixel-art target and a mesh is drawn where the bones put it, so the two agree to the pixel only where both stand on whole pixels. No test pins this on its own.
- **A mesh may draw plain for the frames a game shader's check for meshes takes**: the shader is checked once for quads and once for meshes, and the mesh's check comes in the frame it first draws. No test pins this on its own.
- **A mesh is a draw of its own** (two under a drop shadow, the shadow's and the body's), **with a pipeline change before it and after it**: it merges with nothing, so a skeleton with one mesh among its regions is three draws where it was one. Measured, a weighted mesh of 60 vertices costs about 1.6 µs of a frame's render step and 0.55 µs of a step's update while a deform plays on it, and four allocations a frame inside the GPU API (Meshes, above). A character is made of a few meshes.

### Constraints

- **Two bones under a first bone scaled unevenly or sheared are solved exactly, but where the knee loses its branch the tip is briefly off its target.** Where such a limb's reach dips and rises, as a target passes the dip's level or the top of the rise after it, or an animated scale moves those levels past a held target, the knee swings to its new bend at 6 rad of bend a second, the tip pointing at the target and short of it or past it for those steps. A target walked in and out through the whole reach of 1,000 random limbs: with scales up to 1.5 to one and shears up to 0.3, 2.0% of limbs swing for three steps or more, at most 18 steps with the tip off by up to 0.09 of the reach over 10 s and 24 steps and 0.27 over 1 s; with scales up to 2 to one and shears up to 0.6, 4.2%, 19 steps and 0.19 over 10 s, 34 and 0.43 over 1 s. A one-way pass over 20 to 200 steps swings up to 34 steps, the tip up to 0.55 off (0.65 at up to 2 to one). A squash with the target held swings about 1% of runs, at most 21 steps; a fast squash pops no more than 0.1 of the reach unless faster than 10 steps (over 5 steps once, 0.127; over 3 steps six times, the worst 0.271); a squash with the target moving at once (a landing with a planted foot) can pop up to 0.35 of the reach. A target that leaves the reach in one step from a lower crest's branch holds that crest, short of the farthest reach (3.6% short on a nearly flat reach). The walk is bounded at 96 steps each way (none needed more than 58 over 60,000 random limbs up to 200 to one) and costs about 1 µs a solve against 0.07 for the closed form, 0.74 when the knee follows a slowly moving target. Stateless callers of `two_bones` jump once instead (up to 0.55 of the reach).
- **Stretch turns off on an uneven or sheared first bone**, even one off even by a rounding (scales more than a millionth apart, shears more than 1e-6 rad apart): a stretched limb whose first bone's own scale is animated off even stops stretching. Stretch an even bone, and scale a parent or the entity for an uneven look.
- **A stretch limit cannot be overridden back to no limit**: an override's `max_stretch` of `None` is the setup's, so a game lifts a limit with `f32::MAX`.
- **Stretch on two bones falls a little short** with the second bone off the first's axis and `uniform` off: 3.23 short of a target 120 away, for a second bone at (40, 12) and 30 long.
- **A chain curls and does not fold**: a target nearer than its nearest shape is not reached. Of targets drawn at random from 5% to 30% of the length (among those its parts could reach), 61% to 91% are met, by pose and by count of bones; 48% to 70% with the distance itself drawn evenly over that span; from half the length out, all but one in a thousand.
- **A chain bent the pose's way (`bend: Pose`, its default) changes sides when the way its pose closes changes** while its target is nearer than its tip: swinging through straight flips it (a joint moves 0.7 of the length for a target at half the length, 0.4 at 90%, 0.1 at 99.5%), and so does an S-shaped sway, twice a cycle (0.66 to 0.72 of the length at half the length, 0.23 to 0.35 at 90%). Such a chain is fit for a rope, chain or cable, or a tail or tentacle held in a one-sided curve, reaching from about its nearest shape out; not for a side-to-side or S-wave sway while its target is nearer than its tip. A chain kept to a side (`bend: Positive` or `Negative`) never changes sides, but ignores its animated curve while it reaches: it is solved from straight as an even arc, and its pose comes back only by the mix.
- **A chain wound past half a turn may jump as its target moves out**: uneven parts just past half a turn by a quarter to a third of the length, sixteen bones wound past a full turn by half of it.
- **A chain settles steeply into its nearest shape**, as the square root of how far its target is outside it (a joint moves up to 1.2% of the length walked in steps of a millionth of it), and **a target passing over its first joint turns it half round**.
- **A chain's closing walk can run out of its 96 steps before its first turn**: rare (11 of 150,000 random poses of sixteen bones, turns up to 1 rad), the worst losing 0.026 of the length of its reach inward.
- **A chain assumes even, unsheared bones** but the last: otherwise its tip may miss (no figure measured).
- **A mix strictly between 0 and 1 can jump**: an IK constraint's bones by a full turn times the mix when the solved turn passes half a turn from the animated one, and a world-mode transform constraint's bone when its target passes half a turn. Not at a mix of 1, nor in a local mode.
- **A world-space override on an IK bone changes hands as the mix leaves 0**, where a later constraint moves a bone above it: the game's at 0, the constraint's above (an eye held at 0.3 under a hip turned a quarter turn points at 0.3 at a mix of 0 and about 1.6 further round at 0.01).
- **An override cleared and set again between steps**, on a skeleton placed again before its next step (a socket's rider, a flip), is applied over the old override's result until that step.
- **A local-mode transform constraint copies its target's own values**, so a target an earlier world-mode constraint holds is copied as it was animated, not where it is held.
- **A game cannot read where animation puts a target in the step under way**, only where the last step left it; a target moved from frame-rate input follows at the step's rate.
- **A spring moves one bone and collides with nothing**: a strand of hair is a chain of bones each with its own spring, parent first. Its state is not saved or networked (it settles from rest after a load), and it is tuned at the engine's fixed rate (inertia acts once a step), so a game that changes its fixed rate may need to retune. A stiff spring needs its damping raised with its strength (critical is about `2 × √strength`), and past the 64 sub-steps a step takes (a strength of about 3.7 million at 60 Hz) the strength saturates. A skeleton riding another's socket sees its carrier's motion a step late.
- **Not built**: path constraints, and constraints bound to a skin (every constraint is always on).

### Found while measuring, not skeleton's

- **The entity drawer builds a `QueryState` every frame** (`world.query::<DrawableItem>()` and the composites' in `EntityDrawer::collect`), which allocates a few dozen times a frame for any scene with a `Renderable`, more with each archetype that holds one (about 229 allocations of the engine's own in the allocation test's frames, skeletons or sprites alike). It is older than skeletons and the same for sprites; keeping the states in the drawer would remove it.
- **The first frame a game sprite shader draws on a machine takes about 87 ms** (`skeleton_first_shader_frame`, Meshes above): its pipeline is made in the frame it is first drawn with, on the frame's own thread, and the GPU compiles its fragment stage there. Once the machine's shader cache holds it the frame takes 2.3 ms, 4.0 with a mesh's second pipeline. It is older than meshes and true of any sprite with a game shader: "checked without blocking" (render module notes) is the check for errors, not the compiling. A new version of a shader already drawn is made on the shader thread while the old one draws; making a first use there too, drawing plain meanwhile as a shader being checked already does, would take the hitch out.
- **wgpu's Metal backend allocates twice each time a pass sets a pipeline** (it copies the pipeline's vertex buffer layouts), so a frame's allocations inside the GPU API grow with its pipeline changes: four a mesh between two runs of quads (Allocations, above). The engine's own code allocates nothing there, and a frame whose draws share a pipeline pays it once.

## Later

- **One pipeline for quads and meshes.** A mesh between two runs of quads costs about 0.65 µs of its 1.6 in the pass (0.12 recorded, about 0.5 at the encoder's finish: Meshes, What a mesh costs), and its two pipeline changes, with the four allocations wgpu makes for them, are the part of that a shared pipeline would take out; how much of the 0.65 they are was not measured apart. A vertex stage that places either a quad's corner or a mesh's vertex, told which by the draw, would let a mesh draw with the pipeline of the quads round it, and only its buffers would change. It changes the sprite's vertex stage, which every sprite runs, so it wants its own measurement of what the quads pay for it. Keeping the bind groups across the two pipelines was measured and is not it (0.012 µs a mesh).
- **Holding a skeleton while its scene's `update` waits on a transition.** The other answer to the limit above (In the world): a skeleton whose scene is in a transition would be held, as it is under a scene that stops its own, so that nothing is sent while `update` cannot hear it. It was not taken because it changes how every fade looks (characters stand frozen under it) and parts skeletons from particles and physics, which run on through a transition; holding all three, or letting `update` run during a transition, is an engine-wide decision. The owner's call.
- **A map from (target, property) to timeline for each animation, and an index of its event timeline.** Two searches are linear in an animation's timelines. When a mix starts, each timeline of the outgoing entry looks for the incoming animation's timeline that keys the same properties (`apply_track`'s cover), which is quadratic in timelines, once an entry a mix. And `events_crossed` scans the animation's timelines for its event timeline, every entry every step. Both are negligible for the bench's rig (about 40 timelines an animation). Inferred, not measured: 200 skeletons whose animations have 150 timelines each, all switching animation in one step, would spend about 7 ms on the cover search in that step. A per-animation map built with the data (and the event timeline's index kept beside it) makes both a lookup.
- **What the data cannot yet express, for an importer of another tool's files:** a slot's colour keyed as separate rgb and alpha timelines (one `Color` timeline keys all four here); a curve for each channel of a colour key (one curve a key here); and atlas regions packed rotated (a region here is an upright rectangle of its image). Check each against the tool's public format documentation when that stage is built, not against a runtime's source.
- **A uniform drop shadow, from an offscreen pass.** A skeleton's drop shadow is each region's own silhouette laid over the last, so a translucent one is darker where parts overlap (Limits, Drawing). An even silhouette, as a sprite casts, needs the skeleton drawn offscreen first: its regions into an image (or a mask) of their own, sized by the pose's bounds, which is then drawn once as the shadow. That is new renderer work (a target or an atlas cell for each shadowed skeleton a camera sees, redrawn as the pose changes, and a pass before the world's), with a cost for each shadowed skeleton, so it was left out and characters use blob or contact shadows, which match a sprite's. The same image would give a figure-wide outline or glow, where a sprite shader applies part by part.
