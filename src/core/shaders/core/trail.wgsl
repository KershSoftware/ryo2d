// Trail/Line rendering shader
// Renders trails and lines as triangle strips with interpolated color/width

struct TrailGlobals {
    view_proj: mat4x4<f32>,     // Camera projection matrix
    resolution: vec2<f32>,      // Screen resolution
    time: f32,                  // Current time
    _padding: f32,
};

// Per-vertex data for trail segments
struct TrailVertex {
    @location(0) position: vec2<f32>,  // World position
    @location(1) color: vec4<f32>,     // RGBA color
    @location(2) uv: vec2<f32>,        // UV for texturing (x = along trail, y = across)
};

@group(0) @binding(0) var<uniform> globals: TrailGlobals;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
};

@vertex
fn vs_main(vertex: TrailVertex) -> VertexOutput {
    var out: VertexOutput;

    // Transform world position to clip space
    out.position = globals.view_proj * vec4<f32>(vertex.position, 0.0, 1.0);
    out.color = vertex.color;
    out.uv = vertex.uv;

    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Simple solid color with interpolated alpha
    return in.color;
}

// Alternative fragment shader with soft edges
@fragment
fn fs_soft(in: VertexOutput) -> @location(0) vec4<f32> {
    // Soften edges based on distance from center (uv.y = 0 at center, ±0.5 at edges)
    let edge_dist = abs(in.uv.y * 2.0);
    let alpha = 1.0 - smoothstep(0.7, 1.0, edge_dist);

    return vec4<f32>(in.color.rgb, in.color.a * alpha);
}

// Glow effect fragment shader
@fragment
fn fs_glow(in: VertexOutput) -> @location(0) vec4<f32> {
    // Create a glow effect that's brightest at center
    let edge_dist = abs(in.uv.y * 2.0);
    let core = 1.0 - smoothstep(0.0, 0.3, edge_dist);
    let glow = 1.0 - smoothstep(0.3, 1.0, edge_dist);

    // Core is full brightness, glow fades out
    let brightness = core + glow * 0.5;

    return vec4<f32>(in.color.rgb * brightness, in.color.a * (core + glow * 0.3));
}
