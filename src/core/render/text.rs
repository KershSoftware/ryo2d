//! Glyph quads from laid-out text, for world text (one set per camera) and
//! the UI (the UI camera). Three passes per block: shadows, outlines, fills,
//! so one glyph's outline never covers its neighbour's fill.
//!
//! Placement: a block is laid out in layout pixels and drawn into target
//! units (world units for world text). `TextPlacement::origin` is the
//! block's top-left; `axis_x` and `axis_y` are target units per layout
//! pixel along the block's axes (rotation and scale folded in);
//! `px_per_layout` is render pixels per layout pixel, the size raster
//! glyphs are made at. When nothing is rotated, `pixel_grid` puts raster
//! and pixel glyphs on whole render pixels: the pen's fraction of a pixel
//! picks the glyph's subpixel bin, the baseline is rounded, and the image
//! maps 1:1 onto render pixels. MSDF glyphs scale and never snap.

use std::collections::HashSet;
use std::hash::{DefaultHasher, Hash, Hasher};

use glam::Vec2;

use crate::core::asset::SpriteHandle;
use crate::core::ecs::interpolate::render_global;
use crate::core::ecs::{Entity, GlobalTransform, PreviousGlobalTransform, World};
use crate::core::graphics::{GlyphPage, RenderLayers, SpriteGpuRenderer, SpriteInstance};
use crate::core::render::batch::{BatchShader, BlendMode, Filter, Sampling};
use crate::core::render::{
  CameraDraw, DrawGlyph, DrawKind, DrawList, PixelRect, SpriteLook, SpriteShading,
};
use crate::core::text::{
  BitmapFace, FaceSource, FontId, FontRender, Fonts, GlyphContext, GlyphFormat, GlyphKey, GlyphMod,
  GlyphRasterizer, GlyphStyle, Icon, PlacedGlyph, PlacedIcon, RichText, Text2d, TextBounds,
  TextEffects, TextEngine, TextIcons, TextLayout, TextLayoutState, TextReveal, clean_opacity,
  glyph_id, raster_size_step, reveal_alpha,
};
use crate::core::ui::immediate::PromptContext;
use crate::core::ui::widgets::prompt::resolve_prompts;
use crate::core::ui::{Color, Palette, Rect, UiTheme};
use crate::engine_warn;

/// The largest raster or pixel glyph image world text makes, in render
/// pixels: text seen larger draws this image scaled up. A quarter of a glyph
/// page, so even wide glyphs fit and a zooming camera's sizes stay cheap.
pub(crate) const WORLD_GLYPH_PX: f32 = 256.0;

/// The UI's largest glyph image. UI text is mostly still, so a bigger image
/// is one upload, and big titles on HiDPI displays stay sharp.
pub(crate) const UI_GLYPH_PX: f32 = 512.0;

/// The largest MSDF base size: a font asking for more is made at this size
/// (a field scales freely, and a page must hold every glyph)
const MSDF_MAX_BASE: f32 = 256.0;

/// What the text reads
pub(crate) struct TextSources<'a> {
  pub(crate) fonts: &'a Fonts,
  pub(crate) engine: &'a TextEngine,
  pub(crate) effects: &'a TextEffects,
  pub(crate) icons: &'a TextIcons,
}

/// What the text writes into
pub(crate) struct TextTargets<'a> {
  pub(crate) raster: &'a mut GlyphRasterizer,
  pub(crate) sprites: &'a mut SpriteGpuRenderer,
  pub(crate) device: &'a wgpu::Device,
  pub(crate) queue: &'a wgpu::Queue,
  pub(crate) list: &'a mut DrawList,
  /// Warn-once keys (glow on a raster font, unknown icons, non-finite
  /// transforms, capped glyphs)
  pub(crate) warned: &'a mut HashSet<u64>,
}

/// The draw-list side of a block
#[derive(Debug, Clone, Copy)]
pub(crate) struct TextDraw {
  pub(crate) kind: DrawKind,
  pub(crate) z: i32,
  pub(crate) lit: bool,
  pub(crate) only_camera: Option<Entity>,
  pub(crate) layers: RenderLayers,
  /// The UI clip it is drawn under, as the UI renderer clips it (the UI's
  /// text cache keys texts by it); `None` for world text
  pub(crate) scissor: Option<PixelRect>,
  pub(crate) blend: BlendMode,
}

/// Where and how a block draws (see the module docs)
#[derive(Debug, Clone, Copy)]
pub(crate) struct TextPlacement {
  pub(crate) origin: Vec2,
  pub(crate) axis_x: Vec2,
  pub(crate) axis_y: Vec2,
  pub(crate) px_per_layout: f32,
  /// Raster sizes snap to `raster_size_step` (world text, so a zooming
  /// camera reuses glyphs); the UI's are exact
  pub(crate) size_steps: bool,
  /// The largest raster or pixel glyph image made, in render pixels: larger
  /// text draws that image scaled up (`WORLD_GLYPH_PX`, `UI_GLYPH_PX`)
  pub(crate) max_glyph_px: f32,
  /// Target point of render pixel (0, 0) and render pixels per target unit
  pub(crate) pixel_grid: Option<(Vec2, f32)>,
  pub(crate) draw: TextDraw,
  /// Seconds, for effects
  pub(crate) time: f32,
  /// Clusters shown and the fade length in clusters (`None`: all shown)
  pub(crate) reveal: Option<(f32, f32)>,
  pub(crate) seed: u64,
  pub(crate) opacity: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layer {
  Shadow,
  Outline,
  Fill,
}

/// Lay a block's glyphs and icons into the draw list
pub(crate) fn emit_text(
  sources: &TextSources<'_>,
  targets: &mut TextTargets<'_>,
  layout: &TextLayout,
  place: &TextPlacement,
) {
  emit_glyphs(sources, targets, layout, place);
  emit_icons(sources, targets, layout, place);
}

/// Lay a block's glyphs into the draw list: its shadows, outlines, then
/// fills (a bitmap font's cells are sprites)
pub(crate) fn emit_glyphs(
  sources: &TextSources<'_>,
  targets: &mut TextTargets<'_>,
  layout: &TextLayout,
  place: &TextPlacement,
) {
  if !(place.px_per_layout.is_finite() && place.px_per_layout > 0.0) {
    return;
  }
  let mut block = Block {
    sources,
    targets,
    layout,
    place,
  };
  let mut effects = EffectRun::new(place);
  for layer in [Layer::Shadow, Layer::Outline, Layer::Fill] {
    for (index, glyph) in layout.glyphs.iter().enumerate() {
      let at = Glyph {
        index: u32::try_from(index).unwrap_or(u32::MAX),
        glyph,
      };
      emit_glyph(&mut block, at, layer, &mut effects);
    }
  }
}

/// Lay a block's inline icons into the draw list
pub(crate) fn emit_icons(
  sources: &TextSources<'_>,
  targets: &mut TextTargets<'_>,
  layout: &TextLayout,
  place: &TextPlacement,
) {
  if !(place.px_per_layout.is_finite() && place.px_per_layout > 0.0) {
    return;
  }
  let mut block = Block {
    sources,
    targets,
    layout,
    place,
  };
  for icon in &layout.icons {
    emit_icon(&mut block, icon);
  }
}

/// A block being emitted: what it reads, where it goes, its layout and
/// placement
struct Block<'b, 's, 't> {
  sources: &'b TextSources<'s>,
  targets: &'b mut TextTargets<'t>,
  layout: &'b TextLayout,
  place: &'b TextPlacement,
}

/// A glyph and its index in its block
#[derive(Clone, Copy)]
struct Glyph<'a> {
  index: u32,
  glyph: &'a PlacedGlyph,
}

/// A block's effect state: one context, its args buffer reused for every
/// glyph, and the last effect found unknown, not asked for again in the
/// block (so an unknown name costs one lookup per block, not per glyph)
struct EffectRun<'l> {
  context: GlyphContext,
  unknown: Option<&'l str>,
}

impl EffectRun<'_> {
  fn new(place: &TextPlacement) -> Self {
    Self {
      context: GlyphContext::new(0, 0, 0, place.time, 0.0, place.seed),
      unknown: None,
    }
  }
}

