//! On the GPU world path, entities, shadows, debug overlays and the
//! transition fade draw in the right layers.

use std::time::Duration;

use super::HeadlessEngine;
use crate::core::engine::EngineBuilder;
use crate::prelude::*;

const RED: [u8; 4] = [220, 30, 40, 255];
const MAGENTA: [u8; 4] = [255, 0, 255, 255];
const UI_BLUE: [u8; 4] = [30, 60, 230, 255];

/// A scene that spawns its entities in `on_enter`, draws a UI rect and idles.
struct Fixture {
  setup: fn(&mut SceneContext),
}

impl Scene for Fixture {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    (self.setup)(ctx);
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction {
    ui.draw_rect(Rect::new(4.0, 4.0, 10.0, 10.0), Color::rgb(30, 60, 230));
    SceneAction::Continue
  }
}

fn gpu_engine(builder: EngineBuilder) -> Option<HeadlessEngine> {
  let mut engine = HeadlessEngine::new_or_skip(builder)?;
  engine.engine.use_gpu_world();
  Some(engine)
}

#[test]
fn blob_shadow_under_its_entity() {
  let scene = Fixture {
    setup: |ctx| {
      let sprite =
        SpriteHandle::from_sprite(Sprite::solid_rect(10, 10, 220, 30, 40, 255), "blob-owner");
      let blob = BlobShadow {
        radius_x: 12.0,
        radius_y: 6.0,
        offset_y: 10.0,
        color: Color::new(0, 0, 0, 255),
        ..BlobShadow::default()
      };
      ctx.world.spawn((
        Transform::new(60.0, 40.0),
        Renderable::sprite(sprite).with_blob_shadow(blob),
      ));
    },
  };
  let Some(mut engine) = gpu_engine(EngineBuilder::new().resolution(120, 90).start_scene(scene))
  else {
    return;
  };
  engine.step_frames(2).unwrap();
  let frame = engine.frame().unwrap();
  let px = |x, y| frame.get_pixel(x, y).0;
  let background = px(110, 80);

  // Inside the ellipse and the sprite: the sprite covers the shadow
  assert_eq!(px(65, 48), RED);
  // Inside the ellipse only: the shadow shows
  assert_eq!(px(52, 50), [0, 0, 0, 255], "shadow beside the sprite");
  // Outside both
  assert_eq!(px(110, 80), background);
  assert_ne!(background, [0, 0, 0, 255]);
}

#[test]
fn debug_colliders_draw() {
  let scene = Fixture {
    setup: |ctx| {
      ctx.world.spawn((
        Transform::new(50.0, 40.0),
        Collider::box_collider(30.0, 20.0),
      ));
    },
  };
  let builder = EngineBuilder::new()
    .resolution(120, 90)
    .show_colliders(true)
    .start_scene(scene);
  let Some(mut engine) = gpu_engine(builder) else {
    return;
  };
  engine.step_frames(2).unwrap();
  let frame = engine.frame().unwrap();
  let green = frame
    .pixels()
    .filter(|p| p.0[1] > 200 && p.0[0] < 80 && p.0[2] < 80)
    .count();
  // The outline of a 30x20 box is about 96 pixels
  assert!(green > 60, "collider outline pixels: {green}");
  // Full brightness, not a faint smear: the top edge is on row 40
  assert_eq!(frame.get_pixel(60, 40).0, [0, 255, 0, 255]);
}

/// Draws a green line and a UI rect, and fades through magenta.
struct FadesOut {
  started: bool,
}

/// The scene pushed over `FadesOut`; only the top scene draws UI.
struct Next;

impl Scene for Next {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction {
    ui.draw_rect(Rect::new(4.0, 4.0, 10.0, 10.0), Color::rgb(30, 60, 230));
    SceneAction::Continue
  }
}

