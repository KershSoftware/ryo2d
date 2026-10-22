//! The frame's UI commands as UI instances, in painter's order. What would
//! draw no pixel is culled before it is built, texts go through the text
//! cache (a reused text is one copy of its instances), and a line becomes
//! its builder's triangles. A draw in a scroll slot is built relative to
//! the slot, which the shader moves and clips. A fragment's run is one copy
//! of its kept instances while it is the same (`UiRunCache`); in a slot it
//! is built unculled, so it stays the same while the slot scrolls.

use std::borrow::Cow;

use glam::Vec2;

use super::gpu::{SLOTS, SlotUniform};
use super::instance::{ClipRect, UiInstance, UiKind};
use super::runs::TexNeed;
use crate::core::graphics::{
  CameraView, GlyphPage, Presentation, SpriteGpuRenderer, SpriteInstance, TextureId, place_in_slot,
};
use crate::core::render::batch::{Filter, PageKind, PolyPoint, Sampling, Triangles, push_polyline};
use crate::core::render::text::{TextPlacement, TextSources, TextTargets};
use crate::core::render::ui::{
  line_of, rect_instance, scissor_of, sprite_instance, text_of, ui_view, ui_z,
};
use crate::core::render::ui_retain::{
  KeptRun, TextSink, UiRenderEnv, UiRunCache, UiTextCache, stamp_of,
};
use crate::core::render::{DrawGlyph, DrawList, DrawPayload, PixelRect, UiSpace, reach};
use crate::core::text::GlyphFormat;
use crate::core::ui::immediate::{FragmentRun, ScrollSlot, UiCommand, UiCommands, UiDraw};
use crate::core::ui::{Color, Rect};

/// A sprite instance whose uv and kind are fixed once its texture is
/// resident: its uv rect is placed in the texture's slot, its sampler and
/// kind follow the page it lands on
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SpriteFix {
  /// Its place in [`UiBuild::instances`]
  pub(crate) index: u32,
  pub(crate) texture: TextureId,
  pub(crate) filter: Filter,
}

/// The frame's UI instances in painter's order, what each samples, the
/// sprites among them and the sprite textures to make resident; reused from
/// frame to frame
#[derive(Default)]
pub(crate) struct UiBuild {
  pub(crate) instances: Vec<UiInstance>,
  /// What each instance samples, one per instance
  pub(crate) needs: Vec<TexNeed>,
  pub(crate) sprites: Vec<SpriteFix>,
  /// The frame's scroll slots as the shader reads them (`slot_uniforms`)
  pub(crate) slots: Vec<SlotUniform>,
  /// Every sprite texture the commands drew or would have drawn but for
  /// culling, with the place of its command in the frame's: all are made
  /// resident, in the order recorded, so where the atlas places a texture
  /// does not depend on what is culled
  resident: Vec<(u32, TextureId)>,
  /// `resident`'s textures, in the order recorded
  textures: Vec<TextureId>,
  /// The commands' painter's order, when they arrive out of it
  order: Vec<u32>,
  /// A line's points and triangles
  points: Vec<PolyPoint>,
  triangles: Triangles,
}

impl UiBuild {
  /// Forget the frame's instances, keeping the allocations
  pub(crate) fn clear(&mut self) {
    self.instances.clear();
    self.needs.clear();
    self.sprites.clear();
    self.slots.clear();
    self.resident.clear();
    self.textures.clear();
  }

  /// The sprite textures to make resident before the sprites are fixed:
  /// every one the commands asked for, culled or not, in the order recorded
  /// (see `resident`)
  pub(crate) fn sprite_textures(&self) -> &[TextureId] {
    &self.textures
  }

  fn push(&mut self, instance: UiInstance, need: TexNeed) {
    self.instances.push(instance);
    self.needs.push(need);
  }

  /// Fix the sprites' instances, their textures now resident: each one's uv
  /// rect placed in its texture's slot and its uv bounds the slot's part
  /// (`place_in_slot`, as the batch places sprites), its body its page's (a
  /// render target's is linear), its sampler its filter's under
  /// `presentation`. A sprite whose texture has no slot is dropped, as the
  /// batch skips it.
  pub(crate) fn fix_sprites(&mut self, textures: &SpriteGpuRenderer, presentation: Presentation) {
    let mut gone = Vec::new();
    for fix in self.sprites.drain(..) {
      let Some(instance) = self.instances.get_mut(fix.index as usize) else {
        continue;
      };
      let Some((slot, page_size)) = textures.slot(fix.texture) else {
        gone.push(fix.index);
        continue;
      };
      let sprite: SpriteInstance = bytemuck::cast(instance.data);
      let placed = place_in_slot(&sprite, &slot, page_size, false);
      let kind = match PageKind::of(Some(slot.page)) {
        PageKind::Linear => UiKind::SPRITE_LINEAR,
        PageKind::Srgb8 => UiKind::SPRITE,
      };
      let nearest = fix.filter.resolve(presentation) == Sampling::Nearest;
      let mark = SlotMark::of(instance);
      *instance = mark.on(UiInstance::quad(kind, &placed, instance.clip(), nearest, 0));
    }
    if !gone.is_empty() {
      drop_at(&mut self.instances, &gone);
      drop_at(&mut self.needs, &gone);
    }
  }

