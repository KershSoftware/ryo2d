//! The engine's phases and the schedules behind them.

use std::any::TypeId;
use std::panic::Location;

use crate::core::ecs::kernel::{
  CheckChangeTicks, Direction, InternedSystemSet, IntoScheduleConfigs, NodeId, Schedule,
  ScheduleCleanupPolicy, ScheduleGraph, ScheduleLabel, ScheduleSystem, SingleThreadedExecutor,
  SystemSet, World,
};

#[derive(ScheduleLabel, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct First;
#[derive(ScheduleLabel, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PreUpdate;
#[derive(ScheduleLabel, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FixedUpdate;
#[derive(ScheduleLabel, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Update;
#[derive(ScheduleLabel, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PostUpdate;
#[derive(ScheduleLabel, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Last;

/// Which phase a system is added to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
  /// Once per frame, first: clocks, input, message buffers
  First,
  /// Once per frame, before the fixed loop
  PreUpdate,
  /// Zero or more times per frame, at the fixed rate
  FixedUpdate,
  /// Once per frame, after the fixed loop
  Update,
  /// Once per frame, before rendering: propagation, camera
  PostUpdate,
  /// Once per frame, last
  Last,
}

impl Phase {
  pub const ALL: [Phase; 6] = [
    Phase::First,
    Phase::PreUpdate,
    Phase::FixedUpdate,
    Phase::Update,
    Phase::PostUpdate,
    Phase::Last,
  ];
}

/// Order inside `FixedUpdate`.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FixedSet {
  /// Remember where everything was, for interpolation
  Snapshot,
  /// Animation, tweens, squash and stretch
  Juice,
  /// The scene's `update`
  Scene,
  /// The scenes' fixed systems, and builder systems that name no other place
  Game,
  /// Trigger behaviours, tile collider rebuilds, movement input, knockback
  /// and the movement presets
  Movement,
  /// The physics step: one system running physics' own schedule
  Physics,
  /// Transform propagation
  Transforms,
  /// Skeleton animation and pose (after propagation, so a skeleton stands
  /// where its entity does)
  Skeletons,
  /// The particle step: emitters spawn, and their particles move and die
  /// (after propagation and floors, so an emitter spawned this step emits
  /// from where it stands)
  Particles,
  /// Trail point recording
  Trails,
}

/// Order inside `Update`.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UpdateSet {
  /// The scenes' per-frame systems, and builder systems that name no other place
  Game,
}

/// Order inside `PostUpdate`.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PostUpdateSet {
  /// Transform propagation for per-frame changes
  Transforms,
  /// Camera shake and follow, in real time
  Camera,
  /// Anything that prepares render data
  Extract,
}

fn new_schedule(label: impl ScheduleLabel) -> Schedule {
  let mut schedule = Schedule::new(label);
  schedule.set_executor(SingleThreadedExecutor::new());
  schedule
}

/// One `add_game_systems` call's systems, so they can be placed together.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct GameSystems(u32);

/// The engine's six schedules.
pub struct EngineSchedules {
  first: Schedule,
  pre_update: Schedule,
  fixed_update: Schedule,
  update: Schedule,
  post_update: Schedule,
  last: Schedule,
  game_calls: u32,
}

impl Default for EngineSchedules {
  fn default() -> Self {
    Self::new()
  }
}

impl EngineSchedules {
  pub fn new() -> Self {
    let mut fixed_update = new_schedule(FixedUpdate);
    fixed_update.configure_sets(
      (
        FixedSet::Snapshot,
        FixedSet::Juice,
        FixedSet::Scene,
        FixedSet::Game,
        FixedSet::Movement,
        FixedSet::Physics,
        FixedSet::Transforms,
        FixedSet::Skeletons,
        FixedSet::Particles,
        FixedSet::Trails,
      )
        .chain(),
    );
    let mut update = new_schedule(Update);
    update.configure_sets(UpdateSet::Game);
    let mut post_update = new_schedule(PostUpdate);
    post_update.configure_sets(
      (
        PostUpdateSet::Transforms,
        PostUpdateSet::Camera,
        PostUpdateSet::Extract,
      )
        .chain(),
    );
    Self {
      first: new_schedule(First),
      pre_update: new_schedule(PreUpdate),
      fixed_update,
      update,
      post_update,
      last: new_schedule(Last),
      game_calls: 0,
    }
  }

