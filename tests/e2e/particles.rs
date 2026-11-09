//! The engine's particles over time: a paused clock, hitstop, a pause menu
//! over the game, unscaled emitters, half speed, scene changes and frame
//! rates, read back as particle positions; and drawn: where they are,
//! between steps, on their render layers, culled, hidden when disabled, in
//! a stable order and by z among entities, read back as pixels.
//!
//! Tests skip when no GPU is available.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use ryo2d::prelude::*;
use ryo2d::testing::{HeadlessEngine, particle_positions};

/// What the scenes tell the test, and what the test asks of them
#[derive(Default)]
struct Board {
  /// The level's game-time emitter
  game: Option<Entity>,
  /// The level's unscaled emitter
  unscaled: Option<Entity>,
  /// The pause menu's own emitter
  menu: Option<Entity>,
  /// The level pushes the pause menu in its next update
  push: bool,
  /// The pause menu pops in its next update
  pop: bool,
  /// The level switches to an empty scene in its next update
  switch: bool,
}

type Shared = Arc<Mutex<Board>>;

/// The effects the scenes play
#[derive(Clone, Copy, Default, PartialEq)]
enum Kind {
  /// Two particles a step, each going right at 60 px/s (a pixel a step)
  #[default]
  Stream,
  /// One particle going right at 60 px/s
  Single,
  /// A spray in every direction at random speeds, falling
  Spray,
}

/// How the level places its emitters
#[derive(Clone, Copy)]
struct Setup {
  kind: Kind,
  /// Its game emitter's speed
  speed: f32,
  /// It places an unscaled emitter too
  unscaled: bool,
  /// It moves its game emitter a pixel right each update
  moving: bool,
}

impl Default for Setup {
  fn default() -> Self {
    Self {
      kind: Kind::Stream,
      speed: 1.0,
      unscaled: false,
      moving: false,
    }
  }
}

/// Every particle lives 100 s, and a one-shot's cycle lasts as long
fn effect(ctx: &mut SceneContext, kind: Kind) -> Handle<ParticleEffect> {
  let image = ctx
    .assets()
    .add(Image::solid_rect(2, 2, 255, 255, 255, 255));
  let effect = ParticleEffect::new(image)
    .with_lifetime(100.0)
    .with_duration(100.0)
    .with_direction(0.0, 0.0)
    .with_speed(60.0);
  let effect = match kind {
    Kind::Stream => effect.with_rate(120.0),
    Kind::Single => effect.with_rate(0.0).with_burst(Burst::at(0.0, 1.0)),
    Kind::Spray => effect
      .with_rate(90.0)
      .with_direction(0.0, 360.0)
      .with_speed((20.0, 80.0))
      .with_gravity(0.0, 50.0),
  };
  ctx.assets().add(effect)
}

struct Level {
  board: Shared,
  setup: Setup,
}

impl Scene for Level {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let fx = effect(ctx, self.setup.kind);
    let game = if self.setup.speed == 1.0 {
      ctx.particles().play_at(&fx, Vec2::new(30.0, 30.0))
    } else {
      let emitter = ParticleEmitter::new(fx.clone()).with_speed(self.setup.speed);
      ctx
        .world
        .spawn((Transform::from_xy(30.0, 30.0), emitter))
        .id()
    };
    let unscaled = self.setup.unscaled.then(|| {
      ctx
        .world
        .spawn((
          Transform::from_xy(10.0, 50.0),
          ParticleEmitter::new(fx).unscaled(),
        ))
        .id()
    });
    let mut board = self.board.lock().unwrap();
    board.game = Some(game);
    board.unscaled = unscaled;
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    let mut board = self.board.lock().unwrap();
    if self.setup.moving
      && let Some(game) = board.game
      && let Some(mut transform) = ctx.world.get_mut::<Transform>(game)
    {
      transform.translation.x += 1.0;
    }
    if std::mem::take(&mut board.push) {
      return SceneAction::Push(Box::new(Menu {
        board: self.board.clone(),
      }));
    }
    if std::mem::take(&mut board.switch) {
      return SceneAction::Switch(Box::new(Empty));
    }
    SceneAction::Continue
  }
}

/// A pause menu over the level, with its own emitter
struct Menu {
  board: Shared,
}

