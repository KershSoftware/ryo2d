//! Effects manager for handling effect stacks
//!
//! The manager tracks active effects, handles timed effects (like flashes),
//! and provides a clean API for enabling/disabling effects.

use super::super::ui::Color;
use super::effects::PostProcessEffect;
use crate::core::ecs::{ParamValue, ShaderHandle, ShaderParams};
use std::time::Duration;

/// Why a custom effect could not be registered
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EffectError {
  /// A custom effect with this name already exists
  #[error("a custom effect named \"{0}\" already exists")]
  DuplicateName(String),
}

/// A game-written full-screen effect with its parameters
#[derive(Debug, Clone)]
pub(crate) struct CustomEffect {
  name: String,
  shader: ShaderHandle,
  params: ShaderParams,
  enabled: bool,
}

impl CustomEffect {
  pub(crate) fn shader(&self) -> &ShaderHandle {
    &self.shader
  }

  pub(crate) fn params(&self) -> &ShaderParams {
    &self.params
  }
}

/// Trait for types that can have post-processing effects applied.
///
/// This trait is implemented by both `GameContext` (for global effects)
/// and `Camera` (for per-camera effects). It provides a common interface
/// for effect management.
///
/// # Example
///
/// ```rust,ignore
/// // Both work with the same API
/// ctx.effects().set_fade(0.5);
/// camera.effects().set_vignette(0.3);
/// ```
pub trait EffectTarget {
  /// Get a reference to the effects manager
  fn effects(&self) -> &EffectsManager;

  /// Get a mutable reference to the effects manager
  fn effects_mut(&mut self) -> &mut EffectsManager;
}

/// Manages post-processing effects for a render target.
///
/// The manager handles:
/// - Active effect stack with ordering
/// - Timed effects (flash, fade) with automatic decay
/// - Persistent effects (vignette, CRT)
#[derive(Debug, Clone)]
pub struct EffectsManager {
  /// Currently active effects in render order
  effects: Vec<ActiveEffect>,
  /// Game-written effects, run after the built-ins in insertion order
  custom: Vec<CustomEffect>,
  /// Global time accumulator for animated effects
  time: f32,
}

/// An effect with optional timing/animation state
#[derive(Debug, Clone)]
struct ActiveEffect {
  /// The effect configuration
  effect: PostProcessEffect,
  /// Effect category for replacement logic
  category: EffectCategory,
  /// Remaining duration (None = persistent)
  remaining: Option<f32>,
  /// Initial duration for progress calculation
  initial_duration: Option<f32>,
}

/// Categories for effect replacement logic
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EffectCategory {
  Fade,
  ColorTint,
  Vignette,
  ScreenFlash,
  Crt,
  ChromaticAberration,
  GodRays,
  AmbientOcclusion,
  Bloom,
}

impl EffectCategory {
  fn from_effect(effect: &PostProcessEffect) -> Self {
    match effect {
      PostProcessEffect::Fade { .. } => Self::Fade,
      PostProcessEffect::ColorTint { .. } => Self::ColorTint,
      PostProcessEffect::Vignette { .. } => Self::Vignette,
      PostProcessEffect::ScreenFlash { .. } => Self::ScreenFlash,
      PostProcessEffect::Crt { .. } => Self::Crt,
      PostProcessEffect::ChromaticAberration { .. } => Self::ChromaticAberration,
      PostProcessEffect::GodRays { .. } => Self::GodRays,
      PostProcessEffect::AmbientOcclusion { .. } => Self::AmbientOcclusion,
      PostProcessEffect::Bloom { .. } => Self::Bloom,
    }
  }
}

impl Default for EffectsManager {
  fn default() -> Self {
    Self::new()
  }
}

impl EffectsManager {
  /// Create a new effects manager with no active effects
  pub fn new() -> Self {
    Self {
      effects: Vec::with_capacity(8),
      custom: Vec::new(),
      time: 0.0,
    }
  }

