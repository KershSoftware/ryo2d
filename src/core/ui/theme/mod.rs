//! The UI theme: a palette, metrics, text roles, a look for every widget
//! kind and state, named variants, nine-slice skins and sounds.
//!
//! The theme is the [`UiTheme`] world resource (`ctx.ui_theme()`); the UI
//! reads it once a pass, so a change applies from the next frame. A theme is
//! plain data, and an asset: `.theme.ron` files load through the registry
//! (`ThemeLoader`) and [`Theme::save`] writes one. A file states only what it
//! changes: it is merged over the dark theme map by map (a file's `variants`
//! add to dark's). Three enum fields are written as strings holding their
//! variant names: `base: "Button"` (a variant's kind), `font: "Pixel"` and
//! `filter: "Nearest"`. The UI theme follows a theme asset through its
//! reloads (`ctx.follow_ui_theme`).
//!
//! ```ignore
//! let theme = Theme::dark().variant(
//!     "gold",
//!     Variant::of(WidgetKind::Button).normal(LookPatch::default().fill(Color::YELLOW)),
//! );
//! ctx.ui_theme().set(theme);
//! // ...
//! if ui.button("Buy").variant("gold").clicked() { /* ... */ }
//!
//! // Or a file, used at once and again whenever it is saved
//! let file = ctx.assets().load_now::<Theme>("ui/game.theme.ron")?;
//! ctx.follow_ui_theme(file);
//! ```

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use super::Color;
use super::memory::UiSound;
use crate::core::asset::{Asset, Assets, AssetsView, Handle};
use crate::core::ecs::Resource;
use crate::core::ecs::kernel::{Mut, World};
use crate::core::graphics::Image;
use crate::core::render::batch::Filter;
use crate::core::text::{FontRole, TextStyle};

mod builtin;
mod file;
mod look;

pub use file::ThemeError;
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
  /// Named nine-slice images, drawn by looks that name them
  pub skins: BTreeMap<String, Skin>,
  pub sounds: UiSounds,
}

impl Default for Theme {
  fn default() -> Theme {
    Theme::dark()
  }
}

impl Asset for Theme {}

impl Theme {
  /// Whether a skin has a path (or is a built-in theme's) and no image yet
  pub(crate) fn missing_skins(&self) -> bool {
    self.skins.values().any(Skin::wants_image)
  }

