//! Parallax layers: images laid out for every camera from its own view.
//!
//! ```ignore
//! ctx.world.spawn(ParallaxLayer::new(&sky).with_z_order(-100));
//! ctx.world.spawn(
//!     ParallaxLayer::new(&mountains)
//!         .with_scroll(Vec2::splat(0.3))
//!         .with_tile(TileMode::InfiniteScroll)
//!         .with_auto_scroll(Vec2::new(10.0, 0.0))
//!         .with_z_order(-80),
//! );
//! ```

use glam::Vec2;

use super::{CameraView, SpriteInstance};
use crate::core::asset::SpriteHandle;
use crate::core::ecs::{Component, Query, Res};
use crate::core::render::batch::{BlendMode, Filter};
use crate::core::time::Time;
use crate::core::ui::Color;

/// Controls how a parallax layer tiles horizontally.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TileMode {
  /// No tiling - the sprite is drawn once at its position.
  /// Suitable for static sky backgrounds or non-repeating elements.
  #[default]
  None,

  /// Tile horizontally to fill the screen width.
  /// The sprite repeats seamlessly across the screen.
  Horizontal,

  /// Infinite horizontal scrolling with seamless wrapping.
  /// The sprite tiles continuously as the camera moves,
  /// creating an endless scrolling effect.
  InfiniteScroll,
}

/// The most quads one layer draws per camera (a tiny scale tiles no further).
pub(crate) const MAX_PARALLAX_TILES: usize = 1024;

/// An image behind (or in front of) the world that each camera sees from
/// its own view: fixed on screen, moving with the world, or anything
/// between, and optionally tiled and scrolling by itself.
///
/// Spawn it as an entity; `RenderLayers` on the entity picks the cameras
/// that draw it. It sorts by z with entities and sprites.
#[derive(Component, Debug, Clone)]
pub struct ParallaxLayer {
  /// The layer's image
  pub sprite: SpriteHandle,
  /// How far it moves with the camera, per axis: 0 stays on screen, 1
  /// stays in the world
  pub scroll: Vec2,
  /// World units per second it scrolls by itself (on game time)
  pub auto_scroll: Vec2,
  /// Where it sits on screen, in world units at scale 1 (positive moves it
  /// right and down)
  pub offset: Vec2,
  /// How it tiles horizontally
  pub tile: TileMode,
  /// Its size relative to its image
  pub scale: f32,
  pub tint: Color,
  pub z_order: i32,
  pub blend: BlendMode,
  pub filter: Filter,
  /// Lit by 2D lighting (default `true`)
  pub lighting: bool,
  /// How far it has scrolled by itself so far
  pub scrolled: Vec2,
}

impl ParallaxLayer {
  /// A layer of `sprite` fixed on screen at its top-left, untiled, at its
  /// own size, untinted and lit.
  pub fn new(sprite: &SpriteHandle) -> Self {
    Self {
      sprite: sprite.clone(),
      scroll: Vec2::ZERO,
      auto_scroll: Vec2::ZERO,
      offset: Vec2::ZERO,
      tile: TileMode::None,
      scale: 1.0,
      tint: Color::WHITE,
      z_order: 0,
      blend: BlendMode::Alpha,
      filter: Filter::Default,
      lighting: true,
      scrolled: Vec2::ZERO,
    }
  }

  /// Set how far it moves with the camera, per axis.
  pub fn with_scroll(mut self, scroll: Vec2) -> Self {
    self.scroll = scroll;
    self
  }

  /// Set how fast it scrolls by itself, in world units per second.
  pub fn with_auto_scroll(mut self, speed: Vec2) -> Self {
    self.auto_scroll = speed;
    self
  }

  /// Set where it sits on screen.
  pub fn with_offset(mut self, offset: Vec2) -> Self {
    self.offset = offset;
    self
  }

  /// Set how it tiles.
  pub fn with_tile(mut self, tile: TileMode) -> Self {
    self.tile = tile;
    self
  }

  /// Set its size relative to its image.
  pub fn with_scale(mut self, scale: f32) -> Self {
    self.scale = scale;
    self
  }

  pub fn with_tint(mut self, tint: Color) -> Self {
    self.tint = tint;
    self
  }

  pub fn with_z_order(mut self, z_order: i32) -> Self {
    self.z_order = z_order;
    self
  }

  pub fn with_blend(mut self, blend: BlendMode) -> Self {
    self.blend = blend;
    self
  }

  pub fn with_filter(mut self, filter: Filter) -> Self {
    self.filter = filter;
    self
  }

