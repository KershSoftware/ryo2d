//! A UI text's glyphs, built once and drawn again while the text, its
//! placement and the glyph pages they sample are unchanged

use std::sync::Arc;

use super::UiRenderEnv;
use crate::core::graphics::GlyphPage;
use crate::core::hash::FastMap;
use crate::core::render::PixelRect;
use crate::core::render::draw_list::DrawGlyph;
use crate::core::render::text::{
  TextPlacement, TextSources, TextTargets, emit_glyphs, emit_icons, emit_text,
};
use crate::core::text::TextLayout;

/// Glyph buffers kept for reuse when their texts go
const SPARE: usize = 64;

/// What a text's glyphs were built from, besides the UI's environment: its
/// layout (by address, held by the entry) and its placement.
///
/// What a sink makes of a text's glyphs (which of them it culls included)
/// depends only on the key and the environment, so a kept text is reused
/// exactly as it would be built again.
///
/// Relies on `text_of` (render/ui.rs) keeping the placement's other fields
/// (axes, pixel grid, blend, layers, draw kind, lit, camera, size steps, max
/// glyph size) constant or derived from the layout and the `UiRenderEnv`: a
/// per-text value there must join the key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct TextKey {
  layout: usize,
  origin: [u32; 2],
  reveal: Option<[u32; 2]>,
  opacity: u32,
  seed: u64,
  scissor: Option<PixelRect>,
}

impl TextKey {
  fn of(layout: &Arc<TextLayout>, place: &TextPlacement) -> TextKey {
    TextKey {
      layout: Arc::as_ptr(layout) as usize,
      origin: place.origin.to_array().map(f32::to_bits),
      reveal: place
        .reveal
        .map(|(shown, fade)| [shown.to_bits(), fade.to_bits()]),
      opacity: place.opacity.to_bits(),
      seed: place.seed,
      scissor: place.draw.scissor,
    }
  }
}

/// Where a cached text's glyphs go. The text emitters lay a text's glyphs
/// into `targets.list`; a sink takes them from there (the old path leaves
/// them in the draw list, the UI renderer makes its instances of them), says
/// what it made of a built text's glyphs, to keep, and pushes a kept text
/// again.
pub(crate) trait TextSink<G> {
  /// Where what the sink makes of the next built glyph lands (to capture
  /// from)
  fn mark(&self, targets: &TextTargets<'_>) -> usize;
  /// Take what the text emitters laid into `targets.list` since the last
  /// take
  fn take(&mut self, targets: &mut TextTargets<'_>);
  /// What the sink made of the glyphs built since `mark` gave `from`
  fn captured<'s>(&'s self, targets: &'s TextTargets<'_>, from: usize) -> &'s [G];
  /// Push a text's kept glyphs again, at this frame's place
  fn push_cached(&mut self, targets: &mut TextTargets<'_>, glyphs: &[G], place: &TextPlacement);
}

/// The old path's sink: the glyphs stay in the draw list
struct DrawListSink;

impl TextSink<DrawGlyph> for DrawListSink {
  fn mark(&self, targets: &TextTargets<'_>) -> usize {
    targets.list.glyph_count()
  }

  fn take(&mut self, _targets: &mut TextTargets<'_>) {}

  fn captured<'s>(&'s self, targets: &'s TextTargets<'_>, from: usize) -> &'s [DrawGlyph] {
    targets.list.glyphs_from(from)
  }

  fn push_cached(
    &mut self,
    targets: &mut TextTargets<'_>,
    glyphs: &[DrawGlyph],
    place: &TextPlacement,
  ) {
    let draw = place.draw;
    targets
      .list
      .push_glyphs(draw.kind, draw.z, draw.lit, draw.only_camera, glyphs);
  }
}

/// A text's built glyphs, as its sink made them
#[derive(Debug)]
struct Built<G> {
  /// Held so its address, in the key, is not reused while the entry lives
  _layout: Arc<TextLayout>,
  glyphs: Vec<G>,
  /// Each glyph page the text's glyphs sample, with its generation when they
  /// were built: they are good while every one is unchanged
  pages: Vec<(GlyphPage, u64)>,
  /// The last frame it was drawn in
  used: u64,
}

/// The UI's texts' built glyphs, kept from one frame to the next: as draw
/// list glyphs (`DrawGlyph`, the old path) or as what another sink makes of
/// them
#[derive(Debug)]
pub(crate) struct UiTextCache<G = DrawGlyph> {
  built: FastMap<TextKey, Built<G>>,
  spare: Vec<Vec<G>>,
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
      env: None,
      reuse: false,
      frame: 0,
      texts_built: 0,
      texts_reused: 0,
    }
  }
}

