//! Material system for per-sprite shaders
//!
//! Materials allow custom GPU shaders to be applied to individual sprites
//! during rendering. This is distinct from post-processing effects which
//! operate on the entire frame.
//!
//! # Architecture
//!
//! There are three ways to apply shaders to sprites:
//!
//! 1. **Inline on Renderable** - Simple one-off effects
//! 2. **Material component** - Reusable, hot-swappable, queryable
//! 3. **Custom shaders** - User-provided WGSL for unique effects
//!
//! # Example
//!
//! ```rust,ignore
//! use ryo2d::prelude::*;
//!
//! // Pattern 1: Inline effect on Renderable
//! let player = Renderable::sprite(sprite)
//!     .with_shader(SpriteShader::outline(Color::WHITE, 2));
//!
//! // Pattern 2: Material as separate component
//! world.spawn((
//!     Transform::new(100.0, 200.0),
//!     Renderable::sprite(sprite),
//!     Material::outline(Color::WHITE, 2),
//! ));
//!
//! // Pattern 3: Custom user shader
//! let dissolve = assets.load_shader("shaders/dissolve.wgsl")?;
//! world.spawn((
//!     Transform::new(100.0, 200.0),
//!     Renderable::sprite(sprite),
//!     Material::custom(dissolve)
//!         .with_param("progress", 0.5)
//!         .with_param("edge_glow", 1.0),
//! ));
//! ```
//!
//! # GPU Rendering Requirement
//!
//! Materials require GPU-based sprite rendering. When a sprite has a Material
//! or inline shader, it will be rendered via the GPU pipeline rather than
//! CPU software rendering.

use super::sprite_effects::SpriteEffect;
use crate::core::ui::Color;
use std::collections::HashMap;
use std::sync::Arc;

/// Handle to a loaded shader resource
///
/// Shaders are loaded via the asset system and cached. The handle allows
/// multiple sprites to share the same shader without duplication.
#[derive(Debug, Clone)]
pub struct ShaderHandle {
  /// Unique identifier for cache lookup
  pub(crate) id: String,
  /// The compiled WGSL source (will be GPU pipeline in future)
  pub(crate) source: Arc<String>,
}

impl ShaderHandle {
  /// Create a shader handle from WGSL source
  ///
  /// Typically you'd use `assets.load_shader()` instead of this directly.
  pub fn from_source(id: impl Into<String>, source: impl Into<String>) -> Self {
    Self {
      id: id.into(),
      source: Arc::new(source.into()),
    }
  }

  /// Get the shader's unique identifier
  pub fn id(&self) -> &str {
    &self.id
  }

  /// Get the WGSL source code
  pub fn source(&self) -> &str {
    &self.source
  }
}

impl PartialEq for ShaderHandle {
  fn eq(&self, other: &Self) -> bool {
    self.id == other.id
  }
}

/// Named parameters for custom shaders
///
/// Parameters are passed to the shader as uniforms. The shader can
/// read these values to customize its behavior per-sprite.
///
/// # Supported Types
///
/// Currently supports `f32` values. Future versions may add:
/// - `Vec2`, `Vec3`, `Vec4` for colors/positions
/// - `i32` for integer parameters
/// - Texture samplers for multi-texture effects
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ShaderParams {
  params: HashMap<String, f32>,
}

impl ShaderParams {
  /// Create empty parameters
  pub fn new() -> Self {
    Self::default()
  }

  /// Set a parameter value
  pub fn set(&mut self, name: impl Into<String>, value: f32) -> &mut Self {
    self.params.insert(name.into(), value);
    self
  }

  /// Get a parameter value
  pub fn get(&self, name: &str) -> Option<f32> {
    self.params.get(name).copied()
  }

  /// Get a parameter with a default fallback
  pub fn get_or(&self, name: &str, default: f32) -> f32 {
    self.params.get(name).copied().unwrap_or(default)
  }

  /// Check if a parameter exists
  pub fn has(&self, name: &str) -> bool {
    self.params.contains_key(name)
  }

  /// Iterate over all parameters
  pub fn iter(&self) -> impl Iterator<Item = (&String, &f32)> {
    self.params.iter()
  }

  /// Number of parameters
  pub fn len(&self) -> usize {
    self.params.len()
  }

  /// Check if empty
  pub fn is_empty(&self) -> bool {
    self.params.is_empty()
  }

