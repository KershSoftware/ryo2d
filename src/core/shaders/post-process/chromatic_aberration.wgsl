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
    let offset = effect.params.x;
    let radial = effect.params.y > 0.5;
    
    var offset_vec: vec2<f32>;
    
    if radial {
        // Radial: offset points away from center
        offset_vec = normalize(in.tex_coord - 0.5) * offset;
    } else {
        // Directional: offset along X axis
        offset_vec = vec2<f32>(offset, 0.0);
    }
    
    // Sample each color channel at a different offset
    let r = textureSample(t_input, s_input, in.tex_coord + offset_vec).r;
    let g = textureSample(t_input, s_input, in.tex_coord).g;
    let b = textureSample(t_input, s_input, in.tex_coord - offset_vec).b;
    let a = textureSample(t_input, s_input, in.tex_coord).a;
    
    return vec4<f32>(r, g, b, a);
}