/// A glyph's effect and opacity; `None` when it is not seen
fn modifier<'l>(
  sources: &TextSources<'_>,
  layout: &TextLayout,
  place: &TextPlacement,
  at: Glyph<'_>,
  style: &'l GlyphStyle,
  effects: &mut EffectRun<'l>,
) -> Option<(GlyphMod, f32)> {
  let glyph = at.glyph;
  let mut alpha = place.opacity;
  if let Some((shown, fade)) = place.reveal {
    alpha *= reveal_alpha(glyph.cluster, shown, fade);
  }
  if alpha.is_nan() || alpha <= 0.0 {
    return None;
  }
  let modified = match style.effect.as_deref() {
    Some(effect)
      if effects
        .unknown
        .is_some_and(|name| std::ptr::eq(name, effect)) =>
    {
      GlyphMod::default()
    }
    Some(effect) => {
      let context = &mut effects.context;
      context.index = at.index;
      context.cluster = glyph.cluster;
      context.size = glyph.size / layout.scale;
      sources.effects.apply(effect, context).unwrap_or_else(|| {
        effects.unknown = Some(effect);
        GlyphMod::default()
      })
    }
    None => GlyphMod::default(),
  };
  let alpha = (alpha * modified.alpha).min(1.0);
  (alpha > 0.0).then_some((modified, alpha))
}

/// A glyph image's box about its pen, in image pixels: from the pen to its
/// left edge, from the baseline up to its top edge, and its size
#[derive(Debug, Clone, Copy, PartialEq)]
struct ImageBox {
  left: f32,
  top: f32,
  width: f32,
  height: f32,
}

/// How a quad samples and shades
#[derive(Debug, Clone, Copy)]
struct Shade {
  tint: Color,
  alpha: f32,
  params: [f32; 4],
  extra: [f32; 4],
  sampling: Sampling,
}

impl Shade {
  fn tinted(tint: Color, alpha: f32, sampling: Sampling) -> Self {
    Self {
      tint,
      alpha,
      params: [0.0; 4],
      extra: [0.0; 4],
      sampling,
    }
  }
}

/// One glyph quad to place: its image, where its pen is, and how it maps
#[derive(Debug, Clone, Copy)]
struct GlyphQuad {
  key: GlyphKey,
  /// The pen, layout pixels
  pen: Vec2,
  /// Layout pixels per image pixel
  k: f32,
  /// Render pixels per image pixel, on whole render pixels when the block
  /// is on the pixel grid (raster and pixel glyphs); `None` scales by `k`
  /// (MSDF glyphs, and raster glyphs drawn from a capped image)
  texel: Option<f32>,
  /// Moved by this many render pixels (pixel-font outlines)
  nudge: Vec2,
  shade: Shade,
}

/// A glyph's colour, opacity, scale and pen after its effect
#[derive(Debug, Clone, Copy)]
struct GlyphLook {
  color: Color,
  alpha: f32,
  scale: f32,
  /// Layout pixels
  pen: Vec2,
}

fn emit_glyph<'b>(
  block: &mut Block<'b, '_, '_>,
  at: Glyph<'_>,
  layer: Layer,
  effects: &mut EffectRun<'b>,
) {
  let glyph = at.glyph;
  let layout: &'b TextLayout = block.layout;
  let Some(style) = layout.styles.get(usize::from(glyph.style)) else {
    return;
  };
  // Nothing to draw in this layer: skip before running the effect
  let empty = match layer {
    Layer::Shadow => style.shadow.is_none() && style.glow.is_none(),
    Layer::Outline => style.outline.is_none(),
    Layer::Fill => false,
  };
  if empty {
    return;
  }
  let (sources, layout, place) = (block.sources, block.layout, block.place);
  let Some((modified, alpha)) = modifier(sources, layout, place, at, style, effects) else {
    return;
  };
  let look = GlyphLook {
    color: modified.color.unwrap_or(style.color),
    alpha,
    scale: modified.scale,
    pen: glyph.pos + modified.offset * layout.scale,
  };
  let Some(face) = sources.fonts.face(glyph.font) else {
    return;
  };
  if let FaceSource::Bitmap(bitmap) = &face.source {
    emit_bitmap(block, glyph, style, &look, layer, bitmap);
    return;
  }
  match sources.engine.face_render(glyph.font) {
    FontRender::Raster => emit_raster(block, glyph, style, &look, layer),
    FontRender::Pixel { design_size } => {
      emit_pixel(block, glyph, style, &look, layer, design_size);
    }
    FontRender::Msdf { base, range } => {
      emit_msdf(block, glyph, style, &look, layer, (base, range));
    }
  }
}

/// Warn once that glow needs an MSDF font
fn warn_glow(warned: &mut HashSet<u64>, style: &GlyphStyle) {
  let Some(glow) = style.glow else {
    return;
  };
  if warned.insert(hash_of(&("glow", glow.radius.to_bits(), glow.color))) {
    engine_warn!(module: "text", "glow needs an MSDF font (FontRender::Msdf); drawn without it");
  }
}

/// A raster glyph: hinted coverage at the size the camera sees, its
/// outline a stroke band and its shadow a hard copy
fn emit_raster(
  block: &mut Block<'_, '_, '_>,
  glyph: &PlacedGlyph,
  style: &GlyphStyle,
  look: &GlyphLook,
  layer: Layer,
) {
  let (layout, place) = (block.layout, block.place);
  let mut r = glyph.size * place.px_per_layout;
  if place.size_steps {
    r = raster_size_step(r);
  }
  if !(r.is_finite() && r > 0.0) {
    return;
  }
  // Larger text draws a capped image scaled up, off the pixel grid
  let capped = r > place.max_glyph_px;
  if capped {
    r = place.max_glyph_px;
    warn_capped(block.sources, block.targets.warned, glyph.font, r);
  }
  let fraction = |pen: Vec2| {
    if capped {
      0.0
    } else {
      pen_fraction(place, pen)
    }
  };
  let to_quad = |mut key: GlyphKey, pen: Vec2, tint: Color| {
    key.coords = glyph.coords;
    key.skew = glyph.skew;
    key.embolden = glyph.embolden;
    GlyphQuad {
      key,
      pen,
      k: glyph.size / r,
      texel: (!capped).then_some(1.0),
      nudge: Vec2::ZERO,
      shade: Shade::tinted(tint, look.alpha, Sampling::Linear),
    }
  };
  let (font, id) = (glyph.font, glyph.glyph);
  match layer {
    Layer::Shadow => {
      warn_glow(block.targets.warned, style);
      if let Some(shadow) = style.shadow {
        let pen = look.pen + shadow.offset * layout.scale;
        let key = GlyphKey::fill(font, id, r, fraction(pen));
        push_glyph(block, glyph, look, to_quad(key, pen, shadow.color));
      }
    }
    Layer::Outline => {
      if let Some(outline) = style.outline {
        // The band is centred on the outline: twice the width puts `width`
        // outside the glyph
        let width = 2.0 * outline.width * r * layout.scale / glyph.size;
        let key = GlyphKey::stroke(font, id, r, fraction(look.pen), width);
        push_glyph(block, glyph, look, to_quad(key, look.pen, outline.color));
      }
    }
    Layer::Fill => {
      let key = GlyphKey::fill(font, id, r, fraction(look.pen));
      push_glyph(block, glyph, look, to_quad(key, look.pen, look.color));
    }
  }
}

/// Warn once per font and cap that its text is drawn from a capped glyph
/// image, scaled up
fn warn_capped(sources: &TextSources<'_>, warned: &mut HashSet<u64>, font: FontId, cap: f32) {
  if warned.insert(hash_of(&("glyph cap", font, cap.to_bits()))) {
    let family = sources.engine.face_family(font).unwrap_or_default();
    engine_warn!(module: "text", "text in {family:?} larger than {cap} px is drawn from a {cap} px glyph, scaled up; use an MSDF font (FontRender::Msdf) for very large text");
  }
}

/// The render pixel size a pixel font's glyph image is made at: the whole
/// multiple of `design_size` nearest `size_px`, at least one. The layout
/// snaps pixel text to these sizes, so the image maps texel for texel onto
/// its advance at the layout's own scale (`k` is one).
pub(crate) fn pixel_raster_px(size_px: f32, design_size: f32) -> f32 {
  design_size * (size_px / design_size).round().max(1.0)
}

