//! The scene stack.

use std::any::Any;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::core::audio::Audio;
use crate::core::console::CommandRegistry;
use crate::core::ecs::World;
use crate::core::ecs::kernel::CheckChangeTicks;
use crate::core::graphics::GpuRenderContext;
use crate::core::messages::SceneMessages;
use crate::core::schedule::SceneSchedules;
use crate::core::time::Time;
use crate::core::ui::UiContext;
use crate::profile_scope;

use super::cleanup::execute_cleanup;
use super::{PhysicsPaused, Scene, SceneAction, SceneCleanup, SceneContext, Transition};

/// Console scope ids, unique per scene instance for the whole process.
static NEXT_SCOPE_ID: AtomicU64 = AtomicU64::new(1);

/// A scene on the stack with what it owns: its systems, its message read
/// positions and its console scope.
pub(crate) struct SceneSlot {
  pub(crate) scene: Box<dyn Scene>,
  pub(crate) schedules: SceneSchedules,
  pub(crate) messages: SceneMessages,
  pub(crate) scope_id: u64,
  /// Set once this scene's action has been dropped and logged.
  warned_dropped_action: bool,
}

impl SceneSlot {
  pub(crate) fn new(scene: Box<dyn Scene>) -> Self {
    Self {
      scene,
      schedules: SceneSchedules::default(),
      messages: SceneMessages::default(),
      scope_id: NEXT_SCOPE_ID.fetch_add(1, Ordering::Relaxed),
      warned_dropped_action: false,
    }
  }

  /// Only the top scene's action applies: say so, once per scene, when a
  /// scene below returns one from `hook`.
  fn warn_if_dropped(&mut self, action: &SceneAction, hook: &str) {
    if matches!(action, SceneAction::Continue) || self.warned_dropped_action {
      return;
    }
    self.warned_dropped_action = true;
    crate::engine_warn!(
      "scene {} returned {} from {hook} below the top scene; only the top scene's action is applied (logged once)",
      self.scene.name(),
      action.kind()
    );
  }

  /// Run `f` with this slot's scene and a context built for it.
  fn call<R>(
    &mut self,
    world: &mut World,
    audio: &mut Audio,
    f: impl FnOnce(&mut dyn Scene, &mut SceneContext<'_>) -> R,
  ) -> R {
    let SceneSlot {
      scene,
      schedules,
      messages,
      scope_id,
      ..
    } = self;
    SceneContext::scoped_parts(world, audio, schedules, messages, *scope_id, |ctx| {
      f(scene.as_mut(), ctx)
    })
  }
}

/// Manages a stack of scenes.
///
/// The scene at the top of the stack is the active one. Scenes below also
/// update (and run their systems) while every scene above them
/// `updates_below`, and are drawn while every scene above them
/// `is_transparent`.
///
/// Scene changes can include visual transitions (fade, slide).
pub struct SceneManager {
  /// Stack of scenes (back = top)
  scenes: Vec<SceneSlot>,
  /// Active transition state
  transition_state: Option<TransitionState>,
}

/// Internal state for an in-progress transition.
#[derive(Debug)]
struct TransitionState {
  transition: Transition,
  elapsed: Duration,
  /// The action to perform at the midpoint (for fades) or start (for slides)
  pending_action: PendingAction,
  phase: TransitionPhase,
}

enum PendingAction {
  Push(Box<dyn Scene>),
  Pop,
  PopReturn(Box<dyn Any>),
  Switch(Box<dyn Scene>),
  Reset(Box<dyn Scene>),
}

impl std::fmt::Debug for PendingAction {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      PendingAction::Push(_) => write!(f, "Push(...)"),
      PendingAction::Pop => write!(f, "Pop"),
      PendingAction::PopReturn(_) => write!(f, "PopReturn(...)"),
      PendingAction::Switch(_) => write!(f, "Switch(...)"),
      PendingAction::Reset(_) => write!(f, "Reset(...)"),
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransitionPhase {
  /// Fading/sliding out the old scene
  Out,
  /// Fading/sliding in the new scene
  In,
}

/// Drop the console handlers a scene registered.
fn clear_console_scope(world: &mut World, scope_id: u64) {
  if let Some(mut registry) = world.get_resource_mut::<CommandRegistry>() {
    registry.clear_scope(scope_id);
  }
}

impl SceneManager {
  /// Clamp the change ticks of every stacked scene's schedules.
  pub(crate) fn check_change_ticks(&mut self, check: CheckChangeTicks) {
    for slot in &mut self.scenes {
      slot.schedules.check_change_ticks(check);
    }
  }

