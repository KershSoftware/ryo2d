use std::sync::Arc;

use glam::Vec2;

use super::*;
use crate::core::asset::SpriteHandle;
use crate::core::graphics::Sprite;
use crate::core::render::UiSpace;
use crate::core::text::{GlyphStyle, Icon, PlacedIcon, TextIcons, TextLayout};
use crate::core::ui::immediate::{FragmentRun, ScrollSlot, UiCommand, UiCommands};
use crate::core::ui::{Color, Rect, UiId};

fn env() -> UiRenderEnv {
  UiRenderEnv {
    space: UiSpace {
      size: Vec2::new(320.0, 240.0),
      px_scale: 1.0,
      image: (320, 240),
      mouse_scale: 1.0,
    },
    on_target: false,
    text: true,
    fonts: 1,
    bitmaps: 0,
  }
}

fn rect(x: f32) -> UiCommand {
  UiCommand::Rect {
    rect: Rect::new(x, 10.0, 50.0, 20.0),
    fill: Color::WHITE,
    radius: 4.0,
    border: None,
    clip: None,
    layer: 0,
  }
}

fn line(points: &[Vec2]) -> UiCommand {
  UiCommand::Line {
    points: points.into(),
    at: Vec2::ZERO,
    width: 2.0,
    color: Color::WHITE,
    closed: false,
    clip: None,
    layer: 0,
  }
}

fn text(layout: &Arc<TextLayout>) -> UiCommand {
  UiCommand::Text {
    layout: layout.clone(),
    origin: Vec2::new(8.0, 8.0),
    reveal: None,
    opacity: 1.0,
    seed: 3,
    clip: None,
    layer: 0,
  }
}

fn commands(list: &[UiCommand]) -> UiCommands {
  let mut commands = UiCommands::default();
  for (order, command) in list.iter().enumerate() {
    commands.push(order as u32, 0, command.clone());
  }
  commands
}

/// A retainer whose image (epoch 7) holds `list`, drawn and presented
fn drawn(list: &[UiCommand]) -> UiRetain {
  let mut retain = UiRetain::default();
  assert!(
    !retain.keep(&commands(list), None, env(), 7),
    "nothing drawn yet"
  );
  retain.presented(false, true, 7);
  retain
}

#[test]
fn the_same_commands_keep_the_image() {
  let layout = Arc::new(TextLayout::default());
  let list = [rect(0.0), text(&layout)];
  let mut retain = drawn(&list);
  assert!(retain.keep(&commands(&list), None, env(), 7));
  assert!(retain.keep(&commands(&list), None, env(), 7), "and again");
}

#[test]
fn a_changed_number_redraws() {
  let mut retain = drawn(&[rect(0.0)]);
  assert!(!retain.keep(&commands(&[rect(0.5)]), None, env(), 7));
}

#[test]
fn another_order_redraws() {
  let mut retain = drawn(&[rect(0.0)]);
  let mut moved = UiCommands::default();
  moved.push(1, 0, rect(0.0));
  assert!(!retain.keep(&moved, None, env(), 7));
}

#[test]
fn an_equal_layout_elsewhere_is_another_text() {
  let mut retain = drawn(&[text(&Arc::new(TextLayout::default()))]);
  let other = Arc::new(TextLayout::default());
  assert!(!retain.keep(&commands(&[text(&other)]), None, env(), 7));
}

#[test]
fn a_text_effect_never_keeps() {
  let layout = Arc::new(TextLayout {
    styles: vec![GlyphStyle {
      color: Color::WHITE,
      outline: None,
      shadow: None,
      glow: None,
      effect: Some("wave".to_string()),
    }],
    ..TextLayout::default()
  });
  assert!(layout.has_effects());
  let list = [text(&layout)];
  let mut retain = drawn(&list);
  for _ in 0..3 {
    assert!(!retain.keep(&commands(&list), None, env(), 7));
    retain.presented(false, true, 7);
  }
}

#[test]
fn another_environment_or_image_redraws() {
  let list = [rect(0.0)];
  let mut retain = drawn(&list);
  let scaled = UiRenderEnv { fonts: 2, ..env() };
  assert!(
    !retain.keep(&commands(&list), None, scaled, 7),
    "the fonts changed"
  );
  retain.presented(false, true, 7);
  assert!(retain.keep(&commands(&list), None, scaled, 7));
  assert!(
    !retain.keep(&commands(&list), None, scaled, 8),
    "a new UI image"
  );
}