/// A pixel glyph's image size and render pixels per image pixel, the image
/// at most `cap` pixels: the `n`th multiple of `design_size` that
/// `pixel_raster_px` picks, drawn 1:1, or, when that passes the cap, the
/// largest multiple `m` within it that divides `n`, drawn `n / m` render
/// pixels per image pixel (a whole number, so the glyph stays crisp)
pub(crate) fn pixel_image_px(size_px: f32, design_size: f32, cap: f32) -> (f32, f32) {
  let full = pixel_raster_px(size_px, design_size);
  let n = (full / design_size).round();
  let most = (cap / design_size).floor().clamp(1.0, cap.max(1.0));
  if n <= most {
    return (full, 1.0);
  }
  let m = (1..=most as u32)
    .rev()
    .map(|m| m as f32)
    .find(|&m| n % m == 0.0)
    .unwrap_or(1.0);
  (design_size * m, n / m)
}

/// A pixel font's glyph: on/off coverage at a whole multiple of its design
/// size, its outline four copies and its shadow a hard copy
fn emit_pixel(
  block: &mut Block<'_, '_, '_>,
  glyph: &PlacedGlyph,
  style: &GlyphStyle,
  look: &GlyphLook,
  layer: Layer,
  design_size: f32,
) {
  let (layout, place) = (block.layout, block.place);
  // Past the cap the image is a smaller multiple, scaled by a whole number
  let (r, texel) = pixel_image_px(
    glyph.size * place.px_per_layout,
    design_size,
    place.max_glyph_px,
  );
  if !(r.is_finite() && r > 0.0 && texel.is_finite()) {
    return;
  }
  let mut key = GlyphKey::pixel(glyph.font, glyph.glyph, r);
  key.coords = glyph.coords;
  key.skew = glyph.skew;
  key.embolden = glyph.embolden;
  let to_quad = |pen: Vec2, nudge: Vec2, tint: Color| GlyphQuad {
    key,
    pen,
    k: glyph.size / r,
    texel: Some(texel),
    nudge,
    shade: Shade::tinted(tint, look.alpha, Sampling::Nearest),
  };
  match layer {
    Layer::Shadow => {
      warn_glow(block.targets.warned, style);
      if let Some(shadow) = style.shadow {
        let pen = look.pen + shadow.offset * layout.scale;
        push_glyph(block, glyph, look, to_quad(pen, Vec2::ZERO, shadow.color));
      }
    }
    Layer::Outline => {
      if let Some(outline) = style.outline {
        for nudge in outline_nudges(outline.width, layout, place) {
          push_glyph(block, glyph, look, to_quad(look.pen, nudge, outline.color));
        }
      }
    }
    Layer::Fill => push_glyph(
      block,
      glyph,
      look,
      to_quad(look.pen, Vec2::ZERO, look.color),
    ),
  }
}

/// A pixel or bitmap font's outline: four copies a whole number of render
/// pixels (at least one) left, right, up and down
fn outline_nudges(width: f32, layout: &TextLayout, place: &TextPlacement) -> [Vec2; 4] {
  let d = (width * layout.scale * place.px_per_layout)
    .round()
    .max(1.0);
  let d = if d.is_finite() { d } else { 1.0 };
  [
    Vec2::new(-d, 0.0),
    Vec2::new(d, 0.0),
    Vec2::new(0.0, -d),
    Vec2::new(0.0, d),
  ]
}

/// An MSDF glyph: one image at the font's base size, scaled; outline,
/// shadow and glow are shader lanes, kept inside half the range
fn emit_msdf(
  block: &mut Block<'_, '_, '_>,
  glyph: &PlacedGlyph,
  style: &GlyphStyle,
  look: &GlyphLook,
  layer: Layer,
  (base, range): (f32, f32),
) {
  let (layout, place) = (block.layout, block.place);
  warn_msdf_synthesis(block.sources, block.targets.warned, glyph);
  let base = msdf_base(block.sources, block.targets.warned, glyph.font, base);
  let key = msdf_key(glyph, base, range);
  // Layout pixels per base pixel, base pixels per text unit and render
  // pixels per base pixel
  let k = glyph.size / base;
  let base_per_unit = base * layout.scale / glyph.size;
  let render_per_base = k * place.px_per_layout;
  if !(k.is_finite() && k > 0.0 && render_per_base.is_finite() && render_per_base > 0.0) {
    return;
  }
  let px_range = msdf_px_range(range, render_per_base, place);
  let to_quad = |pen: Vec2, tint: Color, alpha: f32, params: [f32; 4], extra: [f32; 4]| GlyphQuad {
    key,
    pen,
    k,
    texel: None,
    nudge: Vec2::ZERO,
    shade: Shade {
      tint,
      alpha,
      params,
      extra,
      sampling: Sampling::Linear,
    },
  };
  match layer {
    Layer::Shadow => {
      // Glow first, under the shadow
      if let Some(glow) = style.glow {
        let radius = glow.radius * base_per_unit;
        let (spread, soft) = msdf_soft(radius, radius * 0.5, range, px_range);
        if narrowed(radius, spread) || narrowed(radius * 0.5, soft) {
          warn_msdf_reach(block.sources, block.targets.warned, glyph.font, range);
        }
        let params = [1.0, spread, soft, range];
        let glow = to_quad(look.pen, glow.color, look.alpha, params, [0.0; 4]);
        push_glyph(block, glyph, look, glow);
      }
      if let Some(shadow) = style.shadow {
        let pen = look.pen + shadow.offset * layout.scale;
        let softness = shadow.softness * base_per_unit;
        let (spread, soft) = msdf_soft(0.0, softness, range, px_range);
        if narrowed(softness, soft) {
          warn_msdf_reach(block.sources, block.targets.warned, glyph.font, range);
        }
        let params = [1.0, spread, soft, range];
        let shadow = to_quad(pen, shadow.color, look.alpha, params, [0.0; 4]);
        push_glyph(block, glyph, look, shadow);
      }
    }
    Layer::Outline => {
      // The outline lane under an invisible fill: the band alone, beneath
      // every fill of the block
      if let Some(outline) = style.outline {
        let asked = outline.width * base_per_unit;
        let width = msdf_outline(asked, range, px_range);
        if narrowed(asked, width) {
          warn_msdf_reach(block.sources, block.targets.warned, glyph.font, range);
        }
        if width > 0.0 {
          let (params, extra) = ([0.0, width, 0.0, range], rgba(outline.color, look.alpha));
          let band = to_quad(look.pen, outline.color, 0.0, params, extra);
          push_glyph(block, glyph, look, band);
        }
      }
    }
    Layer::Fill => {
      let fill = to_quad(
        look.pen,
        look.color,
        look.alpha,
        [0.0, 0.0, 0.0, range],
        [0.0; 4],
      );
      push_glyph(block, glyph, look, fill);
    }
  }
}

/// Whether an MSDF lane was made narrower than asked to fit the range
fn narrowed(asked: f32, got: f32) -> bool {
  asked.is_finite() && got < asked * (1.0 - 1e-4) - 1e-6
}

/// Warn once per font that an outline, shadow or glow reaches past an MSDF
/// font's distance range and is drawn narrower
fn warn_msdf_reach(sources: &TextSources<'_>, warned: &mut HashSet<u64>, font: FontId, range: f32) {
  if warned.insert(hash_of(&("msdf reach", font))) {
    let family = sources.engine.face_family(font).unwrap_or_default();
    engine_warn!(module: "text", "MSDF font {family:?} (range {range} px) holds outlines, shadows and glow only to half its range; a wider one is drawn narrower (give the font a larger range: FontRender::Msdf {{ base, range }})");
  }
}

/// An MSDF glyph's key. Quads cannot shear, so an MSDF font draws only its
/// own faces: synthesized italic and bold are left out of the key (one image
/// serves them all).
fn msdf_key(glyph: &PlacedGlyph, base: f32, range: f32) -> GlyphKey {
  let mut key = GlyphKey::msdf(glyph.font, glyph.glyph, base, range);
  key.coords = glyph.coords;
  key
}

