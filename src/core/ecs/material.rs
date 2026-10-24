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
//! let player = Renderable::image(image.clone())
//!     .with_shader(SpriteShader::outline(Color::WHITE, 2));
//!
//! // Pattern 2: Material as separate component
//! world.spawn((
//!     Transform::from_xy(100.0, 200.0),
//!     Renderable::image(image.clone()),
//!     Material::outline(Color::WHITE, 2),
//! ));
//!
//! // Pattern 3: Custom user shader (WGSL `fn sprite(in: SpriteIn, params: Params)`)
//! let dissolve = ctx.assets().load_now::<Shader>("shaders/dissolve.wgsl")?;
//! world.spawn((
//!     Transform::from_xy(100.0, 200.0),
//!     Renderable::image(image),
//!     Material::custom(dissolve)
//!         .with_param("amount", 0.5)
//!         .with_param("edge", Color::rgb(255, 120, 0)),
//! ));
//! ```
//!
//! A `Material` wins over `Renderable::shader`. Custom params are set by name
//! (`f32`, `Vec2/3/4`, or `Color` as sRGB/255, which fills a `vec3` with rgb or
//! a `vec4` with rgba). Custom shaders draw within the sprite's quad; shadows
//! and tiled tiles draw plain. A shader that fails to compile is logged and the
//! sprite draws plain.
//!
//! # GPU Rendering Requirement
//!
//! Materials require GPU-based sprite rendering. When a sprite has a Material
//! or inline shader, it will be rendered via the GPU pipeline rather than
//! CPU software rendering.

use super::kernel::Component;
use super::sprite_effects::SpriteEffect;
use crate::core::asset::{Asset, Assets, Handle};
use crate::core::ui::Color;
use std::sync::{Arc, OnceLock};

/// WGSL source for a sprite shader or a post-process effect: the game's own
/// function (`fn sprite(in: SpriteIn, params: Params) -> vec4<f32>` or
/// `fn effect(in: EffectIn, params: Params) -> vec4<f32>`) and an optional
/// `struct Params`.
///
/// Load one from a `.wgsl` file with `ctx.assets().load_now::<Shader>(path)`
/// (its name is then its asset path), or embed one with
/// `ctx.assets().add(Shader::from_wgsl(name, source))`. Mistakes in it are
/// reported under its name, at the line and column of its own source.
///
/// A file is compiled as it loads, so one that does not compile fails its
/// load (`AssetError::Decode`), and a saved file that does not compile
/// fails its reload (`AssetEvent::ReloadFailed`) and keeps the last good
/// shader. A shader whose source changes, by a reload or through
/// `Assets::get_mut`, is compiled again for the next frame; shaders with
/// equal sources compile once. A version that does not compile there (one
/// made in code), or whose pipeline the GPU rejects, draws the shader's
/// last version the GPU accepted, warned about once; with none, a sprite
/// draws plain and an effect is skipped
#[derive(Debug, Clone)]
pub struct Shader {
  name: Arc<str>,
  source: Arc<str>,
}

impl Asset for Shader {}

impl Shader {
  /// A shader of WGSL `source`, reported as `name` in its errors
  pub fn from_wgsl(name: impl Into<Arc<str>>, source: impl Into<Arc<str>>) -> Self {
    Self {
      name: name.into(),
      source: source.into(),
    }
  }

  /// The name its errors are reported under: the asset path for a loaded shader
  pub fn name(&self) -> &str {
    &self.name
  }

  /// The game's WGSL
  pub fn source(&self) -> &str {
    &self.source
  }
}

/// The world's shaders: its `Assets<Shader>`, or an empty store for a world
/// without one
pub(crate) fn shaders_of(world: &super::World) -> &Assets<Shader> {
  world
    .get_resource::<Assets<Shader>>()
    .unwrap_or_else(|| no_shaders())
}

/// A store with no shaders, for drawing without a world
pub(crate) fn no_shaders() -> &'static Assets<Shader> {
  static NONE: OnceLock<Assets<Shader>> = OnceLock::new();
  NONE.get_or_init(Assets::new)
}

/// A value for a custom shader parameter
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ParamValue {
  F32(f32),
  Vec2([f32; 2]),
  Vec3([f32; 3]),
  Vec4([f32; 4]),
  /// sRGB/255 with alpha; fills a `vec3<f32>` (rgb) or a `vec4<f32>` (rgba)
  Color([f32; 4]),
}

impl From<f32> for ParamValue {
  fn from(v: f32) -> Self {
    Self::F32(v)
  }
}

impl From<glam::Vec2> for ParamValue {
  fn from(v: glam::Vec2) -> Self {
    Self::Vec2(v.to_array())
  }
}

impl From<glam::Vec3> for ParamValue {
  fn from(v: glam::Vec3) -> Self {
    Self::Vec3(v.to_array())
  }
}

impl From<glam::Vec4> for ParamValue {
  fn from(v: glam::Vec4) -> Self {
    Self::Vec4(v.to_array())
  }
}

impl From<Color> for ParamValue {
  /// sRGB bytes divided by 255, alpha included
  fn from(c: Color) -> Self {
    Self::Color([
      c.r as f32 / 255.0,
      c.g as f32 / 255.0,
      c.b as f32 / 255.0,
      c.a as f32 / 255.0,
    ])
  }
}

/// Named parameters for custom shaders
///
/// Values are matched by name to the fields of the shader's `Params` struct,
/// so a typo is reported rather than silently reordering anything. Fields the
/// game does not set are zero.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ShaderParams {
  values: Vec<(String, ParamValue)>,
}

