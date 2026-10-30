//! The Editing page: the mouse paints the platformer's sky with ground,
//! through the ground's Wang set (`ctx.maps().set_terrain`, Tiled's terrain
//! brush): each cell picks the tile whose corners match its neighbours',
//! so the ground joins up as it grows. It is a corner set over art drawn
//! on cell edges, so one painted cell is the middle of a 3 x 3 block, and
//! ground is at least two cells thick. Shift and a drag fills (or erases) a
//! rectangle the same way, cell by cell. The brush keeps to the open sky:
//! the hill's slopes, the ledge and the bridge are tiles outside the set,
//! and painting beside them is not what this page shows

use demo_kit::prelude::*;

use crate::shared::{DIM, HOT, PLATFORMER, View, asset, clear_to, hud, label_at, load, map_rect};

/// The Wang set, its colour and the layer it paints
const SET: &str = "ground";
const COLOR: &str = "ground";
const LAYER: &str = "ground";
/// The cells the brush may paint: those whose neighbours are all open sky
/// (rows 0-6, columns 0-40), as a painted cell's corners reach them
const BRUSH_MIN: IVec2 = IVec2::new(0, 0);
const BRUSH_MAX: IVec2 = IVec2::new(39, 5);
/// A cell's side
const CELL: f32 = 18.0;
/// What the page paints as it opens, to show what a brush does: one cell,
/// and a row of six
const ONE: IVec2 = IVec2::new(3, 2);
const ROW: (IVec2, IVec2) = (IVec2::new(22, 3), IVec2::new(27, 3));

pub(crate) struct EditingPage {
  view: View,
  map: Option<Handle<TileMap>>,
  root: Option<Entity>,
  bounds: Rect,
  /// The examples are painted
  ready: bool,
  /// The cell under the pointer
  hover: Option<IVec2>,
  /// The last cell a held button painted or erased
  last: Option<IVec2>,
  /// A Shift drag: where it started, and whether it paints
  drag: Option<(IVec2, bool)>,
  painted: u32,
  erased: u32,
  filled: u32,
}

impl EditingPage {
  pub(crate) fn new() -> Self {
    Self {
      view: View::new(false),
      map: None,
      root: None,
      bounds: Rect::new(0.0, 0.0, 864.0, 288.0),
      ready: false,
      hover: None,
      last: None,
      drag: None,
      painted: 0,
      erased: 0,
      filled: 0,
    }
  }

  /// Paints (or erases) one cell through the Wang set; whether it changed
  fn brush(&mut self, ctx: &mut SceneContext, cell: IVec2, paint: bool) -> bool {
    let Some(root) = self.root else {
      return false;
    };
    let color = paint.then_some(COLOR);
    ctx.maps().set_terrain(root, LAYER, cell, SET, color)
  }

  /// Paints (or erases) every cell from `a` to `b`, kept to the sky
  fn fill(&mut self, ctx: &mut SceneContext, a: IVec2, b: IVec2, paint: bool) {
    let (min, max) = (a.min(b).max(BRUSH_MIN), a.max(b).min(BRUSH_MAX));
    let mut changed = 0;
    for y in min.y..=max.y {
      for x in min.x..=max.x {
        if self.brush(ctx, IVec2::new(x, y), paint) {
          changed += 1;
        }
      }
    }
    if changed > 0 {
      self.filled += 1;
    }
  }
}

/// Whether the brush may paint `cell`
fn in_sky(cell: IVec2) -> bool {
  cell.cmpge(BRUSH_MIN).all() && cell.cmple(BRUSH_MAX).all()
}

/// The world rectangle from cell `a` to cell `b`, both included
fn cells_rect(a: IVec2, b: IVec2) -> (Vec2, Vec2) {
  let (min, max) = (a.min(b), a.max(b) + IVec2::ONE);
  let (min, max) = (min.as_vec2() * CELL, max.as_vec2() * CELL);
  ((min + max) / 2.0, max - min)
}

impl DemoPage for EditingPage {
  fn name(&self) -> &str {
    "Editing"
  }

  fn description(&self) -> &str {
    "Paint ground into the sky through the Wang set: every cell picks the tile that joins its neighbours"
  }

