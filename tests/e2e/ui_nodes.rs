//! Node trees in the engine: scene ownership, input only for the top scene,
//! events read in the same frame, observers

use std::sync::{Arc, Mutex};

use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

#[derive(Default)]
struct Log {
  /// In order: "frame" at each of Game's `ui` calls, "observed play" from
  /// the observer on Play, "update X" / "ui X" when Game reads a click on X
  trace: Vec<String>,
  game_root: Option<Entity>,
  paused_root: Option<Entity>,
}

fn note(log: &Arc<Mutex<Log>>, what: impl Into<String>) {
  if let Ok(mut log) = log.lock() {
    log.trace.push(what.into());
  }
}

fn reads(log: &Arc<Mutex<Log>>, prefix: &str) -> Vec<String> {
  log
    .lock()
    .map(|l| {
      l.trace
        .iter()
        .filter(|t| t.starts_with(prefix))
        .cloned()
        .collect()
    })
    .unwrap_or_default()
}

/// The actions of this scene's own clicks since it last read
fn clicks(ctx: &mut SceneContext) -> Vec<String> {
  ctx
    .ui_events()
    .into_iter()
    .filter(|e| e.kind == UiEventKind::Clicked)
    .filter_map(|e| e.action)
    .collect()
}

/// Play (the default) and Pause, spawned on enter, with an observer on Play.
/// It reads its clicks in `update` or in `ui`, and Pause pushes `Paused`.
struct Game {
  log: Arc<Mutex<Log>>,
  read_in_update: bool,
}

impl Game {
  fn react(&mut self, ctx: &mut SceneContext, place: &str) -> SceneAction {
    let read = clicks(ctx);
    for action in &read {
      note(&self.log, format!("{place} {action}"));
    }
    if read.iter().any(|a| a == "pause") {
      return SceneAction::Push(Box::new(Paused {
        log: Arc::clone(&self.log),
      }));
    }
    SceneAction::Continue
  }
}

impl Scene for Game {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let root = ctx.spawn_ui(
      UiNode::column()
        .focus_scope()
        .anchor(Anchor::Center)
        .child(
          UiNode::button("Play")
            .action("play")
            .name("play")
            .default_focus(),
        )
        .child(UiNode::button("Pause").action("pause")),
    );
    if let Some(play) = ctx.find_ui("play") {
      let log = Arc::clone(&self.log);
      ctx
        .world
        .entity_mut(play)
        .observe(move |event: On<UiEvent>| {
          if event.kind == UiEventKind::Clicked {
            note(&log, "observed play");
          }
        });
    }
    if let Ok(mut log) = self.log.lock() {
      log.game_root = Some(root);
    }
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    if self.read_in_update {
      self.react(ctx, "update")
    } else {
      SceneAction::Continue
    }
  }

  fn ui(&mut self, _ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
    note(&self.log, "frame");
    if self.read_in_update {
      SceneAction::Continue
    } else {
      self.react(ctx, "ui")
    }
  }
}

/// A transparent pause menu of Resume (the default), which pops it
struct Paused {
  log: Arc<Mutex<Log>>,
}

impl Scene for Paused {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let root = ctx.spawn_ui(
      UiNode::column()
        .focus_scope()
        .anchor(Anchor::Center)
        .child(UiNode::button("Resume").action("resume").default_focus()),
    );
    if let Ok(mut log) = self.log.lock() {
      log.paused_root = Some(root);
    }
  }

  fn ui(&mut self, _ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
    if clicks(ctx).iter().any(|a| a == "resume") {
      return SceneAction::Pop;
    }
    SceneAction::Continue
  }

  fn is_transparent(&self) -> bool {
    true
  }
}

/// A fixed step every frame (240 Hz), so `update` runs in each frame
fn start(read_in_update: bool) -> Option<(HeadlessEngine, Arc<Mutex<Log>>)> {
  let log = Arc::new(Mutex::new(Log::default()));
  let game = Game {
    log: Arc::clone(&log),
    read_in_update,
  };
  let builder = EngineBuilder::new()
    .resolution(320, 240)
    .fixed_hz(240)
    .start_scene(game);
  let mut engine = HeadlessEngine::new_or_skip(builder)?;
  engine.step_frames(3).unwrap();
  Some((engine, log))
}

fn press(engine: &mut HeadlessEngine, button: GamepadButton) {
  engine.tap_pad(button);
  engine.step_frames(3).unwrap();
}

#[test]
fn update_reads_the_frames_events() {
  let Some((mut engine, log)) = start(true) else {
    return;
  };
  press(&mut engine, GamepadButton::South);
  let trace = log.lock().unwrap().trace.clone();
  let Some(at) = trace.iter().position(|t| t == "observed play") else {
    panic!("no click: {trace:?}")
  };
  assert_eq!(
    trace.get(at + 1..at + 3),
    Some(&["frame".to_string(), "update play".to_string()][..]),
    "the observer, then the frame's ui, then update in the same frame: {trace:?}"
  );
  assert_eq!(reads(&log, "update"), ["update play"], "once");
}

#[test]
fn a_click_that_changes_the_scene_is_delivered_once() {
  let Some((mut engine, log)) = start(false) else {
    return;
  };
  press(&mut engine, GamepadButton::DPadDown);
  press(&mut engine, GamepadButton::South);
  assert_eq!(
    reads(&log, "ui"),
    ["ui pause"],
    "the rerun pass did not repeat the click"
  );
}

