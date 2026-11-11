//! The runtime debug switches.

/// Debug configuration for engine rendering.
///
/// Controls which debug visualizations are enabled.
/// Can be loaded from `Ryo.toml` or set programmatically.
#[derive(Debug, Clone)]
pub struct DebugConfig {
  /// Whether to render collision boundaries (bright green outlines)
  pub show_colliders: bool,
  /// Whether to display FPS counter in corner
  pub show_fps: bool,
  /// Whether to display frame timing breakdown (update/physics/render/present)
  pub show_frame_timing: bool,
  /// Whether to display entity/sprite/batch statistics
  pub show_stats: bool,
  /// Whether to draw velocity vectors on physics entities
  pub show_velocities: bool,
  /// Whether to draw entity origin points (small crosshairs at transform position)
  pub show_origins: bool,
  /// Whether the F1-F5 debug key toggles are off (read once, at start)
  pub disable_debug_keys: bool,
  /// Whether to outline every UI widget with its id (`debug:ui`; runtime only)
  pub show_ui_debug: bool,
  /// Whether to outline the UI node subtrees rebuilt each frame, fading,
  /// with a rebuilt/replayed counter and a render line (the UI image kept,
  /// texts built and reused, over the last second), both updated once a
  /// second, in the bottom-left stack above the other overlays
  /// (`debug:ui-rebuilds`; runtime only)
  pub show_ui_rebuilds: bool,
  /// Whether to open the flamegraph profiler window (requires --features profiler or `ryo2d dev --profiler`)
  pub enable_profiler: bool,
  /// Whether to open the logger dev window at startup (shows engine/game logs in real-time)
  pub logger_window: bool,
  /// Whether to enable dev hotkeys (F9=logger, F10=profiler, F12=frame capture).
  /// On by default in a build with the developer tools; a build without
  /// them has no such keys, and this stays off.
  pub dev_window_keys: bool,
  /// Whether the developer console opens with the backquote key
  /// (`[debug] console`). On by default in a build with the developer
  /// tools; a build without them has no console, and this stays off.
  pub dev_console: bool,
  /// Whether UI node trees replay their unchanged subtrees from the last
  /// frame instead of building them (default true). Off, every node is
  /// built every frame: it draws and behaves the same, slower. For ruling
  /// caching out when chasing a UI bug.
  pub ui_cache: bool,
  /// Whether UI node trees keep the layout and resolved output of unchanged
  /// subtrees between frames (default true). Off, unchanged subtrees are
  /// replayed and resolved every frame: the same output, slower.
  pub ui_fragments: bool,
  /// Whether the renderer keeps the UI image while the UI is unchanged, and
  /// reuses the glyphs built for unchanged texts (default true). Off, it
  /// builds and draws the whole UI every frame: the same pixels, slower.
  /// For ruling retention out when chasing a rendering bug.
  pub ui_retain: bool,
}

impl Default for DebugConfig {
  fn default() -> Self {
    Self {
      show_colliders: false,
      show_fps: false,
      show_frame_timing: false,
      show_stats: false,
      show_velocities: false,
      show_origins: false,
      disable_debug_keys: false,
      show_ui_debug: false,
      show_ui_rebuilds: false,
      enable_profiler: false, // Disabled by default (opt-in)
      logger_window: false,   // Disabled by default (opt-in)
      // The console and the dev window keys follow the build's tools
      dev_window_keys: cfg!(dev_tools),
      dev_console: cfg!(dev_tools),
      ui_cache: true,
      ui_fragments: true,
      ui_retain: true,
    }
  }
}

impl DebugConfig {
  /// Turn off every switch that shows a developer tool or overlay: a build
  /// without the tools has none, whatever the project or the builder set.
  /// The FPS counter is the player's setting and stays.
  #[cfg(not(dev_tools))]
  pub(crate) fn switch_tools_off(&mut self) {
    self.dev_console = false;
    self.dev_window_keys = false;
    self.logger_window = false;
    self.enable_profiler = false;
    self.show_colliders = false;
    self.show_velocities = false;
    self.show_origins = false;
    self.show_frame_timing = false;
    self.show_stats = false;
    self.show_ui_debug = false;
    self.show_ui_rebuilds = false;
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn debug_config_defaults() {
    let config = DebugConfig::default();
    assert!(!config.show_colliders);
    assert!(!config.show_fps);
    assert!(!config.show_frame_timing);
    assert!(!config.show_stats);
    assert!(!config.show_velocities);
    assert!(!config.show_origins);
    assert!(!config.disable_debug_keys);
    // enable_profiler defaults to false (opt-in)
    assert!(!config.enable_profiler);
    assert!(config.ui_cache, "UI node trees are cached by default");
    assert!(config.ui_fragments, "UI fragments are kept by default");
    assert!(config.ui_retain, "UI render retention is on by default");
  }
}
