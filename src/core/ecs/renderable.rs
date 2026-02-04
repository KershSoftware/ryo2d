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
}
