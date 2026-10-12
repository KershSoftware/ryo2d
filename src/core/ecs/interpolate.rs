//! Render interpolation between fixed steps.

use super::kernel::Query;
use super::transform::{GlobalTransform, PreviousGlobalTransform};

/// Runs first in every fixed step: remember where everything was.
pub fn snapshot_previous_transforms(
  mut query: Query<(&GlobalTransform, &mut PreviousGlobalTransform)>,
) {
  for (global, mut previous) in &mut query {
    previous.global = *global;
    previous.skip = false;
  }
}

/// The transform to draw this frame.
pub fn render_global(
  global: &GlobalTransform,
  previous: Option<&PreviousGlobalTransform>,
  alpha: f32,
) -> GlobalTransform {
  match previous {
    Some(prev) if !prev.skip => prev.global.lerp(global, alpha),
    _ => *global,
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::transform::Transform;
  use glam::Vec2;

  fn at(x: f32) -> GlobalTransform {
    GlobalTransform::from_transform(&Transform::from_xy(x, 0.0))
  }

  #[test]
  fn without_previous_the_current_transform_is_drawn() {
    let g = at(10.0);
    assert_eq!(render_global(&g, None, 0.5), g);
  }

  #[test]
  fn with_previous_the_blend_is_drawn() {
    let prev = PreviousGlobalTransform {
      global: at(0.0),
      skip: false,
    };
    let got = render_global(&at(10.0), Some(&prev), 0.25);
    assert!((got.translation() - Vec2::new(2.5, 0.0)).length() < 1e-4);
  }

  #[test]
  fn skip_draws_current() {
    let prev = PreviousGlobalTransform {
      global: at(0.0),
      skip: true,
    };
    assert_eq!(render_global(&at(10.0), Some(&prev), 0.25), at(10.0));
  }

  #[test]
  fn snapshot_copies_and_clears_skip() {
    use crate::core::ecs::kernel::{Schedule, ScheduleLabel, World};
    #[derive(ScheduleLabel, Debug, Clone, Copy, PartialEq, Eq, Hash)]
    struct Step;
    let mut world = World::new();
    let e = world
      .spawn((
        at(3.0),
        PreviousGlobalTransform {
          global: at(0.0),
          skip: true,
        },
      ))
      .id();
    let mut s = Schedule::new(Step);
    s.add_systems(snapshot_previous_transforms);
    s.run(&mut world);
    let prev = world.get::<PreviousGlobalTransform>(e).unwrap();
    assert_eq!(prev.global, at(3.0));
    assert!(!prev.skip);
  }
}
