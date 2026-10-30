//! What the pages share: the files they load, the view each frames on
//! screen, the player (a Tiny Dungeon hero on the character controller,
//! moved with WASD or a pad), an object's place in a map, the colliders'
//! outlines, and the HUD's boxes and labels

use std::f32::consts::{PI, TAU};

use demo_kit::prelude::*;

/// The side-view level: two tilesets, Wang ground, objects and a door
pub(crate) const PLATFORMER: &str = "maps/platformer.tmx";
/// The garden: y-sorted trees, parallax clouds and an image layer
pub(crate) const TOPDOWN: &str = "maps/topdown.tmj";
/// Three rooms side by side
pub(crate) const ROOMS: &str = "rooms/rooms.world";
/// The LDtk project: levels `Platformer` and `Vault`
pub(crate) const LDTK: &str = "ldtk/dungeon.ldtk";
/// Tiny Dungeon's walls and floors as the Wang set `dungeon`
pub(crate) const DUNGEON_TERRAIN: &str = "tilesets/dungeon-terrain.tsj";
/// Tiny Dungeon's sheet as a tileset: props, statues, the animated gargoyle
pub(crate) const DUNGEON: &str = "tilesets/dungeon.tsj";
/// Tiny Dungeon's sheet, where the player's sprite comes from
const HEROES: &str = "kenney-tiny-dungeon/tilemap_packed.png";

/// Labels and hints
pub(crate) const DIM: Color = Color::rgb(170, 170, 185);
/// A value that matters now
pub(crate) const HOT: Color = Color::XP_GOLD;
/// Something that went well
pub(crate) const GOOD: Color = Color::rgb(120, 230, 140);
/// One-way colliders' outlines
pub(crate) const ONE_WAY: Color = Color::rgb(255, 110, 200);
/// The HUD's backing
const BACKING: Color = Color::new(16, 16, 28, 235);

/// The player of a page that has one
#[derive(Component, Debug)]
pub struct Player;

/// `path` loaded at once, or `None` if it failed (the registry warns)
pub(crate) fn load<T: Asset>(ctx: &mut SceneContext, path: &str) -> Option<Handle<T>> {
  ctx.assets().load_now(path).ok()
}

/// A copy of `T` as the store holds it now
pub(crate) fn asset<T: Asset + Clone>(ctx: &mut SceneContext, handle: &Handle<T>) -> Option<T> {
  ctx.assets().get(handle).cloned()
}

/// WASD moves (the arrows belong to the settings panel), Space jumps, and
/// the first pad's left stick or D-pad moves and South jumps
pub(crate) fn bindings() -> MoveBindings {
  MoveBindings::default()
    .with_left([KeyCode::KeyA])
    .with_right([KeyCode::KeyD])
    .with_up([KeyCode::KeyW])
    .with_down([KeyCode::KeyS])
}

/// The WASD row of a page's controls
pub(crate) fn wasd(what: &str) -> Control {
  Control::keys(
    "WASD",
    &[KeyCode::KeyW, KeyCode::KeyA, KeyCode::KeyS, KeyCode::KeyD],
    what,
  )
}

/// The page's main action this frame: Space, or the pad's South button
pub(crate) fn main_action(input: &Input) -> bool {
  input.key_pressed(KeyCode::Space) || input.gamepad_button_pressed(GamepadButton::South)
}

