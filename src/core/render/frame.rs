//! What a frame contains, decided before any pass runs

use crate::core::ecs::Transform;
use crate::core::graphics::{BatchCollector, SpriteGpuRenderer};
use crate::core::lighting::{Light2D, LightOccluder2D, LightingRenderer};
use crate::core::raycast::GpuRaycastRenderer;
use crate::core::shaders::{EffectsManager, OutputBuffer};
use crate::core::trail::TrailRenderer;

/// What this frame contains, decided before any render pass runs.
///
/// Passes read it in [`RenderPass::should_run`](super::RenderPass::should_run)
/// to decide whether they take part in the frame.
///
/// It describes the frame's content, not which passes run. `effects` and
/// `lighting` stay true when the game disabled the `post_process` or
/// `lighting` pass, and `lighting` can be true on raycast frames, where the
/// lighting pass does not run. To know what ran, use
/// [`RenderPipeline::passes_to_run`](super::RenderPipeline::passes_to_run).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameInfo {
  /// Game resolution width in pixels
  pub game_width: u32,
  /// Game resolution height in pixels
  pub game_height: u32,
  /// The GPU raycaster draws this frame instead of the 2D scene
  pub raycast: bool,
  /// GPU sprite batches were submitted this frame
  pub gpu_sprites: bool,
  /// Trails or lines have geometry this frame
  pub trails: bool,
  /// 2D lighting is on
  pub lighting: bool,
  /// Post-process effects are active
  pub effects: bool,
  /// The UI buffer has something to draw
  pub ui: bool,
  /// A frame capture was requested for this frame
  pub capture: bool,
}

/// Which texture holds the frame's latest image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FrameImage {
  /// The game texture (the 2D scene or the raycaster)
  Game,
  /// The lighting output
  Lit,
  /// A post-process ping-pong buffer
  PostProcess(OutputBuffer),
}

/// Lighting inputs for a frame (only built while lighting is enabled).
pub(crate) struct LightingFrame<'a> {
  pub(crate) renderer: &'a mut LightingRenderer,
  pub(crate) lights: &'a [(Transform, Light2D)],
  pub(crate) occluders: &'a [(Transform, LightOccluder2D)],
}

/// The engine subsystems and per-frame data the built-in passes draw from.
pub(crate) struct FrameScene<'a> {
  pub(crate) sprites: &'a SpriteGpuRenderer,
  pub(crate) batches: &'a BatchCollector,
  pub(crate) trails: Option<&'a TrailRenderer>,
  /// `Some` only while lighting is enabled
  pub(crate) lighting: Option<LightingFrame<'a>>,
  /// `Some` only while raycasting is enabled
  pub(crate) raycast: Option<&'a GpuRaycastRenderer>,
  pub(crate) effects: &'a EffectsManager,
  pub(crate) ui_buffer: &'a [u8],
  pub(crate) camera_pos: glam::Vec2,
  pub(crate) camera_zoom: f32,
  pub(crate) time: f32,
}