  /// Create a new empty scene manager.
  pub fn new() -> Self {
    SceneManager {
      scenes: Vec::new(),
      transition_state: None,
    }
  }

  /// Create a scene manager with an initial scene, entered by `enter_initial`.
  pub fn with_scene(scene: Box<dyn Scene>) -> Self {
    let mut manager = Self::new();
    manager.scenes.push(SceneSlot::new(scene));
    manager
  }

  /// Call `on_enter` for the initial scene. The engine calls this once the
  /// GPU and every resource exist.
  pub fn enter_initial(&mut self, world: &mut World, audio: &mut Audio) {
    self.enter_top(world, audio);
  }

  fn enter_top(&mut self, world: &mut World, audio: &mut Audio) {
    let Some(top) = self.scenes.last_mut() else {
      return;
    };
    top.messages = SceneMessages::starting_now(world);
    #[cfg(debug_assertions)]
    let start = web_time::Instant::now();
    top.call(world, audio, |scene, ctx| scene.on_enter(ctx));
    #[cfg(debug_assertions)]
    {
      let elapsed = start.elapsed();
      if elapsed.as_millis() > 5 {
        crate::engine_debug!("scene {} on_enter took {:?}", top.scene.name(), elapsed);
      }
    }
  }

  /// Check if a transition is currently in progress.
  pub fn is_transitioning(&self) -> bool {
    self.transition_state.is_some()
  }

  /// The current transition's progress (0.0 to 1.0), or `None` without one.
  pub fn transition_progress(&self) -> Option<f32> {
    self.transition_state.as_ref().map(|state| {
      let total = state.transition.duration();
      if total.is_zero() {
        1.0
      } else {
        (state.elapsed.as_secs_f32() / total.as_secs_f32()).min(1.0)
      }
    })
  }

  /// The fade overlay's opacity (0.0 to 1.0); 0.0 without a fade.
  pub fn transition_overlay_alpha(&self) -> f32 {
    match &self.transition_state {
      Some(state) => match &state.transition {
        Transition::None => 0.0,
        Transition::Fade { duration, .. } => {
          let half_duration = duration.as_secs_f32();
          let elapsed = state.elapsed.as_secs_f32();
          match state.phase {
            TransitionPhase::Out => (elapsed / half_duration).min(1.0),
            TransitionPhase::In => (1.0 - (elapsed / half_duration)).max(0.0),
          }
        }
        Transition::Slide { .. } => 0.0,
      },
      None => 0.0,
    }
  }

  /// The fade overlay's colour, if a fade is running.
  pub fn transition_overlay_color(&self) -> Option<[u8; 4]> {
    match &self.transition_state {
      Some(state) => match &state.transition {
        Transition::Fade { color, .. } => Some(*color),
        _ => None,
      },
      None => None,
    }
  }

  /// Push a new scene: the current top pauses, the new one enters.
  pub fn push(&mut self, scene: Box<dyn Scene>, world: &mut World, audio: &mut Audio) {
    profile_scope!("cpu:scene_push");
    if let Some(current) = self.scenes.last_mut() {
      current.call(world, audio, |scene, ctx| scene.on_pause(ctx));
    }
    self.scenes.push(SceneSlot::new(scene));
    self.enter_top(world, audio);
  }

  /// Pop the top scene: it exits, the new top resumes with `returned`.
  ///
  /// Returns the popped scene, or `None` if the stack was empty.
  pub fn pop(
    &mut self,
    world: &mut World,
    audio: &mut Audio,
    returned: Option<Box<dyn Any>>,
  ) -> Option<Box<dyn Scene>> {
    let mut slot = self.scenes.pop()?;
    slot.call(world, audio, |scene, ctx| scene.on_exit(ctx));
    clear_console_scope(world, slot.scope_id);
    if let Some(top) = self.scenes.last_mut() {
      top.call(world, audio, |scene, ctx| scene.on_resume(ctx, returned));
    }
    Some(slot.scene)
  }

