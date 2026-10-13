// The look: the latest scene image (linear light, premultiplied, may exceed
// 1) into the display image (Rgba8UnormSrgb, which encodes sRGB on store),
// one texel per pixel.

@group(0) @binding(0) var scene: texture_2d<f32>;

@fragment
fn fs_main(in: FullscreenVertex) -> @location(0) vec4<f32> {
    let c = textureLoad(scene, vec2<i32>(in.position.xy), 0);
    return clamp(c, vec4<f32>(0.0), vec4<f32>(1.0));
}
