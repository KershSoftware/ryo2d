// Line pieces: a line's vertices built from its points, a triangle of a
// piece at a time, mirroring the CPU builder (`render/batch/polyline.rs`,
// the reference) function for function, in the same order, in f32.
//
// A vertex is asked for as a piece's kind, the triangle within it and the
// triangle's corner (0 to 2). It lies at one of the line's points, the
// piece's own or, for the far end of a segment, the next
// (`line_piece_corner`); the caller hands `line_piece_vertex` the five
// points about that one (`LineWindow`, slot 2 the point itself), so the
// world (from its storage buffers) and the UI (from its instances) build
// alike. Every piece that touches a point asks the one corner function
// (`lp_joint`) about it with the same points in the same order, and every
// position is made by one expression at the end (`lp_assemble`), so the
// copies of a place agree bit for bit. Nothing here makes or tests an
// infinity or a NaN: degenerate corners carry flags.

const LINE_FLAG_ROUND: u32 = 1u;
const LINE_FLAG_SECOND: u32 = 2u;
const LINE_FLAG_ALONG: u32 = 4u;
const LINE_NO_EDGE: f32 = 1.0e6;
const LINE_PI: f32 = 3.14159265358979;

const PIECE_SEGMENT: u32 = 0u;
const PIECE_JOIN: u32 = 1u;
const PIECE_START_CAP: u32 = 2u;
const PIECE_END_CAP: u32 = 3u;

const JOIN_MITER: u32 = 0u;
const JOIN_BEVEL: u32 = 1u;
const JOIN_ROUND: u32 = 2u;
const CAP_BUTT: u32 = 0u;
const CAP_SQUARE: u32 = 1u;
const CAP_ROUND: u32 = 2u;

// A round piece's arc: 16 triangles over its angle in 15 steps
const LINE_ARC_STEPS: u32 = 15u;

// A point of a line
struct LinePt {
    pos: vec2<f32>,
    half_width: f32,
    // Along the line (texture coordinate)
    u: f32,
    // sRGB, straight alpha
    color: vec4<f32>,
};

// The five points about a corner: slot 2 is the corner's own, slot k the
// point k - 2 along the line from it
struct LineWindow {
    p: array<LinePt, 5>,
    // Bit k: slot k holds a point (open lines end; closed ones wrap)
    present: u32,
    // Bit k: slot k is a closed line's first point, its seam
    seam: u32,
};

// How a line joins and caps
struct LineLook {
    join: u32,
    cap: u32,
    miter_limit: f32,
};

// One vertex of a piece, unpushed (`Vertex2D` in polyline.rs)
struct PieceVertex {
    position: vec2<f32>,
    uv: vec2<f32>,
    color: vec4<f32>,
    // The unit direction displacement effects move it along
    across: vec2<f32>,
    flags: u32,
    // The dot product of its two edges' outward normals
    k: f32,
    // Across from its piece's centre line (a round piece: its offset's x)
    dist: f32,
    // Inside its second edge (a round piece: its offset's y)
    second: f32,
    half: f32,
    push: vec2<f32>,
    limit: f32,
    dist_rate: f32,
    second_rate: f32,
    // The `u` displacement effects move it by: at a corner point, and at a
    // vertex both segments share, the point's own, so every piece's copy
    // there moves alike
    effect_u: f32,
};

struct LpSegment {
    dir: vec2<f32>,
    normal: vec2<f32>,
    len: f32,
};

// How a corner turns (`Turn`)
struct LpTurn {
    side: f32,
    cos: f32,
    miter: vec2<f32>,
    bisector: vec2<f32>,
    shares_inside: bool,
    mitered: bool,
    bevel: vec2<f32>,
    push0: vec2<f32>,
    push1: vec2<f32>,
    inner_limit: f32,
    angle: f32,
};

const LP_END: u32 = 0u;
const LP_STRAIGHT: u32 = 1u;
const LP_TURN: u32 = 2u;

// What the corner function decides at a point (`Joint`)
struct LpJoint {
    shape: u32,
    // Where the line runs straight on, the pair's normal
    normal: vec2<f32>,
    turn: LpTurn,
    before: f32,
    after: f32,
};

const LP_SECOND_NONE: u32 = 0u;
const LP_SECOND_EDGE: u32 = 1u;
const LP_SECOND_ALONG: u32 = 2u;

// A second edge (`Second`): inside an edge through `point` of outward
// normal `dir`, or along `dir` from `point`, the middle between a
// two-point line's caps
struct LpSecond {
    kind: u32,
    point: vec2<f32>,
    dir: vec2<f32>,
    k: f32,
};

// What a vertex measures, where it is and how it is pushed (`Measure`, and
// the place and look the CPU passes `vertex`)
struct LpPlace {
    // Its place: the corner point plus `dir` times `len`
    dir: vec2<f32>,
    len: f32,
    uv: vec2<f32>,
    across: vec2<f32>,
    dist: f32,
    half: f32,
    normal: vec2<f32>,
    push: vec2<f32>,
    limit: f32,
    second: LpSecond,
    round: bool,
    offset: vec2<f32>,
    // Effects move it by its point's own `u`: a vertex both segments at a
    // corner share
    point_u: bool,
};