  /// Replace the top scene: it exits, its cleanup runs, the new one enters.
  pub fn switch(&mut self, scene: Box<dyn Scene>, world: &mut World, audio: &mut Audio) {
    profile_scope!("cpu:scene_switch");
    if let Some(mut current) = self.scenes.pop() {
      let cleanup = current.scene.cleanup_on_exit();
      current.call(world, audio, |scene, ctx| scene.on_exit(ctx));
      clear_console_scope(world, current.scope_id);
      drop(current);
      execute_cleanup(cleanup, world, audio);
    }
    self.scenes.push(SceneSlot::new(scene));
    self.enter_top(world, audio);
  }

  /// Exit every scene, top to bottom.
  pub fn clear(&mut self, world: &mut World, audio: &mut Audio) {
    while let Some(mut slot) = self.scenes.pop() {
      slot.call(world, audio, |scene, ctx| scene.on_exit(ctx));
      clear_console_scope(world, slot.scope_id);
    }
  }

  /// Exit every scene, run the top scene's cleanup, then enter `scene`.
  pub fn reset(&mut self, scene: Box<dyn Scene>, world: &mut World, audio: &mut Audio) {
    let cleanup = self
      .scenes
      .last()
      .map_or(SceneCleanup::NONE, |slot| slot.scene.cleanup_on_exit());
    self.clear(world, audio);
    execute_cleanup(cleanup, world, audio);
    self.scenes.push(SceneSlot::new(scene));
    self.enter_top(world, audio);
  }

  /// The lowest scene that updates: the top, and below it while each scene
  /// above `updates_below`.
  fn update_from(&self) -> usize {
    let mut from = self.scenes.len().saturating_sub(1);
    while from > 0 && self.scenes[from].scene.updates_below() {
      from -= 1;
    }
    from
  }

  /// The lowest scene that is drawn: the top, and below it while each scene
  /// above `is_transparent`.
  fn render_from(&self) -> usize {
    let mut from = self.scenes.len().saturating_sub(1);
    while from > 0 && self.scenes[from].scene.is_transparent() {
      from -= 1;
    }
    from
  }

  /// One fixed step: advance any transition, then `Scene::update` the
  /// updating scenes bottom-up. Returns the top scene's action (`Quit` once
  /// the stack is empty); scenes do not update while a transition runs.
  pub fn update(&mut self, world: &mut World, audio: &mut Audio) -> SceneAction {
    let dt = world.resource::<Time>().fixed.timestep();
    if self.scenes.is_empty() {
      return SceneAction::Quit;
    }
    if self.is_transitioning() {
      return SceneAction::Continue;
    }
    let top = self.scenes.len() - 1;
    let mut action = SceneAction::Continue;
    for i in self.update_from()..=top {
      let slot = &mut self.scenes[i];
      let result = slot.call(world, audio, |scene, ctx| scene.update(ctx, dt));
      if i == top {
        action = result;
      } else {
        slot.warn_if_dropped(&result, "update");
      }
    }
    action
  }

  /// Apply a scene action. Returns `false` when the game should quit.
  pub fn handle_action(
    &mut self,
    action: SceneAction,
    world: &mut World,
    audio: &mut Audio,
  ) -> bool {
    match action {
      SceneAction::Continue => true,
      SceneAction::Push(scene) => {
        self.push(scene, world, audio);
        true
      }
      SceneAction::Pop => {
        self.pop(world, audio, None);
        !self.scenes.is_empty()
      }
      SceneAction::PopReturn(data) => {
        self.pop(world, audio, Some(data));
        !self.scenes.is_empty()
      }
      SceneAction::Switch(scene) => {
        self.switch(scene, world, audio);
        true
      }
      SceneAction::Reset(scene) => {
        self.reset(scene, world, audio);
        true
      }
      SceneAction::PushWith(scene, transition) => {
        self.start_transition(transition, PendingAction::Push(scene));
        true
      }
      SceneAction::PopWith(transition) => {
        self.start_transition(transition, PendingAction::Pop);
        true
      }
      SceneAction::PopReturnWith(data, transition) => {
        self.start_transition(transition, PendingAction::PopReturn(data));
        true
      }
      SceneAction::SwitchWith(scene, transition) => {
        self.start_transition(transition, PendingAction::Switch(scene));
        true
      }
      SceneAction::ResetWith(scene, transition) => {
        self.start_transition(transition, PendingAction::Reset(scene));
        true
      }
      SceneAction::Quit => false,
    }
  }

