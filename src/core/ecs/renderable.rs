//! Unified rendering component for sprites and tiles
//!
//! The `Renderable` component provides a flexible rendering primitive that
//! supports sprites, tiled sprites, rotation, scale, and parent-child hierarchies.
//!
//! # Example
//!
//! ```rust,ignore
//! use ryo2d::prelude::*;
//!
//! // Simple sprite
//! let sprite = Renderable::sprite(player_handle);
//!
//! // Tiled sprite with rotation
//! let floor = Renderable::tiled(tile_handle, TileDirection::Right, 200.0);
//!
//! // With z-order and visibility
//! let background = Renderable::sprite(bg_handle)
//!     .with_z_order(-10)
//!     .with_visible(true);
//!
//! // With inline shader effect
//! let highlighted = Renderable::sprite(item_handle)
//!     .with_shader(SpriteShader::outline(Color::WHITE, 2));
//!
//! // Spawn with transform (uses WorldTransform if parent-child)
//! world.spawn((Transform::new(100.0, 200.0), sprite));
//! ```
//!
//! # Render Modes
//!
//! The `RenderMode` enum hints to the renderer how to handle this entity:
//!
//! - `Auto` (default): Engine decides based on entity count and complexity
//! - `Cpu`: Force CPU-based software rendering
//! - `Gpu`: Request GPU-batched instanced rendering (for bullet hell / particles)
//!
//! For most games, `Auto` is the right choice. Use `Gpu` when you have thousands
//! of similar sprites (bullets, particles) that benefit from instanced rendering.
//!
//! # Shaders
//!
//! Shaders can be attached in two ways:
//!
//! 1. **Inline on Renderable** - Use `.with_shader()` for simple one-off effects
//! 2. **Material component** - Add a separate `Material` component for reusable,
//!    queryable, hot-swappable effects
//!
//! If both are present, the `Material` component takes precedence.

use super::material::SpriteShader;
use crate::core::asset::SpriteHandle;
use crate::core::ecs::Vec2;
use crate::core::graphics::TileDirection;
use crate::core::ui::Color;

/// Configuration for a drop shadow effect on a sprite.
///
/// Drop shadows are simple offset shadows beneath sprites, useful for
/// platformers, UI elements, or any sprite that needs visual grounding.
///
/// # Example
///
/// ```rust,ignore
/// use ryo2d::prelude::*;
///
/// // Simple drop shadow with default settings
/// let sprite = Renderable::sprite(handle).with_drop_shadow(DropShadow::default());
///
/// // Customized drop shadow
/// let sprite = Renderable::sprite(handle).with_drop_shadow(
///     DropShadow::new()
///         .offset(4.0, 6.0)
///         .color(Color::new(0, 0, 0, 128))
///         .scale(1.0, 0.5)
/// );
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct DropShadow {
  /// Offset from the sprite position (in pixels)
  pub offset: Vec2,
  /// Shadow color (default: semi-transparent black)
  pub color: Color,
  /// Blur radius in pixels (0 = sharp edge)
  /// Note: Blur is approximated by drawing multiple offset copies
  pub blur: f32,
  /// Scale relative to the sprite (1.0, 1.0 = same size)
  pub scale: Vec2,
}

impl Default for DropShadow {
  fn default() -> Self {
    Self {
      offset: Vec2::new(4.0, 4.0),
      color: Color::new(0, 0, 0, 128), // Semi-transparent black
      blur: 0.0,
      scale: Vec2::new(1.0, 1.0),
    }
  }
}

impl DropShadow {
  /// Create a new drop shadow with default settings
  pub fn new() -> Self {
    Self::default()
  }

  /// Set the shadow offset
  pub fn offset(mut self, x: f32, y: f32) -> Self {
    self.offset = Vec2::new(x, y);
    self
  }

  /// Set the shadow color
  pub fn color(mut self, color: Color) -> Self {
    self.color = color;
    self
  }

  /// Set the blur radius (0 = sharp)
  pub fn blur(mut self, blur: f32) -> Self {
    self.blur = blur.max(0.0);
    self
  }

  /// Set the shadow scale relative to sprite
  pub fn scale(mut self, x: f32, y: f32) -> Self {
    self.scale = Vec2::new(x, y);
    self
  }

  /// Create a simple shadow directly below the sprite
  pub fn below(offset_y: f32) -> Self {
    Self {
      offset: Vec2::new(0.0, offset_y),
      ..Default::default()
    }
  }

