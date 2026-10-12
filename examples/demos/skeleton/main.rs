//! Skeleton Demo - Ryo2D
//!
//! Skeletal animation driven by controller state machines with crossfade
//! blending, one character per page: Fighter, Casual and Action RPG.
//! Runs on the demo-kit shell; switch pages with `[` / `]`.

mod scenes;

use std::sync::{Arc, OnceLock};

use demo_kit::prelude::*;

use scenes::{ActionRpgScene, CasualScene, FighterScene};

/// Marker for debug line entities so we can despawn/respawn them each frame.
#[derive(Component, Debug)]
pub(crate) struct DebugBone;

/// The stick figure every page animates, built once and shared
pub(crate) fn stick_fighter() -> Arc<SkeletonDef> {
  static DEF: OnceLock<Arc<SkeletonDef>> = OnceLock::new();
  DEF.get_or_init(build_stick_fighter).clone()
}

fn build_stick_fighter() -> Arc<SkeletonDef> {
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

fn main() {
  Demo::new("Skeleton")
    .resolution(1280, 720)
    .page(FighterScene::default)
    .page(CasualScene::default)
    .page(ActionRpgScene::default)
    .run();
}
