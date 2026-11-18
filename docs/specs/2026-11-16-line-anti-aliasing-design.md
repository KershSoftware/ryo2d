# Line anti-aliasing: design

The last item of the engine audit's list. Lines draw the same on every GPU and in every mesh layout, by computing each pixel's coverage from its distance to the line's edges in pixels, not from screen-space derivatives.

## Why

Every line in the engine (UI lines, `Line2D`, trails, debug shapes, the skeleton overlay) is tessellated by one builder (`render/batch/polyline.rs`) and shaded by one body (`styled` in `shaders/include/line_style.wgsl`). Its only anti-aliasing is the Solid style's `clamp((1 - edge) / fwidth(edge) + 0.5, 0, 1)`, where `edge` is the interpolated distance from the centre line in the line's own units (0 at the centre, 1 at an edge).

That has three faults:

- **It depends on the GPU and the mesh.** `fwidth` is a finite difference over a 2x2 pixel quad. Its value near a triangle's edge depends on how the GPU fills the quad's helper lanes, so the same triangles drawn as an indexed mesh with shared vertices and drawn one by one give different pixels on Metal. Up to 3/255 along slanted edges, up to about 0.36 coverage at miter tips and closed-outline corners. The text demo's page 5 shows 34 such pixels. The owner accepted this in the UI renderer work (2026-10-20) on condition that this project follows.
- **It is wrong at joins.** In a miter wedge the edge distance is interpolated from 0.5 at the corner to the tip, which is `miter × half width` away, not `half width`; so `edge` is not a distance there and its derivative changes from triangle to triangle.
- **It only fades inward.** The geometry stops at the line's edge, so a pixel whose centre lies just outside is never shaded. A line's outer edge is cut, not smoothed, and a line narrower than a pixel cannot get lighter as it narrows: it flickers between drawn and not drawn as it moves.

UI rects (borders, focus rings, outlines) are separate: an analytic rounded-box SDF whose edge width is `length(fwidth(p)) × 0.7071`. They are drawn as one quad per rect, so they do not show the mesh fault, but they share the GPU dependence. Text (MSDF) uses derivatives correctly and is out of scope.

## How others do it (the review)

From their public documentation and descriptions:

- **Dear ImGui and NanoVG:** a fringe of extra geometry about one pixel wide around every stroke and fill, whose vertices carry alpha 1 on the inside and 0 on the outside; the GPU interpolates the alpha. No derivatives. The ramp is one pixel wide whatever the angle, and coverage is approximate: a line narrower than the fringe fades by geometry, not by area.
- **Skia:** analytic coverage. For strokes on the GPU it computes, per pixel, the distance to the stroke's edges and turns it into the area of the pixel inside them, with geometry extended to cover the partially covered pixels.
- **Unity UI Toolkit:** tessellates vector strokes with an anti-aliasing fringe and carries the distance to the edge as a vertex attribute the fragment turns into coverage.
- **MSAA:** multisampling the targets. Rejected: the scene is `Rgba16Float`, MSAA was put out of scope in the HDR renderer design, it multiplies memory and bandwidth, and it still gives only 4 or 8 coverage levels.
- **Per-fragment segment distance** (each segment a quad whose fragments measure the distance to the segment itself): exact everywhere, but neighbouring segments overlap at every join, so a translucent line darkens at its corners.

## The approach

**Analytic box coverage from a distance in pixels, carried as a vertex attribute, with a fringe.** It is the Unity UI Toolkit and Skia approach, in the builder the engine already has.