  /// Convert to a uniform buffer format (16 float array for GPU)
  ///
  /// Parameters are laid out in alphabetical order by name.
  /// Unused slots are filled with 0.0.
  pub fn to_uniform_array(&self) -> [f32; 16] {
    let mut result = [0.0f32; 16];
    let mut keys: Vec<_> = self.params.keys().collect();
    keys.sort();

    for (i, key) in keys.iter().take(16).enumerate() {
      if let Some(&value) = self.params.get(*key) {
        result[i] = value;
      }
    }
    result
  }
}

/// A shader that can be applied to sprites
///
/// Represents either a built-in effect or a custom user shader.
/// This is the core type used by both inline shaders and Material components.
#[derive(Debug, Clone, PartialEq)]
pub enum SpriteShader {
  /// A built-in engine effect (no custom WGSL needed)
  BuiltIn(SpriteEffect),

  /// A custom user-provided shader
  Custom {
    /// Handle to the loaded shader
    shader: ShaderHandle,
    /// Named parameters passed to the shader
    params: ShaderParams,
  },
}

impl SpriteShader {
  // =========================================================================
  // Built-in effect constructors (convenience wrappers)
  // =========================================================================

  /// Create an outline effect
  pub fn outline(color: Color, thickness: u32) -> Self {
    Self::BuiltIn(SpriteEffect::outline(color, thickness))
  }

  /// Create a white outline
  pub fn white_outline(thickness: u32) -> Self {
    Self::BuiltIn(SpriteEffect::white_outline(thickness))
  }

  /// Create a glow effect
  pub fn glow(color: Color, radius: f32) -> Self {
    Self::BuiltIn(SpriteEffect::glow(color, radius))
  }

  /// Create a flash effect (damage flash, power-up)
  pub fn flash(color: Color, intensity: f32) -> Self {
    Self::BuiltIn(SpriteEffect::flash(color, intensity))
  }

  /// Create a red damage flash
  pub fn damage_flash(intensity: f32) -> Self {
    Self::BuiltIn(SpriteEffect::damage_flash(intensity))
  }

  /// Create a color shift effect
  pub fn color_shift(color: Color, amount: f32) -> Self {
    Self::BuiltIn(SpriteEffect::color_shift(color, amount))
  }

  /// Create a silhouette effect
  pub fn silhouette(color: Color) -> Self {
    Self::BuiltIn(SpriteEffect::silhouette(color))
  }

  /// Create a shadow silhouette
  pub fn shadow() -> Self {
    Self::BuiltIn(SpriteEffect::shadow())
  }

  /// Create a pixelation effect
  pub fn pixelate(block_size: u32) -> Self {
    Self::BuiltIn(SpriteEffect::pixelate(block_size))
  }

  /// Create a wave distortion effect
  pub fn wave(amplitude: f32, frequency: f32, speed: f32) -> Self {
    Self::BuiltIn(SpriteEffect::wave(amplitude, frequency, speed))
  }

  /// Create a dissolve effect
  pub fn dissolve(progress: f32) -> Self {
    Self::BuiltIn(SpriteEffect::dissolve(progress))
  }

  /// Create a dissolve with burning edge
  pub fn dissolve_with_edge(progress: f32, edge_color: Color) -> Self {
    Self::BuiltIn(SpriteEffect::dissolve_with_edge(progress, edge_color))
  }

  /// Create an invert effect
  pub fn invert() -> Self {
    Self::BuiltIn(SpriteEffect::invert())
  }

  /// Create a grayscale effect
  pub fn grayscale(intensity: f32) -> Self {
    Self::BuiltIn(SpriteEffect::grayscale(intensity))
  }

  /// Create a directional outline (different colors for solid vs passable edges)
  pub fn directional_outline(solid_color: Color, passable_color: Color, thickness: u32) -> Self {
    Self::BuiltIn(SpriteEffect::directional_outline(solid_color, passable_color, thickness))
  }

  /// Create a platform outline (white solid edges, pale passable edges)
  pub fn platform_outline(thickness: u32) -> Self {
    Self::BuiltIn(SpriteEffect::platform_outline(thickness))
  }

  // =========================================================================
  // Custom shader constructors
  // =========================================================================

  /// Create a custom shader with no parameters
  pub fn custom(shader: ShaderHandle) -> Self {
    Self::Custom {
      shader,
      params: ShaderParams::new(),
    }
  }

  /// Create a custom shader with parameters
  pub fn custom_with_params(shader: ShaderHandle, params: ShaderParams) -> Self {
    Self::Custom { shader, params }
  }

