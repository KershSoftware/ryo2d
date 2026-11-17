// The UI's one pipeline: every UI instance (rects, glyphs, sprites, line
// pieces) through one vertex and one fragment entry. The fragment bodies
// are the world shaders' (include files); the clip is tested last, so it
// passes exactly the pixels a scissor would. An instance in a scroll slot
// is moved by the slot's offset and drawn under its clip.

@group(1) @binding(0) var mask_page: texture_2d<f32>;
@group(1) @binding(1) var color_page: texture_2d<f32>;
@group(1) @binding(2) var msdf_page: texture_2d<f32>;
@group(1) @binding(3) var sprite_page: texture_2d<f32>;
@group(1) @binding(4) var linear_sampler: sampler;
@group(1) @binding(5) var nearest_sampler: sampler;

// One scroll slot (`SlotUniform`): its offset in UI units, added to an
// instance's corners, and in whole pixels, moving an instance's own clip;
// its clip in UI image pixels (x0, y0, x1, y1). Slot 0 is no scroll area.
struct Slot {
    offset: vec2<f32>,
    pixels: vec2<i32>,
    clip: vec4<u32>,
}

@group(2) @binding(0) var<uniform> slots: array<Slot, 256>;

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
// The instance's clip is its own, relative to its slot
const FLAG_OWN_CLIP: u32 = 2u;
// Where an instance's slot sits in its flags (8 bits, below the glyph page)
const SLOT_SHIFT: u32 = 8u;
// Where a collapsed instance's vertices all go: one point, outside the image
const COLLAPSED: vec4<f32> = vec4<f32>(-2.0, -2.0, 0.0, 1.0);

struct QuadVertex {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
}

// A quad's lanes are a sprite instance's: transform rows (a0-a2, emission - 1
// in a0.w), uv rect, tint, effect params, effect extra and uv bounds. A
// line's (`UiInstance::line_piece`) are two triangles of one of its pieces
// and the points they are built from: the window's six positions (a0-a2),
// their `u` (a3, a4.xy), the half width and a two-point line's half length
// between its caps (a4.zw), the line's params (a5), its sRGB colour as two
// exact 16-bit halves, the miter limit and the window's bits with the
// line's edge (a6), and the offset every point is moved by (a7.xy).
struct UiInstanceIn {
    @location(2) a0: vec4<f32>,
    @location(3) a1: vec4<f32>,
    @location(4) a2: vec4<f32>,
    @location(5) a3: vec4<f32>,
    @location(6) a4: vec4<f32>,
    @location(7) a5: vec4<f32>,
    @location(8) a6: vec4<f32>,
    @location(9) a7: vec4<f32>,
    // x: x0 | x1 << 16, y: y0 | y1 << 16, z: kind, w: flags | slot << 8 |
    // page << 16
    @location(10) packed: vec4<u32>,
}

struct UiOut {
    // Invariant, so copies of a line's place in its pieces' instances land
    // on the same spot, as the world's do
    @builtin(position) @invariant position: vec4<f32>,
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
    // A line's measures (`LineFringe`); its `k`, half length between caps,
    // vertex flags and whether it draws Hard ride in `extra`
    @location(10) measure: vec3<f32>,
    @location(11) across_axis: f32,
}

// The clip an instance draws under: its own in slot 0; in a slot the
// slot's, met with the instance's own moved by the slot's offset when it has
// one (`ClipRect::in_slot`)
fn clip_in_slot(own: vec4<u32>, slot: u32, flags: u32) -> vec4<u32> {
    if slot == 0u {
        return own;
    }
    let clip = slots[slot].clip;
    if (flags & FLAG_OWN_CLIP) == 0u {
        return clip;
    }
    let by = vec4<i32>(slots[slot].pixels, slots[slot].pixels);
    let moved = vec4<u32>(max(vec4<i32>(own) + by, vec4<i32>(0)));
    return vec4<u32>(max(clip.xy, moved.xy), min(clip.zw, moved.zw));
}

// Whether a primitive inside the box `lo`..`hi` (image pixels) covers no
// pixel `clip` passes: the box lies wholly outside it, or it is empty. No
// pixel centre outside the box is covered, so collapsing it changes nothing.
fn outside_clip(lo: vec2<f32>, hi: vec2<f32>, clip: vec4<u32>) -> bool {
    let c = vec4<f32>(clip);
    return hi.x <= c.x || lo.x >= c.z || hi.y <= c.y || lo.y >= c.w || c.z <= c.x || c.w <= c.y;
}

