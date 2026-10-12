//! Drawing the world: scene GPU sprites, entities, trails, lines, lights and
//! overlays into the frame's draw list, then the render pipeline.

use std::time::Instant;

use super::{Engine, PresentOutcome, SceneHost};
use crate::core::config::DebugStats;
use crate::core::config::debug::{OverlayFlags, overlay_lines};
use crate::core::ecs::{GlobalTransform, Mut, Vec2};
use crate::core::graphics::{Camera, GpuRenderContext};
use crate::core::lighting::{Light2D, LightOccluder2D, LightingSettings};
use crate::core::render::{DrawKind, FrameScene, LightingFrame, RenderPipeline};
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
    let (Some(mut rend), Some(mut gpu)) = (self.renderer.take(), self.sprite_gpu.take()) else {
      return PresentOutcome::Skipped;
    };
    let mut lighting = self.lighting.take();
    let mut trails = self.trail_renderer.take();

    let camera = self.world.resource::<Camera>().clone();
    let (alpha, real_time, game_time) = {
      let time = self.world.resource::<Time>();
      (
        time.fixed.overstep_fraction(),
        time.real.elapsed(),
        time.game.elapsed(),
      )
    };
    let camera_pos = Vec2::new(camera.x, camera.y);
    let size = (rend.game_width(), rend.game_height());

    let (entity_sprites, lights, occluders) = {
      profile_scope!("cpu:render");

      // The scenes' GPU-batched sprites
      self.batch_collector.clear();
      if let Some(mut host) = self.world.remove_non_send::<SceneHost>() {
        let mut gpu_ctx = GpuRenderContext::new(
          &mut self.batch_collector,
          &mut gpu,
          rend.device(),
          rend.queue(),
          &camera,
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
        for trail in self.world.query::<&Trail2D>().iter(&self.world) {
          trails.add_trail_with_device(rend.device(), rend.queue(), trail);
        }
        for (global, line) in self
          .world
          .query::<(&GlobalTransform, &Line2D)>()
          .iter(&self.world)
        {
          trails.add_line(line, global.translation());
        }
        // Unlit lines on top, added last so that when the vertex budget runs
        // out the overlays are dropped, not game trails
        let flags = OverlayFlags {
          colliders: self.config.debug.show_colliders,
          velocities: self.config.debug.show_velocities,
          origins: self.config.debug.show_origins,
        };
        for line in overlay_lines(&mut self.world, &camera, flags) {
          trails.add_line(&line, Vec2::ZERO);
        }
      }

      // Lights at their world position; occluders keep their rotation
      let lights: Vec<(Vec2, Light2D)> = self
        .world
        .query::<(&GlobalTransform, &Light2D)>()
        .iter(&self.world)
        .map(|(global, light)| (global.translation(), light.clone()))
        .collect();
      let occluders: Vec<(GlobalTransform, LightOccluder2D)> = self
        .world
        .query::<(&GlobalTransform, &LightOccluder2D)>()
        .iter(&self.world)
        .map(|(global, occluder)| (*global, occluder.clone()))
        .collect();
      if let Some(lighting) = lighting.as_mut() {
        if !lights.is_empty() && !lighting.is_enabled() {
          lighting.set_enabled(true);
        }
        let settings = self.world.resource::<LightingSettings>();
        lighting.set_ambient(*settings.ambient());
        lighting.set_atmosphere(settings.atmosphere().clone());
        let mut effects = self.world.resource_mut::<EffectsManager>();
        lighting.update_god_rays(&mut effects, &lights, camera_pos, camera.zoom);
      }

      // Sprite effects animate on game time; custom shaders also get real time
      gpu.set_viewport(
        rend.queue(),
        size.0 as f32,
        size.1 as f32,
        game_time,
        real_time,
      );

      self.draw_list.clear();
      self
        .batch_collector
        .push_to(&mut self.draw_list, DrawKind::Sprite);
      let entity_sprites = self.entity_drawer.collect(
        &mut self.world,
        &camera,
        alpha,
        size,
        &mut gpu,
        rend.device(),
        rend.queue(),
        &mut self.draw_list,
      );
      if let Some(trails) = trails.as_ref() {
        for z in trails.groups(true) {
          self.draw_list.push_trail_group(true, z);
        }
        for z in trails.groups(false) {
          self.draw_list.push_trail_group(false, z);
        }
        // Upload the trail and line vertices once; the world passes only draw
        trails.prepare(rend.queue(), (camera_pos, camera.zoom, real_time));
      }
      self.draw_list.sorted();
      gpu.prepare_world(rend.device(), rend.queue(), &self.draw_list);
      (entity_sprites, lights, occluders)
    };

    let trail_draws = trails.as_ref().map_or(0, |t| t.draw_count());
    self.debug_stats = DebugStats {
      entity_count: self.world.entities().len(),
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
        let scene = FrameScene {
          sprites: &gpu,
          draw_list: &self.draw_list,
          trails: trails.as_ref(),
          lighting: lighting
            .as_mut()
            .filter(|l| l.is_enabled())
            .map(|renderer| LightingFrame {
              renderer,
              lights: &lights,
              occluders: &occluders,
            }),
          effects: world.resource::<EffectsManager>(),
          ui_buffer: &self.ui_buffer,
          camera_pos,
          camera_zoom: camera.zoom,
          time: real_time,
          overlay,
        };
        rend.render_frame(&mut pipeline, scene)
      });

    let outcome = match result {
      Ok(()) => PresentOutcome::Presented,
      // Recoverable: reconfigure and draw the next frame
      Err(wgpu::SurfaceError::Outdated) | Err(wgpu::SurfaceError::Timeout) => {
        self.surface_needs_reconfigure = true;
        PresentOutcome::Skipped
      }
      Err(wgpu::SurfaceError::Lost) => {
        if let Some(window) = &self.window {
          let size = window.inner_size();
          rend.resize(size.width, size.height);
        }
        PresentOutcome::Skipped
      }
      Err(wgpu::SurfaceError::OutOfMemory) => {
        crate::engine_error!(module: "Render", "out of GPU memory");
        PresentOutcome::Fatal("out of GPU memory".into())
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
