# 2D skeletal animation: design

## Overview

Ryo2D's skeleton module (`src/core/skeleton/`, about 1,300 lines) can only draw debug lines. It has:
- forward-kinematics bones;
- keyframed bone tracks, with their scale keys ignored;
- crossfade blending;
- a state-machine controller.

Nothing visible can be attached to a bone, there is no IK, and a skeleton can only be authored by writing bone coordinates in code. The only user is its own demo, which the owner found sloppy and buggy. The audit (`docs/research/2026-10-11-engine-audit.md`) said to finish it or cut it.

The owner chose to rebuild it from the ground up to a professional standard. This document designs the whole rebuild, which merges in four stages:
1. **Core:** bones, slots, sprite attachments, skins, animations, mixing, events and sockets, the asset format, and a new demo.
2. **Meshes:** mesh attachments with weights, and deform keys.
3. **Constraints:** IK and transform constraints.
4. **Import:** reading an animation tool's export.

Each stage gets its own plan, reviews and hand-play. The old module and its demo are deleted in stage 1.

## Goals

- A game can show a cutout character whose sprites ride its bones, with a lit, sorted and floored draw like any sprite.
- That character plays animations authored as keyframes on bones and slots, and mixes between them:
  - crossfades;
  - layers, so it can run while waving;
  - additive layers.
- Events keyed in an animation reach the game exactly once each, for footsteps, hit frames and sounds.
- An entity can follow a bone: a sword in the hand, a hitbox on the fist, a particle emitter at a torch.
- The game can override a bone after animation, for example to aim an arm at the mouse.
- Skins swap attachments wholesale, and can be combined (a body skin plus a hat skin).
- A skeleton is an asset file that hot-reloads, and can also be built in code.
- Later stages add:
  - meshes bent by weighted bones, with per-vertex deform keys;
  - IK (one-bone, two-bone and chain) and transform constraints;
  - import from a real animation tool.
- Steady state does no per-frame allocation, and 200 animated skeletons fit in a frame's budget (Targets).

## Non-goals

- **No skeleton editor in the engine.** Authoring is in a tool, then imported (stage 4), or in the file format by hand or in code.
- **No path constraints and no physics constraints** (Spine 4.2's springs). Both go to the backlog.
- **No clipping attachments** (stencil masks). Backlog.
- **No GPU skinning.** Meshes are deformed on the CPU into a vertex stream, as Spine's own runtimes do. GPU skinning is a backlog lever if the targets demand it.
- **No 3D bones**, and no Spine binary format. Stage 4 reads JSON only.
- **Networking:** replicating animation state is out of scope.

## Anti-goals (the professional bar)

- **Drawing:** a skeleton must not draw differently from a sprite in the same scene. Lighting, shadows (sprite shadows and occluders), `Floor`/`DrawLift`, z order, interpolation and the camera all apply.
- **Timing:** bones and sockets must not lag a step behind the entity they belong to (the old system did).
- **Events:**
  - a loop, a large step, a time scale or a pause must not drop or repeat an event;
  - a crossfade must not fire the outgoing animation's events after its mix-out ends, unless the mix asks for that (Spine's `event threshold`).
- **Hot reload:** a reload must not reset a playing character to its setup pose. Tracks and the time within them survive by name.
- **The data model:** it must not be a toy. It follows Spine's well-proven model closely enough that stage 4 is a translation, not a redesign.

## Decisions

### The owner's decisions

- Rebuild from scratch to a professional standard. The old module and demo are thrown away. Breaking changes are fine.
- The design is left to me ("no preconceived ideas … other than a professional one"). Merge in stages, each with its own plan, review and hand-play (approved playback, 2026-11-12).

### My calls (the owner may revisit them in review)

- **The data model follows Spine's:**
  - bones, then slots (draw order and colour), then attachments;
  - skins;
  - animations made of timelines;
  - an animation state of tracks with mixing.
  - Spine is the de facto standard; Godot's and Unity's 2D tools follow similar shapes. Matching it makes stage 4 a translation and gives artists a familiar model.
- **Bones carry rotation, translation, scale and shear, with five inherit modes:** normal, only translation, no rotation or reflection, no scale, and no scale or reflection. These are Spine's. Y is down and positive rotation is clockwise, as in the rest of the engine. An importer flips Spine's y-up angles.
- **The schedule:** skeletons update in a new fixed set, `FixedSet::Skeletons`, after `FixedSet::Transforms` and before `FixedSet::Particles`. They read this step's `GlobalTransform` of their entity, so there is no lag. Sockets are placed in the same set (see Sockets).
- **Interpolation:** bones are interpolated at draw time. Each skeleton keeps the last two steps' bone world matrices, and the drawer blends them by the frame's interpolation factor, as `Interpolated` entities are. A skeleton whose entity skips interpolation (a teleport) skips it too.
- **The draw:** a skeleton is a drawable entity, so it sorts and shades exactly like a sprite entity.
  - **Where its settings come from:** it takes its tint, `lit`, layers, z, `YSort` key and storey from the same components a sprite does (`Renderable`, with its fields; the plan settles whether that is `Renderable` itself or a sibling component carrying the same fields).
  - **Order within the skeleton:** its slots draw in the skeleton's draw order, as one group of consecutive draw items. The group splits only where the texture or blend changes.
  - **Sorting:** the group sorts as one unit among entities by the entity sort key (z, y-sort, storey, then submission). It does not use the particle run's fixed place after every entity at equal z.
  - **Shadows:** it casts sprite shadows and ambient occlusion as entities do.
  - **Stage 1 builds this first:** the draw list gets a group item that carries an entity's sort key and emits shadows, with its own e2e cases (y-sorted among `YSort` sprites and tile layers; shadowed).
