// Text fragment entries: composed after sprite.wgsl (its vertex stage,
// VertexOutput and group 1 texture) and the glyph bodies

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
