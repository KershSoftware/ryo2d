//! Skeleton Demo - Ryo2D Example
//!
//! Demonstrates the skeletal animation system with controller-based state machines:
//! - Fighter scene: idle, walk, punch, kick, block with crossfade blending
//! - Casual scene: idle, walk, wave, jump, dance
//! - Action RPG scene: idle, run, attack, dodge, victory
//!
//! ## Controls
//!
//! - Tab: Cycle between scenes
//! - Escape: Exit
//! - See each scene for specific controls

mod scenes;

use std::sync::Arc;

use ryo2d::prelude::*;

use scenes::{ActionRpgScene, CasualScene, FighterScene};

/// Marker for debug line entities so we can despawn/respawn them each frame.
#[derive(Debug)]
pub(crate) struct DebugBone;

const SCENE_NAMES: [&str; 3] = ["Fighter", "Casual", "Action RPG"];

fn create_stick_fighter() -> Arc<SkeletonDef> {
  Arc::new(
    SkeletonDef::builder()
      // Core (Y-down: negative Y = up on screen)
      .bone("hips", None, Vec2::new(0.0, 0.0), 0.0, 10.0)
      .bone("spine", Some("hips"), Vec2::new(0.0, -30.0), 0.0, 30.0)
      .bone("chest", Some("spine"), Vec2::new(0.0, -30.0), 0.0, 20.0)
      .bone("neck", Some("chest"), Vec2::new(0.0, -15.0), 0.0, 10.0)
      .bone("head", Some("neck"), Vec2::new(0.0, -10.0), 0.0, 15.0)
      // Left arm
      .bone(
        "l_shoulder",
        Some("chest"),
        Vec2::new(20.0, -10.0),
        0.0,
        5.0,
      )
      .bone(
        "l_upper_arm",
        Some("l_shoulder"),
        Vec2::new(25.0, 0.0),
        0.0,
        25.0,
      )
      .bone(
        "l_forearm",
        Some("l_upper_arm"),
        Vec2::new(25.0, 0.0),
        0.0,
        22.0,
      )
      .bone("l_hand", Some("l_forearm"), Vec2::new(22.0, 0.0), 0.0, 10.0)
      // Right arm
      .bone(
        "r_shoulder",
        Some("chest"),
        Vec2::new(-20.0, -10.0),
        0.0,
        5.0,
      )
      .bone(
        "r_upper_arm",
        Some("r_shoulder"),
        Vec2::new(-25.0, 0.0),
        0.0,
        25.0,
      )
      .bone(
        "r_forearm",
        Some("r_upper_arm"),
        Vec2::new(-25.0, 0.0),
        0.0,
        22.0,
      )
      .bone(
        "r_hand",
        Some("r_forearm"),
        Vec2::new(-22.0, 0.0),
        0.0,
        10.0,
      )
      // Left leg
      .bone("l_hip", Some("hips"), Vec2::new(10.0, 0.0), 0.0, 5.0)
      .bone("l_thigh", Some("l_hip"), Vec2::new(0.0, 30.0), 0.0, 30.0)
      .bone("l_shin", Some("l_thigh"), Vec2::new(0.0, 30.0), 0.0, 28.0)
      .bone("l_foot", Some("l_shin"), Vec2::new(0.0, 28.0), 0.0, 12.0)
      // Right leg
      .bone("r_hip", Some("hips"), Vec2::new(-10.0, 0.0), 0.0, 5.0)
      .bone("r_thigh", Some("r_hip"), Vec2::new(0.0, 30.0), 0.0, 30.0)
      .bone("r_shin", Some("r_thigh"), Vec2::new(0.0, 30.0), 0.0, 28.0)
      .bone("r_foot", Some("r_shin"), Vec2::new(0.0, 28.0), 0.0, 12.0)
      .build(),
  )
}

// =============================================================================
// Main Scene - Delegates to sub-scenes
// =============================================================================

struct SkeletonDemoScene {
  skeleton_def: Arc<SkeletonDef>,
  current_scene: usize,
  fighter: FighterScene,
  casual: CasualScene,
  action_rpg: ActionRpgScene,
  entered: bool,
}

impl SkeletonDemoScene {
  fn new() -> Self {
    let def = create_stick_fighter();
    let fighter = FighterScene::new(&def);
    let casual = CasualScene::new(&def);
    let action_rpg = ActionRpgScene::new(&def);
    Self {
      skeleton_def: def,
      current_scene: 0,
      fighter,
      casual,
      action_rpg,
      entered: false,
    }
  }

  fn enter_current(&mut self, world: &mut World) {
    let def = &self.skeleton_def;
    match self.current_scene {
      0 => self.fighter.enter(world, def),
      1 => self.casual.enter(world, def),
      2 => self.action_rpg.enter(world, def),
      _ => {}
    }
  }

  fn exit_current(&mut self, world: &mut World) {
    match self.current_scene {
      0 => self.fighter.exit(world),
      1 => self.casual.exit(world),
      2 => self.action_rpg.exit(world),
      _ => {}
    }
  }
}

impl Scene for SkeletonDemoScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.camera.set_position(640.0, 360.0);
    self.enter_current(ctx.world);
    self.entered = true;
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Quit;
    }

    // Tab: cycle scenes
    if ctx.input.key_pressed(KeyCode::Tab) {
      self.exit_current(ctx.world);
      self.current_scene = (self.current_scene + 1) % SCENE_NAMES.len();
      self.enter_current(ctx.world);
    }

    // Delegate update to current sub-scene
    match self.current_scene {
      0 => self.fighter.update(ctx, dt),
      1 => self.casual.update(ctx, dt),
      2 => self.action_rpg.update(ctx, dt),
      _ => {}
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction {
    match self.current_scene {
      0 => self.fighter.render_ui(ui),
      1 => self.casual.render_ui(ui),
      2 => self.action_rpg.render_ui(ui),
      _ => {}
    }

    // Scene indicator at top-right
    let gray = Color::rgb(150, 150, 150);
    let scene_label = format!(
      "[{}/{}] {}",
      self.current_scene + 1,
      SCENE_NAMES.len(),
      SCENE_NAMES[self.current_scene]
    );
    ui.draw_text(&scene_label, 1050.0, 10.0, gray);

    SceneAction::Continue
  }
}

fn main() {
  EngineBuilder::new()
    .title("Skeleton Demo")
    .resolution(1280, 720)
    .start_scene(SkeletonDemoScene::new())
    .build()
    .run()
    .unwrap();
}
