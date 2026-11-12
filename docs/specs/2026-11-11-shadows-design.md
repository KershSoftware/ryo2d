# Shadows Design

## Overview

**Intent.** 2D shadows should look and cost what they do in a professional 2D engine: soft edges that are physically plausible (sharp where an object meets its shadow, wider farther away), any number of shadow-casting lights for the price of their pixels rather than three or four draws each, CPU work that grows with what is near a visible light rather than with every light times every occluder, and the cases games meet every day (a character carrying a lantern, a tile map full of walls, split screen) working without workarounds.

**Why now.** This is the fourth of the six sub-projects left from the engine audit (`docs/research/2026-10-11-engine-audit.md`, "Lighting": "keep accumulation and compose, rewrite shadows (per-light 1D shadow maps or SDF) with an occluder spatial index"). Part of the audit is out of date: the HDR renderer work (`docs/specs/2026-10-13-renderer-hdr-design.md`) already made each light mask itself, so one light's shadow no longer darkens another's, removed the light and vertex caps and the dead raymarch path, and stopped the per-pair allocations. It left the shadow rewrite itself out of scope. What remains today:

- **Soft shadows are a fake.** `shaders/core/shadow.wgsl` fades a shadow with distance from the light and keeps a hard outline: no penumbra. The backlog says "true penumbrae wait for the shadow rewrite".
- **Each shadowed light costs three or four draws** of its own per camera (reset its mask, its volumes, the masked add, the coloured add: `render/batch/gpu.rs`, `pipelines.rs` `light_blend`), and the HDR spec's risks say "dozens of them add up until the shadow rewrite".
- **Shadow geometry is built on the CPU for every light in the world**, visible or not, against every occluder (`lighting/shadow.rs` `push_shadow_volumes`, called from `engine/extract.rs` for each light): 288 bytes of vertices and 24 of indices per casting edge per light per frame, no spatial index. One shadowed light over the 1024 x 1024 cave of `benches/tilemap.rs` (2,378 chains) costs about 30 µs a frame more than the unlit map.
- **A light inside a closed occluder casts the whole occluder outward.** Every edge faces away from it, so a lantern held inside its bearer's occluder blocks itself in every direction. A carried light cannot sit on something that casts a shadow.
- **Overlapping edges darken twice.** A map's shape tile whose outline lies on a neighbour's side casts that edge twice (a recorded tilemap Limit), and partial-strength shadows compound through every wall they cross.
- **The showcase does not show shadows.** The `lighting` demo's lamp never turns shadows on, so its occluders cast nothing; leap-king's soft-shadow lamps have no occluders at all. Nobody has checked either by eye (`docs/backlog/follow-ups.md`).

**Who it serves.** The game developer (often an AI agent working from the skills), who wants to put a light down, mark what blocks it, and get good-looking shadows on every platform with no tuning; and the player, whose frame time should not depend on how many torches are outside the screen.

## Goals