  fn controls(&self) -> Vec<Control> {
    vec![Control::mouse(
      "Left paints, right erases; Shift + drag fills or erases a rectangle",
    )]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    self.map = load::<TileMap>(ctx, PLATFORMER);
    let Some(map) = self.map.clone() else {
      return;
    };
    if let Some(data) = asset(ctx, &map) {
      self.bounds = map_rect(&data, Vec2::ZERO);
      clear_to(ctx, data.background.unwrap_or(Color::rgb(127, 240, 165)));
    }
    self.root = Some(
      ctx
        .world
        .spawn((Transform::IDENTITY, MapRoot::new(map)))
        .id(),
    );
    self.view.fit(ctx, self.bounds);
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    let Some(root) = self.root else {
      return;
    };
    if !ctx.maps().is_ready(root) {
      return;
    }
    if !self.ready {
      self.brush(ctx, ONE, true);
      for x in ROW.0.x..=ROW.1.x {
        self.brush(ctx, IVec2::new(x, ROW.0.y), true);
      }
      self.ready = true;
    }
    let (mx, my) = ctx.input.mouse_position();
    let at = ctx.camera().screen_to_world(Vec2::new(mx, my));
    self.hover = ctx.maps().world_to_cell(root, LAYER, at);
    let input = &*ctx.input;
    let shift = input.key_down(KeyCode::ShiftLeft) || input.key_down(KeyCode::ShiftRight);
    let (left, right) = (
      input.mouse_down(MouseButton::Left),
      input.mouse_down(MouseButton::Right),
    );
    let pressed = input.mouse_pressed(MouseButton::Left) || input.mouse_pressed(MouseButton::Right);
    let released =
      input.mouse_released(MouseButton::Left) || input.mouse_released(MouseButton::Right);

    if let Some((start, paint)) = self.drag {
      if released || !(left || right) {
        if let Some(end) = self.hover {
          self.fill(ctx, start, end, paint);
        }
        self.drag = None;
      }
    } else if shift && pressed {
      self.drag = self.hover.map(|cell| (cell, left));
    } else if (left || right)
      && let Some(cell) = self.hover
      && in_sky(cell)
      && self.last != Some(cell)
    {
      if self.brush(ctx, cell, left) {
        if left {
          self.painted += 1;
        } else {
          self.erased += 1;
        }
      }
      self.last = Some(cell);
    }
    if !(left || right) {
      self.last = None;
    }

    // The sky the brush keeps to, the cell under the pointer, a drag's
    // rectangle
    let mut debug = ctx.debug_draw();
    let (centre, size) = cells_rect(BRUSH_MIN, BRUSH_MAX);
    debug.rect(centre, size, Color::new(255, 255, 255, 70));
    if let Some(cell) = self.hover {
      let color = if in_sky(cell) {
        Color::WHITE
      } else {
        Color::rgb(255, 90, 90)
      };
      let (centre, size) = cells_rect(cell, cell);
      debug.width(2.0).rect(centre, size, color);
      if let Some((start, paint)) = self.drag {
        let (centre, size) = cells_rect(
          start.max(BRUSH_MIN).min(BRUSH_MAX),
          cell.max(BRUSH_MIN).min(BRUSH_MAX),
        );
        let color = if paint { HOT } else { Color::rgb(255, 90, 90) };
        debug.width(2.0).rect(centre, size, color);
      }
    }
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    self.view.measure(ui);
    self.view.fit(ctx, self.bounds);
    let area = self.view.area;
    let top = vec![
      (
        "A corner set over art drawn on cell edges: a painted cell is the middle".to_string(),
        Color::WHITE,
      ),
      (
        "of a 3 x 3 block, so ground grows at least two cells thick".to_string(),
        Color::WHITE,
      ),
      (
        "The brush keeps to the open sky (outlined): the slopes, ledge and bridge are not in the set"
          .to_string(),
        DIM,
      ),
    ];
    hud(ui, area.x + 4.0, area.y + 4.0, &top);
    if self.ready {
      let one = (ONE.as_vec2() + Vec2::new(0.5, -1.0)) * CELL;
      label_at(ui, ctx, one, "one cell painted", HOT);
      let row =
        (Vec2::new((ROW.0.x + ROW.1.x) as f32 / 2.0, ROW.0.y as f32) + Vec2::new(0.5, -1.0)) * CELL;
      label_at(ui, ctx, row, "a row of six", HOT);
    }
    let hover = self
      .hover
      .map_or_else(|| "-".to_string(), |c| format!("{}, {}", c.x, c.y));
    let lines = vec![(
      format!(
        "Cell {hover}   painted {}, erased {}, rectangles {}",
        self.painted, self.erased, self.filled
      ),
      DIM,
    )];
    hud(ui, area.x + 4.0, area.bottom() - 30.0, &lines);
  }
}
