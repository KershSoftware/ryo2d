//! Debug shapes drawn over the world for a step or a frame.
//!
//! Shapes are kept in world space and built per camera, so each is its
//! width in that camera's screen pixels at any zoom.

use std::ops::Range;

use glam::Vec2;

use super::batch::{LineLook, PolyPoint, PolyStyle, Triangles, push_polyline};
use super::{CameraDraw, DrawList};
use crate::core::ecs::{ResMut, Resource};
use crate::core::graphics::RenderLayers;
use crate::core::render::batch::{BlendMode, Filter};
use crate::core::trail::{LineCap, LineJoin};
use crate::core::ui::Color;

/// What a debug shape is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShapeKind {
  /// Points: the two ends
  Line,
  /// Points: every point
  Polyline,
  /// Points: the centre and the size
  Rect,
  /// Points: the centre and (radius, 0)
  Circle,
  /// Points: the tail and the head
  Arrow,
  /// Points: the centre and (size, 0)
  Cross,
}

/// One debug shape: its points in its list's buffer, colour, width in
/// screen pixels and the layers of the cameras that draw it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DebugShape {
  pub(crate) kind: ShapeKind,
  pub(crate) points: Range<usize>,
  pub(crate) color: Color,
  pub(crate) width: f32,
  pub(crate) layers: RenderLayers,
}

/// Shapes over a point buffer of their own, both reused.
#[derive(Default)]
struct ShapeList {
  shapes: Vec<DebugShape>,
  points: Vec<Vec2>,
}

impl ShapeList {
  fn clear(&mut self) {
    self.shapes.clear();
    self.points.clear();
  }
}

/// Lines, rectangles, circles, arrows and crosses drawn over the world, in
/// world space: `ctx.debug_draw()`, or `ResMut<DebugDraw>` in a system.
///
/// Shapes added during a fixed step (`Scene::update`, fixed systems) are
/// drawn until the next fixed step begins, so a frame running several steps
/// draws the last one's and a frame without a step draws the previous
/// one's; shapes added anywhere else are drawn by the next frame only. They
/// are unlit, drawn after everything else in the world, one screen pixel
/// wide by default:
///
/// ```ignore
/// let mut debug = ctx.debug_draw();
/// debug.line(start, end, Color::RED);
/// debug.width(3.0).circle(target, 20.0, Color::GREEN);
/// ```
#[derive(Resource, Default)]
pub struct DebugDraw {
  fixed: ShapeList,
  frame: ShapeList,
  in_fixed_step: bool,
}

/// A [`DebugDraw`] with a width or layers of its own (from
/// [`DebugDraw::width`] or [`DebugDraw::layers`]).
pub struct DebugPen<'a> {
  debug: &'a mut DebugDraw,
  width: f32,
  layers: RenderLayers,
}

