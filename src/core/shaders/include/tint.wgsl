// Tinting a sampled colour: the sprite and UI fragment stages share these

// Linear colour premultiplied by alpha as straight sRGB (0 to 1)
fn straight_srgb(c: vec4<f32>) -> vec4<f32> {
    if c.a <= 0.0 {
        return vec4<f32>(0.0);
    }
    let rgb = clamp(c.rgb / c.a, vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(linear_to_srgb(rgb), min(c.a, 1.0));
}

// A linear premultiplied texel tinted in linear light (the tint decoded from
// sRGB); values above 1 are kept
fn finish_linear(color: vec4<f32>, tint: vec4<f32>, emission: f32) -> vec4<f32> {
    let a = clamp(tint.a, 0.0, 1.0);
    return vec4<f32>(color.rgb * srgb_to_linear(tint.rgb) * a * emission, color.a * a);
}

// Tint like the CPU renderer, then convert RGB to linear and premultiply.
//
// RGB is quantised to bytes: the source byte times the tint fraction,
// truncated. Alpha is source alpha times tint alpha, truncated to a byte
// unless it reaches 1 (so opaque stays opaque).
fn finish(color: vec4<f32>, tint: vec4<f32>, emission: f32) -> vec4<f32> {
    let source = round(clamp(color.rgb, vec3<f32>(0.0), vec3<f32>(1.0)) * 255.0);
    let rgb = floor(min(source * tint.rgb, vec3<f32>(255.0))) / 255.0;
    let a = clamp(color.a * tint.a, 0.0, 1.0);
    let alpha = select(floor(a * 255.0) / 255.0, 1.0, a >= 1.0);
    return vec4<f32>(srgb_to_linear(rgb) * alpha * emission, alpha);
}

// Tint a premultiplied colour like `finish`: RGB in sRGB, with the tint's
// alpha scaling colour and alpha together; then convert RGB to linear.
fn finish_premultiplied(color: vec4<f32>, tint: vec4<f32>, emission: f32) -> vec4<f32> {
    let rgb = srgb_to_linear(clamp(color.rgb * tint.rgb, vec3<f32>(0.0), vec3<f32>(1.0)));
    let alpha = clamp(color.a * tint.a, 0.0, 1.0);
    return vec4<f32>(rgb * tint.a * emission, alpha);
}
