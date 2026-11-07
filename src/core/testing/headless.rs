//! `HeadlessEngine`: a real engine without a window.

use std::time::Duration;

use glam::IVec2;
use image::RgbaImage;
use winit::event::{MouseButton, MouseScrollDelta};
use winit::keyboard::KeyCode;

use super::ui::{UiCacheStats, UiSnapshot, runs_snapshot, ui_snapshot};
use super::{HeadlessError, shared_gpu};
use crate::core::asset::AssetsView;
use crate::core::audio::{AudioEngine, AudioLogEntry};
use crate::core::ecs::kernel::Messages;
use crate::core::ecs::{Entity, World};
use crate::core::engine::{Engine, EngineBuilder, FrameOutcome};
use crate::core::graphics::{ScreenMapping, WgpuRenderer};
use crate::core::input::{GamepadButton, InputEvent, Stick};
use crate::core::render::UiRenderStats;
use crate::core::ui::nav::nav_targets;
use crate::core::ui::widgets::scroll::drawn_bars;
use crate::core::ui::{Rect, UiEvent, UiId};

/// Largest supported render target side, in pixels.
const MAX_DIMENSION: u32 = 8192;

/// How the offscreen window stand-in is made.
#[derive(Debug, Clone, Copy)]
pub struct OffscreenOptions {
  pub(crate) format: wgpu::TextureFormat,
}

impl Default for OffscreenOptions {
  fn default() -> Self {
    Self {
      format: wgpu::TextureFormat::Rgba8UnormSrgb,
    }
  }
}

/// Offscreen options with a chosen target format, for testing output that
/// is not sRGB.
pub fn offscreen_with_format(format: wgpu::TextureFormat) -> OffscreenOptions {
  OffscreenOptions { format }
}

/// Whether the scene is still running after a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepOutcome {
  /// The scene is still running
  Running,
  /// The scene (or console) asked to quit; further steps do nothing
  Quit,
}

/// The engine running without a window. See the [module docs](crate::testing).
pub struct HeadlessEngine {
  pub(super) engine: Engine,
  presented: u64,
  quit: bool,
  pending_releases: Vec<KeyCode>,
  pending_pad_releases: Vec<GamepadButton>,
  /// The node cache's debug spot checks, set before each step when asked
  /// for (`None`: as the engine has them, on in debug builds)
  ui_spot_check: Option<bool>,
}

impl HeadlessEngine {
  /// Build the engine headless and enter its initial scene.
  ///
  /// # Panics
  ///
  /// Panics if the builder has no start scene (`EngineBuilder::start_scene`).
  pub fn new(builder: EngineBuilder) -> Result<Self, HeadlessError> {
    Self::new_with(builder, OffscreenOptions::default())
  }

  /// [`new`](Self::new) with offscreen options (a different target format).
  pub fn new_with(
    builder: EngineBuilder,
    options: OffscreenOptions,
  ) -> Result<Self, HeadlessError> {
    Self::start(builder, options, 1.0)
  }

  /// A headless engine whose window has `scale_factor` physical pixels per
  /// logical pixel (a HiDPI display at 2.0).
  pub fn with_scale_factor(
    builder: EngineBuilder,
    scale_factor: f64,
  ) -> Result<Self, HeadlessError> {
    Self::start(builder, OffscreenOptions::default(), scale_factor)
  }

  /// [`with_scale_factor`](Self::with_scale_factor), or `None` (with a note)
  /// when this machine has no GPU.
  pub fn new_or_skip_with_scale_factor(builder: EngineBuilder, scale_factor: f64) -> Option<Self> {
    Self::or_skip(Self::with_scale_factor(builder, scale_factor))
  }

