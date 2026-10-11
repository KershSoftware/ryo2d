// Fullscreen quad shader for rendering the game texture to screen
// with letterboxing/pillarboxing support.

struct ViewportUniform {
    // x: scale_x, y: scale_y, z: offset_x (unused), w: offset_y (unused)
    transform: vec4<f32>,
};

@group(0) @binding(0) var t_game: texture_2d<f32>;
@group(0) @binding(1) var s_game: sampler;
@group(0) @binding(2) var<uniform> viewport: ViewportUniform;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) tex_coord: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    // Generate fullscreen quad vertices (2 triangles = 6 vertices)
    // Vertex order: 0-1-2, 2-1-3
    //   0 --- 1
    //   |   / |
    //   | /   |
    //   2 --- 3
    
    var positions = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, 1.0),   // 0: top-left
        vec2<f32>(1.0, 1.0),    // 1: top-right
        vec2<f32>(-1.0, -1.0),  // 2: bottom-left
        vec2<f32>(-1.0, -1.0),  // 2: bottom-left
        vec2<f32>(1.0, 1.0),    // 1: top-right
        vec2<f32>(1.0, -1.0),   // 3: bottom-right
    );
    
    var tex_coords = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0),    // top-left
        vec2<f32>(1.0, 0.0),    // top-right
        vec2<f32>(0.0, 1.0),    // bottom-left
        vec2<f32>(0.0, 1.0),    // bottom-left
        vec2<f32>(1.0, 0.0),    // top-right
        vec2<f32>(1.0, 1.0),    // bottom-right
    );
    
    var out: VertexOutput;
    
    // Apply viewport scaling
    let pos = positions[vertex_index];
    out.position = vec4<f32>(
        pos.x * viewport.transform.x,
        pos.y * viewport.transform.y,
        0.0,
        1.0
    );
    out.tex_coord = tex_coords[vertex_index];
    
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(t_game, s_game, in.tex_coord);
}