- **Real soft shadows.** A shadow's edge is a penumbra whose width grows with the distance from the occluder and with the light's size: contact-hardening, as area lights give. A light with no size casts a hard, clean edge.
- **One light draw per camera.** Every light, shadowed or not, draws in one instanced draw per lit camera, sampling its shadow from a shared shadow atlas. The per-light mask passes and their pipelines go.
- **Shadows are computed on the GPU, once a frame per light, for the lights some camera can see**, and shared by every camera that sees them (split screen and minimaps pay once).
- **An occluder index.** Each shadowed light reads only the occluders near it, from an index updated only for occluders that moved, changed, appeared or went. A static tile map costs nothing to index frame after frame.
- **A light inside a closed occluder is not shadowed by that occluder.** A character can carry a lantern and still cast a shadow from other lights; a lamp can sit inside a closed occluder of its own (a bracket, a lamp post's base). A tile map's walls are open chains and are not covered: a light inside a map's wall still lights nothing outside it, so a wall light belongs at the wall's face (a recorded limit).
- **Each light is held back once, however many occluders cover a point**: no double-darkening from coincident edges or stacked walls.
- **No regressions in what works today**: one light's shadow never darkens another; coloured shadows; spot lights' cones; floors (a light on a storey is blocked only by that storey's occluders and `Floor::Every`'s, drawn at the light's lift); tile maps' chain occluders casting as the polygons they bound; a lit frame over a static map allocating nothing; every camera lit; HDR light above 1; the web build (WebGPU).
- **The showcase shows it.** The `lighting` demo and the `shadows` demo show hard and soft shadows, a carried lantern and many shadowed lights; leap-king's lamps get the platform occluders they were meant to have.

## Non-goals

- **A light image per storey.** A light still brightens every storey under it on screen (one light buffer per camera); per-storey light buffers are a separate piece of work (`docs/specs/2026-10-31-tilemap-shapes-design.md`, Floors).
- **Normal maps, light cookies, area-light shapes** (lines, rectangles): the light's size is a disc radius.
- **Shadows from sprites' alpha** (a sprite casting by its pixels). Occluders are geometry.
- **3D-style height** (occluders with a height that shortens their shadow, sun angles). A directional light (sun or moon casting parallel shadows across the whole view) is out of scope here and goes to the backlog.
- **Sprite shadows** (`DropShadow`, `BlobShadow`, `ContactShadow`) and the ambient-occlusion post effect: unchanged, independent of lights.
- **WebGL2.** The web build is WebGPU only, as today.

## Anti-goals (the professional bar)

- **Shimmering or crawling edges** as a light or an occluder moves slowly, or as the camera pans: the edge must move smoothly, sub-pixel.
- **Light leaking** through the joints of a tile map's walls or between touching occluders, and **acne** (speckled self-shadow on lit ground near an occluder).
- **A shadow quality that depends on the camera's zoom** in a way a player notices (soft at one zoom, aliased at another).
- **A cost for lights off screen**, or for a light that casts no shadow, beyond today's.
- **A per-frame allocation** in a lit frame over a static scene, or a frame time that grows with every occluder in the world when only a few are near the visible lights.
- **A hidden cap that silently drops shadows** without saying so in the log.

## Decisions

### The owner's decisions

- The owner left the design to the planner ("continue", 2026-11-11, as for the last two sub-projects: design it as a professional engine would, and bring the spec and plan together for one approval). Breaking changes to the engine and its demos and games are fine.

### My calls (the owner may revisit them in review)

- **Per-light shadow maps in an atlas, not signed distance fields.** Each shadowed light renders its occluders into rows of a shared depth atlas: four 90° views around the light (right, down, left, up), each a row of nearest-occluder distances. This is what Godot's 2D renderer does. A distance field of the occluders (jump flood per frame, then ray-marched shadows) gives soft shadows for any light at a fixed cost per pixel per light, but it is per camera and per view rather than per light, has a resolution tied to the view (the zoom anti-goal), and costs a march per lit pixel per light, which scales badly with many lights. Shadow maps make a light's cost its occluders near it plus its pixels, share between cameras, and filter well.
- **Soft shadows by filtering the map with a width from the geometry.** A lit pixel compares its distance to the light with the map's nearest occluder, looking up a short arc of the row around its angle. The arc's width follows from the light's size and the gap between the pixel and the occluder (percentage-closer soft shadows, with the blocker search a first look-up). A light of size 0 takes one look-up: a hard edge.
- **`Light2D`'s soft-shadow setting becomes the light's size in pixels.** `with_soft_shadows(size)` and `set_shadow_softness` take the radius of the light's source in pixels (0 hard), in place of today's 0-to-1 fade. Today's fade goes. Artist-friendly units match the rest of the light (its radius is in pixels).
- **Only edges facing away from the light cast**, as today. An occluder's own body and the faces turned to the light stay lit, so a wall's top or a crate standing on its own occluder is not darkened by it, and a map's chains cast as the polygons they bound. This is the back-face rule today's shadow volumes use, kept for the maps. (An occluder may now shadow itself instead: Self-shadowing occluders, below.)
- **A closed occluder that contains the light does not shadow it.** Decided per light when its map is built (the light's position tested against the occluder's placed polygon). Open chains have no inside and always cast.
- **Shadow strength applies once.** A point behind any occluder of a light gets that light times one minus the shadow's strength (the shadow colour's alpha), however many occluders lie between. Today two 50% shadows give 25%; after, 50%. This follows from storing the nearest occluder, and it is what Godot and Unity's 2D lights do. The e2e test pinning compounding is replaced.
- **Coloured shadows keep their meaning**: where a light is held back, the shadow colour's colour lets that colour of the light through, in proportion to how shadowed the pixel is (penumbrae blend).
- **The atlas** is a 2D depth texture, one row per view, four rows per shadowed light, rows packed in the order the lights are gathered. Its width (the resolution of a view) has four steps, `ShadowQuality::{Low, Medium, High, Ultra}` = 256, 512, 1,024 and 2,048 texels a view, default `High`. The quality also chooses how many look-ups a soft edge takes.
- **Shadow quality is a player graphics setting**, in the player's `Settings` (`ctx.settings()`, saved to `system.json` with the other graphics settings, as vsync is), so it survives scene changes and a game's options screen can offer it. A game sets its default as it does the other settings' (settings module notes).
- **Edge quality has a stated envelope.** A shadow map's resolution is per angle, so a texel's size on screen grows with the distance from the light and with the camera's zoom: at `High` one texel at the view's centre is about 0.6 px at 300 px from the light and 2 px at 1,000 px, at zoom 1. The edge targets below hold for lights up to 512 px radius at zoom 1 at `High` (and proportionally: half that radius at `Medium`, twice at `Ultra`); beyond that, edges soften and step by up to a texel's width, and a larger light or a zoomed-in camera calls for a higher quality. Adaptive per-light resolution is out of scope (backlog).
- **A frame shadows at most 512 lights.** If more shadowed lights reach a lit camera, the ones nearest a camera's centre (in world units, then by entity order) keep their shadows; the rest draw unshadowed, and the log warns once per run naming the count. 512 lights x 4 rows is 2,048 rows, well inside WebGPU's default 8,192-texel limit (the width, set by the quality, is at most 2,048). The cap is the smaller of 512 and a quarter of the device's maximum texture height, a constant in one place.
- **Which lights get a map.** A light casts only if it casts shadows, is enabled, has a positive finite radius and intensity, and its box (centre plus or minus radius, lifted on a storey) meets the view of at least one lit camera it shares `RenderLayers` with. Maps are built once a frame, before any camera's light pass, and every lit camera samples the same atlas.
- **Which occluders a light reads.** The occluders whose world bounds meet the light's box, from the index, then filtered by floor (`Floor::meets`, unchanged) and by containment (above). A spot light skips views wholly outside its cone and edges outside its cone (as today's cone test, with a margin for its softness).
- **The occluder index** is engine-owned: each enabled occluder's world bounds (its placed vertices' box), keyed by entity, kept in a uniform grid of cells (the cell size a constant chosen in the plan from the tile map chunk size and typical light radii). It is updated in `PostUpdate` after transforms propagate, for occluders whose `GlobalTransform` or `LightOccluder2D` changed (which covers `set_enabled` and a map chain moved through its parent), that were added, or that were removed, despawned, given the ECS's `Disabled` or had it taken away (both transitions watched, as neither shows as a transform change). A frame where nothing moved does no index work. An interpolated occluder's drawn placement (`render_global`, which blends rotation as well as position) may differ from its current placement: its index bounds are the union of its previous and current placed boxes, so a swinging door is never missed.
- **Occluder geometry goes to the GPU as edges, per light**, as instances: for each casting edge of a read occluder and each of the light's views it crosses (decided on the CPU from its end points' angles, so at most four), its two world-space end points, the light's slot and the view. The vertex shader draws it as a quad one texel tall on the view's row (lines have no dependable width or coverage in WebGPU, and a zero-height triangle covers nothing), projected so the view's ±45° fall on the row's two ends and the GPU's own clipping trims the edge to the view (core WebGPU has no per-instance viewport, scissor or clip distance); a near plane just in front of the light keeps an edge through or behind the light from dividing by zero. Depth-testing keeps the nearest. The CPU work per light is the index query, the facing and containment tests, and writing 16 to 24 bytes per casting edge: no triangles, no projection.
- **One draw builds every map.** All shadowed lights' edges go in one instance buffer and one draw (or one per view direction, if the plan finds that simpler), writing the whole atlas, then every lit camera's single light draw reads it.
- **Spot lights' cones**, falloff, intensity above 1, volumetric beams and glows (masked by the light's shadow as today), god rays and the compose pass are unchanged.
- **Leap-king's lamps get occluders**: its platforms get the occluders its unused `PLATFORM_OCCLUDER_HEIGHT` constant was meant for, so its soft-shadow lamps cast. The `lighting` demo's lamp turns shadows on.

