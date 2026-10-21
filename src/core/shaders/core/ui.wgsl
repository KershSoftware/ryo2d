// The UI's one pipeline: every UI instance (rects, glyphs, sprites, line
// triangles) through one vertex and one fragment entry. The fragment bodies
// are the world shaders' (include files); the clip is tested last, so it
// passes exactly the pixels a scissor would.

@group(1) @binding(0) var mask_page: texture_2d<f32>;
@group(1) @binding(1) var color_page: texture_2d<f32>;
@group(1) @binding(2) var msdf_page: texture_2d<f32>;
@group(1) @binding(3) var sprite_page: texture_2d<f32>;
@group(1) @binding(4) var linear_sampler: sampler;
@group(1) @binding(5) var nearest_sampler: sampler;

// What an instance draws (`UiKind`)
const KIND_RECT: u32 = 0u;
const KIND_MASK: u32 = 1u;
const KIND_COLOR: u32 = 2u;
const KIND_MSDF: u32 = 3u;
const KIND_SPRITE: u32 = 4u;
const KIND_SPRITE_LINEAR: u32 = 5u;
const KIND_LINE: u32 = 6u;
// Sample with the nearest sampler
const FLAG_NEAREST: u32 = 1u;

struct QuadVertex {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
}

// A quad's lanes are a sprite instance's: transform rows (a0-a2, emission - 1
// in a0.w), uv rect, tint, effect params, effect extra and uv bounds. A line
// triangle's are its corners' position and uv (a0-a2), their colours (a3-a5)
// and the line's params (a6).
struct UiInstanceIn {
    @location(2) a0: vec4<f32>,
    @location(3) a1: vec4<f32>,
    @location(4) a2: vec4<f32>,
    @location(5) a3: vec4<f32>,
    @location(6) a4: vec4<f32>,
    @location(7) a5: vec4<f32>,
    @location(8) a6: vec4<f32>,
    @location(9) a7: vec4<f32>,
    // x: x0 | x1 << 16, y: y0 | y1 << 16, z: kind, w: flags | page << 16
    @location(10) packed: vec4<u32>,
}

struct UiOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    // A line's colour, interpolated across its triangle (linear, straight)
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) tint: vec4<f32>,
    @location(3) @interpolate(flat) params: vec4<f32>,
    @location(4) @interpolate(flat) extra: vec4<f32>,
    @location(5) @interpolate(flat) bounds: vec4<f32>,
    @location(6) @interpolate(flat) emission: f32,
    // x0, y0, x1, y1
    @location(7) @interpolate(flat) clip: vec4<u32>,
    @location(8) @interpolate(flat) kind: u32,
    @location(9) @interpolate(flat) flags: u32,
}

@vertex
fn vs_ui(vertex: QuadVertex, instance: UiInstanceIn, @builtin(vertex_index) index: u32) -> UiOut {
    var out: UiOut;
    let kind = instance.packed.z;
    out.kind = kind;
    out.flags = instance.packed.w & 0xFFFFu;
    out.clip = vec4<u32>(
        instance.packed.x & 0xFFFFu,
        instance.packed.y & 0xFFFFu,
        instance.packed.x >> 16u,
        instance.packed.y >> 16u,
    );
    if kind == KIND_LINE {
        // Corners 0, 1, 2 make the triangle; the quad's other three
        // vertices repeat corner 2, so its second triangle has no area
        let corner = min(index % 6u, 2u);
        var at = instance.a2;
        var color = instance.a5;
        if corner == 0u {
            at = instance.a0;
            color = instance.a3;
        } else if corner == 1u {
            at = instance.a1;
            color = instance.a4;
        }
        // As line.wgsl's vs_main for a solid UI line: no displacement, no snap
        out.position = globals.view_proj * vec4<f32>(at.xy, 0.0, 1.0);
        out.uv = at.zw;
        out.color = color;
        out.params = instance.a6;
        return out;
    }
    // Quads: sprite.wgsl's vs_main, the same arithmetic in the same order

    // Apply 2D affine transform from instance
    let transform = mat3x3<f32>(
        instance.a0.xyz,
        instance.a1.xyz,
        instance.a2.xyz
    );

    let local_pos = vec3<f32>(vertex.position, 1.0);
    let world_pos = transform * local_pos;

    // Through the camera
    var clip = globals.view_proj * vec4<f32>(world_pos.xy, 0.0, 1.0);
    // Pixel art: a quad axis-aligned on screen (itself and the camera
    // unturned) rounds its top-left and size to whole pixels once, so it
    // keeps its width as it moves; under a turned camera it draws unsnapped
    let unturned = abs(instance.a0.y) < 1e-5 && abs(instance.a1.x) < 1e-5;
    let camera_unturned = abs(globals.view_proj[0][1]) < 1e-5 && abs(globals.view_proj[1][0]) < 1e-5;
    let axis_aligned = unturned && camera_unturned;
    if globals.snap > 0.5 && axis_aligned {
        let p0 = to_pixel((transform * vec3<f32>(-0.5, -0.5, 1.0)).xy);
        let p1 = to_pixel((transform * vec3<f32>(0.5, 0.5, 1.0)).xy);
        let origin = round(min(p0, p1));
        let size = round(abs(p1 - p0));
        // A corner that maps to the larger pixel coordinate counts from the far side
        let along = select(vertex.position + 0.5, 0.5 - vertex.position, p1 < p0);
        clip = from_pixel(origin + along * size);
    }
    out.position = clip;

    // Interpolate UVs from uv_rect
    out.uv = mix(instance.a3.xy, instance.a3.zw, vertex.uv);

    out.tint = instance.a4;
    out.params = instance.a5;
    out.extra = instance.a6;
    out.bounds = instance.a7;
    out.emission = instance.a0.w + 1.0;

    return out;
}

