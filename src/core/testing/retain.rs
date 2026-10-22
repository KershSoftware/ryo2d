//! Two engines alike but for UI render retention, or for UI fragments,
//! compared pixel for pixel

use crate::core::engine::EngineBuilder;

use super::{HeadlessEngine, StepOutcome};

/// What the second engine of a pair lacks
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lacks {
  /// UI render retention
  Retention,
  /// The node cache, and so fragments: a full rebuild every frame
  Cache,
}

/// Two engines built alike but for UI render retention (on, off), or for UI
/// fragments (on, the node cache off), driven together and compared after
/// every frame: neither must ever show. For the retention and fragment
/// tests; panics on any difference.
#[doc(hidden)]
pub struct PixelPair {
  /// Retention on, or fragments on
  pub on: HeadlessEngine,
  /// Retention off, or the node cache off
  pub off: HeadlessEngine,
  lacks: Lacks,
  compared: usize,
  kept: usize,
  reused: usize,
  fragments_reused: usize,
  most_draws: u32,
}

impl PixelPair {
  /// The two engines `make` builds, retention on in one and off in the
  /// other; `None` without a GPU
  pub fn new(make: impl Fn() -> EngineBuilder) -> Option<Self> {
    Self::with(
      make().ui_retain(true),
      make().ui_retain(false),
      Lacks::Retention,
    )
  }

  /// The two engines `make` builds, retention on in both: fragments on in
  /// one, the node cache (and so fragments) off in the other, a full
  /// rebuild every frame; `None` without a GPU. The debug spot check keeps
  /// its default (on in debug builds) in both: in the engine with
  /// fragments it builds a clean node and captures a kept fragment afresh
  /// each frame, which must never differ from what was kept.
  pub fn fragments(make: impl Fn() -> EngineBuilder) -> Option<Self> {
    Self::with(
      make().ui_retain(true).ui_cache(true).ui_fragments(true),
      make().ui_retain(true).ui_cache(false),
      Lacks::Cache,
    )
  }

  fn with(on: EngineBuilder, off: EngineBuilder, lacks: Lacks) -> Option<Self> {
    Some(Self {
      on: HeadlessEngine::new_or_skip(on)?,
      off: HeadlessEngine::new_or_skip(off)?,
      lacks,
      compared: 0,
      kept: 0,
      reused: 0,
      fragments_reused: 0,
      most_draws: 0,
    })
  }

  /// Do `f` to both engines
  pub fn each(&mut self, mut f: impl FnMut(&mut HeadlessEngine)) {
    f(&mut self.on);
    f(&mut self.off);
  }

  /// Run `n` frames on both. After each, their frames must be the same
  /// pixels (on a difference both are saved as PNGs in the temp folder's
  /// `ryo2d-ui-retain` or `ryo2d-ui-fragments`). Without retention the
  /// second engine must have kept and reused nothing; without the node
  /// cache it must have replayed and copied nothing, and no fragment the
  /// first engine's spot check captured may differ from what it kept.
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
        let (folder, pair) = match self.lacks {
          Lacks::Retention => ("ryo2d-ui-retain", "retention on and off"),
          Lacks::Cache => ("ryo2d-ui-fragments", "fragments on and the cache off"),
        };
        let folder = std::env::temp_dir().join(folder);
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
          "{what}, frame {frame}: {pair} differ in {differ} pixels, the first at {first:?}{saved}"
        );
      }
      let walk = self.on.ui_cache_stats();
      match self.lacks {
        Lacks::Retention => {
          let off = self.off.ui_render_stats();
          assert!(
            !off.kept && off.texts_reused == 0 && off.runs_copied == 0,
            "{what}, frame {frame}: retention off kept or reused: {off:?}"
          );
        }
        Lacks::Cache => {
          let off = self.off.ui_cache_stats();
          let runs = self.off.ui_render_stats().runs_copied;
          assert!(
            off.replayed == 0 && off.reused == 0 && runs == 0,
            "{what}, frame {frame}: the cache off replayed or copied: {off:?}, {runs} runs"
          );
          assert_eq!(
            walk.spot_fragment_differed, 0,
            "{what}, frame {frame}: a kept fragment differs from its resolve"
          );
        }
      }
      let on = self.on.ui_render_stats();
      self.kept += usize::from(on.kept);
      self.reused += on.texts_reused as usize;
      self.fragments_reused += walk.reused;
      self.most_draws = self.most_draws.max(self.on.ui_draw_count());
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

  /// Fragments the first engine's node walk copied in place of a replay,
  /// so far (not those the spot check captured afresh and found the same)
  pub fn fragments_reused(&self) -> usize {
    self.fragments_reused
  }

  /// The most UI draws the retaining engine made in one frame, so far (a
  /// kept frame makes none)
  pub fn most_draws(&self) -> u32 {
    self.most_draws
  }
}
