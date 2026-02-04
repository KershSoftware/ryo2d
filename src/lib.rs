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
  pub use crate::core::game::{Game, GameContext, GpuRenderContext, TimeControl, WindowRequest};

  // Scene management
  pub use crate::core::scene::{
    Scene, SceneAction, SceneContext, SceneGame, SceneManager, SlideDirection, Transition,
  };

  // Engine builder
  pub use crate::core::engine::{EngineBuilder, WindowMode};

  // ECS types
  pub use crate::core::ecs::{
    Active, Children, Ease, Entity, Inactive, Knockback, Material, Parent, Pool, PoolConfig,
    PoolExhaustedPolicy, PoolStats, RenderLayer, RenderMode, RenderStyle, Renderable, ShaderHandle,
    ShaderParams, SpriteEffect, SpriteShader, SquashAxis, SquashStretch, Transform, Tween,
    TweenLoop, TweenSequence, Velocity, World, WorldTransform, YSort,
  };

  // Math types (from glam)
  pub use crate::core::ecs::{Mat4, Vec2, Vec3, Vec4};

  // Event Bus
  pub use crate::core::events::{
    AssetLoadFailed, AssetReloaded, ClientConnected, ClientDisconnected, CollisionStarted,
    CollisionStopped, ConnectedToServer, DisconnectReason, DisconnectedFromServer, Events,
    KnockbackFinished, KnockbackStarted, SquashStretchFinished, SquashStretchStarted, StateChanged,
  };

  // Physics components
  pub use crate::core::physics::{PhysicsConfig, RigidBody};

  // Physics events (from Rapier integration)
  pub use crate::core::physics::{CollisionEventType, PhysicsCollisionEvent};

  // Collision shapes
  pub use crate::core::physics::{
    AABB, Circle, Collider, ColliderShape, Collision, check_collision,
  };

  // Spatial partitioning
  pub use crate::core::physics::{CollisionPair, CollisionResult, PhysicsWorld, SpatialHash};

  // Rapier physics world
  pub use crate::core::physics::RapierPhysicsWorld;

  // Graphics
  pub use crate::core::graphics::{AnimatedSprite, Sprite};

  // Tiled rendering
  pub use crate::core::graphics::{
    CompositeTile, CompositeTileBuilder, GroupedCollider, TileDirection, TilePart,
  };

  // Animation system
  pub use crate::core::graphics::{
    Animation, AnimationController, AnimationId, Animator, LoopMode, PlaybackState,
    ValidatedAnimator,
  };

  // Camera
  pub use crate::core::graphics::Camera;

  // Parallax backgrounds
  pub use crate::core::graphics::{
    ParallaxBackground, ParallaxBackgroundBuilder, ParallaxLayer, TileMode,
  };

  // GPU-accelerated parallax (recommended for large images)
  pub use crate::core::graphics::{GpuParallaxBackground, GpuParallaxBuilder, GpuParallaxLayer};

  // Assets
  pub use crate::core::asset::{
    AssetManager, AssetStats, AsyncSpriteHandle, LoadingProgress, LoadingState, SpriteHandle,
  };

  // Input (re-export KeyCode for convenience)
  pub use crate::core::cursor::CursorStyle;
  pub use crate::core::input::{GamepadButton, InputState};
  pub use winit::event::MouseButton;
  pub use winit::keyboard::KeyCode;

  // Audio
  pub use crate::core::audio::{
    AudioManager, MusicHandle, MusicOptions, SoundHandle, SoundOptions,
  };

  // Networking
  pub use crate::core::network::{
    Channel, ClientId, LOCAL_HOST_ID, ListenServer, NetworkClient, NetworkClientConfig,
    NetworkEvent, NetworkServer, NetworkServerConfig, ServerLike,
  };

  // Network Replication
  pub use crate::core::network::replication::{
    Authority, ComponentBatchMessage, ComponentRegistry, ComponentTypeId, ComponentUpdateMessage,
    DespawnEntityMessage, DirtyTracker, FullStateSyncMessage, NetworkEntity, NetworkEntityBuilder,
    NetworkId, Replicated, ReplicationManager, ReplicationMessage, SerializedComponent,
    SpawnEntityMessage,
  };

  // RPC System
  pub use crate::core::network::rpc::{
    ReceivedRpc, Rpc, RpcRegistry, RpcTarget, RpcTypeId, SerializedRpc,
  };

  // Workspace & config
  pub use crate::core::config::{SystemDefaults, Workspace, WorkspaceConfig};

  // Save system
  pub use crate::core::config::{
    DisplayMode, DisplaySettings, EntitySnapshot, GameSaveData, Resolution, SaveManager,
    SystemState, WorldState, compress_data, decompress_data,
  };

  // UI system
  pub use crate::core::ui::{
    Anchor, BitmapFont, Color, DropdownState, FocusTrap, Focusable, HAlign, HBox, HotReloadTheme,
    Lerp, Rect, ScrollViewState, Stack, TextInputState, Theme, ThemeError, TweenColor, TweenFloat,
    TweenRect, TweenVec2, UiContext, UiTween, UiTweenLoop, VAlign, VBox, create_debug_font,
  };

  // 2D Lighting system
  pub use crate::core::lighting::{
    AmbientLight, Atmosphere, GodRaysQuality, Light2D, LightOccluder2D, LightingRenderer,
    VolumetricSettings,
  };

  // Post-processing effects
  pub use crate::core::shaders::{EffectTarget, EffectsManager, PostProcessEffect};

  // Trail and Line rendering
  pub use crate::core::trail::{
    Line2D, TextureMode, Trail2D, TrailPoint, TrailRenderer, TrailStyle,
  };

  // GPU sprite batching (for high-performance particle systems)
  pub use crate::core::graphics::{BatchCollector, BatchKey, SpriteBatch, SpriteInstance};

  // Render pipeline (for advanced users creating custom render passes)
  pub use crate::core::render::{RenderPass, RenderPipeline, pass_ids};

  // Error types (centralized)
  pub use crate::core::error::{
    AssetError, AssetResult, AudioError, AudioResult, EngineError, EngineResult, HotReloadError,
    HotReloadResult, NetworkError, NetworkResult, ReplicationError, ReplicationResult, SaveError,
    SaveResult, SpriteError, SpriteResult,
  };

  // Navigation & Pathfinding
  pub use crate::core::navigation::{
    CellState, GridCoord, NavAgent, NavAgentState, NavTarget, NavigationBuilder, NavigationGrid,
    Path, PathCompleted, PathFailed, PathFailureReason, PathInvalidated, PathNode, PathResult,
    PathSmoothing, PathStarted, Pathfinder, PathfinderConfig, steering,
  };

  // Procedural Generation
  pub use crate::core::procgen::{
    BspConfig, CellularConfig, Connection, GenerationCompleted, GenerationFailed,
    GenerationFailureReason, GenerationGrid, GenerationResult, GenerationStarted, GridRect,
    MstConfig, MstEdge, NoiseConfig, NoiseThreshold, NoiseType, PoissonConfig, PoissonResult,
    ProcgenBuilder, RandomWalkConfig, Region, RegionCreated, RegionMetadata, RegionType,
    RegionsConnected, SeededRng, TileType, WfcConfig, WfcDirection, WfcResult, WfcTile,
    poisson_points,
  };

  // State Machines
  pub use crate::core::state_machine::{
    StateMachine, StateStack, TransitionError, TransitionResult,
  };

  // Profiling (macros work as no-ops when `profiler` feature is disabled)
  pub use crate::core::profiler::{profile_function, profile_scope};

  // Logging macros for game code
  pub use crate::game_debug;
  pub use crate::game_error;
  pub use crate::game_info;
  pub use crate::game_trace;
  pub use crate::game_warn;
}
