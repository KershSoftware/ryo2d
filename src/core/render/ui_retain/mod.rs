//! UI render retention: a frame whose UI draws exactly what the UI image
//! holds keeps the image, building, uploading and drawing no UI (see the
//! module notes, "UI render retention")

use std::hash::Hasher;
use std::ops::Range;
use std::sync::Arc;

use crate::core::hash::FastHasher;
use crate::core::render::UiSpace;
use crate::core::text::{Icon, TextIcons};
use crate::core::ui::Rect;
use crate::core::ui::immediate::{FragmentRun, ScrollSlot, UiCommand, UiCommands, UiDraw};

#[cfg(test)]
mod gpu_tests;
#[cfg(test)]
mod tests;
mod texts;

pub(crate) use texts::{TextSink, UiTextCache};

/// What the UI's render step did in a frame
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UiRenderStats {
  /// The UI image was kept from the last frame: no UI was built, uploaded
  /// or drawn
  pub kept: bool,
  /// Texts whose glyphs were built
  pub texts_built: u32,
  /// Texts drawn from glyphs built in an earlier frame
  pub texts_reused: u32,
  /// Glyph pages cleared to make room since the renderer started
  pub glyph_evictions: u64,
}

/// What a frame's UI pixels depend on besides its commands
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct UiRenderEnv {
  pub(crate) space: UiSpace,
  /// Rects snap to whole pixels of the pixel-art target
  pub(crate) on_target: bool,
  /// Text is drawn (the game kept the text resources)
  pub(crate) text: bool,
  /// The text engine's font generation (faces and render modes)
  pub(crate) fonts: u64,
  /// A hash of the bitmap faces' sheets (serial and generation): a sheet
  /// replaced in place changes the glyph cells without a new font generation
  pub(crate) bitmaps: u64,
}

/// Whether the UI image can be shown again as it is, and what it holds
#[derive(Debug, Default)]
pub(crate) struct UiRetain {
  /// The draws outside fragment runs the UI image holds or is being drawn
  /// with: copies, so the layouts and points they share stay alive and no
  /// address in them is reused while compared
  drawn: Vec<UiDraw>,
  /// The slot table they were drawn with
  slots: Vec<ScrollSlot>,
  /// Each of `drawn`'s stamps (`stamp_of`)
  stamps: Vec<u64>,
  /// The fragment runs it holds, by tag, in the order of their ranges: a
  /// run's commands are its fragment's, the same while its version is, and
  /// the fragment keeps them (and the layouts and points they share) alive
  runs: Vec<HeldRun>,
  /// The commands of each run that draw from a sprite or an icon: where in
  /// the run, and the stamp each was drawn with
  run_stamps: Vec<(u32, u64)>,
  /// `runs`' indices in the order of their versions: what was found of a
  /// run's commands is taken again when it is seen with its version
  by_version: Vec<u32>,
  /// The runs and their stamps drawn before, while a frame's are made
  /// (buffers kept)
  runs_was: Vec<HeldRun>,
  run_stamps_was: Vec<(u32, u64)>,
  /// How many draws it holds, outside runs and in them
  count: usize,
  env: Option<UiRenderEnv>,
  /// The epoch of the UI image `drawn` is in, once a presented frame's `ui`
  /// pass drew it
  image: Option<u64>,
  /// The last frame's
  pub(crate) stats: UiRenderStats,
}

/// A fragment run the UI image holds
#[derive(Debug)]
struct HeldRun {
  run: FragmentRun,
  /// One of its commands reads the clock (`animated`): it never keeps
  animated: bool,
  /// Its commands with a stamp, in `UiRetain::run_stamps`
  stamps: Range<usize>,
}

impl UiRetain {
  /// Whether this frame shows the UI image as it is: its commands draw what
  /// the image holds (the same commands in the same order and slots, the
  /// same slot table, the same sprite and icon versions, none time-driven),
  /// in the same environment, and the image (epoch `image`) is the one they
  /// were drawn into. A fragment's run is the same by its tag (its
  /// fragment's version, root, order, layer, slot and range), not by its
  /// commands; the draws outside runs are compared one by one. When not,
  /// the commands become what the image is to hold, once drawn.
  pub(crate) fn keep(
    &mut self,
    commands: &UiCommands,
    icons: Option<&TextIcons>,
    env: UiRenderEnv,
    image: u64,
  ) -> bool {
    let now = commands.as_slice();
    // Runs that do not hold their draws are not trusted: all are compared
    let runs = if commands.runs_hold() {
      commands.runs()
    } else {
      &[]
    };
    let same = !now.is_empty()
      && self.image == Some(image)
      && self.env == Some(env)
      && commands.slots() == self.slots.as_slice()
      && now.len() == self.count
      && runs.len() == self.runs.len()
      && runs.iter().zip(&self.runs).all(|(run, held)| {
        *run == held.run
          && !held.animated
          && self.run_stamps[held.stamps.clone()]
            .iter()
            .all(|&(k, stamp)| {
              let draw = now.get(run.draws.start as usize + k as usize);
              draw.is_some_and(|draw| stamp_of(&draw.command, icons) == stamp)
            })
      })
      && loose(now, runs)
        .zip(&self.drawn)
        .zip(&self.stamps)
        .all(|((draw, was), stamp)| {
          draw.order == was.order
            && draw.slot == was.slot
            && !animated(&draw.command)
            && same(&draw.command, &was.command)
            && stamp_of(&draw.command, icons) == *stamp
        });
    if same {
      return true;
    }
    self.hold(now, runs, icons);
    self.slots.clear();
    self.slots.extend_from_slice(commands.slots());
    self.env = Some(env);
    self.image = None;
    false
  }

