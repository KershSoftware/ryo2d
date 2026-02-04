//! Global logging infrastructure for engine dev tools.
//!
//! This module provides a global log buffer that captures log messages from
//! both engine and game code, with context labeling and level filtering.
//!
//! # Usage
//!
//! ```rust
//! use ryo2d::core::dev_log::{log, LogLevel, LogContext};
//!
//! // Engine code
//! log(LogLevel::Info, LogContext::Engine, None, "Renderer initialized".to_string());
//!
//! // Game code with module context
//! log(LogLevel::Debug, LogContext::Game, Some("player"), format!("Player spawned at ({}, {})", 10, 20));
//! ```

use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime};

/// Log level for filtering and display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
  Trace = 0,
  Debug = 1,
  Info = 2,
  Warn = 3,
  Error = 4,
}

impl LogLevel {
  /// Get the display name for this level.
  pub fn name(&self) -> &'static str {
    match self {
      LogLevel::Trace => "TRACE",
      LogLevel::Debug => "DEBUG",
      LogLevel::Info => "INFO",
      LogLevel::Warn => "WARN",
      LogLevel::Error => "ERROR",
    }
  }

  /// Get the short name for this level.
  pub fn short_name(&self) -> &'static str {
    match self {
      LogLevel::Trace => "T",
      LogLevel::Debug => "D",
      LogLevel::Info => "I",
      LogLevel::Warn => "W",
      LogLevel::Error => "E",
    }
  }
}

/// Context/source of the log message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogContext {
  /// Log from engine internals (renderer, physics, audio, etc.)
  Engine,
  /// Log from game code (user scripts, game logic, etc.)
  Game,
  /// Log from external/unknown source
  External,
}

impl LogContext {
  /// Get the display name for this context.
  pub fn name(&self) -> &'static str {
    match self {
      LogContext::Engine => "ENGINE",
      LogContext::Game => "GAME",
      LogContext::External => "EXT",
    }
  }
}

/// A single log entry.
#[derive(Debug, Clone)]
pub struct LogEntry {
  /// Log level.
  pub level: LogLevel,
  /// Context/source.
  pub context: LogContext,
  /// The log message.
  pub message: String,
  /// Optional module/system name.
  pub module: Option<String>,
  /// Timestamp (milliseconds since logger init).
  pub timestamp_ms: u64,
  /// ISO timestamp string (HH:MM:SS.mmm format).
  pub iso_timestamp: String,
  /// Frame number when logged (if available).
  pub frame: Option<u64>,
}

/// Global log buffer.
struct GlobalLogBuffer {
  entries: VecDeque<LogEntry>,
  max_entries: usize,
  start_time: Instant,
  current_frame: u64,
  min_level: LogLevel,
}

impl GlobalLogBuffer {
  fn new() -> Self {
    Self {
      entries: VecDeque::with_capacity(10000),
      max_entries: 10000,
      start_time: Instant::now(),
      current_frame: 0,
      min_level: LogLevel::Trace,
    }
  }

  fn push(
    &mut self,
    level: LogLevel,
    context: LogContext,
    module: Option<String>,
    message: String,
  ) {
    if level < self.min_level {
      return;
    }

    // Generate ISO timestamp (HH:MM:SS.mmm)
    let iso_timestamp = Self::format_iso_timestamp();

    let entry = LogEntry {
      level,
      context,
      message,
      module,
      timestamp_ms: self.start_time.elapsed().as_millis() as u64,
      iso_timestamp,
      frame: Some(self.current_frame),
    };

    if self.entries.len() >= self.max_entries {
      self.entries.pop_front();
    }
    self.entries.push_back(entry);
  }

  /// Format current system time as ISO timestamp (HH:MM:SS.mmm).
  fn format_iso_timestamp() -> String {
    let now = SystemTime::now();
    let duration = now
      .duration_since(SystemTime::UNIX_EPOCH)
      .unwrap_or_default();

    let total_secs = duration.as_secs();
    let millis = duration.subsec_millis();

    // Convert to time of day (UTC)
    let secs_today = total_secs % 86400;
    let hours = secs_today / 3600;
    let minutes = (secs_today % 3600) / 60;
    let seconds = secs_today % 60;

    format!("{:02}:{:02}:{:02}.{:03}", hours, minutes, seconds, millis)
  }
}