  /// Load the image of every skin that has a path (or is a built-in
  /// theme's) and none yet, through `load`
  pub(crate) fn load_skins(
    &mut self,
    mut load: impl FnMut(SkinSource<'_>) -> Option<Handle<Image>>,
  ) {
    for skin in self.skins.values_mut() {
      if let Some(source) = skin.source() {
        skin.image = load(source);
      }
    }
  }

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

/// A nine-slice image: a look that names it draws it in place of its fill
/// and border, the fill tinting the image
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Skin {
  /// The image's file, an asset path: loaded with the theme for a theme
  /// file, and by the engine when a theme made in code is set
  pub path: Option<String>,
  /// How far in the corners reach, in image pixels: left, top, right, bottom
  pub insets: [f32; 4],
  /// Written as a string in files: `"Nearest"`
  #[serde(with = "names")]
  pub filter: Filter,
  /// The loaded image
  #[serde(skip)]
  pub image: Option<Handle<Image>>,
  /// A built-in theme's image, made in code when the skin is loaded
  #[serde(skip)]
  pub(crate) builtin: Option<BuiltinSkin>,
}

/// The images the built-in themes' skins are made of, in code
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuiltinSkin {
  PixelButton,
  PixelPanel,
}

/// Where a skin's image comes from, for loading it (`UiTheme::load_skins`)
pub(crate) enum SkinSource<'a> {
  /// A file, by its asset path
  Path(&'a str),
  /// A built-in theme's image, made in code
  Made(Image),
}

impl Default for Skin {
  fn default() -> Skin {
    Skin {
      path: None,
      insets: [0.0; 4],
      filter: Filter::Default,
      image: None,
      builtin: None,
    }
  }
}

impl PartialEq for Skin {
  fn eq(&self, other: &Skin) -> bool {
    self.path == other.path
      && self.insets == other.insets
      && self.filter == other.filter
      && self.image == other.image
      && self.builtin == other.builtin
  }
}

impl Skin {
  /// A skin of `image`, whose corners reach `insets` (left, top, right,
  /// bottom) in image pixels
  pub fn from_image(image: Handle<Image>, insets: [f32; 4], filter: Filter) -> Skin {
    Skin {
      path: None,
      insets,
      filter,
      image: Some(image),
      builtin: None,
    }
  }

  /// A built-in theme's skin of `image`, made when the skin is loaded
  pub(crate) fn builtin(image: BuiltinSkin, insets: [f32; 4], filter: Filter) -> Skin {
    Skin {
      path: None,
      insets,
      filter,
      image: None,
      builtin: Some(image),
    }
  }

  /// Where this skin's image comes from while it has none
  fn source(&self) -> Option<SkinSource<'_>> {
    if self.image.is_some() {
      return None;
    }
    match (self.builtin, &self.path) {
      (Some(image), _) => Some(SkinSource::Made(builtin::skin_image(image))),
      (None, Some(path)) => Some(SkinSource::Path(path)),
      (None, None) => None,
    }
  }

  /// Whether this skin has a path (or is a built-in theme's) and no image
  fn wants_image(&self) -> bool {
    self.image.is_none() && (self.builtin.is_some() || self.path.is_some())
  }

  /// A skin of the image file `path` (an asset path), loaded by the engine
  pub fn from_path(path: &str, insets: [f32; 4], filter: Filter) -> Skin {
    Skin {
      path: Some(path.to_string()),
      insets,
      filter,
      image: None,
      builtin: None,
    }
  }
}

/// The sound files the UI plays, by event: focus moved by navigation, a
/// confirm or click, back, a slider step (at most every 0.05 s) and an error
/// (a refused drop, a cancelled rebind). The engine loads them when the
/// theme changes and plays at most one of each kind per frame at `volume`.
/// The default names none, so a game is silent until it sets some; a file
/// that will not load (or no audio device) leaves that sound silent.
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

impl UiSounds {
  /// The file named for `sound`
  pub(crate) fn path(&self, sound: UiSound) -> Option<&str> {
    match sound {
      UiSound::Focus => &self.focus,
      UiSound::Confirm => &self.confirm,
      UiSound::Back => &self.back,
      UiSound::Tick => &self.tick,
      UiSound::Error => &self.error,
    }
    .as_deref()
  }
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

/// Load the skin images of `theme` through `load` when its generation is not
/// the one in `loaded` (`None` before the first pass, so a starting theme's
/// skins load on the first frame), and remember it
pub(crate) fn refresh_skins(
  theme: &mut UiTheme,
  loaded: &mut Option<u64>,
  load: impl FnMut(SkinSource<'_>) -> Option<Handle<Image>>,
) {
  if *loaded != Some(theme.generation()) {
    theme.load_skins(load);
    *loaded = Some(theme.generation());
  }
}

/// A skin's image through the asset registry: a file loaded at once, or a
/// built-in theme's image added. A file that will not load is warned about
/// and leaves the skin bare
pub(crate) fn load_skin(
  assets: &mut AssetsView<'_>,
  source: SkinSource<'_>,
) -> Option<Handle<Image>> {
  match source {
    SkinSource::Path(path) => match assets.load_now::<Image>(path) {
      Ok(image) => Some(image),
      Err(error) => {
        crate::engine_warn!(
          module: "ui",
          "could not load the UI skin {path:?}: {}",
          error.report()
        );
        None
      }
    },
    SkinSource::Made(image) => Some(assets.add(image)),
  }
}

/// Follow `theme` with `world`'s UI theme and use it at once if it is
/// loaded (one still loading is used when it lands), so a `get` or `modify`
/// straight after sees it (`SceneContext::follow_ui_theme`)
pub(crate) fn follow_now(world: &mut World, theme: Handle<Theme>) {
  if !world.contains_resource::<UiTheme>() {
    world.insert_resource(UiTheme::default());
  }
  world.try_resource_scope(|world, mut ui: Mut<UiTheme>| {
    ui.follow(theme);
    use_followed(&mut ui, world);
  });
}

/// Use the followed theme asset if it changed since `ui` last used it: the
/// start of each UI pass (`refresh_ui_theme`), and `follow_now`
pub(crate) fn use_followed(ui: &mut UiTheme, world: &World) {
  if let Some(themes) = world.get_resource::<Assets<Theme>>() {
    ui.update_followed(themes);
  }
}

/// The world resource holding the UI's theme (the dark theme by default):
/// one set in code, or a theme asset it follows (`ctx.follow_ui_theme`,
/// [`UiTheme::follow`])
#[derive(Resource, Debug, Clone)]
pub struct UiTheme {
  theme: Arc<Theme>,
  /// Names the theme's state: taken fresh from [`next_generation`] by every
  /// change, so a theme put in place of the resource never shares one with
  /// the theme it replaced (the engine loads skins, and the node cache
  /// rebuilds, when it moves)
  generation: u64,
  /// The theme asset it follows
  followed: Option<Followed>,
}

/// A theme asset the UI theme follows, held so it stays loaded
#[derive(Debug, Clone)]
struct Followed {
  theme: Handle<Theme>,
  /// The asset's version in use; `None` until its value is first used
  used: Option<u64>,
}

/// A generation no theme state has had in this process
fn next_generation() -> u64 {
  static NEXT: AtomicU64 = AtomicU64::new(1);
  NEXT.fetch_add(1, Ordering::Relaxed)
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
      generation: next_generation(),
      followed: None,
    }
  }

