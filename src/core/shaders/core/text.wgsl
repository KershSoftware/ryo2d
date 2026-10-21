// Text and UI fragment entries: composed after sprite.wgsl (its vertex
// stage, VertexOutput and group 1 texture) and the glyph and rect bodies

// A raster glyph: tint times coverage, premultiplied
@fragment
fn fs_text_mask(in: VertexOutput) -> @location(0) vec4<f32> {
    let coverage = textureSample(sprite_texture, sprite_sampler, in.uv).r;
    return ui_out(glyph_mask(coverage, in.tint, in.emission));
}

// A colour glyph (sRGB bytes, straight alpha) times the tint
@fragment
fn fs_text_color(in: VertexOutput) -> @location(0) vec4<f32> {
    let texel = textureSample(sprite_texture, sprite_sampler, in.uv);
    return ui_out(glyph_color(texel, in.tint, in.emission));
}

// An MSDF glyph; effect_params and effect_extra are described at glyph_msdf
@fragment
fn fs_text_msdf(in: VertexOutput) -> @location(0) vec4<f32> {
    let texel = textureSample(sprite_texture, sprite_sampler, in.uv).rgb;
    let texels = vec2<f32>(textureDimensions(sprite_texture, 0));
    return ui_out(glyph_msdf(texel, texels, fwidth(in.uv), in.tint, in.effect_params, in.effect_extra, in.emission));
}

// A UI rectangle. The quad extends one pixel (in UI units) past the rect on
// every side and uv_rect extends by the same fraction past [0, 1], so the
// outer edge keeps its anti-aliasing.
@fragment
fn fs_ui_rect(in: VertexOutput) -> @location(0) vec4<f32> {
    let size = max(in.effect_params.zw, vec2<f32>(1e-4));
    let p = (in.uv - vec2<f32>(0.5)) * size;
    return ui_out(ui_rect(p, size, fwidth(p), in.tint, in.effect_params, in.effect_extra));
}
