//! The player-preference types: which settings the player's `system.json` may set
//! ([`UserPreference`], [`UserPreferenceFilter`]), and the display types a settings
//! screen offers ([`DisplayMode`], [`Resolution`]).

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// User preference fields that can be loaded from system.json.
///
/// Used with [`UserPreferenceFilter`] to control which user preferences
/// are loaded from `system.json` when using
/// [`with_user_preferences`](crate::core::engine::EngineBuilder::with_user_preferences).
///
/// Preferences not in the filter, or missing from system.json, keep their builder / Ryo.toml /
/// default values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UserPreference {
  /// Master volume (0.0 - 1.0)
  MasterVolume,
  /// Music volume (0.0 - 1.0)
  MusicVolume,
  /// Sound effects volume (0.0 - 1.0)
  SfxVolume,
  /// Interface sounds volume (0.0 - 1.0)
  UiVolume,
  /// Dialogue volume (0.0 - 1.0)
  VoiceVolume,
  /// Ambience volume (0.0 - 1.0)
  AmbienceVolume,
  /// Display resolution (width + height together)
  Resolution,
  /// Display mode (windowed, exclusive_fullscreen, borderless_fullscreen)
  DisplayMode,
  /// VSync preference
  Vsync,
  /// Show FPS counter
  ShowFps,
  /// Shadow quality
  ShadowQuality,
  /// Input action bindings the player changed
  Bindings,
}

impl UserPreference {
  /// Returns all available user preference fields.
  pub fn all() -> &'static [UserPreference] {
    &[
      UserPreference::MasterVolume,
      UserPreference::MusicVolume,
      UserPreference::SfxVolume,
      UserPreference::UiVolume,
      UserPreference::VoiceVolume,
      UserPreference::AmbienceVolume,
      UserPreference::Resolution,
      UserPreference::DisplayMode,
      UserPreference::Vsync,
      UserPreference::ShowFps,
      UserPreference::ShadowQuality,
      UserPreference::Bindings,
    ]
  }
}

/// Filter controlling which user preferences are loaded from `system.json`.
///
/// Used with [`with_user_preferences`](crate::core::engine::EngineBuilder::with_user_preferences)
/// to selectively load player settings from `system.json`.
///
/// # Examples
///
/// ```rust,ignore
/// use ryo2d::prelude::*;
///
/// // Load only the six volume settings from system.json
/// EngineBuilder::new()
///     .title("My Game")
///     .with_user_preferences([
///         UserPreference::MasterVolume,
///         UserPreference::MusicVolume,
///         UserPreference::SfxVolume,
///         UserPreference::UiVolume,
///         UserPreference::VoiceVolume,
///         UserPreference::AmbienceVolume,
///     ])
///     .start_scene(GameScene::new())
///     .build()
///     .run()?;
///
/// // Apply all saved preferences
/// EngineBuilder::new()
///     .with_user_preferences(UserPreferenceFilter::all())
///     .build()
///     .run()?;
///
/// // Apply no saved preferences
/// EngineBuilder::new()
///     .with_user_preferences(UserPreferenceFilter::none())
///     .build()
///     .run()?;
/// ```
#[derive(Debug, Clone)]
pub struct UserPreferenceFilter {
  fields: HashSet<UserPreference>,
}

impl UserPreferenceFilter {
  /// All fields enabled.
  pub fn all() -> Self {
    Self {
      fields: UserPreference::all().iter().copied().collect(),
    }
  }

  /// No fields - no saved preference overrides the game's settings.
  pub fn none() -> Self {
    Self {
      fields: HashSet::new(),
    }
  }

  /// Only specified fields are loaded from system.json.
  pub fn only(fields: impl IntoIterator<Item = UserPreference>) -> Self {
    Self {
      fields: fields.into_iter().collect(),
    }
  }

  /// Check if a field is enabled.
  pub fn contains(&self, field: UserPreference) -> bool {
    self.fields.contains(&field)
  }

  /// Check if filter is empty (no fields enabled).
  pub fn is_empty(&self) -> bool {
    self.fields.is_empty()
  }

  /// Returns the number of enabled fields.
  pub fn len(&self) -> usize {
    self.fields.len()
  }
}

impl Default for UserPreferenceFilter {
  fn default() -> Self {
    Self::all()
  }
}

/// Allow creating a filter from an array of fields.
impl<const N: usize> From<[UserPreference; N]> for UserPreferenceFilter {
  fn from(fields: [UserPreference; N]) -> Self {
    Self::only(fields)
  }
}

