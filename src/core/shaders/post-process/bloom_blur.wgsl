// Bloom blur pass - 9-tap separable Gaussian blur
// Direction controlled by params.w: 0.0 = horizontal, 1.0 = vertical
// Radius scaled by params.z

struct EffectParams {
    color: vec4<f32>,           // Not used (16 bytes)
    params: vec4<f32>,          // x: threshold, y: intensity, z: radius, w: direction (16 bytes)
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
    let radius = effect.params.z;
    let direction = effect.params.w; // 0.0 = horizontal, 1.0 = vertical

    // Pixel size in UV space
    let pixel_size = vec2<f32>(1.0 / effect.time_resolution.y, 1.0 / effect.time_resolution.z);

    // Direction vector: horizontal or vertical
    var dir: vec2<f32>;
    if direction < 0.5 {
        dir = vec2<f32>(pixel_size.x, 0.0);
    } else {
        dir = vec2<f32>(0.0, pixel_size.y);
    }
    dir = dir * radius;

    // 9-tap Gaussian weights (sigma ~= 2.7)
    let w0 = 0.227027027;
    let w1 = 0.1945945946;
    let w2 = 0.1216216216;
    let w3 = 0.0540540541;
    let w4 = 0.0162162162;

    // Center sample
    var result = textureSample(t_input, s_input, in.tex_coord) * w0;

    // Symmetric taps
    result += textureSample(t_input, s_input, in.tex_coord + dir * 1.0) * w1;
    result += textureSample(t_input, s_input, in.tex_coord - dir * 1.0) * w1;
    result += textureSample(t_input, s_input, in.tex_coord + dir * 2.0) * w2;
    result += textureSample(t_input, s_input, in.tex_coord - dir * 2.0) * w2;
    result += textureSample(t_input, s_input, in.tex_coord + dir * 3.0) * w3;
    result += textureSample(t_input, s_input, in.tex_coord - dir * 3.0) * w3;
    result += textureSample(t_input, s_input, in.tex_coord + dir * 4.0) * w4;
    result += textureSample(t_input, s_input, in.tex_coord - dir * 4.0) * w4;

    return vec4<f32>(result.rgb, 1.0);
}
