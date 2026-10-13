// Fade post-process shader
// Blends the screen toward a target color based on progress

struct EffectParams {
    color: vec4<f32>,           // Target fade color (16 bytes)
    params: vec4<f32>,          // x: progress (0-1) (16 bytes)
    time_resolution: vec4<f32>, // x: time, y: width, z: height (16 bytes)
    _padding: vec4<f32>,        // Padding (16 bytes) - total 64 bytes
};

@group(0) @binding(0) var t_input: texture_2d<f32>;
@group(0) @binding(1) var s_input: sampler;
@group(0) @binding(2) var<uniform> effect: EffectParams;

@fragment
fn fs_main(in: FullscreenVertex) -> @location(0) vec4<f32> {
    let scene_color = textureSample(t_input, s_input, in.uv);
    let progress = effect.params.x;
    
    // Linear interpolation between scene and fade color
    return mix(scene_color, effect.color, progress);
}
