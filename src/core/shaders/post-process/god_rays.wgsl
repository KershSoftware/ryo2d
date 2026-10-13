// God Rays / Light Shafts post-process shader
// Creates radial light beams from bright light sources using screen-space ray marching

// Extended uniform parameters for god rays
// Total: 128 bytes (16-byte aligned)
struct GodRaysParams {
    // x: intensity, y: samples, z: decay, w: density
    params: vec4<f32>,
    // x: weight, y: threshold, z: num_lights, w: unused
    params2: vec4<f32>,
    // x: time, y: width, z: height, w: unused
    time_resolution: vec4<f32>,
    // Light positions 0-1 in UV space: x: l0.x, y: l0.y, z: l1.x, w: l1.y
    light_pos_01: vec4<f32>,
    // Light positions 2-3 in UV space: x: l2.x, y: l2.y, z: l3.x, w: l3.y
    light_pos_23: vec4<f32>,
    // Padding
    _padding0: vec4<f32>,
    _padding1: vec4<f32>,
    _padding2: vec4<f32>,
};

@group(0) @binding(0) var t_input: texture_2d<f32>;
@group(0) @binding(1) var s_input: sampler;
@group(0) @binding(2) var<uniform> effect: GodRaysParams;

// Calculate brightness/luminance of a color
fn luminance(color: vec3<f32>) -> f32 {
    return dot(color, vec3<f32>(0.299, 0.587, 0.114));
}

// Sample god rays from a single light source
fn sample_god_rays(
    tex_coord: vec2<f32>,
    light_pos: vec2<f32>,
    samples: i32,
    decay: f32,
    density: f32,
    weight: f32,
    threshold: f32
) -> vec3<f32> {
    // Distance from pixel to light (used for falloff)
    let dist_to_light = length(light_pos - tex_coord);

    // Reduce effect when very close to light source to prevent blow-out
    // Falloff starts at distance 0.1, full effect at distance 0.3+
    let proximity_falloff = smoothstep(0.05, 0.25, dist_to_light);

    // Direction from pixel to light
    let delta_uv = (light_pos - tex_coord) * density / f32(samples);

    var sample_uv = tex_coord;
    var decay_factor = 1.0;
    var accumulated = vec3<f32>(0.0);

    // Ray march toward the light
    for (var i = 0; i < samples; i++) {
        sample_uv += delta_uv;

        // Clamp to valid UV range
        let clamped_uv = clamp(sample_uv, vec2<f32>(0.0), vec2<f32>(1.0));

        // Sample the scene
        let sample_color = textureSample(t_input, s_input, clamped_uv);
        let brightness = luminance(sample_color.rgb);

        // Light above the threshold, eased in over the half threshold above it
        let knee = max(threshold * 0.5, 1e-4);
        let contribution = smoothstep(threshold, threshold + knee, brightness);
        accumulated += min(sample_color.rgb, vec3<f32>(65504.0)) * decay_factor * weight * contribution;

        // Apply decay for each sample (rays fade as they extend)
        decay_factor *= decay;
    }

    return accumulated * proximity_falloff;
}

@fragment
fn fs_main(in: FullscreenVertex) -> @location(0) vec4<f32> {
    // Get original scene color
    let original = textureSample(t_input, s_input, in.uv);

    // Extract parameters
    let intensity = effect.params.x;
    let samples = i32(effect.params.y);
    let decay = effect.params.z;
    let density = effect.params.w;
    let weight = effect.params2.x;
    let threshold = effect.params2.y;
    let num_lights = i32(effect.params2.z);

    // Early out if no lights or zero intensity
    if num_lights == 0 || intensity < 0.001 {
        return original;
    }

    var god_rays = vec3<f32>(0.0);

    // Process each light source (max 4)
    if num_lights >= 1 {
        let light_pos = vec2<f32>(effect.light_pos_01.x, effect.light_pos_01.y);
        god_rays += sample_god_rays(in.uv, light_pos, samples, decay, density, weight, threshold);
    }

    if num_lights >= 2 {
        let light_pos = vec2<f32>(effect.light_pos_01.z, effect.light_pos_01.w);
        god_rays += sample_god_rays(in.uv, light_pos, samples, decay, density, weight, threshold);
    }

    if num_lights >= 3 {
        let light_pos = vec2<f32>(effect.light_pos_23.x, effect.light_pos_23.y);
        god_rays += sample_god_rays(in.uv, light_pos, samples, decay, density, weight, threshold);
    }

    if num_lights >= 4 {
        let light_pos = vec2<f32>(effect.light_pos_23.z, effect.light_pos_23.w);
        god_rays += sample_god_rays(in.uv, light_pos, samples, decay, density, weight, threshold);
    }

    // Added to the HDR scene; the tonemap rolls it off
    return vec4<f32>(original.rgb + god_rays * intensity, original.a);
}
