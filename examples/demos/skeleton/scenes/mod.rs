mod action_rpg;
mod casual;
mod fighter;

pub use action_rpg::ActionRpgScene;
pub use casual::CasualScene;
pub use fighter::FighterScene;

use std::sync::Arc;

use demo_kit::prelude::*;

/// Shared state and helpers for skeleton demo scenes.
///
/// Holds the common fields (entity, debug lines, cached UI state) and
/// implements the identical enter/exit/debug-line/UI logic so each scene
/// only needs to provide its unique `update()` input handling.
pub(crate) struct SceneBase {
  def: Arc<SkeletonDef>,
  pub controller_template: SkeletalController,
  pub skeleton_entity: Option<Entity>,
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

  /// Draw the skeleton's bones and joints this step.
  pub fn draw_bones(&self, ctx: &mut SceneContext) {
    let Some(entity) = self.skeleton_entity else {
      return;
    };
    let Some(skeleton) = ctx.world.get::<Skeleton>(entity) else {
      return;
    };
    let (def, transforms) = (skeleton.def.clone(), skeleton.world_transforms.clone());
    let mut debug = ctx.debug_draw();
    let (bone, joint) = (self.bone_color, self.joint_color);
    draw_skeleton(&mut debug, &def, &transforms, bone, joint, 2.0, 4.0);
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
