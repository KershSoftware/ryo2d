//! The page "A hex board": `maps/hex-board.tmx`, 8 x 9 pointy-topped
//! hexagons of 60 x 70 (Kenney's Hexagon Pack, halved) in rows, every other row half a hex
//! right, written by Tiled. The sand's dunes are turned a sixth of a turn
//! at a time (Tiled's hex turns, `TileFlip::HEX_ROTATE_60` and
//! `HEX_ROTATE_120`), so their ripples run three ways; the trees and rocks
//! stand unturned.
//!
//! The pointer picks the hex under it (`LayerGrid::cell_at_world`: the
//! hex whose drawn outline holds the point, which on these hexes, close to
//! regular but not quite, is not always the one with the nearest centre),
//! outlined in white, and a click selects one, outlined in gold. Through the layer's
//! grid (`ctx.maps().grid`) the board then marks the selected hex's range
//! and its ring at the Radius setting, the line from it to the hovered hex
//! and the hovered hex's neighbours, each on an empty layer of its own the
//! map holds above the board, in the rim its `mark` property names; the
//! HUD gives the distance between them. Hexagonal layers neither collide
//! nor cast shadows, so the board has no character

use demo_kit::prelude::*;

use crate::shared::{
  DIM, GOOD, HOT, View, beside_panel, clear_to, draw_outline, hovered_cell, hud, load,
};

/// The board
const BOARD: &str = "maps/hex-board.tmx";
/// Its layers: the terrain, and the marks over it, bottom first
const TERRAIN: &str = "board";
const MARKS: [&str; 4] = ["range", "ring", "neighbours", "line"];
/// Each mark's place in `MARKS` and in the page's `marks`
const RANGE: usize = 0;
const RING: usize = 1;
const NEIGHBOURS: usize = 2;
const LINE: usize = 3;
/// The view's background, should the map give none
const NIGHT: Color = Color::rgb(36, 49, 63);
/// The zoom the board fits the view at: 60 x 70 px a hex, a texel a pixel
const ZOOM: f32 = 1.0;
/// The hex selected until a click picks another: the board's middle
const FIRST: IVec2 = IVec2::new(4, 4);
/// The settings rows, for the notes under the panel
const ROWS: usize = 1;

pub(crate) struct HexPage {
  view: View,
  root: Option<Entity>,
  hover: Option<IVec2>,
  selected: IVec2,
  /// Each mark layer's rim, from its `mark` property, and the cells it
  /// marks now
  marks: [(u32, Vec<IVec2>); 4],
  /// What the marks were drawn for: selected, hovered and radius
  marked: Option<(IVec2, Option<IVec2>, i32)>,
  covered: Vec<Rect>,
  radius: i32,
}

impl HexPage {
  pub(crate) fn new() -> Self {
    Self {
      view: View::new(true),
      root: None,
      hover: None,
      selected: FIRST,
      marks: Default::default(),
      marked: None,
      covered: Vec::new(),
      radius: 2,
    }
  }

  /// Each mark layer's rim tile, from the map's layer properties
  fn read_marks(&mut self, map: &TileMap) {
    for (i, name) in MARKS.iter().enumerate() {
      let rim = map
        .layer_index(name)
        .and_then(|index| map.layers[index].info.properties.int("mark"))
        .and_then(|id| u32::try_from(id).ok());
      if let Some(rim) = rim {
        self.marks[i].0 = rim;
      }
    }
  }

  /// The marks again, if what they show changed: the old cells cleared, the
  /// new ones set, each layer in its own rim
  fn mark(&mut self, ctx: &mut SceneContext, root: Entity) {
    let now = (self.selected, self.hover, self.radius);
    if self.marked == Some(now) {
      return;
    }
    let (Some(layer), Some(bounds)) = (ctx.maps().grid(root, TERRAIN), ctx.maps().bounds(root))
    else {
      return;
    };
    self.marked = Some(now);
    let grid = layer.grid;
    let radius = self.radius.max(0) as u32;
    let mut wanted: [Vec<IVec2>; 4] = Default::default();
    wanted[RANGE] = grid.range(self.selected, radius);
    wanted[RING] = grid.ring(self.selected, radius);
    if let Some(hover) = self.hover {
      wanted[NEIGHBOURS] = grid.neighbours(hover).collect();
      wanted[LINE] = grid.line(self.selected, hover);
    }
    for cells in &mut wanted {
      cells.retain(|cell| bounds.contains(*cell));
    }
    let mut maps = ctx.maps();
    for ((name, (rim, cells)), new) in MARKS.iter().zip(&mut self.marks).zip(wanted) {
      for cell in cells.drain(..) {
        maps.set_tile(root, *name, cell, None);
      }
      for &cell in &new {
        maps.set_tile(root, *name, cell, Some(Tile::new(0, *rim)));
      }
      *cells = new;
    }
  }

