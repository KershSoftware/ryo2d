//! # Ferrum2D
//!
//! A simple, scriptable 2D game engine built in Rust.
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use ferrum2d::prelude::*;
//!
//! struct MyGame;
//!
//! impl Game for MyGame {
//!     fn init(&mut self, ctx: &mut GameContext) {
//!         // Set up your game world
//!     }
//!
//!     fn update(&mut self, ctx: &mut GameContext, dt: f32) {
//!         // Game logic runs here
//!     }
//! }
//!
//! fn main() {
//!     EngineBuilder::new()
//!         .title("My Game")
//!         .resolution(800, 600)
//!         .with_game(MyGame)
//!         .build()
//!         .run()
//!         .unwrap();
//! }
//! ```
//!
//! ## Architecture
//!
//! Ferrum2D uses a simple Entity-Component-System (ECS) architecture:
//!
//! - **Entities** are unique IDs representing game objects
//! - **Components** are data attached to entities (Transform, Velocity, SpriteRenderer)
//! - **Systems** process entities with specific components (handled by the engine)
//!
//! You implement the [`game::Game`] trait to define your game's behavior.

pub mod camera;
pub mod ecs;
pub mod engine;
pub mod game;
pub mod input;
pub mod sprite;

/// The prelude module - import everything you need with `use ferrum2d::prelude::*`
pub mod prelude {
  // Core game trait
  pub use crate::game::{Game, GameContext};

  // Engine builder
  pub use crate::engine::EngineBuilder;

  // ECS types
  pub use crate::ecs::{Entity, RenderLayer, SpriteRenderer, Transform, Velocity, World};

  // Graphics
  pub use crate::sprite::{AnimatedSprite, Sprite};

  // Camera
  pub use crate::camera::Camera;

  // Input (re-export KeyCode for convenience)
  pub use crate::input::InputState;
  pub use winit::event::MouseButton;
  pub use winit::keyboard::KeyCode;
}
