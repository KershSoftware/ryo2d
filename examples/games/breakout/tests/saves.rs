use breakout::prelude::{load_high_score, save_high_score};
use breakout::scenes::{MenuScene, PauseScene};
use ryo2d::prelude::*;
use ryo2d::testing::{HeadlessEngine, MemorySaves};

/// Saves a high score as it enters
struct Saver(u32);

impl Scene for Saver {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    save_high_score(ctx, self.0);
  }
}

/// Reads the high score as it enters, for the test to see
struct Reader;

#[derive(Resource)]
struct Read(u32);

impl Scene for Reader {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let score = load_high_score(ctx);
    ctx.insert_resource(Read(score));
  }
}

fn engine(store: &MemorySaves, scene: impl Scene + 'static) -> Option<HeadlessEngine> {
  HeadlessEngine::new_or_skip(
    EngineBuilder::new()
      .resolution(800, 600)
      .asset_source(breakout::embedded())
      .memory_saves(store.clone())
      .start_scene(scene),
  )
}

#[test]
fn the_high_score_survives_a_restart_of_the_scene() {
  let store = MemorySaves::new();
  let Some(mut first) = engine(&store, Saver(70)) else {
    return;
  };
  first.step_frames(3).unwrap();
  drop(first);

  let Some(mut second) = engine(&store, Reader) else {
    return;
  };
  second.step_frames(3).unwrap();
  assert_eq!(second.world().resource::<Read>().0, 70);

  // The menu shows it
  let Some(mut menu) = engine(&store, MenuScene::new()) else {
    return;
  };
  menu.step_frames(5).unwrap();
  let world = menu.world_mut();
  let shown = world
    .query::<&UiLabel>()
    .iter(world)
    .any(|label| label.0 == "High Score: 70");
  assert!(shown, "the menu's high score");
}

/// The game under the pause menu, which it pushes on its first update
struct Under {
  pushed: bool,
}

impl Scene for Under {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    if self.pushed {
      return SceneAction::Continue;
    }
    self.pushed = true;
    SceneAction::Push(Box::new(PauseScene::new(0, 0)))
  }
}

#[test]
fn a_volume_set_in_the_pause_menu_is_kept_when_quitting_to_the_menu() {
  let store = MemorySaves::new();
  let Some(mut engine) = HeadlessEngine::new_or_skip(
    EngineBuilder::new()
      .resolution(800, 600)
      .asset_source(breakout::embedded())
      .memory_saves(store.clone())
      .with_user_preferences(UserPreferenceFilter::only([
        UserPreference::MasterVolume,
        UserPreference::MusicVolume,
        UserPreference::SfxVolume,
      ]))
      .start_scene(Under { pushed: false }),
  ) else {
    return;
  };
  engine.step_frames(5).unwrap();
  // Up from Resume reaches a volume slider, which a step to the left lowers
  engine.tap_key(KeyCode::ArrowUp);
  engine.step_frames(2).unwrap();
  engine.tap_key(KeyCode::ArrowLeft);
  engine.step_frames(2).unwrap();
  // Q quits to the menu, which no Resume went through
  engine.tap_key(KeyCode::KeyQ);
  engine.step_frames(5).unwrap();
  drop(engine);

  let file = String::from_utf8(store.get("system.json").expect("the settings file")).unwrap();
  let lowered = ["master_volume", "music_volume", "sfx_volume"]
    .iter()
    .any(|key| file.contains(key));
  assert!(lowered, "a volume is kept: {file}");
}
