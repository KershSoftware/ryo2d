// Test-only entries over the line module (`compose::LINE_TEST`, built for
// tests alone): a fragment stage counting each fragment and a compute
// entry writing every vertex the pieces build.

// Each fragment counts one (tests: a pixel drawn twice shows two)
@fragment
fn fs_count(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(1.0 / 255.0);
}

// Every vertex the frame's world lines draw, three a triangle in the index
// buffer's order, unpushed, as the pieces build them (tests compare them
// with the CPU builder's)
struct LineDump {
    position: vec2<f32>,
    uv: vec2<f32>,
    across: vec2<f32>,
    push: vec2<f32>,
    color: vec4<f32>,
    dist: f32,
    second: f32,
    half: f32,
    limit: f32,
    dist_rate: f32,
    second_rate: f32,
    k: f32,
    flags: u32,
    effect_u: f32,
    pad0: f32,
    pad1: f32,
    pad2: f32,
};

@group(3) @binding(0) var<storage, read_write> line_dump: array<LineDump>;
// The line index buffer's values
@group(3) @binding(1) var<storage, read> line_dump_codes: array<u32>;

@compute @workgroup_size(64)
fn cs_line_dump(@builtin(global_invocation_id) id: vec3<u32>) {
    let index = id.x;
    if index >= arrayLength(&line_dump) {
        return;
    }
    let v = world_vertex(line_dump_codes[index]).vertex;
    line_dump[index] = LineDump(
        v.position,
        v.uv,
        v.across,
        v.push,
        v.color,
        v.dist,
        v.second,
        v.half,
        v.limit,
        v.dist_rate,
        v.second_rate,
        v.k,
        v.flags,
        v.effect_u,
        0.0,
        0.0,
        0.0,
    );
}

// Every slot `line_piece_slot` names: entry `i` is triangle `i / 3 % 32`'s
// corner `i % 3` of a piece of kind `i / 864`, join `i / 288 % 3` and cap
// `i / 96 % 3` (tests compare them with the CPU's templates)
@group(3) @binding(2) var<storage, read_write> slot_dump: array<u32>;

@compute @workgroup_size(64)
fn cs_slot_dump(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= arrayLength(&slot_dump) {
        return;
    }
    slot_dump[i] = line_piece_slot(i / 864u, i / 288u % 3u, i / 96u % 3u, i / 3u % 32u, i % 3u);
}
