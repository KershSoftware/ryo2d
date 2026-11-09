//! 2D gradient noise for particle turbulence.
//!
//! Perlin-style noise written in the module, so a seed gives the same field
//! on every platform and engine build.

use glam::Vec2;

/// Unit gradient directions; a lattice point picks one by hash.
const GRADIENTS: [(f32, f32); 8] = [
  (1.0, 0.0),
  (-1.0, 0.0),
  (0.0, 1.0),
  (0.0, -1.0),
  (
    std::f32::consts::FRAC_1_SQRT_2,
    std::f32::consts::FRAC_1_SQRT_2,
  ),
  (
    -std::f32::consts::FRAC_1_SQRT_2,
    std::f32::consts::FRAC_1_SQRT_2,
  ),
  (
    std::f32::consts::FRAC_1_SQRT_2,
    -std::f32::consts::FRAC_1_SQRT_2,
  ),
  (
    -std::f32::consts::FRAC_1_SQRT_2,
    -std::f32::consts::FRAC_1_SQRT_2,
  ),
];

fn hash(seed: u32, x: i32, y: i32) -> u32 {
  let mut h = seed ^ (x as u32).wrapping_mul(0x85EB_CA6B) ^ (y as u32).wrapping_mul(0xC2B2_AE35);
  h ^= h >> 16;
  h = h.wrapping_mul(0x7FEB_352D);
  h ^= h >> 15;
  h = h.wrapping_mul(0x846C_A68B);
  h ^ (h >> 16)
}

fn corner(seed: u32, ix: i32, iy: i32, dx: f32, dy: f32) -> f32 {
  let (gx, gy) = GRADIENTS[(hash(seed, ix, iy) & 7) as usize];
  gx * dx + gy * dy
}

fn fade(t: f32) -> f32 {
  t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Gradient noise at `(x, y)` in `[-1, 1]`; `0.0` for a non-finite point.
#[cfg_attr(not(test), expect(dead_code, reason = "used by the particle step"))]
pub(crate) fn noise2(seed: u32, x: f32, y: f32) -> f32 {
  if !x.is_finite() || !y.is_finite() {
    return 0.0;
  }
  let (fx, fy) = (x.floor(), y.floor());
  let (ix, iy) = (fx as i32, fy as i32);
  let (dx, dy) = (x - fx, y - fy);
  let (u, v) = (fade(dx), fade(dy));
  let x1 = ix.wrapping_add(1);
  let y1 = iy.wrapping_add(1);
  let bottom = lerp(
    corner(seed, ix, iy, dx, dy),
    corner(seed, x1, iy, dx - 1.0, dy),
    u,
  );
  let top = lerp(
    corner(seed, ix, y1, dx, dy - 1.0),
    corner(seed, x1, y1, dx - 1.0, dy - 1.0),
    u,
  );
  // Unit-gradient Perlin peaks at about 0.7; widen it toward the full range.
  (lerp(bottom, top, v) * 1.4).clamp(-1.0, 1.0)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
  a + (b - a) * t
}

/// A force in `[-1, 1]` per axis from two noise fields, sampled at
/// `at * frequency + time`.
#[cfg_attr(not(test), expect(dead_code, reason = "used by the particle step"))]
pub(crate) fn noise_force(seed: u32, at: Vec2, time: f32, frequency: f32) -> Vec2 {
  let p = at * frequency + Vec2::splat(time);
  Vec2::new(noise2(seed, p.x, p.y), noise2(seed ^ 0x9E37_79B9, p.x, p.y))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn noise_stays_in_range_and_is_smooth() {
    for i in 0..200 {
      for j in 0..200 {
        let (x, y) = (i as f32 * 0.37 - 20.0, j as f32 * 0.29 - 15.0);
        let n = noise2(5, x, y);
        assert!(n.abs() <= 1.0, "{n}");
        assert!((n - noise2(5, x + 0.01, y)).abs() < 0.05);
        assert!((n - noise2(5, x, y + 0.01)).abs() < 0.05);
      }
    }
  }

  #[test]
  fn noise_differs_by_seed_and_repeats_for_one() {
    let at = |seed| {
      (0..50)
        .map(|i| noise2(seed, i as f32 * 0.31, 0.7))
        .collect::<Vec<_>>()
    };
    assert_eq!(at(1), at(1));
    assert_ne!(at(1), at(2));
    assert_ne!(
      noise_force(1, Vec2::new(0.3, 0.4), 0.0, 1.0).x,
      noise_force(1, Vec2::new(0.3, 0.4), 0.0, 1.0).y
    );
  }
}
