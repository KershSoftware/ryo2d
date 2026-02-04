//! Integration tests for Ryo2D engine
//!
//! Run with: `cargo test --test integration`
//! Or via run script: `./run test:integration`
//!
//! These tests verify that engine subsystems work correctly together.

#[path = "integration/lighting.rs"]
mod lighting;

#[path = "integration/network.rs"]
mod network;

#[path = "integration/transform.rs"]
mod transform;

#[path = "integration/ui.rs"]
mod ui;

#[path = "integration/navigation.rs"]
mod navigation;

#[path = "integration/render_pipeline.rs"]
mod render_pipeline;

#[path = "integration/profiler.rs"]
mod profiler;

#[path = "integration/tweening.rs"]
mod tweening;

#[path = "integration/knockback.rs"]
mod knockback;

#[path = "integration/squash_stretch.rs"]
mod squash_stretch;
