// A line's fringe and coverage, shared by the world's lines (`line.wgsl`)
// and the UI's (`ui.wgsl`), so both push and cover alike: each vertex is
// pushed a pixel of its image out as far as its limit lets it, its
// distances grown by the push, and each pixel is covered by its distances
// to the line's edges in the image's pixels (no derivative). A Hard edge
// draws a pixel whole when its centre lies inside the line, its distance
// across taken along the image's axis nearest the line's normal, so a
// line of a pixel is one pixel a column
// (`a_hard_line_is_one_pixel_per_column_at_any_angle`,
// `a_hard_line_is_its_width_in_pixels_per_column`,
// `a_line_draws_whole_pixels_only_when_hard_on_any_target`), and its
// corners follow its segments' drawn edges: each half of a miter's point
// meets the other segment's drawn edge
// (`a_hard_miter_tip_narrows_evenly_to_its_point`) and a round piece is as
// wide as its segments are drawn
// (`a_hard_round_piece_keeps_to_its_segments_drawn_width`).

// A line's edge as the line table numbers it (`edge_code`): Smooth unless
// Hard, on every target
const LINE_EDGE_HARD: u32 = 1u;

// How far a Hard band is moved up (or left) in pixels, so a pixel centre
// on its edge, as a line on whole or half pixels puts them, falls inside
// on the upper side and outside on the lower however the interpolation
// rounds
const LINE_HARD_NUDGE: f32 = 1.0 / 64.0;

// Whether a line of `edge` draws Hard
fn line_hard(edge: u32) -> bool {
    return edge == LINE_EDGE_HARD;
}

// `v`, a direction in the line's units, in the image's pixels (x right, y
// down) through `to_clip` into an image of `resolution` pixels
fn line_image_dir(v: vec2<f32>, to_clip: mat4x4<f32>, resolution: vec2<f32>) -> vec2<f32> {
    let clip = (to_clip * vec4<f32>(v, 0.0, 0.0)).xy;
    return clip * vec2<f32>(0.5, -0.5) * resolution;
}

// A vertex's normal in the image (unit, zero for none) and its component
// along the image's axis nearest it, signed so that a distance over it
// runs down (or right) in the image (0 for no normal)
struct LineAxis {
    normal: vec2<f32>,
    major: f32,
};

// What a Hard line's vertex hands its fragments about a second edge it
// takes along the image's axis (`line_second_bits`): that it does, and that
// the edge's outward normal runs up (or left) in the image
const LINE_SECOND_NARROWED: u32 = 128u;
const LINE_SECOND_UP: u32 = 256u;

fn line_second_bits(second: LineAxis) -> u32 {
    if second.major == 0.0 {
        return 0u;
    }
    return LINE_SECOND_NARROWED | select(0u, LINE_SECOND_UP, second.major < 0.0);
}

fn line_axis(normal: vec2<f32>, to_clip: mat4x4<f32>, resolution: vec2<f32>) -> LineAxis {
    let image = line_image_dir(normal, to_clip, resolution);
    let len = length(image);
    if len <= 0.0 {
        return LineAxis(vec2<f32>(0.0), 0.0);
    }
    let n = image / len;
    if abs(n.y) >= abs(n.x) {
        return LineAxis(n, n.y);
    }
    return LineAxis(n, n.x);
}

// A Hard line's vertex is put on a 1/256 pixel grid, which the rasteriser's
// fixed-point grid holds exactly, so the rasteriser does not move it, and
// its distances are moved with it (`line_moved`): each triangle's
// distances are then those of its pixels' true places, and two triangles
// agree to within a float's rounding on which pixels a column draws. Left
// to the rasteriser, a vertex moves up to 1/512 of a pixel while its
// distances stay, and two triangles meeting in a column could disagree
// about a pixel centre that near a band's edge, drawing it twice or not at
// all. Copies of a place move alike, as they are alike
const LINE_GRID: f32 = 256.0;

// Where `clip` lands on the grid of an image of `resolution` pixels, and
// how far it moved in its pixels
struct LineGridded {
    clip: vec4<f32>,
    moved: vec2<f32>,
};

