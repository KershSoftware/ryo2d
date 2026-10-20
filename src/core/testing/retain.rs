//! Two engines alike but for UI render retention, compared pixel for pixel

use crate::core::engine::EngineBuilder;

use super::{HeadlessEngine, StepOutcome};

/// Two engines built alike but for UI render retention (on, off), driven
/// together and compared after every frame: retention must never show.
/// For the retention tests; panics on any difference.
#[doc(hidden)]
pub struct PixelPair {
  /// Retention on
  pub on: HeadlessEngine,
  /// Retention off
  pub off: HeadlessEngine,
  compared: usize,
  kept: usize,
  reused: usize,
}

impl PixelPair {
  /// The two engines `make` builds, retention on in one and off in the
  /// other; `None` without a GPU
  pub fn new(make: impl Fn() -> EngineBuilder) -> Option<Self> {
    Some(Self {
      on: HeadlessEngine::new_or_skip(make().ui_retain(true))?,
      off: HeadlessEngine::new_or_skip(make().ui_retain(false))?,
      compared: 0,
      kept: 0,
      reused: 0,
    })
  }

  /// Do `f` to both engines
  pub fn each(&mut self, mut f: impl FnMut(&mut HeadlessEngine)) {
    f(&mut self.on);
    f(&mut self.off);
  }

  /// Run `n` frames on both. After each, their frames must be the same
  /// pixels (on a difference both are saved as PNGs in the temp folder's
  /// `ryo2d-ui-retain`), and the engine without retention must have kept
  /// and reused nothing.
  pub fn frames(&mut self, n: u32, what: &str) {
    for _ in 0..n {
      let (on, off) = (self.on.step(), self.off.step());
      self.compared += 1;
      let frame = self.compared;
      match (on, off) {
        (Ok(StepOutcome::Running), Ok(StepOutcome::Running)) => {}
        other => panic!("{what}, frame {frame}: {other:?}"),
      }
      let (Ok(on), Ok(off)) = (self.on.frame(), self.off.frame()) else {
        panic!("{what}, frame {frame}: no frame presented");
      };
      if on.as_raw() != off.as_raw() {
        let differ = on
          .pixels()
          .zip(off.pixels())
          .filter(|(a, b)| a != b)
          .count();
        let first = on
          .enumerate_pixels()
          .zip(off.pixels())
          .find(|((_, _, a), b)| a != b)
          .map(|((x, y, _), _)| (x, y));
        let folder = std::env::temp_dir().join("ryo2d-ui-retain");
        let name: String = what
          .chars()
          .map(|c| if c.is_alphanumeric() { c } else { '-' })
          .collect();
        let saved = std::fs::create_dir_all(&folder).is_ok()
          && on
            .save(folder.join(format!("{name}-{frame}-on.png")))
            .is_ok()
          && off
            .save(folder.join(format!("{name}-{frame}-off.png")))
            .is_ok();
        let saved = if saved {
          format!(" (both saved in {})", folder.display())
        } else {
          String::new()
        };
        panic!(
          "{what}, frame {frame}: retention on and off differ in {differ} pixels, the first at {first:?}{saved}"
        );
      }
      let off = self.off.ui_render_stats();
      assert!(
        !off.kept && off.texts_reused == 0,
        "{what}, frame {frame}: retention off kept or reused: {off:?}"
      );
      let on = self.on.ui_render_stats();
      self.kept += usize::from(on.kept);
      self.reused += on.texts_reused as usize;
    }
  }

  /// Frames compared so far
  pub fn compared(&self) -> usize {
    self.compared
  }

  /// Frames the retaining engine kept its UI image in, so far
  pub fn kept(&self) -> usize {
    self.kept
  }

  /// Texts the retaining engine reused, so far
  pub fn reused(&self) -> usize {
    self.reused
  }
}