  /// A sprite quad, its uv and kind fixed once its texture is resident
  fn push_sprite(
    &mut self,
    instance: &SpriteInstance,
    texture: TextureId,
    filter: Filter,
    clip: ClipRect,
    mark: SlotMark,
  ) {
    self.sprites.push(SpriteFix {
      index: self.instances.len() as u32,
      texture,
      filter,
    });
    let instance = UiInstance::quad(UiKind::SPRITE, instance, clip, false, 0);
    self.push(mark.on(instance), TexNeed::Sprite(texture));
  }
}

/// What the UI image shows: the UI camera's view, as the batch culls by it,
/// and the camera's map from UI units to image pixels, to test clips
struct Seen {
  view: CameraView,
  /// Image pixels per UI unit, then the image pixel of UI point (0, 0)
  scale: Vec2,
  offset: Vec2,
}

impl Seen {
  fn of(env: &UiRenderEnv) -> Seen {
    let view = ui_view(&env.space);
    let image = Vec2::new(env.space.image.0 as f32, env.space.image.1 as f32);
    let scale = image / view.rect_size;
    Seen {
      view,
      scale,
      offset: (view.rect_size / 2.0 - view.center) * scale,
    }
  }

  /// Whether an item inside the box `min..max` (UI units) can draw a pixel
  /// in `clip`: the box meets the UI camera's view (exactly the batch's
  /// culling) and the clip
  fn sees(&self, min: Vec2, max: Vec2, clip: ClipRect) -> bool {
    self.view.is_visible(min, max) && self.meets(min, max, clip)
  }

  /// Whether the box `min..max` (UI units) comes within a pixel of `clip`:
  /// one wholly further out covers no pixel centre in it, even after a
  /// pixel-art snap moves its edges (by up to a pixel)
  fn meets(&self, min: Vec2, max: Vec2, clip: ClipRect) -> bool {
    let low = min * self.scale + self.offset - 1.0;
    let high = max * self.scale + self.offset + 1.0;
    high.x > f32::from(clip.x0)
      && low.x < f32::from(clip.x1)
      && high.y > f32::from(clip.y0)
      && low.y < f32::from(clip.y1)
  }
}

/// What every command of the frame is built with
struct Frame<'f> {
  env: &'f UiRenderEnv,
  /// The frame's slot table: a draw in one of the shader's slots is built
  /// relative to it, one in a slot past them moved by its offset and
  /// clipped by its clip first (`Frame::place`)
  slots: &'f [ScrollSlot],
  seen: Seen,
  sources: Option<&'f TextSources<'f>>,
  real_time: f32,
}

/// An instance's scroll slot in the shader, and whether its clip is its own
/// (relative to the slot)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SlotMark {
  slot: u8,
  own: bool,
}

impl SlotMark {
  /// Slot 0: positions and clip absolute
  const NONE: SlotMark = SlotMark {
    slot: 0,
    own: false,
  };

  fn of(instance: &UiInstance) -> SlotMark {
    SlotMark {
      slot: instance.slot(),
      own: instance.own_clip(),
    }
  }

  /// `instance` marked with it
  fn on(self, instance: UiInstance) -> UiInstance {
    let instance = instance.with_slot(self.slot);
    if self.own {
      instance.with_own_clip()
    } else {
      instance
    }
  }
}

/// Where a draw is built: its command as it is built (relative to its slot
/// in one of the shader's, else absolute), the clip its instances carry and
/// the clip it is culled by where it is drawn
struct Placed<'d> {
  command: Cow<'d, UiCommand>,
  /// The clip its instances carry: the scissor of its clip when built
  /// absolute; in a slot its own relative to the slot, else the whole
  /// image (the shader takes the slot's clip from the slot)
  clip: ClipRect,
  /// `clip` as a text is placed with it (and kept by it in the text cache)
  scissor: Option<PixelRect>,
  mark: SlotMark,
  /// Where its slot draws it, in UI units (zero when built absolute)
  by: Vec2,
  /// Its clip where it is drawn: what its boxes, moved by `by`, are culled
  /// against; `None` when nothing of it is culled (a fragment run's draw in
  /// a slot, kept to be copied wherever the slot is drawn)
  cull: Option<ClipRect>,
}