fn line_on_grid(clip: vec4<f32>, resolution: vec2<f32>) -> LineGridded {
    let w = select(1.0, clip.w, clip.w != 0.0);
    let pixel = (clip.xy / w * vec2<f32>(0.5, -0.5) + 0.5) * resolution;
    let on = round(pixel * LINE_GRID) / LINE_GRID;
    let back = (on / resolution - 0.5) * vec2<f32>(2.0, -2.0) * w;
    return LineGridded(vec4<f32>(back, clip.zw), on - pixel);
}

// `fringe`, of a vertex of `flags`, moved `moved` image pixels: its
// distances across over the axis (its own and a second edge's, `second`)
// moved with it, or a round piece's offset (`ppu` pixels a unit, the
// image's axes in the line's units from `to_clip` and `resolution`). A
// second edge's plain distance stays, off by no more than the
// rasteriser's own move would leave it
fn line_moved(
    fringe: LineFringe,
    flags: u32,
    moved: vec2<f32>,
    axis: LineAxis,
    second: LineAxis,
    to_clip: mat4x4<f32>,
    resolution: vec2<f32>,
    ppu: f32,
) -> LineFringe {
    var out = fringe;
    if ppu <= 0.0 {
        return out;
    }
    if (flags & LINE_FLAG_ROUND) != 0u {
        let x = line_image_dir(vec2<f32>(1.0, 0.0), to_clip, resolution);
        let y = line_image_dir(vec2<f32>(0.0, 1.0), to_clip, resolution);
        out.measure = vec3<f32>(
            out.measure.xy + vec2<f32>(dot(moved, x), dot(moved, y)) / (ppu * ppu),
            out.measure.z,
        );
    } else {
        if axis.major != 0.0 {
            out.across_axis += dot(moved, axis.normal) / (ppu * axis.major);
        }
        if second.major != 0.0 {
            out.second_axis += dot(moved, second.normal) / (ppu * second.major);
        }
    }
    return out;
}

// A Hard line's round piece is as wide as its segments are drawn: each
// segment's half width along the image's axis is its normal's component
// on that axis times its half width across it, so a round piece's offset
// is measured over that factor, taken from the segment before at the arc's
// start to the segment after at its end (a cap's is its segment's), its
// distance and half width left as they are
// (`a_hard_round_piece_keeps_to_its_segments_drawn_width`)
fn line_round_axis(
    fringe: LineFringe,
    vertex: PieceVertex,
    to_clip: mat4x4<f32>,
    resolution: vec2<f32>,
) -> LineFringe {
    var out = fringe;
    if (vertex.flags & LINE_FLAG_ROUND) == 0u {
        return out;
    }
    let before = abs(line_axis(vertex.normal, to_clip, resolution).major);
    let after = abs(line_axis(vertex.second_normal, to_clip, resolution).major);
    // How far round from the segment before to the one after the offset
    // lies, by its parts along the two normals
    let offset = vec2<f32>(vertex.dist, vertex.second);
    let across = vertex.normal.x * vertex.second_normal.y - vertex.normal.y * vertex.second_normal.x;
    var t = 0.5;
    if abs(across) > 1.0e-6 {
        let a = (offset.x * vertex.second_normal.y - offset.y * vertex.second_normal.x) / across;
        let b = (vertex.normal.x * offset.y - vertex.normal.y * offset.x) / across;
        if abs(a + b) > 1.0e-6 {
            t = clamp(b / (a + b), 0.0, 1.0);
        }
    }
    let factor = mix(before, after, t);
    if factor > 0.0 {
        out.measure = vec3<f32>(out.measure.xy / factor, out.measure.z);
    }
    return out;
}

// A vertex pushed out by the fringe: where it is, what it measures (across,
// inside the second edge and the half width; a round piece's offset in the
// first two, in the line's units) and its texture coordinate
struct LineFringe {
    pos: vec2<f32>,
    measure: vec3<f32>,
    uv: vec2<f32>,
    // The distance across over `axis` (`line_axis`): along the image's
    // axis nearest the normal, in the line's units
    across_axis: f32,
    // A second edge's segment's distance across over its axis: outside
    // the edge by its distance outside, plus the half width, over the
    // edge's normal's component on the axis nearest it
    second_axis: f32,
};

