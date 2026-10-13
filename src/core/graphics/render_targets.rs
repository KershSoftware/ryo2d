//! Textures cameras render into and sprites show

use std::collections::HashMap;

use crate::core::ecs::Resource;

use super::RenderTarget;

/// The largest side a render target can have: WebGPU's default
/// `max_texture_dimension_2d`.
pub const MAX_TARGET_SIDE: u32 = 8192;

/// The render targets: textures a camera renders into
/// ([`CameraTarget::Texture`](super::CameraTarget::Texture)) and sprites
/// show ([`Renderable::render_target`](crate::core::ecs::Renderable::render_target)).
///
/// Reach it as `ctx.render_targets()`. It records what the game asks for;
/// the GPU textures are created, resized and freed at the next frame. A
/// scene switch or reset that clears the world removes every target.
///
/// ```ignore
/// let minimap = ctx.render_targets().create(128, 128);
/// ctx.world.spawn((
///   Transform::from_xy(0.0, 0.0),
///   Camera2d { target: CameraTarget::Texture(minimap), zoom: 0.25, ..Default::default() },
/// ));
/// ctx.world.spawn((Transform::from_xy(560.0, 60.0), Renderable::render_target(minimap)));
/// ```
#[derive(Resource, Debug, Default)]
pub struct RenderTargets {
  sizes: HashMap<RenderTarget, (u32, u32)>,
  /// The id the next target gets (ids are never reused)
  next: u32,
}

impl RenderTargets {
  /// A new `width`x`height` target. Each side is clamped to
  /// `1..=`[`MAX_TARGET_SIDE`] (with a warning when it was out of range).
  pub fn create(&mut self, width: u32, height: u32) -> RenderTarget {
    let target = RenderTarget(self.next);
    self.next = self.next.wrapping_add(1);
    self.sizes.insert(target, clamped(width, height));
    target
  }

  /// Resize `target` (sides clamped as in [`create`](Self::create)); `false`
  /// when there is no such target.
  pub fn resize(&mut self, target: RenderTarget, width: u32, height: u32) -> bool {
    match self.sizes.get_mut(&target) {
      Some(size) => {
        *size = clamped(width, height);
        true
      }
      None => false,
    }
  }

  /// Remove `target`; `false` when there was no such target. Cameras aiming
  /// at it draw nothing, and sprites showing it are not drawn.
  pub fn remove(&mut self, target: RenderTarget) -> bool {
    self.sizes.remove(&target).is_some()
  }

  /// The size of `target`, in pixels.
  pub fn size(&self, target: RenderTarget) -> Option<(u32, u32)> {
    self.sizes.get(&target).copied()
  }

  /// Whether `target` exists.
  pub fn contains(&self, target: RenderTarget) -> bool {
    self.sizes.contains_key(&target)
  }

  /// Every target and its size.
  pub(crate) fn iter(&self) -> impl Iterator<Item = (RenderTarget, (u32, u32))> + '_ {
    self.sizes.iter().map(|(&target, &size)| (target, size))
  }

  /// Remove every target.
  pub(crate) fn clear(&mut self) {
    self.sizes.clear();
  }
}

/// `width`x`height` with each side in `1..=MAX_TARGET_SIDE`, warning when
/// either was out of range.
fn clamped(width: u32, height: u32) -> (u32, u32) {
  let size = (
    width.clamp(1, MAX_TARGET_SIDE),
    height.clamp(1, MAX_TARGET_SIDE),
  );
  if size != (width, height) {
    crate::engine_warn!(
      module: "Render",
      "Render target size {width}x{height} is out of range; using {}x{}",
      size.0,
      size.1
    );
  }
  size
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn sizes_are_clamped_to_what_a_gpu_can_hold() {
    let mut targets = RenderTargets::default();
    let tiny = targets.create(0, 0);
    let huge = targets.create(100_000, 64);
    assert_eq!(targets.size(tiny), Some((1, 1)));
    assert_eq!(targets.size(huge), Some((8192, 64)));
  }

  #[test]
  fn a_removed_target_is_gone() {
    let mut targets = RenderTargets::default();
    let target = targets.create(32, 32);
    assert!(targets.resize(target, 64, 16));
    assert_eq!(targets.size(target), Some((64, 16)));
    assert!(targets.remove(target));
    assert!(!targets.contains(target) && targets.size(target).is_none());
    assert!(!targets.remove(target));
  }
}
