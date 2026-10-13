//! What a frame contains, decided before any pass runs

use super::DrawList;
use crate::core::ecs::{Entity, GlobalTransform, Vec2};
use crate::core::graphics::camera_gpu::CameraUniforms;
use crate::core::graphics::{
  Camera2d, CameraClear, CameraTarget, CameraView, RenderLayers, SpriteGpuRenderer, Viewport,
};
use crate::core::lighting::{Light2D, LightOccluder2D, LightingRenderer};
use crate::core::shaders::{EffectsManager, OutputBuffer};
use crate::core::trail::TrailRenderer;

/// What this frame contains, decided before any render pass runs.
///
/// Passes read it in [`RenderPass::should_run`](super::RenderPass::should_run)
/// to decide whether they take part in the frame.
///
/// It describes the frame's content, not which passes run. `effects` and
/// `lighting` stay true when the game disabled the `post_process` or
/// `lighting` pass. To know what ran, use
/// [`RenderPipeline::passes_to_run`](super::RenderPipeline::passes_to_run).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FrameInfo {
  /// Width of screen space (logical pixels, or the pixel-art target)
  pub screen_width: f32,
  /// Height of screen space
  pub screen_height: f32,
  /// Width the scene renders at, in pixels
  pub render_width: u32,
  /// Height the scene renders at, in pixels
  pub render_height: u32,
  /// The draw list has lit items (entities, sprites, trails or lines drawn
  /// before lighting)
  pub world: bool,
  /// 2D lighting is on
  pub lighting: bool,
  /// Post-process effects are active
  pub effects: bool,
  /// The UI buffer has something to draw
  pub ui: bool,
  /// A scene transition's fade overlay is visible
  pub transition: bool,
  /// A frame capture was requested for this frame
  pub capture: bool,
}

/// Fade overlay opacity above which a scene transition is drawn.
pub(crate) const TRANSITION_VISIBLE: f32 = 0.001;

/// Which texture holds the frame's latest image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FrameImage {
  /// The game texture (the 2D scene)
  Game,
  /// The lighting output
  Lit,
  /// A post-process ping-pong buffer
  PostProcess(OutputBuffer),
  /// A window camera's own image (inside its camera passes)
  Camera(Entity),
}

/// Lighting inputs for a frame (only built while lighting is enabled).
pub(crate) struct LightingFrame<'a> {
  pub(crate) renderer: &'a mut LightingRenderer,
  /// Each light's world position
  pub(crate) lights: &'a [(Vec2, Light2D)],
  /// Each occluder where it is drawn, borrowed from the world
  pub(crate) occluders: &'a [(GlobalTransform, &'a LightOccluder2D)],
}

/// The engine subsystems and per-frame data the built-in passes draw from.
pub(crate) struct FrameScene<'a> {
  pub(crate) sprites: &'a SpriteGpuRenderer,
  /// The z-sorted drawables of the world passes
  pub(crate) draw_list: &'a DrawList,
  pub(crate) trails: Option<&'a TrailRenderer>,
  /// `Some` only while lighting is enabled
  pub(crate) lighting: Option<LightingFrame<'a>>,
  pub(crate) effects: &'a EffectsManager,
  pub(crate) ui_buffer: &'a [u8],
  /// The cameras the camera passes run for, in render order, and their
  /// uniforms
  pub(crate) cameras: &'a [CameraDraw],
  pub(crate) camera_uniforms: &'a CameraUniforms,
  pub(crate) time: f32,
  /// The scene transition's fade overlay: colour (RGB bytes) and opacity.
  /// Opacity 0 means no fade.
  pub(crate) overlay: ([u8; 3], f32),
}

/// A rectangle of whole pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PixelRect {
  pub(crate) x: u32,
  pub(crate) y: u32,
  pub(crate) width: u32,
  pub(crate) height: u32,
}

impl PixelRect {
  /// The pixels `viewport` covers on an image of `size`: edges rounded to
  /// whole pixels (so viewports that meet share an edge), at least one pixel,
  /// inside the image.
  pub(crate) fn of_viewport(viewport: &Viewport, size: (u32, u32)) -> Self {
    let edges = |min: f32, max: f32, side: u32| {
      let side = side.max(1);
      let pixel = |at: f32| {
        let at = if at.is_finite() { at } else { 0.0 };
        (at.clamp(0.0, 1.0) * side as f32).round() as u32
      };
      let start = pixel(min).min(side - 1);
      let end = pixel(max).clamp(start + 1, side);
      (start, end - start)
    };
    let (x, width) = edges(viewport.min.x, viewport.max.x, size.0);
    let (y, height) = edges(viewport.min.y, viewport.max.y, size.1);
    Self {
      x,
      y,
      width,
      height,
    }
  }