fn lp_has(w: LineWindow, j: i32) -> bool {
    let slot = j + 2;
    return slot >= 0 && slot < 5 && ((w.present >> u32(slot)) & 1u) == 1u;
}

fn lp_pt(w: LineWindow, j: i32) -> LinePt {
    var points = w.p;
    return points[clamp(j + 2, 0, 4)];
}

fn lp_perp_dot(a: vec2<f32>, b: vec2<f32>) -> f32 {
    return a.x * b.y - a.y * b.x;
}

fn lp_normalize_or(v: vec2<f32>, fallback: vec2<f32>) -> vec2<f32> {
    let len = length(v);
    if len > 1.0e-30 {
        return v * (1.0 / len);
    }
    return fallback;
}

fn lp_segment(a: vec2<f32>, b: vec2<f32>) -> LpSegment {
    let len = distance(a, b);
    let dir = lp_normalize_or(b - a, vec2<f32>(1.0, 0.0));
    return LpSegment(dir, vec2<f32>(-dir.y, dir.x), len);
}

// The push that moves a point out across two edges of outward normals `a`
// and `b` one unit each per unit
fn lp_miter_of(a: vec2<f32>, b: vec2<f32>) -> vec2<f32> {
    return (a + b) / (1.0 + dot(a, b));
}

fn lp_limit_on(push: vec2<f32>, segment: LpSegment, inner_limit: f32) -> f32 {
    let along = abs(dot(push, segment.dir));
    var runs = LINE_NO_EDGE;
    if along > 1.0e-6 {
        runs = min(segment.len / (2.0 * along), LINE_NO_EDGE);
    }
    return min(runs, inner_limit);
}

// The one corner function: the corner at point `j` of the window
fn lp_joint(w: LineWindow, j: i32, look: LineLook) -> LpJoint {
    var out: LpJoint;
    out.shape = LP_END;
    out.before = LINE_NO_EDGE;
    out.after = LINE_NO_EDGE;
    if !(lp_has(w, j - 1) && lp_has(w, j) && lp_has(w, j + 1)) {
        return out;
    }
    let point = lp_pt(w, j);
    let before = lp_segment(lp_pt(w, j - 1).pos, point.pos);
    let after = lp_segment(point.pos, lp_pt(w, j + 1).pos);
    let hw = point.half_width;
    let n0 = before.normal;
    let n1 = after.normal;
    let turn = lp_perp_dot(before.dir, after.dir);
    let cos = dot(n0, n1);
    if abs(turn) < 1.0e-6 && cos > 0.0 {
        out.shape = LP_STRAIGHT;
        out.normal = lp_normalize_or(n0 + n1, n0);
        return out;
    }
    let side = select(1.0, -1.0, turn > 0.0);
    let m0 = side * n0;
    let m1 = side * n1;
    let u_turn = 1.0 + cos <= 1.0e-6;
    var miter = vec2<f32>(0.0);
    if !u_turn {
        miter = (n0 + n1) / (1.0 + cos);
    }
    let miter_length = length(miter);
    let shortest = min(before.len, after.len);
    let shares_inside = !u_turn && miter_length * hw <= shortest / 2.0;
    let mitered = look.join == JOIN_MITER && !u_turn && miter_length <= look.miter_limit;
    var bevel = before.dir;
    if !u_turn {
        bevel = lp_normalize_or(m0 + m1, before.dir);
    }
    var push0 = m0;
    var push1 = m1;
    if look.join != JOIN_ROUND {
        if mitered {
            push0 = side * miter;
            push1 = side * miter;
        } else {
            push0 = lp_miter_of(m0, bevel);
            push1 = lp_miter_of(m1, bevel);
        }
    }
    var inner_limit = LINE_NO_EDGE;
    if shares_inside {
        inner_limit = max(shortest / (2.0 * miter_length) - hw, 0.0);
    }
    var angle: f32;
    if u_turn {
        angle = select(-LINE_PI, LINE_PI, lp_perp_dot(m0, before.dir) >= 0.0);
    } else {
        angle = acos(clamp(dot(m0, m1), -1.0, 1.0));
        if lp_perp_dot(m0, m1) < 0.0 {
            angle = -angle;
        }
    }
    out.shape = LP_TURN;
    out.turn = LpTurn(
        side,
        cos,
        miter,
        lp_normalize_or(n0 + n1, n0),
        shares_inside,
        mitered,
        bevel,
        push0,
        push1,
        inner_limit,
        angle,
    );
    out.before = lp_limit_on(push0, before, inner_limit);
    out.after = lp_limit_on(push1, after, inner_limit);
    return out;
}

