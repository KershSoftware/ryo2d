//! The engine's text engine: Parley contexts over the registered fonts, the
//! layout cache, measuring and hit-testing. Engine-owned (not a resource),
//! so the UI pass and the world-text extract can borrow it mutably while
//! scenes hold the world.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::ops::Range;
use std::sync::Arc;

use glam::Vec2;
use parley::fontique::{Blob, Collection, CollectionOptions, FontInfoOverride, SourceCache};
use parley::{
  Affinity, Alignment, AlignmentOptions, Cursor, FontContext, FontFamily, FontFamilyName,
  FontStyle, FontWeight, InlineBox, InlineBoxKind, Layout, LayoutContext, LineHeight, OverflowWrap,
  PositionedLayoutItem, RangedBuilder, Selection, StyleProperty, VerticalAlign,
};

use crate::core::text::layout::{GlyphStyle, LineInfo, PlacedGlyph, PlacedIcon, TextLayout};
use crate::core::text::rich::{Icon, RichText};
use crate::core::text::style::{TextAlign, TextFont, TextLayoutOptions, TextOverflow, TextStyle};
use crate::core::text::{BitmapFace, FaceSource, FontId, FontRender, FontRole, Fonts};
use crate::core::ui::Rect;
use crate::engine_warn;

/// Cached layouts unused for this many frames are dropped
const CACHE_FRAMES: u64 = 120;

/// How often (in frames) the caches are swept
const SWEEP_FRAMES: u64 = 60;

const ROLES: [FontRole; 3] = [FontRole::Ui, FontRole::Mono, FontRole::Pixel];

const ELLIPSIS: char = '\u{2026}';

/// Cache counters (tests and the benchmark read them)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct CacheStats {
  pub(crate) hits: u64,
  pub(crate) misses: u64,
}

struct Cached {
  layout: Arc<TextLayout>,
  last_used: u64,
}

/// What the engine knows about one registered face
struct FaceInfo {
  family: String,
  weight: f32,
  italic: bool,
  render: FontRender,
  bitmap: Option<BitmapFace>,
}

pub(crate) struct TextEngine {
  generation: Option<u64>,
  fcx: FontContext,
  lcx: LayoutContext<u16>,
  faces: HashMap<FontId, FaceInfo>,
  /// Parley font (blob id, face index) to the registry's id
  by_blob: HashMap<(u64, u32), FontId>,
  roles: [Option<FontId>; 3],
  cache: HashMap<u64, Cached>,
  /// Single-line plain layouts for carets and hit tests
  lines: HashMap<u64, (Layout<u16>, u64)>,
  coords: HashMap<u64, Arc<[i16]>>,
  warned: HashSet<u64>,
  frame: u64,
  stats: CacheStats,
}

impl Default for TextEngine {
  fn default() -> Self {
    Self::new()
  }
}

impl TextEngine {
  /// An engine with no fonts; `sync` gives it the registry's
  pub(crate) fn new() -> Self {
    Self {
      generation: None,
      fcx: font_context(),
      lcx: LayoutContext::new(),
      faces: HashMap::new(),
      by_blob: HashMap::new(),
      roles: [None; 3],
      cache: HashMap::new(),
      lines: HashMap::new(),
      coords: HashMap::new(),
      warned: HashSet::new(),
      frame: 0,
      stats: CacheStats::default(),
    }
  }

  /// Take the registry's fonts when they changed since the last sync; every
  /// cached layout is dropped then
  pub(crate) fn sync(&mut self, fonts: &Fonts) {
    if Some(fonts.generation()) == self.generation {
      return;
    }
    self.fcx = font_context();
    self.faces.clear();
    self.by_blob.clear();
    self.cache.clear();
    self.lines.clear();
    self.roles = ROLES.map(|role| fonts.role(role));
    for (id, face) in fonts.faces() {
      let bitmap = match &face.source {
        FaceSource::Outline { data, index } => {
          let blob = Blob::new(Arc::new(data.clone()));
          let blob_id = blob.id();
          // Parley's family names are the registry's
          let info = FontInfoOverride {
            family_name: Some(&face.family),
            ..Default::default()
          };
          if self
            .fcx
            .collection
            .register_fonts(blob, Some(info))
            .is_empty()
          {
            if self.first_warning(hash_of(&("unreadable", id))) {
              engine_warn!(module: "text", "font {:?} could not be read for layout; text falls back", face.family);
            }
          } else {
            self.by_blob.insert((blob_id, *index), id);
          }
          None
        }
        FaceSource::Bitmap(bitmap) => Some(bitmap.clone()),
      };
      self.faces.insert(
        id,
        FaceInfo {
          family: face.family.clone(),
          weight: face.weight,
          italic: face.italic,
          render: face.render,
          bitmap,
        },
      );
    }
    self.generation = Some(fonts.generation());
  }

  /// Lay out `text` (cached). `None` when the style, options or scale cannot
  /// be laid out (warned once).
  pub(crate) fn layout(
    &mut self,
    text: &RichText,
    style: &TextStyle,
    options: &TextLayoutOptions,
    scale: f32,
  ) -> Option<Arc<TextLayout>> {
    if !style.is_valid() {
      let mut h = DefaultHasher::new();
      style.hash_into(&mut h);
      if self.first_warning(h.finish()) {
        engine_warn!(module: "text", "text style with size {} cannot be laid out; nothing is drawn", style.size);
      }
      return None;
    }
    if !options.is_valid() || !valid_scale(scale) {
      let key = hash_of(&("layout", options.wrap.map(f32::to_bits), scale.to_bits()));
      if self.first_warning(key) {
        engine_warn!(module: "text", "text with wrap width {:?} at scale {} cannot be laid out; nothing is drawn", options.wrap, scale);
      }
      return None;
    }

    let key = {
      let mut h = DefaultHasher::new();
      text.hash_into(&mut h);
      style.hash_into(&mut h);
      options.hash_into(&mut h);
      scale.to_bits().hash(&mut h);
      self.generation.hash(&mut h);
      h.finish()
    };
    if let Some(cached) = self.cache.get_mut(&key) {
      cached.last_used = self.frame;
      self.stats.hits += 1;
      return Some(Arc::clone(&cached.layout));
    }

    self.stats.misses += 1;
    let bitmap = self
      .resolve(&style.font)
      .filter(|id| self.bitmap(*id).is_some());
    let laid = Arc::new(match bitmap {
      Some(face) => self.layout_bitmap(face, text, style, options, scale),
      None => self.layout_parley(text, style, options, scale),
    });
    self.cache.insert(
      key,
      Cached {
        layout: Arc::clone(&laid),
        last_used: self.frame,
      },
    );
    Some(laid)
  }

  /// The block's size in text units (zero when it cannot be laid out)
  pub(crate) fn measure(
    &mut self,
    text: &RichText,
    style: &TextStyle,
    options: &TextLayoutOptions,
    scale: f32,
  ) -> Vec2 {
    self
      .layout(text, style, options, scale)
      .map_or(Vec2::ZERO, |laid| laid.size / scale)
  }

  /// The height of one line in text units
  pub(crate) fn line_height(&self, style: &TextStyle, scale: f32) -> f32 {
    if !style.is_valid() || !valid_scale(scale) {
      return 0.0;
    }
    match self.style_bitmap(style) {
      Some(bitmap) => {
        let ch = bitmap.cell.1 as f32;
        let k = style.size * scale / ch;
        ch * k * style.line_height / scale
      }
      None => style.size * style.line_height,
    }
  }

  /// The caret's x before `byte` of one line of plain text, in text units
  pub(crate) fn caret_x(&mut self, text: &str, style: &TextStyle, byte: usize, scale: f32) -> f32 {
    if !style.is_valid() || !valid_scale(scale) {
      return 0.0;
    }
    if let Some(bitmap) = self.style_bitmap(style) {
      return bitmap_caret(bitmap, text, byte, style.size);
    }
    self.single_line(text, style, scale).map_or(0.0, |line| {
      Cursor::from_byte_index(line, byte, Affinity::Downstream)
        .geometry(line, 1.0)
        .x0 as f32
        / scale
    })
  }

  /// The byte whose caret is nearest `x` (text units) on one line of plain
  /// text
  pub(crate) fn byte_at(&mut self, text: &str, style: &TextStyle, x: f32, scale: f32) -> usize {
    if !style.is_valid() || !valid_scale(scale) {
      return 0;
    }
    let x = if x.is_nan() { 0.0 } else { x.clamp(-1e9, 1e9) };
    if let Some(bitmap) = self.style_bitmap(style) {
      return bitmap_byte_at(bitmap, text, x, style.size);
    }
    self
      .single_line(text, style, scale)
      .map_or(0, |line| Cursor::from_point(line, x * scale, 0.0).index())
  }

  /// The left and right x (text units) of a selection on one line of plain
  /// text; `(0, 0)` for an empty or invalid range
  pub(crate) fn selection_x(
    &mut self,
    text: &str,
    style: &TextStyle,
    range: Range<usize>,
    scale: f32,
  ) -> (f32, f32) {
    if !style.is_valid()
      || !valid_scale(scale)
      || range.start >= range.end
      || range.start > text.len()
    {
      return (0.0, 0.0);
    }
    let end = range.end.min(text.len());
    if let Some(bitmap) = self.style_bitmap(style) {
      return (
        bitmap_caret(bitmap, text, range.start, style.size),
        bitmap_caret(bitmap, text, end, style.size),
      );
    }
    self
      .single_line(text, style, scale)
      .map_or((0.0, 0.0), |line| {
        let selection = Selection::new(
          Cursor::from_byte_index(line, range.start, Affinity::Downstream),
          Cursor::from_byte_index(line, end, Affinity::Upstream),
        );
        selection
          .geometry(line)
          .first()
          .map_or((0.0, 0.0), |(rect, _)| {
            (rect.x0 as f32 / scale, rect.x1 as f32 / scale)
          })
      })
  }

