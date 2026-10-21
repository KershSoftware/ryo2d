//! The frame's UI commands as UI instances, in painter's order. What would
//! draw no pixel is culled before it is built, texts go through the text
//! cache (a reused text is one copy of its instances), and a line becomes
//! its builder's triangles.

use glam::Vec2;

use super::instance::{ClipRect, UiInstance, UiKind};
use super::runs::TexNeed;
use crate::core::graphics::{CameraView, GlyphPage, SpriteInstance, TextureId};
use crate::core::render::batch::{Filter, PolyPoint, Sampling, Triangles, push_polyline};
use crate::core::render::text::{TextPlacement, TextSources, TextTargets};
use crate::core::render::ui::{
  line_of, rect_instance, scissor_of, sprite_instance, text_of, ui_view, ui_z,
};
use crate::core::render::ui_retain::{TextSink, UiRenderEnv, UiTextCache};
use crate::core::render::{DrawGlyph, DrawList, DrawPayload, reach};
use crate::core::text::GlyphFormat;
use crate::core::ui::immediate::{UiCommand, UiCommands};

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

/// The frame's UI instances in painter's order, what each samples, and the
/// sprites among them; reused from frame to frame
#[derive(Default)]
pub(crate) struct UiBuild {
  pub(crate) instances: Vec<UiInstance>,
  /// What each instance samples, one per instance
  pub(crate) needs: Vec<TexNeed>,
  pub(crate) sprites: Vec<SpriteFix>,
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
  }

  fn push(&mut self, instance: UiInstance, need: TexNeed) {
    self.instances.push(instance);
    self.needs.push(need);
  }

  /// A sprite quad, its uv and kind fixed once its texture is resident
  fn push_sprite(
    &mut self,
    instance: &SpriteInstance,
    texture: TextureId,
    filter: Filter,
    clip: ClipRect,
  ) {
    self.sprites.push(SpriteFix {
      index: self.instances.len() as u32,
      texture,
      filter,
    });
    let instance = UiInstance::quad(UiKind::SPRITE, instance, clip, false, 0);
    self.push(instance, TexNeed::Sprite(texture));
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
  seen: Seen,
  sources: Option<&'f TextSources<'f>>,
  real_time: f32,
}

/// Where a command falls in the painter's order: as the batch sorts the old
/// path's items, by z (the layer, then the recorded order), then with lines
/// after the rest at one z (the batch's `DrawKind` order), then as recorded
fn paint_key((order, command): &(u32, UiCommand)) -> (i32, bool) {
  let line = matches!(command, UiCommand::Line { .. });
  (ui_z(command.draw_layer(), *order), line)
}

/// Build the frame's UI commands into `build` (cleared first): the UI's
/// instances in painter's order, what each samples, and its sprites, whose
/// uv and kind are fixed once their textures are resident.
///
/// A command drawing no pixel (wholly outside its clip or the UI camera's
/// view, or with an empty clip) builds nothing. Texts go through `texts`;
/// without `sources` they are skipped. `targets.list` is scratch, cleared
/// for each text and line.
pub(crate) fn build_ui(
  build: &mut UiBuild,
  commands: &UiCommands,
  env: &UiRenderEnv,
  sources: Option<&TextSources<'_>>,
  targets: &mut TextTargets<'_>,
  real_time: f32,
  texts: &mut UiTextCache<UiInstance>,
) {
  build.clear();
  let commands = commands.as_slice();
  let frame = Frame {
    env,
    seen: Seen::of(env),
    sources,
    real_time,
  };
  // Commands usually arrive in the painter's order: sort only when one is
  // out of it
  let mut last = None;
  let in_order = commands.iter().all(|command| {
    let key = Some(paint_key(command));
    let rising = key >= last;
    last = key;
    rising
  });
  if in_order {
    for (order, command) in commands {
      build_command(build, &frame, targets, texts, *order, command);
    }
    return;
  }
  let mut painted = std::mem::take(&mut build.order);
  painted.clear();
  painted.extend(0..commands.len() as u32);
  // Stable: commands equal in the order keep the order recorded
  painted.sort_by_key(|&index| paint_key(&commands[index as usize]));
  for &index in &painted {
    if let Some((order, command)) = commands.get(index as usize) {
      build_command(build, &frame, targets, texts, *order, command);
    }
  }
  build.order = painted;
}

