//! The UI theme: a palette, metrics, text roles, a look for every widget
//! kind and state, named variants, nine-slice skins and sounds.
//!
//! The theme is the [`UiTheme`] world resource (`ctx.ui_theme()`); the UI
//! reads it once a pass, so a change applies from the next frame. A theme is
//! plain data: it loads from and saves to RON, and a file states only what it
//! changes: it is merged over the dark theme map by map (a file's `variants`
//! add to dark's). Three enum fields are written as strings holding their
//! variant names: `base: "Button"` (a variant's kind), `font: "Pixel"` and
//! `filter: "Nearest"`.
//!
//! ```ignore
//! let theme = Theme::dark().variant(
//!     "gold",
//!     Variant::of(WidgetKind::Button).normal(LookPatch::default().fill(Color::YELLOW)),
//! );
//! ctx.ui_theme().set(theme);
//! // ...
//! if ui.button("Buy").variant("gold").clicked() { /* ... */ }
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::Color;
use crate::core::asset::SpriteHandle;
use crate::core::ecs::Resource;
use crate::core::render::batch::Filter;
use crate::core::text::{FontRole, TextStyle};

mod builtin;
mod file;
mod look;

pub use file::ThemeError;
#[cfg(any(test, debug_assertions))]
pub(crate) use file::ThemeWatch;
pub(crate) use look::warn_once;
pub use look::{Look, LookPatch, Looks, Variant, WidgetKind, WidgetLooks, WidgetState};

/// An enum a theme file writes as a string holding its variant name
pub(crate) trait Named: Sized + Copy + 'static {
  /// The type's name, for errors
  const TYPE: &'static str;
  /// Every variant with its name
  const ALL: &'static [(&'static str, Self)];
}

impl Named for WidgetKind {
  const TYPE: &'static str = "WidgetKind";
  const ALL: &'static [(&'static str, Self)] = &[
    ("Button", WidgetKind::Button),
    ("Panel", WidgetKind::Panel),
    ("Checkbox", WidgetKind::Checkbox),
    ("Toggle", WidgetKind::Toggle),
    ("Radio", WidgetKind::Radio),
    ("Slider", WidgetKind::Slider),
    ("TextInput", WidgetKind::TextInput),
    ("Dropdown", WidgetKind::Dropdown),
    ("Tab", WidgetKind::Tab),
    ("Choice", WidgetKind::Choice),
    ("Tooltip", WidgetKind::Tooltip),
    ("Scroll", WidgetKind::Scroll),
    ("Slot", WidgetKind::Slot),
    ("Progress", WidgetKind::Progress),
    ("Rebind", WidgetKind::Rebind),
    ("Modal", WidgetKind::Modal),
    ("Prompt", WidgetKind::Prompt),
  ];
}

impl Named for ThemeFont {
  const TYPE: &'static str = "ThemeFont";
  const ALL: &'static [(&'static str, Self)] = &[
    ("Ui", ThemeFont::Ui),
    ("Mono", ThemeFont::Mono),
    ("Pixel", ThemeFont::Pixel),
  ];
}

impl Named for Filter {
  const TYPE: &'static str = "Filter";
  const ALL: &'static [(&'static str, Self)] = &[
    ("Default", Filter::Default),
    ("Nearest", Filter::Nearest),
    ("Linear", Filter::Linear),
  ];
}

/// Field-level serde (`#[serde(with = "names")]`) that writes a [`Named`]
/// enum as its variant name in a string, so a file can be merged as plain
/// data (RON loses the names of bare unit variants)
pub(crate) mod names {
  use serde::de::Error;
  use serde::{Deserialize, Deserializer, Serializer};

  use super::Named;

  pub(crate) fn serialize<T: Named + PartialEq, S: Serializer>(
    value: &T,
    serializer: S,
  ) -> Result<S::Ok, S::Error> {
    match T::ALL.iter().find(|(_, v)| v == value) {
      Some((name, _)) => serializer.serialize_str(name),
      None => Err(serde::ser::Error::custom("an enum with no name")),
    }
  }

  pub(crate) fn deserialize<'de, T: Named, D: Deserializer<'de>>(
    deserializer: D,
  ) -> Result<T, D::Error> {
    let name = String::deserialize(deserializer)?;
    match T::ALL.iter().find(|(n, _)| *n == name) {
      Some((_, value)) => Ok(*value),
      None => {
        let all: Vec<&str> = T::ALL.iter().map(|(n, _)| *n).collect();
        Err(D::Error::custom(format!(
          "unknown {} name {name:?} (one of {})",
          T::TYPE,
          all.join(", ")
        )))
      }
    }
  }
}

