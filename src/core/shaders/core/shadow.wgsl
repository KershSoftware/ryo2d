// 2D Shadow rendering shader
// Renders shadow volume geometry to subtract from the light accumulation buffer

struct ShadowGlobals {
    view_proj: mat4x4<f32>,     // Camera projection matrix
    resolution: vec2<f32>,      // Screen resolution
    time: f32,                  // Current time
    _padding: f32,
};

struct ShadowVertex {
    // Position in world space
    @location(0) position: vec2<f32>,
    // Light position (for falloff)
    @location(1) light_pos: vec2<f32>,
    // Light radius
    @location(2) light_radius: f32,
};

@group(0) @binding(0) var<uniform> globals: ShadowGlobals;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_pos: vec2<f32>,
    @location(1) light_pos: vec2<f32>,
    @location(2) light_radius: f32,
};

@vertex
fn vs_main(in: ShadowVertex) -> VertexOutput {
    var out: VertexOutput;

    // Transform to clip space
    out.clip_position = globals.view_proj * vec4<f32>(in.position, 0.0, 1.0);
    out.world_pos = in.position;
    out.light_pos = in.light_pos;
    out.light_radius = in.light_radius;

    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Calculate distance from this fragment to the light
    let dx = in.world_pos.x - in.light_pos.x;
    let dy = in.world_pos.y - in.light_pos.y;
    let dist = sqrt(dx * dx + dy * dy);

    // Shadow intensity based on distance from light
    // Shadows are darker closer to the light (where there would be more light)
    // and fade out toward the edge of the light radius
    let normalized_dist = clamp(dist / in.light_radius, 0.0, 1.0);

    // Quadratic falloff matching the light falloff
    // This ensures shadows properly subtract the light contribution
    let shadow_intensity = pow(1.0 - normalized_dist, 2.0);

    // Output shadow color
    // With reverse-subtract blending: dst = dst - src
    // So we output the amount to subtract (matching light intensity)
    return vec4<f32>(shadow_intensity, shadow_intensity, shadow_intensity, 1.0);
}
