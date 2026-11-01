//! The chunk overlay the pages share. The engine keeps a tile layer's cells
//! in chunks of 32 x 32 (`CHUNK_SIZE`), made where tiles are first written
//! or, on a streamed map, as the stream lands them; the overlay outlines
//! the chunks a map's tile layers have, labels them with their coordinates
//! (and, on a streamed map, what they are) in world text, and draws them on
//! a minimap with the map's bounds and the view. It reads only what a game
//! can: each layer entity's `TileLayerData` grid, and the `ChunkEvent`s a
//! stream sends

use demo_kit::prelude::*;

use crate::shared::{BACKING, DIM, GOOD, HOT};

/// A chunk's label, and the light line inside its outline
pub(crate) const CHUNK_LINE: Color = Color::new(255, 255, 255, 235);
/// The dark line under a chunk's outline, so it shows over sky and earth
const CHUNK_EDGE: Color = Color::new(20, 30, 70, 200);
/// The map's bounds
pub(crate) const BOUNDS_LINE: Color = HOT;
/// A chunk on the minimap
pub(crate) const CHUNK_FILL: Color = Color::new(200, 225, 255, 210);
/// A chunk a stream generated: its outline, label and square on the minimap
pub(crate) const GENERATED: Color = Color::rgb(150, 240, 150);
/// A chunk just made
pub(crate) const FRESH: Color = Color::rgb(255, 120, 90);
/// Seconds a chunk just landed glows
const FLASH: f32 = 1.5;

/// A chunk's side in cells
const SIDE: i32 = CHUNK_SIZE as i32;

/// The chunks the tile layers of `map` have, each once, by row then column,
/// into `out`: on a streamed map, the chunks resident
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

/// Whether `chunk` is in `chunks`, sorted by row then column
pub(crate) fn has_chunk(chunks: &[IVec2], chunk: IVec2) -> bool {
  let key = |c: &IVec2| (c.y, c.x);
  chunks.binary_search_by_key(&key(&chunk), key).is_ok()
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

  /// The chunks a world rectangle from `min` to `max` overlaps, first and
  /// last, as a stream counts them
  fn chunks_over(self, min: Vec2, max: Vec2) -> (IVec2, IVec2) {
    let side = SIDE as f32;
    let a = self.to_cells(min) / side;
    let b = self.to_cells(max) / side;
    (a.floor().as_ivec2(), (b.ceil() - Vec2::ONE).as_ivec2())
  }
}

/// Outlines a chunk's world rectangle `rect` with `debug`: a light line over
/// a dark one, clear over the sky, the grass and the earth at any zoom
pub(crate) fn chunk_outline(debug: &mut DebugDraw, rect: Rect) {
  chunk_outline_in(debug, rect, CHUNK_LINE);
}

/// Outlines a chunk's world rectangle in `color` over the dark line
pub(crate) fn chunk_outline_in(debug: &mut DebugDraw, rect: Rect, color: Color) {
  outline(debug, rect, CHUNK_EDGE, 3.0);
  outline(debug, rect, color, 1.0);
}

/// Outlines `rect` (in world pixels) with `debug`, `width` screen pixels wide
pub(crate) fn outline(debug: &mut DebugDraw, rect: Rect, color: Color, width: f32) {
  let (x, y) = rect.center();
  debug
    .width(width)
    .rect(Vec2::new(x, y), Vec2::new(rect.width, rect.height), color);
}

/// What a streamed map has, as a game reads it: the chunks resident (its
/// layers' chunks), those landed just now, how many landed and unloaded
/// since the page opened (`ChunkEvent`s), and how many chunks the stream
/// wants and has not landed. The engine keeps no count of its requests a
/// game can read, so that last is worked out here as the stream works it
/// out: the chunks within its load distance of the camera's view or of an
/// anchor, along each axis, not resident
#[derive(Debug, Default)]
pub(crate) struct Streamed {
  /// The chunks resident, by row then column
  pub(crate) chunks: Vec<IVec2>,
  /// Chunks landed just now, each with how long it has left to glow
  pub(crate) fresh: Vec<(IVec2, f32)>,
  pub(crate) landed: usize,
  pub(crate) unloaded: usize,
  /// Chunks wanted and not landed: asked of the generator, or waiting to
  /// land; and the most there have been at once
  pub(crate) in_flight: usize,
  pub(crate) most_in_flight: usize,
}

