//! The engine: world, schedules, renderers and the window.
//!
//! [`EngineBuilder`] resolves settings and builds an [`Engine`];
//! [`Engine::run`] opens a window (`window.rs`) and runs one frame
//! (`frame.rs`) per redraw. Each frame ends by drawing the world
//! (`extract.rs`).

mod extract;
mod frame;
mod slow_frame;
mod window;

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use web_time::Instant;

use serde::Serialize;
use serde::de::DeserializeOwned;

use gilrs::Gilrs;
use winit::event_loop::EventLoop;
#[cfg(target_arch = "wasm32")]
use winit::event_loop::EventLoopProxy;
use winit::window::Window;

use crate::core::asset::{AssetLoader, AssetServer, AssetSource, DiskSource, insert_registry};
use crate::core::audio::{AudioEngine, VolumeKind, audio_system, default_mixer};
use crate::core::capture::{AutoCaptureConfig, CaptureReport, CaptureState};
use crate::core::config::{DisplayMode, ProjectFile, UserPreference};
use crate::core::console::{Command, CommandSpec};
use crate::core::cursor::CursorRequest;
use crate::core::diagnostics::{
  DebugConfig, DebugStats, FrameTimings, OverlayMode, SmoothedDebugStats, SmoothedFrameTimings,
};
use crate::core::ecs::interpolate::snapshot_previous_transforms;
use crate::core::ecs::propagate::propagate_transforms;
use crate::core::ecs::{
  FallbackErrorHandler, IntoScheduleConfigs, Message, Mut, Resource, ScheduleSystem, World,
};
use crate::core::error::{EngineError, EngineResult};
use crate::core::gameplay::register_gameplay_systems;
use crate::core::graphics::camera_mut::{
  camera_shake_system, camera_view_system, spawn_main_camera,
};
use crate::core::graphics::parallax::advance_parallax;
use crate::core::graphics::{
  BatchCollector, Gpu, Presentation, ScreenMapping, SpriteGpuRenderer, WgpuRenderer,
};
use crate::core::input::Input;
use crate::core::input_script::InputScript;
use crate::core::lighting::{LightingRenderer, LightingSettings};
use crate::core::messages::{register_engine_messages, register_message};
use crate::core::particles::{
  PARTICLE_HEADLESS_SEED, ParticleStore, step_particles, step_unscaled_particles,
};
#[cfg(not(target_arch = "wasm32"))]
use crate::core::particles::{StepState, run_particle_step, run_unscaled_step};
use crate::core::physics::sync::register_physics_systems;
use crate::core::physics::{Physics, PhysicsConfig};
use crate::core::render::batch::Batch2D;
use crate::core::render::ui_renderer::UiRenderer;
use crate::core::render::{
  DrawList, EntityDrawer, RenderPipeline, UiResolution, UiSpace, begin_debug_fixed_step,
  end_debug_fixed_step,
};
use crate::core::saves::versions::VersionRegistry;
use crate::core::saves::{
  MemorySaves, QUIT_FLUSH_GIVE_UP, QUIT_FLUSH_WARN, SaveService, SaveVersions, drain_save_results,
};
use crate::core::scene::{PhysicsPaused, Scene, SceneManager};
use crate::core::schedule::{EngineSchedules, FixedSet, Phase, PostUpdateSet, UpdateSet};
use crate::core::settings::apply::AppliedSettings;
use crate::core::settings::file::{FORMAT as SETTINGS_FORMAT, SettingsFile};
use crate::core::settings::persist::SettingsWriter;
use crate::core::settings::sections::SectionRegistry;
use crate::core::settings::{EngineValues, Settings};
use crate::core::shaders::EffectsManager;
use crate::core::systems::{record_trail_points, register_juice_systems};
use crate::core::text::{GlyphRasterizer, TextEngine, advance_text_reveal};
use crate::core::time::Time;
use crate::core::ui::immediate::UiCommands;
use crate::core::ui::layout::engine::LayoutEngine;
use crate::core::ui::memory::UiMemory;
use crate::core::window::{WindowInfo, WindowRequest};

#[cfg(dev_tools)]
pub(crate) use extract::game_entity_count;
use frame::{
  advance_clock_system, asset_registry_system, clear_trackers_system, publish_input_system,
  scene_fixed_systems, scene_transition_system, scene_update_system, scene_update_systems,
  update_effects_system, update_messages_system,
};

/// Events the engine sends its own event loop.
// Only the web sends any
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) enum EngineEvent {
  /// The web's asynchronous GPU setup finished
  GpuReady(Box<Result<WgpuRenderer, crate::core::graphics::GpuError>>),
  /// The browser's save store opened, or did not within its time
  #[cfg(target_arch = "wasm32")]
  StoreLoaded(Box<Result<WebStore, crate::core::saves::SaveError>>),
}

/// The browser's save store, as it opens
#[cfg(target_arch = "wasm32")]
type WebStore = crate::core::saves::browser::BrowserStorage<crate::core::saves::idb::IdbStore>;

/// The web's start-up: the GPU and the browser's store, until both arrive
#[cfg(target_arch = "wasm32")]
type WebStartup = crate::core::saves::browser::StartupGate<
  Box<Result<WgpuRenderer, crate::core::graphics::GpuError>>,
  Box<Result<WebStore, crate::core::saves::SaveError>>,
>;

/// Configuration for the engine
#[derive(Debug, Clone)]
pub struct EngineConfig {
  pub title: String,
  pub width: u32,
  pub height: u32,
  pub target_fps: u32,
  /// Fixed updates per second (the fixed timestep is `1 / fixed_hz`)
  pub fixed_hz: u32,
  pub physics: PhysicsConfig,
  pub debug: DebugConfig,
  /// How the world reaches the window (`width`x`height` is the canvas in
  /// [`Presentation::Fit`] and the target in [`Presentation::PixelArt`])
  pub presentation: Presentation,
  /// The window's initial logical size; `None` picks one from the resolution
  pub window_size: Option<(u32, u32)>,
  /// Whether the player can resize the window (default `true`)
  pub resizable: bool,
  /// Display mode (windowed, fullscreen, borderless)
  pub display_mode: DisplayMode,
  /// Path to window icon (PNG). If None, uses embedded Ryo2D logo.
  pub icon_path: Option<std::path::PathBuf>,
  /// Render pixels per UI unit. `None` (the default) keeps UI units equal to
  /// screen units at any display scale; `Some(2.0)` draws the UI twice as
  /// big as one render pixel per unit would.
  pub ui_scale: Option<f32>,
  /// What resolution the UI image has (default `UiResolution::Render`)
  pub ui_resolution: UiResolution,
  /// Target FPS when the window is unfocused. 0 = use target_fps. Default: 0.
  pub target_background_fps: u32,
  /// Whether to keep drawing when the window is unfocused (the game keeps
  /// running either way). Default: true.
  pub background_rendering: bool,
  /// The most particles alive at once, over every emitter (default
  /// [`DEFAULT_PARTICLE_BUDGET`](crate::core::particles::DEFAULT_PARTICLE_BUDGET))
  pub particle_budget: u32,
}

impl Default for EngineConfig {
  fn default() -> Self {
    EngineConfig {
      title: String::from("Ryo2D"),
      width: 1280,
      height: 720,
      target_fps: 60,
      fixed_hz: 60,
      physics: PhysicsConfig::default(),
      debug: DebugConfig::default(),
      presentation: Presentation::default(),
      window_size: None,
      resizable: true,
      display_mode: DisplayMode::default(),
      icon_path: None,
      ui_scale: None,
      ui_resolution: UiResolution::default(),
      target_background_fps: 0,
      background_rendering: true,
      particle_budget: crate::core::particles::DEFAULT_PARTICLE_BUDGET,
    }
  }
}

/// The scene stack. Its scenes are not `Send`, so it lives in the world as
/// a non-send resource that the scene systems take out while they run.
pub(crate) struct SceneHost {
  pub(crate) scenes: SceneManager,
}

/// A resource or message registration the builder defers to engine start.
type WorldSetup = Box<dyn FnOnce(&mut World)>;

/// A scene (or the console) asked to quit; the frame loop stops.
#[derive(Resource)]
pub(crate) struct QuitRequested;

/// What sets the frame rate: with vsync on the display's refresh rate
/// (`target_fps` is not used), with it off `target_fps`
pub(crate) fn frame_rate_summary(vsync: bool, target_fps: u32) -> String {
  if vsync {
    "vsync on: frames follow the display's refresh rate".to_string()
  } else {
    format!("vsync off: up to {target_fps} fps")
  }
}

