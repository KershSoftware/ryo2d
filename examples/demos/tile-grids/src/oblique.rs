//! The page "An oblique map": a dungeon of Tiny Dungeon's 16 px tiles on an
//! oblique grid, each row of cells shifted 4 px further right than the one
//! above (`maps/dungeon.tmx`, skew (4, 0)), and its mirror, shifted 4 px
//! left (`maps/dungeon-mirror.tmx`, skew (-4, 0)), both written by Tiled.
//! The Skew setting swaps one map for the other. A tile's picture moves
//! with its cell, unskewed, so the walls lean as staircases, while they
//! collide along their cells' skewed outlines: the character walks the
//! floor and stops at the walls' slanted sides. The pointer's cell is
//! outlined on the layer's grid

use demo_kit::prelude::*;

use crate::shared::{DIM, GOOD, HOT, View, beside_panel, clear_to, hud, load, wasd};
use crate::walker::{Walk, draw_outline, face, hovered_cell, map_start, place_of, spawn_walker};

/// The dungeon at each skew, as the Skew setting names them
const MAPS: [&str; 2] = ["maps/dungeon.tmx", "maps/dungeon-mirror.tmx"];
const SKEWS: [&str; 2] = ["The map's own: 4, 0", "Its mirror: -4, 0"];
/// How far each row of cells sits across from the one above, in each map
const SHIFTS: [i32; 2] = [4, -4];
/// The layers: the floor, and the walls over it
const FLOOR: &str = "floor";
const WALLS: &str = "walls";
const DARK: Color = Color::rgb(37, 19, 26);
/// Tiny Dungeon's knight, at its own size on 16 px cells
const KNIGHT: Walk = Walk {
  sprite: 96,
  scale: 1.0,
  radius: 5.0,
  speed: 80.0,
};
/// The settings rows, for the notes under the panel
const ROWS: usize = 2;

pub(crate) struct ObliquePage {
  view: View,
  root: Option<Entity>,
  shown: usize,
  start: Vec2,
  walls_z: i32,
  walker: Option<Entity>,
  hover: Option<IVec2>,
  covered: Vec<Rect>,
  skew: usize,
  zoom: i32,
}

impl ObliquePage {
  pub(crate) fn new() -> Self {
    Self {
      view: View::new(true),
      root: None,
      shown: 0,
      start: Vec2::ZERO,
      walls_z: 1,
      walker: None,
      hover: None,
      covered: Vec::new(),
      skew: 0,
      zoom: 2,
    }
  }

  /// The dungeon at the chosen skew, in place of the one there; the
  /// character comes back at the new map's start once it has landed
  fn spawn_dungeon(&mut self, ctx: &mut SceneContext) {
    for entity in [self.root.take(), self.walker.take()].into_iter().flatten() {
      ctx.world.despawn(entity);
    }
    self.shown = self.skew;
    let Some(map) = load::<TileMap>(ctx, MAPS[self.skew]) else {
      return;
    };
    let background = {
      let assets = ctx.assets();
      let Some(data) = assets.get(&map) else {
        return;
      };
      self.start = map_start(data).unwrap_or_default();
      self.walls_z = data.layer_index(WALLS).map_or(1, |i| i as i32);
      data.background
    };
    clear_to(ctx, background.unwrap_or(DARK));
    self.root = Some(
      ctx
        .world
        .spawn((Transform::IDENTITY, MapRoot::new(map)))
        .id(),
    );
  }

  fn follow(&self, ctx: &mut SceneContext, jump: bool) {
    let focus = place_of(ctx, self.walker).unwrap_or(self.start);
    let zoom = self.zoom as f32;
    let centre = self.view.centre_for(ctx, focus, zoom);
    self.view.place(ctx, centre, zoom, jump);
  }
}

impl DemoPage for ObliquePage {
  fn name(&self) -> &str {
    "An oblique map"
  }

  fn description(&self) -> &str {
    "Skewed cells: the walls lean, and the knight stops at their slanted sides"
  }

  fn settings(&mut self, s: &mut Settings) {
    s.choice("Skew", &mut self.skew, &SKEWS);
    s.int("Zoom", &mut self.zoom, 1..=3);
  }

  fn controls(&self) -> Vec<Control> {
    vec![
      wasd("Walk (pad: left stick)"),
      Control::mouse("Point at a cell"),
    ]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    self.spawn_dungeon(ctx);
    self.follow(ctx, true);
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    if self.skew != self.shown {
      self.spawn_dungeon(ctx);
      self.follow(ctx, true);
    }
    let Some(root) = self.root else {
      return;
    };
    if self.walker.is_none() && ctx.maps().is_ready(root) {
      // At the walls' z: in front of the floor, sorted with nothing there
      self.walker = Some(spawn_walker(ctx, self.start, KNIGHT, self.walls_z + 1));
    }
    if let Some(walker) = self.walker {
      face(ctx, walker);
    }
    self.follow(ctx, false);
    self.hover = hovered_cell(ctx, root, FLOOR, &self.covered);
    if let (Some(cell), Some(layer)) = (self.hover, ctx.maps().grid(root, FLOOR)) {
      draw_outline(
        &mut ctx.debug_draw(),
        &layer.outline_world(cell),
        Color::WHITE,
        1.0,
      );
    }
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    self.view.measure(ui);
    self.covered.clear();
    let Some(root) = self.root else {
      return;
    };
    let pointer = self.hover.map_or_else(
      || "Pointer: off the dungeon".to_string(),
      |cell| {
        let wall = ctx.maps().tile(root, WALLS, cell).is_some();
        format!(
          "Pointer: cell {}, {}{}",
          cell.x,
          cell.y,
          if wall { ", a wall" } else { "" }
        )
      },
    );
    let here = place_of(ctx, self.walker).and_then(|at| ctx.maps().world_to_cell(root, FLOOR, at));
    let lines = vec![
      (
        format!("Oblique cells of 16 px, skew ({}, 0)", SHIFTS[self.shown]),
        Color::WHITE,
      ),
      (pointer, GOOD),
      (
        here.map_or_else(
          || "Knight: waiting for the map".to_string(),
          |cell| format!("Knight: cell {}, {}", cell.x, cell.y),
        ),
        Color::WHITE,
      ),
      (
        format!(
          "Each row of cells sits {} px {} of the one above",
          SHIFTS[self.shown].abs(),
          if SHIFTS[self.shown] > 0 {
            "right"
          } else {
            "left"
          }
        ),
        HOT,
      ),
      (
        "The pictures move unskewed; the walls collide skewed".to_string(),
        DIM,
      ),
    ];
    let column = beside_panel(ui, ROWS);
    let notes = hud(ui, column.x, column.y, column.width, &lines);
    self.covered.push(notes);
  }
}