impl UiTextCache<DrawGlyph> {
  /// Draw a text into the draw list (see [`emit_into`](Self::emit_into))
  pub(crate) fn emit(
    &mut self,
    sources: &TextSources<'_>,
    targets: &mut TextTargets<'_>,
    layout: &Arc<TextLayout>,
    place: &TextPlacement,
  ) {
    self.emit_into(sources, targets, &mut DrawListSink, layout, place);
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
  /// frame), else newly built ones, kept for the next frames. Its icons are
  /// drawn anew each time; a text with an effect is built every frame.
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
    let key = TextKey::of(layout, place);
    if let Some(built) = self.built.get_mut(&key)
      && built
        .pages
        .iter()
        .all(|&(page, generation)| targets.sprites.glyph_page_generation(page) == Some(generation))
    {
      for &(page, _) in &built.pages {
        targets.sprites.pin_glyph_page(page);
      }
      built.used = self.frame;
      sink.push_cached(targets, &built.glyphs, place);
      self.texts_reused += 1;
    } else {
      let from = sink.mark(targets);
      let (glyphs, sprites) = (targets.list.glyph_count(), targets.list.sprite_count());
      let refused = targets.sprites.glyphs_refused();
      emit_glyphs(sources, targets, layout, place);
      sink.take(targets);
      self.texts_built += 1;
      // Kept only when every glyph was placed (a full atlas refuses some
      // for this frame) and none is a bitmap font's sprite cell
      let complete =
        targets.sprites.glyphs_refused() == refused && targets.list.sprite_count() == sprites;
      if complete {
        self.keep(key, layout, targets, glyphs, sink.captured(targets, from));
      }
    }
    emit_icons(sources, targets, layout, place);
    sink.take(targets);
  }

  /// Keep `made` as `key`'s, its pages those of the glyphs laid into the
  /// draw list from the `start`-th on (every glyph built, also those a sink
  /// left out, so reusing the text pins what building it did)
  fn keep(
    &mut self,
    key: TextKey,
    layout: &Arc<TextLayout>,
    targets: &TextTargets<'_>,
    start: usize,
    made: &[G],
  ) {
    let mut pages: Vec<(GlyphPage, u64)> = Vec::new();
    for glyph in targets.list.glyphs_from(start) {
      if pages.iter().any(|&(page, _)| page == glyph.page) {
        continue;
      }
      match targets.sprites.glyph_page_generation(glyph.page) {
        Some(generation) => pages.push((glyph.page, generation)),
        None => return,
      }
    }
    let mut glyphs = self.spare.pop().unwrap_or_default();
    glyphs.clear();
    glyphs.extend_from_slice(made);
    let built = Built {
      _layout: layout.clone(),
      glyphs,
      pages,
      used: self.frame,
    };
    if let Some(old) = self.built.insert(key, built) {
      self.recycle(old.glyphs);
    }
  }

  fn recycle(&mut self, glyphs: Vec<G>) {
    if self.spare.len() < SPARE {
      self.spare.push(glyphs);
    }
  }

  /// End the frame: texts not drawn in it are dropped
  pub(crate) fn end(&mut self) {
    let frame = self.frame;
    let mut gone = Vec::new();
    self.built.retain(|_, built| {
      let drawn = built.used == frame;
      if !drawn {
        gone.push(std::mem::take(&mut built.glyphs));
      }
      drawn
    });
    for glyphs in gone {
      self.recycle(glyphs);
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
}