- **Time:** animation time follows the game clock: time scale, pause and hitstop. Each skeleton has its own speed (`time_scale`), and each track has its own speed too.
- **Events reach the game as a message** (`SkeletonEvent`), plus per-track queries for completion. There are no callbacks, matching the engine's message style.
- **The file format is `.skeleton.ron`,** an asset with hot reload. Images are dependencies (`Handle<Image>`). Regions are named rectangles of an image: an atlas, or one image per part. A reload keeps the `AssetId`, and live skeletons rebuild their pose by bone and slot name.
- **Art for the demo:** rgsdev's "Free CC0 Modular Animated Vector Characters 2D" (CC0, https://rgsdev.itch.io/free-cc0-modular-animated-vector-characters-2d). If its parts don't make a convincing rig, Kenney's "Modular Characters" (CC0, https://kenney.nl/assets/modular-characters). Stage 1's plan checks the files and the licence first. Spine's example art (spineboy, raptor) has no redistribution licence and is not used.
- **Clean room from the start** (the owner asked for my advice, 2026-11-12): no stage reads Esoteric's runtime source.
  - Mixing and event rules are written from Spine's public user guide and format documentation.
  - Where those are silent, we choose a rule, write it in the module notes and pin it with a test.
  - The rules are Spine-like, not bound to match Spine in every edge case. A difference found later is recorded, not a bug, unless it hurts a game.
- **The stage 4 import target is open; the owner decides at stage 4:**
  - Spine's licences neither allow nor forbid a third-party runtime. A clean-room reader of the public JSON format, with no Esoteric code, is the professional choice, but only after Esoteric confirms in writing. Contacting them is the owner's call.
  - The fallback is DragonBones 5.5 JSON. Its runtimes and tools are MIT-licensed, but the editor appears to be discontinued.
  - Research notes: `docs/research/2026-11-12-skeleton-formats.md` (committed with this spec).

## Design

### Data (the asset: `SkeletonData`)

- **Bones:** a tree in parent-first order. Each bone has:
  - a name, a parent, and a length (for debug drawing and IK);
  - its setup pose: x, y, rotation, scale x and y, shear x and y;
  - an inherit mode.
- **Slots:** in setup draw order. Each slot has:
  - a name and a bone;
  - a setup colour (sRGB with alpha);
  - a setup attachment name, or none;
  - a blend mode: normal, additive, multiply or screen. Use what the sprite path supports, and record any it can't do.
- **Skins:** a map from (slot, attachment name) to an attachment. There is a default skin. A runtime skin can be built by combining skins.
- **Attachments, stage 1:**
  - **Region:** a named image region, with offset x and y, rotation, scale, width and height, and colour.
  - **Point:** a named position and rotation on a bone, for spawning things.
  - **Bounding box:** a polygon on a bone, which the game can query for hit tests.
- **Attachments, stage 2:** **Mesh**. Vertices are either unweighted or weighted to several bones each, with UVs, triangles and hull edges.
- **Animations:** a name, a duration, and timelines. Each timeline has frames with curves: linear, stepped, or cubic bezier per frame, as Spine has.
  - Bone timelines key rotate, translate, scale and shear, each per axis or combined.
  - Slot timelines key attachment, colour (RGBA) and dark colour (backlog).
  - There are draw-order and event timelines.
  - Stage 2 adds deform timelines.
  - Stage 3 adds IK and transform-constraint mix timelines.
- **Events:** named, with default int, float and string values, and an optional audio path with volume and balance. An event key can override the values.
  - The engine does not play an event's audio itself. The path, volume and balance ride on `SkeletonEvent`, and the game plays it, as Spine's own runtimes leave it to the game.
  - The path is a string, not a loaded dependency.
  - The demo shows the pattern: on a footstep event, play the sound at the skeleton's position.
- **Validation at load:** names are unique per kind; parents come before children; references resolve; frames are in time order; curve values are finite.
  - A bad file fails to load with a message that names the file, the item and the problem.
  - A failed reload keeps the last good data and sends `ReloadFailed`, as other assets do.
- **Built in code:** the same data can be built through a builder, `SkeletonData::builder()`, and added with `ctx.assets().add`.

### Runtime

- **`Skeleton`** (component, on an entity with a `Transform`). It holds:
  - a `Handle<SkeletonData>`;
  - the current pose: local and world bone transforms, slot colours and attachments, and the draw order;
  - the active skin;
  - the bone overrides;
  - the last two steps' world matrices, for interpolation.
  - Methods: `bone_world(name)`, `set_skin`, `set_attachment(slot, name)`, `bounding_box(name)` in world space, and `point(name)` in world space.
- **`SkeletonAnimation`** (component, optional; without it a skeleton holds its setup pose). It holds tracks, numbered from 0, each with a current entry and a queue:
  - `set_animation(track, name, looping)` replaces the track at once, with the mix the data or the call gives;
  - `add_animation(track, name, looping, delay)` queues an animation;
  - `set_empty(track, mix)` fades a layer out;
  - `clear(track)`.
  - Each entry has a `TrackEntry` handle with speed, alpha, mix duration, mix blend (replace, or add for additive), event threshold, reverse, and a hold-previous flag.
  - Default mixes per pair of animations live in the data, with a default mix. A call can override them.
  - Track order is the layering: higher tracks apply over lower ones by their alpha.
