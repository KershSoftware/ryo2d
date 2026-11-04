//! The character the town, the island and the dungeon share: a top-down
//! walker, Tiny Dungeon's hero scaled to the map's cells, y-sorted at a map
//! layer's z so it walks behind and in front of what that layer draws; and
//! where a map's `player_start` stands

use demo_kit::prelude::*;

use crate::shared::{bindings, hero};

/// A character's look and stride
#[derive(Debug, Clone, Copy)]
pub(crate) struct Walk {
  /// The hero on Tiny Dungeon's sheet
  pub(crate) sprite: u32,
  /// The sprite's scale: whole numbers keep its pixels square
  pub(crate) scale: f32,
  /// Its collider, a circle round its feet, in world pixels
  pub(crate) radius: f32,
  /// Its top speed, in world pixels a second
  pub(crate) speed: f32,
}

/// The character at `at` (its feet), walked by WASD or a pad, its sprite
/// y-sorted at `z`. Its feet are the collider's centre and the sprite's
/// bottom, which it sorts by
pub(crate) fn spawn_walker(ctx: &mut SceneContext, at: Vec2, walk: Walk, z: i32) -> Entity {
  let walker = ctx
    .world
    .spawn((
      Transform::from_translation(at),
      Interpolated,
      Collider::circle(walk.radius),
      TopDownMovement::default().with_speed(walk.speed, 0.06, 0.08),
      bindings(),
    ))
    .id();
  if let Some(image) = hero(ctx, walk.sprite) {
    ctx.spawn_child(
      walker,
      (
        Transform::IDENTITY.with_scale(Vec2::splat(walk.scale)),
        Renderable::image(image)
          .with_anchor(Anchor::BottomCenter)
          .with_z_order(z),
        YSort::new(),
      ),
    );
  }
  walker
}

/// Turns the character's sprite the way it walks
pub(crate) fn face(ctx: &mut SceneContext, walker: Entity) {
  let Some(dx) = ctx.world.get::<Velocity>(walker).map(|v| v.dx) else {
    return;
  };
  if dx.abs() < 1.0 {
    return;
  }
  let sprites: Vec<Entity> = ctx
    .world
    .get::<Children>(walker)
    .map(|children| children.iter().copied().collect())
    .unwrap_or_default();
  for sprite in sprites {
    if let Some(mut renderable) = ctx.world.get_mut::<Renderable>(sprite)
      && renderable.flip_x != (dx < 0.0)
    {
      renderable.flip_x = dx < 0.0;
    }
  }
}

/// Where the character is, once it has spawned
pub(crate) fn place_of(ctx: &SceneContext, walker: Option<Entity>) -> Option<Vec2> {
  walker
    .and_then(|walker| ctx.world.get::<Transform>(walker))
    .map(|transform| transform.translation)
}

/// Where the designer's `player_start` stands in the map's pixels: on
/// isometric and oblique maps the loader has already projected it
pub(crate) fn map_start(map: &TileMap) -> Option<Vec2> {
  map.layers.iter().find_map(|layer| {
    let LayerKind::Objects(objects) = &layer.kind else {
      return None;
    };
    objects
      .iter()
      .find(|object| object.class.as_deref() == Some("player_start"))
      .map(|object| layer.info.offset + object.position)
  })
}
