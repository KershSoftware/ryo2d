// A shadowed light's four views (`lighting::ShadowAtlas`): 0 right (-45 to
// 45 degrees), 1 down, 2 left, 3 up, with y down. The shadow map shader
// draws a view's row and the light shader reads it through these, so the
// two cannot disagree on where an angle lies.

// `p` (relative to the light) turned into view `view`'s frame, looking
// along +x: its place along the view's row is `y / x`, the tangent of its
// angle within the view
fn in_view(p: vec2<f32>, view: u32) -> vec2<f32> {
    switch view {
        case 1u: { return vec2<f32>(p.y, -p.x); }
        case 2u: { return vec2<f32>(-p.x, -p.y); }
        case 3u: { return vec2<f32>(-p.y, p.x); }
        default: { return p; }
    }
}

// The view `p` (relative to the light) lies in: the one where it is at
// least as far along the view as across it (on a 45 degree boundary, the
// earlier of the two, which holds it at its row's end)
fn view_of(p: vec2<f32>) -> u32 {
    if p.x >= abs(p.y) {
        return 0u;
    }
    if p.y >= abs(p.x) {
        return 1u;
    }
    if -p.x >= abs(p.y) {
        return 2u;
    }
    return 3u;
}
