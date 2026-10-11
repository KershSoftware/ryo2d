// Vignette post-process shader
// Darkens the edges of the screen for a cinematic look

struct EffectParams {
    color: vec4<f32>,           // Not used (16 bytes)
    params: vec4<f32>,          // x: intensity, y: radius, z: softness (16 bytes)
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
    let scene_color = textureSample(t_input, s_input, in.tex_coord);
    
    let intensity = effect.params.x;
    let radius = effect.params.y;
    let softness = effect.params.z;
    
    // Distance from center, normalized so the corners are at 1.0
    let center = vec2<f32>(0.5, 0.5);
    let dist = distance(in.tex_coord, center) / 0.70710678;
    
    // Smooth vignette falloff
    // At dist <= radius: vignette = 0
    // At dist >= radius + softness: vignette = intensity
    let vignette = smoothstep(radius, radius + softness, dist) * intensity;
    
    // Darken the scene by the vignette amount
    let darkened = scene_color.rgb * (1.0 - vignette);
    
    return vec4<f32>(darkened, scene_color.a);
}
