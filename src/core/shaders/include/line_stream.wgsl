// The world's line stream as the GPU holds it: the line table, the points
// and each point's corner, worked out once a frame by the corner pass
// (`core/line_corners.wgsl`) and read by every vertex there (`line.wgsl`).

// `LineInfo` (render/batch/lines.rs)
struct LineInfo {
    params: vec4<f32>,
    bounds: vec4<f32>,
    emission: f32,
    along_half: f32,
    first: u32,
    count: u32,
    join: u32,
    cap: u32,
    closed: u32,
    edge: u32,
    miter_limit: f32,
    pad0: u32,
    pad1: u32,
    pad2: u32,
};

// `LinePoint` (render/batch/lines.rs)
struct LinePoint {
    pos: vec2<f32>,
    half_width: f32,
    u: f32,
    // sRGB bytes, straight alpha
    color: u32,
    line: u32,
};

// A point's corner (`LpJoint`), 64 bytes: its turn's vectors and figures,
// the limits it sets the quads beside it, and in `bits` its shape (bits 0
// and 1), whether its inner side is shared (2), whether it is mitered (3)
// and whether it turns right (4). Where the line runs straight on,
// `bisector` holds the pair's normal. Its limits are also kept apart, two
// floats a point, for the vertices of the corners beside it to read
struct LineCorner {
    miter: vec2<f32>,
    bisector: vec2<f32>,
    bevel: vec2<f32>,
    push0: vec2<f32>,
    push1: vec2<f32>,
    cos: f32,
    inner_limit: f32,
    angle: f32,
    bits: u32,
    before: f32,
    after: f32,
};

// Point `i` of a closed line of `count`, `i` no more than `count` past
// either end
fn line_wrap(i: i32, count: i32) -> i32 {
    if i < 0 {
        return i + count;
    }
    if i >= count {
        return i - count;
    }
    return i;
}

fn line_pt(point: LinePoint) -> LinePt {
    return LinePt(point.pos, point.half_width, point.u, unpack4x8unorm(point.color));
}

fn line_corner_of(joint: LpJoint) -> LineCorner {
    var c: LineCorner;
    c.bits = joint.shape;
    c.before = joint.before;
    c.after = joint.after;
    if joint.shape == LP_STRAIGHT {
        c.bisector = joint.normal;
    } else if joint.shape == LP_TURN {
        let t = joint.turn;
        c.miter = t.miter;
        c.bisector = t.bisector;
        c.bevel = t.bevel;
        c.push0 = t.push0;
        c.push1 = t.push1;
        c.cos = t.cos;
        c.inner_limit = t.inner_limit;
        c.angle = t.angle;
        c.bits |= select(0u, 4u, t.shares_inside) | select(0u, 8u, t.mitered) | select(0u, 16u, t.side < 0.0);
    }
    return c;
}

fn line_joint_of(c: LineCorner) -> LpJoint {
    var joint: LpJoint;
    joint.shape = c.bits & 3u;
    joint.normal = c.bisector;
    joint.turn = LpTurn(
        select(1.0, -1.0, (c.bits & 16u) != 0u),
        c.cos,
        c.miter,
        c.bisector,
        (c.bits & 4u) != 0u,
        (c.bits & 8u) != 0u,
        c.bevel,
        c.push0,
        c.push1,
        c.inner_limit,
        c.angle,
    );
    joint.before = c.before;
    joint.after = c.after;
    return joint;
}
