//! Integration tests for Ryo2D engine
//!
//! Run with: `cargo test --test integration`
//! Or via the dev script: `./dev test:integration`
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

#[path = "integration/trail.rs"]
mod trail;

#[path = "integration/y_sort.rs"]
mod y_sort;

#[path = "integration/ui_tween.rs"]
mod ui_tween;

#[path = "integration/network_input.rs"]
mod network_input;

#[path = "integration/network_action.rs"]
mod network_action;

#[path = "integration/input_handler.rs"]
mod input_handler;

#[path = "integration/prediction.rs"]
mod prediction;

#[path = "integration/config_resolution.rs"]
mod config_resolution;

#[path = "integration/console.rs"]
mod console;

#[path = "integration/tilemap.rs"]
mod tilemap;

#[path = "integration/inventory.rs"]
mod inventory;
