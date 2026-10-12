//! # Ryo2D
//!
//! A 2D game engine built in Rust.
//!
//! ## Quick Start
//!
//! ```no_run
//! use ryo2d::prelude::*;
//!
//! struct MyScene;
//!
//! impl Scene for MyScene {
//!     fn on_enter(&mut self, ctx: &mut SceneContext) {
//!         // Set up your game world
//!     }
//!
//!     fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
//!         // Game logic runs here
//!         SceneAction::Continue
//!     }
//! }
//!
//! fn main() {
//!     EngineBuilder::new()
//!         .title("My Game")
//!         .resolution(800, 600)
//!         .start_scene(MyScene)
//!         .build()
//!         .run()
//!         .unwrap();
//! }
//! ```
//!
//! ## Architecture
//!
//! Ryo2D's ECS ([`ecs`], all in the prelude) holds the game state:
//!
//! - **Entities** are game objects: `ctx.world.spawn((Transform::from_xy(..), ..)).id()`
//! - **Components** are data on entities (`Transform`, `Renderable`, `RigidBody`, your own)
//! - **Resources** are world singletons (`Time`, `Input`, `Camera`, your own)
//! - **Systems** are functions over queries and resources, run in the engine's
//!   phases; gameplay runs at a fixed rate (`EngineBuilder::fixed_hz`)
//! - **Messages** carry buffered notifications between systems and scenes
//!
//! You implement the [`core::scene::Scene`] trait for your game's flow:
//! `update(ctx, dt)` each fixed step, `ui(ui, ctx)` once a frame and
//! `render(gpu)` for batched GPU sprites. Scenes add their own systems with
//! `ctx.add_systems`.

pub mod cli;
pub mod core;
pub use core::ecs;
pub use core::testing;

// Re-export puffin so that profile_scope!/profile_function! macros resolve
// the `::puffin` path in downstream crates without requiring a direct dependency.
#[cfg(feature = "profiler")]
pub use puffin;

/// The GPU types the engine renders with. Custom [`RenderPass`](prelude::RenderPass)es
/// use these (`ryo2d::wgpu`), so a game needs no `wgpu` dependency of its own.
pub use wgpu;

/// The crate behind `ryo2d::ecs`, re-exported under its own name only so the
/// derive macros in the prelude resolve from a game crate. Never name it.
#[doc(hidden)]
pub use bevy_ecs;

/// The prelude module - import everything you need with `use ryo2d::prelude::*`
pub mod prelude {
  // The ECS derives expand to paths rooted at the kernel crate's own name;
  // bringing that name into scope through this glob is what makes
  // `#[derive(Component)]` work in a game crate. Never name it in game code.
  #[doc(hidden)]
  pub use crate::bevy_ecs;

  // ECS: the kernel, Transform, the engine's components and glam's math types
  pub use crate::core::ecs::*;

  // Clocks and phases
  pub use crate::core::schedule::{FixedSet, Phase, PostUpdateSet, SceneScheduleError, UpdateSet};
  pub use crate::core::time::Time;

  // Scenes
  pub use crate::core::scene::{
    ConsoleScope, Scene, SceneAction, SceneCleanup, SceneContext, SlideDirection, Transition,
    downcast_return,
  };

  // Engine builder
  pub use crate::core::engine::{EngineBuilder, WindowMode};

  // Engine messages
  pub use crate::core::messages::{
    AssetLoadFailed, AssetReloaded, CollisionStarted, CollisionStopped, KnockbackFinished,
    KnockbackStarted, SquashStretchFinished, SquashStretchStarted, TweenCompleted, TweenStarted,
  };

  // Input (with winit's key and mouse button types)
  pub use crate::core::input::{GamepadButton, Input, InputEvent};
  pub use winit::event::MouseButton;
  pub use winit::keyboard::KeyCode;

  // Audio
  pub use crate::core::audio::{
    Audio, AudioManager, MusicHandle, MusicOptions, SoundHandle, SoundOptions,
  };

  // Graphics
  pub use crate::core::graphics::{AnimatedSprite, Camera, Gpu, GpuRenderContext, Sprite};

  // Tiled rendering
  pub use crate::core::graphics::{
    CompositeTile, CompositeTileBuilder, GroupedCollider, TileDirection, TilePart,
  };

  // Sprite animation
  pub use crate::core::graphics::{
    Animation, AnimationController, AnimationId, Animator, LoopMode, PlaybackState,
    ValidatedAnimator,
  };

  // Parallax backgrounds (GPU-accelerated)
  pub use crate::core::graphics::TileMode;
  pub use crate::core::graphics::{GpuParallaxBackground, GpuParallaxBuilder, GpuParallaxLayer};

  // GPU sprite batching (for high-performance particle systems)
  pub use crate::core::graphics::{BatchCollector, SpriteInstance};

  // Physics
  pub use crate::core::physics::{
    Collider, ColliderShape, CollisionEventType, PhysicsCollisionEvent, PhysicsConfig,
    RapierPhysicsWorld, RigidBody,
  };

  // 2D lighting
  pub use crate::core::lighting::{
    AmbientLight, Atmosphere, GodRaysQuality, Light2D, LightOccluder2D, LightingSettings,
    VolumetricSettings,
  };

  // Assets
  pub use crate::core::asset::{
    AssetManager, AssetStats, AsyncSpriteHandle, LoadingProgress, LoadingState, SpriteHandle,
  };

  // Window and cursor
  pub use crate::core::cursor::{CursorRequest, CursorStyle};
  pub use crate::core::window::{WindowInfo, WindowRequest};

