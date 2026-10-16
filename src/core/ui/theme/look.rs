//! How a widget looks: a [`Look`] per kind and state, patches that change
//! only what they name, and the named variants that extend a kind

use std::collections::BTreeSet;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use super::Theme;
use crate::core::ui::{Color, Edges};

/// Names already warned about, so a bad name warns once however often a
/// frame asks for it
static WARNED: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

/// Warn about `key` once; `message` is built only the first time
pub(crate) fn warn_once(key: String, message: impl FnOnce() -> String) {
  let first = WARNED.lock().is_ok_and(|mut warned| warned.insert(key));
  if first {
    crate::engine_warn!(module: "ui", "{}", message());
  }
}

/// The kinds of widget a theme styles
#[derive(
  Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize,
)]
pub enum WidgetKind {
  #[default]
  Button,
  Panel,
  Checkbox,
  Toggle,
  Radio,
  Slider,
  TextInput,
  Dropdown,
  Tab,
  Choice,
  Tooltip,
  /// A scroll area's bar: `fill` is the track, `border` the thumb
  Scroll,
  Slot,
  /// A progress bar: `fill` is the track, `border` the filled part
  Progress,
  Rebind,
  Modal,
  Prompt,
}

/// What a widget is doing, which picks the look it is drawn in
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum WidgetState {
  #[default]
  Normal,
  Hovered,
  Pressed,
  /// Focused with the focus ring showing
  Focused,
  Disabled,
}

/// A resolved look: everything a widget needs to draw itself in one state
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Look {
  /// The fill; with a skin, the tint on its sprite (white keeps its colours)
  pub fill: Color,
  pub border: Color,
  pub border_width: f32,
  pub radius: f32,
  /// The label's colour
  pub text: Color,
  /// Room between the edges and the contents; zero takes the widget's size
  /// from the theme's metrics
  pub padding: Edges,
  /// Moves the contents, in UI units (a pressed look nudging its label)
  pub offset: [f32; 2],
  /// The name of a theme skin drawn in place of the fill and border
  pub skin: Option<String>,
}

impl Default for Look {
  fn default() -> Look {
    Look {
      fill: Color::TRANSPARENT,
      border: Color::TRANSPARENT,
      border_width: 0.0,
      radius: 0.0,
      text: Color::WHITE,
      padding: Edges::ZERO,
      offset: [0.0, 0.0],
      skin: None,
    }
  }
}

/// Changes to a look: only what is `Some` replaces
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LookPatch {
  pub fill: Option<Color>,
  pub border: Option<Color>,
  pub border_width: Option<f32>,
  pub radius: Option<f32>,
  pub text: Option<Color>,
  pub padding: Option<Edges>,
  pub offset: Option<[f32; 2]>,
  pub skin: Option<String>,
}

impl LookPatch {
  /// Change the fill
  pub fn fill(mut self, color: Color) -> Self {
    self.fill = Some(color);
    self
  }

  /// Change the border colour
  pub fn border(mut self, color: Color) -> Self {
    self.border = Some(color);
    self
  }

  /// Change the label colour
  pub fn text(mut self, color: Color) -> Self {
    self.text = Some(color);
    self
  }

  /// Change the corner radius
  pub fn radius(mut self, radius: f32) -> Self {
    self.radius = Some(radius);
    self
  }

  /// Draw the theme skin `name`
  pub fn skin(mut self, name: &str) -> Self {
    self.skin = Some(name.to_string());
    self
  }

  /// Replace what `look` has with what this patch names
  pub(crate) fn apply(&self, look: &mut Look) {
    if let Some(fill) = self.fill {
      look.fill = fill;
    }
    if let Some(border) = self.border {
      look.border = border;
    }
    if let Some(width) = self.border_width {
      look.border_width = width;
    }
    if let Some(radius) = self.radius {
      look.radius = radius;
    }
    if let Some(text) = self.text {
      look.text = text;
    }
    if let Some(padding) = self.padding {
      look.padding = padding;
    }
    if let Some(offset) = self.offset {
      look.offset = offset;
    }
    if let Some(skin) = &self.skin {
      look.skin = Some(skin.clone());
    }
  }
}