impl Placed<'_> {
  /// Whether an item inside the box `min..max` (as built) is built: it can
  /// draw a pixel where its slot draws it, or nothing of it is culled
  fn sees(&self, seen: &Seen, min: Vec2, max: Vec2) -> bool {
    self
      .cull
      .is_none_or(|cull| seen.sees(min + self.by, max + self.by, cull))
  }
}

impl Frame<'_> {
  /// Slot `slot` and its index when the shader moves the draws in it: one
  /// of its slots, in the table, off the pixel-art target (see `place`)
  fn shader_slot(&self, slot: u16) -> Option<(u8, &ScrollSlot)> {
    if slot == 0 || usize::from(slot) >= SLOTS || self.env.on_target {
      return None;
    }
    Some((u8::try_from(slot).ok()?, self.slots.get(usize::from(slot))?))
  }

  /// Where `draw` is built (see [`Placed`]), or `None` when it draws
  /// nothing: its slot is not in the table, or its clip is empty.
  ///
  /// In slot 0 it is built as it is, under its clip. In slot `k` below
  /// [`SLOTS`] it is built relative to the slot, which the shader moves by
  /// its offset: under the slot's clip (which the shader reads from the
  /// slot), or, with a clip of its own (a text field's view in a scrolled
  /// list), under that clip relative to the slot, which the shader moves
  /// and meets with the slot's; with `cull` off nothing of it is culled
  /// (the shader collapses what lies outside its clip). Past those slots,
  /// or when its own clip does not fit a [`ClipRect`] relative to the slot,
  /// it is built absolute, moved and clipped on the CPU (`UiDraw::placed`),
  /// and culled.
  ///
  /// On the pixel-art target every draw in a slot is built absolute: there
  /// a quad's corners are rounded to whole pixels, and a corner on a half
  /// pixel rounds by the last bit of its position, which moving it on the
  /// CPU and moving it in the shader compute differently.
  fn place<'d>(&self, draw: &'d UiDraw, cull: bool) -> Option<Placed<'d>> {
    let space = &self.env.space;
    if let Some((k, slot)) = self.shader_slot(draw.slot) {
      match in_slot(draw, slot, k, space, cull) {
        InSlot::Built(placed) => return Some(placed),
        InSlot::Nothing => return None,
        InSlot::Absolute => {}
      }
    }
    let command = draw.placed(self.slots, space.px_scale)?;
    let scissor = scissor_of(command.clip_rect(), space)?;
    let clip = ClipRect::of(scissor, space.image);
    Some(Placed {
      command,
      clip,
      scissor,
      mark: SlotMark::NONE,
      by: Vec2::ZERO,
      cull: Some(clip),
    })
  }

  /// Where a fragment run in slot `slot` is built
  fn run_at(&self, slot: u16) -> RunAt {
    if slot == 0 {
      RunAt::Image
    } else if let Some((k, _)) = self.shader_slot(slot) {
      RunAt::Slot(k)
    } else {
      RunAt::Elsewhere
    }
  }

  /// A rect command's instance where `placed` puts it, or `None` when it
  /// draws nothing there
  fn rect(
    &self,
    placed: &Placed<'_>,
    rect: Rect,
    fill: Color,
    radius: f32,
    border: Option<(f32, Color)>,
  ) -> Option<UiInstance> {
    let pixel_art = self.env.on_target;
    let (instance, min, max) =
      rect_instance(rect, fill, radius, border, &self.env.space, pixel_art)?;
    if !placed.sees(&self.seen, min, max) {
      return None;
    }
    let instance = UiInstance::quad(UiKind::RECT, &instance, placed.clip, false, 0);
    Some(placed.mark.on(instance))
  }
}

/// What becomes of a draw in one of the shader's slots
enum InSlot<'d> {
  /// Built relative to the slot
  Built(Placed<'d>),
  /// It draws nothing: the slot's clip is empty, or its own clip does not
  /// meet the slot's where the slot draws it (when culled)
  Nothing,
  /// Its own clip does not fit a [`ClipRect`] relative to the slot: it is
  /// built absolute
  Absolute,
}

