use super::*;
use crate::core::graphics::{GlyphPage, SpriteInstance};
use crate::core::render::PixelRect;
use crate::core::text::GlyphFormat;

#[test]
fn a_ui_instance_is_144_bytes_and_keeps_the_sprite_lanes() {
  assert_eq!(std::mem::size_of::<UiInstance>(), 144);
  let sprite = SpriteInstance::new()
    .with_transform(10.0, 20.0, 0.0, 30.0, 40.0)
    .with_uv(0.1, 0.2, 0.3, 0.4);
  let clip = ClipRect::of(None, (320, 240));
  let quad = UiInstance::quad(UiKind::MASK, &sprite, clip, true, 3);
  assert_eq!(quad.kind(), UiKind::MASK);
  assert_eq!(quad.page(), 3);
  assert_eq!(
    quad.clip(),
    ClipRect {
      x0: 0,
      y0: 0,
      x1: 320,
      y1: 240
    }
  );
  // The eight lanes are the sprite instance's, in its order
  assert_eq!(bytemuck::bytes_of(&quad.data), bytemuck::bytes_of(&sprite));
}

#[test]
fn a_clip_is_the_scissor_or_the_whole_image() {
  let scissor = PixelRect {
    x: 4,
    y: 5,
    width: 10,
    height: 20,
  };
  assert_eq!(
    ClipRect::of(Some(scissor), (320, 240)),
    ClipRect {
      x0: 4,
      y0: 5,
      x1: 14,
      y1: 25
    }
  );
  assert_eq!(
    ClipRect::of(None, (100_000, 240)),
    ClipRect {
      x0: 0,
      y0: 0,
      x1: u16::MAX,
      y1: 240
    }
  );
}

#[test]
fn runs_break_only_when_a_slot_needs_another_texture() {
  let page = |index| GlyphPage {
    format: GlyphFormat::Mask,
    index,
  };
  let needs = [
    TexNeed::None, // a rect
    TexNeed::Glyph(page(0)),
    TexNeed::None,
    TexNeed::Glyph(page(0)),
    TexNeed::Glyph(page(1)), // another mask page: a new run
    TexNeed::None,
    TexNeed::Glyph(page(0)), // back: another run
  ];
  let runs = plan_runs(&needs, |need| match need {
    TexNeed::Glyph(p) => Some((Slot::Mask, TexKey::Glyph(p))),
    _ => None,
  });
  let ranges: Vec<_> = runs.iter().map(|run| run.range.clone()).collect();
  assert_eq!(ranges, vec![0..4, 4..6, 6..7]);
}

#[test]
fn the_kinds_and_the_nearest_flag_are_the_shaders() {
  let shader = crate::core::shaders::compose::UI;
  for (name, kind) in [
    ("RECT", UiKind::RECT),
    ("MASK", UiKind::MASK),
    ("COLOR", UiKind::COLOR),
    ("MSDF", UiKind::MSDF),
    ("SPRITE", UiKind::SPRITE),
    ("SPRITE_LINEAR", UiKind::SPRITE_LINEAR),
    ("LINE", UiKind::LINE),
  ] {
    let constant = format!("const KIND_{name}: u32 = {kind}u;");
    assert!(shader.contains(&constant), "{constant}");
  }
  let clip = ClipRect::of(None, (1, 1));
  let flags = |nearest| {
    UiInstance::quad(UiKind::SPRITE, &SpriteInstance::new(), clip, nearest, 0).meta[3] & 0xFFFF
  };
  assert_eq!(flags(false), 0);
  let constant = format!("const FLAG_NEAREST: u32 = {}u;", flags(true));
  assert!(shader.contains(&constant), "{constant}");
}
