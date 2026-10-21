//! Drawing the world: scene GPU sprites, entities, trails, lines, lights and
//! overlays into the frame's draw list, then the render pipeline.

use web_time::Instant;

use super::{Engine, PresentOutcome, SceneHost};
use crate::core::config::DebugStats;
use crate::core::config::debug::{OverlayFlags, overlay_shapes};
use crate::core::ecs::interpolate::render_global;
use crate::core::ecs::kernel::{QueryState, SystemIdMarker};
use crate::core::ecs::{
  DetectChangesMut, Entity, GlobalTransform, IsResource, Mut, Observer, PreviousGlobalTransform,
  Vec2, World,
};
use crate::core::graphics::camera_gpu::CameraGlobals;
use crate::core::graphics::camera_mut::main_view;
use crate::core::graphics::renderer::FrameError;
use crate::core::graphics::{
  Camera2d, CameraView, GpuRenderContext, Presentation, RenderLayers, RenderTargets, ScreenMapping,
};
use crate::core::graphics::{ParallaxLayer, parallax};
use crate::core::lighting::{FrameLight, Light2D, LightOccluder2D, LightingSettings};
use crate::core::render::batch::{Filter, LineLook, line_points, push_polyline, trail_points};
use crate::core::render::scratch::recycle;
use crate::core::render::text::{
  TextSources, TextTargets, WorldTextFrame, emit_world_text, layout_world_text, prewarm,
};
use crate::core::render::ui::{UI_CAMERA, ui_view};
use crate::core::render::ui_retain::UiRenderEnv;
use crate::core::render::{
  DebugDraw, DrawKind, FrameScene, LightingFrame, RenderPipeline, SpriteLook, SpriteShading,
  UiRenderStats, plan_cameras,
};
use crate::core::shaders::EffectsManager;
use crate::core::text::{Fonts, TextEffects, TextIcons};
use crate::core::time::Time;
use crate::core::trail::{Line2D, Trail2D};
use crate::profile_scope;

impl Engine {
  /// The scene transition's fade colour and opacity.
  fn transition_overlay(&self) -> ([u8; 3], f32) {
    let Some(host) = self.world.get_non_send::<SceneHost>() else {
      return ([0; 3], 0.0);
    };
    let [r, g, b, _] = host
      .scenes
      .transition_overlay_color()
      .unwrap_or([0, 0, 0, 255]);
    ([r, g, b], host.scenes.transition_overlay_alpha())
  }

  /// Render and present one frame: the single present path for window and
  /// headless runs.
  pub(super) fn present_frame(&mut self) -> PresentOutcome {
    let outcome = self.draw_frame();
    // This frame's debug shapes were drawn or skipped (the last fixed
    // step's stay until the next step)
    self.world.resource_mut::<DebugDraw>().end_frame();
    outcome
  }