  /// Update timed effects (call once per frame with delta time)
  pub fn update(&mut self, dt: f32) {
    self.time += dt;

    // Update timed effects
    self.effects.retain_mut(|active| {
      if let Some(remaining) = &mut active.remaining {
        *remaining -= dt;
        if *remaining <= 0.0 {
          return false; // Remove expired effect
        }

        // Update effect intensity based on remaining time
        if let Some(initial) = active.initial_duration {
          let progress = *remaining / initial;
          active.effect = Self::update_timed_effect(&active.effect, progress);
        }
      }
      true
    });
  }

  /// Update a timed effect's intensity based on remaining progress
  fn update_timed_effect(effect: &PostProcessEffect, progress: f32) -> PostProcessEffect {
    match effect {
      PostProcessEffect::ScreenFlash { color, .. } => {
        PostProcessEffect::ScreenFlash {
          color: *color,
          intensity: progress, // Fade out as time progresses
        }
      }
      // Other effects maintain their parameters
      other => other.clone(),
    }
  }

  /// Get current time value for shader animations
  pub fn time(&self) -> f32 {
    self.time
  }

  /// Get all currently active effects
  pub fn active_effects(&self) -> impl Iterator<Item = &PostProcessEffect> {
    self
      .effects
      .iter()
      .map(|a| &a.effect)
      .filter(|e| e.is_active())
  }

  /// Check if any effects are currently active
  pub fn has_active_effects(&self) -> bool {
    self.effects.iter().any(|a| a.effect.is_active()) || self.custom.iter().any(|c| c.enabled)
  }

  /// Clear all effects, custom ones included
  pub fn clear(&mut self) {
    self.effects.clear();
    self.custom.clear();
  }

  // ========== Custom Effects ==========

  /// Register a game-written full-screen effect under `name`.
  ///
  /// The shader defines `fn effect(in: EffectIn, params: Params) -> vec4<f32>`
  /// and reads the image so far with `scene(uv)`. Custom effects run after all
  /// built-in effects, in the order they were added, while enabled.
  ///
  /// Colour space: the scene texture is sRGB-format, so `scene()` returns
  /// linear colour and the returned colour is linear too.
  ///
  /// A shader that fails to compile is logged once and skipped; the rest of
  /// the chain still runs.
  ///
  /// # Errors
  ///
  /// [`EffectError::DuplicateName`] if `name` is already registered.
  pub fn add_custom(
    &mut self,
    name: impl Into<String>,
    shader: ShaderHandle,
  ) -> Result<(), EffectError> {
    let name = name.into();
    if self.has_custom(&name) {
      return Err(EffectError::DuplicateName(name));
    }
    self.custom.push(CustomEffect {
      name,
      shader,
      params: ShaderParams::new(),
      enabled: true,
    });
    Ok(())
  }

  /// Set a parameter of the custom effect `name`
  ///
  /// Returns `false` for an unknown effect name.
  pub fn set_custom_param(
    &mut self,
    name: &str,
    param: &str,
    value: impl Into<ParamValue>,
  ) -> bool {
    match self.custom.iter_mut().find(|c| c.name == name) {
      Some(effect) => {
        effect.params.set(param, value);
        true
      }
      None => false,
    }
  }

  /// Enable or disable the custom effect `name`
  ///
  /// Returns `false` for an unknown effect name.
  pub fn set_custom_enabled(&mut self, name: &str, enabled: bool) -> bool {
    match self.custom.iter_mut().find(|c| c.name == name) {
      Some(effect) => {
        effect.enabled = enabled;
        true
      }
      None => false,
    }
  }

  /// Remove the custom effect `name`
  ///
  /// Returns `false` for an unknown effect name.
  pub fn remove_custom(&mut self, name: &str) -> bool {
    let before = self.custom.len();
    self.custom.retain(|c| c.name != name);
    self.custom.len() != before
  }

  /// Whether a custom effect named `name` is registered (enabled or not)
  pub fn has_custom(&self, name: &str) -> bool {
    self.custom.iter().any(|c| c.name == name)
  }

