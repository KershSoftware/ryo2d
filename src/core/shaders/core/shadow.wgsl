// Shadow volumes as batch triangles (`lighting::push_shadow_volumes`): each
// keeps 1 - strength of its light's mask in the light buffer's alpha.
//
// A vertex carries the light's position (uv), its radius and shadow
// softness (params.xy) and the shadow colour (linear; alpha its strength).

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) params: vec4<f32>,
    @location(4) across: vec2<f32>,
    @location(5) bounds: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world: vec2<f32>,
    @location(1) @interpolate(flat) light: vec2<f32>,
    @location(2) @interpolate(flat) strength: f32,
    // Radius, softness
    @location(3) @interpolate(flat) shape: vec2<f32>,
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.position = globals.view_proj * vec4<f32>(in.position, 0.0, 1.0);
    out.world = in.position;
    out.light = in.uv;
    out.strength = in.color.a;
    out.shape = in.params.xy;
    return out;
}

@fragment
fn fs_shadow(in: VertexOutput) -> @location(0) vec4<f32> {
    let reach = clamp(distance(in.world, in.light) / max(in.shape.x, 1e-3), 0.0, 1.0);
    // Soft shadows fade with distance from the light; hard ones stay whole
    let softness = clamp(in.shape.y, 0.0, 1.0);
    let edge = 1.0 - smoothstep(0.0, max(softness * 0.5, 1e-3), reach) * softness;
    return vec4<f32>(0.0, 0.0, 0.0, clamp(in.strength * edge, 0.0, 1.0));
}