  fn start_transition(&mut self, transition: Transition, action: PendingAction) {
    if matches!(transition, Transition::None) {
      return;
    }
    self.transition_state = Some(TransitionState {
      transition,
      elapsed: Duration::ZERO,
      pending_action: action,
      phase: TransitionPhase::Out,
    });
  }

  /// Advance a running transition by `dt`, performing its scene change at
  /// the midpoint (fades) or start (slides). Returns `false` once the stack
  /// is empty.
  pub fn update_transition(&mut self, dt: f32, world: &mut World, audio: &mut Audio) -> bool {
    let dt = Duration::from_secs_f32(dt.max(0.0));
    let action_to_execute = if let Some(state) = &mut self.transition_state {
      state.elapsed += dt;
      match &state.transition {
        Transition::None => {
          self.transition_state = None;
          None
        }
        Transition::Fade { duration, .. } => {
          let duration = *duration;
          match state.phase {
            TransitionPhase::Out if state.elapsed >= duration => {
              state.elapsed = Duration::ZERO;
              state.phase = TransitionPhase::In;
              Some(std::mem::replace(
                &mut state.pending_action,
                PendingAction::Pop,
              ))
            }
            TransitionPhase::Out => None,
            TransitionPhase::In => {
              if state.elapsed >= duration {
                self.transition_state = None;
              }
              None
            }
          }
        }
        Transition::Slide { duration, .. } => {
          let duration = *duration;
          if state.phase == TransitionPhase::Out {
            state.phase = TransitionPhase::In;
            Some(std::mem::replace(
              &mut state.pending_action,
              PendingAction::Pop,
            ))
          } else {
            if state.elapsed >= duration {
              self.transition_state = None;
            }
            None
          }
        }
      }
    } else {
      None
    };

    if let Some(action) = action_to_execute {
      self.execute_pending_action(action, world, audio);
    }
    !self.scenes.is_empty()
  }

  fn execute_pending_action(
    &mut self,
    action: PendingAction,
    world: &mut World,
    audio: &mut Audio,
  ) {
    match action {
      PendingAction::Push(scene) => self.push(scene, world, audio),
      PendingAction::Pop => {
        self.pop(world, audio, None);
      }
      PendingAction::PopReturn(data) => {
        self.pop(world, audio, Some(data));
      }
      PendingAction::Switch(scene) => self.switch(scene, world, audio),
      PendingAction::Reset(scene) => self.reset(scene, world, audio),
    }
  }

  /// Run `Scene::ui` for the visible scenes, bottom-up. Returns the top
  /// scene's action.
  pub fn ui_pass(
    &mut self,
    world: &mut World,
    audio: &mut Audio,
    ui: &mut UiContext,
  ) -> SceneAction {
    if self.scenes.is_empty() {
      return SceneAction::Continue;
    }
    let top = self.scenes.len() - 1;
    let mut action = SceneAction::Continue;
    // Scenes under the top one draw first but must not take the pointer the
    // top scene's widgets sit over: they run with it occluded
    let occluded = ui.is_mouse_occluded();
    for i in self.render_from()..=top {
      if i < top {
        ui.set_mouse_occluded();
      } else {
        ui.restore_mouse_occluded(occluded);
      }
      let slot = &mut self.scenes[i];
      let result = slot.call(world, audio, |scene, ctx| scene.ui(ui, ctx));
      // The scene's open lists draw above everything it drew
      ui.finish_dropdowns();
      if i == top {
        action = result;
      } else {
        slot.warn_if_dropped(&result, "ui");
      }
    }
    ui.restore_mouse_occluded(occluded);
    action
  }

