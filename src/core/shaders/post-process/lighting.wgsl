// 2D Lighting post-process shader
// Applies the light accumulation buffer to the scene

struct EffectParams {
    color: vec4<f32>,           // Ambient light color (RGB) + unused (16 bytes)
    params: vec4<f32>,          // x: ambient intensity, y: unused, z: unused, w: unused (16 bytes)
    time_resolution: vec4<f32>, // x: time, y: width, z: height (16 bytes)
    _padding: vec4<f32>,        // Padding (16 bytes) - total 64 bytes
};

@group(0) @binding(0) var t_scene: texture_2d<f32>;
@group(0) @binding(1) var t_light: texture_2d<f32>;
@group(0) @binding(2) var s_input: sampler;
@group(0) @binding(3) var<uniform> effect: EffectParams;

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
    let scene_color = textureSample(t_scene, s_input, in.tex_coord);
    let light_color = textureSample(t_light, s_input, in.tex_coord);
    let ambient_color = effect.color.rgb;
    let ambient_intensity = effect.params.x;
    let ambient = ambient_color * ambient_intensity;
    let total_light = ambient + light_color.rgb;
    let lit_color = scene_color.rgb * total_light;

    return vec4<f32>(lit_color, scene_color.a);
}
