//! Cameras as entities: what a camera sees and where it draws.

use glam::{Mat4, Vec2, Vec3};

use super::TextureId;
use crate::core::ecs::{Component, Transform};
use crate::core::ui::Color;

use super::render_layers::RenderLayers;

/// A 2D camera. Its entity's `GlobalTransform` places it: the translation is
/// the world point at the centre of its view, the rotation turns the view.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
#[require(Transform, CameraView, RenderLayers)]
pub struct Camera2d {
  /// Magnification on top of the projection (2 shows half as much world)
  pub zoom: f32,
  /// How world units map to the camera's rectangle
  pub projection: Projection,
  /// The part of the target the camera draws into
  pub viewport: Viewport,
  /// Cameras on one target draw in ascending order
  pub order: i32,
  /// Where the camera draws
  pub target: CameraTarget,
  /// What the camera's rectangle is cleared to before it draws
  pub clear: CameraClear,
}

impl Default for Camera2d {
  fn default() -> Self {
    Self {
      zoom: 1.0,
      projection: Projection::ScreenUnits,
      viewport: Viewport::FULL,
      order: 0,
      target: CameraTarget::Window,
      clear: CameraClear::Background,
    }
  }
}

/// How world units map to a camera's rectangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Projection {
  /// One world unit is one screen unit (one target pixel off screen)
  ScreenUnits,
  /// Fit a `width`x`height` world area: the shorter axis decides the scale,
  /// so the longer one shows more world
  Fit { width: f32, height: f32 },
}

/// A rectangle of a camera's target, in fractions of it (0..1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
  pub min: Vec2,
  pub max: Vec2,
}

impl Viewport {
  /// The whole target.
  pub const FULL: Viewport = Viewport {
    min: Vec2::ZERO,
    max: Vec2::ONE,
  };

  /// The rectangle at `(x, y)` of `width`x`height`, in fractions of the target.
  pub fn new(x: f32, y: f32, width: f32, height: f32) -> Viewport {
    Viewport {
      min: Vec2::new(x, y),
      max: Vec2::new(x + width, y + height),
    }
  }
}

/// Where a camera draws.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CameraTarget {
  /// The window (screen space)
  Window,
  /// An offscreen image, which sprites can then draw
  Texture(RenderTarget),
}

/// An offscreen image cameras draw into (made by `RenderTargets`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RenderTarget(pub(crate) TextureId);

/// What a camera's rectangle is cleared to before it draws.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CameraClear {
  /// The frame's background colour
  Background,
  /// This colour
  Color(Color),
  /// Nothing: draw over what is there
  None,
}

/// Marks the camera that `ctx.camera()` and the pointer's world position
/// follow.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct MainCamera;

/// A camera's view this frame, computed by the engine from its transform,
/// `Camera2d` and shake.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct CameraView {
  /// World to the camera rectangle's clip space
  pub view_proj: Mat4,
  /// The world point at the centre of the view
  pub center: Vec2,
  /// The view's rotation, in radians
  pub rotation: f32,
  /// Screen units (or target pixels) per world unit
  pub scale: f32,
  /// The shake offset, in screen units
  pub shake: Vec2,
  /// The camera's rectangle in its target's units: top-left and size
  pub rect_min: Vec2,
  pub rect_size: Vec2,
  /// The world box the view can see (it encloses a rotated view)
  pub bounds: CameraBounds,
}

impl CameraView {
  /// The view of a camera centred on `center`, turned by `rotation`, shaken
  /// by `shake`, on a target of `target_size`.
  pub fn compute(
    center: Vec2,
    rotation: f32,
    camera: &Camera2d,
    shake: Vec2,
    target_size: Vec2,
  ) -> Self {
    let finite = |v: Vec2| if v.is_finite() { v } else { Vec2::ZERO };
    let positive = |x: f32| if x.is_finite() && x > 0.0 { x } else { 1.0 };
    let target_size = finite(target_size).max(Vec2::ONE);
    let rect_min = finite(camera.viewport.min).clamp(Vec2::ZERO, Vec2::ONE) * target_size;
    let rect_size = (finite(camera.viewport.max - camera.viewport.min)
      .clamp(Vec2::ZERO, Vec2::ONE)
      * target_size)
      .max(Vec2::ONE);
    let zoom = if camera.zoom.is_finite() {
      camera.zoom.clamp(0.01, 100.0)
    } else {
      1.0
    };
    let base = match camera.projection {
      Projection::ScreenUnits => 1.0,
      Projection::Fit { width, height } => {
        (rect_size.x / positive(width)).min(rect_size.y / positive(height))
      }
    };
    let scale = base * zoom;
    let rotation = if rotation.is_finite() { rotation } else { 0.0 };
    let (center, shake) = (finite(center), finite(shake));
    let view_proj = Mat4::from_scale(Vec3::new(2.0 / rect_size.x, -2.0 / rect_size.y, 1.0))
      * Mat4::from_translation(shake.extend(0.0))
      * Mat4::from_scale(Vec3::new(scale, scale, 1.0))
      * Mat4::from_rotation_z(-rotation)
      * Mat4::from_translation((-center).extend(0.0));
    let half = rect_size / (2.0 * scale);
    let (sin, cos) = rotation.sin_cos();
    let extent = Vec2::new(
      half.x * cos.abs() + half.y * sin.abs(),
      half.x * sin.abs() + half.y * cos.abs(),
    );
    let bounds = CameraBounds {
      left: center.x - extent.x,
      right: center.x + extent.x,
      top: center.y - extent.y,
      bottom: center.y + extent.y,
    };
    Self {
      view_proj,
      center,
      rotation,
      scale,
      shake,
      rect_min,
      rect_size,
      bounds,
    }
  }

