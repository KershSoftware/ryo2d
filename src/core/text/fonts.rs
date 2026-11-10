//! The font registry text lays out with (`ctx.fonts()`): the faces of the
//! font assets the game holds and of the built-in fonts, and how each face
//! draws. It follows the font stores (`Assets<Font>`, `Assets<BitmapFont>`)
//! right after the asset pump each frame, at the start of each UI pass and
//! whenever a scene takes `ctx.fonts()`; the text engine picks up a change
//! when it next syncs (it compares `generation`, which moves when a face or
//! role changes, and `slot_version`, which moves when a font asset's
//! registration does).

use std::collections::HashMap;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use swash::{FontRef, StringId};
use thiserror::Error;

use crate::core::asset::assets::follower_token;
use crate::core::asset::{Asset, AssetId, Assets, Handle};
use crate::core::ecs::Resource;
use crate::core::ecs::kernel::{DetectChangesMut, World};
use crate::core::graphics::Image;

/// One registered font face: a font asset's face, a bitmap font's, or a
/// built-in one. Ids are never reused, so one names a single face for the
/// registry's life
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FontId(pub(crate) u32);

/// How a font's glyphs are drawn
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[non_exhaustive]
pub enum FontRender {
  /// Hinted and anti-aliased, rasterized at the exact pixel size
  #[default]
  Raster,
  /// No anti-aliasing: glyphs land on whole pixels. Text in a pixel face
  /// is laid out at the whole multiple of `design_size` (in layout pixels,
  /// at least one) nearest the size asked for: with a 16 px design, sizes
  /// under 24 px draw at 16 and 24 to 39 px at 32. Spans that set a pixel
  /// face, or a size in one, snap too. Measuring, carets and line heights
  /// use the snapped size.
  Pixel { design_size: f32 },
  /// A multi-channel signed distance field made once at `base` pixels with
  /// a distance `range` (pixels at that size): scales freely, and the
  /// shader draws outlines, soft shadows and glow
  Msdf { base: f32, range: f32 },
}

impl FontRender {
  /// A pixel font designed at `design_size` pixels
  pub const fn pixel(design_size: f32) -> Self {
    Self::Pixel { design_size }
  }

  /// MSDF at the default base (32 px) and range (4 px)
  pub const fn msdf() -> Self {
    Self::Msdf {
      base: 32.0,
      range: 4.0,
    }
  }
}

/// The built-in roles a style can name instead of a family
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FontRole {
  /// The UI and world font (Inter)
  #[default]
  Ui,
  /// The monospace font (JetBrains Mono): console, overlays, dev windows
  Mono,
  /// The pixel font (m5x7)
  Pixel,
}

impl FontRole {
  pub(crate) fn index(self) -> usize {
    match self {
      Self::Ui => 0,
      Self::Mono => 1,
      Self::Pixel => 2,
    }
  }
}

/// Why a font was refused
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum FontError {
  #[error("not a TrueType or OpenType font")]
  Invalid,
  #[error("the font file has no face {0}")]
  NoFace(u32),
  #[error("a bitmap font needs a non-empty cell and at least one character")]
  EmptyBitmap,
}

/// One font face: a TrueType or OpenType file, or one face of a collection
/// (`.ttc`, the face chosen by `FontSettings`). Load one with
/// `ctx.assets().load_now::<Font>("fonts/Game.ttf")`, or embed one, which
/// the web build needs (it has no files):
///
/// ```
/// use ryo2d::prelude::*;
///
/// fn use_game_font(ctx: &mut SceneContext<'_>, bytes: &'static [u8]) -> Result<(), FontError> {
///   let font = ctx.assets().add(Font::from_bytes(bytes)?);
///   ctx.fonts().set_role(FontRole::Ui, font);
///   Ok(())
/// }
/// ```
///
/// Its face is registered with the text system while a handle holds it,
/// and leaves it, with its glyphs, once the font is freed. Text reaches it
/// by its family (`TextStyle::family`), through a role (`Fonts::set_role`,
/// which holds it) or exactly (`TextFont::Face`, which holds it too). A
/// changed or reloaded font draws its new glyphs; text using it lays out
/// again. Two fonts of the same bytes and face share one face
#[derive(Clone)]
pub struct Font {
  data: Arc<[u8]>,
  index: u32,
  family: Arc<str>,
}

impl Asset for Font {}

impl Font {
  /// A TrueType or OpenType font (face 0 of a collection)
  pub fn from_bytes(data: impl Into<Arc<[u8]>>) -> Result<Font, FontError> {
    Self::from_bytes_face(data, 0)
  }

  /// Face `index` of a font collection (or 0 of a single font)
  pub fn from_bytes_face(data: impl Into<Arc<[u8]>>, index: u32) -> Result<Font, FontError> {
    let data = data.into();
    let family = {
      let font = FontRef::from_index(&data, index as usize).ok_or(if index == 0 {
        FontError::Invalid
      } else {
        FontError::NoFace(index)
      })?;
      family_name(&font).ok_or(FontError::Invalid)?
    };
    Ok(Font {
      data,
      index,
      family: family.into(),
    })
  }

  /// The family text names it by
  pub fn family(&self) -> &str {
    &self.family
  }
}

// The font bytes are left out: a font file is hundreds of kilobytes
impl fmt::Debug for Font {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("Font")
      .field("family", &self.family)
      .field("face", &self.index)
      .field("bytes", &self.data.len())
      .finish()
  }
}

/// A font drawn from a sheet image of fixed cells: `chars` names the cells
/// in order, left to right, then top to bottom, in as many columns as the
/// sheet is wide. Load one from a `.bitmap.ron` descriptor, whose sheet is a
/// dependency (`(sheet: "fonts/pixel.png", cell: (8, 8), chars: " !..",
/// family: "Pixel")`), or make one from an image the game holds with
/// [`BitmapFont::new`] and `ctx.assets().add(..)`.
///
/// Text reaches it by its family (`TextStyle::family`) while a handle holds
/// it. It draws as a pixel font designed at its cell's height, its cells
/// taken from the sheet as it is now, so a changed or reloaded sheet
/// redraws its text. One with an empty cell or no characters is not
/// registered (warned)
#[derive(Debug, Clone)]
pub struct BitmapFont {
  /// The family text names it by
  pub family: String,
  /// The sheet its cells are on; the font holds it
  pub sheet: Handle<Image>,
  /// A cell's width and height in texels
  pub cell: (u32, u32),
  /// The cells' characters, in order
  pub chars: String,
}

impl Asset for BitmapFont {}

impl BitmapFont {
  /// A bitmap font of `family` with `cell`-sized cells on `sheet`
  pub fn new(
    family: impl Into<String>,
    sheet: Handle<Image>,
    cell: (u32, u32),
    chars: impl Into<String>,
  ) -> BitmapFont {
    BitmapFont {
      family: family.into(),
      sheet,
      cell,
      chars: chars.into(),
    }
  }

  /// Whether it has cells to draw
  pub(crate) fn is_drawable(&self) -> bool {
    self.cell.0 > 0 && self.cell.1 > 0 && !self.chars.is_empty()
  }
}

/// A grid of glyphs on a sheet image: `chars[i]` is cell `i`, row-major, in
/// as many columns as the image is wide
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BitmapFace {
  pub(crate) sheet: AssetId<Image>,
  pub(crate) cell: (u32, u32),
  pub(crate) chars: Vec<char>,
}

/// Where a face's glyphs come from
#[derive(Clone)]
pub(crate) enum FaceSource {
  Outline { data: Arc<[u8]>, index: u32 },
  Bitmap(BitmapFace),
}