impl DebugDraw {
  /// A pen drawing `pixels` screen pixels wide.
  pub fn width(&mut self, pixels: f32) -> DebugPen<'_> {
    self.pen().width(pixels)
  }

  /// A pen drawing for the cameras on `layers`.
  pub fn layers(&mut self, layers: RenderLayers) -> DebugPen<'_> {
    self.pen().layers(layers)
  }

  /// A line from `a` to `b`.
  pub fn line(&mut self, a: Vec2, b: Vec2, color: Color) {
    self.pen().line(a, b, color);
  }

  /// A line through `points`.
  pub fn polyline(&mut self, points: &[Vec2], color: Color) {
    self.pen().polyline(points, color);
  }

  /// The outline of the rectangle of `size` centred on `center`.
  pub fn rect(&mut self, center: Vec2, size: Vec2, color: Color) {
    self.pen().rect(center, size, color);
  }

  /// The outline of the circle of `radius` round `center`.
  pub fn circle(&mut self, center: Vec2, radius: f32, color: Color) {
    self.pen().circle(center, radius, color);
  }

  /// An arrow from `from` to its head at `to`.
  pub fn arrow(&mut self, from: Vec2, to: Vec2, color: Color) {
    self.pen().arrow(from, to, color);
  }

  /// A `+` of `size` across, centred on `at`.
  pub fn cross(&mut self, at: Vec2, size: f32, color: Color) {
    self.pen().cross(at, size, color);
  }

  fn pen(&mut self) -> DebugPen<'_> {
    DebugPen {
      debug: self,
      width: 1.0,
      layers: RenderLayers::default(),
    }
  }

  fn push(
    &mut self,
    kind: ShapeKind,
    points: &[Vec2],
    color: Color,
    width: f32,
    layers: RenderLayers,
  ) {
    let list = if self.in_fixed_step {
      &mut self.fixed
    } else {
      &mut self.frame
    };
    let start = list.points.len();
    list.points.extend_from_slice(points);
    list.shapes.push(DebugShape {
      kind,
      points: start..list.points.len(),
      color,
      width,
      layers,
    });
  }

  /// A fixed step begins: the last step's shapes go, and shapes from now
  /// on belong to this step.
  pub(crate) fn begin_fixed_step(&mut self) {
    self.fixed.clear();
    self.in_fixed_step = true;
  }

  /// The fixed step ended: shapes from now on last one frame.
  pub(crate) fn end_fixed_step(&mut self) {
    self.in_fixed_step = false;
  }

  /// The frame was drawn: its shapes go (the last step's stay).
  pub(crate) fn end_frame(&mut self) {
    self.frame.clear();
  }

  /// How many shapes would be drawn now.
  #[cfg(test)]
  pub(crate) fn len(&self) -> usize {
    self.fixed.shapes.len() + self.frame.shapes.len()
  }

  /// Every shape with its points.
  pub(crate) fn each(&self) -> impl Iterator<Item = (&DebugShape, &[Vec2])> {
    [&self.fixed, &self.frame].into_iter().flat_map(|list| {
      list
        .shapes
        .iter()
        .map(|shape| (shape, list.points.get(shape.points.clone()).unwrap_or(&[])))
    })
  }

  /// Every shape.
  #[cfg(test)]
  pub(crate) fn shapes(&self) -> impl Iterator<Item = &DebugShape> {
    self.each().map(|(shape, _)| shape)
  }

  /// Build the shapes `camera` sees into `triangles` and add them to `list`
  /// for that camera alone, `scratch` holding the points.
  pub(crate) fn push_for_camera(
    &self,
    camera: &CameraDraw,
    list: &mut DrawList,
    triangles: &mut Triangles,
    scratch: &mut Vec<PolyPoint>,
  ) {
    let view = &camera.view;
    let scale = view.scale;
    if !(scale.is_finite() && scale > 0.0) {
      return;
    }
    for (shape, points) in self.each() {
      let drawable = shape.width > 0.0;
      if !shape.layers.intersects(&camera.layers) || !drawable {
        continue;
      }
      let half_width = shape.width / 2.0 / scale;
      // An odd width covers whole pixels when centred on a pixel's middle
      let odd = (shape.width.round() as i64) % 2 == 1;
      let centre = |world: Vec2| {
        if odd {
          view.screen_to_world(view.world_to_screen(world).floor() + 0.5)
        } else {
          world
        }
      };
      let look = LineLook {
        lit: false,
        z: i32::MAX,
        texture: None,
        blend: BlendMode::Alpha,
        filter: Filter::Default,
        layers: shape.layers,
        only_camera: Some(camera.entity),
      };
      let mut add = |path: &[Vec2], closed: bool| {
        scratch.clear();
        let last = path.len().saturating_sub(1).max(1) as f32;
        scratch.extend(path.iter().enumerate().map(|(i, &pos)| PolyPoint {
          pos,
          half_width,
          color: super::batch::line_color(shape.color),
          u: i as f32 / last,
        }));
        let style = PolyStyle {
          join: LineJoin::Miter,
          cap: LineCap::Butt,
          closed,
          // Solid, no effect
          params: [0.0; 4],
        };
        push_polyline(list, triangles, scratch, &style, look);
      };
      match (shape.kind, points) {
        (ShapeKind::Line, &[a, b]) => add(&[centre(a), centre(b)], false),
        (ShapeKind::Polyline, points) => add(points, false),
        (ShapeKind::Rect, &[middle, size]) => {
          let half = size.abs() / 2.0;
          let corners = [
            middle - half,
            middle + Vec2::new(half.x, -half.y),
            middle + half,
            middle + Vec2::new(-half.x, half.y),
          ];
          // Corners move inward onto pixel middles, so the outline stays
          // inside the rectangle
          let inside = |corner: Vec2| {
            if !odd {
              return corner;
            }
            let (at, mid) = (view.world_to_screen(corner), view.world_to_screen(middle));
            let snap = |c: f32, m: f32| {
              if c <= m {
                c.floor() + 0.5
              } else {
                c.ceil() - 0.5
              }
            };
            view.screen_to_world(Vec2::new(snap(at.x, mid.x), snap(at.y, mid.y)))
          };
          add(&corners.map(inside), true);
        }
        (ShapeKind::Circle, &[middle, radius]) => {
          let radius = radius.x.abs();
          let segments = (radius * scale / 2.0).clamp(12.0, 64.0) as usize;
          let mut ring = [Vec2::ZERO; 64];
          for (i, point) in ring.iter_mut().take(segments).enumerate() {
            let angle = std::f32::consts::TAU * i as f32 / segments as f32;
            *point = middle + Vec2::from_angle(angle) * radius;
          }
          add(&ring[..segments], true);
        }
        (ShapeKind::Arrow, &[from, to]) => {
          let (from, to) = (centre(from), centre(to));
          add(&[from, to], false);
          // A head of two 6-pixel barbs, at most a third of the shaft
          let shaft = to - from;
          let length = shaft.length();
          if length > 0.0 {
            let barb = (6.0 / scale).min(length / 3.0);
            let back = -shaft / length * barb;
            let left = to + Vec2::from_angle(0.45).rotate(back);
            let right = to + Vec2::from_angle(-0.45).rotate(back);
            add(&[left, to, right], false);
          }
        }
        (ShapeKind::Cross, &[at, size]) => {
          let arm = size.x.abs() / 2.0;
          add(
            &[centre(at - Vec2::X * arm), centre(at + Vec2::X * arm)],
            false,
          );
          add(
            &[centre(at - Vec2::Y * arm), centre(at + Vec2::Y * arm)],
            false,
          );
        }
        _ => {}
      }
    }
  }
}