// A line instance's window bits: bits 0-5 the slots holding a point, 6-11
// those holding a closed line's first point (its seam), then the piece's
// kind, which pair of its triangles and the line's edge
const LINE_KIND_SHIFT: u32 = 12u;
const LINE_PAIR_SHIFT: u32 = 14u;
const LINE_EDGE_SHIFT: u32 = 18u;

// A line instance's points about its piece's point, from slot `first` (the
// window about a vertex's own point starts where `line_piece_corner` says),
// each moved by `offset`
fn ui_line_window(instance: UiInstanceIn, first: i32, offset: vec2<f32>) -> LineWindow {
    var xy = array<vec2<f32>, 6>(
        instance.a0.xy, instance.a0.zw, instance.a1.xy, instance.a1.zw, instance.a2.xy, instance.a2.zw,
    );
    var us = array<f32, 6>(instance.a3.x, instance.a3.y, instance.a3.z, instance.a3.w, instance.a4.x, instance.a4.y);
    let bits = u32(instance.a6.w);
    let colour = unpack4x8unorm(u32(instance.a6.x) | (u32(instance.a6.y) << 16u));
    var w: LineWindow;
    w.present = 0u;
    w.seam = 0u;
    for (var slot = 0; slot < 5; slot++) {
        let at = first + slot;
        w.p[slot] = LinePt(xy[at] + offset, instance.a4.z, us[at], colour);
        w.present |= ((bits >> u32(at)) & 1u) << u32(slot);
        w.seam |= ((bits >> u32(6 + at)) & 1u) << u32(slot);
    }
    return w;
}

// A line instance's vertex: two triangles of its piece, built by the
// world's piece functions from its points, each moved by the line's offset
// and its slot's (`at + by`, as the CPU moves a line placed absolute, so
// both build from the same points), pushed a pixel of the UI image out and
// measured as the world's are (`line_pieces.wgsl`, `line_cover.wgsl`: no
// displacement, no snap; Hard by its edge or, as its target's, on the
// pixel-art target, where the UI camera's `snap` is set). An instance wholly outside its clip, its points'
// box grown by how far a vertex reaches from them (`ui_line_reach` on the
// CPU), is collapsed
fn ui_line(instance: UiInstanceIn, index: u32, by: vec2<f32>, clip: vec4<u32>, base: UiOut) -> UiOut {
    var out = base;
    let bits = u32(instance.a6.w);
    let kind = (bits >> LINE_KIND_SHIFT) & 3u;
    let pair = (bits >> LINE_PAIR_SHIFT) & 0xFu;
    let ppu = globals.px_per_unit;
    let half_width = instance.a4.z;
    let miter_limit = instance.a6.z;
    let offset = instance.a7.xy + by;

    // The box of the points its vertices lie about: a segment's ends and
    // their neighbours (slots 1 to 4), a join's point and its neighbours
    var xy = array<vec2<f32>, 6>(
        instance.a0.xy, instance.a0.zw, instance.a1.xy, instance.a1.zw, instance.a2.xy, instance.a2.zw,
    );
    let last = select(3, 4, kind == PIECE_SEGMENT);
    var lo = xy[2];
    var hi = xy[2];
    for (var slot = 1; slot <= last; slot++) {
        if ((bits >> u32(slot)) & 1u) == 1u {
            lo = min(lo, xy[slot]);
            hi = max(hi, xy[slot]);
        }
    }
    var pixel = 0.0;
    if ppu > 0.0 {
        pixel = 1.0 / ppu;
    }
    let reach = vec2<f32>(miter_limit * (max(half_width, 0.0) + pixel));
    let p0 = pixel_of(globals.view_proj * vec4<f32>(lo + offset - reach, 0.0, 1.0));
    let p1 = pixel_of(globals.view_proj * vec4<f32>(hi + offset + reach, 0.0, 1.0));
    if outside_clip(min(p0, p1), max(p0, p1), clip) {
        out.position = COLLAPSED;
        return out;
    }

    let tri = pair * 2u + (index % 6u) / 3u;
    let corner = index % 3u;
    let w = ui_line_window(instance, line_piece_corner(kind, tri, corner), offset);
    let look = LineLook(JOIN_MITER, CAP_BUTT, miter_limit);
    let vertex = line_piece_vertex(w, look, kind, tri, corner);
    let hard = line_hard((bits >> LINE_EDGE_SHIFT) & 3u, globals.snap);
    let axis = line_axis(vertex.normal, globals.view_proj, globals.resolution);
    var fringe = line_fringe(vertex, vertex.position, ppu, axis.major);
    out.position = globals.view_proj * vec4<f32>(fringe.pos, 0.0, 1.0);
    if hard {
        let gridded = line_on_grid(out.position, globals.resolution);
        out.position = gridded.clip;
        fringe = line_moved(fringe, vertex.flags, gridded.moved, axis, globals.view_proj, globals.resolution, ppu);
    }
    out.color = vec4<f32>(srgb_to_linear(vertex.color.rgb), vertex.color.a);
    out.uv = fringe.uv;
    out.params = instance.a5;
    out.measure = fringe.measure;
    out.extra = vec4<f32>(vertex.k, instance.a4.w, f32(vertex.flags), select(0.0, 1.0, hard));
    out.across_axis = fringe.across_axis;
    return out;
}

