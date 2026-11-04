//! The page "An isometric town": `maps/town.tmx`, an isometric map of 16 x
//! 16 cells of 116 x 55 drawn with Kenney's Sketch Town (halved), written by
//! Tiled.
//! Its `ground` layer holds grass and water blocks, their tops on the cells
//! (the ground tileset draws them a cell lower), and its `props` layer,
//! y-sorted, the castle's walls and towers and the trees, taller than their
//! cells, standing on those tops. A character (Tiny Dungeon's knight,
//! scaled to the cells) walks among them at the props layer's z, so it
//! draws behind a wall or a tree whose cell is below its feet and in front
//! of one above them. The walls and the water collide along their cells'
//! diamonds.
//!
//! The pointer picks the cell under it on the layer's grid
//! (`ctx.maps().grid`), outlined with `LayerGrid::outline_world`; the left
//! button paints water there and the right grass, through the ground
//! tileset's Wang set (`ctx.maps().set_terrain`), its edges picked from
//! the corners round it, and the painted water collides at once

use demo_kit::prelude::*;

use crate::shared::{
  DIM, GOOD, HOT, View, beside_panel, clear_to, draw_outline, hovered_cell, hud, load, wasd,
};
use crate::walker::{Walk, face, map_start, place_of, spawn_walker};

/// The town
const TOWN: &str = "maps/town.tmx";
/// Its layers: the blocks, and the props the character walks among
const GROUND: &str = "ground";
const PROPS: &str = "props";
/// The ground tileset's Wang set and its colours
const TERRAIN: &str = "terrain";
const WATER: &str = "water";
const GRASS: &str = "grass";
/// The view's background, should the map give none
const SKY: Color = Color::rgb(120, 185, 216);
/// The knight, 32 px tall on cells 116 wide, 16 screen pixels at half zoom
const KNIGHT: Walk = Walk {
  sprite: 97,
  scale: 2.0,
  radius: 10.0,
  speed: 180.0,
};
/// The zooms the page offers: a half shows most of the town, a whole
/// draws the knight's pixels two to a texel
const ZOOMS: [f32; 2] = [0.5, 1.0];
/// The settings rows, for the notes under the panel
const ROWS: usize = 1;

pub(crate) struct TownPage {
  view: View,
  root: Option<Entity>,
  /// Where `player_start` stands
  start: Vec2,
  /// The props layer's z, at which the character sorts among them
  props_z: i32,
  walker: Option<Entity>,
  hover: Option<IVec2>,
  painted: u32,
  /// Where the page's notes are on screen: the pointer paints nothing there
  covered: Vec<Rect>,
  zoom: usize,
}

impl TownPage {
  pub(crate) fn new() -> Self {
    Self {
      view: View::new(true),
      root: None,
      start: Vec2::ZERO,
      props_z: 1,
      walker: None,
      hover: None,
      painted: 0,
      covered: Vec::new(),
      zoom: 0,
    }
  }

  /// The camera on the character (on the start until it spawns), on whole
  /// screen pixels
  fn follow(&self, ctx: &mut SceneContext, jump: bool) {
    let focus = place_of(ctx, self.walker).unwrap_or(self.start);
    let zoom = ZOOMS[self.zoom];
    let centre = self.view.centre_for(ctx, focus, zoom);
    self.view.place(ctx, centre, zoom, jump);
  }

  /// The left button paints water at `cell`, the right grass. Painting a
  /// cell turns its four corners to water, so water would wall the knight
  /// in on any cell sharing a corner with a cell it stands on: those are
  /// refused
  fn paint(&mut self, ctx: &mut SceneContext, root: Entity, cell: IVec2) {
    let (left, right) = (
      ctx.input.mouse_down(MouseButton::Left),
      ctx.input.mouse_down(MouseButton::Right),
    );
    let color = match (left, right) {
      (true, _) => WATER,
      (false, true) => GRASS,
      _ => return,
    };
    let feet = place_of(ctx, self.walker);
    let mut maps = ctx.maps();
    let walled_in = color == WATER
      && feet
        .zip(maps.grid(root, GROUND))
        .is_some_and(|(at, layer)| beside_knight(&layer, at, cell));
    if !walled_in && maps.set_terrain(root, GROUND, cell, TERRAIN, Some(color)) {
      self.painted += 1;
    }
  }
}

