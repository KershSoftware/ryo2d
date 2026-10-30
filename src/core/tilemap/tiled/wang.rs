//! Wang sets: the colors and tiles of a tileset's auto-tiling.

use super::build::MAX_TILES;
use super::project::{Props, parse_color};
use super::raw::RawTransformations;
use super::raw::RawWangSet;
use crate::core::asset::AssetError;
use crate::core::tilemap::{TileTransformations, WangColor, WangKind, WangSet, WangTile};
use crate::core::ui::Color;

/// The tileset's transformations from what its file says
pub(super) fn transformations(raw: &RawTransformations) -> TileTransformations {
  TileTransformations {
    hflip: raw.hflip,
    vflip: raw.vflip,
    rotate: raw.rotate,
    prefer_untransformed: raw.preferuntransformed,
  }
}

/// The Wang sets of the tileset `place` names; `probability` is each tile's
/// own, which the set carries
pub(super) fn build_wang_sets(
  px: &mut Props<'_>,
  place: &str,
  raw: Vec<RawWangSet>,
  transformations: TileTransformations,
  probability: impl Fn(u32) -> f32,
) -> Result<Vec<WangSet>, AssetError> {
  let mut sets = Vec::with_capacity(raw.len());
  for set in raw {
    let place = format!("{place}, wang set {:?}", set.name);
    let kind = match set.ty.as_str() {
      "corner" => WangKind::Corner,
      "edge" => WangKind::Edge,
      "mixed" => WangKind::Mixed,
      other => {
        return Err(px.fail(
          &place,
          format!("the type {other:?} is not corner, edge or mixed"),
        ));
      }
    };
    // Tiled holds 254 colors; index 0 is no color
    if set.colors.len() > 254 {
      return Err(px.fail(
        &place,
        format!("{} colors is past the 254 a set holds", set.colors.len()),
      ));
    }
    let mut colors = Vec::with_capacity(set.colors.len());
    for color in set.colors {
      let place = format!("{place}, color {:?}", color.name);
      let tile = match color.tile {
        None => None,
        Some(t) if t < 0 => None,
        Some(t) => Some(
          u32::try_from(t)
            .ok()
            .filter(|&t| t < MAX_TILES)
            .ok_or_else(|| {
              px.fail(
                &place,
                format!("the tile {t} is past the {MAX_TILES} a tileset holds"),
              )
            })?,
        ),
      };
      let rgb = if color.color.is_empty() {
        Color::WHITE
      } else {
        parse_color(&color.color)
          .ok_or_else(|| px.fail(&place, format!("{:?} is not a color", color.color)))?
      };
      colors.push(WangColor {
        properties: px.read(&place, &color.properties, color.class.as_deref())?,
        name: color.name,
        color: rgb,
        tile,
        probability: color.probability.unwrap_or(1.0),
      });
    }
    let mut tiles = Vec::with_capacity(set.wangtiles.len());
    for tile in set.wangtiles {
      let place = format!("{place}, tile {}", tile.tileid);
      if tile.tileid >= MAX_TILES {
        return Err(px.fail(
          &place,
          format!("the id is past the {MAX_TILES} tiles a tileset holds"),
        ));
      }
      let Ok::<[u32; 8], _>(ids) = tile.wangid.clone().try_into() else {
        return Err(px.fail(
          &place,
          format!("the wangid has {} entries, not 8", tile.wangid.len()),
        ));
      };
      let mut wangid = [0u8; 8];
      for (slot, id) in wangid.iter_mut().zip(ids) {
        *slot = u8::try_from(id)
          .ok()
          .filter(|&i| usize::from(i) <= colors.len())
          .ok_or_else(|| {
            px.fail(
              &place,
              format!("the color {id} is not one of the set's {}", colors.len()),
            )
          })?;
      }
      tiles.push(WangTile {
        tile: tile.tileid,
        wangid,
        probability: probability(tile.tileid),
      });
    }
    sets.push(WangSet::new(set.name, kind, colors, tiles, transformations));
  }
  Ok(sets)
}
