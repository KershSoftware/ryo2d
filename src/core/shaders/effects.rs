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
    /// Distance from the center where darkening begins (0.0 = center, 1.0 = corners)
    radius: f32,
    /// Distance over which the darkening fades in, in the same units as `radius`
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

  /// God rays / light shafts effect
  ///
  /// Creates radial light beams emanating from bright light sources.
  /// Works by sampling along rays from each light position.
  GodRays {
    /// Overall intensity of the effect (0.0 - 2.0)
    intensity: f32,
    /// Number of samples per ray (8-64, more = higher quality)
    samples: u32,
    /// Decay rate per sample (0.9 - 0.99)
    decay: f32,
    /// Density/length of rays (0.5 - 2.0)
    density: f32,
    /// Weight per sample (0.0 - 1.0)
    weight: f32,
    /// Brightness threshold for contribution (0.0 - 1.0)
    threshold: f32,
    /// Light positions in screen UV space (0-1), max 4 lights
    /// Format: [(x, y), ...] where each position is a light
    light_positions: Vec<(f32, f32)>,
  },

  /// Ambient occlusion effect
  ///
  /// Darkens corners and crevices based on edge detection.
  /// Works by detecting edges in 8 directions and darkening where edges converge.
  AmbientOcclusion {
    /// How dark corners get (0.0 = none, 1.0 = fully black)
    intensity: f32,
    /// Sampling radius in pixels (1.0 - 64.0)
    radius: f32,
    /// Edge detection threshold (0.001 - 0.5)
    bias: f32,
  },

  /// Bloom / glow effect
  ///
  /// Extracts bright areas of the scene and blurs them to create a soft glow.
  /// This is a multi-pass effect (extract -> blur -> composite).
  Bloom {
    /// Brightness threshold for extraction (0.0 - 1.0)
    threshold: f32,
    /// Glow strength (0.0 - 5.0)
    intensity: f32,
    /// Blur spread in pixels (1.0 - 20.0)
    radius: f32,
    /// Number of blur passes for quality (1 - 5, each pass = H+V blur)
    blur_passes: u32,
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

  /// Create god rays effect with default settings
  ///
  /// # Arguments
  /// * `intensity` - Overall effect intensity (0.0 - 2.0)
  /// * `light_positions` - Screen-space UV positions of lights (0-1 range)
  pub fn god_rays(intensity: f32, light_positions: Vec<(f32, f32)>) -> Self {
    Self::GodRays {
      intensity: intensity.clamp(0.0, 2.0),
      samples: 16,
      decay: 0.96,
      density: 1.0,
      weight: 0.5,
      threshold: 0.5,
      light_positions,
    }
  }

  /// Create god rays effect with custom settings
  #[allow(clippy::too_many_arguments)]
  pub fn god_rays_custom(
    intensity: f32,
    samples: u32,
    decay: f32,
    density: f32,
    threshold: f32,
    light_positions: Vec<(f32, f32)>,
  ) -> Self {
    Self::GodRays {
      intensity: intensity.clamp(0.0, 2.0),
      samples: samples.clamp(4, 64),
      decay: decay.clamp(0.8, 0.999),
      density: density.clamp(0.1, 3.0),
      weight: 0.5,
      threshold: threshold.clamp(0.0, 1.0),
      light_positions,
    }
  }

  /// Create ambient occlusion effect with default bias
  ///
  /// # Arguments
  /// * `intensity` - How dark corners get (0.0 - 1.0)
  /// * `radius` - Sampling radius in pixels (1.0 - 64.0)
  pub fn ambient_occlusion(intensity: f32, radius: f32) -> Self {
    Self::AmbientOcclusion {
      intensity: intensity.clamp(0.0, 1.0),
      radius: radius.clamp(1.0, 64.0),
      bias: 0.05,
    }
  }

  /// Create ambient occlusion effect with custom settings
  ///
  /// # Arguments
  /// * `intensity` - How dark corners get (0.0 - 1.0)
  /// * `radius` - Sampling radius in pixels (1.0 - 64.0)
  /// * `bias` - Edge detection threshold (0.001 - 0.5)
  pub fn ambient_occlusion_custom(intensity: f32, radius: f32, bias: f32) -> Self {
    Self::AmbientOcclusion {
      intensity: intensity.clamp(0.0, 1.0),
      radius: radius.clamp(1.0, 64.0),
      bias: bias.clamp(0.001, 0.5),
    }
  }

  /// Create a bloom effect with default settings
  ///
  /// # Arguments
  /// * `intensity` - Glow strength (0.0 - 5.0)
  pub fn bloom(intensity: f32) -> Self {
    Self::Bloom {
      threshold: 0.7,
      intensity: intensity.clamp(0.0, 5.0),
      radius: 5.0,
      blur_passes: 2,
    }
  }

  /// Create a bloom effect with custom settings
  ///
  /// # Arguments
  /// * `threshold` - Brightness cutoff (0.0 - 1.0)
  /// * `intensity` - Glow strength (0.0 - 5.0)
  /// * `radius` - Blur spread (1.0 - 20.0)
  /// * `blur_passes` - Quality level (1 - 5, each pass = H+V blur)
  pub fn bloom_custom(threshold: f32, intensity: f32, radius: f32, blur_passes: u32) -> Self {
    Self::Bloom {
      threshold: threshold.clamp(0.0, 1.0),
      intensity: intensity.clamp(0.0, 5.0),
      radius: radius.clamp(1.0, 20.0),
      blur_passes: blur_passes.clamp(1, 5),
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
      Self::GodRays { .. } => "god_rays",
      Self::AmbientOcclusion { .. } => "ambient_occlusion",
      Self::Bloom { .. } => "bloom",
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
      Self::GodRays {
        intensity,
        light_positions,
        ..
      } => *intensity > 0.001 && !light_positions.is_empty(),
      Self::AmbientOcclusion { intensity, .. } => *intensity > 0.001,
      Self::Bloom { intensity, .. } => *intensity > 0.001,
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
      PostProcessEffect::GodRays {
        intensity,
        samples,
        decay,
        density,
        ..
      } => (
        [0.0, 0.0, 0.0, 1.0],
        [*intensity, *samples as f32, *decay, *density],
      ),
      PostProcessEffect::AmbientOcclusion {
        intensity,
        radius,
        bias,
      } => ([0.0, 0.0, 0.0, 1.0], [*intensity, *radius, *bias, 0.0]),
      PostProcessEffect::Bloom {
        threshold,
        intensity,
        radius,
        blur_passes,
      } => (
        [0.0, 0.0, 0.0, 1.0],
        [*threshold, *intensity, *radius, *blur_passes as f32],
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

/// Extended parameters for god rays effect.
///
/// This struct includes the light positions which don't fit in the standard
/// EffectParams. The shader needs to handle this specially.
/// Must be 128 bytes total with 16-byte alignment.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GodRaysParams {
  /// Primary parameters - 16 bytes
  /// x: intensity, y: samples, z: decay, w: density
  pub params: [f32; 4],
  /// Secondary parameters - 16 bytes
  /// x: weight, y: threshold, z: num_lights, w: unused
  pub params2: [f32; 4],
  /// Time + resolution packed - 16 bytes
  pub time_resolution: [f32; 4],
  /// Light position 0 (UV coords) - 16 bytes
  /// x: light0.x, y: light0.y, z: light1.x, w: light1.y
  pub light_pos_01: [f32; 4],
  /// Light position 2-3 (UV coords) - 16 bytes
  /// x: light2.x, y: light2.y, z: light3.x, w: light3.y
  pub light_pos_23: [f32; 4],
  /// Padding to reach 128 bytes - 48 bytes
  pub _padding: [[f32; 4]; 3],
}

impl GodRaysParams {
  /// Create god rays parameters from an effect
  pub fn from_effect(effect: &PostProcessEffect, time: f32, width: u32, height: u32) -> Self {
    if let PostProcessEffect::GodRays {
      intensity,
      samples,
      decay,
      density,
      weight,
      threshold,
      light_positions,
    } = effect
    {
      let num_lights = light_positions.len().min(4) as f32;

      // Pack light positions into vec4s
      let mut light_pos_01 = [0.5, 0.5, 0.5, 0.5]; // default to center
      let mut light_pos_23 = [0.5, 0.5, 0.5, 0.5];

      if !light_positions.is_empty() {
        light_pos_01[0] = light_positions[0].0;
        light_pos_01[1] = light_positions[0].1;
      }
      if light_positions.len() > 1 {
        light_pos_01[2] = light_positions[1].0;
        light_pos_01[3] = light_positions[1].1;
      }
      if light_positions.len() > 2 {
        light_pos_23[0] = light_positions[2].0;
        light_pos_23[1] = light_positions[2].1;
      }
      if light_positions.len() > 3 {
        light_pos_23[2] = light_positions[3].0;
        light_pos_23[3] = light_positions[3].1;
      }

      Self {
        params: [*intensity, *samples as f32, *decay, *density],
        params2: [*weight, *threshold, num_lights, 0.0],
        time_resolution: [time, width as f32, height as f32, 0.0],
        light_pos_01,
        light_pos_23,
        _padding: [[0.0; 4]; 3],
      }
    } else {
      Self::default()
    }
  }
}

impl Default for GodRaysParams {
  fn default() -> Self {
    Self {
      params: [0.8, 16.0, 0.96, 1.0],
      params2: [0.5, 0.5, 0.0, 0.0],
      time_resolution: [0.0, 1.0, 1.0, 0.0],
      light_pos_01: [0.5, 0.5, 0.5, 0.5],
      light_pos_23: [0.5, 0.5, 0.5, 0.5],
      _padding: [[0.0; 4]; 3],
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

  #[test]
  fn god_rays_params_size_is_aligned() {
    // GPU uniform buffers need 16-byte alignment
    assert_eq!(std::mem::size_of::<GodRaysParams>() % 16, 0);
    // Should be 128 bytes
    assert_eq!(std::mem::size_of::<GodRaysParams>(), 128);
  }

  #[test]
  fn god_rays_effect_is_active_with_lights() {
    let effect = PostProcessEffect::god_rays(0.8, vec![(0.5, 0.5)]);
    assert!(effect.is_active());
  }

  #[test]
  fn god_rays_effect_inactive_without_lights() {
    let effect = PostProcessEffect::god_rays(0.8, vec![]);
    assert!(!effect.is_active());
  }

  #[test]
  fn bloom_clamps_parameters() {
    let effect = PostProcessEffect::bloom_custom(2.0, 10.0, 50.0, 10);
    if let PostProcessEffect::Bloom {
      threshold,
      intensity,
      radius,
      blur_passes,
    } = effect
    {
      assert!((threshold - 1.0).abs() < 0.001);
      assert!((intensity - 5.0).abs() < 0.001);
      assert!((radius - 20.0).abs() < 0.001);
      assert_eq!(blur_passes, 5);
    } else {
      panic!("Expected Bloom variant");
    }
  }

  #[test]
  fn bloom_is_active_when_nonzero() {
    assert!(PostProcessEffect::bloom(1.0).is_active());
    assert!(!PostProcessEffect::bloom(0.0).is_active());
  }

  #[test]
  fn bloom_shader_name() {
    assert_eq!(PostProcessEffect::bloom(1.0).shader_name(), "bloom");
  }

  #[test]
  fn bloom_params_packing() {
    let effect = PostProcessEffect::bloom_custom(0.7, 1.5, 5.0, 2);
    let params = EffectParams::from_effect(&effect, 0.0, 1920, 1080);
    assert!((params.params[0] - 0.7).abs() < 0.001); // threshold
    assert!((params.params[1] - 1.5).abs() < 0.001); // intensity
    assert!((params.params[2] - 5.0).abs() < 0.001); // radius
    assert!((params.params[3] - 2.0).abs() < 0.001); // blur_passes
  }

  #[test]
  fn god_rays_params_packs_lights() {
    let effect = PostProcessEffect::god_rays(0.8, vec![(0.1, 0.2), (0.3, 0.4)]);
    let params = GodRaysParams::from_effect(&effect, 0.0, 1920, 1080);

    assert!((params.light_pos_01[0] - 0.1).abs() < 0.001);
    assert!((params.light_pos_01[1] - 0.2).abs() < 0.001);
    assert!((params.light_pos_01[2] - 0.3).abs() < 0.001);
    assert!((params.light_pos_01[3] - 0.4).abs() < 0.001);
    assert!((params.params2[2] - 2.0).abs() < 0.001); // num_lights
  }
}