// `u` per unit along the segment from point `j`
fn lp_u_scale(w: LineWindow, j: i32) -> f32 {
    if lp_has(w, j) && lp_has(w, j + 1) {
        let a = lp_pt(w, j);
        let b = lp_pt(w, j + 1);
        let len = distance(a.pos, b.pos);
        if len > 0.0 {
            return (b.u - a.u) / len;
        }
    }
    return 0.0;
}

// The `u` the segment ending at point `j` ends with: past a closed line's
// seam it runs on
fn lp_end_u(w: LineWindow, j: i32) -> f32 {
    if !lp_has(w, j) {
        return 0.0;
    }
    let point = lp_pt(w, j);
    let seam = ((w.seam >> u32(j + 2)) & 1u) == 1u;
    if seam && lp_has(w, j - 1) {
        let last = lp_pt(w, j - 1);
        return last.u + distance(last.pos, point.pos) * lp_u_scale(w, j - 2);
    }
    return point.u;
}

fn lp_cap_reach(point: LinePt, cap: u32) -> f32 {
    return select(0.0, point.half_width, cap == CAP_SQUARE);
}

// The second edge a line's end measures on the segment from `a` to `b`
fn lp_end_second(a: LinePt, b: LinePt, cap: u32, first: bool, last: bool) -> LpSecond {
    var none: LpSecond;
    none.kind = LP_SECOND_NONE;
    if cap == CAP_ROUND {
        return none;
    }
    let dir = lp_normalize_or(b.pos - a.pos, vec2<f32>(0.0));
    let ra = lp_cap_reach(a, cap);
    let rb = lp_cap_reach(b, cap);
    if first && last {
        return LpSecond(LP_SECOND_ALONG, (a.pos - dir * ra + b.pos + dir * rb) / 2.0, dir, 0.0);
    }
    if first {
        return LpSecond(LP_SECOND_EDGE, a.pos - dir * ra, -dir, 0.0);
    }
    if last {
        return LpSecond(LP_SECOND_EDGE, b.pos + dir * rb, dir, 0.0);
    }
    return none;
}

// The second edge the segment from point `j` measures
fn lp_segment_second(w: LineWindow, j: i32, cap: u32) -> LpSecond {
    if !(lp_has(w, j) && lp_has(w, j + 1)) {
        var none: LpSecond;
        none.kind = LP_SECOND_NONE;
        return none;
    }
    return lp_end_second(lp_pt(w, j), lp_pt(w, j + 1), cap, !lp_has(w, j - 1), !lp_has(w, j + 2));
}

fn lp_edge_second(point: vec2<f32>, out: vec2<f32>, k: f32) -> LpSecond {
    return LpSecond(LP_SECOND_EDGE, point, out, k);
}

// On the edge of a strip of half width `half`, on `side` of its centre
// line of normal `normal`, pushed along `push` by at most `limit`
fn lp_edge(side: f32, half: f32, normal: vec2<f32>, push: vec2<f32>, limit: f32) -> LpPlace {
    var m: LpPlace;
    m.dist = side * half;
    m.half = half;
    m.normal = normal;
    m.push = push;
    m.limit = limit;
    m.second.kind = LP_SECOND_NONE;
    m.round = false;
    return m;
}

// A corner point, never pushed
fn lp_centre(half: f32) -> LpPlace {
    return lp_edge(0.0, half, vec2<f32>(0.0), vec2<f32>(0.0), 0.0);
}

// A vertex of a round piece, `offset` from its corner point
fn lp_round(half: f32, offset: vec2<f32>, push: vec2<f32>, limit: f32) -> LpPlace {
    var m = lp_edge(0.0, half, vec2<f32>(0.0), push, limit);
    m.round = true;
    m.offset = offset;
    return m;
}

fn lp_placed(measure: LpPlace, dir: vec2<f32>, len: f32, uv: vec2<f32>, across: vec2<f32>) -> LpPlace {
    var m = measure;
    m.dir = dir;
    m.len = len;
    m.uv = uv;
    m.across = across;
    return m;
}

fn lp_with_second(measure: LpPlace, second: LpSecond) -> LpPlace {
    var m = measure;
    m.second = second;
    return m;
}

// Moved by effects as its point is: a vertex both segments share
fn lp_shared(measure: LpPlace) -> LpPlace {
    var m = measure;
    m.point_u = true;
    return m;
}

fn lp_with_round(measure: LpPlace, offset: vec2<f32>) -> LpPlace {
    var m = measure;
    m.round = true;
    m.offset = offset;
    return m;
}

