// The camera every world shader draws through (one uniform buffer per camera)
struct CameraGlobals {
    view_proj: mat4x4<f32>,
    // The camera image's size in pixels
    resolution: vec2<f32>,
    game_time: f32,
    real_time: f32,
    // 1.0 snaps axis-aligned sprites to whole pixels (pixel art)
    snap: f32,
};

@group(0) @binding(0) var<uniform> globals: CameraGlobals;
