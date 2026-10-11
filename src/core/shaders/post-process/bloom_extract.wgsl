// Bloom extract pass - extracts bright pixels above luminance threshold
// Uses soft knee to prevent hard edges at the threshold boundary.
// Writes a half-resolution image: each output texel averages the 2x2 scene
// texels under it, thresholded one by one so thin bright lines survive.

struct EffectParams {
    color: vec4<f32>,           // Not used (16 bytes)
    params: vec4<f32>,          // x: threshold, y: intensity, z: radius, w: blur_passes (16 bytes)
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

fn luminance(color: vec3<f32>) -> f32 {
    return dot(color, vec3<f32>(0.2126, 0.7152, 0.0722));
}

// The bright part of one scene texel, with a soft knee from threshold-0.1
// to threshold+0.1
fn bright(uv: vec2<f32>, threshold: f32) -> vec3<f32> {
    let color = textureSampleLevel(t_input, s_input, uv, 0.0);
    let knee = 0.1;
    return color.rgb * smoothstep(threshold - knee, threshold + knee, luminance(color.rgb));
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let threshold = effect.params.x;
    // Half a scene texel: the four samples land on the 2x2 texel centres
    let half_texel = 0.5 / vec2<f32>(effect.time_resolution.y, effect.time_resolution.z);
    var sum = bright(in.tex_coord + vec2<f32>(-half_texel.x, -half_texel.y), threshold);
    sum += bright(in.tex_coord + vec2<f32>(half_texel.x, -half_texel.y), threshold);
    sum += bright(in.tex_coord + vec2<f32>(-half_texel.x, half_texel.y), threshold);
    sum += bright(in.tex_coord + vec2<f32>(half_texel.x, half_texel.y), threshold);
    return vec4<f32>(sum * 0.25, 1.0);
}