  /// The byte range of each line `text` wraps to at `width` text units
  pub(crate) fn wrap_lines(
    &mut self,
    text: &str,
    style: &TextStyle,
    width: f32,
    scale: f32,
  ) -> Vec<Range<usize>> {
    let laid = (width.is_finite() && width >= 0.0)
      .then(|| {
        let options = TextLayoutOptions::default().wrap(width);
        self.layout(&RichText::new(text), style, &options, scale)
      })
      .flatten()
      .filter(|laid| !laid.lines.is_empty());
    match laid {
      Some(laid) => laid.lines.iter().map(|line| line.range.clone()).collect(),
      // One line of the whole text
      None => std::iter::once(0..text.len()).collect(),
    }
  }

  /// The glyph a role's font draws `c` with
  #[cfg(test)]
  pub(crate) fn glyph_for(&mut self, role: FontRole, c: char) -> Option<u16> {
    let style = TextStyle::default().role(role);
    let laid = self.layout(
      &RichText::new(c.to_string()),
      &style,
      &TextLayoutOptions::default(),
      1.0,
    )?;
    laid.glyphs.first().map(|glyph| glyph.glyph)
  }

  /// The variable-font coordinates a glyph's `coords` hash names
  pub(crate) fn coords(&self, hash: u64) -> Option<Arc<[i16]>> {
    self.coords.get(&hash).cloned()
  }

  /// How a face draws (`Raster` for an unknown id)
  pub(crate) fn face_render(&self, font: FontId) -> FontRender {
    self
      .faces
      .get(&font)
      .map_or(FontRender::Raster, |face| face.render)
  }

  /// The family a face belongs to
  pub(crate) fn face_family(&self, font: FontId) -> Option<&str> {
    self.faces.get(&font).map(|face| face.family.as_str())
  }

  /// Count a frame; every so often drop the layouts unused for a while
  pub(crate) fn end_frame(&mut self) {
    self.frame += 1;
    if self.frame.is_multiple_of(SWEEP_FRAMES) {
      let frame = self.frame;
      self
        .cache
        .retain(|_, cached| frame - cached.last_used <= CACHE_FRAMES);
      self
        .lines
        .retain(|_, (_, last_used)| frame - *last_used <= CACHE_FRAMES);
    }
  }

  pub(crate) fn stats(&self) -> CacheStats {
    self.stats
  }

  /// True the first time a warning's key is seen
  fn first_warning(&mut self, key: u64) -> bool {
    self.warned.insert(key)
  }

  fn bitmap(&self, font: FontId) -> Option<&BitmapFace> {
    self.faces.get(&font).and_then(|face| face.bitmap.as_ref())
  }

  /// The bitmap face a style's own font is, if it is one
  fn style_bitmap(&self, style: &TextStyle) -> Option<&BitmapFace> {
    self.resolve(&style.font).and_then(|id| self.bitmap(id))
  }

  fn resolve(&self, font: &TextFont) -> Option<FontId> {
    match font {
      TextFont::Role(role) => self.roles[role.index()].filter(|id| self.faces.contains_key(id)),
      TextFont::Id(id) => self.faces.contains_key(id).then_some(*id),
      // The regular face of the family, as `Fonts::find`
      TextFont::Family(name) => self
        .faces
        .iter()
        .filter(|(_, face)| face.family == *name)
        .map(|(id, face)| {
          let distance = (face.weight - 400.0).abs() + if face.italic { 1000.0 } else { 0.0 };
          (*id, distance)
        })
        .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
        .map(|(id, _)| id),
    }
  }

  /// The weight and slant of an exact outline face (`TextFont::Id`): Parley
  /// picks faces by family, weight and slant, so these select that face
  fn exact_face(&self, font: &TextFont) -> Option<(f32, bool)> {
    let TextFont::Id(id) = font else {
      return None;
    };
    self
      .faces
      .get(id)
      .filter(|face| face.bitmap.is_none())
      .map(|face| (face.weight, face.italic))
  }

  /// The weight and slant to lay a style out with
  fn face_style(&self, style: &TextStyle) -> (f32, bool) {
    self
      .exact_face(&style.font)
      .unwrap_or((style.weight, style.italic))
  }

  /// The family names Parley tries, in order: the span's or style's font,
  /// the style's fallbacks, the UI font, then the mono font (which has the
  /// box drawing, arrows and shapes the UI font lacks)
  fn stack(&mut self, style: &TextStyle, span_font: Option<&TextFont>) -> Vec<String> {
    let mut ids = Vec::with_capacity(style.fallbacks.len() + 3);
    let mut unknown = Vec::new();
    for font in std::iter::once(span_font.unwrap_or(&style.font)).chain(&style.fallbacks) {
      match self.resolve(font) {
        Some(id) => ids.push(id),
        None => {
          if let TextFont::Family(name) = font {
            unknown.push(name.clone());
          }
        }
      }
    }
    ids.extend(
      [FontRole::Ui, FontRole::Mono]
        .iter()
        .filter_map(|role| self.roles[role.index()]),
    );

    let mut names: Vec<String> = Vec::with_capacity(ids.len());
    for id in ids {
      // Bitmap faces are not Parley's
      if let Some(face) = self.faces.get(&id)
        && face.bitmap.is_none()
        && !names.contains(&face.family)
      {
        names.push(face.family.clone());
      }
    }
    for name in unknown {
      if self.first_warning(hash_of(&("family", &name))) {
        engine_warn!(module: "text", "no font family named {name:?}; using the fallbacks");
      }
    }
    names
  }

  /// A span's family stack, or `None` (warned once) when its font is a
  /// bitmap font, which outline text cannot use
  fn span_stack(&mut self, style: &TextStyle, font: &TextFont) -> Option<Vec<String>> {
    if let Some(id) = self.resolve(font)
      && self.bitmap(id).is_some()
    {
      if self.first_warning(hash_of(&("span bitmap", font))) {
        engine_warn!(module: "text", "a bitmap font cannot style part of outline text; the span {font:?} keeps the base font");
      }
      return None;
    }
    Some(self.stack(style, Some(font)))
  }

  fn layout_parley(
    &mut self,
    text: &RichText,
    style: &TextStyle,
    options: &TextLayoutOptions,
    scale: f32,
  ) -> TextLayout {
    let source = text.text.as_str();
    let stack = self.stack(style, None);
    let Some(family) = stack.first().cloned() else {
      if self.first_warning(hash_of(&("no font", &style.font))) {
        engine_warn!(module: "text", "no font for text in {:?}, and no UI or mono font to fall back on; nothing is drawn", style.font);
      }
      return TextLayout {
        scale,
        ..TextLayout::default()
      };
    };

    let spans = self.span_runs(text, style);
    let face = self.face_style(style);
    let wrap_px = options.wrap.map(|width| width * scale);
    let mut builder = self.lcx.ranged_builder(&mut self.fcx, source, scale, true);
    push_defaults(&mut builder, &stack, style, face);
    for span in &spans {
      span.push(&mut builder);
    }
    for (i, mark) in text.icons.iter().enumerate() {
      if mark.at <= source.len() && source.is_char_boundary(mark.at) {
        let size = icon_size(&mark.icon, style.size) * scale;
        builder.push_inline_box(InlineBox {
          id: i as u64,
          kind: InlineBoxKind::InFlow,
          index: mark.at,
          width: size.x,
          height: size.y,
          baseline: None,
          vertical_align: VerticalAlign::BASELINE,
        });
      }
    }
    let mut parley_layout = builder.build(source);
    parley_layout.break_all_lines(wrap_px);
    parley_layout.align(alignment(options.align), AlignmentOptions::default());

    let keep = options.max_lines.map_or(parley_layout.len(), |n| {
      (n as usize).min(parley_layout.len())
    });
    let mut placed = self.place(&parley_layout, text, keep);
    self.warn_missing(&placed, source, &family);

    // Without a wrap Parley aligns lines within the widest line of the whole
    // text, dropped lines included: move the kept lines to their own left edge
    if wrap_px.is_none() {
      placed.shift_left(placed.left());
    }
    // The width is the kept lines'. Parley lays empty text out as one space.
    let mut width = if source.is_empty() {
      placed.icons_right(0)
    } else {
      placed.right()
    };
    if keep < parley_layout.len() && options.overflow == TextOverflow::Ellipsis {
      let dots = self.layout_parley(
        &RichText::new(ELLIPSIS.to_string()),
        style,
        &TextLayoutOptions::default(),
        scale,
      );
      if let Some(end) = placed.end_with(&dots, source, wrap_px) {
        width = width.max(end);
      }
    }

    // Lines aligned within a wrap width sit in a frame that wide; no lines
    // (`max_lines(0)`) make an empty block
    let frame = match wrap_px {
      _ if placed.lines.is_empty() => 0.0,
      Some(wrap) if options.align != TextAlign::Start || parley_layout.is_rtl() => wrap,
      Some(wrap) => width.min(wrap),
      None => width,
    };
    let clusters = placed.number_clusters();
    let pixel = !placed.glyphs.is_empty()
      && placed
        .glyphs
        .iter()
        .all(|glyph| matches!(self.face_render(glyph.font), FontRender::Pixel { .. }));
    TextLayout {
      size: Vec2::new(frame, placed.lines.iter().map(|line| line.height).sum()),
      scale,
      glyphs: placed.glyphs,
      icons: placed.icons,
      lines: placed.lines,
      styles: glyph_styles(text, style),
      clusters,
      pixel,
    }
  }

