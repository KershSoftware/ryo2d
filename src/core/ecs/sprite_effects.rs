//! Per-sprite visual effects
//!
//! These effects are applied to individual sprites during rendering,
//! unlike post-processing effects which operate on the entire frame.
//!
//! # Example
//!
//! ```rust,ignore
//! use ryo2d::prelude::*;
//!
//! // Add an outline to a sprite
//! let player = Renderable::sprite(sprite_handle)
//!     .with_effect(SpriteEffect::outline(Color::WHITE, 1));
//!
//! // Flash a sprite when hit
//! let enemy = Renderable::sprite(sprite_handle)
//!     .with_effect(SpriteEffect::flash(Color::RED, 0.8));
//!
//! // Multiple effects can be combined
//! let boss = Renderable::sprite(sprite_handle)
//!     .with_effect(SpriteEffect::glow(Color::PURPLE, 4.0))
//!     .with_effect(SpriteEffect::color_shift(Color::RED, 0.3));
//! ```

use crate::core::ui::Color;

/// Visual effects that can be applied to individual sprites during rendering.
///
/// Sprite effects are processed in order. Some effects may be combined,
/// while others (like color replacement) will override previous color effects.
#[derive(Debug, Clone, PartialEq)]
pub enum SpriteEffect {
  /// Draw an outline around the sprite's visible pixels
  ///
  /// Uses edge detection to find sprite boundaries and draws a solid color outline.
  Outline {
    /// Color of the outline
    color: Color,
    /// Thickness in pixels (1-4 recommended)
    thickness: u32,
  },

  /// Add a glow/bloom effect around the sprite
  ///
  /// Creates a soft, blurred halo around the sprite's visible pixels.
  Glow {
    /// Color of the glow
    color: Color,
    /// Radius of the glow in pixels
    radius: f32,
    /// Intensity of the glow (0.0-1.0)
    intensity: f32,
  },

  /// Flash the sprite with a solid color (damage flash, power-up)
  ///
  /// Blends the sprite with a solid color. At intensity 1.0, the sprite
  /// becomes a solid silhouette of the flash color.
  Flash {
    /// Color to flash
    color: Color,
    /// Flash intensity (0.0 = normal, 1.0 = solid color)
    intensity: f32,
  },

  /// Shift sprite colors toward a target color
  ///
  /// Useful for status effects (poisoned = green shift, frozen = blue shift)
  ColorShift {
    /// Target color to shift toward
    color: Color,
    /// How much to shift (0.0 = original, 1.0 = fully replaced)
    amount: f32,
  },

  /// Replace all colors with a single color (silhouette mode)
  ///
  /// Preserves alpha but replaces RGB. Good for shadows, death effects.
  Silhouette {
    /// The replacement color
    color: Color,
  },

  /// Apply a pixelation effect
  ///
  /// Groups pixels into larger blocks. Useful for retro/death/transition effects.
  Pixelate {
    /// Size of pixel blocks (2 = 2x2, 4 = 4x4, etc.)
    block_size: u32,
  },

  /// Wave/distortion effect
  ///
  /// Creates a wavy distortion. Good for underwater, heat shimmer, ghost effects.
  Wave {
    /// Amplitude of the wave in pixels
    amplitude: f32,
    /// Frequency of the wave (waves per sprite width)
    frequency: f32,
    /// Speed of wave animation (0.0 for static)
    speed: f32,
    /// Current time offset (updated by engine)
    time: f32,
  },

  /// Dissolve/disintegrate effect
  ///
  /// Uses noise to progressively hide pixels. Perfect for death animations.
  Dissolve {
    /// Progress (0.0 = fully visible, 1.0 = fully dissolved)
    progress: f32,
    /// Edge glow color (optional, for a burning edge effect)
    edge_color: Option<Color>,
  },

  /// Invert all colors
  Invert,

  /// Convert to grayscale
  Grayscale {
    /// Intensity (0.0 = original, 1.0 = fully grayscale)
    intensity: f32,
  },
}

