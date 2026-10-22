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

#[test]
fn a_slot_rides_in_the_flags_beside_the_nearest_flag_and_the_page() {
  let clip = ClipRect::of(None, (320, 240));
  let quad = UiInstance::quad(UiKind::MASK, &SpriteInstance::new(), clip, true, 3);
  assert_eq!(quad.slot(), 0);
  assert!(!quad.own_clip());
  for slot in [1, 7, 255] {
    let placed = quad.with_slot(slot);
    assert_eq!(placed.slot(), slot);
    assert_eq!(
      (placed.kind(), placed.page(), placed.clip()),
      (quad.kind(), quad.page(), quad.clip())
    );
    assert_eq!(placed.meta[3] & 0xFF, quad.meta[3] & 0xFF, "the flags");
    assert_eq!(placed.with_slot(0), quad);
    // Its own clip is a flag of its own
    let own = placed.with_own_clip();
    assert!(own.own_clip());
    assert_eq!(own.slot(), slot);
    assert_eq!(own.page(), 3);
  }
}

#[test]
fn the_slot_bits_and_the_slot_table_are_the_shaders() {
  let shader = crate::core::shaders::compose::UI;
  let clip = ClipRect::of(None, (1, 1));
  let quad = UiInstance::quad(UiKind::RECT, &SpriteInstance::new(), clip, false, 0);
  let shift = quad.with_slot(1).meta[3].trailing_zeros();
  let constant = format!("const SLOT_SHIFT: u32 = {shift}u;");
  assert!(shader.contains(&constant), "{constant}");
  let own = quad.with_own_clip().meta[3];
  let constant = format!("const FLAG_OWN_CLIP: u32 = {own}u;");
  assert!(shader.contains(&constant), "{constant}");
  let table = format!("var<uniform> slots: array<Slot, {SLOTS}>;");
  assert!(shader.contains(&table), "{table}");
  // A uniform array's elements are 16-byte aligned
  assert_eq!(std::mem::size_of::<SlotUniform>(), 32);
  assert_eq!(SLOTS, usize::from(u8::MAX) + 1, "a slot fits its bits");
}

#[test]
fn an_own_clip_moves_with_its_slot_and_meets_the_slots_clip() {
  let slot = ClipRect {
    x0: 0,
    y0: 10,
    x1: 100,
    y1: 60,
  };
  let own = ClipRect {
    x0: 4,
    y0: 5,
    x1: 44,
    y1: 15,
  };
  assert_eq!(
    own.in_slot([0, 12], slot),
    ClipRect {
      x0: 4,
      y0: 17,
      x1: 44,
      y1: 27
    }
  );
  // Moved up past the slot's top: cut there
  assert_eq!(
    own.in_slot([-3, 0], slot),
    ClipRect {
      x0: 1,
      y0: 10,
      x1: 41,
      y1: 15
    }
  );
  // Moved past the image's left edge: from 0
  let moved = own.in_slot([-50, 12], ClipRect::of(None, (320, 240)));
  assert_eq!((moved.x0, moved.x1), (0, 0));
  assert!(moved.is_empty());
  assert!(!own.is_empty());
}

mod slots {
  use glam::Vec2;

  use super::super::build::{rect_built, slot_uniforms};
  use super::*;
  use crate::core::render::UiSpace;
  use crate::core::render::ui_retain::UiRenderEnv;
  use crate::core::ui::immediate::{ScrollSlot, UiCommand, UiDraw};
  use crate::core::ui::{Color, Rect};

  fn env_at(px_scale: f32) -> UiRenderEnv {
    UiRenderEnv {
      space: UiSpace {
        size: Vec2::new(320.0, 240.0) / px_scale,
        px_scale,
        image: (320, 240),
        mouse_scale: 1.0,
      },
      on_target: false,
      text: true,
      fonts: 1,
      bitmaps: 0,
    }
  }

  fn rect(rect: Rect, clip: Option<Rect>) -> UiCommand {
    UiCommand::Rect {
      rect,
      fill: Color::new(40, 160, 220, 200),
      radius: 0.0,
      border: None,
      clip,
      layer: 0,
    }
  }

  /// A quad's centre (its third lane is its translation)
  fn centre(instance: &UiInstance) -> Vec2 {
    Vec2::new(instance.data[2][0], instance.data[2][1])
  }

  /// A slot drawn 12 pixels down, under a clip 50 high from y = 10
  const SLOT: ScrollSlot = ScrollSlot {
    offset: [0, 12],
    clip: Some(Rect {
      x: 0.0,
      y: 10.0,
      width: 100.0,
      height: 50.0,
    }),
    cull: None,
  };

  /// The slot's clip in image pixels
  const SLOT_CLIP: ClipRect = ClipRect {
    x0: 0,
    y0: 10,
    x1: 100,
    y1: 60,
  };

  #[test]
  fn a_draw_in_a_slot_builds_relative_to_it_under_its_clip() {
    let env = env_at(1.0);
    let mut slots = vec![ScrollSlot::NONE, SLOT];
    let draw = UiDraw {
      order: 0,
      slot: 1,
      command: rect(Rect::new(0.0, 0.0, 50.0, 20.0), None),
    };
    let instance = rect_built(&draw, &slots, &env).expect("seen");
    assert_eq!(instance.slot(), 1);
    assert!(!instance.own_clip());
    // Where the command puts it, not moved by the slot's offset
    assert_eq!(centre(&instance), Vec2::new(25.0, 10.0));
    // The shader takes its clip from the slot: it carries the whole image,
    // the same wherever the slot is drawn and whatever it is clipped by
    assert_eq!(instance.clip(), ClipRect::of(None, env.space.image));

    // Past the slots the shader holds: moved on the CPU, as before
    slots.resize(301, SLOT);
    let draw = UiDraw { slot: 300, ..draw };
    let instance = rect_built(&draw, &slots, &env).expect("seen");
    assert_eq!(instance.slot(), 0);
    assert_eq!(centre(&instance), Vec2::new(25.0, 22.0));
    assert_eq!(instance.clip(), SLOT_CLIP);
  }

