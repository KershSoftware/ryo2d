// Pixel-art snapping: world positions to target pixels and back, through
// the camera (`globals`)

// Target pixel coordinates (top-left origin) of a world position
fn to_pixel(world: vec2<f32>) -> vec2<f32> {
    let clip = globals.view_proj * vec4<f32>(world, 0.0, 1.0);
    return (clip.xy * vec2<f32>(0.5, -0.5) + 0.5) * globals.resolution;
}

// The clip position of target pixel coordinates
fn from_pixel(pixel: vec2<f32>) -> vec4<f32> {
    let ndc = (pixel / globals.resolution - 0.5) * vec2<f32>(2.0, -2.0);
    return vec4<f32>(ndc, 0.0, 1.0);
}
