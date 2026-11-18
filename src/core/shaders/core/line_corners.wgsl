// The corner pass: once a frame, before the camera passes, one thread a
// point of the frame's world lines works out that point's corner with the
// one corner function (`lp_joint`) and writes it, and the limits it sets
// the quads beside it, for every vertex at the point to read
// (`line.wgsl`). Threads past 65,535 workgroups go on in the grid's next
// rows.

@group(0) @binding(0) var<storage, read> lines: array<LineInfo>;
@group(0) @binding(1) var<storage, read> line_points: array<LinePoint>;
@group(0) @binding(2) var<storage, read_write> line_corners: array<LineCorner>;
@group(0) @binding(3) var<storage, read_write> line_limits: array<vec2<f32>>;
// How many points the frame has
@group(0) @binding(4) var<uniform> line_point_count: vec4<u32>;

@compute @workgroup_size(64)
fn cs_line_corners(
    @builtin(workgroup_id) group: vec3<u32>,
    @builtin(num_workgroups) groups: vec3<u32>,
    @builtin(local_invocation_index) local: u32,
) {
    let at = (group.y * groups.x + group.x) * 64u + local;
    if at >= line_point_count.x {
        return;
    }
    let point = line_points[at];
    let info = lines[point.line];
    let count = i32(info.count);
    let closed = info.closed == 1u;
    let own = i32(at - info.first);
    // The point and its neighbours, in the window's middle slots
    var w: LineWindow;
    w.present = 0u;
    w.seam = 0u;
    for (var slot = 1; slot < 4; slot++) {
        var i = own + slot - 2;
        if closed {
            i = ((i % count) + count) % count;
        }
        if i >= 0 && i < count {
            w.p[slot] = line_pt(line_points[info.first + u32(i)]);
            w.present |= 1u << u32(slot);
        }
    }
    let joint = lp_joint(w, 0, LineLook(info.join, info.cap, info.miter_limit));
    line_corners[at] = line_corner_of(joint);
    line_limits[at] = vec2<f32>(joint.before, joint.after);
}