static GLOBAL_LOG_BUFFER: OnceLock<Mutex<GlobalLogBuffer>> = OnceLock::new();

fn get_buffer() -> &'static Mutex<GlobalLogBuffer> {
  GLOBAL_LOG_BUFFER.get_or_init(|| Mutex::new(GlobalLogBuffer::new()))
}

/// Initialize the global log buffer (optional, auto-initialized on first use).
pub fn init() {
  let _ = get_buffer();
}

/// Set the minimum log level for filtering.
pub fn set_min_level(level: LogLevel) {
  if let Ok(mut buffer) = get_buffer().lock() {
    buffer.min_level = level;
  }
}

/// Set the current frame number for log entries.
pub fn set_frame(frame: u64) {
  if let Ok(mut buffer) = get_buffer().lock() {
    buffer.current_frame = frame;
  }
}

/// Log a message to the global buffer.
pub fn log(level: LogLevel, context: LogContext, module: Option<&str>, message: String) {
  if let Ok(mut buffer) = get_buffer().lock() {
    buffer.push(level, context, module.map(|s| s.to_string()), message);
  }
}

/// Get a snapshot of the log entries.
pub fn get_entries() -> Vec<LogEntry> {
  if let Ok(buffer) = get_buffer().lock() {
    buffer.entries.iter().cloned().collect()
  } else {
    Vec::new()
  }
}

/// Get log entries since a specific index.
pub fn get_entries_since(start_index: usize) -> Vec<LogEntry> {
  if let Ok(buffer) = get_buffer().lock() {
    buffer.entries.iter().skip(start_index).cloned().collect()
  } else {
    Vec::new()
  }
}

/// Get the total number of log entries.
pub fn entry_count() -> usize {
  if let Ok(buffer) = get_buffer().lock() {
    buffer.entries.len()
  } else {
    0
  }
}

/// Clear all log entries.
pub fn clear() {
  if let Ok(mut buffer) = get_buffer().lock() {
    buffer.entries.clear();
  }
}

/// Log a message with formatting.
#[macro_export]
macro_rules! dev_log {
  ($level:expr, $context:expr, module: $module:expr, $($arg:tt)*) => {
    $crate::core::dev_log::log($level, $context, Some($module), format!($($arg)*))
  };
  ($level:expr, $context:expr, $($arg:tt)*) => {
    $crate::core::dev_log::log($level, $context, None, format!($($arg)*))
  };
}

/// Log an engine trace message.
#[macro_export]
macro_rules! engine_trace {
  (module: $module:expr, $($arg:tt)*) => {
    $crate::core::dev_log::log(
      $crate::core::dev_log::LogLevel::Trace,
      $crate::core::dev_log::LogContext::Engine,
      Some($module),
      format!($($arg)*)
    )
  };
  ($($arg:tt)*) => {
    $crate::core::dev_log::log(
      $crate::core::dev_log::LogLevel::Trace,
      $crate::core::dev_log::LogContext::Engine,
      None,
      format!($($arg)*)
    )
  };
}

/// Log an engine debug message.
#[macro_export]
macro_rules! engine_debug {
  (module: $module:expr, $($arg:tt)*) => {
    $crate::core::dev_log::log(
      $crate::core::dev_log::LogLevel::Debug,
      $crate::core::dev_log::LogContext::Engine,
      Some($module),
      format!($($arg)*)
    )
  };
  ($($arg:tt)*) => {
    $crate::core::dev_log::log(
      $crate::core::dev_log::LogLevel::Debug,
      $crate::core::dev_log::LogContext::Engine,
      None,
      format!($($arg)*)
    )
  };
}

/// Log an engine info message.
#[macro_export]
macro_rules! engine_info {
  (module: $module:expr, $($arg:tt)*) => {
    $crate::core::dev_log::log(
      $crate::core::dev_log::LogLevel::Info,
      $crate::core::dev_log::LogContext::Engine,
      Some($module),
      format!($($arg)*)
    )
  };
  ($($arg:tt)*) => {
    $crate::core::dev_log::log(
      $crate::core::dev_log::LogLevel::Info,
      $crate::core::dev_log::LogContext::Engine,
      None,
      format!($($arg)*)
    )
  };
}