// `vertex`, placed at `at` (its position moved by effects or a scroll
// slot), pushed a pixel out at `ppu` pixels per unit, its normal's
// component along its image's nearest axis `axis` and its second edge's
// normal's `second` (0 for none)
fn line_fringe(vertex: PieceVertex, at: vec2<f32>, ppu: f32, axis: f32, second: f32) -> LineFringe {
    var push = 0.0;
    if ppu > 0.0 {
        push = min(1.0 / ppu, vertex.limit);
    }
    var pos = at;
    pos += vertex.push * push;
    var measure = vec3<f32>(
        vertex.dist + vertex.dist_rate * push,
        vertex.second - vertex.second_rate * push,
        vertex.half,
    );
    // The texture across runs on over the fringe as the distance does
    // (`uv.y` is 0.5 - 0.5 × dist ÷ half), so it is not squeezed inside
    var uv = vertex.uv;
    if (vertex.flags & LINE_FLAG_ROUND) != 0u {
        let offset = vec2<f32>(vertex.dist, vertex.second);
        measure = vec3<f32>(offset + vertex.push * push, vertex.half);
        let radius = length(offset);
        if radius > 0.0 {
            uv.y = 0.5 + (uv.y - 0.5) * length(measure.xy) / radius;
        }
    } else if vertex.half > 0.0 {
        uv.y -= 0.5 * vertex.dist_rate * push / vertex.half;
    }
    // A vertex without a normal is on its centre line
    var across_axis = 0.0;
    if axis != 0.0 && (vertex.flags & LINE_FLAG_ROUND) == 0u {
        across_axis = measure.x / axis;
    }
    var second_axis = 0.0;
    if second != 0.0 && (vertex.flags & (LINE_FLAG_ROUND | LINE_FLAG_ALONG)) == 0u {
        second_axis = (measure.z - measure.y) / second;
    }
    return LineFringe(pos, measure, uv, across_axis, second_axis);
}

// What a line's fragment is given: its measures (as `LineFringe`'s), its
// vertex's flags and `k`, a two-point capped line's half length between
// its caps, and whether it draws Hard
struct LineMeasure {
    measure: vec3<f32>,
    across_axis: f32,
    second_axis: f32,
    // The vertex's flags, with `line_second_bits`'
    flags: u32,
    k: f32,
    along_half: f32,
    hard: bool,
};

// How much of the pixel a line of half width `half` covers: each edge's
// distance in the image's pixels through a one-pixel box, two edges blended
// by their angle
fn line_cover(m: LineMeasure, half: f32, ppu: f32) -> f32 {
    if ppu <= 0.0 {
        return 1.0;
    }
    if (m.flags & LINE_FLAG_ROUND) != 0u {
        return clamp((half - length(m.measure.xy)) * ppu + 0.5, 0.0, 1.0);
    }
    var across = 1.0;
    if m.measure.z < LINE_NO_EDGE * 0.5 {
        let d = m.measure.x * ppu;
        let hw = half * ppu;
        across = clamp(min(d + 0.5, hw) - max(d - 0.5, -hw), 0.0, 1.0);
    }
    if (m.flags & LINE_FLAG_SECOND) == 0u {
        return across;
    }
    var second: f32;
    if (m.flags & LINE_FLAG_ALONG) != 0u {
        let d = m.measure.y * ppu;
        let hw = m.along_half * ppu;
        second = clamp(min(d + 0.5, hw) - max(d - 0.5, -hw), 0.0, 1.0);
    } else {
        second = clamp(m.measure.y * ppu + 0.5, 0.0, 1.0);
    }
    let product = across * second;
    var toward = max(across + second - 1.0, 0.0);
    if m.k >= 0.0 {
        toward = min(across, second);
    }
    return product + (toward - product) * m.k * m.k;
}

// Whether a pixel `d` pixels across lies in the half-open band of half
// width `hw` pixels about a centre line, moved up by the nudge
fn line_in_band(d: f32, hw: f32) -> bool {
    return d >= -hw - LINE_HARD_NUDGE && d < hw - LINE_HARD_NUDGE;
}

