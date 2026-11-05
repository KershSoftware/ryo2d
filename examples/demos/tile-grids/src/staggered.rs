//! The page "A staggered map": an island of Sketch Town's blocks on a
//! staggered grid, diamonds in rows with every other row shifted half a
//! cell right (`maps/island-rows.tmx`, staggered along y), and the same
//! island with every other column shifted half a cell down
//! (`maps/island-columns.tmx`, along x), both written by Tiled. The Grid
//! setting swaps one map for the other: a map's grid is its file's, so the
//! page spawns the other map rather than reshaping this one. A character
//! walks among the island's trees (its props layer y-sorted) and the sea
//! stops it along its cells' diamonds; the pointer's cell is outlined on
//! the layer's grid, and the HUD names its neighbours

use demo_kit::prelude::*;

use crate::shared::{
  DIM, GOOD, HOT, View, beside_panel, clear_to, draw_outline, hovered_cell, hud, load, wasd,
};
use crate::walker::{Walk, face, map_start, place_of, spawn_walker};

/// The island on each grid, as the Grid setting names them
const MAPS: [&str; 2] = ["maps/island-rows.tmx", "maps/island-columns.tmx"];
const GRIDS: [&str; 2] = ["Rows (y)", "Columns (x)"];
/// And as the HUD tells them
const SHIFTED: [&str; 2] = ["rows shifted along y", "columns shifted along x"];
/// The layers: the blocks, and the trees and rocks
const GROUND: &str = "ground";
const PROPS: &str = "props";
const SEA: Color = Color::rgb(120, 185, 216);
/// The walker: Tiny Dungeon's woman, scaled to the cells
const WALKER: Walk = Walk {
  sprite: 99,
  scale: 4.0,
  radius: 20.0,
  speed: 360.0,
};
const ZOOM: f32 = 0.25;
/// The settings rows, for the notes under the panel
const ROWS: usize = 1;

pub(crate) struct StaggeredPage {
  view: View,
  root: Option<Entity>,
  /// The map the root shows, by the Grid setting's index
  shown: usize,
  start: Vec2,
  props_z: i32,
  walker: Option<Entity>,
  hover: Option<IVec2>,
  covered: Vec<Rect>,
  grid: usize,
}

impl StaggeredPage {
  pub(crate) fn new() -> Self {
    Self {
      view: View::new(true),
      root: None,
      shown: 0,
      start: Vec2::ZERO,
      props_z: 1,
      walker: None,
      hover: None,
      covered: Vec::new(),
      grid: 0,
    }
  }

  /// The island on the chosen grid, in place of the one there; the walker
  /// goes too, and comes back at the new map's start once it has landed
  fn spawn_island(&mut self, ctx: &mut SceneContext) {
    for entity in [self.root.take(), self.walker.take()].into_iter().flatten() {
      ctx.world.despawn(entity);
    }
    self.shown = self.grid;
    let Some(map) = load::<TileMap>(ctx, MAPS[self.grid]) else {
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
    clear_to(ctx, background.unwrap_or(SEA));
    self.root = Some(
      ctx
        .world
        .spawn((Transform::IDENTITY, MapRoot::new(map)))
        .id(),
    );
  }

  fn follow(&self, ctx: &mut SceneContext, jump: bool) {
    let focus = place_of(ctx, self.walker).unwrap_or(self.start);
    let centre = self.view.centre_for(ctx, focus, ZOOM);
    self.view.place(ctx, centre, ZOOM, jump);
  }
}

impl DemoPage for StaggeredPage {
  fn name(&self) -> &str {
    "A staggered map"
  }

  fn description(&self) -> &str {
    "Diamonds in shifted rows or columns: an island to walk, its sea stopping you at its cells"
  }

  fn settings(&mut self, s: &mut Settings) {
    s.choice("Grid", &mut self.grid, &GRIDS);
  }

  fn controls(&self) -> Vec<Control> {
    vec![
      wasd("Walk (pad: left stick)"),
      Control::mouse("Point at a cell"),
    ]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    self.spawn_island(ctx);
    self.follow(ctx, true);
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    if self.grid != self.shown {
      self.spawn_island(ctx);
      self.follow(ctx, true);
    }
    let Some(root) = self.root else {
      return;
    };
    if self.walker.is_none() && ctx.maps().is_ready(root) {
      self.walker = Some(spawn_walker(ctx, self.start, WALKER, self.props_z));
    }
    if let Some(walker) = self.walker {
      face(ctx, walker);
    }
    self.follow(ctx, false);
    self.hover = hovered_cell(ctx, root, GROUND, &self.covered);
    if let (Some(cell), Some(layer)) = (self.hover, ctx.maps().grid(root, GROUND)) {
      draw_outline(
        &mut ctx.debug_draw(),
        &layer.outline_world(cell),
        Color::WHITE,
        2.0,
      );
    }
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    self.view.measure(ui);
    self.covered.clear();
    let Some(root) = self.root else {
      return;
    };
    let layer = ctx.maps().grid(root, GROUND);
    let pointer = match (self.hover, layer) {
      (Some(cell), Some(layer)) => {
        let around: Vec<String> = layer
          .grid
          .neighbours(cell)
          .map(|n| format!("{}, {}", n.x, n.y))
          .collect();
        vec![
          (format!("Pointer: cell {}, {}", cell.x, cell.y), GOOD),
          (format!("Its neighbours: {}", around.join("; ")), DIM),
        ]
      }
      _ => vec![("Pointer: off the island's map".to_string(), GOOD)],
    };
    let here = place_of(ctx, self.walker).and_then(|at| ctx.maps().world_to_cell(root, GROUND, at));
    let mut lines = vec![(
      format!("Staggered diamonds of 232 x 110, {}", SHIFTED[self.shown]),
      Color::WHITE,
    )];
    lines.extend(pointer);
    lines.push((
      here.map_or_else(
        || "Walker: waiting for the map".to_string(),
        |cell| format!("Walker: cell {}, {}", cell.x, cell.y),
      ),
      Color::WHITE,
    ));
    lines.push(("The sea stops the walker at its cells".to_string(), HOT));
    let column = beside_panel(ui, ROWS);
    let notes = hud(ui, column.x, column.y, column.width, &lines);
    self.covered.push(notes);
  }
}