/// Where a stream loads round: its anchors (world points; the camera's
/// view is one too), and its load distance
pub(crate) struct Loading<'a> {
  pub(crate) anchors: &'a [Vec2],
  pub(crate) distance: f32,
}

impl Streamed {
  /// This step's look at `map`, with the chunk events read this step
  pub(crate) fn update(
    &mut self,
    ctx: &mut SceneContext,
    map: Entity,
    grid: Grid,
    events: &[ChunkEvent],
    loading: Loading,
    dt: f32,
  ) {
    map_chunks(ctx.world, map, &mut self.chunks);
    for (_, left) in &mut self.fresh {
      *left -= dt;
    }
    for event in events {
      match event {
        ChunkEvent::Loaded { map: m, chunk } if *m == map => {
          self.landed += 1;
          self.fresh.retain(|(c, _)| c != chunk);
          self.fresh.push((*chunk, FLASH));
        }
        ChunkEvent::Unloading { map: m, .. } if *m == map => self.unloaded += 1,
        _ => {}
      }
    }
    let chunks = &self.chunks;
    self
      .fresh
      .retain(|(chunk, left)| *left > 0.0 && has_chunk(chunks, *chunk));

    // The areas the stream loads: the view's and each anchor's, grown by
    // the load distance
    let view = ctx.camera().visible_bounds();
    let load = loading.distance;
    let mut areas = vec![grid.chunks_over(
      Vec2::new(view.left, view.top) - load,
      Vec2::new(view.right, view.bottom) + load,
    )];
    areas.extend(
      loading
        .anchors
        .iter()
        .map(|&at| grid.chunks_over(at - load, at + load)),
    );
    let (mut from, mut to) = areas[0];
    for &(a, b) in &areas {
      from = from.min(a);
      to = to.max(b);
    }
    let wanted = |chunk: IVec2| {
      areas
        .iter()
        .any(|&(a, b)| chunk.cmpge(a).all() && chunk.cmple(b).all())
    };
    self.in_flight = (from.y..=to.y)
      .flat_map(|y| (from.x..=to.x).map(move |x| IVec2::new(x, y)))
      .filter(|&chunk| wanted(chunk) && !has_chunk(chunks, chunk))
      .count();
    self.most_in_flight = self.most_in_flight.max(self.in_flight);
  }

  /// How fresh `chunk` is: 1 just landed, 0 settled
  pub(crate) fn freshness(&self) -> Vec<(IVec2, f32)> {
    self
      .fresh
      .iter()
      .map(|&(chunk, left)| (chunk, left / FLASH))
      .collect()
  }
}

/// A chunk label, world text at a chunk's top-left corner
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkLabel {
  /// The chunk it names
  pub chunk: IVec2,
}