  /// Create a shadow offset diagonally (classic drop shadow look)
  pub fn diagonal(offset: f32) -> Self {
    Self {
      offset: Vec2::new(offset, offset),
      ..Default::default()
    }
  }

  /// Create a squashed shadow (good for top-down or isometric)
  pub fn squashed(offset_y: f32, squash: f32) -> Self {
    Self {
      offset: Vec2::new(0.0, offset_y),
      scale: Vec2::new(1.0, squash),
      ..Default::default()
    }
  }
}

/// Configuration for a blob shadow effect beneath a sprite.
///
/// Blob shadows are simple circular or elliptical shadows placed at the
/// entity's feet. They're commonly used in platformers and top-down games
/// to provide visual grounding without matching the sprite's exact shape.
///
/// # Example
///
/// ```rust,ignore
/// use ryo2d::prelude::*;
///
/// // Simple circular blob shadow
/// let sprite = Renderable::sprite(handle).with_blob_shadow(BlobShadow::default());
///
/// // Elliptical shadow for a wider character
/// let sprite = Renderable::sprite(handle).with_blob_shadow(
///     BlobShadow::ellipse(32.0, 12.0)
/// );
///
/// // Shadow that scales with "height" (for jumping)
/// let sprite = Renderable::sprite(handle).with_blob_shadow(
///     BlobShadow::circle(20.0).with_height_scale(true)
/// );
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct BlobShadow {
  /// Horizontal radius of the shadow ellipse
  pub radius_x: f32,
  /// Vertical radius of the shadow ellipse (smaller = more squashed)
  pub radius_y: f32,
  /// Vertical offset from sprite position (positive = below)
  pub offset_y: f32,
  /// Shadow color (default: semi-transparent black)
  pub color: Color,
  /// Current "height" factor for dynamic shadows (0.0 = on ground, 1.0 = max height)
  /// When height > 0, shadow shrinks and becomes more transparent
  pub height: f32,
  /// Maximum height value for scaling calculations
  pub max_height: f32,
  /// Whether to scale shadow based on height (for jumping characters)
  pub height_scaling: bool,
}

impl Default for BlobShadow {
  fn default() -> Self {
    Self {
      radius_x: 16.0,
      radius_y: 8.0,
      offset_y: 0.0,
      color: Color::new(0, 0, 0, 100),
      height: 0.0,
      max_height: 100.0,
      height_scaling: false,
    }
  }
}

impl BlobShadow {
  /// Create a new blob shadow with default settings
  pub fn new() -> Self {
    Self::default()
  }

  /// Create a circular blob shadow
  pub fn circle(radius: f32) -> Self {
    Self {
      radius_x: radius,
      radius_y: radius * 0.5, // Slightly squashed by default
      ..Default::default()
    }
  }

  /// Create an elliptical blob shadow
  pub fn ellipse(radius_x: f32, radius_y: f32) -> Self {
    Self {
      radius_x,
      radius_y,
      ..Default::default()
    }
  }

  /// Set the shadow radii
  pub fn radii(mut self, x: f32, y: f32) -> Self {
    self.radius_x = x.max(1.0);
    self.radius_y = y.max(1.0);
    self
  }

  /// Set the vertical offset from sprite position
  pub fn offset(mut self, offset_y: f32) -> Self {
    self.offset_y = offset_y;
    self
  }

  /// Set the shadow color
  pub fn color(mut self, color: Color) -> Self {
    self.color = color;
    self
  }

  /// Enable height-based scaling (shadow shrinks when entity is "in the air")
  pub fn with_height_scale(mut self, enabled: bool) -> Self {
    self.height_scaling = enabled;
    self
  }

  /// Set the maximum height for scaling calculations
  pub fn max_height(mut self, max: f32) -> Self {
    self.max_height = max.max(1.0);
    self
  }

  /// Set the current height (0.0 = on ground)
  pub fn set_height(&mut self, height: f32) {
    self.height = height.max(0.0);
  }

  /// Get the effective radius accounting for height scaling
  pub fn effective_radius(&self) -> (f32, f32) {
    if !self.height_scaling || self.height <= 0.0 {
      return (self.radius_x, self.radius_y);
    }

    // Shadow shrinks as height increases
    let height_factor = (1.0 - (self.height / self.max_height).min(1.0)).max(0.2);
    (self.radius_x * height_factor, self.radius_y * height_factor)
  }