  /// Clamp every system's change ticks after the world checked its own.
  /// Bevy clamps the schedules in its `Schedules` resource only; the
  /// engine's are not there (see `engine::frame::PendingTickCheck`).
  pub(crate) fn check_change_ticks(&mut self, check: CheckChangeTicks) {
    for schedule in [
      &mut self.first,
      &mut self.pre_update,
      &mut self.fixed_update,
      &mut self.update,
      &mut self.post_update,
      &mut self.last,
    ] {
      schedule.check_change_ticks(check);
    }
  }

  fn get(&mut self, phase: Phase) -> &mut Schedule {
    match phase {
      Phase::First => &mut self.first,
      Phase::PreUpdate => &mut self.pre_update,
      Phase::FixedUpdate => &mut self.fixed_update,
      Phase::Update => &mut self.update,
      Phase::PostUpdate => &mut self.post_update,
      Phase::Last => &mut self.last,
    }
  }

  /// Add systems to a phase for the whole run.
  pub fn add_systems<M>(
    &mut self,
    phase: Phase,
    systems: impl IntoScheduleConfigs<ScheduleSystem, M>,
  ) {
    self.get(phase).add_systems(systems);
  }

  /// Add a game's systems to a phase for the whole run
  /// (`EngineBuilder::add_systems`). In `FixedUpdate` and `Update`, a call
  /// whose systems are not in, before or after anything outside the call (a
  /// phase set, a set of the game's, another call's system) runs in the
  /// phase's `Game` set; one that names something keeps exactly that order.
  pub fn add_game_systems<M>(
    &mut self,
    phase: Phase,
    systems: impl IntoScheduleConfigs<ScheduleSystem, M>,
  ) {
    let default = match phase {
      Phase::FixedUpdate => FixedSet::Game.intern(),
      Phase::Update => UpdateSet::Game.intern(),
      _ => {
        self.get(phase).add_systems(systems);
        return;
      }
    };
    self.game_calls += 1;
    let group = GameSystems(self.game_calls);
    let schedule = self.get(phase);
    schedule.add_systems(systems.in_set(group));
    if !places_itself(schedule, group.intern()) {
      schedule.configure_sets(group.in_set(default));
    }
  }

  /// Run one phase, applying its deferred commands at the end.
  pub fn run(&mut self, phase: Phase, world: &mut World) {
    self.get(phase).run(world);
  }
}

/// Whether the systems in `group` are in, before or after anything outside
/// the call that added them: a set, or a system they did not bring. Their
/// own type sets and anonymous `run_if` sets do not count.
fn places_itself(schedule: &Schedule, group: InternedSystemSet) -> bool {
  let graph = schedule.graph();
  let Some(group) = graph.system_sets.get_key(group).map(NodeId::Set) else {
    return false;
  };
  let hierarchy = graph.hierarchy().graph();
  let dependency = graph.dependency().graph();
  let members: Vec<NodeId> = hierarchy
    .neighbors_directed(group, Direction::Outgoing)
    .collect();
  let incidental = |node: NodeId| match node {
    NodeId::Set(key) => graph
      .system_sets
      .get(key)
      .is_some_and(|set| set.system_type().is_some() || set.is_anonymous()),
    NodeId::System(_) => false,
  };
  // A system, or a type or anonymous set holding only this call's systems
  let inside = |node: NodeId| {
    members.contains(&node)
      || (incidental(node)
        && hierarchy
          .neighbors_directed(node, Direction::Outgoing)
          .all(|child| members.contains(&child)))
  };
  members.iter().any(|&system| {
    hierarchy
      .neighbors_directed(system, Direction::Incoming)
      .any(|parent| parent != group && !incidental(parent))
      || dependency
        .neighbors_directed(system, Direction::Incoming)
        .chain(dependency.neighbors_directed(system, Direction::Outgoing))
        .any(|node| !inside(node))
  })
}