/// Whether `cell` shares a corner with a cell the knight's collider, at
/// `feet`, reaches into (on an isometric grid, the cells round a cell by
/// index)
fn beside_knight(layer: &LayerGrid, feet: Vec2, cell: IVec2) -> bool {
  let under = layer.cell_at_world(feet);
  (-1..=1)
    .flat_map(|dy| (-1..=1).map(move |dx| under + IVec2::new(dx, dy)))
    .filter(|&stood| touches(layer.outline_world(stood).points(), feet, KNIGHT.radius))
    .any(|stood| (cell - stood).abs().max_element() <= 1)
}

/// Whether a circle of `radius` at `at` reaches into the convex polygon
/// `outline` (clockwise on screen)
fn touches(outline: &[Vec2], at: Vec2, radius: f32) -> bool {
  let n = outline.len();
  let mut inside = true;
  for i in 0..n {
    let (a, b) = (outline[i], outline[(i + 1) % n]);
    let edge = b - a;
    if edge.length_squared() == 0.0 {
      continue;
    }
    let t = ((at - a).dot(edge) / edge.length_squared()).clamp(0.0, 1.0);
    if at.distance(a + edge * t) < radius {
      return true;
    }
    inside &= edge.perp_dot(at - a) >= 0.0;
  }
  inside
}

impl DemoPage for TownPage {
  fn name(&self) -> &str {
    "An isometric town"
  }

  fn description(&self) -> &str {
    "Tall walls and trees y-sorted on diamonds: walk behind and in front of them, and paint water"
  }

  fn settings(&mut self, s: &mut Settings) {
    s.choice("Zoom", &mut self.zoom, &["1/2", "1"]);
  }

  fn controls(&self) -> Vec<Control> {
    vec![
      wasd("Walk (pad: left stick)"),
      Control::mouse("Left paints water, right grass"),
    ]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    let Some(map) = load::<TileMap>(ctx, TOWN) else {
      return;
    };
    let background = {
      let assets = ctx.assets();
      let Some(data) = assets.get(&map) else {
        return;
      };
      self.start = map_start(data).unwrap_or_default();
      self.props_z = data.layer_index(PROPS).map_or(1, |i| i as i32);
      data.background
    };
    clear_to(ctx, background.unwrap_or(SKY));
    self.root = Some(
      ctx
        .world
        .spawn((Transform::IDENTITY, MapRoot::new(map)))
        .id(),
    );
    self.follow(ctx, true);
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    let Some(root) = self.root else {
      return;
    };
    if self.walker.is_none() && ctx.maps().is_ready(root) {
      self.walker = Some(spawn_walker(ctx, self.start, KNIGHT, self.props_z));
    }
    if let Some(walker) = self.walker {
      face(ctx, walker);
    }
    self.follow(ctx, false);
    self.hover = hovered_cell(ctx, root, PROPS, &self.covered);
    let Some(cell) = self.hover else {
      return;
    };
    self.paint(ctx, root, cell);
    let Some(layer) = ctx.maps().grid(root, PROPS) else {
      return;
    };
    let painting =
      ctx.input.mouse_down(MouseButton::Left) || ctx.input.mouse_down(MouseButton::Right);
    let color = if painting { HOT } else { Color::WHITE };
    draw_outline(
      &mut ctx.debug_draw(),
      &layer.outline_world(cell),
      color,
      2.0,
    );
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    self.view.measure(ui);
    self.covered.clear();
    let Some(root) = self.root else {
      return;
    };
    let pointer = self.hover.map_or_else(
      || "Pointer: off the town".to_string(),
      |cell| format!("Pointer: cell {}, {}", cell.x, cell.y),
    );
    let here = place_of(ctx, self.walker).and_then(|at| ctx.maps().world_to_cell(root, PROPS, at));
    let walker = here.map_or_else(
      || "Knight: waiting for the map".to_string(),
      |cell| format!("Knight: cell {}, {}", cell.x, cell.y),
    );
    let lines = vec![
      (
        "Isometric cells of 116 x 55; walls and trees y-sorted".to_string(),
        Color::WHITE,
      ),
      (pointer, GOOD),
      (walker, Color::WHITE),
      (format!("Cells painted: {}", self.painted), HOT),
      (
        "Walls and water stop the knight; painted water too".to_string(),
        DIM,
      ),
    ];
    let column = beside_panel(ui, ROWS);
    let notes = hud(ui, column.x, column.y, column.width, &lines);
    self.covered.push(notes);
  }
}