/// The main engine struct
pub struct Engine {
  config: EngineConfig,
  window: Option<Arc<Window>>,
  renderer: Option<WgpuRenderer>,
  world: World,
  schedules: EngineSchedules,
  // GPU renderers the passes draw with (engine-owned, not resources)
  pub(crate) sprite_gpu: Option<SpriteGpuRenderer>,
  pub(crate) batch: Option<Batch2D>,
  /// The UI's own renderer (beside the batch, which it shares the UI
  /// camera's bind group with)
  ui_renderer: Option<UiRenderer>,
  lighting: Option<LightingRenderer>,
  entity_drawer: EntityDrawer,
  pub(crate) tile_drawer: crate::core::render::tiles::TileDrawer,
  /// The engine's particles, one run per emitter (its query kept)
  particle_drawer: crate::core::particles::ParticleDrawer,
  // Text layout (synced with `Fonts`), the glyph rasterizer for that
  // registry and the text warnings already given
  text: TextEngine,
  glyph_raster: GlyphRasterizer,
  pub(crate) text_warned: std::collections::HashSet<u64>,
  // Each camera's uniform buffer, kept across frames
  camera_uniforms: crate::core::graphics::camera_gpu::CameraUniforms,
  // The frame's cameras in render order (reused across frames), the
  // texture cameras left out for want of their target, and those already
  // warned about
  camera_draws: Vec<crate::core::render::CameraDraw>,
  // Extraction's buffers, emptied and refilled each frame (borrowed items
  // are recycled into the frame's lifetime)
  camera_globals: Vec<(
    crate::core::ecs::Entity,
    crate::core::graphics::camera_gpu::CameraGlobals,
  )>,
  lights: Vec<crate::core::lighting::FrameLight>,
  occluders: Vec<(
    crate::core::ecs::GlobalTransform,
    &'static crate::core::lighting::LightOccluder2D,
    crate::core::ecs::Floor,
  )>,
  /// The occluders' query, made the first lit frame
  occluder_query: Option<crate::core::ecs::kernel::QueryState<extract::OccluderItem>>,
  parallax: Vec<crate::core::graphics::SpriteInstance>,
  poly_points: Vec<crate::core::render::batch::PolyPoint>,
  targetless_cameras: Vec<crate::core::ecs::Entity>,
  warned_targetless: std::collections::HashSet<crate::core::ecs::Entity>,
  draw_list: DrawList,
  batch_collector: BatchCollector,
  /// Where the renderer has read the images' events to (their `Removed`
  /// free textures, their `Modified` start new ones), and the freed and
  /// changed ids of a frame (reused)
  image_events: crate::core::ecs::kernel::MessageCursor<
    crate::core::asset::AssetEvent<crate::core::graphics::Image>,
  >,
  freed_images: Vec<crate::core::asset::AssetId<crate::core::graphics::Image>>,
  changed_images: Vec<crate::core::asset::AssetId<crate::core::graphics::Image>>,
  /// Where the renderer has read the shaders' events to (their `Removed`
  /// free pipelines), and the freed ids of a frame (reused)
  shader_events: crate::core::ecs::kernel::MessageCursor<
    crate::core::asset::AssetEvent<crate::core::ecs::Shader>,
  >,
  freed_shaders: Vec<crate::core::asset::AssetId<crate::core::ecs::Shader>>,
  /// The checker drawn in place of an image that is not loaded (debug
  /// builds), added at startup
  missing_image: crate::core::asset::Handle<crate::core::graphics::Image>,
  // UI: the frame's recorded draws and the UI space they are in
  ui_commands: UiCommands,
  /// What the UI sizes images by, brought in line with the world's store
  /// at the start of each UI pass
  ui_images: crate::core::ui::image_sizes::ImageSizes,
  ui_space: UiSpace,
  /// UI render retention: what the UI image holds
  ui_retain: crate::core::render::ui_retain::UiRetain,
  /// The draws the last frame's `ui` pass was given (0 when it did not run)
  ui_draw_count: u32,
  /// Mouse buttons whose press the UI took: until released, their held
  /// state and release are the UI's too
  ui_mouse_capture: Vec<winit::event::MouseButton>,
  /// Bindings of the UI actions the UI used (a direction, Confirm, Back,
  /// Tab, a tab switch): until released, gameplay never sees them
  ui_binding_capture: Vec<crate::core::input::Binding>,
  /// What input prompts read (the last device, the bindings, the game's
  /// glyphs), refreshed before each UI pass and read by world text
  prompts: std::sync::Arc<crate::core::ui::immediate::PromptContext>,
  /// When `TextIcons` last changed as `prompts` copied it
  prompt_icons: Option<crate::core::ecs::kernel::Tick>,
  /// What the UI remembers between frames: records, widget state, focus
  ui_memory: UiMemory,
  /// The `debug:ui-rebuilds` overlay's outlines
  ui_rebuilds: crate::core::ui::debug::RebuildsOverlay,
  /// Lays the UI's containers out, kept between frames
  ui_layout: LayoutEngine,
  /// The query `reload_trees` keeps over the trees built from UI files
  ui_sources: crate::core::ui::nodes::SourceQuery,
  /// The `UiTheme` generation whose skin images are loaded
  theme_generation: Option<u64>,
  /// The `UiTheme` generation whose sounds are loaded
  sounds_generation: Option<u64>,
  /// What the theme's UI sounds play, by event (`UiSound::ALL`'s order)
  ui_sounds: [Option<crate::core::ui::ThemeSoundSource>; 5],
  /// How many of this frame's UI sounds were played already (a rerun pass
  /// appends to the frame's list)
  sounds_played: usize,
  /// UI passes run this frame (a scene switched from `ui` runs a second)
  ui_pass_index: u32,
  /// What the frame's UI pass did besides drawing, for headless snapshots
  #[cfg(not(target_arch = "wasm32"))]
  ui_pass_log: frame::UiPassLog,
  /// The console's cursor override this frame (`None` = no opinion); the
  /// console itself is the world's `Console`
  #[cfg(dev_tools)]
  console_cursor: Option<crate::core::cursor::CursorStyle>,
  /// Which cursor the window shows: the console's override, else the game's
  cursor_resolver: crate::core::cursor::CursorResolver,
  /// The left button was released: show the cursor again (macOS applies a
  /// new icon lazily)
  cursor_refresh: bool,
  pub(crate) capture: CaptureState,
  input_script: InputScript,
  frame_timings: FrameTimings,
  /// The last frame's UI pass on the CPU, its rerun passes included
  #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
  ui_pass_time: std::time::Duration,
  smoothed_timings: SmoothedFrameTimings,
  overlay_mode: OverlayMode,
  debug_stats: DebugStats,
  smoothed_stats: SmoothedDebugStats,
  debug_keys_enabled: bool,
  last_fps_log: Instant,
  // The dev windows (native, a build with the tools; the profiler's with
  // its feature)
  #[cfg(all(feature = "profiler", not(target_arch = "wasm32")))]
  profiler_window: Option<crate::core::windows::profiler_window::ProfilerWindow>,
  #[cfg(all(dev_tools, not(target_arch = "wasm32")))]
  logger_window: Option<crate::core::windows::logger_window::LoggerWindow>,
  game_window_focused: bool,
  #[cfg(all(feature = "profiler", not(target_arch = "wasm32")))]
  profiler_window_focused: bool,
  #[cfg(all(dev_tools, not(target_arch = "wasm32")))]
  logger_window_focused: bool,
  #[cfg(all(feature = "profiler", not(target_arch = "wasm32")))]
  pending_open_profiler: bool,
  #[cfg(all(dev_tools, not(target_arch = "wasm32")))]
  pending_open_logger: bool,
  pending_toggle_borderless: bool,
  // Window and input plumbing
  gilrs: Option<Gilrs>,
  custom_cursor_cache: HashMap<String, winit::window::CustomCursor>,
  cursor_locked: bool,
  /// The game window is hidden (covered, on another space, minimised):
  /// frames run but are not drawn
  window_occluded: bool,
  /// Frames run since the engine started, to tell the startup frames apart
  frames_run: u64,
  /// The scene manager's entry count after the last frame
  last_scene_entries: u64,
  /// A slow frame's warning, held until the window is known to have stayed up
  slow_frame: slow_frame::SlowFrameHold,
  surface_needs_reconfigure: bool,
  scale_factor: f64,
  // Why the window's GPU setup failed, returned from `run`
  startup_error: Option<crate::core::graphics::GpuError>,
  // Delivers the web's asynchronous GPU setup and save store to the event loop
  #[cfg(target_arch = "wasm32")]
  proxy: Option<EventLoopProxy<EngineEvent>>,
  /// The first scene waits for both
  #[cfg(target_arch = "wasm32")]
  startup: WebStartup,
  last_window_size: (u32, u32),
  // The window has a zero side: frames run but nothing is drawn
  minimized: bool,
  vsync: bool,
  /// A vsync setting to apply to the renderer after the frame
  pending_vsync: Option<bool>,
  /// The engine's settings as last applied
  applied_settings: AppliedSettings,
  /// Built headless (`HeadlessEngine`, `RYO_HEADLESS`): no wall-clock overlays
  headless: bool,
  /// Where a windowed native engine keeps its log file (the save folder)
  #[cfg(not(target_arch = "wasm32"))]
  log_folder: Option<std::path::PathBuf>,
  /// The log scope of the thread that built the engine: its frames log in
  /// it, and so do the threads it spawns (0 in a game; a test's capture)
  log_scope: u64,
  /// Run the frame's logic but do not draw it (an unfocused window with
  /// background rendering off)
  skip_present: bool,
  // Headless runs (`RYO_HEADLESS`), and the wall time each of their steps
  // takes at least (`RYO_HEADLESS_HZ`)
  offscreen_run: bool,
  offscreen_refused: bool,
  offscreen_pace: Option<std::time::Duration>,
  /// The game's name and version, for the console's `system:version`
  #[cfg(dev_tools)]
  game_name: Option<String>,
  #[cfg(dev_tools)]
  game_version: Option<String>,
  /// The name the browser keeps the saves under
  #[cfg(target_arch = "wasm32")]
  store_name: String,
  /// The store a headless engine (or a builder's `memory_saves`) keeps saves
  /// in, for `HeadlessEngine::saves`
  #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
  memory_saves: Option<MemorySaves>,
}

/// Result of rendering and presenting one frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PresentOutcome {
  /// The frame reached the screen (or offscreen target)
  Presented,
  /// Nothing was presented (surface outdated/lost, or no renderer yet)
  Skipped,
  /// Rendering failed in a way the window loop treats as fatal
  Fatal(String),
}

/// Result of one engine frame (logic + render).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FrameOutcome {
  Presented,
  Skipped,
  /// The scene or console asked to quit; nothing was rendered
  Quit,
  /// Rendering failed fatally
  Fatal(String),
}

/// `RYO_CAPTURE_*` and `RYO_INPUT` from the environment, warnings reported;
/// the last value is whether any of them was invalid
fn scripted_run_from_env() -> (AutoCaptureConfig, InputScript, bool) {
  let (capture, warnings) = AutoCaptureConfig::from_env();
  for warning in &warnings {
    log::warn!("{}", warning);
  }
  let (script, warning) = InputScript::from_env();
  let invalid = !warnings.is_empty() || warning.is_some();
  if let Some(warning) = warning {
    log::warn!("{}", warning);
  }
  (capture, script, invalid)
}

/// `RYO_HEADLESS_HZ` from the environment, its warning reported: the wall
/// time each step of a headless run takes at least, and whether the value
/// was invalid
fn headless_pace_from_env() -> (Option<std::time::Duration>, bool) {
  let (pace, warning) = crate::core::capture::headless_pace_from_env();
  if let Some(warning) = &warning {
    log::warn!("{}", warning);
  }
  (pace, warning.is_some())
}

/// Systems that fail panic in debug builds and log in release builds.
fn handle_system_error(error: crate::core::ecs::SystemError, ctx: crate::core::ecs::ErrorContext) {
  if cfg!(debug_assertions) {
    crate::core::ecs::kernel::panic_on_error(error, ctx);
  } else {
    log::error!("{ctx}: {error}");
  }
}

/// Insert the resources scene callbacks and systems expect (the GPU ones
/// follow in `Engine::init_gpu`). The particle store holds the config's
/// budget, its emitters' engine seeds drawn from `particle_seed`.
pub(crate) fn init_world_resources(
  world: &mut World,
  config: &EngineConfig,
  registry: AssetServer,
  audio: AudioEngine,
  particle_seed: u64,
) {
  let (w, h) = (config.width, config.height);
  world.insert_resource(Time::new(config.fixed_hz));
  world.insert_resource(Input::new());
  world.insert_resource(ScreenMapping::new(config.presentation, (w, h), 1.0));
  spawn_main_camera(world, glam::Vec2::new(w as f32, h as f32));
  insert_registry(world, registry);
  world.insert_resource(audio);
  world.insert_resource(ParticleStore::new(config.particle_budget, particle_seed));
  world.insert_resource(EffectsManager::new());
  world.insert_resource(crate::core::render::Look::default());
  world.insert_resource(LightingSettings::default());
  world.insert_resource(crate::core::graphics::RenderTargets::default());
  world.insert_resource(crate::core::render::DebugDraw::default());
  world.insert_resource(WindowRequest::default());
  world.insert_resource(CursorRequest::default());
  world.insert_resource(WindowInfo {
    scale_factor: 1.0,
    display_mode: config.display_mode,
  });
  world.insert_resource(RenderPipeline::default_2d());
  crate::core::text::insert_fonts(world);
  world.insert_resource(crate::core::text::TextIcons::default());
  world.insert_resource(crate::core::text::TextEffects::default());
  world.insert_resource(crate::core::ui::UiTheme::default());
  world.insert_resource(PhysicsPaused(false));
  // Shared by map landings and the map drawer from the first frame, so a
  // map spawned entering the first scene is padded off the frame too
  world.insert_resource(crate::core::tilemap::padded::PaddedCache::default());
  world.insert_resource(frame::PendingTickCheck::default());
  world.add_observer(frame::record_tick_check);
  world.insert_resource(config.physics.clone());
  world.insert_resource(Physics::new());
  #[cfg(dev_tools)]
  world.insert_resource(crate::core::console::Console::new());
  world.insert_resource(FallbackErrorHandler(handle_system_error));
  register_engine_messages(world);
}