  pub(crate) fn size(&self) -> (u32, u32) {
    (self.width, self.height)
  }
}

/// One camera's part in the frame: what it sees and where it draws.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CameraDraw {
  pub(crate) entity: Entity,
  pub(crate) view: CameraView,
  /// Drawables on none of these layers are not drawn by this camera
  pub(crate) layers: RenderLayers,
  pub(crate) clear: CameraClear,
  /// Draws straight into the scene image instead of an image of its own
  pub(crate) direct: bool,
  /// Its image's pixels in the scene image (all of it when direct)
  pub(crate) rect: PixelRect,
  /// Lighting runs for it
  pub(crate) lit: bool,
}

/// One camera to plan, as the world holds it.
pub(crate) type CameraSource<'a> = (
  Entity,
  &'a Camera2d,
  &'a CameraView,
  Option<&'a RenderLayers>,
);

/// Fill `out` with the cameras the frame draws, in render order (by
/// `order`, ties by entity), on a scene image of `render_size`.
///
/// The first window camera draws straight into the scene image when it
/// covers the whole window and clears; every other camera draws into an
/// image of its own, sized to its viewport. With `lighting`, every camera is
/// lit. Texture cameras are not drawn.
pub(crate) fn plan_cameras<'a>(
  cameras: impl Iterator<Item = CameraSource<'a>>,
  render_size: (u32, u32),
  lighting: bool,
  out: &mut Vec<CameraDraw>,
) {
  out.clear();
  let mut ordered: Vec<(i32, Entity, &Camera2d, &CameraView, RenderLayers)> = cameras
    .filter(|(_, camera, ..)| camera.target == CameraTarget::Window)
    .map(|(entity, camera, view, layers)| {
      let layers = layers.copied().unwrap_or_default();
      (camera.order, entity, camera, view, layers)
    })
    .collect();
  ordered.sort_by_key(|&(order, entity, ..)| (order, entity));
  for (index, (_, entity, camera, view, layers)) in ordered.into_iter().enumerate() {
    let direct =
      index == 0 && camera.viewport == Viewport::FULL && camera.clear != CameraClear::None;
    out.push(CameraDraw {
      entity,
      view: *view,
      layers,
      clear: camera.clear,
      direct,
      rect: PixelRect::of_viewport(&camera.viewport, render_size),
      lit: lighting,
    });
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::World;

  #[test]
  fn viewports_cover_whole_pixels_that_meet() {
    let left = PixelRect::of_viewport(&Viewport::new(0.0, 0.0, 1.0 / 3.0, 1.0), (100, 50));
    let right = PixelRect::of_viewport(&Viewport::new(1.0 / 3.0, 0.0, 2.0 / 3.0, 1.0), (100, 50));
    assert_eq!((left.x, left.width), (0, 33));
    assert_eq!((right.x, right.width), (33, 67));
    assert_eq!(left.height, 50);
    let empty = PixelRect::of_viewport(&Viewport::new(1.0, 1.0, 0.0, 0.0), (100, 50));
    assert_eq!(
      empty,
      PixelRect {
        x: 99,
        y: 49,
        width: 1,
        height: 1
      }
    );
  }

  #[test]
  fn the_first_full_clearing_window_camera_draws_straight_into_the_scene() {
    let mut world = World::new();
    let [a, b, c] = [(); 3].map(|_| world.spawn_empty().id());
    let view = CameraView::default();
    let full = Camera2d::default();
    let overlay = Camera2d {
      order: 1,
      clear: CameraClear::None,
      ..Camera2d::default()
    };
    let half = Camera2d {
      order: -1,
      viewport: Viewport::new(0.0, 0.0, 0.5, 1.0),
      ..Camera2d::default()
    };
    let mut out = Vec::new();
    let cameras = [(b, &overlay, &view, None), (a, &full, &view, None)];
    plan_cameras(cameras.into_iter(), (64, 32), true, &mut out);
    let plan: Vec<_> = out.iter().map(|c| (c.entity, c.direct, c.lit)).collect();
    assert_eq!(plan, [(a, true, true), (b, false, true)]);

    // A camera before the full one keeps it from drawing straight in
    let cameras = [(a, &full, &view, None), (c, &half, &view, None)];
    plan_cameras(cameras.into_iter(), (64, 32), false, &mut out);
    let plan: Vec<_> = out
      .iter()
      .map(|c| (c.entity, c.direct, c.rect.width))
      .collect();
    assert_eq!(plan, [(c, false, 32), (a, false, 64)]);
  }
}
