//! The particle random stream.
//!
//! SplitMix64, written here so particles repeat for a seed whatever the `rand`
//! crate does between versions.

/// A deterministic SplitMix64 stream, one per emitter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParticleRng(u64);

#[cfg_attr(not(test), expect(dead_code, reason = "used by the particle step"))]
impl ParticleRng {
  /// A stream starting from `seed`.
  pub(crate) fn new(seed: u64) -> Self {
    Self(seed)
  }

  /// The next 64 random bits.
  pub(crate) fn next_u64(&mut self) -> u64 {
    self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = self.0;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
  }

  /// A float in `[0, 1)` from the top 24 bits.
  pub(crate) fn next_f32(&mut self) -> f32 {
    (self.next_u64() >> 40) as f32 / 16_777_216.0
  }

  /// A float in `[min, max)`. Gives `min` unless `max > min`, and `0.0` when
  /// `min` is not finite. Draws nothing in those cases.
  pub(crate) fn range(&mut self, min: f32, max: f32) -> f32 {
    if !min.is_finite() {
      return 0.0;
    }
    if max.partial_cmp(&min) != Some(std::cmp::Ordering::Greater) || !max.is_finite() {
      return min;
    }
    let value = min + (max - min) * self.next_f32();
    value.clamp(min, max)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_stream_repeats_for_a_seed_and_differs_between_seeds() {
    let mut a = ParticleRng::new(7);
    let mut b = ParticleRng::new(7);
    let mut c = ParticleRng::new(8);
    let first: Vec<u64> = (0..8).map(|_| a.next_u64()).collect();
    let again: Vec<u64> = (0..8).map(|_| b.next_u64()).collect();
    let other: Vec<u64> = (0..8).map(|_| c.next_u64()).collect();
    assert_eq!(first, again);
    assert_ne!(first, other);
    // SplitMix64's published first output for seed 0
    assert_eq!(ParticleRng::new(0).next_u64(), 0xE220_A839_7B1D_CDAF);
  }

  #[test]
  fn next_f32_stays_in_zero_to_one() {
    let mut rng = ParticleRng::new(1);
    for _ in 0..1_000_000 {
      let value = rng.next_f32();
      assert!((0.0..1.0).contains(&value), "{value}");
    }
  }

  #[test]
  fn range_handles_odd_bounds() {
    let mut rng = ParticleRng::new(3);
    assert_eq!(rng.range(5.0, 1.0), 5.0);
    assert_eq!(rng.range(f32::NAN, 1.0), 0.0);
    assert_eq!(rng.range(1.0, f32::INFINITY), 1.0);
    let value = rng.range(2.0, 3.0);
    assert!((2.0..3.0).contains(&value));
  }
}
