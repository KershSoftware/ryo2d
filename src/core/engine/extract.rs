//! Drawing the world: scene GPU sprites, entities, trails, lines, lights and
//! overlays into the frame's draw list, then the render pipeline.

use web_time::Instant;

use super::{Engine, PresentOutcome, SceneHost};
use crate::core::config::DebugStats;
use crate::core::config::debug::{OverlayFlags, overlay_lines};
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
  Camera2d, CameraView, GpuRenderContext, Presentation, RenderLayers, RenderTargets,
};
use crate::core::lighting::{Light2D, LightOccluder2D, LightingSettings};
use crate::core::render::{DrawKind, FrameScene, LightingFrame, RenderPipeline, plan_cameras};
use crate::core::shaders::EffectsManager;
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
    let render_start = Instant::now();
    // Taken out for the frame so they can be used beside `self.world`
    // A minimised window draws nothing until it comes back
    if self.minimized {
      return PresentOutcome::Skipped;
    }
    let (Some(mut rend), Some(mut gpu)) = (self.renderer.take(), self.sprite_gpu.take()) else {
      return PresentOutcome::Skipped;
    };
    let mut lighting = self.lighting.take();
    let mut trails = self.trail_renderer.take();

    let camera = main_view(&mut self.world);
    let (alpha, real_time, game_time) = {
      let time = self.world.resource::<Time>();
      (
        time.fixed.overstep_fraction(),
        time.real.elapsed(),
        time.game.elapsed(),
      )
    };

    let (entity_sprites, lights) = {
      profile_scope!("cpu:render");

      // The scenes' GPU-batched sprites
      self.batch_collector.clear();
      if let Some(mut host) = self.world.remove_non_send::<SceneHost>() {
        let mut gpu_ctx = GpuRenderContext::new(
          &mut self.batch_collector,
          &mut gpu,
          rend.device(),
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

      // Trails (world-space points), lines (at their entity) and overlays
      if let Some(trails) = trails.as_mut() {
        trails.begin_frame();
        let (device, queue) = (rend.device(), rend.queue());
        for (trail, layers) in self
          .world
          .query::<(&Trail2D, Option<&RenderLayers>)>()
          .iter(&self.world)
        {
          let texture = trail.texture().map(|sprite| gpu.texture_of(sprite));
          let layers = layers.copied().unwrap_or_default();
          trails.add_trail_with_device(device, queue, trail, texture, layers);
        }
        trails.retain_textures(|id| gpu.is_texture(id));
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
          trails.add_line_in(line, at, layers.copied().unwrap_or_default());
        }
        // Unlit lines on top, added last so that when the vertex budget runs
        // out the overlays are dropped, not game trails
        let flags = OverlayFlags {
          colliders: self.config.debug.show_colliders,
          velocities: self.config.debug.show_velocities,
          origins: self.config.debug.show_origins,
        };
        for line in overlay_lines(&mut self.world, flags, camera.scale) {
          trails.add_line(&line, Vec2::ZERO);
        }
      }

      // Lights at their world position (occluders are gathered while drawing)
      let lights = lights_to_draw(&mut self.world, alpha);
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
        lighting.update_god_rays(&mut effects, &lights, &camera);
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
      let snap = matches!(self.config.presentation, Presentation::PixelArt { .. });
      let cameras: Vec<_> = self
        .camera_draws
        .iter()
        .map(|camera| {
          let size = camera.rect.size();
          let globals = CameraGlobals::new(&camera.view, size, game_time, real_time, snap);
          (camera.entity, globals)
        })
        .collect();
      self
        .camera_uniforms
        .sync(rend.device(), rend.queue(), &cameras);
      gpu.sync_cameras(rend.device(), &self.camera_uniforms);

      self.draw_list.clear();
      self
        .batch_collector
        .push_to(&mut self.draw_list, DrawKind::Sprite, &mut gpu);
      let entity_sprites = self.entity_drawer.collect(
        &mut self.world,
        &self.camera_draws,
        alpha,
        &mut gpu,
        rend.device(),
        rend.queue(),
        &mut self.draw_list,
      );
      if let Some(trails) = trails.as_mut() {
        for (z, layers) in trails.groups(true) {
          self.draw_list.push_trail_group(true, z, layers);
        }
        for (z, layers) in trails.groups(false) {
          self.draw_list.push_trail_group(false, z, layers);
        }
        // Upload the trail and line vertices once; the world passes only draw
        trails.sync_cameras(rend.device(), &self.camera_uniforms);
        trails.prepare(rend.queue());
      }
      self.draw_list.sorted();
      gpu.prepare_world(
        rend.device(),
        rend.queue(),
        &self.draw_list,
        &self.camera_draws,
      );
      (entity_sprites, lights)
    };

    let trail_draws = trails.as_ref().map_or(0, |t| t.draw_count());
    self.debug_stats = DebugStats {
      entity_count: game_entity_count(&self.world),
      draw_calls: (gpu.world_draw_count() + trail_draws) as u32,
      sprite_count: (entity_sprites + self.batch_collector.sprite_count()) as u32,
    };
    self.frame_timings.render = render_start.elapsed();

    profile_scope!("gpu:present");
    let present_start = Instant::now();
    let overlay = self.transition_overlay();
    let result = self
      .world
      .resource_scope(|world, mut pipeline: Mut<RenderPipeline>| {
        let mut occluder_query = world.query::<OccluderItem>();
        let world: &World = world;
        let mut lighting = lighting.as_mut().filter(|l| l.is_enabled());
        // Lights and shadows are uploaded once; every lit camera draws them
        if let Some(lighting) = lighting.as_deref_mut() {
          let occluders = occluders_to_draw(world, &mut occluder_query, alpha);
          lighting.prepare_frame(rend.queue(), &lights, &occluders);
        }
        let scene = FrameScene {
          sprites: &gpu,
          draw_list: &self.draw_list,
          trails: trails.as_ref(),
          lighting: lighting.map(|renderer| LightingFrame { renderer }),
          effects: world.resource::<EffectsManager>(),
          ui_buffer: &self.ui_buffer,
          cameras: &self.camera_draws,
          camera_uniforms: &self.camera_uniforms,
          time: real_time,
          overlay,
        };
        rend.render_frame(&mut pipeline, scene)
      });

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
    self.lighting = lighting;
    self.trail_renderer = trails;
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
/// interpolated entities, like their sprites).
fn lights_to_draw(world: &mut World, alpha: f32) -> Vec<(Vec2, Light2D)> {
  world
    .query::<(&GlobalTransform, Option<&PreviousGlobalTransform>, &Light2D)>()
    .iter(world)
    .map(|(global, previous, light)| {
      (
        render_global(global, previous, alpha).translation(),
        light.clone(),
      )
    })
    .collect()
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
) -> Vec<(GlobalTransform, &'w LightOccluder2D)> {
  query
    .iter(world)
    .map(|(global, previous, occluder)| (render_global(global, previous, alpha), occluder))
    .collect()
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
    let lights = lights_to_draw(&mut world, 0.5);
    assert_eq!(lights[0].0, Vec2::new(5.0, 0.0));
    let mut query = world.query::<OccluderItem>();
    let occluders = occluders_to_draw(&world, &mut query, 0.5);
    assert_eq!(occluders[0].0.translation(), Vec2::new(5.0, 0.0));
  }
}