  // Post-processing effects
  pub use crate::core::shaders::{EffectError, EffectsManager, PostProcessEffect};

  // Trail and line rendering
  pub use crate::core::trail::{
    Line2D, LineCap, LineJoin, TextureMode, Trail2D, TrailEffect, TrailPoint, TrailRenderer,
    TrailStyle,
  };

  // Skeletal animation
  pub use crate::core::skeleton::{
    BoneDef, BoneTransform, BoneWorldTransform, Pose, SkeletalAnimation, SkeletalAnimator,
    SkeletalBlender, SkeletalController, Skeleton, SkeletonDef, skeleton_debug_lines,
  };

  // Particles
  pub use crate::core::particles::{
    AttractorType, CollisionConfig, CollisionResponse, EmissionMode, EmissionShape,
    MIN_BOUNCE_EVENT_SPEED, ParticleAttractor, ParticleCollision, ParticleEmitter, ParticleSystem,
  };

  // Tilemaps
  pub use crate::core::tilemap::{
    AutoTileMode, AutoTileSet, ChunkCoord, ChunkState, CollisionRect, TileAnimation, TileId,
    TileProperties, TiledObject, TiledShape, TiledValue, Tilemap, TilemapError, TilemapLayer,
    TilemapManager, TilemapOrientation, Tileset, compute_bitmask_full, compute_bitmask_minimal,
    generate_collision_rects, generate_collision_rects_region, load_tiled_map,
  };

  // Texture management
  pub use crate::core::texture::{
    TextureArrayHandle, TextureContext, TextureHandle, TextureManager,
  };

  // Render pipeline
  pub use crate::core::render::{
    FrameInfo, RenderContext, RenderPass, RenderPipeline, RenderPipelineError, pass_ids,
  };

  // Workspace and config
  pub use crate::core::config::{SystemDefaults, Workspace, WorkspaceConfig};

  // Saves and preferences
  pub use crate::core::config::{
    DisplayMode, GameSaveData, Resolution, SaveManager, SystemState, UserPreference,
    UserPreferenceFilter, compress_data, decompress_data, parse_resolution,
  };

  // UI
  pub use crate::core::ui::{
    Anchor, BitmapFont, Color, DropdownState, FocusTrap, Focusable, HAlign, HBox, HotReloadTheme,
    Rect, ScrollViewState, Stack, TextInputState, Theme, ThemeError, TweenColor, TweenFloat,
    TweenRect, TweenVec2, UiContext, UiInput, UiTween, UiTweenLoop, VAlign, VBox,
    create_debug_font,
  };

  // Errors
  pub use crate::core::error::{
    AssetError, AssetResult, AudioError, AudioResult, EngineError, EngineResult, HotReloadError,
    HotReloadResult, SaveError, SaveResult, SpriteError, SpriteResult,
  };
  pub use crate::core::graphics::GpuError;

  // Navigation and pathfinding
  pub use crate::core::navigation::{
    CellState, GridCoord, NavAgent, NavAgentState, NavTarget, NavigationBuilder, NavigationGrid,
    Path, PathNode, PathResult, PathSmoothing, Pathfinder, PathfinderConfig, steering,
  };

  // State machines
  pub use crate::core::state_machine::{
    StateMachine, StateStack, TransitionError, TransitionResult,
  };

  // Networking
  pub use crate::core::network::{
    Channel, ClientId, ClientLike, LOCAL_HOST_ID, ListenServer, NetworkClient, NetworkClientConfig,
    NetworkError, NetworkEvent, NetworkResult, NetworkServer, NetworkServerConfig,
    ReplicationError, ReplicationResult, ServerLike, TransportConfig,
  };

  // WebRTC transport (requires the `matchbox` feature)
  #[cfg(feature = "matchbox")]
  pub use crate::core::network::{IceServer, WebRtcConfig};

  // Network replication
  pub use crate::core::network::replication::{
    Authority, ComponentBatchMessage, ComponentRegistry, ComponentTypeId, ComponentUpdateMessage,
    DespawnEntityMessage, DirtyTracker, FullStateSyncMessage, NetworkEntity, NetworkEntityBuilder,
    NetworkId, NetworkedValue, Replicated, ReplicationManager, ReplicationMessage,
    SerializedComponent, SpawnEntityMessage,
  };

  // RPC
  pub use crate::core::network::rpc::{
    ReceivedRpc, Rpc, RpcRegistry, RpcTarget, RpcTypeId, SerializedRpc,
  };

  // Network input (derive macro and helpers)
  pub use crate::core::network::input::{
    InputBuffer, InputProcessor, NetworkInput, NetworkInputTrait, NetworkedInputHandler,
    SequencedInput,
  };

  // Network actions (local and networked execution)
  pub use crate::core::network::action::{
    ActionContext, ActionError, NetworkAction, SerializedAction, apply_received_action,
    execute_action, execute_action_broadcast, execute_action_server, try_execute_action,
  };

  // Client-side prediction
  pub use crate::core::network::prediction::{
    Predicted, PredictionHelper, ServerPositionUpdate, UpdateResult,
  };

  // Profiling (macros are no-ops without the `profiler` feature)
  pub use crate::core::profiler::{profile_function, profile_scope};

  // Developer console
  pub use crate::core::console::{
    CommandContext, CommandRegistry, CommandResult, ConsoleCommandBuilder, ConsoleState,
  };

  // Logging macros for game code
  pub use crate::game_debug;
  pub use crate::game_error;
  pub use crate::game_info;
  pub use crate::game_trace;
  pub use crate::game_warn;

  // Common utilities re-exported for convenience
  pub use rand;
  pub use rand::Rng;
}
