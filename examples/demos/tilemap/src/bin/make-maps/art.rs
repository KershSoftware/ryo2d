//! The tiles make-maps derives from Kenney's sheets, pixel by pixel: the
//! ground sheet (Pixel Platformer's terrain tiles, two diagonal tiles and
//! two slopes with their feet), the dungeon terrain sheet (Tiny Dungeon's
//! walls and floors, the same extras, tiles for walls one cell thick and
//! wall tops open above), and the backdrop (one of Pixel Platformer's
//! backgrounds).

use std::path::Path;

use image::{Rgba, RgbaImage};

const CLEAR: Rgba<u8> = Rgba([0, 0, 0, 0]);
/// Pixel Platformer's dark corner where two outlines meet (tiles 4, 5, 24, 25)
const NOTCH: Rgba<u8> = Rgba([0x34, 0x55, 0x51, 0xff]);
/// Tiny Dungeon's rock
const ROCK: Rgba<u8> = Rgba([0x76, 0x3b, 0x36, 0xff]);
/// Tiny Dungeon's floor, which shows between the stones along a wall's top
const SAND: Rgba<u8> = Rgba([0xea, 0xa5, 0x6c, 0xff]);

/// A sheet of square tiles packed edge to edge
struct Sheet {
  image: RgbaImage,
  tile: u32,
  columns: u32,
}

impl Sheet {
  fn open(path: &Path, tile: u32) -> Result<Sheet, String> {
    let image = image::open(path)
      .map_err(|error| format!("{}: {error}", path.display()))?
      .to_rgba8();
    let columns = image.width() / tile;
    Ok(Sheet {
      image,
      tile,
      columns,
    })
  }

  /// Tile `id`, counted row by row
  fn tile(&self, id: u32) -> RgbaImage {
    let (x, y) = (
      (id % self.columns) * self.tile,
      (id / self.columns) * self.tile,
    );
    image::imageops::crop_imm(&self.image, x, y, self.tile, self.tile).to_image()
  }
}

/// `base` with the pixels of `top` that `keep` accepts laid over it
fn overlay(base: &RgbaImage, top: &RgbaImage, keep: impl Fn(Rgba<u8>) -> bool) -> RgbaImage {
  let mut out = base.clone();
  for (x, y, pixel) in top.enumerate_pixels() {
    if keep(*pixel) {
      out.put_pixel(x, y, *pixel);
    }
  }
  out
}

/// Rows above `split` from `top`, the rest from `bottom`
fn rows(top: &RgbaImage, bottom: &RgbaImage, split: u32) -> RgbaImage {
  RgbaImage::from_fn(top.width(), top.height(), |x, y| {
    if y < split {
      *top.get_pixel(x, y)
    } else {
      *bottom.get_pixel(x, y)
    }
  })
}

/// Columns left of `split` from `left`, the rest from `right`
fn columns(left: &RgbaImage, right: &RgbaImage, split: u32) -> RgbaImage {
  RgbaImage::from_fn(left.width(), left.height(), |x, y| {
    if x < split {
      *left.get_pixel(x, y)
    } else {
      *right.get_pixel(x, y)
    }
  })
}

fn mirror(tile: &RgbaImage) -> RgbaImage {
  image::imageops::flip_horizontal(tile)
}

/// `tile` with Tiny Dungeon's floor taken out from between the stones of a
/// wall's top, for walls seen from the side with open air above them
fn open_air(tile: &RgbaImage) -> RgbaImage {
  let mut out = tile.clone();
  for pixel in out.pixels_mut() {
    if *pixel == SAND {
      *pixel = CLEAR;
    }
  }
  out
}

