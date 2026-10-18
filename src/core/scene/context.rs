//! The context scene callbacks receive.

use crate::core::asset::{AssetManager, SpriteHandle};
use crate::core::audio::Audio;
use crate::core::console::{CommandContext, CommandRegistry, CommandResult};
use crate::core::cursor::{CursorRequest, CursorStyle, CustomCursorData};
use crate::core::ecs::{
  Bundle, Entity, IntoScheduleConfigs, Message, Messages, Mut, Mutable, Resource, ScheduleSystem,
  World, WorldExt,
};
use crate::core::graphics::{CameraMut, RenderTargets, ScreenMapping};
use crate::core::input::{Actions, Input};
use crate::core::lighting::LightingSettings;
use crate::core::messages::{SceneMessages, register_message};
use crate::core::physics::Physics;
use crate::core::render::Look;
use crate::core::render::{DebugDraw, RenderPipeline};
use crate::core::schedule::{Phase, SceneScheduleError, SceneSchedules};
use crate::core::shaders::EffectsManager;
use crate::core::text::Fonts;
use crate::core::tilemap::TilemapManager;
use crate::core::time::Time;
use crate::core::ui::{BindValue, UiId, UiNode, UiTheme, nodes};
use crate::core::window::{DisplayMode, WindowInfo, WindowRequest};

#[cfg(test)]
use super::SceneSlot;

/// What a scene callback can reach.
///
/// `time`, `input` and `assets` are the engine's resources, taken
/// out of the world for the duration of the callback (systems see them as
/// resources as usual). Everything else lives in `world`; the helpers below
/// are shorthands for it. The engine inserts every resource the helpers read
/// before any scene runs.
pub struct SceneContext<'a> {
  pub world: &'a mut World,
  pub time: &'a mut Time,
  pub input: &'a mut Input,
  pub assets: &'a mut AssetManager,
  pub audio: &'a mut Audio,
  schedules: &'a mut SceneSchedules,
  messages: &'a mut SceneMessages,
  scope_id: u64,
}