  /// The theme in use
  pub fn get(&self) -> &Theme {
    &self.theme
  }

  /// Use `theme` from the next frame, no longer following a theme asset
  pub fn set(&mut self, theme: Theme) {
    self.followed = None;
    self.theme = Arc::new(theme);
    self.generation = next_generation();
  }

  /// Follow the theme asset `theme`: its value is used once it is loaded,
  /// and again whenever it changes (its file saved, an image of its skins
  /// reloaded, or a change through `Assets<Theme>::get_mut`), each time as
  /// [`UiTheme::set`] would. A reload that fails keeps the theme in use, and
  /// the registry warns once per error. The resource holds the handle while
  /// it follows, so the asset stays loaded; `set` stops following, and
  /// `follow` with another handle follows that one instead.
  ///
  /// The resource cannot reach the theme store, so alone this uses the
  /// value at the next UI pass: until then `get` returns the old theme, and
  /// a `modify` made before then is overwritten. `ctx.follow_ui_theme(theme)`
  /// follows and uses a loaded theme at once, so use that from a scene:
  ///
  /// ```ignore
  /// if let Ok(theme) = ctx.assets().load_now::<Theme>("ui/game.theme.ron") {
  ///   ctx.follow_ui_theme(theme);
  /// }
  /// ```
  pub fn follow(&mut self, theme: Handle<Theme>) {
    self.followed = Some(Followed { theme, used: None });
  }

  /// Use the followed theme asset's value if it changed since it was last
  /// used: a load landing, a reload and a change in code each move its
  /// version on. One not in the store (still loading, or failed) leaves the
  /// theme in use. Called once a UI pass; with nothing changed it is one
  /// lookup
  pub(crate) fn update_followed(&mut self, themes: &Assets<Theme>) {
    let Some(followed) = &mut self.followed else {
      return;
    };
    let Some(version) = themes.version(&followed.theme) else {
      return;
    };
    if followed.used == Some(version) {
      return;
    }
    let Some(value) = themes.get(&followed.theme) else {
      return;
    };
    followed.used = Some(version);
    self.theme = Arc::new(value.clone());
    self.generation = next_generation();
  }

