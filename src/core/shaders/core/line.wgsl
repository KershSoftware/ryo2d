// Trails and lines: triangles built on the CPU (`Vertex2D`), styled across
// their width (Solid, Soft, Glow) and optionally displaced along their
// perpendicular (Lightning, Noise, Ribbon) or scrolled (UV scroll).
//
// Colours are taken as linear (2a's trail renderer did the same) and leave
// premultiplied by alpha. The textured entry samples the atlas slot in
// `bounds` (raw sRGB bytes), repeating it along the line.

@group(1) @binding(0) var line_texture: texture_2d<f32>;
@group(1) @binding(1) var line_sampler: sampler;

struct LineVertex {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) params: vec4<f32>,
    @location(4) across: vec2<f32>,
    @location(5) bounds: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) @interpolate(flat) params: vec4<f32>,
    @location(3) @interpolate(flat) bounds: vec4<f32>,
};

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
fn vs_main(vertex: LineVertex) -> VertexOutput {
    var out: VertexOutput;
    var pos = vertex.position;
    let params = vertex.params;
    let effect_type = u32(params[0]) & 0xFu;

    // Displacement effects move the vertex along its perpendicular
    if effect_type == 1u {
        pos += vertex.across * lightning_displacement(vertex.uv.x, params[1], params[2], params[3], globals.real_time);
    } else if effect_type == 2u {
        pos += vertex.across * noise_displacement(vertex.uv.x, params[1], params[2], params[3], globals.real_time);
    } else if effect_type == 4u {
        pos += vertex.across * ribbon_displacement(vertex.uv, params[1], params[2], params[3], globals.real_time);
    }

    out.position = globals.view_proj * vec4<f32>(pos, 0.0, 1.0);
    out.color = vertex.color;
    out.uv = vertex.uv;
    out.params = params;
    out.bounds = vertex.bounds;
    if effect_type == 3u {
        out.uv.x += globals.real_time * params[1];
    }
    return out;
}

// ========== Fragment Shaders ==========

// `color` shaded across the line by its style, premultiplied by alpha
fn styled(color_in: vec4<f32>, uv: vec2<f32>, params: vec4<f32>, aa: f32) -> vec4<f32> {
    let style = (u32(params[0]) >> 4u) & 0x3u;
    let edge_dist = abs(uv.y - 0.5) * 2.0;
    var color = color_in;
    if style == 0u {
        // Solid: how much of the pixel the line covers, ramping over the
        // pixel its edge crosses (`aa` is edge_dist per pixel), so a line
        // one pixel wide stays solid
        let to_edge = (1.0 - edge_dist) / max(aa, 1e-4);
        color.a *= clamp(to_edge + 0.5, 0.0, 1.0);
    } else if style == 1u {
        // Soft edges
        color.a *= 1.0 - smoothstep(0.7, 1.0, edge_dist);
    } else {
        // Glow: a bright core inside a fainter halo
        let core_val = 1.0 - smoothstep(0.0, 0.3, edge_dist);
        let glow_val = 1.0 - smoothstep(0.3, 1.0, edge_dist);
        let brightness = core_val + glow_val * 0.5;
        color = vec4<f32>(color.rgb * brightness, color.a * (core_val + glow_val * 0.3));
    }
    // Glow alpha peaks above 1, which a float image would not clamp: the
    // blend would subtract from what is below
    let p = premultiply(color);
    return vec4<f32>(p.rgb, min(p.a, 1.0));
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let aa = fwidth(abs(in.uv.y - 0.5) * 2.0);
    return ui_out(styled(in.color, in.uv, in.params, aa));
}

@fragment
fn fs_textured(in: VertexOutput) -> @location(0) vec4<f32> {
    // Derivatives first, while control flow is uniform
    let ddx = dpdx(in.uv);
    let ddy = dpdy(in.uv);
    let aa = fwidth(abs(in.uv.y - 0.5) * 2.0);
    let texel = sample_wrapped(line_texture, line_sampler, in.uv, in.bounds, ddx, ddy);
    let texture = vec4<f32>(srgb_to_linear(texel.rgb), texel.a);
    return ui_out(styled(texture * in.color, in.uv, in.params, aa));
}
