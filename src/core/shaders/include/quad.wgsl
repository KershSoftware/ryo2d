// Placing a quad: the sprite and UI vertex stages both put a quad's corners
// where this says, through the camera (`globals`) and the pixel-art snap
// (`snap.wgsl`), so the UI places its quads exactly as sprites are placed

// The clip position of a unit quad's `corner` (-0.5 to 0.5 on each axis)
// under the quad's 2D affine transform: an instance's three transform lanes
// (their w is not read)
fn quad_corner(t0: vec4<f32>, t1: vec4<f32>, t2: vec4<f32>, corner: vec2<f32>) -> vec4<f32> {
    // Apply 2D affine transform from instance
    let transform = mat3x3<f32>(
        t0.xyz,
        t1.xyz,
        t2.xyz
    );

    let local_pos = vec3<f32>(corner, 1.0);
    let world_pos = transform * local_pos;

    // Through the camera
    var clip = globals.view_proj * vec4<f32>(world_pos.xy, 0.0, 1.0);
    // Pixel art: a quad axis-aligned on screen (itself and the camera
    // unturned) rounds its top-left and size to whole pixels once, so it
    // keeps its width as it moves; under a turned camera it draws unsnapped
    let unturned = abs(t0.y) < 1e-5 && abs(t1.x) < 1e-5;
    let camera_unturned = abs(globals.view_proj[0][1]) < 1e-5 && abs(globals.view_proj[1][0]) < 1e-5;
    let axis_aligned = unturned && camera_unturned;
    if globals.snap > 0.5 && axis_aligned {
        let p0 = to_pixel((transform * vec3<f32>(-0.5, -0.5, 1.0)).xy);
        let p1 = to_pixel((transform * vec3<f32>(0.5, 0.5, 1.0)).xy);
        let origin = round(min(p0, p1));
        let size = round(abs(p1 - p0));
        // A corner that maps to the larger pixel coordinate counts from the far side
        let along = select(corner + 0.5, 0.5 - corner, p1 < p0);
        clip = from_pixel(origin + along * size);
    }
    return clip;
}
