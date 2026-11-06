//! The step's hooks: the floor filter, and the contact hook one-way
//! colliders shape dynamic bodies' contacts through.
//!
//! A collider's floor rides in its Rapier user data beside its one-way
//! direction (bits 0 to 64, `one_way.rs`): bits 65 to 72 hold the floor's
//! number and bit 73 marks every floor. The ground floor is 0, so a ground
//! collider's user data is what it was before floors.

use rapier2d::prelude::{
  ActiveHooks, ContactModificationContext, PairFilterContext, PhysicsHooks, SolverFlags,
};

use super::collider::Collider;
use super::one_way;
use crate::core::ecs::Floor;

/// Where a floor's number starts in the user data.
const FLOOR_SHIFT: u32 = 65;
/// Marks a collider on every floor.
const EVERY: u128 = 1 << 73;

/// A floor as user data bits.
pub(crate) fn floor_bits(floor: Floor) -> u128 {
  match floor {
    Floor::On(n) => u128::from(n) << FLOOR_SHIFT,
    Floor::Every => EVERY,
  }
}

/// The floor in a collider's user data.
pub(crate) fn floor_of(user_data: u128) -> Floor {
  if user_data & EVERY != 0 {
    Floor::Every
  } else {
    Floor::On((user_data >> FLOOR_SHIFT) as u8)
  }
}

/// The filters a collider on `floor` needs. Any hook turns off Rapier's
/// contact reuse for the pair, so only colliders off the ground floor and
/// not on every floor are filtered: two unfiltered colliders are both on
/// the ground floor (or on every floor), which always meet.
pub(crate) fn floor_hooks(floor: Floor) -> ActiveHooks {
  match floor {
    Floor::On(n) if n > 0 => {
      ActiveHooks::FILTER_CONTACT_PAIRS | ActiveHooks::FILTER_INTERSECTION_PAIR
    }
    _ => ActiveHooks::empty(),
  }
}

/// A collider's Rapier user data: its one-way direction and its floor.
pub(crate) fn user_data(collider: &Collider, floor: Floor) -> u128 {
  one_way::encode(collider.one_way) | floor_bits(floor)
}

/// The hooks a collider needs for its one-way setting and its floor.
pub(crate) fn active_hooks(collider: &Collider, floor: Floor) -> ActiveHooks {
  one_way::hooks(collider.one_way) | floor_hooks(floor)
}

/// Whether a pair's colliders are on floors that meet.
fn floors_meet(context: &PairFilterContext) -> bool {
  match (
    context.colliders.get(context.collider1),
    context.colliders.get(context.collider2),
  ) {
    (Some(a), Some(b)) => floor_of(a.user_data).meets(floor_of(b.user_data)),
    _ => true,
  }
}

/// The hooks `Physics::step` passes Rapier: pairs on floors that do not
/// meet are dropped (contacts and trigger overlaps alike), and contacts
/// with one-way colliders are shaped.
pub(crate) struct EngineHooks;

impl PhysicsHooks for EngineHooks {
  fn filter_contact_pair(&self, context: &PairFilterContext) -> Option<SolverFlags> {
    floors_meet(context).then(SolverFlags::default)
  }

  fn filter_intersection_pair(&self, context: &PairFilterContext) -> bool {
    floors_meet(context)
  }

  fn modify_solver_contacts(&self, context: &mut ContactModificationContext) {
    one_way::modify_contacts(context);
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::Vec2;
  use crate::core::physics::Collider;

  #[test]
  fn only_colliders_off_the_ground_floor_and_not_on_every_floor_get_floor_hooks() {
    let filters = ActiveHooks::FILTER_CONTACT_PAIRS | ActiveHooks::FILTER_INTERSECTION_PAIR;
    assert_eq!(floor_hooks(Floor::GROUND), ActiveHooks::empty());
    assert_eq!(floor_hooks(Floor::Every), ActiveHooks::empty());
    assert_eq!(floor_hooks(Floor::On(1)), filters);
    assert_eq!(floor_hooks(Floor::On(255)), filters);
    let platform = Collider::rect(100.0, 10.0).one_way();
    assert_eq!(
      active_hooks(&platform, Floor::On(1)),
      ActiveHooks::MODIFY_SOLVER_CONTACTS | filters
    );
    assert_eq!(
      active_hooks(&platform, Floor::GROUND),
      ActiveHooks::MODIFY_SOLVER_CONTACTS
    );
    assert_eq!(floor_bits(Floor::GROUND), 0, "one-way data is unchanged");
    for floor in [Floor::On(0), Floor::On(1), Floor::On(255), Floor::Every] {
      assert_eq!(floor_of(floor_bits(floor)), floor);
      // The direction keeps its bits beside the floor's
      let data = user_data(&platform, floor);
      assert_eq!(floor_of(data), floor);
      assert_eq!(one_way::decode(data), Some(Vec2::NEG_Y), "{floor:?}");
    }
  }
}
