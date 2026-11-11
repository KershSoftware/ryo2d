// Shadow maps (`lighting::ShadowAtlas`): each casting edge of a shadowed
// light, once per view it crosses, drawn into that view's row of the depth
// atlas as the distance from the light over its radius.
//
// A view is a 90 degree square projection around the light, looking along
// +x once turned into its frame: a point's place along the row is the
// tangent of its angle within the view (clip x = local y, w = local x), so
// the GPU's clipping trims the edge to the view's two 45 degree ends.

struct Atlas {
    // Width and height in texels
    size: vec4<f32>,
}

@group(0) @binding(0) var<uniform> atlas: Atlas;

struct EdgeInput {
    @location(0) a: vec2<f32>,
    @location(1) b: vec2<f32>,
    @location(2) light: vec2<f32>,
    @location(3) radius: f32,
    // The light's slot times four plus the view (0 right, 1 down, 2 left, 3 up)
    @location(4) row: u32,
}

struct EdgeOutput {
    @builtin(position) position: vec4<f32>,
    // Interpolated correctly under the projection: the edge's point at
    // this texel's angle
    @location(0) world: vec2<f32>,
    @location(1) @interpolate(flat) light: vec2<f32>,
    @location(2) @interpolate(flat) radius: f32,
}

// Points nearer the light than this along the view (px) are clipped away,
// so no division by zero or sign flip reaches the rasteriser
const NEAR: f32 = 0.5;
// How far each end of an edge reaches past itself along the row, in texels,
// so the joints of touching edges leave no gap
const END_PAD: f32 = 0.25;

// `p` (relative to the light) turned into view `view`'s frame
fn in_view(p: vec2<f32>, view: u32) -> vec2<f32> {
    switch view {
        case 1u: { return vec2<f32>(p.y, -p.x); }
        case 2u: { return vec2<f32>(-p.x, -p.y); }
        case 3u: { return vec2<f32>(-p.y, p.x); }
        default: { return p; }
    }
}

// Six vertices an instance: two triangles, (a below, b below, a above) and
// (b below, b above, a above), half a texel above and below the row's centre
@vertex
fn vs_edge(in: EdgeInput, @builtin(vertex_index) index: u32) -> EdgeOutput {
    var out: EdgeOutput;
    out.light = in.light;
    out.radius = in.radius;
    let view = in.row % 4u;
    var a = in_view(in.a - in.light, view);
    var b = in_view(in.b - in.light, view);
    var world_a = in.a;
    var world_b = in.b;
    if a.x < NEAR && b.x < NEAR {
        // Wholly behind the near plane: every vertex at one point, no area
        out.position = vec4<f32>(2.0, 2.0, 0.0, 1.0);
        out.world = in.light;
        return out;
    }
    // An end behind the near plane moves to it along the edge
    if a.x < NEAR {
        let t = (NEAR - a.x) / (b.x - a.x);
        a = mix(a, b, t);
        world_a = mix(world_a, world_b, t);
    } else if b.x < NEAR {
        let t = (NEAR - b.x) / (a.x - b.x);
        b = mix(b, a, t);
        world_b = mix(world_b, world_a, t);
    }
    let along_a = a.y / a.x;
    let along_b = b.y / b.x;
    // A texel is 2 / width across in clip space; each end reaches outward
    let pad = 2.0 * END_PAD / atlas.size.x;
    var out_a = sign(along_a - along_b);
    if out_a == 0.0 {
        out_a = -1.0;
    }
    let centre = 1.0 - 2.0 * (f32(in.row) + 0.5) / atlas.size.y;
    let half = 1.0 / atlas.size.y;
    let at_b = index == 1u || index == 3u || index == 4u;
    let above = index == 2u || index == 4u || index == 5u;
    let side = select(-1.0, 1.0, above);
    let x = select(along_a + out_a * pad, along_b - out_a * pad, at_b);
    let w = select(a.x, b.x, at_b);
    out.position = vec4<f32>(x * w, (centre + side * half) * w, 0.0, w);
    out.world = select(world_a, world_b, at_b);
    return out;
}

@fragment
fn fs_edge(in: EdgeOutput) -> @builtin(frag_depth) f32 {
    return clamp(distance(in.world, in.light) / max(in.radius, 1e-6), 0.0, 1.0);
}
