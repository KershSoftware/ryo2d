//! The built-in passes of [`RenderPipeline::default_2d`](super::RenderPipeline::default_2d)
//!
//! Each pass is thin: the GPU pipelines live in the subsystem renderers
//! (sprites, trails, lighting, post-process, compositor) and a pass
//! calls into one of them with the frame's targets and scene data.

mod lighting;
mod output;
mod post_process;
mod scene;
mod transition;
mod world;

use super::RenderPass;

/// The built-in passes, in `default_2d` order.
pub(crate) fn built_in() -> [Box<dyn RenderPass>; 9] {
  [
    Box::new(scene::Clear),
    Box::new(world::World),
    Box::new(lighting::Lighting),
    Box::new(world::UnlitWorld),
    Box::new(transition::TransitionPass::default()),
    Box::new(post_process::PostProcess),
    Box::new(output::Ui),
    Box::new(output::Composite),
    Box::new(output::Capture),
  ]
}
