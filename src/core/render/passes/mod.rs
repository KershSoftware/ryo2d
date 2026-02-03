//! Built-in render passes
//!
//! This module contains all the default render passes that make up the
//! standard 2D rendering pipeline.

mod clear;
mod parallax;
mod upload;

pub use clear::ClearPass;
pub use parallax::{GpuParallaxLayer, ParallaxPass};
pub use upload::UploadPass;

// Re-export pass IDs for convenience
pub use super::pass_ids;