  /// A world point in the target's units.
  pub fn world_to_screen(&self, world: Vec2) -> Vec2 {
    let local =
      Vec2::from_angle(-self.rotation).rotate(world - self.center) * self.scale + self.shake;
    self.rect_min + self.rect_size / 2.0 + local
  }

  /// A point in the target's units in the world.
  pub fn screen_to_world(&self, screen: Vec2) -> Vec2 {
    let local = screen - self.rect_min - self.rect_size / 2.0 - self.shake;
    Vec2::from_angle(self.rotation).rotate(local / self.scale) + self.center
  }

  /// [`world_to_screen`](Self::world_to_screen) for the engine's per-axis code.
  pub(crate) fn world_to_screen_xy(&self, x: f32, y: f32) -> (f32, f32) {
    let screen = self.world_to_screen(Vec2::new(x, y));
    (screen.x, screen.y)
  }

  /// [`screen_to_world`](Self::screen_to_world) for the engine's per-axis code.
  pub(crate) fn screen_to_world_xy(&self, x: f32, y: f32) -> (f32, f32) {
    let world = self.screen_to_world(Vec2::new(x, y));
    (world.x, world.y)
  }

  /// Whether the world rectangle at `(x, y)` of `width`x`height` can be seen.
  pub(crate) fn is_visible_rect(&self, x: f32, y: f32, width: f32, height: f32) -> bool {
    self.is_visible(Vec2::new(x, y), Vec2::new(x + width, y + height))
  }

  /// Whether the world box `min..max` can be seen.
  pub fn is_visible(&self, min: Vec2, max: Vec2) -> bool {
    max.x > self.bounds.left
      && min.x < self.bounds.right
      && max.y > self.bounds.top
      && min.y < self.bounds.bottom
  }
}

impl Default for CameraView {
  fn default() -> Self {
    Self::compute(Vec2::ZERO, 0.0, &Camera2d::default(), Vec2::ZERO, Vec2::ONE)
  }
}

/// Represents the visible world-space bounds of the camera
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraBounds {
  pub left: f32,
  pub right: f32,
  pub top: f32,
  pub bottom: f32,
}

impl CameraBounds {
  pub fn width(&self) -> f32 {
    self.right - self.left
  }

  pub fn height(&self) -> f32 {
    self.bottom - self.top
  }