  fn draw_frame(&mut self) -> PresentOutcome {
    let render_start = Instant::now();
    // Taken out for the frame so they can be used beside `self.world`
    // A minimised window draws nothing until it comes back
    if self.minimized {
      return PresentOutcome::Skipped;
    }
    let (Some(mut rend), Some(mut gpu), Some(mut batch), Some(mut ui_renderer)) = (
      self.renderer.take(),
      self.sprite_gpu.take(),
      self.batch.take(),
      self.ui_renderer.take(),
    ) else {
      return PresentOutcome::Skipped;
    };
    let mut lighting = self.lighting.take();

    let camera = main_view(&mut self.world);
    let (alpha, real_time, game_time) = {
      let time = self.world.resource::<Time>();
      (
        time.fixed.overstep_fraction(),
        time.real.elapsed(),
        time.game.elapsed(),
      )
    };

    let (entity_sprites, lights, ui_kept, ui_frame) = {
      profile_scope!("cpu:render");

      // The scenes' GPU-batched sprites
      self.batch_collector.clear();
      if let Some(mut host) = self.world.remove_non_send::<SceneHost>() {
        let mut gpu_ctx = GpuRenderContext::new(
          &mut self.batch_collector,
          &mut gpu,
          &camera,
          rend.screen_size(),
          &self.world,
        );
        host.scenes.render(&mut gpu_ctx);
        self.world.insert_non_send(host);
      }

      if let Some(path) = self.capture.path_for_next_frame() {
        rend.request_capture(path);
      }

      // Lights at their world position (occluders are gathered with the
      // frame's triangles, for their shadow volumes)
      let mut lights = std::mem::take(&mut self.lights);
      lights_to_draw(&mut self.world, alpha, &mut lights);
      if let Some(lighting) = lighting.as_mut() {
        let mut settings = self.world.resource_mut::<LightingSettings>();
        lighting.set_enabled(
          settings
            .bypass_change_detection()
            .resolve(!lights.is_empty()),
        );
        let settings = self.world.resource::<LightingSettings>();
        lighting.set_ambient(*settings.ambient());
        lighting.set_atmosphere(settings.atmosphere().clone());
        let mut effects = self.world.resource_mut::<EffectsManager>();
        let lights_at = lights.iter().map(|l| (l.position, &l.light));
        crate::core::lighting::update_god_rays(
          &mut effects,
          lights_at,
          &camera,
          rend.screen_size(),
          lighting.atmosphere(),
        );
      }

      // The render targets' textures, then the cameras in render order and
      // each one's uniforms: its matrix, image size, clocks and pixel
      // snapping (sprite effects and custom sprite shaders animate on game
      // time)
      let targets = self.world.get_resource::<RenderTargets>();
      if let Some(targets) = targets {
        gpu.sync_targets(rend.device(), targets);
      }
      let lit = lighting
        .as_ref()
        .is_some_and(|lighting| lighting.is_enabled());
      let mut cameras = self
        .world
        .query::<(Entity, &Camera2d, &CameraView, Option<&RenderLayers>)>();
      plan_cameras(
        cameras.iter(&self.world),
        rend.render_size(),
        lit,
        self.world.get_resource::<RenderTargets>(),
        &mut self.camera_draws,
        &mut self.targetless_cameras,
      );
      for &camera in &self.targetless_cameras {
        if self.warned_targetless.insert(camera) {
          crate::engine_warn!(
            module: "Render",
            "{camera:?} renders into a render target that does not exist; it draws nothing"
          );
        }
      }
      if let Some(lighting) = lighting.as_mut() {
        lighting.sync_cameras(rend.device(), &self.camera_draws);
      }
      let pixel_art = matches!(self.config.presentation, Presentation::PixelArt { .. });
      // Their uniforms are synced once the UI camera is known (below)
      self.camera_globals.clear();
      self
        .camera_globals
        .extend(self.camera_draws.iter().map(|camera| {
          let size = camera.rect.size();
          let globals = CameraGlobals::new(&camera.view, size, game_time, real_time, pixel_art);
          (camera.entity, globals)
        }));

      self.draw_list.clear();
      self
        .batch_collector
        .push_to(&mut self.draw_list, DrawKind::Sprite, &mut gpu);
      // Parallax layers, laid out for each camera from its own view
      let mut quads = std::mem::take(&mut self.parallax);
      for (layer, layers) in self
        .world
        .query::<(&ParallaxLayer, Option<&RenderLayers>)>()
        .iter(&self.world)
      {
        let layers = layers.copied().unwrap_or_default();
        let texels = Vec2::new(layer.sprite.width() as f32, layer.sprite.height() as f32);
        let texture = gpu.texture_of(&layer.sprite);
        let look = SpriteLook {
          shading: SpriteShading::Plain,
          blend: layer.blend,
          filter: layer.filter,
          lit: layer.lighting,
        };
        for camera in &self.camera_draws {
          if !camera.layers.intersects(&layers) {
            continue;
          }
          parallax::layout(layer, texels, &camera.view, &mut quads);
          for quad in &quads {
            let seen = (layers, None, Some(camera.entity));
            let z = layer.z_order;
            let list = &mut self.draw_list;
            list.push_sprite_in(DrawKind::Sprite, z, texture, look, *quad, seen);
          }
        }
      }
      let entity_sprites = self.entity_drawer.collect(
        &mut self.world,
        &self.camera_draws,
        alpha,
        &mut gpu,
        rend.device(),
        rend.queue(),
        &mut self.draw_list,
      );
      self.parallax = quads;
      // World text: lay out what changed, build the glyphs asked for ahead,
      // then each camera's glyph quads (none if the game removed `Fonts`)
      if self.world.contains_resource::<Fonts>() {
        layout_world_text(&mut self.world, &mut self.text, &self.prompts);
        let render_scale = self
          .world
          .get_resource::<ScreenMapping>()
          .map_or(1.0, |mapping| {
            mapping.render_size().0 as f32 / mapping.screen_size().x
          });
        let mut text_targets = TextTargets {
          raster: &mut self.glyph_raster,
          sprites: &mut gpu,
          device: rend.device(),
          queue: rend.queue(),
          list: &mut self.draw_list,
          warned: &mut self.text_warned,
        };
        if let Some(mut fonts) = self.world.get_resource_mut::<Fonts>() {
          prewarm(
            fonts.bypass_change_detection(),
            &self.text,
            &mut text_targets,
          );
        }
        let text_frame = WorldTextFrame {
          cameras: &self.camera_draws,
          render_scale,
          alpha,
          game_time,
        };
        emit_world_text(&mut self.world, &self.text, &text_frame, &mut text_targets);
      }
      // Trails (world-space points), lines (at their entity) and the debug
      // overlays become triangles, one draw-list item each
      let triangles = batch.triangles();
      triangles.clear();
      // Lights, each with its shadow volumes, for the lit cameras whose
      // layers and view reach them
      let mut occluders = recycle(std::mem::take(&mut self.occluders));
      if lit {
        let mut query = self.world.query::<OccluderItem>();
        occluders_to_draw(&self.world, &mut query, alpha, &mut occluders);
        for light in &lights {
          if let Some(item) = crate::core::lighting::light_item(light, &occluders, triangles) {
            self.draw_list.push_light(item);
          }
        }
      }
      self.occluders = recycle(occluders);
      let mut points = std::mem::take(&mut self.poly_points);
      for (trail, layers) in self
        .world
        .query::<(&Trail2D, Option<&RenderLayers>)>()
        .iter(&self.world)
      {
        let Some(style) = trail_points(trail, &mut points) else {
          continue;
        };
        let look = LineLook {
          lit: trail.lighting(),
          z: trail.z_order(),
          texture: trail.texture().map(|sprite| gpu.texture_of(sprite)),
          blend: trail.blend(),
          filter: trail.filter(),
          layers: layers.copied().unwrap_or_default(),
          only_camera: None,
        };
        push_polyline(&mut self.draw_list, triangles, &points, &style, look);
      }
      let line_look = |line: &Line2D, layers: Option<&RenderLayers>| LineLook {
        lit: line.lighting(),
        z: line.z_order(),
        texture: None,
        blend: line.blend(),
        filter: Filter::Default,
        layers: layers.copied().unwrap_or_default(),
        only_camera: None,
      };
      for (global, previous, line, layers) in self
        .world
        .query::<(
          &GlobalTransform,
          Option<&PreviousGlobalTransform>,
          &Line2D,
          Option<&RenderLayers>,
        )>()
        .iter(&self.world)
      {
        let at = render_global(global, previous, alpha).translation();
        if let Some(style) = line_points(line, at, &mut points) {
          let look = line_look(line, layers);
          push_polyline(&mut self.draw_list, triangles, &points, &style, look);
        }
      }
      // The overlays join the debug shapes, which each camera builds in its
      // own screen pixels
      let flags = OverlayFlags {
        colliders: self.config.debug.show_colliders,
        velocities: self.config.debug.show_velocities,
        origins: self.config.debug.show_origins,
      };
      self
        .world
        .resource_scope(|world, mut debug: Mut<DebugDraw>| {
          overlay_shapes(world, flags, &mut debug);
        });
      let debug = self.world.resource::<DebugDraw>();
      for camera in &self.camera_draws {
        debug.push_for_camera(camera, &mut self.draw_list, triangles, &mut points);
      }
      self.poly_points = points;
      // The UI, last: the frame's UI commands through the UI camera. On the
      // pixel-art target it snaps like the world; at window resolution it
      // does not
      let ui_on_target = pixel_art && self.ui_space.image == rend.render_size();
      let sources = match (
        self.world.get_resource::<Fonts>(),
        self.world.get_resource::<TextEffects>(),
        self.world.get_resource::<TextIcons>(),
      ) {
        (Some(fonts), Some(effects), Some(icons)) => Some(TextSources {
          fonts,
          engine: &self.text,
          effects,
          icons,
        }),
        // A game that removed the text resources draws its UI without text
        _ => None,
      };
      let mut text_targets = TextTargets {
        raster: &mut self.glyph_raster,
        sprites: &mut gpu,
        device: rend.device(),
        queue: rend.queue(),
        list: &mut self.draw_list,
        warned: &mut self.text_warned,
      };
      // An unchanged UI keeps the UI image: none of it is built, uploaded
      // or drawn (render module notes, "UI render retention")
      let retain = self.config.debug.ui_retain;
      if !retain {
        self.ui_retain.clear();
      }
      let env = UiRenderEnv {
        space: self.ui_space,
        on_target: ui_on_target,
        text: sources.is_some(),
        fonts: self.text.generation(),
        bitmaps: self
          .world
          .get_resource::<Fonts>()
          .map_or(0, Fonts::bitmap_stamp),
      };
      let icons = sources.as_ref().map(|sources| sources.icons);
      let ui_kept = retain
        && self
          .ui_retain
          .keep(&self.ui_commands, icons, env, rend.ui_image_epoch());
      // A changed UI is built by the UI renderer. It is drawn when it
      // recorded anything, also when all of it is culled: the `ui` pass then
      // clears the UI image, so an unchanged next frame keeps it
      let ui_built = !ui_kept;
      if ui_built {
        ui_renderer.build(
          &self.ui_commands,
          env,
          retain,
          sources.as_ref(),
          &mut text_targets,
          real_time,
        );
      }
      // Bind groups hold their textures alive: a kept UI image keeps only
      // those it was drawn with, and a frame without UI none
      if ui_kept {
        ui_renderer.image_kept();
      } else if self.ui_commands.is_empty() {
        ui_renderer.release();
      }
      let ui_pass = ui_built && !self.ui_commands.is_empty();
      let (texts_built, texts_reused) = if ui_built {
        ui_renderer.text_counts()
      } else {
        (0, 0)
      };
      self.ui_retain.stats = UiRenderStats {
        kept: ui_kept,
        texts_built,
        texts_reused,
        glyph_evictions: gpu.glyph_evictions(),
      };
      // The UI camera's uniforms (the UI renderer binds the bind group the
      // batch makes for them)
      if ui_pass {
        let globals = CameraGlobals::new(
          &ui_view(&self.ui_space),
          self.ui_space.image,
          game_time,
          real_time,
          ui_on_target,
        );
        self.camera_globals.push((UI_CAMERA, globals));
      }
      self
        .camera_uniforms
        .sync(rend.device(), rend.queue(), &self.camera_globals);
      batch.sync_cameras(rend.device(), &self.camera_uniforms);
      self.draw_list.sorted();
      // The UI renderer's sprite textures are made resident with the
      // world's, then its frame is finished with their slots
      let ui_textures = if ui_built {
        ui_renderer.sprite_textures()
      } else {
        &[]
      };
      batch.prepare(
        rend.device(),
        rend.queue(),
        &self.draw_list,
        &self.camera_draws,
        &mut gpu,
        ui_textures,
      );
      let ui_frame = match batch.camera_group(UI_CAMERA) {
        Some(camera) if ui_pass => Some(ui_renderer.finish(
          rend.device(),
          rend.queue(),
          &gpu,
          self.config.presentation,
          camera,
        )),
        _ => None,
      };
      self.text.end_frame();
      (entity_sprites, lights, ui_kept, ui_frame)
    };

    let ui_draws = ui_frame.map_or(0, |frame| frame.draw_count());
    self.ui_draw_count = ui_draws as u32;
    self.debug_stats = DebugStats {
      entity_count: game_entity_count(&self.world),
      draw_calls: (batch.draw_count() + ui_draws) as u32,
      sprite_count: (entity_sprites + self.batch_collector.sprite_count()) as u32,
    };
    self.frame_timings.render = render_start.elapsed();

    profile_scope!("gpu:present");
    let present_start = Instant::now();
    let overlay = self.transition_overlay();
    let result = self
      .world
      .resource_scope(|world, mut pipeline: Mut<RenderPipeline>| {
        let world: &World = world;
        let lighting = lighting.as_mut().filter(|l| l.is_enabled());
        let scene = FrameScene {
          sprites: &gpu,
          batch: Some(&batch),
          draw_list: &self.draw_list,
          lighting: lighting.map(|renderer| LightingFrame { renderer }),
          effects: world.resource::<EffectsManager>(),
          look: world.resource::<crate::core::render::Look>(),
          ui_frame,
          ui_size: self.ui_space.image,
          ui_kept,
          cameras: &self.camera_draws,
          time: real_time,
          overlay,
        };
        rend.render_frame(&mut pipeline, scene)
      });

    // What the UI image holds now
    match &result {
      Ok(()) => self
        .ui_retain
        .presented(ui_kept, rend.ui_drawn(), rend.ui_image_epoch()),
      Err(_) => self.ui_retain.lost(),
    }

    let outcome = match result {
      Ok(()) => {
        if rend.take_needs_reconfigure() {
          self.surface_needs_reconfigure = true;
        }
        PresentOutcome::Presented
      }
      // Recoverable: reconfigure and draw the next frame
      Err(FrameError::Skip) => PresentOutcome::Skipped,
      Err(FrameError::Reconfigure) => {
        self.surface_needs_reconfigure = true;
        PresentOutcome::Skipped
      }
      Err(FrameError::Recreate) => {
        if let Some(window) = &self.window {
          let size = window.inner_size();
          rend.resize(size.width, size.height);
        }
        PresentOutcome::Skipped
      }
      Err(FrameError::Fatal(message)) => {
        crate::engine_error!(module: "Render", "{}", message);
        PresentOutcome::Fatal(message)
      }
    };

    self.smoothed_stats.update(&self.debug_stats);
    self.renderer = Some(rend);
    self.sprite_gpu = Some(gpu);
    self.batch = Some(batch);
    self.ui_renderer = Some(ui_renderer);
    self.lighting = lighting;
    self.lights = lights;
    self.frame_timings.present = present_start.elapsed();

    if outcome == PresentOutcome::Presented {
      self.on_frame_presented();
    }
    outcome
  }
}