  /// Each span's layout overrides. Brush `i + 1` is span `i`; brush 0 is the
  /// base style.
  fn span_runs(&mut self, text: &RichText, style: &TextStyle) -> Vec<SpanRun> {
    let mut spans = Vec::with_capacity(text.spans.len());
    for (i, mark) in text.spans.iter().enumerate() {
      let Ok(brush) = u16::try_from(i + 1) else {
        break;
      };
      if !is_text_range(&text.text, &mark.range) {
        continue;
      }
      let size = mark
        .span
        .size
        .filter(|size| size.is_finite() && *size > 0.0);
      if let Some(bad) = mark.span.size
        && size.is_none()
        && self.first_warning(hash_of(&("span size", bad.to_bits())))
      {
        engine_warn!(module: "text", "text span with size {bad} cannot be laid out; it keeps the base size");
      }
      let stack = match &mark.span.font {
        Some(font) => self.span_stack(style, font),
        None => None,
      };
      // An exact face brings its own weight and slant
      let exact = mark
        .span
        .font
        .as_ref()
        .filter(|_| stack.is_some())
        .and_then(|font| self.exact_face(font));
      spans.push(SpanRun {
        range: mark.range.clone(),
        brush,
        size,
        stack,
        weight: exact
          .map(|(weight, _)| weight)
          .or(mark.span.weight.filter(|weight| weight.is_finite())),
        italic: exact.map(|(_, italic)| italic).or(mark.span.italic),
      });
    }
    spans
  }

  /// Walk the first `keep` lines into placed glyphs, icons and lines
  fn place(&mut self, parley_layout: &Layout<u16>, text: &RichText, keep: usize) -> Placed {
    let source = text.text.as_str();
    let mut placed = Placed::default();
    for line in parley_layout.lines().take(keep) {
      let metrics = line.metrics();
      // Parley gives a line of only an inline box a start past its end, and
      // empty text a one-space line: keep ranges inside the text, in order
      let range = line.text_range();
      let prev_end = placed.lines.last().map_or(0, |prev| prev.range.end);
      let from = if range.start > range.end {
        prev_end
      } else {
        range.start
      }
      .min(source.len());
      placed.lines.push(LineInfo {
        top: metrics.block_min_coord,
        baseline: metrics.baseline,
        height: metrics.line_height,
        range: from..range.end.clamp(from, source.len()),
      });
      // Where the line's content starts and ends (trailing spaces hang)
      let left = metrics.offset + metrics.inline_min_coord;
      placed
        .extents
        .push((left, left + metrics.advance - metrics.hanging_advance));
      placed.last_line = (placed.glyphs.len(), placed.icons.len());

      // A run's clusters yield their glyphs in the order its glyph runs
      // place them, so each glyph's cluster is the next in `run_starts`
      let mut current_run = None;
      let mut run_starts: Vec<usize> = Vec::new();
      let mut next = 0;
      for item in line.items() {
        match item {
          PositionedLayoutItem::GlyphRun(glyph_run) => {
            let run = glyph_run.run();
            if current_run != Some(run.index()) {
              current_run = Some(run.index());
              run_starts.clear();
              next = 0;
              for cluster in run.visual_clusters() {
                let start = cluster.text_range().start;
                placed.cluster_starts.push(start);
                run_starts.extend(std::iter::repeat_n(start, cluster.glyphs().count()));
              }
            }
            let data = run.font();
            let Some(&font) = self.by_blob.get(&(data.data.id(), data.index)) else {
              next += glyph_run.positioned_glyphs().count();
              if self.first_warning(hash_of(&("unknown font", data.data.id(), data.index))) {
                engine_warn!(module: "text", "text laid out with a font face the registry does not have; its glyphs are skipped");
              }
              continue;
            };
            let synthesis = run.synthesis();
            let coords = self.coords_key(
              run
                .normalized_coords()
                .iter()
                .map(|coord| coord.to_bits())
                .collect(),
            );
            let fallback_start = run.text_range().start;
            for glyph in glyph_run.positioned_glyphs() {
              let start = run_starts.get(next).copied().unwrap_or(fallback_start);
              next += 1;
              placed.push_glyph(
                PlacedGlyph {
                  font,
                  glyph: u16::try_from(glyph.id).unwrap_or(0),
                  pos: Vec2::new(glyph.x, glyph.y),
                  size: run.font_size(),
                  style: glyph_run.style().brush,
                  cluster: 0,
                  skew: synthesis.skew().is_some(),
                  embolden: synthesis.embolden(),
                  coords,
                },
                glyph.advance,
                start,
              );
            }
          }
          PositionedLayoutItem::InlineBox(inline_box) => {
            if let Some(mark) = usize::try_from(inline_box.id)
              .ok()
              .and_then(|i| text.icons.get(i))
            {
              placed.icons.push(PlacedIcon {
                icon: mark.icon.clone(),
                rect: Rect::new(
                  inline_box.x,
                  inline_box.y,
                  inline_box.width,
                  inline_box.height,
                ),
                cluster: 0,
              });
              placed.icon_at.push(mark.at);
            }
          }
        }
      }
    }
    placed
  }

  /// A character no font has draws as glyph 0, the first font's box: warn
  /// once per family and character
  fn warn_missing(&mut self, placed: &Placed, source: &str, family: &str) {
    for (glyph, &start) in placed.glyphs.iter().zip(&placed.starts) {
      if glyph.glyph != 0 {
        continue;
      }
      let Some(c) = first_char(source, start) else {
        continue;
      };
      if !c.is_whitespace() && !c.is_control() && self.first_warning(hash_of(&("tofu", family, c)))
      {
        engine_warn!(module: "text", "no font has {:?} (U+{:04X}); drawn as {}'s box", c, c as u32, family);
      }
    }
  }

  /// A monospace grid layout for a bitmap font: every character advances
  /// one cell
  fn layout_bitmap(
    &mut self,
    font: FontId,
    text: &RichText,
    style: &TextStyle,
    options: &TextLayoutOptions,
    scale: f32,
  ) -> TextLayout {
    let Some(face) = self.bitmap(font).cloned() else {
      return TextLayout {
        scale,
        ..TextLayout::default()
      };
    };
    let family = self.face_family(font).unwrap_or_default().to_string();
    let grid = Grid::new(&face, style.size * scale);
    let line_height = grid.size * style.line_height;
    let wrap_px = options.wrap.map(|width| width * scale);

    for mark in &text.spans {
      if let Some(span_font) = &mark.span.font
        && self.resolve(span_font) != Some(font)
        && self.first_warning(hash_of(&("bitmap span", span_font)))
      {
        engine_warn!(module: "text", "text in bitmap font {family:?} uses only that font; the span {span_font:?} is ignored");
      }
    }
    let (items, count, missing) = grid_cells(&grid, text, style.size, scale);
    for c in missing {
      if self.first_warning(hash_of(&("bitmap missing", font, c))) {
        let instead = if grid.question.is_some() {
          "drawn as '?'"
        } else {
          "skipped"
        };
        engine_warn!(module: "text", "bitmap font {family:?} has no {c:?} (U+{:04X}); {instead}", c as u32);
      }
    }

    let mut rows = break_rows(&items, wrap_px);
    let ranges = row_ranges(&items, &rows, text.text.len());
    let keep = options
      .max_lines
      .map_or(rows.len(), |n| (n as usize).min(rows.len()));
    let truncated = keep < rows.len();
    rows.truncate(keep);
    let mut clusters = if truncated {
      rows
        .last()
        .and_then(|row| items.get(row.end))
        .map_or(0, |item| item.cluster)
    } else {
      count
    };

    // The ellipsis: "…" if the font has it, else "..."
    let mut dots: Vec<u16> = Vec::new();
    if truncated
      && options.overflow == TextOverflow::Ellipsis
      && let Some(row) = rows.last_mut()
    {
      dots = match (grid.cell(ELLIPSIS), grid.cell('.')) {
        (Some(cell), _) => vec![cell],
        (None, Some(cell)) => vec![cell; 3],
        (None, None) => Vec::new(),
      };
      if !dots.is_empty() {
        let limit = wrap_px.map_or(f32::INFINITY, |wrap| {
          wrap - dots.len() as f32 * grid.advance
        });
        let mut end = row_width(&items, row);
        while row.end > row.start {
          let item = &items[row.end - 1];
          if !item.space && end <= limit {
            break;
          }
          end -= item.width;
          row.end -= 1;
        }
        clusters = items
          .get(row.end)
          .map_or(clusters, |item| item.cluster.min(clusters));
      }
    }

    let mut widths: Vec<f32> = rows.iter().map(|row| row_width(&items, row)).collect();
    if let Some(last) = widths.last_mut() {
      *last += dots.len() as f32 * grid.advance;
    }
    let block = widths.iter().copied().fold(0.0, f32::max);
    let frame = match wrap_px {
      _ if rows.is_empty() => 0.0,
      Some(wrap) if options.align != TextAlign::Start => wrap,
      Some(wrap) => block.min(wrap),
      None => block,
    };

    let mut glyphs = Vec::new();
    let mut icons = Vec::new();
    let mut lines = Vec::with_capacity(rows.len());
    for (r, row) in rows.iter().enumerate() {
      let top = r as f32 * line_height;
      let baseline = top + grid.size;
      let mut pen = match options.align {
        TextAlign::Center => (frame - widths[r]) / 2.0,
        TextAlign::End => frame - widths[r],
        TextAlign::Start | TextAlign::Justify => 0.0,
      };
      // The ellipsis takes the look and the cluster of the character it follows
      let mut last = (0, items.get(row.start).map_or(0, |item| item.cluster));
      for item in &items[row.clone()] {
        match item.kind {
          CellKind::Glyph {
            cell: Some(cell),
            style,
          } => {
            glyphs.push(grid.glyph(font, cell, Vec2::new(pen, baseline), style, item.cluster));
            last = (style, item.cluster);
          }
          CellKind::Icon { index, size } => icons.push(PlacedIcon {
            icon: text.icons[index].icon.clone(),
            rect: Rect::new(pen, baseline - size.y, size.x, size.y),
            cluster: item.cluster,
          }),
          CellKind::Glyph { cell: None, .. } | CellKind::Break => {}
        }
        pen += item.width;
      }
      if r + 1 == rows.len() {
        for &cell in &dots {
          glyphs.push(grid.glyph(font, cell, Vec2::new(pen, baseline), last.0, last.1));
          pen += grid.advance;
        }
      }
      lines.push(LineInfo {
        top,
        baseline,
        height: line_height,
        range: ranges[r].clone(),
      });
    }

    TextLayout {
      size: Vec2::new(frame, rows.len() as f32 * line_height),
      scale,
      glyphs,
      icons,
      lines,
      styles: glyph_styles(text, style),
      clusters,
      pixel: true,
    }
  }

