mod action_rpg;
mod casual;
mod fighter;

pub use action_rpg::ActionRpgScene;
pub use casual::CasualScene;
pub use fighter::FighterScene;

use std::sync::Arc;

use ryo2d::prelude::*;

use crate::DebugBone;

/// Shared state and helpers for skeleton demo scenes.
///
/// Holds the common fields (entity, debug lines, cached UI state) and
/// implements the identical enter/exit/debug-line/UI logic so each scene
/// only needs to provide its unique `update()` input handling.
pub(crate) struct SceneBase {
  pub controller_template: SkeletalController,
  pub skeleton_entity: Option<Entity>,
  debug_entities: Vec<Entity>,
  pub current_state: &'static str,
  pub blending: bool,
  bone_color: Color,
  joint_color: Color,
}

impl SceneBase {
  pub fn new(controller: SkeletalController, bone_color: Color, joint_color: Color) -> Self {
    Self {
      controller_template: controller,
      skeleton_entity: None,
      debug_entities: Vec::new(),
      current_state: "idle",
      blending: false,
      bone_color,
      joint_color,
    }
  }

  pub fn enter(&mut self, world: &mut World, def: &Arc<SkeletonDef>) {
    let entity = world.spawn((
      Transform::new(640.0, 360.0),
      Skeleton::new(def.clone()),
      self.controller_template.clone(),
    ));
    self.skeleton_entity = Some(entity);
    self.current_state = "idle";
    self.blending = false;
  }

  pub fn exit(&mut self, world: &mut World) {
    for entity in self.debug_entities.drain(..) {
      let _ = world.despawn(entity);
    }
    if let Some(entity) = self.skeleton_entity.take() {
      let _ = world.despawn(entity);
    }
  }

  /// Cache current controller state for UI rendering.
  pub fn cache_state(&mut self, ctrl: &SkeletalController) {
    self.current_state = ctrl.current();
    self.blending = ctrl.is_blending();
  }

  /// Redraw debug bone lines from the skeleton component.
  pub fn redraw_debug_lines(&mut self, world: &mut World) {
    // Despawn old
    for entity in self.debug_entities.drain(..) {
      let _ = world.despawn(entity);
    }

    // Spawn new
    let lines = if let Some(entity) = self.skeleton_entity {
      world
        .get::<Skeleton>(entity)
        .map(|skeleton| {
          skeleton_debug_lines(
            &skeleton.def,
            &skeleton.world_transforms,
            self.bone_color,
            self.joint_color,
            2.0,
            4.0,
          )
        })
        .unwrap_or_default()
    } else {
      Vec::new()
    };

    for line in lines {
      let e = world.spawn((Transform::default(), line, DebugBone));
      self.debug_entities.push(e);
    }
  }

  /// Render the standard UI: scene name, state, blend status, controls.
  pub fn render_ui(&self, ui: &mut UiContext, scene_name: &str, controls: &str) {
    let white = Color::rgb(220, 220, 220);
    let gray = Color::rgb(150, 150, 150);
    let yellow = Color::rgb(255, 255, 100);

    ui.draw_text(scene_name, 10.0, 10.0, white);
    ui.draw_text(
      &format!("State: {}", self.current_state),
      10.0,
      30.0,
      yellow,
    );

    let blend_status = if self.blending {
      "Blending..."
    } else {
      "Stable"
    };
    ui.draw_text(&format!("Blend: {}", blend_status), 10.0, 50.0, gray);

    ui.draw_text(controls, 10.0, 80.0, gray);
    ui.draw_text("Tab: Next scene  Esc: Exit", 10.0, 100.0, gray);
  }
}