/// Why [`SceneContext::add_systems`](crate::core::scene::SceneContext::add_systems)
/// refused a scene's systems.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SceneScheduleError {
  #[error("scenes can add systems to FixedUpdate and Update only, not {0:?}")]
  UnsupportedPhase(Phase),
  #[error(
    "scene systems run in the phase's Game set and cannot be placed by {0}; \
     add systems that order against the engine's sets with EngineBuilder::add_systems"
  )]
  EngineSet(&'static str),
  #[error(
    "the add_systems call at {0} already added these systems in an earlier callback; \
     add a scene's systems once, in on_enter"
  )]
  AlreadyAdded(&'static Location<'static>),
}

/// Schedules a scene owns; they run inside the `Game` sets and go away with the scene.
#[derive(Default)]
pub struct SceneSchedules {
  fixed: Option<Schedule>,
  update: Option<Schedule>,
  /// The scene's callbacks so far, counted by `begin_callback`
  callback: u64,
  /// Numbers each call's `SceneCallSet`
  next_call: u32,
  /// The calls whose systems were added
  calls: Vec<SceneCall>,
}

#[derive(ScheduleLabel, Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct SceneOwned;

/// One scene `add_systems` call's systems, so they can be checked and taken
/// out together.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct SceneCallSet(u32);

/// A scene `add_systems` call whose systems were added.
struct SceneCall {
  phase: Phase,
  site: &'static Location<'static>,
  /// The scene callback it was made in
  callback: u64,
  /// Its systems' types
  systems: Vec<TypeId>,
}

/// How a scene's systems name one of the engine's sets.
#[derive(Clone, Copy)]
enum Placement {
  In,
  After,
  Before,
}

/// The engine's sets a scene's systems may name, and their names.
fn engine_sets() -> [(InternedSystemSet, &'static str); 13] {
  [
    (FixedSet::Snapshot.intern(), "FixedSet::Snapshot"),
    (FixedSet::Juice.intern(), "FixedSet::Juice"),
    (FixedSet::Scene.intern(), "FixedSet::Scene"),
    (FixedSet::Game.intern(), "FixedSet::Game"),
    (FixedSet::Movement.intern(), "FixedSet::Movement"),
    (FixedSet::Physics.intern(), "FixedSet::Physics"),
    (FixedSet::Transforms.intern(), "FixedSet::Transforms"),
    (FixedSet::Particles.intern(), "FixedSet::Particles"),
    (FixedSet::Trails.intern(), "FixedSet::Trails"),
    (UpdateSet::Game.intern(), "UpdateSet::Game"),
    (
      PostUpdateSet::Transforms.intern(),
      "PostUpdateSet::Transforms",
    ),
    (PostUpdateSet::Camera.intern(), "PostUpdateSet::Camera"),
    (PostUpdateSet::Extract.intern(), "PostUpdateSet::Extract"),
  ]
}

/// Whether a scene's systems in `phase`, which run in the phase's `Game`
/// set, really are `placement` the engine's `set`.
fn holds(phase: Phase, placement: Placement, set: InternedSystemSet) -> bool {
  let any = |sets: &[FixedSet]| sets.iter().any(|fixed| fixed.intern() == set);
  match (phase, placement) {
    (Phase::FixedUpdate, Placement::In) => set == FixedSet::Game.intern(),
    (Phase::FixedUpdate, Placement::After) => {
      any(&[FixedSet::Snapshot, FixedSet::Juice, FixedSet::Scene])
    }
    (Phase::FixedUpdate, Placement::Before) => any(&[
      FixedSet::Movement,
      FixedSet::Physics,
      FixedSet::Transforms,
      FixedSet::Particles,
      FixedSet::Trails,
    ]),
    (Phase::Update, Placement::In) => set == UpdateSet::Game.intern(),
    _ => false,
  }
}

/// The first of the engine's sets that `members` name in a way that does not
/// hold for scene systems in `phase`.
fn misplaced(graph: &ScheduleGraph, phase: Phase, members: &[NodeId]) -> Option<&'static str> {
  let hierarchy = graph.hierarchy().graph();
  let dependency = graph.dependency().graph();
  engine_sets().into_iter().find_map(|(set, name)| {
    let set_node = NodeId::Set(graph.system_sets.get_key(set)?);
    let names = |&member: &NodeId, placement| match placement {
      Placement::In => hierarchy.contains_edge(set_node, member),
      Placement::After => dependency.contains_edge(set_node, member),
      Placement::Before => dependency.contains_edge(member, set_node),
    };
    let wrong = members.iter().any(|member| {
      [Placement::In, Placement::After, Placement::Before]
        .into_iter()
        .any(|placement| names(member, placement) && !holds(phase, placement, set))
    });
    wrong.then_some(name)
  })
}

