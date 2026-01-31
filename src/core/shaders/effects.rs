//! Post-processing effect definitions
//!
//! Each effect is a self-contained visual transformation applied to the
//! rendered frame. Effects can be stacked and composed.

use super::super::ui::Color;

/// A post-processing effect that can be applied to the rendered frame.
///
/// Effects are applied in order: earlier effects in the stack are processed first.
/// Each effect reads from the previous output and writes to a new texture.
#[derive(Debug, Clone, PartialEq)]
pub enum PostProcessEffect {
  /// Fade the screen to a color (typically black for scene transitions)
  ///
  /// - `progress`: 0.0 = fully visible, 1.0 = fully faded to color
  /// - `color`: The color to fade to (default: black)
  Fade { progress: f32, color: Color },

  /// Apply a color tint/grade to the entire screen
  ///
  /// Useful for mood effects (sepia, night vision, underwater blue)
  ColorTint {
    /// The tint color to blend
    color: Color,
    /// How strongly to apply the tint (0.0 = none, 1.0 = full replacement)
    intensity: f32,
  },

  /// Darken the edges of the screen for a cinematic look
  Vignette {
    /// How dark the edges get (0.0 = none, 1.0 = fully black)
    intensity: f32,
    /// How far the vignette extends from edges (0.0 = edges only, 1.0 = center)
    radius: f32,
    /// How soft/gradual the vignette transition is
    softness: f32,
  },

  /// Flash the entire screen with a color (damage, power-up, etc.)
  ///
  /// This is typically a one-shot effect that fades out quickly.
  ScreenFlash {
    /// The flash color
    color: Color,
    /// Current intensity (1.0 = full flash, fades to 0.0)
    intensity: f32,
  },

  /// CRT monitor scanline effect for retro aesthetics
  Crt {
    /// Intensity of scanlines (0.0 = none, 1.0 = very dark lines)
    scanline_intensity: f32,
    /// Screen curvature amount (0.0 = flat, 0.1 = subtle curve)
    curvature: f32,
    /// RGB separation amount for color bleeding
    rgb_offset: f32,
  },

  /// Chromatic aberration (RGB color fringing)
  ///
  /// Subtle amounts add polish; high amounts create a glitchy look.
  ChromaticAberration {
    /// How far to offset RGB channels (in UV space, 0.001-0.01 is subtle)
    offset: f32,
    /// Direction of the aberration (radial from center or directional)
    radial: bool,
  },
}

impl PostProcessEffect {
  /// Create a black fade at the given progress
  pub fn fade(progress: f32) -> Self {
    Self::Fade {
      progress: progress.clamp(0.0, 1.0),
      color: Color::BLACK,
    }
  }

  /// Create a fade to a specific color
  pub fn fade_to_color(progress: f32, color: Color) -> Self {
    Self::Fade {
      progress: progress.clamp(0.0, 1.0),
      color,
    }
  }

  /// Create a color tint effect
  pub fn color_tint(color: Color, intensity: f32) -> Self {
    Self::ColorTint {
      color,
      intensity: intensity.clamp(0.0, 1.0),
    }
  }

  /// Create a sepia tone effect
  pub fn sepia(intensity: f32) -> Self {
    Self::ColorTint {
      color: Color::new(255, 220, 180, 255),
      intensity: intensity.clamp(0.0, 1.0),
    }
  }

  /// Create a vignette with default settings
  pub fn vignette(intensity: f32) -> Self {
    Self::Vignette {
      intensity: intensity.clamp(0.0, 1.0),
      radius: 0.7,
      softness: 0.4,
    }
  }

  /// Create a vignette with custom settings
  pub fn vignette_custom(intensity: f32, radius: f32, softness: f32) -> Self {
    Self::Vignette {
      intensity: intensity.clamp(0.0, 1.0),
      radius: radius.clamp(0.0, 1.0),
      softness: softness.clamp(0.0, 1.0),
    }
  }

  /// Create a screen flash effect
  pub fn flash(color: Color, intensity: f32) -> Self {
    Self::ScreenFlash {
      color,
      intensity: intensity.clamp(0.0, 1.0),
    }
  }

  /// Create a white damage flash
  pub fn flash_white(intensity: f32) -> Self {
    Self::flash(Color::WHITE, intensity)
  }

  /// Create a red damage flash
  pub fn flash_red(intensity: f32) -> Self {
    Self::flash(Color::RED, intensity)
  }

  /// Create a CRT effect with default settings
  pub fn crt() -> Self {
    Self::Crt {
      scanline_intensity: 0.2,
      curvature: 0.03,
      rgb_offset: 0.002,
    }
  }