  // =========================================================================
  // Queries
  // =========================================================================

  /// Check if this is a built-in effect
  pub fn is_builtin(&self) -> bool {
    matches!(self, Self::BuiltIn(_))
  }

  /// Check if this is a custom shader
  pub fn is_custom(&self) -> bool {
    matches!(self, Self::Custom { .. })
  }

  /// Get the built-in effect if this is one
  pub fn as_builtin(&self) -> Option<&SpriteEffect> {
    match self {
      Self::BuiltIn(effect) => Some(effect),
      _ => None,
    }
  }

  /// Get mutable access to the built-in effect
  pub fn as_builtin_mut(&mut self) -> Option<&mut SpriteEffect> {
    match self {
      Self::BuiltIn(effect) => Some(effect),
      _ => None,
    }
  }

  /// Get the shader handle if this is a custom shader
  pub fn shader_handle(&self) -> Option<&ShaderHandle> {
    match self {
      Self::Custom { shader, .. } => Some(shader),
      _ => None,
    }
  }

  /// Get the parameters if this is a custom shader
  pub fn params(&self) -> Option<&ShaderParams> {
    match self {
      Self::Custom { params, .. } => Some(params),
      _ => None,
    }
  }

  /// Get mutable access to parameters
  pub fn params_mut(&mut self) -> Option<&mut ShaderParams> {
    match self {
      Self::Custom { params, .. } => Some(params),
      _ => None,
    }
  }

  /// Update time-based effects. Returns true if the shader state changed.
  pub fn update(&mut self, dt: f32) -> bool {
    match self {
      Self::BuiltIn(effect) => effect.update(dt),
      Self::Custom { .. } => false, // Custom shaders handle time via params
    }
  }
}

/// Material component for attaching shaders to entities
///
/// When an entity has both a `Renderable` and a `Material`, the material's
/// shader is applied during rendering. This allows:
///
/// - Sharing materials across multiple entities
/// - Hot-swapping materials at runtime
/// - Querying entities by material type
///
/// # Priority
///
/// If a `Renderable` has an inline shader AND the entity has a `Material`,
/// the `Material` takes precedence (it's easier to swap/query).
#[derive(Debug, Clone, PartialEq)]
pub struct Material {
  /// The shader to apply
  pub shader: SpriteShader,
}

impl Material {
  /// Create a material with any sprite shader
  pub fn new(shader: SpriteShader) -> Self {
    Self { shader }
  }

  // =========================================================================
  // Built-in effect constructors
  // =========================================================================

  /// Create an outline material
  pub fn outline(color: Color, thickness: u32) -> Self {
    Self::new(SpriteShader::outline(color, thickness))
  }

  /// Create a white outline material
  pub fn white_outline(thickness: u32) -> Self {
    Self::new(SpriteShader::white_outline(thickness))
  }

  /// Create a glow material
  pub fn glow(color: Color, radius: f32) -> Self {
    Self::new(SpriteShader::glow(color, radius))
  }

  /// Create a flash material
  pub fn flash(color: Color, intensity: f32) -> Self {
    Self::new(SpriteShader::flash(color, intensity))
  }

  /// Create a damage flash material
  pub fn damage_flash(intensity: f32) -> Self {
    Self::new(SpriteShader::damage_flash(intensity))
  }

  /// Create a color shift material
  pub fn color_shift(color: Color, amount: f32) -> Self {
    Self::new(SpriteShader::color_shift(color, amount))
  }

  /// Create a silhouette material
  pub fn silhouette(color: Color) -> Self {
    Self::new(SpriteShader::silhouette(color))
  }

  /// Create a shadow material
  pub fn shadow() -> Self {
    Self::new(SpriteShader::shadow())
  }

  /// Create a pixelate material
  pub fn pixelate(block_size: u32) -> Self {
    Self::new(SpriteShader::pixelate(block_size))
  }

  /// Create a wave material
  pub fn wave(amplitude: f32, frequency: f32, speed: f32) -> Self {
    Self::new(SpriteShader::wave(amplitude, frequency, speed))
  }

  /// Create a dissolve material
  pub fn dissolve(progress: f32) -> Self {
    Self::new(SpriteShader::dissolve(progress))
  }

  /// Create a dissolve with edge material
  pub fn dissolve_with_edge(progress: f32, edge_color: Color) -> Self {
    Self::new(SpriteShader::dissolve_with_edge(progress, edge_color))
  }

  /// Create an invert material
  pub fn invert() -> Self {
    Self::new(SpriteShader::invert())
  }

