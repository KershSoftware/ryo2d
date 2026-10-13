//! The look: how the frame's linear HDR image becomes the picture on screen

use glam::{Mat3, Vec3};

use crate::core::asset::SpriteHandle;
use crate::core::ecs::Resource;
use crate::core::graphics::Sprite;

/// How the frame's linear HDR image becomes the display image, once per
/// frame after every camera has drawn (`ctx.look()`). The tonemap pass
/// applies, in order: exposure; white balance, contrast and saturation in
/// linear HDR; the curve; lift, gamma and gain on the display colour; the LUT
/// in sRGB.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct Look {
  /// Exposure in EV (stops): +1 doubles the light, -1 halves it (default 0)
  pub exposure: f32,
  /// How light near and above white rolls off (default
  /// [`Tonemap::Soft`] with knee 0.8 and white 4)
  pub tonemap: Tonemap,
  /// Colour grading (default neutral)
  pub grading: ColorGrading,
  /// Exposure that follows the scene (default off); added to `exposure`
  pub auto_exposure: Option<AutoExposure>,
}

/// Exposure that follows the scene's brightness, measured on the GPU each
/// frame: the average luminance, without the darkest and brightest 5%, is
/// brought to mid-grey within `min_ev..=max_ev`, plus `compensation`. It
/// starts at its target and then moves at its speeds, in real time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutoExposure {
  /// The least exposure it uses, in EV (default -4)
  pub min_ev: f32,
  /// The most, in EV (default 4)
  pub max_ev: f32,
  /// Added to what it measures, in EV: positive brightens (default 0)
  pub compensation: f32,
  /// How fast it brightens, in EV per second (default 3)
  pub speed_up: f32,
  /// How fast it darkens, in EV per second (default 1)
  pub speed_down: f32,
}

impl Default for AutoExposure {
  fn default() -> Self {
    Self {
      min_ev: -4.0,
      max_ev: 4.0,
      compensation: 0.0,
      speed_up: 3.0,
      speed_down: 1.0,
    }
  }
}

impl Default for Look {
  fn default() -> Self {
    Self {
      exposure: 0.0,
      tonemap: Tonemap::default(),
      grading: ColorGrading::default(),
      auto_exposure: None,
    }
  }
}

/// Colour grading, applied by the tonemap pass. White balance, contrast and
/// saturation work on the HDR light before the curve; lift, gamma and gain
/// on the display colour after it; the LUT last, in sRGB. The default
/// changes nothing.
#[derive(Debug, Clone, PartialEq)]
pub struct ColorGrading {
  /// Warmer (positive) or cooler (negative), -1 to 1
  pub temperature: f32,
  /// Toward magenta (positive) or green (negative), -1 to 1
  pub tint: f32,
  /// Contrast about mid-grey (0.18) in log2 space; 1 is neutral
  pub contrast: f32,
  /// 0 is grey, 1 neutral, above 1 more vivid
  pub saturation: f32,
  /// Raises the shadows of the display colour, per channel; 0 is neutral
  pub lift: Vec3,
  /// Bends the mid-tones, per channel; 1 is neutral, above 1 brightens
  pub gamma: Vec3,
  /// Scales the display colour, per channel; 1 is neutral
  pub gain: Vec3,
  /// A colour lookup table, applied last
  pub lut: Option<ColorLut>,
  /// How much of the LUT's result shows, 0 to 1
  pub lut_strength: f32,
}

impl Default for ColorGrading {
  fn default() -> Self {
    Self {
      temperature: 0.0,
      tint: 0.0,
      contrast: 1.0,
      saturation: 1.0,
      lift: Vec3::ZERO,
      gamma: Vec3::ONE,
      gain: Vec3::ONE,
      lut: None,
      lut_strength: 1.0,
    }
  }
}

/// A colour lookup table: a strip of N tiles of N x N side by side (1024x32
/// for N = 32, 256x16 for N = 16). Tile `b` holds blue `b / (N - 1)`; within
/// a tile red grows to the right and green downward. Lookups are in sRGB,
/// where LUTs are authored. It follows its sprite's hot reload.
#[derive(Debug, Clone)]
pub struct ColorLut {
  sprite: SpriteHandle,
  size: u32,
}