impl FaceSource {
  /// Whether `font` is this face's data
  fn is_font(&self, font: &Font) -> bool {
    matches!(self, Self::Outline { data, index }
      if *index == font.index && (Arc::ptr_eq(data, &font.data) || **data == *font.data))
  }
}

// The font bytes are left out: a font file is hundreds of kilobytes
impl fmt::Debug for FaceSource {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::Outline { data, index } => f
        .debug_struct("Outline")
        .field("bytes", &data.len())
        .field("index", index)
        .finish(),
      Self::Bitmap(bitmap) => f.debug_tuple("Bitmap").field(bitmap).finish(),
    }
  }
}

/// One registered face
#[derive(Debug, Clone)]
pub(crate) struct Face {
  pub(crate) family: String,
  pub(crate) weight: f32,
  pub(crate) italic: bool,
  pub(crate) source: FaceSource,
  pub(crate) render: FontRender,
  /// Unique in the process and new whenever the face's data is replaced:
  /// the glyph atlas and swash's scaler cache key on it, so neither keeps
  /// what was made from the old data
  pub(crate) serial: u64,
  /// The font assets registered as this face (two of the same data share it)
  holders: u32,
  /// An outline face's key in `Fonts::by_data`: a hash of its bytes, and
  /// its face index
  key: Option<(u64, u32)>,
}

/// A face serial no face has had in this process
fn next_face_serial() -> u64 {
  static NEXT: AtomicU64 = AtomicU64::new(1);
  NEXT.fetch_add(1, Ordering::Relaxed)
}

/// A request to build glyphs ahead of their first use
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Prewarm {
  pub(crate) font: FontId,
  pub(crate) size: f32,
  pub(crate) chars: String,
}

/// The font a role uses, and the face it draws with: its font's, or until
/// that is registered the face the role had before, whose font it holds
/// meanwhile so it is not freed
#[derive(Clone)]
struct RoleFont {
  font: Handle<Font>,
  face: Option<FontId>,
  kept: Option<Handle<Font>>,
}

/// The faces of the fonts in a followed store, by slot: each live asset's
/// generation and the face it registered as
type Slots = Vec<Option<(u32, FontId)>>;

/// The built-in fonts' handles (feature `default-fonts`): the engine holds
/// them, so they are never freed whatever the roles are pointed at
#[derive(Resource, Clone, Default, Debug)]
pub(crate) struct BuiltinFonts {
  pub(crate) ui: Option<Handle<Font>>,
  // Held, not read outside tests: no role keeps Inter Bold
  #[cfg_attr(not(test), allow(dead_code))]
  pub(crate) bold: Option<Handle<Font>>,
  pub(crate) mono: Option<Handle<Font>>,
  pub(crate) pixel: Option<Handle<Font>>,
}

/// The fonts text can use: the built-in ones (feature `default-fonts`) and
/// the font assets the game adds or loads. A resource: `ctx.fonts()` in
/// scenes, `ResMut<Fonts>` in systems.
///
/// The registry follows the world's font stores: a font is registered once
/// it is in `Assets<Font>` (a scene's `ctx.fonts()` sees one it has just
/// added; a system sees it from the next frame) and unregistered when it is
/// freed. Its glyphs then leave the glyph atlas. Faces are registered in
/// the order their fonts arrive, the built-ins first; a family's faces are
/// told apart by weight and slant, and at a tie the face registered first
/// wins.
///
/// `FontId`s are its indices, never reused, which the glyph caches key on,
/// so there is no public constructor. Registering is idempotent: two fonts
/// of the same bytes and face index, or bitmap fonts of the same family,
/// sheet, cell and characters, share one face, and a `set_role` or
/// `set_render` that changes nothing bumps nothing. A font that comes to
/// share a face, or stops, moves `slot_version` instead of `generation`:
/// text lays out again (a `TextFont::Face` may resolve otherwise), but the
/// faces stay as they are.
#[derive(Resource)]
pub struct Fonts {
  faces: Vec<Option<Face>>,
  roles: [Option<RoleFont>; 3],
  generation: u64,
  prewarm: Vec<Prewarm>,
  /// Outline faces by a hash of their bytes, and their face index
  by_data: HashMap<(u64, u32), FontId>,
  /// The followed `Assets<Font>`'s fonts and `Assets<BitmapFont>`'s
  fonts: Slots,
  bitmaps: Slots,
  /// Moves on whenever a font asset's registration changes (registered,
  /// sharing a face, changed or freed), which `generation` does not always
  /// see: what `TextFont::Face` resolves to changes with it
  slot_version: u64,
  /// The stores followed (`Assets::serial`), and the registry's token as
  /// their follower
  font_store: Option<u64>,
  bitmap_store: Option<u64>,
  follower: u64,
}

// Faces are listed by name: their font bytes are not printed
impl fmt::Debug for Fonts {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let faces: Vec<_> = self
      .faces()
      .map(|(id, face)| (id, &face.family, face.weight, face.italic, face.render))
      .collect();
    let roles = [FontRole::Ui, FontRole::Mono, FontRole::Pixel].map(|role| self.role(role));
    f.debug_struct("Fonts")
      .field("faces", &faces)
      .field("roles", &roles)
      .field("generation", &self.generation)
      .field("prewarm", &self.prewarm.len())
      .finish_non_exhaustive()
  }
}

impl Fonts {
  /// An empty registry (no built-in fonts), following no store yet
  pub(crate) fn new() -> Self {
    Self {
      faces: Vec::new(),
      roles: [None, None, None],
      generation: 0,
      prewarm: Vec::new(),
      by_data: HashMap::new(),
      fonts: Vec::new(),
      bitmaps: Vec::new(),
      slot_version: 0,
      font_store: None,
      bitmap_store: None,
      follower: follower_token(),
    }
  }

  /// A registry of the built-in fonts (feature `default-fonts`) in a store
  /// of its own, for text that has no world (the dev windows, tests). It
  /// must not follow a world's stores: their ids are another store's
  pub(crate) fn with_builtins() -> Self {
    let mut fonts = Self::new();
    let mut store = Assets::<Font>::new();
    let _ = builtins::add(&mut fonts, &mut store);
    fonts
  }

  /// Change how a face draws; text using it lays out again (the mode it
  /// already has changes nothing)
  pub fn set_render(&mut self, font: FontId, render: FontRender) {
    if let Some(face) = self.face_mut(font)
      && !same_render(face.render, render)
    {
      face.render = render;
      self.generation += 1;
    }
  }

  /// How a face draws (`Raster` for an unknown id)
  pub fn render(&self, font: FontId) -> FontRender {
    self
      .face(font)
      .map_or(FontRender::Raster, |face| face.render)
  }

  /// The family a face belongs to (`None` once its font is freed)
  pub fn family(&self, font: FontId) -> Option<&str> {
    self.face(font).map(|face| face.family.as_str())
  }

  /// The face a font registered as: `None` while it is not loaded, and
  /// once it is freed
  pub fn face_of(&self, font: impl Into<AssetId<Font>>) -> Option<FontId> {
    self.slot_face(&self.fonts, font.into())
  }

  /// A face of `family`, the regular (weight 400, upright) one if there
  /// is one
  pub fn find(&self, family: &str) -> Option<FontId> {
    self.find_face(family, 400.0, false)
  }

