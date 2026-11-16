//! What a shipped build (no debug assertions, no `dev-tools` feature) leaves out.
//! Empty in a development test run: `cargo test --release --test no_tools`.
#![cfg(not(dev_tools))]

use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

#[test]
#[allow(clippy::assertions_on_constants)]
fn dev_tools_are_off_in_a_release_build() {
  assert!(!ryo2d::DEV_TOOLS);
}

/// The keys a development build takes for its tools
const TOOL_KEYS: [KeyCode; 8] = [
  KeyCode::F1,
  KeyCode::F2,
  KeyCode::F3,
  KeyCode::F4,
  KeyCode::F9,
  KeyCode::F10,
  KeyCode::F12,
  KeyCode::Backquote,
];

/// The keys the scene saw pressed, in its fixed steps
#[derive(Resource, Default)]
struct Seen(Vec<KeyCode>);

/// Registers a command and prints to the console, then records the keys
/// it sees pressed
struct Game;

impl Scene for Game {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.world.insert_resource(Seen::default());
    ctx
      .console()
      .command(("game:win", "Win"), |_: &mut Game, _ctx, cmd| {
        cmd.print("won");
        Ok(())
      });
    ctx.console().print("a line nobody sees");
    assert!(!ctx.console().remove("game:win"), "there is no console");
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    let pressed: Vec<KeyCode> = TOOL_KEYS
      .into_iter()
      .chain([KeyCode::F5])
      .filter(|&key| ctx.input.key_pressed(key))
      .collect();
    ctx.world.resource_mut::<Seen>().0.extend(pressed);
    SceneAction::Continue
  }
}

fn engine(builder: EngineBuilder) -> Option<HeadlessEngine> {
  HeadlessEngine::new_or_skip(builder.resolution(64, 64))
}

#[test]
fn console_calls_compile_and_do_nothing() {
  let builder = EngineBuilder::new()
    .console_command(("game:gold", "Add gold"), |_world, cmd| {
      cmd.print("gold");
      Ok(())
    })
    .remove_console_command("help")
    .start_scene(Game);
  let Some(mut e) = engine(builder) else {
    return;
  };
  e.step_frames(3).unwrap();
  assert_eq!(e.frame_count(), 3);
  // A game's command test compiles here too, and sees no output
  assert!(e.console_run("game:gold").is_empty());
  assert!(e.console_run("game:win").is_empty());
}

#[test]
fn the_debug_keys_reach_the_game() {
  let Some(mut e) = engine(EngineBuilder::new().start_scene(Game)) else {
    return;
  };
  // As a windowed game takes them, every tool key on
  e.windowed_debug_keys();
  e.step().unwrap();
  for key in TOOL_KEYS {
    e.press_key(key);
  }
  e.step().unwrap();
  let seen = &e.world().resource::<Seen>().0;
  for key in TOOL_KEYS {
    assert!(
      seen.contains(&key),
      "{key:?} did not reach the game: {seen:?}"
    );
  }
}

#[test]
fn f5_still_toggles_borderless() {
  let Some(mut e) = engine(EngineBuilder::new().start_scene(Game)) else {
    return;
  };
  e.windowed_debug_keys();
  e.step().unwrap();
  assert!(!e.borderless_toggle_pending());
  e.tap_key(KeyCode::F5);
  e.step().unwrap();
  assert!(e.borderless_toggle_pending(), "F5 asks for borderless");
  assert!(
    !e.world().resource::<Seen>().0.contains(&KeyCode::F5),
    "F5 is the engine's"
  );
}

#[test]
fn a_project_cannot_switch_tools_on() {
  let debug = ryo2d::testing::resolved_debug(
    "[debug]\nconsole = true\ndev_window_keys = true\nlogger_window = true\n\
     enable_profiler = true\nshow_colliders = true\nshow_velocities = true\n\
     show_origins = true\nshow_frame_timing = true\n",
    false,
  );
  assert!(!debug.dev_console, "console");
  assert!(!debug.dev_window_keys, "dev window keys");
  assert!(!debug.logger_window, "logger window");
  assert!(!debug.enable_profiler, "profiler");
  assert!(!debug.show_colliders, "colliders");
  assert!(!debug.show_velocities, "velocities");
  assert!(!debug.show_origins, "origins");
  assert!(!debug.show_frame_timing, "frame timing");
  assert!(
    !debug.show_ui_debug && !debug.show_ui_rebuilds,
    "UI overlays"
  );
}

