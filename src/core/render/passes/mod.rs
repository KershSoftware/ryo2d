//! The built-in passes of [`RenderPipeline::default_2d`](super::RenderPipeline::default_2d)
//!
//! Each pass is thin: the GPU pipelines live in the subsystem renderers
//! (sprites, trails, lighting, post-process, raycaster, compositor) and a pass
//! calls into one of them with the frame's targets and scene data.

mod lighting;
mod output;
mod post_process;
mod raycast;
mod scene;
mod trails;

use super::RenderPass;

/// The built-in passes, in `default_2d` order.
pub(crate) fn built_in() -> [Box<dyn RenderPass>; 12] {
  [
    Box::new(scene::Upload),
    Box::new(scene::Clear),
    Box::new(raycast::Raycast),
    Box::new(scene::GpuSprites),
    Box::new(scene::CpuBlend),
    Box::new(trails::Trails),
    Box::new(lighting::Lighting),
    Box::new(trails::UnlitTrails),
    Box::new(post_process::PostProcess),
    Box::new(output::Ui),
    Box::new(output::Composite),
    Box::new(output::Capture),
  ]
}