  /// The face of `family` nearest `weight` (100 to 900), preferring the
  /// slant asked for: `find_face("Inter", 700.0, false)` is Inter Bold. At
  /// equal distance (say 500 between a 400 and a 600 face) the face added
  /// first wins.
  pub fn find_face(&self, family: &str, weight: f32, italic: bool) -> Option<FontId> {
    let mut best: Option<(FontId, f32)> = None;
    for (id, face) in self.faces() {
      if face.family != family {
        continue;
      }
      let slant = if face.italic == italic { 0.0 } else { 1000.0 };
      let distance = (face.weight - weight).abs() + slant;
      if best.is_none_or(|(_, d)| distance < d) {
        best = Some((id, distance));
      }
    }
    best.map(|(id, _)| id)
  }

  /// The face a role draws with
  pub fn role(&self, role: FontRole) -> Option<FontId> {
    self.roles[role.index()].as_ref().and_then(|role| role.face)
  }

  /// The font a role uses
  pub fn role_font(&self, role: FontRole) -> Option<&Handle<Font>> {
    self.roles[role.index()].as_ref().map(|role| &role.font)
  }

  /// Use `font` for a role (e.g. a game's own UI font); the role holds it,
  /// so it stays loaded while the role uses it. A font not registered yet
  /// (still loading) takes over once it is; until then the role keeps the
  /// face it had, and holds that face's font, so it is not freed meanwhile.
  /// Setting the font the role already uses changes nothing
  pub fn set_role(&mut self, role: FontRole, font: Handle<Font>) {
    let slot = role.index();
    if self.roles[slot]
      .as_ref()
      .is_some_and(|role| role.font == font)
    {
      return;
    }
    let before = self.role(role);
    let registered = self.face_of(&font);
    // The font the role draws with until the new one registers
    let kept = match (registered, self.roles[slot].take()) {
      (None, Some(old)) if self.face_of(&old.font).is_some() => Some(old.font),
      (None, Some(old)) => old.kept,
      _ => None,
    };
    let face = registered.or(before);
    self.roles[slot] = Some(RoleFont { font, face, kept });
    if face != before {
      self.generation += 1;
    }
  }

  /// Build these glyphs ahead of their first use (MSDF glyphs cost about half
  /// a millisecond each to make): call it in a loading scene
  pub fn prewarm(&mut self, font: FontId, size: f32, chars: &str) {
    self.prewarm.push(Prewarm {
      font,
      size,
      chars: chars.to_string(),
    });
  }

  /// Changes whenever a face is added, changed or removed, or a role or
  /// render mode changes
  pub fn generation(&self) -> u64 {
    self.generation
  }

  /// A hash of each bitmap face's sheet version (its id, and `version` of
  /// it: the version its texture shows): changes when a sheet is changed
  /// or reloaded, which `generation` does not see (0 with no bitmap faces)
  pub(crate) fn bitmap_stamp(&self, version: impl Fn(AssetId<Image>) -> Option<u64>) -> u64 {
    let mut hasher = crate::core::hash::FastHasher::default();
    let mut any = false;
    for (_, face) in self.faces() {
      if let FaceSource::Bitmap(bitmap) = &face.source {
        any = true;
        bitmap.sheet.hash(&mut hasher);
        version(bitmap.sheet).hash(&mut hasher);
      }
    }
    if any { hasher.finish() } else { 0 }
  }

  pub(crate) fn face(&self, font: FontId) -> Option<&Face> {
    self.faces.get(font.0 as usize)?.as_ref()
  }

  fn face_mut(&mut self, font: FontId) -> Option<&mut Face> {
    self.faces.get_mut(font.0 as usize)?.as_mut()
  }

  /// The live faces, in registration order
  pub(crate) fn faces(&self) -> impl Iterator<Item = (FontId, &Face)> {
    self
      .faces
      .iter()
      .enumerate()
      .filter_map(|(i, face)| Some((FontId(u32::try_from(i).ok()?), face.as_ref()?)))
  }

  /// A face's serial (`None`: no such face, or freed)
  pub(crate) fn face_serial(&self, font: FontId) -> Option<u64> {
    self.face(font).map(|face| face.serial)
  }

  /// The faces of the followed store's fonts, by slot (the text engine
  /// resolves `TextFont::Face` with them)
  pub(crate) fn font_slots(&self) -> &[Option<(u32, FontId)>] {
    &self.fonts
  }

  /// Moves on whenever `font_slots` changes
  pub(crate) fn slot_version(&self) -> u64 {
    self.slot_version
  }

  /// The face a bitmap font registered as (tests)
  #[cfg(test)]
  pub(crate) fn bitmap_face_of(&self, font: impl Into<AssetId<BitmapFont>>) -> Option<FontId> {
    let font = font.into();
    let (generation, face) = (*self.bitmaps.get(font.index() as usize)?)?;
    (generation == font.generation() && self.face(face).is_some()).then_some(face)
  }

  /// The glyphs asked for ahead, once (the renderer builds them)
  pub(crate) fn take_prewarm(&mut self) -> Vec<Prewarm> {
    std::mem::take(&mut self.prewarm)
  }

  fn slot_face(&self, slots: &Slots, font: AssetId<Font>) -> Option<FontId> {
    let (generation, face) = (*slots.get(font.index() as usize)?)?;
    (generation == font.generation() && self.face(face).is_some()).then_some(face)
  }

  /// Follows `store` alone (the built-ins, and tests)
  pub(crate) fn follow_fonts(&mut self, store: &mut Assets<Font>) {
    let fonts = Changed::take(store, self.follower, self.font_store);
    self.apply(StoreChanges {
      fonts,
      bitmaps: Changed::default(),
    });
  }

  /// Brings the registry in line with what changed in the stores
  fn apply(&mut self, changes: StoreChanges) {
    let StoreChanges { fonts, bitmaps } = changes;
    if let Some((serial, whole)) = fonts.store {
      // Another store's ids are not this one's: start over
      if whole {
        for (_, face) in std::mem::take(&mut self.fonts).into_iter().flatten() {
          self.release(face);
          self.slot_version += 1;
        }
      }
      self.font_store = Some(serial);
      for (index, now) in fonts.slots {
        self.font_slot(index, now);
      }
    }
    if let Some((serial, whole)) = bitmaps.store {
      if whole {
        for (_, face) in std::mem::take(&mut self.bitmaps).into_iter().flatten() {
          self.release(face);
        }
      }
      self.bitmap_store = Some(serial);
      for (index, now) in bitmaps.slots {
        self.bitmap_slot(index, now);
      }
    }
    self.resolve_roles();
  }

  /// Slot `index` of the font store holds `now` (its generation and font)
  fn font_slot(&mut self, index: u32, now: Option<(u32, Font)>) {
    let at = index as usize;
    let held = self.fonts.get(at).copied().flatten();
    match (held, now) {
      // The same asset, changed in place or reloaded
      (Some((generation, face)), Some((now_generation, font))) if generation == now_generation => {
        let face = self.change_font(face, &font);
        self.set_font_slot(at, face.map(|face| (generation, face)));
      }
      (held, now) => {
        if let Some((_, face)) = held {
          self.set_font_slot(at, None);
          self.release(face);
        }
        if let Some((generation, font)) = now {
          let face = self.hold_font(&font, FontRender::Raster);
          self.set_font_slot(at, face.map(|face| (generation, face)));
        }
      }
    }
  }

  /// Slot `at` of the font store's table holds `value`
  fn set_font_slot(&mut self, at: usize, value: Option<(u32, FontId)>) {
    if set_slot(&mut self.fonts, at, value) {
      self.slot_version += 1;
    }
  }

