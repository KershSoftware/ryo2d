// The UI rectangle fragment body: anti-aliased rounded corners and an
// optional border. p is measured from the rect's centre and p_width is its
// fwidth; size is in UI units.
// params: x = corner radius, y = border width, zw = size (UI units).
// extra = border colour. Returns premultiplied colour, before the stage's
// output encoding.
fn ui_rect(
    p: vec2<f32>,
    size: vec2<f32>,
    p_width: vec2<f32>,
    tint: vec4<f32>,
    params: vec4<f32>,
    extra: vec4<f32>,
) -> vec4<f32> {
    let r = clamp(params.x, 0.0, 0.5 * min(size.x, size.y));
    let q = abs(p) - 0.5 * size + vec2<f32>(r);
    let d = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
    let aa = max(length(p_width) * 0.70710678, 1e-4);
    let outer = clamp(0.5 - d / aa, 0.0, 1.0);
    let fill_rgb = srgb_to_linear(tint.rgb);
    let border = params.y;
    if border <= 0.0 {
        let a = outer * tint.a;
        return vec4<f32>(fill_rgb * a, a);
    }
    let inner = clamp(0.5 - (d + border) / aa, 0.0, 1.0);
    let a_fill = inner * tint.a;
    let a_border = (outer - inner) * extra.a;
    let rgb = fill_rgb * a_fill + srgb_to_linear(extra.rgb) * a_border;
    return vec4<f32>(rgb, a_fill + a_border);
}