  /// A plain one-line layout for carets and hit tests, cached
  fn single_line(&mut self, text: &str, style: &TextStyle, scale: f32) -> Option<&Layout<u16>> {
    let key = {
      let mut h = DefaultHasher::new();
      text.hash(&mut h);
      style.hash_into(&mut h);
      scale.to_bits().hash(&mut h);
      self.generation.hash(&mut h);
      h.finish()
    };
    if !self.lines.contains_key(&key) {
      let stack = self.stack(style, None);
      if stack.is_empty() {
        return None;
      }
      let face = self.face_style(style);
      let mut builder = self.lcx.ranged_builder(&mut self.fcx, text, scale, true);
      push_defaults(&mut builder, &stack, style, face);
      let mut line = builder.build(text);
      line.break_all_lines(None);
      line.align(Alignment::Start, AlignmentOptions::default());
      self.lines.insert(key, (line, self.frame));
    }
    let frame = self.frame;
    let (line, last_used) = self.lines.get_mut(&key)?;
    *last_used = frame;
    Some(&*line)
  }

  /// The key a glyph's variable-font coordinates are stored under (0: none)
  fn coords_key(&mut self, coords: Vec<i16>) -> u64 {
    if coords.is_empty() {
      return 0;
    }
    let key = hash_of(&coords).max(1);
    self.coords.entry(key).or_insert_with(|| coords.into());
    key
  }
}

/// One span's layout overrides
struct SpanRun {
  range: Range<usize>,
  brush: u16,
  size: Option<f32>,
  stack: Option<Vec<String>>,
  weight: Option<f32>,
  italic: Option<bool>,
}

impl SpanRun {
  fn push(&self, builder: &mut RangedBuilder<'_, u16>) {
    let range = || self.range.clone();
    builder.push(StyleProperty::Brush(self.brush), range());
    if let Some(size) = self.size {
      builder.push(StyleProperty::FontSize(size), range());
    }
    if let Some(names) = &self.stack {
      builder.push(family_list(names), range());
    }
    if let Some(weight) = self.weight {
      builder.push(StyleProperty::FontWeight(FontWeight::new(weight)), range());
    }
    if let Some(italic) = self.italic {
      builder.push(StyleProperty::FontStyle(font_style(italic)), range());
    }
  }
}

/// Glyphs and icons placed by Parley, with what the ellipsis and the
/// cluster numbering need: per glyph its advance and its cluster's first
/// byte, per icon its byte
#[derive(Default)]
struct Placed {
  glyphs: Vec<PlacedGlyph>,
  advances: Vec<f32>,
  starts: Vec<usize>,
  icons: Vec<PlacedIcon>,
  icon_at: Vec<usize>,
  lines: Vec<LineInfo>,
  /// Each line's left and right content edge
  extents: Vec<(f32, f32)>,
  /// The first byte of every cluster on the kept lines
  cluster_starts: Vec<usize>,
  /// Where the last line's glyphs and icons begin
  last_line: (usize, usize),
}

impl Placed {
  fn push_glyph(&mut self, glyph: PlacedGlyph, advance: f32, start: usize) {
    self.glyphs.push(glyph);
    self.advances.push(advance);
    self.starts.push(start);
  }

  fn pop_glyph(&mut self) {
    self.glyphs.pop();
    self.advances.pop();
    self.starts.pop();
  }

  /// The leftmost line start (0 for no lines)
  fn left(&self) -> f32 {
    self
      .extents
      .iter()
      .map(|&(left, _)| left)
      .reduce(f32::min)
      .unwrap_or(0.0)
  }

  /// The rightmost line end (0 for no lines)
  fn right(&self) -> f32 {
    self
      .extents
      .iter()
      .map(|&(_, right)| right)
      .fold(0.0, f32::max)
  }

  /// Move everything placed `dx` to the left
  fn shift_left(&mut self, dx: f32) {
    if dx == 0.0 {
      return;
    }
    for glyph in &mut self.glyphs {
      glyph.pos.x -= dx;
    }
    for icon in &mut self.icons {
      icon.rect.x -= dx;
    }
    for (left, right) in &mut self.extents {
      *left -= dx;
      *right -= dx;
    }
  }

  /// The right edge of the icons from index `from`
  fn icons_right(&self, from: usize) -> f32 {
    self.icons[from..]
      .iter()
      .map(|icon| icon.rect.x + icon.rect.width)
      .fold(0.0, f32::max)
  }

  /// End the last line in the ellipsis `dots` (laid out alone): drop what
  /// would overflow the wrap width with it, and trailing spaces. Returns
  /// the ellipsis' right edge.
  fn end_with(&mut self, dots: &TextLayout, source: &str, wrap_px: Option<f32>) -> Option<f32> {
    let (&mark, line) = (dots.glyphs.first()?, self.lines.last()?);
    let (line_range_start, line_baseline) = (line.range.start, line.baseline);
    let (first_glyph, first_icon) = self.last_line;
    let dots_width = dots.size.x;
    let limit = wrap_px.map_or(f32::INFINITY, |wrap| wrap - dots_width);
    let line_start = self.glyphs[first_glyph..]
      .first()
      .map_or(0.0, |glyph| glyph.pos.x);

    while self.glyphs.len() > first_glyph {
      let i = self.glyphs.len() - 1;
      let blank = first_char(source, self.starts[i]).is_some_and(char::is_whitespace);
      if !blank && self.glyphs[i].pos.x + self.advances[i] <= limit {
        break;
      }
      self.pop_glyph();
    }
    let mut i = first_icon;
    while i < self.icons.len() {
      if self.icons[i].rect.x + self.icons[i].rect.width > limit {
        self.icons.remove(i);
        self.icon_at.remove(i);
      } else {
        i += 1;
      }
    }

    let kept = self.glyphs.len() > first_glyph || self.icons.len() > first_icon;
    let pen = if kept {
      self.glyphs[first_glyph..]
        .iter()
        .zip(&self.advances[first_glyph..])
        .map(|(glyph, advance)| glyph.pos.x + advance)
        .fold(self.icons_right(first_icon), f32::max)
    } else {
      line_start
    };
    // It takes the look and the cluster of the character it follows
    let (style, start) = match self.glyphs[first_glyph..].last() {
      Some(glyph) => (
        glyph.style,
        self.starts.last().copied().unwrap_or(line_range_start),
      ),
      None => (0, line_range_start),
    };
    let dots_baseline = dots.lines.first().map_or(mark.pos.y, |line| line.baseline);
    self.push_glyph(
      PlacedGlyph {
        pos: Vec2::new(pen + mark.pos.x, line_baseline + mark.pos.y - dots_baseline),
        style,
        ..mark
      },
      dots_width,
      start,
    );
    // The clusters after it were dropped or are trailing spaces
    self.cluster_starts.retain(|&s| s <= start);
    if !self.cluster_starts.contains(&start) {
      self.cluster_starts.push(start);
    }
    Some(pen + dots_width)
  }

  /// Number the clusters in logical order, give each glyph and icon its
  /// cluster, and return the count. An icon shares the cluster after it; an
  /// icon with none after it gets a cluster of its own, so a reveal shows it.
  fn number_clusters(&mut self) -> u32 {
    self.cluster_starts.sort_unstable();
    self.cluster_starts.dedup();
    let starts = &self.cluster_starts;
    let cluster_of = |byte: usize| starts.partition_point(|&s| s < byte);
    for (glyph, &start) in self.glyphs.iter_mut().zip(&self.starts) {
      glyph.cluster = to_u32(cluster_of(start));
    }
    let mut count = starts.len();
    for (icon, &at) in self.icons.iter_mut().zip(&self.icon_at) {
      let cluster = cluster_of(at);
      if cluster < starts.len() {
        icon.cluster = to_u32(cluster);
      } else {
        icon.cluster = to_u32(count);
        count += 1;
      }
    }
    to_u32(count)
  }
}

/// A bitmap font at one size
struct Grid<'a> {
  face: &'a BitmapFace,
  /// Cell advance and height, layout pixels
  advance: f32,
  size: f32,
  question: Option<u16>,
}

impl<'a> Grid<'a> {
  fn new(face: &'a BitmapFace, size_px: f32) -> Self {
    let k = size_px / face.cell.1 as f32;
    let mut grid = Self {
      face,
      advance: face.cell.0 as f32 * k,
      size: face.cell.1 as f32 * k,
      question: None,
    };
    grid.question = grid.cell('?');
    grid
  }

  fn cell(&self, c: char) -> Option<u16> {
    self
      .face
      .chars
      .iter()
      .position(|&x| x == c)
      .and_then(|i| u16::try_from(i).ok())
  }

  fn glyph(&self, font: FontId, cell: u16, pos: Vec2, style: u16, cluster: u32) -> PlacedGlyph {
    PlacedGlyph {
      font,
      glyph: cell,
      pos,
      size: self.size,
      style,
      cluster,
      skew: false,
      embolden: false,
      coords: 0,
    }
  }
}

