//! What the pages share: the files they load, the ground tile rule, the
//! view each frames on screen (on whole screen pixels, so the pixel art
//! stays crisp), panning input, the HUD's boxes and labels, and on the other
//! grids the cell under the pointer and its outline

use demo_kit::prelude::*;

/// Pixel Platformer as a tileset: its terrain solid, the planks and the
/// rope bridge one-way, the coin a class tile
pub(crate) const TILESET: &str = "tilesets/platformer.tsx";
/// Tiny Dungeon's sheet, where the runner's sprite comes from
const HEROES: &str = "kenney-tiny-dungeon/tilemap_packed.png";
/// A cell's side in pixels (the tileset's tiles)
pub(crate) const CELL: f32 = 18.0;

/// Labels and hints
pub(crate) const DIM: Color = Color::rgb(170, 170, 185);
/// A value that matters now
pub(crate) const HOT: Color = Color::XP_GOLD;
/// Something that went well
pub(crate) const GOOD: Color = Color::rgb(120, 230, 140);
/// A path a character can walk, and the dark edge it is drawn on
pub(crate) const PATH: Color = Color::rgb(110, 200, 255);
const PATH_EDGE: Color = Color::rgb(18, 38, 66);
/// The HUD's backing
pub(crate) const BACKING: Color = Color::new(16, 16, 28, 235);

/// `path` loaded at once, or `None` if it failed (the registry warns)
pub(crate) fn load<T: Asset>(ctx: &mut SceneContext, path: &str) -> Option<Handle<T>> {
  ctx.assets().load_now(path).ok()
}

/// The Pixel Platformer tile for a ground cell, from which of the four cells
/// beside it are ground: the sheet's rows for open above and below (a one
/// cell platform), open above (a grass top), closed both ways (earth) and
/// open below (a bottom edge); in each row, the tiles for open on both
/// sides, open on the left, closed both sides, and open on the right. The
/// meadow's ground was laid by the same rule (`scripts/make-maps.js`)
pub(crate) fn ground_tile(up: bool, down: bool, left: bool, right: bool) -> u32 {
  let row = match (up, down) {
    (false, false) => 0,
    (false, true) => 20,
    (true, true) => 120,
    (true, false) => 140,
  };
  let column = match (left, right) {
    (false, false) => 0,
    (false, true) => 1,
    (true, true) => 2,
    (true, false) => 3,
  };
  row + column
}

/// The WASD row of a page's controls
pub(crate) fn wasd(what: &str) -> Control {
  Control::keys(
    "WASD",
    &[KeyCode::KeyW, KeyCode::KeyA, KeyCode::KeyS, KeyCode::KeyD],
    what,
  )
}

/// A character's keys: WASD moves (the arrows belong to the settings
/// panel), Space jumps, and the first pad's left stick or D-pad moves and
/// South jumps
pub(crate) fn bindings() -> MoveBindings {
  MoveBindings::default()
    .with_left([KeyCode::KeyA])
    .with_right([KeyCode::KeyD])
    .with_up([KeyCode::KeyW])
    .with_down([KeyCode::KeyS])
}

/// Where WASD and the first pad's sticks point, each axis -1 to 1 (+Y
/// down); the arrows belong to the settings panel
pub(crate) fn pan_input(input: &Input) -> Vec2 {
  let key = |k: KeyCode| f32::from(u8::from(input.key_down(k)));
  let (lx, ly) = input.left_stick();
  let (rx, ry) = input.right_stick();
  let keys = Vec2::new(
    key(KeyCode::KeyD) - key(KeyCode::KeyA),
    key(KeyCode::KeyS) - key(KeyCode::KeyW),
  );
  (keys + Vec2::new(lx + rx, ly + ry)).clamp_length_max(1.0)
}

/// Where a page's world shows on screen, and the camera that frames it
/// there. The area is the content area's left 55% on a page with settings
/// (the panel's column covers the right); a page sets it from its `ui` each
/// frame, and moves the camera in `update`
#[derive(Debug, Clone, Copy)]
pub(crate) struct View {
  pub(crate) area: Rect,
  settings: bool,
}

impl View {
  /// The view of a page with or without settings, until its first `ui`
  /// measures the content area (this is what 960 x 600 gives with a
  /// one-line legend)
  pub(crate) fn new(settings: bool) -> View {
    let area = if settings {
      Rect::new(9.0, 70.0, 519.0, 495.0)
    } else {
      Rect::new(9.0, 70.0, 942.0, 495.0)
    };
    View { area, settings }
  }

  /// Takes this frame's content area
  pub(crate) fn measure(&mut self, ui: &UiContext) {
    let area = content_area(ui);
    self.area = if self.settings {
      let right = ui.screen_size().0 as f32 * 0.55;
      Rect::new(area.x, area.y, (right - area.x).max(1.0), area.height)
    } else {
      area
    };
  }

  /// The camera centre that puts world point `focus` at the area's centre
  /// at `zoom`
  pub(crate) fn centre_for(&self, ctx: &SceneContext, focus: Vec2, zoom: f32) -> Vec2 {
    let (w, h) = ctx.screen_size();
    let (cx, cy) = self.area.center();
    focus - (Vec2::new(cx, cy) - Vec2::new(w, h) / 2.0) / zoom
  }