impl SceneSchedules {
  /// A scene callback begins: the calls from here are its calls.
  pub(crate) fn begin_callback(&mut self) {
    self.callback += 1;
  }

  /// Add a scene's systems to `phase`. A call is refused, and adds none of
  /// its systems, when it places them where scene systems never run (they
  /// run in the phase's `Game` set), or when the same place in the game's
  /// code added any of them in an earlier callback.
  #[track_caller]
  pub fn add_systems<M>(
    &mut self,
    phase: Phase,
    systems: impl IntoScheduleConfigs<ScheduleSystem, M>,
    world: &mut World,
  ) -> Result<(), SceneScheduleError> {
    let site = Location::caller();
    let slot = match phase {
      Phase::FixedUpdate => &mut self.fixed,
      Phase::Update => &mut self.update,
      other => return Err(SceneScheduleError::UnsupportedPhase(other)),
    };
    let schedule = slot.get_or_insert_with(|| new_schedule(SceneOwned));
    let group = SceneCallSet(self.next_call);
    // A call refused every step counts on for years; a refused call's set is gone
    self.next_call = self.next_call.wrapping_add(1);
    schedule.add_systems(systems.in_set(group));

    let graph = schedule.graph();
    let members: Vec<NodeId> = graph
      .system_sets
      .get_key(group.intern())
      .map(|key| {
        graph
          .hierarchy()
          .graph()
          .neighbors_directed(NodeId::Set(key), Direction::Outgoing)
          .collect()
      })
      .unwrap_or_default();
    let types: Vec<TypeId> = members
      .iter()
      .filter_map(|member| match member {
        NodeId::System(key) => graph.systems.get(*key),
        NodeId::Set(_) => None,
      })
      .map(|system| system.system().system_type())
      .collect();
    let callback = self.callback;
    let refused = misplaced(graph, phase, &members)
      .map(SceneScheduleError::EngineSet)
      .or_else(|| {
        let again = self.calls.iter().any(|call| {
          call.phase == phase
            && call.site == site
            && call.callback != callback
            && call.systems.iter().any(|ty| types.contains(ty))
        });
        again.then_some(SceneScheduleError::AlreadyAdded(site))
      });

    if let Some(error) = refused {
      if let Err(removal) =
        schedule.remove_systems_in_set(group, world, ScheduleCleanupPolicy::RemoveSetAndSystems)
      {
        log::warn!("a refused scene add_systems call could not be taken out: {removal}");
      }
      return Err(error);
    }
    self.calls.push(SceneCall {
      phase,
      site,
      callback,
      systems: types,
    });
    Ok(())
  }

  pub fn run_fixed(&mut self, world: &mut World) {
    if let Some(schedule) = &mut self.fixed {
      schedule.run(world);
    }
  }

  pub fn run_update(&mut self, world: &mut World) {
    if let Some(schedule) = &mut self.update {
      schedule.run(world);
    }
  }

