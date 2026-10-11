//! The pass pipeline draws exactly what the old render paths drew.

use super::HeadlessEngine;
use crate::core::engine::EngineBuilder;
use crate::prelude::*;

/// Run the same scene through the old paths and the pipeline; every frame
/// must be byte-identical, and the pipeline must have run `expected` passes.
fn assert_paths_match(name: &str, make: impl Fn() -> EngineBuilder, steps: u32, expected: &[&str]) {
  let Some(mut legacy) = HeadlessEngine::new_or_skip(make()) else {
    return;
  };
  let Some(mut pipeline) = HeadlessEngine::new_or_skip(make()) else {
    return;
  };
  legacy.engine.legacy_render = true;
  pipeline.engine.legacy_render = false;

  for step in 1..=steps {
    legacy.step().unwrap();
    pipeline.step().unwrap();
    let (old, new) = (legacy.frame().unwrap(), pipeline.frame().unwrap());
    let differing = old
      .pixels()
      .zip(new.pixels())
      .filter(|(a, b)| a != b)
      .count();
    assert_eq!(
      differing, 0,
      "{name}: {differing} pixels differ at step {step}"
    );
  }

  let frame = pipeline
    .engine
    .last_frame_info()
    .expect("the pipeline rendered");
  assert_eq!(
    pipeline.engine.render_pipeline.passes_to_run(&frame),
    expected,
    "{name}: passes run"
  );
}

const STEPS: u32 = 5;

fn builder(scene: impl Scene + 'static) -> EngineBuilder {
  EngineBuilder::new().resolution(160, 120).start_scene(scene)
}

/// What a [`Showcase`] puts on screen.
#[derive(Debug, Default, Clone, Copy)]
struct Parts {
  /// Two overlapping CPU sprites at different z
  cpu_sprites: bool,
  /// Two GPU sprite instances added in `render_gpu`
  gpu_sprites: bool,
  /// A lit line and a `with_lighting(false)` line
  lines: bool,
  /// A trail on an entity that moves every update
  trail: bool,
  /// A point light (shadow-casting, with an occluder, when `shadows`)
  light: bool,
  shadows: bool,
  vignette: bool,
  bloom: bool,
  /// A UI rectangle and text
  ui: bool,
}

/// A deterministic 2D scene built from [`Parts`].
struct Showcase {
  parts: Parts,
  gpu_sprite: SpriteHandle,
  trail: Option<Entity>,
}

impl Showcase {
  fn new(parts: Parts) -> Self {
    Self {
      parts,
      gpu_sprite: SpriteHandle::from_sprite(
        Sprite::solid_rect(8, 8, 40, 200, 90, 255),
        "gpu-green",
      ),
      trail: None,
    }
  }
}

