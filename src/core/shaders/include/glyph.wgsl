// Glyph fragment bodies: what a text pipeline does with a sampled texel.
// Each returns the premultiplied colour, before the stage's output encoding.

// A raster glyph: tint times coverage, premultiplied
fn glyph_mask(coverage: f32, tint: vec4<f32>, emission: f32) -> vec4<f32> {
    let a = coverage * tint.a;
    return vec4<f32>(srgb_to_linear(tint.rgb) * a * emission, a);
}

// A colour glyph (sRGB bytes, straight alpha) times the tint
fn glyph_color(texel: vec4<f32>, tint: vec4<f32>, emission: f32) -> vec4<f32> {
    let a = texel.a * tint.a;
    return vec4<f32>(srgb_to_linear(texel.rgb * tint.rgb) * a * emission, a);
}

fn ryo_median3(c: vec3<f32>) -> f32 {
    return max(min(c.r, c.g), min(max(c.r, c.g), c.b));
}

// An MSDF glyph. params: x = mode (0 fill with an optional outline,
// 1 a soft layer for shadows and glow), y = outline width or soft spread
// (base pixels), z = softness (base pixels), w = distance range (base
// pixels). extra = outline colour. `texel` is the sampled rgb, `texels` the
// texture's size and `uv_width` the uv's fwidth.
fn glyph_msdf(
    texel: vec3<f32>,
    texels: vec2<f32>,
    uv_width: vec2<f32>,
    tint: vec4<f32>,
    params: vec4<f32>,
    extra: vec4<f32>,
    emission: f32,
) -> vec4<f32> {
    let range = max(params.w, 1e-4);
    let d = ryo_median3(texel) - 0.5;
    let unit = vec2<f32>(range) / texels;
    let screen = vec2<f32>(1.0) / max(uv_width, vec2<f32>(1e-6));
    let px_range = max(0.5 * dot(unit, screen), 1.0);
    // Signed distance in screen pixels, positive inside
    let dist = d * px_range;
    let to_screen = px_range / range;
    if params.x < 0.5 {
        let fill = clamp(dist + 0.5, 0.0, 1.0) * tint.a;
        let width = params.y * to_screen;
        let fill_rgb = srgb_to_linear(tint.rgb);
        if width <= 0.0 {
            return vec4<f32>(fill_rgb * fill * emission, fill);
        }
        let outer = clamp(dist + width + 0.5, 0.0, 1.0) * extra.a;
        let rgb = fill_rgb * fill + srgb_to_linear(extra.rgb) * outer * (1.0 - fill);
        return vec4<f32>(rgb * emission, fill + outer * (1.0 - fill));
    }
    let spread = params.y * to_screen;
    let soft = max(params.z * to_screen, 1.0);
    let a = smoothstep(-spread - soft, -spread + soft, dist) * tint.a;
    return vec4<f32>(srgb_to_linear(tint.rgb) * a, a);
}