impl Scene for FadesOut {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let line =
      Line2D::horizontal(30.0, 10.0, 100.0, Color::new(0, 255, 0, 255), 4.0).with_lighting(false);
    ctx.world.spawn((Transform::new(0.0, 0.0), line));
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    if self.started {
      return SceneAction::Continue;
    }
    self.started = true;
    // Push keeps the world, so the line stays under the fade
    SceneAction::PushWith(
      Box::new(Next),
      Transition::fade_color(Duration::from_millis(200), 255, 0, 255),
    )
  }

  fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction {
    ui.draw_rect(Rect::new(4.0, 4.0, 10.0, 10.0), Color::rgb(30, 60, 230));
    SceneAction::Continue
  }
}

#[test]
fn transition_covers_trails_not_ui() {
  let builder = EngineBuilder::new()
    .resolution(120, 90)
    .start_scene(FadesOut { started: false });
  let Some(mut engine) = gpu_engine(builder) else {
    return;
  };
  assert!(engine.engine.render_pipeline.contains("transition"));

  // Step until the fade-out has fully covered the frame
  let mut covered = None;
  for _ in 0..40 {
    engine.step().unwrap();
    let frame = engine.frame().unwrap();
    if frame.get_pixel(60, 30).0 == MAGENTA {
      covered = Some(frame);
      break;
    }
  }
  let frame = covered.expect("the fade never covered the line");
  // The UI draws above the fade
  assert_eq!(frame.get_pixel(8, 8).0, UI_BLUE);
  // Background is covered too
  assert_eq!(frame.get_pixel(100, 70).0, MAGENTA);
}

#[test]
fn replaced_sprite_pixels_are_drawn_again() {
  let scene = Fixture {
    setup: |ctx| {
      let sprite =
        SpriteHandle::from_sprite(Sprite::solid_rect(10, 10, 220, 30, 40, 255), "reload");
      ctx.world.spawn((
        Transform::new(40.0, 40.0),
        Renderable::sprite(sprite.clone()),
      ));
    },
  };
  let Some(mut engine) = gpu_engine(EngineBuilder::new().resolution(120, 90).start_scene(scene))
  else {
    return;
  };
  engine.step_frames(2).unwrap();
  assert_eq!(engine.frame().unwrap().get_pixel(45, 45).0, RED);

  // A hot reload replaces the pixels behind the same handle
  let handle = {
    let world = engine.world();
    let entity = *world.entities_with::<Renderable>().first().unwrap();
    world.get::<Renderable>(entity).unwrap().sprite.clone()
  };
  handle.update(Sprite::solid_rect(10, 10, 10, 200, 30, 255));
  engine.step_frames(2).unwrap();
  assert_eq!(
    engine.frame().unwrap().get_pixel(45, 45).0,
    [10, 200, 30, 255]
  );
}

/// Changes the blob shadow's radius and colour every update.
struct VaryingBlob;

impl Scene for VaryingBlob {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let sprite = SpriteHandle::from_sprite(Sprite::solid_rect(8, 8, 220, 30, 40, 255), "varying");
    ctx.world.spawn((
      Transform::new(60.0, 40.0),
      Renderable::sprite(sprite).with_blob_shadow(BlobShadow::default()),
    ));
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    for entity in ctx.world.entities_with::<Renderable>() {
      if let Some(mut renderable) = ctx.world.get_mut::<Renderable>(entity)
        && let Some(blob) = renderable.blob_shadow.as_mut()
      {
        blob.radius_x += 0.37;
        blob.radius_y += 0.21;
        let shift = (blob.radius_x * 7.0) as u8;
        blob.color = Color::new(shift, 255 - shift, shift / 2, 200);
      }
    }
    SceneAction::Continue
  }
}

#[test]
fn varying_shadows_do_not_grow_the_texture_cache() {
  let builder = EngineBuilder::new()
    .resolution(120, 90)
    .start_scene(VaryingBlob);
  let Some(mut engine) = gpu_engine(builder) else {
    return;
  };
  let count = |engine: &HeadlessEngine| engine.engine.sprite_gpu.as_ref().unwrap().texture_count();
  engine.step().unwrap();
  let first = count(&engine);
  engine.step_frames(200).unwrap();
  assert_eq!(count(&engine), first, "shadow textures leaked");
}