/// What a label says after a chunk's coordinates, and its colour
pub(crate) type Tag = Option<(&'static str, Color)>;

/// The labels' z: over the map's layers and what walks on them
const LABEL_Z: i32 = 50;
/// A label's text height, and the gap from its chunk's corner, in screen
/// pixels at every zoom
const LABEL_SIZE: f32 = 14.0;
const LABEL_GAP: f32 = 4.0;
/// The room a label takes, in screen pixels: a chunk showing less of
/// itself than this is left unlabelled, so its label never lies over its
/// neighbour's
const LABEL_ROOM: Vec2 = Vec2::new(200.0, 24.0);

/// The chunk labels: world text (`Text2d`) at the top-left corner of each
/// chunk in view, naming it (and saying what it is, where the page tells:
/// hand-made, generated), kept inside the page's content area while enough
/// of the chunk shows there for its label. As world text they draw with the tiles, the
/// camera's movement between fixed steps included, so they move exactly
/// with the world; they are placed in the fixed step, after the camera
#[derive(Debug, Default)]
pub(crate) struct ChunkLabels {
  /// The content area on screen, from the last `ui`
  area: Option<Rect>,
  labels: Vec<(IVec2, Entity, Tag)>,
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

  /// Labels each of `chunks` in view (none when `show` is off), each
  /// tagged by `tag`; call it after moving the camera this step
  pub(crate) fn place(
    &mut self,
    ctx: &mut SceneContext,
    grid: Grid,
    chunks: &[IVec2],
    show: bool,
    tag: &dyn Fn(IVec2) -> Tag,
  ) {
    self.seen.clear();
    let zoom = ctx.camera().zoom();
    if let (true, Some(area)) = (show, self.area) {
      let camera = ctx.camera();
      let top_left = camera.screen_to_world(Vec2::new(area.x, area.y));
      let bottom_right = camera.screen_to_world(Vec2::new(area.right(), area.bottom()));
      // On whole screen pixels: the camera is, and so are a chunk's corner
      // and the area's
      let gap = Vec2::splat(LABEL_GAP / zoom);
      let room = LABEL_ROOM / zoom;
      for &chunk in chunks {
        let rect = grid.chunk(chunk);
        let corner = Vec2::new(rect.x.max(top_left.x), rect.y.max(top_left.y));
        let shown = Vec2::new(
          rect.right().min(bottom_right.x),
          rect.bottom().min(bottom_right.y),
        ) - corner;
        // Where the corner is out of view, the label is pushed in to the
        // area's edge: only while the chunk has room for it there
        let room_x = corner.x == rect.x || shown.x >= room.x;
        let room_y = corner.y == rect.y || shown.y >= room.y;
        if shown.x > 0.0 && shown.y > 0.0 && room_x && room_y {
          self.seen.push((chunk, corner + gap));
        }
      }
    }
    // Labels of chunks gone from view, or whose tag changed, go
    let seen = &self.seen;
    self.labels.retain(|&(chunk, entity, was)| {
      let keep = seen.iter().any(|&(c, _)| c == chunk) && tag(chunk) == was;
      if !keep {
        ctx.world.despawn(entity);
      }
      keep
    });
    let resized = zoom != self.zoom;
    self.zoom = zoom;
    let style = |color: Color| {
      TextStyle::new()
        .size(LABEL_SIZE / zoom)
        .color(color)
        .outline(2.0 / zoom, CHUNK_EDGE)
    };
    for &(chunk, at) in &self.seen {
      match self.labels.iter().find(|(c, _, _)| *c == chunk) {
        Some(&(_, entity, was)) => {
          if let Some(mut transform) = ctx.world.get_mut::<Transform>(entity) {
            transform.translation = at;
          }
          if resized && let Some(mut text) = ctx.world.get_mut::<Text2d>(entity) {
            text.style = style(was.map_or(CHUNK_LINE, |(_, color)| color));
          }
        }
        None => {
          let tagged = tag(chunk);
          let (text, color) = match tagged {
            Some((what, color)) => (format!("chunk {}, {}, {what}", chunk.x, chunk.y), color),
            None => (format!("chunk {}, {}", chunk.x, chunk.y), CHUNK_LINE),
          };
          let entity = ctx
            .world
            .spawn((
              Transform::from_translation(at),
              Interpolated,
              Text2d::new(text)
                .with_style(style(color))
                .with_layout(TextLayoutOptions::default().anchor(Anchor::TopLeft))
                .with_z(LABEL_Z),
              ChunkLabel { chunk },
            ))
            .id();
          self.labels.push((chunk, entity, tagged));
        }
      }
    }
  }
}

/// What a minimap shows, in cells
pub(crate) struct Minimap<'a> {
  /// The line over it, saying what its colours are
  pub(crate) caption: &'a str,
  /// The chunks there are, by row then column
  pub(crate) chunks: &'a [IVec2],
  /// Of those, the ones a stream generated (by row then column), drawn in
  /// their own colour
  pub(crate) generated: &'a [IVec2],
  /// Chunks just made, each with how fresh it is (1 new, 0 old)
  pub(crate) fresh: &'a [(IVec2, f32)],
  /// Chunks to outline in gold wherever they are: the ones kept for later
  pub(crate) marked: &'a [IVec2],
  /// The map's bounds, outlined (`None` for none)
  pub(crate) bounds: Option<CellRect>,
  /// The camera's view
  pub(crate) view: Rect,
  /// A point to mark: the runner, the ball
  pub(crate) dot: Option<Vec2>,
  /// Frame this many chunks a side round the view's middle rather than
  /// every chunk: an endless world's minimap follows the view
  pub(crate) window: Option<IVec2>,
}

/// The largest square a chunk gets on the minimap
const MOST: f32 = 34.0;

/// `rect` cut to `within`, if any of it is left
fn clip(rect: Rect, within: Rect) -> Option<Rect> {
  let (x, y) = (rect.x.max(within.x), rect.y.max(within.y));
  let (right, bottom) = (
    rect.right().min(within.right()),
    rect.bottom().min(within.bottom()),
  );
  (right > x && bottom > y).then(|| Rect::new(x, y, right - x, bottom - y))
}

