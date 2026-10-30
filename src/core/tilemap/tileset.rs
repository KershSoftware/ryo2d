//! Tilesets: the art a map's tiles name, and what each tile carries.

use std::time::Duration;

use glam::{IVec2, UVec2, Vec2};

use super::property::Properties;
use crate::core::asset::{Asset, Handle};
use crate::core::graphics::Image;
use crate::core::hash::FastMap;
use crate::core::ui::Color;

/// A rectangle of texels in an image: `min` inclusive, `max` exclusive
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct URect {
  pub min: UVec2,
  pub max: UVec2,
}

impl URect {
  /// The rectangle from `(x0, y0)` to `(x1, y1)`
  pub const fn new(x0: u32, y0: u32, x1: u32, y1: u32) -> URect {
    URect {
      min: UVec2::new(x0, y0),
      max: UVec2::new(x1, y1),
    }
  }

  /// Width and height
  pub fn size(&self) -> UVec2 {
    self.max - self.min
  }
}

/// Where a tileset's pictures come from
#[derive(Clone, Debug)]
pub enum TilesetImage {
  /// One image holding every tile in a grid; `size` is its size in texels
  Atlas { image: Handle<Image>, size: UVec2 },
  /// Each tile has its own image, in [`TileData::image`]
  Collection,
}

/// One frame of a tile's animation
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct TileFrame {
  /// The tile shown
  pub tile: u32,
  /// For how long
  pub duration: Duration,
}

/// A closed polygon on a tile, in tile pixels with the origin at the top
/// left and y down. Rectangles are four points and ellipses sixteen.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TileShape {
  pub points: Vec<Vec2>,
  /// Solid only from above
  pub one_way: bool,
  /// Blocks light
  pub occluder: bool,
  pub properties: Properties,
}

/// What one tile carries beyond its picture
#[derive(Clone, Debug)]
pub struct TileData {
  pub class: Option<String>,
  pub properties: Properties,
  /// Collision shapes
  pub shapes: Vec<TileShape>,
  /// Frames, empty if the tile is still
  pub animation: Vec<TileFrame>,
  /// Weight when a brush picks among equals
  pub probability: f32,
  /// The tile's own image and its size, for collection tilesets
  pub image: Option<(Handle<Image>, UVec2)>,
}

impl Default for TileData {
  fn default() -> Self {
    TileData {
      class: None,
      properties: Properties::default(),
      shapes: Vec::new(),
      animation: Vec::new(),
      probability: 1.0,
      image: None,
    }
  }
}

/// Which parts of a tile a Wang color touches
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum WangKind {
  Corner,
  Edge,
  Mixed,
}

/// A color of a Wang set
#[derive(Clone, Debug)]
pub struct WangColor {
  pub name: String,
  pub color: Color,
  /// The tile that stands for the color
  pub tile: Option<u32>,
  pub probability: f32,
  pub properties: Properties,
}

/// A set of tiles joined by edge or corner colors, for auto-tiling
#[derive(Clone, Debug)]
pub struct WangSet {
  pub name: String,
  pub kind: WangKind,
  pub colors: Vec<WangColor>,
  /// Each tile with the color index at its eight positions (0 for none),
  /// clockwise from the top edge
  pub tiles: Vec<(u32, [u8; 8])>,
}

/// A set of tiles: an atlas or a collection of images, with the data tiles
/// carry
#[derive(Clone, Debug)]
pub struct Tileset {
  pub name: String,
  pub class: Option<String>,
  pub tile_size: UVec2,
  pub columns: u32,
  pub tile_count: u32,
  pub spacing: u32,
  pub margin: u32,
  /// Where Tiled draws each tile from its cell, in pixels
  pub tile_offset: IVec2,
  pub image: TilesetImage,
  pub wang_sets: Vec<WangSet>,
  pub properties: Properties,
  tiles: FastMap<u32, TileData>,
}

impl Asset for Tileset {}

impl Tileset {
  /// An empty tileset of `tile_size` tiles; set `columns`, `tile_count`,
  /// `spacing` and `margin` to describe an atlas
  pub fn new(name: impl Into<String>, tile_size: UVec2, image: TilesetImage) -> Tileset {
    Tileset {
      name: name.into(),
      class: None,
      tile_size,
      columns: 0,
      tile_count: 0,
      spacing: 0,
      margin: 0,
      tile_offset: IVec2::ZERO,
      image,
      wang_sets: Vec::new(),
      properties: Properties::default(),
      tiles: FastMap::default(),
    }
  }

  /// The data of tile `id`, if it carries any
  pub fn tile(&self, id: u32) -> Option<&TileData> {
    self.tiles.get(&id)
  }

  /// Sets the data of tile `id`
  pub fn insert_tile(&mut self, id: u32, data: TileData) {
    self.tiles.insert(id, data);
  }

  /// Where tile `id` sits in the atlas image, in texels; `None` past
  /// `tile_count`, with no columns, or where the arithmetic overflows (a file
  /// can claim any numbers)
  pub fn tile_rect(&self, id: u32) -> Option<URect> {
    if self.columns == 0 || id >= self.tile_count {
      return None;
    }
    let col = id % self.columns;
    let row = id / self.columns;
    let x = col
      .checked_mul(self.tile_size.x.checked_add(self.spacing)?)?
      .checked_add(self.margin)?;
    let y = row
      .checked_mul(self.tile_size.y.checked_add(self.spacing)?)?
      .checked_add(self.margin)?;
    Some(URect::new(
      x,
      y,
      x.checked_add(self.tile_size.x)?,
      y.checked_add(self.tile_size.y)?,
    ))
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use glam::UVec2;

  fn sheet() -> Tileset {
    let mut t = Tileset::new("t", UVec2::splat(16), TilesetImage::Collection);
    t.columns = 4;
    t.tile_count = 16;
    t.margin = 1;
    t.spacing = 2;
    t
  }

  #[test]
  fn tile_rect_honours_margin_and_spacing() {
    let t = sheet();
    assert_eq!(t.tile_rect(5), Some(URect::new(19, 19, 35, 35)));
    assert_eq!(t.tile_rect(0), Some(URect::new(1, 1, 17, 17)));
    assert_eq!(t.tile_rect(16), None);
    let mut none = sheet();
    none.columns = 0;
    assert_eq!(none.tile_rect(0), None);
  }

  #[test]
  fn tile_rect_is_none_where_the_arithmetic_overflows() {
    let mut t = sheet();
    t.tile_size = UVec2::splat(u32::MAX / 2);
    t.tile_count = u32::MAX;
    t.columns = 4;
    assert_eq!(t.tile_rect(0).map(|r| r.min), Some(UVec2::splat(1)));
    assert_eq!(t.tile_rect(5), None);
    t.spacing = u32::MAX;
    assert_eq!(t.tile_rect(0), None);
  }

  #[test]
  fn tile_data_is_looked_up_by_id() {
    let mut t = sheet();
    assert!(t.tile(3).is_none());
    t.insert_tile(3, TileData::default());
    assert_eq!(t.tile(3).map(|d| d.probability), Some(1.0));
  }
}
