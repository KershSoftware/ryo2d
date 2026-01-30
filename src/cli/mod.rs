//! Command-line interface for Ryo2D
//!
//! The `ryo2d` CLI provides commands for building, running, and developing
//! Ryo2D game projects.
//!
//! # Commands
//!
//! - `ryo2d build` - Build a production release
//! - `ryo2d run` - Run the production build
//! - `ryo2d dev` - Development mode with file watching
//!
//! # Example
//!
//! ```bash
//! # Start a new project
//! cd my_game
//!
//! # Development workflow
//! ryo2d dev
//!
//! # Production build
//! ryo2d build
//! ryo2d run
//! ```

mod commands;

pub use commands::{CliError, CliResult, run_cli};