/// The engine's own systems, in their phases and sets.
fn register_engine_systems(schedules: &mut EngineSchedules) {
  schedules.add_systems(
    Phase::First,
    (
      advance_clock_system,
      publish_input_system,
      update_messages_system,
    )
      .chain(),
  );
  schedules.add_systems(
    Phase::PreUpdate,
    // The pump after the transition: in a switch's frame, what the leaving scene let go is
    // freed and what the entering one asked for is pumped before the new scene's UI pass.
    // Maps spawn after the pump, so a map whose asset lands this frame spawns this frame,
    // and a spawned map follows a saved file in the frame its reload lands; then streamed
    // maps load and unload chunks round their anchors, and class tiles follow the chunks
    // loaded and unloaded, before anything draws or collides
    (
      scene_transition_system,
      asset_registry_system,
      crate::core::tilemap::spawn_maps,
      crate::core::tilemap::reload_maps,
      crate::core::tilemap::stream_maps,
      crate::core::tilemap::follow_chunk_swaps,
    )
      .chain(),
  );
  schedules.add_systems(Phase::PreUpdate, drain_save_results);
  schedules.add_systems(
    Phase::FixedUpdate,
    (begin_debug_fixed_step, snapshot_previous_transforms)
      .chain()
      .in_set(FixedSet::Snapshot),
  );
  register_juice_systems(schedules);
  schedules.add_systems(
    Phase::FixedUpdate,
    scene_update_system.in_set(FixedSet::Scene),
  );
  schedules.add_systems(
    Phase::FixedUpdate,
    scene_fixed_systems.in_set(FixedSet::Game),
  );
  register_physics_systems(schedules);
  register_gameplay_systems(schedules);
  crate::core::tilemap::register_tilemap_systems(schedules);
  schedules.add_systems(
    Phase::FixedUpdate,
    propagate_transforms.in_set(FixedSet::Transforms),
  );
  schedules.add_systems(
    Phase::FixedUpdate,
    step_particles.in_set(FixedSet::Particles),
  );
  schedules.add_systems(
    Phase::FixedUpdate,
    (record_trail_points, end_debug_fixed_step)
      .chain()
      .in_set(FixedSet::Trails),
  );
  schedules.add_systems(Phase::Update, scene_update_systems.in_set(UpdateSet::Game));
  schedules.add_systems(
    Phase::PostUpdate,
    (
      propagate_transforms.in_set(PostUpdateSet::Transforms),
      (camera_shake_system, camera_view_system)
        .chain()
        .in_set(PostUpdateSet::Camera),
      (advance_parallax, advance_text_reveal, update_effects_system)
        .chain()
        .in_set(PostUpdateSet::Extract),
      // Unscaled emitters, on the frame's real time
      step_unscaled_particles.in_set(PostUpdateSet::Extract),
      // After the scene's update and the cameras (the listener)
      audio_system.after(PostUpdateSet::Camera),
    ),
  );
  // Removal events last two tracker clears, and the fixed steps may not run
  // that often: physics tracks removals (including lost bodies and parents,
  // whose colliders the next step gives a new owner) and re-enables every
  // frame too, before the trackers are cleared. Additions and edits are
  // tick-based and wait for the next fixed step.
  schedules.add_systems(
    Phase::Last,
    (
      crate::core::physics::sync::forget_removed,
      crate::core::physics::sync::track_disabled,
      clear_trackers_system,
    )
      .chain(),
  );
}

impl Engine {
  fn new(resolved: ResolvedBuild, headless: bool) -> Self {
    let ResolvedBuild {
      mut config,
      initial_scene,
      settings,
      audio_buffer_frames,
      tiled_project,
      #[cfg(dev_tools)]
      game_name,
      #[cfg(dev_tools)]
      game_version,
      #[cfg(target_arch = "wasm32")]
      store_name,
      #[cfg(not(target_arch = "wasm32"))]
        store_name: _,
      #[cfg(dev_tools)]
      console_setup,
      pending_resources,
      mut schedules,
      saved_bindings,
      workspace,
      asset_source,
      hot_reload,
      settings_writer,
      saves,
      memory_saves,
    } = resolved;

    // The log is installed before anything logs; another `log` logger set
    // first is kept, and said once on stderr
    crate::core::logging::ensure();
    let log_scope = crate::core::logging::current_scope();
    if headless {
      crate::core::logging::headless_run();
    }

    let initial_size = (config.width, config.height);
    #[cfg(not(target_arch = "wasm32"))]
    let log_folder = workspace
      .as_ref()
      .filter(|_| !headless)
      .map(crate::core::config::Workspace::save_location);

    // Tell players and developers where settings and saves are kept
    if !headless && let Some(workspace) = &workspace {
      log::info!("Save data: {}", workspace.save_location().display());
    }

    // Assets load from the project's asset folder unless the game gave a source
    let registry = AssetServer::with_source(asset_source.unwrap_or_else(|| {
      let root = workspace.as_ref().map_or_else(
        || std::path::PathBuf::from("assets"),
        crate::core::config::Workspace::assets_dir,
      );
      Box::new(DiskSource::new(root))
    }));
    // Changed files reload only while hot reload is on: otherwise no watcher starts
    if hot_reload {
      registry.start_watching();
    }

    // Initialize gamepad input (gracefully handle no gamepad support)
    let gilrs = if headless {
      None
    } else {
      match Gilrs::new() {
        Ok(g) => {
          log::info!("Initialized successfully");
          Some(g)
        }
        Err(e) => {
          log::warn!(
            "Could not initialize gamepad support: {}. Continuing without gamepads.",
            e
          );
          None
        }
      }
    };

    if headless {
      // Wall-clock overlays would make headless frames differ run to run
      config.debug.show_fps = false;
      config.debug.show_frame_timing = false;
      config.debug.show_stats = false;
      config.debug.logger_window = false;
      config.debug.dev_window_keys = false;
    }

    let debug_keys_enabled = !config.debug.disable_debug_keys && !headless;
    let initial_overlay_mode = OverlayMode::from_flags(
      config.debug.show_fps,
      config.debug.show_frame_timing,
      config.debug.show_stats,
    );
    let initial_ui_space = UiSpace::of(
      &ScreenMapping::new(config.presentation, initial_size, 1.0),
      config.ui_scale,
      config.ui_resolution,
    );

    // The built-ins, then the builder's console calls in the order made: a
    // builder command named like a built-in replaces it
    #[cfg(dev_tools)]
    let console_commands = {
      use crate::core::console::{ConsoleCommands, Handler, add_builtins};
      let mut commands = ConsoleCommands::default();
      add_builtins(&mut commands);
      for setup in console_setup {
        match setup {
          ConsoleSetup::Add(spec, handler) => commands.add(spec, Handler::World(handler)),
          ConsoleSetup::Remove(name) => {
            commands.remove(&name);
          }
        }
      }
      commands
    };

    let (capture_config, input_script) = if headless {
      (AutoCaptureConfig::default(), InputScript::default())
    } else {
      let (capture, script, _) = scripted_run_from_env();
      (capture, script)
    };

    let mut world = World::new();
    // On the device, or silently on kira's mock backend when headless or
    // when the device fails to open, at the player's volume settings
    let mut audio_engine = AudioEngine::open(!headless, audio_buffer_frames);
    // A headless run picks clips and draws gains and pitches the same way
    // each time, so its captures and tests repeat
    if headless {
      audio_engine.seed(crate::core::audio::HEADLESS_SEED);
    }
    // And seeds its particle emitters the same way each time
    let particle_seed = if headless {
      PARTICLE_HEADLESS_SEED
    } else {
      rand::random()
    };
    for kind in VolumeKind::ALL {
      audio_engine.set_volume_setting(kind, settings.volume(kind));
    }
    let vsync = settings.vsync();
    let applied_settings = AppliedSettings::new(*settings.values());
    init_world_resources(&mut world, &config, registry, audio_engine, particle_seed);
    #[cfg(dev_tools)]
    {
      world.insert_resource(console_commands);
      // Everything the ring holds of this engine's scope: the builder's
      // warnings, and those of the game's `main`
      let (seq, mark) = crate::core::logging::mirror_start(log_scope);
      world
        .resource_mut::<crate::core::console::Console>()
        .start_log(seq, mark);
    }
    world.insert_resource(saves);
    world.insert_resource(settings);
    if let Some(writer) = settings_writer {
      world.insert_resource(writer);
    }
    if let Some(project) = tiled_project {
      let server = world.resource::<AssetServer>();
      server.register_loader(crate::core::tilemap::TiledMapLoader::with_project(
        project.clone(),
      ));
      server.register_loader(crate::core::tilemap::TiledTilesetLoader::with_project(
        project,
      ));
    }
    // The game's mixer builds the bus tree once it loads; without one, the
    // standard layout plays
    let mixer = default_mixer(world.resource::<AssetServer>());
    world.resource_mut::<AudioEngine>().set_mixer(mixer);
    // Drawn in place of an image that is not loaded (debug builds); held,
    // so it lives as long as the engine
    let missing_image =
      crate::core::asset::AssetsView::new(&mut world).add(crate::core::graphics::Image::checker());
    for insert in pending_resources {
      insert(&mut world);
    }
    if let Some(saved) = &saved_bindings {
      world
        .resource_mut::<Input>()
        .actions_mut()
        .load_saved(saved);
    }
    world.insert_non_send(SceneHost {
      scenes: SceneManager::with_scene(initial_scene),
    });
    register_engine_systems(&mut schedules);

    Engine {
      config,
      window: None,
      renderer: None,
      world,
      schedules,
      sprite_gpu: None,
      batch: None,
      ui_renderer: None,
      lighting: None,
      entity_drawer: EntityDrawer::default(),
      tile_drawer: Default::default(),
      particle_drawer: Default::default(),
      text: TextEngine::new(),
      glyph_raster: GlyphRasterizer::new(),
      text_warned: Default::default(),
      camera_uniforms: Default::default(),
      camera_draws: Vec::new(),
      camera_globals: Vec::new(),
      lights: Vec::new(),
      occluders: Vec::new(),
      occluder_query: None,
      parallax: Vec::new(),
      poly_points: Vec::new(),
      targetless_cameras: Vec::new(),
      warned_targetless: Default::default(),
      draw_list: DrawList::default(),
      batch_collector: BatchCollector::new(),
      image_events: Default::default(),
      freed_images: Vec::new(),
      changed_images: Vec::new(),
      shader_events: Default::default(),
      freed_shaders: Vec::new(),
      missing_image,
      ui_commands: UiCommands::default(),
      ui_images: Default::default(),
      ui_space: initial_ui_space,
      ui_retain: Default::default(),
      ui_draw_count: 0,
      ui_mouse_capture: Vec::new(),
      ui_binding_capture: Vec::new(),
      prompts: Default::default(),
      prompt_icons: None,
      ui_memory: UiMemory::new(),
      ui_rebuilds: Default::default(),
      ui_layout: LayoutEngine::new(),
      ui_sources: Default::default(),
      // None: the starting theme's skins load on the first pass
      theme_generation: None,
      sounds_generation: None,
      ui_sounds: Default::default(),
      sounds_played: 0,
      ui_pass_index: 0,
      #[cfg(not(target_arch = "wasm32"))]
      ui_pass_log: frame::UiPassLog::default(),
      #[cfg(dev_tools)]
      console_cursor: None,
      cursor_resolver: crate::core::cursor::CursorResolver::default(),
      cursor_refresh: false,
      capture: CaptureState::new(capture_config),
      input_script,
      frame_timings: FrameTimings::default(),
      ui_pass_time: std::time::Duration::ZERO,
      smoothed_timings: SmoothedFrameTimings::default(),
      overlay_mode: initial_overlay_mode,
      debug_stats: DebugStats::default(),
      smoothed_stats: SmoothedDebugStats::default(),
      debug_keys_enabled,
      last_fps_log: Instant::now(),
      #[cfg(all(feature = "profiler", not(target_arch = "wasm32")))]
      profiler_window: None,
      #[cfg(all(dev_tools, not(target_arch = "wasm32")))]
      logger_window: None,
      game_window_focused: true,
      #[cfg(all(feature = "profiler", not(target_arch = "wasm32")))]
      profiler_window_focused: false,
      #[cfg(all(dev_tools, not(target_arch = "wasm32")))]
      logger_window_focused: false,
      #[cfg(all(feature = "profiler", not(target_arch = "wasm32")))]
      pending_open_profiler: false,
      #[cfg(all(dev_tools, not(target_arch = "wasm32")))]
      pending_open_logger: false,
      pending_toggle_borderless: false,
      gilrs,
      custom_cursor_cache: HashMap::new(),
      cursor_locked: false,
      window_occluded: false,
      frames_run: 0,
      last_scene_entries: 0,
      slow_frame: slow_frame::SlowFrameHold::default(),
      surface_needs_reconfigure: false,
      scale_factor: 1.0,
      startup_error: None,
      #[cfg(target_arch = "wasm32")]
      proxy: None,
      #[cfg(target_arch = "wasm32")]
      startup: WebStartup::new(),
      last_window_size: initial_size,
      minimized: false,
      vsync,
      pending_vsync: None,
      applied_settings,
      headless,
      #[cfg(not(target_arch = "wasm32"))]
      log_folder,
      log_scope,
      skip_present: false,
      offscreen_run: false,
      offscreen_refused: false,
      offscreen_pace: None,
      #[cfg(dev_tools)]
      game_name,
      #[cfg(dev_tools)]
      game_version,
      #[cfg(target_arch = "wasm32")]
      store_name,
      memory_saves,
    }
  }