/// The entities the game made: every entity but bevy's resources, observers
/// and registered systems. Counted per archetype, so the cost does not grow
/// with the entity count.
fn game_entity_count(world: &World) -> u32 {
  let internal: Vec<_> = [
    world.component_id::<IsResource>(),
    world.component_id::<Observer>(),
    world.component_id::<SystemIdMarker>(),
    world.component_id::<crate::core::graphics::MainCamera>(),
  ]
  .into_iter()
  .flatten()
  .collect();
  world
    .archetypes()
    .iter()
    .filter(|archetype| !internal.iter().any(|&id| archetype.contains(id)))
    .map(|archetype| archetype.len())
    .sum()
}

/// The lights to draw this frame, at their world positions (blended for
/// interpolated entities, like their sprites), with the layers that see them.
fn lights_to_draw(world: &mut World, alpha: f32, out: &mut Vec<FrameLight>) {
  out.clear();
  out.extend(
    world
      .query::<(
        &GlobalTransform,
        Option<&PreviousGlobalTransform>,
        &Light2D,
        Option<&RenderLayers>,
      )>()
      .iter(world)
      .map(|(global, previous, light, layers)| FrameLight {
        position: render_global(global, previous, alpha).translation(),
        light: light.clone(),
        layers: layers.copied().unwrap_or_default(),
      }),
  );
}