/// `FixedSet::Snapshot`, first: the step's shapes replace the last step's.
pub(crate) fn begin_debug_fixed_step(mut debug: ResMut<DebugDraw>) {
  debug.begin_fixed_step();
}

/// `FixedSet::Trails`, last: shapes added after the step last one frame.
pub(crate) fn end_debug_fixed_step(mut debug: ResMut<DebugDraw>) {
  debug.end_fixed_step();
}

impl DebugPen<'_> {
  /// Draw `pixels` screen pixels wide.
  pub fn width(mut self, pixels: f32) -> Self {
    self.width = pixels;
    self
  }

  /// Draw for the cameras on `layers`.
  pub fn layers(mut self, layers: RenderLayers) -> Self {
    self.layers = layers;
    self
  }

  /// A line from `a` to `b`.
  pub fn line(&mut self, a: Vec2, b: Vec2, color: Color) {
    self.push(ShapeKind::Line, &[a, b], color);
  }

  /// A line through `points`.
  pub fn polyline(&mut self, points: &[Vec2], color: Color) {
    self.push(ShapeKind::Polyline, points, color);
  }

  /// The outline of the rectangle of `size` centred on `center`.
  pub fn rect(&mut self, center: Vec2, size: Vec2, color: Color) {
    self.push(ShapeKind::Rect, &[center, size], color);
  }

  /// The outline of the circle of `radius` round `center`.
  pub fn circle(&mut self, center: Vec2, radius: f32, color: Color) {
    self.push(ShapeKind::Circle, &[center, Vec2::new(radius, 0.0)], color);
  }

  /// An arrow from `from` to its head at `to`.
  pub fn arrow(&mut self, from: Vec2, to: Vec2, color: Color) {
    self.push(ShapeKind::Arrow, &[from, to], color);
  }

  /// A `+` of `size` across, centred on `at`.
  pub fn cross(&mut self, at: Vec2, size: f32, color: Color) {
    self.push(ShapeKind::Cross, &[at, Vec2::new(size, 0.0)], color);
  }

  fn push(&mut self, kind: ShapeKind, points: &[Vec2], color: Color) {
    let (width, layers) = (self.width, self.layers);
    self.debug.push(kind, points, color, width, layers);
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn fixed_step_shapes_last_until_the_next_step() {
    let mut debug = DebugDraw::default();
    debug.begin_fixed_step();
    debug.line(Vec2::ZERO, Vec2::X, Color::WHITE);
    debug.end_fixed_step();
    assert_eq!(debug.len(), 1);
    debug.end_frame();
    assert_eq!(debug.len(), 1, "a frame without a step still draws it");
    debug.begin_fixed_step();
    assert_eq!(debug.len(), 0, "the next step starts afresh");
  }

  #[test]
  fn other_shapes_last_one_frame() {
    let mut debug = DebugDraw::default();
    debug.circle(Vec2::ZERO, 4.0, Color::WHITE);
    assert_eq!(debug.len(), 1);
    debug.end_frame();
    assert_eq!(debug.len(), 0);
  }

  #[test]
  fn a_pen_sets_width_and_layers() {
    let mut debug = DebugDraw::default();
    debug
      .width(3.0)
      .layers(RenderLayers::layer(2))
      .rect(Vec2::ZERO, Vec2::ONE, Color::WHITE);
    let shape = debug.shapes().next().unwrap();
    assert_eq!((shape.width, shape.layers), (3.0, RenderLayers::layer(2)));
  }
}