/// An MSDF font's base size, capped at `MSDF_MAX_BASE` so a glyph page holds
/// its glyphs (warned once per font)
fn msdf_base(sources: &TextSources<'_>, warned: &mut HashSet<u64>, font: FontId, base: f32) -> f32 {
  if base <= MSDF_MAX_BASE {
    return base;
  }
  if warned.insert(hash_of(&("msdf base", font))) {
    let family = sources.engine.face_family(font).unwrap_or_default();
    engine_warn!(module: "text", "MSDF font {family:?} has a base of {base} px; its glyphs are made at {MSDF_MAX_BASE} px (a distance field scales freely)");
  }
  capped_msdf_base(base)
}

/// `base` within `MSDF_MAX_BASE` (a base that is not a number is the cap)
fn capped_msdf_base(base: f32) -> f32 {
  if base <= MSDF_MAX_BASE {
    base
  } else {
    MSDF_MAX_BASE
  }
}

/// Warn once per font that an MSDF font's text asked for a synthesized
/// italic or bold it cannot have
fn warn_msdf_synthesis(sources: &TextSources<'_>, warned: &mut HashSet<u64>, glyph: &PlacedGlyph) {
  if (glyph.skew || glyph.embolden) && warned.insert(hash_of(&("msdf synthesis", glyph.font))) {
    let family = sources.engine.face_family(glyph.font).unwrap_or_default();
    engine_warn!(module: "text", "MSDF font {family:?} has no italic or bold face of its own; its text draws upright and regular (add the real face)");
  }
}

/// The fewest screen pixels the MSDF shader can take the distance range to
/// span for this glyph. It measures the range from the UV's screen
/// derivatives (`fwidth`), which read up to √2 too wide on a turned quad, on
/// the block's thinner axis, and never below one pixel; the lane clamps
/// below must hold for that reading.
fn msdf_px_range(range: f32, render_per_base: f32, place: &TextPlacement) -> f32 {
  let (ax, ay) = (place.axis_x.length(), place.axis_y.length());
  let squash = ax.min(ay) / ax.max(ay);
  let squash = if squash.is_finite() && squash > 0.0 {
    squash
  } else {
    1.0
  };
  // Only a block on the pixel grid is known to be unturned on screen
  let turn = if place.pixel_grid.is_some() {
    1.0
  } else {
    std::f32::consts::SQRT_2
  };
  (range * render_per_base * squash / turn).max(1.0)
}

/// The widest MSDF outline (base pixels) the distance field holds: half
/// the range, less half a screen pixel so the band fades out where the
/// field still reaches rather than at the quad's edge. `px_range` is
/// `msdf_px_range`.
fn msdf_outline(width: f32, range: f32, px_range: f32) -> f32 {
  let reach = (range / 2.0 - 0.5 * range / px_range).max(0.0);
  if width.is_finite() {
    width.clamp(0.0, reach)
  } else {
    0.0
  }
}

/// A soft layer's spread and softness (base pixels), shrunk together to
/// fit inside half the range. The shader softens over at least one screen
/// pixel, so that much is kept for the softness.
fn msdf_soft(spread: f32, softness: f32, range: f32, px_range: f32) -> (f32, f32) {
  let finite = |v: f32| if v.is_finite() { v.max(0.0) } else { 0.0 };
  let (spread, softness) = (finite(spread), finite(softness));
  let reach = range / 2.0;
  let least = range / px_range;
  let total = spread + softness.max(least);
  if total <= reach {
    return (spread, softness);
  }
  let fit = reach / total;
  let softness = softness * fit;
  let spread = (spread * fit).min((reach - softness.max(least)).max(0.0));
  (spread, softness)
}

/// A colour's straight sRGB channels, its alpha times `alpha`
fn rgba(color: Color, alpha: f32) -> [f32; 4] {
  [
    f32::from(color.r) / 255.0,
    f32::from(color.g) / 255.0,
    f32::from(color.b) / 255.0,
    f32::from(color.a) / 255.0 * alpha,
  ]
}

/// The pen's fraction of a render pixel (0 off the pixel grid)
fn pen_fraction(place: &TextPlacement, pen: Vec2) -> f32 {
  place.pixel_grid.map_or(0.0, |(grid, ppu)| {
    let x = ((place.origin + place.axis_x * pen.x + place.axis_y * pen.y - grid) * ppu).x;
    x - x.floor()
  })
}

/// Fetch (or make) a glyph quad's image and push it
fn push_glyph(
  block: &mut Block<'_, '_, '_>,
  glyph: &PlacedGlyph,
  look: &GlyphLook,
  glyph_quad: GlyphQuad,
) {
  let (sources, place) = (block.sources, block.place);
  let TextTargets {
    raster,
    sprites,
    device,
    queue,
    list,
    ..
  } = &mut *block.targets;
  let key = glyph_quad.key;
  let Some(slot) = sprites.glyph(device, queue, key, || {
    let coords = sources.engine.coords(glyph.coords);
    raster.rasterize(sources.fonts, coords.as_deref(), &key)
  }) else {
    return;
  };
  let image = ImageBox {
    left: slot.left as f32,
    top: slot.top as f32,
    width: slot.width as f32,
    height: slot.height as f32,
  };
  let texel = glyph_quad.texel;
  let (centre, size, rotation) = quad(
    place,
    glyph_quad.pen,
    image,
    glyph_quad.k,
    texel,
    glyph_quad.nudge,
  );
  let size = size * look.scale;
  if !(centre.is_finite() && size.is_finite() && size.x != 0.0 && size.y != 0.0) {
    return;
  }
  let shade = glyph_quad.shade;
  let mut instance = SpriteInstance::new()
    .with_transform(centre.x, centre.y, rotation, size.x, size.y)
    .with_uv(slot.uv[0], slot.uv[1], slot.uv[2], slot.uv[3])
    .with_tint(shade.tint, shade.alpha);
  instance.uv_bounds = slot.uv;
  instance.effect_params = shade.params;
  instance.effect_extra = shade.extra;
  let reach = Vec2::splat(size.length() / 2.0);
  let draw = place.draw;
  list.push_glyph(
    draw.kind,
    draw.z,
    draw.lit,
    draw.only_camera,
    DrawGlyph {
      page: slot.page,
      instance,
      shader: shader_for(slot.page),
      blend: draw.blend,
      sampling: shade.sampling,
      layers: draw.layers,
      min: centre - reach,
      max: centre + reach,
    },
  );
}

/// The text shader a glyph page draws with
fn shader_for(page: GlyphPage) -> BatchShader {
  match page.format {
    GlyphFormat::Mask => BatchShader::TextMask,
    GlyphFormat::Color => BatchShader::TextColor,
    GlyphFormat::Msdf => BatchShader::TextMsdf,
  }
}

/// Centre, size (target units) and rotation of an image's quad. `k` is
/// layout pixels per image pixel; with `texel` (render pixels per image
/// pixel) and a pixel grid the quad lands on whole render pixels. `nudge`
/// moves it by render pixels.
fn quad(
  place: &TextPlacement,
  pen: Vec2,
  image: ImageBox,
  k: f32,
  texel: Option<f32>,
  nudge: Vec2,
) -> (Vec2, Vec2, f32) {
  if let (Some(texel), Some((grid, ppu))) = (texel, place.pixel_grid) {
    let pen_t = place.origin + place.axis_x * pen.x + place.axis_y * pen.y;
    let px = (pen_t - grid) * ppu;
    let mut ix = px.x.floor();
    // A subpixel bin that rounds up to 4 is the next whole pixel
    if ((px.x - ix) * 4.0).round() >= 4.0 {
      ix += 1.0;
    }
    let iy = px.y.round();
    let top_left = Vec2::new(ix + image.left * texel, iy - image.top * texel) + nudge;
    let size = Vec2::new(image.width, image.height) * texel;
    let centre = grid + (top_left + size / 2.0) / ppu;
    return (centre, size / ppu, 0.0);
  }
  let pen = pen + nudge / place.px_per_layout;
  let top_left = pen + Vec2::new(image.left * k, -image.top * k);
  let local = top_left + Vec2::new(image.width, image.height) * k / 2.0;
  let centre = place.origin + place.axis_x * local.x + place.axis_y * local.y;
  // A mirrored block draws its glyphs mirrored
  let mirror = if place.axis_x.perp_dot(place.axis_y) < 0.0 {
    -1.0
  } else {
    1.0
  };
  let size = Vec2::new(
    image.width * k * place.axis_x.length(),
    image.height * k * place.axis_y.length() * mirror,
  );
  (centre, size, place.axis_x.y.atan2(place.axis_x.x))
}