- **Distance attribute.** Every vertex carries `d`, its signed distance in the line's own units from the centre line of the segment its triangle belongs to, and `along`, its distance from the nearest cap edge (large away from caps). Both are exact affine functions over each triangle: the builder makes every triangle belong to one straight piece, whose centre line a distance is measured from. This holds for segment quads; for a miter join each wedge measures from the centre line of its own segment (the tip lies on both edges, so `d = half width` there); for a bevel the wedge measures from the bevel's own edge line; round joins and caps measure the distance from the corner point, which over a thin arc triangle is affine to within a small fraction of a pixel (the arc is stepped finely enough for that at the line's drawn size).
- **Fringe.** The builder marks the outer vertices of each edge and gives them their outward direction. The vertex shader pushes each marked vertex outward by one pixel in the target (half a pixel of coverage reach plus slack), using the existing `across` attribute and the camera's pixels per unit, and extends `d` and `along` by the same amount. So the fringe is one pixel at any zoom and the builder does not need to know the camera.
- **Coverage.** The fragment converts `d`, `along` and the half width to pixels with the camera's pixels per unit, a uniform (2D cameras scale uniformly), and computes the exact area of a one-pixel box filter inside the line across, and the same along: `cover(d, hw) = clamp(min(d + 0.5, hw) − max(d − 0.5, −hw), 0, 1)`. Coverage is the product. No derivatives.
- **Thin lines.** The box formula makes a line narrower than a pixel draw at a coverage equal to its width, so it fades smoothly as it thins and moves without flicker. Debug lines of one screen pixel at odd widths keep their snap to pixel centres and draw solid.
- **Joins and caps.** The builder's rule that every corner is one wedge fanned from the corner point stays, so nothing is covered twice and a translucent corner is no darker than its segments. The inner side of a sharp join keeps its present overlap.
- **Styles.** Solid takes the coverage. Soft and Glow keep their shaping across the width and are multiplied by the coverage, so their edges are smooth too.
- **Effects.** Lightning, Noise and Ribbon move vertices along `across`; the fringe moves with them.

## Pixel art

On the pixel-art target a smoothed edge is magnified by the upscale into a blurred band. Lines get an **edge** setting: `Smooth` (the box coverage) or `Hard` (coverage rounded at a half: a pixel is drawn wholly when its centre is inside). The default follows the target: `Hard` on a pixel-art target, `Smooth` elsewhere. `Line2D`, `Trail2D`, debug shapes and UI lines can set it explicitly. UI on the pixel-art target follows the same rule; UI at window resolution is smooth.

## UI rects

The rounded-box SDF keeps its shape, and its edge width comes from the UI pass's pixels per unit (a uniform, or per instance where a transform scales it) instead of `fwidth`. Same coverage function: the box filter across the edge. So borders and focus rings are derivative-free too. The rect quad's existing one-pixel reach stays.

## What changes for a game

Lines look better: edges smooth on both sides, thin lines fade instead of flickering, corners clean. They look the same on every GPU. Pixel-art games' lines become crisp by default. No API is removed; `edge` is new.

## Limits

- A round join or cap's coverage is exact to within a small fraction of a pixel (the arc is stepped).
- The inner side of a sharp join still overlaps (as today).
- **The inner corner.** Where two segments share their inner vertex, a pixel within a pixel of it (or near it within a pixel of both inner edges) lies across both strips, and one triangle's distances cannot give their union: its coverage is off from the exact area by up to 0.16 at a 60 degree turn, 0.26 at 90, 0.34 at 120, 0.39 at 150 and 0.45 at 165, whatever the join (measured on the builder's pushed mesh, `the_pushed_mesh_gives_every_pixel_its_coverage`). The owner accepted this (2026-11-16); fixing it would take about four more vertices a corner. Everywhere else the pushed mesh is within 0.12 (worst 0.105, at round joins).
- **A split corner** (a corner whose inner side cannot share, as its segments are short): near its corner point, where the segments' ends and the fan meet and the bevel or arc cuts within a pixel past them, a pixel lies against edges its triangle does not measure. It is off by up to 0.09 at a 60 degree turn, 0.15 at 90, 0.22 at 120, 0.32 at 150 and 0.37 at 165 (0.13 at 90 and 0.21 at 120 while its segments are at least `miter × hw` long; measured with segments of 0.5, 1.0 and 1.9 times that; 0.85 to 0.98 before its ends pushed straight out with fringe triangles to the fan). The owner accepted this (2026-11-16); `the_pushed_mesh_gives_every_pixel_its_coverage` holds the figures.
- Coverage across times coverage along is exact away from caps and slightly light right at a butt cap's corner.
- Text and textured sprites are untouched.

## Settled at the builder check

These replace anything above that says otherwise.

