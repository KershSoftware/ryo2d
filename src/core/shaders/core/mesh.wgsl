// Meshes: triangles built on the CPU (`Vertex2D`) of an image, drawn as a
// sprite of that image is. The fragment stages sample and tint with the
// sprite's own functions (`sample_masked`, `finish`), so a quad of an image
// drawn as a mesh has the sprite's pixels.
//
// A vertex carries its UV as a fraction of the image, the image's slot on
// its page (`slot`, set when the mesh is placed) and the region of the
// image it may read, as fractions of the image too. The vertex stage maps
// both into the slot, as a sprite's instance is mapped on the CPU: a UV
// outside the region reads nothing, so neither another region of the image
// nor the slot's neighbours on the page ever show.
//
// The page holds raw sRGB bytes (an image's, never a render target's). The
// tint is a sprite's: sRGB with straight alpha. Output is linear,
// premultiplied by alpha, like every batch shader. Vertices are placed where
// they are given: the pixel-art snap is a quad's, not a triangle's.

@group(1) @binding(0) var mesh_texture: texture_2d<f32>;
@group(1) @binding(1) var mesh_sampler: sampler;

struct MeshVertex {
    @location(0) position: vec2<f32>,
    // A fraction of the image
    @location(1) uv: vec2<f32>,
    @location(2) tint: vec4<f32>,
    // The region of the image it may read: UV min and max, as fractions of
    // the image
    @location(3) region: vec4<f32>,
    // x: emission - 1 (a zeroed lane means 1, as a sprite's does)
    @location(4) extra: vec2<f32>,
    // The image's slot on its page: UV min and max
    @location(5) slot: vec4<f32>,
};

struct MeshOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) tint: vec4<f32>,
    @location(2) @interpolate(flat) uv_bounds: vec4<f32>,
    @location(3) @interpolate(flat) emission: f32,
};

@vertex
fn vs_main(vertex: MeshVertex) -> MeshOutput {
    var out: MeshOutput;
    out.position = globals.view_proj * vec4<f32>(vertex.position, 0.0, 1.0);

    // Into the slot, as `place_in_slot` puts a sprite's UVs and bounds: the
    // region clipped to the slot
    let origin = vertex.slot.xy;
    let size = vertex.slot.zw - vertex.slot.xy;
    out.uv = origin + vertex.uv * size;
    let one = origin + vertex.region.xy * size;
    let other = origin + vertex.region.zw * size;
    out.uv_bounds = vec4<f32>(
        max(min(one, other), vertex.slot.xy),
        min(max(one, other), vertex.slot.zw),
    );

    out.tint = vertex.tint;
    out.emission = vertex.extra.x + 1.0;
    return out;
}

// The image at the fragment's UV, transparent outside its region. The
// derivatives are taken here, first thing in each entry, where control flow
// is uniform
fn sample_mesh(in: MeshOutput) -> vec4<f32> {
    let ddx = dpdx(in.uv);
    let ddy = dpdy(in.uv);
    return sample_masked(mesh_texture, mesh_sampler, in.uv, in.uv_bounds, ddx, ddy);
}

// As the sprite's `fs_base`
@fragment
fn fs_mesh(in: MeshOutput) -> @location(0) vec4<f32> {
    return ui_out(finish(sample_mesh(in), in.tint, in.emission));
}

// As the sprite's `fs_base_premultiplied`: the image's colour is already
// multiplied by its alpha
@fragment
fn fs_mesh_premultiplied(in: MeshOutput) -> @location(0) vec4<f32> {
    return ui_out(finish_premultiplied(sample_mesh(in), in.tint, in.emission));
}