// The vertex a place makes at `corner`: the one place every position is
// made
fn lp_assemble(corner: LinePt, m: LpPlace) -> PieceVertex {
    var v: PieceVertex;
    v.position = corner.pos + m.dir * m.len;
    v.uv = m.uv;
    v.color = corner.color;
    v.across = m.across;
    v.flags = 0u;
    v.k = 0.0;
    v.dist = m.dist;
    v.second = LINE_NO_EDGE;
    v.half = m.half;
    v.push = m.push;
    v.limit = select(0.0, min(m.limit, LINE_NO_EDGE), m.limit > 0.0);
    v.dist_rate = dot(m.push, m.normal);
    v.second_rate = 0.0;
    v.effect_u = select(m.uv.x, corner.u, m.point_u || all(v.position == corner.pos));
    if m.round {
        v.dist = m.offset.x;
        v.second = m.offset.y;
        v.dist_rate = 0.0;
        v.flags = LINE_FLAG_ROUND;
    } else if m.second.kind == LP_SECOND_EDGE {
        v.second = dot(m.second.point - v.position, m.second.dir);
        v.second_rate = dot(m.push, m.second.dir);
        v.flags = LINE_FLAG_SECOND;
        v.k = m.second.k;
    } else if m.second.kind == LP_SECOND_ALONG {
        v.second = dot(v.position - m.second.point, m.second.dir);
        v.second_rate = -dot(m.push, m.second.dir);
        v.flags = LINE_FLAG_SECOND | LINE_FLAG_ALONG;
        v.k = m.second.k;
    }
    return v;
}

// The vertex unused triangles collapse to, at the corner point
fn lp_collapse(point: LinePt) -> LpPlace {
    return lp_placed(lp_centre(point.half_width), vec2<f32>(0.0), 0.0, vec2<f32>(point.u, 0.5), vec2<f32>(0.0));
}

// The most a vertex of the quad from point `j` (-1 or 0) may push
fn lp_quad_limit(joints: array<LpJoint, 3>, j: i32) -> f32 {
    var all = joints;
    return min(all[j + 1].after, all[j + 2].before);
}

// The most a vertex both quads at the corner share may push
fn lp_both(w: LineWindow, joints: array<LpJoint, 3>) -> f32 {
    let before = select(LINE_NO_EDGE, lp_quad_limit(joints, -1), lp_has(w, -1));
    let after = select(LINE_NO_EDGE, lp_quad_limit(joints, 0), lp_has(w, 1));
    return min(before, after);
}

const LP_LEFT: u32 = 0u;
const LP_RIGHT: u32 = 1u;
const LP_SPLIT: u32 = 2u;

// A butt or square capped end's edge vertex on `which` side, or a round
// capped end's
fn lp_open_pair(
    point: LinePt,
    segment: LpSegment,
    limit: f32,
    second: LpSecond,
    start: bool,
    cap: u32,
    which: u32,
) -> LpPlace {
    let hw = point.half_width;
    let n = segment.normal;
    let out = select(segment.dir, -segment.dir, start);
    let left = which == LP_LEFT;
    let side = select(-1.0, 1.0, left);
    var m: LpPlace;
    if cap == CAP_ROUND {
        m = lp_edge(side, hw, n, side * n, limit);
    } else {
        m = lp_with_second(lp_edge(side, hw, n, side * n + out, limit), second);
    }
    return lp_placed(m, side * n, hw, vec2<f32>(point.u, select(1.0, 0.0, left)), n);
}

// The vertex `which` at the corner as an end of the segment beside it:
// the one after it (`start`) or the one before
fn lp_end(w: LineWindow, look: LineLook, joints: array<LpJoint, 3>, start: bool, which: u32) -> LpPlace {
    var all = joints;
    let point = lp_pt(w, 0);
    let j = select(-1, 0, start);
    let segment = lp_segment(lp_pt(w, j).pos, lp_pt(w, j + 1).pos);
    let limit = lp_quad_limit(joints, j);
    let second = lp_segment_second(w, j, look.cap);
    let hw = point.half_width;
    let n = segment.normal;
    let u = select(lp_end_u(w, 0), point.u, start);
    let joint = all[1];
    if joint.shape == LP_END {
        if which == LP_SPLIT {
            return lp_placed(lp_centre(hw), vec2<f32>(0.0), 0.0, vec2<f32>(u, 0.5), n);
        }
        return lp_open_pair(point, segment, limit, second, start, look.cap, which);
    }
    if joint.shape == LP_STRAIGHT {
        let both = lp_both(w, joints);
        let normal = joint.normal;
        let left = which == LP_LEFT;
        let side = select(-1.0, 1.0, left);
        let m = lp_shared(lp_with_second(lp_edge(side, hw, normal, side * normal, both), second));
        return lp_placed(m, side * normal, hw, vec2<f32>(u, select(1.0, 0.0, left)), normal);
    }
    let turn = joint.turn;
    let side = turn.side;
    let v_out = select(1.0, 0.0, side > 0.0);
    let v_in = select(0.0, 1.0, side > 0.0);
    let m = side * n;
    // Where the inner side splits, the end's edge pushes straight out,
    // keeping to its line through the corner point; the join's fringe
    // triangles take it on to the corner's own push
    var push = m;
    if turn.shares_inside {
        push = select(turn.push0, turn.push1, start);
    }
    if which == LP_SPLIT {
        let centre = lp_with_second(lp_edge(0.0, hw, n, vec2<f32>(0.0), 0.0), second);
        return lp_placed(centre, vec2<f32>(0.0), 0.0, vec2<f32>(u, 0.5), turn.bisector);
    }
    let outer = (which == LP_LEFT) == (side > 0.0);
    if outer {
        let edge = lp_with_second(lp_edge(side, hw, n, push, limit), second);
        return lp_placed(edge, m, hw, vec2<f32>(u, v_out), n);
    }
    if turn.shares_inside {
        let both = lp_both(w, joints);
        let edge = lp_shared(lp_with_second(lp_edge(-side, hw, n, -side * turn.miter, both), second));
        return lp_placed(edge, -(side * turn.miter), hw, vec2<f32>(u, v_in), turn.bisector);
    }
    let edge = lp_with_second(lp_edge(-side, hw, n, -push, limit), second);
    return lp_placed(edge, -m, hw, vec2<f32>(u, v_in), n);
}