#[test]
fn an_unpresented_frame_redraws() {
  let list = [rect(0.0)];
  let mut retain = drawn(&list);
  assert!(retain.keep(&commands(&list), None, env(), 7));
  retain.lost();
  assert!(!retain.keep(&commands(&list), None, env(), 7));
}

#[test]
fn a_frame_whose_ui_pass_did_not_draw_redraws() {
  let list = [rect(0.0)];
  let mut retain = UiRetain::default();
  assert!(!retain.keep(&commands(&list), None, env(), 7));
  retain.presented(false, false, 7);
  assert!(!retain.keep(&commands(&list), None, env(), 7));
}

#[test]
fn a_kept_frame_leaves_the_image_as_drawn() {
  let list = [rect(0.0)];
  let mut retain = drawn(&list);
  assert!(retain.keep(&commands(&list), None, env(), 7));
  // The `ui` pass was disabled: the image is untouched, still the commands'
  retain.presented(true, false, 7);
  assert!(retain.keep(&commands(&list), None, env(), 7));
}

#[test]
fn no_commands_never_keep() {
  let mut retain = drawn(&[]);
  assert!(!retain.keep(&UiCommands::default(), None, env(), 7));
}

#[test]
fn a_sprite_updated_in_place_redraws() {
  let sprite = SpriteHandle::from_sprite(Sprite::solid_rect(4, 4, 255, 0, 0, 255), "swatch");
  let list = [UiCommand::Sprite {
    sprite: sprite.clone(),
    rect: Rect::new(0.0, 0.0, 4.0, 4.0),
    uv: [0.0, 0.0, 1.0, 1.0],
    tint: Color::WHITE,
    filter: Default::default(),
    clip: None,
    layer: 0,
  }];
  let mut retain = drawn(&list);
  assert!(retain.keep(&commands(&list), None, env(), 7));
  sprite.update(Sprite::solid_rect(4, 4, 0, 0, 255, 255));
  assert!(!retain.keep(&commands(&list), None, env(), 7));
}

#[test]
fn a_renamed_icon_redraws() {
  let red = SpriteHandle::from_sprite(Sprite::solid_rect(4, 4, 255, 0, 0, 255), "red");
  let blue = SpriteHandle::from_sprite(Sprite::solid_rect(4, 4, 0, 0, 255, 255), "blue");
  let mut icons = TextIcons::default();
  icons.insert("confirm", red);
  let layout = Arc::new(TextLayout {
    icons: vec![PlacedIcon {
      icon: Icon::Named {
        name: "confirm".to_string(),
        size: None,
      },
      rect: Rect::new(0.0, 0.0, 4.0, 4.0),
      cluster: 0,
    }],
    ..TextLayout::default()
  });
  let list = [text(&layout)];
  let mut retain = UiRetain::default();
  assert!(!retain.keep(&commands(&list), Some(&icons), env(), 7));
  retain.presented(false, true, 7);
  assert!(retain.keep(&commands(&list), Some(&icons), env(), 7));
  icons.insert("confirm", blue);
  assert!(!retain.keep(&commands(&list), Some(&icons), env(), 7));
}

#[test]
fn lines_with_equal_points_keep_and_a_moved_point_redraws() {
  let points = [Vec2::new(1.0, 2.0), Vec2::new(30.0, 40.0)];
  let mut retain = drawn(&[line(&points)]);
  // Another allocation holding the same points, as a widget makes each frame
  assert!(retain.keep(&commands(&[line(&points)]), None, env(), 7));
  let moved = [Vec2::new(1.0, 2.0), Vec2::new(30.0, 41.0)];
  assert!(!retain.keep(&commands(&[line(&moved)]), None, env(), 7));
}