  /// Hold `now` with its `runs`: copies of the draws outside them, each
  /// run's tag, and every stamp
  fn hold(&mut self, now: &[UiDraw], runs: &[FragmentRun], icons: Option<&TextIcons>) {
    self.drawn.clear();
    self.stamps.clear();
    for draw in loose(now, runs) {
      self.drawn.push(draw.clone());
      self.stamps.push(stamp_of(&draw.command, icons));
    }
    self.count = now.len();
    std::mem::swap(&mut self.runs, &mut self.runs_was);
    std::mem::swap(&mut self.run_stamps, &mut self.run_stamps_was);
    self.runs.clear();
    self.run_stamps.clear();
    for run in runs {
      let draws = now
        .get(run.draws.start as usize..run.draws.end as usize)
        .unwrap_or_default();
      let from = self.run_stamps.len();
      // A run seen with its version has the commands it had: which have a
      // stamp, and whether one reads the clock, was found then
      let animated = match self.seen(run) {
        Some(was) => {
          let was = &self.runs_was[was];
          for &(k, _) in &self.run_stamps_was[was.stamps.clone()] {
            if let Some(draw) = draws.get(k as usize) {
              self.run_stamps.push((k, stamp_of(&draw.command, icons)));
            }
          }
          was.animated
        }
        None => {
          let mut any = false;
          for (k, draw) in (0u32..).zip(draws) {
            any |= animated(&draw.command);
            if stamped(&draw.command) {
              self.run_stamps.push((k, stamp_of(&draw.command, icons)));
            }
          }
          any
        }
      };
      self.runs.push(HeldRun {
        run: run.clone(),
        animated,
        stamps: from..self.run_stamps.len(),
      });
    }
    let held = &self.runs;
    self.by_version.clear();
    self
      .by_version
      .extend(0..u32::try_from(held.len()).unwrap_or(u32::MAX));
    self
      .by_version
      .sort_unstable_by_key(|&k| held.get(k as usize).map(|held| held.run.version));
  }

  /// Where `run` is among the runs drawn before, if one of them is it with
  /// its version (its commands the same)
  fn seen(&self, run: &FragmentRun) -> Option<usize> {
    let version = |k: &u32| self.runs_was.get(*k as usize).map(|held| held.run.version);
    let at = self
      .by_version
      .binary_search_by_key(&Some(run.version), version)
      .ok()?;
    let k = *self.by_version.get(at)? as usize;
    let was = &self.runs_was.get(k)?.run;
    let count = |run: &FragmentRun| run.draws.end.saturating_sub(run.draws.start);
    (was.id == run.id && count(was) == count(run)).then_some(k)
  }

  /// The frame was presented. Unless it kept the image, the image now holds
  /// its commands if its `ui` pass drew them (`drew`) into the image of
  /// epoch `image`, and is unknown otherwise.
  pub(crate) fn presented(&mut self, kept: bool, drew: bool, image: u64) {
    if !kept {
      self.image = drew.then_some(image);
    }
  }

  /// The frame was not presented: what the image holds is unknown
  pub(crate) fn lost(&mut self) {
    self.image = None;
  }

  /// Forget the image (retention was turned off)
  pub(crate) fn clear(&mut self) {
    self.drawn.clear();
    self.slots.clear();
    self.stamps.clear();
    self.runs.clear();
    self.run_stamps.clear();
    self.by_version.clear();
    self.runs_was.clear();
    self.run_stamps_was.clear();
    self.count = 0;
    self.env = None;
    self.image = None;
  }
}

/// The draws of `now` outside `runs` (in the order of their ranges, which
/// do not overlap), in order
fn loose<'a>(now: &'a [UiDraw], runs: &'a [FragmentRun]) -> impl Iterator<Item = &'a UiDraw> {
  let ranges = runs
    .iter()
    .map(|run| (run.draws.start as usize, run.draws.end as usize))
    .chain(std::iter::once((now.len(), now.len())));
  ranges
    .scan(0, move |at, (start, end)| {
      let gap = now.get(*at..start).unwrap_or_default();
      *at = end;
      Some(gap)
    })
    .flatten()
}

/// Whether drawing `command` reads the clock: a text with an effect
fn animated(command: &UiCommand) -> bool {
  matches!(command, UiCommand::Text { layout, .. } if layout.has_effects())
}