impl SpriteEffect {
  /// Create an outline effect with default thickness (1 pixel)
  pub fn outline(color: Color, thickness: u32) -> Self {
    Self::Outline {
      color,
      thickness: thickness.clamp(1, 8),
    }
  }

  /// Create a white outline (common for selection/highlight)
  pub fn white_outline(thickness: u32) -> Self {
    Self::outline(Color::WHITE, thickness)
  }

  /// Create a glow effect
  pub fn glow(color: Color, radius: f32) -> Self {
    Self::Glow {
      color,
      radius: radius.max(1.0),
      intensity: 0.8,
    }
  }

  /// Create a glow with custom intensity
  pub fn glow_with_intensity(color: Color, radius: f32, intensity: f32) -> Self {
    Self::Glow {
      color,
      radius: radius.max(1.0),
      intensity: intensity.clamp(0.0, 1.0),
    }
  }

  /// Create a damage flash effect
  pub fn flash(color: Color, intensity: f32) -> Self {
    Self::Flash {
      color,
      intensity: intensity.clamp(0.0, 1.0),
    }
  }

  /// Create a red damage flash
  pub fn damage_flash(intensity: f32) -> Self {
    Self::flash(Color::RED, intensity)
  }

  /// Create a white hit flash
  pub fn hit_flash(intensity: f32) -> Self {
    Self::flash(Color::WHITE, intensity)
  }

  /// Create a color shift effect
  pub fn color_shift(color: Color, amount: f32) -> Self {
    Self::ColorShift {
      color,
      amount: amount.clamp(0.0, 1.0),
    }
  }

  /// Create a silhouette (solid color preserving alpha)
  pub fn silhouette(color: Color) -> Self {
    Self::Silhouette { color }
  }

  /// Create a black shadow silhouette
  pub fn shadow() -> Self {
    Self::silhouette(Color::new(0, 0, 0, 128))
  }

  /// Create a pixelation effect
  pub fn pixelate(block_size: u32) -> Self {
    Self::Pixelate {
      block_size: block_size.clamp(2, 32),
    }
  }

  /// Create a wave distortion effect
  pub fn wave(amplitude: f32, frequency: f32, speed: f32) -> Self {
    Self::Wave {
      amplitude,
      frequency,
      speed,
      time: 0.0,
    }
  }

  /// Create a dissolve effect
  pub fn dissolve(progress: f32) -> Self {
    Self::Dissolve {
      progress: progress.clamp(0.0, 1.0),
      edge_color: None,
    }
  }

  /// Create a dissolve with burning edge
  pub fn dissolve_with_edge(progress: f32, edge_color: Color) -> Self {
    Self::Dissolve {
      progress: progress.clamp(0.0, 1.0),
      edge_color: Some(edge_color),
    }
  }

  /// Create an invert effect
  pub fn invert() -> Self {
    Self::Invert
  }

  /// Create a grayscale effect
  pub fn grayscale(intensity: f32) -> Self {
    Self::Grayscale {
      intensity: intensity.clamp(0.0, 1.0),
    }
  }

  /// Full grayscale conversion
  pub fn full_grayscale() -> Self {
    Self::grayscale(1.0)
  }

  /// Check if this effect requires animation updates (time-based effects)
  pub fn is_animated(&self) -> bool {
    matches!(self, Self::Wave { speed, .. } if *speed != 0.0)
  }

  /// Update time-based effects. Returns true if the effect changed.
  pub fn update(&mut self, dt: f32) -> bool {
    match self {
      Self::Wave { speed, time, .. } if *speed != 0.0 => {
        *time += dt * *speed;
        true
      }
      _ => false,
    }
  }