  /// How the hovered hex's tile is turned, as the HUD says it
  fn turn_line(&self, ctx: &mut SceneContext, root: Entity) -> Option<String> {
    let tile = ctx.maps().tile(root, TERRAIN, self.hover?)?;
    let flip = tile.flip;
    let half = flip.contains(TileFlip::HORIZONTAL) && flip.contains(TileFlip::VERTICAL);
    let degrees = 60 * u32::from(flip.contains(TileFlip::HEX_ROTATE_60))
      + 120 * u32::from(flip.contains(TileFlip::HEX_ROTATE_120))
      + 180 * u32::from(half);
    Some(if degrees == 0 {
      "Its tile: unturned".to_string()
    } else {
      format!("Its tile: turned {degrees} degrees clockwise")
    })
  }
}

impl DemoPage for HexPage {
  fn name(&self) -> &str {
    "A hex board"
  }

  fn description(&self) -> &str {
    "Pointy hexagons in rows: pick, select, and see a hex's range, ring, line and neighbours"
  }

  fn settings(&mut self, s: &mut Settings) {
    s.int("Radius", &mut self.radius, 1..=4);
  }

  fn controls(&self) -> Vec<Control> {
    vec![Control::mouse("Point at a hex, click to select it")]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    let Some(map) = load::<TileMap>(ctx, BOARD) else {
      return;
    };
    let (background, size) = {
      let assets = ctx.assets();
      let Some(data) = assets.get(&map) else {
        return;
      };
      self.read_marks(data);
      let size = data.grid().bounds_of(CellRect::from_size(data.size));
      (data.background, size)
    };
    clear_to(ctx, background.unwrap_or(NIGHT));
    self.root = Some(
      ctx
        .world
        .spawn((Transform::IDENTITY, MapRoot::new(map)))
        .id(),
    );
    let (cx, cy) = size.center();
    let centre = self.view.centre_for(ctx, Vec2::new(cx, cy), ZOOM);
    self.view.place(ctx, centre, ZOOM, true);
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    let Some(root) = self.root else {
      return;
    };
    if !ctx.maps().is_ready(root) {
      return;
    }
    self.hover = hovered_cell(ctx, root, TERRAIN, &self.covered);
    if let Some(cell) = self.hover
      && ctx.input.mouse_pressed(MouseButton::Left)
    {
      self.selected = cell;
    }
    self.mark(ctx, root);
    let Some(layer) = ctx.maps().grid(root, TERRAIN) else {
      return;
    };
    let mut debug = ctx.debug_draw();
    draw_outline(&mut debug, &layer.outline_world(self.selected), HOT, 3.0);
    if let Some(cell) = self.hover {
      draw_outline(&mut debug, &layer.outline_world(cell), Color::WHITE, 2.0);
    }
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    self.view.measure(ui);
    self.covered.clear();
    let Some(root) = self.root else {
      return;
    };
    let column = beside_panel(ui, ROWS);
    let Some(layer) = ctx.maps().grid(root, TERRAIN) else {
      let waiting = [("Board: waiting for the map".to_string(), DIM)];
      hud(ui, column.x, column.y, column.width, &waiting);
      return;
    };
    let grid = layer.grid;
    let radius = self.radius.max(0) as u32;
    let (range, ring) = (self.marks[RANGE].1.len(), self.marks[RING].1.len());
    let mut lines = vec![
      (
        "Pointy hexagons of 60 x 70, sides of 35".to_string(),
        Color::WHITE,
      ),
      (
        format!("Selected: hex {}, {}", self.selected.x, self.selected.y),
        HOT,
      ),
      (
        format!(
          "Range {radius}: {range} hexes on the board; its ring {ring} (of {})",
          grid.ring(self.selected, radius).len()
        ),
        Color::WHITE,
      ),
    ];
    match self.hover {
      Some(cell) => {
        lines.push((format!("Pointer: hex {}, {}", cell.x, cell.y), GOOD));
        lines.push((
          format!(
            "Distance: {} steps; the line {} hexes",
            grid.distance(self.selected, cell),
            self.marks[LINE].1.len()
          ),
          GOOD,
        ));
        if let Some(turn) = self.turn_line(ctx, root) {
          lines.push((turn, DIM));
        }
      }
      None => lines.push(("Pointer: off the board".to_string(), GOOD)),
    }
    lines.push((
      "Beige: the ring; grey: neighbours; red: the line".to_string(),
      DIM,
    ));
    let notes = hud(ui, column.x, column.y, column.width, &lines);
    self.covered.push(notes);
  }
}
