//! What a frame contains, decided before any pass runs

use super::DrawList;
use super::batch::Batch2D;
use super::look::Look;
use crate::core::ecs::Entity;
use crate::core::graphics::{
  Camera2d, CameraClear, CameraTarget, CameraView, RenderLayers, RenderTarget, RenderTargets,
  SpriteGpuRenderer, Viewport,
};
use crate::core::lighting::LightingRenderer;
use crate::core::shaders::{EffectsManager, OutputBuffer};

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
  /// HDR-stage effects are active
  pub hdr_effects: bool,
  /// Display-stage effects are active
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
  /// A buffer of the HDR effect stage
  Hdr(OutputBuffer),
  /// A buffer of the display effect stage: `A` is the display image the
  /// `tonemap` pass writes
  Display(OutputBuffer),
  /// A window camera's own image (inside its camera passes)
  Camera(Entity),
}

/// Lighting inputs for a frame (only built while lighting is enabled).
pub(crate) struct LightingFrame<'a> {
  /// Each lit camera's light buffer and the compose pass
  pub(crate) renderer: &'a mut LightingRenderer,
}

/// The engine subsystems and per-frame data the built-in passes draw from.
pub(crate) struct FrameScene<'a> {
  /// The sprites' textures (atlas and render-target pages)
  pub(crate) sprites: &'a SpriteGpuRenderer,
  /// The world's draws, laid out per camera (`None` before the engine's
  /// renderers exist)
  pub(crate) batch: Option<&'a Batch2D>,
  /// The z-sorted drawables of the world passes
  pub(crate) draw_list: &'a DrawList,
  /// `Some` only while lighting is enabled
  pub(crate) lighting: Option<LightingFrame<'a>>,
  pub(crate) effects: &'a EffectsManager,
  /// The frame's look (the tonemap pass reads it)
  pub(crate) look: &'a Look,
  pub(crate) ui_buffer: &'a [u8],
  /// The cameras the camera passes run for, in render order
  pub(crate) cameras: &'a [CameraDraw],
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
  /// The render target it draws into (`None` for a window camera)
  pub(crate) target: Option<RenderTarget>,
  /// Draws straight into the scene image instead of an image of its own
  pub(crate) direct: bool,
  /// Draws into an image of its own, placed in its rectangle afterwards (a
  /// window camera that is not direct, or a texture camera with a partial
  /// viewport); otherwise into the scene image or its whole target
  pub(crate) own_image: bool,
  /// Its rectangle in the scene image or its target (all of the scene image
  /// when direct)
  pub(crate) rect: PixelRect,
  /// Lighting runs for it
  pub(crate) lit: bool,
  /// A UI camera: sees only the items drawn for it, and draws in
  /// `UI_FORMAT`
  pub(crate) ui: bool,
}

/// One camera to plan, as the world holds it.
pub(crate) type CameraSource<'a> = (
  Entity,
  &'a Camera2d,
  &'a CameraView,
  Option<&'a RenderLayers>,
);

