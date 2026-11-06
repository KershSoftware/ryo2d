//! Storeys: which floor of a map an entity stands on.
//!
//! "Floor" here is a storey (raised ground, an upper floor, a bridge), not
//! the surface underfoot that `CharacterState::floor` names.

use super::kernel::Component;

/// The storey an entity is on. Colliders on floors that do not meet never
/// interact: no contacts, no trigger entries, and a character's sweep
/// passes through them. This is apart from the 32 collision layer and mask
/// bits, which stay the game's.
///
/// An entity without one is on the ground floor (`Floor::GROUND`). A
/// collider takes its own entity's `Floor`, else its body's. `Every` puts
/// an entity on every floor at once, for a trigger that should catch
/// whatever floor enters it.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Floor {
  /// Floor `n`: 0 is the ground floor, higher numbers storeys above it
  On(u8),
  /// Every floor at once
  Every,
}

impl Default for Floor {
  fn default() -> Self {
    Self::GROUND
  }
}

impl Floor {
  /// The ground floor, where everything without a `Floor` is.
  pub const GROUND: Floor = Floor::On(0);

  /// Whether things on the two floors meet: a floor meets its own number,
  /// and `Every` meets every floor.
  pub fn meets(self, other: Floor) -> bool {
    match (self, other) {
      (Floor::On(a), Floor::On(b)) => a == b,
      _ => true,
    }
  }

  /// The floor's number; `None` for `Every`.
  pub fn number(self) -> Option<u8> {
    match self {
      Floor::On(n) => Some(n),
      Floor::Every => None,
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn floors_meet_their_own_number_and_every_floor_meets_all() {
    assert_eq!(Floor::default(), Floor::GROUND);
    assert_eq!(Floor::GROUND, Floor::On(0));
    assert!(Floor::On(1).meets(Floor::On(1)));
    assert!(!Floor::On(1).meets(Floor::On(2)));
    assert!(!Floor::GROUND.meets(Floor::On(1)));
    assert!(!Floor::On(255).meets(Floor::GROUND));
    for floor in [Floor::GROUND, Floor::On(1), Floor::On(255), Floor::Every] {
      assert!(
        Floor::Every.meets(floor) && floor.meets(Floor::Every),
        "{floor:?}"
      );
    }
    assert_eq!(Floor::GROUND.number(), Some(0));
    assert_eq!(Floor::On(3).number(), Some(3));
    assert_eq!(Floor::Every.number(), None);
  }
}
