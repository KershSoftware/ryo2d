//! Core engine modules

// Foundation
pub mod ecs;
pub mod error;
pub mod messages;

// Core systems
pub mod asset;
pub mod audio;
pub(crate) mod clipboard;
pub mod cursor;
pub mod input;
pub(crate) mod input_script;
#[cfg(not(target_arch = "wasm32"))]
pub mod network;

// Graphics & rendering
pub mod graphics;
pub mod lighting;
pub mod particles;
pub mod render;
pub mod shaders;
pub mod skeleton;
pub mod texture;
pub mod tilemap;
pub mod trail;
pub mod ui;
pub mod window;

// Physics
pub mod physics;

// Navigation & Pathfinding
pub mod navigation;

// State Machines
pub mod state_machine;

// Configuration & persistence
pub mod config;

// Profiling instrumentation
pub mod profiler;

// Development tools - logging, dev windows, and console
pub mod capture;
pub mod console;
pub mod dev_log;
pub mod windows;

// Frame structure, clocks and test support
pub mod schedule;
pub mod systems;
#[cfg(not(target_arch = "wasm32"))]
pub mod testing;
pub mod time;

// Engine orchestration
pub mod engine;
pub mod scene;