fn bits(rect: Rect) -> [u32; 4] {
  [rect.x, rect.y, rect.width, rect.height].map(f32::to_bits)
}

/// Whether `a` draws what `b` does: shared layouts by address, line points
/// by value (immediate widgets allocate them anew every frame), numbers by
/// their bits, sprites by handle (their versions are stamps)
fn same(a: &UiCommand, b: &UiCommand) -> bool {
  let clips = |a: &Option<Rect>, b: &Option<Rect>| a.map(bits) == b.map(bits);
  match (a, b) {
    (
      UiCommand::Rect {
        rect,
        fill,
        radius,
        border,
        clip,
        layer,
      },
      UiCommand::Rect {
        rect: rect_b,
        fill: fill_b,
        radius: radius_b,
        border: border_b,
        clip: clip_b,
        layer: layer_b,
      },
    ) => {
      bits(*rect) == bits(*rect_b)
        && fill == fill_b
        && radius.to_bits() == radius_b.to_bits()
        && border.map(|(width, color)| (width.to_bits(), color))
          == border_b.map(|(width, color)| (width.to_bits(), color))
        && clips(clip, clip_b)
        && layer == layer_b
    }
    (
      UiCommand::Sprite {
        sprite,
        rect,
        uv,
        tint,
        filter,
        clip,
        layer,
      },
      UiCommand::Sprite {
        sprite: sprite_b,
        rect: rect_b,
        uv: uv_b,
        tint: tint_b,
        filter: filter_b,
        clip: clip_b,
        layer: layer_b,
      },
    ) => {
      sprite.serial() == sprite_b.serial()
        && bits(*rect) == bits(*rect_b)
        && uv.map(f32::to_bits) == uv_b.map(f32::to_bits)
        && tint == tint_b
        && filter == filter_b
        && clips(clip, clip_b)
        && layer == layer_b
    }
    (
      UiCommand::Line {
        points,
        at,
        width,
        color,
        closed,
        clip,
        layer,
      },
      UiCommand::Line {
        points: points_b,
        at: at_b,
        width: width_b,
        color: color_b,
        closed: closed_b,
        clip: clip_b,
        layer: layer_b,
      },
    ) => {
      (Arc::ptr_eq(points, points_b)
        || (points.len() == points_b.len()
          && points
            .iter()
            .zip(points_b.iter())
            .all(|(p, q)| p.to_array().map(f32::to_bits) == q.to_array().map(f32::to_bits))))
        && at.to_array().map(f32::to_bits) == at_b.to_array().map(f32::to_bits)
        && width.to_bits() == width_b.to_bits()
        && color == color_b
        && closed == closed_b
        && clips(clip, clip_b)
        && layer == layer_b
    }
    (
      UiCommand::Text {
        layout,
        origin,
        reveal,
        opacity,
        seed,
        clip,
        layer,
      },
      UiCommand::Text {
        layout: layout_b,
        origin: origin_b,
        reveal: reveal_b,
        opacity: opacity_b,
        seed: seed_b,
        clip: clip_b,
        layer: layer_b,
      },
    ) => {
      Arc::ptr_eq(layout, layout_b)
        && origin.to_array().map(f32::to_bits) == origin_b.to_array().map(f32::to_bits)
        && reveal.map(|(shown, fade)| (shown.to_bits(), fade.to_bits()))
          == reveal_b.map(|(shown, fade)| (shown.to_bits(), fade.to_bits()))
        && opacity.to_bits() == opacity_b.to_bits()
        && seed == seed_b
        && clips(clip, clip_b)
        && layer == layer_b
    }
    _ => false,
  }
}

/// Whether `command` draws from something besides its own value: a sprite,
/// or a text with icons (`stamp_of`)
fn stamped(command: &UiCommand) -> bool {
  match command {
    UiCommand::Sprite { .. } => true,
    UiCommand::Text { layout, .. } => !layout.icons.is_empty(),
    _ => false,
  }
}

/// What `command` draws from besides its own value: a sprite's version, or
/// a text's icons (each sprite's version; a name's sprite and region in
/// `icons`); 0 for neither
fn stamp_of(command: &UiCommand, icons: Option<&TextIcons>) -> u64 {
  match command {
    UiCommand::Sprite { sprite, .. } => sprite.generation(),
    UiCommand::Text { layout, .. } if !layout.icons.is_empty() => {
      let mut hasher = FastHasher::default();
      for placed in &layout.icons {
        match &placed.icon {
          Icon::Sprite { sprite, .. } => {
            hasher.write_usize(sprite.serial());
            hasher.write_u64(sprite.generation());
          }
          Icon::Named { name, .. } => match icons.and_then(|icons| icons.get(name.as_str())) {
            Some(source) => {
              hasher.write_usize(source.sprite.serial());
              hasher.write_u64(source.sprite.generation());
              for bit in source.region.map_or([u32::MAX; 4], bits) {
                hasher.write_u32(bit);
              }
            }
            None => hasher.write_u64(u64::MAX),
          },
        }
      }
      hasher.finish()
    }
    _ => 0,
  }
}