impl Scene for Menu {
  fn is_transparent(&self) -> bool {
    true
  }

  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let fx = effect(ctx, Kind::Stream);
    let menu = ctx.particles().play_at(&fx, Vec2::new(10.0, 10.0));
    self.board.lock().unwrap().menu = Some(menu);
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    if std::mem::take(&mut self.board.lock().unwrap().pop) {
      SceneAction::Pop
    } else {
      SceneAction::Continue
    }
  }
}

struct Empty;

impl Scene for Empty {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

fn engine(setup: Setup) -> Option<(HeadlessEngine, Shared)> {
  let board = Shared::default();
  let level = Level {
    board: board.clone(),
    setup,
  };
  let engine =
    HeadlessEngine::new_or_skip(EngineBuilder::new().resolution(64, 64).start_scene(level))?;
  Some((engine, board))
}

fn entity(board: &Shared, which: impl Fn(&Board) -> Option<Entity>) -> Entity {
  which(&board.lock().unwrap()).expect("the scene placed it")
}

fn ask(board: &Shared, what: impl Fn(&mut Board)) {
  what(&mut board.lock().unwrap());
}

fn positions(engine: &HeadlessEngine, emitter: Entity) -> Vec<Vec2> {
  particle_positions(engine.world(), emitter)
}

/// Where `emitter`'s oldest particle is
fn oldest(engine: &HeadlessEngine, emitter: Entity) -> Vec2 {
  positions(engine, emitter)[0]
}

fn time(engine: &mut HeadlessEngine) -> Mut<'_, Time> {
  engine.world_mut().resource_mut::<Time>()
}

fn timestep(engine: &HeadlessEngine) -> Duration {
  engine.world().resource::<Time>().fixed.timestep_duration()
}

fn fixed_steps(engine: &HeadlessEngine) -> u64 {
  engine.world().resource::<Time>().fixed.step_count()
}

#[test]
fn a_paused_clock_and_hitstop_freeze_particles() {
  let Some((mut engine, board)) = engine(Setup::default()) else {
    return;
  };
  engine.step_frames(10).unwrap();
  let game = entity(&board, |b| b.game);
  let moving = positions(&engine, game);
  assert!(!moving.is_empty());

  time(&mut engine).game.pause();
  engine.step_frames(30).unwrap();
  assert_eq!(
    positions(&engine, game),
    moving,
    "a paused clock runs no step"
  );
  time(&mut engine).game.resume();
  engine.step_frames(1).unwrap();
  let resumed = positions(&engine, game);
  assert_ne!(resumed, moving, "they move on as the clock resumes");

  time(&mut engine).game.hitstop(0.5);
  engine.step_frames(20).unwrap();
  assert_eq!(positions(&engine, game), resumed, "hitstop runs no step");
  engine.step_frames(20).unwrap();
  assert_ne!(
    positions(&engine, game),
    resumed,
    "they move on as it ends, in real time"
  );
}

#[test]
fn a_pause_menu_freezes_the_game_s_particles_but_not_its_own() {
  let Some((mut engine, board)) = engine(Setup::default()) else {
    return;
  };
  engine.step_frames(10).unwrap();
  ask(&board, |b| b.push = true);
  engine.step_frames(5).unwrap();
  let game = entity(&board, |b| b.game);
  let menu = entity(&board, |b| b.menu);
  let held = positions(&engine, game);
  let menu_before = oldest(&engine, menu);

  engine.step_frames(30).unwrap();
  assert_eq!(
    positions(&engine, game),
    held,
    "the game's particles are held"
  );
  assert!(
    oldest(&engine, menu).x > menu_before.x,
    "the menu's own particles move"
  );

  ask(&board, |b| b.pop = true);
  engine.step_frames(5).unwrap();
  assert!(
    oldest(&engine, game).x > held[0].x,
    "the game's move on after the menu pops"
  );
}

#[test]
fn an_unscaled_emitter_runs_through_pauses() {
  let setup = Setup {
    unscaled: true,
    ..Setup::default()
  };
  let Some((mut engine, board)) = engine(setup) else {
    return;
  };
  engine.step_frames(10).unwrap();
  let game = entity(&board, |b| b.game);
  let unscaled = entity(&board, |b| b.unscaled);
  let check = |engine: &mut HeadlessEngine, under: &str| {
    let game_before = positions(engine, game);
    let unscaled_before = oldest(engine, unscaled);
    engine.step_frames(10).unwrap();
    assert_eq!(
      positions(engine, game),
      game_before,
      "{under}: the game-time emitter is frozen"
    );
    let moved = oldest(engine, unscaled).x - unscaled_before.x;
    assert!(
      (moved - 10.0).abs() < 1e-3,
      "{under}: the unscaled emitter's particle moves a pixel a frame, not {moved}"
    );
  };

  time(&mut engine).game.pause();
  check(&mut engine, "a paused clock");
  time(&mut engine).game.resume();
  time(&mut engine).game.hitstop(10.0);
  check(&mut engine, "hitstop");
  time(&mut engine).game.clear_hitstop();
  ask(&board, |b| b.push = true);
  engine.step_frames(3).unwrap();
  check(&mut engine, "a pause menu");
}