/// Where a page's world shows on screen, and the camera that frames it
/// there. The area is the content area (its left 55% on a page with
/// settings, which the panel's column covers on the right); a page sets it
/// from its `ui` each frame, the camera moving in `update` or `ui`
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
  fn centre_for(&self, ctx: &SceneContext, focus: Vec2, zoom: f32) -> Vec2 {
    let (w, h) = ctx.screen_size();
    let (cx, cy) = self.area.center();
    focus - (Vec2::new(cx, cy) - Vec2::new(w, h) / 2.0) / zoom
  }

  /// The zoom that fits `world` (in world pixels) in the area: whole
  /// numbers from 1 up, quarters below, so pixel art stays even where it can
  pub(crate) fn fit_zoom(&self, world: Rect) -> f32 {
    let fit =
      (self.area.width / world.width.max(1.0)).min(self.area.height / world.height.max(1.0));
    if fit >= 1.0 {
      fit.floor()
    } else {
      ((fit * 4.0).floor() / 4.0).max(0.25)
    }
  }

  /// Frames `world` in the area, centred, at [`fit_zoom`](Self::fit_zoom)
  pub(crate) fn fit(&self, ctx: &mut SceneContext, world: Rect) {
    let zoom = self.fit_zoom(world);
    let (x, y) = world.center();
    self.look_at(ctx, Vec2::new(x, y), zoom);
  }

  /// Puts `focus` at the area's centre at `zoom`, at once
  pub(crate) fn look_at(&self, ctx: &mut SceneContext, focus: Vec2, zoom: f32) {
    let centre = self.centre_for(ctx, focus, zoom);
    let mut camera = ctx.camera();
    camera.set_zoom(zoom);
    camera.snap_to(centre);
  }

  /// Follows `focus` at `zoom`, keeping the area inside `bounds` (centred
  /// on an axis the area is wider than), and the whole screen too on an
  /// axis `bounds` fills it on, so no edge of the world shows beside the
  /// area; easing by `lerp` (1 jumps there, without blending from where it
  /// was)
  pub(crate) fn follow(
    &self,
    ctx: &mut SceneContext,
    focus: Vec2,
    zoom: f32,
    bounds: Rect,
    lerp: f32,
  ) {
    let half = Vec2::new(self.area.width, self.area.height) / (2.0 * zoom);
    let axis = |at: f32, low: f32, high: f32| {
      if low > high {
        (low + high) / 2.0
      } else {
        at.clamp(low, high)
      }
    };
    let focus = Vec2::new(
      axis(focus.x, bounds.x + half.x, bounds.right() - half.x),
      axis(focus.y, bounds.y + half.y, bounds.bottom() - half.y),
    );
    let mut centre = self.centre_for(ctx, focus, zoom);
    let (w, h) = ctx.screen_size();
    let screen = Vec2::new(w, h) / (2.0 * zoom);
    if bounds.width >= 2.0 * screen.x {
      centre.x = centre
        .x
        .clamp(bounds.x + screen.x, bounds.right() - screen.x);
    }
    if bounds.height >= 2.0 * screen.y {
      centre.y = centre
        .y
        .clamp(bounds.y + screen.y, bounds.bottom() - screen.y);
    }
    let mut camera = ctx.camera();
    camera.set_zoom(zoom);
    if lerp >= 1.0 {
      camera.snap_to(centre);
    } else {
      camera.follow(centre, lerp);
    }
  }
}

/// A map's size in world pixels, placed at `origin`
pub(crate) fn map_rect(map: &TileMap, origin: Vec2) -> Rect {
  Rect::new(
    origin.x,
    origin.y,
    (map.size.x * map.tile_size.x) as f32,
    (map.size.y * map.tile_size.y) as f32,
  )
}

/// Clears the view to `color` (the map's background) instead of the
/// frame's; the page's camera reset puts it back
pub(crate) fn clear_to(ctx: &mut SceneContext, color: Color) {
  let camera = ctx.camera().entity();
  if let Some(mut camera) = ctx.world.get_mut::<Camera2d>(camera) {
    camera.clear = CameraClear::Color(color);
  }
}

/// A rectangle of `color` behind a map (the renderer draws a map's tiles,
/// not its background colour), at `z`
pub(crate) fn backdrop(ctx: &mut SceneContext, rect: Rect, color: Color, z: i32) -> Entity {
  let pixel = ctx
    .assets()
    .add(Image::solid_rect(1, 1, color.r, color.g, color.b, color.a));
  let (x, y) = rect.center();
  ctx
    .world
    .spawn((
      Transform::from_xy(x, y).with_scale(Vec2::new(rect.width, rect.height)),
      Renderable::image(pixel).with_z_order(z),
    ))
    .id()
}

/// Four walls round `bounds`, for a map whose own edges are open: the
/// platformer's ground runs off its sides, the garden's paths too
pub(crate) fn fence(ctx: &mut SceneContext, bounds: Rect) {
  const THICK: f32 = 16.0;
  let (cx, cy) = bounds.center();
  let walls = [
    (
      Vec2::new(bounds.x - THICK / 2.0, cy),
      Vec2::new(THICK, bounds.height + 2.0 * THICK),
    ),
    (
      Vec2::new(bounds.right() + THICK / 2.0, cy),
      Vec2::new(THICK, bounds.height + 2.0 * THICK),
    ),
    (
      Vec2::new(cx, bounds.y - THICK / 2.0),
      Vec2::new(bounds.width + 2.0 * THICK, THICK),
    ),
    (
      Vec2::new(cx, bounds.bottom() + THICK / 2.0),
      Vec2::new(bounds.width + 2.0 * THICK, THICK),
    ),
  ];
  for (at, size) in walls {
    ctx.world.spawn((
      Transform::from_translation(at),
      Collider::rect(size.x, size.y),
    ));
  }
}

