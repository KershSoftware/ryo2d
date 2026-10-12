//! The engine's phases and the schedules behind them.

use crate::core::ecs::kernel::{
  IntoScheduleConfigs, Schedule, ScheduleLabel, ScheduleSystem, SingleThreadedExecutor,
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
  /// Animation, tweens, knockback, squash, skeletons
  Juice,
  /// The scene's `update`
  Scene,
  /// Scene-registered fixed systems
  Game,
  /// Register, sync, step, sync back
  Physics,
  /// Transform propagation
  Transforms,
  /// Trail point recording
  Trails,
}

/// Order inside `Update`.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UpdateSet {
  /// Scene-registered per-frame systems
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

/// The engine's six schedules.
pub struct EngineSchedules {
  first: Schedule,
  pre_update: Schedule,
  fixed_update: Schedule,
  update: Schedule,
  post_update: Schedule,
  last: Schedule,
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
        FixedSet::Physics,
        FixedSet::Transforms,
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

  /// Run one phase, applying its deferred commands at the end.
  pub fn run(&mut self, phase: Phase, world: &mut World) {
    self.get(phase).run(world);
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SceneScheduleError {
  #[error("scenes can add systems to FixedUpdate and Update only, not {0:?}")]
  UnsupportedPhase(Phase),
}

/// Schedules a scene owns; they run inside the `Game` sets and go away with the scene.
#[derive(Default)]
pub struct SceneSchedules {
  fixed: Option<Schedule>,
  update: Option<Schedule>,
}

#[derive(ScheduleLabel, Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct SceneOwned;

impl SceneSchedules {
  pub fn add_systems<M>(
    &mut self,
    phase: Phase,
    systems: impl IntoScheduleConfigs<ScheduleSystem, M>,
  ) -> Result<(), SceneScheduleError> {
    let slot = match phase {
      Phase::FixedUpdate => &mut self.fixed,
      Phase::Update => &mut self.update,
      other => return Err(SceneScheduleError::UnsupportedPhase(other)),
    };
    let schedule = slot.get_or_insert_with(|| new_schedule(SceneOwned));
    schedule.add_systems(systems);
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

  #[test]
  fn fixed_sets_run_in_the_declared_order() {
    let mut world = World::new();
    world.init_resource::<Log>();
    let mut s = EngineSchedules::new();
    s.add_systems(Phase::FixedUpdate, push("physics").in_set(FixedSet::Physics));
    s.add_systems(Phase::FixedUpdate, push("snapshot").in_set(FixedSet::Snapshot));
    s.add_systems(
      Phase::FixedUpdate,
      push("transforms").in_set(FixedSet::Transforms),
    );
    s.add_systems(Phase::FixedUpdate, push("game").in_set(FixedSet::Game));
    s.run(Phase::FixedUpdate, &mut world);
    assert_eq!(
      world.resource::<Log>().0,
      ["snapshot", "game", "physics", "transforms"]
    );
  }

  #[test]
  fn game_systems_can_order_against_engine_sets() {
    let mut world = World::new();
    world.init_resource::<Log>();
    let mut s = EngineSchedules::new();
    s.add_systems(Phase::FixedUpdate, push("physics").in_set(FixedSet::Physics));
    s.add_systems(Phase::FixedUpdate, push("after").after(FixedSet::Physics));
    s.add_systems(Phase::FixedUpdate, push("before").before(FixedSet::Physics));
    s.run(Phase::FixedUpdate, &mut world);
    assert_eq!(world.resource::<Log>().0, ["before", "physics", "after"]);
  }

  #[test]
  fn scene_schedules_accept_only_fixed_and_update() {
    let mut scene = SceneSchedules::default();
    assert!(scene.add_systems(Phase::FixedUpdate, push("a")).is_ok());
    assert!(scene.add_systems(Phase::Update, push("b")).is_ok());
    assert!(matches!(
      scene.add_systems(Phase::Last, push("c")),
      Err(SceneScheduleError::UnsupportedPhase(Phase::Last))
    ));
  }

  #[test]
  fn scene_schedules_run_and_flush_commands() {
    let mut world = World::new();
    world.init_resource::<Log>();
    let mut scene = SceneSchedules::default();
    scene
      .add_systems(Phase::FixedUpdate, |mut commands: Commands| {
        commands.spawn(Spawned);
      })
      .unwrap();
    scene.add_systems(Phase::Update, push("u")).unwrap();
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
