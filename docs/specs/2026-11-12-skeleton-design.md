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
