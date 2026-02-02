//! Theme system for UI styling with hot-reload support
//!
//! Provides centralized styling for all UI widgets with the ability to
//! reload themes at runtime without rebuilding the application.
//!
//! # Example
//!
//! ```ignore
//! // Load a theme from file (supports hot-reload)
//! let theme = Theme::load("assets/ui/theme.json")?;
//!
//! // Or use the default theme
//! let theme = Theme::default();
//!
//! // Access widget styles
//! let button_style = &theme.button;
//! ```

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
#[cfg(debug_assertions)]
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use super::types::Color;

// ============================================================================
// Widget Styles
// ============================================================================

/// Style configuration for buttons.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ButtonStyle {
  /// Background color in normal state
  pub background: Color,
  /// Background color when hovered
  pub background_hovered: Color,
  /// Background color when pressed
  pub background_pressed: Color,
  /// Background color when disabled
  pub background_disabled: Color,
  /// Border color in normal state
  pub border: Color,
  /// Border color when hovered
  pub border_hovered: Color,
  /// Border thickness in pixels
  pub border_thickness: f32,
  /// Text color in normal state
  pub text: Color,
  /// Text color when disabled
  pub text_disabled: Color,
  /// Padding inside the button (horizontal, vertical)
  pub padding: (f32, f32),
}

impl Default for ButtonStyle {
  fn default() -> Self {
    Self {
      background: Color::rgb(40, 40, 60),
      background_hovered: Color::rgb(50, 50, 70),
      background_pressed: Color::rgb(60, 60, 80),
      background_disabled: Color::rgb(30, 30, 40),
      border: Color::rgb(80, 80, 100),
      border_hovered: Color::rgb(100, 100, 140),
      border_thickness: 1.0,
      text: Color::WHITE,
      text_disabled: Color::GRAY,
      padding: (8.0, 4.0),
    }
  }
}

/// Style configuration for panels/containers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelStyle {
  /// Background color
  pub background: Color,
  /// Border color (None for no border)
  pub border: Option<Color>,
  /// Border thickness in pixels
  pub border_thickness: f32,
  /// Inner padding
  pub padding: f32,
}

impl Default for PanelStyle {
  fn default() -> Self {
    Self {
      background: Color::new(20, 20, 30, 200),
      border: Some(Color::rgb(80, 80, 100)),
      border_thickness: 1.0,
      padding: 8.0,
    }
  }
}

/// Style configuration for progress bars.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressBarStyle {
  /// Background (unfilled) color
  pub background: Color,
  /// Fill color
  pub fill: Color,
  /// Border color (None for no border)
  pub border: Option<Color>,
  /// Border thickness
  pub border_thickness: f32,
  /// Height of the progress bar
  pub height: f32,
}

impl Default for ProgressBarStyle {
  fn default() -> Self {
    Self {
      background: Color::DARK_GRAY,
      fill: Color::rgb(80, 150, 255),
      border: None,
      border_thickness: 1.0,
      height: 20.0,
    }
  }
}

/// Style configuration for sliders.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SliderStyle {
  /// Track background color
  pub track_background: Color,
  /// Track fill color (portion before handle)
  pub track_fill: Color,
  /// Handle color in normal state
  pub handle: Color,
  /// Handle color when hovered/active
  pub handle_hovered: Color,
  /// Track height
  pub track_height: f32,
  /// Handle width
  pub handle_width: f32,
  /// Handle height (typically larger than track)
  pub handle_height: f32,
}

impl Default for SliderStyle {
  fn default() -> Self {
    Self {
      track_background: Color::rgb(60, 60, 80),
      track_fill: Color::rgb(80, 150, 255),
      handle: Color::WHITE,
      handle_hovered: Color::rgb(200, 220, 255),
      track_height: 8.0,
      handle_width: 12.0,
      handle_height: 16.0,
    }
  }
}

/// Style configuration for checkboxes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckboxStyle {
  /// Box background color (unchecked)
  pub background: Color,
  /// Box background color (checked)
  pub background_checked: Color,
  /// Box background when hovered
  pub background_hovered: Color,
  /// Border color
  pub border: Color,
  /// Checkmark color
  pub checkmark: Color,
  /// Size of the checkbox (width and height)
  pub size: f32,
  /// Border thickness
  pub border_thickness: f32,
}