#[test]
fn half_speed_halves_the_distance() {
  // How far the lone particle goes in 120 fixed steps' worth of real time,
  // each frame `real` long
  let distance = |speed: f32, clock: f32, real_steps: u32| -> Option<f32> {
    let setup = Setup {
      kind: Kind::Single,
      speed,
      ..Setup::default()
    };
    let (mut engine, board) = engine(setup)?;
    time(&mut engine).game.set_speed(clock);
    let real = timestep(&engine) * real_steps;
    for _ in 0..5 {
      engine.step_real(real);
    }
    let game = entity(&board, |b| b.game);
    let start = oldest(&engine, game);
    for _ in 0..120 / real_steps {
      engine.step_real(real);
    }
    Some(oldest(&engine, game).x - start.x)
  };
  let Some(full) = distance(1.0, 1.0, 1) else {
    return;
  };
  assert!((full - 120.0).abs() < 1e-2, "60 px/s for 2 s, not {full}");
  // Half-speed clock frames of two steps' real time each run one step
  let Some(slow_motion) = distance(1.0, 0.5, 2) else {
    return;
  };
  assert!(
    (slow_motion - full / 2.0).abs() < 1e-2,
    "half the game time: {slow_motion} of {full}"
  );
  let Some(half_emitter) = distance(0.5, 1.0, 1) else {
    return;
  };
  assert!(
    (half_emitter - full / 2.0).abs() < 1e-2,
    "an emitter at half speed: {half_emitter} of {full}"
  );
}

#[test]
fn a_switch_leaves_no_particles_and_push_pop_keeps_them() {
  let Some((mut engine, board)) = engine(Setup::default()) else {
    return;
  };
  engine.step_frames(20).unwrap();
  let game = entity(&board, |b| b.game);
  let before = positions(&engine, game);
  assert!(!before.is_empty());

  ask(&board, |b| b.push = true);
  engine.step_frames(5).unwrap();
  ask(&board, |b| b.pop = true);
  engine.step_frames(5).unwrap();
  let after = positions(&engine, game);
  assert!(
    after.len() > before.len(),
    "the game emits on after the pop"
  );
  assert!(after[0].x > before[0].x, "its oldest particle lived on");
  assert!(engine.world().get_entity(game).is_ok());

  ask(&board, |b| b.switch = true);
  engine.step_frames(3).unwrap();
  assert!(
    engine.world().get_entity(game).is_err(),
    "the switch despawned the level's entities"
  );
  assert_eq!(ParticlesView::new(engine.world_mut()).count(), 0);
}

#[test]
fn frame_rate_does_not_change_particles() {
  let mut runs = Vec::new();
  // 30, 60 and 144 frames a second, at a 60 Hz fixed step
  for (steps, of) in [(2, 1), (1, 1), (5, 12)] {
    let setup = Setup {
      kind: Kind::Spray,
      moving: true,
      ..Setup::default()
    };
    let Some((mut engine, board)) = engine(setup) else {
      return;
    };
    let real = timestep(&engine) * steps / of;
    let mut frames = 0;
    while fixed_steps(&engine) < 120 {
      engine.step_real(real);
      frames += 1;
      assert!(frames < 1_000, "120 steps never came");
    }
    assert_eq!(fixed_steps(&engine), 120, "at {real:?} a frame");
    let game = entity(&board, |b| b.game);
    runs.push((frames, positions(&engine, game)));
  }
  let (_, sixty) = &runs[1];
  assert!(sixty.len() > 100, "{} particles", sixty.len());
  for (frames, particles) in &runs {
    assert_eq!(particles, sixty, "after {frames} frames");
  }
}

// Drawing

type Placed = Arc<Mutex<Vec<Entity>>>;

