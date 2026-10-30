//! The Collision page: the hero on the platformer's own tiles. The ground
//! layer collides by its tiles' shapes from the tilesets: full cells as
//! solid, the hill's slopes as 45° slopes, the rope bridge and the planks as
//! one-way platforms (jump up through them, S + Space drops through); the
//! door's cells are solid too, the scenery and water layers do not collide.
//! The outlines are the map's colliders as physics has them: an outline
//! per chunk of solid and slope cells, and the one-way runs on their own

use std::f32::consts::{PI, TAU};

use demo_kit::prelude::*;

use crate::shared::{DIM, HOT, Player, SideLevel, View, hud_beside_panel, wasd};

/// One-way colliders' outlines
const ONE_WAY: Color = Color::rgb(255, 110, 200);

pub(crate) struct CollisionPage {
  view: View,
  level: SideLevel,
  outlines: bool,
  jump_height: f32,
  run_speed: f32,
  /// The movement the hero was given last, to see a setting change
  applied: (f32, f32),
}

impl CollisionPage {
  pub(crate) fn new() -> Self {
    Self {
      view: View::new(true),
      level: SideLevel::new(),
      outlines: true,
      jump_height: 64.0,
      run_speed: 140.0,
      applied: (0.0, 0.0),
    }
  }

  fn movement(&self) -> PlatformerMovement {
    PlatformerMovement::default()
      .with_run(self.run_speed, 0.08, 0.06)
      .with_jump(self.jump_height, 0.36)
  }
}

/// What the hero stands on, from its last move
fn floor_text(state: &CharacterState) -> String {
  if !state.on_floor {
    return "in the air".to_string();
  }
  let angle = (-state.floor_normal.y).clamp(-1.0, 1.0).acos().to_degrees();
  let kind = if state.floor_one_way {
    "a one-way tile"
  } else if angle > 1.0 {
    "a slope"
  } else {
    "solid ground"
  };
  format!("on {kind}, {angle:.0} degrees")
}

impl DemoPage for CollisionPage {
  fn name(&self) -> &str {
    "Collision"
  }

  fn description(&self) -> &str {
    "The hero on the map's own collision: solid tiles, 45 degree slopes and one-way bridge tiles"
  }

  fn settings(&mut self, s: &mut Settings) {
    s.toggle("Collider outlines", &mut self.outlines);
    s.float("Jump height", &mut self.jump_height, 24.0..=96.0, 4.0);
    s.float("Run speed", &mut self.run_speed, 60.0..=240.0, 10.0);
  }

  fn controls(&self) -> Vec<Control> {
    vec![
      wasd("Run (pad: left stick)"),
      Control::key(KeyCode::Space, "Jump, S + Space drops through (pad: A)"),
    ]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    self.level.enter(ctx, MapRoot::new);
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    if self.level.spawn_when_ready(ctx, self.movement()) {
      self.applied = (self.jump_height, self.run_speed);
    }
    if let Some(player) = self.level.player
      && self.applied != (self.jump_height, self.run_speed)
    {
      let movement = self.movement();
      if let Some(mut current) = ctx.world.get_mut::<PlatformerMovement>(player) {
        *current = movement;
      }
      self.applied = (self.jump_height, self.run_speed);
    }
    self.level.follow(ctx, &self.view, 2.0);
    if self.outlines {
      draw_colliders(ctx);
    }
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    self.view.measure(ui);
    let state = self
      .level
      .player
      .and_then(|player| ctx.world.get::<CharacterState>(player).cloned());
    let mut lines = vec![(
      "Ground: solid cells, slopes, one-way bridge and planks".to_string(),
      Color::WHITE,
    )];
    if let Some(state) = state {
      let color = if state.floor_one_way { ONE_WAY } else { HOT };
      lines.push((format!("Hero: {}", floor_text(&state)), color));
    }
    if self.outlines {
      lines.push((
        "Blue: solid outlines, pink: one-way, white: the hero".to_string(),
        DIM,
      ));
    }
    hud_beside_panel(ui, 3, &lines);
  }
}

/// Outlines every collider: the map's (its packed chunk outlines and
/// one-way runs) in blue, one-way in pink, triggers in gold, the player in
/// white. In entity index order, so overlapping outlines keep their colours
fn draw_colliders(ctx: &mut SceneContext) {
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
