//! The page "An endless world": an empty infinite map built in code
//! (`TileMap::infinite`, a scenery layer and a ground layer over the Pixel
//! Platformer tileset, under them a dark backdrop of earth, seen where the
//! ground is dug or hollow), spawned with a `MapRoot` and a `MapStream` whose
//! generator makes every chunk (`terrain::Wilds`): rolling hills of earth
//! going down without end, caves opening under the grass, sprouts, pines
//! and clouds. Chunks load within 256 px of the view and of the digger
//! (its `StreamAnchor`), and unload once 768 px from both.
//!
//! The digger walks and jumps with WASD; the left button digs out the
//! cell under the pointer and the right button builds one, within six
//! cells of it, each changed cell and its four neighbours taking the tiles
//! their neighbours call for. The stream keeps a `SavedChunks`: a chunk the
//! player changed goes into it as it unloads, and comes back from it, as
//! left, instead of from the generator. Q and E travel 4,000 px left or
//! right, so the chunks round the old place unload (the changed ones into
//! the store) and those round the new one land; the digger waits in the air
//! until `ctx.maps().stream_ready` says the ground round it has landed.
//! The HUD shows the stream's own counts (`ctx.maps().stream_status`: the
//! chunks resident and those in flight) and the chunks the store keeps
//! (`SavedChunks::len`); the labels and the minimap mark the kept ones in
//! gold (`SavedChunks::contains`, `SavedChunks::keys`)

use demo_kit::prelude::*;

use crate::overlay::{self, ChunkLabels, FRESH, GENERATED, Grid, Minimap, Streamed, Tag};
use crate::shared::{
  CELL, DIM, GOOD, HOT, TILESET, View, beside_panel, bindings, clear_to, ground_tile, hero, hud,
  load, wasd,
};
use crate::terrain::{BACKDROP, GROUND, SCENERY, Wilds, standing};

/// The view's background, and the earth behind the ground, seen where it
/// is dug or hollow
const SKY: Color = Color::rgb(159, 220, 244);
const BACKDROP_TINT: Color = Color::rgb(105, 80, 88);
/// The digger's sprite on Tiny Dungeon's sheet (a dwarf in a horned helm),
/// drawn over the map's two layers (z 0 and 1)
const DIGGER_SPRITE: u32 = 87;
const DIGGER_Z: i32 = 10;
/// The digger's box, and how fast it walks (pixels a second)
const DIGGER_SIZE: Vec2 = Vec2::new(10.0, 14.0);
const WALK_SPEED: f32 = 120.0;
/// Chunks load within this many pixels of the view and the digger, and
/// unload past this many from both
const LOAD: f32 = 256.0;
const UNLOAD: f32 = 768.0;
/// How far the digger reaches, in cells
const REACH: f32 = 6.0;
/// How far Q and E travel, in pixels
const TRAVEL: f32 = 4000.0;
/// The settings rows, for the notes under the panel
const ROWS: usize = 2;
/// The chunks a side the minimap shows round the view
const WINDOW: IVec2 = IVec2::new(9, 5);
/// The four cells beside a cell
const BESIDE: [IVec2; 4] = [IVec2::NEG_Y, IVec2::Y, IVec2::NEG_X, IVec2::X];

/// The endless world's digger
#[derive(Component, Debug)]
pub struct Digger;

pub(crate) struct EndlessPage {
  view: View,
  root: Option<Entity>,
  wilds: Option<Wilds>,
  /// Where the stream keeps the chunks the player changed, as they unload
  saved: SavedChunks,
  /// The chunks it keeps, by row, as this step found them
  kept: Vec<IVec2>,
  digger: Option<Entity>,
  sprite: Option<Entity>,
  facing: f32,
  /// Where the digger waits while the ground round it lands: as the page
  /// opens, and after each travel
  arriving: Option<Vec2>,
  grid: Option<Grid>,
  streamed: Streamed,
  /// The camera's centre, easing after the digger (`None` jumps there)
  eye: Option<Vec2>,
  /// The cell under the pointer, when it is over the world, and whether
  /// the digger reaches it
  hover: Option<(IVec2, bool)>,
  /// The last cell a held button dug or built, and which
  last: Option<(IVec2, bool)>,
  dug: u32,
  built: u32,
  /// Where the page's notes are on screen: the pointer digs nothing there
  covered: Vec<Rect>,
  overlay: bool,
  /// The chunks' labels, world text placed after the camera moves
  labels: ChunkLabels,
  zoom: i32,
}

