// Trails and lines, built on the GPU from their points: each triangle's
// code names its piece (`line_pieces.wgsl` builds its vertices), styled
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
// half width (`batch/line_cover_tests.rs`).

@group(1) @binding(0) var line_texture: texture_2d<f32>;
@group(1) @binding(1) var line_sampler: sampler;

// `LineInfo` (render/batch/polyline.rs)
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
// One a triangle: its piece's point << 7 | the piece's kind << 5 | the
// triangle within the piece
@group(2) @binding(2) var<storage, read> line_codes: array<u32>;

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
};

struct WorldVertex {
    vertex: PieceVertex,
    line: u32,
};

fn line_point(at: u32) -> LinePt {
    let point = line_points[at];
    return LinePt(point.pos, point.half_width, point.u, unpack4x8unorm(point.color));
}

// Vertex `index` of the frame's world lines, three a triangle
fn world_vertex(index: u32) -> WorldVertex {
    let code = line_codes[index / 3u];
    let corner = index % 3u;
    let at = code >> 7u;
    let kind = (code >> 5u) & 3u;
    let tri = code & 31u;
    let line_id = line_points[at].line;
    let info = lines[line_id];
    let count = i32(info.count);
    let closed = info.closed == 1u;
    let point = i32(at - info.first) + line_piece_corner(kind, tri, corner);
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
    return WorldVertex(line_piece_vertex(w, look, kind, tri, corner), line_id);
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
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    var out: VertexOutput;
    let world = world_vertex(index);
    let vertex = world.vertex;
    var pos = vertex.position;
    let line = lines[world.line];
    let params = line.params;
    let effect_type = u32(params[0]) & 0xFu;
    let across = vertex.across;

    // Displacement effects move the vertex along its perpendicular
    // by their own `u` (a corner point's copies by the point's)
    let along = vec2<f32>(vertex.effect_u, vertex.uv.y);
    if effect_type == 1u {
        pos += across * lightning_displacement(along.x, params[1], params[2], params[3], globals.real_time);
    } else if effect_type == 2u {
        pos += across * noise_displacement(along.x, params[1], params[2], params[3], globals.real_time);
    } else if effect_type == 4u {
        pos += across * ribbon_displacement(along, params[1], params[2], params[3], globals.real_time);
    }

    // The fringe: a pixel of the camera's image out, as far as the vertex
    // may go, its distances grown by the push
    var push = 0.0;
    if globals.px_per_unit > 0.0 {
        push = min(1.0 / globals.px_per_unit, vertex.limit);
    }
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

    out.position = globals.view_proj * vec4<f32>(pos, 0.0, 1.0);
    out.color = vec4<f32>(srgb_to_linear(vertex.color.rgb) * line.emission, vertex.color.a);
    out.uv = uv;
    out.params = params;
    out.bounds = line.bounds;
    out.measure = measure;
    out.flags = vertex.flags;
    out.k = vertex.k;
    out.along_half = line.along_half;
    if effect_type == 3u {
        out.uv.x += globals.real_time * params[1];
    }
    return out;
}

// ========== Fragment Shaders ==========

// How much of the pixel the line covers: each edge's distance in the
// image's pixels through a one-pixel box, two edges blended by their angle
fn line_cover(in: VertexOutput) -> f32 {
    let ppu = globals.px_per_unit;
    if ppu <= 0.0 {
        return 1.0;
    }
    if (in.flags & LINE_FLAG_ROUND) != 0u {
        return clamp((in.measure.z - length(in.measure.xy)) * ppu + 0.5, 0.0, 1.0);
    }
    var across = 1.0;
    if in.measure.z < LINE_NO_EDGE * 0.5 {
        let d = in.measure.x * ppu;
        let hw = in.measure.z * ppu;
        across = clamp(min(d + 0.5, hw) - max(d - 0.5, -hw), 0.0, 1.0);
    }
    if (in.flags & LINE_FLAG_SECOND) == 0u {
        return across;
    }
    var second: f32;
    if (in.flags & LINE_FLAG_ALONG) != 0u {
        let d = in.measure.y * ppu;
        let hw = in.along_half * ppu;
        second = clamp(min(d + 0.5, hw) - max(d - 0.5, -hw), 0.0, 1.0);
    } else {
        second = clamp(in.measure.y * ppu + 0.5, 0.0, 1.0);
    }
    let product = across * second;
    var toward = max(across + second - 1.0, 0.0);
    if in.k >= 0.0 {
        toward = min(across, second);
    }
    return product + (toward - product) * in.k * in.k;
}

// The pixel's distance from the centre line over the half width, which
// Soft and Glow shape by (a round piece's from its corner point)
fn line_edge(in: VertexOutput) -> f32 {
    if in.measure.z <= 0.0 || in.measure.z >= LINE_NO_EDGE * 0.5 {
        return 0.0;
    }
    if (in.flags & LINE_FLAG_ROUND) != 0u {
        return length(in.measure.xy) / in.measure.z;
    }
    return abs(in.measure.x) / in.measure.z;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return ui_out(styled(in.color, line_edge(in), in.params, line_cover(in)));
}

@fragment
fn fs_textured(in: VertexOutput) -> @location(0) vec4<f32> {
    // Derivatives first, while control flow is uniform
    let ddx = dpdx(in.uv);
    let ddy = dpdy(in.uv);
    let texel = sample_wrapped(line_texture, line_sampler, in.uv, in.bounds, ddx, ddy);
    let texture = vec4<f32>(srgb_to_linear(texel.rgb), texel.a);
    return ui_out(styled(texture * in.color, line_edge(in), in.params, line_cover(in)));
}

// ========== Tests ==========

// Each fragment counts one (tests: a pixel drawn twice shows two)
@fragment
fn fs_count(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(1.0 / 255.0);
}

// Every vertex of the frame's world lines, unpushed, as the pieces build
// them (tests compare them with the CPU builder's)
struct LineDump {
    position: vec2<f32>,
    uv: vec2<f32>,
    across: vec2<f32>,
    push: vec2<f32>,
    color: vec4<f32>,
    dist: f32,
    second: f32,
    half: f32,
    limit: f32,
    dist_rate: f32,
    second_rate: f32,
    k: f32,
    flags: u32,
    effect_u: f32,
    pad0: f32,
    pad1: f32,
    pad2: f32,
};

@group(3) @binding(0) var<storage, read_write> line_dump: array<LineDump>;

@compute @workgroup_size(64)
fn cs_line_dump(@builtin(global_invocation_id) id: vec3<u32>) {
    let index = id.x;
    if index >= arrayLength(&line_dump) {
        return;
    }
    let v = world_vertex(index).vertex;
    line_dump[index] = LineDump(
        v.position,
        v.uv,
        v.across,
        v.push,
        v.color,
        v.dist,
        v.second,
        v.half,
        v.limit,
        v.dist_rate,
        v.second_rate,
        v.k,
        v.flags,
        v.effect_u,
        0.0,
        0.0,
        0.0,
    );
}