  #[test]
  fn a_draw_in_a_slot_is_culled_where_its_offset_puts_it() {
    let env = env_at(1.0);
    let slots = [ScrollSlot::NONE, SLOT];
    let seen = |y: f32| {
      let draw = UiDraw {
        order: 0,
        slot: 1,
        command: rect(Rect::new(0.0, y, 50.0, 5.0), None),
      };
      rect_built(&draw, &slots, &env).is_some()
    };
    // Drawn at 14 to 19: inside the clip, though not before the offset
    assert!(seen(2.0));
    // Drawn at 2 to 7 and at 62 to 67: outside, though not before it
    assert!(!seen(-10.0));
    assert!(!seen(50.0));
  }

  #[test]
  fn a_draw_with_its_own_clip_in_a_slot_carries_it_relative_to_the_slot() {
    let env = env_at(1.0);
    let slots = [ScrollSlot::NONE, SLOT];
    // A text field's view inside a scrolled list, relative to the slot
    let own = Rect::new(4.0, 5.0, 40.0, 10.0);
    let draw = UiDraw {
      order: 0,
      slot: 1,
      command: rect(Rect::new(0.0, 0.0, 50.0, 20.0), Some(own)),
    };
    let instance = rect_built(&draw, &slots, &env).expect("seen");
    assert_eq!((instance.slot(), instance.own_clip()), (1, true));
    assert_eq!(centre(&instance), Vec2::new(25.0, 10.0));
    assert_eq!(
      instance.clip(),
      ClipRect {
        x0: 4,
        y0: 5,
        x1: 44,
        y1: 15
      }
    );
    // Moved out of the slot's clip by its offset: culled
    let draw = UiDraw {
      command: rect(
        Rect::new(0.0, 0.0, 50.0, 20.0),
        Some(Rect::new(4.0, -10.0, 40.0, 5.0)),
      ),
      ..draw
    };
    assert!(rect_built(&draw, &slots, &env).is_none());
  }

  /// Ruling R1: on the pixel-art target a draw in a slot is moved and
  /// clipped on the CPU, as past the shader's slots
  #[test]
  fn on_the_pixel_art_target_a_draw_in_a_slot_is_moved_on_the_cpu() {
    let env = UiRenderEnv {
      on_target: true,
      ..env_at(1.0)
    };
    let slots = [ScrollSlot::NONE, SLOT];
    let draw = UiDraw {
      order: 0,
      slot: 1,
      command: rect(Rect::new(0.0, 0.0, 50.0, 20.0), None),
    };
    let instance = rect_built(&draw, &slots, &env).expect("seen");
    assert_eq!(instance.slot(), 0);
    assert_eq!(centre(&instance), Vec2::new(25.0, 22.0));
    assert_eq!(instance.clip(), SLOT_CLIP);
  }

  /// A clip of its own beyond a `ClipRect`'s reach in its slot (a text
  /// field 70,000 pixels down a list): the draw is moved and clipped on the
  /// CPU
  #[test]
  fn a_draw_whose_own_clip_is_out_of_reach_is_moved_on_the_cpu() {
    let env = env_at(1.0);
    let slot = ScrollSlot {
      offset: [0, -69_990],
      ..SLOT
    };
    let slots = [ScrollSlot::NONE, slot];
    let own = Rect::new(4.0, 70_000.0, 40.0, 10.0);
    let draw = UiDraw {
      order: 0,
      slot: 1,
      command: rect(Rect::new(0.0, 69_995.0, 50.0, 20.0), Some(own)),
    };
    let instance = rect_built(&draw, &slots, &env).expect("seen");
    assert_eq!((instance.slot(), instance.own_clip()), (0, false));
    assert_eq!(centre(&instance), Vec2::new(25.0, 15.0));
    assert_eq!(
      instance.clip(),
      ClipRect {
        x0: 4,
        y0: 10,
        x1: 44,
        y1: 20
      }
    );
  }

  #[test]
  fn the_slot_uniforms_are_each_slots_offset_and_clip() {
    let env = env_at(1.25);
    let space = &env.space;
    let slot = ScrollSlot {
      offset: [3, -7],
      ..SLOT
    };
    let mut slots = vec![ScrollSlot::NONE, slot];
    let mut uniforms = Vec::new();
    slot_uniforms(&slots, space, &mut uniforms);
    // Slot 0: nothing moved, the whole image
    assert_eq!(
      uniforms[0],
      SlotUniform {
        offset: [0.0, 0.0],
        pixels: [0, 0],
        clip: [0, 0, 320, 240],
      }
    );
    // The offset Task 2 moved draws by on the CPU, in UI units, and in
    // whole pixels; the clip as the scissor would be
    let by = slot.offset_units(space.px_scale);
    assert_eq!(
      uniforms[1],
      SlotUniform {
        offset: [by.x, by.y],
        pixels: [3, -7],
        clip: [0, 12, 125, 75],
      }
    );
    // Only the slots the shader holds
    slots.resize(300, slot);
    slot_uniforms(&slots, space, &mut uniforms);
    assert_eq!(uniforms.len(), SLOTS);
  }
}