impl EndlessPage {
  pub(crate) fn new() -> Self {
    Self {
      view: View::new(true),
      root: None,
      wilds: None,
      saved: SavedChunks::default(),
      kept: Vec::new(),
      digger: None,
      sprite: None,
      facing: 1.0,
      arriving: None,
      grid: None,
      streamed: Streamed::default(),
      eye: None,
      hover: None,
      last: None,
      dug: 0,
      built: 0,
      covered: Vec::new(),
      overlay: true,
      labels: ChunkLabels::default(),
      zoom: 2,
    }
  }

  /// Where the digger's middle goes to stand on the generated grass of the
  /// column at world x `x`
  fn arrival(&self, x: f32) -> Option<Vec2> {
    let wilds = self.wilds?;
    let column = (x / CELL).floor() as i32;
    let top = wilds.top(column);
    Some(feet_on(column, top))
  }

  /// Once the map has landed: its grid, and the digger waiting where it
  /// arrives
  fn land(&mut self, ctx: &mut SceneContext, root: Entity) {
    self.grid = Grid::of(ctx, root, GROUND, CELL);
    let at = self.arriving.unwrap_or(Vec2::ZERO);
    let digger = ctx
      .world
      .spawn((
        Transform::from_translation(at),
        Interpolated,
        Collider::rect(DIGGER_SIZE.x, DIGGER_SIZE.y),
        CharacterController::platformer().with_step_height(4.0),
        PlatformerMovement::default()
          .with_run(WALK_SPEED, 0.08, 0.06)
          .with_jump(58.0, 0.36),
        bindings(),
        // The chunks round it stay loaded, wherever the camera looks
        StreamAnchor,
        Digger,
      ))
      .id();
    if let Some(image) = hero(ctx, DIGGER_SPRITE) {
      let sprite = ctx.spawn_child(
        digger,
        (
          Transform::from_xy(0.0, -1.0),
          Renderable::image(image).with_z_order(DIGGER_Z),
        ),
      );
      self.sprite = Some(sprite);
    }
    self.digger = Some(digger);
  }

  fn digger_at(&self, ctx: &SceneContext) -> Option<Vec2> {
    self
      .digger
      .and_then(|digger| ctx.world.get::<Transform>(digger))
      .map(|transform| transform.translation)
  }

  /// Sends the digger `dx` pixels along: it waits in the air there until
  /// the ground round it has landed, and the camera jumps with it
  fn travel(&mut self, ctx: &SceneContext, dx: f32) {
    let Some(at) = self.digger_at(ctx) else {
      return;
    };
    if let Some(to) = self.arrival(at.x + dx) {
      self.arriving = Some(to);
      self.eye = None;
      self.last = None;
    }
  }

  /// Holds the digger where it arrives until the chunks round it have
  /// landed, then stands it on the ground there as it is now (the player
  /// may have built on it)
  fn arrive(&mut self, ctx: &mut SceneContext, root: Entity) {
    let (Some(digger), Some(at)) = (self.digger, self.arriving) else {
      return;
    };
    let place = |ctx: &mut SceneContext, to: Vec2| {
      if let Some(mut transform) = ctx.world.get_mut::<Transform>(digger) {
        transform.translation = to;
      }
      if let Some(mut velocity) = ctx.world.get_mut::<Velocity>(digger) {
        *velocity = Velocity::zero();
      }
    };
    place(ctx, at);
    let around = Rect::new(
      at.x - 10.0 * CELL,
      at.y - 8.0 * CELL,
      20.0 * CELL,
      16.0 * CELL,
    );
    let maps = ctx.maps();
    if !maps.stream_ready(root, around) {
      return;
    }
    let Some(cell) = maps.world_to_cell(root, GROUND, at) else {
      return;
    };
    // The cell it stands on: up past whatever was built there
    let mut top = cell.y + 1;
    for _ in 0..64 {
      if maps
        .tile(root, GROUND, IVec2::new(cell.x, top - 1))
        .is_none()
      {
        break;
      }
      top -= 1;
    }
    place(ctx, feet_on(cell.x, top));
    self.arriving = None;
  }