  pub(crate) fn check_change_ticks(&mut self, check: CheckChangeTicks) {
    for schedule in [&mut self.fixed, &mut self.update].into_iter().flatten() {
      schedule.check_change_ticks(check);
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::kernel::{Commands, Component, ResMut, Resource};

  #[derive(Resource, Default)]
  struct Log(Vec<&'static str>);

  #[derive(Component)]
  struct Spawned;

  fn push(label: &'static str) -> impl Fn(ResMut<Log>) {
    move |mut log: ResMut<Log>| log.0.push(label)
  }

  fn log(world: &World) -> Vec<&'static str> {
    world.resource::<Log>().0.clone()
  }

  fn engine_fixed_sets(s: &mut EngineSchedules) {
    s.add_systems(
      Phase::FixedUpdate,
      (
        push("juice").in_set(FixedSet::Juice),
        push("scene").in_set(FixedSet::Scene),
        push("physics").in_set(FixedSet::Physics),
        push("transforms").in_set(FixedSet::Transforms),
      ),
    );
  }

  #[test]
  fn the_skeleton_set_runs_after_transforms_and_before_particles() {
    let mut world = World::new();
    world.init_resource::<Log>();
    let mut s = EngineSchedules::new();
    s.add_systems(
      Phase::FixedUpdate,
      (
        push("particles").in_set(FixedSet::Particles),
        push("skeletons").in_set(FixedSet::Skeletons),
        push("transforms").in_set(FixedSet::Transforms),
      ),
    );
    s.run(Phase::FixedUpdate, &mut world);
    assert_eq!(log(&world), ["transforms", "skeletons", "particles"]);
  }

  #[test]
  fn a_game_system_that_names_no_set_runs_in_game() {
    let mut world = World::new();
    world.init_resource::<Log>();
    let mut s = EngineSchedules::new();
    s.add_game_systems(Phase::FixedUpdate, (push("a"), push("b")).chain());
    engine_fixed_sets(&mut s);
    s.add_game_systems(Phase::FixedUpdate, push("c"));
    s.run(Phase::FixedUpdate, &mut world);
    let log = log(&world);
    let at = |label| log.iter().position(|l| *l == label).unwrap_or(usize::MAX);
    for game in ["a", "b", "c"] {
      assert!(
        at("scene") < at(game) && at(game) < at("physics"),
        "{game}: {log:?}"
      );
    }
    assert!(at("a") < at("b"), "a chain inside a call is kept: {log:?}");
  }

  #[test]
  fn a_game_system_keeps_the_place_it_names() {
    let mut world = World::new();
    world.init_resource::<Log>();
    let mut s = EngineSchedules::new();
    s.add_game_systems(
      Phase::FixedUpdate,
      push("late")
        .after(FixedSet::Physics)
        .before(FixedSet::Transforms),
    );
    s.add_game_systems(Phase::FixedUpdate, push("early").in_set(FixedSet::Juice));
    engine_fixed_sets(&mut s);
    s.run(Phase::FixedUpdate, &mut world);
    let log = log(&world);
    let at = |label| log.iter().position(|l| *l == label).unwrap_or(usize::MAX);
    assert!(at("early") < at("scene"), "{log:?}");
    assert!(
      at("physics") < at("late") && at("late") < at("transforms"),
      "{log:?}"
    );
  }

  fn read_contacts(mut log: ResMut<Log>) {
    log.0.push("read");
  }

  fn react(mut log: ResMut<Log>) {
    log.0.push("react");
  }

  #[test]
  fn a_game_system_ordered_against_another_keeps_that_order() {
    let mut world = World::new();
    world.init_resource::<Log>();
    let mut s = EngineSchedules::new();
    engine_fixed_sets(&mut s);
    s.add_game_systems(Phase::FixedUpdate, read_contacts.after(FixedSet::Physics));
    // Putting this in Game (before Physics) would make a cycle
    s.add_game_systems(Phase::FixedUpdate, react.after(read_contacts));
    s.run(Phase::FixedUpdate, &mut world);
    let log = log(&world);
    let at = |label| log.iter().position(|l| *l == label).unwrap_or(usize::MAX);
    assert!(
      at("physics") < at("read") && at("read") < at("react"),
      "{log:?}"
    );
  }

  #[test]
  fn fixed_sets_run_in_the_declared_order() {
    let mut world = World::new();
    world.init_resource::<Log>();
    let mut s = EngineSchedules::new();
    s.add_systems(
      Phase::FixedUpdate,
      push("physics").in_set(FixedSet::Physics),
    );
    s.add_systems(
      Phase::FixedUpdate,
      push("movement").in_set(FixedSet::Movement),
    );
    s.add_systems(
      Phase::FixedUpdate,
      push("snapshot").in_set(FixedSet::Snapshot),
    );
    s.add_systems(
      Phase::FixedUpdate,
      push("transforms").in_set(FixedSet::Transforms),
    );
    s.add_systems(Phase::FixedUpdate, push("trails").in_set(FixedSet::Trails));
    s.add_systems(
      Phase::FixedUpdate,
      push("particles").in_set(FixedSet::Particles),
    );
    s.add_systems(Phase::FixedUpdate, push("game").in_set(FixedSet::Game));
    s.run(Phase::FixedUpdate, &mut world);
    assert_eq!(
      world.resource::<Log>().0,
      [
        "snapshot",
        "game",
        "movement",
        "physics",
        "transforms",
        "particles",
        "trails"
      ]
    );
  }

  #[test]
  fn game_systems_can_order_against_engine_sets() {
    let mut world = World::new();
    world.init_resource::<Log>();
    let mut s = EngineSchedules::new();
    s.add_systems(
      Phase::FixedUpdate,
      push("physics").in_set(FixedSet::Physics),
    );
    s.add_systems(Phase::FixedUpdate, push("after").after(FixedSet::Physics));
    s.add_systems(Phase::FixedUpdate, push("before").before(FixedSet::Physics));
    s.run(Phase::FixedUpdate, &mut world);
    assert_eq!(world.resource::<Log>().0, ["before", "physics", "after"]);
  }

  #[test]
  fn scene_schedules_accept_only_fixed_and_update() {
    let mut world = World::new();
    let mut scene = SceneSchedules::default();
    assert!(
      scene
        .add_systems(Phase::FixedUpdate, push("a"), &mut world)
        .is_ok()
    );
    assert!(
      scene
        .add_systems(Phase::Update, push("b"), &mut world)
        .is_ok()
    );
    assert!(matches!(
      scene.add_systems(Phase::Last, push("c"), &mut world),
      Err(SceneScheduleError::UnsupportedPhase(Phase::Last))
    ));
  }

  #[test]
  fn scene_schedules_run_and_flush_commands() {
    let mut world = World::new();
    world.init_resource::<Log>();
    let mut scene = SceneSchedules::default();
    scene
      .add_systems(
        Phase::FixedUpdate,
        |mut commands: Commands| {
          commands.spawn(Spawned);
        },
        &mut world,
      )
      .unwrap();
    scene
      .add_systems(Phase::Update, push("u"), &mut world)
      .unwrap();
    scene.run_fixed(&mut world);
    assert_eq!(
      world.query::<&Spawned>().iter(&world).count(),
      1,
      "commands flushed"
    );
    scene.run_update(&mut world);
    assert_eq!(world.resource::<Log>().0, ["u"]);
    // An empty schedule is fine to run
    SceneSchedules::default().run_fixed(&mut world);
  }

  #[test]
  fn every_phase_runs() {
    let mut world = World::new();
    world.init_resource::<Log>();
    let mut s = EngineSchedules::new();
    for (phase, name) in [
      (Phase::First, "first"),
      (Phase::PreUpdate, "pre"),
      (Phase::FixedUpdate, "fixed"),
      (Phase::Update, "update"),
      (Phase::PostUpdate, "post"),
      (Phase::Last, "last"),
    ] {
      s.add_systems(phase, push(name));
    }
    for phase in Phase::ALL {
      s.run(phase, &mut world);
    }
    assert_eq!(
      world.resource::<Log>().0,
      ["first", "pre", "fixed", "update", "post", "last"]
    );
  }
}