  /// Run `Scene::render` for the visible scenes, bottom-up.
  pub fn render(&mut self, gpu: &mut GpuRenderContext) {
    if self.scenes.is_empty() {
      return;
    }
    let from = self.render_from();
    for slot in &mut self.scenes[from..] {
      slot.scene.render(gpu);
    }
  }

  /// Run the fixed-step systems of the updating scenes, bottom-up.
  pub fn run_fixed_systems(&mut self, world: &mut World) {
    if self.scenes.is_empty() {
      return;
    }
    let from = self.update_from();
    for slot in &mut self.scenes[from..] {
      slot.schedules.run_fixed(world);
    }
  }

  /// Run the per-frame systems of the updating scenes, bottom-up.
  pub fn run_update_systems(&mut self, world: &mut World) {
    if self.scenes.is_empty() {
      return;
    }
    let from = self.update_from();
    for slot in &mut self.scenes[from..] {
      slot.schedules.run_update(world);
    }
  }

  /// Check if the scene stack is empty.
  pub fn is_empty(&self) -> bool {
    self.scenes.is_empty()
  }

  /// Get the number of scenes on the stack.
  pub fn len(&self) -> usize {
    self.scenes.len()
  }

  /// The top scene, if any.
  pub fn top(&self) -> Option<&dyn Scene> {
    self.scenes.last().map(|slot| slot.scene.as_ref())
  }

  /// The top scene's identity, unique to each scene entered (a scene of the
  /// same type entered again has a new one).
  pub(crate) fn top_scope(&self) -> Option<u64> {
    self.scenes.last().map(|slot| slot.scope_id)
  }

  /// The top scene, if any.
  pub fn top_mut(&mut self) -> Option<&mut dyn Scene> {
    self.scenes.last_mut().map(|slot| slot.scene.as_mut())
  }

  /// The stack for logs, bottom to top, the active scene starred:
  /// `"[MenuScene] -> [GameplayScene] -> [PauseScene*]"`.
  pub fn debug_stack(&self) -> String {
    if self.scenes.is_empty() {
      return String::from("(empty)");
    }
    let top = self.scenes.len() - 1;
    self
      .scenes
      .iter()
      .enumerate()
      .map(|(i, slot)| {
        if i == top {
          format!("[{}*]", slot.scene.name())
        } else {
          format!("[{}]", slot.scene.name())
        }
      })
      .collect::<Vec<_>>()
      .join(" -> ")
  }

  /// The names of all scenes in the stack (bottom to top).
  pub fn scene_names(&self) -> Vec<&str> {
    self.scenes.iter().map(|slot| slot.scene.name()).collect()
  }

  /// Whether physics should be paused: the top scene asks for it, or it sits
  /// over other scenes without updating them.
  pub fn is_physics_paused(&self) -> bool {
    let Some(top) = self.scenes.last() else {
      return false;
    };
    top.scene.is_physics_paused() || (self.scenes.len() > 1 && !top.scene.updates_below())
  }

  /// Write `is_physics_paused` into the `PhysicsPaused` resource.
  pub fn sync_physics_paused(&self, world: &mut World) {
    if let Some(mut paused) = world.get_resource_mut::<PhysicsPaused>() {
      paused.0 = self.is_physics_paused();
    }
  }
}

impl Default for SceneManager {
  fn default() -> Self {
    Self::new()
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::console::CommandResult;
  use crate::core::ecs::{Allow, Component, Disabled, ResMut, Resource, Transform, World};
  use crate::core::messages::{register_message, update_messages};
  use crate::core::schedule::Phase;
  use std::sync::{Arc, Mutex};

  #[derive(Resource, Default)]
  struct Log(Vec<&'static str>);

  #[derive(Component)]
  struct Thing;

  #[derive(crate::core::ecs::Message, Clone)]
  struct Ping;

  type Shared = Arc<Mutex<Vec<&'static str>>>;

  struct Rec {
    log: Shared,
    name: &'static str,
    action: Option<fn() -> SceneAction>,
    updates_below: bool,
  }

  impl Scene for Rec {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
      self.log.lock().unwrap().push("enter");
      let name = self.name;
      ctx
        .add_systems(Phase::FixedUpdate, move |mut log: ResMut<Log>| {
          log.0.push(name)
        })
        .unwrap();
      ctx.world.spawn((Transform::IDENTITY, Thing));
    }
    fn on_exit(&mut self, _ctx: &mut SceneContext) {
      self.log.lock().unwrap().push("exit");
    }
    fn on_pause(&mut self, _ctx: &mut SceneContext) {
      self.log.lock().unwrap().push("pause");
    }
    fn on_resume(&mut self, _ctx: &mut SceneContext, _r: Option<Box<dyn std::any::Any>>) {
      self.log.lock().unwrap().push("resume");
    }
    fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
      self.log.lock().unwrap().push("update");
      self
        .action
        .take()
        .map(|f| f())
        .unwrap_or(SceneAction::Continue)
    }
    fn updates_below(&self) -> bool {
      self.updates_below
    }
  }