// Whether the corner, as an end of the segment beside it, has a vertex at
// its point that the segment's quad fans from
fn lp_splits(look: LineLook, joint: LpJoint) -> bool {
    if joint.shape == LP_END {
        return look.cap == CAP_ROUND;
    }
    if joint.shape == LP_TURN {
        return !joint.turn.shares_inside;
    }
    return false;
}

// A segment's corner slots: its quad's outline fanned from its end's
// corner point (or its left vertex)
const LP_FAN: u32 = 0u;
const LP_TO_LEFT: u32 = 1u;
const LP_FROM_LEFT: u32 = 2u;
const LP_START_SPLIT: u32 = 3u;
const LP_FROM_RIGHT: u32 = 4u;
const LP_TO_RIGHT: u32 = 5u;

fn lp_segment_slot(tri: u32, corner: u32) -> u32 {
    if corner == 0u {
        return LP_FAN;
    }
    switch tri {
        case 0u: {
            return select(LP_FROM_LEFT, LP_TO_LEFT, corner == 1u);
        }
        case 1u: {
            return select(LP_START_SPLIT, LP_FROM_LEFT, corner == 1u);
        }
        case 2u: {
            return select(LP_FROM_RIGHT, LP_START_SPLIT, corner == 1u);
        }
        default: {
            return select(LP_TO_RIGHT, LP_FROM_RIGHT, corner == 1u);
        }
    }
}

// Which of a piece's points (0, or 1 for a segment's far end) the corner
// of triangle `tri` at `corner` lies at
fn line_piece_corner(kind: u32, tri: u32, corner: u32) -> i32 {
    if kind != PIECE_SEGMENT {
        return 0;
    }
    let slot = lp_segment_slot(tri, corner);
    return select(0, 1, slot == LP_FAN || slot == LP_TO_LEFT || slot == LP_TO_RIGHT);
}

fn lp_segment_vertex(w: LineWindow, look: LineLook, joints: array<LpJoint, 3>, tri: u32, corner: u32) -> LpPlace {
    var all = joints;
    let slot = lp_segment_slot(tri, corner);
    let start = !(slot == LP_FAN || slot == LP_TO_LEFT || slot == LP_TO_RIGHT);
    let j = select(-1, 0, start);
    if !(lp_has(w, j) && lp_has(w, j + 1)) {
        return lp_collapse(lp_pt(w, 0));
    }
    var which = LP_LEFT;
    if slot == LP_FROM_RIGHT || slot == LP_TO_RIGHT {
        which = LP_RIGHT;
    } else if (slot == LP_FAN || slot == LP_START_SPLIT) && lp_splits(look, all[1]) {
        which = LP_SPLIT;
    }
    return lp_end(w, look, joints, start, which);
}

// Which of a fan triangle's vertices (0 to 2) is at `corner`, given as for
// a left turn and wound like the strip (`side` the outside)
fn lp_wedge(side: f32, corner: u32) -> u32 {
    if corner == 0u || side < 0.0 {
        return corner;
    }
    return 3u - corner;
}

// A join's vertices
const LJ_CENTRE: u32 = 0u;
const LJ_OUTER_END: u32 = 1u;
const LJ_OUTER_START: u32 = 2u;
const LJ_INNER_END: u32 = 3u;
const LJ_INNER_START: u32 = 4u;
const LJ_POINT: u32 = 5u;
const LJ_END_POINT: u32 = 6u;
const LJ_TIP: u32 = 7u;
const LJ_START_POINT: u32 = 8u;
const LJ_ROUND_CENTRE: u32 = 9u;
const LJ_FIRST: u32 = 10u;
const LJ_LAST: u32 = 11u;
const LJ_BEVEL_CENTRE: u32 = 12u;
const LJ_BEVEL_INNER_END: u32 = 13u;
const LJ_BEVEL_INNER_START: u32 = 14u;
const LJ_ELBOW_END: u32 = 15u;
const LJ_ELBOW_START: u32 = 16u;
const LJ_MIDDLE: u32 = 17u;
// Where the inner side splits, the strip's elbows as the segments push
// them, measuring what the fan measures
const LJ_STRIP_END: u32 = 18u;
const LJ_STRIP_START: u32 = 19u;
// Arc vertex k is LJ_ARC + k
const LJ_ARC: u32 = 32u;

