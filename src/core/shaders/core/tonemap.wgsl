// The look: the latest scene image (linear light, premultiplied, may exceed
// 1) into the display image (Rgba8UnormSrgb, which encodes sRGB on store),
// one texel per pixel: exposure, then the curve. `Tonemap::apply` in
// render/look.rs is the same maths on the CPU; keep them together.

struct Look {
    // x: exposure (EV), y: curve (0 soft, 1 clamp, 2 ACES, 3 AgX), z: knee, w: white
    curve: vec4<f32>,
}

@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var<uniform> look: Look;

// Negative light as none, infinity as the largest value f16 holds
fn finite(c: vec3<f32>) -> vec3<f32> {
    return min(max(c, vec3<f32>(0.0)), vec3<f32>(65504.0));
}

// Identity up to the knee, then extended Reinhard on the excess of the
// largest channel, scaling all three so hues stay
fn soft(c: vec3<f32>, knee: f32, white: f32) -> vec3<f32> {
    let m = max(max(c.r, c.g), c.b);
    if m <= knee {
        return c;
    }
    let t = (m - knee) / (1.0 - knee);
    let tw = (white - knee) / (1.0 - knee);
    let g = t * (1.0 + t / (tw * tw)) / (1.0 + t);
    let mapped = min(knee + (1.0 - knee) * g, 1.0);
    return c * (mapped / m);
}

// Narkowicz's fit of the ACES curve
fn aces(x: vec3<f32>) -> vec3<f32> {
    return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), vec3<f32>(0.0), vec3<f32>(1.0));
}

// AgX with its base contrast
fn agx(c: vec3<f32>) -> vec3<f32> {
    let inset = mat3x3<f32>(
        vec3<f32>(0.8424791, 0.04232824, 0.04237565),
        vec3<f32>(0.0784336, 0.8784686, 0.0784336),
        vec3<f32>(0.07922374, 0.07916613, 0.879143),
    );
    let outset = mat3x3<f32>(
        vec3<f32>(1.196879, -0.05289685, -0.05297164),
        vec3<f32>(-0.09802088, 1.151903, -0.09804345),
        vec3<f32>(-0.09902974, -0.09896118, 1.1510737),
    );
    let min_ev = -12.47393;
    let max_ev = 4.026069;
    var v = inset * max(c, vec3<f32>(1e-10));
    v = clamp(log2(v), vec3<f32>(min_ev), vec3<f32>(max_ev));
    v = (v - min_ev) / (max_ev - min_ev);
    let v2 = v * v;
    let v4 = v2 * v2;
    v = 15.5 * v4 * v2 - 40.14 * v4 * v + 31.96 * v4 - 6.868 * v2 * v + 0.4298 * v2 + 0.1191 * v - 0.00232;
    v = clamp(outset * v, vec3<f32>(0.0), vec3<f32>(1.0));
    return pow(v, vec3<f32>(2.2));
}

fn curve(c: vec3<f32>) -> vec3<f32> {
    switch i32(look.curve.y) {
        case 1: {
            return clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
        }
        case 2: {
            return aces(c);
        }
        case 3: {
            return agx(c);
        }
        default: {
            return soft(c, look.curve.z, look.curve.w);
        }
    }
}

@fragment
fn fs_main(in: FullscreenVertex) -> @location(0) vec4<f32> {
    let texel = textureLoad(scene, vec2<i32>(in.position.xy), 0);
    let exposed = finite(texel.rgb) * exp2(look.curve.x);
    return vec4<f32>(curve(exposed), clamp(texel.a, 0.0, 1.0));
}