/// `draw` built relative to `slot`, the shader's slot `k`, culled where the
/// slot draws it with `cull` on (see [`Frame::place`])
fn in_slot<'d>(
  draw: &'d UiDraw,
  slot: &ScrollSlot,
  k: u8,
  space: &UiSpace,
  cull: bool,
) -> InSlot<'d> {
  // Where the slot draws, to cull by: nothing under an empty clip
  let slot_clip = if cull {
    let Some(scissor) = scissor_of(slot.clip, space) else {
      return InSlot::Nothing;
    };
    Some(ClipRect::of(scissor, space.image))
  } else {
    None
  };
  let by = slot.offset_units(space.px_scale);
  let command = Cow::Borrowed(&draw.command);
  let Some(own) = draw.command.clip_rect() else {
    // The shader draws it under the slot's clip, read from the slot: it
    // carries none (the whole image), so neither it nor its text's key
    // changes as the slot's clip moves (a nested list's, as an outer list
    // scrolls it)
    return InSlot::Built(Placed {
      command,
      clip: ClipRect::of(None, space.image),
      scissor: None,
      mark: SlotMark {
        slot: k,
        own: false,
      },
      by,
      cull: slot_clip,
    });
  };
  let Some(own) = relative_scissor(own, space.px_scale) else {
    return InSlot::Absolute;
  };
  let clip = ClipRect::of(Some(own), space.image);
  let cull = match slot_clip {
    Some(slot_clip) => {
      let cull = clip.in_slot(slot.offset, slot_clip);
      if cull.is_empty() {
        return InSlot::Nothing;
      }
      Some(cull)
    }
    None => None,
  };
  InSlot::Built(Placed {
    command,
    clip,
    scissor: Some(own),
    mark: SlotMark { slot: k, own: true },
    by,
    cull,
  })
}

/// A draw's own clip in a slot (UI units, relative to the slot) out to whole
/// image pixels as `scissor_of` takes a clip out, but not clamped to the
/// image (the slot moves it): empty when the clip is, `None` when it does
/// not fit a [`ClipRect`]
fn relative_scissor(clip: Rect, px_scale: f32) -> Option<PixelRect> {
  let s = px_scale;
  let (x0, y0) = ((clip.x * s).floor(), (clip.y * s).floor());
  let (x1, y1) = ((clip.right() * s).ceil(), (clip.bottom() * s).ceil());
  // NaN fails this too
  let fits = |v: f32| (0.0..=f32::from(u16::MAX)).contains(&v);
  if !(fits(x0) && fits(y0) && fits(x1) && fits(y1)) {
    return None;
  }
  Some(PixelRect {
    x: x0 as u32,
    y: y0 as u32,
    width: (x1 - x0).max(0.0) as u32,
    height: (y1 - y0).max(0.0) as u32,
  })
}

/// The frame's scroll slots as the shader reads them (into `out`, cleared
/// first): the first [`SLOTS`] of the table, each with its offset in UI
/// units (exactly what the CPU moves a draw in it by, `UiDraw::placed`) and
/// in whole pixels, and its clip in image pixels as the scissor would be
/// (empty when it is). Slot 0 holds no offset and the whole image.
pub(crate) fn slot_uniforms(slots: &[ScrollSlot], space: &UiSpace, out: &mut Vec<SlotUniform>) {
  out.clear();
  out.extend(slots.iter().take(SLOTS).map(|slot| {
    let by = slot.offset_units(space.px_scale);
    let clip = match scissor_of(slot.clip, space) {
      Some(scissor) => ClipRect::of(scissor, space.image),
      None => ClipRect {
        x0: 0,
        y0: 0,
        x1: 0,
        y1: 0,
      },
    };
    SlotUniform {
      offset: [by.x, by.y],
      pixels: slot.offset,
      clip: [clip.x0, clip.y0, clip.x1, clip.y1].map(u32::from),
    }
  }));
}

/// Where a command falls in the painter's order: by z (the layer, then the
/// recorded order), then with lines after the rest at one z (as the batch
/// draws a line after a sprite at equal z), then as recorded
fn paint_key(draw: &UiDraw) -> (i32, bool) {
  let line = matches!(draw.command, UiCommand::Line { .. });
  (ui_z(draw.command.draw_layer(), draw.order), line)
}