#[test]
fn off_screen_entities_are_not_uploaded() {
  let scene = Fixture {
    setup: |ctx| {
      let near = SpriteHandle::from_sprite(Sprite::solid_rect(8, 8, 1, 2, 3, 255), "near");
      let far = SpriteHandle::from_sprite(Sprite::solid_rect(8, 8, 4, 5, 6, 255), "far");
      ctx
        .world
        .spawn((Transform::new(20.0, 20.0), Renderable::sprite(near)));
      ctx
        .world
        .spawn((Transform::new(5000.0, 20.0), Renderable::sprite(far)));
    },
  };
  let Some(mut engine) = gpu_engine(EngineBuilder::new().resolution(120, 90).start_scene(scene))
  else {
    return;
  };
  engine.step_frames(2).unwrap();
  let handles: Vec<SpriteHandle> = {
    let world = engine.world();
    world
      .entities_with::<Renderable>()
      .into_iter()
      .map(|e| world.get::<Renderable>(e).unwrap().sprite.clone())
      .collect()
  };
  let gpu = engine.engine.sprite_gpu.as_ref().unwrap();
  let uploaded = |name: &str| {
    handles
      .iter()
      .filter(|h| h.id() == name)
      .any(|h| gpu.has_entity_sprite(h))
  };
  assert!(uploaded("near"));
  assert!(!uploaded("far"), "an off-screen sprite was uploaded");
}

#[test]
fn composite_below_an_entity_draws_under_it() {
  let scene = Fixture {
    setup: |ctx| {
      let solid = |name: &str, c: [u8; 3]| {
        SpriteHandle::from_sprite(Sprite::solid_rect(20, 20, c[0], c[1], c[2], 255), name)
      };
      ctx.world.spawn((
        Transform::new(40.0, 40.0),
        Renderable::sprite(solid("entity-over", [220, 30, 40])).with_z_order(1),
      ));
      let composite = CompositeTile::new().add_part(TilePart::static_part(
        solid("tile-under", [30, 200, 60]),
        0.0,
        0.0,
      ));
      ctx
        .world
        .spawn((Transform::new(50.0, 40.0), composite.with_z_order(0)));
    },
  };
  let Some(mut engine) = gpu_engine(EngineBuilder::new().resolution(120, 90).start_scene(scene))
  else {
    return;
  };
  engine.step_frames(2).unwrap();
  let frame = engine.frame().unwrap();
  // Overlap (x 50..60): the entity at the higher z covers the composite
  assert_eq!(frame.get_pixel(55, 50).0, RED);
  // The composite's own part shows beyond the entity
  assert_eq!(frame.get_pixel(65, 50).0, [30, 200, 60, 255]);
}

#[test]
fn inactive_and_invisible_entities_do_not_draw() {
  let scene = Fixture {
    setup: |ctx| {
      let solid =
        |name: &str| SpriteHandle::from_sprite(Sprite::solid_rect(10, 10, 220, 30, 40, 255), name);
      ctx.world.spawn((
        Transform::new(10.0, 10.0),
        Renderable::sprite(solid("inactive")),
        Inactive,
      ));
      ctx.world.spawn((
        Transform::new(40.0, 10.0),
        Renderable::sprite(solid("hidden")).with_visible(false),
      ));
      ctx.world.spawn((
        Transform::new(70.0, 10.0),
        Renderable::sprite(solid("shown")),
      ));
    },
  };
  let Some(mut engine) = gpu_engine(EngineBuilder::new().resolution(120, 90).start_scene(scene))
  else {
    return;
  };
  engine.step_frames(2).unwrap();
  let frame = engine.frame().unwrap();
  assert_ne!(frame.get_pixel(15, 15).0, RED, "inactive entity drew");
  assert_ne!(frame.get_pixel(45, 15).0, RED, "invisible entity drew");
  assert_eq!(frame.get_pixel(75, 15).0, RED);
}
