//! The look: how the frame's linear HDR image becomes the picture on screen

use glam::{Mat3, Vec3};

use crate::core::ecs::Resource;

/// How the frame's linear HDR image becomes the display image, once per
/// frame after every camera has drawn (`ctx.look()`). The tonemap pass
/// applies the exposure, then the curve.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct Look {
  /// Exposure in EV (stops): +1 doubles the light, -1 halves it (default 0)
  pub exposure: f32,
  /// How light near and above white rolls off (default
  /// [`Tonemap::Soft`] with knee 0.8 and white 4)
  pub tonemap: Tonemap,
}

impl Default for Look {
  fn default() -> Self {
    Self {
      exposure: 0.0,
      tonemap: Tonemap::default(),
    }
  }
}

/// The curve that maps linear light, which may exceed 1, to the display's
/// 0 to 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Tonemap {
  /// Identity up to `knee`, then a smooth shoulder reaching 1 at `white`
  /// (both linear light) and clamped beyond. It scales all three channels by
  /// what it does to the brightest, so hues stay. The default (knee 0.8,
  /// white 4) keeps colours below sRGB 231 exact and shows pure white at
  /// about sRGB 243
  Soft { knee: f32, white: f32 },
  /// Clips at 1: the look before HDR
  Clamp,
  /// The fitted ACES filmic curve: contrasty, highlights desaturate
  Aces,
  /// AgX with its base contrast: soft, hue-faithful highlights
  AgX,
}

impl Default for Tonemap {
  fn default() -> Self {
    Tonemap::Soft {
      knee: 0.8,
      white: 4.0,
    }
  }
}

impl Tonemap {
  /// What the curve does to a linear colour (after exposure): the display
  /// colour in linear light, each channel 0 to 1. NaN counts as black,
  /// anything past f16's range as its largest value.
  pub fn apply(self, rgb: Vec3) -> Vec3 {
    let rgb = finite(rgb);
    match self.sanitized() {
      Tonemap::Soft { knee, white } => soft(rgb, knee, white),
      Tonemap::Clamp => rgb.clamp(Vec3::ZERO, Vec3::ONE),
      Tonemap::Aces => aces(rgb),
      Tonemap::AgX => agx(rgb),
    }
  }

  /// `Soft` with its knee in 0..=0.99 and white above it; NaN parts take the
  /// default's.
  pub(crate) fn sanitized(self) -> Self {
    match self {
      Tonemap::Soft { knee, white } => {
        let knee = if knee.is_nan() {
          0.8
        } else {
          knee.clamp(0.0, 0.99)
        };
        let white = if white.is_nan() { 4.0 } else { white };
        Tonemap::Soft {
          knee,
          white: white.max(knee + 0.01),
        }
      }
      other => other,
    }
  }
}

/// The largest value an f16 scene image holds.
const F16_MAX: f32 = 65504.0;

/// NaN and negative channels as 0, anything larger than f16 holds as its
/// largest value.
fn finite(rgb: Vec3) -> Vec3 {
  let channel = |c: f32| {
    if c.is_nan() {
      0.0
    } else {
      c.clamp(0.0, F16_MAX)
    }
  };
  Vec3::new(channel(rgb.x), channel(rgb.y), channel(rgb.z))
}

/// Extended Reinhard on the excess of the largest channel over the knee.
fn soft(rgb: Vec3, knee: f32, white: f32) -> Vec3 {
  let m = rgb.max_element();
  if m <= knee {
    return rgb;
  }
  let t = (m - knee) / (1.0 - knee);
  let tw = (white - knee) / (1.0 - knee);
  let g = t * (1.0 + t / (tw * tw)) / (1.0 + t);
  let mapped = (knee + (1.0 - knee) * g).min(1.0);
  rgb * (mapped / m)
}

/// Narkowicz's fit of the ACES curve.
fn aces(x: Vec3) -> Vec3 {
  ((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14)).clamp(Vec3::ZERO, Vec3::ONE)
}

/// AgX's inset and outset matrices (columns) and its log2 range.
const AGX_INSET: Mat3 = Mat3::from_cols_array(&[
  0.842_479_1,
  0.042_328_24,
  0.042_375_65,
  0.078_433_6,
  0.878_468_6,
  0.078_433_6,
  0.079_223_74,
  0.079_166_13,
  0.879_143,
]);
const AGX_OUTSET: Mat3 = Mat3::from_cols_array(&[
  1.196_879,
  -0.052_896_85,
  -0.052_971_64,
  -0.098_020_88,
  1.151_903,
  -0.098_043_45,
  -0.099_029_74,
  -0.098_961_18,
  1.151_073_7,
]);
const AGX_MIN_EV: f32 = -12.473_93;
const AGX_MAX_EV: f32 = 4.026_069;

/// AgX with its base contrast: log2 encoding, a polynomial sigmoid, then
/// back to linear light (the fit's output is gamma 2.2).
fn agx(rgb: Vec3) -> Vec3 {
  let v = AGX_INSET * rgb.max(Vec3::splat(1e-10));
  let v = Vec3::new(v.x.log2(), v.y.log2(), v.z.log2())
    .clamp(Vec3::splat(AGX_MIN_EV), Vec3::splat(AGX_MAX_EV));
  let v = (v - AGX_MIN_EV) / (AGX_MAX_EV - AGX_MIN_EV);
  let v2 = v * v;
  let v4 = v2 * v2;
  let v = 15.5 * v4 * v2 - 40.14 * v4 * v + 31.96 * v4 - 6.868 * v2 * v + 0.4298 * v2 + 0.1191 * v
    - 0.00232;
  let v = (AGX_OUTSET * v).clamp(Vec3::ZERO, Vec3::ONE);
  Vec3::new(v.x.powf(2.2), v.y.powf(2.2), v.z.powf(2.2))
}