/// One item of a bitmap font's line
struct Cell {
  kind: CellKind,
  byte: usize,
  cluster: u32,
  width: f32,
  space: bool,
}

enum CellKind {
  /// A character: its cell, or `None` for a blank (a space the font lacks)
  Glyph {
    cell: Option<u16>,
    style: u16,
  },
  Icon {
    index: usize,
    size: Vec2,
  },
  Break,
}

impl Cell {
  fn icon(index: usize, byte: usize, cluster: u32, size: Vec2) -> Self {
    Self {
      kind: CellKind::Icon { index, size },
      byte,
      cluster,
      width: size.x,
      space: false,
    }
  }
}

/// One cell per character (each its own cluster) and icon, in logical
/// order; the cluster count (an icon after the last character has a
/// cluster of its own); the characters the font lacks
fn grid_cells(
  grid: &Grid<'_>,
  text: &RichText,
  size: f32,
  scale: f32,
) -> (Vec<Cell>, u32, Vec<char>) {
  let source = text.text.as_str();
  let style_at = |byte: usize| {
    text
      .spans
      .iter()
      .position(|mark| mark.range.contains(&byte))
      .and_then(|i| u16::try_from(i + 1).ok())
      .unwrap_or(0)
  };
  let icon_px = |i: usize| icon_size(&text.icons[i].icon, size) * scale;
  let mut order: Vec<usize> = (0..text.icons.len())
    .filter(|&i| text.icons[i].at <= source.len())
    .collect();
  order.sort_by_key(|&i| text.icons[i].at);
  let mut pending = order.into_iter().peekable();

  let mut cells = Vec::with_capacity(source.len());
  let mut missing = Vec::new();
  let mut count = 0;
  for (cluster, (byte, c)) in source.char_indices().enumerate() {
    let cluster = to_u32(cluster);
    count = cluster + 1;
    while let Some(i) = pending.next_if(|&i| text.icons[i].at <= byte) {
      cells.push(Cell::icon(i, byte, cluster, icon_px(i)));
    }
    if c == '\n' {
      cells.push(Cell {
        kind: CellKind::Break,
        byte,
        cluster,
        width: 0.0,
        space: false,
      });
      continue;
    }
    let cell = match grid.cell(c) {
      Some(cell) => Some(cell),
      None if c.is_whitespace() => None,
      None if c.is_control() => continue,
      None => {
        missing.push(c);
        match grid.question {
          Some(cell) => Some(cell),
          None => continue,
        }
      }
    };
    cells.push(Cell {
      kind: CellKind::Glyph {
        cell,
        style: style_at(byte),
      },
      byte,
      cluster,
      width: grid.advance,
      space: c.is_whitespace(),
    });
  }
  for i in pending {
    cells.push(Cell::icon(i, source.len(), count, icon_px(i)));
    count += 1;
  }
  (cells, count, missing)
}

/// Rows of cells: a break at each newline, and at the last space before
/// the wrap width (mid-word when a word is wider than it)
fn break_rows(cells: &[Cell], wrap_px: Option<f32>) -> Vec<Range<usize>> {
  let mut rows = Vec::new();
  let (mut start, mut pen, mut space, mut i) = (0, 0.0, None, 0);
  while i < cells.len() {
    if matches!(cells[i].kind, CellKind::Break) {
      rows.push(start..i);
      (start, pen, space) = (i + 1, 0.0, None);
      i += 1;
      continue;
    }
    if let Some(wrap) = wrap_px
      && i > start
      && pen + cells[i].width > wrap
    {
      // A space that overflows breaks the line itself
      let at = if cells[i].space { Some(i) } else { space };
      space = None;
      match at {
        Some(at) => {
          rows.push(start..at);
          start = at + 1;
          i = i.max(start);
          pen = row_width(cells, &(start..i));
        }
        None => {
          rows.push(start..i);
          (start, pen) = (i, 0.0);
        }
      }
      continue;
    }
    if cells[i].space {
      space = Some(i);
    }
    pen += cells[i].width;
    i += 1;
  }
  rows.push(start..cells.len());
  rows
}

/// Each row's byte range: it runs to where the next row starts
fn row_ranges(cells: &[Cell], rows: &[Range<usize>], len: usize) -> Vec<Range<usize>> {
  let first_byte = |row: &Range<usize>| cells.get(row.start).map_or(len, |cell| cell.byte);
  rows
    .iter()
    .enumerate()
    .map(|(r, row)| {
      let from = if r == 0 { 0 } else { first_byte(row) };
      let to = rows.get(r + 1).map_or(len, first_byte);
      from..to.max(from)
    })
    .collect()
}

fn row_width(cells: &[Cell], row: &Range<usize>) -> f32 {
  cells[row.clone()].iter().map(|cell| cell.width).sum()
}

/// `styles[0]` is the base style; `styles[i + 1]` the base with span `i`'s
/// colour, outline and effect
fn glyph_styles(text: &RichText, style: &TextStyle) -> Vec<GlyphStyle> {
  let base = GlyphStyle {
    color: style.color,
    outline: style.outline,
    shadow: style.shadow,
    glow: style.glow,
    effect: None,
  };
  let mut styles = Vec::with_capacity(text.spans.len() + 1);
  styles.push(base.clone());
  styles.extend(text.spans.iter().map(|mark| GlyphStyle {
    color: mark.span.color.unwrap_or(base.color),
    outline: mark.span.outline.or(base.outline),
    effect: mark.span.effect.clone(),
    ..base.clone()
  }));
  styles
}

fn first_char(source: &str, byte: usize) -> Option<char> {
  source.get(byte..).and_then(|rest| rest.chars().next())
}

/// Whether a bitmap font gives `c` a cell (its own, a blank or '?')
fn takes_cell(face: &BitmapFace, c: char) -> bool {
  if c == '\n' {
    return false;
  }
  face.chars.contains(&c) || c.is_whitespace() || (!c.is_control() && face.chars.contains(&'?'))
}

/// A bitmap font's cell advance in text units at `size`
fn bitmap_advance(face: &BitmapFace, size: f32) -> f32 {
  face.cell.0 as f32 * size / face.cell.1 as f32
}

fn bitmap_caret(face: &BitmapFace, text: &str, byte: usize, size: f32) -> f32 {
  let cells = text
    .char_indices()
    .take_while(|&(at, _)| at < byte)
    .filter(|&(_, c)| takes_cell(face, c))
    .count();
  cells as f32 * bitmap_advance(face, size)
}

fn bitmap_byte_at(face: &BitmapFace, text: &str, x: f32, size: f32) -> usize {
  let advance = bitmap_advance(face, size);
  let mut pen = 0.0;
  for (byte, c) in text.char_indices() {
    if !takes_cell(face, c) {
      continue;
    }
    if x < pen + advance / 2.0 {
      return byte;
    }
    pen += advance;
  }
  text.len()
}

fn font_context() -> FontContext {
  FontContext {
    collection: Collection::new(CollectionOptions {
      shared: false,
      system_fonts: false,
    }),
    source_cache: SourceCache::default(),
  }
}

/// The base style's properties; `face` is the weight and slant to ask for
fn push_defaults(
  builder: &mut RangedBuilder<'_, u16>,
  stack: &[String],
  style: &TextStyle,
  (weight, italic): (f32, bool),
) {
  builder.push_default(family_list(stack));
  builder.push_default(StyleProperty::FontSize(style.size));
  builder.push_default(StyleProperty::FontWeight(FontWeight::new(weight)));
  builder.push_default(StyleProperty::FontStyle(font_style(italic)));
  builder.push_default(StyleProperty::Brush(0u16));
  builder.push_default(LineHeight::FontSizeRelative(style.line_height));
  builder.push_default(StyleProperty::LetterSpacing(style.letter_spacing));
  // A word wider than the wrap width breaks inside it, as on a bitmap grid
  builder.push_default(StyleProperty::OverflowWrap(OverflowWrap::Anywhere));
}

fn family_list(names: &[String]) -> StyleProperty<'_, u16> {
  StyleProperty::FontFamily(FontFamily::List(Cow::Owned(
    names
      .iter()
      .map(|name| FontFamilyName::Named(Cow::Borrowed(name.as_str())))
      .collect(),
  )))
}

fn font_style(italic: bool) -> FontStyle {
  if italic {
    FontStyle::Italic
  } else {
    FontStyle::Normal
  }
}

fn alignment(align: TextAlign) -> Alignment {
  match align {
    TextAlign::Start => Alignment::Start,
    TextAlign::Center => Alignment::Center,
    TextAlign::End => Alignment::End,
    TextAlign::Justify => Alignment::Justify,
  }
}

fn icon_size(icon: &Icon, line: f32) -> Vec2 {
  icon
    .size()
    .filter(|size| size.is_finite() && size.x >= 0.0 && size.y >= 0.0)
    .unwrap_or(Vec2::splat(line))
}

fn is_text_range(text: &str, range: &Range<usize>) -> bool {
  range.start < range.end
    && range.end <= text.len()
    && text.is_char_boundary(range.start)
    && text.is_char_boundary(range.end)
}

fn valid_scale(scale: f32) -> bool {
  scale.is_finite() && scale > 0.0
}

fn hash_of(value: &impl Hash) -> u64 {
  let mut h = DefaultHasher::new();
  value.hash(&mut h);
  h.finish()
}

fn to_u32(value: usize) -> u32 {
  u32::try_from(value).unwrap_or(u32::MAX)
}

