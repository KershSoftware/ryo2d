// CRT monitor effect shader
// Adds scanlines, curvature, and RGB separation for retro aesthetics

struct EffectParams {
    color: vec4<f32>,           // Not used (16 bytes)
    params: vec4<f32>,          // x: scanline_intensity, y: curvature, z: rgb_offset (16 bytes)
    time_resolution: vec4<f32>, // x: time, y: width, z: height (16 bytes)
    _padding: vec4<f32>,        // Padding (16 bytes) - total 64 bytes
};

@group(0) @binding(0) var t_input: texture_2d<f32>;
@group(0) @binding(1) var s_input: sampler;
@group(0) @binding(2) var<uniform> effect: EffectParams;

// Apply barrel distortion for CRT curvature
fn apply_curvature(uv: vec2<f32>, curvature: f32) -> vec2<f32> {
    let centered = uv - 0.5;
    let dist = length(centered);
    let distorted = centered * (1.0 + curvature * dist * dist);
    return distorted + 0.5;
}

@fragment
fn fs_main(in: FullscreenVertex) -> @location(0) vec4<f32> {
    let scanline_intensity = effect.params.x;
    let curvature = effect.params.y;
    let rgb_offset = effect.params.z;
    
    // Apply screen curvature
    var uv = in.uv;
    if curvature > 0.0 {
        uv = apply_curvature(in.uv, curvature);
        
        // Black out pixels outside the curved screen
        if uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0 {
            return vec4<f32>(0.0, 0.0, 0.0, 1.0);
        }
    }
    
    // RGB separation (chromatic aberration from CRT)
    var color: vec3<f32>;
    if rgb_offset > 0.0 {
        let offset_vec = (uv - 0.5) * rgb_offset;
        color.r = textureSample(t_input, s_input, uv + offset_vec).r;
        color.g = textureSample(t_input, s_input, uv).g;
        color.b = textureSample(t_input, s_input, uv - offset_vec).b;
    } else {
        color = textureSample(t_input, s_input, uv).rgb;
    }
    
    // Scanlines
    if scanline_intensity > 0.0 {
        let screen_y = in.uv.y * effect.time_resolution.z;  // z = height
        // Alternate dark lines
        let scanline = sin(screen_y * 3.14159) * 0.5 + 0.5;
        let darkness = 1.0 - scanline_intensity * (1.0 - scanline);
        color = color * darkness;
    }
    
    return vec4<f32>(color, 1.0);
}