  /// Apply this effect to a copy of sprite pixel data (CPU-based rendering).
  ///
  /// This creates a modified version of the sprite with the effect applied.
  /// Used by the software renderer when GPU shaders are not available.
  ///
  /// # Arguments
  /// * `src` - Source RGBA pixel data (4 bytes per pixel)
  /// * `width` - Sprite width in pixels
  /// * `height` - Sprite height in pixels
  ///
  /// # Returns
  /// Tuple of (pixel_data, new_width, new_height). Some effects like outline and glow
  /// expand the sprite bounds to accommodate the effect.
  pub fn apply_cpu(&self, src: &[u8], width: u32, height: u32) -> (Vec<u8>, u32, u32) {
    let w = width as usize;
    let h = height as usize;

    match self {
      Self::Flash { color, intensity } => {
        let mut dst = src.to_vec();
        // Blend each pixel toward the flash color
        for pixel in dst.chunks_exact_mut(4) {
          if pixel[3] > 0 {
            // Only affect visible pixels
            let t = *intensity;
            pixel[0] = lerp_u8(pixel[0], color.r, t);
            pixel[1] = lerp_u8(pixel[1], color.g, t);
            pixel[2] = lerp_u8(pixel[2], color.b, t);
          }
        }
        (dst, width, height)
      }

      Self::ColorShift { color, amount } => {
        let mut dst = src.to_vec();
        // Shift colors toward target
        for pixel in dst.chunks_exact_mut(4) {
          if pixel[3] > 0 {
            let t = *amount;
            pixel[0] = lerp_u8(pixel[0], color.r, t);
            pixel[1] = lerp_u8(pixel[1], color.g, t);
            pixel[2] = lerp_u8(pixel[2], color.b, t);
          }
        }
        (dst, width, height)
      }

      Self::Silhouette { color } => {
        let mut dst = src.to_vec();
        // Replace all RGB with silhouette color, preserve alpha
        for pixel in dst.chunks_exact_mut(4) {
          if pixel[3] > 0 {
            pixel[0] = color.r;
            pixel[1] = color.g;
            pixel[2] = color.b;
            // Optionally blend alpha: pixel[3] = ((pixel[3] as u16 * color.a as u16) / 255) as u8;
          }
        }
        (dst, width, height)
      }

      Self::Invert => {
        let mut dst = src.to_vec();
        // Invert RGB channels
        for pixel in dst.chunks_exact_mut(4) {
          if pixel[3] > 0 {
            pixel[0] = 255 - pixel[0];
            pixel[1] = 255 - pixel[1];
            pixel[2] = 255 - pixel[2];
          }
        }
        (dst, width, height)
      }

      Self::Grayscale { intensity } => {
        let mut dst = src.to_vec();
        // Convert to grayscale
        for pixel in dst.chunks_exact_mut(4) {
          if pixel[3] > 0 {
            // Standard luminance weights
            let gray =
              (0.299 * pixel[0] as f32 + 0.587 * pixel[1] as f32 + 0.114 * pixel[2] as f32) as u8;
            let t = *intensity;
            pixel[0] = lerp_u8(pixel[0], gray, t);
            pixel[1] = lerp_u8(pixel[1], gray, t);
            pixel[2] = lerp_u8(pixel[2], gray, t);
          }
        }
        (dst, width, height)
      }

      Self::Outline { color, thickness } => {
        // Expand canvas to accommodate outline
        let thick = *thickness as usize;
        let padding = thick;
        let new_w = w + padding * 2;
        let new_h = h + padding * 2;
        let mut dst = vec![0u8; new_w * new_h * 4];

        // Copy source sprite to center of expanded canvas
        for y in 0..h {
          for x in 0..w {
            let src_idx = (y * w + x) * 4;
            let dst_idx = ((y + padding) * new_w + (x + padding)) * 4;
            dst[dst_idx..dst_idx + 4].copy_from_slice(&src[src_idx..src_idx + 4]);
          }
        }

        // Draw outline by checking for transparent neighbors of opaque pixels
        let thick = *thickness as i32;
        let mut outline_mask = vec![false; new_w * new_h];

        // Find outline pixels in the expanded canvas
        for y in 0..new_h {
          for x in 0..new_w {
            let idx = y * new_w + x;
            // If this pixel is transparent
            if dst[idx * 4 + 3] == 0 {
              // Check if any neighbor within thickness is opaque
              'outer: for dy in -thick..=thick {
                for dx in -thick..=thick {
                  if dx == 0 && dy == 0 {
                    continue;
                  }
                  let nx = x as i32 + dx;
                  let ny = y as i32 + dy;
                  if nx >= 0 && nx < new_w as i32 && ny >= 0 && ny < new_h as i32 {
                    let nidx = ny as usize * new_w + nx as usize;
                    if dst[nidx * 4 + 3] > 0 {
                      outline_mask[idx] = true;
                      break 'outer;
                    }
                  }
                }
              }
            }
          }
        }

        // Apply outline
        for (idx, &is_outline) in outline_mask.iter().enumerate() {
          if is_outline {
            dst[idx * 4] = color.r;
            dst[idx * 4 + 1] = color.g;
            dst[idx * 4 + 2] = color.b;
            dst[idx * 4 + 3] = color.a;
          }
        }

        (dst, new_w as u32, new_h as u32)
      }

      Self::Glow {
        color,
        radius,
        intensity,
      } => {
        // Expand canvas to accommodate glow
        let padding = (*radius as usize).max(1);
        let new_w = w + padding * 2;
        let new_h = h + padding * 2;

        // Create expanded canvas with sprite centered
        let mut expanded = vec![0u8; new_w * new_h * 4];
        for y in 0..h {
          for x in 0..w {
            let src_idx = (y * w + x) * 4;
            let dst_idx = ((y + padding) * new_w + (x + padding)) * 4;
            expanded[dst_idx..dst_idx + 4].copy_from_slice(&src[src_idx..src_idx + 4]);
          }
        }

        // Create output buffer starting with the expanded sprite
        let mut dst = expanded.clone();

        // Simple glow: sample from expanded (read-only), write to dst
        let r = *radius as i32;
        let int = *intensity;

        // Calculate glow contribution for transparent pixels near opaque ones
        for y in 0..new_h {
          for x in 0..new_w {
            let idx = y * new_w + x;
            // Only apply glow to fully transparent areas
            if expanded[idx * 4 + 3] == 0 {
              let mut glow_strength = 0.0f32;

              // Sample nearby opaque pixels from the ORIGINAL expanded buffer
              for dy in -r..=r {
                for dx in -r..=r {
                  let nx = x as i32 + dx;
                  let ny = y as i32 + dy;
                  if nx >= 0 && nx < new_w as i32 && ny >= 0 && ny < new_h as i32 {
                    let nidx = ny as usize * new_w + nx as usize;
                    if expanded[nidx * 4 + 3] > 0 {
                      let dist = ((dx * dx + dy * dy) as f32).sqrt();
                      if dist < *radius {
                        let falloff = 1.0 - dist / *radius;
                        glow_strength = glow_strength.max(falloff * int);
                      }
                    }
                  }
                }
              }

              if glow_strength > 0.0 {
                let glow_alpha = (glow_strength * 255.0).min(255.0) as u8;
                dst[idx * 4] = color.r;
                dst[idx * 4 + 1] = color.g;
                dst[idx * 4 + 2] = color.b;
                dst[idx * 4 + 3] = glow_alpha;
              }
            }
          }
        }

        (dst, new_w as u32, new_h as u32)
      }

      Self::Pixelate { block_size } => {
        let bs = *block_size as usize;
        if bs < 2 {
          return (src.to_vec(), width, height);
        }

        let mut dst = src.to_vec();

        // For each block, average colors and fill
        for by in (0..h).step_by(bs) {
          for bx in (0..w).step_by(bs) {
            let mut r_sum = 0u32;
            let mut g_sum = 0u32;
            let mut b_sum = 0u32;
            let mut a_sum = 0u32;
            let mut count = 0u32;

            // Average block
            for dy in 0..bs {
              for dx in 0..bs {
                let x = bx + dx;
                let y = by + dy;
                if x < w && y < h {
                  let idx = (y * w + x) * 4;
                  r_sum += src[idx] as u32;
                  g_sum += src[idx + 1] as u32;
                  b_sum += src[idx + 2] as u32;
                  a_sum += src[idx + 3] as u32;
                  count += 1;
                }
              }
            }

            if count > 0 {
              let r = (r_sum / count) as u8;
              let g = (g_sum / count) as u8;
              let b = (b_sum / count) as u8;
              let a = (a_sum / count) as u8;

              // Fill block
              for dy in 0..bs {
                for dx in 0..bs {
                  let x = bx + dx;
                  let y = by + dy;
                  if x < w && y < h {
                    let idx = (y * w + x) * 4;
                    dst[idx] = r;
                    dst[idx + 1] = g;
                    dst[idx + 2] = b;
                    dst[idx + 3] = a;
                  }
                }
              }
            }
          }
        }
        (dst, width, height)
      }

      Self::Wave {
        amplitude,
        frequency,
        time,
        ..
      } => {
        // Wave distortion - shift pixels horizontally based on y position
        let mut dst = vec![0u8; src.len()];
        for y in 0..h {
          let wave_offset = (*amplitude
            * (y as f32 * *frequency / h as f32 * std::f32::consts::TAU + *time).sin())
            as i32;

          for x in 0..w {
            let src_x = (x as i32 - wave_offset).rem_euclid(w as i32) as usize;
            let dst_idx = (y * w + x) * 4;
            let src_idx = (y * w + src_x) * 4;
            dst[dst_idx..dst_idx + 4].copy_from_slice(&src[src_idx..src_idx + 4]);
          }
        }
        (dst, width, height)
      }

      Self::Dissolve {
        progress,
        edge_color,
      } => {
        let mut dst = src.to_vec();
        // Dissolve using pseudo-random noise based on position
        for y in 0..h {
          for x in 0..w {
            let idx = (y * w + x) * 4;
            if src[idx + 3] > 0 {
              // Simple hash-based noise
              let noise = simple_noise(x as u32, y as u32);
              if noise < *progress {
                // Dissolved - make transparent
                dst[idx + 3] = 0;
              } else if let Some(edge) = edge_color {
                // Edge effect - color pixels near the dissolve threshold
                let edge_dist = (noise - *progress).abs();
                if edge_dist < 0.1 {
                  let t = 1.0 - edge_dist / 0.1;
                  dst[idx] = lerp_u8(dst[idx], edge.r, t);
                  dst[idx + 1] = lerp_u8(dst[idx + 1], edge.g, t);
                  dst[idx + 2] = lerp_u8(dst[idx + 2], edge.b, t);
                }
              }
            }
          }
        }
        (dst, width, height)
      }
    }
  }
}

