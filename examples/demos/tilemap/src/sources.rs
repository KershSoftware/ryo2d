//! The Tiled and LDtk page: one level from each editor on one screen. The
//! platformer is a Tiled map (`maps/platformer.tmx`, XML, its ground layer
//! zlib-compressed), the vault a level of an LDtk project
//! (`ldtk/dungeon.ldtk`): both load through the asset registry and spawn
//! with `MapRoot`, and each says what it holds, read from its asset. The
//! vault sits under the platformer, not beside it: the platformer's
//! backdrop is an image layer repeated along x, as far as the view goes, as
//! Tiled repeats it, and it would cross a level beside the map

use demo_kit::prelude::*;

use crate::shared::{DIM, HOT, LDTK, PLATFORMER, View, asset, backdrop, hud, load, map_rect};

/// The LDtk level the page shows
const LEVEL: &str = "Vault";
/// Where the vault sits: under the platformer, clear of its backdrop band
const VAULT_AT: Vec2 = Vec2::new(0.0, 320.0);

pub(crate) struct SourcesPage {
  view: View,
  tiled: Option<Handle<TileMap>>,
  ldtk: Option<Handle<LdtkProject>>,
  /// Each map's place in the world and what it holds
  tiled_rect: Rect,
  vault_rect: Rect,
  tiled_lines: Vec<String>,
  vault_lines: Vec<String>,
}

impl SourcesPage {
  pub(crate) fn new() -> Self {
    Self {
      view: View::new(false),
      tiled: None,
      ldtk: None,
      tiled_rect: Rect::new(0.0, 0.0, 864.0, 288.0),
      vault_rect: Rect::new(VAULT_AT.x, VAULT_AT.y, 320.0, 240.0),
      tiled_lines: Vec::new(),
      vault_lines: Vec::new(),
    }
  }
}

/// A map's layers by name, bottom first, any but a tile layer saying what
/// it is
fn layer_list(map: &TileMap) -> String {
  map
    .layers
    .iter()
    .map(|layer| {
      let kind = match &layer.kind {
        LayerKind::Tiles {
          intgrid: Some(_), ..
        } => " (IntGrid)",
        LayerKind::Tiles { .. } => "",
        LayerKind::Objects(_) => " (objects)",
        LayerKind::Image(_) => " (image)",
      };
      format!("{}{kind}", layer.info.name)
    })
    .collect::<Vec<_>>()
    .join(", ")
}

/// How many objects a map's object layers hold
fn object_count(map: &TileMap) -> usize {
  map
    .layers
    .iter()
    .map(|layer| match &layer.kind {
      LayerKind::Objects(objects) => objects.len(),
      _ => 0,
    })
    .sum()
}

impl DemoPage for SourcesPage {
  fn name(&self) -> &str {
    "Tiled and LDtk"
  }

  fn description(&self) -> &str {
    "A Tiled map and an LDtk level, each loaded through the asset registry and spawned with MapRoot"
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    self.tiled = load::<TileMap>(ctx, PLATFORMER);
    self.ldtk = load::<LdtkProject>(ctx, LDTK);
    if let Some(map) = self.tiled.clone() {
      if let Some(data) = asset(ctx, &map) {
        self.tiled_rect = map_rect(&data, Vec2::ZERO);
        self.tiled_lines = vec![
          format!("Tiled: {PLATFORMER}"),
          format!(
            "{} x {} cells of {} px, {} tilesets, {} objects",
            data.size.x,
            data.size.y,
            data.tile_size.x,
            data.tilesets.len(),
            object_count(&data)
          ),
          layer_list(&data),
        ];
        let color = data.background.unwrap_or(Color::rgb(127, 240, 165));
        backdrop(ctx, self.tiled_rect, color, -100);
      }
      ctx.world.spawn((Transform::IDENTITY, MapRoot::new(map)));
    }
    if let Some(project) = self.ldtk.clone() {
      if let Some(level) = asset(ctx, &project).and_then(|p| p.level(LEVEL).cloned()) {
        self.vault_rect = Rect::new(
          VAULT_AT.x,
          VAULT_AT.y,
          level.size.x as f32,
          level.size.y as f32,
        );
        let title = level.fields.string("title").unwrap_or(LEVEL).to_string();
        self.vault_lines = vec![
          format!("LDtk: {LDTK}, level {LEVEL} (\"{title}\")"),
          format!(
            "{} x {} px of {} px cells, {} entities",
            level.size.x,
            level.size.y,
            level.map.tile_size.x,
            object_count(&level.map)
          ),
          layer_list(&level.map),
        ];
        let color = level.map.background.unwrap_or(Color::rgb(40, 30, 40));
        backdrop(ctx, self.vault_rect, color, -100);
      }
      ctx.world.spawn((
        Transform::from_translation(VAULT_AT),
        MapRoot::ldtk(project, LEVEL),
      ));
    }
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    self.view.measure(ui);
    // Both levels framed together, with room above each for its name
    let both = Rect::new(
      self.tiled_rect.x,
      self.tiled_rect.y - 24.0,
      self
        .tiled_rect
        .width
        .max(self.vault_rect.right() - self.tiled_rect.x),
      self.vault_rect.bottom() - self.tiled_rect.y + 24.0,
    );
    self.view.fit(ctx, both);

    let camera = ctx.camera();
    let tiled_top = camera.world_to_screen(Vec2::new(self.tiled_rect.x, self.tiled_rect.y));
    let vault_top = camera.world_to_screen(Vec2::new(self.vault_rect.x, self.vault_rect.y));
    let notes =
      camera.world_to_screen(Vec2::new(self.vault_rect.right() + 24.0, self.vault_rect.y));

    ui.draw_text("Tiled 1.12", tiled_top.x, tiled_top.y - 20.0, HOT);
    ui.draw_text("LDtk 1.5", vault_top.x, vault_top.y - 20.0, HOT);
    // What each holds, beside the vault
    let mut y = notes.y;
    for lines in [&self.tiled_lines, &self.vault_lines] {
      let lines: Vec<(String, Color)> = lines
        .iter()
        .enumerate()
        .map(|(i, line)| (line.clone(), if i == 0 { Color::WHITE } else { DIM }))
        .collect();
      y += hud(ui, notes.x, y, &lines) + 12.0;
    }
  }
}