@vertex
fn vs_ui(vertex: QuadVertex, instance: UiInstanceIn, @builtin(vertex_index) index: u32) -> UiOut {
    var out: UiOut;
    let kind = instance.packed.z;
    let flags = instance.packed.w & 0xFFFFu;
    out.kind = kind;
    out.flags = flags;
    // Its slot's offset moves it (slot 0's is zero)
    let slot = (flags >> SLOT_SHIFT) & 0xFFu;
    let by = slots[slot].offset;
    let own = vec4<u32>(
        instance.packed.x & 0xFFFFu,
        instance.packed.y & 0xFFFFu,
        instance.packed.x >> 16u,
        instance.packed.y >> 16u,
    );
    let clip = clip_in_slot(own, slot, flags);
    out.clip = clip;
    if kind == KIND_LINE {
        return ui_line(instance, index, by, clip, out);
    }
    // Quads: where sprite.wgsl's vs_main puts them (quad.wgsl), moved by
    // the slot's offset before the camera and the pixel-art snap; one wholly
    // outside its clip is collapsed
    let c0 = pixel_of(quad_corner(instance.a0, instance.a1, instance.a2, vec2<f32>(-0.5, -0.5), by));
    let c1 = pixel_of(quad_corner(instance.a0, instance.a1, instance.a2, vec2<f32>(0.5, -0.5), by));
    let c2 = pixel_of(quad_corner(instance.a0, instance.a1, instance.a2, vec2<f32>(-0.5, 0.5), by));
    let c3 = pixel_of(quad_corner(instance.a0, instance.a1, instance.a2, vec2<f32>(0.5, 0.5), by));
    if outside_clip(min(min(c0, c1), min(c2, c3)), max(max(c0, c1), max(c2, c3)), clip) {
        out.position = COLLAPSED;
        return out;
    }
    out.position = quad_corner(instance.a0, instance.a1, instance.a2, vertex.position, by);

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
    // and the MSDF body's fwidth(uv)
    let uv_ddx = dpdx(in.uv);
    let uv_ddy = dpdy(in.uv);
    let uv_width = fwidth(in.uv);
    let rect_size = max(in.params.zw, vec2<f32>(1e-4));
    let rect_p = (in.uv - vec2<f32>(0.5)) * rect_size;
    let nearest = (in.flags & FLAG_NEAREST) != 0u;

    var color = vec4<f32>(0.0);
    switch in.kind {
        // fs_ui_rect
        case KIND_RECT: {
            color = ui_rect(rect_p, rect_size, 1.0 / max(globals.px_per_unit, 1e-4), in.tint, in.params, in.extra);
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
        // line.wgsl's fs_main: covered by its distances in pixels
        case KIND_LINE: {
            let m = LineMeasure(in.measure, in.across_axis, u32(in.extra.z), in.extra.x, in.extra.y, in.extra.w > 0.5);
            let pixel = line_pixel(m, in.params, globals.px_per_unit);
            color = styled(in.color, pixel.shape, in.params, pixel.cover);
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