#[test]
fn a_launch_cannot_switch_the_profiler_on() {
  // As `ryo2d dev --profiler` launches a game (`RYO_PROFILER_AUTO_OPEN`)
  let debug = ryo2d::testing::resolved_debug("[debug]\nenable_profiler = true\n", true);
  assert!(
    !debug.enable_profiler,
    "the launch has the last word only with tools"
  );
}

#[test]
fn the_fps_counter_still_follows_its_setting() {
  let Some(mut e) = engine(EngineBuilder::new().start_scene(Game)) else {
    return;
  };
  // A windowed engine follows the setting; a headless one leaves it off
  e.windowed_debug_keys();
  e.step_frames(2).unwrap();
  assert_eq!(e.ui_draw_count(), 0, "nothing draws before the setting");
  e.world_mut().resource_mut::<Settings>().set_show_fps(true);
  e.step_frames(2).unwrap();
  assert!(e.ui_draw_count() > 0, "the FPS counter draws");
  e.world_mut().resource_mut::<Settings>().set_show_fps(false);
  e.step_frames(2).unwrap();
  assert_eq!(e.ui_draw_count(), 0, "and goes with the setting");
}

#[test]
fn a_scripted_run_shows_its_log() {
  // A shipped game's log is off stderr until a scripting variable is set
  ryo2d::testing::set_log_stderr(false);
  ryo2d::testing::start_log_for_script(&["RYO_CAPTURE_FRAMES"]);
  assert!(
    ryo2d::testing::log_stderr_on(),
    "a run with RYO_CAPTURE_FRAMES says what it captured"
  );
}

#[test]
fn the_bones_overlay_draws_without_developer_tools() {
  use ryo2d::testing::{SheetError, SheetOptions, SheetSkeleton, skeleton_sheet};
  use std::sync::Arc;

  // A red square on a bone 20 long: the sheet draws the bone over it, in
  // the overlay's amber, in a build that has no `debug:skeletons`
  let rig = |world: &mut World| {
    let image = world
      .resource_mut::<Assets<Image>>()
      .add(Image::solid_rect(12, 12, 220, 30, 40, 255));
    let shown = SlotSetup {
      attachment: Some(Arc::from("body")),
      ..SlotSetup::default()
    };
    let bone = BoneSetup {
      length: 20.0,
      ..BoneSetup::default()
    };
    SkeletonData::builder()
      .bone("body", None, bone)
      .slot("body", "body", shown)
      .region(SkeletonRegion {
        name: Arc::from("body"),
        image,
        x: 0,
        y: 0,
        width: 12,
        height: 12,
      })
      .attachment(
        SkeletonData::DEFAULT_SKIN,
        "body",
        "body",
        SkeletonAttachment::Region(RegionAttachment::new(0, 12.0, 12.0)),
      )
      .build()
  };
  let bone_pixels = |bones: bool| {
    let options = SheetOptions {
      moments: 1,
      bones,
      cell: Some(UVec2::new(120, 100)),
      ..SheetOptions::default()
    };
    match skeleton_sheet(SheetSkeleton::built(rig), &options) {
      Ok(sheet) => Some(
        sheet
          .pixels()
          .filter(|pixel| (0..3).all(|c| pixel.0[c].abs_diff([0xff, 0xc0, 0x20][c]) < 12))
          .count(),
      ),
      Err(SheetError::NoGpu) => None,
      Err(error) => panic!("the sheet failed: {error}"),
    }
  };
  let (Some(off), Some(on)) = (bone_pixels(false), bone_pixels(true)) else {
    eprintln!("skipping: no GPU available for the sheet");
    return;
  };
  assert_eq!(off, 0, "no bones unless asked for");
  assert!(on > 8, "{on} bone pixels");
}