/// Linear interpolation for u8 values
fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
  (a as f32 + (b as f32 - a as f32) * t).clamp(0.0, 255.0) as u8
}

/// Simple pseudo-random noise function (0.0 to 1.0)
fn simple_noise(x: u32, y: u32) -> f32 {
  // Hash function
  let mut n = x
    .wrapping_mul(374761393)
    .wrapping_add(y.wrapping_mul(668265263));
  n = (n ^ (n >> 13)).wrapping_mul(1274126177);
  n ^= n >> 16;
  (n as f32) / (u32::MAX as f32)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn outline_clamps_thickness() {
    let effect = SpriteEffect::outline(Color::WHITE, 100);
    if let SpriteEffect::Outline { thickness, .. } = effect {
      assert_eq!(thickness, 8); // max is 8
    }

    let thin = SpriteEffect::outline(Color::WHITE, 0);
    if let SpriteEffect::Outline { thickness, .. } = thin {
      assert_eq!(thickness, 1); // min is 1
    }
  }

  #[test]
  fn white_outline_convenience() {
    let effect = SpriteEffect::white_outline(2);
    match effect {
      SpriteEffect::Outline { color, thickness } => {
        assert_eq!(color, Color::WHITE);
        assert_eq!(thickness, 2);
      }
      _ => panic!("Expected Outline effect"),
    }
  }

  #[test]
  fn glow_defaults() {
    let effect = SpriteEffect::glow(Color::YELLOW, 5.0);
    match effect {
      SpriteEffect::Glow {
        color,
        radius,
        intensity,
      } => {
        assert_eq!(color, Color::YELLOW);
        assert_eq!(radius, 5.0);
        assert_eq!(intensity, 0.8);
      }
      _ => panic!("Expected Glow effect"),
    }
  }

  #[test]
  fn glow_with_custom_intensity() {
    let effect = SpriteEffect::glow_with_intensity(Color::BLUE, 3.0, 0.5);
    match effect {
      SpriteEffect::Glow { intensity, .. } => {
        assert_eq!(intensity, 0.5);
      }
      _ => panic!("Expected Glow effect"),
    }
  }

  #[test]
  fn glow_clamps_intensity() {
    let too_high = SpriteEffect::glow_with_intensity(Color::RED, 2.0, 2.0);
    if let SpriteEffect::Glow { intensity, .. } = too_high {
      assert_eq!(intensity, 1.0);
    }

    let too_low = SpriteEffect::glow_with_intensity(Color::RED, 2.0, -0.5);
    if let SpriteEffect::Glow { intensity, .. } = too_low {
      assert_eq!(intensity, 0.0);
    }
  }

  #[test]
  fn flash_clamps_intensity() {
    let effect = SpriteEffect::flash(Color::RED, 2.0);
    if let SpriteEffect::Flash { intensity, .. } = effect {
      assert!((intensity - 1.0).abs() < 0.001);
    }

    let low = SpriteEffect::flash(Color::RED, -0.5);
    if let SpriteEffect::Flash { intensity, .. } = low {
      assert_eq!(intensity, 0.0);
    }
  }

  #[test]
  fn damage_flash_convenience() {
    let effect = SpriteEffect::damage_flash(0.6);
    match effect {
      SpriteEffect::Flash { color, intensity } => {
        assert_eq!(color, Color::RED);
        assert_eq!(intensity, 0.6);
      }
      _ => panic!("Expected Flash effect"),
    }
  }

  #[test]
  fn hit_flash_convenience() {
    let effect = SpriteEffect::hit_flash(0.9);
    match effect {
      SpriteEffect::Flash { color, intensity } => {
        assert_eq!(color, Color::WHITE);
        assert_eq!(intensity, 0.9);
      }
      _ => panic!("Expected Flash effect"),
    }
  }

  #[test]
  fn color_shift_clamps_amount() {
    let high = SpriteEffect::color_shift(Color::GREEN, 1.5);
    if let SpriteEffect::ColorShift { amount, .. } = high {
      assert_eq!(amount, 1.0);
    }

    let low = SpriteEffect::color_shift(Color::GREEN, -0.2);
    if let SpriteEffect::ColorShift { amount, .. } = low {
      assert_eq!(amount, 0.0);
    }
  }

  #[test]
  fn silhouette_creation() {
    let effect = SpriteEffect::silhouette(Color::new(100, 50, 200, 255));
    match effect {
      SpriteEffect::Silhouette { color } => {
        assert_eq!(color.r, 100);
        assert_eq!(color.g, 50);
        assert_eq!(color.b, 200);
      }
      _ => panic!("Expected Silhouette effect"),
    }
  }

  #[test]
  fn shadow_convenience() {
    let effect = SpriteEffect::shadow();
    match effect {
      SpriteEffect::Silhouette { color } => {
        assert_eq!(color.r, 0);
        assert_eq!(color.g, 0);
        assert_eq!(color.b, 0);
        assert_eq!(color.a, 128);
      }
      _ => panic!("Expected Silhouette effect"),
    }
  }

  #[test]
  fn pixelate_clamps_block_size() {
    let large = SpriteEffect::pixelate(100);
    if let SpriteEffect::Pixelate { block_size } = large {
      assert_eq!(block_size, 32); // max is 32
    }

    let small = SpriteEffect::pixelate(1);
    if let SpriteEffect::Pixelate { block_size } = small {
      assert_eq!(block_size, 2); // min is 2
    }
  }

  #[test]
  fn wave_creation() {
    let effect = SpriteEffect::wave(3.0, 2.0, 1.5);
    match effect {
      SpriteEffect::Wave {
        amplitude,
        frequency,
        speed,
        time,
      } => {
        assert_eq!(amplitude, 3.0);
        assert_eq!(frequency, 2.0);
        assert_eq!(speed, 1.5);
        assert_eq!(time, 0.0);
      }
      _ => panic!("Expected Wave effect"),
    }
  }

  #[test]
  fn wave_is_animated() {
    let animated = SpriteEffect::wave(2.0, 1.0, 1.0);
    let static_wave = SpriteEffect::wave(2.0, 1.0, 0.0);

    assert!(animated.is_animated());
    assert!(!static_wave.is_animated());
  }

  #[test]
  fn wave_updates_time() {
    let mut effect = SpriteEffect::wave(2.0, 1.0, 2.0);

    let changed = effect.update(0.5);
    assert!(changed);

    if let SpriteEffect::Wave { time, .. } = effect {
      assert!((time - 1.0).abs() < 0.001); // 0.5 * 2.0 = 1.0
    }
  }

  #[test]
  fn non_animated_effects_dont_update() {
    let mut flash = SpriteEffect::flash(Color::RED, 0.5);
    let mut invert = SpriteEffect::invert();
    let mut grayscale = SpriteEffect::grayscale(0.5);

    assert!(!flash.update(1.0));
    assert!(!invert.update(1.0));
    assert!(!grayscale.update(1.0));
  }

  #[test]
  fn dissolve_clamps_progress() {
    let effect = SpriteEffect::dissolve(1.5);
    if let SpriteEffect::Dissolve { progress, .. } = effect {
      assert!((progress - 1.0).abs() < 0.001);
    }

    let low = SpriteEffect::dissolve(-0.1);
    if let SpriteEffect::Dissolve { progress, .. } = low {
      assert_eq!(progress, 0.0);
    }
  }

  #[test]
  fn dissolve_with_edge() {
    let effect = SpriteEffect::dissolve_with_edge(0.5, Color::RED);
    match effect {
      SpriteEffect::Dissolve {
        progress,
        edge_color,
      } => {
        assert_eq!(progress, 0.5);
        assert_eq!(edge_color, Some(Color::RED));
      }
      _ => panic!("Expected Dissolve effect"),
    }
  }

  #[test]
  fn dissolve_without_edge() {
    let effect = SpriteEffect::dissolve(0.3);
    match effect {
      SpriteEffect::Dissolve {
        progress,
        edge_color,
      } => {
        assert_eq!(progress, 0.3);
        assert_eq!(edge_color, None);
      }
      _ => panic!("Expected Dissolve effect"),
    }
  }

  #[test]
  fn invert_creation() {
    let effect = SpriteEffect::invert();
    assert!(matches!(effect, SpriteEffect::Invert));
  }

  #[test]
  fn grayscale_clamps_intensity() {
    let high = SpriteEffect::grayscale(2.0);
    if let SpriteEffect::Grayscale { intensity } = high {
      assert_eq!(intensity, 1.0);
    }

    let low = SpriteEffect::grayscale(-0.5);
    if let SpriteEffect::Grayscale { intensity } = low {
      assert_eq!(intensity, 0.0);
    }
  }

  #[test]
  fn full_grayscale_convenience() {
    let effect = SpriteEffect::full_grayscale();
    match effect {
      SpriteEffect::Grayscale { intensity } => {
        assert_eq!(intensity, 1.0);
      }
      _ => panic!("Expected Grayscale effect"),
    }
  }

  #[test]
  fn effect_equality() {
    let flash1 = SpriteEffect::flash(Color::RED, 0.5);
    let flash2 = SpriteEffect::flash(Color::RED, 0.5);
    let flash3 = SpriteEffect::flash(Color::BLUE, 0.5);

    assert_eq!(flash1, flash2);
    assert_ne!(flash1, flash3);
  }

  #[test]
  fn effect_clone() {
    let original = SpriteEffect::glow(Color::GREEN, 3.0);
    let cloned = original.clone();
    assert_eq!(original, cloned);
  }
}
