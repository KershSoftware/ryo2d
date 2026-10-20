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
/// layout (by address, held by the entry) and its placement
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

/// A text's built glyphs
#[derive(Debug)]
struct Built {
  /// Held so its address, in the key, is not reused while the entry lives
  _layout: Arc<TextLayout>,
  glyphs: Vec<DrawGlyph>,
  /// Each glyph page the glyphs sample, with its generation when they were
  /// built: they are good while every one is unchanged
  pages: Vec<(GlyphPage, u64)>,
  /// The last frame it was drawn in
  used: u64,
}

/// The UI's texts' built glyphs, kept from one frame to the next
#[derive(Debug, Default)]
pub(crate) struct UiTextCache {
  built: FastMap<TextKey, Built>,
  spare: Vec<Vec<DrawGlyph>>,
  env: Option<UiRenderEnv>,
  /// Keep and reuse built glyphs (retention is on)
  reuse: bool,
  frame: u64,
  texts_built: u32,
  texts_reused: u32,
}

impl UiTextCache {
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

  /// Draw a text: the glyphs built for it earlier when it has them and
  /// their pages still hold them (pinning the pages for this frame), else
  /// newly built ones, kept for the next frames. Its icons are drawn anew
  /// each time; a text with an effect is built every frame.
  pub(crate) fn emit(
    &mut self,
    sources: &TextSources<'_>,
    targets: &mut TextTargets<'_>,
    layout: &Arc<TextLayout>,
    place: &TextPlacement,
  ) {
    if !self.reuse || layout.has_effects() {
      emit_text(sources, targets, layout, place);
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
      let draw = place.draw;
      targets
        .list
        .push_glyphs(draw.kind, draw.z, draw.lit, draw.only_camera, &built.glyphs);
      self.texts_reused += 1;
    } else {
      let (glyphs, sprites) = (targets.list.glyph_count(), targets.list.sprite_count());
      let refused = targets.sprites.glyphs_refused();
      emit_glyphs(sources, targets, layout, place);
      self.texts_built += 1;
      // Kept only when every glyph was placed (a full atlas refuses some
      // for this frame) and none is a bitmap font's sprite cell
      let complete =
        targets.sprites.glyphs_refused() == refused && targets.list.sprite_count() == sprites;
      if complete {
        self.keep(key, layout, targets, glyphs);
      }
    }
    emit_icons(sources, targets, layout, place);
  }

  /// Keep the glyphs pushed from the `start`-th on as `key`'s
  fn keep(
    &mut self,
    key: TextKey,
    layout: &Arc<TextLayout>,
    targets: &TextTargets<'_>,
    start: usize,
  ) {
    let mut glyphs = self.spare.pop().unwrap_or_default();
    glyphs.clear();
    glyphs.extend_from_slice(targets.list.glyphs_from(start));
    let mut pages: Vec<(GlyphPage, u64)> = Vec::new();
    for glyph in &glyphs {
      if pages.iter().any(|&(page, _)| page == glyph.page) {
        continue;
      }
      match targets.sprites.glyph_page_generation(glyph.page) {
        Some(generation) => pages.push((glyph.page, generation)),
        None => return,
      }
    }
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

  fn recycle(&mut self, glyphs: Vec<DrawGlyph>) {
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