/// A complete UI theme
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Theme {
  pub name: String,
  pub palette: Palette,
  pub metrics: Metrics,
  pub text: TextRoles,
  pub widgets: WidgetLooks,
  /// Named changes to a widget kind's looks, picked with `.variant(name)`
  pub variants: BTreeMap<String, Variant>,
  /// Named nine-slice sprites, drawn by looks that name them
  pub skins: BTreeMap<String, Skin>,
  pub sounds: UiSounds,
  /// The file it was loaded from, watched for changes in debug builds
  #[serde(skip)]
  source: Option<PathBuf>,
}

impl Default for Theme {
  fn default() -> Theme {
    Theme::dark()
  }
}

impl Theme {
  /// The dark theme (the default)
  pub fn dark() -> Theme {
    builtin::dark()
  }

  /// The light theme
  pub fn light() -> Theme {
    builtin::light()
  }

  /// A pixel-art theme: the pixel font, square corners and skinned buttons
  /// and panels
  pub fn pixel() -> Theme {
    builtin::pixel()
  }

  /// This theme with the variant `name`
  pub fn variant(mut self, name: &str, variant: Variant) -> Theme {
    self.variants.insert(name.to_string(), variant);
    self
  }

  /// This theme with the skin `name`
  pub fn skin(mut self, name: &str, skin: Skin) -> Theme {
    self.skins.insert(name.to_string(), skin);
    self
  }

  /// The file this theme was loaded from
  pub fn source(&self) -> Option<&Path> {
    self.source.as_deref()
  }
}

/// The colours the looks and text roles are made from
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Palette {
  pub background: Color,
  pub surface: Color,
  pub text: Color,
  /// Secondary text
  pub muted: Color,
  pub accent: Color,
  pub danger: Color,
  pub success: Color,
  pub warning: Color,
}

impl Default for Palette {
  fn default() -> Palette {
    builtin::dark_palette()
  }
}

/// Sizes and timings shared by the widgets
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Metrics {
  /// The corner radius of the built-in looks
  pub radius: f32,
  /// Room between siblings in the built-in layouts
  pub spacing: f32,
  pub border: f32,
  pub focus_ring: FocusRing,
  /// UI units one wheel line scrolls
  pub scroll_line: f32,
  /// The time constant of a scroll area catching up with its target (seconds)
  pub scroll_smoothing: f32,
  /// A held direction first repeats after this long (seconds)
  pub repeat_delay: f32,
  /// Then it repeats this often (seconds)
  pub repeat_rate: f32,
  /// How long the pointer rests on a widget before its tooltip shows (seconds)
  pub tooltip_delay: f32,
  /// UI units a press must move to become a drag
  pub drag_threshold: f32,
  /// Two clicks this close (seconds) make a double click
  pub double_click: f32,
  pub sizes: Sizes,
}

impl Default for Metrics {
  fn default() -> Metrics {
    builtin::metrics()
  }
}

/// The ring around the focused widget
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FocusRing {
  pub width: f32,
  /// The room between the widget's edges and the ring
  pub offset: f32,
  pub color: Color,
}

impl Default for FocusRing {
  fn default() -> FocusRing {
    builtin::metrics().focus_ring
  }
}

/// The sizes of the widgets, in UI units
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Sizes {
  /// Room between a button's edges and its label, across and down (used
  /// when its look has no padding of its own)
  pub button_padding: [f32; 2],
  pub button_min_height: f32,
  /// A panel's padding (used when its look has none of its own)
  pub panel_padding: f32,
  /// A slider's width and track height
  pub slider: [f32; 2],
  /// A slider handle's width and height
  pub slider_handle: [f32; 2],
  /// A toggle's width and height
  pub toggle: [f32; 2],
  pub checkbox: f32,
  /// A scrollbar's width
  pub scrollbar: f32,
  /// An inventory slot's side
  pub slot: f32,
  /// A text field's width and height
  pub text_input: [f32; 2],
}

impl Default for Sizes {
  fn default() -> Sizes {
    builtin::metrics().sizes
  }
}

/// The text styles widgets draw their labels in
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextRoles {
  pub body: TextLook,
  pub heading: TextLook,
  pub button: TextLook,
  pub caption: TextLook,
  pub mono: TextLook,
}