/// Fill `out` with the cameras the frame draws, in render order: texture
/// cameras, then window cameras, each by `order` (ties by entity). Window
/// cameras draw on a scene image of `render_size`.
///
/// The first window camera draws straight into the scene image when it
/// covers the whole window and clears; every other window camera draws into
/// an image of its own, sized to its viewport. A texture camera draws into
/// its viewport's part of its render target: straight into the target when
/// the viewport covers it, otherwise into an image of its own placed there;
/// one whose target is missing from `targets` is left out and pushed onto
/// `targetless`. With `lighting`, every camera is lit.
pub(crate) fn plan_cameras<'a>(
  cameras: impl Iterator<Item = CameraSource<'a>>,
  render_size: (u32, u32),
  lighting: bool,
  targets: Option<&RenderTargets>,
  out: &mut Vec<CameraDraw>,
  targetless: &mut Vec<Entity>,
) {
  out.clear();
  targetless.clear();
  let mut ordered: Vec<(bool, i32, Entity, &Camera2d, &CameraView, RenderLayers)> = cameras
    .map(|(entity, camera, view, layers)| {
      let window = camera.target == CameraTarget::Window;
      let layers = layers.copied().unwrap_or_default();
      (window, camera.order, entity, camera, view, layers)
    })
    .collect();
  ordered.sort_by_key(|&(window, order, entity, ..)| (window, order, entity));
  let mut first_window = true;
  for (_, _, entity, camera, view, layers) in ordered {
    let (target, direct, own_image, rect) = match camera.target {
      CameraTarget::Texture(target) => {
        let Some(size) = targets.and_then(|targets| targets.size(target)) else {
          targetless.push(entity);
          continue;
        };
        let rect = PixelRect::of_viewport(&camera.viewport, size);
        (Some(target), false, rect.size() != size, rect)
      }
      CameraTarget::Window => {
        let direct = std::mem::take(&mut first_window)
          && camera.viewport == Viewport::FULL
          && camera.clear != CameraClear::None;
        let rect = PixelRect::of_viewport(&camera.viewport, render_size);
        (None, direct, !direct, rect)
      }
    };
    out.push(CameraDraw {
      entity,
      view: *view,
      layers,
      clear: camera.clear,
      target,
      direct,
      own_image,
      rect,
      lit: lighting,
      ui: false,
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
    let (mut out, mut targetless) = (Vec::new(), Vec::new());
    let cameras = [(b, &overlay, &view, None), (a, &full, &view, None)];
    plan_cameras(
      cameras.into_iter(),
      (64, 32),
      true,
      None,
      &mut out,
      &mut targetless,
    );
    let plan: Vec<_> = out.iter().map(|c| (c.entity, c.direct, c.lit)).collect();
    assert_eq!(plan, [(a, true, true), (b, false, true)]);

    // A camera before the full one keeps it from drawing straight in
    let cameras = [(a, &full, &view, None), (c, &half, &view, None)];
    plan_cameras(
      cameras.into_iter(),
      (64, 32),
      false,
      None,
      &mut out,
      &mut targetless,
    );
    let plan: Vec<_> = out
      .iter()
      .map(|c| (c.entity, c.direct, c.rect.width))
      .collect();
    assert_eq!(plan, [(c, false, 32), (a, false, 64)]);
  }

  #[test]
  fn texture_cameras_come_first_and_need_their_target() {
    let mut world = World::new();
    let [window, texture, gone] = [(); 3].map(|_| world.spawn_empty().id());
    let mut targets = RenderTargets::default();
    let target = targets.create(16, 8);
    let removed = targets.create(4, 4);
    targets.remove(removed);
    let view = CameraView::default();
    let main = Camera2d::default();
    let mirror = Camera2d {
      order: 5,
      target: CameraTarget::Texture(target),
      ..Camera2d::default()
    };
    let lost = Camera2d {
      target: CameraTarget::Texture(removed),
      ..Camera2d::default()
    };
    let (mut out, mut targetless) = (Vec::new(), Vec::new());
    let cameras = [
      (window, &main, &view, None),
      (texture, &mirror, &view, None),
      (gone, &lost, &view, None),
    ];
    plan_cameras(
      cameras.into_iter(),
      (64, 32),
      false,
      Some(&targets),
      &mut out,
      &mut targetless,
    );
    let plan: Vec<_> = out
      .iter()
      .map(|c| (c.entity, c.direct, c.rect.size()))
      .collect();
    assert_eq!(plan, [(texture, false, (16, 8)), (window, true, (64, 32))]);
    assert!(out.iter().all(|c| !c.own_image), "both draw straight in");
    assert_eq!(targetless, [gone]);

    // A partial viewport draws into an image of its own, placed in the target
    let half = Camera2d {
      viewport: Viewport::new(0.5, 0.0, 0.5, 1.0),
      ..mirror
    };
    let cameras = [(texture, &half, &view, None)];
    plan_cameras(
      cameras.into_iter(),
      (64, 32),
      false,
      Some(&targets),
      &mut out,
      &mut targetless,
    );
    let rect = out[0].rect;
    assert!(out[0].own_image);
    assert_eq!((rect.x, rect.y, rect.size()), (8, 0, (8, 8)));
  }
}