type OccluderItem = (
  &'static GlobalTransform,
  Option<&'static PreviousGlobalTransform>,
  &'static LightOccluder2D,
);

/// Every occluder where it is drawn (blended for interpolated entities, its
/// rotation kept), borrowed from the world rather than copied.
fn occluders_to_draw<'w>(
  world: &'w World,
  query: &mut QueryState<OccluderItem>,
  alpha: f32,
  out: &mut Vec<(GlobalTransform, &'w LightOccluder2D)>,
) {
  out.clear();
  out.extend(
    query
      .iter(world)
      .map(|(global, previous, occluder)| (render_global(global, previous, alpha), occluder)),
  );
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::Transform;
  use crate::core::ui::Color;

  /// An interpolated entity that moved from x = 0 to x = 10 in the last step.
  fn moved(world: &mut World, part: impl crate::core::ecs::Bundle) {
    world.spawn((
      Transform::from_xy(10.0, 0.0),
      PreviousGlobalTransform {
        global: GlobalTransform::from_transform(&Transform::from_xy(0.0, 0.0)),
        skip: false,
      },
      part,
    ));
  }

  #[test]
  fn interpolated_lights_and_occluders_draw_where_their_sprites_do() {
    let mut world = World::new();
    moved(&mut world, Light2D::point(Color::WHITE, 5.0));
    moved(&mut world, LightOccluder2D::rect(4.0, 4.0));
    let mut lights = Vec::new();
    lights_to_draw(&mut world, 0.5, &mut lights);
    assert_eq!(lights[0].position, Vec2::new(5.0, 0.0));
    let mut query = world.query::<OccluderItem>();
    let mut occluders = Vec::new();
    occluders_to_draw(&world, &mut query, 0.5, &mut occluders);
    assert_eq!(occluders[0].0.translation(), Vec2::new(5.0, 0.0));
  }
}