  /// The digger faces the way it walks
  fn face(&mut self, ctx: &mut SceneContext) {
    let Some(digger) = self.digger else {
      return;
    };
    if let Some(velocity) = ctx.world.get::<Velocity>(digger)
      && velocity.dx.abs() > 1.0
    {
      self.facing = velocity.dx.signum();
    }
    if let Some(sprite) = self.sprite
      && let Some(mut renderable) = ctx.world.get_mut::<Renderable>(sprite)
    {
      renderable.flip_x = self.facing < 0.0;
    }
  }

  /// The camera on the digger (on where it arrives until it spawns): there
  /// at once after a travel, easing after
  fn follow(&mut self, ctx: &mut SceneContext) {
    let Some(focus) = self.digger_at(ctx).or(self.arriving) else {
      return;
    };
    let zoom = self.zoom as f32;
    let target = self.view.centre_for(ctx, focus, zoom);
    let (eye, jump) = match self.eye {
      Some(eye) => (eye + (target - eye) * 0.12, false),
      None => (target, true),
    };
    self.eye = Some(eye);
    self.view.place(ctx, eye, zoom, jump);
  }

  /// Digs out (`dig`) or builds the cell `cell`, and gives it and its four
  /// neighbours the tiles their neighbours call for; whether it changed.
  /// Digging takes what stood on the cell too; building never fills the
  /// digger's own cells. Only landed chunks take edits, so a cell in one
  /// not landed changes nothing
  fn act(&mut self, ctx: &mut SceneContext, root: Entity, cell: IVec2, dig: bool) -> bool {
    let (Some(wilds), Some(at)) = (self.wilds, self.digger_at(ctx)) else {
      return false;
    };
    let mut maps = ctx.maps();
    let ground = maps.tile(root, GROUND, cell).is_some();
    if dig {
      if !ground {
        // Dig up a sprout or a pine standing in the open
        let standing_here = maps
          .tile(root, SCENERY, cell)
          .is_some_and(|tile| standing(tile.id));
        return standing_here && maps.set_tile(root, SCENERY, cell, None);
      }
      if !maps.set_tile(root, GROUND, cell, None) {
        return false;
      }
      let above = cell + IVec2::NEG_Y;
      if maps
        .tile(root, SCENERY, above)
        .is_some_and(|tile| standing(tile.id))
      {
        maps.set_tile(root, SCENERY, above, None);
      }
      self.dug += 1;
    } else {
      let half = DIGGER_SIZE / 2.0;
      let (lo, hi) = (cell.as_vec2() * CELL, (cell + IVec2::ONE).as_vec2() * CELL);
      let on_digger = lo.x < at.x + half.x
        && hi.x > at.x - half.x
        && lo.y < at.y + half.y
        && hi.y > at.y - half.y;
      if ground || on_digger {
        return false;
      }
      if !maps.set_tile(root, GROUND, cell, Some(Tile::new(wilds.tileset, 0))) {
        return false;
      }
      maps.set_tile(root, SCENERY, cell, None);
      self.built += 1;
    }
    for next in std::iter::once(cell).chain(BESIDE.map(|step| cell + step)) {
      if maps.tile(root, GROUND, next).is_none() {
        continue;
      }
      let ground = |step: IVec2| maps.tile(root, GROUND, next + step).is_some();
      let id = ground_tile(
        ground(IVec2::NEG_Y),
        ground(IVec2::Y),
        ground(IVec2::NEG_X),
        ground(IVec2::X),
      );
      maps.set_tile(root, GROUND, next, Some(Tile::new(wilds.tileset, id)));
    }
    true
  }