  /// Enabled custom effects in the order they run
  pub(crate) fn enabled_custom(&self) -> impl Iterator<Item = &CustomEffect> {
    self.custom.iter().filter(|c| c.enabled)
  }

  // ========== Convenience Methods ==========

  /// Set screen fade progress (0.0 = visible, 1.0 = faded to black)
  pub fn set_fade(&mut self, progress: f32) {
    self.set_fade_color(progress, Color::BLACK);
  }

  /// Set screen fade with custom color
  pub fn set_fade_color(&mut self, progress: f32, color: Color) {
    let effect = PostProcessEffect::fade_to_color(progress, color);
    self.set_effect(effect);
  }

  /// Set color tint
  pub fn set_color_tint(&mut self, color: Color, intensity: f32) {
    let effect = PostProcessEffect::color_tint(color, intensity);
    self.set_effect(effect);
  }

  /// Set sepia tone
  pub fn set_sepia(&mut self, intensity: f32) {
    let effect = PostProcessEffect::sepia(intensity);
    self.set_effect(effect);
  }

  /// Set vignette intensity (0.0 = off, 1.0 = strong)
  pub fn set_vignette(&mut self, intensity: f32) {
    let effect = PostProcessEffect::vignette(intensity);
    self.set_effect(effect);
  }

  /// Set vignette with custom parameters
  ///
  /// Darkening begins `radius` from the center (0.0 = center, 1.0 = corners)
  /// and reaches `intensity` over the next `softness`.
  pub fn set_vignette_custom(&mut self, intensity: f32, radius: f32, softness: f32) {
    let effect = PostProcessEffect::vignette_custom(intensity, radius, softness);
    self.set_effect(effect);
  }

  /// Trigger a screen flash that fades out over the given duration
  pub fn flash(&mut self, color: Color, duration: Duration) {
    self.flash_secs(color, duration.as_secs_f32());
  }

  /// Trigger a screen flash (duration in seconds)
  pub fn flash_secs(&mut self, color: Color, duration_secs: f32) {
    let effect = PostProcessEffect::flash(color, 1.0);
    self.set_timed_effect(effect, duration_secs);
  }

  /// Trigger a white flash
  pub fn flash_white(&mut self, duration: Duration) {
    self.flash(Color::WHITE, duration);
  }

  /// Trigger a red damage flash
  pub fn flash_red(&mut self, duration: Duration) {
    self.flash(Color::RED, duration);
  }

  /// Enable CRT effect with default settings
  pub fn enable_crt(&mut self) {
    self.set_effect(PostProcessEffect::crt());
  }

  /// Enable CRT effect with custom settings
  pub fn enable_crt_custom(&mut self, scanline_intensity: f32, curvature: f32, rgb_offset: f32) {
    let effect = PostProcessEffect::crt_custom(scanline_intensity, curvature, rgb_offset);
    self.set_effect(effect);
  }

  /// Disable CRT effect
  pub fn disable_crt(&mut self) {
    self.remove_category(EffectCategory::Crt);
  }

  /// Enable chromatic aberration
  pub fn set_chromatic_aberration(&mut self, offset: f32) {
    let effect = PostProcessEffect::chromatic_aberration(offset);
    self.set_effect(effect);
  }

  /// Disable chromatic aberration
  pub fn disable_chromatic_aberration(&mut self) {
    self.remove_category(EffectCategory::ChromaticAberration);
  }

  /// Enable god rays effect
  ///
  /// # Arguments
  /// * `intensity` - Overall effect intensity (0.0 - 2.0)
  /// * `light_positions` - Screen-space UV positions of lights (0-1 range)
  pub fn set_god_rays(&mut self, intensity: f32, light_positions: Vec<(f32, f32)>) {
    let effect = PostProcessEffect::god_rays(intensity, light_positions);
    self.set_effect(effect);
  }