  /// Set whether 2D lighting lights it.
  pub fn with_lighting(mut self, lighting: bool) -> Self {
    self.lighting = lighting;
    self
  }
}

/// `layer`'s quads (its image `texels` in size) for `view`, into `out`
/// (cleared first): laid out in the view's screen space, scrolled by the
/// camera's centre times `scroll` and by `scrolled`, then taken into the
/// world. Nothing when its scaled size is not finite and positive; at most
/// [`MAX_PARALLAX_TILES`].
pub(crate) fn layout(
  layer: &ParallaxLayer,
  texels: Vec2,
  view: &CameraView,
  out: &mut Vec<SpriteInstance>,
) {
  out.clear();
  let scale = layer.scale * view.scale;
  let size = texels * scale;
  if !(size.x.is_finite() && size.y.is_finite() && size.x > 0.0 && size.y > 0.0) {
    return;
  }
  let camera = view.center * layer.scroll;
  let shift = (camera + layer.scrolled - layer.offset) * scale;
  let y = -shift.y;
  let screen_width = view.rect_size.x;
  let mut place = |x: f32| {
    if out.len() >= MAX_PARALLAX_TILES {
      return;
    }
    let corner = Vec2::new(x, y);
    let center = view.screen_to_world(view.rect_min + corner + size / 2.0);
    let world = size / view.scale;
    out.push(
      SpriteInstance::new()
        .with_transform(center.x, center.y, view.rotation, world.x, world.y)
        .with_tint(layer.tint, 1.0),
    );
  };
  let visible = |x: f32| x + size.x >= 0.0 && x < screen_width;
  match layer.tile {
    TileMode::None => place(-shift.x),
    TileMode::Horizontal => {
      let tiles = (screen_width / size.x).ceil() + 1.0;
      for i in 0..tiles.min(MAX_PARALLAX_TILES as f32) as usize {
        let x = i as f32 * size.x - shift.x;
        if visible(x) {
          place(x);
        }
      }
    }
    TileMode::InfiniteScroll => {
      let tiles = (screen_width / size.x).ceil() + 2.0;
      let base = shift.x.rem_euclid(size.x);
      for i in 0..tiles.min(MAX_PARALLAX_TILES as f32) as usize {
        let x = i as f32 * size.x - base - size.x;
        if visible(x) {
          place(x);
        }
      }
    }
  }
}

/// `PostUpdateSet::Extract`: layers scroll by themselves on game time.
pub(crate) fn advance_parallax(time: Res<Time>, mut layers: Query<&mut ParallaxLayer>) {
  let dt = time.game.delta();
  for mut layer in &mut layers {
    if layer.auto_scroll != Vec2::ZERO {
      let step = layer.auto_scroll * dt;
      layer.scrolled += step;
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::graphics::{Camera2d, CameraView, Sprite};

  fn view(center: Vec2) -> CameraView {
    CameraView::compute(
      center,
      0.0,
      &Camera2d::default(),
      Vec2::ZERO,
      Vec2::new(64.0, 64.0),
    )
  }

  fn layer() -> ParallaxLayer {
    let sprite = SpriteHandle::from_sprite(Sprite::solid_rect(16, 16, 255, 255, 255, 255), "plx");
    ParallaxLayer::new(&sprite)
      .with_scroll(Vec2::splat(0.5))
      .with_tile(TileMode::InfiniteScroll)
  }

  #[test]
  fn each_camera_lays_the_layer_out_from_its_own_view() {
    let (mut a, mut b) = (Vec::new(), Vec::new());
    layout(
      &layer(),
      Vec2::splat(16.0),
      &view(Vec2::new(32.0, 32.0)),
      &mut a,
    );
    layout(
      &layer(),
      Vec2::splat(16.0),
      &view(Vec2::new(1032.0, 32.0)),
      &mut b,
    );
    assert!(!a.is_empty() && !b.is_empty());
    // Every quad stays inside its own camera's view
    for (quads, centre) in [(&a, 32.0), (&b, 1032.0)] {
      for q in quads {
        assert!(
          (q.transform[2][0] - centre).abs() <= 32.0 + 16.0,
          "{}",
          q.transform[2][0]
        );
      }
    }
  }

  #[test]
  fn a_degenerate_scale_draws_nothing_or_a_bounded_count() {
    for scale in [0.0, -1.0, f32::NAN, 1e-9] {
      let mut out = Vec::new();
      layout(
        &layer().with_scale(scale),
        Vec2::splat(16.0),
        &view(Vec2::splat(32.0)),
        &mut out,
      );
      assert!(out.len() <= MAX_PARALLAX_TILES, "{scale}: {}", out.len());
    }
  }
}
