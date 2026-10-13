// Color tint/grade post-process shader
// Applies a color overlay with adjustable intensity

struct EffectParams {
    color: vec4<f32>,           // Tint color (16 bytes)
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
    
    // Multiply blend mode for color grading
    // This preserves luminance while shifting colors
    let tinted = scene_color.rgb * mix(vec3<f32>(1.0), effect.color.rgb, intensity);
    
    return vec4<f32>(tinted, scene_color.a);
}
