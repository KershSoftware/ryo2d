//! Drawing the world: scene GPU sprites, entities, trails, lines, lights and
//! overlays into the frame's draw list, then the render pipeline.

use web_time::Instant;

use super::{Engine, PresentOutcome, SceneHost};
use crate::core::asset::{Asset, AssetEvent, AssetId};
use crate::core::config::DebugStats;
use crate::core::config::debug::{OverlayFlags, overlay_shapes};
use crate::core::ecs::interpolate::render_global;
use crate::core::ecs::kernel::{MessageCursor, Messages};
use crate::core::ecs::kernel::{QueryState, SystemIdMarker};
use crate::core::ecs::{
  DetectChangesMut, DrawLift, Entity, Floor, GlobalTransform, IsResource, Mut, Observer,
  PreviousGlobalTransform, Vec2, World, shaders_of,
};
use crate::core::graphics::camera_gpu::CameraGlobals;
use crate::core::graphics::camera_mut::main_view;
use crate::core::graphics::renderer::FrameError;
use crate::core::graphics::{
  Camera2d, CameraView, GpuRenderContext, Image, Presentation, RenderLayers, RenderTargets,
  ScreenMapping, images_of,
};
use crate::core::graphics::{ParallaxLayer, parallax};
use crate::core::lighting::{FrameLight, Light2D, LightOccluder2D, LightingSettings};
use crate::core::render::batch::{Filter, LineLook, line_points, push_polyline, trail_points};
use crate::core::render::scratch::recycle;
use crate::core::render::text::{
  TextSources, TextTargets, WorldTextFrame, emit_world_text, layout_world_text, prewarm,
};
use crate::core::render::tiles::TileLayerFrame;
use crate::core::render::ui::{UI_CAMERA, ui_view};
use crate::core::render::ui_retain::{StampSources, UiRenderEnv, shown_version};
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
    // Images freed since the last frame lose their textures at once, and
    // changed ones start their new ones, before anything draws; the checker
    // stands in for images that are not loaded (debug builds)
    let missing = Some(self.missing_image.id());
    image_changes(
      &self.world,
      &mut self.image_events,
      (&mut self.freed_images, &mut self.changed_images),
    );
    gpu.sync_images(
      rend.device(),
      self.freed_images.drain(..),
      self.changed_images.drain(..),
      images_of(&self.world),
      missing,
    );
    // Shaders freed since the last frame lose their pipelines at once
    freed_since(
      &self.world,
      &mut self.shader_events,
      &mut self.freed_shaders,
    );
    let shaders = shaders_of(&self.world);
    gpu.sync_shaders(&self.freed_shaders, shaders);
    rend.sync_shaders(&self.freed_shaders, shaders);
    // New versions of shaders already drawn, made off the main thread, take
    // over before anything draws
    batch.sync_shaders(rend.device(), &mut gpu);
    // Faces freed or changed since the last frame lose their glyphs, before
    // any text is drawn
    if let Some(fonts) = self.world.get_resource::<Fonts>() {
      gpu.sync_fonts(fonts);
    }
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
        // Where each light draws, lifted on a floor
        let lights_at = lights.iter().map(|l| (l.position + l.lift, &l.light));
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
      self.batch_collector.push_to(
        &mut self.draw_list,
        DrawKind::Sprite,
        &mut gpu,
        images_of(&self.world),
      );
      // Parallax layers, laid out for each camera from its own view
      let mut quads = std::mem::take(&mut self.parallax);
      let mut parallax_layers = self
        .world
        .query::<(&ParallaxLayer, Option<&RenderLayers>)>();
      let images = images_of(&self.world);
      for (layer, layers) in parallax_layers.iter(&self.world) {
        let layers = layers.copied().unwrap_or_default();
        let Some(shown) = gpu.shown_image(layer.sprite.id(), images) else {
          continue;
        };
        let (width, height) = shown.size();
        let texels = Vec2::new(width as f32, height as f32);
        let texture = gpu.shown_texture(shown);
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
      // Spawned maps' layers: their visible chunks (built into the batch's
      // chunk store when they change), rows and tile objects, which take
      // their places among the entities' draws, and image layers
      let tile_frame = TileLayerFrame {
        cameras: &self.camera_draws,
        alpha,
        game_time,
      };
      self.tile_drawer.collect(
        &mut self.world,
        &tile_frame,
        &mut gpu,
        batch.chunks_mut(),
        &mut self.draw_list,
      );
      let entity_sprites = self.entity_drawer.collect(
        &mut self.world,
        &self.camera_draws,
        alpha,
        &mut gpu,
        rend.device(),
        rend.queue(),
        &mut self.draw_list,
        self.tile_drawer.ordered(),
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
        // Kept, so a frame makes no query state
        let query = self
          .occluder_query
          .get_or_insert_with(|| self.world.query::<OccluderItem>());
        occluders_to_draw(&self.world, query, alpha, &mut occluders);
        for light in &lights {
          if let Some(item) = crate::core::lighting::light_item(light, &occluders, triangles) {
            self.draw_list.push_light(item);
          }
        }
      }
      self.occluders = recycle(occluders);
      let mut points = std::mem::take(&mut self.poly_points);
      let mut trails = self.world.query::<(&Trail2D, Option<&RenderLayers>)>();
      let images = images_of(&self.world);
      for (trail, layers) in trails.iter(&self.world) {
        // A texture that is not loaded draws the checker (debug builds), or
        // no trail
        let texture = match trail.texture() {
          Some(image) => match gpu.texture_of(image.id(), images) {
            Some(texture) => Some(texture),
            None => continue,
          },
          None => None,
        };
        let Some(style) = trail_points(trail, &mut points) else {
          continue;
        };
        let look = LineLook {
          lit: trail.lighting(),
          z: trail.z_order(),
          texture,
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
          images: images_of(&self.world),
        }),
        // A game that removed the text resources draws its UI without text
        _ => None,
      };
      // An unchanged UI keeps the UI image: none of it is built, uploaded
      // or drawn (render module notes, "UI render retention")
      let retain = self.config.debug.ui_retain;
      if !retain {
        self.ui_retain.clear();
      }
      let images = images_of(&self.world);
      let showing = gpu.showing_old();
      let env = UiRenderEnv {
        space: self.ui_space,
        on_target: ui_on_target,
        text: sources.is_some(),
        fonts: self.text.generation(),
        // By the version each sheet's texture shows, as the stamps are
        bitmaps: self.world.get_resource::<Fonts>().map_or(0, |fonts| {
          fonts.bitmap_stamp(|sheet| shown_version(images, showing, sheet))
        }),
      };
      let stamps = StampSources {
        images,
        icons: sources.as_ref().map(|sources| sources.icons),
        showing,
      };
      let ui_kept = retain
        && self
          .ui_retain
          .keep(&self.ui_commands, stamps, env, rend.ui_image_epoch());
      let mut text_targets = TextTargets {
        raster: &mut self.glyph_raster,
        sprites: &mut gpu,
        device: rend.device(),
        queue: rend.queue(),
        list: &mut self.draw_list,
        warned: &mut self.text_warned,
      };
      // A changed UI is built by the UI renderer. It is drawn when it
      // recorded anything, also when all of it is culled: the `ui` pass then
      // clears the UI image, so an unchanged next frame keeps it
      let ui_built = !ui_kept;
      if ui_built {
        ui_renderer.build(
          &self.ui_commands,
          env,
          retain,
          (sources.as_ref(), images_of(&self.world)),
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
      let ((texts_built, texts_reused), (runs_copied, runs_built), slot_runs_copied) = if ui_built {
        (
          ui_renderer.text_counts(),
          ui_renderer.run_counts(),
          ui_renderer.slot_runs_copied(),
        )
      } else {
        ((0, 0), (0, 0), 0)
      };
      self.ui_retain.stats = UiRenderStats {
        kept: ui_kept,
        texts_built,
        texts_reused,
        glyph_evictions: gpu.glyph_evictions(),
        runs_copied,
        slot_runs_copied,
        runs_built,
        instances_written: 0,
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
      // world's, then its frame is finished with their slots. A kept UI
      // image shows the last build's: they count as used, each once, so the
      // standalone cap and a full atlas free the textures shown least
      // recently
      if !ui_built {
        gpu.mark_shown(ui_renderer.kept_textures());
      }
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
      self.ui_retain.stats.instances_written = ui_frame.map_or(0, |frame| frame.written());
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
          images: images_of(world),
          shaders: shaders_of(world),
          server: world.get_resource::<crate::core::asset::AssetServer>(),
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
    // The wait for the window's next image is vsync's pacing, not work
    let wait = self
      .renderer
      .as_ref()
      .map_or(std::time::Duration::ZERO, |renderer| {
        renderer.acquire_wait()
      });
    self.frame_timings.wait = wait;
    self.frame_timings.present = present_start.elapsed().saturating_sub(wait);

    if outcome == PresentOutcome::Presented {
      self.on_frame_presented();
    }
    outcome
  }
}

/// The ids of the `T`s freed since `cursor` last read the world's events,
/// in `freed` (emptied first)
fn freed_since<T: Asset>(
  world: &World,
  cursor: &mut MessageCursor<AssetEvent<T>>,
  freed: &mut Vec<AssetId<T>>,
) {
  freed.clear();
  if let Some(events) = world.get_resource::<Messages<AssetEvent<T>>>() {
    let removed = cursor.read(events).filter_map(|event| match event {
      AssetEvent::Removed { id } => Some(*id),
      _ => None,
    });
    freed.extend(removed);
  }
}

/// The images freed (`Removed`) and changed (`Modified`) since `cursor`
fn image_changes(
  world: &World,
  cursor: &mut MessageCursor<AssetEvent<Image>>,
  (freed, changed): (&mut Vec<AssetId<Image>>, &mut Vec<AssetId<Image>>),
) {
  freed.clear();
  changed.clear();
  if let Some(events) = world.get_resource::<Messages<AssetEvent<Image>>>() {
    for event in cursor.read(events) {
      match event {
        AssetEvent::Removed { id } => freed.push(*id),
        AssetEvent::Modified { id } => changed.push(*id),
        _ => {}
      }
    }
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

/// The floor a light or occluder is on and how far up it draws: its
/// `DrawLift` (its own floor's, or its nearest ancestor's with one), else
/// its own `Floor` unlifted, else the ground floor
fn floor_and_lift(drawn: Option<&DrawLift>, floor: Option<&Floor>) -> (Floor, Vec2) {
  match (drawn, floor) {
    (Some(drawn), _) => (drawn.floor, drawn.lift),
    (None, Some(&floor)) => (floor, Vec2::ZERO),
    (None, None) => (Floor::GROUND, Vec2::ZERO),
  }
}

/// The lights to draw this frame, where they stand (blended for
/// interpolated entities, like their sprites), with the layers that see
/// them and their floors and lifts.
fn lights_to_draw(world: &mut World, alpha: f32, out: &mut Vec<FrameLight>) {
  out.clear();
  out.extend(
    world
      .query::<(
        &GlobalTransform,
        Option<&PreviousGlobalTransform>,
        &Light2D,
        Option<&RenderLayers>,
        Option<&DrawLift>,
        Option<&Floor>,
      )>()
      .iter(world)
      .map(|(global, previous, light, layers, drawn, floor)| {
        let (floor, lift) = floor_and_lift(drawn, floor);
        FrameLight {
          position: render_global(global, previous, alpha).translation(),
          light: light.clone(),
          layers: layers.copied().unwrap_or_default(),
          floor,
          lift,
        }
      }),
  );
}

pub(super) type OccluderItem = (
  &'static GlobalTransform,
  Option<&'static PreviousGlobalTransform>,
  &'static LightOccluder2D,
  Option<&'static DrawLift>,
  Option<&'static Floor>,
);

/// Every occluder where it stands (blended for interpolated entities, its
/// rotation kept) and on its floor, borrowed from the world rather than
/// copied. Shadows are cast where occluders stand, so their lift is not
/// needed: a light's shadows draw by its own.
fn occluders_to_draw<'w>(
  world: &'w World,
  query: &mut QueryState<OccluderItem>,
  alpha: f32,
  out: &mut Vec<(GlobalTransform, &'w LightOccluder2D, Floor)>,
) {
  out.clear();
  out.extend(
    query
      .iter(world)
      .map(|(global, previous, occluder, drawn, floor)| {
        let (floor, _) = floor_and_lift(drawn, floor);
        (render_global(global, previous, alpha), occluder, floor)
      }),
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

  #[test]
  fn a_held_light_on_a_floor_casts_that_floor_s_shadows_drawn_lifted() {
    use crate::core::asset::Assets;
    use crate::core::ecs::ChildOf;
    use crate::core::lighting::{light_item, push_shadow_volumes};
    use crate::core::render::batch::Triangles;
    use crate::core::tilemap::test_util::MapWorld;
    use crate::core::tilemap::{MapRoot, TileMap};

    let mut maps = MapWorld::new();
    // A map of 8 x 4 cells of 16 px whose floor 1 draws 32 px up, its layers
    // empty
    let mut map = TileMap::new(glam::UVec2::new(8, 4), glam::UVec2::splat(16));
    map.add_tile_layer("ground");
    let upper = map.add_tile_layer("upper");
    map.layers[upper].info.floor = 1;
    map.layers[upper].info.offset = Vec2::new(0.0, -32.0);
    let handle = maps.world.resource_mut::<Assets<TileMap>>().add(map);
    maps.spawn(MapRoot::new(handle));
    // A hero on floor 1 holding a torch (a child), among walls on floor 1,
    // on the ground floor (no `Floor`) and on every floor
    let world = &mut maps.world;
    let hero = world
      .spawn((Transform::from_xy(60.0, 40.0), Floor::On(1)))
      .id();
    let torch = Light2D::point(Color::WHITE, 100.0).with_shadows();
    world.spawn((Transform::from_xy(4.0, 0.0), torch, ChildOf(hero)));
    let wall = LightOccluder2D::rect(4.0, 16.0);
    world.spawn((Transform::from_xy(100.0, 40.0), wall.clone(), Floor::On(1)));
    world.spawn((Transform::from_xy(20.0, 40.0), wall.clone()));
    world.spawn((Transform::from_xy(64.0, 10.0), wall, Floor::Every));
    maps.frame();

    let mut lights = Vec::new();
    lights_to_draw(&mut maps.world, 1.0, &mut lights);
    let [torch] = lights.as_slice() else {
      panic!("one light: {lights:?}");
    };
    let lift = Vec2::new(0.0, -32.0);
    assert_eq!(torch.position, Vec2::new(64.0, 40.0), "where it stands");
    assert_eq!((torch.floor, torch.lift), (Floor::On(1), lift));
    let mut query = maps.world.query::<OccluderItem>();
    let mut occluders = Vec::new();
    occluders_to_draw(&maps.world, &mut query, 1.0, &mut occluders);
    let mut floors: Vec<(i32, Floor)> = occluders
      .iter()
      .map(|(at, _, floor)| (at.translation().x as i32, *floor))
      .collect();
    floors.sort_by_key(|&(x, _)| x);
    assert_eq!(
      floors,
      [(20, Floor::GROUND), (64, Floor::Every), (100, Floor::On(1))]
    );

    // It casts from floor 1's wall and every floor's, where it stands, and
    // draws them and itself lifted
    let mut drawn = Triangles::default();
    let Some(item) = light_item(torch, &occluders, &mut drawn) else {
      panic!("a usable light");
    };
    let theirs: Vec<_> = occluders
      .iter()
      .filter(|(.., floor)| *floor != Floor::GROUND)
      .cloned()
      .collect();
    let mut standing = Triangles::default();
    let (light, at) = (&torch.light, torch.position);
    push_shadow_volumes(at, Vec2::ZERO, light, Floor::Every, &theirs, &mut standing);
    assert!(!standing.indices.is_empty());
    assert_eq!(item.shadows.len(), standing.indices.len());
    assert_eq!(drawn.vertices.len(), standing.vertices.len());
    for (up, there) in drawn.vertices.iter().zip(&standing.vertices) {
      assert_eq!(Vec2::from(up.position), Vec2::from(there.position) + lift);
      assert_eq!(Vec2::from(up.uv), at + lift);
    }
    // The ground floor's wall casts for a light on the ground
    let ground: Vec<_> = occluders
      .iter()
      .filter(|(.., floor)| *floor == Floor::GROUND)
      .cloned()
      .collect();
    let mut grounded = Triangles::default();
    push_shadow_volumes(at, Vec2::ZERO, light, Floor::GROUND, &ground, &mut grounded);
    assert!(!grounded.indices.is_empty());
    assert_eq!(item.instance.transform[2][..2], [64.0, 8.0]);
    let reach = Vec2::splat(100.0);
    assert_eq!((item.min, item.max), (at + lift - reach, at + lift + reach));
  }
}