/// A bitmap font's cell, drawn from its sprite sheet at a whole number of
/// render pixels per texel; its outline is four copies, its shadow a copy
fn emit_bitmap(
  block: &mut Block<'_, '_, '_>,
  glyph: &PlacedGlyph,
  style: &GlyphStyle,
  look: &GlyphLook,
  layer: Layer,
  bitmap: &BitmapFace,
) {
  let (layout, place) = (block.layout, block.place);
  let (cw, ch) = (bitmap.cell.0 as f32, bitmap.cell.1 as f32);
  let sheet = Vec2::new(bitmap.sprite.width() as f32, bitmap.sprite.height() as f32);
  if cw <= 0.0 || ch <= 0.0 || sheet.x <= 0.0 || sheet.y <= 0.0 {
    return;
  }
  let cell = u32::from(glyph.glyph);
  let columns = bitmap.columns.max(1);
  let (col, row) = ((cell % columns) as f32, (cell / columns) as f32);
  let uv = [
    col * cw / sheet.x,
    row * ch / sheet.y,
    (col + 1.0) * cw / sheet.x,
    (row + 1.0) * ch / sheet.y,
  ];
  let k = glyph.size / ch;
  let image = ImageBox {
    left: 0.0,
    top: ch,
    width: cw,
    height: ch,
  };
  let texel = (k * place.px_per_layout).round().max(1.0);
  let mut cell_at = |pen: Vec2, nudge: Vec2, tint: Color| {
    let (centre, size, rotation) = quad(place, pen, image, k, Some(texel), nudge);
    let size = size * look.scale;
    if !(centre.is_finite() && size.is_finite() && size.x != 0.0 && size.y != 0.0) {
      return;
    }
    let instance = SpriteInstance::new()
      .with_transform(centre.x, centre.y, rotation, size.x, size.y)
      .with_uv(uv[0], uv[1], uv[2], uv[3])
      .with_tint(tint, look.alpha);
    push_sprite(
      block.targets,
      place,
      &bitmap.sprite,
      Filter::Nearest,
      instance,
    );
  };
  match layer {
    Layer::Shadow => {
      if let Some(shadow) = style.shadow {
        let pen = look.pen + shadow.offset * layout.scale;
        cell_at(pen, Vec2::ZERO, shadow.color);
      }
      warn_glow(block.targets.warned, style);
    }
    Layer::Outline => {
      if let Some(outline) = style.outline {
        for nudge in outline_nudges(outline.width, layout, place) {
          cell_at(look.pen, nudge, outline.color);
        }
      }
    }
    Layer::Fill => cell_at(look.pen, Vec2::ZERO, look.color),
  }
}

/// Push a sprite quad (a bitmap cell or an icon) as part of the block
fn push_sprite(
  targets: &mut TextTargets<'_>,
  place: &TextPlacement,
  sprite: &SpriteHandle,
  filter: Filter,
  instance: SpriteInstance,
) {
  let texture = targets.sprites.texture_of(sprite);
  let draw = place.draw;
  let look = SpriteLook {
    shading: SpriteShading::Plain,
    blend: draw.blend,
    filter,
    lit: draw.lit,
  };
  let seen = (draw.layers, None, draw.only_camera);
  targets
    .list
    .push_sprite_in(draw.kind, draw.z, texture, look, instance, seen);
}

/// An inline icon: its sprite (or a region of it) over its box, revealed
/// with its cluster
fn emit_icon(block: &mut Block<'_, '_, '_>, icon: &PlacedIcon) {
  let place = block.place;
  let mut alpha = place.opacity;
  if let Some((shown, fade)) = place.reveal {
    alpha *= reveal_alpha(icon.cluster, shown, fade);
  }
  if alpha.is_nan() || alpha <= 0.0 {
    return;
  }
  let (sprite, uv) = match &icon.icon {
    Icon::Sprite { sprite, .. } => (sprite, [0.0, 0.0, 1.0, 1.0]),
    Icon::Named { name, .. } => {
      let Some(source) = block.sources.icons.get(name) else {
        if block.targets.warned.insert(hash_of(&("icon", name))) {
          engine_warn!(module: "text", "no text icon named {name:?}; leaving its space empty");
        }
        return;
      };
      let size = Vec2::new(source.sprite.width() as f32, source.sprite.height() as f32);
      let uv = source
        .region
        .map_or([0.0, 0.0, 1.0, 1.0], |region| region_uv(region, size));
      (&source.sprite, uv)
    }
  };
  // The box's top-left is its "pen", in layout pixels
  let rect = icon.rect;
  let image = ImageBox {
    left: 0.0,
    top: 0.0,
    width: rect.width,
    height: rect.height,
  };
  let pen = Vec2::new(rect.x, rect.y);
  let (centre, size, rotation) = quad(place, pen, image, 1.0, None, Vec2::ZERO);
  if !(centre.is_finite() && size.is_finite() && size.x != 0.0 && size.y != 0.0) {
    return;
  }
  let instance = SpriteInstance::new()
    .with_transform(centre.x, centre.y, rotation, size.x, size.y)
    .with_uv(uv[0], uv[1], uv[2], uv[3])
    .with_tint(Color::WHITE, alpha);
  push_sprite(block.targets, place, sprite, Filter::Default, instance);
}

/// A sprite region's UVs (a region in pixels on a `size` sprite), inset
/// half a texel on each side so linear filtering never reads the texels
/// around it (a region under a texel wide samples its middle)
fn region_uv(region: Rect, size: Vec2) -> [f32; 4] {
  if !(size.x > 0.0 && size.y > 0.0) {
    return [0.0, 0.0, 1.0, 1.0];
  }
  let inset_x = (region.width.max(0.0) / 2.0).min(0.5);
  let inset_y = (region.height.max(0.0) / 2.0).min(0.5);
  [
    (region.x + inset_x) / size.x,
    (region.y + inset_y) / size.y,
    (region.x + region.width - inset_x) / size.x,
    (region.y + region.height - inset_y) / size.y,
  ]
}

/// Lay out the `Text2d`s whose text, style, options or fonts changed, and
/// set their `TextBounds`. Syncs the engine with `Fonts` first. Prompts
/// (`[prompt=jump]`) resolve against `prompts`, labels in the UI theme's
/// accent, and a text whose resolved form changed (a new device, a glyph
/// registered) is laid out again.
pub(crate) fn layout_world_text(
  world: &mut World,
  engine: &mut TextEngine,
  prompts: &PromptContext,
) {
  let Some(fonts) = world.get_resource::<Fonts>() else {
    return;
  };
  engine.sync(fonts);
  let generation = engine.generation();
  let accent = world.get_resource::<UiTheme>().map_or_else(
    || Palette::default().accent,
    |theme| theme.get().palette.accent,
  );
  let mut stale: Vec<(Entity, u64)> = Vec::new();
  let mut query = world.query::<(Entity, &Text2d, Option<&TextLayoutState>)>();
  for (entity, text, state) in query.iter(world) {
    let resolved = resolve_prompts(&text.text, prompts, accent);
    let key = world_key(text, &resolved, generation);
    if state.is_none_or(|state| state.key != key) {
      stale.push((entity, key));
    }
  }
  for (entity, key) in stale {
    let Some(text) = world.get::<Text2d>(entity) else {
      continue;
    };
    // The reveal starts over for a new text, not for a new device
    let text_key = hash_of_rich(text);
    let anchor = text.layout.anchor;
    let resolved = resolve_prompts(&text.text, prompts, accent);
    let laid = engine.layout(&resolved, &text.style, &text.layout, 1.0);
    let Ok(mut entity) = world.get_entity_mut(entity) else {
      continue;
    };
    match laid {
      Some(layout) => {
        // A new text starts its reveal over (a new style or font does not)
        let restart = entity
          .get::<TextLayoutState>()
          .is_some_and(|state| state.text != text_key);
        let bounds = TextBounds {
          size: layout.size,
          offset: -anchor.fraction() * layout.size,
        };
        entity.insert((
          bounds,
          TextLayoutState {
            layout,
            key,
            text: text_key,
          },
        ));
        if restart && let Some(mut reveal) = entity.get_mut::<TextReveal>() {
          reveal.restart();
        }
      }
      // The engine warned; the entity draws nothing
      None => {
        entity.remove::<TextLayoutState>();
        if entity.get::<TextBounds>() != Some(&TextBounds::default()) {
          entity.insert(TextBounds::default());
        }
      }
    }
  }
}

