//! What the Menus page and its node version share

/// The dialog open over the menu
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Dialog {
  Options,
  Credits,
  Quit,
}

/// The credits: 15 roles and who filled them, 30 lines
pub(super) const CREDITS: [(&str, &str); 15] = [
  ("Game design", "The Wandering Studio"),
  ("Programming", "Ryo2D and its UI"),
  ("Art", "Pixels drawn in code"),
  ("Music", "The sound of silence"),
  ("Sound", "A click, a tick and a whoosh"),
  ("Quest writing", "A shopkeeper who remembers"),
  ("Level design", "One menu, many dialogs"),
  ("Interface", "Containers, scopes and modals"),
  ("Navigation", "The D-pad and nothing else"),
  ("Localisation", "Ελληνικά, Čeština, Русский"),
  ("Fonts", "Inter, JetBrains Mono and m5x7"),
  ("Testing", "A gamepad and two buttons"),
  ("Accessibility", "Every widget within reach"),
  ("Special thanks", "Everyone who plays with a pad"),
  ("Made with", "Ryo2D"),
];

pub(super) const NO_SAVE: &str = "No saved game yet: start a New Game.";
pub(super) const CONTINUES: &str = "Your journey continues.";
pub(super) const NEW_GAME: &str = "A new game begins.";
pub(super) const QUIT_CHOSEN: &str = "Quit chosen (the demo stays open).";