#[test]
fn a_frame_whose_only_change_is_a_slot_offset_is_not_kept() {
  let mut retain = UiRetain::default();
  let clip = Some(Rect::new(0.0, 0.0, 100.0, 100.0));
  let mut commands = UiCommands::default();
  commands.clear();
  let slot = commands.push_slot(ScrollSlot {
    offset: [0, -10],
    clip,
    cull: None,
  });
  commands.push(0, slot, rect(0.0));
  assert!(
    !retain.keep(&commands, None, env(), 7),
    "the first frame is drawn"
  );
  retain.presented(false, true, 7);
  commands.clear();
  let slot = commands.push_slot(ScrollSlot {
    offset: [0, -11],
    clip,
    cull: None,
  });
  commands.push(0, slot, rect(0.0));
  assert!(
    !retain.keep(&commands, None, env(), 7),
    "a moved slot moves pixels"
  );
  retain.presented(false, true, 7);
  assert!(
    retain.keep(&commands, None, env(), 7),
    "unchanged, it is kept"
  );
}

#[test]
fn a_draw_moved_to_another_slot_is_not_kept() {
  let clip = Some(Rect::new(0.0, 0.0, 100.0, 100.0));
  let frame = |slot_of_rect: bool| {
    let mut commands = UiCommands::default();
    let slot = commands.push_slot(ScrollSlot {
      offset: [0, 0],
      clip,
      cull: None,
    });
    commands.push(0, if slot_of_rect { slot } else { 0 }, rect(0.0));
    commands
  };
  let mut retain = UiRetain::default();
  assert!(!retain.keep(&frame(false), None, env(), 7));
  retain.presented(false, true, 7);
  assert!(
    !retain.keep(&frame(true), None, env(), 7),
    "the same command in a slot draws elsewhere"
  );
}

/// Where a fragment's run of two rects goes in a frame
#[derive(Clone, Copy)]
struct Placed {
  id: u64,
  version: u64,
  /// Its second rect's x
  x: f32,
  /// Its root's order
  base: u32,
  /// Its slot (the table holds one besides slot 0)
  slot: u16,
  /// The loose rects before it (the rest go after it, of the frame's two)
  before: usize,
}

const PLACED: Placed = Placed {
  id: 7,
  version: 1,
  x: 20.0,
  base: 10,
  slot: 0,
  before: 0,
};

/// A frame of two loose rects and a fragment's run placed by `at`
fn run_frame(at: Placed) -> UiCommands {
  let mut commands = UiCommands::default();
  commands.clear();
  commands.push_slot(ScrollSlot {
    offset: [0, 0],
    clip: Some(Rect::new(0.0, 0.0, 100.0, 100.0)),
    cull: None,
  });
  let loose = |commands: &mut UiCommands, k: usize| commands.push(k as u32, 0, rect(0.0));
  (0..at.before).for_each(|k| loose(&mut commands, k));
  let start = commands.as_slice().len() as u32;
  commands.push(at.base, at.slot, rect(0.0));
  commands.push(at.base + 1, at.slot, rect(at.x));
  commands.push_run(FragmentRun {
    id: UiId(at.id),
    version: at.version,
    base: at.base,
    layer: 0,
    slot: at.slot,
    draws: start..start + 2,
  });
  (at.before..2).for_each(|k| loose(&mut commands, k));
  commands
}

#[test]
fn an_unchanged_run_is_kept_without_comparing_its_commands() {
  let mut retain = UiRetain::default();
  let frame = |version: u64| run_frame(Placed { version, ..PLACED });
  assert!(
    !retain.keep(&frame(1), None, env(), 7),
    "drawn the first time"
  );
  retain.presented(false, true, 7);
  assert!(
    retain.keep(&frame(1), None, env(), 7),
    "the same run is kept"
  );
  // Its tag vouches for its commands: while its version is the same, its
  // fragment pushed the same ones
  let other = run_frame(Placed { x: 25.0, ..PLACED });
  assert!(
    retain.keep(&other, None, env(), 7),
    "a run's commands are not compared"
  );
  assert!(
    !retain.keep(&frame(2), None, env(), 7),
    "a new version is drawn"
  );
}