  // =============================================================================
  // Main Loop
  // =============================================================================

  /// Run the engine
  ///
  /// With `RYO_HEADLESS=1` in a dev build this renders offscreen instead of
  /// opening a window (see [`capture`](crate::core::capture)).
  pub fn run(mut self) -> EngineResult<()> {
    #[cfg(not(target_arch = "wasm32"))]
    if self.offscreen_run {
      return self.run_offscreen();
    }

    #[cfg(not(target_arch = "wasm32"))]
    self.open_log_file();

    #[cfg(dev_tools)]
    Self::print_startup_banner(&self.config, self.vsync, self.debug_keys_enabled);

    log::info!("Initializing {}...", self.config.title);
    log::info!("Resolution: {}x{}", self.config.width, self.config.height);
    log::info!(
      "Frame rate: {}",
      frame_rate_summary(self.vsync, self.config.target_fps)
    );

    let event_loop = EventLoop::<EngineEvent>::with_user_event()
      .build()
      .map_err(|e| EngineError::Window(format!("{}", e)))?;

    // The browser owns the loop: hand the engine to it and return
    #[cfg(target_arch = "wasm32")]
    {
      use winit::platform::web::EventLoopExtWebSys;
      console_error_panic_hook::set_once();
      self.proxy = Some(event_loop.create_proxy());
      event_loop.spawn_app(self);
      Ok(())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
      let ran = event_loop
        .run_app(&mut self)
        .map_err(|e| EngineError::Window(format!("Event loop error: {}", e)));
      // Lines logged after the loop's last event reach the file too
      crate::core::logging::flush_log_file();
      ran?;
      match self.startup_error.take() {
        Some(error) => Err(EngineError::Gpu(error)),
        None => Ok(()),
      }
    }
  }

  /// A windowed native engine keeps the log in its save folder and writes a
  /// panic to it. A file that cannot be had is said once and the game runs
  /// without it.
  #[cfg(not(target_arch = "wasm32"))]
  fn open_log_file(&self) {
    let Some(folder) = &self.log_folder else {
      crate::core::logging::no_log_file();
      return;
    };
    if let Some(path) = crate::core::logging::open_log_file(folder) {
      log::info!("Log file: {}", path.display());
    }
  }

  /// Make [`run`](Self::run) render offscreen with these schedules (`RYO_HEADLESS`).
  pub(crate) fn prepare_offscreen_run(&mut self, capture: AutoCaptureConfig, script: InputScript) {
    self.capture = CaptureState::new(capture);
    self.input_script = script;
    self.offscreen_run = true;
  }

  /// `RYO_HEADLESS`: run without a window, one fixed update per frame.
  #[cfg(not(target_arch = "wasm32"))]
  fn run_offscreen(mut self) -> EngineResult<()> {
    // A scripted run with a typo would capture the wrong thing or never end
    if self.offscreen_refused {
      log::error!("not running: fix the invalid RYO_* values above");
      std::process::exit(2);
    }
    log::info!(
      "{} at {}x{}, no window",
      self.config.title,
      self.config.width,
      self.config.height
    );
    if !self.capture.exits_after_last() {
      log::info!("no RYO_CAPTURE_EXIT=1 with RYO_CAPTURE_FRAMES: running until the scene quits");
    }
    if let Some(pace) = self.offscreen_pace {
      log::info!(
        "paced: each step takes at least {:.2} ms of wall time",
        pace.as_secs_f64() * 1000.0
      );
    }
    if self.run_offscreen_frames()? {
      std::process::exit(1);
    }
    Ok(())
  }

  /// The offscreen loop: step until the scene quits or `RYO_CAPTURE_EXIT` is
  /// met, then flush captures. Returns whether a scheduled capture failed.
  ///
  /// Each step advances the game by the fixed timestep whatever the wall
  /// time; with a pace (`RYO_HEADLESS_HZ`) a step that finishes early sleeps
  /// out the rest of it, so work off the main thread (a map's landing, an
  /// asset's load) gets the wall time a windowed run would give it.
  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn run_offscreen_frames(&mut self) -> EngineResult<bool> {
    let (device, queue) =
      crate::core::testing::shared_gpu().map_err(|e| EngineError::Render(e.to_string()))?;
    let mapping = ScreenMapping::new(self.config.presentation, self.game_size(), 1.0);
    self.init_gpu(
      WgpuRenderer::new_offscreen_mapped(
        device,
        queue,
        mapping,
        wgpu::TextureFormat::Rgba8UnormSrgb,
      ),
      1.0,
    );

    let dt = self.fixed_timestep();
    let mut fatal = None;
    loop {
      let started = std::time::Instant::now();
      let outcome = self.frame(Some(dt));
      self.apply_headless_requests();
      let reports = self.capture.poll_writes();
      self.report_captures(reports);
      match outcome {
        FrameOutcome::Quit => break,
        FrameOutcome::Fatal(error) => {
          fatal = Some(error);
          break;
        }
        FrameOutcome::Presented | FrameOutcome::Skipped => {}
      }
      if self.capture.exit_requested() {
        break;
      }
      if let Some(rest) = self
        .offscreen_pace
        .and_then(|pace| pace.checked_sub(started.elapsed()))
      {
        std::thread::sleep(rest);
      }
    }

    // However the loop ended, write the settings, finish in-flight writes
    // and report frames that were never presented
    self.flush_settings();
    self.flush_saves();
    let reports = self.capture.finish();
    self.report_captures(reports);
    if let Some(error) = fatal {
      return Err(EngineError::Render(error));
    }
    Ok(self.capture.exit_with_failure())
  }

  /// Print startup banner to console with useful info.
  #[cfg(dev_tools)]
  fn print_startup_banner(config: &EngineConfig, vsync: bool, debug_keys: bool) {
    let debug = &config.debug;
    let version = env!("CARGO_PKG_VERSION");

    println!();
    println!("  {} | Ryo2D v{}", config.title, version);
    println!(
      "  {}x{} | {}",
      config.width,
      config.height,
      frame_rate_summary(vsync, config.target_fps)
    );
    println!();
    if debug.dev_console {
      println!("  `      Developer console");
    }
    if debug_keys {
      println!("  F1-F4  Debug overlays");
      println!("  F5     Toggle borderless");
    }
    if debug.dev_window_keys {
      println!("  F9     Logger");
      #[cfg(feature = "profiler")]
      println!("  F10    Profiler");
      println!("  F12    Capture frame");
    }
    println!();
  }

  /// The fixed logic timestep.
  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn fixed_timestep(&self) -> std::time::Duration {
    self.world.resource::<Time>().fixed.timestep_duration()
  }

  /// Writes the settings file now if what it keeps changed (the game quits
  /// or is hidden), with the bindings changed since the last frame. On the
  /// way out it runs before `flush_saves`, which then waits for it too
  pub(crate) fn flush_settings(&mut self) {
    if !self.world.contains_resource::<SettingsWriter>() {
      return;
    }
    let bindings = self.changed_bindings();
    self.with_settings_writer(|writer, settings, saves| {
      if let Some(map) = bindings {
        writer.set_bindings(map);
      }
      writer.flush_now(settings, saves);
    });
  }

  /// Runs `f` on the settings writer with the settings and the saves; does
  /// nothing in a build that keeps no settings
  pub(crate) fn with_settings_writer(
    &mut self,
    f: impl FnOnce(&mut SettingsWriter, &Settings, &SaveService),
  ) {
    self
      .world
      .try_resource_scope(|world, mut writer: Mut<SettingsWriter>| {
        if let (Some(settings), Some(saves)) = (
          world.get_resource::<Settings>(),
          world.get_resource::<SaveService>(),
        ) {
          f(&mut writer, settings, saves);
        }
      });
  }

  /// The player's bindings that differ from the defaults, when they changed
  /// since the last call
  pub(crate) fn changed_bindings(&mut self) -> Option<BTreeMap<String, Vec<String>>> {
    let mut input = self.world.get_resource_mut::<Input>()?;
    let actions = input.actions_mut();
    actions.take_dirty().then(|| actions.saved_map())
  }

  /// Finishes the queued save writes as the engine quits: warns after 3 s
  /// and gives up after 10 s. Called on every way out; once the writes are
  /// done it returns at once
  pub(crate) fn flush_saves(&self) {
    if let Some(saves) = self.world.get_resource::<SaveService>() {
      saves.flush(QUIT_FLUSH_WARN, QUIT_FLUSH_GIVE_UP);
    }
  }

  /// The browser's store arrived (the web's start-up, before the first
  /// scene): the saves keep their files in it, and the settings file is read
  /// from it and applied as `Engine::new` applies it. A store that did not
  /// open leaves saves unavailable, and the settings at their defaults for
  /// the session
  #[cfg(any(target_arch = "wasm32", test))]
  pub(crate) fn store_arrived(
    &mut self,
    store: Result<Arc<dyn crate::core::saves::storage::Storage>, crate::core::saves::SaveError>,
  ) {
    let read = {
      let Some(saves) = self.world.get_resource::<SaveService>() else {
        return;
      };
      if !crate::core::saves::browser::install(saves, store)
        || !self.world.contains_resource::<SettingsWriter>()
      {
        return;
      }
      // The web never kept a file of an older format, whose `env` is
      // compared with the project's
      read_settings_file(saves, &serde_json::Map::new())
    };
    let mut settings = self.world.resource_mut::<Settings>();
    let (writer, bindings) = apply_settings_file(read, &mut settings);
    let values = *settings.values();
    self.world.insert_resource(writer);
    if let Some(mut audio) = self.world.get_resource_mut::<AudioEngine>() {
      for (kind, volume) in VolumeKind::ALL.into_iter().zip(values.volumes) {
        audio.set_volume_setting(kind, volume);
      }
    }
    if let Some(saved) = &bindings {
      self
        .world
        .resource_mut::<Input>()
        .actions_mut()
        .load_saved(saved);
    }
    self.vsync = values.vsync;
    if !self.headless {
      let debug = &mut self.config.debug;
      debug.show_fps = values.show_fps;
      self.overlay_mode =
        OverlayMode::from_flags(values.show_fps, debug.show_frame_timing, debug.show_stats);
    }
    // The page sizes the canvas, and fullscreen waits for the player's
    // gesture, so the window is not asked again for what the file holds
    self.applied_settings = AppliedSettings::new(values);
  }

  /// The store the saves are kept in, when it is memory (headless)
  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn memory_saves(&self) -> Option<&MemorySaves> {
    self.memory_saves.as_ref()
  }

  /// The window (or offscreen target) is now `width x height` physical
  /// pixels. A zero side (minimised) skips frames until it comes back.
  pub(crate) fn resize_window(&mut self, width: u32, height: u32) {
    self.minimized = width == 0 || height == 0;
    if self.minimized {
      return;
    }
    self.last_window_size = (width, height);
    let mapping = ScreenMapping::new(self.config.presentation, (width, height), self.scale_factor);
    self.apply_mapping(mapping);
  }

  /// Follow a new screen mapping: the UI space, the renderer's targets
  /// (the UI image among them), the lighting projection and the camera's
  /// viewport (position, zoom and shake are kept).
  fn apply_mapping(&mut self, mapping: ScreenMapping) {
    let Some(renderer) = &mut self.renderer else {
      return;
    };
    self.ui_space = UiSpace::of(&mapping, self.config.ui_scale, self.config.ui_resolution);
    renderer.set_mapping(mapping, self.ui_space.image);
    self.world.insert_resource(mapping);
  }

  /// Recompute the UI space for the current mapping (after a UI-scale
  /// change; the UI image keeps its size)
  pub(super) fn refresh_ui_space(&mut self) {
    if let Some(renderer) = &self.renderer {
      let mapping = renderer.mapping();
      self.ui_space = UiSpace::of(&mapping, self.config.ui_scale, self.config.ui_resolution);
    }
  }

  /// How the world reaches the window.
  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn presentation(&self) -> Presentation {
    self.config.presentation
  }