  /// Slot `index` of the bitmap font store holds `now`
  fn bitmap_slot(&mut self, index: u32, now: Option<(u32, BitmapFont)>) {
    let at = index as usize;
    let held = self.bitmaps.get(at).copied().flatten();
    match (held, now) {
      // The same asset, changed in place, reloaded, or told its sheet did
      (Some((generation, face)), Some((now_generation, font))) if generation == now_generation => {
        let face = self.change_bitmap(face, &font);
        set_slot(&mut self.bitmaps, at, face.map(|face| (generation, face)));
      }
      (held, now) => {
        if let Some((_, face)) = held {
          set_slot(&mut self.bitmaps, at, None);
          self.release(face);
        }
        if let Some((generation, font)) = now {
          let face = self.hold_bitmap(&font);
          set_slot(&mut self.bitmaps, at, face.map(|face| (generation, face)));
        }
      }
    }
  }

  /// The face `font` registers as: the face of the same data if there is
  /// one, else a new face drawn with `render`
  fn hold_font(&mut self, font: &Font, render: FontRender) -> Option<FontId> {
    let key = (hash_bytes(&font.data), font.index);
    if let Some(&id) = self.by_data.get(&key)
      && let Some(face) = self.face_mut(id)
      && face.source.is_font(font)
    {
      face.holders += 1;
      return Some(id);
    }
    let Some((weight, italic)) = attributes(font) else {
      log::warn!("font {:?} could not be read; text falls back", font.family);
      return None;
    };
    let id = self.push(Face {
      family: font.family.to_string(),
      weight,
      italic,
      source: FaceSource::Outline {
        data: Arc::clone(&font.data),
        index: font.index,
      },
      render,
      serial: next_face_serial(),
      holders: 1,
      key: Some(key),
    });
    self.by_data.insert(key, id);
    Some(id)
  }

  /// A font registered as `face` changed to `font`: a face it holds alone
  /// takes the new data in place (keeping its id and render mode); a shared
  /// one is let go for a face of its own. The face it is registered as now
  fn change_font(&mut self, face: FontId, font: &Font) -> Option<FontId> {
    let Some(known) = self.face(face) else {
      return self.hold_font(font, FontRender::Raster);
    };
    if known.source.is_font(font) {
      return Some(face);
    }
    let render = known.render;
    if known.holders > 1 {
      self.release(face);
      return self.hold_font(font, render);
    }
    let Some((weight, italic)) = attributes(font) else {
      log::warn!("font {:?} could not be read; text falls back", font.family);
      self.release(face);
      return None;
    };
    self.forget_key(face);
    let key = (hash_bytes(&font.data), font.index);
    self.by_data.entry(key).or_insert(face);
    if let Some(known) = self.face_mut(face) {
      known.family = font.family.to_string();
      known.weight = weight;
      known.italic = italic;
      known.source = FaceSource::Outline {
        data: Arc::clone(&font.data),
        index: font.index,
      };
      known.serial = next_face_serial();
      known.key = Some(key);
    }
    self.generation += 1;
    Some(face)
  }

  /// A bitmap font registered as `face` changed to `font`: what it draws is
  /// the same (a sheet that reloaded is the same sheet), or a face it holds
  /// alone takes the new cells in place, or a shared one is let go for a
  /// face of its own. The face it is registered as now
  fn change_bitmap(&mut self, face: FontId, font: &BitmapFont) -> Option<FontId> {
    let bitmap = bitmap_face(font);
    let Some(known) = self.face(face) else {
      return self.hold_bitmap(font);
    };
    let same = known.family == font.family
      && matches!(&known.source, FaceSource::Bitmap(known) if *known == bitmap);
    if same && font.is_drawable() {
      return Some(face);
    }
    if known.holders > 1 || !font.is_drawable() {
      self.release(face);
      return self.hold_bitmap(font);
    }
    if let Some(known) = self.face_mut(face) {
      known.family = font.family.clone();
      known.source = FaceSource::Bitmap(bitmap);
      known.render = FontRender::pixel(font.cell.1 as f32);
      known.serial = next_face_serial();
    }
    self.generation += 1;
    Some(face)
  }

  /// The face a bitmap font registers as: the face of the same family,
  /// sheet, cell and characters if there is one, else a new one. `None` for
  /// one with nothing to draw (warned)
  fn hold_bitmap(&mut self, font: &BitmapFont) -> Option<FontId> {
    if !font.is_drawable() {
      log::warn!(
        "bitmap font {:?} has an empty cell or no characters; it is not registered",
        font.family
      );
      return None;
    }
    let bitmap = bitmap_face(font);
    let known = self.faces().find_map(|(id, face)| {
      (face.family == font.family
        && matches!(&face.source, FaceSource::Bitmap(known) if *known == bitmap))
      .then_some(id)
    });
    if let Some(id) = known
      && let Some(face) = self.face_mut(id)
    {
      face.holders += 1;
      return Some(id);
    }
    Some(self.push(Face {
      family: font.family.clone(),
      weight: 400.0,
      italic: false,
      source: FaceSource::Bitmap(bitmap),
      render: FontRender::pixel(font.cell.1 as f32),
      serial: next_face_serial(),
      holders: 1,
      key: None,
    }))
  }

  /// One holder of `face` goes; the face goes with its last
  fn release(&mut self, face: FontId) {
    let Some(known) = self.face_mut(face) else {
      return;
    };
    known.holders = known.holders.saturating_sub(1);
    if known.holders > 0 {
      return;
    }
    self.forget_key(face);
    if let Some(slot) = self.faces.get_mut(face.0 as usize) {
      *slot = None;
    }
    self.generation += 1;
  }

  /// `face`'s data key no longer finds it
  fn forget_key(&mut self, face: FontId) {
    if let Some(key) = self.face(face).and_then(|known| known.key)
      && self.by_data.get(&key) == Some(&face)
    {
      self.by_data.remove(&key);
    }
  }

  /// Each role draws with its font's face once that is registered (and
  /// lets the font it kept meanwhile go), and with no face once the one it
  /// kept is gone
  fn resolve_roles(&mut self) {
    for at in 0..self.roles.len() {
      let Some(role) = self.roles[at].as_ref() else {
        continue;
      };
      let registered = self.face_of(&role.font);
      let kept = role.face.filter(|&face| self.face(face).is_some());
      let face = registered.or(kept);
      if let Some(role) = self.roles[at].as_mut() {
        if registered.is_some() {
          role.kept = None;
        }
        if role.face != face {
          role.face = face;
          self.generation += 1;
        }
      }
    }
  }

  fn push(&mut self, face: Face) -> FontId {
    let id = FontId(u32::try_from(self.faces.len()).unwrap_or(u32::MAX));
    self.faces.push(Some(face));
    self.generation += 1;
    id
  }
}

/// Slot `at` of `slots` holds `value`; whether that changed it
fn set_slot(slots: &mut Slots, at: usize, value: Option<(u32, FontId)>) -> bool {
  if slots.len() <= at {
    if value.is_none() {
      return false;
    }
    slots.resize(at + 1, None);
  }
  match slots.get_mut(at) {
    Some(slot) if *slot != value => {
      *slot = value;
      true
    }
    _ => false,
  }
}

/// What changed in one store since the registry last followed it
struct Changed<T> {
  /// The store's serial, and whether every slot is given (a store not
  /// followed before); `None`: no store
  store: Option<(u64, bool)>,
  /// The slots changed, in the order they changed: each live asset's
  /// generation and value
  slots: Vec<(u32, Option<(u32, T)>)>,
}

impl<T> Default for Changed<T> {
  fn default() -> Self {
    Self {
      store: None,
      slots: Vec::new(),
    }
  }
}

