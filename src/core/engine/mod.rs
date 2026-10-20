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

use std::collections::HashMap;
use std::sync::Arc;
use web_time::Instant;

use gilrs::Gilrs;
use winit::event_loop::EventLoop;
#[cfg(target_arch = "wasm32")]
use winit::event_loop::EventLoopProxy;
use winit::window::Window;

use crate::core::asset::AssetManager;
use crate::core::audio::{Audio, AudioManager};
use crate::core::capture::{AutoCaptureConfig, CaptureReport, CaptureState};
use crate::core::config::{
  DebugConfig, DebugStats, DisplayMode, FrameTimings, OverlayMode, SmoothedDebugStats,
  SmoothedFrameTimings,
};
use crate::core::console::{
  CommandRegistry, ConsoleCommandBuilder, ConsoleState, register_builtins,
};
use crate::core::cursor::CursorRequest;
use crate::core::ecs::interpolate::snapshot_previous_transforms;
use crate::core::ecs::propagate::propagate_transforms;
use crate::core::ecs::{
  FallbackErrorHandler, IntoScheduleConfigs, Message, Resource, ScheduleSystem, World,
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
use crate::core::physics::sync::register_physics_systems;
use crate::core::physics::{Physics, PhysicsConfig};
use crate::core::render::batch::Batch2D;
use crate::core::render::{
  DrawList, EntityDrawer, RenderPipeline, UiResolution, UiSpace, begin_debug_fixed_step,
  end_debug_fixed_step,
};
use crate::core::scene::{PhysicsPaused, Scene, SceneManager};
use crate::core::schedule::{EngineSchedules, FixedSet, Phase, PostUpdateSet, UpdateSet};
use crate::core::shaders::EffectsManager;
use crate::core::systems::{record_trail_points, register_juice_systems};
use crate::core::text::{GlyphRasterizer, TextEngine, advance_text_reveal};
use crate::core::time::Time;
use crate::core::ui::immediate::UiCommands;
use crate::core::ui::layout::engine::LayoutEngine;
use crate::core::ui::memory::UiMemory;
use crate::core::window::{WindowInfo, WindowRequest};
use crate::{engine_info, engine_warn};

use frame::{
  advance_clock_system, asset_pump_system, clear_trackers_system, publish_input_system,
  scene_fixed_systems, scene_transition_system, scene_update_system, scene_update_systems,
  update_effects_system, update_messages_system,
};

/// Events the engine sends its own event loop.
// Only the web sends any
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) enum EngineEvent {
  /// The web's asynchronous GPU setup finished
  GpuReady(Box<Result<WgpuRenderer, crate::core::graphics::GpuError>>),
}

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
    }
  }
}

