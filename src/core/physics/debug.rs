//! The F2 overlay: Rapier's own geometry (every collider shape, joints and
//! contact points) drawn into `DebugDraw`, coloured by body kind.

use rapier2d::prelude::{
  DebugColor, DebugRenderBackend, DebugRenderMode, DebugRenderObject, DebugRenderPipeline,
  DebugRenderStyle, RigidBodySet, RigidBodyType, Vector,
};

use super::convert::from_rapier;
use super::world::Physics;
use crate::core::config::debug::colors;
use crate::core::render::DebugDraw;
use crate::core::ui::Color;

/// Alpha for sleeping bodies and disabled colliders.
const DIMMED: u8 = 0x60;

struct Overlay<'a> {
  debug: &'a mut DebugDraw,
  bodies: &'a RigidBodySet,
}

impl DebugRenderBackend for Overlay<'_> {
  fn draw_line(&mut self, object: DebugRenderObject, a: Vector, b: Vector, _color: DebugColor) {
    if let Some(color) = colour(object, self.bodies) {
      self.debug.line(from_rapier(a), from_rapier(b), color);
    }
  }
}

fn rgba(c: [u8; 4]) -> Color {
  Color::new(c[0], c[1], c[2], c[3])
}

fn colour(object: DebugRenderObject, bodies: &RigidBodySet) -> Option<Color> {
  match object {
    DebugRenderObject::Collider(_, collider) => {
      let body = collider.parent().and_then(|handle| bodies.get(handle));
      let base = if collider.is_sensor() {
        colors::TRIGGER
      } else {
        match body.map(|body| body.body_type()) {
          Some(RigidBodyType::Dynamic) => colors::DYNAMIC,
          Some(RigidBodyType::KinematicPositionBased | RigidBodyType::KinematicVelocityBased) => {
            colors::KINEMATIC
          }
          _ => colors::FIXED,
        }
      };
      let mut color = rgba(base);
      if body.is_some_and(|body| body.is_sleeping()) || !collider.is_enabled() {
        color.a = DIMMED;
      }
      Some(color)
    }
    DebugRenderObject::ImpulseJoint(..) => Some(rgba(colors::JOINT)),
    DebugRenderObject::ContactPair(..) => Some(rgba(colors::CONTACT)),
    _ => None,
  }
}

/// Draw the simulation's colliders, joints and contacts into `debug`.
pub(crate) fn draw(physics: &Physics, debug: &mut DebugDraw) {
  let mode = DebugRenderMode::COLLIDER_SHAPES | DebugRenderMode::JOINTS | DebugRenderMode::CONTACTS;
  let mut pipeline = DebugRenderPipeline::new(DebugRenderStyle::default(), mode);
  let mut overlay = Overlay {
    debug,
    bodies: &physics.bodies,
  };
  pipeline.render(
    &mut overlay,
    &physics.bodies,
    &physics.colliders,
    &physics.joints,
    &physics.multibody_joints,
    &physics.narrow_phase,
    &physics.soft_bodies,
  );
}
