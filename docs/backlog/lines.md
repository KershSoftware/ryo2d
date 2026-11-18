# Lines: speed

## Measured

What is measured: a thousand lines and trails drawn together, by `cargo bench --bench lines` (release), on the code before the line anti-aliasing work (`docs/specs/2026-11-16-line-anti-aliasing-design.md`, Acceptance, Speed). The work is held to within 5% of these figures, and allocates nothing more in a steady frame.

Machine: Apple M5 Pro, 18 cores, shared with other work.

The scene: 1920 x 1080 headless (a GPU), unlit, camera over the view. 500 `Line2D`s and 500 `Trail2D`s in a grid of 40 x 25 cells of 48 x 43.2 px, each of five to seven points that turn at every point, with joins (miter, bevel, round), caps (butt, round, square) and widths (0.5, 1, 2, 3, 6, 10 px) mixed, every fourth translucent. None moves. The bench warms 60 frames, then measures 300; each figure is the median of the frames of one run, and the figure recorded is the median of three runs.

What each figure covers:

- `lines_render`: the render step's CPU time as a whole (`HeadlessEngine::render_time`): the lines and trails built into triangles, then the draw (placing, uploading, encoding and submitting).
- `lines_prepare`: `Batch2D::prepare` within it (`HeadlessEngine::batch_prepare_time`).
- `lines_extract`: the render step less prepare. This is where the lines and trails are built and the frame recorded and submitted; the entity drawer's own timer (`entity_draw_time`) does not cover the lines, which are built after it.

### The record

One-minute load before each run: 10.08 for all three (the clock had not moved between them). That is above 3 and so flagged: the machine was shared, and nothing waited for a quiet one. The spread of the runs is under 1%.

Medians in microseconds:

| Bench | Run 1 | Run 2 | Run 3 | Median of runs |
|-------|-------|-------|-------|----------------|
| `lines_render` | 716.0 | 712.6 | 718.9 | 716.0 |
| `lines_prepare` | 317.0 | 314.0 | 317.8 | 317.0 |
| `lines_extract` | 396.0 | 397.6 | 399.0 | 397.6 |

The guards are loose, for regressions of another order: `lines_extract` under 8,000 µs and `lines_render` under 16,000 µs.

## Against main, with lines built on the GPU (2026-11-17)

The finished work (lines built on the GPU from their points and covered analytically, `docs/specs/2026-11-16-line-anti-aliasing-design.md`) measured against main (076356c5) in one sitting: main exported with `git archive` and built with its own target directory, three runs of each, alternating, release, on the same machine (Apple M5 Pro, Metal). The owner's bound: within about 15% on the lines-only bench (CPU and GPU), prepare and upload within 5%.