  /// The pointer moved to `(x, y)` physical window pixels: queue it in screen
  /// space (over the bars too, outside `0..screen_size`).
  pub(crate) fn cursor_moved(&mut self, x: f64, y: f64) {
    let position = match &self.renderer {
      Some(renderer) => renderer
        .mapping()
        .window_to_screen(glam::Vec2::new(x as f32, y as f32)),
      None => glam::Vec2::new(x as f32, y as f32),
    };
    self
      .world
      .resource_mut::<Input>()
      .queue(crate::core::input::InputEvent::MouseMove {
        x: f64::from(position.x),
        y: f64::from(position.y),
      });
  }

  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn game_size(&self) -> (u32, u32) {
    (self.config.width, self.config.height)
  }

  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn world(&self) -> &World {
    &self.world
  }

  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn world_mut(&mut self) -> &mut World {
    &mut self.world
  }

  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn input_mut(&mut self) -> crate::core::ecs::Mut<'_, Input> {
    self.world.resource_mut::<Input>()
  }

  /// The UI's memory: its records, focus and widget state
  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn ui_memory(&self) -> &UiMemory {
    &self.ui_memory
  }

  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn ui_memory_mut(&mut self) -> &mut UiMemory {
    &mut self.ui_memory
  }

  /// What the UI's render step did in the last frame
  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn ui_render_stats(&self) -> crate::core::render::UiRenderStats {
    self.ui_retain.stats
  }

  /// The draws the last frame's `ui` pass was given
  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn ui_draw_count(&self) -> u32 {
    self.ui_draw_count
  }

  /// Take the debug keys and follow the FPS setting as a windowed engine
  /// does, every tool key on (`HeadlessEngine::windowed_debug_keys`)
  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn windowed_debug_keys(&mut self) {
    self.headless = false;
    self.debug_keys_enabled = !self.config.debug.disable_debug_keys;
    self.config.debug.dev_window_keys = true;
    self.config.debug.dev_console = true;
  }

  /// Whether F5 asked for borderless fullscreen (applied with a window)
  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn borderless_toggle_pending(&self) -> bool {
    self.pending_toggle_borderless
  }

  /// The debug switches, for a test to turn an overlay on
  #[cfg(all(test, not(target_arch = "wasm32")))]
  pub(crate) fn debug_config_mut(&mut self) -> &mut DebugConfig {
    &mut self.config.debug
  }

  /// How many pipelines game shaders have: the batch's for sprite shaders
  /// and both effect stages' for custom effects
  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn shader_pipeline_count(&self) -> usize {
    let sprites = self
      .batch
      .as_ref()
      .map_or(0, |batch| batch.custom_pipeline_count());
    let effects = self
      .renderer
      .as_ref()
      .map_or(0, |renderer| renderer.custom_effect_pipeline_count());
    sprites + effects
  }

  /// Whether game shader `id` waits to be drawn at its current version, by
  /// the sprite renderer or an effect stage: its new version is not yet
  /// drawn with a pipeline the GPU accepted
  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn shader_reload_pending(
    &self,
    id: crate::core::asset::AssetId<crate::core::ecs::Shader>,
  ) -> bool {
    let shaders = crate::core::ecs::shaders_of(&self.world);
    let sprites = self
      .sprite_gpu
      .as_ref()
      .is_some_and(|gpu| gpu.custom_shader_pending(id, shaders));
    let effects = self
      .renderer
      .as_ref()
      .is_some_and(|renderer| renderer.custom_effect_pending(id, shaders));
    sprites || effects
  }

  /// The gates of the threads game shaders compile on: the batch's and
  /// both effect stages' (tests)
  #[cfg(test)]
  pub(crate) fn shader_compile_gates(
    &self,
  ) -> Vec<std::sync::Arc<crate::core::shaders::compile::CompileGate>> {
    let mut gates = Vec::new();
    if let Some(batch) = self.batch.as_ref() {
      gates.push(batch.compile_gate());
    }
    if let Some(renderer) = self.renderer.as_ref() {
      gates.extend(renderer.compile_gates());
    }
    gates
  }

  /// Compiles sent to the shader threads so far, and how many are out
  /// (tests)
  #[cfg(test)]
  pub(crate) fn shader_compiles(&self) -> (usize, usize) {
    let (sprites, out) = self
      .batch
      .as_ref()
      .map_or((0, false), |batch| batch.compiles());
    let (effects, effects_out) = self
      .renderer
      .as_ref()
      .map_or((0, 0), |renderer| renderer.effect_compiles());
    (sprites + effects, usize::from(out) + effects_out)
  }

  /// The last frame's render step on the CPU (building, uploading and
  /// encoding; not the UI pass)
  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn render_time(&self) -> std::time::Duration {
    self.frame_timings.render + self.frame_timings.present
  }

  /// The last frame's UI pass: recording the scenes' UI, the node walk,
  /// layout and resolve, the overlays and the console (not the render side)
  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn ui_pass_time(&self) -> std::time::Duration {
    self.ui_pass_time
  }

  /// What the particle systems took when they last ran, on the CPU: the
  /// fixed step and the unscaled pass together, and the drawer (the
  /// `cpu:particles` and `render:particles` scopes)
  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn particle_times(&self) -> (std::time::Duration, std::time::Duration) {
    self
      .world
      .get_resource::<ParticleStore>()
      .map_or(Default::default(), |store| {
        (store.times.step + store.times.unscaled, store.times.draw)
      })
  }

  /// One run of the particle systems outside a frame (the fixed step, the
  /// unscaled pass and the drawer), on `step` and `unscaled` as their kept
  /// states, as an idle frame runs them (`benches/particles.rs`)
  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn run_particle_systems(&mut self, step: &mut StepState, unscaled: &mut StepState) {
    run_particle_step(&mut self.world, step);
    run_unscaled_step(&mut self.world, unscaled);
    if let Some(gpu) = self.sprite_gpu.as_mut() {
      self.particle_drawer.collect(
        &mut self.world,
        &mut self.draw_list,
        gpu,
        0.0,
        &self.camera_draws,
      );
    }
  }

  /// The UI commands the last frame recorded
  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn ui_commands(&self) -> &UiCommands {
    &self.ui_commands
  }

  /// What the last frame's UI pass did besides drawing
  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn ui_pass_log(&self) -> &frame::UiPassLog {
    &self.ui_pass_log
  }

  /// The top scene's scope id (`None` with no scene)
  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn top_scope(&self) -> Option<u64> {
    self
      .world
      .get_non_send::<SceneHost>()
      .and_then(|host| host.scenes.top_scope())
  }

  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn read_frame(&self) -> crate::core::error::CaptureResult<image::RgbaImage> {
    match &self.renderer {
      Some(renderer) => renderer.read_scene(),
      None => Err(crate::core::error::CaptureError::Readback(
        "renderer not initialized".to_string(),
      )),
    }
  }

  /// The presented window, bars included (offscreen renderers only).
  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn read_window(&self) -> crate::core::error::CaptureResult<image::RgbaImage> {
    match &self.renderer {
      Some(renderer) => renderer.read_frame(),
      None => Err(crate::core::error::CaptureError::Readback(
        "renderer not initialized".to_string(),
      )),
    }
  }

  /// Finish GPU-dependent setup once a renderer exists (window or offscreen).
  ///
  /// Creates the sprite, UI and lighting renderers and the batch, inserts the
  /// `Gpu` resource and the profiler, then enters the initial scene.
  pub(crate) fn init_gpu(&mut self, renderer: WgpuRenderer, scale_factor: f64) {
    let format = renderer.game_texture_format();
    // Sprites sample as their filter says, by default nearest for pixel art
    // and smooth at the window's resolution (the batch resolves it)
    let sprite_gpu = SpriteGpuRenderer::new(renderer.device(), renderer.queue());
    let batch = Batch2D::new(renderer.device(), format, self.config.presentation);
    let ui_renderer = UiRenderer::new(renderer.device(), batch.camera_layout());
    // Each camera is lit in light targets of its own size
    let lighting = LightingRenderer::new(renderer.device(), format);
    let mapping = renderer.mapping();

    self.scale_factor = scale_factor;
    self.world.resource_mut::<WindowInfo>().scale_factor = scale_factor;
    self
      .world
      .resource_mut::<Input>()
      .set_scale_factor(scale_factor);
    self.world.insert_resource(Gpu {
      device: renderer.device_arc(),
      queue: renderer.queue_arc(),
    });
    // Tilesets pad into images the device's textures hold
    if let Some(cache) = self
      .world
      .get_resource::<crate::core::tilemap::padded::PaddedCache>()
    {
      let side = renderer.device().limits().max_texture_dimension_2d;
      cache.lock().set_texture_limit(side);
    }

    self.renderer = Some(renderer);
    self.sprite_gpu = Some(sprite_gpu);
    self.batch = Some(batch);
    self.ui_renderer = Some(ui_renderer);
    self.lighting = Some(lighting);
    self.last_window_size = mapping.physical_size();
    self.apply_mapping(mapping);

    // Enables puffin scope collection so F10 can open the profiler later
    // (a no-op without the `profiler` feature)
    crate::core::profiler::init(self.config.debug.enable_profiler);

    frame::with_host(&mut self.world, |world, host| {
      host.scenes.enter_initial(world);
      host.scenes.sync_pauses(world);
    });

    log::info!("Engine initialized successfully!");
    log::info!(
      "Game loop using fixed timestep: {}Hz updates",
      self.config.fixed_hz
    );
  }

  /// Advance the capture schedule after a frame reached the screen.
  fn on_frame_presented(&mut self) {
    let result = self
      .renderer
      .as_mut()
      .and_then(WgpuRenderer::take_capture_result);
    let reports = self.capture.on_presented(result);
    self.report_captures(reports);
  }

  /// Log capture outcomes to stdout/stderr, the dev log and the console.
  fn report_captures(&mut self, reports: Vec<CaptureReport>) {
    for report in reports {
      match report {
        CaptureReport::Written { frame, path } => {
          let message = format!("frame {frame} → {}", path.display());
          log::info!("{}", message);
          #[cfg(dev_tools)]
          if let Some(mut console) = self
            .world
            .get_resource_mut::<crate::core::console::Console>()
          {
            console.print_system(format!("Captured {message}"));
          }
        }
        CaptureReport::Failed { frame, error } => {
          let message = format!("FAILED frame {frame}: {error}");
          log::warn!("{}", message);
          #[cfg(dev_tools)]
          if let Some(mut console) = self
            .world
            .get_resource_mut::<crate::core::console::Console>()
          {
            console.print_error(format!("Capture {message}"));
          }
        }
      }
    }
  }

  /// The stats of the last frame (tests, and the headless engine's reads)
  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn debug_stats(&self) -> &DebugStats {
    &self.debug_stats
  }

  /// The log scope its frames log in (the headless engine's log feed)
  #[cfg(all(dev_tools, not(target_arch = "wasm32")))]
  pub(crate) fn log_scope(&self) -> u64 {
    self.log_scope
  }

  /// The UI renderer, between frames (tests)
  #[cfg(test)]
  pub(crate) fn ui_renderer(&self) -> Option<&UiRenderer> {
    self.ui_renderer.as_ref()
  }

  /// Per-camera GPU state: the uniform buffers, the camera images and the
  /// cameras' light targets.
  #[cfg(test)]
  pub(crate) fn camera_gpu_entries(&self) -> usize {
    let images = self
      .renderer
      .as_ref()
      .map_or(0, WgpuRenderer::camera_image_count);
    let lights = self
      .lighting
      .as_ref()
      .map_or(0, LightingRenderer::camera_count);
    self.camera_uniforms.len() + images + lights
  }
}

impl Drop for Engine {
  fn drop(&mut self) {
    // A setting changed or a save queued just before the engine goes still lands
    self.flush_settings();
    self.flush_saves();
  }
}

