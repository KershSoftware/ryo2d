//! A UI text's glyphs, built once and drawn again while the text, its
//! clip and the glyph pages they sample are unchanged: as they were where
//! they were built, or laid again from their seeds where the text has moved

use std::sync::Arc;

use glam::Vec2;

use super::UiRenderEnv;
use crate::core::graphics::GlyphPage;
use crate::core::hash::FastMap;
use crate::core::render::text::{
  GlyphSeed, TextPlacement, TextSources, TextTargets, emit_glyphs_seeded, emit_icons, emit_text,
  lay_seeds, seeds_fit,
};
use crate::core::render::{DrawGlyph, PixelRect};
use crate::core::text::TextLayout;

/// Glyph and seed buffers kept for reuse when their texts go
const SPARE: usize = 64;

/// What a text's glyphs were built from, besides the UI's environment and
/// its origin: its layout (by address, held by the entry) and the rest of
/// its placement, and which of the frame's texts with these it is (the
/// first, the second, in painter's order), so a layout drawn twice keeps
/// two entries.
///
/// What the text emitters make of a text's glyphs follows from the key, the
/// environment and the origin, which only places them: a seed laid at the
/// new origin is what building it there gives, when the glyph's image is
/// the same (`GlyphSeed::fits`). What a sink makes of them (which it culls
/// included) follows from those glyphs and the key's clip.
///
/// Relies on `text_of` (render/ui.rs) keeping the placement's other fields
/// (axes, pixel grid, blend, layers, draw kind, lit, camera, size steps, max
/// glyph size) constant or derived from the layout and the `UiRenderEnv`: a
/// per-text value there must join the key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct TextKey {
  layout: usize,
  reveal: Option<[u32; 2]>,
  opacity: u32,
  seed: u64,
  scissor: Option<PixelRect>,
  /// Texts with this key drawn before it in the frame
  nth: u32,
}

impl TextKey {
  fn of(layout: &Arc<TextLayout>, place: &TextPlacement) -> TextKey {
    TextKey {
      layout: Arc::as_ptr(layout) as usize,
      reveal: place
        .reveal
        .map(|(shown, fade)| [shown.to_bits(), fade.to_bits()]),
      opacity: place.opacity.to_bits(),
      seed: place.seed,
      scissor: place.draw.scissor,
      nth: 0,
    }
  }
}

/// An origin's bits: two origins are the same place when these are equal
fn bits(origin: Vec2) -> [u32; 2] {
  origin.to_array().map(f32::to_bits)
}

