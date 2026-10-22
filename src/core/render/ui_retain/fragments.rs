//! A fragment run's instances, built once and copied in later frames while
//! the run is the same: its version, the environment, the glyph pages it
//! samples, its sprites' and icons' versions and its textures
//!
//! A run in one of the shader's scroll slots is built relative to the slot
//! and unculled, so its instances hold wherever the slot is drawn; a copy
//! takes the slot's index in its frame. A run built absolute (past the
//! shader's slots, on the pixel-art target, or with a draw whose own clip
//! does not fit relative to its slot) is never kept. A copy pins the glyph
//! pages it samples for the frame, and holds its texts in the text cache as
//! drawing them would, so the run's next version reuses them.

use super::texts::TextKey;
use super::{UiRenderEnv, animated, stamp_of, stamped};
use crate::core::graphics::{GlyphPage, TextureId};
use crate::core::hash::FastMap;
use crate::core::render::ui_renderer::{SpriteFix, TexNeed, UiInstance};
use crate::core::text::TextIcons;
use crate::core::ui::UiId;
use crate::core::ui::immediate::{FragmentRun, UiDraw};

/// Kept runs' buffers held for reuse when their runs go
const SPARE: usize = 64;

/// A fragment run's instances as built, to copy while the run is the same
#[derive(Debug)]
pub(crate) struct KeptRun {
  pub(crate) version: u64,
  /// Built under this environment
  pub(crate) env: UiRenderEnv,
  /// The scroll slot its instances are marked with: 0 when built in slot
  /// 0, else one of the shader's (built relative to it, unculled)
  pub(crate) slot: u8,
  pub(crate) instances: Vec<UiInstance>,
  pub(crate) needs: Vec<TexNeed>,
  /// Its sprites, `index` relative to its first instance
  pub(crate) sprites: Vec<SpriteFix>,
  /// The sprite textures its commands asked for, `command` relative to its
  /// first draw, in the order asked
  pub(crate) resident: Vec<(u32, TextureId)>,
  /// The glyph pages it samples and their generations when built
  pub(crate) pages: Vec<(GlyphPage, u64)>,
  /// Its commands with a stamp (a sprite, a text with icons): where in the
  /// run, and the stamp (`stamp_of`) when built
  pub(crate) stamps: Vec<(u32, u64)>,
  /// Its texts' keys in the text cache, in order: a copy holds them there
  /// (`UiTextCache::hold`), so the run's next version reuses them
  pub(crate) texts: Vec<TextKey>,
  /// A command reads the clock (a text with an effect): built every frame
  pub(crate) animated: bool,
}

impl KeptRun {
  fn new(env: UiRenderEnv) -> KeptRun {
    KeptRun {
      version: 0,
      env,
      slot: 0,
      instances: Vec::new(),
      needs: Vec::new(),
      sprites: Vec::new(),
      resident: Vec::new(),
      pages: Vec::new(),
      stamps: Vec::new(),
      texts: Vec::new(),
      animated: false,
    }
  }

  /// Empty, built in `env` (its buffers' allocations kept)
  fn reset(&mut self, env: UiRenderEnv) {
    self.version = 0;
    self.env = env;
    self.slot = 0;
    self.instances.clear();
    self.needs.clear();
    self.sprites.clear();
    self.resident.clear();
    self.pages.clear();
    self.stamps.clear();
    self.texts.clear();
    self.animated = false;
  }

  /// Note the stamps of `draws` (the run's commands), with `icons`
  /// resolving icon names, and whether one reads the clock
  pub(crate) fn note(&mut self, draws: &[UiDraw], icons: Option<&TextIcons>) {
    for (k, draw) in (0u32..).zip(draws) {
      self.animated |= animated(&draw.command);
      if stamped(&draw.command) {
        self.stamps.push((k, stamp_of(&draw.command, icons)));
      }
    }
  }
}

/// A kept run, and the last frame it was drawn in
#[derive(Debug)]
struct Entry {
  run: KeptRun,
  used: u64,
}

/// Every fragment run's kept instances, by its root's id. A run not drawn
/// in a frame is dropped, and so is everything when the environment
/// changes or retention is off.
#[derive(Debug, Default)]
pub(crate) struct UiRunCache {
  kept: FastMap<UiId, Entry>,
  spare: Vec<KeptRun>,
  env: Option<UiRenderEnv>,
  /// Keep and copy runs (retention is on)
  keep: bool,
  frame: u64,
  copied: u32,
  built: u32,
}