/// Builder pattern for Engine configuration
pub struct EngineBuilder {
  /// Builder-only settings (target FPS, fixed rate, window mode, ...).
  /// Settings other sources can also set live in `overrides`.
  config: EngineConfig,
  /// Settings set by builder calls, layered over project values in `resolve`
  overrides: crate::core::config::resolve::BuilderOverrides,
  initial_scene: Option<Box<dyn Scene>>,
  /// The console calls made on the builder, in order
  #[cfg(dev_tools)]
  console_setup: Vec<ConsoleSetup>,
  /// Filters passed to `with_user_preferences`, combined when the engine is built
  user_preferences: Vec<crate::core::config::UserPreferenceFilter>,
  /// Resources, message types and asset loaders the game registers,
  /// applied after the engine's own
  pending_resources: Vec<WorldSetup>,
  /// The game's engine-wide systems
  schedules: EngineSchedules,
  /// Where assets load from (`None`: the project's asset folder)
  asset_source: Option<Box<dyn AssetSource>>,
  /// Whether changed asset files reload (`None`: in windowed debug builds)
  hot_reload: Option<bool>,
  /// The saved types' versions, registered as `save_versions` is called
  save_versions: VersionRegistry,
  /// Where the saves are kept instead of the save folder
  memory_saves: Option<MemorySaves>,
  /// The game's Ryo.toml compiled in, for where none is found on disk
  project_file: ProjectFile,
  /// The game's settings sections, registered as `game_settings` is called
  sections: SectionRegistry,
}

impl EngineBuilder {
  /// A builder with the engine's defaults. It installs the engine's log
  /// first, so what the build logs (Ryo.toml, the save folder, the settings
  /// file) reaches every output, even before the game logs anything.
  pub fn new() -> Self {
    crate::core::logging::ensure();
    EngineBuilder {
      config: EngineConfig::default(),
      overrides: Default::default(),
      initial_scene: None,
      #[cfg(dev_tools)]
      console_setup: Vec::new(),
      user_preferences: Vec::new(),
      pending_resources: Vec::new(),
      schedules: EngineSchedules::new(),
      asset_source: None,
      hot_reload: None,
      save_versions: VersionRegistry::default(),
      memory_saves: None,
      project_file: ProjectFile::none(),
      sections: SectionRegistry::default(),
    }
  }

  pub fn title(mut self, title: impl Into<String>) -> Self {
    self.overrides.title = Some(title.into());
    self
  }

  /// Set the game's resolution: screen space is `width`x`height`, drawn at
  /// the window's resolution and scaled by any factor to fit it, centred
  /// with bars where the aspect ratios differ ([`Presentation::Fit`]; with
  /// `pixel_art = true` in Ryo.toml, the pixel-art target). Without one the
  /// game is resolution independent. Overrides Ryo.toml.
  pub fn resolution(mut self, width: u32, height: u32) -> Self {
    self.overrides.resolution = Some((width, height));
    self
  }

  /// Cap the frame rate when vsync is off.
  pub fn target_fps(mut self, fps: u32) -> Self {
    self.config.target_fps = fps;
    self
  }

  /// Set the target FPS when the window is unfocused. 0 = use target_fps.
  ///
  /// Overrides Ryo.toml.
  pub fn background_fps(mut self, fps: u32) -> Self {
    self.overrides.background_fps = Some(fps);
    self
  }

  /// Set whether to keep drawing when the window is unfocused (the game keeps
  /// running either way).
  ///
  /// Overrides Ryo.toml.
  pub fn background_rendering(mut self, enabled: bool) -> Self {
    self.overrides.background_rendering = Some(enabled);
    self
  }

  /// Set whether VSync is enabled.
  ///
  /// - `true` - VSync on (Fifo: frames wait for the display, no tearing)
  /// - `false` - VSync off (Mailbox or Immediate, capped at `target_fps`)
  ///
  /// Default: `true`. Overrides Ryo.toml; a saved `vsync` in system.json overrides this when the
  /// [`with_user_preferences`](Self::with_user_preferences) filter includes `UserPreference::Vsync`.
  pub fn vsync(mut self, enabled: bool) -> Self {
    self.overrides.vsync = Some(enabled);
    self
  }

  /// Fixed updates per second (default 60): `Scene::update`, physics and
  /// every `FixedUpdate` system run at this rate. Clamped to at least 1.
  pub fn fixed_hz(mut self, hz: u32) -> Self {
    self.config.fixed_hz = hz.max(1);
    self
  }

  /// Draw pixel art: the world renders into a `width`x`height` target that
  /// is scaled by a whole number to fit the window, centred with bars.
  /// Sets the resolution too. Overrides Ryo.toml.
  pub fn pixel_art(mut self, width: u32, height: u32) -> Self {
    self.overrides.pixel_art = Some(true);
    self.overrides.resolution = Some((width, height));
    self
  }

  /// The window's initial logical size (by default the resolution, shrunk to
  /// fit 1280x720 when larger, or for pixel art the largest whole multiple
  /// that fits 1280x720). A saved
  /// resolution preference overrides it when the `with_user_preferences`
  /// filter includes it.
  pub fn window_size(mut self, width: u32, height: u32) -> Self {
    self.overrides.window_size = Some((width, height));
    self
  }

  /// Whether the player can resize the window (default `true`).
  pub fn resizable(mut self, resizable: bool) -> Self {
    self.config.resizable = resizable;
    self
  }

  /// Set the display mode (windowed, fullscreen, or borderless).
  ///
  /// Overrides Ryo.toml. A saved preference overrides this when the
  /// `with_user_preferences` filter includes it.
  pub fn display_mode(mut self, mode: DisplayMode) -> Self {
    self.overrides.display_mode = Some(mode);
    self
  }

  /// Render pixels per UI unit; `None` (the default) keeps UI units equal
  /// to screen units at any display scale. `Some(2.0)` draws the UI, rects
  /// and text alike, twice as big as one render pixel per unit would; mouse
  /// positions are converted to match. A value that is not a positive
  /// number is warned about and treated as `None`.
  pub fn ui_scale(mut self, scale: Option<f32>) -> Self {
    self.config.ui_scale = scale;
    self
  }

  /// The most particles alive at once, over every emitter (default
  /// [`DEFAULT_PARTICLE_BUDGET`](crate::core::particles::DEFAULT_PARTICLE_BUDGET),
  /// 20,000). A safety cap, not a quality setting: a spawn past it is
  /// dropped, with one warning per effect, and live particles are never cut
  /// short.
  pub fn particle_budget(mut self, max: u32) -> Self {
    self.config.particle_budget = max;
    self
  }

  /// Whether UI node trees replay their unchanged subtrees instead of
  /// building them every frame (default true). Off, they draw and behave
  /// the same, slower: for ruling caching out when chasing a UI bug.
  pub fn ui_cache(mut self, on: bool) -> Self {
    self.config.debug.ui_cache = on;
    self
  }

  /// Whether UI node trees keep the layout and resolved output of unchanged
  /// subtrees between frames, copying them instead of replaying them
  /// (default true; only while `ui_cache` is on). Off, unchanged subtrees
  /// are replayed and resolved every frame: the same output, slower. For
  /// ruling fragments out when chasing a UI bug.
  pub fn ui_fragments(mut self, on: bool) -> Self {
    self.config.debug.ui_fragments = on;
    self
  }

  /// Whether the renderer keeps the UI image while the UI is unchanged and
  /// reuses the glyphs of unchanged texts (default true). Off, the whole UI
  /// is built and drawn every frame: the same pixels, slower. For ruling
  /// retention out when chasing a rendering bug.
  pub fn ui_retain(mut self, on: bool) -> Self {
    self.config.debug.ui_retain = on;
    self
  }

  /// What resolution the UI image has: `UiResolution::Render` (the default)
  /// draws the UI at the render size, so in pixel-art mode it is pixel art
  /// too; `UiResolution::Window` draws it at the window's resolution.
  pub fn ui_resolution(mut self, resolution: UiResolution) -> Self {
    self.config.ui_resolution = resolution;
    self
  }

  /// Set a custom window icon from a PNG file (default: the Ryo2D logo).
  pub fn icon(mut self, path: impl Into<std::path::PathBuf>) -> Self {
    self.overrides.icon = Some(path.into());
    self
  }

  /// Show collider outlines. Overrides Ryo.toml.
  pub fn show_colliders(mut self, enabled: bool) -> Self {
    self.overrides.show_colliders = Some(enabled);
    self
  }

  /// Show the FPS counter. Overrides `Ryo.toml`; a saved user preference
  /// (see [`with_user_preferences`](Self::with_user_preferences)) overrides this.
  pub fn show_fps(mut self, enabled: bool) -> Self {
    self.overrides.show_fps = Some(enabled);
    self
  }

  /// Show velocity vectors. Overrides Ryo.toml.
  pub fn show_velocities(mut self, enabled: bool) -> Self {
    self.overrides.show_velocities = Some(enabled);
    self
  }

  /// Show entity origins. Overrides Ryo.toml.
  pub fn show_origins(mut self, enabled: bool) -> Self {
    self.overrides.show_origins = Some(enabled);
    self
  }

  /// Chooses which of the player's settings (`Settings`, `ctx.settings()`)
  /// the settings file, `system.json`, keeps; the others last for the
  /// session.
  ///
  /// Startup settings resolve in one order, each layer overriding only what it
  /// sets: engine defaults → `Ryo.toml` (dev) or the values `ryo2d build`
  /// embedded from it (release) → `EngineBuilder` calls. Those are the game's
  /// defaults, and the player's file applies over the settings in the filter
  /// (only the keys it holds). The file keeps only what the player changed,
  /// so a default changed later reaches every setting the player never
  /// touched. It is written a second after the last change to what it keeps,
  /// and at once when the game quits or is hidden. Without a filter or a game
  /// settings section ([`game_settings`](Self::game_settings)), `system.json`
  /// is never read or written. A headless build keeps it in its memory store,
  /// never on disk. Repeated calls combine their filters.
  pub fn with_user_preferences(
    mut self,
    filter: impl Into<crate::core::config::UserPreferenceFilter>,
  ) -> Self {
    // Read when the engine is built, so headless builds can skip it entirely
    self.user_preferences.push(filter.into());
    self
  }

  /// Start with `theme` as the UI theme (`ctx.ui_theme()` changes it later;
  /// `ctx.follow_ui_theme(handle)` follows a `.theme.ron` file through its
  /// reloads).
  pub fn ui_theme(self, theme: crate::core::ui::Theme) -> Self {
    self.insert_resource(crate::core::ui::UiTheme::new(theme))
  }

  /// Insert a resource every scene and system can reach
  /// (`ctx.resource::<T>()`, `Res<T>`). It persists across scene changes.
  pub fn insert_resource(mut self, value: impl Resource) -> Self {
    self
      .pending_resources
      .push(Box::new(move |world: &mut World| {
        world.insert_resource(value);
      }));
    self
  }

  /// Add systems to an engine phase for the whole run (scenes add their own
  /// with `SceneContext::add_systems`).
  ///
  /// In `FixedUpdate` and `Update`, a call whose systems are not ordered
  /// against anything outside it runs in the phase's `Game` set
  /// (`FixedSet::Game`: after the scene's `update`, before physics). A call
  /// that names a set or another call's system with `.in_set(..)`,
  /// `.before(..)` or `.after(..)` keeps exactly that order.
  pub fn add_systems<M>(
    mut self,
    phase: Phase,
    systems: impl IntoScheduleConfigs<ScheduleSystem, M>,
  ) -> Self {
    self.schedules.add_game_systems(phase, systems);
    self
  }

  /// Register a game message type up front (scenes also register one on
  /// their first `write_message`).
  pub fn add_message<T: Message>(mut self) -> Self {
    self
      .pending_resources
      .push(Box::new(|world: &mut World| register_message::<T>(world)));
    self
  }

  /// Adds a loader for the game's own asset type (and registers the type),
  /// before any scene starts. For an extension an engine loader of the same
  /// asset type also takes, the game's loader wins
  pub fn asset_loader<L: AssetLoader>(mut self, loader: L) -> Self {
    self
      .pending_resources
      .push(Box::new(move |world: &mut World| {
        // The engine's world always has its registry by now
        if let Some(registry) = world.get_resource::<AssetServer>().cloned() {
          registry.register_asset::<L::Asset>(world);
          registry.register_loader(loader);
        }
      }));
    self
  }

  /// Reads the class and enum types of Tiled maps from the Tiled project
  /// at `path`, an asset path (for example `"tiled/game.tiled-project"`).
  /// Without it the project is the one named by `[tilemap] tiled_project` in
  /// Ryo.toml, or `project.tiled-project` at the asset root if there is one.
  /// A project lets a map's objects, layers and tiles take their class's
  /// default for every member the file leaves out
  pub fn tiled_project(mut self, path: &str) -> Self {
    self.overrides.tiled_project = Some(path.to_string());
    self
  }