impl Default for CheckboxStyle {
  fn default() -> Self {
    Self {
      background: Color::rgb(40, 40, 60),
      background_checked: Color::rgb(60, 120, 200),
      background_hovered: Color::rgb(50, 50, 70),
      border: Color::rgb(80, 80, 100),
      checkmark: Color::WHITE,
      size: 20.0,
      border_thickness: 1.0,
    }
  }
}

/// Style configuration for toggle switches.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToggleStyle {
  /// Track color when off
  pub track_off: Color,
  /// Track color when on
  pub track_on: Color,
  /// Track color when hovered
  pub track_hovered: Color,
  /// Knob color
  pub knob: Color,
  /// Track width
  pub width: f32,
  /// Track height
  pub height: f32,
  /// Knob diameter (should be slightly less than height)
  pub knob_size: f32,
}

impl Default for ToggleStyle {
  fn default() -> Self {
    Self {
      track_off: Color::rgb(60, 60, 80),
      track_on: Color::rgb(60, 150, 80),
      track_hovered: Color::rgb(70, 70, 90),
      knob: Color::WHITE,
      width: 44.0,
      height: 24.0,
      knob_size: 20.0,
    }
  }
}

/// Style configuration for text input fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextInputStyle {
  /// Background color
  pub background: Color,
  /// Background when focused
  pub background_focused: Color,
  /// Border color
  pub border: Color,
  /// Border when focused
  pub border_focused: Color,
  /// Text color
  pub text: Color,
  /// Placeholder text color
  pub placeholder: Color,
  /// Cursor color
  pub cursor: Color,
  /// Selection highlight color
  pub selection: Color,
  /// Border thickness
  pub border_thickness: f32,
  /// Internal padding
  pub padding: (f32, f32),
}

impl Default for TextInputStyle {
  fn default() -> Self {
    Self {
      background: Color::rgb(30, 30, 45),
      background_focused: Color::rgb(35, 35, 50),
      border: Color::rgb(70, 70, 90),
      border_focused: Color::rgb(100, 140, 200),
      text: Color::WHITE,
      placeholder: Color::GRAY,
      cursor: Color::WHITE,
      selection: Color::new(100, 140, 200, 100),
      border_thickness: 1.0,
      padding: (8.0, 4.0),
    }
  }
}

/// Style configuration for labels.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelStyle {
  /// Default text color
  pub text: Color,
  /// Muted/secondary text color
  pub text_muted: Color,
  /// Accent text color (for emphasis)
  pub text_accent: Color,
  /// Error text color
  pub text_error: Color,
  /// Success text color
  pub text_success: Color,
  /// Warning text color
  pub text_warning: Color,
}

impl Default for LabelStyle {
  fn default() -> Self {
    Self {
      text: Color::WHITE,
      text_muted: Color::GRAY,
      text_accent: Color::rgb(100, 180, 255),
      text_error: Color::rgb(255, 80, 80),
      text_success: Color::rgb(80, 200, 80),
      text_warning: Color::rgb(255, 200, 80),
    }
  }
}

/// Style configuration for tooltips.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TooltipStyle {
  /// Background color
  pub background: Color,
  /// Border color
  pub border: Color,
  /// Text color
  pub text: Color,
  /// Padding inside the tooltip
  pub padding: f32,
  /// Border thickness
  pub border_thickness: f32,
  /// Delay before showing tooltip (in seconds)
  pub delay: f32,
}

impl Default for TooltipStyle {
  fn default() -> Self {
    Self {
      background: Color::new(30, 30, 40, 240),
      border: Color::rgb(70, 70, 90),
      text: Color::WHITE,
      padding: 6.0,
      border_thickness: 1.0,
      delay: 0.5,
    }
  }
}

/// Style configuration for scrollbars.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrollbarStyle {
  /// Track background color
  pub track: Color,
  /// Thumb color
  pub thumb: Color,
  /// Thumb color when hovered
  pub thumb_hovered: Color,
  /// Thumb color when dragging
  pub thumb_active: Color,
  /// Scrollbar width
  pub width: f32,
  /// Minimum thumb length
  pub min_thumb_length: f32,
}

