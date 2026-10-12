//! Core engine modules

// Foundation
pub mod ecs;
pub mod error;
pub mod messages;

// Core systems
pub mod asset;
pub mod audio;
pub mod cursor;
pub mod input;
pub(crate) mod input_script;
// MIGRATION: Task 17
// pub mod network;

// Graphics & rendering
pub mod graphics;
pub mod lighting;
// MIGRATION: Task 16
// pub mod particles;
// MIGRATION: Task 20 (parked in attic/)
// pub mod raycast;
pub mod render;
pub mod shaders;
// MIGRATION: Task 16
// pub mod skeleton;
pub mod texture;
// MIGRATION: Task 16
// pub mod tilemap;
pub mod trail;
pub mod ui;
pub mod window;

// Physics
pub mod physics;

// Navigation & Pathfinding
// MIGRATION: Task 16
// pub mod navigation;

// Procedural Generation
pub mod procgen;

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

// Inventory & Item System
// MIGRATION: Task 19 (moves to crates/ryo2d-inventory)
// pub mod inventory;

// Frame structure, clocks and test support
pub mod schedule;
pub mod systems;
pub mod testing;
pub mod time;

// Engine orchestration
pub mod engine;
pub mod scene;