impl PartialEq for ColorLut {
  fn eq(&self, other: &Self) -> bool {
    self.size == other.size && self.sprite.serial() == other.sprite.serial()
  }
}

/// Why a sprite is not a [`ColorLut`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LutError {
  /// A strip is N x N tiles side by side: N * N wide, N high, N from 2 to 64
  #[error("a LUT strip is N*N wide and N high (N from 2 to 64); this sprite is {width}x{height}")]
  WrongSize { width: u32, height: u32 },
}

/// N for a `width` x `height` strip, if it is one.
pub(crate) fn strip_size(width: u32, height: u32) -> Option<u32> {
  ((2..=64).contains(&height) && width == height * height).then_some(height)
}

impl ColorLut {
  /// The LUT `sprite` holds as a strip.
  pub fn from_sprite(sprite: &SpriteHandle) -> Result<Self, LutError> {
    let (width, height) = (sprite.width(), sprite.height());
    let size = strip_size(width, height).ok_or(LutError::WrongSize { width, height })?;
    Ok(Self {
      sprite: sprite.clone(),
      size,
    })
  }

  /// N: the strip's height.
  pub fn size(&self) -> u32 {
    self.size
  }

  /// The strip's sprite.
  pub(crate) fn sprite(&self) -> &SpriteHandle {
    &self.sprite
  }

  /// The strip that changes nothing, N = `size` (2 to 64): a starting point
  /// for LUTs built in code.
  pub fn identity_strip(size: u32) -> Sprite {
    let n = size.clamp(2, 64);
    let level = |i: u32| ((i * 255 + (n - 1) / 2) / (n - 1)) as u8;
    let mut data = Vec::with_capacity((n * n * n * 4) as usize);
    for g in 0..n {
      for b in 0..n {
        for r in 0..n {
          data.extend_from_slice(&[level(r), level(g), level(b), 255]);
        }
      }
    }
    Sprite::from_rgba(data, n * n, n)
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
  /// White balance multipliers (rgb), w: contrast
  pub(crate) balance: [f32; 4],
  /// x: saturation, y: LUT strength (0 without one), z: the LUT's N, w: 1
  /// when the grading is not neutral
  pub(crate) grade: [f32; 4],
  pub(crate) lift: [f32; 4],
  /// 1 / gamma
  pub(crate) inv_gamma: [f32; 4],
  pub(crate) gain: [f32; 4],
  /// x: 1 when auto-exposure adds its stored EV
  pub(crate) auto: [f32; 4],
}

/// `v` if finite, else `default`, then clamped.
pub(crate) fn num(v: f32, default: f32, min: f32, max: f32) -> f32 {
  if v.is_finite() {
    v.clamp(min, max)
  } else {
    default
  }
}

fn per_channel(v: Vec3, default: f32, min: f32, max: f32) -> [f32; 4] {
  [
    num(v.x, default, min, max),
    num(v.y, default, min, max),
    num(v.z, default, min, max),
    0.0,
  ]
}

/// Luminance weights of linear sRGB (Rec. 709).
const LUMA: Vec3 = Vec3::new(0.2126, 0.7152, 0.0722);

/// Multipliers that warm or cool and tint, keeping a grey's luminance.
fn white_balance(temperature: f32, tint: f32) -> Vec3 {
  let m = Vec3::new(
    1.0 + 0.25 * temperature,
    1.0 - 0.25 * tint,
    1.0 - 0.25 * temperature,
  );
  m / m.dot(LUMA)
}

impl Look {
  /// What the tonemap shader reads, every parameter sanitized; `lut_size`
  /// is the N of the LUT the GPU holds (0 for none).
  pub(crate) fn uniforms(&self, lut_size: u32) -> LookUniforms {
    let exposure = num(self.exposure, 0.0, -16.0, 16.0);
    let (curve, knee, white) = match self.tonemap.sanitized() {
      Tonemap::Soft { knee, white } => (0.0, knee, white),
      Tonemap::Clamp => (1.0, 0.0, 1.0),
      Tonemap::Aces => (2.0, 0.0, 1.0),
      Tonemap::AgX => (3.0, 0.0, 1.0),
    };
    let g = &self.grading;
    let temperature = num(g.temperature, 0.0, -1.0, 1.0);
    let tint = num(g.tint, 0.0, -1.0, 1.0);
    let contrast = num(g.contrast, 1.0, 0.01, 10.0);
    let saturation = num(g.saturation, 1.0, 0.0, 4.0);
    let lift = per_channel(g.lift, 0.0, -1.0, 1.0);
    let gamma = per_channel(g.gamma, 1.0, 0.01, 10.0);
    let gain = per_channel(g.gain, 1.0, 0.0, 10.0);
    let neutral = temperature == 0.0
      && tint == 0.0
      && contrast == 1.0
      && saturation == 1.0
      && lift[..3] == [0.0; 3]
      && gamma[..3] == [1.0; 3]
      && gain[..3] == [1.0; 3];
    let balance = white_balance(temperature, tint);
    let strength = if lut_size > 0 {
      num(g.lut_strength, 1.0, 0.0, 1.0)
    } else {
      0.0
    };
    LookUniforms {
      curve: [exposure, curve, knee, white],
      balance: [balance.x, balance.y, balance.z, contrast],
      grade: [
        saturation,
        strength,
        lut_size as f32,
        if neutral { 0.0 } else { 1.0 },
      ],
      lift,
      inv_gamma: [1.0 / gamma[0], 1.0 / gamma[1], 1.0 / gamma[2], 0.0],
      gain,
      auto: [
        if self.auto_exposure.is_some() {
          1.0
        } else {
          0.0
        },
        0.0,
        0.0,
        0.0,
      ],
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
      ..Look::default()
    };
    let curve = look.uniforms(0).curve;
    assert_eq!(curve[0], 0.0, "NaN exposure is 0 EV");
    assert!(curve[2] <= 0.99 && curve[3] > curve[2], "{curve:?}");
  }

