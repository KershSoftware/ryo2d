// The line style: shading across a line's width

const LINE_SOLID: u32 = 0u;
const LINE_SOFT: u32 = 1u;
const LINE_GLOW: u32 = 2u;

// The shapes' means across the width, (alpha, brightness): Soft fades over
// its outer 0.3, losing half of it; Glow's core keeps 0.15 and its halo
// 0.65, its alpha the core and 0.3 of the halo, its brightness the core and
// half the halo
const SOFT_MEAN: vec2<f32> = vec2<f32>(0.85, 1.0);
const GLOW_MEAN: vec2<f32> = vec2<f32>(0.345, 0.475);

fn line_style(params: vec4<f32>) -> u32 {
    return (u32(params[0]) >> 4u) & 0x3u;
}

// Soft's and Glow's shape at `edge_dist`, the distance from the centre line
// over the half width (1 at the edge): (alpha, brightness)
fn style_shape_at(style: u32, edge_dist: f32) -> vec2<f32> {
    if style == LINE_SOFT {
        return vec2<f32>(1.0 - smoothstep(0.7, 1.0, edge_dist), 1.0);
    }
    if style == LINE_GLOW {
        // A bright core inside a fainter halo
        let core = 1.0 - smoothstep(0.0, 0.3, edge_dist);
        let halo = 1.0 - smoothstep(0.3, 1.0, edge_dist);
        return vec2<f32>(core + halo * 0.3, core + halo * 0.5);
    }
    return vec2<f32>(1.0);
}

// The integral of smoothstep(a, a + w, t) for t from 0 to `x`, x >= 0
fn smoothstep_area(x: f32, a: f32, w: f32) -> f32 {
    let t = (x - a) / w;
    if t <= 0.0 {
        return 0.0;
    }
    if t >= 1.0 {
        return w * (t - 0.5);
    }
    return w * t * t * t * (1.0 - 0.5 * t);
}

// The integral of Soft's or Glow's shape from the centre line to `x`
// across (signed, `hw` the half width, `hw` > 0): (alpha, brightness)
fn style_area(style: u32, x: f32, hw: f32) -> vec2<f32> {
    let a = min(abs(x), hw);
    var area: vec2<f32>;
    if style == LINE_SOFT {
        area = vec2<f32>(a - smoothstep_area(a, 0.7 * hw, 0.3 * hw), a);
    } else {
        let core = a - smoothstep_area(a, 0.0, 0.3 * hw);
        let halo = a - smoothstep_area(a, 0.3 * hw, 0.7 * hw);
        area = vec2<f32>(core + halo * 0.3, core + halo * 0.5);
    }
    return sign(x) * area;
}

// Soft's or Glow's shape over a pixel `d` pixels from the centre line of a
// line `hw` pixels in half width (`hw` > 0): averaged over the part of the
// pixel's box the line covers, as the coverage is, and blended toward its
// mean over the width as the half width falls from 3 pixels to 1, where
// the shape is too fine for the pixels to show and its average would come
// and go as the line moves; (alpha, brightness)
fn style_shape(style: u32, d: f32, hw: f32) -> vec2<f32> {
    let mean = select(GLOW_MEAN, SOFT_MEAN, style == LINE_SOFT);
    let lo = max(d - 0.5, -hw);
    let hi = min(d + 0.5, hw);
    var boxed = mean;
    if hi - lo > 1.0e-3 {
        boxed = (style_area(style, hi, hw) - style_area(style, lo, hw)) / (hi - lo);
    } else if hi > lo {
        // A sliver at the edge, where the difference would lose its digits
        boxed = style_shape_at(style, abs(lo + hi) * 0.5 / hw);
    }
    return mix(mean, boxed, clamp((hw - 1.0) * 0.5, 0.0, 1.0));
}

// `color` shaded across the line by its `style` (`line_style`),
// premultiplied by alpha:
// `shape` is Soft's or Glow's (alpha, brightness) at the pixel and `cover`
// how much of the pixel the line covers, which multiplies the straight
// alpha of every style
fn styled(color_in: vec4<f32>, shape: vec2<f32>, style: u32, cover: f32) -> vec4<f32> {
    var color = color_in;
    if style == LINE_SOFT {
        color.a *= shape.x;
    } else if style == LINE_GLOW {
        color = vec4<f32>(color.rgb * shape.y, color.a * shape.x);
    }
    color.a *= cover;
    // Glow alpha peaks above 1, which a float image would not clamp: the
    // blend would subtract from what is below
    let p = premultiply(color);
    return vec4<f32>(p.rgb, min(p.a, 1.0));
}