impl Default for TextRoles {
  fn default() -> TextRoles {
    builtin::text_roles(&builtin::dark_palette())
  }
}

/// A text role: what a theme file can say about a text style
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextLook {
  pub size: f32,
  pub color: Color,
  /// Written as a string in files: `"Pixel"`
  #[serde(with = "names")]
  pub font: ThemeFont,
  pub bold: bool,
}

impl Default for TextLook {
  fn default() -> TextLook {
    TextLook {
      size: 14.0,
      color: Color::WHITE,
      font: ThemeFont::Ui,
      bold: false,
    }
  }
}

impl TextLook {
  /// The text style of this role
  pub fn style(&self) -> TextStyle {
    let style = TextStyle::new()
      .role(self.font.role())
      .size(self.size)
      .color(self.color);
    if self.bold { style.bold() } else { style }
  }
}

/// The built-in fonts a theme can name
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ThemeFont {
  /// The UI font
  #[default]
  Ui,
  /// The monospace font
  Mono,
  /// The pixel font
  Pixel,
}

impl ThemeFont {
  fn role(self) -> FontRole {
    match self {
      ThemeFont::Ui => FontRole::Ui,
      ThemeFont::Mono => FontRole::Mono,
      ThemeFont::Pixel => FontRole::Pixel,
    }
  }
}

/// A nine-slice sprite: a look that names it draws it in place of its fill
/// and border, the fill tinting the sprite
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Skin {
  /// The sprite's file, loaded by the engine
  pub path: Option<String>,
  /// How far in the corners reach, in sprite pixels: left, top, right, bottom
  pub insets: [f32; 4],
  /// Written as a string in files: `"Nearest"`
  #[serde(with = "names")]
  pub filter: Filter,
  /// The loaded sprite
  #[serde(skip)]
  pub sprite: Option<SpriteHandle>,
}

impl Default for Skin {
  fn default() -> Skin {
    Skin {
      path: None,
      insets: [0.0; 4],
      filter: Filter::Default,
      sprite: None,
    }
  }
}

impl PartialEq for Skin {
  fn eq(&self, other: &Skin) -> bool {
    self.path == other.path
      && self.insets == other.insets
      && self.filter == other.filter
      && self.sprite.as_ref().map(SpriteHandle::serial)
        == other.sprite.as_ref().map(SpriteHandle::serial)
  }
}

impl Skin {
  /// A skin of `sprite`, whose corners reach `insets` (left, top, right,
  /// bottom) in sprite pixels
  pub fn from_sprite(sprite: SpriteHandle, insets: [f32; 4], filter: Filter) -> Skin {
    Skin {
      path: None,
      insets,
      filter,
      sprite: Some(sprite),
    }
  }

  /// A skin of the sprite file `path`, loaded by the engine
  pub fn from_path(path: &str, insets: [f32; 4], filter: Filter) -> Skin {
    Skin {
      path: Some(path.to_string()),
      insets,
      filter,
      sprite: None,
    }
  }
}

/// The sound files the UI plays (named as the audio manager loads them)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiSounds {
  pub focus: Option<String>,
  pub confirm: Option<String>,
  pub back: Option<String>,
  pub tick: Option<String>,
  pub error: Option<String>,
  pub volume: f32,
}

impl Default for UiSounds {
  fn default() -> UiSounds {
    UiSounds {
      focus: None,
      confirm: None,
      back: None,
      tick: None,
      error: None,
      volume: 1.0,
    }
  }
}

/// Load the skin sprites of `theme` through `load` when its generation is not
/// the one in `loaded` (`None` before the first pass, so a starting theme's
/// skins load on the first frame), and remember it
pub(crate) fn refresh_skins(
  theme: &mut UiTheme,
  loaded: &mut Option<u64>,
  load: impl FnMut(&str) -> Option<SpriteHandle>,
) {
  if *loaded != Some(theme.generation()) {
    *loaded = Some(theme.generation());
    theme.load_skins(load);
  }
}

/// The world resource holding the UI's theme (the dark theme by default)
#[derive(Resource, Debug, Clone)]
pub struct UiTheme {
  theme: Arc<Theme>,
  /// Counts the changes; the engine loads skin sprites when it moves
  generation: u64,
}

impl Default for UiTheme {
  fn default() -> UiTheme {
    UiTheme::new(Theme::dark())
  }
}