/// Where the first object of `class` in `map` is, in map pixels: a point's
/// own place, a rectangle's centre
pub(crate) fn object_point(map: &TileMap, class: &str) -> Option<Vec2> {
  map.layers.iter().find_map(|layer| match &layer.kind {
    LayerKind::Objects(objects) => objects
      .iter()
      .find(|object| object.class.as_deref() == Some(class))
      .map(|object| layer.info.offset + object.position + object.size / 2.0),
    _ => None,
  })
}

/// One 16 px hero from Tiny Dungeon's sheet (`id` counts across its rows of
/// twelve), cut out as an image of its own
pub(crate) fn hero(ctx: &mut SceneContext, id: u32) -> Option<Handle<Image>> {
  sheet_tile(ctx, HEROES, 16, 12, id)
}

/// Tile `id` of the packed sheet at `path` (square tiles `side` pixels
/// across, `columns` to a row, no spacing), cut out as an image of its own
pub(crate) fn sheet_tile(
  ctx: &mut SceneContext,
  path: &str,
  side: u32,
  columns: u32,
  id: u32,
) -> Option<Handle<Image>> {
  let sheet = load::<Image>(ctx, path)?;
  let cut = {
    let assets = ctx.assets();
    let sheet = assets.get(&sheet)?;
    let (x0, y0) = ((id % columns) * side, (id / columns) * side);
    if x0 + side > sheet.width() || y0 + side > sheet.height() {
      return None;
    }
    let row_bytes = sheet.width() as usize * 4;
    let mut data = Vec::with_capacity((side * side * 4) as usize);
    for y in y0..y0 + side {
      let start = y as usize * row_bytes + x0 as usize * 4;
      data.extend_from_slice(&sheet.data()[start..start + side as usize * 4]);
    }
    Image::from_rgba(data, side, side)
  };
  Some(ctx.assets().add(cut))
}

/// The hero on the platformer controller, its feet at `feet`, drawn at `z`
/// over the map's layers. Its collider is 10 x 14 (it fits the door's two
/// cells and under the ledges); `Respawn::here` sends it back to `feet`
pub(crate) fn spawn_platformer_player(
  ctx: &mut SceneContext,
  feet: Vec2,
  sprite: Option<Handle<Image>>,
  movement: PlatformerMovement,
  z: i32,
) -> Entity {
  const SIZE: Vec2 = Vec2::new(10.0, 14.0);
  let player = ctx
    .world
    .spawn((
      Transform::from_translation(feet - Vec2::new(0.0, SIZE.y / 2.0 + 0.5)),
      Interpolated,
      Collider::rect(SIZE.x, SIZE.y),
      movement,
      bindings(),
      Respawn::here(),
      Player,
    ))
    .id();
  if let Some(sprite) = sprite {
    // 16 px tall over a 14 px box: the sprite's feet on the box's
    ctx.spawn_child(
      player,
      (
        Transform::from_xy(0.0, -1.0),
        Renderable::image(sprite).with_z_order(z),
      ),
    );
  }
  player
}

/// The hero on the top-down controller at `at`, its sprite y-sorted at `z`
/// (a y-sorted map layer's z, so it walks behind and in front of what that
/// layer draws). Its collider is a circle at its feet
pub(crate) fn spawn_top_down_player(
  ctx: &mut SceneContext,
  at: Vec2,
  sprite: Option<Handle<Image>>,
  z: i32,
) -> Entity {
  let player = ctx
    .world
    .spawn((
      Transform::from_translation(at),
      Interpolated,
      Collider::circle(4.5),
      TopDownMovement::default().with_speed(110.0, 0.06, 0.08),
      bindings(),
      Player,
    ))
    .id();
  if let Some(sprite) = sprite {
    // The sprite stands on the circle: its bottom edge at the circle's
    ctx.spawn_child(
      player,
      (
        Transform::from_xy(0.0, -3.5),
        Renderable::image(sprite).with_z_order(z),
        YSort::new(),
      ),
    );
  }
  player
}

/// The hero's sprite on the side-view pages: a Tiny Dungeon adventurer
const SIDE_HERO: u32 = 88;
/// The hero draws over the platformer's layers (the root's z is 0, its
/// seven layers 0 to 6)
const SIDE_HERO_Z: i32 = 10;

