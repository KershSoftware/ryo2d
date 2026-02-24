//! Shared linear interpolation trait.
//!
//! The [`Lerp`] trait provides a generic interface for linearly interpolating
//! between two values. It is used by both the ECS tween system
//! ([`super::Tween`]) and the UI tween system ([`crate::core::ui::UiTween`]).

use glam::Vec2;

use crate::core::ui::{Color, Rect};

/// Trait for types that can be linearly interpolated.
///
/// Implementors must also satisfy `Send + Sync + 'static` so that
/// values can be stored inside ECS components.
pub trait Lerp: Clone + Send + Sync + 'static {
  /// Linearly interpolate between two values.
  ///
  /// When `t = 0.0`, returns `start`.
  /// When `t = 1.0`, returns `end`.
  fn lerp(start: &Self, end: &Self, t: f32) -> Self;
}

impl Lerp for f32 {
  fn lerp(start: &Self, end: &Self, t: f32) -> Self {
    start + (end - start) * t
  }
}

impl Lerp for (f32, f32) {
  fn lerp(start: &Self, end: &Self, t: f32) -> Self {
    (
      f32::lerp(&start.0, &end.0, t),
      f32::lerp(&start.1, &end.1, t),
    )
  }
}

impl Lerp for Color {
  fn lerp(start: &Self, end: &Self, t: f32) -> Self {
    Color::new(
      f32::lerp(&(start.r as f32), &(end.r as f32), t).round() as u8,
      f32::lerp(&(start.g as f32), &(end.g as f32), t).round() as u8,
      f32::lerp(&(start.b as f32), &(end.b as f32), t).round() as u8,
      f32::lerp(&(start.a as f32), &(end.a as f32), t).round() as u8,
    )
  }
}

impl Lerp for Rect {
  fn lerp(start: &Self, end: &Self, t: f32) -> Self {
    Rect::new(
      f32::lerp(&start.x, &end.x, t),
      f32::lerp(&start.y, &end.y, t),
      f32::lerp(&start.width, &end.width, t),
      f32::lerp(&start.height, &end.height, t),
    )
  }
}

impl Lerp for Vec2 {
  fn lerp(start: &Self, end: &Self, t: f32) -> Self {
    Vec2::new(
      f32::lerp(&start.x, &end.x, t),
      f32::lerp(&start.y, &end.y, t),
    )
  }
}
