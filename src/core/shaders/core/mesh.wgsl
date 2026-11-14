// Meshes: an entity's own triangles of an image, drawn as a sprite of the
// image is. This is their vertex stage, composed after `sprite.wgsl`; they
// have no fragment stage of their own. A mesh's vertex is where it draws
// and its UV across the mesh's rectangle of the image, laid out as the unit
// quad's corner is (`VertexInput`), and one sprite instance a mesh carries
// the rest (`InstanceInput`: that rectangle and its bounds in the image's
// atlas slot, the tint, the effect lanes, the emission). So the sprite's
// fragment entries, and a game's own sprite shader, shade a mesh as they
// shade a quad.
//
// The instance's transform is not read but for its emission lane: a mesh's
// vertices are placed where they are given, and the pixel-art snap, which
// is a quad's (`quad_corner`), never moves them.

@vertex
fn vs_mesh(vertex: VertexInput, instance: InstanceInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = globals.view_proj * vec4<f32>(vertex.position, 0.0, 1.0);

    // As `vs_main` fills them
    out.uv = mix(instance.uv_rect.xy, instance.uv_rect.zw, vertex.uv);
    out.tint = instance.tint;
    out.effect_params = instance.effect_params;
    out.effect_extra = instance.effect_extra;
    out.uv_bounds = instance.uv_bounds;
    out.emission = instance.transform0.w + 1.0;

    return out;
}
