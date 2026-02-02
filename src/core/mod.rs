//! Core engine modules

// Foundation
pub mod ecs;
pub mod error;

// Core systems
pub mod asset;
pub mod audio;
pub mod input;

// Graphics & rendering
pub mod graphics;
pub mod lighting;
pub mod shaders;
pub mod trail;
pub mod ui;

// Physics
pub mod physics;

// Configuration & persistence
pub mod config;

// Engine orchestration
pub mod engine;
pub mod game;
pub mod scene;