- **The coverage model.** A pixel's coverage across an edge is the 1D box filter of its signed distance from that edge, perpendicular to it: `cover(e) = clamp(e + 0.5, 0, 1)` for an edge `e` pixels away (positive inside), and for a strip of half width `hw` about a centre line, `clamp(min(d + 0.5, hw) − max(d − 0.5, −hw), 0, 1)`. This is the model GPU analytic anti-aliasing uses; it equals a square pixel's area for edges along the pixel axes and is within about 10/255 of it at 45°, which reads as smooth. **Corners take two edges:** a triangle that lies against two edges (a miter wedge near its tip, a bevel corner, a butt or square cap's corners, a closed outline's corner) carries the distance to both, and its coverage combines the two covers, so a convex tip is not drawn as if its edge ran on. They blend by the angle between the edges (`k`, the dot product of their outward normals, stored per triangle): the product moves toward the lesser cover by `k²` where the edges are less than a right angle apart, and toward `max(c1 + c2 - 1, 0)` by `k²` where they are more (settled at the builder's review: the product alone was 0.28 off at shallow convex corners, the blend 0.08). Every distance is an affine function over its triangle, so it is exact wherever it is measured.
- **Round joins and caps** carry the 2D offset from their corner point (affine over a triangle), and the fragment takes its length: the radius is exact, with no dependence on the step count.
- **The attributes.** Each vertex carries: its distance across (from the centre line of its triangle's piece, signed), the half width there, a second edge distance (the other edge at a corner, the cap edge near a cap, or "none"), the round offset where it applies, its outward push direction in the line's units (the unit direction a one-pixel push moves it, scaled so the attributes grow by exactly one pixel: at a miter tip that is the miter length), and the most it may be pushed in the line's units (unlimited, except at a shared inner join vertex, where it is limited so a pushed vertex never passes the shorter segment's middle and the quad never folds). The vertex shader pushes each vertex by one pixel times its direction, clamped to its limit, and grows its distances by the push. `along` is replaced by the second edge distance; a segment with caps at both ends measures each cap's edge in the triangles beside it.
- **Joins in the builder.** A miter join keeps its shared vertices (the tip lies on both outer edges). A bevel gives its wedge its own vertices, pushed along the bevel's normal, with a small fringe triangle at each elbow so the mesh stays watertight. A round cap has its own copies of the strip's end vertices, at the same positions with the same push. The inner side of a sharp join keeps its present split when it does not share.
- **Pixels per unit** reach the line shader in the camera's uniform (`CameraGlobals`' padding): the render image's pixels per world unit, `scale × image size ÷ view size`, one figure (2D cameras scale uniformly; a fraction of a pixel's difference between x and y at a Fit viewport's rounding is ignored). The UI uses `UiSpace::px_scale`. Debug shapes use the same figure; their odd-width snap stays.
- **Hard edges (pixel art)** use the line's major axis, so a one-pixel line is one pixel per column (or row) like a drawn pixel line: with `n` the line's normal in the target, the distance is divided by `max(|n.x|, |n.y|)` and the half width is not (dividing both would cancel, and a one-pixel line at 45° would be 1.41 pixels tall in a column), so the band is `2 hw` tall along the axis; the half width is at least half a pixel, and a pixel is drawn when its centre lies in the half-open band `[−hw, hw)`. A line narrower than a pixel draws one pixel wide. UI rects on the pixel-art target are already snapped to whole pixels and keep their smooth rounded corners.
- **Styles.** Soft and Glow shape by `d ÷ hw` (the true distance), not the interpolated `uv.y`, and are multiplied by the coverage.
- **Textured trails** extrapolate `uv.y` onto the fringe, so the texture is not squeezed. Their mip level still comes from the uv's screen derivatives, which is a legitimate use; the no-derivative rule is about coverage.
- **Culling** grows a line's box by one pixel per camera for the fringe, and by the miter tip's reach (`MITER_LIMIT × hw`), which the box misses today (a pre-existing fault, fixed here). The UI's clip collapse and build cull grow by the push likewise.
- **UI rects:** with no transform on UI commands, `fwidth(rect_p)` is exactly `1 ÷ px_scale`, so the derivative-free edge is the same pixels as today.
- **Alpha:** coverage multiplies straight alpha before premultiplying, as now.

## Acceptance