  /// Enable god rays with custom settings
  #[allow(clippy::too_many_arguments)]
  pub fn set_god_rays_custom(
    &mut self,
    intensity: f32,
    samples: u32,
    decay: f32,
    density: f32,
    threshold: f32,
    light_positions: Vec<(f32, f32)>,
  ) {
    let effect = PostProcessEffect::god_rays_custom(
      intensity,
      samples,
      decay,
      density,
      threshold,
      light_positions,
    );
    self.set_effect(effect);
  }

  /// Update god rays light positions (keeps other settings)
  ///
  /// This is more efficient than recreating the effect each frame.
  pub fn update_god_rays_lights(&mut self, light_positions: Vec<(f32, f32)>) {
    // Find existing god rays effect and update it
    for active in &mut self.effects {
      if let PostProcessEffect::GodRays {
        light_positions: positions,
        ..
      } = &mut active.effect
      {
        *positions = light_positions;
        return;
      }
    }
  }

  /// Disable god rays
  pub fn disable_god_rays(&mut self) {
    self.remove_category(EffectCategory::GodRays);
  }

  /// Check if god rays effect is enabled
  pub fn has_god_rays(&self) -> bool {
    self
      .effects
      .iter()
      .any(|a| matches!(a.effect, PostProcessEffect::GodRays { .. }))
  }

  /// Enable ambient occlusion effect
  ///
  /// # Arguments
  /// * `intensity` - How dark corners get (0.0 - 1.0)
  /// * `radius` - Sampling radius in pixels (1.0 - 64.0)
  pub fn set_ambient_occlusion(&mut self, intensity: f32, radius: f32) {
    let effect = PostProcessEffect::ambient_occlusion(intensity, radius);
    self.set_effect(effect);
  }

  /// Enable ambient occlusion effect with custom settings
  ///
  /// # Arguments
  /// * `intensity` - How dark corners get (0.0 - 1.0)
  /// * `radius` - Sampling radius in pixels (1.0 - 64.0)
  /// * `bias` - Edge detection threshold (0.001 - 0.5)
  pub fn set_ambient_occlusion_custom(&mut self, intensity: f32, radius: f32, bias: f32) {
    let effect = PostProcessEffect::ambient_occlusion_custom(intensity, radius, bias);
    self.set_effect(effect);
  }

  /// Disable ambient occlusion
  pub fn disable_ambient_occlusion(&mut self) {
    self.remove_category(EffectCategory::AmbientOcclusion);
  }

  /// Check if ambient occlusion effect is enabled
  pub fn has_ambient_occlusion(&self) -> bool {
    self
      .effects
      .iter()
      .any(|a| matches!(a.effect, PostProcessEffect::AmbientOcclusion { .. }))
  }

  /// Enable bloom effect with default settings
  ///
  /// # Arguments
  /// * `intensity` - Glow strength (0.0 - 5.0)
  pub fn set_bloom(&mut self, intensity: f32) {
    let effect = PostProcessEffect::bloom(intensity);
    self.set_effect(effect);
  }

  /// Enable bloom effect with custom settings
  ///
  /// # Arguments
  /// * `threshold` - Brightness cutoff (0.0 - 1.0)
  /// * `intensity` - Glow strength (0.0 - 5.0)
  /// * `radius` - Blur spread (1.0 - 20.0)
  /// * `blur_passes` - Quality level (1 - 5, each pass = H+V blur)
  pub fn set_bloom_custom(
    &mut self,
    threshold: f32,
    intensity: f32,
    radius: f32,
    blur_passes: u32,
  ) {
    let effect = PostProcessEffect::bloom_custom(threshold, intensity, radius, blur_passes);
    self.set_effect(effect);
  }

  /// Disable bloom effect
  pub fn disable_bloom(&mut self) {
    self.remove_category(EffectCategory::Bloom);
  }

  /// Check if bloom effect is enabled
  pub fn has_bloom(&self) -> bool {
    self
      .effects
      .iter()
      .any(|a| matches!(a.effect, PostProcessEffect::Bloom { .. }))
  }

  // ========== Internal Methods ==========