/// A scene placing what its `setup` spawns, telling the test the entities,
/// on a black background with colours drawn as they are
struct Stage {
  setup: fn(&mut SceneContext) -> Vec<Entity>,
  placed: Placed,
}

impl Scene for Stage {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.look().tonemap = Tonemap::Clamp;
    let camera = ctx.camera().entity();
    if let Some(mut camera) = ctx.world.get_mut::<Camera2d>(camera) {
      camera.clear = CameraClear::Color(Color::BLACK);
    }
    *self.placed.lock().unwrap() = (self.setup)(ctx);
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }
}

/// A headless engine `size` big running a [`Stage`] of `setup`
fn stage(
  size: (u32, u32),
  setup: fn(&mut SceneContext) -> Vec<Entity>,
) -> Option<(HeadlessEngine, Placed)> {
  let placed = Placed::default();
  let scene = Stage {
    setup,
    placed: placed.clone(),
  };
  let builder = EngineBuilder::new()
    .resolution(size.0, size.1)
    .start_scene(scene);
  Some((HeadlessEngine::new_or_skip(builder)?, placed))
}

/// The `i`th entity the stage placed
fn nth(placed: &Placed, i: usize) -> Entity {
  placed.lock().unwrap()[i]
}

/// One particle from a burst, standing still, `size` px across, living
/// 100 s, `color` all its life
fn still(ctx: &mut SceneContext, size: f32, color: Color) -> ParticleEffect {
  let image = ctx
    .assets()
    .add(Image::solid_rect(4, 4, 255, 255, 255, 255));
  ParticleEffect::new(image)
    .with_rate(0.0)
    .with_burst(Burst::at(0.0, 1.0))
    .with_speed(0.0)
    .with_size(size)
    .with_lifetime(100.0)
    .with_duration(100.0)
    .with_color(color)
    .with_color_over_life(Gradient::constant(Color::WHITE))
}

/// An emitter of `effect` at (`x`, `y`)
fn emitter_at(ctx: &mut SceneContext, effect: ParticleEffect, x: f32, y: f32) -> Entity {
  let effect = ctx.assets().add(effect);
  ctx
    .world
    .spawn((Transform::from_xy(x, y), ParticleEmitter::new(effect)))
    .id()
}

const WHITE: [u8; 4] = [255, 255, 255, 255];
const BLACK: [u8; 4] = [0, 0, 0, 255];
const RED: [u8; 4] = [220, 30, 40, 255];

/// The last frame's pixel at (`x`, `y`)
fn pixel(engine: &HeadlessEngine, x: f32, y: f32) -> [u8; 4] {
  engine.frame().unwrap().get_pixel(x as u32, y as u32).0
}

#[test]
fn particles_draw_where_they_are() {
  let Some((mut engine, _)) = stage((200, 200), |ctx| {
    let fx = still(ctx, 16.0, Color::WHITE);
    vec![emitter_at(ctx, fx, 100.0, 100.0)]
  }) else {
    return;
  };
  engine.step_frames(3).unwrap();
  assert_eq!(pixel(&engine, 100.0, 100.0), WHITE, "its centre");
  assert_eq!(pixel(&engine, 120.0, 100.0), BLACK, "20 px right");
  assert_eq!(pixel(&engine, 100.0, 80.0), BLACK, "20 px up");
  assert_eq!(engine.debug_stats().sprite_count, 1, "one particle");
}

#[test]
fn a_local_particle_draws_with_its_emitter() {
  let Some((mut engine, placed)) = stage((128, 64), |ctx| {
    let fx = still(ctx, 16.0, Color::WHITE).with_space(Space::Local);
    vec![emitter_at(ctx, fx, 32.0, 32.0)]
  }) else {
    return;
  };
  engine.step_frames(3).unwrap();
  assert_eq!(pixel(&engine, 32.0, 32.0), WHITE);
  let emitter = nth(&placed, 0);
  if let Some(mut transform) = engine.world_mut().get_mut::<Transform>(emitter) {
    transform.translation.x = 96.0;
  }
  engine.step_frames(2).unwrap();
  assert_eq!(pixel(&engine, 96.0, 32.0), WHITE, "where its emitter is");
  assert_eq!(pixel(&engine, 32.0, 32.0), BLACK, "not where it was born");
  assert_eq!(positions(&engine, emitter), [Vec2::new(96.0, 32.0)]);
}