impl Default for ScrollbarStyle {
  fn default() -> Self {
    Self {
      track: Color::new(40, 40, 50, 100),
      thumb: Color::rgb(80, 80, 100),
      thumb_hovered: Color::rgb(100, 100, 120),
      thumb_active: Color::rgb(120, 120, 140),
      width: 12.0,
      min_thumb_length: 30.0,
    }
  }
}

// ============================================================================
// Theme
// ============================================================================

/// Complete UI theme containing styles for all widgets.
///
/// Themes can be loaded from JSON files and support hot-reloading during development.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Theme {
  /// Theme name/identifier
  pub name: String,
  /// Button style
  pub button: ButtonStyle,
  /// Panel style
  pub panel: PanelStyle,
  /// Progress bar style
  pub progress_bar: ProgressBarStyle,
  /// Slider style
  pub slider: SliderStyle,
  /// Checkbox style
  pub checkbox: CheckboxStyle,
  /// Toggle switch style
  pub toggle: ToggleStyle,
  /// Text input style
  pub text_input: TextInputStyle,
  /// Label style
  pub label: LabelStyle,
  /// Tooltip style
  pub tooltip: TooltipStyle,
  /// Scrollbar style
  pub scrollbar: ScrollbarStyle,
  /// Global spacing between elements
  pub spacing: f32,
  /// Global margin around containers
  pub margin: f32,
}

impl Default for Theme {
  fn default() -> Self {
    Self {
      name: "default".to_string(),
      button: ButtonStyle::default(),
      panel: PanelStyle::default(),
      progress_bar: ProgressBarStyle::default(),
      slider: SliderStyle::default(),
      checkbox: CheckboxStyle::default(),
      toggle: ToggleStyle::default(),
      text_input: TextInputStyle::default(),
      label: LabelStyle::default(),
      tooltip: TooltipStyle::default(),
      scrollbar: ScrollbarStyle::default(),
      spacing: 8.0,
      margin: 16.0,
    }
  }
}

impl Theme {
  /// Create a new default theme.
  pub fn new() -> Self {
    Self::default()
  }

  /// Load a theme from a JSON file.
  ///
  /// # Errors
  /// Returns an error if the file cannot be read or parsed.
  pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, ThemeError> {
    let path = path.as_ref();
    let contents = std::fs::read_to_string(path).map_err(|e| ThemeError::IoError {
      path: path.to_path_buf(),
      error: e.to_string(),
    })?;

    serde_json::from_str(&contents).map_err(|e| ThemeError::ParseError {
      path: path.to_path_buf(),
      error: e.to_string(),
    })
  }

  /// Save the theme to a JSON file.
  ///
  /// # Errors
  /// Returns an error if the file cannot be written.
  pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<(), ThemeError> {
    let path = path.as_ref();

    let contents = serde_json::to_string_pretty(self).map_err(|e| ThemeError::SerializeError {
      error: e.to_string(),
    })?;

    std::fs::write(path, contents).map_err(|e| ThemeError::IoError {
      path: path.to_path_buf(),
      error: e.to_string(),
    })
  }

  /// Create a dark theme variant.
  pub fn dark() -> Self {
    Self::default() // Default is already dark
  }

  /// Create a light theme variant.
  pub fn light() -> Self {
    Self {
      name: "light".to_string(),
      button: ButtonStyle {
        background: Color::rgb(220, 220, 230),
        background_hovered: Color::rgb(200, 200, 210),
        background_pressed: Color::rgb(180, 180, 190),
        background_disabled: Color::rgb(240, 240, 245),
        border: Color::rgb(180, 180, 190),
        border_hovered: Color::rgb(150, 150, 160),
        text: Color::rgb(30, 30, 40),
        text_disabled: Color::rgb(150, 150, 160),
        ..ButtonStyle::default()
      },
      panel: PanelStyle {
        background: Color::new(245, 245, 250, 230),
        border: Some(Color::rgb(200, 200, 210)),
        ..PanelStyle::default()
      },
      label: LabelStyle {
        text: Color::rgb(30, 30, 40),
        text_muted: Color::rgb(120, 120, 130),
        ..LabelStyle::default()
      },
      text_input: TextInputStyle {
        background: Color::rgb(255, 255, 255),
        background_focused: Color::rgb(250, 250, 255),
        border: Color::rgb(180, 180, 190),
        border_focused: Color::rgb(100, 140, 200),
        text: Color::rgb(30, 30, 40),
        placeholder: Color::rgb(150, 150, 160),
        ..TextInputStyle::default()
      },
      tooltip: TooltipStyle {
        background: Color::new(50, 50, 60, 240),
        text: Color::WHITE,
        ..TooltipStyle::default()
      },
      ..Self::default()
    }
  }
}

