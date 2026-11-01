//! The chunk overlay the pages share. The engine keeps a tile layer's cells
//! in chunks of 32 x 32 (`CHUNK_SIZE`), made where tiles are first written;
//! the overlay outlines the chunks a map's tile layers have, labels them
//! with their coordinates in world text, and draws them on a minimap with the map's
//! bounds and the view. It reads only what a game can: each layer entity's
//! `TileLayerData` grid

use demo_kit::prelude::*;

use crate::shared::{BACKING, DIM, GOOD, HOT};

/// A chunk's label, and the light line inside its outline
pub(crate) const CHUNK_LINE: Color = Color::new(255, 255, 255, 235);
/// The dark line under a chunk's outline, so it shows over sky and earth
const CHUNK_EDGE: Color = Color::new(20, 30, 70, 200);
/// The map's bounds
pub(crate) const BOUNDS_LINE: Color = HOT;
/// A chunk on the minimap
const CHUNK_FILL: Color = Color::new(200, 225, 255, 210);
/// A chunk just made
pub(crate) const FRESH: Color = Color::rgb(255, 120, 90);

/// A chunk's side in cells
const SIDE: i32 = CHUNK_SIZE as i32;

/// The chunks the tile layers of `map` have, each once, by row then column,
/// into `out`
pub(crate) fn map_chunks(world: &mut World, map: Entity, out: &mut Vec<IVec2>) {
  out.clear();
  let mut layers = world.query::<(&MapLayerRef, &TileLayerData)>();
  for (layer, data) in layers.iter(world) {
    if layer.map == map {
      out.extend(data.grid.chunk_coords());
    }
  }
  out.sort_unstable_by_key(|chunk| (chunk.y, chunk.x));
  out.dedup();
}

/// Where cells sit in the world: cell (0, 0)'s top-left, and a cell's side
#[derive(Debug, Clone, Copy)]
pub(crate) struct Grid {
  pub(crate) origin: Vec2,
  pub(crate) cell: f32,
}

impl Grid {
  /// The grid of `layer` of `map`, once it has landed
  pub(crate) fn of(ctx: &mut SceneContext, map: Entity, layer: &str, cell: f32) -> Option<Grid> {
    let centre = ctx.maps().cell_to_world(map, layer, IVec2::ZERO)?;
    Some(Grid {
      origin: centre - Vec2::splat(cell / 2.0),
      cell,
    })
  }

  /// A world point in cells (fractional)
  pub(crate) fn to_cells(self, world: Vec2) -> Vec2 {
    (world - self.origin) / self.cell
  }

  /// The world rectangle of cells `min` (included) to `max` (not)
  pub(crate) fn rect(self, min: IVec2, max: IVec2) -> Rect {
    let a = self.origin + min.as_vec2() * self.cell;
    let b = self.origin + max.as_vec2() * self.cell;
    Rect::new(a.x, a.y, b.x - a.x, b.y - a.y)
  }

  /// A chunk's world rectangle
  pub(crate) fn chunk(self, chunk: IVec2) -> Rect {
    self.rect(chunk * SIDE, (chunk + IVec2::ONE) * SIDE)
  }
}

/// Outlines a chunk's world rectangle `rect` with `debug`: a light line over
/// a dark one, clear over the sky, the grass and the earth at any zoom
pub(crate) fn chunk_outline(debug: &mut DebugDraw, rect: Rect) {
  outline(debug, rect, CHUNK_EDGE, 3.0);
  outline(debug, rect, CHUNK_LINE, 1.0);
}

/// Outlines `rect` (in world pixels) with `debug`, `width` screen pixels wide
pub(crate) fn outline(debug: &mut DebugDraw, rect: Rect, color: Color, width: f32) {
  let (x, y) = rect.center();
  debug
    .width(width)
    .rect(Vec2::new(x, y), Vec2::new(rect.width, rect.height), color);
}

/// A chunk label, world text at a chunk's top-left corner
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkLabel {
  /// The chunk it names
  pub chunk: IVec2,
}

