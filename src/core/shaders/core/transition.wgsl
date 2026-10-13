// Scene transition fade: the frame mixed toward one colour.
//
// The CPU renderer blended sRGB bytes, which is the pace players expect a
// fade to pass through mid-tones at. Blending in linear light would hold
// bright areas back and then snap at the end, so the pass reads a copy of the
// frame, mixes in sRGB and writes the result over it.

struct Overlay {
    // sRGB colour (0-1), the fade's opacity in alpha
    color: vec4<f32>,
    // x: 1.0 when the frame texture is sRGB-encoded (reads and writes are
    // linear), 0.0 when it stores the bytes as they are
    srgb_frame: vec4<f32>,
}

@group(0) @binding(0) var<uniform> overlay: Overlay;
@group(0) @binding(1) var frame: texture_2d<f32>;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    // One triangle covering the screen
    let x = f32((index << 1u) & 2u);
    let y = f32(index & 2u);
    return vec4<f32>(x * 2.0 - 1.0, y * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let below = textureLoad(frame, vec2<i32>(position.xy), 0);
    if overlay.srgb_frame.x < 0.5 {
        return vec4<f32>(mix(below.rgb, overlay.color.rgb, overlay.color.a), below.a);
    }
    let mixed = mix(linear_to_srgb(below.rgb), overlay.color.rgb, overlay.color.a);
    return vec4<f32>(srgb_to_linear(mixed), below.a);
}