  /// The pointer digs and builds within reach, outside the page's notes
  fn point(&mut self, ctx: &mut SceneContext, root: Entity) {
    let (mx, my) = ctx.input.mouse_position();
    let world = ctx.camera().screen_to_world(Vec2::new(mx, my));
    let over_notes = self.covered.iter().any(|rect| rect.contains(mx, my));
    let digger = self.digger_at(ctx);
    self.hover = match (over_notes, digger) {
      (false, Some(at)) => ctx.maps().world_to_cell(root, GROUND, world).map(|cell| {
        let middle = (cell.as_vec2() + Vec2::splat(0.5)) * CELL;
        (cell, middle.distance(at) <= REACH * CELL)
      }),
      _ => None,
    };
    let (left, right) = (
      ctx.input.mouse_down(MouseButton::Left),
      ctx.input.mouse_down(MouseButton::Right),
    );
    match self.hover {
      Some((cell, true)) if (left || right) && self.arriving.is_none() => {
        let dig = left;
        if self.last != Some((cell, dig)) {
          self.act(ctx, root, cell, dig);
        }
        self.last = Some((cell, dig));
      }
      _ => self.last = None,
    }
  }
}

/// Where a body's middle goes to stand on cell row `top` of column `column`
fn feet_on(column: i32, top: i32) -> Vec2 {
  Vec2::new(
    (column as f32 + 0.5) * CELL,
    top as f32 * CELL - DIGGER_SIZE.y / 2.0 - 0.5,
  )
}

/// What a chunk's label says it is: kept in the store, or as the
/// generator made it
fn tag(saved: &SavedChunks, chunk: IVec2) -> Tag {
  if saved.contains(chunk) {
    Some(("kept", HOT))
  } else {
    Some(("generated", GENERATED))
  }
}

impl DemoPage for EndlessPage {
  fn name(&self) -> &str {
    "An endless world"
  }

  fn description(&self) -> &str {
    "Streamed from a generator: dig and build, leave, and find it as you left it when you come back"
  }

  fn settings(&mut self, s: &mut Settings) {
    s.toggle("Chunk overlay", &mut self.overlay);
    s.int("Zoom", &mut self.zoom, 1..=3);
  }

