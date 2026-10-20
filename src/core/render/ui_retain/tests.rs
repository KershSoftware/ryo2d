use std::sync::Arc;

use glam::Vec2;

use super::*;
use crate::core::asset::SpriteHandle;
use crate::core::graphics::Sprite;
use crate::core::render::UiSpace;
use crate::core::text::{GlyphStyle, Icon, PlacedIcon, TextIcons, TextLayout};
use crate::core::ui::immediate::{UiCommand, UiCommands};
use crate::core::ui::{Color, Rect};

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
    commands.push(order as u32, command.clone());
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
  moved.push(1, rect(0.0));
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