  /// Get the effective alpha accounting for height scaling
  pub fn effective_alpha(&self) -> f32 {
    let base_alpha = self.color.a as f32 / 255.0;

    if !self.height_scaling || self.height <= 0.0 {
      return base_alpha;
    }

    // Shadow fades as height increases
    let height_factor = (1.0 - (self.height / self.max_height).min(1.0)).max(0.1);
    base_alpha * height_factor
  }
}

/// Configuration for a contact shadow effect at the base of a sprite.
///
/// Contact shadows are small, dark shadows that appear at the contact point
/// between an object and the surface it's resting on. They're darkest right
/// at the contact point and fade radially outward, simulating ambient occlusion.
///
/// Unlike blob shadows (which are larger and more diffuse), contact shadows are
/// concentrated at the feet/base of objects and create visual grounding.
///
/// # Example
///
/// ```rust,ignore
/// use ryo2d::prelude::*;
///
/// // Simple contact shadow at sprite base
/// let sprite = Renderable::sprite(handle).with_contact_shadow(ContactShadow::default());
///
/// // Wider contact shadow for a large object
/// let sprite = Renderable::sprite(handle).with_contact_shadow(
///     ContactShadow::new().radius(12.0).intensity(0.8)
/// );
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ContactShadow {
  /// Radius of the contact shadow (how far it spreads)
  pub radius: f32,
  /// Shadow intensity/darkness (0.0 = invisible, 1.0 = fully opaque black)
  pub intensity: f32,
  /// Vertical offset from sprite position (positive = below)
  pub offset_y: f32,
  /// Shadow color (default: black)
  pub color: Color,
  /// Falloff curve (1.0 = linear, 2.0 = quadratic/sharper, 0.5 = softer)
  pub falloff: f32,
}

impl Default for ContactShadow {
  fn default() -> Self {
    Self {
      radius: 8.0,
      intensity: 0.6,
      offset_y: 0.0,
      color: Color::BLACK,
      falloff: 2.0, // Sharp falloff by default for concentrated contact point
    }
  }
}

impl ContactShadow {
  /// Create a new contact shadow with default settings
  pub fn new() -> Self {
    Self::default()
  }

  /// Set the shadow radius
  pub fn radius(mut self, radius: f32) -> Self {
    self.radius = radius.max(1.0);
    self
  }

  /// Set the shadow intensity (0.0 = invisible, 1.0 = fully opaque)
  pub fn intensity(mut self, intensity: f32) -> Self {
    self.intensity = intensity.clamp(0.0, 1.0);
    self
  }

  /// Set the vertical offset from sprite position
  pub fn offset(mut self, offset_y: f32) -> Self {
    self.offset_y = offset_y;
    self
  }

  /// Set the shadow color
  pub fn color(mut self, color: Color) -> Self {
    self.color = color;
    self
  }

  /// Set the falloff curve (1.0 = linear, 2.0 = sharp, 0.5 = soft)
  pub fn falloff(mut self, falloff: f32) -> Self {
    self.falloff = falloff.clamp(0.1, 4.0);
    self
  }

  /// Create a sharp contact shadow (concentrated at contact point)
  pub fn sharp(radius: f32) -> Self {
    Self {
      radius,
      intensity: 0.7,
      falloff: 3.0,
      ..Default::default()
    }
  }

  /// Create a soft contact shadow (more diffuse)
  pub fn soft(radius: f32) -> Self {
    Self {
      radius,
      intensity: 0.4,
      falloff: 1.0,
      ..Default::default()
    }
  }

  /// Get the alpha at a given distance from center (for rendering)
  pub fn alpha_at_distance(&self, distance: f32) -> f32 {
    if distance >= self.radius {
      return 0.0;
    }

    let normalized = distance / self.radius;
    let falloff_value = (1.0 - normalized).powf(self.falloff);
    self.intensity * falloff_value
  }
}

/// Hints for how the renderer should handle this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RenderMode {
  /// Let the engine decide (default). Uses CPU for low counts, GPU for high counts.
  #[default]
  Auto,
  /// Force CPU software rendering. Good for unique sprites with complex transforms.
  Cpu,
  /// Request GPU instanced rendering. Ideal for many similar sprites (bullets, particles).
  Gpu,
}

