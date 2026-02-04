// Textured trail/line rendering shader
// Renders trails and lines with texture mapping along the trail

struct TrailGlobals {
    view_proj: mat4x4<f32>,     // Camera projection matrix
    resolution: vec2<f32>,      // Screen resolution
    time: f32,                  // Current time
    _padding: f32,
};

// Per-vertex data for trail segments
struct TrailVertex {
    @location(0) position: vec2<f32>,  // World position
    @location(1) color: vec4<f32>,     // RGBA color (tint)
    @location(2) uv: vec2<f32>,        // UV for texturing (x = along trail, y = across 0-1)
};

@group(0) @binding(0) var<uniform> globals: TrailGlobals;

// Texture bindings (group 1)
@group(1) @binding(0) var trail_texture: texture_2d<f32>;
@group(1) @binding(1) var trail_sampler: sampler;

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
    // Sample texture
    let tex_color = textureSample(trail_texture, trail_sampler, in.uv);

    // Multiply texture color by vertex color (for tinting and alpha)
    return tex_color * in.color;
}

// Alternative fragment shader with soft edges
@fragment
fn fs_soft(in: VertexOutput) -> @location(0) vec4<f32> {
    // Sample texture
    let tex_color = textureSample(trail_texture, trail_sampler, in.uv);

    // Soften edges based on distance from center
    // UV.y is 0 at left edge, 1 at right edge, so center is 0.5
    let edge_dist = abs(in.uv.y - 0.5) * 2.0; // 0 at center, 1 at edges
    let alpha = 1.0 - smoothstep(0.7, 1.0, edge_dist);

    let color = tex_color * in.color;
    return vec4<f32>(color.rgb, color.a * alpha);
}

// Glow effect fragment shader
@fragment
fn fs_glow(in: VertexOutput) -> @location(0) vec4<f32> {
    // Sample texture
    let tex_color = textureSample(trail_texture, trail_sampler, in.uv);

    // Create a glow effect that's brightest at center
    let edge_dist = abs(in.uv.y - 0.5) * 2.0;
    let core = 1.0 - smoothstep(0.0, 0.3, edge_dist);
    let glow = 1.0 - smoothstep(0.3, 1.0, edge_dist);

    // Core is full brightness, glow fades out
    let brightness = core + glow * 0.5;

    let color = tex_color * in.color;
    return vec4<f32>(color.rgb * brightness, color.a * (core + glow * 0.3));
}
