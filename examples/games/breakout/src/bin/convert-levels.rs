//! Converts Breakout's text levels into the Tiled files the game loads. Kept
//! for reference: the `.txt` files it read are gone from the tree (they are in
//! git history at `7af8ba84:examples/games/breakout/levels/`).
//!
//! ```text
//! convert-levels <folder of level1.txt..level10.txt> <assets folder>
//! ```
//!
//! writes `levels/level1.tmx`..`level10.tmx`, `levels/bricks.tsx`,
//! `levels/bricks.png` and `project.tiled-project` under the assets folder,
//! and prints each level's bricks as the table `tests/levels.rs` holds.
//!
//! A brick is a tile of the class `brick` in the tileset `bricks` (one tile
//! a colour), with the properties `color` (the enum `BlockColor`), `hits` and
//! `points`. The map's cells are the game's 84 x 29 brick grid, so with the
//! map's corner at (118, 58) a cell's centre is a brick's centre. Seven
//! cells of the text files held a Cyrillic letter (`О` and `Р`) where a
//! Latin one was meant, and so no brick; they convert to the bricks they
//! were meant to be.

use std::fmt::Write as _;
use std::path::Path;

use breakout::components::BlockColor;
use breakout::constants::*;
use image::{Rgba, RgbaImage};

/// Letter, colour, hits and points: what the text format said
const LETTERS: [(char, BlockColor, u32, u32); 8] = [
  ('R', BlockColor::Red, 1, 10),
  ('O', BlockColor::Orange, 1, 20),
  ('Y', BlockColor::Yellow, 1, 30),
  ('G', BlockColor::Green, 1, 40),
  ('B', BlockColor::Blue, 1, 50),
  ('P', BlockColor::Purple, 1, 60),
  ('W', BlockColor::White, 2, 100),
  ('S', BlockColor::Silver, 3, 200),
];

/// The Latin letters their Cyrillic look-alikes stood for
const LOOK_ALIKES: [(char, char); 2] = [('\u{41e}', 'O'), ('\u{420}', 'P')];

const TILED_VERSION: &str = "1.12.2";
const CELL_W: u32 = (BLOCK_WIDTH + BLOCK_PADDING) as u32;
const CELL_H: u32 = (BLOCK_HEIGHT + BLOCK_PADDING) as u32;

fn main() {
  let args: Vec<String> = std::env::args().collect();
  let [_, from, to] = &args[..] else {
    eprintln!("usage: convert-levels <folder of level*.txt> <assets folder>");
    std::process::exit(2);
  };
  if let Err(error) = run(Path::new(from), Path::new(to)) {
    eprintln!("convert-levels: {error}");
    std::process::exit(1);
  }
}

fn run(from: &Path, to: &Path) -> Result<(), String> {
  let levels_dir = to.join("levels");
  std::fs::create_dir_all(&levels_dir).map_err(|e| format!("{}: {e}", levels_dir.display()))?;
  write(&levels_dir.join("bricks.png"), &bricks_png())?;
  write(&levels_dir.join("bricks.tsx"), bricks_tsx().as_bytes())?;
  write(&to.join("project.tiled-project"), project().as_bytes())?;

  println!("const LEVELS: [&[&str]; {}] = [", MAX_LEVEL);
  for level in 1..=MAX_LEVEL {
    let path = from.join(format!("level{level}.txt"));
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let rows = rows(level, &text);
    write(
      &levels_dir.join(format!("level{level}.tmx")),
      map(&rows)?.as_bytes(),
    )?;
    let bricks: usize = rows
      .iter()
      .map(|row| row.chars().filter(|&c| c != '.').count())
      .sum();
    println!("  // level {level}: {bricks} bricks");
    println!("  &[");
    for row in &rows {
      println!("    \"{row}\",");
    }
    println!("  ],");
  }
  println!("];");
  Ok(())
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
  std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// A level's rows as the text format read them: comment and blank lines
/// leave, a line is trimmed, and a letter that is not a block's stays empty
/// (but for the Cyrillic look-alikes, which are put right and reported)
fn rows(level: u32, text: &str) -> Vec<String> {
  let mut rows = Vec::new();
  for (line_no, line) in text.lines().enumerate() {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
      continue;
    }
    let row: String = line
      .chars()
      .enumerate()
      .map(|(col, c)| {
        if let Some(&(_, latin)) = LOOK_ALIKES.iter().find(|(cyrillic, _)| *cyrillic == c) {
          eprintln!(
            "level {level} line {} column {} (from 0): {c} -> {latin}",
            line_no + 1,
            col
          );
          latin
        } else if c == '.' || LETTERS.iter().any(|(l, ..)| *l == c) {
          c
        } else {
          panic!("level {level} line {}: unknown {c:?}", line_no + 1)
        }
      })
      .collect();
    rows.push(row);
  }
  rows
}

fn tile_id(letter: char) -> Option<usize> {
  LETTERS.iter().position(|(l, ..)| *l == letter)
}