/// The labels' z: over the map's layers and what walks on them
const LABEL_Z: i32 = 50;
/// A label's text height, and the gap from its chunk's corner, in screen
/// pixels at every zoom
const LABEL_SIZE: f32 = 14.0;
const LABEL_GAP: f32 = 4.0;

/// The chunk labels: world text (`Text2d`) at the top-left corner of each
/// chunk in view, naming it, kept inside the page's content area while any
/// of the chunk shows there. As world text they draw with the tiles, the
/// camera's movement between fixed steps included, so they move exactly
/// with the world; they are placed in the fixed step, after the camera
#[derive(Debug, Default)]
pub(crate) struct ChunkLabels {
  /// The content area on screen, from the last `ui`
  area: Option<Rect>,
  labels: Vec<(IVec2, Entity)>,
  /// The chunks in view this step and where their labels go
  seen: Vec<(IVec2, Vec2)>,
  /// The zoom the labels' text is sized for
  zoom: f32,
}

impl ChunkLabels {
  /// Takes this frame's content area
  pub(crate) fn measure(&mut self, ui: &UiContext) {
    self.area = Some(content_area(ui));
  }

  /// Labels each of `chunks` in view (none when `show` is off); call it
  /// after moving the camera this step
  pub(crate) fn place(&mut self, ctx: &mut SceneContext, grid: Grid, chunks: &[IVec2], show: bool) {
    self.seen.clear();
    let zoom = ctx.camera().zoom();
    if let (true, Some(area)) = (show, self.area) {
      let camera = ctx.camera();
      let top_left = camera.screen_to_world(Vec2::new(area.x, area.y));
      let bottom_right = camera.screen_to_world(Vec2::new(area.right(), area.bottom()));
      // On whole screen pixels: the camera is, and so are a chunk's corner
      // and the area's
      let gap = Vec2::splat(LABEL_GAP / zoom);
      for &chunk in chunks {
        let rect = grid.chunk(chunk);
        let seen = rect.right() > top_left.x
          && rect.x < bottom_right.x
          && rect.bottom() > top_left.y
          && rect.y < bottom_right.y;
        if seen {
          let corner = Vec2::new(rect.x.max(top_left.x), rect.y.max(top_left.y));
          self.seen.push((chunk, corner + gap));
        }
      }
    }
    let seen = &self.seen;
    self.labels.retain(|&(chunk, entity)| {
      let keep = seen.iter().any(|&(c, _)| c == chunk);
      if !keep {
        ctx.world.despawn(entity);
      }
      keep
    });
    let resized = zoom != self.zoom;
    self.zoom = zoom;
    let style = TextStyle::new()
      .size(LABEL_SIZE / zoom)
      .color(CHUNK_LINE)
      .outline(2.0 / zoom, CHUNK_EDGE);
    for &(chunk, at) in &self.seen {
      match self.labels.iter().find(|(c, _)| *c == chunk) {
        Some(&(_, entity)) => {
          if let Some(mut transform) = ctx.world.get_mut::<Transform>(entity) {
            transform.translation = at;
          }
          if resized && let Some(mut text) = ctx.world.get_mut::<Text2d>(entity) {
            text.style = style.clone();
          }
        }
        None => {
          let entity = ctx
            .world
            .spawn((
              Transform::from_translation(at),
              Interpolated,
              Text2d::new(format!("chunk {}, {}", chunk.x, chunk.y))
                .with_style(style.clone())
                .with_layout(TextLayoutOptions::default().anchor(Anchor::TopLeft))
                .with_z(LABEL_Z),
              ChunkLabel { chunk },
            ))
            .id();
          self.labels.push((chunk, entity));
        }
      }
    }
  }
}

/// What a minimap shows, in cells
pub(crate) struct Minimap<'a> {
  /// The line over it, saying what its colours are
  pub(crate) caption: &'a str,
  /// The chunks there are
  pub(crate) chunks: &'a [IVec2],
  /// Chunks just made, each with how fresh it is (1 new, 0 old)
  pub(crate) fresh: &'a [(IVec2, f32)],
  /// The map's bounds, outlined (`None` for none)
  pub(crate) bounds: Option<CellRect>,
  /// The camera's view
  pub(crate) view: Rect,
  /// A point to mark: the runner, the ball
  pub(crate) dot: Option<Vec2>,
}