/// How the sprite should be rendered
#[derive(Debug, Clone, Default)]
pub enum RenderStyle {
  /// Single sprite rendered once
  #[default]
  Single,
  /// Sprite repeated to fill a target size in one direction
  Tiled {
    /// Direction of tiling
    direction: TileDirection,
    /// Total size to fill in pixels
    target_size: f32,
  },
}

/// Unified rendering component for sprites and tiles
///
/// Works with `Transform` for local positioning and `WorldTransform` for
/// hierarchical rendering (parent-child relationships).
///
/// The renderer will prefer `WorldTransform` if present, otherwise uses `Transform`.
#[derive(Debug, Clone)]
pub struct Renderable {
  /// The sprite to render
  pub sprite: SpriteHandle,
  /// How to render the sprite (single or tiled)
  pub style: RenderStyle,
  /// Rendering hint (CPU/GPU/Auto)
  pub mode: RenderMode,
  /// Whether this entity is visible
  pub visible: bool,
  /// Render order (lower = behind, higher = in front)
  pub z_order: i32,
  /// Flip horizontally
  pub flip_x: bool,
  /// Flip vertically
  pub flip_y: bool,
  /// Alpha transparency (0.0 = invisible, 1.0 = opaque)
  pub alpha: f32,
  /// Tint color multiplied with sprite colors (default: WHITE = no tint)
  ///
  /// Use this for color effects like damage flash, power-ups, or fading.
  /// The alpha component of tint is multiplied with the `alpha` field.
  pub tint: Color,
  /// Optional inline shader effect
  ///
  /// For simple one-off effects. For reusable/queryable shaders,
  /// use a separate `Material` component instead.
  pub shader: Option<SpriteShader>,
  /// Optional drop shadow effect
  ///
  /// When set, a shadow is rendered beneath the sprite at the specified
  /// offset, color, and scale. Useful for platformers, UI, or visual grounding.
  pub drop_shadow: Option<DropShadow>,
  /// Optional blob shadow effect
  ///
  /// When set, a simple elliptical shadow is rendered beneath the sprite.
  /// Useful for characters in platformers or top-down games.
  /// If both drop_shadow and blob_shadow are set, both are rendered.
  pub blob_shadow: Option<BlobShadow>,
  /// Optional contact shadow effect
  ///
  /// When set, a small concentrated shadow is rendered at the contact point
  /// (base of sprite), simulating ambient occlusion. Contact shadows are
  /// darker at the center and fade radially outward.
  pub contact_shadow: Option<ContactShadow>,
  /// Enable render interpolation for smooth motion.
  ///
  /// When true, the entity's position is interpolated between the previous
  /// and current Transform during rendering. This eliminates micro-stutter
  /// when the render FPS exceeds the fixed update rate (e.g., 144hz display
  /// with 60hz physics).
  ///
  /// Requires a `PreviousTransform` component to be present (added automatically
  /// by the engine when interpolation is enabled).
  ///
  /// Default: false (pixel-perfect rendering at update positions)
  pub interpolate: bool,
}

impl Renderable {
  /// Create a simple sprite renderable
  pub fn sprite(sprite: SpriteHandle) -> Self {
    Renderable {
      sprite,
      style: RenderStyle::Single,
      mode: RenderMode::Auto,
      visible: true,
      z_order: 0,
      flip_x: false,
      flip_y: false,
      alpha: 1.0,
      tint: Color::WHITE,
      shader: None,
      drop_shadow: None,
      blob_shadow: None,
      contact_shadow: None,
      interpolate: false,
    }
  }

  /// Create a tiled sprite renderable
  pub fn tiled(sprite: SpriteHandle, direction: TileDirection, target_size: f32) -> Self {
    Renderable {
      sprite,
      style: RenderStyle::Tiled {
        direction,
        target_size,
      },
      mode: RenderMode::Auto,
      visible: true,
      z_order: 0,
      flip_x: false,
      flip_y: false,
      alpha: 1.0,
      tint: Color::WHITE,
      shader: None,
      drop_shadow: None,
      blob_shadow: None,
      contact_shadow: None,
      interpolate: false,
    }
  }

  /// Set visibility
  pub fn with_visible(mut self, visible: bool) -> Self {
    self.visible = visible;
    self
  }

  /// Set z-order for render ordering
  pub fn with_z_order(mut self, z_order: i32) -> Self {
    self.z_order = z_order;
    self
  }

  /// Set render mode hint
  pub fn with_mode(mut self, mode: RenderMode) -> Self {
    self.mode = mode;
    self
  }