#[test]
fn render_layers_choose_the_cameras() {
  // The main camera (layer 0) on the left half and a layer 1 camera on the
  // right, both over (16, 16), where a green particle is on layer 0 and a
  // white one on layer 1
  let Some((mut engine, _)) = stage((64, 32), |ctx| {
    ctx.camera().set_position(16.0, 16.0);
    ctx.camera().set_viewport(Viewport::new(0.0, 0.0, 0.5, 1.0));
    let right = Camera2d {
      viewport: Viewport::new(0.5, 0.0, 0.5, 1.0),
      order: 1,
      clear: CameraClear::Color(Color::BLACK),
      ..Default::default()
    };
    let at = Transform::from_xy(16.0, 16.0);
    let right = ctx.world.spawn((at, right, RenderLayers::layer(1))).id();
    let green = still(ctx, 8.0, Color::rgb(0, 255, 0));
    let green = emitter_at(ctx, green, 16.0, 16.0);
    let white = still(ctx, 8.0, Color::WHITE);
    let white = emitter_at(ctx, white, 16.0, 16.0);
    ctx.world.entity_mut(white).insert(RenderLayers::layer(1));
    vec![right, green, white]
  }) else {
    return;
  };
  engine.step_frames(3).unwrap();
  assert_eq!(
    pixel(&engine, 16.0, 16.0),
    [0, 255, 0, 255],
    "the main camera draws layer 0's alone"
  );
  assert_eq!(
    pixel(&engine, 48.0, 16.0),
    WHITE,
    "the second camera draws layer 1's alone"
  );
}

#[test]
fn draws_blend_between_steps() {
  // A particle going right at 1200 px/s: 20 px a step
  let Some((mut engine, placed)) = stage((128, 64), |ctx| {
    let fx = still(ctx, 4.0, Color::WHITE)
      .with_speed(1200.0)
      .with_direction(0.0, 0.0);
    vec![emitter_at(ctx, fx, 20.0, 32.0)]
  }) else {
    return;
  };
  engine.step_frames(3).unwrap();
  let emitter = nth(&placed, 0);
  let now = positions(&engine, emitter)[0];
  assert!(now.x > 50.0, "it moved: {now}");
  // Frames of whole steps draw it where it was the step before
  assert_eq!(pixel(&engine, now.x - 20.0, now.y), WHITE);
  assert_eq!(pixel(&engine, now.x, now.y), BLACK);

  // Half a step later no step has run, and it draws halfway
  let half = timestep(&engine) / 2;
  engine.step_real(half);
  assert_eq!(positions(&engine, emitter)[0], now, "no step ran");
  assert_eq!(pixel(&engine, now.x - 10.0, now.y), WHITE, "halfway");
  assert_eq!(pixel(&engine, now.x - 20.0, now.y), BLACK);
  assert_eq!(pixel(&engine, now.x, now.y), BLACK);
}

#[test]
fn an_off_screen_emitter_submits_nothing_but_keeps_stepping() {
  let Some((mut engine, placed)) = stage((64, 64), |ctx| {
    let fx = still(ctx, 4.0, Color::WHITE)
      .with_speed(60.0)
      .with_direction(0.0, 0.0);
    let away = emitter_at(ctx, fx.clone(), 5000.0, 5000.0);
    let here = emitter_at(ctx, fx, 10.0, 32.0);
    vec![away, here]
  }) else {
    return;
  };
  engine.step_frames(3).unwrap();
  let away = nth(&placed, 0);
  let before = positions(&engine, away);
  assert_eq!(before.len(), 1);
  engine.step_frames(3).unwrap();
  assert!(
    positions(&engine, away)[0].x > before[0].x,
    "it steps off screen"
  );
  let stats = engine.debug_stats();
  assert_eq!(stats.particles, 2, "both alive");
  assert_eq!(stats.sprite_count, 1, "only the one on screen submitted");
}

#[test]
fn a_disabled_emitter_draws_nothing() {
  let Some((mut engine, placed)) = stage((64, 64), |ctx| {
    let fx = still(ctx, 16.0, Color::WHITE);
    vec![emitter_at(ctx, fx, 32.0, 32.0)]
  }) else {
    return;
  };
  engine.step_frames(2).unwrap();
  assert_eq!(pixel(&engine, 32.0, 32.0), WHITE);
  let emitter = nth(&placed, 0);

  engine.world_mut().entity_mut(emitter).insert(Disabled);
  engine.step_frames(1).unwrap();
  assert_eq!(pixel(&engine, 32.0, 32.0), BLACK, "hidden");
  assert_eq!(engine.debug_stats().sprite_count, 0);
  assert_eq!(positions(&engine, emitter).len(), 1, "kept, frozen");

  engine.world_mut().entity_mut(emitter).remove::<Disabled>();
  engine.step_frames(1).unwrap();
  assert_eq!(pixel(&engine, 32.0, 32.0), WHITE, "shown again");
}