impl ShaderParams {
  /// Create empty parameters
  pub fn new() -> Self {
    Self::default()
  }

  /// Set a parameter value (replaces an earlier value with the same name)
  pub fn set(&mut self, name: impl Into<String>, value: impl Into<ParamValue>) -> &mut Self {
    let name = name.into();
    let value = value.into();
    match self.values.iter_mut().find(|(n, _)| *n == name) {
      Some(slot) => slot.1 = value,
      None => self.values.push((name, value)),
    }
    self
  }

  /// Builder form of [`set`](Self::set)
  pub fn with(mut self, name: impl Into<String>, value: impl Into<ParamValue>) -> Self {
    self.set(name, value);
    self
  }

  /// Get a parameter value
  pub fn get(&self, name: &str) -> Option<ParamValue> {
    self.values.iter().find(|(n, _)| n == name).map(|(_, v)| *v)
  }

  /// Iterate the values in insertion order
  pub(crate) fn iter(&self) -> impl Iterator<Item = (&str, ParamValue)> {
    self.values.iter().map(|(n, v)| (n.as_str(), *v))
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
    /// The shader, kept loaded while the sprite holds it
    shader: Handle<Shader>,
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
    Self::BuiltIn(SpriteEffect::directional_outline(
      solid_color,
      passable_color,
      thickness,
    ))
  }

  /// Create a platform outline (white solid edges, pale passable edges)
  pub fn platform_outline(thickness: u32) -> Self {
    Self::BuiltIn(SpriteEffect::platform_outline(thickness))
  }

  // =========================================================================
  // Custom shader constructors
  // =========================================================================

  /// Create a custom shader with no parameters
  pub fn custom(shader: Handle<Shader>) -> Self {
    Self::Custom {
      shader,
      params: ShaderParams::new(),
    }
  }

  /// Create a custom shader with parameters
  pub fn custom_with_params(shader: Handle<Shader>, params: ShaderParams) -> Self {
    Self::Custom { shader, params }
  }

  /// Set a shader parameter (no-op for built-in effects)
  pub fn with_param(mut self, name: impl Into<String>, value: impl Into<ParamValue>) -> Self {
    if let Some(params) = self.params_mut() {
      params.set(name, value);
    }
    self
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

  /// Get the shader handle if this is a custom shader
  pub fn shader_handle(&self) -> Option<&Handle<Shader>> {
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
#[derive(Component, Debug, Clone, PartialEq)]
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
    Self::new(SpriteShader::directional_outline(
      solid_color,
      passable_color,
      thickness,
    ))
  }

  /// Create a platform outline material (white solid, pale passable)
  pub fn platform_outline(thickness: u32) -> Self {
    Self::new(SpriteShader::platform_outline(thickness))
  }

  // =========================================================================
  // Custom shader constructors
  // =========================================================================

  /// Create a custom shader material
  pub fn custom(shader: Handle<Shader>) -> Self {
    Self::new(SpriteShader::custom(shader))
  }

  /// Create a custom shader material with initial parameters
  pub fn custom_with_params(shader: Handle<Shader>, params: ShaderParams) -> Self {
    Self::new(SpriteShader::custom_with_params(shader, params))
  }

  // =========================================================================
  // Parameter builder pattern for custom shaders
  // =========================================================================

  /// Set a shader parameter (only works for custom shaders)
  ///
  /// For built-in effects, this is a no-op.
  pub fn with_param(mut self, name: impl Into<String>, value: impl Into<ParamValue>) -> Self {
    self.shader = self.shader.with_param(name, value);
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
  fn shader_params_basic() {
    let mut params = ShaderParams::new();
    params.set("progress", 0.5);
    params.set("intensity", 1.0);
    params.set("progress", 0.75);

    assert_eq!(params.get("progress"), Some(ParamValue::F32(0.75)));
    assert_eq!(params.get("intensity"), Some(ParamValue::F32(1.0)));
    assert_eq!(params.get("missing"), None);
    let names: Vec<_> = params.iter().map(|(n, _)| n).collect();
    assert_eq!(names, ["progress", "intensity"]);
  }

  #[test]
  fn param_value_conversions() {
    assert_eq!(
      ParamValue::from(glam::Vec2::new(1.0, 2.0)),
      ParamValue::Vec2([1.0, 2.0])
    );
    assert_eq!(
      ParamValue::from(Color::new(255, 0, 51, 255)),
      ParamValue::Color([1.0, 0.0, 0.2, 1.0])
    );
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
    let mut shaders = Assets::<Shader>::new();
    let handle = shaders.add(Shader::from_wgsl("dissolve", "// shader code"));
    let shader = SpriteShader::custom(handle);

    assert!(!shader.is_builtin());
    assert!(shader.is_custom());
    assert!(shader.shader_handle().is_some());
    assert!(shader.params().is_some());
  }

  #[test]
  fn material_with_param() {
    let mut shaders = Assets::<Shader>::new();
    let handle = shaders.add(Shader::from_wgsl("test", "// code"));
    let material = Material::custom(handle)
      .with_param("progress", 0.5)
      .with_param("edge", 1.0);

    let params = material.shader.params().expect("custom shader has params");
    assert_eq!(params.get("progress"), Some(ParamValue::F32(0.5)));
    assert_eq!(params.get("edge"), Some(ParamValue::F32(1.0)));
  }

  #[test]
  fn material_builtin_ignores_params() {
    let material = Material::outline(Color::WHITE, 2).with_param("ignored", 1.0);

    // with_param is a no-op for built-in effects
    assert!(material.is_builtin());
    assert!(material.shader.params().is_none());
  }
}
