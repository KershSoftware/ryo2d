mod action_rpg;
mod casual;
mod fighter;

pub use action_rpg::ActionRpgScene;
pub use casual::CasualScene;
pub use fighter::FighterScene;

use std::sync::Arc;

use demo_kit::prelude::*;

use crate::DebugBone;

/// Shared state and helpers for skeleton demo scenes.
///
/// Holds the common fields (entity, debug lines, cached UI state) and
/// implements the identical enter/exit/debug-line/UI logic so each scene
/// only needs to provide its unique `update()` input handling.
pub(crate) struct SceneBase {
  def: Arc<SkeletonDef>,
  pub controller_template: SkeletalController,
  pub skeleton_entity: Option<Entity>,
  debug_entities: Vec<Entity>,
  pub current_state: &'static str,
  pub blending: bool,
  bone_color: Color,
  joint_color: Color,
}

impl SceneBase {
  pub fn new(
    def: Arc<SkeletonDef>,
    controller: SkeletalController,
    bone_color: Color,
    joint_color: Color,
  ) -> Self {
    Self {
      def,
      controller_template: controller,
      skeleton_entity: None,
      debug_entities: Vec::new(),
      current_state: "idle",
      blending: false,
      bone_color,
      joint_color,
    }
  }

  pub fn enter(&mut self, ctx: &mut SceneContext) {
    ctx.camera().set_position(640.0, 360.0);
    let entity = ctx
      .world
      .spawn((
        Transform::from_xy(640.0, 360.0),
        Skeleton::new(self.def.clone()),
        self.controller_template.clone(),
      ))
      .id();
    self.skeleton_entity = Some(entity);
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
      world.despawn(entity);
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
      let e = world.spawn((Transform::default(), line, DebugBone)).id();
      self.debug_entities.push(e);
    }
  }

  /// Render the controller state and blend status at the top of the content area
  pub fn ui(&self, ui: &mut UiContext) {
    let gray = Color::rgb(150, 150, 150);
    let yellow = Color::rgb(255, 255, 100);

    let area = content_area(ui);
    ui.draw_text(
      &format!("State: {}", self.current_state),
      area.x + 10.0,
      area.y + 8.0,
      yellow,
    );

    let blend_status = if self.blending {
      "Blending..."
    } else {
      "Stable"
    };
    ui.draw_text(
      &format!("Blend: {}", blend_status),
      area.x + 10.0,
      area.y + 28.0,
      gray,
    );
  }
}