/// Apply the patches `state` calls for to `look`: hovered for hovered and
/// pressed, then pressed for pressed; focused and disabled their own
fn apply_state(look: &mut Look, state: WidgetState, patches: [&LookPatch; 4]) {
  let [hovered, pressed, focused, disabled] = patches;
  match state {
    WidgetState::Normal => {}
    WidgetState::Hovered => hovered.apply(look),
    WidgetState::Pressed => {
      hovered.apply(look);
      pressed.apply(look);
    }
    WidgetState::Focused => focused.apply(look),
    WidgetState::Disabled => disabled.apply(look),
  }
}

/// One widget kind's looks: a normal look and what each other state changes
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Looks {
  pub normal: Look,
  pub hovered: LookPatch,
  /// Applied over `hovered`
  pub pressed: LookPatch,
  /// Applied over `normal` alone
  pub focused: LookPatch,
  pub disabled: LookPatch,
}

impl Looks {
  /// The look in `state`: `normal` with the state's patch over it
  pub fn resolve(&self, state: WidgetState) -> Look {
    let mut look = self.normal.clone();
    apply_state(
      &mut look,
      state,
      [&self.hovered, &self.pressed, &self.focused, &self.disabled],
    );
    look
  }
}

/// The looks of every widget kind
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WidgetLooks {
  pub button: Looks,
  pub panel: Looks,
  pub checkbox: Looks,
  pub toggle: Looks,
  pub radio: Looks,
  pub slider: Looks,
  pub text_input: Looks,
  pub dropdown: Looks,
  pub tab: Looks,
  pub choice: Looks,
  pub tooltip: Looks,
  pub scroll: Looks,
  pub slot: Looks,
  pub progress: Looks,
  pub rebind: Looks,
  pub modal: Looks,
  pub prompt: Looks,
}

impl WidgetLooks {
  /// The looks of `kind`
  pub fn get(&self, kind: WidgetKind) -> &Looks {
    match kind {
      WidgetKind::Button => &self.button,
      WidgetKind::Panel => &self.panel,
      WidgetKind::Checkbox => &self.checkbox,
      WidgetKind::Toggle => &self.toggle,
      WidgetKind::Radio => &self.radio,
      WidgetKind::Slider => &self.slider,
      WidgetKind::TextInput => &self.text_input,
      WidgetKind::Dropdown => &self.dropdown,
      WidgetKind::Tab => &self.tab,
      WidgetKind::Choice => &self.choice,
      WidgetKind::Tooltip => &self.tooltip,
      WidgetKind::Scroll => &self.scroll,
      WidgetKind::Slot => &self.slot,
      WidgetKind::Progress => &self.progress,
      WidgetKind::Rebind => &self.rebind,
      WidgetKind::Modal => &self.modal,
      WidgetKind::Prompt => &self.prompt,
    }
  }
}

impl Default for WidgetLooks {
  /// The dark theme's looks
  fn default() -> WidgetLooks {
    super::builtin::dark().widgets
  }
}

/// A named change to a widget kind's looks, picked with `.variant(name)`
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Variant {
  /// The kind it is made for (written as a string in files: `"Button"`)
  #[serde(with = "super::names")]
  pub base: WidgetKind,
  pub normal: LookPatch,
  pub hovered: LookPatch,
  pub pressed: LookPatch,
  pub focused: LookPatch,
  pub disabled: LookPatch,
}

impl Variant {
  /// A variant of `base` that changes nothing yet
  pub fn of(base: WidgetKind) -> Variant {
    Variant {
      base,
      ..Variant::default()
    }
  }

  /// What it changes in every state
  pub fn normal(mut self, patch: LookPatch) -> Self {
    self.normal = patch;
    self
  }

  /// What it changes when hovered (and pressed)
  pub fn hovered(mut self, patch: LookPatch) -> Self {
    self.hovered = patch;
    self
  }

  /// What it changes when pressed
  pub fn pressed(mut self, patch: LookPatch) -> Self {
    self.pressed = patch;
    self
  }

