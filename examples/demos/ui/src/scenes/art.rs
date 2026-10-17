//! The pages' pixel art, drawn in code: the item icons and the shopkeeper

use demo_kit::prelude::*;

/// A small RGBA image drawn pixel by pixel
pub struct Canvas {
  width: u32,
  height: u32,
  pixels: Vec<u8>,
}

impl Canvas {
  /// A clear canvas
  pub fn new(width: u32, height: u32) -> Self {
    Self {
      width,
      height,
      pixels: vec![0; (width * height * 4) as usize],
    }
  }

  /// One pixel; outside the canvas, nothing
  pub fn set(&mut self, x: i32, y: i32, rgba: Color) {
    if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
      return;
    }
    let at = ((y as u32 * self.width + x as u32) * 4) as usize;
    self.pixels[at..at + 4].copy_from_slice(&rgba.to_array());
  }

  pub fn fill_rect(&mut self, x: i32, y: i32, width: i32, height: i32, rgba: Color) {
    for py in y..y + height {
      for px in x..x + width {
        self.set(px, py, rgba);
      }
    }
  }

  /// Pixel art with its top-left at (x, y): each character of `rows` is a
  /// pixel in the colour `palette` gives it, `.` (or any it does not name)
  /// left clear
  pub fn paint(&mut self, x: i32, y: i32, rows: &[&str], palette: &[(char, Color)]) {
    for (dy, row) in rows.iter().enumerate() {
      for (dx, mark) in row.chars().enumerate() {
        if let Some(&(_, color)) = palette.iter().find(|(key, _)| *key == mark) {
          self.set(x + dx as i32, y + dy as i32, color);
        }
      }
    }
  }

  pub fn into_sprite(self, name: &str) -> SpriteHandle {
    SpriteHandle::from_sprite(
      Sprite::from_rgba(self.pixels, self.width, self.height),
      name,
    )
  }
}

/// An icon's side in art pixels
pub const ICON: u32 = 16;

/// A slot insets its icon 6 units from each side, so a 48 slot shows a 36
/// unit icon. The 16 pixel art is drawn on an 18 pixel canvas with a clear
/// pixel round it: 2 units a pixel, the art 32 units across.
const ICON_CANVAS: u32 = ICON + 2;

const OUTLINE: Color = Color::rgb(24, 20, 30);

/// An item icon from its art
pub fn icon(rows: &[&str], palette: &[(char, Color)], name: &str) -> SpriteHandle {
  let mut canvas = Canvas::new(ICON_CANVAS, ICON_CANVAS);
  canvas.paint(1, 1, rows, palette);
  canvas.into_sprite(name)
}

/// The outline colour every icon shares, as a palette entry
fn ink() -> (char, Color) {
  ('k', OUTLINE)
}

pub const SWORD: [&str; 16] = [
  "................",
  "............kkk.",
  "...........kwwk.",
  "..........kwwsk.",
  ".........kwwsk..",
  "........kwwsk...",
  ".......kwwsk....",
  "..kk..kwwsk.....",
  "..kgkkwwsk......",
  "...kgwwsk.......",
  "....kgsk........",
  "...kgkgk........",
  "..kbk.kgk.......",
  ".kbk...kk.......",
  ".kk.............",
  "................",
];

pub const SHIELD: [&str; 16] = [
  "................",
  "..kkkkkkkkkkkk..",
  "..kbbbbyybbbbk..",
  "..kbbbbyybbbbk..",
  "..kbbbbyybbbbk..",
  "..kyyyyyyyyyyk..",
  "..kyyyyyyyyyyk..",
  "..kbbbbyybbbbk..",
  "..kbbbbyybbbbk..",
  "...kbbbyybbbk...",
  "...kbbbyybbbk...",
  "....kbbyybbk....",
  ".....kbyybk.....",
  "......kyyk......",
  ".......kk.......",
  "................",
];