  /// Set horizontal flip
  pub fn with_flip_x(mut self, flip: bool) -> Self {
    self.flip_x = flip;
    self
  }

  /// Set vertical flip
  pub fn with_flip_y(mut self, flip: bool) -> Self {
    self.flip_y = flip;
    self
  }

  /// Set alpha transparency
  pub fn with_alpha(mut self, alpha: f32) -> Self {
    self.alpha = alpha.clamp(0.0, 1.0);
    self
  }

  /// Set tint color
  ///
  /// The tint color is multiplied with sprite colors. Use `Color::WHITE` for no tint.
  pub fn with_tint(mut self, tint: Color) -> Self {
    self.tint = tint;
    self
  }

  /// Set an inline shader effect
  ///
  /// For simple one-off effects. If you need the shader to be queryable,
  /// hot-swappable, or shared, use a `Material` component instead.
  ///
  /// If the entity has both an inline shader and a `Material` component,
  /// the `Material` takes precedence.
  pub fn with_shader(mut self, shader: SpriteShader) -> Self {
    self.shader = Some(shader);
    self
  }

  /// Check if this renderable has an inline shader
  pub fn has_shader(&self) -> bool {
    self.shader.is_some()
  }

  /// Set a drop shadow effect
  ///
  /// The shadow is rendered beneath the sprite at the specified offset,
  /// color, and scale. Useful for platformers, UI elements, or visual grounding.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// use ryo2d::prelude::*;
  ///
  /// // Simple drop shadow
  /// let sprite = Renderable::sprite(handle).with_drop_shadow(DropShadow::default());
  ///
  /// // Custom shadow
  /// let sprite = Renderable::sprite(handle).with_drop_shadow(
  ///     DropShadow::new().offset(4.0, 6.0).color(Color::new(0, 0, 0, 100))
  /// );
  /// ```
  pub fn with_drop_shadow(mut self, shadow: DropShadow) -> Self {
    self.drop_shadow = Some(shadow);
    self
  }

  /// Check if this renderable has a drop shadow
  pub fn has_drop_shadow(&self) -> bool {
    self.drop_shadow.is_some()
  }

  /// Set a blob shadow effect
  ///
  /// Blob shadows are simple elliptical shadows rendered beneath the sprite.
  /// Useful for characters in platformers or top-down games.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// use ryo2d::prelude::*;
  ///
  /// // Simple blob shadow
  /// let sprite = Renderable::sprite(handle).with_blob_shadow(BlobShadow::default());
  ///
  /// // Custom elliptical shadow
  /// let sprite = Renderable::sprite(handle).with_blob_shadow(
  ///     BlobShadow::ellipse(24.0, 10.0).color(Color::new(0, 0, 0, 80))
  /// );
  /// ```
  pub fn with_blob_shadow(mut self, shadow: BlobShadow) -> Self {
    self.blob_shadow = Some(shadow);
    self
  }

  /// Check if this renderable has a blob shadow
  pub fn has_blob_shadow(&self) -> bool {
    self.blob_shadow.is_some()
  }

  /// Add a contact shadow effect to this renderable.
  ///
  /// Contact shadows are small, concentrated shadows at the contact point
  /// (base of sprite), simulating ambient occlusion. They're darkest at the
  /// center and fade radially outward.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// use ryo2d::prelude::*;
  ///
  /// // Simple contact shadow
  /// let sprite = Renderable::sprite(handle).with_contact_shadow(ContactShadow::default());
  ///
  /// // Sharp contact shadow for heavy objects
  /// let sprite = Renderable::sprite(handle).with_contact_shadow(
  ///     ContactShadow::sharp(10.0)
  /// );
  /// ```
  pub fn with_contact_shadow(mut self, shadow: ContactShadow) -> Self {
    self.contact_shadow = Some(shadow);
    self
  }

  /// Check if this renderable has a contact shadow
  pub fn has_contact_shadow(&self) -> bool {
    self.contact_shadow.is_some()
  }

