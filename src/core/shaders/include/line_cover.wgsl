// A line's fringe and coverage, shared by the world's lines (`line.wgsl`)
// and the UI's (`ui.wgsl`), so both push and cover alike: each vertex is
// pushed a pixel of its image out as far as its limit lets it, its
// distances grown by the push, and each pixel is covered by its distances
// to the line's edges in the image's pixels (no derivative).

// A vertex pushed out by the fringe: where it is, what it measures (across,
// inside the second edge and the half width; a round piece's offset in the
// first two, in the line's units) and its texture coordinate
struct LineFringe {
    pos: vec2<f32>,
    measure: vec3<f32>,
    uv: vec2<f32>,
};

// `vertex`, placed at `at` (its position moved by effects or a scroll
// slot), pushed a pixel out at `ppu` pixels per unit
fn line_fringe(vertex: PieceVertex, at: vec2<f32>, ppu: f32) -> LineFringe {
    var push = 0.0;
    if ppu > 0.0 {
        push = min(1.0 / ppu, vertex.limit);
    }
    var pos = at;
    pos += vertex.push * push;
    var measure = vec3<f32>(
        vertex.dist + vertex.dist_rate * push,
        vertex.second - vertex.second_rate * push,
        vertex.half,
    );
    // The texture across runs on over the fringe as the distance does
    // (`uv.y` is 0.5 - 0.5 × dist ÷ half), so it is not squeezed inside
    var uv = vertex.uv;
    if (vertex.flags & LINE_FLAG_ROUND) != 0u {
        let offset = vec2<f32>(vertex.dist, vertex.second);
        measure = vec3<f32>(offset + vertex.push * push, vertex.half);
        let radius = length(offset);
        if radius > 0.0 {
            uv.y = 0.5 + (uv.y - 0.5) * length(measure.xy) / radius;
        }
    } else if vertex.half > 0.0 {
        uv.y -= 0.5 * vertex.dist_rate * push / vertex.half;
    }
    return LineFringe(pos, measure, uv);
}

// What a line's fragment is given: its measures (as `LineFringe`'s), its
// vertex's flags and `k`, and a two-point capped line's half length
// between its caps
struct LineMeasure {
    measure: vec3<f32>,
    flags: u32,
    k: f32,
    along_half: f32,
};

// How much of the pixel a line of half width `half` covers: each edge's
// distance in the image's pixels through a one-pixel box, two edges blended
// by their angle
fn line_cover(m: LineMeasure, half: f32, ppu: f32) -> f32 {
    if ppu <= 0.0 {
        return 1.0;
    }
    if (m.flags & LINE_FLAG_ROUND) != 0u {
        return clamp((half - length(m.measure.xy)) * ppu + 0.5, 0.0, 1.0);
    }
    var across = 1.0;
    if m.measure.z < LINE_NO_EDGE * 0.5 {
        let d = m.measure.x * ppu;
        let hw = half * ppu;
        across = clamp(min(d + 0.5, hw) - max(d - 0.5, -hw), 0.0, 1.0);
    }
    if (m.flags & LINE_FLAG_SECOND) == 0u {
        return across;
    }
    var second: f32;
    if (m.flags & LINE_FLAG_ALONG) != 0u {
        let d = m.measure.y * ppu;
        let hw = m.along_half * ppu;
        second = clamp(min(d + 0.5, hw) - max(d - 0.5, -hw), 0.0, 1.0);
    } else {
        second = clamp(m.measure.y * ppu + 0.5, 0.0, 1.0);
    }
    let product = across * second;
    var toward = max(across + second - 1.0, 0.0);
    if m.k >= 0.0 {
        toward = min(across, second);
    }
    return product + (toward - product) * m.k * m.k;
}

// The pixel's distance from the centre line over the half width, which
// Soft and Glow shape by (a round piece's from its corner point)
fn line_edge(m: LineMeasure) -> f32 {
    if m.measure.z <= 0.0 || m.measure.z >= LINE_NO_EDGE * 0.5 {
        return 0.0;
    }
    if (m.flags & LINE_FLAG_ROUND) != 0u {
        return length(m.measure.xy) / m.measure.z;
    }
    return abs(m.measure.x) / m.measure.z;
}

// What the line gives a pixel: its coverage, and Soft's and Glow's shape
struct LinePixel {
    cover: f32,
    shape: vec2<f32>,
};

// A Soft or Glow line under half a pixel in half width covers the pixels
// one of half a pixel would, dimmed by the ratio of their widths, so it
// stays whole wherever it lies, and its shape is averaged over each pixel
// (`style_shape`). Solid takes its own coverage, which stays whole as it is
fn line_pixel(m: LineMeasure, params: vec4<f32>, ppu: f32) -> LinePixel {
    let style = line_style(params);
    let half = m.measure.z;
    if style == LINE_SOLID || ppu <= 0.0 || half <= 0.0 || half >= LINE_NO_EDGE * 0.5 {
        return LinePixel(line_cover(m, half, ppu), style_shape_at(style, line_edge(m)));
    }
    let hw = half * ppu;
    let covered = max(hw, 0.5);
    var d = abs(m.measure.x) * ppu;
    if (m.flags & LINE_FLAG_ROUND) != 0u {
        d = length(m.measure.xy) * ppu;
    }
    return LinePixel(line_cover(m, covered / ppu, ppu) * (hw / covered), style_shape(style, d, hw));
}
