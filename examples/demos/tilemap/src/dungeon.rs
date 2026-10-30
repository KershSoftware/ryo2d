//! The Procgen dungeon page: `ryo2d-procgen` makes a grid of floor and
//! wall (BSP rooms joined by corridors, or cellular caves), and
//! `to_terrain_map` turns it into a `TileMap` drawn through Tiny Dungeon's
//! Wang set `dungeon` (from `tilesets/dungeon-terrain.tsj`): every corner
//! takes the strongest colour of the cells that meet there, and floor is
//! the set's higher colour, so corridors one cell wide keep their floor.
//! The map is an asset made in code, spawned with `MapRoot` like a file's

use demo_kit::prelude::*;
use ryo2d_procgen::prelude::*;

use crate::shared::{DIM, DUNGEON_TERRAIN, HOT, View, asset, hud_beside_panel, load};

/// The grid, in 16 px cells
const WIDTH: u32 = 32;
const HEIGHT: u32 = 30;
/// `ProcgenBuilder::build` times itself with `std::time::Instant`, which
/// panics on the web (wasm32 has no std clock, and the panic ends the
/// demo), so until procgen reads a web clock the web build makes no dungeon
const NO_PROCGEN: bool = cfg!(target_arch = "wasm32");
/// BSP rooms joined by corridors, or cellular caves
const GENERATORS: [&str; 2] = ["Rooms", "Caves"];

pub(crate) struct DungeonPage {
  view: View,
  terrain: Option<Handle<Tileset>>,
  set: Option<WangSet>,
  root: Option<Entity>,
  generator: usize,
  seed: i32,
  /// What the map on screen was made from
  built: Option<(usize, i32)>,
  rooms: usize,
  floor: usize,
}

impl DungeonPage {
  pub(crate) fn new() -> Self {
    Self {
      view: View::new(true),
      terrain: None,
      set: None,
      root: None,
      generator: 0,
      seed: 7,
      built: None,
      rooms: 0,
      floor: 0,
    }
  }

  /// A new grid from the settings, as a map in place of the last one
  fn generate(&mut self, ctx: &mut SceneContext) {
    let (Some(terrain), Some(set)) = (self.terrain.clone(), self.set.as_ref()) else {
      return;
    };
    if let Some(root) = self.root.take() {
      ctx.world.despawn(root);
    }
    if NO_PROCGEN {
      self.built = Some((self.generator, self.seed));
      return;
    }
    let builder = ProcgenBuilder::new(WIDTH, HEIGHT).with_seed(self.seed as u64);
    let result = match self.generator {
      0 => builder
        .fill(TileType::Wall)
        .bsp(BspConfig::default().with_extra_connectivity(0.2))
        .build(),
      _ => builder.cellular(CellularConfig::cave_preset()).build(),
    };
    let grid = result.grid();
    self.rooms = grid.regions().len();
    self.floor = grid.iter().filter(|(_, _, t)| t.is_walkable()).count();
    let map = grid.to_terrain_map(
      UVec2::splat(16),
      TilesetSource::Asset(terrain),
      set,
      |tile| match tile {
        TileType::Floor | TileType::Door => Some("floor"),
        _ => Some("wall"),
      },
    );
    let map = ctx.assets().add(map);
    self.root = Some(
      ctx
        .world
        .spawn((Transform::IDENTITY, MapRoot::new(map)))
        .id(),
    );
    self.built = Some((self.generator, self.seed));
  }
}

impl DemoPage for DungeonPage {
  fn name(&self) -> &str {
    "Procgen dungeon"
  }

  fn description(&self) -> &str {
    "A generated grid turned into a map by a Wang set: rooms or caves whose walls and floors join up"
  }

  fn settings(&mut self, s: &mut Settings) {
    s.choice("Generator", &mut self.generator, &GENERATORS);
    s.int("Seed", &mut self.seed, 1..=9999);
  }

  fn controls(&self) -> Vec<Control> {
    vec![Control::keys(
      "Space / G",
      &[KeyCode::Space, KeyCode::KeyG],
      "A new dungeon (the next seed; pad: A)",
    )]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    self.terrain = load::<Tileset>(ctx, DUNGEON_TERRAIN);
    self.set = self
      .terrain
      .clone()
      .and_then(|terrain| asset(ctx, &terrain))
      .and_then(|tileset| tileset.wang_set("dungeon").cloned());
    ctx.camera().set_zoom(1.0);
    self.generate(ctx);
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    if main_action_or_g(ctx.input) {
      self.seed = if self.seed >= 9999 { 1 } else { self.seed + 1 };
    }
    if self.built != Some((self.generator, self.seed)) {
      self.generate(ctx);
    }
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    self.view.measure(ui);
    let world = Rect::new(0.0, 0.0, (WIDTH * 16) as f32, (HEIGHT * 16) as f32);
    self.view.fit(ctx, world);
    let lines = if NO_PROCGEN {
      vec![
        ("Not on the web yet: procgen's builder".to_string(), HOT),
        ("reads std::time::Instant, which the web".to_string(), DIM),
        ("lacks. The native demo shows this page.".to_string(), DIM),
      ]
    } else {
      vec![
        (
          format!(
            "Seed {}: {} regions, {} floor cells of {}",
            self.seed,
            self.rooms,
            self.floor,
            WIDTH * HEIGHT
          ),
          HOT,
        ),
        (
          "Floor is the higher colour: one-cell corridors keep it".to_string(),
          DIM,
        ),
      ]
    };
    hud_beside_panel(ui, 2, &lines);
  }
}

/// Space, G or the pad's South button this step
fn main_action_or_g(input: &Input) -> bool {
  crate::shared::main_action(input) || input.key_pressed(KeyCode::KeyG)
}