#[test]
fn a_lower_scenes_nodes_get_no_input() {
  let Some((mut engine, log)) = start(false) else {
    return;
  };
  press(&mut engine, GamepadButton::DPadDown);
  press(&mut engine, GamepadButton::South);
  // Paused is on top: South presses its Resume, never Game's focused Pause
  press(&mut engine, GamepadButton::South);
  assert_eq!(reads(&log, "ui"), ["ui pause"]);
}

#[test]
fn ui_events_are_the_scenes_own() {
  let Some((mut engine, log)) = start(true) else {
    return;
  };
  press(&mut engine, GamepadButton::DPadDown);
  press(&mut engine, GamepadButton::South);
  press(&mut engine, GamepadButton::South); // Resume, which Game's update must not read
  engine.step_frames(3).unwrap();
  assert_eq!(reads(&log, "update"), ["update pause"]);
}

#[test]
fn a_popped_scenes_roots_are_despawned() {
  let Some((mut engine, log)) = start(false) else {
    return;
  };
  press(&mut engine, GamepadButton::DPadDown);
  press(&mut engine, GamepadButton::South);
  press(&mut engine, GamepadButton::South); // Resume pops Paused
  let (game, paused) = {
    let log = log.lock().unwrap();
    (log.game_root, log.paused_root)
  };
  let (Some(game), Some(paused)) = (game, paused) else {
    panic!("no roots")
  };
  assert!(
    engine.world().get_entity(paused).is_err(),
    "Paused's tree left with it"
  );
  assert!(engine.world().get_entity(game).is_ok(), "Game's stayed");
}

/// A scene showing one modal root, which closes (despawns) on Back
struct Dialog {
  backs: Arc<Mutex<Vec<Option<String>>>>,
  root: Option<Entity>,
}

impl Scene for Dialog {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    self.root = Some(
      ctx.spawn_ui(
        UiNode::panel()
          .modal()
          .action("options")
          .child(UiNode::button("Done").action("done").default_focus()),
      ),
    );
  }

  fn ui(&mut self, _ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
    for event in ctx.ui_events() {
      if event.kind == UiEventKind::Back {
        if let Ok(mut backs) = self.backs.lock() {
          backs.push(event.action.clone());
        }
        if let Some(root) = self.root.take() {
          ctx.world.despawn(root);
        }
      }
    }
    SceneAction::Continue
  }
}

#[test]
fn a_modal_root_traps_focus_and_back_sends_back() {
  let backs = Arc::new(Mutex::new(Vec::new()));
  let scene = Dialog {
    backs: Arc::clone(&backs),
    root: None,
  };
  let Some(mut engine) =
    HeadlessEngine::new_or_skip(EngineBuilder::new().resolution(320, 240).start_scene(scene))
  else {
    return;
  };
  engine.step_frames(3).unwrap();
  assert_eq!(engine.ui_focusables().len(), 1, "only Done");
  press(&mut engine, GamepadButton::East);
  assert_eq!(*backs.lock().unwrap(), [Some("options".to_string())]);
  assert!(engine.ui_focusables().is_empty(), "the dialog closed");
  assert_eq!(
    engine.step_frames(2).unwrap(),
    ryo2d::testing::StepOutcome::Running,
    "Back did not quit"
  );
}

/// A menu that pushes `Board` from its `ui` on its second frame
struct Menu {
  frames: u32,
  list: Arc<Mutex<Option<Entity>>>,
}

impl Scene for Menu {
  fn ui(&mut self, _ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    self.frames += 1;
    if self.frames == 2 {
      return SceneAction::Push(Box::new(Board {
        list: Arc::clone(&self.list),
      }));
    }
    SceneAction::Continue
  }
}

/// A focus scope holding only a node list, spawned on enter: the rerun UI
/// pass draws it before its pool's first update
struct Board {
  list: Arc<Mutex<Option<Entity>>>,
}

impl Scene for Board {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.spawn_ui(
      UiNode::column().focus_scope().anchor(Anchor::Center).child(
        UiNode::list(100, 20.0)
          .height(100.0)
          .width(200.0)
          .row_shape(|| UiNode::button("{name}").bound())
          .name("board"),
      ),
    );
    if let Ok(mut list) = self.list.lock() {
      *list = ctx.find_ui("board");
    }
  }

  fn ui(&mut self, _ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
    for bind in ctx.messages::<UiListBind>() {
      ctx.ui_set(bind.row, "name", format!("Player {}", bind.index + 1));
    }
    SceneAction::Continue
  }
}

#[test]
fn a_list_pushed_from_ui_gives_its_new_scope_the_default_focus() {
  let list = Arc::new(Mutex::new(None));
  let menu = Menu {
    frames: 0,
    list: Arc::clone(&list),
  };
  let Some(mut engine) =
    HeadlessEngine::new_or_skip(EngineBuilder::new().resolution(320, 240).start_scene(menu))
  else {
    return;
  };
  engine.step_frames(6).unwrap();
  let Some(list) = *list.lock().unwrap() else {
    panic!("Board was not pushed")
  };
  let world = engine.world();
  let first = world
    .get::<Children>(list)
    .into_iter()
    .flatten()
    .find(|&&row| world.get::<UiListRow>(row).is_some_and(|r| r.index() == 0))
    .copied();
  let id = first.and_then(|row| world.get::<UiInteraction>(row).and_then(UiInteraction::id));
  assert!(id.is_some(), "item 0's row is drawn");
  assert_eq!(engine.ui_focused(), id, "item 0's button has the focus");
}
