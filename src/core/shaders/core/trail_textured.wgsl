// Textured trail/line uber-shader
// Same as trail.wgsl but with texture sampling

// The camera the trails are drawn through (`CameraGlobals`)
struct TrailGlobals {
    view_proj: mat4x4<f32>,
    resolution: vec2<f32>,
    game_time: f32,
    real_time: f32,
    snap: f32,
};

struct TrailVertex {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) effect_params: vec4<f32>,
    @location(4) perpendicular: vec2<f32>,
};

@group(0) @binding(0) var<uniform> globals: TrailGlobals;

@group(1) @binding(0) var trail_texture: texture_2d<f32>;
@group(1) @binding(1) var trail_sampler: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) effect_params: vec4<f32>,
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
    return gradient_noise(vec2<f32>(along_t * frequency, time * speed)) * amplitude;
}

fn ribbon_displacement(uv: vec2<f32>, amplitude: f32, frequency: f32, speed: f32, time: f32) -> f32 {
    return sin(uv.x * frequency * 6.28318 + time * speed) * amplitude;
}

// ========== Vertex Shader ==========

@vertex
fn vs_main(vertex: TrailVertex) -> VertexOutput {
    var out: VertexOutput;
    var pos = vertex.position;

    let type_style = u32(vertex.effect_params[0]);
    let effect_type = type_style & 0xFu;

    if (effect_type == 1u) {
        let d = lightning_displacement(vertex.uv.x, vertex.effect_params[1], vertex.effect_params[2], vertex.effect_params[3], globals.real_time);
        pos += vertex.perpendicular * d;
    } else if (effect_type == 2u) {
        let d = noise_displacement(vertex.uv.x, vertex.effect_params[1], vertex.effect_params[2], vertex.effect_params[3], globals.real_time);
        pos += vertex.perpendicular * d;
    } else if (effect_type == 4u) {
        let d = ribbon_displacement(vertex.uv, vertex.effect_params[1], vertex.effect_params[2], vertex.effect_params[3], globals.real_time);
        pos += vertex.perpendicular * d;
    }

    out.position = globals.view_proj * vec4<f32>(pos, 0.0, 1.0);
    out.color = vertex.color;
    out.uv = vertex.uv;
    out.effect_params = vertex.effect_params;

    if (effect_type == 3u) {
        out.uv.x += globals.real_time * vertex.effect_params[1];
    }

    return out;
}

// ========== Fragment Shader ==========

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let tex_color = textureSample(trail_texture, trail_sampler, in.uv);
    let type_style = u32(in.effect_params[0]);
    let style = (type_style >> 4u) & 0x3u;

    let edge_dist = abs(in.uv.y - 0.5) * 2.0;
    let aa = fwidth(edge_dist);

    var color = tex_color * in.color;

    if (style == 0u) {
        let alpha = 1.0 - smoothstep(1.0 - aa * 2.0, 1.0, edge_dist);
        color.a *= alpha;
    } else if (style == 1u) {
        let alpha = 1.0 - smoothstep(0.7, 1.0, edge_dist);
        color.a *= alpha;
    } else {
        let core_val = 1.0 - smoothstep(0.0, 0.3, edge_dist);
        let glow_val = 1.0 - smoothstep(0.3, 1.0, edge_dist);
        let brightness = core_val + glow_val * 0.5;
        color = vec4<f32>(color.rgb * brightness, color.a * (core_val + glow_val * 0.3));
    }

    return color;
}
