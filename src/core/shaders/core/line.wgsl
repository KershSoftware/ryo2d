// Trails and lines, built on the GPU from their points: each vertex's code
// (its index) names its piece and the piece's slot (`line_pieces.wgsl`
// builds its vertices), styled
// across their width (Solid, Soft, Glow) and optionally displaced along
// their perpendicular (Lightning, Noise, Ribbon) or scrolled (UV scroll).
//
// A point's colour arrives in sRGB and is decoded to linear light, times
// its line's emission; it leaves premultiplied by alpha. What a line's
// pieces share (style and effect parameters, the atlas slot a textured
// line repeats, emission, its points, join and cap) is its entry in the
// line table. The textured entry samples the atlas slot in `bounds` (raw
// sRGB bytes), repeating it along the line.
//
// Every pixel is covered by its distances to the line's edges in the
// image's pixels (no derivative); the coverage multiplies the straight
// alpha of every style, and Soft and Glow shape by the distance over the
// half width, averaged over the pixel; a Soft or Glow line under half a
// pixel in half width covers as one of half a pixel, dimmed
// (`batch/line_cover_tests.rs`). A Hard line (its own edge, or its
// target's on a pixel-art camera, whose `snap` is set) draws each pixel
// whole or not at all. The fringe and the coverage are `line_cover.wgsl`'s,
// which the UI's lines use too.

@group(1) @binding(0) var line_texture: texture_2d<f32>;
@group(1) @binding(1) var line_sampler: sampler;

@group(2) @binding(0) var<storage, read> lines: array<LineInfo>;
@group(2) @binding(1) var<storage, read> line_points: array<LinePoint>;
// Each point's corner, and the limits it sets the quads beside it, as the
// corner pass worked them out this frame (`line_corners.wgsl`)
@group(2) @binding(2) var<storage, read> line_corners: array<LineCorner>;
@group(2) @binding(3) var<storage, read> line_limits: array<vec2<f32>>;
// The frame's point count, and whether the corner pass ran (1) or each
// vertex works its corners out in place (0): a small stream skips the pass
// (`corners.rs`)
@group(2) @binding(4) var<uniform> line_frame: vec4<u32>;

// What a vertex hands its fragments in `flags` besides its own (round,
// second edge, along): its line's style in bits 4 and 5, and `LINE_HARD`
// when its line draws Hard
const LINE_OWN_FLAGS: u32 = 7u;
const LINE_STYLE_SHIFT: u32 = 4u;
const LINE_HARD: u32 = 64u;

// What else the fragment stage shares across a line (the atlas slot a
// textured line repeats, a two-point line's half length between its caps)
// it reads from the line table by the line's number where it needs it, so
// a vertex hands on only what varies
struct VertexOutput {
    @builtin(position) @invariant position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
    // Across, inside the second edge and the half width (a round piece's
    // offset in the first two), in world units
    @location(2) measure: vec3<f32>,
    @location(3) @interpolate(flat) flags: u32,
    @location(4) @interpolate(flat) k: f32,
    @location(5) @interpolate(flat) line: u32,
    // Across over the normal's component on the image's nearest axis
    // (`line_axis`), in world units
    @location(6) across_axis: f32,
};

struct WorldVertex {
    vertex: PieceVertex,
    line: u32,
};