/// Build the frame's UI commands into `build` (cleared first): the UI's
/// instances in painter's order, what each samples, its sprites, whose uv
/// and kind are fixed once their textures are resident, and its scroll
/// slots as the shader reads them.
///
/// A draw in a slot is built relative to it, for the shader to move by its
/// offset and clip by its clip (past the shader's slots, moved and clipped
/// on the CPU; see `Frame::place`); one whose slot is not in the table
/// builds nothing. A command drawing no pixel (wholly outside its clip or
/// the UI camera's view where it is drawn, or with an empty clip) builds
/// nothing; in a slot a text is not culled glyph by glyph (see
/// `TextInstances`). Texts go through `texts`; without `sources` they are
/// skipped. `targets.list` is scratch, cleared for each text and line.
///
/// When the commands arrive in the painter's order, a fragment's run is
/// copied from `runs` while it is the same, or built whole and kept there
/// (`build_run`); out of order every command is built, and no run is kept.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_ui(
  build: &mut UiBuild,
  commands: &UiCommands,
  env: &UiRenderEnv,
  sources: Option<&TextSources<'_>>,
  targets: &mut TextTargets<'_>,
  real_time: f32,
  texts: &mut UiTextCache<UiInstance>,
  runs: &mut UiRunCache,
) {
  build.clear();
  slot_uniforms(commands.slots(), &env.space, &mut build.slots);
  let frame = Frame {
    env,
    slots: commands.slots(),
    seen: Seen::of(env),
    sources,
    real_time,
  };
  let draws = commands.as_slice();
  // Commands usually arrive in the painter's order: sort only when one is
  // out of it
  let mut last = None;
  let in_order = draws.iter().all(|draw| {
    let key = Some(paint_key(draw));
    let rising = key >= last;
    last = key;
    rising
  });
  if in_order {
    // Each run's draws are contiguous, in the order of the runs' ranges;
    // runs that do not hold their draws are not trusted
    let held = if commands.runs_hold() {
      commands.runs()
    } else {
      &[]
    };
    let mut next = held.iter().filter(|run| !run.draws.is_empty()).peekable();
    let mut index = 0;
    while let Some(draw) = draws.get(index as usize) {
      match next.next_if(|run| run.draws.start == index) {
        Some(run) => {
          build_run(build, &frame, targets, texts, runs, run, draws);
          index = run.draws.end;
        }
        None => {
          build_command(build, &frame, targets, texts, index, draw, true);
          index += 1;
        }
      }
    }
  } else {
    let mut painted = std::mem::take(&mut build.order);
    painted.clear();
    painted.extend(0..draws.len() as u32);
    // Stable: commands equal in the order keep the order recorded
    painted.sort_by_key(|&index| draws.get(index as usize).map(paint_key));
    for &index in &painted {
      if let Some(draw) = draws.get(index as usize) {
        build_command(build, &frame, targets, texts, index, draw, true);
      }
    }
    build.order = painted;
    // Stable: a command's textures keep the order they were asked for in
    build.resident.sort_by_key(|&(command, _)| command);
  }
  let UiBuild {
    resident, textures, ..
  } = build;
  textures.extend(resident.iter().map(|&(_, texture)| texture));
}

/// Where a fragment run is built
#[derive(Debug, Clone, Copy)]
enum RunAt {
  /// Slot 0: as its draws are, culled as any draw is
  Image,
  /// One of the shader's slots (its index): relative to the slot and
  /// unculled, so its instances hold wherever the slot is drawn
  Slot(u8),
  /// Past the shader's slots or on the pixel-art target (moved and clipped
  /// on the CPU), or in no slot of the table: built every frame
  Elsewhere,
}

/// Build `run` (its draws `draws`' at its range), or copy its kept
/// instances from `runs` when they are what building it gives (see
/// `UiRunCache::get`). A copy pins the glyph pages it samples for the
/// frame, and holds its texts in `texts` as drawing them would, so its next
/// version reuses them. In a slot nothing of it is culled. Built, it is
/// kept, unless it was built absolute (`RunAt::Elsewhere`, or a draw of it
/// took the absolute path) or a full atlas cut a text of it short.
fn build_run(
  build: &mut UiBuild,
  frame: &Frame<'_>,
  targets: &mut TextTargets<'_>,
  texts: &mut UiTextCache<UiInstance>,
  runs: &mut UiRunCache,
  run: &FragmentRun,
  draws: &[UiDraw],
) {
  let start = run.draws.start;
  let Some(own) = draws.get(start as usize..run.draws.end as usize) else {
    return;
  };
  let at = frame.run_at(run.slot);
  let slot = match at {
    RunAt::Slot(k) => k,
    RunAt::Image | RunAt::Elsewhere => 0,
  };
  let icons = frame.sources.map(|sources| sources.icons);
  if !matches!(at, RunAt::Elsewhere) {
    let sprites = &*targets.sprites;
    let kept = runs.get(
      run,
      slot,
      |page| sprites.glyph_page_generation(page),
      |texture| sprites.is_texture(texture),
      |k| {
        own
          .get(k as usize)
          .map_or(0, |draw| stamp_of(&draw.command, icons))
      },
    );
    if let Some(kept) = kept {
      copy_run(build, kept, start, slot);
      for &(page, _) in &kept.pages {
        targets.sprites.pin_glyph_page(page);
      }
      texts.hold(&kept.texts);
      return;
    }
  }
  let from = Marks::of(build);
  let drawn = texts.drawn_count();
  let refused = targets.sprites.glyphs_refused();
  let cull = !matches!(at, RunAt::Slot(_));
  let mut absolute = matches!(at, RunAt::Elsewhere);
  for (index, draw) in (start..).zip(own) {
    absolute |= build_command(build, frame, targets, texts, index, draw, cull);
  }
  runs.count_built();
  if absolute || targets.sprites.glyphs_refused() != refused {
    return;
  }
  let Some(mut kept) = runs.spare() else {
    return;
  };
  kept.version = run.version;
  kept.slot = slot;
  kept.note(own, icons);
  kept.texts.extend_from_slice(texts.drawn_from(drawn));
  if kept.animated || keep_built(&mut kept, build, from, start, targets.sprites) {
    runs.store(run.id, kept);
  } else {
    runs.recycle(kept);
  }
}