  /// Loads assets from `source` instead of the project's asset folder
  /// (`Workspace::assets_dir`); a test gives a `MemorySource`
  pub fn asset_source(mut self, source: impl AssetSource) -> Self {
    self.asset_source = Some(Box::new(source));
    self
  }

  /// Whether changed asset files reload while the game runs. When on, the
  /// engine watches the asset folder: an asset whose file is saved reloads
  /// under the same handle about 100 ms after the last write, and one whose
  /// new file does not load keeps its last good value.
  ///
  /// The default is on in windowed debug builds, and off in release and
  /// headless ones. `true` turns it on in any build, release and headless
  /// included, as tests of reloading do; `false` turns it off. The web build
  /// has no folder to watch
  pub fn hot_reload(mut self, on: bool) -> Self {
    self.hot_reload = Some(on);
    self
  }

  /// Registers the versions of a saved type `T`: its current version and a
  /// typed step from each older version, so `ctx.saves().load::<T>(..)`
  /// upgrades an older save through the steps. A type never registered is
  /// version 1. Registering `T` again replaces its versions
  ///
  /// ```
  /// use ryo2d::prelude::*;
  /// use serde::{Deserialize, Serialize};
  ///
  /// #[derive(Serialize, Deserialize)]
  /// struct HeroV1 { hp: u32 }
  /// #[derive(Serialize, Deserialize)]
  /// struct Hero { hp: u32, mp: u32 }
  ///
  /// let builder = EngineBuilder::new().save_versions(
  ///   SaveVersions::<Hero>::new(2).step(1, |old: HeroV1| Hero { hp: old.hp, mp: 0 }),
  /// );
  /// ```
  ///
  /// # Panics
  ///
  /// Here, at the call, when the versions have a gap: a step is missing, out
  /// of range or given twice, or the current version is 0
  pub fn save_versions<T: 'static>(mut self, versions: SaveVersions<T>) -> Self {
    self.save_versions.register(versions);
    self
  }

  /// Registers `T` as the game's own settings section `key`: scenes read it
  /// with `ctx.settings().game::<T>()` and change it with
  /// `ctx.settings().game_mut::<T>()`. It starts at `T::default()`, and the
  /// settings file keeps the fields that differ from it. Registering a key
  /// or a type again replaces it
  ///
  /// ```
  /// use ryo2d::prelude::*;
  /// use serde::{Deserialize, Serialize};
  ///
  /// #[derive(Default, Serialize, Deserialize)]
  /// struct Video { subtitles: bool }
  ///
  /// let builder = EngineBuilder::new().game_settings::<Video>("video");
  /// ```
  pub fn game_settings<T>(mut self, key: &str) -> Self
  where
    T: Default + Serialize + DeserializeOwned + Send + Sync + 'static,
  {
    self.sections.register::<T>(key, None);
    self
  }

  /// [`game_settings`](Self::game_settings) for a section whose type changed
  /// over the game's versions: an older section in the settings file upgrades
  /// through the typed steps of `versions`, as a save does
  /// ([`save_versions`](Self::save_versions)). Fields left at their default
  /// were never written, so an older section type should carry
  /// `#[serde(default)]`
  ///
  /// # Panics
  ///
  /// Here, at the call, when the versions have a gap
  pub fn game_settings_versioned<T>(mut self, key: &str, versions: SaveVersions<T>) -> Self
  where
    T: Default + Serialize + DeserializeOwned + Send + Sync + 'static,
  {
    self.sections.register(key, Some(versions));
    self
  }

  /// Keeps the saves and the settings file (`system.json`) in `store`, in
  /// memory, never on disk. A headless engine keeps them in memory anyway, in
  /// a store of its own without this; a test gives one to plant saves or
  /// settings before the engine starts, or to read what the game wrote
  /// (`HeadlessEngine::saves` gives the store either way)
  pub fn memory_saves(mut self, store: MemorySaves) -> Self {
    self.memory_saves = Some(store);
    self
  }

  /// Gives the game's Ryo.toml, compiled in with `ryo2d::project!()`, for
  /// builds with no Ryo.toml to read: the web, or a native run with none in
  /// the working directory or above. The values resolve as a `ryo2d build` of
  /// the same file does. A build's own values and a Ryo.toml on disk come
  /// first.
  pub fn project(mut self, file: ProjectFile) -> Self {
    self.project_file = file;
    self
  }

  /// Set the initial scene that will run when the engine starts.
  pub fn start_scene(mut self, scene: impl Scene + 'static) -> Self {
    self.initial_scene = Some(Box::new(scene));
    self
  }

  /// Register a console command for the whole run; its handler is called
  /// with the world and the command. A command named like a built-in
  /// replaces it, and a later call of the same name replaces an earlier one.
  /// A scene's commands (`ctx.console().command`) answer over these. A name
  /// that is empty or has a space is refused with a warning. Does nothing
  /// in a build without the developer tools.
  ///
  /// ```
  /// use ryo2d::prelude::*;
  ///
  /// #[derive(Resource, Default)]
  /// struct Gold(u32);
  ///
  /// let builder = EngineBuilder::new()
  ///   .insert_resource(Gold::default())
  ///   .console_command(
  ///     CommandSpec::new("game:gold", "Add gold").usage("game:gold <amount>"),
  ///     |world, cmd| {
  ///       let amount: u32 = cmd.arg(0)?;
  ///       world.resource_mut::<Gold>().0 += amount;
  ///       Ok(())
  ///     },
  ///   );
  /// # let _ = builder;
  /// ```
  pub fn console_command(
    self,
    spec: impl Into<CommandSpec>,
    handler: impl Fn(&mut World, &mut Command<'_>) -> Result<(), String> + Send + Sync + 'static,
  ) -> Self {
    #[cfg(dev_tools)]
    {
      let mut this = self;
      let spec = spec.into();
      if crate::core::console::valid_name(&spec.name) {
        this
          .console_setup
          .push(ConsoleSetup::Add(spec, Arc::new(handler)));
      }
      this
    }
    #[cfg(not(dev_tools))]
    {
      let _ = (spec, handler);
      self
    }
  }

  /// Remove the console command `name`, a built-in included, before the
  /// engine starts (after the calls made before this one). Does nothing in
  /// a build without the developer tools.
  pub fn remove_console_command(self, name: impl Into<String>) -> Self {
    #[cfg(dev_tools)]
    {
      let mut this = self;
      this.console_setup.push(ConsoleSetup::Remove(name.into()));
      this
    }
    #[cfg(not(dev_tools))]
    {
      let _ = name;
      self
    }
  }

  /// Build the engine (consumes the builder).
  ///
  /// In a `ryo2d build` release the process switches to the game's folder
  /// during this call, so relative paths are resolved against that folder
  /// afterwards. Resolve command-line file arguments before calling it.
  ///
  /// With `RYO_HEADLESS=1` in a dev build the engine is built like a
  /// `HeadlessEngine` (sound on the silent mock backend, its variations
  /// seeded so runs repeat; saves and settings in memory; no gamepads or
  /// wall-clock overlays) but keeps `RYO_CAPTURE_*` and `RYO_INPUT`, and `run` renders
  /// offscreen, each step taking at least its share of a second when
  /// `RYO_HEADLESS_HZ` is set.
  pub fn build(self) -> Engine {
    let (offscreen, warning) = crate::core::capture::headless_run_from_env();
    if let Some(warning) = warning {
      log::warn!("{warning}");
    }
    if !offscreen {
      return self.build_engine(false);
    }
    let mut engine = self.build_engine(true);
    let (capture, script, invalid) = scripted_run_from_env();
    let (pace, invalid_pace) = headless_pace_from_env();
    engine.prepare_offscreen_run(capture, script);
    engine.offscreen_pace = pace;
    engine.offscreen_refused = invalid || invalid_pace;
    engine
  }

  /// Build an engine for `testing::HeadlessEngine`: sound on the silent mock
  /// backend; saves and settings in memory; no gamepads, hot reload, capture
  /// env vars or wall-clock overlays.
  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn build_headless(self) -> Engine {
    self.build_engine(true)
  }

  fn build_engine(self, headless: bool) -> Engine {
    let resolved = self.resolve(headless);
    Engine::new(resolved, headless)
  }

  /// Apply every configuration source in priority order, without creating the engine.
  fn resolve(self, headless: bool) -> ResolvedBuild {
    // Release bundles resolve assets and levels relative to their own folder
    if !headless {
      crate::core::config::save_location::enter_bundle_dir();
    }
    let workspace = crate::core::config::Workspace::discover().ok();
    self.resolve_in(workspace.as_ref(), headless)
  }

  /// [`resolve`](Self::resolve) against an explicit workspace (`None` = no project files).
  ///
  /// Order: engine defaults → embedded values (release) or Ryo.toml (dev) →
  /// builder calls, the game's defaults → the player's settings file over the
  /// settings the `with_user_preferences` filters keep, read through the save
  /// storage (a headless build's memory store, never the disk).
  fn resolve_in(
    self,
    workspace: Option<&crate::core::config::Workspace>,
    headless: bool,
  ) -> ResolvedBuild {
    use crate::core::config::project::{ProjectLayer, compiled_embedded_value};
    use crate::core::config::resolve::resolve_settings;
    use crate::core::config::save_location::store_name;

    let initial_scene = self
      .initial_scene
      .expect("EngineBuilder requires a scene - call .start_scene() before .build()");

    let embedded = ProjectLayer::from_embedded(compiled_embedded_value);
    let on_disk = workspace.filter(|ws| ws.has_config_file());
    let file = if embedded.is_none() && on_disk.is_none() {
      self.project_file.read()
    } else {
      None
    };

    // `names`: what names the browser's store, from the layer that won
    let (project, game_name, game_version, names) = match (embedded, on_disk, file) {
      (Some(embedded), ..) => {
        let folder = compiled_embedded_value("RYO_EMBED_SAVE_FOLDER").map(str::to_string);
        (embedded, None, None, (folder, None))
      }
      (None, Some(ws), _) => {
        let config = ws.config();
        (
          ProjectLayer::from_workspace(ws),
          Some(config.name.clone()),
          config.version.clone(),
          (
            config.release.save_folder.clone(),
            Some(config.name.clone()),
          ),
        )
      }
      (None, None, Some(file)) => (file.layer, None, None, (file.save_folder, file.name)),
      (None, None, None) => match workspace {
        Some(ws) => (
          ProjectLayer::from_workspace(ws),
          Some(ws.config().name.clone()),
          ws.config().version.clone(),
          (None, None),
        ),
        None => (ProjectLayer::default(), None, None, (None, None)),
      },
    };

    let settings = resolve_settings(&project, &self.overrides);
    // Only the console names the game
    #[cfg(not(dev_tools))]
    let _ = (game_name, game_version);

    let (folder, name) = names;
    let store_name = store_name(
      folder.as_deref(),
      name.as_deref(),
      &settings.title,
      cfg!(target_arch = "wasm32"),
    );

    // Headless saves and settings stay in memory, never on disk
    let memory_saves = self
      .memory_saves
      .or_else(|| headless.then(MemorySaves::new));
    let saves = SaveService::new(self.save_versions);
    if let Some(storage) = save_storage(memory_saves.as_ref(), workspace) {
      saves.set_storage(storage);
    }

    // The player's settings: the resolved values as the defaults, then the
    // settings file, read when the game keeps any setting or section
    let kept = combined_filter(&self.user_preferences);
    let reads_file = !kept.is_empty() || !self.sections.is_empty();
    let mut player = Settings::new(player_defaults(&settings), kept, self.sections);
    let mut saved_bindings = None;
    // The file is written by the same rule as it is read: when the game
    // keeps anything (headless builds into their memory store)
    let settings_writer = reads_file.then(|| {
      // An older file's `env` is compared with the project's, as it was seeded from it
      let project_env = workspace
        .map(|ws| ws.config().env_json().into_iter().collect())
        .unwrap_or_default();
      let (writer, bindings) =
        apply_settings_file(read_settings_file(&saves, &project_env), &mut player);
      saved_bindings = bindings;
      writer
    });

    let mut config = self.config;
    apply_debug_settings(&mut config.debug, &settings, player.show_fps());
    config.presentation = settings.presentation();
    config.window_size = Some(player.window_size());
    config.title = settings.title;
    config.width = settings.resolution.0;
    config.height = settings.resolution.1;
    config.display_mode = player.display_mode();
    config.target_background_fps = settings.background_fps;
    config.background_rendering = settings.background_rendering;
    config.icon_path = settings.icon;

    let exe = std::env::current_exe().ok();
    apply_launch_overrides(
      &mut config,
      std::env::var("RYO_PROFILER_AUTO_OPEN").is_ok(),
      exe.as_deref().and_then(std::path::Path::parent),
    );

    ResolvedBuild {
      config,
      initial_scene,
      settings: player,
      settings_writer,
      audio_buffer_frames: settings.audio_buffer_frames,
      tiled_project: settings.tiled_project,
      #[cfg(dev_tools)]
      game_name,
      #[cfg(dev_tools)]
      game_version,
      store_name,
      #[cfg(dev_tools)]
      console_setup: self.console_setup,
      pending_resources: self.pending_resources,
      schedules: self.schedules,
      saved_bindings,
      workspace: workspace.cloned(),
      asset_source: self.asset_source,
      hot_reload: self
        .hot_reload
        .unwrap_or(cfg!(debug_assertions) && !headless),
      saves,
      memory_saves,
    }
  }
}