#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use super::*;
  use crate::core::text::{FontRole, Fonts, Span};
  use crate::core::ui::Color;

  fn engine() -> (TextEngine, Fonts) {
    let fonts = Fonts::with_builtins();
    let mut engine = TextEngine::new();
    engine.sync(&fonts);
    (engine, fonts)
  }

  fn layout(
    engine: &mut TextEngine,
    text: impl Into<RichText>,
    style: &TextStyle,
  ) -> Arc<TextLayout> {
    engine
      .layout(&text.into(), style, &TextLayoutOptions::default(), 1.0)
      .expect("a layout")
  }

  #[test]
  fn plain_text_lays_out_one_glyph_per_letter() {
    let (mut engine, _) = engine();
    let laid = layout(&mut engine, "Hello", &TextStyle::default());
    assert_eq!(laid.glyphs.len(), 5);
    assert_eq!(laid.lines.len(), 1);
    assert!(
      laid.size.x > 20.0 && laid.size.x < 60.0,
      "width {}",
      laid.size.x
    );
    assert!(
      (laid.size.y - 14.0 * 1.2).abs() < 1.5,
      "height {}",
      laid.size.y
    );
    let mut xs: Vec<f32> = laid.glyphs.iter().map(|g| g.pos.x).collect();
    let sorted = {
      let mut s = xs.clone();
      s.sort_by(f32::total_cmp);
      s
    };
    assert_eq!(xs, sorted, "glyphs run left to right");
    xs.dedup();
    assert_eq!(xs.len(), 5);
  }

  #[test]
  fn the_cache_reuses_a_layout() {
    let (mut engine, _) = engine();
    let style = TextStyle::default();
    let a = layout(&mut engine, "cached", &style);
    let b = layout(&mut engine, "cached", &style);
    assert!(Arc::ptr_eq(&a, &b));
    let stats = engine.stats();
    assert_eq!((stats.hits, stats.misses), (1, 1));
    let c = layout(&mut engine, "cached", &style.clone().size(20.0));
    assert!(
      !Arc::ptr_eq(&a, &c),
      "a different style is a different layout"
    );
  }

  #[test]
  fn a_font_registration_invalidates_cached_layouts() {
    let mut fonts = Fonts::new();
    let inter = fonts
      .add_bytes(include_bytes!("../../assets/fonts/Inter-Regular.ttf").to_vec())
      .unwrap();
    fonts.set_role(FontRole::Ui, inter);
    let mut engine = TextEngine::new();
    engine.sync(&fonts);
    let style = TextStyle::default().family("JetBrains Mono");
    let before = layout(&mut engine, "abc", &style);
    assert!(
      before.glyphs.iter().all(|g| g.font == inter),
      "falls back to the UI font"
    );

    let mono = fonts
      .add_bytes(include_bytes!("../../assets/fonts/JetBrainsMono-Regular.ttf").to_vec())
      .unwrap();
    engine.sync(&fonts);
    let after = layout(&mut engine, "abc", &style);
    assert!(!Arc::ptr_eq(&before, &after));
    assert!(
      after.glyphs.iter().all(|g| g.font == mono),
      "now uses the new font"
    );
  }

  #[test]
  fn spans_set_colour_and_size_per_run() {
    let (mut engine, _) = engine();
    let red = Color::rgb(255, 0, 0);
    let text = RichText::new("ab").span("cd", Span::color(red).with_size(28.0));
    let laid = layout(&mut engine, text, &TextStyle::default());
    assert_eq!(laid.glyphs.len(), 4);
    let styles: Vec<Color> = laid
      .glyphs
      .iter()
      .map(|g| laid.styles[g.style as usize].color)
      .collect();
    assert_eq!(styles, vec![Color::WHITE, Color::WHITE, red, red]);
    assert_eq!(laid.glyphs[3].size, 28.0);
    assert!(laid.size.y > 28.0, "the line grows to the big span");
  }

  #[test]
  fn wrapping_breaks_lines_and_alignment_moves_them() {
    let (mut engine, _) = engine();
    let options = TextLayoutOptions::default().wrap(60.0);
    let text: RichText = "one two three four five".into();
    let start = engine
      .layout(&text, &TextStyle::default(), &options, 1.0)
      .unwrap();
    assert!(start.lines.len() >= 3);
    assert!(start.size.x <= 60.5);
    let centred = engine
      .layout(
        &text,
        &TextStyle::default(),
        &options.align(TextAlign::Center),
        1.0,
      )
      .unwrap();
    let first_x = |l: &TextLayout| l.glyphs.iter().map(|g| g.pos.x).fold(f32::MAX, f32::min);
    assert!(first_x(&centred) > first_x(&start));
  }

  #[test]
  fn max_lines_with_ellipsis_ends_in_an_ellipsis() {
    let (mut engine, _) = engine();
    let options = TextLayoutOptions::default()
      .wrap(60.0)
      .max_lines(1)
      .overflow(TextOverflow::Ellipsis);
    let laid = engine
      .layout(
        &"one two three four".into(),
        &TextStyle::default(),
        &options,
        1.0,
      )
      .unwrap();
    assert_eq!(laid.lines.len(), 1);
    assert!(laid.size.x <= 60.5);
    let ellipsis = engine.glyph_for(FontRole::Ui, '\u{2026}').unwrap();
    assert_eq!(laid.glyphs.last().map(|g| g.glyph), Some(ellipsis));
  }

  #[test]
  fn an_inline_icon_gets_a_line_high_box() {
    let (mut engine, _) = engine();
    let text = RichText::new("Press ")
      .icon_named("a_button")
      .plain(" to jump");
    let laid = layout(&mut engine, text, &TextStyle::default());
    assert_eq!(laid.icons.len(), 1);
    let icon = &laid.icons[0];
    assert!(
      (icon.rect.height - 14.0).abs() < 1.0,
      "line-high: {}",
      icon.rect.height
    );
    assert!(icon.rect.x > 20.0, "after 'Press '");
  }

  #[test]
  fn a_character_no_font_has_lays_out_as_tofu_and_warns_once() {
    let (mut engine, _) = engine();
    let style = TextStyle::default();
    let laid = layout(&mut engine, "a\u{6F22}b", &style);
    assert_eq!(laid.glyphs.len(), 3);
    assert_eq!(laid.glyphs[1].glyph, 0, "the .notdef (tofu) glyph");
    let _ = layout(&mut engine, "x\u{6F22}y", &style);
    let warnings = crate::core::dev_log::get_entries()
      .iter()
      .filter(|e| e.message.contains("U+6F22") && e.message.contains("Inter"))
      .count();
    assert_eq!(warnings, 1);
  }

  #[test]
  fn degenerate_sizes_lay_out_nothing() {
    let (mut engine, _) = engine();
    for size in [0.0, -4.0, f32::NAN, f32::INFINITY] {
      let style = TextStyle::default().size(size);
      assert!(
        engine
          .layout(&"x".into(), &style, &TextLayoutOptions::default(), 1.0)
          .is_none()
      );
    }
    let bad_wrap = TextLayoutOptions::default().wrap(f32::NAN);
    assert!(
      engine
        .layout(&"x".into(), &TextStyle::default(), &bad_wrap, 1.0)
        .is_none()
    );
    assert!(
      engine
        .layout(
          &"x".into(),
          &TextStyle::default(),
          &TextLayoutOptions::default(),
          0.0
        )
        .is_none()
    );
    assert_eq!(
      engine.measure(
        &"x".into(),
        &TextStyle::default().size(f32::NAN),
        &TextLayoutOptions::default(),
        1.0
      ),
      Vec2::ZERO
    );

    // Each bad style (and the bad wrap and scale) warned once: laying them
    // all out again adds no warning
    assert_eq!(engine.warned.len(), 6);
    for size in [0.0, -4.0, f32::NAN, f32::INFINITY] {
      let style = TextStyle::default().size(size);
      let _ = engine.layout(&"y".into(), &style, &TextLayoutOptions::default(), 1.0);
    }
    let _ = engine.layout(&"y".into(), &TextStyle::default(), &bad_wrap, 1.0);
    assert_eq!(engine.warned.len(), 6);
    assert!(crate::core::dev_log::get_entries().iter().any(|e| {
      e.message
        .contains("text style with size -4 cannot be laid out")
    }));
  }

  #[test]
  fn carets_and_hit_tests_agree() {
    let (mut engine, _) = engine();
    let style = TextStyle::default();
    let text = "héllo wörld";
    let mut last = -1.0;
    for (byte, _) in text.char_indices() {
      let x = engine.caret_x(text, &style, byte, 1.0);
      assert!(x > last, "carets move right");
      last = x;
      assert_eq!(engine.byte_at(text, &style, x + 0.1, 1.0), byte);
    }
    let (from, to) = engine.selection_x(text, &style, 0..2, 1.0);
    assert!(from < to);
  }

  #[test]
  fn wrap_lines_returns_byte_ranges() {
    let (mut engine, _) = engine();
    let text = "alpha beta gamma delta";
    let lines = engine.wrap_lines(text, &TextStyle::default(), 50.0, 1.0);
    assert!(lines.len() >= 2);
    assert_eq!(lines.first().map(|r| r.start), Some(0));
    assert_eq!(lines.last().map(|r| r.end), Some(text.len()));
  }

  #[test]
  fn scale_lays_out_in_layout_pixels_and_measure_returns_units() {
    let (mut engine, _) = engine();
    let style = TextStyle::default();
    let one = engine
      .layout(&"Scale".into(), &style, &TextLayoutOptions::default(), 1.0)
      .unwrap();
    let two = engine
      .layout(&"Scale".into(), &style, &TextLayoutOptions::default(), 2.0)
      .unwrap();
    assert!((two.size.x - 2.0 * one.size.x).abs() < 2.0);
    assert_eq!(two.glyphs[0].size, 28.0);
    let units = engine.measure(&"Scale".into(), &style, &TextLayoutOptions::default(), 2.0);
    assert!((units.x - one.size.x).abs() < 1.0);
  }

  #[test]
  fn a_bitmap_font_lays_out_on_its_grid() {
    let mut fonts = Fonts::with_builtins();
    let sheet = crate::core::asset::SpriteHandle::from_sprite(
      crate::core::graphics::Sprite::solid_rect(24, 8, 255, 255, 255, 255),
      "grid",
    );
    fonts.add_bitmap("Grid", sheet, (8, 8), "ABC").unwrap();
    let mut engine = TextEngine::new();
    engine.sync(&fonts);
    let style = TextStyle::default().family("Grid").size(16.0);
    let laid = layout(&mut engine, "ABCA", &style);
    assert!(laid.pixel);
    let ids: Vec<u16> = laid.glyphs.iter().map(|g| g.glyph).collect();
    assert_eq!(ids, vec![0, 1, 2, 0]);
    let xs: Vec<f32> = laid.glyphs.iter().map(|g| g.pos.x).collect();
    assert_eq!(xs, vec![0.0, 16.0, 32.0, 48.0]);
  }

  #[test]
  fn unused_layouts_are_dropped_and_used_ones_kept() {
    let (mut engine, _) = engine();
    let style = TextStyle::default();
    let kept = layout(&mut engine, "kept", &style);
    let _ = layout(&mut engine, "dropped", &style);
    for _ in 0..CACHE_FRAMES + SWEEP_FRAMES {
      let again = layout(&mut engine, "kept", &style);
      assert!(Arc::ptr_eq(&kept, &again));
      engine.end_frame();
    }
    let misses = engine.stats().misses;
    let _ = layout(&mut engine, "dropped", &style);
    assert_eq!(engine.stats().misses, misses + 1, "laid out again");
  }

  #[test]
  fn line_height_is_the_size_times_the_multiple_in_text_units() {
    let (engine, _) = engine();
    let style = TextStyle::default().size(20.0).line_height(1.5);
    assert!((engine.line_height(&style, 1.0) - 30.0).abs() < 1e-4);
    assert!((engine.line_height(&style, 3.0) - 30.0).abs() < 1e-4);
    assert_eq!(engine.line_height(&style.clone().size(f32::NAN), 1.0), 0.0);
  }

  #[test]
  fn static_fonts_carry_no_variation_coordinates() {
    let (mut engine, _) = engine();
    let laid = layout(
      &mut engine,
      RichText::new("a").span("b", Span::bold()),
      &TextStyle::default(),
    );
    assert!(laid.glyphs.iter().all(|g| g.coords == 0));
    assert!(engine.coords(0).is_none());
    assert_ne!(
      laid.glyphs[1].font, laid.glyphs[0].font,
      "bold uses Inter Bold"
    );
    assert!(
      !laid.glyphs[1].embolden,
      "a real bold face is not synthesized"
    );
  }

  fn bitmap_engine() -> TextEngine {
    let mut fonts = Fonts::with_builtins();
    let sheet = crate::core::asset::SpriteHandle::from_sprite(
      crate::core::graphics::Sprite::solid_rect(40, 8, 255, 255, 255, 255),
      "grid",
    );
    fonts.add_bitmap("Grid", sheet, (8, 8), "ABC.?").unwrap();
    let mut engine = TextEngine::new();
    engine.sync(&fonts);
    engine
  }

  fn grid_style() -> TextStyle {
    TextStyle::default().family("Grid").size(8.0)
  }

  #[test]
  fn an_outline_and_a_glow_with_the_same_numbers_are_different_layouts() {
    let (mut engine, _) = engine();
    let outlined = layout(
      &mut engine,
      "fx",
      &TextStyle::default().outline(2.0, Color::BLACK),
    );
    let glowing = layout(
      &mut engine,
      "fx",
      &TextStyle::default().glow(2.0, Color::BLACK),
    );
    assert!(!Arc::ptr_eq(&outlined, &glowing));
    assert!(outlined.styles[0].outline.is_some() && outlined.styles[0].glow.is_none());
    assert!(glowing.styles[0].glow.is_some() && glowing.styles[0].outline.is_none());
  }

  #[test]
  fn a_trailing_icon_has_its_own_cluster() {
    let (mut engine, _) = engine();
    let laid = layout(
      &mut engine,
      RichText::new("abc").icon_named("star"),
      &TextStyle::default(),
    );
    assert_eq!(laid.clusters, 4);
    assert_eq!(laid.icons[0].cluster, 3);

    let alone = layout(
      &mut engine,
      RichText::new("").icon_named("star"),
      &TextStyle::default(),
    );
    assert_eq!((alone.clusters, alone.icons[0].cluster), (1, 0));

    let mut grid = bitmap_engine();
    let laid = layout(
      &mut grid,
      RichText::new("ABC").icon_named("star"),
      &grid_style(),
    );
    assert_eq!((laid.clusters, laid.icons[0].cluster), (4, 3));
    let alone = layout(
      &mut grid,
      RichText::new("").icon_named("star"),
      &grid_style(),
    );
    assert_eq!((alone.clusters, alone.icons[0].cluster), (1, 0));
  }

  #[test]
  fn max_lines_measures_only_the_kept_lines() {
    let (mut engine, _) = engine();
    let text: RichText = "OK\nand a much longer second line that is dropped".into();
    let options = TextLayoutOptions::default().max_lines(1);
    let laid = engine
      .layout(&text, &TextStyle::default(), &options, 1.0)
      .unwrap();
    let end = laid.glyphs.iter().map(|g| g.pos.x).fold(0.0, f32::max);
    assert!(
      laid.size.x < 30.0 && laid.size.x > end,
      "width {}",
      laid.size.x
    );

    // Centred without a wrap, the kept line still sits in its own block
    let centred = engine
      .layout(
        &text,
        &TextStyle::default(),
        &options.align(TextAlign::Center),
        1.0,
      )
      .unwrap();
    assert_eq!(centred.glyphs[0].pos.x, 0.0);
    assert!((centred.size.x - laid.size.x).abs() < 0.01);
  }

  #[test]
  fn max_lines_zero_lays_out_an_empty_block() {
    let (mut engine, _) = engine();
    for options in [
      TextLayoutOptions::default().max_lines(0),
      TextLayoutOptions::default()
        .wrap(60.0)
        .align(TextAlign::Center)
        .max_lines(0)
        .overflow(TextOverflow::Ellipsis),
    ] {
      let laid = engine
        .layout(&"some text".into(), &TextStyle::default(), &options, 1.0)
        .unwrap();
      assert_eq!(laid.size, Vec2::ZERO);
      assert!(laid.glyphs.is_empty() && laid.lines.is_empty());
      assert_eq!(laid.clusters, 0);
      let grid = bitmap_engine()
        .layout(&"AB CA".into(), &grid_style(), &options, 1.0)
        .unwrap();
      assert_eq!(grid.size, Vec2::ZERO);
      assert!(grid.glyphs.is_empty() && grid.lines.is_empty());
    }
  }

  #[test]
  fn an_exact_face_lays_out_in_that_face() {
    let (mut engine, fonts) = engine();
    let bold = fonts
      .faces()
      .find(|(_, face)| face.family == "Inter" && face.weight == 700.0)
      .map(|(id, _)| id)
      .unwrap();
    let laid = layout(
      &mut engine,
      "Bold",
      &TextStyle::default().font(TextFont::Id(bold)),
    );
    assert!(laid.glyphs.iter().all(|g| g.font == bold));
    let span = RichText::new("a").span("b", Span::font(TextFont::Id(bold)));
    let laid = layout(&mut engine, span, &TextStyle::default());
    assert_ne!(laid.glyphs[0].font, bold);
    assert_eq!(laid.glyphs[1].font, bold);
  }

  #[test]
  fn a_word_wider_than_the_wrap_breaks_inside_it() {
    let (mut engine, _) = engine();
    let text = "supercalifragilistic";
    let style = TextStyle::default();
    let laid = engine
      .layout(
        &text.into(),
        &style,
        &TextLayoutOptions::default().wrap(40.0),
        1.0,
      )
      .unwrap();
    assert!(laid.lines.len() >= 3);
    for line in &laid.lines {
      let width = engine
        .measure(
          &text[line.range.clone()].into(),
          &style,
          &TextLayoutOptions::default(),
          1.0,
        )
        .x;
      assert!(
        width <= 40.5,
        "line {:?} is {width} wide",
        &text[line.range.clone()]
      );
    }
  }

  // Regression tests for what the layout probes found

  #[test]
  fn clusters_count_characters_in_logical_order() {
    let (mut engine, _) = engine();
    let laid = layout(&mut engine, "h\u{e9}llo w", &TextStyle::default());
    let clusters: Vec<u32> = laid.glyphs.iter().map(|g| g.cluster).collect();
    assert_eq!(clusters, vec![0, 1, 2, 3, 4, 5, 6]);
    assert_eq!(laid.clusters, 7);

    // An icon is revealed with the character after it
    let text = RichText::new("ab").icon_named("star").plain("cd");
    let laid = layout(&mut engine, text, &TextStyle::default());
    let clusters: Vec<u32> = laid.glyphs.iter().map(|g| g.cluster).collect();
    assert_eq!(clusters, vec![0, 1, 2, 3]);
    assert_eq!((laid.icons[0].cluster, laid.clusters), (2, 4));
  }

  #[test]
  fn the_ellipsis_shares_the_cluster_before_it_and_drops_the_rest() {
    let (mut engine, _) = engine();
    let options = TextLayoutOptions::default()
      .wrap(60.0)
      .max_lines(1)
      .overflow(TextOverflow::Ellipsis);
    let laid = engine
      .layout(
        &"one two three four".into(),
        &TextStyle::default(),
        &options,
        1.0,
      )
      .unwrap();
    let n = laid.glyphs.len();
    let (dots, before) = (laid.glyphs[n - 1], laid.glyphs[n - 2]);
    assert_eq!(dots.cluster, before.cluster);
    assert_eq!(
      laid.clusters,
      before.cluster + 1,
      "dropped characters do not count"
    );
    assert!(
      laid.size.x > dots.pos.x + 5.0,
      "the block covers the ellipsis"
    );
    assert!(laid.size.x <= 60.5);

    // Without a wrap, the kept line's newline does not count either
    let options = TextLayoutOptions::default()
      .max_lines(2)
      .overflow(TextOverflow::Ellipsis);
    let laid = engine
      .layout(
        &"line one\nline two\nline three".into(),
        &TextStyle::default(),
        &options,
        1.0,
      )
      .unwrap();
    let dots = laid.glyphs.last().copied().unwrap();
    assert_eq!(
      (dots.cluster, laid.clusters),
      (16, 17),
      "\"line one\\nline two\""
    );
    assert!(
      laid.size.x > dots.pos.x + 5.0,
      "the block covers the ellipsis"
    );
  }

  #[test]
  fn a_wrapped_block_aligned_center_end_or_justify_is_the_wrap_width() {
    let (mut engine, _) = engine();
    let style = TextStyle::default();
    let text = "one two three four five";
    let wrapped = |align| TextLayoutOptions::default().wrap(60.0).align(align);
    for align in [TextAlign::Center, TextAlign::End, TextAlign::Justify] {
      let laid = engine
        .layout(&text.into(), &style, &wrapped(align), 1.0)
        .unwrap();
      assert_eq!(laid.size.x, 60.0, "{align:?}");
    }
    for (align, share) in [(TextAlign::Center, 0.5), (TextAlign::End, 1.0)] {
      let laid = engine
        .layout(&text.into(), &style, &wrapped(align), 1.0)
        .unwrap();
      for line in &laid.lines {
        let content = text[line.range.clone()].trim_end();
        let width = engine
          .measure(&content.into(), &style, &TextLayoutOptions::default(), 1.0)
          .x;
        let left = laid
          .glyphs
          .iter()
          .filter(|g| (g.pos.y - line.baseline).abs() < 0.5)
          .map(|g| g.pos.x)
          .fold(f32::MAX, f32::min);
        let expected = (60.0 - width) * share;
        assert!(
          (left - expected).abs() < 1.0,
          "{align:?} {content:?}: {left} vs {expected}"
        );
      }
    }
  }

  #[test]
  fn empty_text_is_one_empty_line_and_icon_only_text_is_its_icon() {
    let (mut engine, _) = engine();
    let empty = layout(&mut engine, "", &TextStyle::default());
    assert_eq!(empty.size.x, 0.0, "not Parley's stand-in space");
    assert!((empty.size.y - 14.0 * 1.2).abs() < 1.5);
    assert_eq!(empty.lines.len(), 1);
    assert_eq!(empty.lines[0].range, 0..0);
    assert!(empty.glyphs.is_empty());
    assert_eq!(empty.clusters, 0);

    let icon = layout(
      &mut engine,
      RichText::new("").icon_named("star"),
      &TextStyle::default(),
    );
    assert_eq!(icon.size.x, 14.0);
    assert_eq!(
      icon.lines[0].range,
      0..0,
      "Parley's range starts past its end"
    );
    let rect = icon.icons[0].rect;
    assert_eq!((rect.x, rect.width, rect.height), (0.0, 14.0, 14.0));
  }

  #[test]
  fn wrap_lines_cover_the_text_in_order() {
    let (mut engine, _) = engine();
    let style = TextStyle::default();
    for text in ["alpha beta\ngamma", "a\n", "", "alpha beta gamma delta"] {
      let lines = engine.wrap_lines(text, &style, 50.0, 1.0);
      assert!(!lines.is_empty(), "{text:?}");
      assert_eq!(lines[0].start, 0, "{text:?}");
      assert_eq!(lines.last().map(|r| r.end), Some(text.len()), "{text:?}");
      for pair in lines.windows(2) {
        assert_eq!(pair[0].end, pair[1].start, "{text:?}: {lines:?}");
      }
    }
    assert_eq!(
      engine.wrap_lines("alpha beta\ngamma", &style, 1000.0, 1.0),
      vec![0..11, 11..16]
    );
  }

  #[test]
  fn a_bitmap_font_wraps_at_a_space_and_inside_a_long_word() {
    let mut engine = bitmap_engine();
    let wrap = |width| TextLayoutOptions::default().wrap(width);
    let laid = engine
      .layout(&"AB CA BB".into(), &grid_style(), &wrap(40.0), 1.0)
      .unwrap();
    let ranges: Vec<Range<usize>> = laid.lines.iter().map(|l| l.range.clone()).collect();
    assert_eq!(
      ranges,
      vec![0..6, 6..8],
      "the space that overflows breaks the line"
    );
    let xs: Vec<f32> = laid.glyphs.iter().map(|g| g.pos.x).collect();
    assert_eq!(xs, vec![0.0, 8.0, 24.0, 32.0, 0.0, 8.0]);

    let long = engine
      .layout(&"ABCABCABC".into(), &grid_style(), &wrap(20.0), 1.0)
      .unwrap();
    assert_eq!(long.lines.len(), 5);
    assert!(long.glyphs.iter().all(|g| g.pos.x + 8.0 <= 20.0));
    assert_eq!(long.size.x, 16.0);
  }

  #[test]
  fn a_bitmap_font_ends_in_dots_and_aligns_its_lines() {
    let mut engine = bitmap_engine();
    let options = TextLayoutOptions::default()
      .wrap(40.0)
      .max_lines(1)
      .overflow(TextOverflow::Ellipsis);
    let laid = engine
      .layout(&"AB CA BB".into(), &grid_style(), &options, 1.0)
      .unwrap();
    let ids: Vec<u16> = laid.glyphs.iter().map(|g| g.glyph).collect();
    assert_eq!(
      ids,
      vec![0, 1, 3, 3, 3],
      "\"AB...\": the sheet has '.', not '…'"
    );
    assert_eq!(laid.clusters, 2);

    for (align, x) in [(TextAlign::Center, 12.0), (TextAlign::End, 24.0)] {
      let options = TextLayoutOptions::default().wrap(40.0).align(align);
      let laid = engine
        .layout(&"AB CA BB".into(), &grid_style(), &options, 1.0)
        .unwrap();
      assert_eq!(laid.size.x, 40.0);
      assert_eq!(
        laid.glyphs[4].pos.x, x,
        "{align:?}: \"BB\" on the second line"
      );
    }
  }

  #[test]
  fn bitmap_carets_step_a_cell_at_any_scale() {
    let mut engine = bitmap_engine();
    let style = grid_style();
    for scale in [1.0, 2.0] {
      assert_eq!(engine.caret_x("AB C", &style, 2, scale), 16.0);
      assert_eq!(engine.caret_x("AB C", &style, 4, scale), 32.0);
      assert_eq!(engine.byte_at("AB C", &style, 11.0, scale), 1);
      assert_eq!(engine.byte_at("AB C", &style, 13.0, scale), 2);
      assert_eq!(engine.selection_x("AB C", &style, 1..3, scale), (8.0, 24.0));
    }
  }

  #[test]
  fn spans_and_icons_off_a_char_boundary_are_skipped() {
    use crate::core::text::rich::{IconMark, SpanMark};
    let text = RichText {
      text: "h\u{e9}llo".into(),
      spans: vec![SpanMark {
        range: 0..2,
        span: Span::color(Color::BLACK),
      }],
      icons: vec![IconMark {
        at: 2,
        icon: Icon::Named {
          name: "star".into(),
          size: None,
        },
      }],
      pauses: Vec::new(),
    };
    let (mut engine, _) = engine();
    let laid = layout(&mut engine, text.clone(), &TextStyle::default());
    assert_eq!(laid.glyphs.len(), 5);
    assert!(
      laid.glyphs.iter().all(|g| g.style == 0),
      "the span is skipped"
    );
    assert!(laid.icons.is_empty(), "the icon is skipped");
    let grid = bitmap_engine().layout(&text, &grid_style(), &TextLayoutOptions::default(), 1.0);
    assert!(
      grid.is_some(),
      "the bitmap layouter does not slice the text"
    );
  }

  #[test]
  fn carets_selections_and_hit_tests_hold_at_scale_two() {
    let (mut engine, _) = engine();
    let style = TextStyle::default();
    let text = "h\u{e9}llo w\u{f6}rld";
    for (byte, _) in text.char_indices() {
      let one = engine.caret_x(text, &style, byte, 1.0);
      let two = engine.caret_x(text, &style, byte, 2.0);
      assert!((one - two).abs() < 1.0, "byte {byte}: {one} vs {two}");
      assert_eq!(engine.byte_at(text, &style, two + 0.1, 2.0), byte);
    }
    let (a, b) = engine.selection_x(text, &style, 1..5, 1.0);
    let (c, d) = engine.selection_x(text, &style, 1..5, 2.0);
    assert!(c < d);
    assert!((a - c).abs() < 1.0 && (b - d).abs() < 1.0);
  }

  #[test]
  fn an_unknown_family_warns_once() {
    let (mut engine, _) = engine();
    let style = TextStyle::default().family("No Such Family Q7");
    for text in ["abc", "abd", "abe"] {
      let _ = layout(&mut engine, text, &style);
    }
    let _ = engine.caret_x("xyz", &style, 1, 1.0);
    let warnings = crate::core::dev_log::get_entries()
      .iter()
      .filter(|e| e.message.contains("No Such Family Q7"))
      .count();
    assert_eq!(warnings, 1);
  }
}
