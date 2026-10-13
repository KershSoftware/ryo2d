// Screen flash post-process shader
// Additively blends a color over the entire screen

struct EffectParams {
    color: vec4<f32>,           // Flash color (16 bytes)
    params: vec4<f32>,          // x: intensity (0-1) (16 bytes)
    time_resolution: vec4<f32>, // x: time, y: width, z: height (16 bytes)
    _padding: vec4<f32>,        // Padding (16 bytes) - total 64 bytes
};

@group(0) @binding(0) var t_input: texture_2d<f32>;
@group(0) @binding(1) var s_input: sampler;
@group(0) @binding(2) var<uniform> effect: EffectParams;

@fragment
fn fs_main(in: FullscreenVertex) -> @location(0) vec4<f32> {
    let scene_color = textureSample(t_input, s_input, in.uv);
    let intensity = effect.params.x;
    
    // Additive blend for bright flash, normal blend for colored overlays
    // Use the flash color's alpha to control blend mode
    let flash = effect.color.rgb * intensity;
    
    // Screen blend mode: 1 - (1-a)*(1-b)
    // This brightens without blowing out already bright areas
    let blended = 1.0 - (1.0 - scene_color.rgb) * (1.0 - flash);
    
    return vec4<f32>(blended, scene_color.a);
}
