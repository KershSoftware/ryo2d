// Bloom blur pass - separable Gaussian blur
// Direction controlled by params.w: 0.0 = horizontal, 1.0 = vertical
// params.z is sigma, in texels of the (half-resolution) bloom image

struct EffectParams {
    color: vec4<f32>,           // Not used (16 bytes)
    params: vec4<f32>,          // x: threshold, y: intensity, z: sigma, w: direction (16 bytes)
    time_resolution: vec4<f32>, // x: time, y: width, z: height (16 bytes)
    _padding: vec4<f32>,        // Padding (16 bytes) - total 64 bytes
};

@group(0) @binding(0) var t_input: texture_2d<f32>;
@group(0) @binding(1) var s_input: sampler;
@group(0) @binding(2) var<uniform> effect: EffectParams;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) tex_coord: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var positions = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(1.0, -1.0),
    );

    var tex_coords = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 1.0),
    );

    var out: VertexOutput;
    out.position = vec4<f32>(positions[vertex_index], 0.0, 1.0);
    out.tex_coord = tex_coords[vertex_index];
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let sigma = clamp(effect.params.z, 0.25, 16.0);
    let direction = effect.params.w; // 0.0 = horizontal, 1.0 = vertical

    // One texel along the blur direction, in UV space
    var dir: vec2<f32>;
    if direction < 0.5 {
        dir = vec2<f32>(1.0 / effect.time_resolution.y, 0.0);
    } else {
        dir = vec2<f32>(0.0, 1.0 / effect.time_resolution.z);
    }

    // Gaussian sampled at every texel out to 3 sigma. Each bilinear sample
    // covers two neighbouring texels, placed between them by their weights,
    // so the blur stays continuous at any size.
    let falloff = -1.0 / (2.0 * sigma * sigma);
    let last = i32(ceil(3.0 * sigma));

    var result = textureSampleLevel(t_input, s_input, in.tex_coord, 0.0);
    var total = 1.0;
    for (var i = 1; i <= last; i += 2) {
        let a = f32(i);
        let b = a + 1.0;
        let wa = exp(a * a * falloff);
        let wb = exp(b * b * falloff);
        let w = wa + wb;
        let offset = dir * ((a * wa + b * wb) / w);
        result += textureSampleLevel(t_input, s_input, in.tex_coord + offset, 0.0) * w;
        result += textureSampleLevel(t_input, s_input, in.tex_coord - offset, 0.0) * w;
        total += 2.0 * w;
    }

    return vec4<f32>(result.rgb / total, 1.0);
}