/// Build one command, at its place in the painter's order
fn build_command(
  build: &mut UiBuild,
  frame: &Frame<'_>,
  targets: &mut TextTargets<'_>,
  texts: &mut UiTextCache<UiInstance>,
  order: u32,
  command: &UiCommand,
) {
  let space = &frame.env.space;
  let Some(scissor) = scissor_of(command.clip_rect(), space) else {
    return;
  };
  let clip = ClipRect::of(scissor, space.image);
  let z = ui_z(command.draw_layer(), order);
  match command {
    UiCommand::Rect {
      rect,
      fill,
      radius,
      border,
      ..
    } => {
      let pixel_art = frame.env.on_target;
      let Some((instance, min, max)) =
        rect_instance(*rect, *fill, *radius, *border, space, pixel_art)
      else {
        return;
      };
      if frame.seen.sees(min, max, clip) {
        let instance = UiInstance::quad(UiKind::RECT, &instance, clip, false, 0);
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
        return;
      };
      // Asked for before culling, as the old path does
      let texture = targets.sprites.texture_of(sprite);
      let (min, max) = reach(&instance);
      if frame.seen.sees(min, max, clip) {
        build.push_sprite(&instance, texture, *filter, clip);
      }
    }
    UiCommand::Line { .. } => build_line(build, &frame.seen, targets.list, command, z, clip),
    UiCommand::Text { .. } => {
      let Some(sources) = frame.sources else {
        return;
      };
      let Some((layout, place)) = text_of(command, space, z, scissor, frame.real_time) else {
        return;
      };
      targets.list.clear();
      let mut sink = TextInstances {
        build,
        seen: &frame.seen,
        clip,
        taken: 0,
      };
      texts.emit_into(sources, targets, &mut sink, layout, &place);
    }
  }
}

/// A line command as its builder's triangles, one instance each, in index
/// order. The line is culled by its box as the batch culls it; then a
/// triangle wholly outside the clip is left out (lines are never snapped).
fn build_line(
  build: &mut UiBuild,
  seen: &Seen,
  list: &mut DrawList,
  command: &UiCommand,
  z: i32,
  clip: ClipRect,
) {
  let UiBuild {
    instances,
    needs,
    points,
    triangles,
    ..
  } = build;
  let Some((style, look)) = line_of(command, z, points) else {
    return;
  };
  list.clear();
  triangles.clear();
  push_polyline(list, triangles, points, &style, look);
  let Some(line) = list.triangles(0) else {
    return;
  };
  if !seen.view.is_visible(line.min, line.max) {
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
    if seen.meets(min, max, clip) {
      instances.push(UiInstance::triangle([a, b, c], params, clip));
      needs.push(TexNeed::None);
    }
  }
}

/// A text's glyphs, bitmap cells and icons, taken from the scratch draw
/// list into the UI's instances, each culled by its box. A built text keeps
/// the instances of its glyphs that were not culled: what is culled follows
/// from the text cache's key (placement and clip) and environment (the UI
/// space), so the same text is culled the same way when it is reused.
struct TextInstances<'b> {
  build: &'b mut UiBuild,
  seen: &'b Seen,
  clip: ClipRect,
  /// The scratch list's items already taken
  taken: usize,
}

impl TextSink<UiInstance> for TextInstances<'_> {
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
          if let Some(glyph) = list.glyph(index)
            && self.seen.sees(glyph.min, glyph.max, self.clip)
          {
            let instance = glyph_instance(glyph, self.clip);
            self.build.push(instance, TexNeed::Glyph(glyph.page));
          }
        }
        DrawPayload::Sprite(index) => {
          if let Some(sprite) = list.sprite(index)
            && self.seen.sees(sprite.min, sprite.max, self.clip)
          {
            let clip = self.clip;
            self
              .build
              .push_sprite(&sprite.instance, sprite.texture, sprite.filter, clip);
          }
        }
        // Text lays glyphs and sprites only
        DrawPayload::Triangles(_) | DrawPayload::Shape(_) => {}
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
    // Their clip is the text's, part of its key
    self.build.instances.extend_from_slice(glyphs);
    self.build.needs.extend(glyphs.iter().map(glyph_need));
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

/// The glyph page a glyph instance samples
fn glyph_need(instance: &UiInstance) -> TexNeed {
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