/// Draws `map` in `area` (screen pixels), framing its chunks, bounds and
/// view (or its window round the view), a square a chunk; returns the box
/// it drew, under a caption
pub(crate) fn minimap(ui: &mut UiContext, area: Rect, map: &Minimap) -> Rect {
  let side = SIDE as f32;
  let view_min = (Vec2::new(map.view.x, map.view.y) / side)
    .floor()
    .as_ivec2();
  let view_max = (Vec2::new(map.view.right(), map.view.bottom()) / side)
    .ceil()
    .as_ivec2()
    - IVec2::ONE;
  let (from, to) = match map.window {
    // The window's chunks, centred on the view's middle
    Some(window) => {
      let (cx, cy) = map.view.center();
      let middle = (Vec2::new(cx, cy) / side).floor().as_ivec2();
      let from = middle - window / 2;
      (from, from + window - IVec2::ONE)
    }
    // Every chunk, the bounds and the view
    None => {
      let (mut from, mut to) = (view_min, view_max);
      for &chunk in map.chunks {
        from = from.min(chunk);
        to = to.max(chunk);
      }
      if let Some(bounds) = map.bounds.filter(|b| !b.is_empty()) {
        from = from.min(bounds.min.div_euclid(IVec2::splat(SIDE)));
        to = to.max((bounds.max - IVec2::ONE).div_euclid(IVec2::splat(SIDE)));
      }
      (from, to)
    }
  };
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
  let inside = Rect::new(top_left.x, top_left.y, size.x, size.y);
  // A cell position on the minimap
  let place = |cells: Vec2| top_left + (cells / side - from.as_vec2()) * square;
  let framed = |chunk: IVec2| chunk.cmpge(from).all() && chunk.cmple(to).all();
  let gap = if square >= 6.0 { 1.0 } else { 0.0 };
  for &chunk in map.chunks {
    if !framed(chunk) {
      continue;
    }
    let at = place((chunk * SIDE).as_vec2());
    let color = if has_chunk(map.generated, chunk) {
      GENERATED
    } else {
      CHUNK_FILL
    };
    ui.draw_rect(
      Rect::new(
        at.x + gap,
        at.y + gap,
        (square - 2.0 * gap).max(1.0),
        (square - 2.0 * gap).max(1.0),
      ),
      color,
    );
  }
  for &(chunk, fresh) in map.fresh {
    if !framed(chunk) {
      continue;
    }
    let at = place((chunk * SIDE).as_vec2());
    let color = FRESH.with_alpha((fresh.clamp(0.0, 1.0) * 255.0) as u8);
    ui.draw_rect(Rect::new(at.x, at.y, square, square), color);
  }
  for &chunk in map.marked {
    if !framed(chunk) {
      continue;
    }
    let at = place((chunk * SIDE).as_vec2());
    ui.draw_rect_outline(Rect::new(at.x, at.y, square, square), HOT, 2.0);
  }
  if let Some(bounds) = map.bounds.filter(|b| !b.is_empty()) {
    let a = place(bounds.min.as_vec2());
    let b = place(bounds.max.as_vec2());
    if let Some(rect) = clip(Rect::new(a.x, a.y, b.x - a.x, b.y - a.y), inside) {
      ui.draw_rect_outline(rect, BOUNDS_LINE, 1.0);
    }
  }
  let a = place(Vec2::new(map.view.x, map.view.y));
  let b = place(Vec2::new(map.view.right(), map.view.bottom()));
  if let Some(rect) = clip(Rect::new(a.x, a.y, b.x - a.x, b.y - a.y), inside) {
    ui.draw_rect_outline(rect, Color::WHITE, 1.0);
  }
  // The origin, cell (0, 0)
  let origin = place(Vec2::ZERO);
  if inside.contains(origin.x, origin.y) {
    ui.draw_rect(Rect::new(origin.x - 1.0, origin.y - 3.0, 2.0, 6.0), DIM);
    ui.draw_rect(Rect::new(origin.x - 3.0, origin.y - 1.0, 6.0, 2.0), DIM);
  }
  if let Some(dot) = map.dot {
    let at = place(dot);
    if inside.contains(at.x, at.y) {
      ui.draw_rect(Rect::new(at.x - 2.0, at.y - 2.0, 4.0, 4.0), GOOD);
    }
  }
  frame
}