pub const HELMET: [&str; 16] = [
  "................",
  "................",
  "......kkkk......",
  "....kkwwsskk....",
  "...kwwsssssk....",
  "..kwssssssssk...",
  "..kwsssssssssk..",
  "..kssssssssssk..",
  "..kkkkkkkkkkkk..",
  "..ksk......ksk..",
  "..kssssssssssk..",
  "..kssskssksssk..",
  "..ksssskksssk...",
  "...kssssssssk...",
  "....kkkkkkkk....",
  "................",
];

pub const ARMOR: [&str; 16] = [
  "................",
  "..kkk......kkk..",
  ".krrrk....krrrk.",
  ".krrrrkkkkrrrrk.",
  ".krrrrryyrrrrrk.",
  "..kkrrryyrrrkk..",
  "...krrryyrrrk...",
  "...krrryyrrrk...",
  "...krrryyrrrk...",
  "...kyyyyyyyyk...",
  "...krrryyrrrk...",
  "...krrryyrrrk...",
  "...krrrrrrrrk...",
  "....kkkkkkkk....",
  "................",
  "................",
];

pub const POTION: [&str; 16] = [
  "................",
  "......kkkk......",
  "......kcck......",
  "......kkkk......",
  ".......kwk......",
  ".......kwk......",
  ".....kkwwkk.....",
  "....kwwwwwwk....",
  "...kwrrrrrrrk...",
  "...krrrrrrrrk...",
  "...krwrrrrrrk...",
  "...krwrrrrrrk...",
  "...krrrrrrrrk...",
  "....krrrrrrk....",
  ".....kkkkkk.....",
  "................",
];

pub const GEM: [&str; 16] = [
  "................",
  "................",
  "....kkkkkkkk....",
  "...kwwbbbbbbk...",
  "..kwwbbbbbbbbk..",
  ".kkkkkkkkkkkkkk.",
  ".kbbwbbbbbbdbbk.",
  "..kbbwbbbbdbbk..",
  "...kbbwbbdbbk...",
  "....kbbwdbbk....",
  ".....kbbdbk.....",
  "......kbbk......",
  ".......kk.......",
  "................",
  "................",
  "................",
];

pub const KEY: [&str; 16] = [
  "................",
  "................",
  "................",
  "...kkkk.........",
  "..kyyyyk........",
  ".kyykkyyk.......",
  ".kyk..kyk.......",
  ".kyk..kykkkkkkk.",
  ".kyykkyyyyyyyyyk",
  "..kyyyyykkykykk.",
  "...kkkk..kk.kk..",
  "................",
  "................",
  "................",
  "................",
  "................",
];

pub const BREAD: [&str; 16] = [
  "................",
  "................",
  "................",
  "................",
  ".....kkkkkk.....",
  "...kkbbbbbbkk...",
  "..kbblbblbblbk..",
  ".kbbbllbllbllbk.",
  ".kbbbbbbbbbbbbk.",
  ".kbbbbbbbbbbbbk.",
  ".kdbbbbbbbbbbdk.",
  "..kddddddddddk..",
  "...kkkkkkkkkk...",
  "................",
  "................",
  "................",
];

pub fn sword() -> SpriteHandle {
  let palette = [
    ink(),
    ('w', Color::rgb(236, 240, 248)),
    ('s', Color::rgb(150, 160, 180)),
    ('g', Color::rgb(230, 186, 70)),
    ('b', Color::rgb(130, 82, 50)),
  ];
  icon(&SWORD, &palette, "ui-demo-sword")
}

pub fn shield() -> SpriteHandle {
  let palette = [
    ink(),
    ('b', Color::rgb(60, 100, 200)),
    ('y', Color::rgb(240, 200, 80)),
  ];
  icon(&SHIELD, &palette, "ui-demo-shield")
}

pub fn helmet() -> SpriteHandle {
  let palette = [
    ink(),
    ('w', Color::rgb(236, 240, 248)),
    ('s', Color::rgb(150, 160, 180)),
  ];
  icon(&HELMET, &palette, "ui-demo-helmet")
}

pub fn armor() -> SpriteHandle {
  let palette = [
    ink(),
    ('r', Color::rgb(170, 60, 60)),
    ('y', Color::rgb(240, 200, 80)),
  ];
  icon(&ARMOR, &palette, "ui-demo-armor")
}

