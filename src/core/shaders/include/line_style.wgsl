// The line style: shading across a line's width

// `color` shaded across the line by its style, premultiplied by alpha:
// `edge_dist` is the pixel's distance from the centre line over the half
// width (1 at the edge) and `cover` how much of the pixel the line covers,
// which multiplies the straight alpha of every style
fn styled(color_in: vec4<f32>, edge_dist: f32, params: vec4<f32>, cover: f32) -> vec4<f32> {
    let style = (u32(params[0]) >> 4u) & 0x3u;
    var color = color_in;
    if style == 1u {
        // Soft edges
        color.a *= 1.0 - smoothstep(0.7, 1.0, edge_dist);
    } else if style == 2u {
        // Glow: a bright core inside a fainter halo
        let core_val = 1.0 - smoothstep(0.0, 0.3, edge_dist);
        let glow_val = 1.0 - smoothstep(0.3, 1.0, edge_dist);
        let brightness = core_val + glow_val * 0.5;
        color = vec4<f32>(color.rgb * brightness, color.a * (core_val + glow_val * 0.3));
    }
    color.a *= cover;
    // Glow alpha peaks above 1, which a float image would not clamp: the
    // blend would subtract from what is below
    let p = premultiply(color);
    return vec4<f32>(p.rgb, min(p.a, 1.0));
}