impl Scene for Showcase {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let parts = self.parts;
    if parts.cpu_sprites {
      let red = SpriteHandle::from_sprite(Sprite::solid_rect(30, 20, 230, 40, 40, 255), "red");
      let blue = SpriteHandle::from_sprite(Sprite::solid_rect(20, 30, 40, 80, 230, 255), "blue");
      ctx.world.spawn((
        Transform::new(60.0, 50.0),
        Renderable::sprite(red).with_z_order(1),
      ));
      ctx.world.spawn((
        Transform::new(70.0, 58.0),
        Renderable::sprite(blue).with_z_order(0),
      ));
    }
    if parts.lines {
      let line = |y: f32, color: Color| {
        Line2D::new(vec![Vec2::new(10.0, y), Vec2::new(150.0, y)], color, 4.0)
      };
      ctx.world.spawn((
        Transform::new(0.0, 0.0),
        line(90.0, Color::rgb(255, 255, 255)),
      ));
      ctx.world.spawn((
        Transform::new(0.0, 0.0),
        line(100.0, Color::rgb(255, 220, 0)).with_lighting(false),
      ));
    }
    if parts.trail {
      let trail = Trail2D::new(Color::rgb(0, 230, 230), 5.0);
      self.trail = Some(ctx.world.spawn((Transform::new(20.0, 75.0), trail)));
    }
    if parts.light {
      let mut light = Light2D::point(Color::rgb(255, 230, 200), 90.0);
      if parts.shadows {
        light = light.with_shadows();
        ctx.world.spawn((
          Transform::new(105.0, 45.0),
          LightOccluder2D::rect(14.0, 14.0),
        ));
      }
      ctx.world.spawn((Transform::new(80.0, 40.0), light));
    }
    // Effects in a chain share one uniform buffer and all read the last
    // effect's parameters, so the vignette goes last to stay visible
    if parts.bloom {
      ctx.effects.set_bloom(0.8);
    }
    if parts.vignette {
      ctx.effects.set_vignette_custom(0.8, 0.3, 0.3);
    }
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    if let Some(entity) = self.trail
      && let Some(mut transform) = ctx.world.get_mut::<Transform>(entity)
    {
      transform.x += 6.0;
      transform.y += 1.0;
    }
    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction {
    if self.parts.ui {
      ui.draw_rect(Rect::new(4.0, 4.0, 40.0, 10.0), Color::rgb(0, 255, 0));
      ui.draw_text("UI", 6.0, 106.0, Color::WHITE);
    }
    SceneAction::Continue
  }

  fn render_gpu(&mut self, ctx: &mut GpuRenderContext) {
    if !self.parts.gpu_sprites {
      return;
    }
    ctx.upload_sprite(&self.gpu_sprite);
    let id = self.gpu_sprite.id();
    for (x, y, size, z) in [(30.0, 30.0, 16.0, 0), (64.0, 60.0, 28.0, 1)] {
      let instance = SpriteInstance::new().with_transform(x, y, 0.0, size, size);
      ctx.add_sprite_instance(&id, instance, z, false);
    }
  }
}

#[test]
fn cpu_sprites_and_ui() {
  let parts = Parts {
    cpu_sprites: true,
    ui: true,
    ..Parts::default()
  };
  assert_paths_match(
    "cpu_sprites_and_ui",
    || builder(Showcase::new(parts)),
    STEPS,
    &["upload", "clear", "cpu_blend", "ui", "composite"],
  );
}

#[test]
fn gpu_and_cpu_sprites() {
  let parts = Parts {
    cpu_sprites: true,
    gpu_sprites: true,
    ..Parts::default()
  };
  assert_paths_match(
    "gpu_and_cpu_sprites",
    || builder(Showcase::new(parts)),
    STEPS,
    &["upload", "clear", "gpu_sprites", "cpu_blend", "composite"],
  );
}

#[test]
fn trails_without_lighting() {
  let parts = Parts {
    lines: true,
    trail: true,
    ..Parts::default()
  };
  assert_paths_match(
    "trails_without_lighting",
    || builder(Showcase::new(parts)),
    STEPS,
    &[
      "upload",
      "clear",
      "cpu_blend",
      "trails",
      "unlit_trails",
      "composite",
    ],
  );
}

#[test]
fn lighting_with_shadows() {
  let parts = Parts {
    cpu_sprites: true,
    gpu_sprites: true,
    lines: true,
    light: true,
    shadows: true,
    ..Parts::default()
  };
  assert_paths_match(
    "lighting_with_shadows",
    || builder(Showcase::new(parts)),
    STEPS,
    &[
      "upload",
      "clear",
      "gpu_sprites",
      "cpu_blend",
      "trails",
      "lighting",
      "unlit_trails",
      "composite",
    ],
  );
}

#[test]
fn effects_without_lighting() {
  let parts = Parts {
    cpu_sprites: true,
    vignette: true,
    bloom: true,
    ui: true,
    ..Parts::default()
  };
  assert_paths_match(
    "effects_without_lighting",
    || builder(Showcase::new(parts)),
    STEPS,
    &[
      "upload",
      "clear",
      "cpu_blend",
      "post_process",
      "ui",
      "composite",
    ],
  );
}