## Design

### The frame

1. **Gather** (`engine/extract.rs`): lights as today. The occluders are no longer gathered whole each frame: the index holds them.
2. **Plan the shadows** (CPU, once a frame): from the lit cameras' views and layers, the lights that get a map (at most 512, nearest first); for each, its slot (four rows) and its casting edges from the index, written into a reused instance buffer. A light without shadows, or beyond the cap, gets no slot.
3. **Shadow work** (internal, once a frame, before the camera passes: encoded with the frame's batch preparation, not a pass in the public pipeline, which has camera and frame scopes only and runs every camera pass first): clear the atlas to far, then one draw of every casting edge's views, depth-tested, writing each light's four rows. It runs only when at least one light has a slot.
4. **Camera passes**, unchanged in order (`Clear`, `World`, `Lighting`, `UnlitWorld`). The `Lighting` pass's light buffer is cleared and every light visible to the camera draws in one instanced draw: each light's fragment computes its falloff, cone and volumetric term as today, then, if it has a slot, its shadow from the atlas (angle to the view row, distance, blocker search and filter), and adds `light x (1 - strength x shadowed) + coloured part`. Then the compose pass as today.

### Data

- **`ShadowAtlas`** (lighting renderer): the depth texture (`Depth32Float`, `RENDER_ATTACHMENT | TEXTURE_BINDING`), width from the quality, height four rows per slot rounded up as slots grow, recreated when the quality changes or more rows are needed; a comparison sampler and a plain one for the blocker search.
- **Per-light data on the GPU**: each light instance carries its slot (or none), its size in pixels, its shadow colour and strength, and its position (already there).
- **`OccluderIndex`** (engine resource, crate-internal): entity to bounds, a grid of cells to entities, rebuilt incrementally. Map chunk occluders are entities like any other.

### Edges and views

- A view is a 90° square projection around the light (like one face of a cube map): the position along its row is the tangent of the angle within the view, so straight edges stay straight in the row and are never split. The stored value is the distance from the light divided by the light's radius (written by the fragment as its depth, in [0, 1], not the projection's depth), so the four views compare like for like.
- Look-ups near a view's end continue into the neighbouring view's row ("up" wraps to "right"): each tap computes its own view and row and samples at the row's centre, so no tap ever bleeds into another light's rows; hardware filtering is used only along a row.
- Edges are padded by a fraction of a texel at their ends so the joints of a map's chains and touching occluders leave no gap (the light-leak anti-goal), and the stored distance takes a small bias that scales with the texel's world size at that distance (the acne anti-goal); both constants are fixed in the plan and pinned by tests.

