//! The World of rooms page: `rooms/rooms.world`, a Tiled world of three
//! rooms side by side (a TMJ, a TMX and a TMJ). Only the rooms near the
//! hero exist: each step the page asks `ctx.maps().spawn_levels_in_with` for
//! the rooms its reach (the box round the hero) overlaps, which spawns those
//! not yet spawned, each drawn from the z the page gives it, and despawns
//! rooms the hero has left well behind. Walk through the doorways to the
//! east and watch the next room arrive

use demo_kit::prelude::*;

use crate::shared::{
  DIM, GOOD, HOT, View, asset, hero, hud, hud_height, load, object_point, place_of,
  spawn_top_down_player, wasd,
};

/// Three rooms side by side
const ROOMS: &str = "rooms/rooms.world";
/// How far round the hero rooms spawn, in world pixels
const REACH: f32 = 64.0;
/// How much farther a room must be before it goes
const KEEP: f32 = 96.0;
/// The hero's sprite: a Tiny Dungeon wizard
const HERO: u32 = 84;
/// The z each room draws from: its y-sorted props layer, the third, is then
/// at z 0 with the hero, who walks among the props
const ROOMS_Z: i32 = -2;

/// A level of the world: its name and where it is
struct Room {
  name: String,
  rect: Rect,
}

pub(crate) struct RoomsPage {
  view: View,
  world: Option<Handle<TileWorld>>,
  rooms: Vec<Room>,
  bounds: Rect,
  start: Vec2,
  player: Option<Entity>,
  /// The rooms spawned now, by level index, and whether each has landed
  live: Vec<(usize, bool)>,
  arrivals: u32,
  departures: u32,
}

impl RoomsPage {
  pub(crate) fn new() -> Self {
    Self {
      view: View::new(false),
      world: None,
      rooms: Vec::new(),
      bounds: Rect::new(0.0, 0.0, 960.0, 192.0),
      start: Vec2::new(80.0, 96.0),
      player: None,
      live: Vec::new(),
      arrivals: 0,
      departures: 0,
    }
  }

  /// The hero's reach: the box in which rooms spawn
  fn reach(at: Vec2) -> Rect {
    Rect::new(at.x - REACH, at.y - REACH, 2.0 * REACH, 2.0 * REACH)
  }
}

/// How far `at` is from `rect` (0 inside it)
fn distance(rect: &Rect, at: Vec2) -> f32 {
  let dx = (rect.x - at.x).max(at.x - rect.right()).max(0.0);
  let dy = (rect.y - at.y).max(at.y - rect.bottom()).max(0.0);
  dx.hypot(dy)
}

impl DemoPage for RoomsPage {
  fn name(&self) -> &str {
    "World of rooms"
  }

  fn description(&self) -> &str {
    "A Tiled world of three rooms: each spawns as the hero comes near and goes once it is far behind"
  }

  fn controls(&self) -> Vec<Control> {
    vec![wasd("Walk (pad: left stick)")]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    self.world = load::<TileWorld>(ctx, ROOMS);
    let Some(world) = self.world.clone() else {
      return;
    };
    let (rooms, first_map) = {
      let assets = ctx.assets();
      let Some(loaded) = assets.get(&world) else {
        return;
      };
      let rooms: Vec<Room> = loaded
        .layout
        .levels
        .iter()
        .map(|level| Room {
          name: level.name.clone(),
          rect: Rect::new(
            level.position.x,
            level.position.y,
            level.size.x,
            level.size.y,
          ),
        })
        .collect();
      (rooms, loaded.map(0).cloned())
    };
    self.rooms = rooms;
    if let Some(first) = self.rooms.first() {
      let mut bounds = first.rect;
      for room in &self.rooms {
        let right = bounds.right().max(room.rect.right());
        let bottom = bounds.bottom().max(room.rect.bottom());
        bounds.x = bounds.x.min(room.rect.x);
        bounds.y = bounds.y.min(room.rect.y);
        bounds.width = right - bounds.x;
        bounds.height = bottom - bounds.y;
      }
      self.bounds = bounds;
      // The first room's start, placed where the world puts that room
      let origin = Vec2::new(first.rect.x, first.rect.y);
      let point = first_map
        .and_then(|map| asset(ctx, &map))
        .and_then(|map| object_point(&map, "player_start"));
      if let Some(point) = point {
        self.start = origin + point;
      }
    }
    // No gravity on a top-down hero: it can stand before its room lands
    let sprite = hero(ctx, HERO);
    self.player = Some(spawn_top_down_player(ctx, self.start, sprite, 0));
    self.view.follow(ctx, self.start, 2.0, self.bounds, 1.0);
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    let (Some(world), Some(at)) = (self.world.clone(), place_of(ctx, self.player)) else {
      return;
    };
    let arrived = ctx
      .maps()
      .spawn_levels_in_with(&world, Self::reach(at), 0, |_, room| room.with_z(ROOMS_Z));
    self.arrivals += arrived.len() as u32;

    // Rooms the hero has left far behind go
    let mut levels = ctx.world.query::<(Entity, &WorldLevelOf, Has<MapIds>)>();
    let mut gone = Vec::new();
    self.live.clear();
    for (entity, level, landed) in levels.iter(ctx.world) {
      let far = self
        .rooms
        .get(level.level)
        .is_some_and(|room| distance(&room.rect, at) > REACH + KEEP);
      if far {
        gone.push(entity);
      } else {
        self.live.push((level.level, landed));
      }
    }
    for entity in gone {
      ctx.world.despawn(entity);
      self.departures += 1;
    }
    self.live.sort_unstable();

    self.view.follow(ctx, at, 2.0, self.bounds, 0.15);
    let reach = Self::reach(at);
    let (x, y) = reach.center();
    let mut debug = ctx.debug_draw();
    debug
      .width(2.0)
      .rect(Vec2::new(x, y), Vec2::new(reach.width, reach.height), HOT);
    for room in &self.rooms {
      let (x, y) = room.rect.center();
      debug.rect(
        Vec2::new(x, y),
        Vec2::new(room.rect.width, room.rect.height),
        Color::new(255, 255, 255, 60),
      );
    }
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    self.view.measure(ui);
    let area = self.view.area;
    let mut lines = vec![(
      "A room spawns when the gold box meets it".to_string(),
      Color::WHITE,
    )];
    for (index, room) in self.rooms.iter().enumerate() {
      let state = self.live.iter().find(|(level, _)| *level == index);
      let (text, color) = match state {
        Some((_, true)) => ("spawned", GOOD),
        Some((_, false)) => ("landing", HOT),
        None => ("not spawned", DIM),
      };
      lines.push((format!("{}: {text}", room.name), color));
    }
    lines.push((
      format!("{} arrivals, {} departures", self.arrivals, self.departures),
      DIM,
    ));
    let height = hud_height(lines.len());
    hud_right(ui, area.right() - 4.0, area.bottom() - height - 4.0, &lines);
  }
}

/// [`hud`] with its right edge at `right`
fn hud_right(ui: &mut UiContext, right: f32, y: f32, lines: &[(String, Color)]) -> f32 {
  let width = lines
    .iter()
    .map(|(text, _)| ui.text_size(text).0 as f32)
    .fold(0.0, f32::max);
  hud(ui, right - width - 12.0, y, lines)
}