#[test]
fn lighting_effects_and_ui() {
  let parts = Parts {
    cpu_sprites: true,
    light: true,
    vignette: true,
    ui: true,
    ..Parts::default()
  };
  assert_paths_match(
    "lighting_effects_and_ui",
    || builder(Showcase::new(parts)),
    STEPS,
    &[
      "upload",
      "clear",
      "cpu_blend",
      "lighting",
      "post_process",
      "ui",
      "composite",
    ],
  );
}

/// A small raycast room with solid-colour textures, one billboard sprite, a
/// vignette and UI (modelled on the raycasting demo's `on_enter`).
struct RaycastRoom;

impl Scene for RaycastRoom {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let mut world = RaycastWorld::new(8, 8);
    for i in 0..8 {
      for (x, y) in [(i, 0), (i, 7), (0, i), (7, i)] {
        *world.get_mut(x, y).unwrap() = MapCell::wall(1);
      }
    }
    *world.get_mut(5, 3).unwrap() = MapCell::wall(2);
    for y in 1..7 {
      for x in 1..7 {
        let cell = world.get_mut(x, y).unwrap();
        if cell.wall_texture == 0 {
          cell.floor_texture = 1;
          cell.ceiling_texture = 2;
        }
      }
    }

    let solid = |rgba: [u8; 4]| rgba.repeat(16 * 16);
    let (walls, floors, sprites) = {
      let mut tex = ctx.textures();
      let walls = [
        tex.from_rgba(solid([200, 70, 50, 255]), 16, 16),
        tex.from_rgba(solid([90, 90, 120, 255]), 16, 16),
      ];
      let floors = [
        tex.from_rgba(solid([120, 90, 50, 255]), 16, 16),
        tex.from_rgba(solid([60, 60, 70, 255]), 16, 16),
      ];
      let sprites = [tex.from_rgba(solid([240, 200, 40, 255]), 16, 16)];
      (
        tex.create_array(&walls),
        tex.create_array(&floors),
        tex.create_array(&sprites),
      )
    };

    let rc = ctx.raycast_renderer().unwrap();
    rc.set_world(world);
    rc.set_map_info(MapInfoUniform {
      dimensions: [8, 8],
      max_distance: 16.0,
      fog_density: 0.04,
      fog_color: [0.05, 0.05, 0.1],
      diminish_factor: 0.06,
      ambient_color: [0.6, 0.65, 0.8],
      ambient_intensity: 0.5,
    });
    rc.set_wall_textures(walls);
    rc.set_floor_textures(floors);
    rc.set_sprite_textures(sprites);
    ctx.enable_raycast();

    ctx
      .world
      .spawn((Transform::new(1.5, 3.5), RaycastCamera::new(66.0)));
    let mut sprite = RaycastSprite::new(0);
    sprite.scale = (0.5, 0.5);
    sprite.v_offset = -0.25;
    ctx.world.spawn((Transform::new(3.5, 4.0), sprite));
    ctx.world.spawn((
      Transform::new(3.0, 3.0),
      RaycastLight::new(3.0, (1.0, 0.8, 0.4), 1.2),
    ));
    ctx.effects.set_vignette_custom(0.8, 0.3, 0.3);
  }

  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction {
    ui.draw_rect(Rect::new(4.0, 4.0, 40.0, 10.0), Color::rgb(0, 255, 0));
    ui.draw_text("UI", 6.0, 106.0, Color::WHITE);
    SceneAction::Continue
  }
}

#[test]
fn raycast_scene() {
  assert_paths_match(
    "raycast_scene",
    || builder(RaycastRoom),
    STEPS,
    &["raycast", "post_process", "ui", "composite"],
  );
}