  /// Enable render interpolation for smooth motion.
  ///
  /// When enabled, the entity's position is interpolated between the previous
  /// and current Transform during rendering. This eliminates micro-stutter
  /// when the render FPS exceeds the fixed update rate.
  ///
  /// Best for:
  /// - Smoothly moving characters, projectiles, cameras
  /// - High refresh rate displays (120hz, 144hz, 240hz)
  ///
  /// Not recommended for:
  /// - Pixel art that should snap to pixel boundaries
  /// - Static or rarely-moving entities
  /// - Entities that teleport frequently
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// // Enable interpolation for smooth player movement
  /// let player = Renderable::sprite(handle).interpolated();
  ///
  /// // Or explicitly set it
  /// let enemy = Renderable::sprite(handle).with_interpolation(true);
  /// ```
  pub fn interpolated(mut self) -> Self {
    self.interpolate = true;
    self
  }

  /// Set render interpolation enabled/disabled.
  ///
  /// See [`interpolated`](Self::interpolated) for details.
  pub fn with_interpolation(mut self, enabled: bool) -> Self {
    self.interpolate = enabled;
    self
  }

  /// Check if this is a single sprite (not tiled)
  pub fn is_single(&self) -> bool {
    matches!(self.style, RenderStyle::Single)
  }

  /// Check if this is a tiled sprite
  pub fn is_tiled(&self) -> bool {
    matches!(self.style, RenderStyle::Tiled { .. })
  }

