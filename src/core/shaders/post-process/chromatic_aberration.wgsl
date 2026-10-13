// Chromatic aberration post-process shader
// Separates RGB channels with an offset for a glitchy or cinematic look

struct EffectParams {
    color: vec4<f32>,           // Not used (16 bytes)
    params: vec4<f32>,          // x: offset, y: radial (0 or 1) (16 bytes)
    time_resolution: vec4<f32>, // x: time, y: width, z: height (16 bytes)
    _padding: vec4<f32>,        // Padding (16 bytes) - total 64 bytes
};

@group(0) @binding(0) var t_input: texture_2d<f32>;
@group(0) @binding(1) var s_input: sampler;
@group(0) @binding(2) var<uniform> effect: EffectParams;

@fragment
fn fs_main(in: FullscreenVertex) -> @location(0) vec4<f32> {
    let offset = effect.params.x;
    let radial = effect.params.y > 0.5;
    
    var offset_vec: vec2<f32>;
    
    if radial {
        // Radial: offset points away from center
        offset_vec = normalize(in.uv - 0.5) * offset;
    } else {
        // Directional: offset along X axis
        offset_vec = vec2<f32>(offset, 0.0);
    }
    
    // Sample each color channel at a different offset
    let r = textureSample(t_input, s_input, in.uv + offset_vec).r;
    let g = textureSample(t_input, s_input, in.uv).g;
    let b = textureSample(t_input, s_input, in.uv - offset_vec).b;
    let a = textureSample(t_input, s_input, in.uv).a;
    
    return vec4<f32>(r, g, b, a);
}