pub fn potion() -> SpriteHandle {
  let palette = [
    ink(),
    ('c', Color::rgb(160, 110, 70)),
    ('w', Color::rgb(220, 236, 255)),
    ('r', Color::rgb(220, 50, 70)),
  ];
  icon(&POTION, &palette, "ui-demo-potion")
}

pub fn gem() -> SpriteHandle {
  let palette = [
    ink(),
    ('w', Color::rgb(220, 250, 255)),
    ('b', Color::rgb(70, 190, 240)),
    ('d', Color::rgb(30, 110, 180)),
  ];
  icon(&GEM, &palette, "ui-demo-gem")
}

pub fn key() -> SpriteHandle {
  let palette = [ink(), ('y', Color::rgb(240, 200, 80))];
  icon(&KEY, &palette, "ui-demo-key")
}

pub fn bread() -> SpriteHandle {
  let palette = [
    ink(),
    ('b', Color::rgb(210, 150, 80)),
    ('l', Color::rgb(245, 210, 140)),
    ('d', Color::rgb(150, 95, 45)),
  ];
  icon(&BREAD, &palette, "ui-demo-bread")
}

/// The portrait's side in art pixels
pub const FACE_SIDE: u32 = 24;

pub const FACE: [&str; 24] = [
  "........................",
  "........kkkkkkkk........",
  "......kkhhhhhhhhkk......",
  ".....khhhhhhhhhhhhk.....",
  "....khhhhhhhhhhhhhhk....",
  "....khhffffffffffhhk....",
  "...khhffffffffffffhhk...",
  "...khffffffffffffffhk...",
  "...kfffkkffffffkkfffk...",
  "...kffkwekffffkwekffk...",
  "...kffffffffffffffffk...",
  "..kfffffffffnnfffffffk..",
  "..kffffffffffnffffffffk.",
  "...kffffffffffffffffk...",
  "...kffmmmmmmmmmmmmffk...",
  "...kfmmmmrrrrrrmmmmfk...",
  "....kmmmmmmmmmmmmmmk....",
  "....kmmmmmmmmmmmmmmk....",
  ".....kmmmmmmmmmmmmk.....",
  "......kkmmmmmmmmkk......",
  ".......ccckkkkccc.......",
  ".....cccccccccccccc.....",
  "....cccccccccccccccc....",
  "...cccccccccccccccccc...",
];

/// The shopkeeper: a bearded face, 24 pixels square
pub fn shopkeeper() -> SpriteHandle {
  let palette = [
    ink(),
    ('h', Color::rgb(90, 60, 40)),
    ('f', Color::rgb(240, 196, 150)),
    ('w', Color::rgb(250, 250, 250)),
    ('e', Color::rgb(40, 80, 140)),
    ('n', Color::rgb(210, 150, 110)),
    ('m', Color::rgb(170, 170, 180)),
    ('r', Color::rgb(150, 70, 70)),
    ('c', Color::rgb(70, 110, 80)),
  ];
  let mut canvas = Canvas::new(FACE_SIDE, FACE_SIDE);
  // The shop behind: a wall, and the counter's top
  canvas.fill_rect(0, 0, 24, 24, Color::rgb(58, 46, 70));
  canvas.fill_rect(0, 20, 24, 4, Color::rgb(96, 64, 44));
  canvas.paint(0, 0, &FACE, &palette);
  canvas.into_sprite("ui-demo-shopkeeper")
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn every_drawing_is_its_size() {
    let icons: [(&str, &[&str]); 8] = [
      ("sword", &SWORD),
      ("shield", &SHIELD),
      ("helmet", &HELMET),
      ("armor", &ARMOR),
      ("potion", &POTION),
      ("gem", &GEM),
      ("key", &KEY),
      ("bread", &BREAD),
    ];
    for (name, rows) in icons {
      for (y, row) in rows.iter().enumerate() {
        assert_eq!(row.len(), ICON as usize, "{name}, row {y}");
      }
    }
    for (y, row) in FACE.iter().enumerate() {
      assert_eq!(row.len(), FACE_SIDE as usize, "face, row {y}");
    }
  }
}
