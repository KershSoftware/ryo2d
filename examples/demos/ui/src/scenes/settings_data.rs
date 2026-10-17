//! What the Settings page and its node version share

use demo_kit::prelude::*;

pub(super) const TABS: [&str; 3] = ["Audio", "Video", "Controls"];
pub(super) const OUTPUTS: [&str; 3] = ["Default", "Headphones", "Speakers"];
pub(super) const WINDOWS: [&str; 3] = ["Windowed", "Borderless", "Fullscreen"];
/// A display's modes: more than fit under the dropdown, so its list scrolls
pub(super) const RESOLUTIONS: [&str; 21] = [
  "640×480",
  "800×600",
  "1024×768",
  "1152×864",
  "1280×720",
  "1280×800",
  "1280×1024",
  "1360×768",
  "1366×768",
  "1440×900",
  "1536×864",
  "1600×900",
  "1680×1050",
  "1920×1080",
  "1920×1200",
  "2048×1152",
  "2560×1080",
  "2560×1440",
  "2560×1600",
  "3440×1440",
  "3840×2160",
];
/// The page's actions and their labels, for the Controls tab
pub(super) const ACTIONS: [(&str, &str); 7] = [
  ("move_left", "Move left"),
  ("move_right", "Move right"),
  ("jump", "Jump"),
  ("attack", "Attack"),
  ("interact", "Interact"),
  ("inventory", "Inventory"),
  ("pause", "Pause"),
];
/// The widest a row's widget is
pub(super) const WIDGET_WIDTH: f32 = 220.0;

/// The actions the Controls tab rebinds
pub(super) fn bind_actions(ctx: &mut SceneContext) {
  use Binding::{Key, Pad};
  let stick = |direction| {
    Binding::Stick(StickDirection {
      stick: Stick::Left,
      direction,
    })
  };
  ctx
    .actions()
    .bind(
      "move_left",
      [
        Key(KeyCode::KeyA),
        Key(KeyCode::ArrowLeft),
        stick(Direction::Left),
      ],
    )
    .bind(
      "move_right",
      [
        Key(KeyCode::KeyD),
        Key(KeyCode::ArrowRight),
        stick(Direction::Right),
      ],
    )
    .bind("jump", [Key(KeyCode::Space), Pad(GamepadButton::South)])
    .bind("attack", [Key(KeyCode::KeyJ), Pad(GamepadButton::West)])
    .bind("interact", [Key(KeyCode::KeyE), Pad(GamepadButton::North)])
    .bind(
      "inventory",
      [Key(KeyCode::KeyI), Pad(GamepadButton::Select)],
    )
    .bind("pause", [Key(KeyCode::KeyP), Pad(GamepadButton::Start)]);
}