/// The platformer level with the hero on it: the collision and objects
/// pages. The hero spawns at the map's `player_start` once the map has
/// landed, when its colliders are there for the hero's first step
pub(crate) struct SideLevel {
  pub(crate) map: Option<Handle<TileMap>>,
  pub(crate) root: Option<Entity>,
  pub(crate) player: Option<Entity>,
  pub(crate) bounds: Rect,
  start: Vec2,
  /// The camera has been put on the hero (it eases from then on)
  framed: bool,
}

impl SideLevel {
  pub(crate) fn new() -> Self {
    Self {
      map: None,
      root: None,
      player: None,
      bounds: Rect::new(0.0, 0.0, 864.0, 288.0),
      start: Vec2::new(45.0, 216.0),
      framed: false,
    }
  }

  /// Loads the platformer and spawns it with `root` (a `MapRoot` of it)
  pub(crate) fn enter(
    &mut self,
    ctx: &mut SceneContext,
    root: impl FnOnce(Handle<TileMap>) -> MapRoot,
  ) {
    self.map = load::<TileMap>(ctx, PLATFORMER);
    let Some(map) = self.map.clone() else {
      return;
    };
    if let Some(data) = asset(ctx, &map) {
      self.bounds = map_rect(&data, Vec2::ZERO);
      self.start = object_point(&data, "player_start").unwrap_or(self.start);
      clear_to(ctx, data.background.unwrap_or(Color::rgb(127, 240, 165)));
    }
    self.root = Some(ctx.world.spawn((Transform::IDENTITY, root(map))).id());
    fence(ctx, self.bounds);
  }

  /// Spawns the hero with `movement` on the step the map is ready; true
  /// on that step
  pub(crate) fn spawn_when_ready(
    &mut self,
    ctx: &mut SceneContext,
    movement: PlatformerMovement,
  ) -> bool {
    let Some(root) = self.root else {
      return false;
    };
    if self.player.is_some() || !ctx.maps().is_ready(root) {
      return false;
    }
    let sprite = hero(ctx, SIDE_HERO);
    self.player = Some(spawn_platformer_player(
      ctx,
      self.start,
      sprite,
      movement,
      SIDE_HERO_Z,
    ));
    true
  }

  /// The camera on the hero (on the start until it spawns): there at
  /// once the first time, easing after
  pub(crate) fn follow(&mut self, ctx: &mut SceneContext, view: &View, zoom: f32) {
    let at = place_of(ctx, self.player).unwrap_or(self.start);
    let lerp = if self.framed { 0.2 } else { 1.0 };
    view.follow(ctx, at, zoom, self.bounds, lerp);
    self.framed = true;
  }
}

/// The player's place now, if it has one
pub(crate) fn place_of(ctx: &SceneContext, entity: Option<Entity>) -> Option<Vec2> {
  ctx
    .world
    .get::<Transform>(entity?)
    .map(|transform| transform.translation)
}

/// A dark box behind `lines` from (`x`, `y`), one line each; returns the
/// height it took
pub(crate) fn hud(ui: &mut UiContext, x: f32, y: f32, lines: &[(String, Color)]) -> f32 {
  const LINE: f32 = 18.0;
  const PAD: f32 = 6.0;
  if lines.is_empty() {
    return 0.0;
  }
  let width = lines
    .iter()
    .map(|(text, _)| ui.text_size(text).0 as f32)
    .fold(0.0, f32::max);
  let height = lines.len() as f32 * LINE + 2.0 * PAD - 4.0;
  ui.draw_rect(Rect::new(x, y, width + 2.0 * PAD, height), BACKING);
  for (i, (text, color)) in lines.iter().enumerate() {
    ui.draw_text(text, x + PAD, y + PAD + i as f32 * LINE, *color);
  }
  height
}

/// [`hud`] with its right edge at `right`
pub(crate) fn hud_right(ui: &mut UiContext, right: f32, y: f32, lines: &[(String, Color)]) -> f32 {
  let width = lines
    .iter()
    .map(|(text, _)| ui.text_size(text).0 as f32)
    .fold(0.0, f32::max);
  hud(ui, right - width - 12.0, y, lines)
}