// The world line vertex of `code`, a value of the line index buffer: its
// piece's point << 7 | the piece's kind << 5 | the piece's slot
fn world_vertex(code: u32) -> WorldVertex {
    let at = code >> 7u;
    let kind = (code >> 5u) & 3u;
    let slot = code & 31u;
    let line_id = line_points[at].line;
    let info = lines[line_id];
    let count = i32(info.count);
    let closed = info.closed == 1u;
    let point = i32(at - info.first) + line_piece_corner(kind, slot);
    var w: LineWindow;
    w.present = 0u;
    w.seam = 0u;
    // On a closed line (three points or more) a window slot lies at most
    // one turn past either end, so one step round brings it back
    for (var k = 0; k < 5; k++) {
        var i = point + k - 2;
        if closed {
            i = line_wrap(i, count);
        }
        if i >= 0 && i < count {
            w.p[k] = line_pt(line_points[info.first + u32(i)]);
            w.present |= 1u << u32(k);
            if closed && i == 0 {
                w.seam |= 1u << u32(k);
            }
        }
    }
    let look = LineLook(info.join, info.cap, info.miter_limit);
    if line_frame.y == 0u {
        return WorldVertex(line_piece_vertex(w, look, lp_corner_here(w, look), kind, slot), line_id);
    }
    // The corner at the vertex's point, and the limits its neighbours set
    // (none past an open line's ends)
    var own = point;
    if closed {
        own = line_wrap(own, count);
    }
    var corner: LpCorner;
    corner.joint = line_joint_of(line_corners[info.first + u32(own)]);
    corner.prev_after = LINE_NO_EDGE;
    corner.next_before = LINE_NO_EDGE;
    if lp_has(w, -1) {
        let prev = select(own - 1, line_wrap(own - 1, count), closed);
        corner.prev_after = line_limits[info.first + u32(prev)].y;
    }
    if lp_has(w, 1) {
        let next = select(own + 1, line_wrap(own + 1, count), closed);
        corner.next_before = line_limits[info.first + u32(next)].x;
    }
    return WorldVertex(line_piece_vertex(w, look, corner, kind, slot), line_id);
}

// ========== Noise Functions ==========

fn hash11(p: f32) -> f32 {
    var p2 = fract(p * 0.1031);
    p2 = p2 * (p2 + 33.33);
    p2 = p2 * (p2 + p2);
    return fract(p2);
}

fn hash21(p: vec2<f32>) -> f32 {
    var p3 = fract(p * vec2<f32>(0.1031, 0.1030));
    p3 = p3 + dot(p3, p3.yx + 33.33);
    return fract((p3.x + p3.y) * p3.x);
}

fn value_noise(p: f32) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(hash11(i), hash11(i + 1.0), u);
}

fn gradient_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash21(i);
    let b = hash21(i + vec2<f32>(1.0, 0.0));
    let c = hash21(i + vec2<f32>(0.0, 1.0));
    let d = hash21(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y) * 2.0 - 1.0;
}

// ========== Effect Functions ==========