#[test]
fn draw_order_is_stable_as_particles_die() {
  // Three particles in one place, born 0.1 s apart, each living 0.25 s and
  // going from red to blue over its life: the youngest, the reddest, is on
  // top until it is the last
  let Some((mut engine, placed)) = stage((64, 64), |ctx| {
    let image = ctx
      .assets()
      .add(Image::solid_rect(4, 4, 255, 255, 255, 255));
    let red_to_blue = Gradient::new([(0.0, Color::rgb(255, 0, 0)), (1.0, Color::rgb(0, 0, 255))]);
    let fx = ParticleEffect::new(image)
      .with_rate(0.0)
      .with_burst(Burst::at(0.0, 1.0))
      .with_burst(Burst::at(0.1, 1.0))
      .with_burst(Burst::at(0.2, 1.0))
      .with_speed(0.0)
      .with_size(16.0)
      .with_lifetime(0.25)
      .with_duration(100.0)
      .with_color_over_life(red_to_blue);
    vec![emitter_at(ctx, fx, 32.0, 32.0)]
  }) else {
    return;
  };
  let emitter = nth(&placed, 0);
  // The frame each living particle was born in, the oldest first (all live
  // as long, so the oldest dies first)
  let mut births: Vec<u32> = Vec::new();
  let mut seen = Vec::new();
  for frame in 0..40 {
    engine.step_frames(1).unwrap();
    let alive = positions(&engine, emitter).len();
    while births.len() > alive {
      births.remove(0);
    }
    while births.len() < alive {
      births.push(frame);
    }
    if let (true, Some(&born)) = (alive >= 2, births.last()) {
      // A step a frame: the youngest's colour at its age
      let t = (frame - born) as f32 / 60.0 / 0.25;
      let red = 255.0 * (1.0 - t);
      let top = pixel(&engine, 32.0, 32.0);
      assert!(
        (f32::from(top[0]) - red).abs() <= 3.0,
        "with {alive} alive the youngest ({red:.0} red) is on top: {top:?}"
      );
    }
    // From the first particle on
    if seen.last() != Some(&alive) && (alive > 0 || !seen.is_empty()) {
      seen.push(alive);
    }
  }
  assert_eq!(seen, [1, 2, 3, 2, 1, 0], "born and died in order");
}

#[test]
fn a_run_sorts_by_z_with_entities() {
  // A red square at z 1 between particles at z 0 and at z 2
  let Some((mut engine, _)) = stage((64, 64), |ctx| {
    let red = ctx
      .assets()
      .add(Image::solid_rect(24, 24, 220, 30, 40, 255));
    let square = Renderable::image(red).with_z_order(1);
    let square = ctx
      .world
      .spawn((Transform::from_xy(32.0, 32.0), square))
      .id();
    let below = still(ctx, 6.0, Color::WHITE);
    let below = emitter_at(ctx, below, 26.0, 32.0);
    let above = still(ctx, 6.0, Color::WHITE).with_z(2);
    let above = emitter_at(ctx, above, 38.0, 32.0);
    // The emitter's z over its effect's
    let lowered = still(ctx, 6.0, Color::WHITE).with_z(2);
    let lowered = ctx.assets().add(lowered);
    let lowered = ParticleEmitter::new(lowered).with_z(0);
    let lowered = ctx
      .world
      .spawn((Transform::from_xy(32.0, 24.0), lowered))
      .id();
    vec![square, below, above, lowered]
  }) else {
    return;
  };
  engine.step_frames(3).unwrap();
  assert_eq!(pixel(&engine, 26.0, 32.0), RED, "over particles at z 0");
  assert_eq!(pixel(&engine, 38.0, 32.0), WHITE, "under particles at z 2");
  assert_eq!(
    pixel(&engine, 32.0, 24.0),
    RED,
    "under an emitter's own z 0"
  );
  assert_eq!(engine.debug_stats().sprite_count, 4, "the square and three");
}