/// Log an engine warning message.
#[macro_export]
macro_rules! engine_warn {
  (module: $module:expr, $($arg:tt)*) => {
    $crate::core::dev_log::log(
      $crate::core::dev_log::LogLevel::Warn,
      $crate::core::dev_log::LogContext::Engine,
      Some($module),
      format!($($arg)*)
    )
  };
  ($($arg:tt)*) => {
    $crate::core::dev_log::log(
      $crate::core::dev_log::LogLevel::Warn,
      $crate::core::dev_log::LogContext::Engine,
      None,
      format!($($arg)*)
    )
  };
}

/// Log an engine error message.
#[macro_export]
macro_rules! engine_error {
  (module: $module:expr, $($arg:tt)*) => {
    $crate::core::dev_log::log(
      $crate::core::dev_log::LogLevel::Error,
      $crate::core::dev_log::LogContext::Engine,
      Some($module),
      format!($($arg)*)
    )
  };
  ($($arg:tt)*) => {
    $crate::core::dev_log::log(
      $crate::core::dev_log::LogLevel::Error,
      $crate::core::dev_log::LogContext::Engine,
      None,
      format!($($arg)*)
    )
  };
}

/// Log a game trace message.
#[macro_export]
macro_rules! game_trace {
  (module: $module:expr, $($arg:tt)*) => {
    $crate::core::dev_log::log(
      $crate::core::dev_log::LogLevel::Trace,
      $crate::core::dev_log::LogContext::Game,
      Some($module),
      format!($($arg)*)
    )
  };
  ($($arg:tt)*) => {
    $crate::core::dev_log::log(
      $crate::core::dev_log::LogLevel::Trace,
      $crate::core::dev_log::LogContext::Game,
      None,
      format!($($arg)*)
    )
  };
}

/// Log a game debug message.
#[macro_export]
macro_rules! game_debug {
  (module: $module:expr, $($arg:tt)*) => {
    $crate::core::dev_log::log(
      $crate::core::dev_log::LogLevel::Debug,
      $crate::core::dev_log::LogContext::Game,
      Some($module),
      format!($($arg)*)
    )
  };
  ($($arg:tt)*) => {
    $crate::core::dev_log::log(
      $crate::core::dev_log::LogLevel::Debug,
      $crate::core::dev_log::LogContext::Game,
      None,
      format!($($arg)*)
    )
  };
}

/// Log a game info message.
#[macro_export]
macro_rules! game_info {
  (module: $module:expr, $($arg:tt)*) => {
    $crate::core::dev_log::log(
      $crate::core::dev_log::LogLevel::Info,
      $crate::core::dev_log::LogContext::Game,
      Some($module),
      format!($($arg)*)
    )
  };
  ($($arg:tt)*) => {
    $crate::core::dev_log::log(
      $crate::core::dev_log::LogLevel::Info,
      $crate::core::dev_log::LogContext::Game,
      None,
      format!($($arg)*)
    )
  };
}

/// Log a game warning message.
#[macro_export]
macro_rules! game_warn {
  (module: $module:expr, $($arg:tt)*) => {
    $crate::core::dev_log::log(
      $crate::core::dev_log::LogLevel::Warn,
      $crate::core::dev_log::LogContext::Game,
      Some($module),
      format!($($arg)*)
    )
  };
  ($($arg:tt)*) => {
    $crate::core::dev_log::log(
      $crate::core::dev_log::LogLevel::Warn,
      $crate::core::dev_log::LogContext::Game,
      None,
      format!($($arg)*)
    )
  };
}

/// Log a game error message.
#[macro_export]
macro_rules! game_error {
  (module: $module:expr, $($arg:tt)*) => {
    $crate::core::dev_log::log(
      $crate::core::dev_log::LogLevel::Error,
      $crate::core::dev_log::LogContext::Game,
      Some($module),
      format!($($arg)*)
    )
  };
  ($($arg:tt)*) => {
    $crate::core::dev_log::log(
      $crate::core::dev_log::LogLevel::Error,
      $crate::core::dev_log::LogContext::Game,
      None,
      format!($($arg)*)
    )
  };
}

// Re-export macros at module level
pub use crate::dev_log;
pub use crate::engine_debug;
pub use crate::engine_error;
pub use crate::engine_info;
pub use crate::engine_trace;
pub use crate::engine_warn;
pub use crate::game_debug;
pub use crate::game_error;
pub use crate::game_info;
pub use crate::game_trace;
pub use crate::game_warn;
