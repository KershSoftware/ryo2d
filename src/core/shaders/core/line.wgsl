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

// `LineInfo` (render/batch/lines.rs)
struct LineInfo {
    params: vec4<f32>,
    bounds: vec4<f32>,
    emission: f32,
    along_half: f32,
    first: u32,
    count: u32,
    join: u32,
    cap: u32,
    closed: u32,
    edge: u32,
    miter_limit: f32,
    pad0: u32,
    pad1: u32,
    pad2: u32,
};

// `LinePoint` (render/batch/lines.rs)
struct LinePoint {
    pos: vec2<f32>,
    half_width: f32,
    u: f32,
    // sRGB bytes, straight alpha
    color: u32,
    line: u32,
};

@group(2) @binding(0) var<storage, read> lines: array<LineInfo>;
@group(2) @binding(1) var<storage, read> line_points: array<LinePoint>;

struct VertexOutput {
    @builtin(position) @invariant position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) @interpolate(flat) params: vec4<f32>,
    @location(3) @interpolate(flat) bounds: vec4<f32>,
    // Across, inside the second edge and the half width (a round piece's
    // offset in the first two), in world units
    @location(4) measure: vec3<f32>,
    @location(5) @interpolate(flat) flags: u32,
    @location(6) @interpolate(flat) k: f32,
    @location(7) @interpolate(flat) along_half: f32,
    // Across over the normal's component on the image's nearest axis
    // (`line_axis`), in world units
    @location(8) across_axis: f32,
    @location(9) @interpolate(flat) hard: u32,
};

struct WorldVertex {
    vertex: PieceVertex,
    line: u32,
};

fn line_point(at: u32) -> LinePt {
    let point = line_points[at];
    return LinePt(point.pos, point.half_width, point.u, unpack4x8unorm(point.color));
}

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
    for (var slot = 0; slot < 5; slot++) {
        var i = point + slot - 2;
        if closed {
            i = ((i % count) + count) % count;
        }
        if i >= 0 && i < count {
            w.p[slot] = line_point(info.first + u32(i));
            w.present |= 1u << u32(slot);
            if closed && i == 0 {
                w.seam |= 1u << u32(slot);
            }
        }
    }
    let look = LineLook(info.join, info.cap, info.miter_limit);
    return WorldVertex(line_piece_vertex(w, look, kind, slot), line_id);
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
    let axis = line_axis(vertex.normal, globals.view_proj, globals.resolution);
    var fringe = line_fringe(vertex, pos, globals.px_per_unit, axis.major);
    out.position = globals.view_proj * vec4<f32>(fringe.pos, 0.0, 1.0);
    if hard {
        let gridded = line_on_grid(out.position, globals.resolution);
        out.position = gridded.clip;
        fringe = line_moved(fringe, vertex.flags, gridded.moved, axis, globals.view_proj, globals.resolution, globals.px_per_unit);
    }
    out.color = vec4<f32>(srgb_to_linear(vertex.color.rgb) * line.emission, vertex.color.a);
    out.uv = fringe.uv;
    out.params = params;
    out.bounds = line.bounds;
    out.measure = fringe.measure;
    out.flags = vertex.flags;
    out.k = vertex.k;
    out.along_half = line.along_half;
    out.across_axis = fringe.across_axis;
    out.hard = u32(hard);
    if effect_type == 3u {
        out.uv.x += globals.real_time * params[1];
    }
    return out;
}

// ========== Fragment Shaders ==========

// What the fragment's line measures (`line_cover.wgsl`)
fn line_measure(in: VertexOutput) -> LineMeasure {
    return LineMeasure(in.measure, in.across_axis, in.flags, in.k, in.along_half, in.hard == 1u);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let pixel = line_pixel(line_measure(in), in.params, globals.px_per_unit);
    return ui_out(styled(in.color, pixel.shape, in.params, pixel.cover));
}

@fragment
fn fs_textured(in: VertexOutput) -> @location(0) vec4<f32> {
    // Derivatives first, while control flow is uniform
    let ddx = dpdx(in.uv);
    let ddy = dpdy(in.uv);
    let texel = sample_wrapped(line_texture, line_sampler, in.uv, in.bounds, ddx, ddy);
    let texture = vec4<f32>(srgb_to_linear(texel.rgb), texel.a);
    let pixel = line_pixel(line_measure(in), in.params, globals.px_per_unit);
    return ui_out(styled(texture * in.color, pixel.shape, in.params, pixel.cover));
}
