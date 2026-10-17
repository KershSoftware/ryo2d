//! End-to-end tests for Ryo2D engine
//!
//! Run with: `cargo test --test e2e`
//! Or via the dev script: `./dev test:e2e`
//!
//! E2E tests verify complete workflows and full engine initialization.
//! Tests gracefully skip when GPU is unavailable (CI without GPU).

#[path = "common/mod.rs"]
mod common;

#[path = "e2e/engine_lifecycle.rs"]
mod engine_lifecycle;

#[path = "e2e/gpu_basics.rs"]
mod gpu_basics;

#[path = "e2e/capture_readback.rs"]
mod capture_readback;

#[path = "e2e/headless_engine.rs"]
mod headless_engine;

#[path = "e2e/network.rs"]
mod network;

#[path = "e2e/render_pipeline.rs"]
mod render_pipeline;

#[path = "e2e/post_process.rs"]
mod post_process;

#[path = "e2e/sprite_atlas.rs"]
mod sprite_atlas;

#[path = "e2e/gpu_world.rs"]
mod gpu_world;

#[path = "e2e/custom_shaders.rs"]
mod custom_shaders;

#[path = "e2e/sprite_effects.rs"]
mod sprite_effects;

#[path = "e2e/look.rs"]
mod look;

#[path = "e2e/lighting.rs"]
mod lighting;

#[path = "e2e/text.rs"]
mod text;

#[path = "e2e/ui_drawing.rs"]
mod ui_drawing;

#[path = "e2e/ui_focus.rs"]
mod ui_focus;

#[path = "e2e/ui_nodes.rs"]
mod ui_nodes;
