//! The Flips and mixed tilesets page: the platformer's ground layer draws
//! from two tilesets at once (Pixel Platformer's sheet and the derived
//! ground with its slopes), its scenery holds tiles Tiled flipped, and the
//! page puts a signpost and a slope into the sky in each of the eight ways
//! a cell can turn (`ctx.maps().set_tile` with a `TileFlip`), turning or
//! mirroring them all at a key

use demo_kit::prelude::*;

use crate::shared::{
  DIM, HOT, PLATFORMER, View, asset, clear_to, hud_beside_panel, label_at, load,
};

/// The eight orientations as Tiled stores them (horizontal, vertical and
/// diagonal bits; drawn diagonal first, then horizontal, then vertical)
const FLIPS: [(bool, bool, bool); 8] = [
  (false, false, false),
  (true, false, false),
  (false, true, false),
  (true, true, false),
  (false, false, true),
  (true, false, true),
  (false, true, true),
  (true, true, true),
];
/// The signpost pointing right, in the platformer's sheet (tileset 0)
const SIGN: Tile = Tile {
  tileset: 0,
  id: 85,
  flip: TileFlip::NONE,
};
/// The rising slope, in the derived ground (tileset 1)
const SLOPE: Tile = Tile {
  tileset: 1,
  id: 16,
  flip: TileFlip::NONE,
};
/// The rows the two strips sit in, in the open sky; slot `i` is column
/// `2 * i`
const SIGN_ROW: i32 = 3;
const SLOPE_ROW: i32 = 5;
/// What the page frames: columns 0-18, the map's whole height, at 1.5x (2x
/// would leave the strips' last slot out of the area)
const FRAMED: Rect = Rect::new(0.0, 0.0, 342.0, 288.0);
const ZOOM: f32 = 1.5;
/// Each tileset's mark
const MARKS: [Color; 2] = [Color::rgb(255, 150, 60), Color::rgb(90, 220, 255)];

/// A flip from its three bits
fn flip_of(h: bool, v: bool, d: bool) -> TileFlip {
  let mut flip = TileFlip::NONE;
  for (on, bit) in [
    (h, TileFlip::HORIZONTAL),
    (v, TileFlip::VERTICAL),
    (d, TileFlip::DIAGONAL),
  ] {
    if on {
      flip = flip | bit;
    }
  }
  flip
}

/// `flip` mirrored left to right on screen: horizontal comes after
/// diagonal in Tiled's order, so it is the horizontal bit toggled
fn mirrored(flip: TileFlip) -> TileFlip {
  flip_of(
    !flip.contains(TileFlip::HORIZONTAL),
    flip.contains(TileFlip::VERTICAL),
    flip.contains(TileFlip::DIAGONAL),
  )
}

/// A flip's bits as text
fn bits(flip: TileFlip) -> String {
  let names: Vec<&str> = [
    (TileFlip::DIAGONAL, "D"),
    (TileFlip::HORIZONTAL, "H"),
    (TileFlip::VERTICAL, "V"),
  ]
  .into_iter()
  .filter(|(bit, _)| flip.contains(*bit))
  .map(|(_, name)| name)
  .collect();
  if names.is_empty() {
    "none".to_string()
  } else {
    names.join(" ")
  }
}

pub(crate) struct FlipsPage {
  view: View,
  map: Option<Handle<TileMap>>,
  root: Option<Entity>,
  placed: bool,
  marks: bool,
  turns: u32,
  mirrors: u32,
  /// Cells of each layer the map's file filled, by tileset, and how many
  /// are flipped
  counts: [usize; 2],
  flipped: usize,
}

impl FlipsPage {
  pub(crate) fn new() -> Self {
    Self {
      view: View::new(true),
      map: None,
      root: None,
      placed: false,
      marks: true,
      turns: 0,
      mirrors: 0,
      counts: [0; 2],
      flipped: 0,
    }
  }

  /// Every strip cell, with its tile now
  fn strip(&self, ctx: &mut SceneContext) -> Vec<(IVec2, Tile)> {
    let Some(root) = self.root else {
      return Vec::new();
    };
    let maps = ctx.maps();
    let mut cells = Vec::new();
    for row in [SIGN_ROW, SLOPE_ROW] {
      for slot in 0..FLIPS.len() as i32 {
        let cell = IVec2::new(2 * slot, row);
        if let Some(tile) = maps.tile(root, "scenery", cell) {
          cells.push((cell, tile));
        }
      }
    }
    cells
  }

  /// Every strip tile's flip changed by `change`
  fn turn_all(&self, ctx: &mut SceneContext, change: impl Fn(TileFlip) -> TileFlip) {
    let Some(root) = self.root else {
      return;
    };
    for (cell, tile) in self.strip(ctx) {
      let turned = tile.with_flip(change(tile.flip));
      ctx.maps().set_tile(root, "scenery", cell, Some(turned));
    }
  }
}

