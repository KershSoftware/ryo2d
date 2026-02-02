//! Tiled rendering system with grouped collision support
//!
//! This module provides components for rendering repeating tile patterns
//! and composite tiles (like pipes with cap + body), along with grouped
//! colliders that cover the entire composite as a single physics body.
//!
//! # Components
//!
//! - [`CompositeTile`] - Combines multiple parts (static + tiled) into one entity
//! - [`GroupedCollider`] - Single collision box covering a composite tile
//! - [`TileDirection`] - Direction for tiled rendering (Up, Down, Left, Right)
//!
//! # Example
//!
//! ```rust,ignore
//! // Create a pipe with cap and tiled body
//! let (composite, collider) = CompositeTileBuilder::new()
//!     .cap(cap_sprite, 0.0, 0.0)
//!     .body_tiled(body_sprite, 0.0, 32.0, TileDirection::Down, 128.0)
//!     .build_with_collider();
//!
//! world.spawn((
//!     Transform::new(100.0, 50.0),
//!     RigidBody::kinematic(),
//!     composite,
//!     collider,
//! ));
//! ```

use crate::core::asset::SpriteHandle;

/// Direction in which tiles are repeated
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TileDirection {
  /// Tiles repeat upward (negative Y)
  Up,
  /// Tiles repeat downward (positive Y)
  #[default]
  Down,
  /// Tiles repeat leftward (negative X)
  Left,
  /// Tiles repeat rightward (positive X)
  Right,
}

impl TileDirection {
  /// Get the offset for each tile in this direction
  ///
  /// Returns (dx, dy) per tile where exactly one component is non-zero.
  pub fn offset(&self, tile_size: f32) -> (f32, f32) {
    match self {
      TileDirection::Up => (0.0, -tile_size),
      TileDirection::Down => (0.0, tile_size),
      TileDirection::Left => (-tile_size, 0.0),
      TileDirection::Right => (tile_size, 0.0),
    }
  }

  /// Returns true if this direction is horizontal
  pub fn is_horizontal(&self) -> bool {
    matches!(self, TileDirection::Left | TileDirection::Right)
  }

  /// Returns true if this direction is vertical
  pub fn is_vertical(&self) -> bool {
    matches!(self, TileDirection::Up | TileDirection::Down)
  }
}

/// A single part of a composite tile
///
/// Each part can be either static (drawn once) or tiled (repeated).
#[derive(Debug, Clone)]
pub struct TilePart {
  /// The sprite for this part
  pub sprite: SpriteHandle,
  /// Offset from the composite tile's origin (X)
  pub offset_x: f32,
  /// Offset from the composite tile's origin (Y)
  pub offset_y: f32,
  /// Tiling configuration: None = static, Some = (direction, target_size)
  pub tiling: Option<(TileDirection, f32)>,
  /// Flip sprite horizontally
  pub flip_x: bool,
  /// Flip sprite vertically
  pub flip_y: bool,
}

impl TilePart {
  /// Create a static (non-tiled) part
  pub fn static_part(sprite: SpriteHandle, offset_x: f32, offset_y: f32) -> Self {
    TilePart {
      sprite,
      offset_x,
      offset_y,
      tiling: None,
      flip_x: false,
      flip_y: false,
    }
  }

  /// Create a tiled part
  pub fn tiled_part(
    sprite: SpriteHandle,
    offset_x: f32,
    offset_y: f32,
    direction: TileDirection,
    target_size: f32,
  ) -> Self {
    TilePart {
      sprite,
      offset_x,
      offset_y,
      tiling: Some((direction, target_size)),
      flip_x: false,
      flip_y: false,
    }
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

  /// Check if this part is tiled
  pub fn is_tiled(&self) -> bool {
    self.tiling.is_some()
  }

  /// Get the bounds of this part
  ///
  /// For static parts, returns the sprite size.
  /// For tiled parts, returns the full tiled area.
  pub fn bounds(&self) -> (f32, f32) {
    let sprite_w = self.sprite.width() as f32;
    let sprite_h = self.sprite.height() as f32;

    match &self.tiling {
      None => (sprite_w, sprite_h),
      Some((direction, target_size)) => match direction {
        TileDirection::Up | TileDirection::Down => (sprite_w, *target_size),
        TileDirection::Left | TileDirection::Right => (*target_size, sprite_h),
      },
    }
  }
}

/// Composite tile component
///
/// Combines multiple parts (static and/or tiled) into a single entity.
/// Useful for complex structures like pipes (cap + body) or walls with
/// decorative edges.
///
/// # Example
///
/// ```rust,ignore
/// let composite = CompositeTile::new()
///     .add_part(TilePart::static_part(cap, 0.0, 0.0))
///     .add_part(TilePart::tiled_part(body, 0.0, 32.0, TileDirection::Down, 128.0));
/// ```
#[derive(Debug, Clone)]
pub struct CompositeTile {
  /// The parts that make up this composite
  pub parts: Vec<TilePart>,
  /// Whether this composite is visible
  pub visible: bool,
  /// Render order (higher = drawn later, on top)
  pub z_order: i32,
}

impl CompositeTile {
  /// Create a new empty composite tile
  pub fn new() -> Self {
    CompositeTile {
      parts: Vec::new(),
      visible: true,
      z_order: 0,
    }
  }

