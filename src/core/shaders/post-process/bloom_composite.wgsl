// Bloom composite pass - blends blurred bloom with original scene
// Custom bind group: 2 input textures (scene + bloom), sampler, uniform

struct EffectParams {
    color: vec4<f32>,           // Not used (16 bytes)
    params: vec4<f32>,          // x: threshold, y: intensity, z: radius, w: blur_passes (16 bytes)
    time_resolution: vec4<f32>, // x: time, y: width, z: height (16 bytes)
    _padding: vec4<f32>,        // Padding (16 bytes) - total 64 bytes
};

@group(0) @binding(0) var t_scene: texture_2d<f32>;
@group(0) @binding(1) var t_bloom: texture_2d<f32>;
@group(0) @binding(2) var s_input: sampler;
@group(0) @binding(3) var<uniform> effect: EffectParams;

@fragment
fn fs_main(in: FullscreenVertex) -> @location(0) vec4<f32> {
    let scene = textureSample(t_scene, s_input, in.uv);
    let bloom = textureSample(t_bloom, s_input, in.uv);
    let intensity = effect.params.y;

    // Additive blend: scene + bloom * intensity
    let result = scene.rgb + bloom.rgb * intensity;

    return vec4<f32>(result, scene.a);
}