impl<T: Asset + Clone> Changed<T> {
  /// What changed in `store` since `follower`, following the store
  /// `followed`, last looked: allocates only when something did
  fn take(store: &mut Assets<T>, follower: u64, followed: Option<u64>) -> Self {
    let whole = followed != Some(store.serial()) || !store.is_followed_by(follower);
    let mut slots = Vec::new();
    store.take_changes(follower, whole, |index, now| {
      slots.push((index, now.map(|now| (now.generation, now.value.clone()))));
    });
    Self {
      store: Some((store.serial(), whole)),
      slots,
    }
  }

  fn is_empty(&self) -> bool {
    self.slots.is_empty() && self.store.is_none_or(|(_, whole)| !whole)
  }
}

/// What changed in the font stores
struct StoreChanges {
  fonts: Changed<Font>,
  bitmaps: Changed<BitmapFont>,
}

/// Brings the world's `Fonts` in line with its font stores: fonts added,
/// changed or freed since it last did. The engine runs it right after the
/// asset pump (so a freed font's face goes before the frame's text is laid
/// out), at the start of each UI pass and in `ctx.fonts()`. Nothing changed,
/// it costs a few lookups and allocates nothing
pub(crate) fn fonts_follow_assets(world: &mut World) {
  let Some(fonts) = world.get_resource::<Fonts>() else {
    return;
  };
  let (follower, font_store, bitmap_store) = (fonts.follower, fonts.font_store, fonts.bitmap_store);
  let mut changes = StoreChanges {
    fonts: Changed::default(),
    bitmaps: Changed::default(),
  };
  // Following a store changes nothing a reader of the store can see
  if let Some(mut store) = world.get_resource_mut::<Assets<Font>>() {
    changes.fonts = Changed::take(store.bypass_change_detection(), follower, font_store);
  }
  if let Some(mut store) = world.get_resource_mut::<Assets<BitmapFont>>() {
    changes.bitmaps = Changed::take(store.bypass_change_detection(), follower, bitmap_store);
  }
  if changes.fonts.is_empty() && changes.bitmaps.is_empty() {
    return;
  }
  if let Some(mut fonts) = world.get_resource_mut::<Fonts>() {
    fonts.apply(changes);
  }
}

/// Puts the engine's fonts in `world`: the built-in fonts (feature
/// `default-fonts`) added to its `Assets<Font>`, held by `BuiltinFonts`,
/// and the `Fonts` registry following its stores
pub(crate) fn insert_fonts(world: &mut World) {
  let mut fonts = Fonts::new();
  let builtins = match world.get_resource_mut::<Assets<Font>>() {
    Some(mut store) => builtins::add(&mut fonts, store.bypass_change_detection()),
    None => BuiltinFonts::default(),
  };
  world.insert_resource(builtins);
  world.insert_resource(fonts);
}

/// What a bitmap font draws, as a face holds it
fn bitmap_face(font: &BitmapFont) -> BitmapFace {
  BitmapFace {
    sheet: font.sheet.id(),
    cell: font.cell,
    chars: font.chars.chars().collect(),
  }
}

/// A face's weight and slant, read from its font
fn attributes(font: &Font) -> Option<(f32, bool)> {
  let face = FontRef::from_index(&font.data, font.index as usize)?;
  let attributes = face.attributes();
  Some((
    f32::from(attributes.weight().0),
    !matches!(attributes.style(), swash::Style::Normal),
  ))
}

/// Whether two render modes are the same, bit for bit (so a mode with a
/// value that is not a number equals itself)
fn same_render(a: FontRender, b: FontRender) -> bool {
  match (a, b) {
    (FontRender::Raster, FontRender::Raster) => true,
    (FontRender::Pixel { design_size: x }, FontRender::Pixel { design_size: y }) => {
      x.to_bits() == y.to_bits()
    }
    (
      FontRender::Msdf {
        base: b1,
        range: r1,
      },
      FontRender::Msdf {
        base: b2,
        range: r2,
      },
    ) => (b1.to_bits(), r1.to_bits()) == (b2.to_bits(), r2.to_bits()),
    _ => false,
  }
}

/// A hash of a font's bytes, to find a face of the same data
fn hash_bytes(data: &[u8]) -> u64 {
  let mut h = crate::core::hash::FastHasher::default();
  data.hash(&mut h);
  h.finish()
}

/// The typographic family (name 16) if the font has one, else its family
/// (name 1)
fn family_name(font: &FontRef<'_>) -> Option<String> {
  let strings = font.localized_strings();
  strings
    .find_by_id(StringId::TypographicFamily, None)
    .or_else(|| strings.find_by_id(StringId::Family, None))
    .map(|name| name.chars().collect::<String>())
    .filter(|name| !name.is_empty())
}

mod builtins {
  use super::{Assets, BuiltinFonts, Font, Fonts};
  #[cfg(feature = "default-fonts")]
  use super::{FontRender, FontRole};

  #[cfg(feature = "default-fonts")]
  const INTER_REGULAR: &[u8] = include_bytes!("../../assets/fonts/Inter-Regular.ttf");
  #[cfg(feature = "default-fonts")]
  const INTER_BOLD: &[u8] = include_bytes!("../../assets/fonts/Inter-Bold.ttf");
  #[cfg(feature = "default-fonts")]
  const JETBRAINS_MONO: &[u8] = include_bytes!("../../assets/fonts/JetBrainsMono-Regular.ttf");
  #[cfg(feature = "default-fonts")]
  const M5X7: &[u8] = include_bytes!("../../assets/fonts/m5x7.ttf");

  /// Adds the built-in fonts to `store`, in order, registers them in
  /// `fonts` (which then follows `store`) and points the roles at them.
  /// Without `default-fonts` it adds nothing
  pub(super) fn add(fonts: &mut Fonts, store: &mut Assets<Font>) -> BuiltinFonts {
    #[cfg(feature = "default-fonts")]
    {
      // A built-in font that fails to parse is left out (its role stays
      // empty and text using it falls back), never a panic
      let mut add = |bytes: &'static [u8]| Font::from_bytes(bytes).ok().map(|font| store.add(font));
      let builtins = BuiltinFonts {
        ui: add(INTER_REGULAR),
        bold: add(INTER_BOLD),
        mono: add(JETBRAINS_MONO),
        pixel: add(M5X7),
      };
      fonts.follow_fonts(store);
      if let Some(ui) = &builtins.ui {
        fonts.set_role(FontRole::Ui, ui.clone());
      }
      if let Some(mono) = &builtins.mono {
        fonts.set_role(FontRole::Mono, mono.clone());
      }
      if let Some(pixel) = &builtins.pixel {
        if let Some(face) = fonts.face_of(pixel) {
          fonts.set_render(face, FontRender::pixel(16.0));
        }
        fonts.set_role(FontRole::Pixel, pixel.clone());
      }
      builtins
    }
    #[cfg(not(feature = "default-fonts"))]
    {
      fonts.follow_fonts(store);
      BuiltinFonts::default()
    }
  }
}

/// The built-in fonts in stores of their own, with a registry following
/// them, for tests without a world
#[cfg(test)]
pub(crate) struct TestFonts {
  pub(crate) registry: Fonts,
  pub(crate) store: Assets<Font>,
  pub(crate) bitmaps: Assets<BitmapFont>,
  pub(crate) builtins: BuiltinFonts,
}

#[cfg(test)]
impl TestFonts {
  /// The built-in fonts (feature `default-fonts`)
  pub(crate) fn new() -> Self {
    let mut registry = Fonts::new();
    let mut store = Assets::new();
    let builtins = builtins::add(&mut registry, &mut store);
    let mut fonts = Self {
      registry,
      store,
      bitmaps: Assets::new(),
      builtins,
    };
    fonts.follow();
    fonts
  }

