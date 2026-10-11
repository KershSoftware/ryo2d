// Custom sprite shader: noise dissolve with a coloured edge.
// The sprite code works in the sprite's sRGB values; the engine tints afterwards.

struct Params {
  edge: vec3<f32>,
  amount: f32,
}

fn hash(p: vec2<f32>) -> f32 {
  return fract(sin(dot(p, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

fn sprite(in: SpriteIn, params: Params) -> vec4<f32> {
  let color = sample(in.uv);
  // One noise value per sprite texel so the dissolve stays pixel-art crisp.
  let noise = hash(floor(in.uv * in.size));
  let edge_width = 0.08;
  if noise < params.amount {
    return vec4<f32>(0.0);
  }
  let glow = 1.0 - smoothstep(params.amount, params.amount + edge_width, noise);
  return vec4<f32>(mix(color.rgb, params.edge, glow), color.a);
}