  fn world() -> (World, Audio) {
    let mut world = World::new();
    crate::core::scene::init_world_resources_for_tests(&mut world);
    world.init_resource::<Log>();
    register_message::<Ping>(&mut world);
    (world, Audio::new(None))
  }

  fn rec(log: &Shared, name: &'static str) -> Box<Rec> {
    Box::new(Rec {
      log: log.clone(),
      name,
      action: None,
      updates_below: false,
    })
  }

  struct Listener(Arc<Mutex<usize>>);

  impl Scene for Listener {
    fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
      *self.0.lock().unwrap() += ctx.messages::<Ping>().len();
      SceneAction::Continue
    }
  }

  #[test]
  fn a_pushed_scene_reads_only_messages_written_after_it_entered() {
    let log: Shared = Arc::default();
    let (mut world, mut audio) = world();
    let mut m = SceneManager::with_scene(rec(&log, "a"));
    m.enter_initial(&mut world, &mut audio);
    world
      .resource_mut::<crate::core::ecs::Messages<Ping>>()
      .write(Ping);
    let heard = Arc::new(Mutex::new(0));
    m.handle_action(
      SceneAction::Push(Box::new(Listener(heard.clone()))),
      &mut world,
      &mut audio,
    );
    m.update(&mut world, &mut audio);
    assert_eq!(*heard.lock().unwrap(), 0, "written before it entered");
    world
      .resource_mut::<crate::core::ecs::Messages<Ping>>()
      .write(Ping);
    m.update(&mut world, &mut audio);
    assert_eq!(*heard.lock().unwrap(), 1);
  }

  struct DroppedQuitter;

  impl Scene for DroppedQuitter {
    fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
      SceneAction::Quit
    }
  }

  #[test]
  fn a_lower_scene_s_dropped_action_is_warned_about_once() {
    let log: Shared = Arc::default();
    let (mut world, mut audio) = world();
    let mut m = SceneManager::with_scene(Box::new(DroppedQuitter));
    m.enter_initial(&mut world, &mut audio);
    let mut overlay = rec(&log, "overlay");
    overlay.updates_below = true;
    m.push(overlay, &mut world, &mut audio);
    for _ in 0..3 {
      let action = m.update(&mut world, &mut audio);
      assert!(
        matches!(action, SceneAction::Continue),
        "only the top's action applies"
      );
    }
    let warnings = crate::core::dev_log::get_entries()
      .iter()
      .filter(|entry| entry.message.contains("DroppedQuitter"))
      .count();
    assert_eq!(warnings, 1);
  }

  struct UiQuitter;

  impl Scene for UiQuitter {
    fn ui(&mut self, _ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
      SceneAction::Quit
    }
  }

  struct Glass;

  impl Scene for Glass {
    fn is_transparent(&self) -> bool {
      true
    }
  }