/// A 45° slope rising to the right, solid below the line from the
/// bottom-left corner to the top-right one. `band(x, d)` colours the pixels
/// `d` steps inside the line (the surface: an outline, then grass or a rim)
/// while it returns one; below the band, the pixels of `fill`
fn slope(fill: &RgbaImage, band: impl Fn(u32, u32) -> Option<Rgba<u8>>) -> RgbaImage {
  let side = fill.width();
  RgbaImage::from_fn(side, side, |x, y| {
    let Some(d) = (x + y).checked_sub(side - 1) else {
      return CLEAR;
    };
    band(x, d).unwrap_or(*fill.get_pixel(x, y))
  })
}

/// The tile under a slope rising to the right: `base` with the slope's band
/// carried on into its top-left corner, so the surface runs on unbroken
/// into the ground the slope stands on
fn foot(base: &RgbaImage, band: impl Fn(u32, u32) -> Option<Rgba<u8>>) -> RgbaImage {
  RgbaImage::from_fn(base.width(), base.height(), |x, y| {
    band(x, x + y + 1).unwrap_or(*base.get_pixel(x, y))
  })
}

/// Lays `tiles` out row by row, `columns` to a row; `None` leaves a cell
/// clear
fn sheet(columns: u32, side: u32, tiles: &[Option<RgbaImage>]) -> RgbaImage {
  let rows = (tiles.len() as u32).div_ceil(columns);
  let mut out = RgbaImage::from_pixel(columns * side, rows * side, CLEAR);
  for (i, tile) in tiles.iter().enumerate() {
    if let Some(tile) = tile {
      let (x, y) = ((i as u32 % columns) * side, (i as u32 / columns) * side);
      image::imageops::replace(&mut out, tile, i64::from(x), i64::from(y));
    }
  }
  out
}

/// Pixel Platformer's ground: the corner Wang set's tiles (every corner
/// combination, the two diagonal ones made from the inner corners), a second
/// fill, and two slopes with the tiles under them. Ids, row by row:
/// 0-4 top-left, top, top-right, inner corners bottom-right and bottom-left;
/// 5-9 left, fill, right, inner corners top-right and top-left; 10-14
/// bottom-left, bottom, bottom-right, the diagonals (top-left and
/// bottom-right ground; top-right and bottom-left ground); 15 the second
/// fill, 16 and 17 the slopes `/` and `\`, 18 and 19 their feet
fn ground(pack: &Sheet) -> RgbaImage {
  let t = |id| pack.tile(id);
  let top = t(22);
  // the grass and outline of the top edge, read down its own column
  let band = |x: u32, d: u32| (d <= 9).then(|| *top.get_pixel(x, d));
  let notch = |p: Rgba<u8>| p == NOTCH;
  let up = slope(&t(122), band);
  let up_foot = foot(&t(25), band);
  let tiles = [
    t(21),
    t(22),
    t(23),
    t(4),
    t(5),
    t(121),
    t(122),
    t(123),
    t(24),
    t(25),
    t(141),
    t(142),
    t(143),
    overlay(&t(24), &t(5), notch),
    overlay(&t(25), &t(4), notch),
    t(104),
    up.clone(),
    mirror(&up),
    up_foot.clone(),
    mirror(&up_foot),
  ];
  sheet(5, 18, &tiles.map(Some))
}