  fn controls(&self) -> Vec<Control> {
    vec![
      Control::mouse("Left digs, right builds, within reach"),
      wasd("Walk (pad: stick)"),
      Control::key(KeyCode::Space, "Jump (pad: A)"),
      Control::keys(
        "Q E",
        &[KeyCode::KeyQ, KeyCode::KeyE],
        "Travel 4,000 px left, right",
      ),
    ]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    clear_to(ctx, SKY);
    let Some(tileset) = load::<Tileset>(ctx, TILESET) else {
      return;
    };
    let mut map = TileMap::infinite(UVec2::splat(CELL as u32));
    let set = map.add_tileset(TilesetSource::Asset(tileset));
    // The earth behind the ground, dark and not colliding, then what stands
    // on the ground, then the ground
    let backdrop = map.add_tile_layer(BACKDROP);
    if let Some(layer) = map.layers.get_mut(backdrop) {
      layer.info.tint = BACKDROP_TINT;
      layer.info.collides = false;
    }
    let scenery = map.add_tile_layer(SCENERY);
    if let Some(layer) = map.layers.get_mut(scenery) {
      layer.info.collides = false;
    }
    map.add_tile_layer(GROUND);
    let map = ctx.assets().add(map);
    let wilds = Wilds { tileset: set };
    self.wilds = Some(wilds);
    let stream = MapStream {
      load_distance: LOAD,
      unload_distance: UNLOAD,
      ..MapStream::new(move |request: ChunkRequest| wilds.chunk(request))
    }
    .with_saved(self.saved.clone());
    self.root = Some(
      ctx
        .world
        .spawn((Transform::IDENTITY, MapRoot::new(map), stream))
        .id(),
    );
    // The digger arrives on the grass at the origin's column
    self.arriving = self.arrival(0.0);
    self.follow(ctx);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    let Some(root) = self.root else {
      return;
    };
    if self.digger.is_none() && ctx.maps().is_ready(root) {
      self.land(ctx, root);
    }
    let events = ctx.messages::<ChunkEvent>();
    if self.arriving.is_none() {
      if ctx.input.key_pressed(KeyCode::KeyQ) {
        self.travel(ctx, -TRAVEL);
      } else if ctx.input.key_pressed(KeyCode::KeyE) {
        self.travel(ctx, TRAVEL);
      }
    }
    self.arrive(ctx, root);
    self.face(ctx);
    self.follow(ctx);
    let Some(grid) = self.grid else {
      return;
    };
    self.point(ctx, root);

    self.streamed.update(ctx, root, &events, dt);
    self.kept = self.saved.keys();
    let saved = &self.saved;
    self
      .labels
      .place(ctx, grid, &self.streamed.chunks, self.overlay, &|chunk| {
        tag(saved, chunk)
      });
    if self.overlay {
      let mut debug = ctx.debug_draw();
      for &chunk in &self.streamed.chunks {
        let color = if self.saved.contains(chunk) {
          HOT
        } else {
          GENERATED
        };
        overlay::chunk_outline_in(&mut debug, grid.chunk(chunk), color);
      }
      for (chunk, fresh) in self.streamed.freshness() {
        let alpha = (fresh * 255.0) as u8;
        overlay::outline(&mut debug, grid.chunk(chunk), FRESH.with_alpha(alpha), 3.0);
      }
    }
    // The cell under the pointer: white within reach, dim beyond it
    if let Some((cell, reach)) = self.hover {
      let color = if reach { Color::WHITE } else { DIM };
      let rect = grid.rect(cell, cell + IVec2::ONE);
      overlay::outline(&mut ctx.debug_draw(), rect, color, 2.0);
    }
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    self.view.measure(ui);
    self.covered.clear();
    let Some(root) = self.root else {
      return;
    };
    self.labels.measure(ui);
    self.streamed.read_status(ctx, root);

    let digger = self.digger_at(ctx);
    let digger_line = match digger.and_then(|at| ctx.maps().world_to_cell(root, GROUND, at)) {
      Some(cell) => {
        let chunk = TileGrid::chunk_of(cell);
        format!(
          "Digger: cell {}, {} in chunk {}, {}",
          cell.x, cell.y, chunk.x, chunk.y
        )
      }
      None => "Digger: waiting for the map".to_string(),
    };
    let saved = self.saved.len();
    let status = if self.arriving.is_some() {
      "Travelling: waiting for the ground round you"
    } else {
      "Q, E travel far; chunks past 768 px unload"
    };
    let streamed = &self.streamed;
    let lines = vec![
      (
        "Dig and build; leave, come back: it is kept".to_string(),
        Color::WHITE,
      ),
      (streamed.resident_line(), GENERATED),
      (
        format!(
          "Saved: {saved} chunk{} you changed and left",
          if saved == 1 { "" } else { "s" }
        ),
        HOT,
      ),
      (
        format!("Dug {}, built {}", self.dug, self.built),
        Color::WHITE,
      ),
      (digger_line, GOOD),
      (status.to_string(), DIM),
    ];
    let column = beside_panel(ui, ROWS);
    let notes = hud(ui, column.x, column.y, column.width, &lines);
    self.covered.push(notes);
    let Some(grid) = self.grid else {
      return;
    };
    let view = ctx.camera().visible_bounds();
    let (a, b) = (
      grid.to_cells(Vec2::new(view.left, view.top)),
      grid.to_cells(Vec2::new(view.right, view.bottom)),
    );
    let below = notes.bottom() + 8.0;
    let area = Rect::new(column.x, below, column.width, column.bottom() - below);
    let fresh = self.streamed.freshness();
    let drawn = overlay::minimap(
      ui,
      area,
      &Minimap {
        caption: "Green: resident; gold: kept in SavedChunks",
        chunks: &self.streamed.chunks,
        generated: &self.streamed.chunks,
        fresh: &fresh,
        marked: &self.kept,
        bounds: None,
        view: Rect::new(a.x, a.y, b.x - a.x, b.y - a.y),
        dot: digger.map(|at| grid.to_cells(at)),
        window: Some(WINDOW),
      },
    );
    self.covered.push(drawn);
  }
}