/// Where a run's build starts in a `UiBuild`
#[derive(Debug, Clone, Copy)]
struct Marks {
  instances: usize,
  sprites: usize,
  resident: usize,
}

impl Marks {
  fn of(build: &UiBuild) -> Marks {
    Marks {
      instances: build.instances.len(),
      sprites: build.sprites.len(),
      resident: build.resident.len(),
    }
  }
}

/// Keep in `kept` what a run whose first draw is the frame's `start`-th
/// built into `build` from `from`: its instances, what each samples, its
/// sprites and sprite textures (relative to its first instance and draw),
/// and the glyph pages it samples with their generations. `false` when a
/// page has none (it is not kept).
fn keep_built(
  kept: &mut KeptRun,
  build: &UiBuild,
  from: Marks,
  start: u32,
  sprites: &SpriteGpuRenderer,
) -> bool {
  let first = from.instances as u32;
  kept
    .instances
    .extend_from_slice(build.instances.get(from.instances..).unwrap_or_default());
  kept
    .needs
    .extend_from_slice(build.needs.get(from.instances..).unwrap_or_default());
  let fixes = build.sprites.get(from.sprites..).unwrap_or_default();
  kept.sprites.extend(fixes.iter().map(|fix| SpriteFix {
    index: fix.index.saturating_sub(first),
    ..*fix
  }));
  let resident = build.resident.get(from.resident..).unwrap_or_default();
  kept.resident.extend(
    resident
      .iter()
      .map(|&(command, texture)| (command.saturating_sub(start), texture)),
  );
  let KeptRun { needs, pages, .. } = kept;
  for need in needs.iter() {
    let TexNeed::Glyph(page) = *need else {
      continue;
    };
    if pages.iter().any(|&(kept, _)| kept == page) {
      continue;
    }
    match sprites.glyph_page_generation(page) {
      Some(generation) => pages.push((page, generation)),
      None => return false,
    }
  }
  true
}

/// Append `kept`'s instances to `build`, the run's first draw the frame's
/// `start`-th: its instances marked with `slot` (this frame's index of the
/// slot it is in), its sprites and sprite textures moved to where its
/// first instance and draw land
fn copy_run(build: &mut UiBuild, kept: &KeptRun, start: u32, slot: u8) {
  let first = build.instances.len();
  build.instances.extend_from_slice(&kept.instances);
  if kept.slot != slot
    && let Some(copied) = build.instances.get_mut(first..)
  {
    for instance in copied {
      *instance = instance.with_slot(slot);
    }
  }
  build.needs.extend_from_slice(&kept.needs);
  let first = first as u32;
  build
    .sprites
    .extend(kept.sprites.iter().map(|fix| SpriteFix {
      index: fix.index + first,
      ..*fix
    }));
  build.resident.extend(
    kept
      .resident
      .iter()
      .map(|&(command, texture)| (command + start, texture)),
  );
}