/// Where a cached text's glyphs go. The text emitters lay a text's glyphs
/// into `targets.list`, and a sink takes them from there (the UI renderer
/// makes its instances of them); a kept text's seeds are laid straight to
/// it. A sink says what it made of a built text's glyphs, to keep, and
/// pushes a kept text again.
pub(crate) trait TextSink<G> {
  /// Where what the sink makes of the next built glyph lands (to capture
  /// from)
  fn mark(&self, targets: &TextTargets<'_>) -> usize;
  /// Take what the text emitters laid into `targets.list` since the last
  /// take
  fn take(&mut self, targets: &mut TextTargets<'_>);
  /// What the sink made of the glyphs built or laid since `mark` gave
  /// `from`
  fn captured<'s>(&'s self, targets: &'s TextTargets<'_>, from: usize) -> &'s [G];
  /// Push a text's kept glyphs again, at this frame's place
  fn push_cached(&mut self, targets: &mut TextTargets<'_>, glyphs: &[G], place: &TextPlacement);
  /// Make what it makes of a glyph laid straight to it, not through the
  /// draw list, as `take` makes it of a glyph found there
  fn glyph(&mut self, glyph: &DrawGlyph);
}

/// A text's built glyphs: what its sink made of them where they were last
/// laid, and their seeds, to lay them anywhere
#[derive(Debug)]
struct Built<G> {
  /// Held so its address, in the key, is not reused while the entry lives
  _layout: Arc<TextLayout>,
  /// Where `glyphs` were laid (`bits`)
  origin: [u32; 2],
  /// Where it was last drawn (`bits`): its seeds' subpixel bins hold there
  last: [u32; 2],
  glyphs: Vec<G>,
  seeds: Vec<GlyphSeed>,
  /// Each glyph page the text's glyphs sample, with its generation when they
  /// were built: they are good while every one is unchanged
  pages: Vec<(GlyphPage, u64)>,
  /// The last frame it was drawn in
  used: u64,
  /// On a key's first entry: the texts with its key drawn in frame `used`
  drawn: u32,
}

/// The UI's texts' built glyphs, kept from one frame to the next as what
/// their sink made of them (the UI renderer's instances) and as seeds
#[derive(Debug)]
pub(crate) struct UiTextCache<G> {
  built: FastMap<TextKey, Built<G>>,
  spare: Vec<Vec<G>>,
  spare_seeds: Vec<Vec<GlyphSeed>>,
  env: Option<UiRenderEnv>,
  /// Keep and reuse built glyphs (retention is on)
  reuse: bool,
  frame: u64,
  texts_built: u32,
  texts_reused: u32,
}

impl<G> Default for UiTextCache<G> {
  fn default() -> Self {
    Self {
      built: FastMap::default(),
      spare: Vec::new(),
      spare_seeds: Vec::new(),
      env: None,
      reuse: false,
      frame: 0,
      texts_built: 0,
      texts_reused: 0,
    }
  }
}

impl<G: Clone> UiTextCache<G> {
  /// Start a frame drawn in `env`. Glyphs built in another environment are
  /// dropped, and so is everything when `reuse` is off.
  pub(crate) fn begin(&mut self, env: UiRenderEnv, reuse: bool) {
    if !reuse || self.env != Some(env) {
      self.clear();
    }
    self.env = Some(env);
    self.reuse = reuse;
    self.frame += 1;
    self.texts_built = 0;
    self.texts_reused = 0;
  }

  /// Draw a text into `sink`: the glyphs built for it earlier when it has
  /// them and their pages still hold them (pinning the pages for this
  /// frame), else newly built ones, kept for the next frames. Kept glyphs
  /// are pushed as they are where they were laid, and laid again from their
  /// seeds where the text has moved (built anew when a glyph's image would
  /// differ there). Its icons are drawn anew each time; a text with an
  /// effect is built every frame.
  pub(crate) fn emit_into(
    &mut self,
    sources: &TextSources<'_>,
    targets: &mut TextTargets<'_>,
    sink: &mut impl TextSink<G>,
    layout: &Arc<TextLayout>,
    place: &TextPlacement,
  ) {
    if !self.reuse || layout.has_effects() {
      emit_text(sources, targets, layout, place);
      sink.take(targets);
      self.texts_built += 1;
      return;
    }
    let mut key = TextKey::of(layout, place);
    let frame = self.frame;
    let reused = match self.built.get_mut(&key) {
      // Another text with this key was drawn first: this one has an entry
      // of its own
      Some(first) if first.used == frame => {
        key.nth = first.drawn;
        first.drawn += 1;
        self
          .built
          .get_mut(&key)
          .is_some_and(|built| reuse(built, frame, targets, sink, place))
      }
      Some(first) => {
        first.drawn = 1;
        reuse(first, frame, targets, sink, place)
      }
      None => false,
    };
    if reused {
      self.texts_reused += 1;
    } else {
      self.build(sources, targets, sink, layout, place, key);
    }
    emit_icons(sources, targets, layout, place);
    sink.take(targets);
  }

  /// Build a text's glyphs into `sink` and keep them as `key`'s
  fn build(
    &mut self,
    sources: &TextSources<'_>,
    targets: &mut TextTargets<'_>,
    sink: &mut impl TextSink<G>,
    layout: &Arc<TextLayout>,
    place: &TextPlacement,
    key: TextKey,
  ) {
    let from = sink.mark(targets);
    let (glyphs, sprites) = (targets.list.glyph_count(), targets.list.sprite_count());
    let refused = targets.sprites.glyphs_refused();
    let mut seeds = self.spare_seeds.pop().unwrap_or_default();
    seeds.clear();
    emit_glyphs_seeded(sources, targets, layout, place, &mut seeds);
    sink.take(targets);
    self.texts_built += 1;
    // Kept only when every glyph was placed (a full atlas refuses some
    // for this frame) and none is a bitmap font's sprite cell
    let complete =
      targets.sprites.glyphs_refused() == refused && targets.list.sprite_count() == sprites;
    if complete {
      let made = sink.captured(targets, from);
      self.keep(key, layout, place.origin, targets, glyphs, made, seeds);
    } else {
      self.spare_seeds.push(seeds);
    }
  }

  /// Keep `made` and `seeds` as `key`'s, laid at `origin`, its pages those
  /// of the glyphs laid into the draw list from the `start`-th on (every
  /// glyph built, also those a sink left out, so reusing the text pins what
  /// building it did)
  #[allow(clippy::too_many_arguments)]
  fn keep(
    &mut self,
    key: TextKey,
    layout: &Arc<TextLayout>,
    origin: Vec2,
    targets: &TextTargets<'_>,
    start: usize,
    made: &[G],
    seeds: Vec<GlyphSeed>,
  ) {
    let mut pages: Vec<(GlyphPage, u64)> = Vec::new();
    for glyph in targets.list.glyphs_from(start) {
      if pages.iter().any(|&(page, _)| page == glyph.page) {
        continue;
      }
      match targets.sprites.glyph_page_generation(glyph.page) {
        Some(generation) => pages.push((glyph.page, generation)),
        None => {
          self.spare_seeds.push(seeds);
          return;
        }
      }
    }
    let mut glyphs = self.spare.pop().unwrap_or_default();
    glyphs.clear();
    glyphs.extend_from_slice(made);
    let built = Built {
      _layout: layout.clone(),
      origin: bits(origin),
      last: bits(origin),
      glyphs,
      seeds,
      pages,
      used: self.frame,
      drawn: 1,
    };
    if let Some(old) = self.built.insert(key, built) {
      self.recycle(old.glyphs, old.seeds);
    }
  }

  fn recycle(&mut self, glyphs: Vec<G>, seeds: Vec<GlyphSeed>) {
    if self.spare.len() < SPARE {
      self.spare.push(glyphs);
    }
    if self.spare_seeds.len() < SPARE {
      self.spare_seeds.push(seeds);
    }
  }

  /// End the frame: texts not drawn in it are dropped
  pub(crate) fn end(&mut self) {
    let frame = self.frame;
    let mut gone = Vec::new();
    self.built.retain(|_, built| {
      let drawn = built.used == frame;
      if !drawn {
        gone.push((
          std::mem::take(&mut built.glyphs),
          std::mem::take(&mut built.seeds),
        ));
      }
      drawn
    });
    for (glyphs, seeds) in gone {
      self.recycle(glyphs, seeds);
    }
  }

  /// Drop everything built
  pub(crate) fn clear(&mut self) {
    self.built.clear();
    self.env = None;
  }

  /// Texts built in the frame
  pub(crate) fn built(&self) -> u32 {
    self.texts_built
  }

  /// Texts drawn from glyphs built earlier, in the frame
  pub(crate) fn reused(&self) -> u32 {
    self.texts_reused
  }

  /// The texts kept
  #[cfg(test)]
  pub(crate) fn entries(&self) -> usize {
    self.built.len()
  }
}

/// Draw a kept text into `sink` at `place`, in `frame`, from `built`: as it
/// is where it was laid, laid again from its seeds elsewhere (keeping what
/// the sink made of them once it is drawn at one place twice, so a text
/// that keeps moving is never copied). `false`, drawing nothing, when its
/// glyph pages changed or, moved, a glyph's image would differ.
fn reuse<G: Clone>(
  built: &mut Built<G>,
  frame: u64,
  targets: &mut TextTargets<'_>,
  sink: &mut impl TextSink<G>,
  place: &TextPlacement,
) -> bool {
  let held = built
    .pages
    .iter()
    .all(|&(page, generation)| targets.sprites.glyph_page_generation(page) == Some(generation));
  if !held {
    return false;
  }
  let origin = bits(place.origin);
  if built.origin == origin {
    sink.push_cached(targets, &built.glyphs, place);
  } else {
    // The bins follow from the origin's x alone, and hold at the last's
    if origin[0] != built.last[0] && !seeds_fit(place, &built.seeds) {
      return false;
    }
    let from = sink.mark(targets);
    lay_seeds(place, &built.seeds, |glyph| sink.glyph(glyph));
    if origin == built.last {
      built.glyphs.clear();
      built.glyphs.extend_from_slice(sink.captured(targets, from));
      built.origin = origin;
    }
  }
  built.last = origin;
  for &(page, _) in &built.pages {
    targets.sprites.pin_glyph_page(page);
  }
  built.used = frame;
  true
}
