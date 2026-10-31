//! The Objects page: what the designer placed in Tiled becomes entities.
//! The platformer's thirteen coins are tile objects of the coin tile, whose
//! class `coin` they take, as Tiled shows them (one worth 10, the rest worth
//! the class's default of 1): each gets a trigger and a `Pickup`, and goes
//! when the hero touches it. The spikes are tiles of
//! class `spikes`, spawned as entities (`MapRoot::with_entity_tiles`):
//! taken out of their layer, they are the game's to draw, and each gets its
//! tile's art and a kill zone that sends the hero back to the start. The
//! door is a rectangle object whose `switch` property refers to the lever
//! object: touching the lever clears the door's tiles from the colliding
//! `doors` layer, and the way to the prize is open

use demo_kit::prelude::*;

use crate::shared::{
  DIM, GOOD, HOT, PLATFORMER_CELL as CELL, SideLevel, View, hud, hud_height, sheet_tile, wasd,
};

/// The lever's tile when pulled (the file's lever is tile 64, leaning left)
const LEVER_PULLED: u32 = 66;
/// The platformer's sheet, for the spikes' art: 18 px tiles, 20 to a row
const SHEET: &str = "kenney-pixel-platformer/tilemap_packed.png";
/// The spikes draw at their layer's z (the root's is 0, `hazards` is 5)
const SPIKES_Z: i32 = 5;

/// The objects page's tally, kept as a resource while the page is open so a
/// test (or another system) can read it
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Coins {
  /// Coins collected since the page opened
  pub collected: u32,
  /// Their values summed
  pub score: i64,
}

pub(crate) struct ObjectsPage {
  view: View,
  level: SideLevel,
  /// Each coin's entity and its `value`
  coins: Vec<(Entity, i64)>,
  total: usize,
  tally: Coins,
  spikes: usize,
  deaths: u32,
  door: Option<Entity>,
  lever: Option<Entity>,
  open: bool,
}

impl ObjectsPage {
  pub(crate) fn new() -> Self {
    Self {
      view: View::new(false),
      level: SideLevel::new(),
      coins: Vec::new(),
      total: 0,
      tally: Coins::default(),
      spikes: 0,
      deaths: 0,
      door: None,
      lever: None,
      open: false,
    }
  }

  /// Gives the landed map's objects and class tiles what they do
  fn wire(&mut self, ctx: &mut SceneContext) {
    let mut coins = Vec::new();
    let mut doors = Vec::new();
    let mut objects = ctx.world.query::<(Entity, &MapObject)>();
    for (entity, object) in objects.iter(ctx.world) {
      match object.class.as_deref() {
        Some("coin") => coins.push((entity, coin_value(object))),
        Some("door") => doors.push((entity, object.reference("switch", ctx.world))),
        _ => {}
      }
    }
    for &(coin, _) in &coins {
      ctx
        .world
        .entity_mut(coin)
        .insert((Collider::circle(6.0).as_trigger(), Pickup::new()));
    }
    self.total = coins.len();
    self.coins = coins;

    // The spikes, at their cells' centres, kill along their cells' bottoms
    let mut tiles = ctx.world.query::<(Entity, &MapTile)>();
    let spikes: Vec<(Entity, Tile)> = tiles
      .iter(ctx.world)
      .filter(|(_, tile)| tile.class == "spikes")
      .map(|(entity, tile)| (entity, tile.tile))
      .collect();
    for &(spike, tile) in &spikes {
      let art = sheet_tile(ctx, SHEET, 18, 20, tile.id).map(|image| {
        Renderable::image(image)
          .with_z_order(SPIKES_Z)
          .with_flip_x(tile.flip.contains(TileFlip::HORIZONTAL))
          .with_flip_y(tile.flip.contains(TileFlip::VERTICAL))
      });
      let mut entity = ctx.world.entity_mut(spike);
      entity.insert((
        Collider::rect(16.0, 6.0).with_offset(0.0, 5.0).as_trigger(),
        KillZone::new(),
      ));
      if let Some(art) = art {
        entity.insert(art);
      }
    }
    self.spikes = spikes.len();

    if let Some(&(door, Some(lever))) = doors.first() {
      ctx
        .world
        .entity_mut(lever)
        .insert(Collider::rect(16.0, 16.0).as_trigger());
      self.door = Some(door);
      self.lever = Some(lever);
    }
  }