  /// No fonts
  pub(crate) fn empty() -> Self {
    let mut fonts = Self {
      registry: Fonts::new(),
      store: Assets::new(),
      bitmaps: Assets::new(),
      builtins: BuiltinFonts::default(),
    };
    fonts.follow();
    fonts
  }

  /// Adds the font in `bytes` and registers it
  pub(crate) fn add(&mut self, bytes: &[u8]) -> (Handle<Font>, Option<FontId>) {
    let font = match Font::from_bytes(bytes.to_vec()) {
      Ok(font) => font,
      Err(error) => panic!("not a font: {error}"),
    };
    let handle = self.store.add(font);
    self.follow();
    let face = self.registry.face_of(&handle);
    (handle, face)
  }

  /// Adds `font` and registers it
  pub(crate) fn add_bitmap(&mut self, font: BitmapFont) -> (Handle<BitmapFont>, Option<FontId>) {
    let handle = self.bitmaps.add(font);
    self.follow();
    let face = self.registry.bitmap_face_of(&handle);
    (handle, face)
  }

  /// Frees the fonts no handle holds, as the pump does, and follows
  pub(crate) fn free_unheld(&mut self) {
    self.store.free_unheld(&mut Vec::new());
    self.bitmaps.free_unheld(&mut Vec::new());
    self.follow();
  }