  /// The window starts at the game resolution in logical pixels.
  fn start(
    builder: EngineBuilder,
    options: OffscreenOptions,
    scale_factor: f64,
  ) -> Result<Self, HeadlessError> {
    let (device, queue) = shared_gpu()?;
    let mut engine = builder.build_headless();
    // On before the start scene enters, so what it plays is in the log
    if let Some(mut audio) = engine.world_mut().get_resource_mut::<AudioEngine>() {
      audio.set_logging(true);
    }
    let (width, height) = engine.game_size();
    if !(1..=MAX_DIMENSION).contains(&width) || !(1..=MAX_DIMENSION).contains(&height) {
      return Err(HeadlessError::Render(format!(
        "invalid resolution {width}x{height} (each side must be 1..={MAX_DIMENSION})"
      )));
    }
    let physical = (
      (f64::from(width) * scale_factor).round().max(1.0) as u32,
      (f64::from(height) * scale_factor).round().max(1.0) as u32,
    );
    let mapping = ScreenMapping::new(engine.presentation(), physical, scale_factor);
    let mut renderer = WgpuRenderer::new_offscreen_mapped(device, queue, mapping, options.format);
    renderer.keep_scene_image();
    engine.init_gpu(renderer, scale_factor);
    Ok(Self {
      engine,
      presented: 0,
      quit: false,
      pending_releases: Vec::new(),
      pending_pad_releases: Vec::new(),
      ui_spot_check: None,
    })
  }

  /// Like [`new`](Self::new), but returns `None` (and prints a skip line) when
  /// there is no GPU, so tests can `return` early. Panics on any other error.
  pub fn new_or_skip(builder: EngineBuilder) -> Option<Self> {
    Self::new_or_skip_with(builder, OffscreenOptions::default())
  }

  /// [`new_or_skip`](Self::new_or_skip) with offscreen options.
  pub fn new_or_skip_with(builder: EngineBuilder, options: OffscreenOptions) -> Option<Self> {
    Self::or_skip(Self::new_with(builder, options))
  }

  fn or_skip(engine: Result<Self, HeadlessError>) -> Option<Self> {
    match engine {
      Ok(engine) => Some(engine),
      Err(HeadlessError::NoGpu) => {
        eprintln!("skipping: no GPU available for HeadlessEngine");
        None
      }
      Err(error) => panic!("failed to start HeadlessEngine: {error}"),
    }
  }

  /// Run one frame: exactly one fixed update, then render.
  pub fn step(&mut self) -> Result<StepOutcome, HeadlessError> {
    if self.quit {
      return Ok(StepOutcome::Quit);
    }
    if let Some(on) = self.ui_spot_check {
      crate::core::ui::nodes::set_spot_check(self.engine.world_mut(), on);
    }
    let dt = self.engine.fixed_timestep();
    let outcome = self.engine.frame(Some(dt));
    self.engine.apply_headless_requests();
    for key in self.pending_releases.drain(..) {
      self.engine.input_mut().queue(InputEvent::Key {
        key,
        pressed: false,
      });
    }
    for button in self.pending_pad_releases.drain(..) {
      self.engine.input_mut().queue(InputEvent::PadButton {
        button,
        pressed: false,
      });
    }
    match outcome {
      FrameOutcome::Presented => {
        self.presented += 1;
        Ok(StepOutcome::Running)
      }
      FrameOutcome::Skipped => Ok(StepOutcome::Running),
      FrameOutcome::Quit => {
        self.quit = true;
        Ok(StepOutcome::Quit)
      }
      FrameOutcome::Fatal(error) => Err(HeadlessError::Render(error)),
    }
  }

  /// Run up to `frames` frames, stopping early if the scene quits.
  pub fn step_frames(&mut self, frames: u32) -> Result<StepOutcome, HeadlessError> {
    for _ in 0..frames {
      if self.step()? == StepOutcome::Quit {
        return Ok(StepOutcome::Quit);
      }
    }
    Ok(StepOutcome::Running)
  }

  /// Number of frames presented so far.
  pub fn frame_count(&self) -> u64 {
    self.presented
  }

  /// The last presented frame (scene + UI) at game resolution, origin top-left.
  ///
  /// Returns an error if no frame has been presented yet.
  pub fn frame(&self) -> Result<RgbaImage, HeadlessError> {
    if self.presented == 0 {
      return Err(HeadlessError::Render("no frame presented yet".to_string()));
    }
    Ok(self.engine.read_frame()?)
  }

  /// Resize as if the window became `width x height`; the game is scaled
  /// into it.
  pub fn resize(&mut self, width: u32, height: u32) {
    self.engine.resize_window(width, height);
  }

  /// The last presented window, bars included (`frame` is the scene image
  /// inside them).
  pub fn window_frame(&self) -> Result<RgbaImage, HeadlessError> {
    if self.presented == 0 {
      return Err(HeadlessError::Render("no frame presented yet".to_string()));
    }
    Ok(self.engine.read_window()?)
  }