**How main was measured.** Main has no `benches/lines.rs`, so this branch's bench was copied into it, with two changes: the headless device asks for `TIMESTAMP_QUERY` where the adapter has it (as this branch's does, `testing/gpu.rs`), and the lines-only scenes stop at 10,000 lines, as main cannot draw 100,000 (its vertex buffer would be 288 MiB, past the device's 256 MiB, and the run fails validation).

**The bench** (`benches/lines.rs`): the mixed thousand of before (500 lines and 500 trails; the width now goes by the shape's index over three, so every join meets every width), and lines-only scenes of 1,000, 10,000 and 100,000 `Line2D`s, mitered or round, each a scaled copy of the thousand's shape in a grid that fills the 1920 x 1080 view, its width scaled with it but never under a pixel. `gpu` is the GPU's time from just before the world pass to just after the unlit world pass, from timestamps written by two empty render passes round them (an empty compute pass gets no timestamps on Metal); it includes the world passes' own load and store of the image. Every frame must have drawn (a draw call and a prepare time above nothing); a frame the GPU gives no time for (both stamps zero, now and then on Metal, on main more often) is left out of the GPU's median. The line copy runs under the `cpu:render:lines` profile scope.

Loads before the runs: 14.0, 13.0, 9.4, 8.5, 6.7 and 6.2 (flagged: above 3; the machine was shared). Medians in microseconds:

| Figure | Main runs | Main | This branch's runs | This branch | Change |
|--------|-----------|------|--------------------|-------------|--------|
| `lines_render` | 803.5, 794.8, 827.4 | 803.5 | 258.4, 248.1, 226.2 | 248.1 | -69% |
| `lines_prepare` | 357.5, 337.8, 365.7 | 357.5 | 69.3, 67.1, 62.3 | 67.1 | -81% |
| `lines_extract` | 441.8, 453.0, 462.7 | 453.0 | 187.1, 179.5, 163.5 | 179.5 | -60% |
| `lines_gpu` | 41.9, 41.2, 34.2 | 41.2 | 133.6, 134.0, 132.7 | 133.6 | +224% |
| `lines_only_miter_1000_render` | 591.7, 596.1, 628.2 | 596.1 | 228.2, 206.5, 197.5 | 206.5 | -65% |
| `lines_only_miter_1000_prepare` | 254.6, 258.4, 268.4 | 258.4 | 59.6, 55.0, 51.6 | 55.0 | -79% |
| `lines_only_miter_1000_extract` | 336.0, 335.8, 356.1 | 336.0 | 167.3, 151.5, 145.9 | 151.5 | -55% |
| `lines_only_miter_1000_gpu` | 33.0, 30.9, 30.3 | 30.9 | 98.9, 99.2, 99.1 | 99.1 | +221% |
| `lines_only_miter_10000_render` | 5187.2, 5227.1, 5261.5 | 5227.1 | 1201.8, 1155.8, 1075.2 | 1155.8 | -78% |
| `lines_only_miter_10000_prepare` | 2547.7, 2583.7, 2591.9 | 2583.7 | 460.4, 447.1, 424.0 | 447.1 | -83% |
| `lines_only_miter_10000_extract` | 2650.7, 2640.0, 2668.4 | 2650.7 | 735.5, 710.2, 645.9 | 710.2 | -73% |
| `lines_only_miter_10000_gpu` | 391.4, 426.9, 389.4 | 391.4 | 858.7, 858.1, 855.1 | 858.1 | +119% |
| `lines_only_round_1000_render` | 1207.7, 1083.5, 1100.9 | 1100.9 | 218.2, 246.2, 221.6 | 221.6 | -80% |
| `lines_only_round_1000_prepare` | 579.9, 523.7, 529.7 | 529.7 | 74.1, 82.8, 76.0 | 76.0 | -86% |
| `lines_only_round_1000_extract` | 629.2, 557.2, 572.4 | 572.4 | 144.2, 161.5, 146.4 | 146.4 | -74% |
| `lines_only_round_1000_gpu` | 53.5, 53.2, 48.5 | 53.2 | 190.4, 190.5, 189.6 | 190.4 | +258% |
| `lines_only_round_10000_render` | 10550.9, 10070.5, 10090.4 | 10090.4 | 1209.2, 1406.0, 1247.9 | 1247.9 | -88% |
| `lines_only_round_10000_prepare` | 5645.5, 5306.0, 5315.1 | 5315.1 | 568.1, 637.7, 578.9 | 578.9 | -89% |
| `lines_only_round_10000_extract` | 4880.6, 4762.3, 4769.7 | 4769.7 | 640.0, 763.5, 662.3 | 662.3 | -86% |
| `lines_only_round_10000_gpu` | 790.2, 807.8, 885.5 | 807.8 | 1752.1, 1760.2, 1750.6 | 1752.1 | +117% |
| `lines_only_miter_100000_render` | - | - | 9581.8, 9977.0, 9586.1 | 9586.1 | main cannot draw it |
| `lines_only_miter_100000_prepare` | - | - | 4074.0, 4358.5, 4071.8 | 4074.0 | main cannot draw it |
| `lines_only_miter_100000_extract` | - | - | 5514.9, 5614.7, 5499.0 | 5514.9 | main cannot draw it |
| `lines_only_miter_100000_gpu` | - | - | 9349.8, 9524.1, 9370.7 | 9370.7 | main cannot draw it |
| `lines_only_round_100000_render` | - | - | 11357.3, 11464.1, 11922.9 | 11464.1 | main cannot draw it |
| `lines_only_round_100000_prepare` | - | - | 5834.8, 5850.8, 6238.7 | 5850.8 | main cannot draw it |
| `lines_only_round_100000_extract` | - | - | 5533.2, 5588.8, 5675.1 | 5588.8 | main cannot draw it |
| `lines_only_round_100000_gpu` | - | - | 18784.3, 19205.4, 19351.5 | 19205.4 | main cannot draw it |

**CPU: well inside the bound.** The render step is 60% to 88% under main, prepare (the upload) 79% to 89% under: the CPU copies points, not vertices.

**GPU: over the bound.** The world passes take 2.2 to 3.6 times main's time: +224% on the mixed thousand, +221% and +119% on 1,000 and 10,000 mitered lines, +258% and +117% round. Every vertex is built from its points (five point loads, the corner function at its point and its two neighbours, the place, the push and the distances), and nothing is shared between triangles, where main's indexed mesh shares its vertices; the fragment covers a wider fringe with more arithmetic. A frame of 10,000 lines costs 0.86 ms of GPU (round 1.75 ms) on this machine; 100,000 cost 9.4 and 19.2 ms.

**The cheap vertex wins**, each measured against the branch before them (6db9caa0) in alternating runs:

| Change | GPU time against before |
|--------|-------------------------|
| A round join's `acos` only for round joins, the window's points and the three corners indexed by constants (no copy of an array to index it by a variable) | -45% to -50% on every scene (mixed 244 to 133 µs; 10,000 mitered 1,715 to 856; 10,000 round 3,274 to 1,756) |
| The same, with each neighbouring corner asked only where a quad's limit is read (not taken) | +6% to +13%: the calls sit in branches the vertices of a group take apart, so a group runs several |

The first is in; the second was measured and left out.

**Next: a compute pre-pass (owner, 2026-11-17).** The GPU figures above are over the bound and stand as they are. A compute pass will build each corner once per frame on the GPU, and the draw read what it built, to bring the GPU's time down; it is a task of its own (below).

**The UI bench** (`benches/ui_render.rs`, the guard, 2,000 interleaved frames, three alternating runs each, loads 6.1 down to 4.9): unchanged within the machine's noise. One label changing over an empty frame: main +31.6, +29.0, +33.3 µs, this branch +29.7, +29.4, +31.3; scrolling: main +42.3, +37.3, +44.2, this branch +38.2, +37.9, +38.7; idle under 1.3 µs on both; the UI pass stepped alone: idle 16.4, 16.4, 16.1 against 15.8, 16.4, 16.5, one label 22.8, 23.1, 23.7 against 23.8, 22.6, 23.2. (The guard's own label and scrolling targets were missed on both, as the machine's load moves them.)

## Indexed vertex codes and the corner pass (2026-11-17)

Built as the design settled it (`docs/specs/2026-11-16-line-anti-aliasing-design.md`, "The compute pre-pass: settled at its builder check"), each step measured in turn: the line bench against main (one alternating round per step, loads of 7 to 18, flagged: main's own figures moved by up to 1.8 times between rounds), and a hand-run split of the GPU's time (`line_time_splits_into_vertex_and_fragment`, `batch/line_time_tests.rs`: the bench's scenes uploaded and drawn as the world pass does into a 1920 x 1080 scene image, timestamped from empty passes round the corner pass and the draw; `Full` the world pass's fragment stage, `Constant` a fragment stage writing a constant, which leaves the vertex stage only the position).

**The split at the start:** vertex work. Scissored to one pixel the time hardly fell (10,000 mitered lines 877 against 875 µs, 100,000 11,028 against 11,053); with the constant fragment stage it halved (5,133), the vertex stage's attributes dropped by the compiler. Metal gave the one-pixel and empty runs a time only now and then, so they are not figures to lean on.

GPU, line bench (µs; main in brackets, the same round):

| Step | mixed | miter 1k | miter 10k | round 1k | round 10k | miter 100k | round 100k |
|---|---|---|---|---|---|---|---|
| Start | 256 (61) | 188 (40) | 1,119 (434) | 190 (57) | 1,759 (820) | 9,237 | 18,865 |
| Indexed codes | 89 (47) | 123 (34) | 658 (370) | 104 (56) | 1,294 (766) | 5,950 | 10,141 |
| Corner pass | 89 (50) | 77 (37) | 558 (393) | 108 (54) | 1,182 (786) | 6,112 | 9,494 |
| Arcs of 8 | 83 (52) | 80 (44) | 530 (445) | 89 (65) | 668 (836) | 5,896 | 7,338 |
| Cull, fragments read the table | 70 (41) | 93 (88) | 809 (711) | 90 (53) | 1,887 (1,124) | 5,645 | 5,760 |
| No modulo, style in the flags | 64 (42) | 59 (34) | 383 (490) | 73 (56) | 528 (889) | 4,449 | 5,255 |

The fifth row's round ran at a load of 15 and both sides' figures with it; the culling does not touch the bench (every line is in view).

The split (`Full`, µs; the corner pass included):

| Step | mixed | miter 1k | miter 10k | round 1k | round 10k | miter 100k |
|---|---|---|---|---|---|---|
| Start | — | 92 | 875 | 185 | 1,778 | 8,973 |
| Indexed codes | 136 | 67 | 516 | 89 | 830 | 5,408 |
| Corner pass | 133 | 94 | 523 | 96 | 796 | 5,419 |
| Arcs of 8 | 127 | 117 | 484 | 77 | 606 | 5,108 |
| Fragments read the table | — | 61 | 397 | 65 | 498 | 4,029 |
| No modulo | — | 47 | 332 | 56 | 414 | 3,460 |
| Style in the flags, Hard-only axis | 51 | 47 | 338 | 57 | 422 | 3,515 |

(The mixed scene ran first and cold until the measure warmed the GPU up; its earlier figures are not comparable.)

- **Indexed vertex codes** cut the vertex stage by about 40%: a piece's slots are shaded once each (a segment 6 vertices for its 12 corners, a miter or bevel join 9 for 12, a round join 23 for 54 then, a round cap 18 for 48).
- **The corner pass** moved the three corners a vertex worked out into one compute thread a point, and changed little: 10,000 mitered lines 516 to 523 µs in the split. Working the corners out was not where the time went. The pass itself costs 5.5 to 6 µs for 6,000 points, mostly a dispatch's fixed cost, 18 to 27 µs for 60,000 and 250 µs for 600,000.
- **Arcs of 8 triangles** took a round join from 23 slots to 15 and a round cap from 18 to 10, and 10,000 round lines 24% down in the split (796 to 606 µs); round pieces' coverage against true discs is within 0.108 (0.105 at 16), and an arc corner's texture coordinate across now runs on past the half width as its offset does, so a textured round cap keeps within 2/255.
- **Fragments reading their line's shared values from the table** (params, slot, a two-point line's half length) took 8 floats out of what a vertex hands on: 18% down on 10,000 and 100,000 lines. Handing the style in the vertex's flags so that only the rare fragments that need the table read it, and working out the image axis only for Hard lines, measured the same in alternating runs (kept: the fragment stage reads no table for a plain line).
- **A closed line's neighbours wrapped by a modulo** (`((i % n) + n) % n`, seven a vertex): replacing it with one step round saved 14 to 17%.
- **Measured and not kept:** building only the place a join vertex's role asks for, rather than every candidate and then choosing, was 2% slower in alternating runs.

**Final, three alternating rounds** (loads 10.5, 8.2 and 7.2, flagged; medians, µs):

| Figure | Main | Branch | Change |
|---|---|---|---|
| mixed render | 836.6 | 328.7 | −61% |
| mixed prepare | 368.1 | 104.2 | −72% |
| mixed GPU | 41.9 | 67.0 | +60% |
| miter 1k render | 649.7 | 267.1 | −59% |
| miter 1k prepare | 276.0 | 85.6 | −69% |
| miter 1k GPU | 39.5 | 81.4 | +106% |
| miter 10k render | 5,397.5 | 1,430.7 | −73% |
| miter 10k prepare | 2,656.5 | 680.4 | −74% |
| miter 10k GPU | 384.0 | 474.6 | +24% |
| round 1k render | 1,164.5 | 284.7 | −76% |
| round 1k prepare | 562.7 | 109.2 | −81% |
| round 1k GPU | 55.3 | 79.0 | +43% |
| round 10k render | 10,731.8 | 1,765.8 | −84% |
| round 10k prepare | 5,831.5 | 912.2 | −84% |
| round 10k GPU | 776.2 | 521.2 | −33% |
| miter 100k render / GPU | — | 12,867.5 / 4,570.8 | main cannot draw it |
| round 100k render / GPU | — | 15,864.4 / 5,278.2 | main cannot draw it |

Each round's GPU figures, branch (main): mixed 68.3, 67.0, 63.2 (41.9, 39.8, 43.5); miter 1k 98.3, 81.4, 60.0 (36.0, 53.5, 39.5); miter 10k 427.3, 682.1, 474.6 (384.0, 419.7, 371.9); round 1k 69.3, 79.0, 79.9 (55.3, 73.8, 54.9); round 10k 859.2, 521.2, 508.2 (773.0, 820.6, 776.2). The corner pass: 5.5 to 6.0 µs (mixed), 5.7 to 9.8 (1k), 19 to 27 (10k), 251 to 254 (100k).

**Against the bound (1.15 times main):** met on 10,000 round lines (0.67) and, in the quieter rounds, on 10,000 mitered ones (0.78 in the round before the final three, 1.24 in their median); missed on the 1,000-line scenes and the mixed thousand (1.4 to 2.1). There the draw itself is about as long as main's whole frame of lines: the split's position-only vertex stage alone takes 33 µs for 1,000 mitered lines (main's world pass about 35 to 40), each vertex loading its line's entry and five points through two dependent reads, mixing segment, join and cap work in each group of vertices, and the corner pass adding its fixed 6. What would cut further: a vertex record the corner pass writes for the vertices a point's pieces share (the points' window loads would go), drawing a line's pieces sorted by kind so each group runs one kind's code, or skipping the corner pass below a few thousand points.

**CPU:** still far under main, but less than before the indexed codes (render −65% to −88%, prepare −79% to −89% then): the index buffer is three codes a triangle where the codes were one.