- **Same on every mesh layout:** the batch drawn indexed, the batch drawn with every vertex its own, and the UI pipeline give equal pixels at 16 sub-pixel shifts, with no exception at miter tips (`lines_draw_as_the_batch_draws_them_with_no_vertex_shared` and an indexed twin).
- **Coverage as modelled:** straight lines of widths 0.25, 0.5, 1, 1.5, 2, 3 and 6 pixels, at angles every 7.5° and 16 sub-pixel offsets, give each pixel the model's coverage to within 2/255 (alpha read from a UI-format image, crate-internal, as the UI proofs read it).
- **Close to the truth:** every pixel of those lines, of miter, bevel and round joins, of every cap, and of a closed outline's corners is within 0.12 coverage of a 16x16-supersampled square-pixel reference computed on the CPU (today's corners are up to 0.36 off).
- **No coverage derivative:** a test reads `line.wgsl`, `line_style.wgsl`, `ui_rect.wgsl` and the line and rect paths of `ui.wgsl` and finds no `fwidth`, and no `dpdx`/`dpdy` but the ones that pick a texture's mip level.
- **Thin lines:** a 0.5-pixel line draws at half coverage; moved by sixteenth-pixel steps, its summed coverage across its width varies by under 2%.
- **Hard edges:** on a pixel-art target a one-pixel line at any angle has exactly one drawn pixel per column (or row along its major axis), and only fully on or fully off pixels; `Smooth` gives partial ones.
- **Translucent corners:** no darker than their segments (the existing test), for miter, bevel and round joins.
- **Culling:** a miter tip reaching into view from a line just off it is drawn.
- **UI rects:** the same pixels as before (the rect tests unchanged).
- **Speed:** a new line bench (a thousand lines and trails of mixed joins, extract and draw) and the UI bench within 5% of main; no allocation added.
- **Visual review:** before-and-after captures of the text demo's page 5, the trails demo's pages, the UI demo's checkbox and dropdown pages, a debug overlay and a pixel-art page, at 1x and 2x, side by side, for the owner's hand-play. The tour changes only on pages with lines, each looked at.
- **Docs:** the render, trail, shaders and UI notes, `ryo-renderer`, the line and trail sections of the skills, `ryo-ui`'s lines, and the backlog notes that pointed here.

## Building on the GPU (owner, 2026-11-16)

Measured after packing (48-byte vertex, a per-line table): the upload is 4% under main, but building lines on the CPU costs +73% and a lines-only frame +40%, over the owner's 15%. The cost is the builder's per-vertex work (about 31% more vertices, each with its edge distances and pushes worked out). The owner chose to move that work to the GPU, the direction modern renderers take for strokes.

- **What the CPU sends.** Per line, its entry in the line table (style, effect, atlas slot, emission, join, cap, edge, miter limit) and its points: position, half width, colour (sRGB bytes) and the distance along, one record a point, in a storage buffer. Lines are appended per frame as today, by copying points; nothing is tessellated on the CPU.
- **Pieces.** A line is drawn as instances of fixed vertex templates, one instance a piece: a segment (its quad and fringe), a join (miter, bevel or round), a cap (butt, square or round). The CPU writes one small piece record per piece (the piece kind, the line, the point it starts at), which tells the vertex shader where to read; the template's vertex count is fixed per kind (a round piece uses enough steps for a fraction of a pixel at the largest width the engine allows, unused steps collapse to nothing, or its radius is exact by the offset's length anyway, so a fixed small step count serves).
- **The vertex shader** reads the piece's points (and, at a join, the neighbouring segments), and computes what Task 2's builder computes today: the corner position, the distances across and to a second edge, the round offset, the push direction and limit, and the blend factor k; then pushes the fringe by a pixel, as already designed. The rules are those of the CPU builder, which stays as the reference the GPU's output is tested against.
- **Watertightness.** Neighbouring pieces compute their shared corners from the same inputs with the same arithmetic in the same function, so they agree bit for bit; a test draws translucent lines over the CPU builder's coverage and finds no pixel covered twice or missed.
- **The UI** draws its lines through the same WGSL functions (an include both the batch and the UI pipeline use), from the same piece records, so world and UI lines stay equal.
- **Debug shapes** keep their per-camera snapping by adjusting their points before they are sent.
- **Culling and effects** work as already designed: the box per line on the CPU from its points; displacement in the vertex shader.
- **The web** already needs WebGPU (storage buffers in the vertex stage).
- **Speed target:** the owner's ~15% on the lines-only bench, expected to land at or under main, since the CPU only copies points.

