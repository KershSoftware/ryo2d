//! # Ryo2D
//!
//! A simple, scriptable 2D game engine built in Rust.
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
//! Ryo2D uses a simple Entity-Component-System (ECS) architecture:
//!
//! - **Entities** are unique IDs representing game objects
//! - **Components** are data attached to entities (Transform, Velocity, Renderable)
//! - **Systems** process entities with specific components (handled by the engine)
//!
//! You implement the [`core::scene::Scene`] trait to define your game's behavior.

pub mod cli;
pub mod core;

// Re-export puffin so that profile_scope!/profile_function! macros resolve
// the `::puffin` path in downstream crates without requiring a direct dependency.
#[cfg(feature = "profiler")]
pub use puffin;

/// The prelude module - import everything you need with `use ryo2d::prelude::*`
pub mod prelude {
  // Core game types
  pub use crate::core::game::{GpuRenderContext, TimeControl, TimeInfo, WindowRequest};

  // Scene management
  pub use crate::core::scene::{
    Scene, SceneAction, SceneCleanup, SceneContext, SceneManager, SlideDirection, Transition,
  };

  // Resources
  pub use crate::core::resources::Resources;

  // Engine builder
  pub use crate::core::engine::{EngineBuilder, WindowMode};

  // ECS types
  pub use crate::core::ecs::{
    Active, BlobShadow, Children, ContactShadow, DropShadow, Ease, Entity, Inactive, Knockback,
    Material, Parent, Pool, PoolConfig, PoolExhaustedPolicy, PoolStats, PreviousTransform,
    RenderLayer, RenderMode, RenderStyle, Renderable, ShaderHandle, ShaderParams, SpriteEffect,
    SpriteShader, SquashAxis, SquashStretch, Transform, Tween, TweenLoop, TweenSequence, Velocity,
    World, WorldTransform, YSort,
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

  // Parallax backgrounds (GPU-accelerated)
  pub use crate::core::graphics::TileMode;
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
    NetworkEvent, NetworkServer, NetworkServerConfig, ServerLike, TransportConfig,
  };

  // WebRTC transport (requires `matchbox` feature)
  #[cfg(feature = "matchbox")]
  pub use crate::core::network::{IceServer, WebRtcConfig};

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

  // Network Input (derive macro + helpers)
  pub use crate::core::network::input::{
    InputBuffer, InputProcessor, NetworkInput, NetworkInputTrait, NetworkedInputHandler,
    SequencedInput,
  };

  // Network Actions (dual local+network execution)
  pub use crate::core::network::action::{
    ActionContext, ActionError, NetworkAction, SerializedAction, apply_received_action,
    execute_action, execute_action_broadcast, execute_action_server, try_execute_action,
  };

  // Client-Side Prediction
  pub use crate::core::network::prediction::{
    Predicted, PredictionHelper, ServerPositionUpdate, UpdateResult,
  };

  // NetworkedValue wrapper
  pub use crate::core::network::replication::NetworkedValue;

  // Workspace & config
  pub use crate::core::config::{SystemDefaults, Workspace, WorkspaceConfig};

  // Save system
  pub use crate::core::config::{
    DisplayMode, EntitySnapshot, GameSaveData, Resolution, SaveManager, SystemState,
    UserPreference, UserPreferenceFilter, WorldState, compress_data, decompress_data,
    parse_resolution,
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

  // Skeletal animation
  pub use crate::core::skeleton::{
    BoneDef, BoneTransform, BoneWorldTransform, Pose, SkeletalAnimation, SkeletalAnimator,
    SkeletalBlender, SkeletalController, Skeleton, SkeletonDef, skeleton_debug_lines,
  };

  // Particle system
  pub use crate::core::particles::{
    AttractorType, CollisionConfig, CollisionResponse, EmissionMode, EmissionShape,
    ParticleAttractor, ParticleCollision, ParticleEmitter, ParticleSystem,
  };

  // Tilemap system
  pub use crate::core::tilemap::{
    AutoTileMode, AutoTileSet, ChunkCoord, ChunkState, CollisionRect, TileAnimation, TileId,
    TileProperties, TiledObject, TiledShape, TiledValue, Tilemap, TilemapError, TilemapLayer,
    TilemapManager, TilemapOrientation, Tileset, compute_bitmask_full, compute_bitmask_minimal,
    generate_collision_rects, generate_collision_rects_region, load_tiled_map,
  };

  // Raycasting (pseudo-3D)
  pub use crate::core::raycast::{
    CameraUniform, DoorSpeed, DoorState, GpuRaycastRenderer, MapCell, MapFlags, MapInfoUniform,
    RaycastCamera, RaycastDoor, RaycastLight, RaycastSprite, RaycastWorld, camera_follow_system,
    door_system,
  };

  // Texture management
  pub use crate::core::texture::{
    TextureArrayHandle, TextureContext, TextureHandle, TextureManager,
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

  // Developer Console
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