  pub fn contains(&self, x: f32, y: f32) -> bool {
    x >= self.left && x <= self.right && y >= self.top && y <= self.bottom
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::f32::consts::FRAC_PI_2;

  fn close(a: Vec2, b: Vec2) -> bool {
    (a - b).length() < 1e-3
  }

  fn camera(projection: Projection, zoom: f32) -> Camera2d {
    Camera2d {
      zoom,
      projection,
      ..Default::default()
    }
  }

  #[test]
  fn a_plain_camera_maps_its_centre_to_the_screen_centre() {
    let view = CameraView::compute(
      Vec2::new(400.0, 300.0),
      0.0,
      &Camera2d::default(),
      Vec2::ZERO,
      Vec2::new(800.0, 600.0),
    );
    assert!(close(
      view.world_to_screen(Vec2::new(400.0, 300.0)),
      Vec2::new(400.0, 300.0)
    ));
    assert!(close(view.world_to_screen(Vec2::ZERO), Vec2::ZERO));
    assert_eq!((view.bounds.left, view.bounds.right), (0.0, 800.0));
    assert_eq!((view.bounds.top, view.bounds.bottom), (0.0, 600.0));
  }

  #[test]
  fn zoom_scales_around_the_centre() {
    let view = CameraView::compute(
      Vec2::new(400.0, 300.0),
      0.0,
      &camera(Projection::ScreenUnits, 2.0),
      Vec2::ZERO,
      Vec2::new(800.0, 600.0),
    );
    assert!(close(
      view.world_to_screen(Vec2::new(410.0, 300.0)),
      Vec2::new(420.0, 300.0)
    ));
    assert_eq!(view.bounds.right - view.bounds.left, 400.0);
  }

  #[test]
  fn fit_keeps_the_design_area_visible() {
    let fit = camera(
      Projection::Fit {
        width: 320.0,
        height: 180.0,
      },
      1.0,
    );
    let exact = CameraView::compute(Vec2::ZERO, 0.0, &fit, Vec2::ZERO, Vec2::new(1280.0, 720.0));
    assert_eq!(exact.scale, 4.0);
    assert!(close(
      exact.world_to_screen(Vec2::new(10.0, 0.0)),
      Vec2::new(680.0, 360.0)
    ));
    let wide = CameraView::compute(Vec2::ZERO, 0.0, &fit, Vec2::ZERO, Vec2::new(1600.0, 720.0));
    assert_eq!(wide.scale, 4.0, "the shorter axis decides");
    assert_eq!(
      wide.bounds.right - wide.bounds.left,
      400.0,
      "extra world on the long axis"
    );
    assert_eq!(wide.bounds.bottom - wide.bounds.top, 180.0);
  }

  #[test]
  fn a_quarter_turn_shows_the_right_hand_side_above() {
    let view = CameraView::compute(
      Vec2::new(50.0, 50.0),
      FRAC_PI_2,
      &Camera2d::default(),
      Vec2::ZERO,
      Vec2::new(100.0, 100.0),
    );
    assert!(close(
      view.world_to_screen(Vec2::new(60.0, 50.0)),
      Vec2::new(50.0, 40.0)
    ));
  }

  #[test]
  fn a_viewport_is_a_rectangle_of_the_target() {
    let half = Camera2d {
      viewport: Viewport::new(0.5, 0.0, 0.5, 1.0),
      ..Default::default()
    };
    let view = CameraView::compute(
      Vec2::new(10.0, 10.0),
      0.0,
      &half,
      Vec2::ZERO,
      Vec2::new(800.0, 600.0),
    );
    assert_eq!(
      (view.rect_min, view.rect_size),
      (Vec2::new(400.0, 0.0), Vec2::new(400.0, 600.0))
    );
    assert!(close(
      view.world_to_screen(Vec2::new(10.0, 10.0)),
      Vec2::new(600.0, 300.0)
    ));
  }

  #[test]
  fn conversions_round_trip_through_everything() {
    let camera = Camera2d {
      zoom: 1.7,
      projection: Projection::Fit {
        width: 300.0,
        height: 200.0,
      },
      viewport: Viewport::new(0.1, 0.2, 0.6, 0.7),
      ..Default::default()
    };
    let view = CameraView::compute(
      Vec2::new(-30.0, 75.0),
      0.6,
      &camera,
      Vec2::new(3.0, -2.0),
      Vec2::new(1024.0, 768.0),
    );
    for p in [
      Vec2::ZERO,
      Vec2::new(123.0, -45.0),
      Vec2::new(-300.0, 900.0),
    ] {
      assert!(
        close(view.screen_to_world(view.world_to_screen(p)), p),
        "{p}"
      );
    }
  }

  #[test]
  fn the_matrix_agrees_with_world_to_screen() {
    let camera = Camera2d {
      zoom: 1.3,
      ..Default::default()
    };
    let view = CameraView::compute(
      Vec2::new(20.0, 40.0),
      0.4,
      &camera,
      Vec2::new(2.0, 1.0),
      Vec2::new(200.0, 100.0),
    );
    let world = Vec2::new(35.0, 52.0);
    let ndc = view.view_proj.project_point3(world.extend(0.0)).truncate();
    let screen = view.rect_min + (Vec2::new(ndc.x, -ndc.y) + Vec2::ONE) / 2.0 * view.rect_size;
    assert!(close(screen, view.world_to_screen(world)));
  }

  #[test]
  fn degenerate_settings_stay_finite() {
    for zoom in [0.0, -1.0, f32::NAN, f32::INFINITY] {
      for projection in [
        Projection::ScreenUnits,
        Projection::Fit {
          width: 0.0,
          height: f32::NAN,
        },
      ] {
        let camera = Camera2d {
          zoom,
          projection,
          ..Default::default()
        };
        let view = CameraView::compute(
          Vec2::new(f32::NAN, 1.0),
          f32::NAN,
          &camera,
          Vec2::ZERO,
          Vec2::ZERO,
        );
        assert!(view.view_proj.is_finite(), "{zoom} {projection:?}");
        assert!(view.world_to_screen(Vec2::new(5.0, 5.0)).is_finite());
      }
    }
  }

  #[test]
  fn a_rotated_view_sees_a_wider_box() {
    let view = CameraView::compute(
      Vec2::ZERO,
      std::f32::consts::FRAC_PI_4,
      &Camera2d::default(),
      Vec2::ZERO,
      Vec2::new(100.0, 100.0),
    );
    assert!(view.bounds.right > 70.0, "{:?}", view.bounds);
    assert!(view.is_visible(Vec2::new(60.0, -1.0), Vec2::new(61.0, 1.0)));
    assert!(!view.is_visible(Vec2::new(80.0, 80.0), Vec2::new(90.0, 90.0)));
  }
}
