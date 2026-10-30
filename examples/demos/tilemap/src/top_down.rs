//! The Top-down page: the garden (`maps/topdown.tmj`). Its trees layer is
//! y-sorted (`y_sort` true in Tiled), so the hero walks behind a pine's
//! crown and in front of its trunk; its clouds layer has parallax 1.25 and
//! drifts over everything as the camera follows; its backdrop is an image
//! layer repeated along x at parallax 0.5. Trunks and statue bases collide

use demo_kit::prelude::*;

use crate::shared::{
  DIM, HOT, View, asset, backdrop, fence, hero, hud_beside_panel, load, map_rect, object_point,
  place_of, spawn_top_down_player, wasd,
};

/// The garden: y-sorted trees, parallax clouds and an image layer
const TOPDOWN: &str = "maps/topdown.tmj";
/// The hero's sprite: a Tiny Dungeon knight
const HERO: u32 = 97;

pub(crate) struct TopDownPage {
  view: View,
  map: Option<Handle<TileMap>>,
  root: Option<Entity>,
  bounds: Rect,
  start: Vec2,
  /// The trees layer's index, which is its z (the root's z is 0)
  trees_z: i32,
  player: Option<Entity>,
  y_sort: bool,
  clouds: bool,
  zoom: f32,
}

impl TopDownPage {
  pub(crate) fn new() -> Self {
    Self {
      view: View::new(true),
      map: None,
      root: None,
      bounds: Rect::new(0.0, 0.0, 480.0, 320.0),
      start: Vec2::new(240.0, 200.0),
      trees_z: 2,
      player: None,
      y_sort: true,
      clouds: true,
      zoom: 2.0,
    }
  }

  /// Sets `layer`'s `LayerInfo` with `change`, if it differs
  fn set_layer(&self, ctx: &mut SceneContext, layer: &str, change: impl Fn(&mut LayerInfo)) {
    let Some(entity) = self
      .root
      .and_then(|root| ctx.maps().layer_entity(root, layer))
    else {
      return;
    };
    if let Some(mut info) = ctx.world.get_mut::<LayerInfo>(entity) {
      let mut next = info.clone();
      change(&mut next);
      if *info != next {
        *info = next;
      }
    }
  }
}

impl DemoPage for TopDownPage {
  fn name(&self) -> &str {
    "Top-down"
  }

  fn description(&self) -> &str {
    "Y-sorted trees the hero walks among, clouds at parallax 1.25 and a backdrop image layer at 0.5"
  }

  fn settings(&mut self, s: &mut Settings) {
    s.toggle("Y-sort trees", &mut self.y_sort);
    s.toggle("Clouds", &mut self.clouds);
    s.float("Zoom", &mut self.zoom, 1.0..=4.0, 1.0);
  }

  fn controls(&self) -> Vec<Control> {
    vec![wasd("Walk (pad: left stick)")]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    self.map = load::<TileMap>(ctx, TOPDOWN);
    let Some(map) = self.map.clone() else {
      return;
    };
    if let Some(data) = asset(ctx, &map) {
      self.bounds = map_rect(&data, Vec2::ZERO);
      self.start = object_point(&data, "player_start").unwrap_or(self.start);
      self.trees_z = data.layer_index("trees").map_or(2, |i| i as i32);
      let ground = data.background.unwrap_or(Color::rgb(46, 176, 130));
      backdrop(ctx, self.bounds, ground, -100);
    }
    self.root = Some(
      ctx
        .world
        .spawn((Transform::IDENTITY, MapRoot::new(map)))
        .id(),
    );
    fence(ctx, self.bounds);
    let at = self.start;
    self.view.follow(ctx, at, self.zoom, self.bounds, 1.0);
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    let Some(root) = self.root else {
      return;
    };
    if self.player.is_none() && ctx.maps().is_ready(root) {
      let sprite = hero(ctx, HERO);
      self.player = Some(spawn_top_down_player(ctx, self.start, sprite, self.trees_z));
    }
    let (y_sort, clouds) = (self.y_sort, self.clouds);
    self.set_layer(ctx, "trees", |info| info.y_sorted = y_sort);
    self.set_layer(ctx, "clouds", |info| info.visible = clouds);
    if let Some(at) = place_of(ctx, self.player) {
      self.view.follow(ctx, at, self.zoom, self.bounds, 0.15);
    }
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    self.view.measure(ui);
    let camera = ctx.camera().position();
    let lines = vec![
      (
        format!(
          "Trees: {}",
          if self.y_sort {
            "y-sorted rows, the hero among them by its feet"
          } else {
            "one flat layer, drawn under the hero"
          }
        ),
        if self.y_sort { Color::WHITE } else { HOT },
      ),
      (
        "Clouds: parallax 1.25, over everything; backdrop: 0.5".to_string(),
        DIM,
      ),
      (format!("Camera at {:.0}, {:.0}", camera.x, camera.y), DIM),
    ];
    hud_beside_panel(ui, 3, &lines);
  }
}