/// The largest square a chunk gets on the minimap
const MOST: f32 = 34.0;

/// Draws `map` in `area` (screen pixels), framing its chunks, bounds and
/// view, a square a chunk; returns the box it drew, under a caption
pub(crate) fn minimap(ui: &mut UiContext, area: Rect, map: &Minimap) -> Rect {
  // The chunks to frame: every chunk, the bounds and the view
  let view_min = (Vec2::new(map.view.x, map.view.y) / SIDE as f32)
    .floor()
    .as_ivec2();
  let view_max = (Vec2::new(map.view.right(), map.view.bottom()) / SIDE as f32)
    .ceil()
    .as_ivec2()
    - IVec2::ONE;
  let (mut from, mut to) = (view_min, view_max);
  for &chunk in map.chunks {
    from = from.min(chunk);
    to = to.max(chunk);
  }
  if let Some(bounds) = map.bounds.filter(|b| !b.is_empty()) {
    from = from.min(bounds.min.div_euclid(IVec2::splat(SIDE)));
    to = to.max((bounds.max - IVec2::ONE).div_euclid(IVec2::splat(SIDE)));
  }
  let span = (to - from + IVec2::ONE).as_vec2();
  let caption = 18.0;
  let room = Vec2::new(area.width - 12.0, area.height - caption - 12.0);
  let square = (room.x / span.x).min(room.y / span.y).clamp(1.0, MOST);
  let size = span * square;
  let wide = (ui.text_size(map.caption).0 as f32).max(size.x);
  let frame = Rect::new(area.x, area.y, wide + 12.0, size.y + caption + 12.0);
  ui.draw_rect(frame, BACKING);
  ui.draw_text(map.caption, frame.x + 6.0, frame.y + 4.0, DIM);
  // The chunks centred under the caption
  let top_left = Vec2::new(
    frame.x + 6.0 + (wide - size.x) / 2.0,
    frame.y + caption + 6.0,
  );
  // A cell position on the minimap
  let place = |cells: Vec2| top_left + (cells / SIDE as f32 - from.as_vec2()) * square;
  let gap = if square >= 6.0 { 1.0 } else { 0.0 };
  for &chunk in map.chunks {
    let at = place((chunk * SIDE).as_vec2());
    ui.draw_rect(
      Rect::new(
        at.x + gap,
        at.y + gap,
        (square - 2.0 * gap).max(1.0),
        (square - 2.0 * gap).max(1.0),
      ),
      CHUNK_FILL,
    );
  }
  for &(chunk, fresh) in map.fresh {
    let at = place((chunk * SIDE).as_vec2());
    let color = FRESH.with_alpha((fresh.clamp(0.0, 1.0) * 255.0) as u8);
    ui.draw_rect(Rect::new(at.x, at.y, square, square), color);
  }
  if let Some(bounds) = map.bounds.filter(|b| !b.is_empty()) {
    let a = place(bounds.min.as_vec2());
    let b = place(bounds.max.as_vec2());
    ui.draw_rect_outline(Rect::new(a.x, a.y, b.x - a.x, b.y - a.y), BOUNDS_LINE, 1.0);
  }
  let a = place(Vec2::new(map.view.x, map.view.y));
  let b = place(Vec2::new(map.view.right(), map.view.bottom()));
  ui.draw_rect_outline(Rect::new(a.x, a.y, b.x - a.x, b.y - a.y), Color::WHITE, 1.0);
  // The origin, cell (0, 0)
  let origin = place(Vec2::ZERO);
  ui.draw_rect(Rect::new(origin.x - 1.0, origin.y - 3.0, 2.0, 6.0), DIM);
  ui.draw_rect(Rect::new(origin.x - 3.0, origin.y - 1.0, 6.0, 2.0), DIM);
  if let Some(dot) = map.dot {
    let at = place(dot);
    ui.draw_rect(Rect::new(at.x - 2.0, at.y - 2.0, 4.0, 4.0), GOOD);
  }
  frame
}
