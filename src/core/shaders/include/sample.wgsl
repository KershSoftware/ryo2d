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

// Sample `t` with `uv` repeating along the slot `bounds`: the fraction of
// `uv.x` runs across the slot's width, and `uv.y` (0..1) is clamped into its
// height. `ddx` and `ddy` are the unwrapped UV's derivatives, so the mip
// level does not jump where the texture repeats.
fn sample_wrapped(
    t: texture_2d<f32>,
    s: sampler,
    uv: vec2<f32>,
    bounds: vec4<f32>,
    ddx: vec2<f32>,
    ddy: vec2<f32>,
) -> vec4<f32> {
    let texel = 1.0 / vec2<f32>(textureDimensions(t));
    let size = bounds.zw - bounds.xy;
    let local = vec2<f32>(fract(uv.x), clamp(uv.y, 0.0, 1.0));
    // Keep half a texel inside, so filtering never reaches past the slot
    let low = bounds.xy + texel * 0.5;
    let high = max(low, bounds.zw - texel * 0.5);
    let inner = clamp(bounds.xy + local * size, low, high);
    return textureSampleGrad(t, s, inner, ddx * size, ddy * size);
}

// Sample a padded tile's cell `bounds` of `t` at `uv`; transparent outside
// it, with `sample_masked`'s slack. The level comes from `ddx` and `ddy`,
// the UV's screen derivatives taken where control flow is uniform: the
// isotropic level (log2 of the longer derivative, in texels), capped at the
// cell's clean `levels`, of which the cell is whole texels. A level within a
// thousandth of a whole one (zooms of 1, 1/2, 1/4) reads that level alone;
// any other reads the two levels round it, each sample kept half a texel of
// its own level inside the cell, and blends them. So no read reaches the
// next cell at any level, and the finer level's outer texels stay exact,
// where one trilinear read kept inside for the coarser level would blur
// them.
fn sample_cell(
    t: texture_2d<f32>,
    s: sampler,
    uv: vec2<f32>,
    bounds: vec4<f32>,
    levels: f32,
    ddx: vec2<f32>,
    ddy: vec2<f32>,
) -> vec4<f32> {
    let dims = vec2<f32>(textureDimensions(t));
    let slack = 0.01 / dims;
    if any(uv < bounds.xy - slack) || any(uv > bounds.zw + slack) {
        return vec4<f32>(0.0);
    }
    let rho = max(length(ddx * dims), length(ddy * dims));
    var level = clamp(log2(max(rho, 1e-8)), 0.0, levels);
    if abs(level - round(level)) < 1e-3 {
        level = round(level);
    }
    let fine = floor(level);
    let near = textureSampleLevel(t, s, inside_cell(uv, bounds, fine, dims), fine);
    if level == fine {
        return near;
    }
    let coarse = min(fine + 1.0, levels);
    let far = textureSampleLevel(t, s, inside_cell(uv, bounds, coarse, dims), coarse);
    return mix(near, far, level - fine);
}

// `uv` kept half a texel of level `level` inside `bounds`, on a texture of
// `dims` texels (a cell is whole texels of every level it reads)
fn inside_cell(uv: vec2<f32>, bounds: vec4<f32>, level: f32, dims: vec2<f32>) -> vec2<f32> {
    let half = 0.5 * exp2(level) / dims;
    let low = bounds.xy + half;
    let high = max(low, bounds.zw - half);
    return clamp(uv, low, high);
}