// A Hard line's coverage: 1 where the pixel's centre lies inside the line,
// 0 elsewhere. Across, the distance and the half width are taken along the
// image's axis nearest the normal (the distance over the normal's
// component on it), the half width at least half a pixel, so a line of a
// pixel is one pixel in each column (or row), and one narrower one wide; a
// miter's other outer edge is that segment's band's outer side, taken the
// same way (`LINE_SECOND_NARROWED`), so each half of a miter's point meets
// the other segment's drawn edge; any other second edge takes the centre
// inside it, and a round piece the centre within its half width of its
// corner point
fn line_hard_cover(m: LineMeasure, ppu: f32) -> f32 {
    if ppu <= 0.0 {
        return 1.0;
    }
    let hw = max(m.measure.z * ppu, 0.5);
    if (m.flags & LINE_FLAG_ROUND) != 0u {
        return select(0.0, 1.0, length(m.measure.xy) * ppu < hw);
    }
    var inside = true;
    if m.measure.z < LINE_NO_EDGE * 0.5 {
        inside = line_in_band(m.across_axis * ppu, hw);
    }
    if (m.flags & LINE_FLAG_SECOND) != 0u {
        if (m.flags & LINE_FLAG_ALONG) != 0u {
            inside = inside && line_in_band(m.measure.y * ppu, max(m.along_half * ppu, 0.5));
        } else if (m.flags & LINE_SECOND_NARROWED) != 0u {
            // The band's outer side, as `line_in_band` bounds it
            let d = m.second_axis * ppu;
            if (m.flags & LINE_SECOND_UP) != 0u {
                inside = inside && d >= -hw - LINE_HARD_NUDGE;
            } else {
                inside = inside && d < hw - LINE_HARD_NUDGE;
            }
        } else {
            inside = inside && m.measure.y * ppu >= -LINE_HARD_NUDGE;
        }
    }
    return select(0.0, 1.0, inside);
}

// The pixel's distance from the centre line over the half width, which
// Soft and Glow shape by (a round piece's from its corner point)
fn line_edge(m: LineMeasure) -> f32 {
    if m.measure.z <= 0.0 || m.measure.z >= LINE_NO_EDGE * 0.5 {
        return 0.0;
    }
    if (m.flags & LINE_FLAG_ROUND) != 0u {
        return length(m.measure.xy) / m.measure.z;
    }
    return abs(m.measure.x) / m.measure.z;
}

// What the line gives a pixel: its coverage, and Soft's and Glow's shape
struct LinePixel {
    cover: f32,
    shape: vec2<f32>,
};

// A Hard line's pixel: whole or nothing (`line_hard_cover`), Soft and Glow
// keeping their shape across it, as a Smooth line of its drawn half width
// shapes it
fn line_hard_pixel(m: LineMeasure, style: u32, ppu: f32) -> LinePixel {
    let cover = line_hard_cover(m, ppu);
    let half = m.measure.z;
    if style == LINE_SOLID || ppu <= 0.0 || half <= 0.0 || half >= LINE_NO_EDGE * 0.5 {
        return LinePixel(cover, style_shape_at(style, line_edge(m)));
    }
    var d = abs(m.measure.x) * ppu;
    if (m.flags & LINE_FLAG_ROUND) != 0u {
        d = length(m.measure.xy) * ppu;
    }
    return LinePixel(cover, style_shape(style, d, max(half * ppu, 0.5)));
}

// A Soft or Glow line under half a pixel in half width covers the pixels
// one of half a pixel would, dimmed by the ratio of their widths, so it
// stays whole wherever it lies, and its shape is averaged over each pixel
// (`style_shape`). Solid takes its own coverage, which stays whole as it is
fn line_pixel(m: LineMeasure, style: u32, ppu: f32) -> LinePixel {
    let half = m.measure.z;
    if m.hard {
        return line_hard_pixel(m, style, ppu);
    }
    if style == LINE_SOLID || ppu <= 0.0 || half <= 0.0 || half >= LINE_NO_EDGE * 0.5 {
        return LinePixel(line_cover(m, half, ppu), style_shape_at(style, line_edge(m)));
    }
    let hw = half * ppu;
    let covered = max(hw, 0.5);
    var d = abs(m.measure.x) * ppu;
    if (m.flags & LINE_FLAG_ROUND) != 0u {
        d = length(m.measure.xy) * ppu;
    }
    return LinePixel(line_cover(m, covered / ppu, ppu) * (hw / covered), style_shape(style, d, hw));
}