/// What a `Text2d`'s layout and bounds depend on: its text with its
/// prompts resolved (`resolved`), its style and options, and the fonts
fn world_key(text: &Text2d, resolved: &RichText, generation: u64) -> u64 {
  let mut h = DefaultHasher::new();
  resolved.hash_into(&mut h);
  text.style.hash_into(&mut h);
  text.layout.hash_into(&mut h);
  // The anchor never changes the layout, but it places the bounds
  text.layout.anchor.hash(&mut h);
  generation.hash(&mut h);
  h.finish()
}

fn hash_of_rich(text: &Text2d) -> u64 {
  let mut h = DefaultHasher::new();
  text.text.hash_into(&mut h);
  h.finish()
}

/// A `Text2d` as world text draws it
type WorldTextItem = (
  Entity,
  &'static GlobalTransform,
  Option<&'static PreviousGlobalTransform>,
  &'static Text2d,
  &'static TextLayoutState,
  &'static TextBounds,
  Option<&'static TextReveal>,
  Option<&'static RenderLayers>,
);

/// The frame's world text, per camera
pub(crate) struct WorldTextFrame<'a> {
  pub(crate) cameras: &'a [CameraDraw],
  /// Render pixels per screen unit (window cameras)
  pub(crate) render_scale: f32,
  /// The render interpolation factor
  pub(crate) alpha: f32,
  /// Game seconds, for effects
  pub(crate) game_time: f32,
}

/// Emit every laid-out `Text2d`'s glyphs for each camera that sees it
pub(crate) fn emit_world_text(
  world: &mut World,
  engine: &TextEngine,
  frame: &WorldTextFrame<'_>,
  targets: &mut TextTargets<'_>,
) {
  let mut query = world.query::<WorldTextItem>();
  let world: &World = world;
  let (Some(fonts), Some(effects), Some(icons)) = (
    world.get_resource::<Fonts>(),
    world.get_resource::<TextEffects>(),
    world.get_resource::<TextIcons>(),
  ) else {
    return;
  };
  let sources = TextSources {
    fonts,
    engine,
    effects,
    icons,
  };
  let render_scale = if frame.render_scale.is_finite() && frame.render_scale > 0.0 {
    frame.render_scale
  } else {
    1.0
  };
  for (entity, global, previous, text, state, bounds, reveal, layers) in query.iter(world) {
    let drawn = render_global(global, previous, frame.alpha);
    let matrix = drawn.affine().matrix2;
    let (axis_x, axis_y, position) = (matrix.x_axis, matrix.y_axis, drawn.translation());
    if !(axis_x.is_finite() && axis_y.is_finite() && position.is_finite()) {
      if targets.warned.insert(hash_of(&("non-finite", entity))) {
        engine_warn!(module: "text", "{entity:?}: Text2d has a non-finite position, scale or rotation; not drawn");
      }
      continue;
    }
    // Scaled to zero, as a pop-in tween starts: nothing to draw, and nothing
    // wrong
    if axis_x.length_squared() == 0.0 || axis_y.length_squared() == 0.0 {
      continue;
    }
    let layout = &state.layout;
    let origin = position + axis_x * bounds.offset.x + axis_y * bounds.offset.y;
    // The block's box, an em wider all round for outlines, shadows and
    // effects that reach past it
    let em = if text.style.size.is_finite() {
      text.style.size.max(0.0) * layout.scale
    } else {
      0.0
    };
    let (lo, hi) = (Vec2::splat(-em), layout.size + Vec2::splat(em));
    let corners = [
      Vec2::new(lo.x, lo.y),
      Vec2::new(hi.x, lo.y),
      Vec2::new(lo.x, hi.y),
      Vec2::new(hi.x, hi.y),
    ]
    .map(|c| origin + axis_x * c.x + axis_y * c.y);
    let min = corners.iter().copied().fold(Vec2::INFINITY, Vec2::min);
    let max = corners.iter().copied().fold(Vec2::NEG_INFINITY, Vec2::max);
    let layers = layers.copied().unwrap_or_default();
    let reveal = reveal.and_then(reveal_window);
    // Glyph images map 1:1 onto render pixels only on an unturned,
    // unmirrored, evenly scaled block
    let on_grid = axis_x.y == 0.0 && axis_y.x == 0.0 && axis_x.x > 0.0 && axis_x.x == axis_y.y;
    for camera in frame.cameras {
      if !camera.layers.intersects(&layers) || !camera.view.is_visible(min, max) {
        continue;
      }
      // Render pixels per world unit: a texture camera's target units are
      // its pixels
      let scale = if camera.target.is_some() {
        1.0
      } else {
        render_scale
      };
      let ppu = camera.view.scale * scale;
      let px_per_layout = ppu * axis_x.length().max(axis_y.length());
      let pixel_grid = (on_grid && camera.view.rotation == 0.0)
        .then(|| (camera.view.screen_to_world(camera.view.rect_min), ppu));
      let place = TextPlacement {
        origin,
        axis_x,
        axis_y,
        px_per_layout,
        size_steps: true,
        max_glyph_px: WORLD_GLYPH_PX,
        pixel_grid,
        draw: TextDraw {
          kind: DrawKind::Text,
          z: text.z_order,
          lit: text.lit,
          only_camera: Some(camera.entity),
          layers,
          scissor: None,
          blend: text.blend,
        },
        time: frame.game_time,
        reveal,
        seed: entity.to_bits(),
        opacity: clean_opacity(text.opacity),
      };
      emit_text(&sources, targets, layout, &place);
    }
  }
}

/// The clusters a reveal draws and its fade in clusters; `None` once it has
/// settled, so every glyph is at full alpha
fn reveal_window(reveal: &TextReveal) -> Option<(f32, f32)> {
  (!reveal.settled()).then(|| reveal.drawn())
}

/// Build the glyphs `Fonts::prewarm` asked for: for each character, the
/// image its font's render mode draws at that size
pub(crate) fn prewarm(fonts: &mut Fonts, engine: &TextEngine, targets: &mut TextTargets<'_>) {
  let requests = fonts.take_prewarm();
  let fonts: &Fonts = fonts;
  for request in &requests {
    let (font, size) = (request.font, request.size);
    if !(size.is_finite() && size > 0.0) {
      continue;
    }
    let render = engine.face_render(font);
    for c in request.chars.chars() {
      let Some(glyph) = glyph_id(fonts, font, c) else {
        continue;
      };
      // The images text at this size draws from (the UI's caps)
      let key = match render {
        FontRender::Raster => GlyphKey::fill(font, glyph, size.min(UI_GLYPH_PX), 0.0),
        FontRender::Pixel { design_size } => {
          let (r, _) = pixel_image_px(size, design_size, UI_GLYPH_PX);
          GlyphKey::pixel(font, glyph, r)
        }
        FontRender::Msdf { base, range } => {
          GlyphKey::msdf(font, glyph, capped_msdf_base(base), range)
        }
      };
      let TextTargets {
        raster,
        sprites,
        device,
        queue,
        ..
      } = targets;
      let _ = sprites.glyph(device, queue, key, || raster.rasterize(fonts, None, &key));
    }
  }
}

