//! Command-line interface for Ryo2D
//!
//! The `ryo2d` CLI provides commands for building, running, and developing
//! Ryo2D game projects.
//!
//! # Commands
//!
//! - `ryo2d init` - Create a new Ryo2D project
//! - `ryo2d build` - Build a production release
//! - `ryo2d run` - Run the production build
//! - `ryo2d dev` - Development mode with file watching
//! - `ryo2d export` - Package release into distributable zip
//! - `ryo2d install-skills` - Install Agent Skills for AI assistants
//! - `ryo2d netplay` - Start a signaling server for netplay testing
//! - `ryo2d check-ui` - Check UI files without running the game
//!
//! # Example
//!
//! ```bash
//! # Start a new project
//! ryo2d init my_game
//! cd my_game
//!
//! # Install AI assistant skills
//! ryo2d install-skills
//!
//! # Development workflow
//! ryo2d dev
//!
//! # Production build
//! ryo2d build
//! ryo2d run
//!
//! # Export for distribution
//! ryo2d export
//! ```

mod commands;

pub use commands::{CliError, CliResult, run_cli};