  /// Change the theme in use, from the next frame. A followed theme asset's
  /// next change replaces it, and so does its first use after
  /// [`UiTheme::follow`] alone (at the next UI pass); after
  /// `ctx.follow_ui_theme`, which uses a loaded theme at once, the change
  /// lasts until the asset next changes
  pub fn modify(&mut self, change: impl FnOnce(&mut Theme)) {
    change(Arc::make_mut(&mut self.theme));
    self.generation = next_generation();
  }

  /// The theme, shared with the UI pass
  pub(crate) fn shared(&self) -> Arc<Theme> {
    Arc::clone(&self.theme)
  }

  /// Moves whenever the theme is replaced or changed
  pub(crate) fn generation(&self) -> u64 {
    self.generation
  }

  /// Load the image of every skin that has a `path` (or is a built-in
  /// theme's) and no image yet, through `load`. Loaded skins change what is
  /// drawn, so this moves the generation.
  pub(crate) fn load_skins(&mut self, load: impl FnMut(SkinSource<'_>) -> Option<Handle<Image>>) {
    if self.theme.missing_skins() {
      Arc::make_mut(&mut self.theme).load_skins(load);
    }
    self.generation = next_generation();
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
  fn the_pixel_theme_skins_buttons_and_panels_with_nearest_images() {
    let theme = Theme::pixel();
    for (name, kind) in [("button", WidgetKind::Button), ("panel", WidgetKind::Panel)] {
      let skin = &theme.skins[name];
      assert!(
        skin.builtin.is_some() && skin.image.is_none(),
        "{name}'s image is made in code when the skin is loaded"
      );
      assert_eq!((skin.insets, skin.filter), ([4.0; 4], Filter::Nearest));
      let look = theme.look(kind, None, WidgetState::Normal);
      assert_eq!(look.skin.as_deref(), Some(name));
    }
    assert_eq!(theme.text.button.font, ThemeFont::Pixel);
    assert_eq!(theme.text.button.size, 16.0);
    // Loaded, each skin gets its image, 12 px square
    let mut images = crate::core::asset::Assets::new();
    let mut theme = theme;
    assert!(theme.missing_skins());
    theme.load_skins(|source| match source {
      SkinSource::Made(image) => Some(images.add(image)),
      SkinSource::Path(_) => None,
    });
    assert!(!theme.missing_skins());
    for name in ["button", "panel"] {
      let image = theme.skins[name].image.as_ref().and_then(|s| images.get(s));
      assert_eq!(image.map(|i| (i.width(), i.height())), Some((12, 12)));
    }
  }

  #[test]
  fn changing_the_theme_moves_its_generation() {
    let mut theme = UiTheme::default();
    let start = theme.generation();
    theme.set(Theme::light());
    assert_eq!(theme.get().name, "light");
    let set = theme.generation();
    theme.modify(|t| t.name = "mine".to_string());
    assert_eq!(theme.get().name, "mine");
    let modified = theme.generation();
    assert!(start != set && set != modified && start != modified);
  }

  #[test]
  fn no_two_theme_states_share_a_generation() {
    // A theme a game puts in place of the resource must never look like
    // the one it replaced, whichever has moved further
    let mut first = UiTheme::new(Theme::dark());
    first.load_skins(|_| None);
    let second = UiTheme::new(Theme::dark());
    let third = UiTheme::default();
    let generations = [first.generation(), second.generation(), third.generation()];
    assert_ne!(generations[0], generations[1]);
    assert_ne!(generations[1], generations[2]);
    assert_ne!(generations[0], generations[2]);
  }

  #[test]
  fn skins_with_a_path_load_once_through_the_loader() {
    let skin = Skin::from_path("ui/frame.png", [2.0; 4], Filter::Linear);
    let mut theme = UiTheme::new(Theme::dark().skin("frame", skin));
    let mut asked = Vec::new();
    let mut images = crate::core::asset::Assets::new();
    for _ in 0..2 {
      theme.load_skins(|source| {
        if let SkinSource::Path(path) = source {
          asked.push(path.to_string());
        }
        Some(images.add(Image::solid_rect(4, 4, 255, 255, 255, 255)))
      });
    }
    assert_eq!(asked, ["ui/frame.png"]);
    assert!(theme.get().skins["frame"].image.is_some());
  }

  #[test]
  fn a_followed_theme_is_used_once_for_each_change() {
    let mut themes = crate::core::asset::Assets::new();
    let file = themes.add(Theme::light());
    let mut theme = UiTheme::default();
    theme.follow(file.clone());
    theme.update_followed(&themes);
    assert_eq!(theme.get().name, "light");
    let used = theme.generation();
    theme.update_followed(&themes);
    assert_eq!(theme.generation(), used, "nothing changed");
    if let Some(value) = themes.get_mut(&file) {
      value.name = "changed".to_string();
    }
    theme.update_followed(&themes);
    assert_eq!(theme.get().name, "changed");
    assert_ne!(theme.generation(), used);
  }

  #[test]
  fn a_followed_theme_still_loading_leaves_the_theme_in_use() {
    let mut themes = crate::core::asset::Assets::<Theme>::new();
    // A slot reserved for a load that has not landed
    let loading = themes
      .ledger()
      .lock()
      .unwrap_or_else(std::sync::PoisonError::into_inner)
      .reserve::<Theme>();
    let mut theme = UiTheme::default();
    let start = theme.generation();
    theme.follow(loading.clone());
    theme.update_followed(&themes);
    assert_eq!(
      (theme.get().name.as_str(), theme.generation()),
      ("dark", start)
    );
    themes.insert(loading.id(), Theme::light());
    theme.update_followed(&themes);
    assert_eq!(theme.get().name, "light", "used once it lands");
  }

  #[test]
  fn setting_a_theme_stops_following() {
    let mut themes = crate::core::asset::Assets::new();
    let file = themes.add(Theme::light());
    let mut theme = UiTheme::default();
    theme.follow(file.clone());
    theme.update_followed(&themes);
    theme.set(Theme::pixel());
    if let Some(value) = themes.get_mut(&file) {
      value.name = "changed".to_string();
    }
    theme.update_followed(&themes);
    assert_eq!(theme.get().name, "pixel");
  }

  #[test]
  fn following_another_theme_replaces_the_first() {
    let mut themes = crate::core::asset::Assets::new();
    let first = themes.add(Theme::light());
    let second = themes.add(Theme::pixel());
    let mut theme = UiTheme::default();
    theme.follow(first.clone());
    theme.update_followed(&themes);
    theme.follow(second);
    theme.update_followed(&themes);
    assert_eq!(theme.get().name, "pixel");
    if let Some(value) = themes.get_mut(&first) {
      value.name = "changed".to_string();
    }
    theme.update_followed(&themes);
    assert_eq!(theme.get().name, "pixel", "the first is no longer followed");
  }

  #[test]
  fn a_starting_theme_with_path_skins_loads_them_on_the_first_frame() {
    let skin = Skin::from_path("ui/start.png", [2.0; 4], Filter::Linear);
    let mut theme = UiTheme::new(Theme::dark().skin("start", skin));
    let mut loaded = None;
    let mut asked = 0;
    let mut images = crate::core::asset::Assets::new();
    let mut load = |_: SkinSource<'_>| {
      asked += 1;
      Some(images.add(Image::solid_rect(4, 4, 255, 255, 255, 255)))
    };
    refresh_skins(&mut theme, &mut loaded, &mut load);
    refresh_skins(&mut theme, &mut loaded, &mut load);
    assert_eq!(asked, 1, "loaded on the first pass, not again");
    assert!(theme.get().skins["start"].image.is_some());
  }

  /// Followed through a registry, as a scene and the engine's UI passes do,
  /// without a GPU
  mod registry {
    use std::time::{Duration, Instant};

    use super::super::{Theme, UiTheme, follow_now, use_followed};
    use crate::core::asset::{
      AssetEvent, AssetServer, Assets, AssetsView, Handle, MemorySource, insert_registry, pump,
    };
    use crate::core::ecs::kernel::{Messages, Mut, World};
    use crate::core::ui::Color;

    /// A registry over `files` and the UI theme, as a scene's world has them
    fn scene_world(files: &MemorySource) -> World {
      let mut world = World::new();
      insert_registry(&mut world, AssetServer::new(files.clone()));
      world.insert_resource(UiTheme::default());
      world
    }

    /// A frame up to its UI pass: the pump, then what the pass does first
    /// with the followed theme (`refresh_ui_theme`)
    fn ui_pass(world: &mut World) {
      pump(world);
      world.resource_scope(|world, mut ui: Mut<UiTheme>| use_followed(&mut ui, world));
    }

    fn load(world: &mut World, path: &str) -> Handle<Theme> {
      match AssetsView::new(world).load_now::<Theme>(path) {
        Ok(theme) => theme,
        Err(error) => panic!("{path}: {}", error.report()),
      }
    }

    /// The name of the theme in use, and its generation
    fn in_use(world: &World) -> (String, u64) {
      let ui = world.resource::<UiTheme>();
      (ui.get().name.clone(), ui.generation())
    }

    /// Pumps until `done`, failing after five seconds
    fn pump_until(world: &mut World, mut done: impl FnMut(&World) -> bool) {
      let start = Instant::now();
      loop {
        pump(world);
        if done(world) {
          return;
        }
        assert!(
          start.elapsed() < Duration::from_secs(5),
          "timed out waiting"
        );
        std::thread::sleep(Duration::from_millis(1));
      }
    }

    /// A `side` x `side` PNG
    fn png(side: u32) -> Vec<u8> {
      let image = image::RgbaImage::from_pixel(side, side, image::Rgba([255; 4]));
      let mut bytes = Vec::new();
      if let Err(error) = image.write_to(
        &mut std::io::Cursor::new(&mut bytes),
        image::ImageFormat::Png,
      ) {
        panic!("encoding a png: {error}");
      }
      bytes
    }

    #[test]
    fn get_after_follow_returns_the_followed_theme() {
      let files = MemorySource::new();
      files.insert("ui/now.theme.ron", "(name: \"file\")");
      let mut world = scene_world(&files);
      let theme = load(&mut world, "ui/now.theme.ron");
      follow_now(&mut world, theme);
      assert_eq!(world.resource::<UiTheme>().get().name, "file");
    }

    #[test]
    fn a_modify_right_after_follow_survives_the_next_ui_pass() {
      let files = MemorySource::new();
      files.insert("ui/tweaked.theme.ron", "(name: \"file\")");
      let mut world = scene_world(&files);
      let theme = load(&mut world, "ui/tweaked.theme.ron");
      follow_now(&mut world, theme.clone());
      world
        .resource_mut::<UiTheme>()
        .modify(|t| t.palette.accent = Color::RED);
      ui_pass(&mut world);
      ui_pass(&mut world);
      let accent = |world: &World| world.resource::<UiTheme>().get().palette.accent;
      assert_eq!(in_use(&world).0, "file");
      assert_eq!(accent(&world), Color::RED, "the change lasts");
      // Until the followed theme next changes
      if let Some(value) = world.resource_mut::<Assets<Theme>>().get_mut(&theme) {
        value.name = "changed".to_string();
      }
      ui_pass(&mut world);
      assert_eq!(in_use(&world).0, "changed");
      assert_eq!(accent(&world), Theme::dark().palette.accent);
    }

    #[test]
    fn a_theme_followed_while_loading_is_used_when_it_lands() {
      let files = MemorySource::new();
      files.insert("ui/later.theme.ron", "(name: \"later\")");
      let mut world = scene_world(&files);
      let server = world.resource::<AssetServer>().clone();
      let theme = server.load::<Theme>("ui/later.theme.ron");
      follow_now(&mut world, theme.clone());
      assert_eq!(in_use(&world).0, "dark", "not loaded yet");
      pump_until(&mut world, |_| server.is_ready(&theme));
      ui_pass(&mut world);
      assert_eq!(in_use(&world).0, "later");
    }

    #[test]
    fn a_skin_image_reload_uses_the_followed_theme_again_once() {
      let files = MemorySource::new();
      files.insert(
        "ui/skinned.theme.ron",
        r#"(skins: {"frame": (path: "ui/frame.png", insets: (1.0, 1.0, 1.0, 1.0))})"#,
      );
      files.insert("ui/frame.png", png(2));
      let mut world = scene_world(&files);
      let theme = load(&mut world, "ui/skinned.theme.ron");
      follow_now(&mut world, theme.clone());
      ui_pass(&mut world);
      let before = in_use(&world).1;
      let version = |world: &World| world.resource::<Assets<Theme>>().version(&theme);
      let touched = version(&world);
      files.insert("ui/frame.png", png(4));
      world.resource::<AssetServer>().reload("ui/frame.png");
      pump_until(&mut world, |world| version(world) != touched);
      ui_pass(&mut world);
      let after = in_use(&world).1;
      assert_ne!(after, before, "used again for its new image");
      ui_pass(&mut world);
      ui_pass(&mut world);
      assert_eq!(in_use(&world).1, after, "once");
    }

    #[test]
    fn the_ui_theme_keeps_a_followed_theme_the_game_dropped() {
      let path = "ui/dropped.theme.ron";
      let files = MemorySource::new();
      files.insert(path, "(name: \"first\")");
      let mut world = scene_world(&files);
      let theme = load(&mut world, path);
      follow_now(&mut world, theme);
      // The game's handle is gone; frames free what nothing holds
      ui_pass(&mut world);
      ui_pass(&mut world);
      assert_eq!(world.resource::<Assets<Theme>>().len(), 1, "still held");
      files.insert(path, "(name: \"second\")");
      world.resource::<AssetServer>().reload(path);
      let start = Instant::now();
      while in_use(&world).0 != "second" {
        assert!(start.elapsed() < Duration::from_secs(5), "never reloaded");
        ui_pass(&mut world);
        std::thread::sleep(Duration::from_millis(1));
      }
    }

    #[test]
    fn a_failed_reload_leaves_the_followed_theme_in_use() {
      let path = "ui/fails.theme.ron";
      let files = MemorySource::new();
      files.insert(path, "(name: \"kept\")");
      let mut world = scene_world(&files);
      let theme = load(&mut world, path);
      follow_now(&mut world, theme);
      ui_pass(&mut world);
      let before = in_use(&world);
      let mut events = world
        .resource::<Messages<AssetEvent<Theme>>>()
        .get_cursor_current();
      files.insert(path, "(name: \"broken\", palette: nonsense");
      world.resource::<AssetServer>().reload(path);
      pump_until(&mut world, |world| {
        events
          .read(world.resource::<Messages<AssetEvent<Theme>>>())
          .any(|event| matches!(event, AssetEvent::ReloadFailed { .. }))
      });
      ui_pass(&mut world);
      assert_eq!(
        in_use(&world),
        before,
        "the same theme, at the same generation"
      );
    }
  }
}