/// The scene stack and the audio device. Neither is `Send`, so they live in
/// the world as a non-send resource that the scene systems take out while
/// they run.
pub(crate) struct SceneHost {
  pub(crate) scenes: SceneManager,
  pub(crate) audio: Audio,
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
  /// Saves the player's changed input bindings (see `UserPreference::Bindings`)
  bindings_saver: Option<crate::core::config::SaveManager>,
  bindings_save_warned: bool,
  window: Option<Arc<Window>>,
  renderer: Option<WgpuRenderer>,
  world: World,
  schedules: EngineSchedules,
  // GPU renderers the passes draw with (engine-owned, not resources)
  pub(crate) sprite_gpu: Option<SpriteGpuRenderer>,
  pub(crate) batch: Option<Batch2D>,
  lighting: Option<LightingRenderer>,
  entity_drawer: EntityDrawer,
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
  )>,
  parallax: Vec<crate::core::graphics::SpriteInstance>,
  poly_points: Vec<crate::core::render::batch::PolyPoint>,
  targetless_cameras: Vec<crate::core::ecs::Entity>,
  warned_targetless: std::collections::HashSet<crate::core::ecs::Entity>,
  draw_list: DrawList,
  batch_collector: BatchCollector,
  // UI: the frame's recorded draws, the UI space they are in, the UI
  // camera they draw through (when any were recorded) and the cameras the
  // batch lays out (the world's, then the UI's; refilled each frame)
  ui_commands: UiCommands,
  ui_space: UiSpace,
  ui_camera: Option<crate::core::render::CameraDraw>,
  /// UI render retention: what the UI image holds
  ui_retain: crate::core::render::ui_retain::UiRetain,
  batch_cameras: Vec<crate::core::render::CameraDraw>,
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
  /// The `UiTheme` generation whose skin sprites are loaded
  theme_generation: Option<u64>,
  /// The `UiTheme` generation whose sounds are loaded
  sounds_generation: Option<u64>,
  /// UI sound paths that failed to load (each warned about once)
  sounds_warned: std::collections::HashSet<String>,
  /// How many of this frame's UI sounds were played already (a rerun pass
  /// appends to the frame's list)
  sounds_played: usize,
  /// Reloads the theme's file when it changes
  #[cfg(debug_assertions)]
  theme_watch: crate::core::ui::theme::ThemeWatch,
  /// UI passes run this frame (a scene switched from `ui` runs a second)
  ui_pass_index: u32,
  /// What the frame's UI pass did besides drawing, for headless snapshots
  #[cfg(not(target_arch = "wasm32"))]
  ui_pass_log: frame::UiPassLog,
  // Developer tools
  console_state: ConsoleState,
  console_quit_requested: bool,
  pub(crate) capture: CaptureState,
  input_script: InputScript,
  frame_timings: FrameTimings,
  smoothed_timings: SmoothedFrameTimings,
  overlay_mode: OverlayMode,
  debug_stats: DebugStats,
  smoothed_stats: SmoothedDebugStats,
  debug_keys_enabled: bool,
  last_fps_log: Instant,
  #[cfg(feature = "profiler")]
  profiler_window: Option<crate::core::windows::profiler_window::ProfilerWindow>,
  logger_window: Option<crate::core::windows::logger_window::LoggerWindow>,
  game_window_focused: bool,
  #[cfg(feature = "profiler")]
  profiler_window_focused: bool,
  logger_window_focused: bool,
  #[cfg(feature = "profiler")]
  pending_open_profiler: bool,
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
  // Delivers the web's asynchronous GPU setup to the event loop
  #[cfg(target_arch = "wasm32")]
  proxy: Option<EventLoopProxy<EngineEvent>>,
  last_window_size: (u32, u32),
  // The window has a zero side: frames run but nothing is drawn
  minimized: bool,
  vsync: bool,
  /// Run the frame's logic but do not draw it (an unfocused window with
  /// background rendering off)
  skip_present: bool,
  // Headless runs (`RYO_HEADLESS`)
  offscreen_run: bool,
  offscreen_refused: bool,
  game_name: Option<String>,
  game_version: Option<String>,
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
    eprintln!("[capture] {warning}");
    engine_warn!(module: "Capture", "{}", warning);
  }
  let (script, warning) = InputScript::from_env();
  let invalid = !warnings.is_empty() || warning.is_some();
  if let Some(warning) = warning {
    eprintln!("[input] {warning}");
    engine_warn!(module: "Input", "{}", warning);
  }
  (capture, script, invalid)
}

/// Systems that fail panic in debug builds and log in release builds.
fn handle_system_error(error: crate::core::ecs::SystemError, ctx: crate::core::ecs::ErrorContext) {
  if cfg!(debug_assertions) {
    crate::core::ecs::kernel::panic_on_error(error, ctx);
  } else {
    crate::engine_error!(module: "Systems", "{ctx}: {error}");
  }
}

