//! Profiling instrumentation for Ryo2D.
//!
//! This module provides macros for instrumenting code with profiling data.
//! When the `profiler` feature is enabled, these macros use puffin for
//! hierarchical frame profiling. When disabled, they compile to no-ops.
//!
//! ## Usage
//!
//! Enable profiling by adding the feature flag:
//!
//! ```bash
//! cargo run --features profiler
//! ```
//!
//! In code, use the macros to instrument functions and scopes:
//!
//! ```rust,ignore
//! use ryo2d::core::profiler::{profile_function, profile_scope};
//!
//! fn my_expensive_function() {
//!     profile_function!();  // Profiles entire function
//!
//!     // ... some work ...
//!
//!     {
//!         profile_scope!("inner_loop");  // Profiles just this scope
//!         // ... more work ...
//!     }
//! }
//! ```
//!
//! ## Viewing Profile Data
//!
//! When running with `--features profiler`, the engine automatically starts
//! a puffin HTTP server on port 8585. Connect with puffin_viewer:
//!
//! ```bash
//! cargo install puffin_viewer
//! puffin_viewer --url 127.0.0.1:8585
//! ```
//!
//! This shows a real-time flamegraph of where time is spent each frame.

/// Profile an entire function (adds scope with function name).
///
/// When `profiler` feature is enabled, creates a puffin scope using the
/// function name. When disabled, compiles to nothing.
///
/// # Example
///
/// ```rust,ignore
/// fn update_physics() {
///     profile_function!();
///     // ... physics code ...
/// }
/// ```
#[macro_export]
#[cfg(feature = "profiler")]
macro_rules! profile_function {
  () => {
    ::puffin::profile_function!();
  };
}

#[macro_export]
#[cfg(not(feature = "profiler"))]
macro_rules! profile_function {
  () => {};
}

/// Profile a named scope within a function.
///
/// When `profiler` feature is enabled, creates a puffin scope with the
/// given name. When disabled, compiles to nothing.
///
/// # Example
///
/// ```rust,ignore
/// fn render() {
///     profile_function!();
///
///     {
///         profile_scope!("collect_sprites");
///         // ... sprite collection ...
///     }
///
///     {
///         profile_scope!("submit_batches");
///         // ... GPU submission ...
///     }
/// }
/// ```
#[macro_export]
#[cfg(feature = "profiler")]
macro_rules! profile_scope {
  ($name:expr) => {
    ::puffin::profile_scope!($name);
  };
}

#[macro_export]
#[cfg(not(feature = "profiler"))]
macro_rules! profile_scope {
  ($name:expr) => {};
}

/// Initialize the puffin profiler.
///
/// Call this once at engine startup. When `profiler` feature is enabled and
/// `enabled` is true, this starts the puffin HTTP server. Otherwise, this
/// is a no-op.
///
/// # Arguments
///
/// * `enabled` - Whether to actually start the profiler (from Ryo.toml `puffin_profiler` option)
#[cfg(feature = "profiler")]
pub fn init(enabled: bool) {
  if !enabled {
    return;
  }

  puffin::set_scopes_on(true);

  // Start HTTP server for puffin_viewer connection
  match puffin_http::Server::new("0.0.0.0:8585") {
    Ok(server) => {
      eprintln!("[Profiler] puffin server started on http://127.0.0.1:8585");
      eprintln!("[Profiler] Run `puffin_viewer --url 127.0.0.1:8585` to view");

      // Keep server alive by storing it in a static
      // Using a thread-local to avoid Send/Sync requirements
      std::thread::spawn(move || {
        // Keep server reference alive
        let _server = server;
        loop {
          std::thread::sleep(std::time::Duration::from_secs(3600));
        }
      });
    }
    Err(e) => {
      eprintln!("[Profiler] Failed to start puffin server: {e}");
    }
  }
}

#[cfg(not(feature = "profiler"))]
pub fn init(_enabled: bool) {
  // No-op when profiler feature is disabled
}

/// Mark the start of a new frame for the profiler.
///
/// Call this at the beginning of each frame. When `profiler` feature is
/// enabled, this signals puffin to start a new frame. When disabled,
/// this is a no-op.
#[cfg(feature = "profiler")]
pub fn new_frame() {
  puffin::GlobalProfiler::lock().new_frame();
}

#[cfg(not(feature = "profiler"))]
pub fn new_frame() {
  // No-op when profiler feature is disabled
}

// Re-export macros at module level for convenience
pub use profile_function;
pub use profile_scope;

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_profile_macros_compile() {
    // Just verify macros compile without errors
    profile_function!();
    profile_scope!("test_scope");
  }

  #[test]
  fn test_init_runs() {
    // Verify init doesn't panic (may or may not start server depending on feature)
    // Pass false to avoid actually starting the server during tests
    init(false);
  }

  #[test]
  fn test_new_frame_runs() {
    // Verify new_frame doesn't panic
    new_frame();
  }
}
