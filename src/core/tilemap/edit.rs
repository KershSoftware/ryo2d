//! Finding a spawned map's layers, and reading and editing their cells.

use glam::{Affine2, IVec2, Vec2};

use super::spawn::{MapLayerRef, MapLayers, TileLayerData};
use super::{Tile, TileGrid};
use crate::core::ecs::{ChildOf, DetectChangesMut, Entity, Transform, World};

/// How many ancestors a layer's world transform walks up before giving up:
/// far past any real hierarchy, and a cycle (which nothing else checks for)
/// ends rather than looping
const MAX_DEPTH: usize = 256;

/// A layer of a map: by name (the first layer so called) or by index
/// (bottom first)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerRef<'a> {
  Name(&'a str),
  Index(usize),
}

impl<'a> From<&'a str> for LayerRef<'a> {
  fn from(name: &'a str) -> Self {
    LayerRef::Name(name)
  }
}

impl<'a> From<&'a String> for LayerRef<'a> {
  fn from(name: &'a String) -> Self {
    LayerRef::Name(name)
  }
}

impl From<usize> for LayerRef<'_> {
  fn from(index: usize) -> Self {
    LayerRef::Index(index)
  }
}

/// The entity of `map`'s layer `layer`, once the map has spawned (and while
/// the layer is still there)
pub(crate) fn layer_entity(world: &World, map: Entity, layer: LayerRef<'_>) -> Option<Entity> {
  let layers = &world.get::<MapLayers>(map)?.0;
  let is_layer = |entity: Entity, name: Option<&str>| {
    world
      .get::<MapLayerRef>(entity)
      .is_some_and(|l| l.map == map && name.is_none_or(|name| l.name == name))
  };
  match layer {
    LayerRef::Index(index) => layers
      .get(index)
      .copied()
      .filter(|&entity| is_layer(entity, None)),
    LayerRef::Name(name) => layers
      .iter()
      .copied()
      .find(|&entity| is_layer(entity, Some(name))),
  }
}

/// A tile layer's cells
fn tile_layer<'w>(world: &'w World, map: Entity, layer: LayerRef<'_>) -> Option<&'w TileLayerData> {
  world.get::<TileLayerData>(layer_entity(world, map, layer)?)
}

/// Runs `edit` on a tile layer's grid; the layer is marked changed only
/// when `changed` says the edit changed something
pub(crate) fn edit_tiles<R>(
  world: &mut World,
  map: Entity,
  layer: LayerRef<'_>,
  edit: impl FnOnce(&mut TileGrid) -> R,
  changed: impl FnOnce(&R) -> bool,
) -> Option<R> {
  let entity = layer_entity(world, map, layer)?;
  let mut data = world.get_mut::<TileLayerData>(entity)?;
  let result = edit(&mut data.bypass_change_detection().grid);
  if changed(&result) {
    data.set_changed();
  }
  Some(result)
}

/// The tile at `cell` of a tile layer
pub(crate) fn tile_at(
  world: &World,
  map: Entity,
  layer: LayerRef<'_>,
  cell: IVec2,
) -> Option<Tile> {
  tile_layer(world, map, layer)?.grid.get(cell)
}

/// The IntGrid value at `cell` of a tile layer
pub(crate) fn intgrid_at(
  world: &World,
  map: Entity,
  layer: LayerRef<'_>,
  cell: IVec2,
) -> Option<i32> {
  tile_layer(world, map, layer)?.intgrid.as_ref()?.get(cell)
}

/// Where an entity's local space lies in the world, from its `Transform` and
/// its ancestors' as they are now: an edit made this frame counts before
/// propagation runs. A parent with no `Transform` starts its children from
/// the identity, as propagation does
fn local_to_world(world: &World, entity: Entity) -> Option<Affine2> {
  let mut affine = world.get::<Transform>(entity)?.to_affine();
  let mut child = entity;
  for _ in 0..MAX_DEPTH {
    let Some(parent) = world.get::<ChildOf>(child).map(ChildOf::parent) else {
      return Some(affine);
    };
    let Some(transform) = world.get::<Transform>(parent) else {
      return Some(affine);
    };
    affine = transform.to_affine() * affine;
    child = parent;
  }
  None
}

/// A tile layer's cell size and where its space lies in the world
fn layer_space(world: &World, map: Entity, layer: LayerRef<'_>) -> Option<(Vec2, Affine2)> {
  let entity = layer_entity(world, map, layer)?;
  let cell = world.get::<TileLayerData>(entity)?.cell_size.as_vec2();
  Some((cell, local_to_world(world, entity)?))
}

/// The cell of a tile layer under the world position `point`, inside the
/// layer or not; `None` where the layer is scaled to nothing
pub(crate) fn world_to_cell(
  world: &World,
  map: Entity,
  layer: LayerRef<'_>,
  point: Vec2,
) -> Option<IVec2> {
  let (cell, affine) = layer_space(world, map, layer)?;
  if affine.matrix2.determinant() == 0.0 {
    return None;
  }
  let local = affine.inverse().transform_point2(point) / cell;
  local.is_finite().then(|| local.floor().as_ivec2())
}

/// The world position of the centre of a tile layer's `cell`
pub(crate) fn cell_to_world(
  world: &World,
  map: Entity,
  layer: LayerRef<'_>,
  cell: IVec2,
) -> Option<Vec2> {
  let (size, affine) = layer_space(world, map, layer)?;
  Some(affine.transform_point2((cell.as_vec2() + 0.5) * size))
}