### Building on the GPU: settled at its builder check

- **Drawing by triangle codes.** The CPU writes one 32-bit code per triangle (the piece and the triangle within it) into a storage buffer; a line draw is a plain `draw` over a range of `3 × triangles` vertices, and the vertex shader finds its code by `vertex_index ÷ 3`. Counts are exact, piece kinds mix freely in one draw, and painter's order holds: draw items and their merging work as index ranges do today. Lines need no vertex or index buffer.
- **Templates:** a segment 4 triangles (its quad fanned from the corner point at an end whose inner side splits or that a round cap ends, the two further triangles of no area where neither end does; its vertices measure a cap edge or the two-point strip when capped); a miter or bevel join 4 (the same count, so a miter falling back to a bevel needs no other template); a round join 18 triangles: two inner wedges and an arc of 16, 14 whole steps of angle ÷ 15 and a half step at each end; a square cap 2; a round cap 16; a butt cap none. Round arcs are **circumscribed** (the in-between vertices at the half-step angles, at `hw ÷ cos(step ÷ 2)`, pushed by `1 ÷ cos(step ÷ 2)` pixels), so the polygon contains the disc at any size; the radius is exact by the offset's length.
- **Decided on the GPU, per corner,** by one corner function: miter or bevel (the miter limit), the inner side shared or split, straight-on corners (the join draws nothing), U-turns. A segment reads points `i − 2` to `i + 3`, a join `i − 2` to `i + 2`; neighbours wrap on closed lines. The CPU's piece list depends only on where a point sits in its line, never on geometry. The CPU still filters points as it copies them (non-finite points, points within 1e-4 of the last, a closed line's repeated end, lines under two points or of no width).
- **Watertight by construction:** the vertex position is `@invariant`; every piece gets a shared corner's position from the one corner function with the same values loaded from the same records, in the same order, with no rearranged algebra; the WGSL never makes or tests infinity or NaN (explicit degenerate flags instead). A split inner corner gives the segment's end a vertex at the corner point, removing a T-junction the CPU mesh has. Tested by drawing with additive counting at many sub-pixel offsets and finding no pixel drawn twice and none missed.
- **f32 throughout:** the GPU computes in f32, so the packing roundings (halved limits, the 16-bit angle, k in 127ths) go, from the reference too.
- **UI lines** use the same piece functions (an include that takes points as arguments), with their points carried inline in the UI instance (one instance per two triangles), so retention (kept fragments, in-place uploads, the bitwise compare) works unchanged. World lines load points from the storage buffer.
- **The CPU builder becomes the reference,** restructured into the same self-contained pieces in the same triangle order, in f32: the GPU's vertex outputs are compared with it through a test-only compute entry that calls the same piece-vertex function and writes the results. After the GPU path lands the reference is test-only.
- **Speed** is measured on the CPU (the line bench) and on the GPU (timestamp queries, native only, in the line bench): both within the owner's ~15% of main on the lines-only bench.

## A compute pre-pass (owner, 2026-11-17)

Measured with the vertex shader building every corner: CPU 65–88% under main, GPU 2.2–3.6 times main (each vertex evaluates up to three corners, and a non-indexed draw shares no vertex). The owner chose a compute pre-pass to bring GPU time down.

- **Corners once.** Before the world passes, a compute pass runs one thread per point of every line drawn this frame. It evaluates that point's corner with the one corner function (miter or bevel, shared or split, straight-on, U-turn, the pushes, limits and k) and writes a corner record to a storage buffer.
- **Vertices once.** A second dispatch (or the same, by a thread per piece) builds each piece's vertices from the corner records, all attributes as the vertex shader builds them today, into a vertex storage buffer laid out per triangle code; the draw then reads its vertex by `vertex_index` with no further corner work (the vertex shader keeps only the per-camera push, effects and the fringe, which depend on the camera).
- **Bit for bit.** The corner records are computed once and read by every piece that shares the corner, so watertightness no longer rests on identical recomputation; `@invariant` stays for the pushed position.
- **The UI** keeps building in its vertex shader (its lines are few and its instances carry their points for retention), unless the measurement shows the UI benefits too.
- **Testing.** The equality test compares the pre-pass's vertex records with the CPU reference; the count draw and every coverage test stay.
- **Speed target:** GPU time near main's or under, with the CPU savings kept.

