//! The map: layers of tiles, objects or an image over a list of tilesets.

use std::sync::Arc;

use glam::{IVec2, UVec2, Vec2};

use super::grid::TileGrid;
use super::property::Properties;
use super::tile::Tile;
use super::tileset::Tileset;
use crate::core::asset::{Asset, Handle};
use crate::core::ecs::Component;
use crate::core::graphics::Image;
use crate::core::hash::FastMap;
use crate::core::render::batch::BlendMode;
use crate::core::ui::Color;

/// Where a tileset of a map comes from
#[derive(Clone, Debug)]
pub enum TilesetSource {
  /// A tileset asset, shared with other maps
  Asset(Handle<Tileset>),
  /// A tileset the map owns
  Embedded(Arc<Tileset>),
}

/// Values painted on an integer layer: 0 is empty
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IntGrid {
  pub size: UVec2,
  pub values: Vec<i32>,
  /// What each value is called
  pub names: FastMap<i32, String>,
}

impl IntGrid {
  /// The value at `cell`; `None` if empty or outside
  pub fn get(&self, cell: IVec2) -> Option<i32> {
    if cell.x < 0 || cell.y < 0 || cell.x as u32 >= self.size.x || cell.y as u32 >= self.size.y {
      return None;
    }
    let v = *self
      .values
      .get(cell.y as usize * self.size.x as usize + cell.x as usize)?;
    (v != 0).then_some(v)
  }
}

/// What every kind of layer has. A spawned map's layer entities carry it
/// as a component (their `Transform` carries the offset)
#[derive(Component, Clone, Debug, PartialEq)]
pub struct LayerInfo {
  pub id: u32,
  pub name: String,
  pub class: Option<String>,
  /// Shift in pixels
  pub offset: Vec2,
  pub opacity: f32,
  pub tint: Color,
  /// How much of the camera's motion the layer follows; one is fixed to the
  /// world
  pub parallax: Vec2,
  pub visible: bool,
  pub blend: BlendMode,
  /// Whether its tiles' shapes collide
  pub collides: bool,
  /// Whether its tiles' shapes block light; `None` means as `collides`
  pub occludes: Option<bool>,
  /// Whether its tiles sort among the entities by their y
  pub y_sorted: bool,
  /// Whether lights shade it
  pub lit: bool,
  pub properties: Properties,
}

impl LayerInfo {
  /// A visible, opaque, lit layer
  pub fn new(id: u32, name: impl Into<String>) -> LayerInfo {
    LayerInfo {
      id,
      name: name.into(),
      class: None,
      offset: Vec2::ZERO,
      opacity: 1.0,
      tint: Color::WHITE,
      parallax: Vec2::ONE,
      visible: true,
      blend: BlendMode::default(),
      collides: true,
      occludes: None,
      y_sorted: false,
      lit: true,
      properties: Properties::default(),
    }
  }
}

/// The shape of a map object
#[derive(Clone, Debug, PartialEq)]
pub enum ObjectShape {
  Rect,
  Ellipse,
  Point,
  Capsule,
  Polygon(Vec<Vec2>),
  Polyline(Vec<Vec2>),
  Text(String),
}

/// An object placed on an object layer
#[derive(Clone, Debug, PartialEq)]
pub struct MapObjectData {
  /// Unique in the map, as the file wrote it
  pub id: String,
  pub name: String,
  pub class: Option<String>,
  /// The top-left corner of the object's rectangle in map pixels (the origin
  /// of the points, for a polygon or polyline). A tile object loaded from
  /// Tiled, whose own position is its alignment point, is moved here
  pub position: Vec2,
  pub size: Vec2,
  /// Radians, positive clockwise on screen (y down), turning the object about
  /// `position`
  pub rotation: f32,
  pub shape: ObjectShape,
  /// The tile drawn, for a tile object
  pub tile: Option<Tile>,
  pub visible: bool,
  pub properties: Properties,
}

/// A picture laid across the map. A spawned map's image layer entities
/// carry it as a component
#[derive(Component, Clone, Debug, PartialEq)]
pub struct ImageLayer {
  /// The picture; `None` for a layer given none (Tiled makes a new image
  /// layer so), which draws nothing
  pub image: Option<Handle<Image>>,
  pub repeat_x: bool,
  pub repeat_y: bool,
  /// The colour the picture draws clear where it holds it (Tiled's
  /// transparent colour; its alpha is not compared)
  pub transparent: Option<Color>,
}

/// What a layer holds
#[derive(Clone, Debug)]
pub enum LayerKind {
  Tiles {
    grid: TileGrid,
    intgrid: Option<IntGrid>,
    /// A cell's size in pixels on this layer: the map's `tile_size` unless
    /// the file gave the layer a grid of its own (LDtk does). The grid and
    /// the int grid then count cells of this size
    cell_size: UVec2,
  },
  Objects(Vec<MapObjectData>),
  Image(ImageLayer),
}

/// A layer: what every layer has, and what it holds
#[derive(Clone, Debug)]
pub struct MapLayer {
  pub info: LayerInfo,
  pub kind: LayerKind,
}

