// Ray-marched shadow shader
// Per-pixel ray casting for accurate 2D shadows

// Maximum number of occluder edges we can handle
const MAX_EDGES: u32 = 64u;

struct Globals {
    view_proj: mat4x4<f32>,
    resolution_time: vec4<f32>,  // x: width, y: height, z: time, w: unused
};

struct RaymarchParams {
    // Light position (x, y), radius (z), samples (w)
    light_params: vec4<f32>,
    // Shadow color (RGB) and intensity (A)
    shadow_color: vec4<f32>,
    // Number of edges (x), softness (y), unused (z, w)
    edge_params: vec4<f32>,
};

struct Edge {
    // Start point (x, y), end point (z, w)
    points: vec4<f32>,
};

struct EdgeBuffer {
    edges: array<Edge, 64>,  // MAX_EDGES
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var<uniform> params: RaymarchParams;
@group(0) @binding(2) var<storage, read> edge_buffer: EdgeBuffer;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world_pos: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    // Generate a quad covering the light's area
    let light_pos = params.light_params.xy;
    let light_radius = params.light_params.z;

    // Quad vertices (centered on light, sized to radius)
    var offsets = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(1.0, 1.0),
    );

    let offset = offsets[vertex_index];
    let world_pos = light_pos + offset * light_radius;

    var out: VertexOutput;
    out.position = globals.view_proj * vec4<f32>(world_pos, 0.0, 1.0);
    out.world_pos = world_pos;
    return out;
}

// Line segment intersection test
// Returns true if segments (p1, p2) and (p3, p4) intersect
fn segments_intersect(p1: vec2<f32>, p2: vec2<f32>, p3: vec2<f32>, p4: vec2<f32>) -> bool {
    let d1 = p2 - p1;
    let d2 = p4 - p3;
    let d3 = p1 - p3;

    let cross = d1.x * d2.y - d1.y * d2.x;

    // Parallel lines
    if abs(cross) < 0.0001 {
        return false;
    }

    let t = (d3.x * d2.y - d3.y * d2.x) / cross;
    let u = (d3.x * d1.y - d3.y * d1.x) / cross;

    // Check if intersection is within both segments
    return t >= 0.0 && t <= 1.0 && u >= 0.0 && u <= 1.0;
}

// Count how many edges the ray from pixel to light intersects
fn count_intersections(pixel: vec2<f32>, light: vec2<f32>, num_edges: u32) -> u32 {
    var count: u32 = 0u;

    // Unrolled loop for WGSL compatibility (check up to MAX_EDGES)
    for (var i: u32 = 0u; i < MAX_EDGES; i = i + 1u) {
        if i >= num_edges {
            break;
        }

        let edge = edge_buffer.edges[i];
        let edge_start = edge.points.xy;
        let edge_end = edge.points.zw;

        if segments_intersect(pixel, light, edge_start, edge_end) {
            count = count + 1u;
        }
    }

    return count;
}

// Sample occlusion along the ray with multiple samples
fn sample_occlusion(pixel: vec2<f32>, light: vec2<f32>, samples: u32, num_edges: u32) -> f32 {
    let dir = light - pixel;
    let dist = length(dir);

    if dist < 0.001 {
        return 0.0;  // At light position, no shadow
    }

    var occluded_samples: u32 = 0u;
    let step_size = 1.0 / f32(samples);

    // March along the ray
    for (var i: u32 = 1u; i < samples; i = i + 1u) {
        let t = f32(i) * step_size;
        let sample_pos = pixel + dir * t;

        // Check if this sample point is "inside" any occluder
        // We do this by checking ray intersections from sample to light
        let intersections = count_intersections(sample_pos, light, num_edges);

        // Odd number of intersections = inside occluder = shadowed
        if (intersections % 2u) == 1u {
            occluded_samples = occluded_samples + 1u;
        }
    }

    return f32(occluded_samples) / f32(samples - 1u);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let light_pos = params.light_params.xy;
    let light_radius = params.light_params.z;
    let samples = u32(params.light_params.w);
    let num_edges = u32(params.edge_params.x);
    let softness = params.edge_params.y;

    // Distance from fragment to light
    let to_light = light_pos - in.world_pos;
    let dist = length(to_light);

    // Outside light radius - no shadow contribution
    if dist > light_radius {
        discard;
    }

    // Count edge intersections from this pixel to the light
    let intersections = count_intersections(in.world_pos, light_pos, num_edges);

    // If any edge blocks the ray from pixel to light, pixel is in shadow
    var shadow = 0.0;
    if intersections > 0u {
        shadow = 1.0;
    }

    // Apply shadow intensity - multiply to ensure full light subtraction regardless of intensity
    let final_shadow = shadow * 10.0;

    // Output white to subtract light from the buffer
    // (reverse-subtract blending: dst = dst - src)
    return vec4<f32>(final_shadow, final_shadow, final_shadow, 1.0);
}