impl UiRunCache {
  /// Start a frame drawn in `env`. Runs built in another environment are
  /// dropped, and with `retain` off every run is, and none is kept.
  pub(crate) fn begin(&mut self, env: UiRenderEnv, retain: bool) {
    if !retain || self.env != Some(env) {
      self.clear();
    }
    self.env = Some(env);
    self.keep = retain;
    self.frame += 1;
    self.copied = 0;
    self.built = 0;
  }

  /// `run`'s kept instances, drawn now in scroll slot `slot` (0: none),
  /// when they are what building it gives: built at its version in this
  /// environment, in slot 0 if it is drawn there and in one of the shader's
  /// slots if it is drawn in one, no command of it reading the clock, and
  /// still `pages` giving each glyph page it samples the generation it had
  /// (an evicted page was cleared, and has another), `textures` holding
  /// each sprite texture it draws, and `stamps` giving each of its commands
  /// with a stamp (by its place in the run) the one it had. A run is copied
  /// once a frame: another with its id is built.
  pub(crate) fn get(
    &mut self,
    run: &FragmentRun,
    slot: u8,
    pages: impl Fn(GlyphPage) -> Option<u64>,
    textures: impl Fn(TextureId) -> bool,
    stamps: impl Fn(u32) -> u64,
  ) -> Option<&KeptRun> {
    let (frame, env) = (self.frame, self.env?);
    let entry = self.kept.get_mut(&run.id)?;
    let kept = &entry.run;
    let same = entry.used != frame
      && kept.version == run.version
      && kept.env == env
      && (kept.slot == 0) == (slot == 0)
      && !kept.animated
      && kept
        .pages
        .iter()
        .all(|&(page, generation)| pages(page) == Some(generation))
      && kept.resident.iter().all(|&(_, texture)| textures(texture))
      && kept.stamps.iter().all(|&(k, stamp)| stamps(k) == stamp);
    if !same {
      return None;
    }
    entry.used = frame;
    self.copied += 1;
    Some(&entry.run)
  }

  /// A run built this frame was not copied
  pub(crate) fn count_built(&mut self) {
    self.built += 1;
  }

  /// Buffers to keep a run built this frame in, empty (allocations reused)
  /// and in this frame's environment; `None` when runs are not kept
  pub(crate) fn spare(&mut self) -> Option<KeptRun> {
    let env = self.env.filter(|_| self.keep)?;
    let mut run = self.spare.pop().unwrap_or_else(|| KeptRun::new(env));
    run.reset(env);
    Some(run)
  }

  /// Keep `run`, built this frame, as `id`'s, in place of what was kept
  /// for it; not when one with its id was drawn this frame already
  pub(crate) fn store(&mut self, id: UiId, run: KeptRun) {
    let frame = self.frame;
    if !self.keep {
      recycle(&mut self.spare, run);
      return;
    }
    match self.kept.get_mut(&id) {
      Some(entry) if entry.used == frame => recycle(&mut self.spare, run),
      Some(entry) => {
        let old = std::mem::replace(&mut entry.run, run);
        entry.used = frame;
        recycle(&mut self.spare, old);
      }
      None => {
        self.kept.insert(id, Entry { run, used: frame });
      }
    }
  }

  /// Give back buffers `spare` gave and the run was not kept in
  pub(crate) fn recycle(&mut self, run: KeptRun) {
    recycle(&mut self.spare, run);
  }

  /// End the frame: runs not drawn in it are dropped
  pub(crate) fn end(&mut self) {
    let frame = self.frame;
    let spare = &mut self.spare;
    for (_, entry) in self.kept.extract_if(|_, entry| entry.used != frame) {
      recycle(spare, entry.run);
    }
  }

  /// Drop every kept run
  pub(crate) fn clear(&mut self) {
    let spare = &mut self.spare;
    for (_, entry) in self.kept.drain() {
      recycle(spare, entry.run);
    }
    self.env = None;
  }

  /// The frame's runs: those copied, then those built
  pub(crate) fn stats(&self) -> (u32, u32) {
    (self.copied, self.built)
  }

  /// The glyph pages the kept runs sample, with their generations when
  /// built (tests)
  #[cfg(test)]
  pub(crate) fn kept_pages(&self) -> Vec<(GlyphPage, u64)> {
    self
      .kept
      .values()
      .flat_map(|entry| entry.run.pages.iter().copied())
      .collect()
  }
}

/// Hold `run`'s buffers in `spare` for reuse, up to [`SPARE`] of them
fn recycle(spare: &mut Vec<KeptRun>, run: KeptRun) {
  if spare.len() < SPARE {
    spare.push(run);
  }
}
