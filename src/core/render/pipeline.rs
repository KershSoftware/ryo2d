//! An ordered list of render passes

use super::{FrameInfo, RenderContext, RenderPass, passes};

/// A problem configuring a [`RenderPipeline`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RenderPipelineError {
  /// A pass with this id is already in the pipeline
  #[error("a render pass with id `{0}` is already in the pipeline")]
  DuplicatePass(String),
  /// No pass has this id
  #[error("no render pass with id `{0}`")]
  UnknownPass(String),
}

struct Entry {
  pass: Box<dyn RenderPass>,
  enabled: bool,
}

/// The frame's render passes, run in order.
///
/// Each enabled pass whose [`should_run`](RenderPass::should_run) holds for
/// the frame records its work, in list order, on one command encoder.
#[derive(Default)]
pub struct RenderPipeline {
  entries: Vec<Entry>,
}

impl RenderPipeline {
  /// An empty pipeline.
  pub fn new() -> Self {
    Self::default()
  }

  /// The built-in passes, in the order `default_2d` runs them: `upload`,
  /// `clear`, `raycast`, `gpu_sprites`, `cpu_blend`, `trails`, `lighting`,
  /// `unlit_trails`, `post_process`, `ui`, `composite`, `capture` (see
  /// [`pass_ids`](super::pass_ids)). Each takes part in a frame only when its
  /// content is there; `raycast` replaces the 2D scene passes.
  pub fn default_2d() -> Self {
    // Built-in ids are unique, so they go straight in
    let entries = passes::built_in()
      .into_iter()
      .map(|pass| Entry {
        pass,
        enabled: true,
      })
      .collect();
    Self { entries }
  }

  /// The built-in passes of the GPU world path, in the order `world_2d` runs
  /// them: `clear`, `raycast`, `world`, `lighting`, `unlit_world`,
  /// `post_process`, `ui`, `composite`, `capture` (see
  /// [`pass_ids`](super::pass_ids)).
  ///
  /// `world` draws GPU sprites and lit trails and lines together in z order;
  /// `unlit_world` draws the trails and lines that skip lighting. CPU-rendered
  /// entities are not drawn by this pipeline.
  pub fn world_2d() -> Self {
    let entries = passes::world_2d()
      .into_iter()
      .map(|pass| Entry {
        pass,
        enabled: true,
      })
      .collect();
    Self { entries }
  }

  /// Add a pass at the end.
  ///
  /// The last built-in passes are `composite` (copies the frame to the screen)
  /// and `capture`, so a pass pushed after them does not reach the window. To
  /// draw on the frame use [`insert_before`](Self::insert_before) with
  /// `pass_ids::COMPOSITE`.
  pub fn push(&mut self, pass: impl RenderPass + 'static) -> Result<(), RenderPipelineError> {
    self.insert_at(self.entries.len(), Box::new(pass))
  }

  /// Add a pass just before the pass `anchor`.
  pub fn insert_before(
    &mut self,
    anchor: &str,
    pass: impl RenderPass + 'static,
  ) -> Result<(), RenderPipelineError> {
    let index = self.index_of(anchor)?;
    self.insert_at(index, Box::new(pass))
  }

  /// Add a pass just after the pass `anchor`.
  pub fn insert_after(
    &mut self,
    anchor: &str,
    pass: impl RenderPass + 'static,
  ) -> Result<(), RenderPipelineError> {
    let index = self.index_of(anchor)? + 1;
    self.insert_at(index, Box::new(pass))
  }

  /// Take a pass out of the pipeline.
  pub fn remove(&mut self, id: &str) -> Option<Box<dyn RenderPass>> {
    let index = self.position(id)?;
    Some(self.entries.remove(index).pass)
  }

  /// Turn a pass on or off without moving it.
  pub fn set_enabled(&mut self, id: &str, enabled: bool) -> Result<(), RenderPipelineError> {
    let index = self.index_of(id)?;
    self.entries[index].enabled = enabled;
    Ok(())
  }

  /// Whether a pass is on, or `None` if there is no such pass.
  pub fn is_enabled(&self, id: &str) -> Option<bool> {
    self.position(id).map(|index| self.entries[index].enabled)
  }

  /// Whether the pipeline has a pass with this id.
  pub fn contains(&self, id: &str) -> bool {
    self.position(id).is_some()
  }

  /// Every pass id, in order.
  pub fn pass_ids(&self) -> impl Iterator<Item = &str> {
    self.entries.iter().map(|entry| entry.pass.id())
  }

  /// The ids of the passes that would run for `frame`, in order.
  pub fn passes_to_run(&self, frame: &FrameInfo) -> Vec<&str> {
    self
      .entries
      .iter()
      .filter(|entry| entry.enabled && entry.pass.should_run(frame))
      .map(|entry| entry.pass.id())
      .collect()
  }

  /// Record the frame: every enabled pass whose `should_run` holds, in order.
  pub(crate) fn execute(&mut self, ctx: &mut RenderContext<'_>) {
    let frame = *ctx.frame();
    for entry in &mut self.entries {
      if entry.enabled && entry.pass.should_run(&frame) {
        // Name the pass in GPU validation errors
        ctx.encoder.push_debug_group(entry.pass.id());
        entry.pass.execute(ctx);
        ctx.encoder.pop_debug_group();
      }
    }
  }

  fn position(&self, id: &str) -> Option<usize> {
    self.entries.iter().position(|entry| entry.pass.id() == id)
  }

  fn index_of(&self, id: &str) -> Result<usize, RenderPipelineError> {
    self
      .position(id)
      .ok_or_else(|| RenderPipelineError::UnknownPass(id.to_string()))
  }

  fn insert_at(
    &mut self,
    index: usize,
    pass: Box<dyn RenderPass>,
  ) -> Result<(), RenderPipelineError> {
    if self.contains(pass.id()) {
      return Err(RenderPipelineError::DuplicatePass(pass.id().to_string()));
    }
    self.entries.insert(
      index,
      Entry {
        pass,
        enabled: true,
      },
    );
    Ok(())
  }
}