fn lightning_displacement(along_t: f32, jitter: f32, speed: f32, detail: f32, time: f32) -> f32 {
    var displacement = 0.0;
    var amp = jitter;
    var freq = 4.0;
    let octaves = u32(clamp(detail, 1.0, 4.0));
    for (var i = 0u; i < octaves; i++) {
        displacement += (value_noise(along_t * freq + time * speed + f32(i) * 17.0) * 2.0 - 1.0) * amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    return displacement;
}

fn noise_displacement(along_t: f32, frequency: f32, amplitude: f32, speed: f32, time: f32) -> f32 {
    let n1 = gradient_noise(vec2<f32>(along_t * frequency, time * speed));
    let n2 = gradient_noise(vec2<f32>(along_t * frequency * 2.3 + 7.0, time * speed * 0.7 + 3.0));
    return (n1 * 0.7 + n2 * 0.3) * amplitude;
}

fn ribbon_displacement(uv: vec2<f32>, amplitude: f32, frequency: f32, speed: f32, time: f32) -> f32 {
    let wave1 = sin(uv.x * frequency * 6.28318 - time * speed);
    let wave2 = sin(uv.x * frequency * 1.618 * 6.28318 + time * speed * 0.7);
    return (wave1 * 0.7 + wave2 * 0.3) * amplitude;
}

// ========== Vertex Shader ==========

@vertex
fn vs_main(@builtin(vertex_index) code: u32) -> VertexOutput {
    var out: VertexOutput;
    let world = world_vertex(code);
    let vertex = world.vertex;
    var pos = vertex.position;
    let line = lines[world.line];
    let params = line.params;
    let effect_type = u32(params[0]) & 0xFu;
    let across = vertex.across;

    // Displacement effects move the vertex along its perpendicular
    // by their own `u` (a corner point's copies by the point's). A piece
    // whose ends move apart is sheared, its distances unchanged, so it
    // draws narrower across its slant (`polyline.rs`, the shear's limit)
    let along = vec2<f32>(vertex.effect_u, vertex.uv.y);
    if effect_type == 1u {
        pos += across * lightning_displacement(along.x, params[1], params[2], params[3], globals.real_time);
    } else if effect_type == 2u {
        pos += across * noise_displacement(along.x, params[1], params[2], params[3], globals.real_time);
    } else if effect_type == 4u {
        pos += across * ribbon_displacement(along, params[1], params[2], params[3], globals.real_time);
    }

    // The fringe: a pixel of the camera's image out, as far as the vertex
    // may go, its distances grown by the push (`line_cover.wgsl`)
    // A Hard line's vertex on the rasteriser's grid, its distances moved
    // with it (`line_on_grid`)
    let hard = line_hard(line.edge, globals.snap);
    // Only a Hard line measures along the image's axis
    var axis = LineAxis(vec2<f32>(0.0), 0.0);
    if hard {
        axis = line_axis(vertex.normal, globals.view_proj, globals.resolution);
    }
    var fringe = line_fringe(vertex, pos, globals.px_per_unit, axis.major);
    out.position = globals.view_proj * vec4<f32>(fringe.pos, 0.0, 1.0);
    if hard {
        let gridded = line_on_grid(out.position, globals.resolution);
        out.position = gridded.clip;
        fringe = line_moved(fringe, vertex.flags, gridded.moved, axis, globals.view_proj, globals.resolution, globals.px_per_unit);
    }
    out.color = vec4<f32>(srgb_to_linear(vertex.color.rgb) * line.emission, vertex.color.a);
    out.uv = fringe.uv;
    out.measure = fringe.measure;
    out.flags = vertex.flags | (line_style(params) << LINE_STYLE_SHIFT) | select(0u, LINE_HARD, hard);
    out.k = vertex.k;
    out.line = world.line;
    out.across_axis = fringe.across_axis;
    if effect_type == 3u {
        out.uv.x += globals.real_time * params[1];
    }
    return out;
}

// ========== Fragment Shaders ==========

// What the fragment's line measures (`line_cover.wgsl`), a two-point
// capped line's half length between its caps read from its entry only for
// the fragments that measure it
fn line_measure(in: VertexOutput) -> LineMeasure {
    var along_half = 0.0;
    if (in.flags & LINE_FLAG_ALONG) != 0u {
        along_half = lines[in.line].along_half;
    }
    return LineMeasure(in.measure, in.across_axis, in.flags & LINE_OWN_FLAGS, in.k, along_half, (in.flags & LINE_HARD) != 0u);
}

fn line_style_of(in: VertexOutput) -> u32 {
    return (in.flags >> LINE_STYLE_SHIFT) & 3u;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let style = line_style_of(in);
    let pixel = line_pixel(line_measure(in), style, globals.px_per_unit);
    return ui_out(styled(in.color, pixel.shape, style, pixel.cover));
}

@fragment
fn fs_textured(in: VertexOutput) -> @location(0) vec4<f32> {
    // Derivatives first, while control flow is uniform
    let ddx = dpdx(in.uv);
    let ddy = dpdy(in.uv);
    let texel = sample_wrapped(line_texture, line_sampler, in.uv, lines[in.line].bounds, ddx, ddy);
    let texture = vec4<f32>(srgb_to_linear(texel.rgb), texel.a);
    let style = line_style_of(in);
    let pixel = line_pixel(line_measure(in), style, globals.px_per_unit);
    return ui_out(styled(texture * in.color, pixel.shape, style, pixel.cover));
}