  /// Add a part to this composite
  pub fn add_part(mut self, part: TilePart) -> Self {
    self.parts.push(part);
    self
  }

  /// Set visibility
  pub fn with_visible(mut self, visible: bool) -> Self {
    self.visible = visible;
    self
  }

  /// Set z-order for rendering
  pub fn with_z_order(mut self, z_order: i32) -> Self {
    self.z_order = z_order;
    self
  }

  /// Calculate the bounding box of this composite tile
  ///
  /// Returns (min_x, min_y, max_x, max_y) relative to the entity origin.
  pub fn bounds(&self) -> (f32, f32, f32, f32) {
    if self.parts.is_empty() {
      return (0.0, 0.0, 0.0, 0.0);
    }

    let mut min_x = f32::MAX;
    let mut min_y = f32::MAX;
    let mut max_x = f32::MIN;
    let mut max_y = f32::MIN;

    for part in &self.parts {
      let (w, h) = part.bounds();

      // Handle negative direction offsets for Up and Left
      let (part_min_x, part_min_y, part_max_x, part_max_y) = match &part.tiling {
        Some((TileDirection::Up, _)) => {
          // Tiling upward: part extends from offset_y - h to offset_y
          (
            part.offset_x,
            part.offset_y - h,
            part.offset_x + w,
            part.offset_y,
          )
        }
        Some((TileDirection::Left, _)) => {
          // Tiling leftward: part extends from offset_x - w to offset_x
          (
            part.offset_x - w,
            part.offset_y,
            part.offset_x,
            part.offset_y + h,
          )
        }
        _ => {
          // Static, Down, or Right: part extends from offset to offset + size
          (
            part.offset_x,
            part.offset_y,
            part.offset_x + w,
            part.offset_y + h,
          )
        }
      };

      min_x = min_x.min(part_min_x);
      min_y = min_y.min(part_min_y);
      max_x = max_x.max(part_max_x);
      max_y = max_y.max(part_max_y);
    }

    (min_x, min_y, max_x, max_y)
  }

  /// Get the width and height of the composite
  pub fn size(&self) -> (f32, f32) {
    let (min_x, min_y, max_x, max_y) = self.bounds();
    (max_x - min_x, max_y - min_y)
  }
}

impl Default for CompositeTile {
  fn default() -> Self {
    Self::new()
  }
}

/// Grouped collider for composite tiles
///
/// A single collision box that covers an entire composite tile.
/// This is more efficient than having separate colliders for each part
/// and ensures consistent collision behavior.
///
/// # Example
///
/// ```rust,ignore
/// let collider = GroupedCollider::new(32.0, 160.0);
/// world.spawn((
///     Transform::new(100.0, 50.0),
///     RigidBody::kinematic(),
///     composite,
///     collider,
/// ));
/// ```
#[derive(Debug, Clone)]
pub struct GroupedCollider {
  /// Width of the collision box
  pub width: f32,
  /// Height of the collision box
  pub height: f32,
  /// Offset from entity position (X)
  pub offset_x: f32,
  /// Offset from entity position (Y)
  pub offset_y: f32,
  /// Whether this collider is a trigger (no physics response, just events)
  pub is_trigger: bool,
  /// Restitution (bounciness): 0.0 = no bounce, 1.0 = perfect bounce
  pub restitution: f32,
  /// Friction coefficient
  pub friction: f32,
}

impl GroupedCollider {
  /// Create a new grouped collider with the given dimensions
  pub fn new(width: f32, height: f32) -> Self {
    GroupedCollider {
      width,
      height,
      offset_x: 0.0,
      offset_y: 0.0,
      is_trigger: false,
      restitution: 0.0,
      friction: 0.5,
    }
  }

  /// Create a grouped collider from a composite tile's bounds
  pub fn from_composite(composite: &CompositeTile) -> Self {
    let (min_x, min_y, max_x, max_y) = composite.bounds();
    let width = max_x - min_x;
    let height = max_y - min_y;

    GroupedCollider {
      width,
      height,
      offset_x: min_x,
      offset_y: min_y,
      is_trigger: false,
      restitution: 0.0,
      friction: 0.5,
    }
  }

