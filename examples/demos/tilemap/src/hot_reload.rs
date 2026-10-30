//! The Hot reload page: the platformer, spawned from `maps/platformer.tmx`,
//! follows its files while the demo runs. Save the map or one of its
//! tilesets in Tiled and the map is brought to the saved file in the frame
//! the save lands: its layers keep their entities, only the chunks the save
//! changed draw and collide again, and `MapEvent::Reloaded` is sent, which
//! the page counts. A tileset image's save redraws without a reload, and a
//! save that fails to load keeps the last good map

use demo_kit::prelude::*;

use crate::shared::{DIM, GOOD, HOT, View, hud, spawn_platformer};

/// The hot reload page's count of `MapEvent::Reloaded` for its map, kept as
/// a resource while the page is open so a test can read it
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct MapReloads(pub u32);

pub(crate) struct HotReloadPage {
  view: View,
  root: Option<Entity>,
  bounds: Rect,
  reloads: u32,
}

impl HotReloadPage {
  pub(crate) fn new() -> Self {
    Self {
      view: View::new(false),
      root: None,
      bounds: Rect::new(0.0, 0.0, 864.0, 288.0),
      reloads: 0,
    }
  }
}

impl DemoPage for HotReloadPage {
  fn name(&self) -> &str {
    "Hot reload"
  }

  fn description(&self) -> &str {
    "Save the map or a tileset in Tiled while the demo runs: the map follows the file, and the count goes up"
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    if let Some(level) = spawn_platformer(ctx, MapRoot::new) {
      self.root = Some(level.root);
      self.bounds = level.bounds;
    }
    ctx.world.insert_resource(MapReloads(0));
  }

  fn exit(&mut self, ctx: &mut SceneContext) {
    // A resource outlives the page unless taken away
    ctx.world.remove_resource::<MapReloads>();
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    let mut counted = false;
    for event in ctx.messages::<MapEvent>() {
      if let MapEvent::Reloaded { map } = event
        && Some(map) == self.root
      {
        self.reloads += 1;
        counted = true;
      }
    }
    if counted {
      ctx.world.insert_resource(MapReloads(self.reloads));
    }
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    self.view.measure(ui);
    // The map under the instructions
    let mut below = self.view;
    below.area.y += 110.0;
    below.area.height = (below.area.height - 150.0).max(1.0);
    below.fit(ctx, self.bounds);

    let area = self.view.area;
    let lines = vec![
      (
        "Open examples/demos/tilemap/assets/maps/platformer.tmx (or a tileset".to_string(),
        Color::WHITE,
      ),
      (
        "in tilesets/) in Tiled, change it and save: the map takes the saved file".to_string(),
        Color::WHITE,
      ),
      (
        "in the frame the save lands, keeping its entities. Debug builds with a".to_string(),
        DIM,
      ),
      (
        "window watch the folder; release, headless and web builds do not.".to_string(),
        DIM,
      ),
    ];
    hud(ui, area.x + 4.0, area.y + 4.0, &lines);
    let count = vec![(
      format!("MapEvent::Reloaded: {}", self.reloads),
      if self.reloads > 0 { GOOD } else { HOT },
    )];
    hud(ui, area.x + 4.0, area.bottom() - 30.0, &count);
  }
}
