// The line style: shading across a line's width

// `color` shaded across the line by its style, premultiplied by alpha
fn styled(color_in: vec4<f32>, uv: vec2<f32>, params: vec4<f32>, aa: f32) -> vec4<f32> {
    let style = (u32(params[0]) >> 4u) & 0x3u;
    let edge_dist = abs(uv.y - 0.5) * 2.0;
    var color = color_in;
    if style == 0u {
        // Solid: how much of the pixel the line covers, ramping over the
        // pixel its edge crosses (`aa` is edge_dist per pixel), so a line
        // one pixel wide stays solid
        let to_edge = (1.0 - edge_dist) / max(aa, 1e-4);
        color.a *= clamp(to_edge + 0.5, 0.0, 1.0);
    } else if style == 1u {
        // Soft edges
        color.a *= 1.0 - smoothstep(0.7, 1.0, edge_dist);
    } else {
        // Glow: a bright core inside a fainter halo
        let core_val = 1.0 - smoothstep(0.0, 0.3, edge_dist);
        let glow_val = 1.0 - smoothstep(0.3, 1.0, edge_dist);
        let brightness = core_val + glow_val * 0.5;
        color = vec4<f32>(color.rgb * brightness, color.a * (core_val + glow_val * 0.3));
    }
    // Glow alpha peaks above 1, which a float image would not clamp: the
    // blend would subtract from what is below
    let p = premultiply(color);
    return vec4<f32>(p.rgb, min(p.a, 1.0));
}
