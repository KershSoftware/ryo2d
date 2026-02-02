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
