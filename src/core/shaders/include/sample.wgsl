// Sampling a texture's slot of an atlas page

// Sample `t` at `uv`; transparent outside `bounds` (the slot's own rect).
// UVs within a hundredth of a texel of the rect still count as inside (float
// error at quad edges) and are clamped onto its edge texels. `ddx` and `ddy`
// are the UV's screen derivatives, taken where control flow is uniform; they
// pick the mip level.
fn sample_masked(
    t: texture_2d<f32>,
    s: sampler,
    uv: vec2<f32>,
    bounds: vec4<f32>,
    ddx: vec2<f32>,
    ddy: vec2<f32>,
) -> vec4<f32> {
    let texel = 1.0 / vec2<f32>(textureDimensions(t));
    let slack = texel * 0.01;
    if any(uv < bounds.xy - slack) || any(uv > bounds.zw + slack) {
        return vec4<f32>(0.0);
    }
    // A rect narrower than one texel clamps onto its first texel
    let low = bounds.xy + texel * 0.5;
    let high = max(low, bounds.zw - texel * 0.5);
    let inner = clamp(uv, low, high);
    return textureSampleGrad(t, s, inner, ddx, ddy);
}