// ============================================================================
// Hot-Reloadable Theme
// ============================================================================

/// A theme wrapper that supports hot-reloading from a file.
///
/// The theme will automatically reload when the source file changes.
/// This is useful during development for rapid iteration on UI styling.
///
/// **Note:** Hot-reloading is only active in debug builds. In release builds,
/// `poll_reload()` is a no-op and `force_reload()` does nothing.
///
/// # Example
///
/// ```ignore
/// let hot_theme = HotReloadTheme::new("assets/ui/theme.json")?;
///
/// // In your game loop:
/// hot_theme.poll_reload(); // Check for changes (debug only)
/// let theme = hot_theme.get(); // Get current theme
/// ```
pub struct HotReloadTheme {
  theme: Arc<RwLock<Theme>>,
  path: PathBuf,
  #[cfg(debug_assertions)]
  last_modified: RwLock<Option<SystemTime>>,
}

impl HotReloadTheme {
  /// Create a new hot-reloadable theme from a file.
  ///
  /// If the file doesn't exist, it will be created with the default theme.
  pub fn new<P: AsRef<Path>>(path: P) -> Result<Self, ThemeError> {
    let path = path.as_ref().to_path_buf();

    let theme = if path.exists() {
      Theme::load(&path)?
    } else {
      // Create default theme file
      let theme = Theme::default();
      theme.save(&path)?;
      theme
    };

    #[cfg(debug_assertions)]
    let last_modified = std::fs::metadata(&path)
      .ok()
      .and_then(|m| m.modified().ok());

    Ok(Self {
      theme: Arc::new(RwLock::new(theme)),
      path,
      #[cfg(debug_assertions)]
      last_modified: RwLock::new(last_modified),
    })
  }

  /// Check if the theme file has been modified and reload if necessary.
  ///
  /// This is a lightweight operation - it only checks the file's
  /// modification time before deciding to reload.
  ///
  /// **Note:** This is a no-op in release builds.
  ///
  /// Returns `true` if the theme was reloaded.
  #[cfg(debug_assertions)]
  pub fn poll_reload(&self) -> bool {
    let current_modified = std::fs::metadata(&self.path)
      .ok()
      .and_then(|m| m.modified().ok());

    let should_reload = {
      let last = self.last_modified.read().unwrap();
      match (*last, current_modified) {
        (Some(last), Some(current)) => current > last,
        (None, Some(_)) => true,
        _ => false,
      }
    };

    if should_reload && let Ok(new_theme) = Theme::load(&self.path) {
      *self.theme.write().unwrap() = new_theme;
      *self.last_modified.write().unwrap() = current_modified;
      return true;
    }

    false
  }

  /// Check if the theme file has been modified and reload if necessary.
  ///
  /// **Note:** This is a no-op in release builds.
  #[cfg(not(debug_assertions))]
  #[inline]
  pub fn poll_reload(&self) -> bool {
    false
  }

  /// Force a reload from the file, regardless of modification time.
  ///
  /// **Note:** This is a no-op in release builds.
  #[cfg(debug_assertions)]
  pub fn force_reload(&self) -> Result<(), ThemeError> {
    let new_theme = Theme::load(&self.path)?;
    *self.theme.write().unwrap() = new_theme;

    let modified = std::fs::metadata(&self.path)
      .ok()
      .and_then(|m| m.modified().ok());
    *self.last_modified.write().unwrap() = modified;

    Ok(())
  }

  /// Force a reload from the file, regardless of modification time.
  ///
  /// **Note:** This is a no-op in release builds.
  #[cfg(not(debug_assertions))]
  #[inline]
  pub fn force_reload(&self) -> Result<(), ThemeError> {
    Ok(())
  }

  /// Get a clone of the current theme.
  ///
  /// This is the recommended way to access the theme for rendering,
  /// as it doesn't hold the lock during rendering.
  pub fn get(&self) -> Theme {
    self.theme.read().unwrap().clone()
  }

  /// Get a reference to the theme path.
  pub fn path(&self) -> &Path {
    &self.path
  }

