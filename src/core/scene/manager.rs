//! The scene stack.

use std::any::Any;
use std::time::Duration;

use crate::core::audio::{hold_scenes, in_scene, release_scene};
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
  /// A slot for `scene` with the scope id `scope_id`
  pub(crate) fn new(scene: Box<dyn Scene>, scope_id: u64) -> Self {
    Self {
      scene,
      schedules: SceneSchedules::default(),
      messages: SceneMessages::default(),
      scope_id,
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

  /// Run `f` with this slot's scene and a context built for it. The voices
  /// it plays are the scene's.
  fn call<R>(
    &mut self,
    world: &mut World,
    f: impl FnOnce(&mut dyn Scene, &mut SceneContext<'_>) -> R,
  ) -> R {
    let SceneSlot {
      scene,
      schedules,
      messages,
      scope_id,
      ..
    } = self;
    let scope_id = *scope_id;
    in_scene(world, scope_id, |world| {
      SceneContext::scoped_parts(world, schedules, messages, scope_id, |ctx| {
        f(scene.as_mut(), ctx)
      })
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
  /// Scope ids of the scenes that exited since `take_exited`
  exited: Vec<u64>,
  /// Scenes entered so far (each `on_enter`)
  entries: u64,
  /// The scope id the next scene gets. Each scene instance has its own,
  /// counted per manager (so per engine), never 0 (the engine's overlays):
  /// two engines run the same way give their scenes the same ids, and so
  /// their UI the same ids
  next_scope: u64,
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

/// Drop what a scene that exited owned in the world: the console handlers
/// it registered, and the voices it played: its loops, its paused or held
/// one-shots and its one-shots with more than 2 s left (but on `ui`) stop
/// with their fades, and the rest of its playing one-shots play to their
/// end.
fn release_scope(world: &mut World, scope_id: u64) {
  if let Some(mut registry) = world.get_resource_mut::<CommandRegistry>() {
    registry.clear_scope(scope_id);
  }
  release_scene(world, scope_id);
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
      exited: Vec::new(),
      entries: 0,
      next_scope: 1,
    }
  }

  /// A slot for `scene`, with the next scope id
  fn slot(&mut self, scene: Box<dyn Scene>) -> SceneSlot {
    let scope_id = self.next_scope;
    self.next_scope += 1;
    SceneSlot::new(scene, scope_id)
  }

  /// How many scenes have been entered so far: a frame that raises it
  /// loaded a scene
  pub(crate) fn entries(&self) -> u64 {
    self.entries
  }

  /// The scope ids of the scenes that exited (popped, switched away, cleared
  /// or reset) since the last call; the engine forgets their UI state
  pub(crate) fn take_exited(&mut self) -> Vec<u64> {
    std::mem::take(&mut self.exited)
  }

  /// Create a scene manager with an initial scene, entered by `enter_initial`.
  pub fn with_scene(scene: Box<dyn Scene>) -> Self {
    let mut manager = Self::new();
    let slot = manager.slot(scene);
    manager.scenes.push(slot);
    manager
  }

  /// Call `on_enter` for the initial scene. The engine calls this once the
  /// GPU and every resource exist.
  pub fn enter_initial(&mut self, world: &mut World) {
    self.enter_top(world);
  }

  fn enter_top(&mut self, world: &mut World) {
    self.sync_audio_holds(world);
    let Some(top) = self.scenes.last_mut() else {
      return;
    };
    top.messages = SceneMessages::starting_now(world);
    self.entries += 1;
    #[cfg(debug_assertions)]
    let start = web_time::Instant::now();
    top.call(world, |scene, ctx| scene.on_enter(ctx));
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
  pub fn push(&mut self, scene: Box<dyn Scene>, world: &mut World) {
    profile_scope!("cpu:scene_push");
    if let Some(current) = self.scenes.last_mut() {
      current.call(world, |scene, ctx| scene.on_pause(ctx));
    }
    let slot = self.slot(scene);
    self.scenes.push(slot);
    self.enter_top(world);
  }

  /// Pop the top scene: it exits, the new top resumes with `returned`.
  ///
  /// Returns the popped scene, or `None` if the stack was empty.
  pub fn pop(
    &mut self,
    world: &mut World,
    returned: Option<Box<dyn Any>>,
  ) -> Option<Box<dyn Scene>> {
    let mut slot = self.scenes.pop()?;
    slot.call(world, |scene, ctx| scene.on_exit(ctx));
    release_scope(world, slot.scope_id);
    self.exited.push(slot.scope_id);
    // The scenes below update again, and are heard as the new top resumes
    self.sync_audio_holds(world);
    if let Some(top) = self.scenes.last_mut() {
      top.call(world, |scene, ctx| scene.on_resume(ctx, returned));
    }
    Some(slot.scene)
  }

  /// Replace the top scene: it exits, its cleanup runs, the new one enters.
  pub fn switch(&mut self, scene: Box<dyn Scene>, world: &mut World) {
    profile_scope!("cpu:scene_switch");
    if let Some(mut current) = self.scenes.pop() {
      let cleanup = current.scene.cleanup_on_exit();
      current.call(world, |scene, ctx| scene.on_exit(ctx));
      release_scope(world, current.scope_id);
      self.exited.push(current.scope_id);
      drop(current);
      execute_cleanup(cleanup, world);
    }
    let slot = self.slot(scene);
    self.scenes.push(slot);
    self.enter_top(world);
  }

  /// Exit every scene, top to bottom.
  pub fn clear(&mut self, world: &mut World) {
    while let Some(mut slot) = self.scenes.pop() {
      slot.call(world, |scene, ctx| scene.on_exit(ctx));
      release_scope(world, slot.scope_id);
      self.exited.push(slot.scope_id);
    }
    self.sync_audio_holds(world);
  }

  /// Exit every scene, run the top scene's cleanup, then enter `scene`.
  pub fn reset(&mut self, scene: Box<dyn Scene>, world: &mut World) {
    let cleanup = self
      .scenes
      .last()
      .map_or(SceneCleanup::NONE, |slot| slot.scene.cleanup_on_exit());
    self.clear(world);
    execute_cleanup(cleanup, world);
    let slot = self.slot(scene);
    self.scenes.push(slot);
    self.enter_top(world);
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
  pub fn update(&mut self, world: &mut World) -> SceneAction {
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
      let result = slot.call(world, |scene, ctx| scene.update(ctx, dt));
      if i == top {
        action = result;
      } else {
        slot.warn_if_dropped(&result, "update");
      }
    }
    action
  }

  /// Apply a scene action. Returns `false` when the game should quit.
  pub fn handle_action(&mut self, action: SceneAction, world: &mut World) -> bool {
    match action {
      SceneAction::Continue => true,
      SceneAction::Push(scene) => {
        self.push(scene, world);
        true
      }
      SceneAction::Pop => {
        self.pop(world, None);
        !self.scenes.is_empty()
      }
      SceneAction::PopReturn(data) => {
        self.pop(world, Some(data));
        !self.scenes.is_empty()
      }
      SceneAction::Switch(scene) => {
        self.switch(scene, world);
        true
      }
      SceneAction::Reset(scene) => {
        self.reset(scene, world);
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
  pub fn update_transition(&mut self, dt: f32, world: &mut World) -> bool {
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
      self.execute_pending_action(action, world);
    }
    !self.scenes.is_empty()
  }

  fn execute_pending_action(&mut self, action: PendingAction, world: &mut World) {
    match action {
      PendingAction::Push(scene) => self.push(scene, world),
      PendingAction::Pop => {
        self.pop(world, None);
      }
      PendingAction::PopReturn(data) => {
        self.pop(world, Some(data));
      }
      PendingAction::Switch(scene) => self.switch(scene, world),
      PendingAction::Reset(scene) => self.reset(scene, world),
    }
  }

  /// Run `Scene::ui` for the visible scenes, bottom-up. Returns the top
  /// scene's action, except a scene change while a transition runs: the
  /// UI still draws during a fade, and a second click there would restart
  /// the transition with a new change, so only `Quit` gets through.
  pub fn ui_pass(&mut self, world: &mut World, ui: &mut UiContext) -> SceneAction {
    if self.scenes.is_empty() {
      return SceneAction::Continue;
    }
    crate::core::ui::nodes::begin_pass(world);
    let top = self.scenes.len() - 1;
    let mut action = SceneAction::Continue;
    // Scenes under the top one draw first; their widgets answer nothing, as
    // only the top scene's are hit
    for i in self.render_from()..=top {
      let slot = &mut self.scenes[i];
      // Each scene's ids are made under its own root scope. Ending its UI
      // draws its popups above everything it drew: the top scene's on the
      // popup layer, a lower scene's under the scenes above it.
      ui.begin_scene(slot.scope_id, i == top);
      // The scene's node trees draw first, so its `ui` draws above them and
      // reads their events this frame
      let events = crate::core::ui::nodes::draw_nodes(world, ui, slot.scope_id);
      // The observers of its nodes' events are its code: what they play,
      // the commands they queue included, is the scene's
      in_scene(world, slot.scope_id, |world| {
        crate::core::ui::nodes::deliver(world, events);
        world.flush();
      });
      let result = slot.call(world, |scene, ctx| scene.ui(ui, ctx));
      in_scene(world, slot.scope_id, |world| {
        crate::core::ui::nodes::deliver_pending(world);
        world.flush();
      });
      ui.end_scene();
      if i == top {
        action = result;
      } else {
        slot.warn_if_dropped(&result, "ui");
      }
    }
    if self.is_transitioning() && !matches!(action, SceneAction::Continue | SceneAction::Quit) {
      return SceneAction::Continue;
    }
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
      in_scene(world, slot.scope_id, |world| {
        slot.schedules.run_fixed(world)
      });
    }
  }

  /// Run the per-frame systems of the updating scenes, bottom-up.
  pub fn run_update_systems(&mut self, world: &mut World) {
    if self.scenes.is_empty() {
      return;
    }
    let from = self.update_from();
    for slot in &mut self.scenes[from..] {
      in_scene(world, slot.scope_id, |world| {
        slot.schedules.run_update(world)
      });
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

  /// Write the stack's pauses into the world: `is_physics_paused` into the
  /// `PhysicsPaused` resource, and the scenes whose voices the audio engine
  /// holds (see [`Scene::holds_audio_below`]). The stack's changes do the
  /// second as they happen; this follows a scene whose answers changed.
  pub fn sync_pauses(&self, world: &mut World) {
    if let Some(mut paused) = world.get_resource_mut::<PhysicsPaused>() {
      paused.0 = self.is_physics_paused();
    }
    self.sync_audio_holds(world);
  }

  /// Tell the audio engine which scenes are not heard: those not updated
  /// (below `update_from`), when the top scene `holds_audio_below`.
  fn sync_audio_holds(&self, world: &mut World) {
    let held = match self.scenes.last() {
      Some(top) if top.scene.holds_audio_below() => &self.scenes[..self.update_from()],
      _ => &[],
    };
    hold_scenes(world, held.iter().map(|slot| slot.scope_id));
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

  fn world() -> World {
    let mut world = World::new();
    crate::core::scene::init_world_resources_for_tests(&mut world);
    world.init_resource::<Log>();
    register_message::<Ping>(&mut world);
    world
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
  fn each_manager_counts_its_own_scope_ids() {
    let log: Shared = Arc::default();
    let mut world = world();
    let mut a = SceneManager::with_scene(rec(&log, "a"));
    let b = SceneManager::with_scene(rec(&log, "b"));
    // So two engines run the same way give their UI the same ids
    assert_eq!(a.top_scope(), Some(1));
    assert_eq!(b.top_scope(), Some(1));
    a.push(rec(&log, "c"), &mut world);
    a.switch(rec(&log, "d"), &mut world);
    assert_eq!(a.top_scope(), Some(3), "a new id for each scene entered");
    a.reset(rec(&log, "e"), &mut world);
    assert_eq!(a.top_scope(), Some(4));
  }

  #[test]
  fn a_pushed_scene_reads_only_messages_written_after_it_entered() {
    let log: Shared = Arc::default();
    let mut world = world();
    let mut m = SceneManager::with_scene(rec(&log, "a"));
    m.enter_initial(&mut world);
    world
      .resource_mut::<crate::core::ecs::Messages<Ping>>()
      .write(Ping);
    let heard = Arc::new(Mutex::new(0));
    m.handle_action(
      SceneAction::Push(Box::new(Listener(heard.clone()))),
      &mut world,
    );
    m.update(&mut world);
    assert_eq!(*heard.lock().unwrap(), 0, "written before it entered");
    world
      .resource_mut::<crate::core::ecs::Messages<Ping>>()
      .write(Ping);
    m.update(&mut world);
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
    let mut world = world();
    let mut m = SceneManager::with_scene(Box::new(DroppedQuitter));
    m.enter_initial(&mut world);
    let mut overlay = rec(&log, "overlay");
    overlay.updates_below = true;
    m.push(overlay, &mut world);
    for _ in 0..3 {
      let action = m.update(&mut world);
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
    let mut world = world();
    let mut m = SceneManager::with_scene(Box::new(UiQuitter));
    m.enter_initial(&mut world);
    m.push(Box::new(Glass), &mut world);
    let (mut commands, mut text) = (
      Default::default(),
      crate::core::ui::test_util::text_engine(),
    );
    let mut ui = crate::core::ui::test_util::context(&mut commands, &mut text, (16.0, 16.0));
    for _ in 0..3 {
      let action = m.ui_pass(&mut world, &mut ui);
      assert!(matches!(action, SceneAction::Continue));
    }
    let warnings = crate::core::dev_log::get_entries()
      .iter()
      .filter(|entry| entry.message.contains("UiQuitter"))
      .count();
    assert_eq!(warnings, 1);
  }

  /// Draws a closed dropdown, keeping its trigger's id; a test opens it
  /// with Confirm (`ui_frame`'s `activate`)
  struct OpensDropdown {
    selected: usize,
    trigger: Arc<Mutex<crate::core::ui::UiId>>,
  }

  impl OpensDropdown {
    fn new(trigger: &Arc<Mutex<crate::core::ui::UiId>>) -> Box<OpensDropdown> {
      Box::new(OpensDropdown {
        selected: 0,
        trigger: trigger.clone(),
      })
    }
  }

  impl Scene for OpensDropdown {
    fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
      let id = ui
        .dropdown(&mut self.selected, &["one", "two"])
        .at(crate::core::ui::Rect::new(0.0, 0.0, 80.0, 20.0))
        .show()
        .id;
      if let Ok(mut trigger) = self.trigger.lock() {
        *trigger = id;
      }
      SceneAction::Continue
    }
  }

  /// One frame of `m`'s UI over `memory`, as the top scene's Confirm on
  /// `activate` would see it; its commands
  fn ui_frame(
    m: &mut SceneManager,
    world: &mut World,
    memory: &mut crate::core::ui::memory::UiMemory,
    activate: crate::core::ui::UiId,
  ) -> crate::core::ui::immediate::UiCommands {
    let mut commands = crate::core::ui::immediate::UiCommands::default();
    let mut text = crate::core::ui::test_util::text_engine();
    let mut layout = crate::core::ui::layout::engine::LayoutEngine::new();
    let space = crate::core::render::UiSpace {
      size: glam::Vec2::new(320.0, 320.0),
      px_scale: 1.0,
      image: (320, 320),
      mouse_scale: 1.0,
    };
    memory.begin_frame();
    memory.interact.activated = activate;
    {
      let mut ui = UiContext::new(
        &mut commands,
        &mut text,
        memory,
        &mut layout,
        space,
        crate::core::ui::UiInput::default(),
        Arc::new(crate::core::ui::Theme::dark()),
        0,
        crate::core::ui::image_sizes::ImageSizes::none(),
      );
      m.ui_pass(world, &mut ui);
    }
    memory.interact.activated = crate::core::ui::UiId::NONE;
    memory.end_frame();
    commands
  }

  /// The texts `commands` draw
  fn texts(commands: &crate::core::ui::immediate::UiCommands) -> Vec<String> {
    commands
      .commands()
      .filter_map(|c| match c {
        crate::core::ui::immediate::UiCommand::Text { layout, .. } => {
          Some(layout.text().to_string())
        }
        _ => None,
      })
      .collect()
  }

  #[test]
  fn a_top_scene_s_open_dropdown_list_draws_on_the_popup_layer() {
    let mut world = world();
    let trigger = Arc::default();
    let mut m = SceneManager::with_scene(OpensDropdown::new(&trigger));
    m.enter_initial(&mut world);
    let mut memory = crate::core::ui::memory::UiMemory::new();
    let none = crate::core::ui::UiId::NONE;
    ui_frame(&mut m, &mut world, &mut memory, none);
    let id = *trigger.lock().unwrap();
    let commands = ui_frame(&mut m, &mut world, &mut memory, id);
    assert!(
      texts(&commands).contains(&"two".to_string()),
      "the list opened"
    );
    assert!(
      commands.commands().any(|c| c.draw_layer() == 1),
      "the open list is on the popup layer after ui_pass"
    );
  }

  struct DrawsRect;

  impl Scene for DrawsRect {
    fn is_transparent(&self) -> bool {
      true
    }
    fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
      ui.draw_rect(
        crate::core::ui::Rect::new(0.0, 0.0, 300.0, 300.0),
        crate::core::ui::Color::RED,
      );
      SceneAction::Continue
    }
  }

  #[test]
  fn a_lower_scene_s_popups_stay_below_an_upper_scene() {
    let mut world = world();
    let trigger = Arc::default();
    let mut m = SceneManager::with_scene(OpensDropdown::new(&trigger));
    m.enter_initial(&mut world);
    let mut memory = crate::core::ui::memory::UiMemory::new();
    let none = crate::core::ui::UiId::NONE;
    // Opened while it is the top scene, then covered by another
    ui_frame(&mut m, &mut world, &mut memory, none);
    let id = *trigger.lock().unwrap();
    ui_frame(&mut m, &mut world, &mut memory, id);
    m.push(Box::new(DrawsRect), &mut world);
    let commands = ui_frame(&mut m, &mut world, &mut memory, none);
    assert!(
      texts(&commands).contains(&"two".to_string()),
      "the lower scene drew its open list"
    );
    let keys: Vec<(u8, u32)> = commands
      .iter()
      .map(|d| (d.command.draw_layer(), d.order))
      .collect();
    let (last, earlier) = keys.split_last().unwrap();
    assert!(earlier.len() > 1, "the lower scene drew its dropdown");
    assert!(
      earlier.iter().all(|key| key < last),
      "the upper scene's rect draws over the lower popups: {keys:?}"
    );
  }

  /// A see-through scene drawing one panel, keeping its id
  struct PanelScene(Arc<Mutex<Vec<crate::core::ui::UiId>>>);

  impl Scene for PanelScene {
    fn is_transparent(&self) -> bool {
      true
    }
    fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
      let id = ui.panel().width(8.0).height(8.0).show(|_| {}).id;
      self.0.lock().unwrap().push(id);
      SceneAction::Continue
    }
  }

  #[test]
  fn each_scene_draws_its_ui_under_its_own_scope() {
    let mut world = world();
    let ids = Arc::new(Mutex::new(Vec::new()));
    let mut m = SceneManager::with_scene(Box::new(PanelScene(ids.clone())));
    m.enter_initial(&mut world);
    let lower = m.top_scope().unwrap();
    m.push(Box::new(PanelScene(ids.clone())), &mut world);
    let upper = m.top_scope().unwrap();
    let (mut commands, mut text) = (
      crate::core::ui::immediate::UiCommands::default(),
      crate::core::ui::test_util::text_engine(),
    );
    let mut ui = crate::core::ui::test_util::context(&mut commands, &mut text, (64.0, 64.0));
    m.ui_pass(&mut world, &mut ui);
    let ids = ids.lock().unwrap().clone();
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1], "the same code in two scenes");
    let scene_of = |id| ui.memory_for_test().current(id).map(|r| r.scene);
    assert_eq!(
      (scene_of(ids[0]), scene_of(ids[1])),
      (Some(lower), Some(upper))
    );
  }

  /// A see-through scene drawing one button where the others do, keeping its
  /// id and whether it reported the pointer
  struct ButtonScene(Arc<Mutex<Vec<(crate::core::ui::UiId, bool)>>>);

  impl Scene for ButtonScene {
    fn is_transparent(&self) -> bool {
      true
    }
    fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
      let response = ui
        .button("Same")
        .at(crate::core::ui::Rect::new(0.0, 0.0, 40.0, 20.0))
        .show();
      let touched = response.hovered() || response.clicked();
      self.0.lock().unwrap().push((response.id, touched));
      SceneAction::Continue
    }
  }

  #[test]
  fn only_the_top_scenes_widgets_report_the_pointer() {
    let mut world = world();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut m = SceneManager::with_scene(Box::new(ButtonScene(seen.clone())));
    m.enter_initial(&mut world);
    m.push(Box::new(ButtonScene(seen.clone())), &mut world);
    let (mut commands, mut text) = (
      crate::core::ui::immediate::UiCommands::default(),
      crate::core::ui::test_util::text_engine(),
    );
    let mut ui = crate::core::ui::test_util::context(&mut commands, &mut text, (64.0, 64.0));
    m.ui_pass(&mut world, &mut ui);
    let ids: Vec<_> = seen.lock().unwrap().drain(..).map(|(id, _)| id).collect();
    assert_eq!(ids.len(), 2);
    // As if the pointer had clicked each scene's button in turn
    for (i, id) in ids.iter().enumerate() {
      ui.memory.begin_frame();
      ui.memory.interact.hot = *id;
      ui.memory.interact.hovered = *id;
      ui.memory.interact.clicked = *id;
      m.ui_pass(&mut world, &mut ui);
      let touched: Vec<bool> = seen.lock().unwrap().drain(..).map(|(_, t)| t).collect();
      let expected = if i == 0 {
        vec![false, false]
      } else {
        vec![false, true]
      };
      assert_eq!(
        touched, expected,
        "the lower scene's button reports nothing"
      );
    }
  }

  #[test]
  fn scenes_that_exit_are_reported_once() {
    let log: Shared = Arc::default();
    let mut world = world();
    let mut m = SceneManager::with_scene(rec(&log, "a"));
    m.enter_initial(&mut world);
    let a = m.top_scope().unwrap();
    m.push(rec(&log, "b"), &mut world);
    let b = m.top_scope().unwrap();
    assert!(m.take_exited().is_empty(), "a push exits nothing");
    m.pop(&mut world, None);
    assert_eq!(m.take_exited(), vec![b]);
    assert!(m.take_exited().is_empty(), "taken once");
    m.switch(rec(&log, "c"), &mut world);
    let c = m.top_scope().unwrap();
    m.push(rec(&log, "d"), &mut world);
    let d = m.top_scope().unwrap();
    m.reset(rec(&log, "e"), &mut world);
    let e = m.top_scope().unwrap();
    m.clear(&mut world);
    assert_eq!(m.take_exited(), vec![a, d, c, e]);
  }

  #[test]
  fn push_pop_lifecycle_and_scene_systems() {
    let log: Shared = Arc::default();
    let mut world = world();
    let mut m = SceneManager::with_scene(rec(&log, "a"));
    m.enter_initial(&mut world);
    m.run_fixed_systems(&mut world);
    assert_eq!(world.resource::<Log>().0, ["a"]);

    let action = m.update(&mut world);
    assert!(matches!(action, SceneAction::Continue));
    assert!(m.handle_action(SceneAction::Push(rec(&log, "b")), &mut world));
    m.run_fixed_systems(&mut world);
    assert_eq!(
      world.resource::<Log>().0,
      ["a", "b"],
      "only the top scene's systems run"
    );

    assert!(m.handle_action(SceneAction::Pop, &mut world));
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
    let mut world = world();
    let mut m = SceneManager::with_scene(rec(&log, "a"));
    m.enter_initial(&mut world);
    let mut top = rec(&log, "b");
    top.updates_below = true;
    m.handle_action(SceneAction::Push(top), &mut world);
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
    let mut world = world();
    let mut m = SceneManager::with_scene(rec(&log, "a"));
    m.enter_initial(&mut world);
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
    m.handle_action(SceneAction::Switch(rec(&log, "b")), &mut world);
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
    let mut world = world();
    let mut m = SceneManager::with_scene(rec(&log, "a"));
    m.enter_initial(&mut world);
    m.handle_action(SceneAction::Push(rec(&log, "pause-menu")), &mut world);
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
    let mut world = world();
    let mut m = SceneManager::with_scene(rec(&log, "a"));
    m.enter_initial(&mut world);
    let active = |world: &World| {
      world
        .resource::<CommandRegistry>()
        .get_command_info("game:jump")
        .is_some_and(|info| info.is_active)
    };
    m.handle_action(SceneAction::Push(Box::new(Commander)), &mut world);
    assert!(active(&world));
    m.handle_action(SceneAction::Pop, &mut world);
    assert!(!active(&world), "the handler went with the scene");
  }

  #[test]
  fn a_fade_switches_at_its_midpoint_and_scenes_wait_for_it() {
    let log: Shared = Arc::default();
    let mut world = world();
    let mut m = SceneManager::with_scene(rec(&log, "a"));
    m.enter_initial(&mut world);
    let fade = Transition::fade(Duration::from_millis(50));
    assert!(m.handle_action(SceneAction::SwitchWith(rec(&log, "b"), fade), &mut world));
    assert_eq!(m.transition_overlay_color(), Some([0, 0, 0, 255]));
    let mut steps = 0;
    while m.is_transitioning() {
      // The engine advances transitions once a frame on real time
      m.update_transition(0.02, &mut world);
      assert!(matches!(m.update(&mut world), SceneAction::Continue));
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
    let mut world = world();
    let mut slot = SceneSlot::new(
      Box::new(Rec {
        log: Arc::default(),
        name: "x",
        action: None,
        updates_below: false,
      }),
      1,
    );
    SceneContext::scoped(&mut world, &mut slot, |ctx| {
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

  /// What the audio tests share: a world with its audio log on, and a
  /// two-second loop that fades out over 250 ms when the engine stops it,
  /// every play of it in a frame starting
  fn audio_scene_world() -> (World, crate::core::asset::Handle<crate::core::audio::Sound>) {
    use crate::core::audio::{AudioClip, AudioEngine, Sound, SoundClip};
    let mut world = world();
    world.resource_mut::<AudioEngine>().set_logging(true);
    let clip = world
      .resource_mut::<crate::core::asset::Assets<AudioClip>>()
      .add(AudioClip::from_frames(48_000, &vec![[0.1, 0.1]; 96_000]).unwrap());
    let sound = world
      .resource_mut::<crate::core::asset::Assets<Sound>>()
      .add(Sound {
        clips: vec![SoundClip::new(clip)],
        looping: true,
        fade_out: Duration::from_millis(250),
        per_frame: u8::MAX,
        ..Default::default()
      });
    (world, sound)
  }

  /// The voices the audio log says were stopped, with their fades
  fn audio_stops(world: &World) -> Vec<(u32, Duration)> {
    use crate::core::audio::{AudioEngine, AudioLogKind};
    world
      .resource::<AudioEngine>()
      .log()
      .iter()
      .filter(|entry| entry.kind == AudioLogKind::Stop)
      .map(|entry| (entry.voice, entry.fade))
      .collect()
  }

  type Voices = Arc<Mutex<Vec<crate::core::audio::Voice>>>;

  /// The voices `played` holds, in the order they were played
  fn voices<const N: usize>(played: &Voices) -> [crate::core::audio::Voice; N] {
    match <[_; N]>::try_from(played.lock().unwrap().clone()) {
      Ok(voices) => voices,
      Err(voices) => panic!("{} voices played, not {N}", voices.len()),
    }
  }

  /// Plays as it enters: its loop, the loop again persistent, a short
  /// one-shot, and a one-shot it pauses (a clip starts once a frame: the
  /// two are clips of their own)
  struct Player {
    sound: crate::core::asset::Handle<crate::core::audio::Sound>,
    shot: crate::core::asset::Handle<crate::core::audio::AudioClip>,
    held: crate::core::asset::Handle<crate::core::audio::AudioClip>,
    played: Voices,
  }

  impl Scene for Player {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
      let persist = crate::core::audio::PlayOptions {
        persist: true,
        ..Default::default()
      };
      let own = ctx.audio().play(&self.sound);
      let kept = ctx.audio().play_with(&self.sound, persist);
      let shot = ctx.audio().play(&self.shot);
      let paused = ctx.audio().play(&self.held);
      paused.pause();
      self
        .played
        .lock()
        .unwrap()
        .extend([own, kept, shot, paused]);
    }
  }

  /// A scene's exit stops its loops with their fade and lets its one-shots
  /// finish (a confirm click just before a switch is heard out), but a
  /// one-shot it paused, which would never finish; a persistent voice and
  /// one played outside every scene play on
  #[test]
  fn a_scene_exit_stops_its_loops_with_their_fade_and_lets_its_one_shots_finish() {
    use crate::core::audio::{AudioClip, AudioView};
    use crate::core::testing::audio_step;
    let (mut world, sound) = audio_scene_world();
    let mut clips = world.resource_mut::<crate::core::asset::Assets<AudioClip>>();
    let shot = clips.add(AudioClip::from_frames(48_000, &vec![[0.1, 0.1]; 14_400]).unwrap());
    let held = clips.add(AudioClip::from_frames(48_000, &vec![[0.1, 0.1]; 14_400]).unwrap());
    let played: Voices = Arc::default();
    let mut m = SceneManager::with_scene(Box::new(Player {
      sound: sound.clone(),
      shot,
      held,
      played: played.clone(),
    }));
    m.enter_initial(&mut world);
    // A voice played outside every scene is the engine's
    let engine_owned = AudioView::new(&mut world).play(&sound);
    let [own, kept, shot, paused] = voices(&played);
    audio_step(&mut world, Duration::from_millis(20));
    m.switch(rec(&Arc::default(), "next"), &mut world);
    assert_eq!(
      audio_stops(&world),
      [
        (own.id(), Duration::from_millis(250)),
        (paused.id(), Duration::from_millis(10))
      ]
    );
    // kira fades the loop out over its fade
    audio_step(&mut world, Duration::from_millis(100));
    assert!(!own.is_stopped() && !own.is_playing(), "fading: {own:?}");
    assert!(shot.is_playing(), "the one-shot plays on: {shot:?}");
    audio_step(&mut world, Duration::from_millis(200));
    audio_step(&mut world, Duration::from_millis(20));
    assert!(own.is_stopped(), "{own:?}");
    assert!(shot.is_stopped(), "it played to its end: {shot:?}");
    assert!(
      audio_stops(&world)
        .iter()
        .all(|(voice, _)| *voice != shot.id()),
      "it was never stopped"
    );
    assert!(kept.is_playing(), "{kept:?}");
    assert!(engine_owned.is_playing(), "{engine_owned:?}");
  }

  /// Plays each of its sounds as it enters
  struct PlaysEach(
    Vec<crate::core::asset::Handle<crate::core::audio::Sound>>,
    Voices,
  );

  impl Scene for PlaysEach {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
      let voices: Vec<_> = self.0.iter().map(|sound| ctx.audio().play(sound)).collect();
      self.1.lock().unwrap().extend(voices);
    }
  }

  /// At a scene's exit a one-shot playing finishes only if it is nearly
  /// over (2 s or less left) or plays on `ui`: a long voice-over stops with
  /// its fade-out rather than running on into the next scene. A stream
  /// counts what it has left from its position the same way
  #[test]
  fn a_scene_exit_stops_its_long_one_shots_and_lets_its_short_ones_finish() {
    use crate::core::asset::Assets;
    use crate::core::audio::{AudioClip, ClipData, EncodedClip, Sound, SoundClip};
    use crate::core::testing::{audio_step, wav};
    let (mut world, _) = audio_scene_world();
    let rate = 8_000;
    let mut clips = world.resource_mut::<Assets<AudioClip>>();
    let long = clips.add(AudioClip::from_frames(rate, &vec![[0.1, 0.1]; 20 * 8_000]).unwrap());
    let short = clips.add(AudioClip::from_frames(rate, &vec![[0.1, 0.1]; 8_000]).unwrap());
    let streamed = clips.add(AudioClip::new(
      ClipData::Encoded(EncodedClip {
        bytes: wav(rate, 20 * 8_000, 3_000).into(),
        frames: 20 * 8_000,
      }),
      Duration::from_secs(20),
      rate,
    ));
    let fade = Duration::from_millis(300);
    let mut sounds = world.resource_mut::<Assets<Sound>>();
    let mut sound = |clip: &crate::core::asset::Handle<AudioClip>, bus: &str| {
      sounds.add(Sound {
        clips: vec![SoundClip::new(clip.clone())],
        bus: bus.to_string(),
        fade_out: fade,
        ..Default::default()
      })
    };
    let each = vec![
      sound(&long, "voice"),
      sound(&short, "sfx"),
      sound(&long, "ui"),
      sound(&streamed, "voice"),
    ];
    let played: Voices = Arc::default();
    let mut m = SceneManager::with_scene(Box::new(PlaysEach(each, played.clone())));
    m.enter_initial(&mut world);
    let [narration, click, menu_music, streamed] = voices(&played);
    audio_step(&mut world, Duration::from_millis(50));
    m.switch(rec(&Arc::default(), "next"), &mut world);
    assert_eq!(
      audio_stops(&world),
      [(narration.id(), fade), (streamed.id(), fade)],
      "the long ones stop with their fade-out"
    );
    for _ in 0..70 {
      audio_step(&mut world, Duration::from_nanos(16_666_667));
    }
    assert!(narration.is_stopped(), "{narration:?}");
    assert!(streamed.is_stopped(), "{streamed:?}");
    assert!(
      click.is_stopped(),
      "the short one played to its end: {click:?}"
    );
    assert!(
      menu_music.is_playing(),
      "the ui one plays on: {menu_music:?}"
    );
    assert!(
      audio_stops(&world)
        .iter()
        .all(|(voice, _)| *voice != click.id() && *voice != menu_music.id()),
      "{:?}",
      audio_stops(&world)
    );
  }

  /// Plays its loop as it enters
  struct PlaysLoop(
    crate::core::asset::Handle<crate::core::audio::Sound>,
    Voices,
  );

  impl Scene for PlaysLoop {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
      let voice = ctx.audio().play(&self.0);
      self.1.lock().unwrap().push(voice);
    }
  }

  /// Plays a one-shot as it enters
  struct PlaysShot(
    crate::core::asset::Handle<crate::core::audio::AudioClip>,
    Voices,
  );

  impl Scene for PlaysShot {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
      let voice = ctx.audio().play(&self.0);
      self.1.lock().unwrap().push(voice);
    }
  }

  /// "Quit to menu" from a pause: the switch's cleanup resumes the clock,
  /// but the old scene's one-shot, held mid-play, is stopped as it exits,
  /// never resumed over the menu
  #[test]
  fn a_scene_exit_stops_its_one_shots_the_game_pause_holds() {
    use crate::core::audio::{AudioClip, AudioEngine, AudioLogKind};
    use crate::core::testing::audio_step;
    let (mut world, _) = audio_scene_world();
    let clip = world
      .resource_mut::<crate::core::asset::Assets<AudioClip>>()
      .add(AudioClip::from_frames(48_000, &vec![[0.1, 0.1]; 48_000]).unwrap());
    let played: Voices = Arc::default();
    let mut m = SceneManager::with_scene(Box::new(PlaysShot(clip, played.clone())));
    m.enter_initial(&mut world);
    let [shot] = voices(&played);
    audio_step(&mut world, Duration::from_millis(50));
    world.resource_mut::<Time>().game.pause();
    audio_step(&mut world, Duration::from_nanos(16_666_667));
    m.switch(rec(&Arc::default(), "menu"), &mut world);
    assert!(!world.resource::<Time>().game.is_paused());
    assert_eq!(
      audio_stops(&world),
      [(shot.id(), Duration::from_millis(10))]
    );
    for _ in 0..3 {
      audio_step(&mut world, Duration::from_nanos(16_666_667));
    }
    assert!(shot.is_stopped(), "{shot:?}");
    let resumed = world
      .resource::<AudioEngine>()
      .log()
      .iter()
      .any(|entry| entry.voice == shot.id() && entry.kind == AudioLogKind::Resume);
    assert!(!resumed, "it was never heard again");
  }

  /// A reset and a clear exit every scene: each one's loops stop
  #[test]
  fn a_reset_or_a_clear_stops_the_loops_of_every_scene_it_exits() {
    let (mut world, sound) = audio_scene_world();
    let played: Voices = Arc::default();
    let plays = || Box::new(PlaysLoop(sound.clone(), played.clone()));
    let mut m = SceneManager::with_scene(plays());
    m.enter_initial(&mut world);
    m.push(plays(), &mut world);
    m.reset(rec(&Arc::default(), "next"), &mut world);
    let [below, above] = voices(&played);
    let fade = Duration::from_millis(250);
    assert_eq!(
      audio_stops(&world),
      [(above.id(), fade), (below.id(), fade)],
      "top to bottom"
    );
    played.lock().unwrap().clear();
    m.push(plays(), &mut world);
    let [last] = voices(&played);
    m.clear(&mut world);
    assert_eq!(audio_stops(&world).last(), Some(&(last.id(), fade)));
  }

  /// A loop that fades in over 500 ms
  fn bed(world: &mut World) -> crate::core::asset::Handle<crate::core::audio::Sound> {
    use crate::core::audio::{AudioClip, Sound, SoundClip};
    let clip = world
      .resource_mut::<crate::core::asset::Assets<AudioClip>>()
      .add(AudioClip::from_frames(48_000, &vec![[0.1, 0.1]; 96_000]).unwrap());
    world
      .resource_mut::<crate::core::asset::Assets<Sound>>()
      .add(Sound {
        clips: vec![SoundClip::new(clip)],
        looping: true,
        fade_in: Duration::from_millis(500),
        ..Default::default()
      })
  }

  /// Plays its bed as it enters, or as it resumes
  struct Bed {
    sound: crate::core::asset::Handle<crate::core::audio::Sound>,
    played: Voices,
    on_resume: bool,
  }

  impl Scene for Bed {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
      if !self.on_resume {
        let voice = ctx.audio().play(&self.sound);
        self.played.lock().unwrap().push(voice);
      }
    }
    fn on_resume(&mut self, ctx: &mut SceneContext, _r: Option<Box<dyn std::any::Any>>) {
      if self.on_resume {
        let voice = ctx.audio().play(&self.sound);
        self.played.lock().unwrap().push(voice);
      }
    }
  }

  /// A pause menu: the game clock resumes as it exits
  struct Unpauses;

  impl Scene for Unpauses {
    fn on_exit(&mut self, ctx: &mut SceneContext) {
      ctx.time.game.resume();
    }
  }

  /// Three frames on (50 ms), `voice` is still fading in over the bed's
  /// 500 ms: it started held (its bus was still held from the paused frame
  /// before) and its first resume took its fade-in
  fn assert_fades_in(world: &mut World, voice: &crate::core::audio::Voice) {
    use crate::core::audio::{AudioEngine, AudioLogKind};
    use crate::core::testing::audio_step;
    use kira::sound::PlaybackState;
    for _ in 0..3 {
      audio_step(world, Duration::from_nanos(16_666_667));
    }
    let engine = world.resource::<AudioEngine>();
    assert_eq!(
      engine.voice_playback(voice.id()),
      Some(PlaybackState::Resuming)
    );
    let entries: Vec<(AudioLogKind, Duration)> = engine
      .log()
      .iter()
      .filter(|entry| entry.voice == voice.id())
      .map(|entry| (entry.kind, entry.fade))
      .collect();
    assert_eq!(
      entries,
      [
        (AudioLogKind::Start, Duration::ZERO),
        (AudioLogKind::Resume, Duration::from_millis(500))
      ]
    );
  }

  /// A reset from a pause menu: its cleanup (`TIME`) resumes the clock
  /// before the next scene enters and plays
  #[test]
  fn a_loop_played_as_a_reset_resumes_the_clock_keeps_its_fade_in() {
    use crate::core::testing::audio_step;
    let (mut world, _) = audio_scene_world();
    let sound = bed(&mut world);
    let mut m = SceneManager::with_scene(rec(&Arc::default(), "game"));
    m.enter_initial(&mut world);
    world.resource_mut::<Time>().game.pause();
    audio_step(&mut world, Duration::from_nanos(16_666_667));
    let played: Voices = Arc::default();
    let next = Box::new(Bed {
      sound,
      played: played.clone(),
      on_resume: false,
    });
    m.reset(next, &mut world);
    assert!(!world.resource::<Time>().game.is_paused());
    let [voice] = voices(&played);
    assert_fades_in(&mut world, &voice);
  }

  /// A pause overlay pops, resuming the clock, and the scene below plays
  /// in `on_resume`
  #[test]
  fn a_loop_played_in_on_resume_as_the_clock_resumes_keeps_its_fade_in() {
    use crate::core::testing::audio_step;
    let (mut world, _) = audio_scene_world();
    let sound = bed(&mut world);
    let played: Voices = Arc::default();
    let mut m = SceneManager::with_scene(Box::new(Bed {
      sound,
      played: played.clone(),
      on_resume: true,
    }));
    m.enter_initial(&mut world);
    m.push(Box::new(Unpauses), &mut world);
    world.resource_mut::<Time>().game.pause();
    audio_step(&mut world, Duration::from_nanos(16_666_667));
    m.pop(&mut world, None);
    assert!(!world.resource::<Time>().game.is_paused());
    let [voice] = voices(&played);
    assert_fades_in(&mut world, &voice);
  }

  /// A voice a scene's system plays is the scene's too; a scene pushed
  /// above holds it (its handle still says it plays) and never stops it,
  /// and it stops when its scene pops
  #[test]
  fn a_scene_s_systems_play_its_voices_and_a_scene_above_leaves_them_alone() {
    use crate::core::audio::{AudioView, Voice};
    use crate::core::testing::audio_step;

    #[derive(Resource, Default)]
    struct Played(Option<Voice>);

    struct Looper(crate::core::asset::Handle<crate::core::audio::Sound>);

    impl Scene for Looper {
      fn on_enter(&mut self, ctx: &mut SceneContext) {
        let sound = self.0.clone();
        ctx
          .add_systems(Phase::Update, move |world: &mut World| {
            if world.resource::<Played>().0.is_none() {
              let voice = AudioView::new(world).play(&sound);
              world.resource_mut::<Played>().0 = Some(voice);
            }
          })
          .unwrap();
      }
    }

    let (mut world, sound) = audio_scene_world();
    world.init_resource::<Played>();
    let mut m = SceneManager::with_scene(rec(&Arc::default(), "below"));
    m.enter_initial(&mut world);
    m.push(Box::new(Looper(sound)), &mut world);
    m.run_update_systems(&mut world);
    let Some(voice) = world.resource::<Played>().0.clone() else {
      panic!("the system played");
    };
    assert!(voice.is_playing());
    m.push(rec(&Arc::default(), "above"), &mut world);
    m.pop(&mut world, None);
    audio_step(&mut world, Duration::from_millis(20));
    assert_eq!(audio_stops(&world), [], "the looper has not exited");
    assert!(voice.is_playing(), "{voice:?}");
    m.pop(&mut world, None);
    assert_eq!(
      audio_stops(&world),
      [(voice.id(), Duration::from_millis(250))]
    );
  }

  /// What the observers of a scene's UI nodes play is the scene's
  #[test]
  fn a_scene_s_ui_nodes_play_its_voices() {
    use crate::core::audio::{AudioEngine, AudioView, Sound};
    use crate::core::ecs::{Commands, Entity, On};
    use crate::core::ui::{UiEvent, UiEventKind, UiInteraction, UiNode};

    type Node = Arc<Mutex<Option<Entity>>>;

    /// A button that plays its sound when clicked
    struct Clicky(crate::core::asset::Handle<Sound>, Node);

    impl Scene for Clicky {
      fn on_enter(&mut self, ctx: &mut SceneContext) {
        let node = ctx.spawn_ui(UiNode::button("Go").action("go"));
        let sound = self.0.clone();
        ctx
          .world
          .entity_mut(node)
          .observe(move |event: On<UiEvent>, mut commands: Commands| {
            if event.kind == UiEventKind::Clicked {
              let sound = sound.clone();
              commands.queue(move |world: &mut World| {
                AudioView::new(world).play(&sound);
              });
            }
          });
        *self.1.lock().unwrap() = Some(node);
      }
    }

    let (mut world, sound) = audio_scene_world();
    let node: Node = Arc::default();
    let mut m = SceneManager::with_scene(Box::new(Clicky(sound, node.clone())));
    m.enter_initial(&mut world);
    let mut memory = crate::core::ui::memory::UiMemory::new();
    let none = crate::core::ui::UiId::NONE;
    ui_frame(&mut m, &mut world, &mut memory, none);
    let button = node.lock().unwrap().unwrap();
    let Some(id) = world
      .get::<UiInteraction>(button)
      .and_then(UiInteraction::id)
    else {
      panic!("the button is drawn");
    };
    ui_frame(&mut m, &mut world, &mut memory, id);
    assert_eq!(world.resource::<AudioEngine>().voices_on("sfx"), 1);
    m.pop(&mut world, None);
    assert_eq!(audio_stops(&world).len(), 1, "the click's loop stops");
  }

  /// A pause menu: plays each of its sounds as it enters, and holds the
  /// scenes below it unless `holds` is false
  struct PauseMenu {
    sounds: Vec<crate::core::asset::Handle<crate::core::audio::Sound>>,
    played: Voices,
    holds: bool,
  }

  impl PauseMenu {
    fn new(holds: bool) -> Box<Self> {
      Box::new(Self {
        sounds: Vec::new(),
        played: Arc::default(),
        holds,
      })
    }
  }

  impl Scene for PauseMenu {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
      let voices: Vec<_> = self
        .sounds
        .iter()
        .map(|sound| ctx.audio().play(sound))
        .collect();
      self.played.lock().unwrap().extend(voices);
    }
    fn holds_audio_below(&self) -> bool {
      self.holds
    }
  }

  /// A one-second one-shot on `bus`
  fn shot_on(
    world: &mut World,
    bus: &str,
  ) -> crate::core::asset::Handle<crate::core::audio::Sound> {
    use crate::core::audio::{AudioClip, Sound, SoundClip};
    let clip = world
      .resource_mut::<crate::core::asset::Assets<AudioClip>>()
      .add(AudioClip::from_frames(48_000, &vec![[0.1, 0.1]; 48_000]).unwrap());
    world
      .resource_mut::<crate::core::asset::Assets<Sound>>()
      .add(Sound {
        clips: vec![SoundClip::new(clip)],
        bus: bus.to_string(),
        ..Default::default()
      })
  }

  /// What kira is doing with `voice`'s sound
  fn kira(world: &World, voice: &crate::core::audio::Voice) -> Option<kira::sound::PlaybackState> {
    world
      .resource::<crate::core::audio::AudioEngine>()
      .voice_playback(voice.id())
  }

  /// The voices the audio log has an entry of `kind` for, with its fade
  fn voice_log(world: &World, kind: crate::core::audio::AudioLogKind) -> Vec<(u32, Duration)> {
    world
      .resource::<crate::core::audio::AudioEngine>()
      .log()
      .iter()
      .filter(|entry| entry.voice != 0 && entry.kind == kind)
      .map(|entry| (entry.voice, entry.fade))
      .collect()
  }

  const FRAME: Duration = Duration::from_nanos(16_666_667);

  /// The engine's pause idiom, a menu pushed over the level, silences the
  /// level as it freezes its physics: the level's loop is held (its handle
  /// still says it plays), while the menu's own sounds, on `ui` and on
  /// `sfx`, the music and the engine's own loop play on. A pop resumes the
  /// loop where it was
  #[test]
  fn a_pushed_pause_menu_holds_the_level_s_voices_until_it_pops() {
    use crate::core::audio::{AudioLogKind, AudioView};
    use crate::core::testing::audio_step;
    use kira::sound::PlaybackState::{Paused, Playing};
    let (mut world, sound) = audio_scene_world();
    let track = theme(&mut world);
    let click = shot_on(&mut world, "ui");
    let blip = shot_on(&mut world, "sfx");
    let level: Voices = Arc::default();
    let mut m = SceneManager::with_scene(Box::new(PlaysLoop(sound.clone(), level.clone())));
    m.enter_initial(&mut world);
    let mut view = AudioView::new(&mut world);
    view.play_music(&track);
    let engine_owned = view.play(&sound);
    audio_step(&mut world, FRAME);
    let menu: Voices = Arc::default();
    let pause = Box::new(PauseMenu {
      sounds: vec![click, blip],
      played: menu.clone(),
      holds: true,
    });
    m.push(pause, &mut world);
    audio_step(&mut world, FRAME);
    let [bed] = voices(&level);
    let [click, blip] = voices(&menu);
    assert_eq!(kira(&world, &bed), Some(Paused), "held");
    assert!(bed.is_playing(), "a hold leaves it playing: {bed:?}");
    for voice in [&click, &blip, &engine_owned] {
      assert_eq!(kira(&world, voice), Some(Playing), "{voice:?}");
    }
    assert_eq!(voice_log(&world, AudioLogKind::Pause), [(bed.id(), MIN)]);
    assert_eq!(
      voice_entries(&world, "music", AudioLogKind::Start).len(),
      1,
      "the music plays on"
    );
    m.pop(&mut world, None);
    audio_step(&mut world, FRAME);
    assert_eq!(kira(&world, &bed), Some(Playing), "resumed");
    assert_eq!(voice_log(&world, AudioLogKind::Resume), [(bed.id(), MIN)]);
    assert!(
      audio_stops(&world)
        .iter()
        .all(|(voice, _)| *voice != bed.id()),
      "never stopped: {:?}",
      audio_stops(&world)
    );
  }

  const MIN: Duration = Duration::from_millis(10);

  /// A top scene that leaves the scenes below audible holds nothing
  #[test]
  fn a_top_scene_that_leaves_the_scenes_below_audible_holds_nothing() {
    use crate::core::audio::AudioLogKind;
    use crate::core::testing::audio_step;
    let (mut world, sound) = audio_scene_world();
    let level: Voices = Arc::default();
    let mut m = SceneManager::with_scene(Box::new(PlaysLoop(sound, level.clone())));
    m.enter_initial(&mut world);
    audio_step(&mut world, FRAME);
    m.push(PauseMenu::new(false), &mut world);
    audio_step(&mut world, FRAME);
    let [bed] = voices(&level);
    assert_eq!(
      kira(&world, &bed),
      Some(kira::sound::PlaybackState::Playing)
    );
    assert_eq!(voice_log(&world, AudioLogKind::Pause), []);
  }

  /// A voice held both by its scene and by the game's pause resumes only
  /// once both let it go, whichever lets go first
  #[test]
  fn a_voice_held_by_its_scene_and_the_game_s_pause_waits_for_both() {
    use crate::core::testing::audio_step;
    use kira::sound::PlaybackState::{Paused, Playing};
    let (mut world, sound) = audio_scene_world();
    let level: Voices = Arc::default();
    let mut m = SceneManager::with_scene(Box::new(PlaysLoop(sound, level.clone())));
    m.enter_initial(&mut world);
    audio_step(&mut world, FRAME);
    let [bed] = voices(&level);
    let clock = |world: &mut World, paused: bool| {
      let mut time = world.resource_mut::<Time>();
      if paused {
        time.game.pause();
      } else {
        time.game.resume();
      }
    };
    // The menu lets go first
    m.push(PauseMenu::new(true), &mut world);
    clock(&mut world, true);
    audio_step(&mut world, FRAME);
    assert_eq!(kira(&world, &bed), Some(Paused));
    m.pop(&mut world, None);
    audio_step(&mut world, FRAME);
    assert_eq!(kira(&world, &bed), Some(Paused), "the clock still holds it");
    clock(&mut world, false);
    audio_step(&mut world, FRAME);
    assert_eq!(kira(&world, &bed), Some(Playing));
    // The clock lets go first
    clock(&mut world, true);
    m.push(PauseMenu::new(true), &mut world);
    audio_step(&mut world, FRAME);
    assert_eq!(kira(&world, &bed), Some(Paused));
    clock(&mut world, false);
    audio_step(&mut world, FRAME);
    assert_eq!(kira(&world, &bed), Some(Paused), "the menu still holds it");
    m.pop(&mut world, None);
    audio_step(&mut world, FRAME);
    assert_eq!(kira(&world, &bed), Some(Playing));
  }

  /// A loop the level played while its sound loaded lands under the pause
  /// menu: it starts held, silent, and fades in over its fade-in as the
  /// menu pops
  #[test]
  fn a_loop_that_lands_under_a_pause_menu_starts_held() {
    use crate::core::asset::{AssetServer, AssetsView, MemorySource, insert_registry};
    use crate::core::audio::{AudioEngine, AudioLogKind, Sound};
    use crate::core::testing::audio_step;
    use kira::sound::PlaybackState::{Paused, Resuming};

    struct LoadsBed(Voices);

    impl Scene for LoadsBed {
      fn on_enter(&mut self, ctx: &mut SceneContext) {
        let sound = ctx.assets().load::<Sound>("sfx/bed-lands.sound.ron");
        let voice = ctx.audio().play(&sound);
        self.0.lock().unwrap().push(voice);
      }
    }

    let files = MemorySource::new();
    files.insert(
      "sfx/bed-lands.sound.ron",
      r#"(clips: [(path: "sfx/bed-lands.wav")], looping: true, fade_in: 0.2)"#,
    );
    files.insert(
      "sfx/bed-lands.wav",
      crate::core::testing::wav(48_000, 96_000, 4_000),
    );
    let mut world = world();
    insert_registry(&mut world, AssetServer::new(files));
    world.resource_mut::<AudioEngine>().set_logging(true);
    let level: Voices = Arc::default();
    let mut m = SceneManager::with_scene(Box::new(LoadsBed(level.clone())));
    m.enter_initial(&mut world);
    m.push(PauseMenu::new(true), &mut world);
    let sound = AssetsView::new(&mut world).load::<Sound>("sfx/bed-lands.sound.ron");
    for _ in 0..300 {
      if AssetsView::new(&mut world).is_ready(&sound) {
        break;
      }
      audio_step(&mut world, FRAME);
    }
    audio_step(&mut world, FRAME);
    let [bed] = voices(&level);
    assert_eq!(
      voice_log(&world, AudioLogKind::Start),
      [(bed.id(), Duration::ZERO)],
      "started silent"
    );
    assert_eq!(kira(&world, &bed), Some(Paused), "started held");
    assert!(bed.is_playing(), "{bed:?}");
    m.pop(&mut world, None);
    audio_step(&mut world, FRAME);
    assert_eq!(kira(&world, &bed), Some(Resuming), "fading in");
    assert_eq!(
      voice_log(&world, AudioLogKind::Resume),
      [(bed.id(), Duration::from_millis(200))]
    );
  }

  /// The world's entities stop with the scenes below, as physics does: an
  /// emitter's loop is held under the pause menu, and resumes as it pops
  #[test]
  fn an_emitter_s_loop_is_held_under_a_pause_menu() {
    use crate::core::audio::{AudioEmitter, AudioLogKind, Sound};
    use crate::core::testing::audio_step;
    use kira::sound::PlaybackState::{Paused, Playing};

    struct SpawnsEmitter(crate::core::asset::Handle<Sound>);

    impl Scene for SpawnsEmitter {
      fn on_enter(&mut self, ctx: &mut SceneContext) {
        let emitter = AudioEmitter {
          sound: self.0.clone(),
          play_on_spawn: true,
        };
        ctx.world.spawn((emitter, Transform::IDENTITY));
      }
    }

    let (mut world, sound) = audio_scene_world();
    let mut sounds = world.resource_mut::<crate::core::asset::Assets<Sound>>();
    let Some(mut placed) = sounds.get(&sound).cloned() else {
      panic!("the loop is in the store");
    };
    // Heard from the world's origin: the main camera, the listener, is
    // 500 px away
    placed.spatial = Some(crate::core::audio::Spatial {
      min_distance: 100.0,
      max_distance: 1000.0,
      curve: crate::core::audio::Falloff::Linear,
      pan_width: 200.0,
      low_pass_at_max: None,
    });
    let placed = sounds.add(placed);
    let mut m = SceneManager::with_scene(Box::new(SpawnsEmitter(placed)));
    m.enter_initial(&mut world);
    audio_step(&mut world, FRAME);
    let starts = voice_entries(&world, "sfx", AudioLogKind::Start);
    let [start] = &starts[..] else {
      panic!("the emitter started: {starts:?}");
    };
    let engine = world.resource::<crate::core::audio::AudioEngine>();
    assert_eq!(engine.voice_playback(start.voice), Some(Playing));
    m.push(PauseMenu::new(true), &mut world);
    audio_step(&mut world, FRAME);
    let engine = world.resource::<crate::core::audio::AudioEngine>();
    assert_eq!(engine.voice_playback(start.voice), Some(Paused));
    m.pop(&mut world, None);
    audio_step(&mut world, FRAME);
    let engine = world.resource::<crate::core::audio::AudioEngine>();
    assert_eq!(engine.voice_playback(start.voice), Some(Playing));
  }

  /// `sound`, the scene world's loop, heard from where it plays
  fn placed_loop(
    world: &mut World,
    sound: &crate::core::asset::Handle<crate::core::audio::Sound>,
  ) -> crate::core::asset::Handle<crate::core::audio::Sound> {
    use crate::core::audio::{Falloff, Sound, Spatial};
    let mut sounds = world.resource_mut::<crate::core::asset::Assets<Sound>>();
    let Some(mut placed) = sounds.get(sound).cloned() else {
      panic!("the loop is in the store");
    };
    // Heard from the world's origin, where its emitters spawn: the main
    // camera, the listener, is 500 px away
    placed.spatial = Some(Spatial {
      min_distance: 100.0,
      max_distance: 1000.0,
      curve: Falloff::Linear,
      pan_width: 200.0,
      low_pass_at_max: None,
    });
    sounds.add(placed)
  }

  /// Spawns an emitter of its sound as it enters, which plays on spawn; it
  /// holds the scenes below it, and leaves the world as it was when it
  /// exits (its emitter outlives it)
  struct SpawnsEmitter(crate::core::asset::Handle<crate::core::audio::Sound>);

  impl Scene for SpawnsEmitter {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
      let emitter = crate::core::audio::AudioEmitter {
        sound: self.0.clone(),
        play_on_spawn: true,
      };
      ctx.world.spawn((emitter, Transform::IDENTITY));
    }

    fn cleanup_on_exit(&self) -> SceneCleanup {
      SceneCleanup::NONE
    }
  }

  /// The emitters' voices, in the order they started
  fn emitter_voices(world: &World) -> Vec<u32> {
    voice_entries(world, "sfx", crate::core::audio::AudioLogKind::Start)
      .iter()
      .map(|entry| entry.voice)
      .collect()
  }

  /// What kira is doing with voice `id`'s sound
  fn kira_of(world: &World, id: u32) -> Option<kira::sound::PlaybackState> {
    world
      .resource::<crate::core::audio::AudioEngine>()
      .voice_playback(id)
  }

  /// An emitter belongs to the scene that added it: the pause menu's own
  /// emitter plays on while the menu holds the level below, and the
  /// level's is held
  #[test]
  fn an_emitter_spawned_by_the_pause_scene_plays_on_under_it() {
    use crate::core::testing::audio_step;
    use kira::sound::PlaybackState::{Paused, Playing};
    let (mut world, sound) = audio_scene_world();
    let placed = placed_loop(&mut world, &sound);
    let mut m = SceneManager::with_scene(Box::new(SpawnsEmitter(placed.clone())));
    m.enter_initial(&mut world);
    audio_step(&mut world, FRAME);
    m.push(Box::new(SpawnsEmitter(placed)), &mut world);
    audio_step(&mut world, FRAME);
    audio_step(&mut world, FRAME);
    let [level, menu] = emitter_voices(&world)[..] else {
      panic!("two emitters started: {:?}", emitter_voices(&world));
    };
    assert_eq!(kira_of(&world, menu), Some(Playing), "the menu's plays on");
    assert_eq!(kira_of(&world, level), Some(Paused), "the level's is held");
    m.pop(&mut world, None);
    audio_step(&mut world, FRAME);
    assert_eq!(kira_of(&world, level), Some(Playing), "let go at the pop");
  }

  /// The level's emitter is held under a pause menu that spawns none
  #[test]
  fn the_level_s_emitter_is_held_under_a_pause_menu() {
    use crate::core::testing::audio_step;
    use kira::sound::PlaybackState::Paused;
    let (mut world, sound) = audio_scene_world();
    let placed = placed_loop(&mut world, &sound);
    let mut m = SceneManager::with_scene(Box::new(SpawnsEmitter(placed)));
    m.enter_initial(&mut world);
    audio_step(&mut world, FRAME);
    m.push(PauseMenu::new(true), &mut world);
    audio_step(&mut world, FRAME);
    let [level] = emitter_voices(&world)[..] else {
      panic!("one emitter started: {:?}", emitter_voices(&world));
    };
    assert_eq!(kira_of(&world, level), Some(Paused));
  }

  /// An emitter whose scene was switched away (an entity that outlived its
  /// scene) belongs to no scene: as one added outside every scene, it is
  /// held while any scene is
  #[test]
  fn an_emitter_whose_scene_was_switched_away_is_held_under_a_later_pause() {
    use crate::core::testing::audio_step;
    use kira::sound::PlaybackState::{Paused, Playing};
    let (mut world, sound) = audio_scene_world();
    let placed = placed_loop(&mut world, &sound);
    let mut m = SceneManager::with_scene(Box::new(SpawnsEmitter(placed)));
    m.enter_initial(&mut world);
    audio_step(&mut world, FRAME);
    m.switch(PauseMenu::new(true), &mut world);
    audio_step(&mut world, FRAME);
    let [orphan] = emitter_voices(&world)[..] else {
      panic!("one emitter started: {:?}", emitter_voices(&world));
    };
    assert_eq!(
      kira_of(&world, orphan),
      Some(Playing),
      "it outlived its scene"
    );
    m.push(PauseMenu::new(true), &mut world);
    audio_step(&mut world, FRAME);
    assert_eq!(
      kira_of(&world, orphan),
      Some(Paused),
      "held under the pause"
    );
    m.pop(&mut world, None);
    audio_step(&mut world, FRAME);
    assert_eq!(kira_of(&world, orphan), Some(Playing));
  }

  /// A pause menu's emitter that outlives it, as the menu switches to
  /// another that holds the same scenes below, belongs to no scene at once:
  /// it is held while any scene is, though the held scenes did not change
  #[test]
  fn a_menu_s_emitter_is_held_as_the_menu_switches_to_another() {
    use crate::core::testing::audio_step;
    use kira::sound::PlaybackState::{Paused, Playing};
    let (mut world, sound) = audio_scene_world();
    let placed = placed_loop(&mut world, &sound);
    let mut m = SceneManager::with_scene(PauseMenu::new(true));
    m.enter_initial(&mut world);
    audio_step(&mut world, FRAME);
    m.push(Box::new(SpawnsEmitter(placed)), &mut world);
    audio_step(&mut world, FRAME);
    audio_step(&mut world, FRAME);
    let [menu] = emitter_voices(&world)[..] else {
      panic!("one emitter started: {:?}", emitter_voices(&world));
    };
    assert_eq!(kira_of(&world, menu), Some(Playing), "the menu's own");
    m.switch(PauseMenu::new(true), &mut world);
    audio_step(&mut world, FRAME);
    assert_eq!(
      kira_of(&world, menu),
      Some(Paused),
      "held once its scene has gone"
    );
  }

  /// A track of a two-second loop that crossfades over 300 ms
  fn theme(world: &mut World) -> crate::core::asset::Handle<crate::core::audio::Music> {
    use crate::core::audio::{AudioClip, Music};
    let clip = world
      .resource_mut::<crate::core::asset::Assets<AudioClip>>()
      .add(AudioClip::from_frames(48_000, &vec![[0.1, 0.1]; 96_000]).unwrap());
    let mut track = Music::looping(clip, 0.0, None);
    track.crossfade = Duration::from_millis(300);
    world
      .resource_mut::<crate::core::asset::Assets<Music>>()
      .add(track)
  }

  /// Plays its track as it enters (a no-op while it plays), and its loop
  /// and one-shot if it has them
  struct PlaysMusic {
    track: crate::core::asset::Handle<crate::core::audio::Music>,
    sound: Option<crate::core::asset::Handle<crate::core::audio::Sound>>,
    shot: Option<crate::core::asset::Handle<crate::core::audio::AudioClip>>,
    cleanup: SceneCleanup,
  }

  impl PlaysMusic {
    fn new(track: &crate::core::asset::Handle<crate::core::audio::Music>) -> Box<Self> {
      Box::new(Self {
        track: track.clone(),
        sound: None,
        shot: None,
        cleanup: SceneCleanup::DEFAULT,
      })
    }
  }

  impl Scene for PlaysMusic {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
      ctx.audio().play_music(&self.track);
      if let Some(sound) = &self.sound {
        ctx.audio().play(sound);
      }
      if let Some(shot) = &self.shot {
        ctx.audio().play(shot);
      }
    }

    fn cleanup_on_exit(&self) -> SceneCleanup {
      self.cleanup
    }
  }

  /// The log's entries of `kind` for voices on `bus`
  fn voice_entries(
    world: &World,
    bus: &str,
    kind: crate::core::audio::AudioLogKind,
  ) -> Vec<crate::core::audio::AudioLogEntry> {
    use crate::core::audio::AudioEngine;
    world
      .resource::<AudioEngine>()
      .log()
      .iter()
      .filter(|entry| entry.voice != 0 && entry.bus == bus && entry.kind == kind)
      .cloned()
      .collect()
  }

  /// Music belongs to the engine: a switch, a scene pushed over it and
  /// popped, and a reset, each with the default cleanup, leave it playing,
  /// and a scene that plays the track already playing changes nothing
  #[test]
  fn music_survives_a_scene_change() {
    use crate::core::audio::{AudioLogKind, AudioView};
    use crate::core::testing::audio_step;
    let (mut world, _) = audio_scene_world();
    let track = theme(&mut world);
    let mut m = SceneManager::with_scene(PlaysMusic::new(&track));
    m.enter_initial(&mut world);
    let frame = Duration::from_nanos(16_666_667);
    audio_step(&mut world, frame);
    m.switch(rec(&Arc::default(), "next"), &mut world);
    audio_step(&mut world, frame);
    m.push(rec(&Arc::default(), "pause"), &mut world);
    m.pop(&mut world, None);
    audio_step(&mut world, frame);
    m.reset(PlaysMusic::new(&track), &mut world);
    audio_step(&mut world, frame);
    m.switch(PlaysMusic::new(&track), &mut world);
    for _ in 0..30 {
      audio_step(&mut world, frame);
    }
    assert_eq!(voice_entries(&world, "music", AudioLogKind::Start).len(), 1);
    assert_eq!(voice_entries(&world, "music", AudioLogKind::Stop), []);
    let mut view = AudioView::new(&mut world);
    assert_eq!(view.current_music(), Some(track.id()));
    assert_eq!(view.voices_on("music"), 1);
  }

  /// A scene whose cleanup holds `MUSIC` stops the music as it is switched
  /// away, fading it out over its crossfade, and kira lets it go
  #[test]
  fn scene_cleanup_music_stops_it() {
    use crate::core::audio::{AudioEngine, AudioLogKind, AudioView};
    use crate::core::testing::audio_step;
    let (mut world, _) = audio_scene_world();
    let track = theme(&mut world);
    let mut scene = PlaysMusic::new(&track);
    scene.cleanup = SceneCleanup::ALL;
    let mut m = SceneManager::with_scene(scene);
    m.enter_initial(&mut world);
    audio_step(&mut world, Duration::from_millis(20));
    m.switch(rec(&Arc::default(), "menu"), &mut world);
    let stops: Vec<Duration> = voice_entries(&world, "music", AudioLogKind::Stop)
      .iter()
      .map(|entry| entry.fade)
      .collect();
    assert_eq!(stops, [Duration::from_millis(300)]);
    assert_eq!(AudioView::new(&mut world).current_music(), None);
    audio_step(&mut world, Duration::from_millis(150));
    assert_eq!(AudioView::new(&mut world).voices_on("music"), 1, "fading");
    audio_step(&mut world, Duration::from_millis(200));
    assert_eq!(AudioView::new(&mut world).voices_on("music"), 0);
    assert_eq!(world.resource::<AudioEngine>().sounds_on("music"), 0);
  }

  /// Review Focus 2: a scene switch every frame, each scene playing the
  /// track (already playing), a loop and a one-shot. No voice leaks: once
  /// the loops have faded and the one-shots ended, only the music plays,
  /// started once and never restarted
  #[test]
  fn fifty_scene_switches_leak_no_voice_and_keep_one_music() {
    use crate::core::audio::{AudioClip, AudioEngine, AudioLogKind, AudioView};
    use crate::core::testing::audio_step;
    let (mut world, sound) = audio_scene_world();
    let track = theme(&mut world);
    let shot = world
      .resource_mut::<crate::core::asset::Assets<AudioClip>>()
      .add(AudioClip::from_frames(48_000, &vec![[0.1, 0.1]; 2_400]).unwrap());
    let level = || {
      Box::new(PlaysMusic {
        track: track.clone(),
        sound: Some(sound.clone()),
        shot: Some(shot.clone()),
        cleanup: SceneCleanup::DEFAULT,
      })
    };
    let frame = Duration::from_nanos(16_666_667);
    let mut m = SceneManager::with_scene(level());
    m.enter_initial(&mut world);
    for _ in 0..50 {
      audio_step(&mut world, frame);
      m.switch(level(), &mut world);
    }
    // The last level's loop and shot play on; the rest fade out and end
    for _ in 0..30 {
      audio_step(&mut world, frame);
    }
    assert_eq!(voice_entries(&world, "music", AudioLogKind::Start).len(), 1);
    assert_eq!(voice_entries(&world, "music", AudioLogKind::Stop), []);
    assert_eq!(voice_entries(&world, "sfx", AudioLogKind::Start).len(), 102);
    assert_eq!(
      voice_entries(&world, "sfx", AudioLogKind::Stop).len(),
      50,
      "each exited level's loop stopped once"
    );
    let mut view = AudioView::new(&mut world);
    assert_eq!(view.current_music(), Some(track.id()));
    assert_eq!(view.voices_on("music"), 1);
    assert_eq!(view.voices_on("sfx"), 1, "the last level's loop");
    assert_eq!(view.voices_on("master"), 2);
    let engine = world.resource::<AudioEngine>();
    assert_eq!(engine.sounds_on("sfx"), 1, "kira holds only that loop");
    assert_eq!(engine.sounds_on("music"), 1);
  }
}
