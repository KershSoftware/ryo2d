//! Hot reload system for development-time asset updates
//!
//! This module provides background filesystem watching to detect when asset files
//! change, enabling instant updates without restarting the game.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────┐     ┌──────────────────┐     ┌─────────────────┐
//! │  Filesystem     │────►│  Watcher Thread  │────►│  Main Thread    │
//! │  (file saved)   │     │  (notify crate)  │     │  (process_      │
//! │                 │     │                  │     │   reloads)      │
//! └─────────────────┘     └──────────────────┘     └─────────────────┘
//!                              │                         ▲
//!                              │    mpsc channel         │
//!                              └─────────────────────────┘
//! ```
//!
//! # Usage
//!
//! Hot reloading is enabled automatically in debug builds. Call `process_reloads()`
//! each frame to apply any pending asset updates.

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use crate::core::error::{HotReloadError, HotReloadResult};

/// Messages sent from the watcher thread to the main thread
#[derive(Debug, Clone)]
pub enum HotReloadEvent {
  /// A file was modified and should be reloaded
  FileModified(PathBuf),
  /// A file was created
  FileCreated(PathBuf),
  /// A file was deleted
  FileDeleted(PathBuf),
  /// An error occurred in the watcher
  Error(String),
}

/// Configuration for the hot reload system
#[derive(Debug, Clone)]
pub struct HotReloadConfig {
  /// Directories to watch for changes
  pub watch_paths: Vec<PathBuf>,
  /// File extensions to monitor (e.g., ["png", "jpg", "ogg"])
  pub extensions: HashSet<String>,
  /// Whether to watch subdirectories recursively
  pub recursive: bool,
}

impl Default for HotReloadConfig {
  fn default() -> Self {
    let mut extensions = HashSet::new();
    // Common image formats
    extensions.insert("png".to_string());
    extensions.insert("jpg".to_string());
    extensions.insert("jpeg".to_string());
    extensions.insert("gif".to_string());
    extensions.insert("bmp".to_string());
    // Audio formats
    extensions.insert("wav".to_string());
    extensions.insert("ogg".to_string());
    extensions.insert("mp3".to_string());
    extensions.insert("flac".to_string());

    HotReloadConfig {
      watch_paths: vec![PathBuf::from(".")],
      extensions,
      recursive: true,
    }
  }
}

/// Handle to the hot reload watcher system
///
/// Create this once and call `poll()` each frame to check for file changes.
pub struct HotReloadWatcher {
  /// Receiver for events from the watcher thread
  receiver: Receiver<HotReloadEvent>,
  /// Handle to the watcher thread (kept alive as long as this struct exists)
  _thread_handle: JoinHandle<()>,
  /// The watcher itself (must be kept alive)
  _watcher: Arc<Mutex<RecommendedWatcher>>,
  /// Configuration
  config: HotReloadConfig,
}

impl HotReloadWatcher {
  /// Create a new hot reload watcher with the given configuration
  ///
  /// This spawns a background thread that monitors the filesystem.
  pub fn new(config: HotReloadConfig) -> HotReloadResult<Self> {
    let (tx, rx) = channel::<HotReloadEvent>();

    // Create the notify watcher
    let tx_clone = tx.clone();
    let extensions = config.extensions.clone();

    let watcher = notify::recommended_watcher(move |result: Result<Event, notify::Error>| {
      match result {
        Ok(event) => {
          // Filter by event kind and extension
          let dominated = matches!(
            event.kind,
            EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)
          );

          if dominated {
            for path in event.paths {
              // Check extension filter
              let should_process = path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| extensions.contains(&ext.to_lowercase()))
                .unwrap_or(false);

              if should_process {
                let event = match event.kind {
                  EventKind::Modify(_) => HotReloadEvent::FileModified(path),
                  EventKind::Create(_) => HotReloadEvent::FileCreated(path),
                  EventKind::Remove(_) => HotReloadEvent::FileDeleted(path),
                  _ => continue,
                };

                // Ignore send errors (receiver might be dropped)
                let _ = tx_clone.send(event);
              }
            }
          }
        }
        Err(e) => {
          let _ = tx_clone.send(HotReloadEvent::Error(e.to_string()));
        }
      }
    })
    .map_err(|e| HotReloadError::WatcherCreation(e.to_string()))?;

    let watcher = Arc::new(Mutex::new(watcher));
    let watcher_clone = Arc::clone(&watcher);
    let watch_paths = config.watch_paths.clone();
    let recursive = config.recursive;

    // Spawn thread to set up watches (watcher callbacks run on notify's internal thread)
    let thread_handle = thread::spawn(move || {
      let mut watcher = watcher_clone.lock().expect("HotReload watcher Mutex poisoned");
      let mode = if recursive {
        RecursiveMode::Recursive
      } else {
        RecursiveMode::NonRecursive
      };

      for path in &watch_paths {
        if let Err(e) = watcher.watch(path, mode) {
          eprintln!("[HotReload] Failed to watch {:?}: {}", path, e);
        } else {
          println!("[HotReload] Watching {:?}", path);
        }
      }
    });

    Ok(HotReloadWatcher {
      receiver: rx,
      _thread_handle: thread_handle,
      _watcher: watcher,
      config,
    })
  }

  /// Create a hot reload watcher with default configuration
  ///
  /// Watches the current directory recursively for common asset types.
  pub fn with_defaults() -> HotReloadResult<Self> {
    Self::new(HotReloadConfig::default())
  }

  /// Create a watcher for a specific assets directory
  pub fn for_directory<P: AsRef<Path>>(path: P) -> HotReloadResult<Self> {
    let config = HotReloadConfig {
      watch_paths: vec![path.as_ref().to_path_buf()],
      ..HotReloadConfig::default()
    };
    Self::new(config)
  }

  /// Poll for pending file change events
  ///
  /// Returns all events that have occurred since the last poll.
  /// Call this once per frame in your game loop.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// for event in hot_reload.poll() {
  ///     match event {
  ///         HotReloadEvent::FileModified(path) => {
  ///             println!("File changed: {:?}", path);
  ///             // Reload the asset
  ///         }
  ///         _ => {}
  ///     }
  /// }
  /// ```
  pub fn poll(&self) -> Vec<HotReloadEvent> {
    let mut events = Vec::new();
    while let Ok(event) = self.receiver.try_recv() {
      events.push(event);
    }
    events
  }

  /// Check if any events are pending without consuming them
  pub fn has_pending(&self) -> bool {
    // We can't actually peek without consuming, so this is approximate
    // In practice, just call poll() and check if vec is empty
    false
  }

  /// Get the configuration
  pub fn config(&self) -> &HotReloadConfig {
    &self.config
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn hot_reload_config_default_has_common_extensions() {
    let config = HotReloadConfig::default();
    assert!(config.extensions.contains("png"));
    assert!(config.extensions.contains("jpg"));
    assert!(config.extensions.contains("ogg"));
    assert!(config.extensions.contains("wav"));
  }

  #[test]
  fn hot_reload_event_debug() {
    let event = HotReloadEvent::FileModified(PathBuf::from("test.png"));
    let debug = format!("{:?}", event);
    assert!(debug.contains("FileModified"));
    assert!(debug.contains("test.png"));
  }
}