fn hash_of(value: &impl Hash) -> u64 {
  let mut h = DefaultHasher::new();
  value.hash(&mut h);
  h.finish()
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ui::Anchor;

  /// An unrotated block at the origin, one target unit and one render
  /// pixel per layout pixel
  fn placement(reveal: Option<(f32, f32)>) -> TextPlacement {
    TextPlacement {
      origin: Vec2::ZERO,
      axis_x: Vec2::X,
      axis_y: Vec2::Y,
      px_per_layout: 1.0,
      size_steps: false,
      max_glyph_px: UI_GLYPH_PX,
      pixel_grid: Some((Vec2::ZERO, 1.0)),
      draw: TextDraw {
        kind: DrawKind::Text,
        z: 0,
        lit: false,
        only_camera: None,
        layers: RenderLayers::default(),
        scissor: None,
        blend: BlendMode::Alpha,
      },
      time: 0.0,
      reveal,
      seed: 0,
      opacity: 1.0,
    }
  }

  #[test]
  fn snapped_glyphs_land_on_whole_pixels() {
    let mut place = placement(None);
    place.origin = Vec2::new(10.3, 5.6);
    let image = ImageBox {
      left: 1.0,
      top: 9.0,
      width: 7.0,
      height: 9.0,
    };
    let pen = Vec2::new(0.0, 12.0);
    let (centre, size, rotation) = quad(&place, pen, image, 1.0, Some(1.0), Vec2::ZERO);
    // Pen x 10.3 floors to 10 (its fraction picks the bin), baseline 17.6
    // rounds to 18; the image maps 1:1
    assert_eq!(centre - size / 2.0, Vec2::new(11.0, 9.0));
    assert_eq!((size, rotation), (Vec2::new(7.0, 9.0), 0.0));
    // A fraction whose bin rounds up to 4 is the next whole pixel
    place.origin.x = 10.9;
    let (centre, size, _) = quad(&place, pen, image, 1.0, Some(1.0), Vec2::ZERO);
    assert_eq!((centre - size / 2.0).x, 12.0);
    // Off the grid, the image scales by k about the pen
    place.pixel_grid = None;
    let (centre, size, _) = quad(&place, pen, image, 0.5, Some(1.0), Vec2::ZERO);
    assert_eq!(size, Vec2::new(3.5, 4.5));
    assert_eq!(centre - size / 2.0, Vec2::new(10.9 + 0.5, 17.6 - 4.5));
  }

  #[test]
  fn capped_pixel_glyphs_scale_a_smaller_multiple_by_a_whole_number() {
    // Within the cap: the nearest multiple, drawn 1:1
    assert_eq!(pixel_image_px(40.0, 16.0, 256.0), (48.0, 1.0));
    // 3008 px is 188 x 16; within 256 px the largest multiple dividing 188
    // is 4 (64 px), drawn 47 render pixels per image pixel
    assert_eq!(pixel_image_px(3000.0, 16.0, 256.0), (64.0, 47.0));
    // A prime multiple past the cap falls back to the design size
    assert_eq!(pixel_image_px(16.0 * 37.0, 16.0, 256.0), (16.0, 37.0));
    for (r, texel) in [
      pixel_image_px(5000.0, 7.0, 512.0),
      pixel_image_px(1.0e9, 16.0, 256.0),
    ] {
      assert!(r <= 512.0 && texel.fract() == 0.0, "{r} x {texel}");
    }
  }

  #[test]
  fn msdf_bases_past_the_cap_are_made_at_it() {
    assert_eq!(capped_msdf_base(32.0), 32.0);
    assert_eq!(capped_msdf_base(2000.0), MSDF_MAX_BASE);
    assert_eq!(capped_msdf_base(f32::NAN), MSDF_MAX_BASE);
  }

  #[test]
  fn a_mirrored_block_draws_mirrored_quads() {
    let mut place = placement(None);
    place.pixel_grid = None;
    place.axis_y = Vec2::new(0.0, -2.0);
    let image = ImageBox {
      left: 0.0,
      top: 4.0,
      width: 2.0,
      height: 4.0,
    };
    let (_, size, _) = quad(&place, Vec2::ZERO, image, 1.0, None, Vec2::ZERO);
    assert_eq!(size, Vec2::new(2.0, -8.0));
  }

  #[test]
  fn msdf_lanes_stay_inside_half_the_range() {
    // Range 4 spanning 4 screen pixels: 2 base pixels of field, less half a
    // pixel for the band's edge
    assert_eq!(msdf_outline(1.0, 4.0, 4.0), 1.0);
    assert_eq!(msdf_outline(10.0, 4.0, 4.0), 1.5);
    assert_eq!(msdf_outline(f32::NAN, 4.0, 4.0), 0.0);
    // Magnified, the edge needs less of the field
    assert_eq!(msdf_outline(10.0, 4.0, 16.0), 1.875);
    assert_eq!(msdf_soft(1.2, 0.6, 4.0, 5.0), (1.2, 0.6), "fits as asked");
    let (spread, soft) = msdf_soft(10.0, 5.0, 4.0, 4.0);
    assert!(spread + soft.max(1.0) <= 2.0 + 1e-5, "{spread} + {soft}");
    assert!(spread > 0.0 && soft > 0.0, "both shrink, neither vanishes");
    assert_eq!(msdf_soft(f32::INFINITY, -1.0, 4.0, 4.0), (0.0, 0.0));
  }

  #[test]
  fn only_a_lane_cut_short_by_the_range_counts_as_narrowed() {
    let (width, range, px_range) = (10.0, 4.0, 4.0);
    assert!(narrowed(width, msdf_outline(width, range, px_range)));
    assert!(!narrowed(1.0, msdf_outline(1.0, range, px_range)), "fits");
    let (spread, soft) = msdf_soft(1.2, 0.6, 4.0, 5.0);
    assert!(
      !narrowed(1.2, spread) && !narrowed(0.6, soft),
      "fits as asked"
    );
    assert!(
      !narrowed(f32::NAN, 0.0),
      "a bad value is not the range's doing"
    );
    assert!(!narrowed(0.0, 0.0));
  }

  #[test]
  fn the_msdf_px_range_allows_for_turns_squashes_and_tiny_text() {
    let mut place = placement(None);
    assert_eq!(msdf_px_range(4.0, 1.0, &place), 4.0, "on the grid");
    place.pixel_grid = None;
    let turned = msdf_px_range(4.0, 1.0, &place);
    assert!((turned - 4.0 / std::f32::consts::SQRT_2).abs() < 1e-5);
    place.axis_y = Vec2::new(0.0, 0.5);
    assert!((msdf_px_range(4.0, 1.0, &place) - turned / 2.0).abs() < 1e-5);
    assert_eq!(msdf_px_range(4.0, 0.01, &place), 1.0, "never below a pixel");
  }

  #[test]
  fn icon_regions_keep_half_a_texel_off_their_neighbours() {
    let size = Vec2::new(64.0, 32.0);
    let uv = region_uv(Rect::new(16.0, 8.0, 16.0, 16.0), size);
    let (du, dv) = (0.5 / size.x, 0.5 / size.y);
    let expected = [
      16.0 / 64.0 + du,
      8.0 / 32.0 + dv,
      32.0 / 64.0 - du,
      24.0 / 32.0 - dv,
    ];
    for (got, want) in uv.iter().zip(expected) {
      assert!((got - want).abs() < 1e-6, "{uv:?} vs {expected:?}");
    }
    // A region under a texel wide samples its middle
    let thin = region_uv(Rect::new(4.0, 0.0, 0.5, 2.0), size);
    assert_eq!(thin[0], thin[2]);
    assert_eq!(
      region_uv(Rect::new(0.0, 0.0, 1.0, 1.0), Vec2::ZERO),
      [0.0, 0.0, 1.0, 1.0]
    );
  }

  #[test]
  fn anchors_place_the_block() {
    assert_eq!(Anchor::TopLeft.fraction(), Vec2::ZERO);
    assert_eq!(Anchor::Center.fraction(), Vec2::splat(0.5));
    assert_eq!(Anchor::BottomRight.fraction(), Vec2::ONE);
    assert_eq!(Anchor::CenterRight.fraction(), Vec2::new(1.0, 0.5));
  }

  #[cfg(feature = "default-fonts")]
  mod with_fonts {
    use super::*;
    use crate::core::ecs::{Transform, World};
    use crate::core::text::{FontRole, RichText, TextLayoutOptions, TextStyle};

    fn laid(engine: &mut TextEngine, text: &str, style: &TextStyle) -> std::sync::Arc<TextLayout> {
      engine
        .layout(
          &RichText::new(text),
          style,
          &TextLayoutOptions::default(),
          1.0,
        )
        .unwrap()
    }

    #[test]
    fn a_fading_reveal_fades_its_last_glyph_in_smoothly() {
      let fonts = Fonts::with_builtins();
      let mut engine = TextEngine::new();
      engine.sync(&fonts);
      let layout = laid(&mut engine, "HHHHHH", &TextStyle::default());
      let (effects, icons) = (TextEffects::default(), TextIcons::default());
      let sources = TextSources {
        fonts: &fonts,
        engine: &engine,
        effects: &effects,
        icons: &icons,
      };
      let last = layout.glyphs.len() - 1;
      let at = Glyph {
        index: last as u32,
        glyph: &layout.glyphs[last],
      };
      let alpha_of = |reveal: &TextReveal| {
        let place = placement(reveal_window(reveal));
        let mut effects = EffectRun::new(&place);
        modifier(
          &sources,
          &layout,
          &place,
          at,
          &layout.styles[0],
          &mut effects,
        )
        .map_or(0.0, |(_, alpha)| alpha)
      };
      // Six clusters a second, each fading in over half a second: the fade
      // spans three clusters, and a 60 Hz frame moves it a thirtieth
      let mut reveal = TextReveal::new(6.0).with_fade(0.5);
      let dt = 1.0 / 60.0;
      let step = dt * 6.0 / 3.0;
      let (mut revealed, mut alphas) = (0, Vec::new());
      for _ in 0..200 {
        if reveal.settled() {
          break;
        }
        if reveal.advance(dt, &layout) {
          revealed += 1;
        }
        alphas.push(alpha_of(&reveal));
      }
      assert!(reveal.settled() && reveal.is_done());
      assert_eq!(revealed, 1, "TextRevealed once");
      assert_eq!(alpha_of(&reveal), 1.0, "settled at full alpha");
      alphas.push(alpha_of(&reveal));
      for pair in alphas.windows(2) {
        let rise = pair[1] - pair[0];
        assert!(
          (-1e-6..=step + 1e-4).contains(&rise),
          "the last glyph rises smoothly: {pair:?} (a frame's step is {step})"
        );
      }
    }

    #[test]
    fn an_unknown_effect_is_looked_up_once_per_block() {
      let fonts = Fonts::with_builtins();
      let mut engine = TextEngine::new();
      engine.sync(&fonts);
      let text = RichText::markup("[nope-block-effect]abc[/nope-block-effect]");
      let layout = engine
        .layout(
          &text,
          &TextStyle::default(),
          &TextLayoutOptions::default(),
          1.0,
        )
        .unwrap();
      let (effects, icons) = (TextEffects::default(), TextIcons::default());
      let sources = TextSources {
        fonts: &fonts,
        engine: &engine,
        effects: &effects,
        icons: &icons,
      };
      let place = placement(None);
      let mut run = EffectRun::new(&place);
      for (index, glyph) in layout.glyphs.iter().enumerate() {
        let at = Glyph {
          index: index as u32,
          glyph,
        };
        let style = &layout.styles[usize::from(glyph.style)];
        let (modified, _) = modifier(&sources, &layout, &place, at, style, &mut run).unwrap();
        assert_eq!(modified, GlyphMod::default());
        // From the first glyph on, the name is known to be unknown
        assert_eq!(run.unknown, Some("nope-block-effect"));
      }
    }

    #[test]
    fn msdf_text_asking_for_italic_draws_upright_and_warns_once() {
      let mut fonts = Fonts::with_builtins();
      let ui = fonts.role(FontRole::Ui).unwrap();
      fonts.set_render(ui, FontRender::msdf());
      let mut engine = TextEngine::new();
      engine.sync(&fonts);
      // Inter has no italic face: Parley synthesizes one
      let layout = laid(&mut engine, "Slanted", &TextStyle::default().italic());
      assert!(layout.glyphs.iter().all(|glyph| glyph.skew));
      let key = msdf_key(&layout.glyphs[0], 32.0, 4.0);
      assert!(!key.skew && !key.embolden, "one image, upright");
      let (effects, icons) = (TextEffects::default(), TextIcons::default());
      let sources = TextSources {
        fonts: &fonts,
        engine: &engine,
        effects: &effects,
        icons: &icons,
      };
      let mut warned = HashSet::new();
      for glyph in &layout.glyphs {
        warn_msdf_synthesis(&sources, &mut warned, glyph);
      }
      let warnings = crate::core::dev_log::get_entries()
        .iter()
        .filter(|e| {
          e.message
            .contains("MSDF font \"Inter\" has no italic or bold face")
        })
        .count();
      assert_eq!(warnings, 1);
    }

    #[test]
    fn an_msdf_effect_past_the_range_warns_once_per_font() {
      let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
        eprintln!("skipping: no GPU available");
        return;
      };
      let mut fonts = Fonts::with_builtins();
      let mono = fonts.role(FontRole::Mono).unwrap();
      fonts.set_render(
        mono,
        FontRender::Msdf {
          base: 32.0,
          range: 6.0,
        },
      );
      let mut engine = TextEngine::new();
      engine.sync(&fonts);
      // A 6 px outline on 32 px text: twice what a 6 px range reaches
      let style = TextStyle::default()
        .role(FontRole::Mono)
        .size(32.0)
        .outline(6.0, Color::BLACK)
        .glow(12.0, Color::WHITE);
      let layout = laid(&mut engine, "wide effects", &style);
      let (effects, icons) = (TextEffects::default(), TextIcons::default());
      let sources = TextSources {
        fonts: &fonts,
        engine: &engine,
        effects: &effects,
        icons: &icons,
      };
      let mut sprites = SpriteGpuRenderer::new(&device, &queue);
      let (mut raster, mut list) = (GlyphRasterizer::new(), DrawList::default());
      let mut warned = HashSet::new();
      let mut targets = TextTargets {
        raster: &mut raster,
        sprites: &mut sprites,
        device: &device,
        queue: &queue,
        list: &mut list,
        warned: &mut warned,
      };
      for _ in 0..2 {
        emit_text(&sources, &mut targets, &layout, &placement(None));
      }
      let warnings = crate::core::dev_log::get_entries()
        .iter()
        .filter(|e| {
          e.message
            .contains("MSDF font \"JetBrains Mono\" (range 6 px)")
        })
        .count();
      assert_eq!(warnings, 1, "once for the font, over every glyph and frame");
    }

    #[test]
    fn a_new_text_restarts_its_reveal_and_a_new_colour_does_not() {
      let mut world = World::new();
      world.insert_resource(Fonts::with_builtins());
      let mut engine = TextEngine::new();
      let e = world
        .spawn((
          Transform::default(),
          Text2d::new("Hello"),
          TextReveal::new(10.0),
        ))
        .id();
      layout_world_text(&mut world, &mut engine, &PromptContext::default());
      let shown = |world: &World| world.get::<TextReveal>(e).unwrap().shown();
      world.get_mut::<TextReveal>(e).unwrap().clock.shown = 3.0;
      world.get_mut::<Text2d>(e).unwrap().style.color = Color::rgb(255, 0, 0);
      layout_world_text(&mut world, &mut engine, &PromptContext::default());
      assert_eq!(shown(&world), 3.0, "a new colour keeps the reveal");
      world.get_mut::<Text2d>(e).unwrap().set_text("Goodbye");
      layout_world_text(&mut world, &mut engine, &PromptContext::default());
      assert_eq!(shown(&world), 0.0, "a new text starts it over");
    }

    #[test]
    fn a_new_anchor_moves_the_bounds() {
      let mut world = World::new();
      world.insert_resource(Fonts::with_builtins());
      let mut engine = TextEngine::new();
      let e = world
        .spawn((Transform::default(), Text2d::new("Hello")))
        .id();
      layout_world_text(&mut world, &mut engine, &PromptContext::default());
      let size = world.get::<TextBounds>(e).unwrap().size;
      world.get_mut::<Text2d>(e).unwrap().layout.anchor = Anchor::BottomRight;
      layout_world_text(&mut world, &mut engine, &PromptContext::default());
      assert_eq!(world.get::<TextBounds>(e).unwrap().offset, -size);
    }
  }
}