/// Tiny Dungeon's walls and floors: the corner Wang set's tiles (wall and
/// floor at every corner combination, the two diagonal ones made from the
/// inner corners), rock and floor variants, walls one cell thick made from
/// halves of the corner tiles, the steps, and two slopes with the tiles
/// under them. Ids, row by row:
/// 0-7 wall corners and edge (pack 1, 2, 3, 4, 5), rock (0, 12, 24);
/// 8-15 pack 13, floor 48, 15, 16, 17, floor 49, 42, diagonal (top-left and
/// bottom-right wall); 16-23 pack 25, 26, 27, diagonal (top-right and
/// bottom-left wall), a beam across, a beam up, a block alone, a beam's
/// left end; 24-31 its right end, top end, bottom end, the steps (36 to
/// 39), the slope `/`; 32-34 the slope `\` and the slopes' feet; 35-37
/// pack 26, 4 and 5 open above (the floor between their top stones taken
/// out). The beams and ends are made from those, so they too suit walls
/// seen from the side
fn dungeon(pack: &Sheet) -> RgbaImage {
  let t = |id| pack.tile(id);
  let rim = t(26);
  // the rim along a wall's top, row 0 left out (its floor shows there)
  let band = |x: u32, d: u32| (d <= 7).then(|| *rim.get_pixel(x, d + 1));
  let not_rock = |p: Rgba<u8>| p != ROCK;
  let up = slope(&t(0), band);
  let up_foot = foot(&t(0), band);
  // the tiles with a wall's top, open above: for the LDtk level's walls
  let air = |id| open_air(&t(id));
  let corners = |tl: &RgbaImage, tr: &RgbaImage, bl: &RgbaImage, br: &RgbaImage| {
    rows(&columns(tl, tr, 8), &columns(bl, br, 8), 9)
  };
  let tiles = [
    Some(t(1)),
    Some(t(2)),
    Some(t(3)),
    Some(t(4)),
    Some(t(5)),
    Some(t(0)),
    Some(t(12)),
    Some(t(24)),
    Some(t(13)),
    Some(t(48)),
    Some(t(15)),
    Some(t(16)),
    Some(t(17)),
    Some(t(49)),
    Some(t(42)),
    Some(overlay(&overlay(&t(0), &t(25), not_rock), &t(3), not_rock)),
    Some(t(25)),
    Some(t(26)),
    Some(t(27)),
    Some(overlay(&overlay(&t(0), &t(27), not_rock), &t(1), not_rock)),
    Some(rows(&air(26), &t(2), 9)),
    Some(columns(&t(15), &t(13), 8)),
    Some(corners(&air(4), &air(5), &t(16), &t(17))),
    Some(rows(&air(4), &t(16), 9)),
    Some(rows(&air(5), &t(17), 9)),
    Some(columns(&air(4), &air(5), 8)),
    Some(columns(&t(16), &t(17), 8)),
    Some(t(36)),
    Some(t(37)),
    Some(t(38)),
    Some(t(39)),
    Some(up.clone()),
    Some(mirror(&up)),
    Some(up_foot.clone()),
    Some(mirror(&up_foot)),
    Some(air(26)),
    Some(air(4)),
    Some(air(5)),
    None,
    None,
  ];
  sheet(8, 16, &tiles)
}

/// Pixel Platformer's green background: sky, hills and trees, and ground
fn backdrop(backgrounds: &RgbaImage) -> RgbaImage {
  image::imageops::crop_imm(backgrounds, 144, 0, 48, 72).to_image()
}

fn save(image: &RgbaImage, path: &Path) -> Result<(), String> {
  if let Some(folder) = path.parent() {
    std::fs::create_dir_all(folder).map_err(|error| format!("{}: {error}", folder.display()))?;
  }
  image
    .save(path)
    .map_err(|error| format!("{}: {error}", path.display()))
}

/// Writes `images/ground.png`, `images/dungeon-terrain.png` and
/// `images/backdrop.png` under `assets`, from the pack sheets there
pub fn write(assets: &Path) -> Result<Vec<String>, String> {
  let platformer = Sheet::open(
    &assets.join("kenney-pixel-platformer/tilemap_packed.png"),
    18,
  )?;
  let tiny = Sheet::open(&assets.join("kenney-tiny-dungeon/tilemap_packed.png"), 16)?;
  let backgrounds = Sheet::open(
    &assets.join("kenney-pixel-platformer/tilemap-backgrounds_packed.png"),
    24,
  )?;
  let out = [
    ("images/ground.png", ground(&platformer)),
    ("images/dungeon-terrain.png", dungeon(&tiny)),
    ("images/backdrop.png", backdrop(&backgrounds.image)),
  ];
  let mut written = Vec::new();
  for (path, image) in out {
    save(&image, &assets.join(path))?;
    written.push(format!("{path}: {} x {}", image.width(), image.height()));
  }
  Ok(written)
}