  /// Set a persistent effect (replaces any existing effect of the same category)
  fn set_effect(&mut self, effect: PostProcessEffect) {
    let category = EffectCategory::from_effect(&effect);

    // Remove existing effect of same category
    self.remove_category(category);

    // Add new effect if it has any impact
    if effect.is_active() {
      self.effects.push(ActiveEffect {
        effect,
        category,
        remaining: None,
        initial_duration: None,
      });
    }
  }

  /// Set a timed effect that will automatically expire
  fn set_timed_effect(&mut self, effect: PostProcessEffect, duration_secs: f32) {
    let category = EffectCategory::from_effect(&effect);

    // Remove existing effect of same category
    self.remove_category(category);

    self.effects.push(ActiveEffect {
      effect,
      category,
      remaining: Some(duration_secs),
      initial_duration: Some(duration_secs),
    });
  }

  /// Remove all effects of a given category
  fn remove_category(&mut self, category: EffectCategory) {
    self.effects.retain(|a| a.category != category);
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn new_manager_has_no_effects() {
    let manager = EffectsManager::new();
    assert!(!manager.has_active_effects());
  }

  #[test]
  fn set_fade_adds_effect() {
    let mut manager = EffectsManager::new();
    manager.set_fade(0.5);
    assert!(manager.has_active_effects());
  }

  #[test]
  fn set_fade_zero_removes_effect() {
    let mut manager = EffectsManager::new();
    manager.set_fade(0.5);
    manager.set_fade(0.0);
    assert!(!manager.has_active_effects());
  }

  #[test]
  fn flash_expires_after_duration() {
    let mut manager = EffectsManager::new();
    manager.flash_secs(Color::WHITE, 0.5);
    assert!(manager.has_active_effects());

    manager.update(0.6);
    assert!(!manager.has_active_effects());
  }

  #[test]
  fn flash_fades_over_time() {
    let mut manager = EffectsManager::new();
    manager.flash_secs(Color::WHITE, 1.0);

    // At start, intensity should be 1.0
    let effect = manager.active_effects().next().unwrap();
    if let PostProcessEffect::ScreenFlash { intensity, .. } = effect {
      assert!((*intensity - 1.0).abs() < 0.01);
    }

    // After half the duration, intensity should be ~0.5
    manager.update(0.5);
    let effect = manager.active_effects().next().unwrap();
    if let PostProcessEffect::ScreenFlash { intensity, .. } = effect {
      assert!((*intensity - 0.5).abs() < 0.01);
    }
  }

  #[test]
  fn clear_removes_all_effects() {
    let mut manager = EffectsManager::new();
    manager.set_fade(0.5);
    manager.set_vignette(0.3);
    manager.enable_crt();

    manager.clear();
    assert!(!manager.has_active_effects());
  }

  #[test]
  fn set_bloom_adds_effect() {
    let mut manager = EffectsManager::new();
    manager.set_bloom(1.0);
    assert!(manager.has_bloom());
    assert!(manager.has_active_effects());
  }

  #[test]
  fn disable_bloom_removes_effect() {
    let mut manager = EffectsManager::new();
    manager.set_bloom(1.0);
    manager.disable_bloom();
    assert!(!manager.has_bloom());
  }

  #[test]
  fn bloom_replaces_existing_bloom() {
    let mut manager = EffectsManager::new();
    manager.set_bloom(1.0);
    manager.set_bloom_custom(0.5, 2.0, 10.0, 3);
    let count = manager
      .active_effects()
      .filter(|e| matches!(e, PostProcessEffect::Bloom { .. }))
      .count();
    assert_eq!(count, 1);
  }

  #[test]
  fn same_category_replaces_effect() {
    let mut manager = EffectsManager::new();
    manager.set_fade(0.3);
    manager.set_fade(0.7);

    // Should only have one fade effect
    let count = manager.active_effects().count();
    assert_eq!(count, 1);

    // And it should be the newer one
    if let Some(PostProcessEffect::Fade { progress, .. }) = manager.active_effects().next() {
      assert!((*progress - 0.7).abs() < 0.01);
    }
  }
}
