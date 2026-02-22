//! Core engine modules

// Foundation
pub mod ecs;
pub mod error;
pub mod events;

// Core systems
pub mod asset;
pub mod audio;
pub mod cursor;
pub mod input;
pub mod network;

// Graphics & rendering
pub mod graphics;
pub mod lighting;
pub mod particles;
pub mod tilemap;
pub mod render;
pub mod shaders;
pub mod trail;
pub mod ui;

// Physics
pub mod physics;

// Navigation & Pathfinding
pub mod navigation;

// Procedural Generation
pub mod procgen;

// State Machines
pub mod state_machine;

// Configuration & persistence
pub mod config;

// Profiling instrumentation
pub mod profiler;

// Development tools - logging, dev windows, and console
pub mod console;
pub mod dev_log;
pub mod windows;

// Game systems (extracted from engine for modularity)
pub mod systems;
pub mod timing;

// Engine orchestration
pub mod engine;
pub mod game;
pub mod resources;
pub mod scene;
