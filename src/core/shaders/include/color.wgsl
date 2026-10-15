// Colour-space helpers

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let low = c / 12.92;
    let high = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, c <= vec3<f32>(0.04045));
}

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let low = c * 12.92;
    let high = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(high, low, c <= vec3<f32>(0.0031308));
}

// Straight colour to premultiplied colour
fn premultiply(c: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(c.rgb * c.a, c.a);
}

// Set (pipeline override) for pipelines drawing the UI image: its texels
// are gamma-encoded premultiplied colour, so blending happens in sRGB
// space as on the web
override UI_GAMMA: bool = false;

// A fragment's premultiplied linear colour, re-encoded to premultiplied
// sRGB for the UI image
fn ui_out(c: vec4<f32>) -> vec4<f32> {
    if !UI_GAMMA {
        return c;
    }
    if c.a <= 0.0 {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(linear_to_srgb(clamp(c.rgb / c.a, vec3<f32>(0.0), vec3<f32>(1.0))) * c.a, c.a);
}
