// Auto-exposure on the GPU: a histogram of the scene's log2 luminance, then
// its average without the darkest and brightest 5%, turned into the
// exposure that brings it to mid-grey and eased toward it. Nothing is read
// back: the tonemap pass reads `state.ev`.

const BINS: u32 = 64u;
const MIN_LOG: f32 = -12.0;
const MAX_LOG: f32 = 4.0;

struct Params {
    // Least EV, most EV, compensation, unused
    ev_range: vec4<f32>,
    // EV per second brightening, EV per second darkening, seconds since the last frame, unused
    speed: vec4<f32>,
}

struct State {
    ev: f32,
    // 0 until the first measure, which starts at its target
    ready: u32,
    pad0: u32,
    pad1: u32,
}

@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var<storage, read_write> histogram: array<atomic<u32>, 64>;
@group(0) @binding(2) var<storage, read_write> state: State;
@group(0) @binding(3) var<uniform> params: Params;

var<workgroup> local_bins: array<atomic<u32>, 64>;

// A pixel's bin, or BINS for light that is not finite or reached f16's limit
fn bin_of(c: vec3<f32>) -> u32 {
    let luma = dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
    // NaN fails both comparisons; overflowed light the second
    if !(luma >= 0.0 && luma < 60000.0) {
        return BINS;
    }
    let t = (log2(max(luma, 1e-12)) - MIN_LOG) / (MAX_LOG - MIN_LOG);
    return min(u32(clamp(t, 0.0, 1.0) * f32(BINS)), BINS - 1u);
}

@compute @workgroup_size(16, 16)
fn histogram_main(
    @builtin(global_invocation_id) id: vec3<u32>,
    @builtin(local_invocation_index) local: u32,
) {
    if local < BINS {
        atomicStore(&local_bins[local], 0u);
    }
    workgroupBarrier();
    let size = textureDimensions(scene);
    if id.x < size.x && id.y < size.y {
        let bin = bin_of(textureLoad(scene, vec2<i32>(id.xy), 0).rgb);
        if bin < BINS {
            atomicAdd(&local_bins[bin], 1u);
        }
    }
    workgroupBarrier();
    if local < BINS {
        atomicAdd(&histogram[local], atomicLoad(&local_bins[local]));
    }
}

@compute @workgroup_size(1)
fn average_main() {
    var total = 0u;
    for (var i = 0u; i < BINS; i++) {
        total += atomicLoad(&histogram[i]);
    }
    if total == 0u {
        return;
    }
    let low = f32(total) * 0.05;
    let high = f32(total) * 0.95;
    var seen = 0.0;
    var sum = 0.0;
    var weight = 0.0;
    for (var i = 0u; i < BINS; i++) {
        let count = f32(atomicLoad(&histogram[i]));
        // The part of this bin between the dropped tails
        let kept = max(min(seen + count, high) - max(seen, low), 0.0);
        let log_luma = MIN_LOG + (f32(i) + 0.5) * (MAX_LOG - MIN_LOG) / f32(BINS);
        sum += kept * log_luma;
        weight += kept;
        seen += count;
    }
    let average = sum / max(weight, 1e-6);
    let target_ev = clamp(log2(0.18) - average + params.ev_range.z, params.ev_range.x, params.ev_range.y);
    if state.ready == 0u {
        state.ev = target_ev;
        state.ready = 1u;
        return;
    }
    let delta = target_ev - state.ev;
    let step = select(params.speed.y, params.speed.x, delta > 0.0) * params.speed.z;
    state.ev += clamp(delta, -step, step);
}
