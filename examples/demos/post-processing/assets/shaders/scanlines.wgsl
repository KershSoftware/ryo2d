// Custom post-process effect: scanlines with a gentle vignette.
// `scene(uv)` returns LINEAR colour and the result is linear too.

struct Params {
  density: f32,
  strength: f32,
}

fn effect(in: EffectIn, params: Params) -> vec4<f32> {
  let color = scene(in.uv);
  let line = 0.5 + 0.5 * sin(in.uv.y * params.density + in.time * 4.0);
  let dim = 1.0 - params.strength * line;
  let centered = in.uv - vec2<f32>(0.5);
  let vignette = 1.0 - dot(centered, centered) * 0.8;
  return vec4<f32>(color.rgb * dim * vignette, color.a);
}