  #[test]
  fn a_lut_strip_is_n_tiles_of_n_by_n() {
    let lut = |w: u32, h: u32| {
      let sprite = crate::core::asset::SpriteHandle::from_sprite(
        crate::core::graphics::Sprite::solid_rect(w, h, 0, 0, 0, 255),
        format!("lut-{w}x{h}"),
      );
      ColorLut::from_sprite(&sprite).map(|lut| lut.size())
    };
    assert_eq!(lut(256, 16), Ok(16));
    assert_eq!(lut(1024, 32), Ok(32));
    assert_eq!(
      lut(16, 16),
      Err(LutError::WrongSize {
        width: 16,
        height: 16
      })
    );
    assert_eq!(
      lut(1, 1),
      Err(LutError::WrongSize {
        width: 1,
        height: 1
      })
    );
  }

  #[test]
  fn the_identity_strip_is_a_lut() {
    let strip = ColorLut::identity_strip(4);
    assert_eq!((strip.width(), strip.height()), (16, 4));
    // Tile b = 3, red 2, green 1: x = 3 * 4 + 2, y = 1
    let at = (16 + 3 * 4 + 2) * 4;
    assert_eq!(&strip.data()[at..at + 4], &[170, 85, 255, 255]);
  }

  #[test]
  fn neutral_grading_is_flagged_off() {
    assert_eq!(Look::default().uniforms(0).grade[3], 0.0);
    let warm = Look {
      grading: ColorGrading {
        temperature: 0.2,
        ..ColorGrading::default()
      },
      ..Look::default()
    };
    assert_eq!(warm.uniforms(0).grade[3], 1.0);
  }

  #[test]
  fn grading_uniforms_are_finite_whatever_the_settings() {
    let grading = ColorGrading {
      temperature: f32::NAN,
      tint: f32::INFINITY,
      contrast: 0.0,
      saturation: -3.0,
      lift: Vec3::splat(f32::NAN),
      gamma: Vec3::ZERO,
      gain: Vec3::splat(f32::INFINITY),
      lut: None,
      lut_strength: f32::NAN,
    };
    let u = Look {
      grading,
      ..Look::default()
    }
    .uniforms(0);
    let all = [u.curve, u.balance, u.grade, u.lift, u.inv_gamma, u.gain].concat();
    assert!(all.iter().all(|v| v.is_finite()), "{u:?}");
    assert_eq!(u.grade[1], 0.0, "no LUT, no strength");
  }
}