impl DemoPage for FlipsPage {
  fn name(&self) -> &str {
    "Flips and mixed tilesets"
  }

  fn description(&self) -> &str {
    "One layer drawing from two tilesets, tiles flipped in Tiled, and a signpost and a slope in all eight turns"
  }

  fn settings(&mut self, s: &mut Settings) {
    s.toggle("Mark tilesets", &mut self.marks);
  }

  fn controls(&self) -> Vec<Control> {
    vec![
      Control::key(KeyCode::Space, "Turn the strips 90 degrees clockwise"),
      Control::key(KeyCode::KeyF, "Mirror the strips"),
    ]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    self.map = load::<TileMap>(ctx, PLATFORMER);
    let Some(map) = self.map.clone() else {
      return;
    };
    if let Some(data) = asset(ctx, &map) {
      clear_to(ctx, data.background.unwrap_or(Color::rgb(127, 240, 165)));
      for layer in &data.layers {
        if let LayerKind::Tiles { grid, .. } = &layer.kind {
          let size = grid.size().as_ivec2();
          for y in 0..size.y {
            for x in 0..size.x {
              if let Some(tile) = grid.get(IVec2::new(x, y)) {
                if let Some(count) = self.counts.get_mut(usize::from(tile.tileset)) {
                  *count += 1;
                }
                if tile.flip != TileFlip::NONE {
                  self.flipped += 1;
                }
              }
            }
          }
        }
      }
    }
    self.root = Some(
      ctx
        .world
        .spawn((Transform::IDENTITY, MapRoot::new(map)))
        .id(),
    );
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    let Some(root) = self.root else {
      return;
    };
    if !self.placed && ctx.maps().is_ready(root) {
      // Into the scenery layer, which draws and does not collide: tiles of
      // both tilesets, as the file's own layers mix them
      for (slot, &(h, v, d)) in FLIPS.iter().enumerate() {
        let column = 2 * slot as i32;
        let flip = flip_of(h, v, d);
        let mut maps = ctx.maps();
        maps.set_tile(
          root,
          "scenery",
          IVec2::new(column, SIGN_ROW),
          Some(SIGN.with_flip(flip)),
        );
        maps.set_tile(
          root,
          "scenery",
          IVec2::new(column, SLOPE_ROW),
          Some(SLOPE.with_flip(flip)),
        );
      }
      self.placed = true;
    }
    if self.placed && ctx.input.key_pressed(KeyCode::Space) {
      self.turn_all(ctx, TileFlip::rotated_cw);
      self.turns += 1;
    }
    if self.placed && ctx.input.key_pressed(KeyCode::KeyF) {
      self.turn_all(ctx, mirrored);
      self.mirrors += 1;
    }
    if self.marks && self.placed {
      // Each tile of the framed cells, outlined in its tileset's colour; a
      // flipped one gets a cross too
      let mut marks = Vec::new();
      {
        let maps = ctx.maps();
        for layer in ["scenery", "ground", "doors"] {
          for y in 0..16 {
            for x in 0..19 {
              let cell = IVec2::new(x, y);
              if let Some(tile) = maps.tile(root, layer, cell)
                && let Some(at) = maps.cell_to_world(root, layer, cell)
              {
                marks.push((at, tile));
              }
            }
          }
        }
      }
      let mut debug = ctx.debug_draw();
      for (at, tile) in marks {
        let color = MARKS[usize::from(tile.tileset).min(1)];
        debug.rect(at, Vec2::splat(15.0), color);
        if tile.flip != TileFlip::NONE {
          debug.cross(at, 6.0, Color::rgb(255, 90, 200));
        }
      }
    }
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    self.view.measure(ui);
    let (x, y) = FRAMED.center();
    self.view.look_at(ctx, Vec2::new(x, y), ZOOM);

    // Each column's flips as the map has them now (both strips turn alike)
    let strip = self.strip(ctx);
    for (cell, tile) in strip.iter().filter(|(cell, _)| cell.y == SIGN_ROW) {
      let x = cell.x as f32 * 18.0 + 9.0;
      label_at(
        ui,
        ctx,
        Vec2::new(x, SIGN_ROW as f32 * 18.0 - 1.0),
        &bits(tile.flip),
        HOT,
      );
    }
    let first = strip
      .first()
      .map_or_else(|| "-".to_string(), |(_, tile)| bits(tile.flip));
    let lines = vec![
      (
        format!(
          "In the file: {} tiles of tileset 0, {} of tileset 1, {} flipped",
          self.counts[0], self.counts[1], self.flipped
        ),
        Color::WHITE,
      ),
      (
        "Orange: platformer.tsx, blue: ground.tsx, pink: flipped".to_string(),
        DIM,
      ),
      (
        format!(
          "Strips: {} turns, {} mirrors; the first is {first}",
          self.turns, self.mirrors
        ),
        DIM,
      ),
    ];
    hud_beside_panel(ui, 1, &lines);
  }
}