  /// Set the offset from entity position
  pub fn with_offset(mut self, x: f32, y: f32) -> Self {
    self.offset_x = x;
    self.offset_y = y;
    self
  }

  /// Make this a trigger collider
  pub fn as_trigger(mut self) -> Self {
    self.is_trigger = true;
    self
  }

  /// Set restitution (bounciness)
  pub fn with_restitution(mut self, restitution: f32) -> Self {
    self.restitution = restitution;
    self
  }

  /// Set friction coefficient
  pub fn with_friction(mut self, friction: f32) -> Self {
    self.friction = friction;
    self
  }
}

/// Builder for creating composite tiles with colliders
///
/// Provides a fluent API for constructing pipes, walls, and other
/// composite tile structures.
///
/// # Example
///
/// ```rust,ignore
/// let (composite, collider) = CompositeTileBuilder::new()
///     .cap(cap_sprite, 0.0, 0.0)
///     .body_tiled(body_sprite, 0.0, 32.0, TileDirection::Down, 128.0)
///     .z_order(5)
///     .build_with_collider();
/// ```
#[derive(Debug, Clone)]
pub struct CompositeTileBuilder {
  parts: Vec<TilePart>,
  z_order: i32,
  visible: bool,
}

impl CompositeTileBuilder {
  /// Create a new builder
  pub fn new() -> Self {
    CompositeTileBuilder {
      parts: Vec::new(),
      z_order: 0,
      visible: true,
    }
  }

  /// Add a static cap/header part
  pub fn cap(mut self, sprite: SpriteHandle, offset_x: f32, offset_y: f32) -> Self {
    self
      .parts
      .push(TilePart::static_part(sprite, offset_x, offset_y));
    self
  }

  /// Add a static cap/header part with vertical flip
  pub fn cap_flipped_y(mut self, sprite: SpriteHandle, offset_x: f32, offset_y: f32) -> Self {
    self
      .parts
      .push(TilePart::static_part(sprite, offset_x, offset_y).with_flip_y(true));
    self
  }

  /// Add a static cap/header part with specified flip options
  pub fn cap_flipped(
    mut self,
    sprite: SpriteHandle,
    offset_x: f32,
    offset_y: f32,
    flip_x: bool,
    flip_y: bool,
  ) -> Self {
    self.parts.push(
      TilePart::static_part(sprite, offset_x, offset_y)
        .with_flip_x(flip_x)
        .with_flip_y(flip_y),
    );
    self
  }

  /// Add a static part (alias for cap)
  pub fn static_part(self, sprite: SpriteHandle, offset_x: f32, offset_y: f32) -> Self {
    self.cap(sprite, offset_x, offset_y)
  }

  /// Add a tiled body part
  pub fn body_tiled(
    mut self,
    sprite: SpriteHandle,
    offset_x: f32,
    offset_y: f32,
    direction: TileDirection,
    target_size: f32,
  ) -> Self {
    self.parts.push(TilePart::tiled_part(
      sprite,
      offset_x,
      offset_y,
      direction,
      target_size,
    ));
    self
  }

  /// Add a tiled body part with specified flip options
  #[allow(clippy::too_many_arguments)]
  pub fn body_tiled_flipped(
    mut self,
    sprite: SpriteHandle,
    offset_x: f32,
    offset_y: f32,
    direction: TileDirection,
    target_size: f32,
    flip_x: bool,
    flip_y: bool,
  ) -> Self {
    self.parts.push(
      TilePart::tiled_part(sprite, offset_x, offset_y, direction, target_size)
        .with_flip_x(flip_x)
        .with_flip_y(flip_y),
    );
    self
  }

  /// Add a tiled part (alias for body_tiled)
  pub fn tiled_part(
    self,
    sprite: SpriteHandle,
    offset_x: f32,
    offset_y: f32,
    direction: TileDirection,
    target_size: f32,
  ) -> Self {
    self.body_tiled(sprite, offset_x, offset_y, direction, target_size)
  }

  /// Set z-order for rendering
  pub fn z_order(mut self, z_order: i32) -> Self {
    self.z_order = z_order;
    self
  }

  /// Set visibility
  pub fn visible(mut self, visible: bool) -> Self {
    self.visible = visible;
    self
  }

  /// Build just the composite tile
  pub fn build(self) -> CompositeTile {
    CompositeTile {
      parts: self.parts,
      visible: self.visible,
      z_order: self.z_order,
    }
  }