// The join's vertex at `corner` of triangle `tri`
fn lp_join_role(look: LineLook, turn: LpTurn, tri: u32, corner: u32) -> u32 {
    let inner = turn.shares_inside;
    let bevelled = !turn.mitered && look.join != JOIN_ROUND;
    var abc = vec3<u32>(0u);
    if bevelled {
        switch tri {
            case 0u: {
                abc = select(vec3<u32>(LJ_BEVEL_CENTRE, LJ_STRIP_END, LJ_ELBOW_END), vec3<u32>(LJ_BEVEL_CENTRE, LJ_BEVEL_INNER_END, LJ_ELBOW_END), inner);
            }
            case 1u: {
                abc = vec3<u32>(LJ_BEVEL_CENTRE, LJ_ELBOW_END, LJ_MIDDLE);
            }
            case 2u: {
                abc = vec3<u32>(LJ_BEVEL_CENTRE, LJ_MIDDLE, LJ_ELBOW_START);
            }
            default: {
                abc = select(vec3<u32>(LJ_BEVEL_CENTRE, LJ_ELBOW_START, LJ_STRIP_START), vec3<u32>(LJ_BEVEL_CENTRE, LJ_ELBOW_START, LJ_BEVEL_INNER_START), inner);
            }
        }
    } else {
        // A round join's arc is its triangles 1 to 16
        let last = select(3u, LINE_ARC_STEPS + 2u, look.join == JOIN_ROUND);
        // Where the inner side splits, the fringe triangles from the strip's
        // elbows to the fan's ends (a round's arc ends push as they do)
        var fringe_end = vec3<u32>(LJ_ROUND_CENTRE, LJ_FIRST, LJ_FIRST);
        var fringe_start = vec3<u32>(LJ_ROUND_CENTRE, LJ_LAST, LJ_LAST);
        if turn.mitered {
            fringe_end = vec3<u32>(LJ_POINT, LJ_STRIP_END, LJ_END_POINT);
            fringe_start = vec3<u32>(LJ_POINT, LJ_START_POINT, LJ_STRIP_START);
        }
        if tri == 0u {
            abc = select(fringe_end, vec3<u32>(LJ_CENTRE, LJ_INNER_END, LJ_OUTER_END), inner);
        } else if tri == last {
            abc = select(fringe_start, vec3<u32>(LJ_CENTRE, LJ_OUTER_START, LJ_INNER_START), inner);
        } else if turn.mitered {
            abc = select(vec3<u32>(LJ_POINT, LJ_TIP, LJ_START_POINT), vec3<u32>(LJ_POINT, LJ_END_POINT, LJ_TIP), tri == 1u);
        } else {
            let k = tri - 1u;
            let b = select(LJ_ARC + k - 1u, LJ_FIRST, k == 0u);
            let c = select(LJ_ARC + k, LJ_LAST, k == LINE_ARC_STEPS);
            abc = vec3<u32>(LJ_ROUND_CENTRE, b, c);
        }
    }
    return abc[lp_wedge(turn.side, corner)];
}