  /// Clears the door's cells from the `doors` layer, and pulls the lever
  fn open_door(&mut self, ctx: &mut SceneContext) {
    let (Some(root), Some(door)) = (self.level.root, self.door) else {
      return;
    };
    // The door's rectangle in the world (it is a child of its layer, which
    // is a child of the map), and each cell under it opens
    let (Some(size), Some(centre)) = (
      ctx.world.get::<MapObject>(door).map(|object| object.size),
      ctx
        .world
        .get::<GlobalTransform>(door)
        .map(GlobalTransform::translation),
    ) else {
      return;
    };
    let mut maps = ctx.maps();
    let top_left = centre - size / 2.0;
    let mut y = top_left.y + CELL / 2.0;
    while y < top_left.y + size.y {
      let mut x = top_left.x + CELL / 2.0;
      while x < top_left.x + size.x {
        if let Some(cell) = maps.world_to_cell(root, "doors", Vec2::new(x, y)) {
          maps.set_tile(root, "doors", cell, None);
        }
        x += CELL;
      }
      y += CELL;
    }
    if let Some(lever) = self.lever
      && let Some(mut object) = ctx.world.get_mut::<MapObject>(lever)
      && let Some(tile) = object.tile
    {
      object.tile = Some(Tile::new(tile.tileset, LEVER_PULLED).with_flip(tile.flip));
    }
    self.open = true;
  }
}

/// A coin's `value`: its own, or its tile's, which holds the Tiled
/// project's class default (1) the loader fills in. A coin with neither is
/// worth nothing, warned
fn coin_value(coin: &MapObject) -> i64 {
  coin.properties.int("value").unwrap_or_else(|| {
    game_warn!(
      "coin {} has no value: is project.tiled-project loaded?",
      coin.id
    );
    0
  })
}

impl DemoPage for ObjectsPage {
  fn name(&self) -> &str {
    "Objects"
  }

  fn description(&self) -> &str {
    "Coins, spikes and a door with its switch, placed in Tiled, spawned as entities the game gives behaviour"
  }

  fn controls(&self) -> Vec<Control> {
    vec![
      wasd("Run (pad: left stick)"),
      Control::key(KeyCode::Space, "Jump (pad: A)"),
    ]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    self
      .level
      .enter(ctx, |map| MapRoot::new(map).with_entity_tiles(&["spikes"]));
    ctx.world.insert_resource(self.tally);
  }

  fn exit(&mut self, ctx: &mut SceneContext) {
    // A resource outlives the page unless taken away
    ctx.world.remove_resource::<Coins>();
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    let movement = PlatformerMovement::default()
      .with_run(140.0, 0.08, 0.06)
      .with_jump(64.0, 0.36);
    if self.level.spawn_when_ready(ctx, movement) {
      self.wire(ctx);
    }
    let mut changed = false;
    for picked in ctx.messages::<PickedUp>() {
      if let Some(&(_, value)) = self.coins.iter().find(|(coin, _)| *coin == picked.pickup) {
        self.tally.collected += 1;
        self.tally.score += value;
        changed = true;
      }
    }
    if changed {
      ctx.world.insert_resource(self.tally);
    }
    for killed in ctx.messages::<Killed>() {
      if Some(killed.entity) == self.level.player {
        self.deaths += 1;
      }
    }
    if !self.open
      && let (Some(lever), Some(player)) = (self.lever, self.level.player)
      && ctx
        .world
        .get::<TriggerContents>(lever)
        .is_some_and(|inside| inside.contains(player))
    {
      self.open_door(ctx);
    }
    self.level.follow(ctx, &self.view, 2.0);
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    self.view.measure(ui);
    let area = self.view.area;
    let lines = vec![
      (
        format!(
          "Coins {} of {}, score {} (the prize behind the door is worth 10)",
          self.tally.collected, self.total, self.tally.score
        ),
        if self.tally.collected > 0 {
          HOT
        } else {
          Color::WHITE
        },
      ),
      (
        if self.open {
          "Door: open, its switch pulled".to_string()
        } else {
          "Door: shut; its switch property names the lever: touch it".to_string()
        },
        if self.open { GOOD } else { DIM },
      ),
      (
        format!(
          "Spikes: {} class tiles spawned as kill zones; back to the start {} times",
          self.spikes, self.deaths
        ),
        DIM,
      ),
    ];
    let height = hud_height(lines.len());
    hud(ui, area.x + 4.0, area.bottom() - height - 4.0, &lines);
  }
}