/// Build one draw (the `index`-th of the frame's), at its place in the
/// painter's order, where `Frame::place` puts it, culled with `cull` on
/// (see `Frame::place`). `true` when it was in one of the shader's slots
/// but built absolute (its own clip does not fit relative to the slot).
fn build_command(
  build: &mut UiBuild,
  frame: &Frame<'_>,
  targets: &mut TextTargets<'_>,
  texts: &mut UiTextCache<UiInstance>,
  index: u32,
  draw: &UiDraw,
  cull: bool,
) -> bool {
  let space = &frame.env.space;
  let Some(placed) = frame.place(draw, cull) else {
    return false;
  };
  let absolute = draw.slot != 0 && placed.mark.slot == 0;
  let command = &*placed.command;
  let z = ui_z(command.draw_layer(), draw.order);
  match command {
    UiCommand::Rect {
      rect,
      fill,
      radius,
      border,
      ..
    } => {
      if let Some(instance) = frame.rect(&placed, *rect, *fill, *radius, *border) {
        build.push(instance, TexNeed::None);
      }
    }
    UiCommand::Sprite {
      sprite,
      rect,
      uv,
      tint,
      filter,
      ..
    } => {
      let Some(instance) = sprite_instance(*rect, *uv, *tint) else {
        return absolute;
      };
      // Asked for and made resident culled or not (see `UiBuild::resident`)
      let texture = targets.sprites.texture_of(sprite);
      build.resident.push((index, texture));
      let (min, max) = reach(&instance);
      if placed.sees(&frame.seen, min, max) {
        build.push_sprite(&instance, texture, *filter, placed.clip, placed.mark);
      }
    }
    UiCommand::Line { .. } => build_line(build, &frame.seen, targets.list, z, &placed),
    UiCommand::Text { .. } => {
      let Some(sources) = frame.sources else {
        return absolute;
      };
      let Some((layout, place)) = text_of(command, space, z, placed.scissor, frame.real_time)
      else {
        return absolute;
      };
      targets.list.clear();
      let mut sink = TextInstances {
        build,
        seen: &frame.seen,
        clip: placed.clip,
        mark: placed.mark,
        command: index,
        taken: 0,
      };
      texts.emit_into(sources, targets, &mut sink, layout, &place);
    }
  }
  absolute
}

/// A rect draw's instance as `build_ui` builds it, or `None` when it draws
/// nothing (tests)
#[cfg(test)]
pub(crate) fn rect_built(
  draw: &UiDraw,
  slots: &[ScrollSlot],
  env: &UiRenderEnv,
) -> Option<UiInstance> {
  let frame = Frame {
    env,
    slots,
    seen: Seen::of(env),
    sources: None,
    real_time: 0.0,
  };
  let placed = frame.place(draw, true)?;
  let UiCommand::Rect {
    rect,
    fill,
    radius,
    border,
    ..
  } = *placed.command
  else {
    return None;
  };
  frame.rect(&placed, rect, fill, radius, border)
}

/// A line command as its builder's triangles, one instance each, in index
/// order. The line is culled by its box as the batch culls it; then a
/// triangle wholly outside the clip is left out (lines are never snapped).
/// Both are tested where `placed`'s slot draws them, unless nothing of it
/// is culled.
fn build_line(build: &mut UiBuild, seen: &Seen, list: &mut DrawList, z: i32, placed: &Placed<'_>) {
  let UiBuild {
    instances,
    needs,
    points,
    triangles,
    ..
  } = build;
  let Some((style, look)) = line_of(&placed.command, z, points) else {
    return;
  };
  list.clear();
  triangles.clear();
  push_polyline(list, triangles, points, &style, look);
  let Some(line) = list.triangles(0) else {
    return;
  };
  if placed.cull.is_some()
    && !seen
      .view
      .is_visible(line.min + placed.by, line.max + placed.by)
  {
    return;
  }
  let range = line.indices.start as usize..line.indices.end as usize;
  let indices = triangles.indices.get(range).unwrap_or(&[]);
  for triangle in indices.as_chunks::<3>().0 {
    let corner = |index: u32| {
      triangles.vertices.get(index as usize).map(|vertex| {
        let corner = (
          Vec2::from(vertex.position),
          Vec2::from(vertex.uv),
          vertex.color,
        );
        (corner, vertex.params)
      })
    };
    let (Some((a, params)), Some((b, _)), Some((c, _))) = (
      corner(triangle[0]),
      corner(triangle[1]),
      corner(triangle[2]),
    ) else {
      continue;
    };
    let (min, max) = (a.0.min(b.0).min(c.0), a.0.max(b.0).max(c.0));
    let shown = placed
      .cull
      .is_none_or(|cull| seen.meets(min + placed.by, max + placed.by, cull));
    if shown {
      let triangle = UiInstance::triangle([a, b, c], params, placed.clip);
      instances.push(placed.mark.on(triangle));
      needs.push(TexNeed::None);
    }
  }
}

/// A text's glyphs, bitmap cells and icons, taken from the scratch draw
/// list into the UI's instances, each culled by its box. A built text keeps
/// the instances of its glyphs that were not culled: what is culled follows
/// from the text cache's key (placement, clip and the sink's `variant`) and
/// environment (the UI space), so the same text is culled the same way when
/// it is reused.
///
/// In a scroll slot nothing of a text is culled: it is laid relative to the
/// slot, and its kept instances are copied while the slot scrolls, so they
/// must not depend on where the slot draws them (the shader collapses those
/// wholly outside the clip). A copy takes the slot the text is in now.
struct TextInstances<'b> {
  build: &'b mut UiBuild,
  seen: &'b Seen,
  clip: ClipRect,
  mark: SlotMark,
  /// The text command's place in the frame's
  command: u32,
  /// The scratch list's items already taken
  taken: usize,
}

