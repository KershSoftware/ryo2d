// Text and UI fragment entries: composed after sprite.wgsl (its vertex
// stage, VertexOutput and group 1 texture)

// A raster glyph: tint times coverage, premultiplied
@fragment
fn fs_text_mask(in: VertexOutput) -> @location(0) vec4<f32> {
    let coverage = textureSample(sprite_texture, sprite_sampler, in.uv).r;
    let a = coverage * in.tint.a;
    return ui_out(vec4<f32>(srgb_to_linear(in.tint.rgb) * a * in.emission, a));
}

// A colour glyph (sRGB bytes, straight alpha) times the tint
@fragment
fn fs_text_color(in: VertexOutput) -> @location(0) vec4<f32> {
    let texel = textureSample(sprite_texture, sprite_sampler, in.uv);
    let a = texel.a * in.tint.a;
    return ui_out(vec4<f32>(srgb_to_linear(texel.rgb * in.tint.rgb) * a * in.emission, a));
}

fn ryo_median3(c: vec3<f32>) -> f32 {
    return max(min(c.r, c.g), min(max(c.r, c.g), c.b));
}

// An MSDF glyph. effect_params: x = mode (0 fill with an optional outline,
// 1 a soft layer for shadows and glow), y = outline width or soft spread
// (base pixels), z = softness (base pixels), w = distance range (base
// pixels). effect_extra = outline colour.
@fragment
fn fs_text_msdf(in: VertexOutput) -> @location(0) vec4<f32> {
    let range = max(in.effect_params.w, 1e-4);
    let d = ryo_median3(textureSample(sprite_texture, sprite_sampler, in.uv).rgb) - 0.5;
    let texels = vec2<f32>(textureDimensions(sprite_texture, 0));
    let unit = vec2<f32>(range) / texels;
    let screen = vec2<f32>(1.0) / max(fwidth(in.uv), vec2<f32>(1e-6));
    let px_range = max(0.5 * dot(unit, screen), 1.0);
    // Signed distance in screen pixels, positive inside
    let dist = d * px_range;
    let to_screen = px_range / range;
    if in.effect_params.x < 0.5 {
        let fill = clamp(dist + 0.5, 0.0, 1.0) * in.tint.a;
        let width = in.effect_params.y * to_screen;
        let fill_rgb = srgb_to_linear(in.tint.rgb);
        if width <= 0.0 {
            return ui_out(vec4<f32>(fill_rgb * fill * in.emission, fill));
        }
        let outer = clamp(dist + width + 0.5, 0.0, 1.0) * in.effect_extra.a;
        let rgb = fill_rgb * fill + srgb_to_linear(in.effect_extra.rgb) * outer * (1.0 - fill);
        return ui_out(vec4<f32>(rgb * in.emission, fill + outer * (1.0 - fill)));
    }
    let spread = in.effect_params.y * to_screen;
    let soft = max(in.effect_params.z * to_screen, 1.0);
    let a = smoothstep(-spread - soft, -spread + soft, dist) * in.tint.a;
    return ui_out(vec4<f32>(srgb_to_linear(in.tint.rgb) * a, a));
}

// A UI rectangle: anti-aliased rounded corners and an optional border.
// The quad extends one pixel (in UI units) past the rect on every side and
// uv_rect extends by the same fraction past [0, 1], so the outer edge keeps
// its anti-aliasing; p is measured from the rect's centre.
// effect_params: x = corner radius, y = border width, zw = size (UI units).
// effect_extra = border colour.
@fragment
fn fs_ui_rect(in: VertexOutput) -> @location(0) vec4<f32> {
    let size = max(in.effect_params.zw, vec2<f32>(1e-4));
    let p = (in.uv - vec2<f32>(0.5)) * size;
    let r = clamp(in.effect_params.x, 0.0, 0.5 * min(size.x, size.y));
    let q = abs(p) - 0.5 * size + vec2<f32>(r);
    let d = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
    let aa = max(length(fwidth(p)) * 0.70710678, 1e-4);
    let outer = clamp(0.5 - d / aa, 0.0, 1.0);
    let fill_rgb = srgb_to_linear(in.tint.rgb);
    let border = in.effect_params.y;
    if border <= 0.0 {
        let a = outer * in.tint.a;
        return ui_out(vec4<f32>(fill_rgb * a, a));
    }
    let inner = clamp(0.5 - (d + border) / aa, 0.0, 1.0);
    let a_fill = inner * in.tint.a;
    let a_border = (outer - inner) * in.effect_extra.a;
    let rgb = fill_rgb * a_fill + srgb_to_linear(in.effect_extra.rgb) * a_border;
    return ui_out(vec4<f32>(rgb, a_fill + a_border));
}