  /// Build the composite tile with an auto-calculated collider
  pub fn build_with_collider(self) -> (CompositeTile, GroupedCollider) {
    let composite = CompositeTile {
      parts: self.parts,
      visible: self.visible,
      z_order: self.z_order,
    };
    let collider = GroupedCollider::from_composite(&composite);
    (composite, collider)
  }
}

impl Default for CompositeTileBuilder {
  fn default() -> Self {
    Self::new()
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::graphics::Sprite;

  fn test_sprite(w: u32, h: u32) -> SpriteHandle {
    let sprite = Sprite::solid_rect(w, h, 255, 0, 0, 255);
    SpriteHandle::from_sprite(sprite, "test")
  }

  #[test]
  fn tile_direction_offsets() {
    assert_eq!(TileDirection::Up.offset(32.0), (0.0, -32.0));
    assert_eq!(TileDirection::Down.offset(32.0), (0.0, 32.0));
    assert_eq!(TileDirection::Left.offset(32.0), (-32.0, 0.0));
    assert_eq!(TileDirection::Right.offset(32.0), (32.0, 0.0));
  }

  #[test]
  fn tile_direction_orientation() {
    assert!(TileDirection::Left.is_horizontal());
    assert!(TileDirection::Right.is_horizontal());
    assert!(!TileDirection::Up.is_horizontal());
    assert!(!TileDirection::Down.is_horizontal());

    assert!(TileDirection::Up.is_vertical());
    assert!(TileDirection::Down.is_vertical());
    assert!(!TileDirection::Left.is_vertical());
    assert!(!TileDirection::Right.is_vertical());
  }

  #[test]
  fn tile_part_bounds() {
    let sprite = test_sprite(32, 16);

    let static_part = TilePart::static_part(sprite.clone(), 0.0, 0.0);
    assert_eq!(static_part.bounds(), (32.0, 16.0));

    let tiled_part = TilePart::tiled_part(sprite, 0.0, 0.0, TileDirection::Down, 100.0);
    assert_eq!(tiled_part.bounds(), (32.0, 100.0));
  }

  #[test]
  fn composite_tile_bounds() {
    let cap = test_sprite(32, 16);
    let body = test_sprite(32, 8);

    let composite = CompositeTile::new()
      .add_part(TilePart::static_part(cap, 0.0, 0.0))
      .add_part(TilePart::tiled_part(
        body,
        0.0,
        16.0,
        TileDirection::Down,
        64.0,
      ));

    let (min_x, min_y, max_x, max_y) = composite.bounds();
    assert_eq!(min_x, 0.0);
    assert_eq!(min_y, 0.0);
    assert_eq!(max_x, 32.0);
    assert_eq!(max_y, 80.0); // 16 (cap) + 64 (body)

    let (w, h) = composite.size();
    assert_eq!(w, 32.0);
    assert_eq!(h, 80.0);
  }

  #[test]
  fn composite_tile_bounds_upward() {
    let cap = test_sprite(32, 16);
    let body = test_sprite(32, 8);

    // Cap at bottom, body extends upward
    let composite = CompositeTile::new()
      .add_part(TilePart::static_part(cap, 0.0, 0.0))
      .add_part(TilePart::tiled_part(
        body,
        0.0,
        0.0,
        TileDirection::Up,
        64.0,
      ));

    let (min_x, min_y, max_x, max_y) = composite.bounds();
    assert_eq!(min_x, 0.0);
    assert_eq!(min_y, -64.0); // Body extends upward
    assert_eq!(max_x, 32.0);
    assert_eq!(max_y, 16.0); // Cap height
  }

  #[test]
  fn grouped_collider_from_composite() {
    let cap = test_sprite(32, 16);
    let body = test_sprite(32, 8);

    let composite = CompositeTile::new()
      .add_part(TilePart::static_part(cap, 0.0, 0.0))
      .add_part(TilePart::tiled_part(
        body,
        0.0,
        16.0,
        TileDirection::Down,
        64.0,
      ));

    let collider = GroupedCollider::from_composite(&composite);
    assert_eq!(collider.width, 32.0);
    assert_eq!(collider.height, 80.0);
    assert_eq!(collider.offset_x, 0.0);
    assert_eq!(collider.offset_y, 0.0);
  }

  #[test]
  fn composite_builder() {
    let cap = test_sprite(32, 16);
    let body = test_sprite(32, 8);

    let (composite, collider) = CompositeTileBuilder::new()
      .cap(cap, 0.0, 0.0)
      .body_tiled(body, 0.0, 16.0, TileDirection::Down, 64.0)
      .z_order(5)
      .build_with_collider();

    assert_eq!(composite.parts.len(), 2);
    assert_eq!(composite.z_order, 5);
    assert_eq!(collider.width, 32.0);
    assert_eq!(collider.height, 80.0);
  }
}