// A glyph page's texel at `uv` with the sampler the instance asks for
fn page_texel(
    page: texture_2d<f32>,
    uv: vec2<f32>,
    ddx: vec2<f32>,
    ddy: vec2<f32>,
    nearest: bool,
) -> vec4<f32> {
    if nearest {
        return textureSampleGrad(page, nearest_sampler, uv, ddx, ddy);
    }
    return textureSampleGrad(page, linear_sampler, uv, ddx, ddy);
}

// The sprite page at `uv`, transparent outside `bounds`, with the sampler
// the instance asks for
fn sprite_texel(
    uv: vec2<f32>,
    bounds: vec4<f32>,
    ddx: vec2<f32>,
    ddy: vec2<f32>,
    nearest: bool,
) -> vec4<f32> {
    if nearest {
        return sample_masked(sprite_page, nearest_sampler, uv, bounds, ddx, ddy);
    }
    return sample_masked(sprite_page, linear_sampler, uv, bounds, ddx, ddy);
}

@fragment
fn fs_ui(in: UiOut) -> @location(0) vec4<f32> {
    // Every derivative first, where control flow is uniform, each as its
    // world entry takes it: the uv's (sprites, and the glyphs' sampling),
    // the MSDF body's fwidth(uv), the rect's fwidth(p) and the line's
    // across-width rate
    let uv_ddx = dpdx(in.uv);
    let uv_ddy = dpdy(in.uv);
    let uv_width = fwidth(in.uv);
    let rect_size = max(in.params.zw, vec2<f32>(1e-4));
    let rect_p = (in.uv - vec2<f32>(0.5)) * rect_size;
    let rect_width = fwidth(rect_p);
    let line_aa = fwidth(abs(in.uv.y - 0.5) * 2.0);
    let nearest = (in.flags & FLAG_NEAREST) != 0u;

    var color = vec4<f32>(0.0);
    switch in.kind {
        // fs_ui_rect
        case KIND_RECT: {
            color = ui_rect(rect_p, rect_size, rect_width, in.tint, in.params, in.extra);
        }
        // fs_text_mask
        case KIND_MASK: {
            let coverage = page_texel(mask_page, in.uv, uv_ddx, uv_ddy, nearest).r;
            color = glyph_mask(coverage, in.tint, in.emission);
        }
        // fs_text_color
        case KIND_COLOR: {
            let texel = page_texel(color_page, in.uv, uv_ddx, uv_ddy, nearest);
            color = glyph_color(texel, in.tint, in.emission);
        }
        // fs_text_msdf
        case KIND_MSDF: {
            let texel = page_texel(msdf_page, in.uv, uv_ddx, uv_ddy, nearest).rgb;
            let texels = vec2<f32>(textureDimensions(msdf_page, 0));
            color = glyph_msdf(texel, texels, uv_width, in.tint, in.params, in.extra, in.emission);
        }
        // fs_base
        case KIND_SPRITE: {
            let texel = sprite_texel(in.uv, in.bounds, uv_ddx, uv_ddy, nearest);
            color = finish(texel, in.tint, in.emission);
        }
        // fs_base_linear
        case KIND_SPRITE_LINEAR: {
            let texel = sprite_texel(in.uv, in.bounds, uv_ddx, uv_ddy, nearest);
            color = finish_linear(texel, in.tint, in.emission);
        }
        // line.wgsl's fs_main
        case KIND_LINE: {
            color = styled(in.color, in.uv, in.params, line_aa);
        }
        default: {}
    }
    // The scissor test: the pixel's centre inside [x0, x1) x [y0, y1). Under
    // the premultiplied blend a zero output leaves the image as it was
    let px = in.position.xy;
    let inside = px.x >= f32(in.clip.x) && px.x < f32(in.clip.z)
        && px.y >= f32(in.clip.y) && px.y < f32(in.clip.w);
    if !inside {
        return vec4<f32>(0.0);
    }
    return ui_out(color);
}