/// Allow creating a filter from a Vec of fields.
impl From<Vec<UserPreference>> for UserPreferenceFilter {
  fn from(fields: Vec<UserPreference>) -> Self {
    Self::only(fields)
  }
}

/// Allow creating a filter from a slice of fields.
impl From<&[UserPreference]> for UserPreferenceFilter {
  fn from(fields: &[UserPreference]) -> Self {
    Self::only(fields.iter().copied())
  }
}

/// Display/window mode for the game.
///
/// Controls how the game window appears on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayMode {
  /// Regular resizable window
  #[default]
  Windowed,
  /// Exclusive fullscreen (takes over the display)
  ExclusiveFullscreen,
  /// Borderless fullscreen window (fills screen, easy alt-tab)
  BorderlessFullscreen,
}

impl DisplayMode {
  /// Parses a config-file name (case-insensitive); `None` if unrecognized.
  pub(crate) fn from_config_name(name: &str) -> Option<DisplayMode> {
    match name.to_lowercase().as_str() {
      "windowed" => Some(DisplayMode::Windowed),
      "exclusive_fullscreen" => Some(DisplayMode::ExclusiveFullscreen),
      "borderless_fullscreen" => Some(DisplayMode::BorderlessFullscreen),
      _ => None,
    }
  }

  /// The name a config file gives this mode (the one `from_config_name` reads).
  pub(crate) fn config_name(&self) -> &'static str {
    match self {
      DisplayMode::Windowed => "windowed",
      DisplayMode::ExclusiveFullscreen => "exclusive_fullscreen",
      DisplayMode::BorderlessFullscreen => "borderless_fullscreen",
    }
  }

  /// Returns all available display modes for UI selection
  pub fn all() -> &'static [DisplayMode] {
    &[
      DisplayMode::Windowed,
      DisplayMode::ExclusiveFullscreen,
      DisplayMode::BorderlessFullscreen,
    ]
  }

  /// Human-readable name for UI display
  pub fn display_name(&self) -> &'static str {
    match self {
      DisplayMode::Windowed => "Windowed",
      DisplayMode::ExclusiveFullscreen => "Exclusive Fullscreen",
      DisplayMode::BorderlessFullscreen => "Borderless Fullscreen",
    }
  }
}

/// Common screen resolutions for settings UI
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resolution {
  pub width: u32,
  pub height: u32,
}

/// Static list of common resolutions
static COMMON_RESOLUTIONS: &[Resolution] = &[
  Resolution {
    width: 1280,
    height: 720,
  }, // 720p
  Resolution {
    width: 1366,
    height: 768,
  }, // HD
  Resolution {
    width: 1600,
    height: 900,
  }, // HD+
  Resolution {
    width: 1920,
    height: 1080,
  }, // 1080p
  Resolution {
    width: 2560,
    height: 1440,
  }, // 1440p / 2K
  Resolution {
    width: 3840,
    height: 2160,
  }, // 4K UHD
];

impl Resolution {
  pub const fn new(width: u32, height: u32) -> Self {
    Self { width, height }
  }

  /// Returns common resolutions from 720p to 4K
  pub fn common() -> &'static [Resolution] {
    COMMON_RESOLUTIONS
  }

  /// Format as "WxH" string
  pub fn display_name(&self) -> String {
    format!("{}x{}", self.width, self.height)
  }
}