/// Insert the resources scene callbacks and systems expect (the GPU ones
/// follow in `Engine::init_gpu`).
pub(crate) fn init_world_resources(
  world: &mut World,
  config: &EngineConfig,
  assets: AssetManager,
  commands: CommandRegistry,
) {
  let (w, h) = (config.width, config.height);
  world.insert_resource(Time::new(config.fixed_hz));
  world.insert_resource(Input::new());
  world.insert_resource(ScreenMapping::new(config.presentation, (w, h), 1.0));
  spawn_main_camera(world, glam::Vec2::new(w as f32, h as f32));
  world.insert_resource(assets);
  world.insert_resource(EffectsManager::new());
  world.insert_resource(crate::core::render::Look::default());
  world.insert_resource(LightingSettings::default());
  world.insert_resource(crate::core::graphics::RenderTargets::default());
  world.insert_resource(crate::core::render::DebugDraw::default());
  world.insert_resource(crate::core::tilemap::TilemapManager::new());
  world.insert_resource(WindowRequest::default());
  world.insert_resource(CursorRequest::default());
  world.insert_resource(WindowInfo {
    scale_factor: 1.0,
    display_mode: config.display_mode,
  });
  world.insert_resource(RenderPipeline::default_2d());
  world.insert_resource(crate::core::text::Fonts::with_builtins());
  world.insert_resource(crate::core::text::TextIcons::default());
  world.insert_resource(crate::core::text::TextEffects::default());
  world.insert_resource(crate::core::ui::UiTheme::default());
  world.insert_resource(PhysicsPaused(false));
  world.insert_resource(frame::PendingTickCheck::default());
  world.add_observer(frame::record_tick_check);
  world.insert_resource(config.physics.clone());
  world.insert_resource(Physics::new());
  world.insert_resource(commands);
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
    (asset_pump_system, scene_transition_system),
  );
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
  schedules.add_systems(
    Phase::FixedUpdate,
    propagate_transforms.in_set(FixedSet::Transforms),
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
      crate::core::tilemap::manager::forget_despawned_tilemaps,
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
      vsync,
      master_volume,
      music_volume,
      sfx_volume,
      disable_debug_keys,
      game_name,
      game_version,
      console_builder,
      pending_resources,
      mut schedules,
      saved_bindings,
      bindings_saver,
    } = resolved;

    #[cfg_attr(not(debug_assertions), allow(unused_mut))]
    let mut assets = AssetManager::new();
    let initial_size = (config.width, config.height);

    // Tell players and developers where settings and saves are kept
    if !headless && let Ok(workspace) = crate::core::config::Workspace::discover() {
      engine_info!(module: "Save", "Save data: {}", workspace.save_location().display());
    }

    // Enable hot reload in debug builds if a workspace with assets directory exists
    #[cfg(debug_assertions)]
    if !headless && let Ok(workspace) = crate::core::config::Workspace::discover() {
      let assets_dir = workspace.assets_dir();
      if assets_dir.exists()
        && let Err(e) = assets.enable_hot_reload(vec![assets_dir])
      {
        engine_warn!(module: "HotReload", "Failed to enable: {}", e);
      }
    }

    // Try to initialize audio (gracefully handle no audio device)
    let mut audio = if headless {
      None
    } else {
      match AudioManager::new() {
        Ok(manager) => Some(manager),
        Err(e) => {
          engine_warn!(
            module: "Audio",
            "Could not initialize audio: {}. Continuing without sound.",
            e
          );
          None
        }
      }
    };
    if let Some(audio_manager) = &mut audio {
      audio_manager.set_master_volume(master_volume);
      audio_manager.set_music_volume(music_volume);
      audio_manager.set_sfx_volume(sfx_volume);
    }

    // Initialize gamepad input (gracefully handle no gamepad support)
    let gilrs = if headless {
      None
    } else {
      match Gilrs::new() {
        Ok(g) => {
          engine_info!(module: "Gamepad", "Initialized successfully");
          Some(g)
        }
        Err(e) => {
          engine_warn!(
            module: "Gamepad",
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

    let mut command_registry = CommandRegistry::new();
    register_builtins(&mut command_registry);
    if let Some(builder) = console_builder {
      for name in &builder.removals {
        command_registry.remove_builtin(name);
      }
      for (name, description, verbose) in builder.declarations {
        command_registry.declare(&name, &description, verbose.as_deref());
      }
    }

    let (capture_config, input_script) = if headless {
      (AutoCaptureConfig::default(), InputScript::default())
    } else {
      let (capture, script, _) = scripted_run_from_env();
      (capture, script)
    };

    let mut world = World::new();
    init_world_resources(&mut world, &config, assets, command_registry);
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
      audio: Audio::new(audio),
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
      lighting: None,
      entity_drawer: EntityDrawer::default(),
      text: TextEngine::new(),
      glyph_raster: GlyphRasterizer::new(),
      text_warned: Default::default(),
      camera_uniforms: Default::default(),
      camera_draws: Vec::new(),
      camera_globals: Vec::new(),
      lights: Vec::new(),
      occluders: Vec::new(),
      parallax: Vec::new(),
      poly_points: Vec::new(),
      targetless_cameras: Vec::new(),
      warned_targetless: Default::default(),
      draw_list: DrawList::default(),
      batch_collector: BatchCollector::new(),
      ui_commands: UiCommands::default(),
      ui_space: initial_ui_space,
      ui_camera: None,
      ui_retain: Default::default(),
      batch_cameras: Vec::new(),
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
      sounds_warned: Default::default(),
      sounds_played: 0,
      #[cfg(debug_assertions)]
      theme_watch: Default::default(),
      ui_pass_index: 0,
      #[cfg(not(target_arch = "wasm32"))]
      ui_pass_log: frame::UiPassLog::default(),
      console_state: ConsoleState::new(),
      console_quit_requested: false,
      capture: CaptureState::new(capture_config),
      input_script,
      frame_timings: FrameTimings::default(),
      smoothed_timings: SmoothedFrameTimings::default(),
      overlay_mode: initial_overlay_mode,
      debug_stats: DebugStats::default(),
      smoothed_stats: SmoothedDebugStats::default(),
      debug_keys_enabled: !disable_debug_keys && !headless,
      last_fps_log: Instant::now(),
      #[cfg(feature = "profiler")]
      profiler_window: None,
      logger_window: None,
      game_window_focused: true,
      #[cfg(feature = "profiler")]
      profiler_window_focused: false,
      logger_window_focused: false,
      #[cfg(feature = "profiler")]
      pending_open_profiler: false,
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
      last_window_size: initial_size,
      minimized: false,
      vsync,
      skip_present: false,
      offscreen_run: false,
      offscreen_refused: false,
      game_name,
      game_version,
      bindings_saver,
      bindings_save_warned: false,
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

    Self::print_startup_banner(&self.config, self.vsync);

    engine_info!(module: "Init", "Initializing {}...", self.config.title);
    engine_info!(module: "Init", "Resolution: {}x{}", self.config.width, self.config.height);
    engine_info!(
      module: "Init",
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
      event_loop
        .run_app(&mut self)
        .map_err(|e| EngineError::Window(format!("Event loop error: {}", e)))?;
      match self.startup_error.take() {
        Some(error) => Err(EngineError::Gpu(error)),
        None => Ok(()),
      }
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
      eprintln!("[headless] not running: fix the invalid RYO_* values above");
      std::process::exit(2);
    }
    println!(
      "[headless] {} at {}x{}, no window",
      self.config.title, self.config.width, self.config.height
    );
    if !self.capture.exits_after_last() {
      println!(
        "[headless] no RYO_CAPTURE_EXIT=1 with RYO_CAPTURE_FRAMES: running until the scene quits"
      );
    }
    if self.run_offscreen_frames()? {
      std::process::exit(1);
    }
    Ok(())
  }

  /// The offscreen loop: step until the scene quits or `RYO_CAPTURE_EXIT` is
  /// met, then flush captures. Returns whether a scheduled capture failed.
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
    }

    // However the loop ended, finish in-flight writes and report frames that
    // were never presented
    let reports = self.capture.finish();
    self.report_captures(reports);
    if let Some(error) = fatal {
      return Err(EngineError::Render(error));
    }
    Ok(self.capture.exit_with_failure())
  }

  /// Print startup banner to console with useful info.
  fn print_startup_banner(config: &EngineConfig, vsync: bool) {
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
    println!("  `      Developer console");
    println!("  F1-F4  Debug overlays");
    println!("  F5     Toggle borderless");
    println!("  F9     Logger");
    #[cfg(feature = "profiler")]
    println!("  F10    Profiler");
    println!("  F12    Capture frame");
    println!();
  }

  /// The fixed logic timestep.
  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn fixed_timestep(&self) -> std::time::Duration {
    self.world.resource::<Time>().fixed.timestep_duration()
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

  /// Queue a wheel or trackpad scroll; a trackpad's pixels become logical
  /// pixels here, where the scale factor is known.
  pub(crate) fn scroll(&mut self, delta: winit::event::MouseScrollDelta) {
    let event = crate::core::input::scroll_event(delta, self.scale_factor);
    self.world.resource_mut::<Input>().queue(event);
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

  /// The debug switches, for a test to turn an overlay on
  #[cfg(all(test, not(target_arch = "wasm32")))]
  pub(crate) fn debug_config_mut(&mut self) -> &mut DebugConfig {
    &mut self.config.debug
  }

  /// The last frame's render step on the CPU (building, uploading and
  /// encoding; not the UI pass)
  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn render_time(&self) -> std::time::Duration {
    self.frame_timings.render + self.frame_timings.present
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
  /// Creates the sprite and lighting renderers and the batch, inserts the `Gpu`
  /// resource and the profiler, then enters the initial scene.
  pub(crate) fn init_gpu(&mut self, renderer: WgpuRenderer, scale_factor: f64) {
    let format = renderer.game_texture_format();
    // Sprites sample as their filter says, by default nearest for pixel art
    // and smooth at the window's resolution (the batch resolves it)
    let sprite_gpu = SpriteGpuRenderer::new(renderer.device(), renderer.queue());
    let batch = Batch2D::new(renderer.device(), format, self.config.presentation);
    // Each camera is lit in light targets of its own size
    let lighting = LightingRenderer::new(renderer.device(), format);
    let mapping = renderer.mapping();

    self.scale_factor = scale_factor;
    self.world.resource_mut::<WindowInfo>().scale_factor = scale_factor;
    self.world.insert_resource(Gpu {
      device: renderer.device_arc(),
      queue: renderer.queue_arc(),
    });

    self.renderer = Some(renderer);
    self.sprite_gpu = Some(sprite_gpu);
    self.batch = Some(batch);
    self.lighting = Some(lighting);
    self.last_window_size = mapping.physical_size();
    self.apply_mapping(mapping);

    // Enables puffin scope collection so F10 can open the profiler later
    // (a no-op without the `profiler` feature)
    crate::core::profiler::init(self.config.debug.enable_profiler);

    frame::with_host(&mut self.world, |world, host| {
      host.scenes.enter_initial(world, &mut host.audio);
      host.scenes.sync_physics_paused(world);
    });

    engine_info!(module: "Init", "Engine initialized successfully!");
    engine_info!(
      module: "Init",
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
          println!("[capture] {message}");
          engine_info!(module: "Capture", "{}", message);
          self
            .console_state
            .print_system(format!("Captured {message}"));
        }
        CaptureReport::Failed { frame, error } => {
          let message = format!("FAILED frame {frame}: {error}");
          eprintln!("[capture] {message}");
          engine_warn!(module: "Capture", "{}", message);
          self.console_state.print_error(format!("Capture {message}"));
        }
      }
    }
  }

  /// The stats of the last frame, for tests.
  #[cfg(test)]
  pub(crate) fn debug_stats(&self) -> &DebugStats {
    &self.debug_stats
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

/// Builder pattern for Engine configuration
pub struct EngineBuilder {
  /// Builder-only settings (target FPS, fixed rate, window mode, ...).
  /// Settings other sources can also set live in `overrides`.
  config: EngineConfig,
  /// Settings set by builder calls, layered over project values in `resolve`
  overrides: crate::core::config::resolve::BuilderOverrides,
  initial_scene: Option<Box<dyn Scene>>,
  /// Console command configuration (declarations and removals)
  console_builder: Option<ConsoleCommandBuilder>,
  /// Filters passed to `with_user_preferences`, combined when the engine is built
  user_preferences: Vec<crate::core::config::UserPreferenceFilter>,
  /// Resources and message types the game registers, applied after the
  /// engine's own
  pending_resources: Vec<WorldSetup>,
  /// The game's engine-wide systems
  schedules: EngineSchedules,
}

impl EngineBuilder {
  pub fn new() -> Self {
    EngineBuilder {
      config: EngineConfig::default(),
      overrides: Default::default(),
      initial_scene: None,
      console_builder: None,
      user_preferences: Vec::new(),
      pending_resources: Vec::new(),
      schedules: EngineSchedules::new(),
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

  /// Whether UI node trees replay their unchanged subtrees instead of
  /// building them every frame (default true). Off, they draw and behave
  /// the same, slower: for ruling caching out when chasing a UI bug.
  pub fn ui_cache(mut self, on: bool) -> Self {
    self.config.debug.ui_cache = on;
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

  /// Let saved player preferences in `system.json` override settings when the
  /// engine is built (this only chooses which ones may).
  ///
  /// Startup settings resolve in one order, each layer overriding only what it
  /// sets: engine defaults → `Ryo.toml` (dev) or the values `ryo2d build`
  /// embedded from it (release) → `EngineBuilder` calls → saved player
  /// preferences in `system.json` (only keys present in the file and in the
  /// filter). Without `with_user_preferences`, `system.json` is never read. A
  /// new `system.json` is seeded from the resolved settings. Headless builds
  /// never read or write it, and repeated calls combine their filters.
  pub fn with_user_preferences(
    mut self,
    filter: impl Into<crate::core::config::UserPreferenceFilter>,
  ) -> Self {
    // Read when the engine is built, so headless builds can skip it entirely
    self.user_preferences.push(filter.into());
    self
  }

  /// Start with `theme` as the UI theme (`ctx.ui_theme()` changes it later).
  /// A theme loaded with `Theme::load` reloads when its file changes, in
  /// debug builds.
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

  /// Set the initial scene that will run when the engine starts.
  pub fn start_scene(mut self, scene: impl Scene + 'static) -> Self {
    self.initial_scene = Some(Box::new(scene));
    self
  }

  /// Declare game console commands and/or remove built-in ones before the
  /// engine starts. Declared commands do nothing until a scene registers a
  /// handler with `ctx.console().register(..)`.
  pub fn with_console_commands(mut self, f: impl FnOnce(&mut ConsoleCommandBuilder)) -> Self {
    let mut builder = self.console_builder.take().unwrap_or_default();
    f(&mut builder);
    self.console_builder = Some(builder);
    self
  }

  /// Build the engine (consumes the builder).
  ///
  /// In a `ryo2d build` release the process switches to the game's folder
  /// during this call, so relative paths are resolved against that folder
  /// afterwards. Resolve command-line file arguments before calling it.
  ///
  /// With `RYO_HEADLESS=1` in a dev build the engine is built like a
  /// `HeadlessEngine` (no audio, gamepads, user preferences or wall-clock
  /// overlays) but keeps `RYO_CAPTURE_*` and `RYO_INPUT`, and `run` renders
  /// offscreen.
  pub fn build(self) -> Engine {
    let (offscreen, warning) = crate::core::capture::headless_run_from_env();
    if let Some(warning) = warning {
      eprintln!("[headless] {warning}");
    }
    if !offscreen {
      return self.build_engine(false);
    }
    let mut engine = self.build_engine(true);
    let (capture, script, invalid) = scripted_run_from_env();
    engine.prepare_offscreen_run(capture, script);
    engine.offscreen_refused = invalid;
    engine
  }

  /// Build an engine for `testing::HeadlessEngine`: no audio, gamepads, hot
  /// reload, user preferences, capture env vars or wall-clock overlays.
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
  /// builder calls → saved preferences present in system.json and in the
  /// `with_user_preferences` filters. Headless builds never touch system.json.
  fn resolve_in(
    self,
    workspace: Option<&crate::core::config::Workspace>,
    headless: bool,
  ) -> ResolvedBuild {
    use crate::core::config::resolve::{ProjectLayer, compiled_embedded_value, resolve_settings};

    let initial_scene = self
      .initial_scene
      .expect("EngineBuilder requires a scene - call .start_scene() before .build()");

    let (project, game_name, game_version) = match (
      ProjectLayer::from_embedded(compiled_embedded_value),
      workspace,
    ) {
      (Some(embedded), _) => (embedded, None, None),
      (None, Some(ws)) => (
        ProjectLayer::from_workspace(ws),
        Some(ws.config().name.clone()),
        ws.config().version.clone(),
      ),
      (None, None) => (ProjectLayer::default(), None, None),
    };

    let mut settings = resolve_settings(&project, &self.overrides, None);
    let mut saved_bindings = None;
    let mut bindings_saver = None;
    if !headless
      && !self.user_preferences.is_empty()
      && let Some(ws) = workspace
    {
      let filter = combined_filter(&self.user_preferences);
      let bindings_enabled = filter.contains(crate::core::config::UserPreference::Bindings);
      if let Some(prefs) = load_user_preferences(ws, &filter, &settings) {
        if bindings_enabled {
          saved_bindings = prefs.bindings.clone();
        }
        settings = resolve_settings(&project, &self.overrides, Some((&prefs, &filter)));
      }
      if bindings_enabled {
        bindings_saver = ws.create_save_manager().ok();
      }
    }

    let mut config = self.config;
    config.presentation = settings.presentation();
    config.window_size = settings.window_size;
    config.title = settings.title;
    config.width = settings.resolution.0;
    config.height = settings.resolution.1;
    config.display_mode = settings.display_mode;
    config.target_background_fps = settings.background_fps;
    config.background_rendering = settings.background_rendering;
    config.icon_path = settings.icon;
    config.debug.show_fps = settings.show_fps;
    config.debug.show_colliders = settings.show_colliders;
    config.debug.show_velocities = settings.show_velocities;
    config.debug.show_origins = settings.show_origins;
    config.debug.show_frame_timing = settings.show_frame_timing;
    config.debug.enable_profiler = settings.enable_profiler;
    config.debug.logger_window = settings.logger_window;
    config.debug.dev_window_keys = settings.dev_window_keys;
    config.debug.dev_console = settings.dev_console;

    let exe = std::env::current_exe().ok();
    apply_launch_overrides(
      &mut config,
      std::env::var("RYO_PROFILER_AUTO_OPEN").is_ok(),
      exe.as_deref().and_then(std::path::Path::parent),
    );

    ResolvedBuild {
      config,
      initial_scene,
      vsync: settings.vsync,
      master_volume: settings.master_volume,
      music_volume: settings.music_volume,
      sfx_volume: settings.sfx_volume,
      disable_debug_keys: settings.disable_debug_keys,
      game_name,
      game_version,
      console_builder: self.console_builder,
      pending_resources: self.pending_resources,
      schedules: self.schedules,
      saved_bindings,
      bindings_saver,
    }
  }
}

/// Engine settings after every configuration source has been applied.
struct ResolvedBuild {
  config: EngineConfig,
  initial_scene: Box<dyn Scene>,
  vsync: bool,
  master_volume: f32,
  music_volume: f32,
  sfx_volume: f32,
  disable_debug_keys: bool,
  game_name: Option<String>,
  game_version: Option<String>,
  console_builder: Option<ConsoleCommandBuilder>,
  pending_resources: Vec<WorldSetup>,
  schedules: EngineSchedules,
  /// The player's saved input bindings, when bindings are a user preference
  saved_bindings: Option<std::collections::BTreeMap<String, Vec<String>>>,
  /// Where changed bindings are saved (`None`: headless, web or not enabled)
  bindings_saver: Option<crate::core::config::SaveManager>,
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

/// Saved preferences, seeding `system.json` from `seed` when it doesn't exist.
/// Errors are reported and treated as "no saved preferences".
fn load_user_preferences(
  workspace: &crate::core::config::Workspace,
  filter: &crate::core::config::UserPreferenceFilter,
  seed: &crate::core::config::resolve::ResolvedSettings,
) -> Option<crate::core::config::resolve::StoredPreferences> {
  let defaults = &workspace.config().system;
  let result = workspace
    .create_save_manager()
    .map_err(|e| e.to_string())
    .and_then(|manager| {
      manager
        .load_preferences_or_seed(
          &seed.to_system_state(defaults.env.clone()),
          filter,
          defaults,
        )
        .map_err(|e| e.to_string())
    });
  match result {
    Ok(prefs) => prefs,
    Err(e) => {
      eprintln!("[ryo2d] Warning: Couldn't load user preferences ({e}), using project settings");
      None
    }
  }
}

#[cfg(test)]
mod tests;