/// The map: one tile layer of the bricks, which neither draws (the game spawns
/// the class tiles as entities) nor collides
fn map(rows: &[String]) -> Result<String, String> {
  let width = rows.iter().map(|r| r.chars().count()).max().unwrap_or(0);
  let height = rows.len();
  let mut out = String::new();
  let _ = writeln!(out, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
  let _ = writeln!(
    out,
    "<map version=\"1.11\" tiledversion=\"{TILED_VERSION}\" orientation=\"orthogonal\" renderorder=\"right-down\" width=\"{width}\" height=\"{height}\" tilewidth=\"{CELL_W}\" tileheight=\"{CELL_H}\" infinite=\"0\" nextlayerid=\"2\" nextobjectid=\"1\">"
  );
  let _ = writeln!(out, " <tileset firstgid=\"1\" source=\"bricks.tsx\"/>");
  let _ = writeln!(
    out,
    " <layer id=\"1\" name=\"bricks\" width=\"{width}\" height=\"{height}\">"
  );
  let _ = writeln!(out, "  <properties>");
  let _ = writeln!(
    out,
    "   <property name=\"collides\" type=\"bool\" value=\"false\"/>"
  );
  let _ = writeln!(out, "  </properties>");
  let _ = writeln!(out, "  <data encoding=\"csv\">");
  for (y, row) in rows.iter().enumerate() {
    let mut cells = Vec::new();
    for x in 0..width {
      let gid = row.chars().nth(x).and_then(tile_id).map_or(0, |id| id + 1);
      cells.push(gid.to_string());
    }
    let comma = if y + 1 < height { "," } else { "" };
    let _ = writeln!(out, "{}{comma}", cells.join(","));
  }
  let _ = writeln!(out, "</data>");
  let _ = writeln!(out, " </layer>");
  let _ = writeln!(out, "</map>");
  Ok(out)
}

fn bricks_tsx() -> String {
  let columns = LETTERS.len();
  let mut out = String::new();
  let _ = writeln!(out, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
  let _ = writeln!(
    out,
    "<tileset version=\"1.11\" tiledversion=\"{TILED_VERSION}\" name=\"bricks\" tilewidth=\"{CELL_W}\" tileheight=\"{CELL_H}\" tilecount=\"{columns}\" columns=\"{columns}\">"
  );
  let _ = writeln!(
    out,
    " <image source=\"bricks.png\" width=\"{}\" height=\"{CELL_H}\"/>",
    columns as u32 * CELL_W
  );
  for (id, (_, color, hits, points)) in LETTERS.iter().enumerate() {
    let _ = writeln!(out, " <tile id=\"{id}\" type=\"brick\">");
    let _ = writeln!(out, "  <properties>");
    let _ = writeln!(
      out,
      "   <property name=\"color\" propertytype=\"BlockColor\" value=\"{color:?}\"/>"
    );
    let _ = writeln!(
      out,
      "   <property name=\"hits\" type=\"int\" value=\"{hits}\"/>"
    );
    let _ = writeln!(
      out,
      "   <property name=\"points\" type=\"int\" value=\"{points}\"/>"
    );
    let _ = writeln!(out, "  </properties>");
    let _ = writeln!(out, " </tile>");
  }
  let _ = writeln!(out, "</tileset>");
  out
}

/// The Tiled project: the enum `BlockColor` and the tile class `brick`
fn project() -> String {
  let names: Vec<String> = LETTERS
    .iter()
    .map(|(_, color, ..)| format!("                \"{color:?}\""))
    .collect();
  format!(
    r##"{{
    "automappingRulesFile": "",
    "commands": [
    ],
    "extensionsPath": "extensions",
    "folders": [
        "."
    ],
    "properties": [
    ],
    "propertyTypes": [
        {{
            "id": 1,
            "name": "BlockColor",
            "storageType": "string",
            "type": "enum",
            "values": [
{}
            ],
            "valuesAsFlags": false
        }},
        {{
            "color": "#ffa0a0a4",
            "drawFill": true,
            "id": 2,
            "members": [
                {{
                    "name": "color",
                    "propertyType": "BlockColor",
                    "type": "string",
                    "value": "Red"
                }},
                {{
                    "name": "hits",
                    "type": "int",
                    "value": 1
                }},
                {{
                    "name": "points",
                    "type": "int",
                    "value": 10
                }}
            ],
            "name": "brick",
            "type": "class",
            "useAs": [
                "tile"
            ]
        }}
    ]
}}
"##,
    names.join(",\n")
  )
}

/// The tileset's image: each colour's bordered rectangle, as the game draws
/// a brick, in the middle of its 84 x 29 cell
fn bricks_png() -> Vec<u8> {
  let mut image = RgbaImage::new(LETTERS.len() as u32 * CELL_W, CELL_H);
  let (w, h) = (BLOCK_WIDTH as u32, BLOCK_HEIGHT as u32);
  let (pad_x, pad_y) = ((CELL_W - w) / 2, (CELL_H - h) / 2);
  for (id, (_, color, ..)) in LETTERS.iter().enumerate() {
    let (r, g, b, a) = color.to_rgba();
    let border = Rgba([r / 3, g / 3, b / 3, 255]);
    for y in 0..h {
      for x in 0..w {
        let edge = x == 0 || x == w - 1 || y == 0 || y == h - 1;
        let pixel = if edge { border } else { Rgba([r, g, b, a]) };
        image.put_pixel(id as u32 * CELL_W + pad_x + x, pad_y + y, pixel);
      }
    }
  }
  let mut bytes = Vec::new();
  image
    .write_to(
      &mut std::io::Cursor::new(&mut bytes),
      image::ImageFormat::Png,
    )
    .expect("writing a PNG to memory");
  bytes
}
