// Scene transition fade: one colour over the whole frame.
//
// The colour arrives as sRGB bytes (0-1) with the fade's opacity in alpha, the
// way the CPU blended it. The frame is an sRGB target, so the colour is
// converted to linear here and alpha blending does the rest.

struct Overlay {
    color: vec4<f32>,
}

@group(0) @binding(0) var<uniform> overlay: Overlay;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    // One triangle covering the screen
    let x = f32((index << 1u) & 2u);
    let y = f32(index & 2u);
    return vec4<f32>(x * 2.0 - 1.0, y * 2.0 - 1.0, 0.0, 1.0);
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let low = c / 12.92;
    let high = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, c <= vec3<f32>(0.04045));
}

@fragment
fn fs_main() -> @location(0) vec4<f32> {
    return vec4<f32>(srgb_to_linear(overlay.color.rgb), overlay.color.a);
}