fn lp_join_vertex(w: LineWindow, look: LineLook, joints: array<LpJoint, 3>, tri: u32, corner: u32) -> LpPlace {
    var all = joints;
    let point = lp_pt(w, 0);
    let joint = all[1];
    if !(lp_has(w, -1) && lp_has(w, 0) && lp_has(w, 1)) || joint.shape != LP_TURN {
        return lp_collapse(point);
    }
    let turn = joint.turn;
    let p = point.pos;
    let hw = point.half_width;
    let u = point.u;
    let u_end = lp_end_u(w, 0);
    let before = lp_segment(lp_pt(w, -1).pos, p);
    let after = lp_segment(p, lp_pt(w, 1).pos);
    let n0 = before.normal;
    let n1 = after.normal;
    let side = turn.side;
    let miter = turn.miter;
    let bisector = turn.bisector;
    let v_out = select(1.0, 0.0, side > 0.0);
    let v_in = select(0.0, 1.0, side > 0.0);
    let m0 = side * n0;
    let m1 = side * n1;
    let limit0 = lp_quad_limit(joints, -1);
    let limit1 = lp_quad_limit(joints, 0);
    let both = lp_both(w, joints);
    let centre_uv = vec2<f32>(u, 0.5);
    let zero = vec2<f32>(0.0);

    let role = lp_join_role(look, turn, tri, corner);
    let outer_end = lp_placed(lp_edge(side, hw, n0, turn.push0, limit0), m0, hw, vec2<f32>(u_end, v_out), n0);
    let outer_start = lp_placed(lp_edge(side, hw, n1, turn.push1, limit1), m1, hw, vec2<f32>(u, v_out), n1);
    let inner_end = lp_placed(lp_shared(lp_edge(-side, hw, n0, -side * miter, both)), -(side * miter), hw, vec2<f32>(u_end, v_in), bisector);
    let inner_start = lp_placed(lp_shared(lp_edge(-side, hw, n1, -side * miter, both)), -(side * miter), hw, vec2<f32>(u, v_in), bisector);
    // A miter's point measures the other segment's outer edge; a bevel's
    // fans the bevel's edge
    let other0 = lp_edge_second(p + m1 * hw, m1, turn.cos);
    let other1 = lp_edge_second(p + m0 * hw, m0, turn.cos);
    let bevel_edge = lp_edge_second(p + m0 * hw, turn.bevel, dot(m0, turn.bevel));

    if role >= LJ_ARC {
        let k = f32(role - LJ_ARC);
        let step = turn.angle / f32(LINE_ARC_STEPS);
        let scale = 1.0 / cos(step / 2.0);
        let angle = step * (k + 0.5);
        let r = vec2<f32>(cos(angle), sin(angle));
        let outward = vec2<f32>(r.x * m0.x - r.y * m0.y, r.y * m0.x + r.x * m0.y);
        let across = vec2<f32>(r.x * n0.x - r.y * n0.y, r.y * n0.x + r.x * n0.y);
        let offset = outward * (hw * scale);
        return lp_placed(lp_round(hw, offset, outward * scale, LINE_NO_EDGE), outward, hw * scale, vec2<f32>(u, v_out), across);
    }
    switch role {
        case LJ_CENTRE: {
            return lp_placed(lp_centre(hw), zero, 0.0, centre_uv, bisector);
        }
        case LJ_OUTER_END: {
            return outer_end;
        }
        case LJ_OUTER_START: {
            return outer_start;
        }
        case LJ_INNER_END: {
            return inner_end;
        }
        case LJ_INNER_START: {
            return inner_start;
        }
        case LJ_POINT: {
            return lp_placed(lp_with_second(lp_centre(hw), other0), zero, 0.0, centre_uv, bisector);
        }
        case LJ_END_POINT: {
            return lp_with_second(outer_end, other0);
        }
        case LJ_TIP: {
            let tip = lp_with_second(lp_edge(side, hw, n0, side * miter, LINE_NO_EDGE), other0);
            return lp_placed(tip, side * miter, hw, vec2<f32>(u, v_out), bisector);
        }
        case LJ_START_POINT: {
            return lp_with_second(outer_start, other1);
        }
        case LJ_ROUND_CENTRE: {
            return lp_placed(lp_round(hw, zero, zero, 0.0), zero, 0.0, centre_uv, bisector);
        }
        case LJ_FIRST: {
            return lp_with_round(outer_end, m0 * hw);
        }
        case LJ_LAST: {
            return lp_with_round(outer_start, m1 * hw);
        }
        case LJ_BEVEL_CENTRE: {
            return lp_placed(lp_with_second(lp_centre(hw), bevel_edge), zero, 0.0, centre_uv, bisector);
        }
        case LJ_BEVEL_INNER_END: {
            return lp_with_second(inner_end, bevel_edge);
        }
        case LJ_BEVEL_INNER_START: {
            return lp_with_second(inner_start, bevel_edge);
        }
        case LJ_ELBOW_END: {
            return lp_with_second(outer_end, bevel_edge);
        }
        case LJ_ELBOW_START: {
            return lp_with_second(outer_start, bevel_edge);
        }
        case LJ_STRIP_END, LJ_STRIP_START: {
            let end = role == LJ_STRIP_END;
            let m = select(m1, m0, end);
            let n = select(n1, n0, end);
            let limit = select(limit1, limit0, end);
            let uv = vec2<f32>(select(u, u_end, end), v_out);
            var second = bevel_edge;
            if turn.mitered && end {
                second = other0;
            } else if turn.mitered {
                second = other1;
            }
            let strip = lp_with_second(lp_edge(side, hw, n, m, limit), second);
            return lp_placed(strip, m, hw, uv, n);
        }
        default: {
            // The bevel's middle, pushed along its normal
            var middle = lp_edge(side, hw, n0, turn.bevel, LINE_NO_EDGE);
            middle.dist = side * hw * (1.0 + turn.cos) / 2.0;
            middle = lp_with_second(middle, bevel_edge);
            return lp_placed(middle, m0 + m1, hw / 2.0, vec2<f32>(u, v_out), bisector);
        }
    }
}

// A cap's vertices
const LC_LEFT: u32 = 0u;
const LC_RIGHT: u32 = 1u;
const LC_FAR_LEFT: u32 = 2u;
const LC_FAR_RIGHT: u32 = 3u;
const LC_CENTRE: u32 = 4u;
const LC_FIRST: u32 = 5u;
const LC_LAST: u32 = 6u;
// Arc vertex k is LC_ARC + k
const LC_ARC: u32 = 32u;

