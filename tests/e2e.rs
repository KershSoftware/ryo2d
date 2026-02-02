//! End-to-end tests for Ryo2D engine
//!
//! Run with: `cargo test --test e2e`
//! Or via run script: `./run test:e2e`
//!
//! E2E tests verify complete workflows and full engine initialization.
//! Most e2e tests require GPU context and are marked #[ignore] by default.

#[path = "e2e/engine_lifecycle.rs"]
mod engine_lifecycle;