impl TextInstances<'_> {
  /// Whether an item inside `min..max` is built: in a slot every one is
  fn keeps(&self, min: Vec2, max: Vec2) -> bool {
    self.mark.slot != 0 || self.seen.sees(min, max, self.clip)
  }
}

impl TextSink<UiInstance> for TextInstances<'_> {
  fn variant(&self) -> u32 {
    // Culled glyph by glyph (slot 0), kept whole under the slot's clip, or
    // kept whole under a clip of its own
    match self.mark {
      SlotMark { slot: 0, .. } => 0,
      SlotMark { own: false, .. } => 1,
      SlotMark { own: true, .. } => 2,
    }
  }

  fn mark(&self, _targets: &TextTargets<'_>) -> usize {
    self.build.instances.len()
  }

  fn take(&mut self, targets: &mut TextTargets<'_>) {
    let list = &*targets.list;
    // UI text is unlit: its items are the list's unlit ones, in the order
    // laid
    let items = list.unlit_items();
    for item in items.get(self.taken..).unwrap_or(&[]) {
      match item.payload {
        DrawPayload::Glyph(index) => {
          if let Some(glyph) = list.glyph(index) {
            self.glyph(glyph);
          }
        }
        DrawPayload::Sprite(index) => {
          let Some(sprite) = list.sprite(index) else {
            continue;
          };
          // Resident culled or not (see `UiBuild::resident`)
          let resident = (self.command, sprite.texture);
          self.build.resident.push(resident);
          if self.keeps(sprite.min, sprite.max) {
            let (clip, mark) = (self.clip, self.mark);
            self
              .build
              .push_sprite(&sprite.instance, sprite.texture, sprite.filter, clip, mark);
          }
        }
        // Text lays glyphs and sprites only
        DrawPayload::Triangles(_) => {}
      }
    }
    self.taken = items.len();
  }

  fn captured<'s>(&'s self, _targets: &'s TextTargets<'_>, from: usize) -> &'s [UiInstance] {
    self.build.instances.get(from..).unwrap_or(&[])
  }

  fn push_cached(
    &mut self,
    _targets: &mut TextTargets<'_>,
    glyphs: &[UiInstance],
    _place: &TextPlacement,
  ) {
    // Their clip is the text's, part of its key, as is whether it is its
    // own; the slot is the one the text is in now
    let from = self.build.instances.len();
    self.build.instances.extend_from_slice(glyphs);
    self.build.needs.extend(glyphs.iter().map(glyph_need));
    if self.mark.slot != 0
      && let Some(copied) = self.build.instances.get_mut(from..)
    {
      for glyph in copied {
        *glyph = glyph.with_slot(self.mark.slot);
      }
    }
  }

  fn glyph(&mut self, glyph: &DrawGlyph) {
    if self.keeps(glyph.min, glyph.max) {
      let instance = self.mark.on(glyph_instance(glyph, self.clip));
      self.build.push(instance, TexNeed::Glyph(glyph.page));
    }
  }
}

/// A glyph's instance: the fragment body of its page's format, sampled as
/// it asks
fn glyph_instance(glyph: &DrawGlyph, clip: ClipRect) -> UiInstance {
  let kind = match glyph.page.format {
    GlyphFormat::Mask => UiKind::MASK,
    GlyphFormat::Color => UiKind::COLOR,
    GlyphFormat::Msdf => UiKind::MSDF,
  };
  let nearest = glyph.sampling == Sampling::Nearest;
  UiInstance::quad(kind, &glyph.instance, clip, nearest, glyph.page.index)
}

/// Remove the items at the places `gone` (ascending) from `items`
fn drop_at<T>(items: &mut Vec<T>, gone: &[u32]) {
  let mut next = gone.iter().copied().peekable();
  let mut index = 0;
  items.retain(|_| {
    let keep = next.peek() != Some(&index);
    if !keep {
      next.next();
    }
    index += 1;
    keep
  });
}

/// The glyph page a glyph instance samples
fn glyph_need(instance: &UiInstance) -> TexNeed {
  // A text with a sprite (a bitmap font's cell) is never kept, and its
  // icons are drawn anew: a kept text holds glyphs alone
  debug_assert!(
    !matches!(instance.kind(), UiKind::SPRITE | UiKind::SPRITE_LINEAR),
    "a kept text holds no sprite"
  );
  let format = match instance.kind() {
    UiKind::MASK => GlyphFormat::Mask,
    UiKind::COLOR => GlyphFormat::Color,
    UiKind::MSDF => GlyphFormat::Msdf,
    _ => return TexNeed::None,
  };
  TexNeed::Glyph(GlyphPage {
    format,
    index: instance.page(),
  })
}
