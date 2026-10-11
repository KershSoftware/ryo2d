// Ambient Occlusion post-process shader
// Darkens corners and crevices based on edge detection for 2D scenes

struct EffectParams {
    color: vec4<f32>,           // Not used (16 bytes)
    params: vec4<f32>,          // x: intensity, y: radius, z: bias (16 bytes)
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

// Calculate luminance from RGB
fn luminance(color: vec3<f32>) -> f32 {
    return dot(color, vec3<f32>(0.299, 0.587, 0.114));
}

// Check if neighbor has edge (luminance difference exceeds bias)
fn has_edge(uv: vec2<f32>, center_lum: f32, bias: f32) -> bool {
    let neighbor = textureSample(t_input, s_input, uv);
    let neighbor_lum = luminance(neighbor.rgb);
    return abs(neighbor_lum - center_lum) > bias;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let scene_color = textureSample(t_input, s_input, in.tex_coord);

    let intensity = effect.params.x;
    let radius = effect.params.y;
    let bias = effect.params.z;

    // Pixel size in UV space
    let pixel_size = vec2<f32>(1.0 / effect.time_resolution.y, 1.0 / effect.time_resolution.z);
    let step = radius * pixel_size;

    // Sample center luminance
    let center_lum = luminance(scene_color.rgb);

    // 8 directions for sampling (cardinal + diagonal) - unrolled for WGSL compatibility
    // Direction 0: Right (1, 0)
    // Direction 1: Bottom-right (0.707, 0.707)
    // Direction 2: Bottom (0, 1)
    // Direction 3: Bottom-left (-0.707, 0.707)
    // Direction 4: Left (-1, 0)
    // Direction 5: Top-left (-0.707, -0.707)
    // Direction 6: Top (0, -1)
    // Direction 7: Top-right (0.707, -0.707)

    // Detect edges in each direction (unrolled loop)
    var edge_mask: u32 = 0u;

    // Direction 0: Right
    if has_edge(in.tex_coord + vec2<f32>(1.0, 0.0) * step, center_lum, bias) {
        edge_mask = edge_mask | 1u;
    }
    // Direction 1: Bottom-right
    if has_edge(in.tex_coord + vec2<f32>(0.707, 0.707) * step, center_lum, bias) {
        edge_mask = edge_mask | 2u;
    }
    // Direction 2: Bottom
    if has_edge(in.tex_coord + vec2<f32>(0.0, 1.0) * step, center_lum, bias) {
        edge_mask = edge_mask | 4u;
    }
    // Direction 3: Bottom-left
    if has_edge(in.tex_coord + vec2<f32>(-0.707, 0.707) * step, center_lum, bias) {
        edge_mask = edge_mask | 8u;
    }
    // Direction 4: Left
    if has_edge(in.tex_coord + vec2<f32>(-1.0, 0.0) * step, center_lum, bias) {
        edge_mask = edge_mask | 16u;
    }
    // Direction 5: Top-left
    if has_edge(in.tex_coord + vec2<f32>(-0.707, -0.707) * step, center_lum, bias) {
        edge_mask = edge_mask | 32u;
    }
    // Direction 6: Top
    if has_edge(in.tex_coord + vec2<f32>(0.0, -1.0) * step, center_lum, bias) {
        edge_mask = edge_mask | 64u;
    }
    // Direction 7: Top-right
    if has_edge(in.tex_coord + vec2<f32>(0.707, -0.707) * step, center_lum, bias) {
        edge_mask = edge_mask | 128u;
    }

    // Count adjacent edge pairs (indicates corners/crevices)
    // A corner is detected when two adjacent directions both have edges
    var adjacent_count: u32 = 0u;

    // Check pairs: 0-1, 1-2, 2-3, 3-4, 4-5, 5-6, 6-7, 7-0
    if ((edge_mask & 1u) != 0u) && ((edge_mask & 2u) != 0u) { adjacent_count += 1u; }
    if ((edge_mask & 2u) != 0u) && ((edge_mask & 4u) != 0u) { adjacent_count += 1u; }
    if ((edge_mask & 4u) != 0u) && ((edge_mask & 8u) != 0u) { adjacent_count += 1u; }
    if ((edge_mask & 8u) != 0u) && ((edge_mask & 16u) != 0u) { adjacent_count += 1u; }
    if ((edge_mask & 16u) != 0u) && ((edge_mask & 32u) != 0u) { adjacent_count += 1u; }
    if ((edge_mask & 32u) != 0u) && ((edge_mask & 64u) != 0u) { adjacent_count += 1u; }
    if ((edge_mask & 64u) != 0u) && ((edge_mask & 128u) != 0u) { adjacent_count += 1u; }
    if ((edge_mask & 128u) != 0u) && ((edge_mask & 1u) != 0u) { adjacent_count += 1u; }

    // Calculate AO factor based on corner detection
    // More adjacent edge pairs = more corner-like = more darkening
    let corner_factor = f32(adjacent_count) / 8.0;

    // Apply smooth darkening
    let ao = corner_factor * intensity;
    let darkened = scene_color.rgb * (1.0 - ao);

    return vec4<f32>(darkened, scene_color.a);
}