/// [`hud`] in the settings column, under a panel of `rows` rows: the room
/// a page with settings has for its notes, clear of its view
pub(crate) fn hud_beside_panel(ui: &mut UiContext, rows: usize, lines: &[(String, Color)]) -> f32 {
  let area = content_area(ui);
  let x = ui.screen_size().0 as f32 * 0.55 + 8.0;
  let y = area.y + settings_panel_height(ui, rows) + 10.0;
  hud(ui, x, y, lines)
}

/// `text` over the world point `at` (centred on it, above it), on a dark
/// backing
pub(crate) fn label_at(
  ui: &mut UiContext,
  ctx: &mut SceneContext,
  at: Vec2,
  text: &str,
  color: Color,
) {
  let screen = ctx.camera().world_to_screen(at);
  let (w, h) = ui.text_size(text);
  let (w, h) = (w as f32, h as f32);
  let rect = Rect::new(
    screen.x - w / 2.0 - 3.0,
    screen.y - h - 4.0,
    w + 6.0,
    h + 2.0,
  );
  ui.draw_rect(rect, BACKING);
  ui.draw_text(text, rect.x + 3.0, rect.y + 1.0, color);
}

/// Outlines every collider: the map's (its packed chunk outlines and
/// one-way runs) in blue, one-way in pink, triggers in gold, the player in
/// white. In entity index order, so overlapping outlines keep their colours
pub(crate) fn draw_colliders(ctx: &mut SceneContext) {
  let mut query = ctx
    .world
    .query::<(Entity, &GlobalTransform, &Collider, Has<Player>)>();
  let mut lines: Vec<(Entity, Vec<Vec<Vec2>>, Color)> = query
    .iter(ctx.world)
    .map(|(entity, global, collider, player)| {
      let color = if player {
        Color::WHITE
      } else if collider.is_trigger {
        HOT
      } else if collider.one_way.is_some() {
        ONE_WAY
      } else {
        Color::rgb(110, 160, 255)
      };
      (entity, outline(global, collider), color)
    })
    .collect();
  lines.sort_unstable_by_key(|(entity, ..)| entity.index_u32());
  let mut debug = ctx.debug_draw();
  let mut pen = debug.width(2.0);
  for (_, outline, color) in lines {
    for points in outline {
      pen.polyline(&points, color);
    }
  }
}

/// `collider`'s outline on an entity at `global`, in world space, as lines
/// to draw (a tile map packs a chunk's outlines into one collider of
/// separate segments)
fn outline(global: &GlobalTransform, collider: &Collider) -> Vec<Vec<Vec2>> {
  let arc = |centre: Vec2, radius: f32, from: f32, to: f32| {
    (0..=12).map(move |i| centre + Vec2::from_angle(from + (to - from) * i as f32 / 12.0) * radius)
  };
  let local: Vec<Vec2> = match &collider.shape {
    ColliderShape::Segments { points, segments } => {
      return segments
        .iter()
        .filter_map(|&[a, b]| Some([*points.get(a as usize)?, *points.get(b as usize)?]))
        .map(|ends| place(global, collider, ends.to_vec()))
        .collect();
    }
    ColliderShape::Rect { size } => {
      let h = *size / 2.0;
      vec![
        Vec2::new(-h.x, -h.y),
        Vec2::new(h.x, -h.y),
        h,
        Vec2::new(-h.x, h.y),
        Vec2::new(-h.x, -h.y),
      ]
    }
    ColliderShape::Circle { radius } => arc(Vec2::ZERO, *radius, 0.0, TAU).collect(),
    ColliderShape::Capsule { height, radius } => {
      let h = (height / 2.0 - radius).max(0.0);
      let mut points: Vec<Vec2> = arc(Vec2::new(0.0, h), *radius, 0.0, PI)
        .chain(arc(Vec2::new(0.0, -h), *radius, PI, TAU))
        .collect();
      points.push(points[0]);
      points
    }
    ColliderShape::Polygon { points } => points
      .iter()
      .copied()
      .chain(points.first().copied())
      .collect(),
    ColliderShape::Segment { a, b } => vec![*a, *b],
    ColliderShape::Polyline { points } => points.clone(),
    _ => Vec::new(),
  };
  vec![place(global, collider, local)]
}

/// Points in `collider`'s own space, in world space
fn place(global: &GlobalTransform, collider: &Collider, local: Vec<Vec2>) -> Vec<Vec2> {
  let affine = global.affine();
  let turn = Vec2::from_angle(collider.rotation);
  local
    .into_iter()
    .map(|p| affine.transform_point2(collider.offset + turn.rotate(p)))
    .collect()
}