  /// Get tile count if tiled, otherwise 1
  pub fn tile_count(&self) -> u32 {
    match &self.style {
      RenderStyle::Single => 1,
      RenderStyle::Tiled {
        direction,
        target_size,
      } => {
        let tile_size = if direction.is_horizontal() {
          self.sprite.width() as f32
        } else {
          self.sprite.height() as f32
        };

        if tile_size <= 0.0 {
          return 0;
        }

        (target_size / tile_size).ceil() as u32
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::graphics::Sprite;

  fn test_handle() -> SpriteHandle {
    // Create a minimal test sprite handle
    let sprite = Sprite::solid_rect(16, 16, 255, 0, 0, 255);
    SpriteHandle::from_sprite(sprite, "test")
  }

  #[test]
  fn sprite_defaults() {
    let r = Renderable::sprite(test_handle());
    assert!(r.visible);
    assert_eq!(r.z_order, 0);
    assert!(r.is_single());
    assert!(!r.is_tiled());
    assert_eq!(r.mode, RenderMode::Auto);
    assert!(!r.flip_x);
    assert!(!r.flip_y);
    assert_eq!(r.alpha, 1.0);
  }

  #[test]
  fn tiled_defaults() {
    let r = Renderable::tiled(test_handle(), TileDirection::Right, 100.0);
    assert!(r.is_tiled());
    assert!(!r.is_single());
  }

  #[test]
  fn tile_count_calculation() {
    let r = Renderable::tiled(test_handle(), TileDirection::Right, 100.0);
    // 16px sprite, 100px target = ceil(100/16) = 7 tiles
    assert_eq!(r.tile_count(), 7);
  }

  #[test]
  fn builder_methods() {
    let r = Renderable::sprite(test_handle())
      .with_z_order(5)
      .with_visible(false)
      .with_mode(RenderMode::Gpu)
      .with_flip_x(true)
      .with_alpha(0.5);

    assert_eq!(r.z_order, 5);
    assert!(!r.visible);
    assert_eq!(r.mode, RenderMode::Gpu);
    assert!(r.flip_x);
    assert_eq!(r.alpha, 0.5);
  }

  #[test]
  fn alpha_clamps() {
    let r = Renderable::sprite(test_handle()).with_alpha(2.0);
    assert_eq!(r.alpha, 1.0);

    let r = Renderable::sprite(test_handle()).with_alpha(-0.5);
    assert_eq!(r.alpha, 0.0);
  }

  #[test]
  fn drop_shadow_defaults() {
    let shadow = DropShadow::default();
    assert_eq!(shadow.offset.x, 4.0);
    assert_eq!(shadow.offset.y, 4.0);
    assert_eq!(shadow.color, Color::new(0, 0, 0, 128));
    assert_eq!(shadow.blur, 0.0);
    assert_eq!(shadow.scale.x, 1.0);
    assert_eq!(shadow.scale.y, 1.0);
  }

  #[test]
  fn drop_shadow_builder() {
    let shadow = DropShadow::new()
      .offset(10.0, 20.0)
      .color(Color::new(255, 0, 0, 64))
      .blur(5.0)
      .scale(0.8, 0.5);

    assert_eq!(shadow.offset.x, 10.0);
    assert_eq!(shadow.offset.y, 20.0);
    assert_eq!(shadow.color, Color::new(255, 0, 0, 64));
    assert_eq!(shadow.blur, 5.0);
    assert_eq!(shadow.scale.x, 0.8);
    assert_eq!(shadow.scale.y, 0.5);
  }

  #[test]
  fn drop_shadow_presets() {
    let below = DropShadow::below(8.0);
    assert_eq!(below.offset.x, 0.0);
    assert_eq!(below.offset.y, 8.0);

    let diagonal = DropShadow::diagonal(6.0);
    assert_eq!(diagonal.offset.x, 6.0);
    assert_eq!(diagonal.offset.y, 6.0);

    let squashed = DropShadow::squashed(10.0, 0.3);
    assert_eq!(squashed.offset.x, 0.0);
    assert_eq!(squashed.offset.y, 10.0);
    assert_eq!(squashed.scale.x, 1.0);
    assert_eq!(squashed.scale.y, 0.3);
  }

  #[test]
  fn drop_shadow_blur_clamps() {
    let shadow = DropShadow::new().blur(-5.0);
    assert_eq!(shadow.blur, 0.0);
  }

  #[test]
  fn renderable_with_drop_shadow() {
    let r = Renderable::sprite(test_handle());
    assert!(!r.has_drop_shadow());
    assert!(r.drop_shadow.is_none());

    let r = Renderable::sprite(test_handle()).with_drop_shadow(DropShadow::default());
    assert!(r.has_drop_shadow());
    assert!(r.drop_shadow.is_some());
  }

  #[test]
  fn blob_shadow_defaults() {
    let shadow = BlobShadow::default();
    assert_eq!(shadow.radius_x, 16.0);
    assert_eq!(shadow.radius_y, 8.0);
    assert_eq!(shadow.offset_y, 0.0);
    assert_eq!(shadow.color, Color::new(0, 0, 0, 100));
    assert_eq!(shadow.height, 0.0);
    assert!(!shadow.height_scaling);
  }

  #[test]
  fn blob_shadow_circle() {
    let shadow = BlobShadow::circle(20.0);
    assert_eq!(shadow.radius_x, 20.0);
    assert_eq!(shadow.radius_y, 10.0); // Squashed by default
  }

  #[test]
  fn blob_shadow_ellipse() {
    let shadow = BlobShadow::ellipse(30.0, 15.0);
    assert_eq!(shadow.radius_x, 30.0);
    assert_eq!(shadow.radius_y, 15.0);
  }

  #[test]
  fn blob_shadow_builder() {
    let shadow = BlobShadow::new()
      .radii(24.0, 12.0)
      .offset(10.0)
      .color(Color::new(50, 50, 50, 150))
      .with_height_scale(true)
      .max_height(200.0);

    assert_eq!(shadow.radius_x, 24.0);
    assert_eq!(shadow.radius_y, 12.0);
    assert_eq!(shadow.offset_y, 10.0);
    assert_eq!(shadow.color, Color::new(50, 50, 50, 150));
    assert!(shadow.height_scaling);
    assert_eq!(shadow.max_height, 200.0);
  }

  #[test]
  fn blob_shadow_height_scaling() {
    let mut shadow = BlobShadow::circle(20.0)
      .with_height_scale(true)
      .max_height(100.0);

    // On ground - full size and alpha
    shadow.set_height(0.0);
    let (rx, ry) = shadow.effective_radius();
    assert_eq!(rx, 20.0);
    assert_eq!(ry, 10.0);
    assert!((shadow.effective_alpha() - (100.0 / 255.0)).abs() < 0.01);

    // Half height - reduced size and alpha
    shadow.set_height(50.0);
    let (rx, ry) = shadow.effective_radius();
    assert_eq!(rx, 10.0); // 50% of original
    assert_eq!(ry, 5.0);

    // At max height - minimum size (0.2 factor)
    shadow.set_height(100.0);
    let (rx, ry) = shadow.effective_radius();
    assert_eq!(rx, 4.0); // 20% of original
    assert_eq!(ry, 2.0);
  }

  #[test]
  fn renderable_with_blob_shadow() {
    let r = Renderable::sprite(test_handle());
    assert!(!r.has_blob_shadow());
    assert!(r.blob_shadow.is_none());

    let r = Renderable::sprite(test_handle()).with_blob_shadow(BlobShadow::default());
    assert!(r.has_blob_shadow());
    assert!(r.blob_shadow.is_some());
  }
}