  /// Puts the camera's centre at `centre` at `zoom`, rounded to whole
  /// screen pixels so every tile edge lands on one: `jump` goes there
  /// without blending from where it was
  pub(crate) fn place(&self, ctx: &mut SceneContext, centre: Vec2, zoom: f32, jump: bool) {
    let centre = (centre * zoom).round() / zoom;
    let mut camera = ctx.camera();
    camera.set_zoom(zoom);
    if jump {
      camera.snap_to(centre);
    } else {
      camera.set_position(centre.x, centre.y);
    }
  }

  /// The screen's half size in world pixels at `zoom`
  pub(crate) fn half_screen(ctx: &SceneContext, zoom: f32) -> Vec2 {
    let (w, h) = ctx.screen_size();
    Vec2::new(w, h) / (2.0 * zoom)
  }
}

/// Clears the view to `color` (the map's background) instead of the
/// frame's; the page's camera reset puts it back
pub(crate) fn clear_to(ctx: &mut SceneContext, color: Color) {
  let camera = ctx.camera().entity();
  if let Some(mut camera) = ctx.world.get_mut::<Camera2d>(camera) {
    camera.clear = CameraClear::Color(color);
  }
}

/// One 16 px hero from Tiny Dungeon's sheet (`id` counts across its rows of
/// twelve), cut out as an image of its own
pub(crate) fn hero(ctx: &mut SceneContext, id: u32) -> Option<Handle<Image>> {
  const SIDE: u32 = 16;
  const COLUMNS: u32 = 12;
  let sheet = load::<Image>(ctx, HEROES)?;
  let cut = {
    let assets = ctx.assets();
    let sheet = assets.get(&sheet)?;
    let (x0, y0) = ((id % COLUMNS) * SIDE, (id / COLUMNS) * SIDE);
    if x0 + SIDE > sheet.width() || y0 + SIDE > sheet.height() {
      return None;
    }
    let row_bytes = sheet.width() as usize * 4;
    let mut data = Vec::with_capacity((SIDE * SIDE * 4) as usize);
    for y in y0..y0 + SIDE {
      let start = y as usize * row_bytes + x0 as usize * 4;
      data.extend_from_slice(&sheet.data()[start..start + SIDE as usize * 4]);
    }
    Image::from_rgba(data, SIDE, SIDE)
  };
  Some(ctx.assets().add(cut))
}

/// A HUD line's height, and the box's padding
const LINE: f32 = 18.0;
const PAD: f32 = 6.0;

/// A dark box behind `lines` from (`x`, `y`), one line each, at least
/// `width` wide; returns the box
pub(crate) fn hud(
  ui: &mut UiContext,
  x: f32,
  y: f32,
  width: f32,
  lines: &[(String, Color)],
) -> Rect {
  if lines.is_empty() {
    return Rect::new(x, y, 0.0, 0.0);
  }
  let widest = lines
    .iter()
    .map(|(text, _)| ui.text_size(text).0 as f32)
    .fold(0.0, f32::max);
  let height = lines.len() as f32 * LINE + 2.0 * PAD - 4.0;
  let rect = Rect::new(x, y, (widest + 2.0 * PAD).max(width), height);
  ui.draw_rect(rect, BACKING);
  for (i, (text, color)) in lines.iter().enumerate() {
    ui.draw_text(text, x + PAD, y + PAD + i as f32 * LINE, *color);
  }
  rect
}

/// The settings column under a panel of `rows` rows: the room a page with
/// settings has for its notes, clear of its view (its left, top and width)
pub(crate) fn beside_panel(ui: &UiContext, rows: usize) -> Rect {
  let area = content_area(ui);
  let x = ui.screen_size().0 as f32 * 0.55 + 8.0;
  let y = area.y + settings_panel_height(ui, rows) + 10.0;
  Rect::new(x, y, area.right() - x, area.bottom() - y)
}

/// Draws `outline` (a cell's, in the world) with `debug`, `width` screen
/// pixels wide
pub(crate) fn draw_outline(debug: &mut DebugDraw, outline: &CellOutline, color: Color, width: f32) {
  let points = outline.points();
  let Some(&first) = points.first() else {
    return;
  };
  let mut closed = [first; 7];
  closed[..points.len()].copy_from_slice(points);
  closed[points.len()] = first;
  debug.width(width).polyline(&closed[..=points.len()], color);
}

/// Draws a path through `points` (from `from`, if given) with `debug`,
/// `width` screen pixels wide, on a dark edge so it reads on any ground
pub(crate) fn draw_path(debug: &mut DebugDraw, from: Option<Vec2>, points: &[Vec2], width: f32) {
  let Some(&first) = points.first() else {
    return;
  };
  for (color, width) in [(PATH_EDGE, width + 3.0), (PATH, width)] {
    let mut pen = debug.width(width);
    if let Some(from) = from {
      pen.line(from, first, color);
    }
    if points.len() > 1 {
      pen.polyline(points, color);
    }
  }
}

/// The cell under the pointer on `layer` of `map`, unless the pointer is
/// over one of `covered` (the page's notes) or off the layer's cells
/// (`bounds`, on a bounded map)
pub(crate) fn hovered_cell(
  ctx: &mut SceneContext,
  map: Entity,
  layer: &str,
  covered: &[Rect],
) -> Option<IVec2> {
  let (mx, my) = ctx.input.mouse_position();
  if covered.iter().any(|rect| rect.contains(mx, my)) {
    return None;
  }
  let world = ctx.camera().screen_to_world(Vec2::new(mx, my));
  let maps = ctx.maps();
  let cell = maps.grid(map, layer)?.cell_at_world(world);
  maps.bounds(map)?.contains(cell).then_some(cell)
}