  /// Move the pointer to `(x, y)` physical window pixels, mapped to screen
  /// space like a real cursor (over the bars too).
  pub fn mouse_move_window(&mut self, x: f32, y: f32) {
    self.engine.cursor_moved(f64::from(x), f64::from(y));
  }

  /// The ECS world, for inspecting entities between steps.
  pub fn world(&self) -> &World {
    self.engine.world()
  }

  /// The ECS world, for spawning or changing entities between steps.
  pub fn world_mut(&mut self) -> &mut World {
    self.engine.world_mut()
  }

  /// The asset registry, as scenes reach it with `ctx.assets()`: load, get
  /// and add assets between steps. Queued loads arrive in a step's pump
  pub fn assets(&mut self) -> AssetsView<'_> {
    AssetsView::new(self.engine.world_mut())
  }

  /// Hold `key` down from the next step until [`release_key`](Self::release_key).
  pub fn press_key(&mut self, key: KeyCode) {
    self
      .engine
      .input_mut()
      .queue(InputEvent::Key { key, pressed: true });
  }

  /// Release `key`; the next step sees it as just released.
  pub fn release_key(&mut self, key: KeyCode) {
    self.engine.input_mut().queue(InputEvent::Key {
      key,
      pressed: false,
    });
  }

  /// Deliver typed text, as a window's text event does alongside a key press
  /// (`press_key` and `tap_key` type nothing).
  pub fn type_text(&mut self, text: &str) {
    self
      .engine
      .input_mut()
      .queue(InputEvent::Text(text.to_string()));
  }

  /// Press `key` for the next step only; it is released after that step.
  pub fn tap_key(&mut self, key: KeyCode) {
    self.press_key(key);
    self.pending_releases.push(key);
  }

  /// Hold a mouse button down from the next step.
  pub fn press_mouse(&mut self, button: MouseButton) {
    self.engine.input_mut().queue(InputEvent::MouseButton {
      button,
      pressed: true,
    });
  }

  /// Release a mouse button.
  pub fn release_mouse(&mut self, button: MouseButton) {
    self.engine.input_mut().queue(InputEvent::MouseButton {
      button,
      pressed: false,
    });
  }

  /// Move the mouse to `(x, y)` in game coordinates.
  pub fn mouse_move(&mut self, x: f32, y: f32) {
    self.engine.input_mut().queue(InputEvent::MouseMove {
      x: f64::from(x),
      y: f64::from(y),
    });
  }

  /// Hold a gamepad button down from the next step (a virtual pad connects if none is).
  pub fn press_pad(&mut self, button: GamepadButton) {
    self.engine.input_mut().queue(InputEvent::PadButton {
      button,
      pressed: true,
    });
  }

  /// Release a gamepad button.
  pub fn release_pad(&mut self, button: GamepadButton) {
    self.engine.input_mut().queue(InputEvent::PadButton {
      button,
      pressed: false,
    });
  }

  /// Press a gamepad button for the next step only; it is released after that step.
  pub fn tap_pad(&mut self, button: GamepadButton) {
    self.press_pad(button);
    self.pending_pad_releases.push(button);
  }

  /// Hold a stick at `(x, y)` (-1..1 each axis, +y up) until set again.
  pub fn set_stick(&mut self, stick: Stick, x: f32, y: f32) {
    self
      .engine
      .input_mut()
      .queue(InputEvent::PadStick { stick, x, y });
  }

  /// Turn the mouse wheel by `(dx, dy)` lines in the next step.
  pub fn wheel(&mut self, dx: f32, dy: f32) {
    self
      .engine
      .input_mut()
      .queue(InputEvent::Scroll(MouseScrollDelta::LineDelta(dx, dy)));
  }

  /// Move a trackpad by `(dx, dy)` logical pixels in the next step (+y
  /// scrolls up); the logical form, so the window's scale factor is not
  /// applied (queue `InputEvent::Scroll` with a `PixelDelta` for winit's
  /// physical pixels).
  pub fn trackpad(&mut self, dx: f32, dy: f32) {
    self
      .engine
      .input_mut()
      .queue(InputEvent::ScrollPixels { x: dx, y: dy });
  }

  /// The UI widget with the keyboard and gamepad focus, as the last step
  /// resolved it (`None` when nothing has it).
  pub fn ui_focused(&self) -> Option<UiId> {
    let focused = self.engine.ui_memory().interact.focused;
    (focused != UiId::NONE).then_some(focused)
  }

  /// What directional navigation can reach in the top scene, as last drawn:
  /// each focusable widget (enabled, shown, in a focus scope, not
  /// `nav_skip`) and its rect in UI units, by draw order. While a modal or
  /// another trapping scope is open, only the widgets inside it.
  pub fn ui_focusables(&self) -> Vec<(UiId, Rect)> {
    nav_targets(self.engine.ui_memory(), self.engine.top_scope())
  }

  /// Give widget `id` the focus at the next step's focus resolve, as
  /// `UiContext::request_focus` does (when it can take it then).
  pub fn focus_ui(&mut self, id: UiId) {
    self.engine.ui_memory_mut().request_focus(id);
  }

  /// Everything about the last frame's UI that a player or the game can see,
  /// in `Debug` form, as `UiHarness::frame_snapshot`: the draw commands,
  /// every record, the focus, the node events (those not yet dropped by a
  /// message update), the interaction, the widget states and their values,
  /// the open modals, the UI actions navigation used, the captured rebinds,
  /// the input the UI took and every UI node's components. Two runs that
  /// differ in any of these differ here.
  #[doc(hidden)]
  pub fn ui_snapshot(&self) -> String {
    let engine = &self.engine;
    let events: Vec<UiEvent> = engine
      .world()
      .get_resource::<Messages<UiEvent>>()
      .map(|messages| messages.iter_current_update_messages().cloned().collect())
      .unwrap_or_default();
    let log = engine.ui_pass_log();
    ui_snapshot(&UiSnapshot {
      commands: engine.ui_commands(),
      memory: engine.ui_memory(),
      events: &events,
      outcome: &log.used,
      rebinds: &log.rebinds,
      consumed: &log.consumed.consumed(),
      world: engine.world(),
    })
  }

  /// Turn the node cache's debug spot checks on or off from the next step,
  /// as `UiHarness::set_ui_spot_check`: each frame they build a clean node
  /// in place of its replay and capture a kept fragment in place of its
  /// copy, comparing each with what was kept. On in the engine's debug
  /// builds; release builds have none.
  #[doc(hidden)]
  pub fn set_ui_spot_check(&mut self, on: bool) {
    self.ui_spot_check = Some(on);
  }

  /// The last frame's fragment runs (each kept fragment's commands, tagged
  /// for the UI image's retention) in `Debug` form: a `runs` line (how many,
  /// and whether they hold their draws), then each run's tag. What
  /// [`ui_snapshot`](Self::ui_snapshot) leaves out, since with fragments off
  /// there are none.
  #[doc(hidden)]
  pub fn ui_runs_snapshot(&self) -> String {
    runs_snapshot(self.engine.ui_commands())
  }

  /// The last frame's scroll slots, slot 0 (no scroll area) first: each
  /// one's clip (UI units, absolute; `None` for slot 0) and where its
  /// contents' origin is drawn (whole render pixels). A scroll area that
  /// scrolls moves its slot's origin.
  #[doc(hidden)]
  pub fn ui_scroll_slots(&self) -> Vec<(Option<Rect>, [i32; 2])> {
    self
      .engine
      .ui_commands()
      .slots()
      .iter()
      .map(|slot| (slot.clip, slot.offset))
      .collect()
  }

  /// The scrollbars the last frame drew whose thumbs show: each scroll
  /// area's id, the axis its bar scrolls (0 sideways, 1 down), the bar's
  /// track and its thumb, in UI units, absolute, in draw order. A press on
  /// the thumb grabs it, and the drag follows the pointer along the track.
  #[doc(hidden)]
  pub fn ui_scrollbars(&self) -> Vec<(UiId, usize, Rect, Rect)> {
    drawn_bars(self.engine.ui_memory())
  }

  /// What the node walk did in the last UI pass: the nodes it built (and
  /// which), and how many tree nodes it replayed from records
  #[doc(hidden)]
  pub fn ui_cache_stats(&self) -> UiCacheStats {
    UiCacheStats::of(self.engine.world(), self.engine.ui_memory())
  }

  /// What the UI's render step did in the last frame: whether it kept the
  /// UI image, the texts it built and reused
  #[doc(hidden)]
  pub fn ui_render_stats(&self) -> UiRenderStats {
    self.engine.ui_render_stats()
  }

  /// The draws the last frame's `ui` pass was given: none when the UI image
  /// was kept or the UI drew nothing
  #[doc(hidden)]
  pub fn ui_draw_count(&self) -> u32 {
    self.engine.ui_draw_count()
  }

  /// The engine's debug switches, for a test to turn an overlay on
  #[cfg(test)]
  pub(crate) fn debug_config_mut(&mut self) -> &mut crate::core::diagnostics::DebugConfig {
    self.engine.debug_config_mut()
  }

  /// The engine's UI renderer (tests)
  #[cfg(test)]
  pub(crate) fn ui_renderer(&self) -> Option<&crate::core::render::ui_renderer::UiRenderer> {
    self.engine.ui_renderer()
  }

  /// The generation of glyph page `page` (`None`: no such page), changed
  /// when an eviction clears it (tests)
  #[cfg(test)]
  pub(crate) fn glyph_page_generation(
    &self,
    page: crate::core::graphics::GlyphPage,
  ) -> Option<u64> {
    self.engine.sprite_gpu.as_ref()?.glyph_page_generation(page)
  }

  /// The audio log: each voice's start, stop and changes, oldest first (the
  /// latest 4096). Headless engines play on kira's mock backend, silently,
  /// with the log on from the start
  #[doc(hidden)]
  pub fn audio_log(&self) -> Vec<AudioLogEntry> {
    self
      .engine
      .world()
      .get_resource::<AudioEngine>()
      .map(|audio| audio.log().to_vec())
      .unwrap_or_default()
  }

  /// How many textures the renderer keeps: images', named pixels' and
  /// render targets' (tests that watch an image's texture go with it)
  #[doc(hidden)]
  pub fn texture_count(&self) -> usize {
    self
      .engine
      .sprite_gpu
      .as_ref()
      .map_or(0, |gpu| gpu.texture_count())
  }

  /// How many times an image's pixels have been written to the GPU (tests
  /// that watch what is uploaded again)
  #[doc(hidden)]
  pub fn image_uploads(&self) -> u64 {
    self
      .engine
      .sprite_gpu
      .as_ref()
      .map_or(0, |gpu| gpu.image_uploads())
  }

  /// Whether image `id`'s new version is being written into a new texture
  /// over frames while its old texture draws (benches that measure a reload
  /// until its new pixels are drawn)
  #[doc(hidden)]
  pub fn image_upload_pending(
    &self,
    id: crate::core::asset::AssetId<crate::core::graphics::Image>,
  ) -> bool {
    self
      .engine
      .sprite_gpu
      .as_ref()
      .is_some_and(|gpu| gpu.upload_pending(id))
  }

  /// How many images are being written into new textures over frames
  /// (tests that watch a dropped one go)
  #[cfg(test)]
  pub(crate) fn spread_uploads(&self) -> usize {
    self
      .engine
      .sprite_gpu
      .as_ref()
      .map_or(0, |gpu| gpu.spread_uploads())
  }

  /// Write at most `bytes` of image pixels a frame into textures being
  /// replaced (tests that spread a small image over many frames)
  #[cfg(test)]
  pub(crate) fn set_upload_budget(&mut self, bytes: u64) {
    if let Some(gpu) = self.engine.sprite_gpu.as_mut() {
      gpu.set_upload_budget(bytes);
    }
  }

  /// Give every image a texture of its own from now on, as a full atlas
  /// does (tests of that fallback)
  #[cfg(test)]
  pub(crate) fn refuse_atlas_pages(&mut self) {
    if let Some(gpu) = self.engine.sprite_gpu.as_mut() {
      gpu.refuse_atlas_pages();
    }
  }

  /// How many pipelines game shaders have: one per blend mode a sprite
  /// shader is drawn with, and one per custom effect's source (tests that
  /// watch a freed shader's pipelines go)
  #[doc(hidden)]
  pub fn shader_pipeline_count(&self) -> usize {
    self.engine.shader_pipeline_count()
  }

  /// Whether game shader `id`'s current version waits to be drawn: its
  /// pipeline is not yet made and accepted by the GPU, by the sprite
  /// renderer or an effect stage that draws it (benches that measure a
  /// reload until it is drawn)
  #[doc(hidden)]
  pub fn shader_reload_pending(
    &self,
    id: crate::core::asset::AssetId<crate::core::ecs::Shader>,
  ) -> bool {
    self.engine.shader_reload_pending(id)
  }

  /// Holds the next compile on every shader thread until released, or
  /// releases them (tests of what draws while a new version compiles)
  #[cfg(test)]
  pub(crate) fn hold_shader_compiles(&mut self, hold: bool) {
    for gate in self.engine.shader_compile_gates() {
      gate.hold(hold);
    }
  }

  /// The shader threads' gates, to release them once the engine is gone
  /// (tests)
  #[cfg(test)]
  pub(crate) fn shader_compile_gates(
    &self,
  ) -> Vec<std::sync::Arc<crate::core::shaders::compile::CompileGate>> {
    self.engine.shader_compile_gates()
  }

  /// Compiles sent to the shader threads so far (tests)
  #[cfg(test)]
  pub(crate) fn shader_compiles_started(&self) -> usize {
    self.engine.shader_compiles().0
  }

  /// Compiles out on the shader threads (tests)
  #[cfg(test)]
  pub(crate) fn shader_compiles_out(&self) -> usize {
    self.engine.shader_compiles().1
  }

  /// How many glyphs of `font` the glyph atlas holds (tests that watch a
  /// freed font's glyphs go)
  #[doc(hidden)]
  pub fn glyph_count(&self, font: crate::core::text::FontId) -> usize {
    self
      .engine
      .sprite_gpu
      .as_ref()
      .map_or(0, |gpu| gpu.glyphs_of(font))
  }

  /// How many glyph pages (and their textures) there are, of every format
  /// (tests that watch an emptied page freed)
  #[doc(hidden)]
  pub fn glyph_page_count(&self) -> usize {
    self
      .engine
      .sprite_gpu
      .as_ref()
      .map_or(0, |gpu| gpu.glyph_page_count())
  }

  /// How many textures have room in the atlas: a rect on a page or a
  /// standalone texture (tests that watch an image's room freed)
  #[doc(hidden)]
  pub fn atlas_rects(&self) -> usize {
    self
      .engine
      .sprite_gpu
      .as_ref()
      .map_or(0, |gpu| gpu.atlas_rects())
  }

  /// The tile chunks the batch holds built, each as its layer entity and
  /// chunk, sorted (tests that watch what a view built and culled)
  #[doc(hidden)]
  pub fn tile_chunks(&self) -> Vec<(Entity, IVec2)> {
    let mut chunks: Vec<(Entity, IVec2)> = self
      .engine
      .batch
      .as_ref()
      .map(|batch| batch.chunks().built_chunks().collect())
      .unwrap_or_default();
    chunks.sort_by_key(|&(layer, chunk)| (layer, chunk.y, chunk.x));
    chunks
  }

  /// The tile chunks the batch holds built with a tile leaving its cell's
  /// box, each as its layer entity and chunk, sorted: only a layer drawn in
  /// Tiled's order builds its chunks knowing that (benches that hold a map
  /// to that path)
  #[doc(hidden)]
  pub fn spilling_tile_chunks(&self) -> Vec<(Entity, IVec2)> {
    let mut chunks: Vec<(Entity, IVec2)> = self
      .engine
      .batch
      .as_ref()
      .map(|batch| batch.chunks().spilling_chunks().collect())
      .unwrap_or_default();
    chunks.sort_by_key(|&(layer, chunk)| (layer, chunk.y, chunk.x));
    chunks
  }

  /// The last frame's tile chunk draws, in draw order, each as its layer
  /// entity, chunk, tileset, and the instances of the tileset's part of the
  /// chunk it draws (the first, and how many) (tests of how a layer's
  /// chunks draw)
  #[doc(hidden)]
  pub fn tile_draws(&self) -> Vec<(Entity, IVec2, u16, u32, u32)> {
    self.engine.tile_drawer.chunk_draws().collect()
  }

  /// The last frame's render step on the CPU (building, uploading and
  /// encoding the frame; not the UI pass)
  #[doc(hidden)]
  pub fn render_time(&self) -> Duration {
    self.engine.render_time()
  }

  /// The last frame's UI pass on the CPU: the scenes' UI, the node walk,
  /// layout, resolve and the overlays (not the render side)
  #[doc(hidden)]
  pub fn ui_pass_time(&self) -> Duration {
    self.engine.ui_pass_time()
  }
}