  /// Create a grayscale material
  pub fn grayscale(intensity: f32) -> Self {
    Self::new(SpriteShader::grayscale(intensity))
  }

  /// Create a directional outline material
  pub fn directional_outline(solid_color: Color, passable_color: Color, thickness: u32) -> Self {
    Self::new(SpriteShader::directional_outline(solid_color, passable_color, thickness))
  }

  /// Create a platform outline material (white solid, pale passable)
  pub fn platform_outline(thickness: u32) -> Self {
    Self::new(SpriteShader::platform_outline(thickness))
  }

  // =========================================================================
  // Custom shader constructors
  // =========================================================================

  /// Create a custom shader material
  pub fn custom(shader: ShaderHandle) -> Self {
    Self::new(SpriteShader::custom(shader))
  }

  /// Create a custom shader material with initial parameters
  pub fn custom_with_params(shader: ShaderHandle, params: ShaderParams) -> Self {
    Self::new(SpriteShader::custom_with_params(shader, params))
  }

  // =========================================================================
  // Parameter builder pattern for custom shaders
  // =========================================================================

  /// Set a shader parameter (only works for custom shaders)
  ///
  /// For built-in effects, this is a no-op.
  pub fn with_param(mut self, name: impl Into<String>, value: f32) -> Self {
    if let Some(params) = self.shader.params_mut() {
      params.set(name, value);
    }
    self
  }

  // =========================================================================
  // Queries
  // =========================================================================

  /// Check if this is a built-in effect material
  pub fn is_builtin(&self) -> bool {
    self.shader.is_builtin()
  }

  /// Check if this is a custom shader material
  pub fn is_custom(&self) -> bool {
    self.shader.is_custom()
  }

  /// Update time-based effects
  pub fn update(&mut self, dt: f32) -> bool {
    self.shader.update(dt)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn shader_handle_equality() {
    let h1 = ShaderHandle::from_source("test", "void main() {}");
    let h2 = ShaderHandle::from_source("test", "different source");
    let h3 = ShaderHandle::from_source("other", "void main() {}");

    assert_eq!(h1, h2); // Same ID = equal
    assert_ne!(h1, h3); // Different ID = not equal
  }

  #[test]
  fn shader_params_basic() {
    let mut params = ShaderParams::new();
    params.set("progress", 0.5);
    params.set("intensity", 1.0);

    assert_eq!(params.get("progress"), Some(0.5));
    assert_eq!(params.get("intensity"), Some(1.0));
    assert_eq!(params.get("missing"), None);
    assert_eq!(params.get_or("missing", 0.25), 0.25);
  }

  #[test]
  fn shader_params_to_uniform() {
    let mut params = ShaderParams::new();
    params.set("b_second", 2.0);
    params.set("a_first", 1.0);
    params.set("c_third", 3.0);

    let uniform = params.to_uniform_array();
    assert_eq!(uniform[0], 1.0); // a_first (alphabetical)
    assert_eq!(uniform[1], 2.0); // b_second
    assert_eq!(uniform[2], 3.0); // c_third
    assert_eq!(uniform[3], 0.0); // unused
  }

  #[test]
  fn sprite_shader_builtin() {
    let shader = SpriteShader::outline(Color::WHITE, 2);

    assert!(shader.is_builtin());
    assert!(!shader.is_custom());
    assert!(shader.as_builtin().is_some());
    assert!(shader.shader_handle().is_none());
  }

  #[test]
  fn sprite_shader_custom() {
    let handle = ShaderHandle::from_source("dissolve", "// shader code");
    let shader = SpriteShader::custom(handle);

    assert!(!shader.is_builtin());
    assert!(shader.is_custom());
    assert!(shader.shader_handle().is_some());
    assert!(shader.params().is_some());
  }

  #[test]
  fn material_with_param() {
    let handle = ShaderHandle::from_source("test", "// code");
    let material = Material::custom(handle)
      .with_param("progress", 0.5)
      .with_param("edge", 1.0);

    let params = material.shader.params().unwrap();
    assert_eq!(params.get("progress"), Some(0.5));
    assert_eq!(params.get("edge"), Some(1.0));
  }

  #[test]
  fn material_builtin_ignores_params() {
    let material = Material::outline(Color::WHITE, 2).with_param("ignored", 1.0);

    // with_param is a no-op for built-in effects
    assert!(material.is_builtin());
    assert!(material.shader.params().is_none());
  }
}
