// Bloom extract pass - extracts light above a linear luminance threshold
// (1.0 is white), eased in over the half threshold above it, never clamped.
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

fn luminance(color: vec3<f32>) -> f32 {
    return dot(color, vec3<f32>(0.2126, 0.7152, 0.0722));
}

// The bright part of one scene texel: light whose luminance passes
// `threshold`, eased in over the half threshold above it, never clamped
fn bright(uv: vec2<f32>, threshold: f32) -> vec3<f32> {
    let color = min(textureSampleLevel(t_input, s_input, uv, 0.0).rgb, vec3<f32>(65504.0));
    let knee = max(threshold * 0.5, 1e-4);
    return color * smoothstep(threshold, threshold + knee, luminance(color));
}

@fragment
fn fs_main(in: FullscreenVertex) -> @location(0) vec4<f32> {
    let threshold = effect.params.x;
    // Half a scene texel: the four samples land on the 2x2 texel centres
    let half_texel = 0.5 / vec2<f32>(effect.time_resolution.y, effect.time_resolution.z);
    var sum = bright(in.uv + vec2<f32>(-half_texel.x, -half_texel.y), threshold);
    sum += bright(in.uv + vec2<f32>(half_texel.x, -half_texel.y), threshold);
    sum += bright(in.uv + vec2<f32>(-half_texel.x, half_texel.y), threshold);
    sum += bright(in.uv + vec2<f32>(half_texel.x, half_texel.y), threshold);
    return vec4<f32>(sum * 0.25, 1.0);
}