- **Applying each step**, in `FixedSet::Skeletons`:
  1. advance the tracks by the step's game time × the skeleton's speed × the track's speed;
  2. apply the timelines into the pose (setup, then track 0, mixing, then the higher tracks);
  3. apply the game's bone overrides (`Skeleton::override_bone(name, BoneOverride { rotation, translation, … , mix })`, kept until cleared);
  4. apply constraints (stage 3);
  5. compute world transforms from the entity's `GlobalTransform`;
  6. place sockets;
  7. send events.
- **Events:**
  - `SkeletonEvent { entity, track, animation, name, int, float, string, time }` messages are sent for every event key crossed in the step, in time order, including across loop boundaries. They follow Spine's rules for mixing and the event threshold.
  - `SkeletonTrackEvent { entity, track, animation, kind: Start | Interrupt | End | Complete | Dispose }` messages follow Spine's track listener events.
  - A skeleton that is despawned or whose scene leaves sends nothing more.
- **Bone overrides** let the game aim, look or lean after animation. Mix 0–1 blends from the animated value.
- **Hot reload:** a skeleton whose data reloads keeps its tracks and their times by animation name. A track whose animation is gone is cleared, with one warning. Bone overrides and the skin are kept by name.

### Sockets

- **`BoneSocket { bone, offset: Transform, inherit_scale }`** on a child entity of a skeleton entity. In `FixedSet::Skeletons`, after world transforms are computed, the system writes the child's local `Transform` (relative to the skeleton entity) and its `GlobalTransform`, so it is where the bone is in the same step, with no lag. Its `GlobalTransform` equals the bone's world matrix × the offset for an unskewed bone. Where the bone's transform in the skeleton's space is skewed (a sheared bone, or a turned bone under an unevenly scaled one), the skew is dropped: the socket keeps the bone's place and direction, its length along the bone and its height across it. The reason is that the socket's local `Transform` is the truth and holds no skew: propagation re-places a child from its `Transform` whenever its parent moves, so an exact matrix written to `GlobalTransform` alone would be overwritten in every frame the skeleton's entity moved. It also brings the child's descendants up to date in the same step: the builder chooses how, and must keep propagation's change tracking correct.
- **Interpolation:** a socket entity follows the skeleton's interpolation, through `Interpolated` as children already do.
- **Physics:** colliders on a socket entity follow the bone one physics step later, because physics runs before transforms. This is a documented limit. A kinematic body on a socket is moved by the physics write-back next step.
- **A missing bone name** warns once for that name and leaves the entity where it was.

### Drawing

- **A skeleton draws through the draw list as a group of consecutive draw items at its entity's z**, in the current draw order.
  - A region is a textured quad from its image region, with the slot's colour × the attachment's colour × the entity's tint.
  - **Lighting, floors and interpolation:** the group is lit or unlit as a whole, as a sprite is. It honours `Floor`/`DrawLift`, and it is interpolated.
  - **Sprite shadows and ambient occlusion** apply to the skeleton as they do to a sprite entity (per region). This is required (see the draw call under My calls), not a limit.
  - Particles' `DrawRun` is the closest precedent for batching quads from one component (`render/draw_list.rs`, `particles/draw.rs`), but it sorts after every entity at equal z and has no y-sort or shadows. The skeleton's group item extends the draw list to carry the entity sort key, and the shadows come from the entity drawer's rules.
- **Meshes (stage 2)** need a textured triangle path. The only one today is the polyline stream (`BatchShader::LineTextured`, `Vertex2D`), which is polyline-specific. Stage 2 adds a mesh shader and vertex stream, lit like sprites.
- **Debug drawing:** `debug:skeletons` (a console command and an overlay) draws bones, slots' bounds, bounding boxes and points. It replaces the old `draw_skeleton`.

### Errors and limits

- At most 255 bones a skeleton (an index fits a `u8`). The same for slots. Exceeding either fails to load with a message.
- Non-finite values in a file fail to load. In a bone override, they are ignored, with one warning.
- An animation or skin name that doesn't exist warns once per skeleton per name, and does nothing.
- Every recorded limit goes in the `ryo-skeleton` skill and the module notes.

## Public API and migration