fn lp_cap_role(cap: u32, start: bool, tri: u32, corner: u32) -> u32 {
    if cap == CAP_SQUARE {
        var abc: vec3<u32>;
        if start {
            abc = select(vec3<u32>(LC_FAR_RIGHT, LC_RIGHT, LC_LEFT), vec3<u32>(LC_FAR_LEFT, LC_FAR_RIGHT, LC_LEFT), tri == 0u);
        } else {
            abc = select(vec3<u32>(LC_RIGHT, LC_FAR_RIGHT, LC_FAR_LEFT), vec3<u32>(LC_LEFT, LC_RIGHT, LC_FAR_LEFT), tri == 0u);
        }
        return abc[corner];
    }
    let b = select(LC_ARC + tri - 1u, LC_FIRST, tri == 0u);
    let c = select(LC_ARC + tri, LC_LAST, tri == LINE_ARC_STEPS);
    let abc = vec3<u32>(LC_CENTRE, b, c);
    return abc[lp_wedge(select(1.0, -1.0, start), corner)];
}

fn lp_cap_vertex(w: LineWindow, look: LineLook, joints: array<LpJoint, 3>, start: bool, tri: u32, corner: u32) -> LpPlace {
    let point = lp_pt(w, 0);
    let near_at = select(-1, 1, start);
    if !lp_has(w, near_at) {
        return lp_collapse(point);
    }
    let near = lp_pt(w, near_at);
    var segment: LpSegment;
    var limit: f32;
    var second: LpSecond;
    var scale: f32;
    if start {
        segment = lp_segment(point.pos, near.pos);
        limit = lp_quad_limit(joints, 0);
        second = lp_end_second(point, near, look.cap, true, !lp_has(w, 2));
        scale = -lp_u_scale(w, 0);
    } else {
        segment = lp_segment(near.pos, point.pos);
        limit = lp_quad_limit(joints, -1);
        second = lp_end_second(near, point, look.cap, !lp_has(w, -2), true);
        scale = lp_u_scale(w, -1);
    }
    let hw = point.half_width;
    let n = segment.normal;
    let u = point.u;
    let out = select(segment.dir, -segment.dir, start);
    let role = lp_cap_role(look.cap, start, tri, corner);
    if role >= LC_ARC {
        let turn = select(-1.0, 1.0, start);
        let step = LINE_PI / f32(LINE_ARC_STEPS);
        let arc_scale = 1.0 / cos(step / 2.0);
        let angle = turn * (step * (f32(role - LC_ARC) + 0.5));
        let r = vec2<f32>(cos(angle), sin(angle));
        let outward = vec2<f32>(r.x * n.x - r.y * n.y, r.y * n.x + r.x * n.y);
        let offset = outward * (hw * arc_scale);
        let towards = lp_normalize_or(offset, vec2<f32>(0.0));
        let uv = vec2<f32>(u + dot(offset, out) * scale, 0.5 - 0.5 * dot(towards, n));
        return lp_placed(lp_round(hw, offset, outward * arc_scale, LINE_NO_EDGE), outward, hw * arc_scale, uv, n);
    }
    switch role {
        case LC_LEFT: {
            return lp_open_pair(point, segment, limit, second, start, look.cap, LP_LEFT);
        }
        case LC_RIGHT: {
            return lp_open_pair(point, segment, limit, second, start, look.cap, LP_RIGHT);
        }
        case LC_FAR_LEFT, LC_FAR_RIGHT: {
            let left = role == LC_FAR_LEFT;
            let side = select(-1.0, 1.0, left);
            let corner_measure = lp_with_second(lp_edge(side, hw, n, side * n + out, limit), second);
            let uv = vec2<f32>(u + hw * scale, select(1.0, 0.0, left));
            return lp_placed(corner_measure, side * n + out, hw, uv, n);
        }
        case LC_CENTRE: {
            return lp_placed(lp_round(hw, vec2<f32>(0.0), vec2<f32>(0.0), 0.0), vec2<f32>(0.0), 0.0, vec2<f32>(u, 0.5), n);
        }
        case LC_FIRST: {
            let left = lp_open_pair(point, segment, limit, second, start, look.cap, LP_LEFT);
            return lp_with_round(left, n * hw);
        }
        default: {
            let right = lp_open_pair(point, segment, limit, second, start, look.cap, LP_RIGHT);
            return lp_with_round(right, -n * hw);
        }
    }
}

// The vertex at `corner` of triangle `tri` of a piece of `kind`, given the
// window about the point it lies at (`line_piece_corner`)
fn line_piece_vertex(w: LineWindow, look: LineLook, kind: u32, tri: u32, corner: u32) -> PieceVertex {
    // The corner and its neighbours, each asked once, from one place
    var joints: array<LpJoint, 3>;
    for (var i = 0; i < 3; i++) {
        joints[i] = lp_joint(w, i - 1, look);
    }
    var place: LpPlace;
    switch kind {
        case PIECE_SEGMENT: {
            place = lp_segment_vertex(w, look, joints, tri, corner);
        }
        case PIECE_JOIN: {
            place = lp_join_vertex(w, look, joints, tri, corner);
        }
        default: {
            place = lp_cap_vertex(w, look, joints, kind == PIECE_START_CAP, tri, corner);
        }
    }
    return lp_assemble(lp_pt(w, 0), place);
}