  /// Brings the registry in line with the stores
  pub(crate) fn follow(&mut self) {
    let changes = StoreChanges {
      fonts: Changed::take(
        &mut self.store,
        self.registry.follower,
        self.registry.font_store,
      ),
      bitmaps: Changed::take(
        &mut self.bitmaps,
        self.registry.follower,
        self.registry.bitmap_store,
      ),
    };
    self.registry.apply(changes);
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  const INTER: &[u8] = include_bytes!("../../assets/fonts/Inter-Regular.ttf");
  const MONO: &[u8] = include_bytes!("../../assets/fonts/JetBrainsMono-Regular.ttf");
  const PRESS_START: &[u8] =
    include_bytes!("../../../examples/games/breakout/assets/fonts/PressStart2P-Regular.ttf");

  fn font(bytes: &[u8]) -> Font {
    match Font::from_bytes(bytes.to_vec()) {
      Ok(font) => font,
      Err(error) => panic!("not a font: {error}"),
    }
  }

  #[cfg(feature = "default-fonts")]
  #[test]
  fn the_builtin_fonts_fill_every_role() {
    let fonts = Fonts::with_builtins();
    for role in [FontRole::Ui, FontRole::Mono, FontRole::Pixel] {
      assert!(fonts.role(role).is_some(), "{role:?} has a font");
      assert!(fonts.role_font(role).is_some(), "{role:?} holds its font");
    }
    let ui = fonts.role(FontRole::Ui).unwrap();
    assert_eq!(fonts.family(ui), Some("Inter"));
    let mono = fonts.role(FontRole::Mono).unwrap();
    assert_eq!(fonts.family(mono), Some("JetBrains Mono"));
    let pixel = fonts.role(FontRole::Pixel).unwrap();
    assert!(matches!(fonts.render(pixel), FontRender::Pixel { .. }));
  }

  #[cfg(feature = "default-fonts")]
  #[test]
  fn the_builtin_fonts_keep_their_order() {
    // Family lookup and fallback follow registration order: the built-ins
    // are the first four faces, as they always were
    let fonts = Fonts::with_builtins();
    let faces: Vec<(FontId, &str, f32)> = fonts
      .faces()
      .map(|(id, face)| (id, face.family.as_str(), face.weight))
      .collect();
    assert_eq!(
      faces,
      vec![
        (FontId(0), "Inter", 400.0),
        (FontId(1), "Inter", 700.0),
        (FontId(2), "JetBrains Mono", 400.0),
        (FontId(3), "m5x7", 500.0),
      ]
    );
  }

  #[cfg(feature = "default-fonts")]
  #[test]
  fn inter_bold_is_a_heavier_face_of_the_same_family() {
    let fonts = Fonts::with_builtins();
    let mut weights: Vec<f32> = fonts
      .faces()
      .filter(|(_, face)| face.family == "Inter")
      .map(|(_, face)| face.weight)
      .collect();
    weights.sort_by(f32::total_cmp);
    assert_eq!(weights, vec![400.0, 700.0]);
  }

  #[cfg(feature = "default-fonts")]
  #[test]
  fn find_prefers_the_regular_face() {
    let fonts = Fonts::with_builtins();
    let id = fonts.find("Inter").unwrap();
    assert_eq!(fonts.face(id).unwrap().weight, 400.0);
    assert!(fonts.find("No Such Family").is_none());
  }

  #[cfg(feature = "default-fonts")]
  #[test]
  fn find_face_picks_the_nearest_weight_and_slant() {
    let fonts = Fonts::with_builtins();
    let bold = fonts.find_face("Inter", 700.0, false).unwrap();
    assert_eq!(fonts.face(bold).unwrap().weight, 700.0);
    assert_eq!(fonts.find_face("Inter", 600.0, false), Some(bold));
    assert_eq!(
      fonts.find_face("Inter", 400.0, true),
      fonts.find("Inter"),
      "no italic face: the upright one of that weight"
    );
    assert_eq!(
      fonts.find_face("Inter", 550.0, false),
      fonts.find("Inter"),
      "150 from both faces: the one added first (Regular) wins"
    );
    assert!(fonts.find_face("No Such Family", 400.0, false).is_none());
  }

  #[test]
  fn bytes_that_are_not_a_font_are_refused() {
    assert!(matches!(
      Font::from_bytes(vec![1u8, 2, 3]),
      Err(FontError::Invalid)
    ));
    assert!(matches!(
      Font::from_bytes_face(INTER.to_vec(), 1),
      Err(FontError::NoFace(1))
    ));
    assert_eq!(font(PRESS_START).family(), "Press Start 2P");
  }

  #[cfg(feature = "default-fonts")]
  #[test]
  fn every_change_bumps_the_generation() {
    let mut fonts = Fonts::with_builtins();
    let before = fonts.generation();
    let ui = fonts.role(FontRole::Ui).unwrap();
    fonts.set_render(ui, FontRender::msdf());
    assert!(fonts.generation() > before);
    assert_eq!(fonts.render(ui), FontRender::msdf());
  }

  #[cfg(feature = "default-fonts")]
  #[test]
  fn a_change_to_what_a_font_already_is_keeps_the_generation() {
    let mut fonts = Fonts::with_builtins();
    let ui = fonts.role(FontRole::Ui).unwrap();
    let pixel = fonts.role(FontRole::Pixel).unwrap();
    let ui_font = fonts.role_font(FontRole::Ui).cloned().unwrap();
    let before = fonts.generation();
    fonts.set_role(FontRole::Ui, ui_font.clone());
    fonts.set_render(ui, FontRender::Raster);
    fonts.set_render(pixel, fonts.render(pixel));
    assert_eq!(fonts.generation(), before, "nothing changed");
    fonts.set_role(FontRole::Mono, ui_font);
    assert!(fonts.generation() > before, "a real change still counts");
    assert_eq!(fonts.role(FontRole::Mono), Some(ui));
  }

  #[cfg(feature = "default-fonts")]
  #[test]
  fn debug_output_names_the_faces_without_their_bytes() {
    let fonts = Fonts::with_builtins();
    let printed = format!("{fonts:?}");
    assert!(printed.contains("Inter") && printed.contains("JetBrains Mono"));
    assert!(printed.len() < 1000, "{} bytes printed", printed.len());
    let face = fonts.face(fonts.role(FontRole::Ui).unwrap()).unwrap();
    assert!(
      format!("{face:?}").len() < 1000,
      "a face prints no bytes either"
    );
    let printed = format!("{:?}", font(INTER));
    assert!(
      printed.contains("Inter") && printed.len() < 200,
      "{printed}"
    );
  }

  #[cfg(feature = "default-fonts")]
  #[test]
  fn the_same_font_bytes_twice_are_one_face() {
    let mut fonts = TestFonts::new();
    let builtin = fonts.registry.role(FontRole::Ui);
    let before = fonts.registry.generation();
    let (first, first_face) = fonts.add(INTER);
    let (second, second_face) = fonts.add(INTER);
    assert_ne!(first, second, "two assets");
    assert_eq!(
      (first_face, second_face),
      (builtin, builtin),
      "the built-in face"
    );
    assert_eq!(fonts.registry.generation(), before, "nothing was added");
    // The face stays while any of its fonts is held
    drop(first);
    fonts.free_unheld();
    assert_eq!(fonts.registry.face_of(&second), builtin);
    assert!(builtin.is_some_and(|face| fonts.registry.face(face).is_some()));
  }

  #[test]
  fn fonts_register_in_the_order_they_are_added() {
    let mut fonts = TestFonts::empty();
    let (_press, press) = fonts.add(PRESS_START);
    let (_mono, mono) = fonts.add(MONO);
    let (_inter, inter) = fonts.add(INTER);
    assert!(press < mono && mono < inter, "{press:?} {mono:?} {inter:?}");
    assert_eq!(fonts.registry.find("Press Start 2P"), press);
  }

  #[test]
  fn a_freed_font_leaves_the_registry_and_its_id_is_not_reused() {
    let mut fonts = TestFonts::empty();
    let (press, face) = fonts.add(PRESS_START);
    let face = face.unwrap();
    let id = press.id();
    let generation = fonts.registry.generation();
    drop(press);
    fonts.free_unheld();
    assert_eq!(fonts.registry.face_of(id), None);
    assert_eq!(fonts.registry.family(face), None, "the face is gone");
    assert_eq!(fonts.registry.face_serial(face), None);
    assert!(fonts.registry.find("Press Start 2P").is_none());
    assert!(fonts.registry.generation() > generation);
    // The store reuses the slot; the registry never reuses the face's id
    let (mono, mono_face) = fonts.add(MONO);
    assert_eq!(mono.id().index(), id.index(), "the slot is reused");
    assert!(mono_face.is_some_and(|mono_face| mono_face > face));
  }

  #[test]
  fn a_changed_font_replaces_its_face_in_place() {
    let mut fonts = TestFonts::empty();
    let (press, face) = fonts.add(PRESS_START);
    let face = face.unwrap();
    fonts.registry.set_render(face, FontRender::pixel(8.0));
    let (serial, generation) = (
      fonts.registry.face_serial(face),
      fonts.registry.generation(),
    );
    if let Some(changed) = fonts.store.get_mut(&press) {
      *changed = font(MONO);
    }
    fonts.follow();
    assert_eq!(fonts.registry.face_of(&press), Some(face), "the same face");
    assert_eq!(fonts.registry.family(face), Some("JetBrains Mono"));
    assert!(fonts.registry.face_serial(face).is_some());
    assert_ne!(fonts.registry.face_serial(face), serial, "its data is new");
    assert!(
      fonts.registry.generation() > generation,
      "text lays out again"
    );
    assert_eq!(
      fonts.registry.render(face),
      FontRender::pixel(8.0),
      "it draws as it did"
    );
  }

  #[test]
  fn a_changed_font_that_shares_its_face_gets_one_of_its_own() {
    let mut fonts = TestFonts::empty();
    let (first, face) = fonts.add(PRESS_START);
    let (second, shared) = fonts.add(PRESS_START);
    assert_eq!(shared, face, "shared");
    let face = face.unwrap();
    let serial = fonts.registry.face_serial(face);
    if let Some(changed) = fonts.store.get_mut(&second) {
      *changed = font(MONO);
    }
    fonts.follow();
    // The other keeps its face, serial and data
    assert_eq!(fonts.registry.face_of(&first), Some(face));
    assert_eq!(fonts.registry.face_serial(face), serial);
    assert_eq!(fonts.registry.family(face), Some("Press Start 2P"));
    // The changed one has a face of its own, of its new data
    let own = fonts.registry.face_of(&second);
    assert!(own.is_some() && own != Some(face), "{own:?}");
    assert_eq!(
      own.and_then(|own| fonts.registry.family(own)),
      Some("JetBrains Mono")
    );
  }

  #[test]
  fn a_changed_bitmap_font_that_shares_its_face_gets_one_of_its_own() {
    let mut fonts = TestFonts::empty();
    let sheet = Assets::new().add(Image::solid_rect(16, 16, 255, 255, 255, 255));
    let (first, face) = fonts.add_bitmap(BitmapFont::new("Grid", sheet.clone(), (8, 8), "AB"));
    let (second, shared) = fonts.add_bitmap(BitmapFont::new("Grid", sheet, (8, 8), "AB"));
    assert_eq!(shared, face, "shared");
    let face = face.unwrap();
    let serial = fonts.registry.face_serial(face);
    if let Some(changed) = fonts.bitmaps.get_mut(&second) {
      changed.cell = (8, 16);
    }
    fonts.follow();
    assert_eq!(fonts.registry.bitmap_face_of(&first), Some(face));
    assert_eq!(fonts.registry.face_serial(face), serial);
    assert_eq!(
      fonts.registry.render(face),
      FontRender::pixel(8.0),
      "as it was"
    );
    let own = fonts.registry.bitmap_face_of(&second);
    assert!(own.is_some() && own != Some(face), "{own:?}");
    assert_eq!(
      own.map(|own| fonts.registry.render(own)),
      Some(FontRender::pixel(16.0))
    );
  }

  #[test]
  fn a_change_that_leaves_a_font_as_it_was_changes_nothing() {
    let mut fonts = TestFonts::empty();
    let (press, face) = fonts.add(PRESS_START);
    let (serial, generation) = (
      face.and_then(|face| fonts.registry.face_serial(face)),
      fonts.registry.generation(),
    );
    let _ = fonts.store.get_mut(&press);
    fonts.follow();
    assert_eq!(fonts.registry.generation(), generation);
    assert_eq!(
      face.and_then(|face| fonts.registry.face_serial(face)),
      serial
    );
  }

  #[cfg(feature = "default-fonts")]
  #[test]
  fn a_role_holds_its_font() {
    let mut fonts = TestFonts::new();
    let (press, face) = fonts.add(PRESS_START);
    fonts.registry.set_role(FontRole::Ui, press.clone());
    assert_eq!(fonts.registry.role(FontRole::Ui), face);
    assert_eq!(fonts.registry.role_font(FontRole::Ui), Some(&press));
    let id = press.id();
    drop(press);
    fonts.free_unheld();
    assert_eq!(fonts.registry.face_of(id), face, "the role keeps it");
    // Pointing the role elsewhere lets it go
    let inter = fonts.builtins.ui.clone().unwrap();
    fonts.registry.set_role(FontRole::Ui, inter);
    fonts.free_unheld();
    assert_eq!(fonts.registry.face_of(id), None);
    assert_eq!(
      fonts.registry.role(FontRole::Ui),
      fonts.registry.find("Inter")
    );
  }

  #[cfg(feature = "default-fonts")]
  #[test]
  fn a_role_keeps_its_face_until_its_new_font_registers() {
    let mut fonts = TestFonts::new();
    let inter = fonts.registry.role(FontRole::Ui);
    let press = fonts.store.add(font(PRESS_START));
    fonts.registry.set_role(FontRole::Ui, press.clone());
    assert_eq!(
      fonts.registry.role(FontRole::Ui),
      inter,
      "not registered yet"
    );
    fonts.follow();
    assert_eq!(
      fonts.registry.role(FontRole::Ui),
      fonts.registry.face_of(&press),
      "registered: the role draws with it"
    );
    assert_ne!(fonts.registry.role(FontRole::Ui), inter);
  }

  #[test]
  fn a_role_switched_to_a_font_still_loading_keeps_its_old_font_until_then() {
    let mut fonts = TestFonts::empty();
    let (game, game_face) = fonts.add(PRESS_START);
    assert!(game_face.is_some());
    // The role is the game font's only holder
    fonts.registry.set_role(FontRole::Ui, game);
    // A font still loading: its slot is reserved, with nothing in it yet
    let next = fonts.store.ledger().lock().unwrap().reserve::<Font>();
    fonts.registry.set_role(FontRole::Ui, next.clone());
    fonts.free_unheld();
    assert_eq!(fonts.registry.role(FontRole::Ui), game_face, "the old face");
    assert!(
      game_face.is_some_and(|face| fonts.registry.face(face).is_some()),
      "the old font is held until then"
    );
    // It loads: the role takes it and lets the old one go
    assert!(fonts.store.insert(next.id(), font(MONO)));
    fonts.follow();
    let face = fonts.registry.face_of(&next);
    assert!(face.is_some());
    assert_eq!(fonts.registry.role(FontRole::Ui), face);
    fonts.free_unheld();
    assert!(game_face.is_some_and(|face| fonts.registry.face(face).is_none()));
  }

  #[test]
  fn the_same_bitmap_font_twice_is_one_face() {
    let mut fonts = TestFonts::empty();
    let sheet = Assets::new().add(Image::solid_rect(64, 16, 255, 255, 255, 255));
    let (first, face) = fonts.add_bitmap(BitmapFont::new("Grid", sheet.clone(), (8, 8), "AB"));
    let generation = fonts.registry.generation();
    let (_again, again) = fonts.add_bitmap(BitmapFont::new("Grid", sheet.clone(), (8, 8), "AB"));
    assert_eq!((again, fonts.registry.generation()), (face, generation));
    // Another cell size or character set is another face
    let (_other, other) = fonts.add_bitmap(BitmapFont::new("Grid", sheet, (8, 16), "AB"));
    assert!(other.is_some() && other != face);
    drop(first);
    fonts.free_unheld();
    assert!(
      face.is_some_and(|face| fonts.registry.face(face).is_some()),
      "still held"
    );
  }

  #[test]
  fn a_bitmap_font_needs_a_cell_and_characters() {
    let mut fonts = TestFonts::empty();
    let sheet = Assets::new().add(Image::solid_rect(64, 16, 255, 255, 255, 255));
    let (_no_cell, no_cell) =
      fonts.add_bitmap(BitmapFont::new("Grid", sheet.clone(), (0, 8), "AB"));
    let (_no_chars, no_chars) =
      fonts.add_bitmap(BitmapFont::new("Grid", sheet.clone(), (8, 8), ""));
    assert_eq!((no_cell, no_chars), (None, None), "not registered");
    let (_grid, id) = fonts.add_bitmap(BitmapFont::new("Grid", sheet.clone(), (8, 8), "ABCDEFGH"));
    let id = id.unwrap();
    assert!(
      matches!(fonts.registry.render(id), FontRender::Pixel { design_size } if design_size == 8.0)
    );
    let Some(Face {
      source: FaceSource::Bitmap(grid),
      ..
    }) = fonts.registry.face(id)
    else {
      panic!("a bitmap face");
    };
    assert_eq!(grid.sheet, sheet.id());
  }

  #[test]
  fn a_freed_bitmap_font_leaves_the_registry() {
    let mut fonts = TestFonts::empty();
    let sheet = Assets::new().add(Image::solid_rect(16, 8, 255, 255, 255, 255));
    let (grid, face) = fonts.add_bitmap(BitmapFont::new("Grid", sheet, (8, 8), "AB"));
    assert_eq!(fonts.registry.find("Grid"), face);
    drop(grid);
    fonts.free_unheld();
    assert_eq!(fonts.registry.find("Grid"), None);
    assert_eq!(face.and_then(|face| fonts.registry.family(face)), None);
  }

  #[test]
  fn a_bitmap_font_told_its_sheet_reloaded_keeps_its_face() {
    // A sheet's reload touches the fonts that hold it; the stamp redraws them
    let mut fonts = TestFonts::empty();
    let sheet = Assets::new().add(Image::solid_rect(16, 8, 255, 255, 255, 255));
    let (grid, face) = fonts.add_bitmap(BitmapFont::new("Grid", sheet, (8, 8), "AB"));
    let generation = fonts.registry.generation();
    let _ = fonts.bitmaps.get_mut(&grid);
    fonts.follow();
    assert_eq!(fonts.registry.bitmap_face_of(&grid), face);
    assert_eq!(
      fonts.registry.generation(),
      generation,
      "nothing to lay out again"
    );
  }

  #[test]
  fn a_changed_bitmap_font_takes_its_new_cells_in_place() {
    let mut fonts = TestFonts::empty();
    let sheet = Assets::new().add(Image::solid_rect(16, 16, 255, 255, 255, 255));
    let (grid, face) = fonts.add_bitmap(BitmapFont::new("Grid", sheet, (8, 8), "AB"));
    let generation = fonts.registry.generation();
    if let Some(font) = fonts.bitmaps.get_mut(&grid) {
      font.cell = (8, 16);
    }
    fonts.follow();
    assert_eq!(fonts.registry.bitmap_face_of(&grid), face, "the same face");
    let face = face.unwrap();
    assert_eq!(fonts.registry.render(face), FontRender::pixel(16.0));
    assert!(fonts.registry.generation() > generation);
  }

  #[test]
  fn the_bitmap_stamp_moves_with_its_sheet() {
    let mut fonts = TestFonts::empty();
    let mut images = Assets::new();
    let sheet = images.add(Image::solid_rect(16, 8, 255, 255, 255, 255));
    assert_eq!(
      fonts.registry.bitmap_stamp(|sheet| images.version(sheet)),
      0,
      "no bitmap faces"
    );
    let (_grid, _) = fonts.add_bitmap(BitmapFont::new("Grid", sheet.clone(), (8, 8), "AB"));
    let stamp = fonts.registry.bitmap_stamp(|sheet| images.version(sheet));
    assert_ne!(stamp, 0);
    let _ = images.get_mut(&sheet);
    assert_ne!(
      fonts.registry.bitmap_stamp(|sheet| images.version(sheet)),
      stamp,
      "the sheet changed"
    );
  }

  #[cfg(feature = "default-fonts")]
  #[test]
  fn prewarm_requests_are_queued_for_the_engine() {
    let mut fonts = Fonts::with_builtins();
    let ui = fonts.role(FontRole::Ui).unwrap();
    fonts.prewarm(ui, 32.0, "ABC");
    let queued = fonts.take_prewarm();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].chars, "ABC");
    assert!(fonts.take_prewarm().is_empty());
  }
}
