//! # Ryo2D
//!
//! A simple, scriptable 2D game engine built in Rust.
//!
//! ## Quick Start
//!
//! ```no_run
//! use ryo2d::prelude::*;
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
//! Ryo2D uses a simple Entity-Component-System (ECS) architecture:
//!
//! - **Entities** are unique IDs representing game objects
//! - **Components** are data attached to entities (Transform, Velocity, Renderable)
//! - **Systems** process entities with specific components (handled by the engine)
//!
//! You implement the [`core::game::Game`] trait to define your game's behavior.

pub mod cli;
pub mod core;

/// The prelude module - import everything you need with `use ryo2d::prelude::*`
pub mod prelude {
  // Core game trait
  pub use crate::core::game::{Game, GameContext, GpuRenderContext};

  // Scene management
  pub use crate::core::scene::{
    Scene, SceneAction, SceneContext, SceneGame, SceneManager, SlideDirection, Transition,
  };

  // Engine builder
  pub use crate::core::engine::{EngineBuilder, WindowMode};

  // ECS types
  pub use crate::core::ecs::{
    Children, Entity, Material, Parent, RenderLayer, RenderMode, RenderStyle, Renderable,
    ShaderHandle, ShaderParams, SpriteEffect, SpriteShader, Transform, Velocity, World,
    WorldTransform,
  };

  // Math types (from glam)
  pub use crate::core::ecs::{Mat4, Vec2, Vec3, Vec4};

  // Physics
  pub use crate::core::physics::{PhysicsConfig, RigidBody};

  // Physics events (from Rapier integration)
  pub use crate::core::rapier_physics::{CollisionEventType, PhysicsCollisionEvent};

  // Collision
  pub use crate::core::collision::{
    AABB, Circle, Collider, ColliderShape, Collision, check_collision,
  };

  // Spatial partitioning
  pub use crate::core::spatial::{CollisionPair, CollisionResult, PhysicsWorld, SpatialHash};

  // Graphics
  pub use crate::core::sprite::{AnimatedSprite, Sprite};

  // Tiled rendering
  pub use crate::core::tile::{
    CompositeTile, CompositeTileBuilder, GroupedCollider, TileDirection, TilePart,
  };

  // Animation system
  pub use crate::core::animation::{
    Animation, AnimationController, AnimationId, Animator, LoopMode, PlaybackState,
  };

  // Assets
  pub use crate::core::asset::{
    AssetManager, AssetStats, AsyncSpriteHandle, LoadingState, SpriteHandle,
  };

  // Camera
  pub use crate::core::camera::Camera;

  // Input (re-export KeyCode for convenience)
  pub use crate::core::input::InputState;
  pub use winit::event::MouseButton;
  pub use winit::keyboard::KeyCode;

  // Audio
  pub use crate::core::audio::{
    AudioManager, MusicHandle, MusicOptions, SoundHandle, SoundOptions,
  };

  // Workspace
  pub use crate::core::workspace::{SystemDefaults, Workspace, WorkspaceConfig};

  // Save system
  pub use crate::core::save::{
    DisplaySettings, EntitySnapshot, GameSaveData, SaveManager, SystemState, WorldState,
    compress_data, decompress_data,
  };

  // UI system
  pub use crate::core::ui::{Anchor, BitmapFont, Color, Rect, UiContext, create_debug_font};

  // Post-processing effects
  pub use crate::core::shaders::{EffectTarget, EffectsManager, PostProcessEffect};

  // GPU sprite batching (for high-performance particle systems)
  pub use crate::core::sprite_gpu::{BatchCollector, BatchKey, SpriteBatch, SpriteInstance};

  // Error types (centralized)
  pub use crate::core::error::{
    AssetError, AssetResult, AudioError, AudioResult, EngineError, EngineResult, HotReloadError,
    HotReloadResult, SaveError, SaveResult, SpriteError, SpriteResult,
  };
}