  /// Replace the current theme and save to file.
  #[cfg(debug_assertions)]
  pub fn set(&self, theme: Theme) -> Result<(), ThemeError> {
    theme.save(&self.path)?;
    *self.theme.write().unwrap() = theme;
    let modified = std::fs::metadata(&self.path)
      .ok()
      .and_then(|m| m.modified().ok());
    *self.last_modified.write().unwrap() = modified;
    Ok(())
  }

  /// Replace the current theme and save to file.
  #[cfg(not(debug_assertions))]
  pub fn set(&self, theme: Theme) -> Result<(), ThemeError> {
    theme.save(&self.path)?;
    *self.theme.write().unwrap() = theme;
    Ok(())
  }
}

impl std::fmt::Debug for HotReloadTheme {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("HotReloadTheme")
      .field("path", &self.path)
      .field("theme", &self.theme.read().unwrap().name)
      .finish()
  }
}

// ============================================================================
// Errors
// ============================================================================

/// Errors that can occur when working with themes.
#[derive(Debug)]
pub enum ThemeError {
  /// Failed to read or write file
  IoError { path: PathBuf, error: String },
  /// Failed to parse theme file
  ParseError { path: PathBuf, error: String },
  /// Failed to serialize theme
  SerializeError { error: String },
}

impl std::fmt::Display for ThemeError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      ThemeError::IoError { path, error } => {
        write!(f, "IO error for {:?}: {}", path, error)
      }
      ThemeError::ParseError { path, error } => {
        write!(f, "Parse error in {:?}: {}", path, error)
      }
      ThemeError::SerializeError { error } => {
        write!(f, "Serialization error: {}", error)
      }
    }
  }
}

impl std::error::Error for ThemeError {}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
  use super::*;
  use std::io::Write;
  use tempfile::NamedTempFile;

  #[test]
  fn default_theme_is_valid() {
    let theme = Theme::default();
    assert_eq!(theme.name, "default");
    assert!(theme.spacing > 0.0);
  }

  #[test]
  fn light_theme_is_valid() {
    let theme = Theme::light();
    assert_eq!(theme.name, "light");
  }

  #[test]
  fn theme_save_and_load() {
    let theme = Theme::default();
    let temp_file = NamedTempFile::new().unwrap();
    let path = temp_file.path();

    theme.save(path).unwrap();
    let loaded = Theme::load(path).unwrap();

    assert_eq!(loaded.name, theme.name);
    assert_eq!(loaded.button.background, theme.button.background);
  }

  #[test]
  fn hot_reload_theme_creates_file() {
    let temp_dir = tempfile::tempdir().unwrap();
    let path = temp_dir.path().join("theme.json");

    // File doesn't exist yet
    assert!(!path.exists());

    let hot_theme = HotReloadTheme::new(&path).unwrap();

    // File should now exist
    assert!(path.exists());
    assert_eq!(hot_theme.get().name, "default");
  }

  #[test]
  fn hot_reload_detects_changes() {
    let temp_dir = tempfile::tempdir().unwrap();
    let path = temp_dir.path().join("theme.json");

    let hot_theme = HotReloadTheme::new(&path).unwrap();
    assert_eq!(hot_theme.get().name, "default");

    // Modify the file
    std::thread::sleep(std::time::Duration::from_millis(50));
    let mut file = std::fs::File::create(&path).unwrap();
    let modified_theme = Theme {
      name: "modified".to_string(),
      ..Theme::default()
    };
    let contents = serde_json::to_string_pretty(&modified_theme).unwrap();
    file.write_all(contents.as_bytes()).unwrap();
    drop(file);

    // Poll should detect the change
    std::thread::sleep(std::time::Duration::from_millis(50));
    let reloaded = hot_theme.poll_reload();
    assert!(reloaded);
    assert_eq!(hot_theme.get().name, "modified");
  }

  #[test]
  fn button_style_defaults_are_sensible() {
    let style = ButtonStyle::default();
    assert!(style.border_thickness > 0.0);
    assert!(style.padding.0 > 0.0);
    assert!(style.padding.1 > 0.0);
  }

  #[test]
  fn slider_style_defaults_are_sensible() {
    let style = SliderStyle::default();
    assert!(style.track_height > 0.0);
    assert!(style.handle_width > 0.0);
    assert!(style.handle_height >= style.track_height);
  }
}