  #[test]
  fn a_lower_scene_s_dropped_ui_action_is_warned_about_once() {
    let (mut world, mut audio) = world();
    let mut m = SceneManager::with_scene(Box::new(UiQuitter));
    m.enter_initial(&mut world, &mut audio);
    m.push(Box::new(Glass), &mut world, &mut audio);
    let (mut commands, mut text) = (
      Default::default(),
      crate::core::ui::test_util::text_engine(),
    );
    let mut ui = crate::core::ui::test_util::context(&mut commands, &mut text, (16.0, 16.0));
    for _ in 0..3 {
      let action = m.ui_pass(&mut world, &mut audio, &mut ui);
      assert!(matches!(action, SceneAction::Continue));
    }
    let warnings = crate::core::dev_log::get_entries()
      .iter()
      .filter(|entry| entry.message.contains("UiQuitter"))
      .count();
    assert_eq!(warnings, 1);
  }

  struct OpensDropdown;

  impl Scene for OpensDropdown {
    fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
      let mut state = crate::core::ui::DropdownState::new();
      state.open();
      ui.dropdown(
        &mut state,
        crate::core::ui::Rect::new(0.0, 0.0, 80.0, 20.0),
        &["one", "two"],
      );
      SceneAction::Continue
    }
  }

  #[test]
  fn a_scene_s_open_dropdown_is_drawn_without_finish_dropdowns() {
    let (mut world, mut audio) = world();
    let mut m = SceneManager::with_scene(Box::new(OpensDropdown));
    m.enter_initial(&mut world, &mut audio);
    let (mut commands, mut text) = (
      crate::core::ui::immediate::UiCommands::default(),
      crate::core::ui::test_util::text_engine(),
    );
    let mut ui = crate::core::ui::test_util::context(&mut commands, &mut text, (160.0, 120.0));
    m.ui_pass(&mut world, &mut audio, &mut ui);
    drop(ui);
    assert!(
      commands.iter().any(|c| c.draw_layer() == 1),
      "popup commands are present after ui_pass"
    );
  }

  #[test]
  fn push_pop_lifecycle_and_scene_systems() {
    let log: Shared = Arc::default();
    let (mut world, mut audio) = world();
    let mut m = SceneManager::with_scene(rec(&log, "a"));
    m.enter_initial(&mut world, &mut audio);
    m.run_fixed_systems(&mut world);
    assert_eq!(world.resource::<Log>().0, ["a"]);

    let action = m.update(&mut world, &mut audio);
    assert!(matches!(action, SceneAction::Continue));
    assert!(m.handle_action(SceneAction::Push(rec(&log, "b")), &mut world, &mut audio));
    m.run_fixed_systems(&mut world);
    assert_eq!(
      world.resource::<Log>().0,
      ["a", "b"],
      "only the top scene's systems run"
    );

    assert!(m.handle_action(SceneAction::Pop, &mut world, &mut audio));
    m.run_fixed_systems(&mut world);
    assert_eq!(
      world.resource::<Log>().0,
      ["a", "b", "a"],
      "b's systems went with it"
    );
    assert_eq!(
      *log.lock().unwrap(),
      ["enter", "update", "pause", "enter", "exit", "resume"]
    );
  }

  #[test]
  fn updates_below_runs_lower_scenes_too() {
    let log: Shared = Arc::default();
    let (mut world, mut audio) = world();
    let mut m = SceneManager::with_scene(rec(&log, "a"));
    m.enter_initial(&mut world, &mut audio);
    let mut top = rec(&log, "b");
    top.updates_below = true;
    m.handle_action(SceneAction::Push(top), &mut world, &mut audio);
    m.run_fixed_systems(&mut world);
    assert_eq!(world.resource::<Log>().0, ["a", "b"]);
    assert!(
      !m.is_physics_paused(),
      "a top scene that updates below does not pause physics"
    );
  }

  #[test]
  fn switch_cleans_the_world_and_messages_but_not_resources() {
    let log: Shared = Arc::default();
    let (mut world, mut audio) = world();
    let mut m = SceneManager::with_scene(rec(&log, "a"));
    m.enter_initial(&mut world, &mut audio);
    world.spawn((Transform::IDENTITY, Thing, Disabled));
    world
      .resource_mut::<crate::core::ecs::Messages<Ping>>()
      .write(Ping);
    // Resources are entities too, so count the game's own
    assert_eq!(
      world
        .query_filtered::<&Thing, Allow<Disabled>>()
        .iter(&world)
        .count(),
      2
    );
    m.handle_action(SceneAction::Switch(rec(&log, "b")), &mut world, &mut audio);
    assert_eq!(
      world
        .query_filtered::<&Thing, Allow<Disabled>>()
        .iter(&world)
        .count(),
      1,
      "only b's entity remains"
    );
    update_messages(&mut world);
    let mut cursor = world
      .resource::<crate::core::ecs::Messages<Ping>>()
      .get_cursor();
    assert_eq!(
      cursor
        .read(world.resource::<crate::core::ecs::Messages<Ping>>())
        .count(),
      0
    );
    assert!(
      world.get_resource::<Log>().is_some(),
      "resources survive cleanup"
    );
  }

  #[test]
  fn stacked_opaque_scene_pauses_physics() {
    let log: Shared = Arc::default();
    let (mut world, mut audio) = world();
    let mut m = SceneManager::with_scene(rec(&log, "a"));
    m.enter_initial(&mut world, &mut audio);
    m.handle_action(
      SceneAction::Push(rec(&log, "pause-menu")),
      &mut world,
      &mut audio,
    );
    assert!(m.is_physics_paused());
  }

  struct Commander;

  impl Scene for Commander {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
      let mut console = ctx.console();
      console.declare("game:jump", "Jump", None::<String>);
      assert!(console.register("game:jump", |_, _| CommandResult::Ok));
    }
  }

  #[test]
  fn a_scene_s_console_handlers_leave_with_it() {
    let log: Shared = Arc::default();
    let (mut world, mut audio) = world();
    let mut m = SceneManager::with_scene(rec(&log, "a"));
    m.enter_initial(&mut world, &mut audio);
    let active = |world: &World| {
      world
        .resource::<CommandRegistry>()
        .get_command_info("game:jump")
        .is_some_and(|info| info.is_active)
    };
    m.handle_action(
      SceneAction::Push(Box::new(Commander)),
      &mut world,
      &mut audio,
    );
    assert!(active(&world));
    m.handle_action(SceneAction::Pop, &mut world, &mut audio);
    assert!(!active(&world), "the handler went with the scene");
  }

  #[test]
  fn a_fade_switches_at_its_midpoint_and_scenes_wait_for_it() {
    let log: Shared = Arc::default();
    let (mut world, mut audio) = world();
    let mut m = SceneManager::with_scene(rec(&log, "a"));
    m.enter_initial(&mut world, &mut audio);
    let fade = Transition::fade(Duration::from_millis(50));
    assert!(m.handle_action(
      SceneAction::SwitchWith(rec(&log, "b"), fade),
      &mut world,
      &mut audio
    ));
    assert_eq!(m.transition_overlay_color(), Some([0, 0, 0, 255]));
    let mut steps = 0;
    while m.is_transitioning() {
      // The engine advances transitions once a frame on real time
      m.update_transition(0.02, &mut world, &mut audio);
      assert!(matches!(
        m.update(&mut world, &mut audio),
        SceneAction::Continue
      ));
      steps += 1;
      assert!(steps < 20, "the fade ends");
    }
    assert_eq!(
      *log.lock().unwrap(),
      ["enter", "exit", "enter", "update"],
      "a's update never ran under the fade; b's ran once it ended"
    );
    m.run_fixed_systems(&mut world);
    assert_eq!(world.resource::<Log>().0, ["b"]);
  }

  #[test]
  fn scoped_context_puts_resources_back() {
    let (mut world, mut audio) = world();
    let mut slot = SceneSlot::new(Box::new(Rec {
      log: Arc::default(),
      name: "x",
      action: None,
      updates_below: false,
    }));
    SceneContext::scoped(&mut world, &mut audio, &mut slot, |ctx| {
      ctx.camera().set_zoom(2.0);
      ctx.time.game.set_speed(0.5);
      assert!(
        ctx
          .world
          .get_resource::<crate::core::time::Time>()
          .is_none()
      );
    });
    let zoom = crate::core::graphics::CameraMut::main(&mut world).map(|camera| camera.zoom());
    assert_eq!(zoom, Some(2.0));
    assert_eq!(
      world.resource::<crate::core::time::Time>().game.speed(),
      0.5
    );
  }
}
