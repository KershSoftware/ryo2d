//! Two engines alike but for one setting (UI render retention, or any two
//! configurations), compared pixel for pixel

use crate::core::engine::EngineBuilder;

use super::{HeadlessEngine, StepOutcome};

/// Two engines built alike but for one setting, driven together and
/// compared after every frame. From [`new`](Self::new): UI render retention
/// on and off, which must never show, and the engine without it must keep
/// and reuse nothing. From [`between`](Self::between): any two
/// configurations, `a` and `b`, compared by their pixels alone. For the
/// renderer's tests; panics on any difference.
#[doc(hidden)]
pub struct PixelPair {
  /// Retention on (`new`), or configuration `a` (`between`)
  pub on: HeadlessEngine,
  /// Retention off (`new`), or configuration `b` (`between`)
  pub off: HeadlessEngine,
  /// The pair is `new`'s: `off` must keep and reuse nothing
  retention: bool,
  compared: usize,
  kept: usize,
  reused: usize,
}

impl PixelPair {
  /// The two engines `make` builds, retention on in one and off in the
  /// other; `None` without a GPU
  pub fn new(make: impl Fn() -> EngineBuilder) -> Option<Self> {
    Self::of(make().ui_retain(true), make().ui_retain(false), true)
  }

  /// The two engines `make` builds, one changed by `a` and the other by
  /// `b`, compared by their pixels alone; `None` without a GPU
  pub fn between(
    make: impl Fn() -> EngineBuilder,
    a: impl Fn(EngineBuilder) -> EngineBuilder,
    b: impl Fn(EngineBuilder) -> EngineBuilder,
  ) -> Option<Self> {
    Self::of(a(make()), b(make()), false)
  }

  fn of(on: EngineBuilder, off: EngineBuilder, retention: bool) -> Option<Self> {
    Some(Self {
      on: HeadlessEngine::new_or_skip(on)?,
      off: HeadlessEngine::new_or_skip(off)?,
      retention,
      compared: 0,
      kept: 0,
      reused: 0,
    })
  }

  /// The engine of configuration `a` (`between`; retention on for `new`)
  pub fn a(&self) -> &HeadlessEngine {
    &self.on
  }

  /// The engine of configuration `b` (`between`; retention off for `new`)
  pub fn b(&self) -> &HeadlessEngine {
    &self.off
  }

  /// Do `f` to both engines
  pub fn each(&mut self, mut f: impl FnMut(&mut HeadlessEngine)) {
    f(&mut self.on);
    f(&mut self.off);
  }

  /// Run `n` frames on both. After each, their frames must be the same
  /// pixels (on a difference both are saved as PNGs in the temp folder's
  /// `ryo2d-ui-retain`), and for `new`'s pair the engine without retention
  /// must have kept and reused nothing.
  pub fn frames(&mut self, n: u32, what: &str) {
    // How the engines are named in the PNGs and the message
    let (first, second, both) = if self.retention {
      ("on", "off", "retention on and off")
    } else {
      ("a", "b", "a and b")
    };
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
        let first_at = on
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
            .save(folder.join(format!("{name}-{frame}-{first}.png")))
            .is_ok()
          && off
            .save(folder.join(format!("{name}-{frame}-{second}.png")))
            .is_ok();
        let saved = if saved {
          format!(" (both saved in {})", folder.display())
        } else {
          String::new()
        };
        panic!(
          "{what}, frame {frame}: {both} differ in {differ} pixels, the first at {first_at:?}{saved}"
        );
      }
      if self.retention {
        let off = self.off.ui_render_stats();
        assert!(
          !off.kept && off.texts_reused == 0,
          "{what}, frame {frame}: retention off kept or reused: {off:?}"
        );
      }
      let on = self.on.ui_render_stats();
      self.kept += usize::from(on.kept);
      self.reused += on.texts_reused as usize;
    }
  }

  /// Frames compared so far
  pub fn compared(&self) -> usize {
    self.compared
  }

  /// Frames the first engine (retention on, or `a`) kept its UI image in,
  /// so far
  pub fn kept(&self) -> usize {
    self.kept
  }

  /// Texts the first engine (retention on, or `a`) reused, so far
  pub fn reused(&self) -> usize {
    self.reused
  }
}