impl EngineBuilder {
  /// The debug switches a project with this Ryo.toml resolves to, with the
  /// builder's calls over it and the launch's overrides
  /// (`testing::resolved_debug`). Text that is not TOML is no project.
  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn resolved_debug(self, ryo_toml: &str, profiler_requested: bool) -> DebugConfig {
    use crate::core::config::project::ProjectLayer;
    use crate::core::config::resolve::resolve_settings;
    let project = crate::core::config::Workspace::from_toml_str(ryo_toml, "game".into())
      .map(|workspace| ProjectLayer::from_workspace(&workspace))
      .unwrap_or_default();
    let settings = resolve_settings(&project, &self.overrides);
    let mut config = self.config;
    apply_debug_settings(&mut config.debug, &settings, settings.show_fps);
    apply_launch_overrides(&mut config, profiler_requested, None);
    config.debug
  }
}

/// The engine's debug switches from the resolved settings, the FPS counter
/// from the player's settings. A build without the developer tools turns
/// them off afterwards, in [`apply_launch_overrides`], the last word.
fn apply_debug_settings(
  debug: &mut DebugConfig,
  settings: &crate::core::config::resolve::ResolvedSettings,
  show_fps: bool,
) {
  debug.show_fps = show_fps;
  debug.show_colliders = settings.show_colliders;
  debug.show_velocities = settings.show_velocities;
  debug.show_origins = settings.show_origins;
  debug.disable_debug_keys = settings.disable_debug_keys;
  debug.show_frame_timing = settings.show_frame_timing;
  debug.enable_profiler = settings.enable_profiler;
  debug.logger_window = settings.logger_window;
  debug.dev_window_keys = settings.dev_window_keys;
  debug.dev_console = settings.dev_console;
}

/// A console call made on the builder, applied over the built-ins in order
#[cfg(dev_tools)]
enum ConsoleSetup {
  Add(CommandSpec, Arc<crate::core::console::WorldHandler>),
  Remove(String),
}

/// Engine settings after every configuration source has been applied.
struct ResolvedBuild {
  config: EngineConfig,
  initial_scene: Box<dyn Scene>,
  /// The player's settings: the resolved defaults with the settings file
  /// applied
  settings: Settings,
  /// Writes the settings file, over the file as read (`None`: the game keeps
  /// no settings, and the file is neither read nor written)
  settings_writer: Option<SettingsWriter>,
  /// The audio device's buffer in frames; `None`: the device's default
  audio_buffer_frames: Option<u32>,
  /// The Tiled project's asset path (`None`: the default)
  tiled_project: Option<String>,
  /// The game's name and version, for the console
  #[cfg(dev_tools)]
  game_name: Option<String>,
  #[cfg(dev_tools)]
  game_version: Option<String>,
  /// The name the browser keeps the saves under
  #[cfg_attr(
    all(not(target_arch = "wasm32"), not(test)),
    expect(dead_code, reason = "only the web names a store; tests check the name")
  )]
  store_name: String,
  #[cfg(dev_tools)]
  console_setup: Vec<ConsoleSetup>,
  pending_resources: Vec<WorldSetup>,
  schedules: EngineSchedules,
  /// The player's saved input bindings, when bindings are a user preference
  saved_bindings: Option<BTreeMap<String, Vec<String>>>,
  /// The project, found once (`None`: no working directory)
  workspace: Option<crate::core::config::Workspace>,
  /// Where assets load from (`None`: the project's asset folder)
  asset_source: Option<Box<dyn AssetSource>>,
  /// Whether changed asset files reload
  hot_reload: bool,
  /// The saves, on their storage, holding the saved types' versions
  saves: SaveService,
  /// The store a headless engine (or a builder's `memory_saves`) keeps its
  /// saves and settings in
  memory_saves: Option<MemorySaves>,
}

impl Default for EngineBuilder {
  fn default() -> Self {
    Self::new()
  }
}

/// Settings that come from how the game was launched, applied after layering.
///
/// - `profiler_requested` (`ryo2d dev --profiler` sets `RYO_PROFILER_AUTO_OPEN`)
///   turns the profiler on so puffin scopes are collected.
/// - With no icon configured, an `icon.png` beside the executable (put there by
///   `ryo2d build`) is used; otherwise the embedded Ryo2D logo is.
///
/// Last, a build without the developer tools turns every switch that shows
/// one off, whatever the project, the builder or the launch set.
fn apply_launch_overrides(
  config: &mut EngineConfig,
  profiler_requested: bool,
  exe_dir: Option<&std::path::Path>,
) {
  if profiler_requested {
    config.debug.enable_profiler = true;
  }
  if config.icon_path.is_none()
    && let Some(icon_path) = exe_dir.map(|dir| dir.join("icon.png"))
    && icon_path.exists()
  {
    config.icon_path = Some(icon_path);
  }
  #[cfg(not(dev_tools))]
  config.debug.switch_tools_off();
}

/// Where the saves are kept: `memory` when given, else the project's save
/// folder (native); `None` (saves unavailable) without either
fn save_storage(
  memory: Option<&MemorySaves>,
  workspace: Option<&crate::core::config::Workspace>,
) -> Option<Arc<dyn crate::core::saves::storage::Storage>> {
  if let Some(memory) = memory {
    return Some(Arc::new(memory.clone()));
  }
  #[cfg(not(target_arch = "wasm32"))]
  {
    use crate::core::saves::files::FileStorage;
    let folder = workspace?.save_location();
    match FileStorage::new(folder.clone()) {
      Ok(storage) => Some(Arc::new(storage)),
      Err(error) => {
        log::warn!(
          "saves are unavailable: could not create the save folder {} ({error})",
          folder.display()
        );
        None
      }
    }
  }
  #[cfg(target_arch = "wasm32")]
  {
    let _ = workspace;
    None
  }
}

/// Every preference any `with_user_preferences` call asked for.
fn combined_filter(
  filters: &[crate::core::config::UserPreferenceFilter],
) -> crate::core::config::UserPreferenceFilter {
  use crate::core::config::{UserPreference, UserPreferenceFilter};
  UserPreferenceFilter::only(
    UserPreference::all()
      .iter()
      .copied()
      .filter(|pref| filters.iter().any(|filter| filter.contains(*pref))),
  )
}

/// The player's settings at the resolved values: the game's defaults
fn player_defaults(settings: &crate::core::config::resolve::ResolvedSettings) -> EngineValues {
  EngineValues {
    volumes: [
      settings.master_volume,
      settings.music_volume,
      settings.sfx_volume,
      settings.ui_volume,
      settings.voice_volume,
      settings.ambience_volume,
    ],
    // The window the game opens when the player has not picked one
    window_size: crate::core::graphics::screen::initial_window_size(
      settings.presentation(),
      settings.resolution,
      settings.window_size,
    ),
    display_mode: settings.display_mode,
    vsync: settings.vsync,
    show_fps: settings.show_fps,
  }
}

/// The settings file as [`read_settings_file`] found it
struct SettingsRead {
  /// `None`: none was read, so the defaults stand
  file: Option<SettingsFile>,
  /// The file stays where it is unread (it could not be read, or set
  /// aside), so it must not be overwritten
  unread: bool,
  /// A damaged file's set-aside, while the storage commits it (the
  /// browser's)
  aside: Option<crate::core::saves::storage::Completion>,
}

/// Applies the settings file read to `settings`, warning of its bad values;
/// returns the writer over it and the player's saved bindings, when kept
fn apply_settings_file(
  read: SettingsRead,
  settings: &mut Settings,
) -> (SettingsWriter, Option<BTreeMap<String, Vec<String>>>) {
  let file = read.file.unwrap_or_default();
  let mut warnings = Vec::new();
  settings.apply_file(&file, &mut warnings);
  for warning in warnings {
    log::warn!("{warning}");
  }
  let bindings = if settings.is_kept(UserPreference::Bindings) {
    file.bindings.clone()
  } else {
    None
  };
  // A file this engine could not fully read is never overwritten
  let read_only = read.unread || file.format > SETTINGS_FORMAT;
  let writer = SettingsWriter::new(file, read_only, settings).after_set_aside(read.aside);
  (writer, bindings)
}

/// The settings file, read through the save storage. A damaged file is set
/// aside as `system.json.damaged` and its newest whole copy (the backup)
/// read instead, else none: the defaults. Warns of what it could not read
fn read_settings_file(
  saves: &SaveService,
  project_env: &serde_json::Map<String, serde_json::Value>,
) -> SettingsRead {
  use crate::core::saves::storage::{Origin, Recovery};
  use crate::core::settings::file::{self, DAMAGED_NAME, FILE_NAME, Parsed};

  let mut read = SettingsRead {
    file: None,
    unread: false,
    aside: None,
  };
  let found = match saves.recover_named(FILE_NAME, file::verify) {
    Ok(Recovery::Missing) => return read,
    Ok(Recovery::Damaged) => None,
    Ok(Recovery::Found { bytes, from }) => Some((bytes, from)),
    Err(error) => {
      log::warn!("could not read {FILE_NAME} ({error}); using the defaults");
      read.unread = true;
      return read;
    }
  };
  // Another copy is read when the file itself is damaged, or gone in a crash
  if found
    .as_ref()
    .is_none_or(|(_, from)| *from != Origin::Current)
  {
    let then = match &found {
      None => "using the defaults",
      Some((_, Origin::Backup)) => "using its backup",
      Some(_) => "using its newest whole copy",
    };
    match saves.set_aside_named(FILE_NAME, DAMAGED_NAME).poll() {
      Ok(Err(error)) if error.kind() == std::io::ErrorKind::NotFound => {
        if found.is_none() {
          log::warn!("the backup of {FILE_NAME} is damaged; {then}");
        }
      }
      Ok(Err(error)) => {
        read.unread = true;
        log::warn!("{FILE_NAME} is damaged and could not be set aside ({error}); {then}");
      }
      result => {
        // Renaming in a store that commits later: the writer waits for it
        if let Err(pending) = result {
          read.aside = Some(pending);
        }
        log::warn!("{FILE_NAME} is damaged: set aside as {DAMAGED_NAME}; {then}");
      }
    }
  }
  let Some((bytes, _)) = found else {
    return read;
  };
  match file::parse(&bytes, project_env) {
    Parsed::Read { file, warnings } | Parsed::Newer { file, warnings } => {
      for warning in warnings {
        log::warn!("{warning}");
      }
      read.file = Some(file);
    }
    // Not met: a copy that verified is a JSON object
    Parsed::Damaged(reason) => {
      log::warn!("could not read {FILE_NAME} ({reason}); using the defaults");
      read.unread = true;
    }
  }
  read
}

#[cfg(test)]
mod saves_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod ui_image_tests;
#[cfg(test)]
mod ui_theme_tests;