impl SceneContext<'_> {
  /// Build a context for `slot` over `world`, run `f`, put the scoped resources back.
  ///
  /// Panics if `Time`, `Input` or `AssetManager` is not a resource (the
  /// engine inserts all three at startup).
  #[cfg(test)]
  pub(crate) fn scoped<R>(
    world: &mut World,
    audio: &mut Audio,
    slot: &mut SceneSlot,
    f: impl FnOnce(&mut SceneContext<'_>) -> R,
  ) -> R {
    Self::scoped_parts(
      world,
      audio,
      &mut slot.schedules,
      &mut slot.messages,
      slot.scope_id,
      f,
    )
  }

  /// `scoped` over a slot's parts, so the caller can hold the slot's scene too.
  pub(super) fn scoped_parts<R>(
    world: &mut World,
    audio: &mut Audio,
    schedules: &mut SceneSchedules,
    messages: &mut SceneMessages,
    scope_id: u64,
    f: impl FnOnce(&mut SceneContext<'_>) -> R,
  ) -> R {
    world.resource_scope(|world, mut time: Mut<Time>| {
      world.resource_scope(|world, mut input: Mut<Input>| {
        world.resource_scope(|world, mut assets: Mut<AssetManager>| {
          let mut ctx = SceneContext {
            world,
            time: &mut time,
            input: &mut input,
            assets: &mut assets,
            audio,
            schedules,
            messages,
            scope_id,
          };
          f(&mut ctx)
        })
      })
    })
  }

  // --- systems and messages ---

  /// Add systems that run while this scene is active (or below an
  /// `updates_below` scene) and go away with it.
  ///
  /// Only `Phase::FixedUpdate` and `Phase::Update` take scene systems.
  pub fn add_systems<M>(
    &mut self,
    phase: Phase,
    systems: impl IntoScheduleConfigs<ScheduleSystem, M>,
  ) -> Result<(), SceneScheduleError> {
    self.schedules.add_systems(phase, systems)
  }

  /// Every `T` written since this scene last read them.
  pub fn messages<T: Message + Clone>(&mut self) -> Vec<T> {
    self.messages.read::<T>(self.world)
  }

  /// The UI events of this scene's own nodes since it last read them. It
  /// shares `messages::<UiEvent>()`'s cursor: read one or the other.
  pub fn ui_events(&mut self) -> Vec<nodes::UiEvent> {
    let scene = self.scope_id;
    let mut events = self.messages::<nodes::UiEvent>();
    events.retain(|event| event.scene == scene);
    events
  }

  /// Write a message for systems and scenes to read; registers `T` on first use.
  pub fn write_message<T: Message>(&mut self, message: T) {
    register_message::<T>(self.world);
    self.world.resource_mut::<Messages<T>>().write(message);
  }

  // --- resources ---

  pub fn resource<T: Resource>(&self) -> Option<&T> {
    self.world.get_resource::<T>()
  }

  pub fn resource_mut<T: Resource<Mutability = Mutable>>(&mut self) -> Option<Mut<'_, T>> {
    self.world.get_resource_mut::<T>()
  }

  /// The resource, inserting `T::default()` first if it is missing.
  pub fn resource_or_default<T: Resource<Mutability = Mutable> + Default>(&mut self) -> Mut<'_, T> {
    self.world.get_resource_or_insert_with(T::default)
  }

  pub fn insert_resource<T: Resource>(&mut self, value: T) {
    self.world.insert_resource(value);
  }

  /// The game's actions and their bindings ([`Actions`]): bind "jump" here and
  /// read it with `ctx.input.action_pressed("jump")`
  pub fn actions(&mut self) -> &mut Actions {
    self.input.actions_mut()
  }

  /// Screen-wide post-process effects.
  pub fn effects(&mut self) -> Mut<'_, EffectsManager> {
    self.world.resource_mut::<EffectsManager>()
  }

  /// The simulation: forces and impulses, queries and contact state.
  pub fn physics(&self) -> &Physics {
    self.world.resource::<Physics>()
  }

  /// The simulation, to apply forces and impulses (queued for the next step).
  pub fn physics_mut(&mut self) -> Mut<'_, Physics> {
    self.world.resource_mut::<Physics>()
  }

  /// The frame's look: exposure, the tonemap curve and colour grading.
  pub fn look(&mut self) -> Mut<'_, Look> {
    self.world.resource_mut::<Look>()
  }

  /// Whether lighting runs (forced on or off), ambient light and atmosphere.
  /// Volumetric lighting is per light (`Light2D`).
  pub fn lighting(&mut self) -> Mut<'_, LightingSettings> {
    self.world.resource_mut::<LightingSettings>()
  }

  /// The UI theme every scene's UI draws with. `set` or `modify` it and the
  /// change applies from the next frame.
  pub fn ui_theme(&mut self) -> Mut<'_, UiTheme> {
    self.world.resource_mut::<UiTheme>()
  }

  /// The scene's tilemaps: tile reads and writes, coordinate conversion and
  /// viewport culling, keyed by the tilemap's entity.
  pub fn tilemaps(&mut self) -> Mut<'_, TilemapManager> {
    self.world.resource_mut::<TilemapManager>()
  }

  /// The frame's render pipeline: toggle built-in passes or insert game passes.
  ///
  /// The pipeline belongs to the engine, so changes persist across scene
  /// transitions. A pass a scene inserts keeps running after the scene
  /// pops unless the scene removes it.
  pub fn render_pipeline(&mut self) -> Mut<'_, RenderPipeline> {
    self.world.resource_mut::<RenderPipeline>()
  }

  /// The fonts text can use ([`Fonts`]): add a font file or bytes, pick a
  /// font's render mode, choose the fonts the built-in roles use
  pub fn fonts(&mut self) -> Mut<'_, Fonts> {
    self.world.resource_mut::<Fonts>()
  }

  /// Debug lines, rectangles, circles, arrows and crosses over the world
  /// ([`DebugDraw`]): drawn until the next fixed step when added in
  /// `update`, for one frame otherwise.
  pub fn debug_draw(&mut self) -> Mut<'_, DebugDraw> {
    self.world.resource_mut::<DebugDraw>()
  }

  /// The render targets: textures cameras render into and sprites show.
  /// Changes reach the GPU at the next frame.
  pub fn render_targets(&mut self) -> Mut<'_, RenderTargets> {
    self.world.get_resource_or_init::<RenderTargets>()
  }

  /// Pending window changes (resolution, display mode, UI scale), applied
  /// after the frame.
  pub fn window(&mut self) -> Mut<'_, WindowRequest> {
    self.world.resource_mut::<WindowRequest>()
  }

  /// Pending cursor changes, applied after the frame.
  pub fn cursor(&mut self) -> Mut<'_, CursorRequest> {
    self.world.resource_mut::<CursorRequest>()
  }

  /// Console commands; handlers registered here go away with the scene.
  pub fn console(&mut self) -> ConsoleScope<'_> {
    ConsoleScope {
      registry: self.world.resource_mut::<CommandRegistry>().into_inner(),
      scope_id: self.scope_id,
    }
  }

  // --- UI nodes ---

  /// Spawn a UI node tree as a root owned by this scene; it is despawned when
  /// the scene leaves the stack.
  ///
  /// ```
  /// use ryo2d::prelude::*;
  ///
  /// fn open_menu(ctx: &mut SceneContext<'_>) {
  ///   let menu = ctx.spawn_ui(
  ///     UiNode::column().child(UiNode::button("Play").name("play")).child(UiNode::button("Quit")),
  ///   );
  ///   let play = ctx.find_ui_in(menu, "play");
  ///   # let _ = play;
  /// }
  /// ```
  pub fn spawn_ui(&mut self, tree: UiNode) -> Entity {
    nodes::spawn_ui(self.world, self.scope_id, tree)
  }

  /// Spawn a UI node tree under `parent`, after its last child; `None`
  /// (and a warning) if `parent` is not a container node.
  pub fn spawn_ui_in(&mut self, parent: Entity, tree: UiNode) -> Option<Entity> {
    nodes::spawn_ui_in(self.world, parent, tree)
  }

  /// A UI node's id once it has been drawn, to read its `ctx.ui().scroll_offset`
  /// and the like; `None` before that.
  pub fn ui_id(&self, node: Entity) -> Option<UiId> {
    self
      .world
      .get::<nodes::UiInteraction>(node)
      .and_then(nodes::UiInteraction::id)
  }

  /// Give a UI node the keyboard and gamepad focus once it is drawn (next
  /// frame, if it can take focus). A request for a node not drawn yet waits
  /// for it.
  pub fn focus_ui(&mut self, node: Entity) {
    self
      .world
      .get_resource_or_insert_with(nodes::UiNodes::default)
      .pending_focus = Some(node);
  }

  /// The first node named `name` in this scene's UI trees.
  pub fn find_ui(&self, name: &str) -> Option<Entity> {
    nodes::find_ui(self.world, self.scope_id, name)
  }

  /// The first node named `name` in the tree under `root` (included).
  pub fn find_ui_in(&self, root: Entity, name: &str) -> Option<Entity> {
    nodes::find_ui_in(self.world, root, name)
  }

  /// Set a named binding on the UI tree `node` belongs to: every widget bound
  /// to `name` and every label or tooltip showing it update. Values are kept
  /// per root, so two trees from one file never share them.
  pub fn ui_set(&mut self, node: Entity, name: &str, value: impl Into<BindValue>) {
    nodes::bindings::set(self.world, node, name, value.into());
  }

  /// A named binding's value on the UI tree `node` belongs to, as `T`
  /// (`f32`, `f64`, `i32`, `u32`, `usize`, `bool` or `String`)
  pub fn ui_get<T: TryFrom<BindValue>>(&self, node: Entity, name: &str) -> Option<T> {
    nodes::bindings::get(self.world, node, name).and_then(|value| T::try_from(value).ok())
  }

  // --- hierarchy and interpolation ---

  /// Spawn `bundle` as a child of `parent`.
  pub fn spawn_child(&mut self, parent: Entity, bundle: impl Bundle) -> Entity {
    self.world.spawn_child(parent, bundle)
  }

  /// Draw the entity at its new transform next frame instead of sliding there.
  pub fn skip_interpolation(&mut self, entity: Entity) {
    self.world.skip_interpolation(entity);
  }

  // --- screen and window ---

  /// The main camera: move, zoom, turn and shake it, and convert between
  /// screen and world positions.
  pub fn camera(&mut self) -> CameraMut<'_> {
    CameraMut::main_or_spawn(self.world)
  }

  /// The size of screen space (logical pixels, or the pixel-art target).
  pub fn screen_size(&self) -> (f32, f32) {
    let size = self.screen_size_vec();
    (size.x, size.y)
  }

  pub fn screen_width(&self) -> f32 {
    self.screen_size_vec().x
  }

  pub fn screen_height(&self) -> f32 {
    self.screen_size_vec().y
  }

  fn screen_size_vec(&self) -> glam::Vec2 {
    self
      .world
      .get_resource::<ScreenMapping>()
      .map_or(glam::Vec2::ONE, ScreenMapping::screen_size)
  }

  /// The window's DPI scale factor: 1.0 for standard displays, 2.0 for HiDPI.
  pub fn scale_factor(&self) -> f64 {
    self
      .world
      .get_resource::<WindowInfo>()
      .map_or(1.0, |info| info.scale_factor)
  }

  /// The current display mode (windowed, borderless, fullscreen).
  pub fn display_mode(&self) -> DisplayMode {
    self
      .world
      .get_resource::<WindowInfo>()
      .map(|info| info.display_mode)
      .unwrap_or_default()
  }

  /// Request a display mode change, applied after the frame.
  pub fn request_display_mode(&mut self, mode: DisplayMode) {
    self.window().display_mode = Some(mode);
  }

  /// Request a resolution change, applied after the frame.
  pub fn request_resolution(&mut self, width: u32, height: u32) {
    let mut window = self.window();
    window.width = Some(width);
    window.height = Some(height);
  }

  /// Request a resolution and display mode change together.
  pub fn request_display_change(&mut self, width: u32, height: u32, mode: DisplayMode) {
    let mut window = self.window();
    window.width = Some(width);
    window.height = Some(height);
    window.display_mode = Some(mode);
  }

  /// Request a UI scale in render pixels per UI unit, or `None` for the
  /// automatic scale (UI units equal to screen units at any display
  /// scale). Applied after the frame.
  pub fn request_ui_scale(&mut self, scale: Option<f32>) {
    self.window().ui_scale = Some(scale);
  }

  // --- cursor ---

  /// Set the mouse cursor style.
  pub fn set_cursor(&mut self, style: CursorStyle) {
    self.cursor().cursor = Some(style);
  }

  pub fn set_cursor_visible(&mut self, visible: bool) {
    self.cursor().visible = Some(visible);
  }

  pub fn show_cursor(&mut self) {
    self.set_cursor_visible(true);
  }

  pub fn hide_cursor(&mut self) {
    self.set_cursor_visible(false);
  }

  /// Keep the cursor inside the window. macOS and the web cannot confine a
  /// cursor: there it stays free and the engine warns once (see
  /// [`cursor_confinement_supported`](Self::cursor_confinement_supported)).
  pub fn set_cursor_confined(&mut self, confined: bool) {
    self.cursor().confined = Some(confined);
  }

  /// Whether [`set_cursor_confined`](Self::set_cursor_confined) can keep the
  /// cursor in the window here (not on macOS, iOS, Android or the web), for
  /// offering the option only where it works
  pub fn cursor_confinement_supported(&self) -> bool {
    crate::core::cursor::confinement_supported()
  }

  /// Lock the cursor in place for FPS-style mouse look; motion arrives as
  /// `input.mouse_delta()`.
  pub fn set_cursor_locked(&mut self, locked: bool) {
    self.cursor().locked = Some(locked);
  }

  /// Use a sprite as the cursor; the hotspot is the click point, in pixels
  /// from the sprite's top-left.
  pub fn set_custom_cursor(&mut self, sprite: &SpriteHandle, hotspot_x: u16, hotspot_y: u16) {
    let mut data = None;
    sprite.with_sprite(|s| {
      data = Some(CustomCursorData {
        rgba: s.data().to_vec(),
        width: s.width() as u16,
        height: s.height() as u16,
        hotspot_x,
        hotspot_y,
        id: sprite.id(),
      });
    });
    if let Some(data) = data {
      self.cursor().custom_cursor = Some(data);
    }
  }

  /// Use a sprite as the cursor with the hotspot at its centre.
  pub fn set_custom_cursor_centered(&mut self, sprite: &SpriteHandle) {
    let hotspot_x = (sprite.width() / 2) as u16;
    let hotspot_y = (sprite.height() / 2) as u16;
    self.set_custom_cursor(sprite, hotspot_x, hotspot_y);
  }
}

/// Scene-scoped console command registration.
pub struct ConsoleScope<'a> {
  registry: &'a mut CommandRegistry,
  scope_id: u64,
}

impl ConsoleScope<'_> {
  /// Declare a game command: it appears in help at once and does nothing
  /// until a handler is registered. Declarations are global and idempotent.
  pub fn declare(
    &mut self,
    name: impl Into<String>,
    description: impl Into<String>,
    verbose: Option<impl Into<String>>,
  ) {
    self.registry.declare(name, description, verbose);
  }

  /// Attach a handler to a declared command; it is removed when the scene
  /// leaves the stack. Returns `false` if the command was not declared.
  pub fn register<F>(&mut self, name: &str, handler: F) -> bool
  where
    F: Fn(&[&str], &mut CommandContext) -> CommandResult + Send + Sync + 'static,
  {
    self.registry.register_handler(name, self.scope_id, handler)
  }

  /// Remove a built-in engine command; returns `true` if it existed.
  pub fn remove_builtin(&mut self, name: &str) -> bool {
    self.registry.remove_builtin(name)
  }
}