#[test]
fn a_run_with_another_tag_or_place_redraws() {
  let mut retain = UiRetain::default();
  assert!(!retain.keep(&run_frame(PLACED), None, env(), 7));
  retain.presented(false, true, 7);
  for (at, what) in [
    (Placed { id: 8, ..PLACED }, "another fragment"),
    (Placed { base: 11, ..PLACED }, "another order"),
    (Placed { slot: 1, ..PLACED }, "another slot"),
    (
      Placed {
        before: 1,
        ..PLACED
      },
      "another place in the list",
    ),
  ] {
    assert!(!retain.keep(&run_frame(at), None, env(), 7), "{what}");
    retain.presented(false, true, 7);
    assert!(retain.keep(&run_frame(at), None, env(), 7), "{what}, again");
    // Back to the first frame
    assert!(!retain.keep(&run_frame(PLACED), None, env(), 7));
    retain.presented(false, true, 7);
  }
}

#[test]
fn draws_beside_a_run_are_compared_one_by_one() {
  let frame = |x: f32| {
    let mut commands = UiCommands::default();
    commands.push(0, 0, rect(0.0));
    commands.push(10, 0, rect(0.0));
    commands.push(11, 0, rect(20.0));
    commands.push_run(FragmentRun {
      id: UiId(7),
      version: 1,
      base: 10,
      layer: 0,
      slot: 0,
      draws: 1..3,
    });
    commands.push(12, 0, rect(x));
    commands
  };
  let mut retain = UiRetain::default();
  assert!(!retain.keep(&frame(0.0), None, env(), 7));
  retain.presented(false, true, 7);
  assert!(retain.keep(&frame(0.0), None, env(), 7));
  assert!(
    !retain.keep(&frame(0.5), None, env(), 7),
    "a loose draw changed"
  );
}

#[test]
fn a_sprite_in_a_run_updated_in_place_redraws() {
  let sprite = SpriteHandle::from_sprite(Sprite::solid_rect(4, 4, 255, 0, 0, 255), "swatch");
  let frame = |x: f32| {
    let mut commands = UiCommands::default();
    commands.push(0, 0, rect(x));
    commands.push(10, 0, rect(0.0));
    commands.push(
      11,
      0,
      UiCommand::Sprite {
        sprite: sprite.clone(),
        rect: Rect::new(0.0, 0.0, 4.0, 4.0),
        uv: [0.0, 0.0, 1.0, 1.0],
        tint: Color::WHITE,
        filter: Default::default(),
        clip: None,
        layer: 0,
      },
    );
    commands.push_run(FragmentRun {
      id: UiId(7),
      version: 1,
      base: 10,
      layer: 0,
      slot: 0,
      draws: 1..3,
    });
    commands
  };
  let mut retain = UiRetain::default();
  assert!(!retain.keep(&frame(0.0), None, env(), 7));
  retain.presented(false, true, 7);
  assert!(retain.keep(&frame(0.0), None, env(), 7));
  sprite.update(Sprite::solid_rect(4, 4, 0, 0, 255, 255));
  assert!(
    !retain.keep(&frame(0.0), None, env(), 7),
    "the run's sprite changed"
  );
  retain.presented(false, true, 7);
  assert!(retain.keep(&frame(0.0), None, env(), 7));
  // Drawn again for a loose draw, the run (seen before with its version)
  // keeps watching its sprite
  assert!(!retain.keep(&frame(1.0), None, env(), 7));
  retain.presented(false, true, 7);
  assert!(retain.keep(&frame(1.0), None, env(), 7));
  sprite.update(Sprite::solid_rect(4, 4, 0, 255, 0, 255));
  assert!(
    !retain.keep(&frame(1.0), None, env(), 7),
    "the run's sprite changed again"
  );
}

#[test]
fn a_run_with_a_text_effect_never_keeps() {
  let layout = Arc::new(TextLayout {
    styles: vec![GlyphStyle {
      color: Color::WHITE,
      outline: None,
      shadow: None,
      glow: None,
      effect: Some("wave".to_string()),
    }],
    ..TextLayout::default()
  });
  let frame = || {
    let mut commands = UiCommands::default();
    commands.push(10, 0, rect(0.0));
    commands.push(11, 0, text(&layout));
    commands.push_run(FragmentRun {
      id: UiId(7),
      version: 1,
      base: 10,
      layer: 0,
      slot: 0,
      draws: 0..2,
    });
    commands
  };
  let mut retain = UiRetain::default();
  for _ in 0..3 {
    assert!(!retain.keep(&frame(), None, env(), 7));
    retain.presented(false, true, 7);
  }
}