/// Parse a "WxH" resolution string (e.g., "1280x720"). Returns None if malformed.
pub fn parse_resolution(s: &str) -> Option<(u32, u32)> {
  let parts: Vec<&str> = s.split('x').collect();
  if parts.len() == 2
    && let (Ok(w), Ok(h)) = (parts[0].trim().parse(), parts[1].trim().parse())
  {
    return Some((w, h));
  }
  None
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parse_resolution_valid_strings() {
    assert_eq!(parse_resolution("1280x720"), Some((1280, 720)));
    assert_eq!(parse_resolution("800x600"), Some((800, 600)));
    assert_eq!(parse_resolution("1920x1080"), Some((1920, 1080)));
    assert_eq!(parse_resolution("3840x2160"), Some((3840, 2160)));
    assert_eq!(parse_resolution("320x240"), Some((320, 240)));
  }

  #[test]
  fn parse_resolution_with_whitespace() {
    assert_eq!(parse_resolution(" 800 x 600 "), Some((800, 600)));
    assert_eq!(parse_resolution("1280 x 720"), Some((1280, 720)));
  }

  #[test]
  fn parse_resolution_malformed_returns_none() {
    assert_eq!(parse_resolution(""), None);
    assert_eq!(parse_resolution("1280"), None);
    assert_eq!(parse_resolution("1280x"), None);
    assert_eq!(parse_resolution("x720"), None);
    assert_eq!(parse_resolution("1280x720x60"), None);
    assert_eq!(parse_resolution("widexhigh"), None);
    assert_eq!(parse_resolution("1280*720"), None);
    assert_eq!(parse_resolution("-1x720"), None);
  }

  #[test]
  fn user_preference_filter_all_contains_all_fields() {
    let filter = UserPreferenceFilter::all();

    assert!(filter.contains(UserPreference::MasterVolume));
    assert!(filter.contains(UserPreference::MusicVolume));
    assert!(filter.contains(UserPreference::SfxVolume));
    assert!(filter.contains(UserPreference::UiVolume));
    assert!(filter.contains(UserPreference::VoiceVolume));
    assert!(filter.contains(UserPreference::AmbienceVolume));
    assert!(filter.contains(UserPreference::Resolution));
    assert!(filter.contains(UserPreference::DisplayMode));
    assert!(filter.contains(UserPreference::Vsync));
    assert!(filter.contains(UserPreference::ShowFps));
    assert!(filter.contains(UserPreference::Bindings));
    assert_eq!(filter.len(), 12);
    assert!(!filter.is_empty());
  }

  #[test]
  fn user_preference_filter_none_contains_no_fields() {
    let filter = UserPreferenceFilter::none();

    assert!(!filter.contains(UserPreference::MasterVolume));
    assert!(!filter.contains(UserPreference::MusicVolume));
    assert!(!filter.contains(UserPreference::SfxVolume));
    assert!(!filter.contains(UserPreference::Resolution));
    assert!(!filter.contains(UserPreference::DisplayMode));
    assert!(!filter.contains(UserPreference::Vsync));
    assert!(!filter.contains(UserPreference::ShowFps));
    assert_eq!(filter.len(), 0);
    assert!(filter.is_empty());
  }

  #[test]
  fn user_preference_filter_only_contains_specified_fields() {
    let filter = UserPreferenceFilter::only([
      UserPreference::MasterVolume,
      UserPreference::MusicVolume,
      UserPreference::SfxVolume,
    ]);

    assert!(filter.contains(UserPreference::MasterVolume));
    assert!(filter.contains(UserPreference::MusicVolume));
    assert!(filter.contains(UserPreference::SfxVolume));
    assert!(!filter.contains(UserPreference::Resolution));
    assert!(!filter.contains(UserPreference::DisplayMode));
    assert!(!filter.contains(UserPreference::Vsync));
    assert!(!filter.contains(UserPreference::ShowFps));
    assert_eq!(filter.len(), 3);
  }

  #[test]
  fn user_preference_filter_from_array() {
    let filter: UserPreferenceFilter =
      [UserPreference::Resolution, UserPreference::DisplayMode].into();

    assert!(filter.contains(UserPreference::Resolution));
    assert!(filter.contains(UserPreference::DisplayMode));
    assert!(!filter.contains(UserPreference::MasterVolume));
    assert_eq!(filter.len(), 2);
  }

  #[test]
  fn user_preference_filter_from_vec() {
    let fields = vec![UserPreference::Vsync, UserPreference::ShowFps];
    let filter: UserPreferenceFilter = fields.into();

    assert!(filter.contains(UserPreference::Vsync));
    assert!(filter.contains(UserPreference::ShowFps));
    assert!(!filter.contains(UserPreference::Resolution));
    assert_eq!(filter.len(), 2);
  }

  #[test]
  fn user_preference_filter_default_is_all() {
    let filter = UserPreferenceFilter::default();

    assert_eq!(filter.len(), 12);
    assert!(filter.contains(UserPreference::MasterVolume));
    assert!(filter.contains(UserPreference::Resolution));
  }

  #[test]
  fn user_preference_all_returns_all_variants() {
    let all = UserPreference::all();

    assert_eq!(all.len(), 12);
    assert!(all.contains(&UserPreference::MasterVolume));
    assert!(all.contains(&UserPreference::MusicVolume));
    assert!(all.contains(&UserPreference::SfxVolume));
    assert!(all.contains(&UserPreference::UiVolume));
    assert!(all.contains(&UserPreference::VoiceVolume));
    assert!(all.contains(&UserPreference::AmbienceVolume));
    assert!(all.contains(&UserPreference::Resolution));
    assert!(all.contains(&UserPreference::DisplayMode));
    assert!(all.contains(&UserPreference::Vsync));
    assert!(all.contains(&UserPreference::ShowFps));
    assert!(all.contains(&UserPreference::ShadowQuality));
  }
}
