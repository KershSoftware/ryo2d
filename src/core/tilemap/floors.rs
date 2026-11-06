//! Storeys: a map's layers on floors above the ground (raised ground, upper
//! floors, bridges), each floor drawn as high as Tiled draws it.
//!
//! A layer's floor is its `LayerInfo::floor`. A floor's height (its lift)
//! is the vertical part of the offset of its bottom layer, the first in
//! draw order on it; the ground floor's is zero. A layer above the ground
//! floor stands in ground space, at its offset less its floor's height, so
//! its objects, class tiles, colliders and shadows sit where characters
//! stand, and drawing adds the height back.

use glam::Vec2;

use super::LayerInfo;
use crate::core::ecs::{Component, Floor};

/// A map's floors and their heights, on its [`MapRoot`](super::MapRoot)
/// entity when some layer is above the ground floor (a map without one has
/// none). A floor's height is how far up Tiled draws it, in map pixels:
/// the vertical offset of its bottom layer. Read it in the world, through
/// the map's transform, with [`MapsView::lift`](super::MapsView::lift).
///
/// ```
/// use ryo2d::prelude::*;
///
/// /// How far up the map draws its first storey, in its own pixels
/// fn first_storey(ctx: &mut SceneContext<'_>, map: Entity) -> Option<Vec2> {
///   ctx.maps().floors(map)?.lift(Floor::On(1))
/// }
/// ```
#[derive(Component, Clone, Debug, PartialEq)]
pub struct MapFloors {
  /// Each floor's height by its number, `None` for a floor no layer is on;
  /// the ground floor's is zero
  lifts: Vec<Option<Vec2>>,
}

impl MapFloors {
  /// The floors of `layers` (bottom first, as they draw); `None` when none
  /// is above the ground floor
  pub(crate) fn of<'a>(layers: impl IntoIterator<Item = &'a LayerInfo>) -> Option<MapFloors> {
    let mut lifts = vec![Some(Vec2::ZERO)];
    for info in layers {
      let floor = usize::from(info.floor);
      if floor == 0 {
        continue;
      }
      if lifts.len() <= floor {
        lifts.resize(floor + 1, None);
      }
      lifts[floor].get_or_insert(Vec2::new(0.0, info.offset.y));
    }
    (lifts.len() > 1).then_some(MapFloors { lifts })
  }

  /// The floor's height as drawn, in map pixels: `GROUND` gives
  /// `Some(Vec2::ZERO)`; a floor no layer is on, or `Every`, gives `None`
  pub fn lift(&self, floor: Floor) -> Option<Vec2> {
    let number = usize::from(floor.number()?);
    self.lifts.get(number).copied().flatten()
  }

  /// The floors some layer is on, ascending, the ground floor first
  pub fn floors(&self) -> impl Iterator<Item = Floor> + '_ {
    self
      .lifts
      .iter()
      .enumerate()
      .filter(|(_, lift)| lift.is_some())
      .map(|(number, _)| Floor::On(u8::try_from(number).unwrap_or(u8::MAX)))
  }

  /// The highest floor a layer is on
  pub fn highest(&self) -> Floor {
    self.floors().last().unwrap_or(Floor::GROUND)
  }
}

/// Where the layer `info` stands in its map: its offset, less its floor's
/// height above the ground floor (in ground space)
pub(crate) fn ground_offset(info: &LayerInfo, floors: Option<&MapFloors>) -> Vec2 {
  match floors.and_then(|floors| floors.lift(Floor::On(info.floor))) {
    Some(lift) if info.floor > 0 => info.offset - lift,
    _ => info.offset,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn layer(floor: u8, offset: Vec2) -> LayerInfo {
    let mut info = LayerInfo::new(0, "layer");
    info.floor = floor;
    info.offset = offset;
    info
  }

  #[test]
  fn a_floor_s_height_is_its_first_layer_s_vertical_offset() {
    let layers = [
      layer(0, Vec2::new(0.0, -8.0)),
      layer(2, Vec2::new(5.0, -64.0)),
      layer(2, Vec2::new(0.0, -72.0)),
    ];
    let floors = MapFloors::of(&layers).expect("floors above the ground");
    assert_eq!(floors.lift(Floor::GROUND), Some(Vec2::ZERO));
    assert_eq!(floors.lift(Floor::On(1)), None, "no layer is on it");
    assert_eq!(floors.lift(Floor::On(2)), Some(Vec2::new(0.0, -64.0)));
    assert_eq!(floors.lift(Floor::On(3)), None);
    assert_eq!(floors.lift(Floor::Every), None);
    assert_eq!(
      floors.floors().collect::<Vec<_>>(),
      [Floor::GROUND, Floor::On(2)]
    );
    assert_eq!(floors.highest(), Floor::On(2));
    let ground: Vec<Vec2> = layers
      .iter()
      .map(|info| ground_offset(info, Some(&floors)))
      .collect();
    assert_eq!(
      ground,
      [
        Vec2::new(0.0, -8.0),
        Vec2::new(5.0, 0.0),
        Vec2::new(0.0, -8.0)
      ]
    );
    // The top floor a u8 names
    let top = MapFloors::of(&[layer(255, Vec2::new(0.0, -1.0))]).expect("floors");
    assert_eq!(top.highest(), Floor::On(255));
    assert_eq!(top.lift(Floor::On(255)), Some(Vec2::new(0.0, -1.0)));
  }

  #[test]
  fn layers_all_on_the_ground_floor_make_no_floors() {
    let layers = [layer(0, Vec2::ZERO), layer(0, Vec2::new(3.0, -32.0))];
    assert_eq!(MapFloors::of(&layers), None);
    assert_eq!(MapFloors::of(&[]), None);
    assert_eq!(ground_offset(&layers[1], None), Vec2::new(3.0, -32.0));
  }
}