/// A map of layers over tilesets, from a file or built in code
#[derive(Clone, Debug)]
pub struct TileMap {
  /// Width and height in cells
  pub size: UVec2,
  /// A cell's size in pixels
  pub tile_size: UVec2,
  /// The tilesets; a [`Tile`]'s `tileset` indexes this
  pub tilesets: Vec<TilesetSource>,
  /// Bottom first
  pub layers: Vec<MapLayer>,
  pub class: Option<String>,
  pub background: Option<Color>,
  /// Where parallax is measured from, in map pixels: a camera centred here
  /// shows every layer where it would be without parallax (Tiled's Parallax
  /// Origin; the map's top-left corner, zero, unless the file moves it)
  pub parallax_origin: Vec2,
  pub properties: Properties,
  /// For each tile layer, the highest tile id it used of each tileset, as
  /// loaded: the loader's check measures them against the tilesets once those
  /// are in. A layer index, a tileset index and an id
  pub(crate) used_ids: Vec<(usize, u16, u32)>,
  /// Where the file names each of its dependencies
  pub(crate) dependency_places: super::tiled::build::DependencyPlaces,
}

impl Asset for TileMap {}

impl TileMap {
  /// An empty map of `size` cells of `tile_size` pixels
  pub fn new(size: UVec2, tile_size: UVec2) -> TileMap {
    TileMap {
      size,
      tile_size,
      tilesets: Vec::new(),
      layers: Vec::new(),
      class: None,
      background: None,
      parallax_origin: Vec2::ZERO,
      properties: Properties::default(),
      used_ids: Vec::new(),
      dependency_places: Default::default(),
    }
  }

  /// Adds a tileset and returns the index tiles name it by. A map holds at
  /// most 256
  pub fn add_tileset(&mut self, source: TilesetSource) -> u16 {
    self.tilesets.push(source);
    (self.tilesets.len() - 1) as u16
  }

  fn next_layer_id(&self) -> u32 {
    self
      .layers
      .iter()
      .map(|l| l.info.id.saturating_add(1))
      .max()
      .unwrap_or(1)
  }

  /// Adds an empty tile layer on top and returns its index
  pub fn add_tile_layer(&mut self, name: &str) -> usize {
    let info = LayerInfo::new(self.next_layer_id(), name);
    self.layers.push(MapLayer {
      info,
      kind: LayerKind::Tiles {
        grid: TileGrid::new(self.size),
        intgrid: None,
        cell_size: self.tile_size,
      },
    });
    self.layers.len() - 1
  }

  /// Adds an empty object layer on top and returns its index
  pub fn add_object_layer(&mut self, name: &str) -> usize {
    let mut info = LayerInfo::new(self.next_layer_id(), name);
    info.collides = false;
    self.layers.push(MapLayer {
      info,
      kind: LayerKind::Objects(Vec::new()),
    });
    self.layers.len() - 1
  }

  /// The index of the first layer called `name`
  pub fn layer_index(&self, name: &str) -> Option<usize> {
    self.layers.iter().position(|l| l.info.name == name)
  }

  /// The tiles of layer `layer`, if it is a tile layer
  pub fn tiles(&self, layer: usize) -> Option<&TileGrid> {
    match &self.layers.get(layer)?.kind {
      LayerKind::Tiles { grid, .. } => Some(grid),
      _ => None,
    }
  }

  /// The tiles of layer `layer`, editable
  pub fn tiles_mut(&mut self, layer: usize) -> Option<&mut TileGrid> {
    match &mut self.layers.get_mut(layer)?.kind {
      LayerKind::Tiles { grid, .. } => Some(grid),
      _ => None,
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::tilemap::Tile;
  use glam::{IVec2, UVec2};

  #[test]
  fn a_map_built_in_code_holds_its_layers() {
    let mut map = TileMap::new(UVec2::new(8, 6), UVec2::splat(16));
    let ground = map.add_tile_layer("ground");
    let things = map.add_object_layer("things");
    assert_eq!((ground, things), (0, 1));
    assert_eq!(map.layer_index("things"), Some(1));
    assert_eq!(map.layer_index("nope"), None);
    assert!(map.tiles(things).is_none());
    assert!(map.tiles(9).is_none());
    let grid = map.tiles_mut(ground).expect("tile layer");
    assert_eq!(grid.size(), UVec2::new(8, 6));
    grid.set(IVec2::new(1, 1), Some(Tile::new(0, 4)));
    assert_eq!(
      map.tiles(ground).and_then(|g| g.get(IVec2::new(1, 1))),
      Some(Tile::new(0, 4))
    );
    assert!(matches!(
      map.layers[ground].kind,
      LayerKind::Tiles { cell_size, .. } if cell_size == UVec2::splat(16)
    ));
    let info = &map.layers[ground].info;
    assert!(info.collides && info.lit && info.visible && !info.y_sorted);
    assert_eq!(info.occludes, None);
    assert_eq!(info.opacity, 1.0);
    assert!(!map.layers[things].info.collides);
    assert_ne!(map.layers[0].info.id, map.layers[1].info.id);
  }

  #[test]
  fn tilesets_get_consecutive_indices() {
    use crate::core::tilemap::{Tileset, TilesetImage};
    use std::sync::Arc;
    let mut map = TileMap::new(UVec2::ONE, UVec2::splat(8));
    let ts = || Arc::new(Tileset::new("t", UVec2::splat(8), TilesetImage::Collection));
    assert_eq!(map.add_tileset(TilesetSource::Embedded(ts())), 0);
    assert_eq!(map.add_tileset(TilesetSource::Embedded(ts())), 1);
  }

  #[test]
  fn an_int_grid_treats_zero_as_empty() {
    let g = IntGrid {
      size: UVec2::new(2, 2),
      values: vec![0, 3, 0, 0],
      names: Default::default(),
    };
    assert_eq!(g.get(IVec2::new(1, 0)), Some(3));
    assert_eq!(g.get(IVec2::new(0, 0)), None);
    assert_eq!(g.get(IVec2::new(2, 0)), None);
    assert_eq!(g.get(IVec2::new(0, -1)), None);
  }
}