- **Removed:** every old export (`BoneDef`, `BoneTransform`, `BoneWorldTransform`, `Pose`, `SkeletalAnimation`, `SkeletalAnimator`, `SkeletalBlender`, `SkeletalController`, `Skeleton`, `SkeletonDef`, `draw_skeleton`), `update_skeletons` and its registration in `FixedSet::Juice`, and the old demo.
- **Added to the prelude** (the module's short, generic names are prefixed there, so they stay distinct from the rest of a flat prelude; the types keep their short names in `ryo2d::core::skeleton`):
  - `SkeletonData` and its builder `SkeletonDataBuilder` (with `BoneSetup`, `SlotSetup` and `SkeletonError`), `Skeleton`, `SkeletonAnimation`, `BoneOverride` with `BoneSpace` (`Space`), `BoneSocket`;
  - the animation state's `SkeletonTrackEntry` (`TrackEntry`), `SkeletonTrackEntryId` (`TrackEntryId`), `SkeletonTrackEventKind` (`TrackEventKind`) and `SkeletonMixBlend` (`MixBlend`);
  - `SkeletonEvent`, `SkeletonTrackEvent`;
  - `BoneInherit` (`Inherit`), `BoneData`, `BoneId`, `SlotData`, `SlotId`, `SlotBlend`;
  - `SkeletonAttachment` (`Attachment`) with `RegionAttachment`, `PointAttachment` and `BoundingBoxAttachment`, `SkeletonRegion` (`Region`), `SkeletonSkin` (`Skin`);
  - `SkeletonClip` (`Animation`), `SkeletonTimeline` (`Timeline`), `SkeletonCurve` (`Curve`), `SkeletonEventData` (`EventData`), `SkeletonEventValues` (`EventValues`), `SkeletonEventAudio` (`EventAudio`);
  - `SkeletonLoader`, with the other loaders.
- The loader for `.skeleton.ron` is registered with the asset registry.
- Tests that used the old module (`tests/e2e/gpu_world.rs` "Skeletal" scene) are replaced with tests of the new one.

## Stages

1. **Core.** Data, the format and loader, the builder, bones with inherit modes, slots, region, point and bounding-box attachments, skins, timelines (bone, slot attachment, colour, draw order, event), curves, the animation state (tracks, queue, mixing, additive, event threshold, hold previous), events, bone overrides, sockets, drawing, interpolation, hot reload, `debug:skeletons`, the new demo, the skill, a bench and guards. Delete the old module.
2. **Meshes.** Mesh attachments (weighted and unweighted), deform timelines, the mesh draw path with lighting, the demo's mesh page, a bench case.
3. **Constraints.** IK constraints (one bone, two bones with bend direction, softness, compress and stretch, mix), transform constraints, constraint order, IK and transform mix timelines, the demo's IK page (feet on uneven ground, an arm reaching the pointer).
4. **Import.** The owner chooses Spine 4.x JSON (clean-room, after Esoteric's written answer) or DragonBones 5.5 JSON. The importer maps the tool's data to `SkeletonData` (y-flip, angle sign, curves) and is tested against hand-written files covering every feature. The demo gets an imported character, made by the owner or from freely licensed art.

## Acceptance (stage 1)

### Correctness (tests)

- **Bone maths:** world transforms for each inherit mode, with rotation, scale, shear and negative scale, match hand-computed expectations to 1e-4.
- **Curves:** linear, stepped and bezier sampling against known values. A bezier curve's ends hit their keys exactly.
- **Mixing:**
  - a crossfade's pose at 0%, 50% and 100% matches its expected blend;
  - an additive track adds;
  - a higher track at alpha 0.5 blends half;
  - `set_empty` fades a layer out;
  - the queue starts each entry at its delay.
- **Events:**
  - every key fires exactly once across loops, a step larger than the animation, time scale 0 and 3, and a pause;
  - none fires after a mix-out passes its event threshold;
  - order within a step is by time.
- **Sockets:** a child on a bone is where the bone is in the same step: its `GlobalTransform` equals the bone's world matrix × the offset for an unskewed bone (under a skewed bone the skew is dropped, and the socket keeps the bone's place and direction). It follows interpolation. A missing bone warns once.
- **Hot reload:** a playing skeleton keeps its animation and time after a reload that adds a bone. A reload that removes the playing animation clears that track with one warning. A broken file keeps the last good data.
- **Drawing (e2e, pixels):**
  - a region on a rotated bone draws where expected;
  - a y-sorted skeleton draws behind a `YSort` sprite lower on screen and in front of one higher up, and the same among a y-sorted tile layer;
  - a skeleton casts a sprite shadow;
  - draw order swaps change which region is on top;
  - a skeleton is lit by a light, and sits on a floor's lift;
  - two skeletons at different z sort with a sprite between them.
- **Allocations:** the allocation test's lit scene with 50 animated skeletons allocates nothing per frame after warm-up.
- **Validation:** each malformed-file case fails with a message naming the item.

### Targets (record machine, quiet, `uptime` recorded)

- 200 skeletons of 30 bones and 25 region slots, each playing two tracks with a crossfade in progress:
  - the update is at most 1.0 ms per fixed step;
  - the drawing work in extract is at most 0.6 ms per frame.
  - Both are release builds, the median of three runs, with the load flagged above 3.
- One skeleton's update is at most 5 µs.
- Recorded in `docs/backlog/skeleton.md`, with guards in `benches/skeleton.rs`.

### Demo

A new `examples/demos/skeleton`, written to the demo standard: a library and a binary, its own `assets/` with a README giving the art and its licence, `tests/pages.rs`, and a web build. Pages:
1. a character with idle, walk, run and jump, crossfaded from keys;
2. layers: walk while waving or aiming, with alpha;
3. events: footsteps play a sound and puff dust;
4. sockets: a sword in the hand, with a trail and a hitbox shown;
5. skins: swap outfits and combine a hat;
6. bone override: the head and arm track the pointer;
7. a crowd of 200, with the frame cost shown.

Every page builds and steps headlessly with no warnings, and the tour captures each.

## Documentation

- A new skill, `ryo-skeleton`, covering:
  - the workflow (the file, or the builder);
  - the data model;
  - playing and mixing;
  - events;
  - sockets;
  - overrides;
  - skins;
  - hot reload;
  - limits;
  - testing.
- The `ryo-api` listing, and the `ryo-assets` entry for the new asset type.
- The module notes for `src/core/skeleton/`, and the overview.
- The research note on formats.
- No stage labels in skills.

## Risks

- **Draw path fit:** a skeleton's group of runs must sort as one unit at its z, with per-slot texture changes. If the draw list can't keep a group together against other items at the same z, stage 1 extends it (a group key). Measure first.
- **Mixing semantics:** Spine's mixing rules (hold previous, the event threshold, additive with an empty track) are subtle. Tests pin each rule with a worked example. Where we differ from Spine, it is written down, so stage 4 knows.
- **Art:** the CC0 pack may lack parts for a convincing rig. The fallback is Kenney's, and stage 1 checks both before planning the demo pages.
- **The stage 4 licence:** this is outside our control, and is the owner's step.

## Meshes: details settled at planning (2026-11-13)

Stage 2 builds on the merged core. These are the planner's calls. The owner may revisit them when the plan comes for approval.

### What a mesh is

- **A mesh attachment** names a region and carries:
  - vertices;
  - a UV per vertex, as a fraction of the region (0 to 1 across it);
  - triangles, as vertex indices;
  - a hull, the number of vertices that form its outline in order (for the debug overlay);
  - a colour.
- **Vertices are unweighted or weighted.**
  - An unweighted vertex is a position in the slot's bone space.
  - A weighted vertex is a list of influences, each a bone, a position in that bone's space and a weight. Its place is the weighted sum of those positions, each under its bone.
  - A vertex may have any number of influences. Weights are normalised at load, and a vertex whose weights sum to zero fails to load.
- **Skinning is on the CPU**, each frame a mesh is drawn, from the same interpolated bone matrices the region quads use. It writes into the frame's mesh stream, a stream of the batch's own for meshes beside the triangles' (lines and trails). GPU skinning stays a backlog lever.

### Deform keys

- **A deform timeline** belongs to one slot and one attachment name. Each key holds an offset per influence position: for an unweighted mesh, one per vertex in the slot's bone space; for a weighted mesh, one per influence in that influence's bone space. So a keyed deformation turns with the bones it rides.
- An empty key means no offset: the setup shape.
- **Mixing:** Replace moves the offsets toward the key by alpha. Add adds the key's offsets times alpha. The cover rule of a crossfade treats (slot, attachment name) as the property.
- **When a deform applies:** only while its slot shows that attachment and the attachment is a mesh with the key's count of positions.
  - A deform whose slot shows another attachment, or none, does nothing, in silence. That is ordinary play: an attachment key, a crossfade between attachments, a game's own attachment in the slot.
  - Under its own name, an attachment that is not a mesh, or a mesh with another count of positions, is skipped: a region under the same name, a skin's mesh of another shape, or data reloaded with a different count. Nothing is drawn wrong, and one warning is given per skeleton per animation.
  - A deform that names an attachment no skin has under its slot fails to load, with the animation, the slot and the attachment named: it could never apply.
- **Reset:** the offsets return to zero when the slot's attachment changes.
- **Interpolation:** deform offsets are blended between steps at draw time, as bones are.

### Drawing

- **Order:** a mesh draws in the skeleton's draw order, between its regions, as part of the entity's place in the entity ordering. The draw list gains a mesh item of the entity kind.
- **Look:** a mesh must look exactly like a region quad of the same image.
  - It draws through the sprite shader's own fragment stages. A mesh carries one sprite instance (the rectangle of the image, the tint, the effect parameters, the emission) beside its vertices, each of which is only a position and a UV.
  - Its colour is slot × attachment × the entity's tint and alpha.
  - The entity's built-in sprite effects that work in place (flash, colour shift, silhouette, grayscale, invert, pixelate, wave, dissolve) apply to a mesh as to a region, and so does a game's own sprite shader (`Renderable::shader`, a `Material`).
  - It is lit by the entity's `lit` flag, and lifted by its floor.
  - It mirrors with a flip, and no face is culled.
- **Shadows:** a drop shadow draws the mesh's silhouette as it does a region's. Blob and contact shadows are the entity's, unchanged.
- **Limits, recorded:**
  - Effects that grow their sprite's quad (outline, glow, directional outline) do not apply to mesh parts: a quad is enlarged to hold what they draw round the sprite, and a mesh has its own shape. Under one a skeleton's regions take the effect and its meshes draw plain, with one warning in the log.
  - Each mesh is a draw of its own: its instance is its alone, so meshes do not merge with one another or with the quads round them.
  - Pixel-art snapping does not apply to mesh vertices.
- **Debug:** `debug:skeletons` draws a mesh's hull and triangles.

### File format

A `Mesh(..)` attachment and a `deform` block per slot in an animation. The exact keys are in the plan and are checked by the loader's tests. The format carries everything the data can hold.

### Acceptance (stage 2)

- **Pixel tests:**
  - a two-triangle mesh of a region draws the same pixels as the region quad;
  - a weighted mesh bends between two bones;
  - a mesh sorts between regions by draw order;
  - a mesh on a y-sorted, lit, floored, flipped and interpolated skeleton behaves as its regions do;
  - a deform key moves a vertex;
  - a crossfade blends two deforms.
- **Robustness:**
  - a reload that changes a mesh's vertex count while a deform plays does not panic and draws the setup shape;
  - malformed meshes fail to load with the item named.
- **Allocations:** no per-frame allocation with meshes drawn.
- **Speed** (release, quiet machine, median of three):
  - 200 skeletons of 25 regions plus one weighted mesh of 60 vertices each: the drawing work in extract is at most 600 µs a frame, and the update at most 1,000 µs a step;
  - the region-only figures of the core do not rise by more than 5%.
- **Demo:** a new page with a weighted cape that swings with its bones, a banner rippled by deform keys, and a line pointing at the console's `debug:skeletons` for the overlay. The page tests cover it.

## Constraints: details settled at planning (2026-11-14)

Stage 3 builds on the merged core and meshes. These are the planner's calls. The owner may revisit them when the plan comes for approval.

The rules come from Spine's public user guide and format reference (its pages on IK constraints, transform constraints and constraint order), from geometry and from first principles. Where the solvers below differ from what those pages describe, the difference is stated.

### What a constraint is

- **A constraint moves bones after animation**, by a rule and not by keys. A skeleton's data holds one list of constraints, in the order they apply. There are two kinds:
  - an **IK constraint** turns one bone, two bones or a chain so the last one's tip reaches or points at a target bone;
  - a **transform constraint** makes one or more bones copy a target bone's rotation, place, scale or shear, each by its own mix.
- **A target is a bone.** A rig gives a constraint a target bone of its own (often a child of the root, with no image), and a game moves it with a bone override in world space, or an animation keys it. So a target moves in the same step as the bones that follow it, with no lag.
- **Skeletons without constraints do no new work**: their step takes the path it takes today.

### IK constraints

- **Settings:** a name, the bones (first to last), the target, and:
  - `mix`, 0 to 1: 0 leaves the animated pose, 1 is the solved pose, and between them each bone's rotation is blended the short way;
  - `bend_positive` (two bones): which way the joint bends, judged in the space of the first bone's parent. Positive means the turn from the first part of the limb to the second is in the positive direction there. For a skeleton that is not mirrored that is clockwise on screen; under a mirrored ancestor or a flipped entity the limb is drawn mirrored, so a character turned round keeps its knees. The bones' own scales do not change which side that is;
  - `softness` (two bones): a distance. When the target is within this distance of the bones' full reach, the tip slows as the bones straighten, and never snaps straight;
  - `stretch` (one or two bones): when the target is beyond reach, the first bone is scaled along its length so the tip reaches it;
  - `compress` (one bone): when the target is nearer than the bone's length, the bone is scaled down so its tip is on it;
  - `uniform`: with stretch or compress, scale the bone across its length by the same factor.
- **One bone** turns to point at the target. Pointing needs no length, so a bone of length 0 can look at a target.
- **Two bones** (the second an immediate child of the first) are solved as a triangle: the two parts' lengths and the distance to the target.
  - The solve is in the space of the first bone's parent, so a squashed, sheared or mirrored ancestor is handled exactly: the target is brought into that space first, and every length and distance below is measured there.
  - The first part runs from the first bone's origin to the second bone's origin, wherever that is: the second bone need not lie on the first's axis. The second part is the second bone's length along its axis.
  - Both parts are taken as rigid, at their lengths in the animated pose, and the triangle is solved by the law of cosines. That is exact when the first bone's own scale is even and it has no shear (the usual case). When it is uneven or sheared, the second part's length changes as it turns and the tip misses: on a limb of parts 40 and 30, by up to 7% of the limb's reach under a first bone scaled (1.2, 1), and by more for a more uneven bone (a recorded limit). Spine's guide meets that case by moving the second bone onto the first's axis and clearing the first's shear; this design changes neither.
  - A target too far away straightens the bones toward it. One too near folds them as far as they go.
  - **Softness:** with full reach `m` and softness `s` (taken as `m` at most), a target distance `d` beyond `m - s` is replaced by `(m - s) + s × (1 - e^(-(d - (m - s)) / s))`, which approaches `m` and never reaches it. The slope is continuous where it begins. The triangle is solved for that distance, and `mix` then blends toward the result.
  - **Stretch** applies only when softness is 0 and the first bone's scale is even and unsheared, as Spine's guide has it. It reaches the target exactly when the second bone lies on the first's axis or `uniform` is set.
- **Stretch and compress** multiply the first bone's own scale along its length (and across it with `uniform`) by the target's distance over the reach, blended by `mix` as `1 + (factor - 1) × mix`.
- **A chain** (three to sixteen bones, each an immediate child of the one before) takes one shape from a family of shapes that runs through its animated pose, and is turned onto its target. A chain has `mix` only. Spine has no chains; this is the engine's own.
  - A target farther than the animated pose's tip opens the pose: every bend is scaled by one factor, toward straight. A nearer one closes it: an even turn is added at every joint, the way the pose bends, so the chain curls further as an arc.
  - The shape taken is the one whose tip is the target's distance from the first joint, and the whole chain is then turned about its first joint so the tip is on the target. The shape follows the target's distance alone and the target's direction only turns the chain. So a chain does not pop as its target moves, meets a target near its full reach, and straightens smoothly as the target leaves it.
  - A pose with no bend of its own bends the positive way, as an even arc.
  - The same pose and target always give the same result.
- **Every bone an IK constraint turns must inherit normally** from its parent. Another inherit mode fails to load, as Spine's guide rules it out.

### Transform constraints

- **Settings:** a name, the bones, the target, six mixes (`rotate`, `x`, `y`, `scale_x`, `scale_y`, `shear_y`), six offsets added to the target's values (the same six), and two switches, `local` and `relative`.
- **A mix** of 0 leaves that property alone and 1 applies it fully. A mix may be below 0 or above 1: a gear that turns against another is a rotate mix of -1.
- **World, not relative** (the default): each bone's transform in the skeleton's space moves toward the target's.
  - Place: toward the target's point `(offset x, offset y)`, by the x and y mixes.
  - Rotation: its x axis turns the short way toward the target's x axis plus the offset.
  - Scale: each axis's length moves toward the target's axis's length plus the offset.
  - Shear: the angle between its axes moves toward the target's plus the offset.
- **World and relative:** the target's values (plus offsets) are added to the bone's, times the mix: its place is moved by the target's point, it is turned by the target's turn, and its axes' lengths are multiplied by `1 + (target's length - 1 + offset) × mix`.
- **Local, not relative:** the bone's own values (those animation keys set) move toward the target's own values plus the offsets.
- **Local and relative:** the target's own values plus the offsets are added to the bone's, times the mix (scale multiplies, as above).
- **The target's own values** are read as they stand when the constraint runs: after animation, the game's overrides and any earlier constraint that changed them.
- **Rotation in a local mode is the plain number**, not the short way round: an own rotation is what keys set and may pass a full turn, so a gear at half the speed of another is a local relative rotate mix of 0.5 and turns smoothly. A world mode reads the way an axis points, which is only known within a turn, and goes the short way.
- **Mirroring is the bone's own.** In a world mode a length is never negative and shear is how far a bone's axes are from square on that bone's own side, so a transform constraint never mirrors a bone or undoes its mirror. A mirrored target (one whose axes are reflected) mirrors the rotation and shear offsets.
- A world mode sets a bone's transform in the skeleton's space directly, whatever its inherit mode, and leaves its own values as they were. A local mode changes its own values.
- **A constraint's bones are taken in bone order.** Each is set, and its descendants placed again, before the next is read, so a bone and its own descendant can both be in one constraint.

### Order and the step

- **The step** becomes: animation, then the game's bone overrides and every bone's transform in the skeleton's space (as today), then each constraint in order, then the root transform, sockets and events.
- **After a constraint is applied**, the descendants of the bones it moved are placed again (and, for IK and the local modes, those bones themselves, from their new values), so the next constraint sees them where they now are.
- **A later constraint wins.** If a later constraint moves an ancestor of a bone that an earlier world-mode transform constraint placed, that bone is placed again from its own values and the earlier result is lost. This is Spine's stated rule. IK and local-mode results are kept, since they are in the bones' own values.
- **Overrides come first, and still hold.** A world-space override is converted against the pose before constraints, so an override on a target bone steers the constraint in the same step. When a bone is placed again because a constraint moved a bone above it, its override is applied afresh against the new pose, so a world-space override on it still points where the game asked: a foot held level under a shin that IK turns stays level. A bone that a constraint has itself changed keeps the constraint's result: on that bone the constraint has the last word over the override. An IK constraint at a mix of 0 changes nothing, so it is passed over, as one under a flat parent is, and its bones are left to the game's overrides.
- **Placing a posed skeleton again** between steps (it rides another skeleton's socket, or it is flipped) gives the pose the step gave: constraints run again from the animated pose.
- **Drawing between steps** blends the constrained pose, as it blends any pose.

### Keys

- **An IK timeline** keys one constraint's `mix`, `softness`, `bend_positive`, `compress` and `stretch` together. **A transform timeline** keys one constraint's six mixes.
- A value a key leaves out is the constraint's setup value.
- Numbers blend between keys by the key's curve, and between animations as bone values do (replace toward the key, or add). A crossfade treats a constraint as one property. A mix is clamped to its range after the tracks are applied.
- The three switches are stepped: they change at their key, and follow the module's rule for attachment keys. In a crossfade the newest animation with any weight decides, and if it has no key for the constraint the setup value stands from the first step of the mix.
- An override of a switch forces it, whatever the animations key.
- Two timelines for one constraint in an animation fail to load.

### The game's side

- **`Skeleton::override_constraint(name, ..)`** sets any of an IK constraint's keyed values, or any of a transform constraint's mixes, after animation and until `clear_constraint_override(name)`. A value left out is the animated one. It is how a game turns foot IK off in the air.
- **A target is moved** with `override_bone` on the target bone, in world space.
- Overrides are kept by name through a hot reload. An unknown constraint name warns once per name and does nothing.

### Validation

A constraint that cannot work fails to load, naming the constraint and the reason:

- names are unique among constraints of both kinds; at most 255 constraints;
- every bone and the target exist; a constraint's bones are distinct; the target is not one of its bones nor a descendant of one;
- IK: one to sixteen bones, each after the first an immediate child of the one before; every bone inherits normally; `mix` is 0 to 1; `softness` is finite and not negative;
- IK settings that cannot apply are refused: `bend_positive` set false, or `softness`, on other than two bones; `compress` on other than one; `stretch` or `uniform` on a chain;
- IK lengths: the last bone of a two-bone or chain constraint has a length above 0 (its tip is what reaches, and a length left out would make the constraint do nothing in silence); a one-bone constraint with `stretch` or `compress` has a length above 0;
- transform: at least one bone; mixes and offsets finite;
- timelines: the constraint exists and is of the timeline's kind; values are finite and in range; a key sets no switch its constraint could not have.

### Limits, recorded

- **Constraints bound to a skin** (active only while a skin is worn) are not built: every constraint is always active. The import part decides whether it needs them.
- **Two bones whose first has an uneven scale or a shear of its own** miss the target, by more the more uneven it is. The second part is pointed the right way and comes out another length, so the tip is off along it, by up to the difference between the longest and the shortest the first bone makes that part. On a limb of parts 40 and 30 that is up to 7% of the limb's reach at a scale of (1.2, 1), 14% at (1.5, 1) and 21% at (2, 1). Where such a limb passes from bent to straight or to folded, the tip follows its target steeply, though without a jump. An uneven or sheared ancestor is exact.
- **A chain** assumes each of its bones has an even scale and no shear; otherwise its tip may miss.
- **A chain curls and does not fold.** It reaches inward only as far as an arc of its bones curls. Equal bones curl onto their first joint. Unequal ones stop short of it, a tenth of the chain's length away at the median and a third for one pose in ten, and a nearer target is not reached: the chain takes its nearest shape, pointed at the target. Of targets within 30% of a chain's length, 61% to 91% are met; from half its length out, all but one in a thousand.
- **A chain changes sides when its animation swings through straight** while its target is nearer than its animated tip. It closes the way its pose bends, so it flips to the mirror shape: a joint moves 0.7 of the chain's length for a target at half the length, 0.4 at 90% and 0.1 at 99.5%. A chain that is to reach inward is kept bent one way throughout its animation, as a limb is given a bend direction.
- **A chain wound more than about 250 degrees** may jump as its target moves outward, by half its length.
- **A target passing over a chain's first joint** turns the chain half round.
- **Stretch on two bones** falls a little short when the second bone is off the first's axis and `uniform` is not set.
- **A world mode with a mix that is not a whole number** jumps when its target passes half a turn, since the way an axis points is only known within a turn. A local mode does not.
- **An IK constraint with a mix strictly between 0 and 1** jumps the same way. Each bone is turned part of the short way from its animated rotation to its solved one, and when the solved one passes half a turn from the animated one the short way changes sides: the bone jumps by a full turn times the mix. At a mix of 1 nothing jumps.
- **A world-space override on an IK bone changes hands as the constraint's mix leaves 0**, where a later constraint moves a bone above that bone. At a mix of 0 the constraint is passed over and the override is applied afresh, so the bone points where the game asked; above 0 the constraint has the last word, and the bone keeps its own values and is carried round by the bone above it. An eye held at 0.3 in the world under a hip turned a quarter turn points at 0.3 at a mix of 0 and about 1.6 further round at 0.01.
- **A target moved from frame-rate input** (the pointer) is read in the fixed step, so the bones follow at the step's rate and are drawn blended between steps, as every bone is.
- **A game cannot read where animation puts a bone in the step under way**, only where the last step left it. So lifting a keyed foot target to the ground reads that target a step late (a step's travel, at most), or the game computes the target itself, as the demo's walker does. An override that adds to the animated place, or a read of the animated pose, is the lever if a game needs better.
- **Path and physics constraints** stay in the backlog.

### File format

A `constraints` list at the top of the file, in the order they apply, each `Ik(..)` or `Transform(..)`; and a `constraints` block in an animation, by constraint name, holding an `ik` or a `transform` list of keys. Angles are degrees, as everywhere in the file. The exact keys are in the plan and are checked by the loader's tests. The format carries everything the data can hold.

### Debug and demo

- **`debug:skeletons`** draws, for an IK constraint, a mark at the target and a line from the last bone's tip to it; for a transform constraint, a line from each bone to the target.
- **The demo gets a ninth page, "Constraints":**
  - an arm of two parts that reaches for the pointer, with settings for the bend, softness, stretch and mix;
  - a walker whose two-part legs plant their feet on uneven ground as it crosses it;
  - a part held level by a transform constraint while the arm carrying it turns.
- **The hero's pelvis** follows its trunk through a transform constraint (it is keyed by hand in three animations today), so every animation gets it.

### Acceptance (stage 3)

- **Solvers** (unit tests on positions, to 0.01 of a unit):
  - one bone points at its target, at each mix, with stretch, compress and uniform;
  - two bones reach a target in range on each bend side; straighten toward one out of range; fold for one too near; under softness the tip's distance follows the formula and is continuous;
  - the same under a rotated, unevenly scaled, sheared and mirrored ancestor, and with the second bone off the first's axis; the bend side holds under a mirrored ancestor, a flipped entity and a first bone mirrored by its own scale;
  - a chain meets a target from its nearest shape out, straightens toward one out of range, does not pop as its target moves, and gives the same result on every run;
  - a transform constraint in each of its four modes, each mix alone, with offsets, at mixes of -1, 0.5 and 2, under a mirrored target and on a mirrored bone; a local relative rotate mix of 0.5 turns smoothly through a full turn of its target.
- **Order:** two constraints in each order give the results the order rule says; a world-mode result is lost when a later constraint moves an ancestor; a bone and its descendant in one constraint are both constrained; a world-space override on a bone below an IK bone still holds; a placed-again skeleton equals its step's pose; a skeleton posed in the frame it appears has its constraints applied.
- **Keys and overrides:** a keyed mix blends through a crossfade without a dip; a switch changes at its key and follows the attachment rule in a crossfade; an override beats the animation and is cleared; a target bone moved by a world override is reached in the same step.
- **Pixels:** a two-bone arm reaches a moved target; a constrained skeleton flipped, y-sorted and drawn between steps behaves as an unconstrained one does; a socket on a constrained bone follows it in the same step; the overlay draws its marks at the target and the tip.
- **Robustness:** every validation rule fails with the constraint named; a reload that removes or reorders constraints keeps playing, with overrides kept by name; a target at the first bone's origin, a second bone at the first's origin, a flat (zero-scale) parent, and a flat bone or target under a transform constraint change nothing they cannot solve and make no NaN.
- **Allocations:** no per-step allocation with constraints running.
- **Speed** (release, median of three, with `uptime` recorded):
  - 200 skeletons, each with two two-bone IK constraints, one one-bone IK constraint and one transform constraint: the update is at most 1,000 µs a step;
  - the figures of skeletons without constraints do not rise by more than 5%, measured in one sitting against the build before constraints.
- **Demo:** the Constraints page as above; the hero's pelvis by constraint on every page. The page tests cover both.