/// The look as the tonemap shader reads it (group 0, binding 1).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct LookUniforms {
  /// x: exposure (EV), y: curve (0 soft, 1 clamp, 2 ACES, 3 AgX), z: knee,
  /// w: white
  pub(crate) curve: [f32; 4],
}

impl Look {
  /// What the tonemap shader reads, every parameter sanitized.
  pub(crate) fn uniforms(&self) -> LookUniforms {
    let exposure = if self.exposure.is_finite() {
      self.exposure.clamp(-16.0, 16.0)
    } else {
      0.0
    };
    let (curve, knee, white) = match self.tonemap.sanitized() {
      Tonemap::Soft { knee, white } => (0.0, knee, white),
      Tonemap::Clamp => (1.0, 0.0, 1.0),
      Tonemap::Aces => (2.0, 0.0, 1.0),
      Tonemap::AgX => (3.0, 0.0, 1.0),
    };
    LookUniforms {
      curve: [exposure, curve, knee, white],
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn grey(v: f32) -> Vec3 {
    Vec3::splat(v)
  }

  const CURVES: [Tonemap; 4] = [
    Tonemap::Soft {
      knee: 0.8,
      white: 4.0,
    },
    Tonemap::Clamp,
    Tonemap::Aces,
    Tonemap::AgX,
  ];

  #[test]
  fn soft_is_identity_below_the_knee() {
    for v in [0.0, 0.05, 0.4, 0.79, 0.8] {
      assert_eq!(Tonemap::default().apply(grey(v)), grey(v));
    }
    let colour = Vec3::new(0.7, 0.2, 0.05);
    assert_eq!(Tonemap::default().apply(colour), colour);
  }

  #[test]
  fn soft_leaves_the_knee_at_slope_one() {
    let at = |v: f32| Tonemap::default().apply(grey(v)).x;
    let h = 1e-3;
    let slope = (at(0.8 + h) - at(0.8)) / h;
    assert!((slope - 1.0).abs() < 0.01, "{slope}");
  }

  #[test]
  fn soft_reaches_one_at_white_and_clamps_beyond() {
    let at = |v: f32| Tonemap::default().apply(grey(v)).x;
    assert!((at(4.0) - 1.0).abs() < 1e-5, "{}", at(4.0));
    assert_eq!(at(100.0), 1.0);
  }

  #[test]
  fn soft_shows_white_at_about_0_9() {
    let v = Tonemap::default().apply(grey(1.0)).x;
    assert!((v - 0.9004).abs() < 1e-3, "{v}");
  }

  #[test]
  fn soft_keeps_hues() {
    let c = Tonemap::default().apply(Vec3::new(2.0, 1.0, 0.5));
    assert!((c.x / c.y - 2.0).abs() < 1e-4, "{c:?}");
    assert!((c.y / c.z - 2.0).abs() < 1e-4, "{c:?}");
  }

  #[test]
  fn every_curve_maps_black_to_black_and_never_falls() {
    for tonemap in CURVES {
      assert!(
        tonemap.apply(Vec3::ZERO).max_element() < 1e-4,
        "{tonemap:?}"
      );
      let mut last = 0.0;
      for i in 0..=400 {
        let v = tonemap.apply(grey(i as f32 * 0.04)).x;
        assert!(
          v >= last - 1e-6 && v <= 1.0,
          "{tonemap:?} at step {i}: {v} after {last}"
        );
        last = v;
      }
    }
  }

  #[test]
  fn degenerate_soft_parameters_stay_finite_and_monotonic() {
    let cases = [
      Tonemap::Soft {
        knee: 1.5,
        white: 0.2,
      },
      Tonemap::Soft {
        knee: -1.0,
        white: 4.0,
      },
      Tonemap::Soft {
        knee: f32::NAN,
        white: f32::NAN,
      },
      Tonemap::Soft {
        knee: 0.8,
        white: 0.8,
      },
    ];
    for tonemap in cases {
      let mut last = 0.0;
      for i in 0..=200 {
        let v = tonemap.apply(grey(i as f32 * 0.05)).x;
        assert!(
          v.is_finite() && (0.0..=1.0).contains(&v),
          "{tonemap:?}: {v}"
        );
        assert!(v >= last - 1e-6, "{tonemap:?}: {v} after {last}");
        last = v;
      }
    }
  }

  #[test]
  fn non_finite_light_shows_as_black_or_white() {
    for tonemap in CURVES {
      assert!(
        tonemap.apply(grey(f32::NAN)).max_element() < 1e-4,
        "{tonemap:?}"
      );
      assert!(
        tonemap.apply(grey(f32::INFINITY)).min_element() > 0.99,
        "{tonemap:?}"
      );
    }
  }

  #[test]
  fn uniforms_are_sanitized() {
    let look = Look {
      exposure: f32::NAN,
      tonemap: Tonemap::Soft {
        knee: 2.0,
        white: f32::NAN,
      },
    };
    let curve = look.uniforms().curve;
    assert_eq!(curve[0], 0.0, "NaN exposure is 0 EV");
    assert!(curve[2] <= 0.99 && curve[3] > curve[2], "{curve:?}");
  }
}