impl UiTheme {
  /// A resource holding `theme`
  pub fn new(theme: Theme) -> UiTheme {
    UiTheme {
      theme: Arc::new(theme),
      generation: 0,
    }
  }

  /// The theme in use
  pub fn get(&self) -> &Theme {
    &self.theme
  }

  /// Use `theme` from the next frame
  pub fn set(&mut self, theme: Theme) {
    self.theme = Arc::new(theme);
    self.generation += 1;
  }

  /// Change the theme in use, from the next frame
  pub fn modify(&mut self, change: impl FnOnce(&mut Theme)) {
    change(Arc::make_mut(&mut self.theme));
    self.generation += 1;
  }

  /// The theme, shared with the UI pass
  pub(crate) fn shared(&self) -> Arc<Theme> {
    Arc::clone(&self.theme)
  }

  /// Moves whenever the theme is replaced or changed
  pub(crate) fn generation(&self) -> u64 {
    self.generation
  }

  /// Load the sprite of every skin that has a `path` and no sprite yet,
  /// through `load`, without counting as a change
  pub(crate) fn load_skins(&mut self, mut load: impl FnMut(&str) -> Option<SpriteHandle>) {
    let missing = self
      .theme
      .skins
      .values()
      .any(|skin| skin.sprite.is_none() && skin.path.is_some());
    if !missing {
      return;
    }
    let theme = Arc::make_mut(&mut self.theme);
    for skin in theme.skins.values_mut() {
      if skin.sprite.is_none()
        && let Some(path) = &skin.path
      {
        skin.sprite = load(path);
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_built_in_themes_are_named() {
    let names = [Theme::dark(), Theme::light(), Theme::pixel()].map(|t| t.name);
    assert_eq!(names, ["dark", "light", "pixel"]);
    assert_eq!(Theme::default(), Theme::dark());
  }

  #[test]
  fn the_pixel_theme_skins_buttons_and_panels_with_nearest_sprites() {
    let theme = Theme::pixel();
    for (name, kind) in [("button", WidgetKind::Button), ("panel", WidgetKind::Panel)] {
      let skin = &theme.skins[name];
      assert!(skin.sprite.is_some(), "{name} has a sprite made in code");
      assert_eq!((skin.insets, skin.filter), ([4.0; 4], Filter::Nearest));
      let look = theme.look(kind, None, WidgetState::Normal);
      assert_eq!(look.skin.as_deref(), Some(name));
    }
    assert_eq!(theme.text.button.font, ThemeFont::Pixel);
    assert_eq!(theme.text.button.size, 16.0);
  }

  #[test]
  fn changing_the_theme_moves_its_generation() {
    let mut theme = UiTheme::default();
    let start = theme.generation();
    theme.set(Theme::light());
    assert_eq!(theme.get().name, "light");
    theme.modify(|t| t.name = "mine".to_string());
    assert_eq!(theme.get().name, "mine");
    assert_eq!(theme.generation(), start + 2);
  }

  #[test]
  fn skins_with_a_path_load_once_through_the_loader() {
    let skin = Skin::from_path("ui/frame.png", [2.0; 4], Filter::Linear);
    let mut theme = UiTheme::new(Theme::dark().skin("frame", skin));
    let mut asked = Vec::new();
    for _ in 0..2 {
      theme.load_skins(|path| {
        asked.push(path.to_string());
        let sprite = crate::core::graphics::Sprite::solid_rect(4, 4, 255, 255, 255, 255);
        Some(SpriteHandle::from_sprite(sprite, path))
      });
    }
    assert_eq!(asked, ["ui/frame.png"]);
    assert!(theme.get().skins["frame"].sprite.is_some());
  }

  #[test]
  fn a_starting_theme_with_path_skins_loads_them_on_the_first_frame() {
    let skin = Skin::from_path("ui/start.png", [2.0; 4], Filter::Linear);
    let mut theme = UiTheme::new(Theme::dark().skin("start", skin));
    let mut loaded = None;
    let mut asked = 0;
    let mut load = |path: &str| {
      asked += 1;
      let sprite = crate::core::graphics::Sprite::solid_rect(4, 4, 255, 255, 255, 255);
      Some(SpriteHandle::from_sprite(sprite, path))
    };
    refresh_skins(&mut theme, &mut loaded, &mut load);
    refresh_skins(&mut theme, &mut loaded, &mut load);
    assert_eq!(asked, 1, "loaded on the first pass, not again");
    assert!(theme.get().skins["start"].sprite.is_some());
  }
}