  /// Create a CRT effect with custom settings
  pub fn crt_custom(scanline_intensity: f32, curvature: f32, rgb_offset: f32) -> Self {
    Self::Crt {
      scanline_intensity: scanline_intensity.clamp(0.0, 1.0),
      curvature: curvature.clamp(0.0, 0.2),
      rgb_offset: rgb_offset.clamp(0.0, 0.05),
    }
  }

  /// Create chromatic aberration with default settings
  pub fn chromatic_aberration(offset: f32) -> Self {
    Self::ChromaticAberration {
      offset: offset.clamp(0.0, 0.05),
      radial: true,
    }
  }

  /// Get the shader name for this effect
  pub fn shader_name(&self) -> &'static str {
    match self {
      Self::Fade { .. } => "fade",
      Self::ColorTint { .. } => "color_tint",
      Self::Vignette { .. } => "vignette",
      Self::ScreenFlash { .. } => "screen_flash",
      Self::Crt { .. } => "crt",
      Self::ChromaticAberration { .. } => "chromatic_aberration",
    }
  }

  /// Check if this effect is currently active (has visible impact)
  pub fn is_active(&self) -> bool {
    match self {
      Self::Fade { progress, .. } => *progress > 0.001,
      Self::ColorTint { intensity, .. } => *intensity > 0.001,
      Self::Vignette { intensity, .. } => *intensity > 0.001,
      Self::ScreenFlash { intensity, .. } => *intensity > 0.001,
      Self::Crt {
        scanline_intensity, ..
      } => *scanline_intensity > 0.001,
      Self::ChromaticAberration { offset, .. } => *offset > 0.0001,
    }
  }
}

/// Uniform parameters for a post-process effect.
///
/// This is the data sent to the GPU for each effect.
/// Must be 64 bytes total with 16-byte alignment for WGSL uniform buffers.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct EffectParams {
  /// Primary color (RGBA, normalized 0-1) - 16 bytes
  pub color: [f32; 4],
  /// Effect-specific parameters - 16 bytes
  /// - Fade: [progress, 0, 0, 0]
  /// - ColorTint: [intensity, 0, 0, 0]
  /// - Vignette: [intensity, radius, softness, 0]
  /// - ScreenFlash: [intensity, 0, 0, 0]
  /// - CRT: [scanline_intensity, curvature, rgb_offset, 0]
  /// - ChromaticAberration: [offset, radial(0/1), 0, 0]
  pub params: [f32; 4],
  /// Time + resolution packed into vec4 for alignment - 16 bytes
  /// x: time, y: resolution.x, z: resolution.y, w: unused
  pub time_resolution: [f32; 4],
  /// Padding to reach 64 bytes - 16 bytes
  pub _padding: [f32; 4],
}

impl EffectParams {
  /// Create parameters for an effect
  pub fn from_effect(effect: &PostProcessEffect, time: f32, width: u32, height: u32) -> Self {
    let (color, params) = match effect {
      PostProcessEffect::Fade { progress, color } => {
        (color.to_normalized(), [*progress, 0.0, 0.0, 0.0])
      }
      PostProcessEffect::ColorTint { color, intensity } => {
        (color.to_normalized(), [*intensity, 0.0, 0.0, 0.0])
      }
      PostProcessEffect::Vignette {
        intensity,
        radius,
        softness,
      } => ([0.0, 0.0, 0.0, 1.0], [*intensity, *radius, *softness, 0.0]),
      PostProcessEffect::ScreenFlash { color, intensity } => {
        (color.to_normalized(), [*intensity, 0.0, 0.0, 0.0])
      }
      PostProcessEffect::Crt {
        scanline_intensity,
        curvature,
        rgb_offset,
      } => (
        [0.0, 0.0, 0.0, 1.0],
        [*scanline_intensity, *curvature, *rgb_offset, 0.0],
      ),
      PostProcessEffect::ChromaticAberration { offset, radial } => (
        [0.0, 0.0, 0.0, 1.0],
        [*offset, if *radial { 1.0 } else { 0.0 }, 0.0, 0.0],
      ),
    };

    Self {
      color,
      params,
      time_resolution: [time, width as f32, height as f32, 0.0],
      _padding: [0.0; 4],
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn fade_clamps_progress() {
    let effect = PostProcessEffect::fade(1.5);
    if let PostProcessEffect::Fade { progress, .. } = effect {
      assert!((progress - 1.0).abs() < 0.001);
    }
  }

  #[test]
  fn effect_is_active_when_nonzero() {
    assert!(PostProcessEffect::fade(0.5).is_active());
    assert!(!PostProcessEffect::fade(0.0).is_active());
  }

  #[test]
  fn effect_params_size_is_aligned() {
    // GPU uniform buffers need 16-byte alignment
    assert_eq!(std::mem::size_of::<EffectParams>() % 16, 0);
  }
}