### The compute pre-pass: settled at its builder check

Per-vertex records were found to move the cost into memory (10k mitered lines ≈ 132 MiB at 96 bytes a record, over the storage cap, with read/write traffic comparable to today's time), so the build is:

1. **Measure first:** split the line bench's GPU time into vertex and fragment work (an empty-fragment or 1×1 run).
2. **Indexed vertex codes:** the CPU writes an index buffer whose values are vertex codes (point, kind, template slot); the vertex shader decodes `vertex_index` as it decodes triangle codes now, and the GPU's post-transform reuse shares a piece's vertices between its triangles (about 2–2.4× fewer vertex invocations). Each kind has fixed slots whose role follows the GPU's decision (miter or bevel, shared or split) and is used alike by every triangle; line pipelines draw without face culling.
3. **A corner pre-pass:** one compute dispatch, one thread per point, writes a compact corner record (the point's joint) and a limits array; a vertex reads its own record and two neighbouring limits instead of evaluating three joints. Its own dispatch (not merged with vertex building), a 2D grid or per-thread loop past 65,535 workgroups, in the frame's encoder before the first camera pass and after the upload, skipped with no lines; `StreamCap` takes the record's size into account. Shared vertices assembled by different pieces stay bit-identical by one shader's same arithmetic, `@invariant` and the count draw.
4. **Round arcs of 8 triangles** (circumscribed, radius exact by the offset's length; about 2% more fill).
5. **Lines no camera sees are culled before they are copied** (against every camera's view and layers), saving CPU and GPU.
6. Optionally, flat params and bounds read by the fragment from the line table.

- **The UI** keeps building in its vertex shader through the same function, taking its joints computed in place; if a different compile context moves the UI-equals-world comparison by a level, the test allows 1/255 and the limit is recorded.
- **The bench** stamps time before the pre-pass, so it is counted.
- **Target:** GPU time ≤ 1.15× main on the lines-only scenes, CPU gains kept, 100k lines still drawable.

### The compute pre-pass: as built

- **Slots.** A segment has 6 slots, a miter join 9, a bevel join 6 of the miter's (each place it repeats named by one slot; a miter cannot share, as its slots hold nine places when its inner side is shared), a round join 15, a square cap 4 and a round cap 10 (`piece_template`; `line_piece_slot` for the UI's unindexed instances). Each triangle keeps the reference's first vertex, which flat attributes come from; a right turn's other two are wound the other way, which no culled face shows, and the reference comparison takes them as a pair.
- **The corner record** is 64 bytes a point (`LineCorner`), with its limits again in an array of two floats a point; the stream's points are capped by the record (2,097,152 under the default limits). The corner pass runs after the shadow atlas in the frame's encoder.
- **The UI** works its corners out in place; it draws the world's pixels exactly, so no 1/255 allowance was needed.
- **Fragments** read what a line shares from the line table (a textured line's slot, a two-point line's half length), the style riding in the vertex's flags: measured, the table read cost 18% of the vertex stage's outputs on large scenes.
- **Round pieces' truth.** With arcs of 8 the drawn polygon is no longer close enough to the disc to stand for the line's exact area, so the coverage tests measure round pieces against true discs (`Triangles::true_shape`): within 0.108 (0.105 at 16 steps).
- **Small streams skip the corner pass.** Under 16,384 points the vertices work their corners out in place, as the UI's do, building the same vertices bit for bit. At 1,000 lines this saves the dispatch and its wait (the pass's own 5 to 7 µs); the A/B against the pass was within the bench's noise both ways (3% to 11% faster in place at 1,000 lines, up to 8% slower at 10,000 and over).
- **The target** (GPU ≤ 1.15 × main), at the last round's final run: met on 10,000 mitered lines (0.63), 10,000 round lines (0.60) and 1,000 round lines (1.10); missed on 1,000 mitered lines (1.34) and the mixed thousand (1.27). Main's own 10,000-line figures were well above the sitting before's under the same measure; against the sitting before's main they would be about 1.0 (mitered) and 0.71 (round), within the bound either way. Tried and dropped: sorting a line's pieces by kind, and loading only the window points a vertex reads; not tried: a per-point vertex record (the small scenes no longer run the pass). The figures are in `docs/backlog/lines.md`.