### Soft edges

- Blocker search: a few look-ups of the row in an arc set by the light's size, giving the average distance of what blocks the light near this angle (none: fully lit, early out).
- Penumbra: its width at the pixel is `size x (pixel distance - blocker distance) / blocker distance`, converted to an arc of the row, capped so a huge size cannot read a whole view.
- Filter: look-ups across that arc with the comparison sampler (hardware bilinear compare, through `textureSampleCompareLevel`, as the light shader's control flow is not uniform), weighted, giving the shadowed fraction 0 to 1. The blocker search reads raw distances (`textureLoad`). The number of look-ups follows the quality. A size of 0 takes one comparison.
- Sub-texel stability: the angle maps to a continuous row coordinate (no snapping), so an edge moves smoothly as the light moves.

### Errors and limits

- An occluder with non-finite vertices is skipped (as today), as is a light that is not finite or has no positive radius.
- Past 512 shadowed lights a frame: the nearest keep their shadows, a warning is logged once per run with the count.
- The atlas's height cannot exceed the device's maximum texture dimension: the slot cap is the smaller of 512 and what that height allows at the chosen quality.

## Public API and migration

- `Light2D::with_soft_shadows(size_px: f32)`, `set_shadow_softness(size_px)`, `shadow_softness() -> f32` now mean the light source's radius in pixels (0 = hard). Callers using 0-to-1 values (leap-king's 0.4, the shadows demo's pages, the skills) move to pixel sizes. The doc comment says what the size does.
- `ShadowQuality` in the prelude (`Default` is `High`), a field of the player's `Settings` (`ctx.settings()`), saved with the other graphics settings.
- `LightOccluder2D` keeps its constructors and its back-face, chain and winding rules; `transform_vertices` (public, used by no engine code; listed in the `ryo-api` skill, which is updated) goes.
- Removed from the engine's internals: the shadow volumes (`push_shadow_volumes` and its triangles), the light-mask pipeline steps (`LightStep::ResetMask`, `Shadow`, `Masked`, `Coloured`) and `shadow.wgsl`'s volume shader. The `lighting` pass's single instanced draw replaces them.
- Behaviour changes a game may see: shadow strength no longer compounds through several occluders; a light inside a closed occluder is no longer shadowed by it; soft shadows have real penumbrae; shadows of lights no camera sees are no longer computed.

## Acceptance

### Correctness (tests)

Pixel tests in `tests/e2e/lighting.rs` (skipping without a GPU), unit tests beside the code:

- Within the envelope (a light of 256 px radius at zoom 1, `High`): a hard shadow (size 0) behind an occluder is dark and its edge is a step no wider than about two pixels; light on the far side of the edge is unchanged.
- **Contact hardening**: with a light of size 12, the penumbra measured across a shadow edge near the occluder is narrower than the one far from it (widths measured from the pixel profile), and both are wider than the hard light's.
- One light's shadow never darkens another (the existing test, kept).
- **Strength once**: behind two 50% occluders in a row the light is held back by 50%, not 75%.
- **Carried light**: a light inside a closed occluder lights around it, and another light's shadow from that occluder still falls.
- **No leaks**: a wall of touching map chunk chains (and of touching closed rectangles) leaks no light through its joints along its length, at several light positions including ones aligned with a joint.
- **No acne**: open ground beside an occluder, lit, is within a small tolerance of the same ground with no occluder.
- **Stable edges**: moving a light a quarter pixel moves its shadow edge's measured position by under a pixel and never flips a pixel from lit to dark and back over a sequence of small moves.
- Coloured shadows, spot cones (a shadow outside the cone is not computed; an occluder at the cone's edge casts), floors (the existing floor tests, kept), HDR and NaN guards (kept), every camera lit and split screen sharing one atlas (two cameras, one shadow pass), lights turning with the camera (kept).
- **Cap**: 600 shadowed lights in view: 512 keep shadows (the nearest), the rest draw lit, one warning in the test's capture.
- **Index**: moving one occluder updates only its entry; a static scene's index does no work in a frame (a counter in the test build); a removed or disabled occluder stops casting that frame; a map edit's rebuilt chunk chains cast that frame (the existing map tests, kept).
- **Allocation**: `tests/tilemap_alloc.rs`'s lit frames over a static map (with floors) still allocate nothing; a lit frame with 64 shadowed lights over the map allocates nothing after warm-up.
- The shadows demo's and the lighting demo's pages build and step headlessly with no warning in their capture.
- **Quality**: changing `Settings`' shadow quality resizes the atlas the next frame, survives a scene change and is saved.

### Targets (record machine, quiet, `uptime` recorded)

Before figures are taken first, on today's code, by a new `benches/lighting.rs` (and the existing `benches/tilemap.rs` lit cases):

- One shadowed light over the 1024 x 1024 cave (2,378 chains): its frame cost over the unlit map no more than today's (about 30 µs), guarded.
- 64 shadowed lights spread over the cave, the camera seeing a quarter of them: CPU frame cost at least 4 times lower than today's, guarded; GPU cost recorded (headless frame time with the GPU wait).
- 256 shadowed lights in view on a 1920 x 1080 frame at `High`: recorded, with a target of a frame under 16.6 ms on the record machine.
- A lit frame over a static scene: zero index work and zero allocation (tests above).

### Demo

- The `shadows` demo's light pages show hard shadows, soft shadows with visible contact hardening (a size slider), coloured shadows, a carried lantern on a moving character, a page of many shadowed lights over a tile-map maze, and the quality setting. The `lighting` demo's lamp casts. Each page is captured by `./dev.sh demos:tour` and compared by eye.

## Documentation

- `.claude/skills/ryo-lighting/SKILL.md`: shadows section rewritten (sizes in pixels, contact hardening, the carried-light rule and its map limit, strength once, the quality setting and its envelope, the cap, the cost model); `ryo-config-save` (the quality setting) and `ryo-api` (the API changes); the "stays on after a scene change" claim corrected to what the engine does (lighting resets with `SceneCleanup::LIGHTING`); one minor version bump.
- `src/core/lighting/AGENTS.md`: Architecture and Limits rewritten for the atlas, the index and the single light draw.
- `docs/architecture/overview.md` lighting lines; `docs/backlog/follow-ups.md`: remove the items this fixes (true penumbrae, the doubled shadow's darkening, the lighting-stays-on doc bug, the unchecked leap-king shadows once checked).

## Self-shadowing occluders (added during hand-play)

- **The problem:** on the demo's maze page the walls were lit on top. A wall's shadow starts at its far face under the back-face rule, so a torch's light lands on the wall itself and seems to reach outside the corridors.
- **The decision** (the owner's, in hand-play): an option, as Unity's ShadowCaster2D "Self Shadows" is. `LightOccluder2D::with_self_shadow(true)` (and `set_self_shadow`, `self_shadow()`; off by default, which is the old behaviour). Tile maps take it from the reserved layer property `self_shadow` (Tiled; `LayerInfo::self_shadow` in code), carried on the layer's chains; layers occluding together shadow themselves if any of them does.
- **A self-shadowing occluder casts every edge**, toward the light and away from it. The atlas keeps the nearest depth, so the near face governs and the occluder's own area is in its shadow. The first cut cast the faces toward the light alone, and the review found two leaks: a light on (or just inside) a self-shadowing wall's face saw no face toward it but the one it stood on, which covers no angle, so it shone through the whole wall; and a light carried in one of two touching self-shadowing blocks shone through the other, whose only face toward it was the side they share, cancelled as shared. Casting every edge closes the first (the far sides still block), and a run holding the light sharing no side closes the second (the touching block casts the shared side, so its own area is dark). A light inside a self-shadowing chain region is boxed in by it, as plain chain walls box it in.
- **Why the planner, not the shader:** the atlas holds the nearest casting edge, so which edges cast is all that changes; the light shader, the bias and the soft filter read the near face as they read a far one, and the bias (1.5 texels) keeps the pixels before it lit, at the price of a lit strip that wide along a wall's top, visible at `Low`. A soft light's penumbra starts at the near face, so a wall's top darkens over a soft edge: accepted.
- **Kept rules:** a closed occluder holding the light casts nothing for it (carried lights); sides two closed occluders share exactly cast for neither when both shadow themselves or neither does and neither holds the light (where only one shadows itself, each casts the side by its own rule, so no light passes through the pair); floors, the spot span, degenerate edges and zero scale as before.

## Risks

- **Precision and aliasing** on large lights at `Low` quality: a 2,000-pixel light's view spreads 256 texels over a long arc. Mitigation: the soft filter and the bias scale with texel size; tests at each quality for a large light.
- **The web**: depth textures sampled with a comparison sampler and fragment-written depth are core WebGPU, but browser drivers vary. Mitigation: the lighting demo in Chrome, Safari and Firefox by hand before merge.
- **Many small lights with huge occluder sets near them** (a torch in every cell of a maze): the edge count per light is bounded by what its radius meets, but a dense map near many lights is the worst case; the 64-light bench is built on exactly that.
- **Behaviour change in games** (strength once, carried lights, size units): small, and the demos and games are updated with it.
