use bytemuck::Zeroable;

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

#[test]
fn a_run_break_starts_the_next_run_with_only_the_breaking_texture() {
  use crate::core::graphics::{SlotPage, TextureId};
  let page = |index| GlyphPage {
    format: GlyphFormat::Mask,
    index,
  };
  // Sprite texture n sits on atlas page n
  let atlas = |index: u32| TexKey::Sprite(SlotPage::Atlas(index as usize));
  // Glyph page 0, sprite A, sprite B (a break), glyph page 1: the run
  // sprite B starts holds no glyph page, so page 1 joins it
  let needs = [
    TexNeed::Glyph(page(0)),
    TexNeed::Sprite(TextureId::for_test(1)),
    TexNeed::Sprite(TextureId::for_test(2)),
    TexNeed::Glyph(page(1)),
  ];
  let runs = plan_runs(&needs, |need| match need {
    TexNeed::Glyph(p) => Some((Slot::Mask, TexKey::Glyph(p))),
    TexNeed::Sprite(texture) => {
      let index = (0..8).find(|&i| TextureId::for_test(i) == texture)?;
      Some((Slot::Sprite, atlas(index)))
    }
    TexNeed::None => None,
  });
  let ranges: Vec<_> = runs.iter().map(|run| run.range.clone()).collect();
  assert_eq!(ranges, vec![0..2, 2..4]);
  assert_eq!(
    runs[1].textures,
    [Some(TexKey::Glyph(page(1))), None, None, Some(atlas(2))]
  );
}

/// `n` instances, each told apart by its first lane
fn instances(n: usize) -> Vec<UiInstance> {
  (0..n)
    .map(|i| {
      let mut instance = UiInstance::zeroed();
      instance.data[0][0] = i as f32;
      instance
    })
    .collect()
}

#[test]
fn the_upload_is_the_one_range_that_changed() {
  let held = instances(100);
  let changed = |at: &[usize]| {
    let mut frame = held.clone();
    for &i in at {
      frame[i].data[3][2] += 1.0;
    }
    frame
  };
  // Equal: nothing
  assert!(changed_range(&held, &held).is_empty());
  // Equal lengths: from the first instance that differs to the last
  for first in 0..100 {
    for last in first..100 {
      assert_eq!(
        changed_range(&held, &changed(&[first, last])),
        first..last + 1
      );
    }
  }
  assert_eq!(changed_range(&held, &changed(&[40, 43, 47])), 40..48);
  // Compared bit for bit: zero and minus zero differ
  let mut signed = held.clone();
  signed[60].data[7][3] = -0.0;
  assert_eq!(changed_range(&held, &signed), 60..61);
  // Other lengths: from the first difference to the frame's end
  let mut longer = held.clone();
  longer.extend(instances(120).split_off(100));
  assert_eq!(changed_range(&held, &longer), 100..120);
  longer[10].data[3][2] += 1.0;
  assert_eq!(changed_range(&held, &longer), 10..120);
  assert!(changed_range(&held, &held[..70]).is_empty());
  assert_eq!(changed_range(&held, &changed(&[30])[..70]), 30..70);
  // A new or grown buffer holds nothing yet: the whole frame
  assert_eq!(changed_range(&[], &held), 0..100);
  assert!(changed_range(&[], &[]).is_empty());
}

#[test]
fn a_rect_in_a_slot_builds_as_the_same_rect_placed_absolute() {
  use crate::core::render::UiSpace;
  use crate::core::render::ui::{rect_instance, scissor_of};
  use crate::core::ui::immediate::{ScrollSlot, UiCommand, UiDraw};
  use crate::core::ui::{Color, Rect};
  use glam::Vec2;

  let space = UiSpace {
    size: Vec2::new(320.0, 240.0),
    px_scale: 1.0,
    image: (320, 240),
    mouse_scale: 1.0,
  };
  let rect = |rect, clip| UiCommand::Rect {
    rect,
    fill: Color::new(40, 160, 220, 200),
    radius: 4.0,
    border: Some((1.5, Color::WHITE)),
    clip,
    layer: 0,
  };
  let slots = [
    ScrollSlot {
      offset: [0, 0],
      clip: None,
      cull: None,
    },
    ScrollSlot {
      offset: [0, 7],
      clip: Some(Rect::new(0.0, 5.0, 100.0, 10.0)),
      cull: None,
    },
  ];
  // The instance (data and clip) a rect draw builds, as `build_command` does
  let instance = |draw: &UiDraw| {
    let command = draw.placed(&slots, space.px_scale).expect("a slot");
    let UiCommand::Rect {
      rect,
      fill,
      radius,
      border,
      ..
    } = *command
    else {
      panic!("a rect")
    };
    let scissor = scissor_of(command.clip_rect(), &space).expect("a clip that draws");
    let (quad, _, _) = rect_instance(rect, fill, radius, border, &space, false).expect("a rect");
    UiInstance::quad(
      UiKind::RECT,
      &quad,
      ClipRect::of(scissor, space.image),
      false,
      0,
    )
  };
  let in_slot = UiDraw {
    order: 0,
    slot: 1,
    command: rect(Rect::new(0.0, 0.0, 50.0, 20.0), None),
  };
  let absolute = UiDraw {
    order: 0,
    slot: 0,
    command: rect(
      Rect::new(0.0, 7.0, 50.0, 20.0),
      Some(Rect::new(0.0, 5.0, 100.0, 10.0)),
    ),
  };
  assert_eq!(
    bytemuck::bytes_of(&instance(&in_slot)),
    bytemuck::bytes_of(&instance(&absolute))
  );
  let lost = UiDraw { slot: 2, ..in_slot };
  assert!(
    lost.placed(&slots, space.px_scale).is_none(),
    "past the table"
  );
}