  /// What it changes when focused
  pub fn focused(mut self, patch: LookPatch) -> Self {
    self.focused = patch;
    self
  }

  /// What it changes when disabled; without one it takes its kind's
  /// disabled look
  pub fn disabled(mut self, patch: LookPatch) -> Self {
    self.disabled = patch;
    self
  }
}

impl Theme {
  /// The look of a `kind` widget in `state`, in `variant` when named.
  /// A variant's `normal` patch applies in every state, then its patch for
  /// the state (hovered, pressed and focused over the kind's resolved state;
  /// disabled is the variant's own patch, or the kind's disabled patch when
  /// the variant has none). An unknown variant name warns once and gives the plain look.
  pub fn look(&self, kind: WidgetKind, variant: Option<&str>, state: WidgetState) -> Look {
    let mut look = self.widgets.get(kind).resolve(state);
    let Some(name) = variant else {
      return look;
    };
    match self.variants.get(name) {
      Some(variant) => {
        variant.normal.apply(&mut look);
        if state == WidgetState::Disabled {
          // A variant without a disabled patch is dimmed like its kind, so
          // every disabled widget reads the same
          let own = variant.disabled != LookPatch::default();
          let patch = if own {
            &variant.disabled
          } else {
            &self.widgets.get(kind).disabled
          };
          patch.apply(&mut look);
        } else {
          apply_state(
            &mut look,
            state,
            [
              &variant.hovered,
              &variant.pressed,
              &variant.focused,
              &variant.disabled,
            ],
          );
        }
      }
      None => warn_once(format!("variant:{name}"), || {
        format!("no UI variant named {name:?}; using the plain look")
      }),
    }
    look
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ui::Color;

  fn looks() -> Looks {
    Looks {
      normal: Look {
        fill: Color::rgb(10, 10, 10),
        border: Color::rgb(20, 20, 20),
        text: Color::WHITE,
        ..Look::default()
      },
      hovered: LookPatch::default().fill(Color::rgb(30, 30, 30)),
      pressed: LookPatch::default().border(Color::rgb(90, 90, 90)),
      focused: LookPatch::default(),
      disabled: LookPatch::default().text(Color::GRAY),
    }
  }

  #[test]
  fn a_state_patch_overrides_only_its_fields() {
    let looks = looks();
    let hovered = looks.resolve(WidgetState::Hovered);
    assert_eq!(
      (hovered.fill, hovered.border),
      (Color::rgb(30, 30, 30), Color::rgb(20, 20, 20))
    );
    let pressed = looks.resolve(WidgetState::Pressed);
    assert_eq!(
      (pressed.fill, pressed.border),
      (Color::rgb(30, 30, 30), Color::rgb(90, 90, 90)),
      "pressed builds on hovered"
    );
    assert_eq!(looks.resolve(WidgetState::Disabled).text, Color::GRAY);
  }

  #[test]
  fn a_variant_extends_its_base() {
    let theme = Theme::dark().variant(
      "gold",
      Variant::of(WidgetKind::Button).normal(LookPatch::default().fill(Color::YELLOW)),
    );
    let gold = theme.look(WidgetKind::Button, Some("gold"), WidgetState::Normal);
    let plain = theme.look(WidgetKind::Button, None, WidgetState::Normal);
    assert_eq!(gold.fill, Color::YELLOW);
    assert_eq!(gold.border, plain.border, "the rest is the base's");
  }

  #[test]
  fn an_unknown_variant_warns_once_and_uses_the_plain_look() {
    let theme = Theme::dark();
    let plain = theme.look(WidgetKind::Button, None, WidgetState::Normal);
    for _ in 0..3 {
      assert_eq!(
        theme.look(
          WidgetKind::Button,
          Some("nope-variant"),
          WidgetState::Normal
        ),
        plain
      );
    }
    let warnings = crate::core::dev_log::get_entries()
      .iter()
      .filter(|e| e.message.contains("nope-variant"))
      .count();
    assert_eq!(warnings, 1);
  }
}
