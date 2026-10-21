//! The UI's own renderer: every UI draw item is one kind of GPU instance,
//! drawn in painter's order through one pipeline that clips in the shader

mod build;
mod gpu;
mod instance;
mod runs;

#[cfg(test)]
mod gpu_tests;
#[cfg(test)]
mod tests;

use std::ops::Range;

#[cfg(test)]
use build::SpriteFix;
use build::{UiBuild, build_ui};
use gpu::UiGpu;
#[cfg(test)]
use gpu::{KEEP_FRAMES, changed_range};
use instance::UiInstance;
#[cfg(test)]
use instance::{ClipRect, UiKind};
#[cfg(test)]
use runs::Run;
pub(crate) use runs::TexKey;
use runs::{Slot, TexNeed, plan_runs};

use crate::core::graphics::{Presentation, SpriteGpuRenderer, TextureId};
use crate::core::render::DrawList;
use crate::core::render::text::{TextSources, TextTargets};
use crate::core::render::ui_retain::{UiRenderEnv, UiTextCache};
use crate::core::text::GlyphFormat;
use crate::core::ui::immediate::UiCommands;

/// The UI renderer: the frame's UI commands built into instances, fixed once
/// their sprite textures are resident, split into runs and uploaded (what
/// changed since the last upload, in one write), for the `ui` pass to draw
/// with one pipeline
pub(crate) struct UiRenderer {
  gpu: UiGpu,
  build: UiBuild,
  /// The texts' built instances, kept from frame to frame
  texts: UiTextCache<UiInstance>,
  /// Where texts and lines are laid while built (cleared for each): never
  /// the frame's draw list, which holds the world's items
  scratch: DrawList,
  /// The frame's runs, each with its bind group
  runs: Vec<(Range<u32>, wgpu::BindGroup)>,
  /// Frames whose UI drew instances, which age the bind groups (a kept
  /// frame, or one whose commands were all culled, ages none)
  frame: u64,
}

impl UiRenderer {
  /// A renderer drawing through cameras bound with `camera_layout` (the
  /// batch's)
  pub(crate) fn new(device: &wgpu::Device, camera_layout: &wgpu::BindGroupLayout) -> UiRenderer {
    UiRenderer {
      gpu: UiGpu::new(device, camera_layout),
      build: UiBuild::default(),
      texts: UiTextCache::default(),
      scratch: DrawList::default(),
      runs: Vec::new(),
      frame: 0,
    }
  }

  /// Build the frame's UI commands (see [`build_ui`]), its texts through the
  /// text cache, begun in `env` (with `retain` off nothing is reused).
  /// Glyphs are made through `targets`, but texts and lines are laid into
  /// the renderer's scratch list, never `targets.list`.
  pub(crate) fn build(
    &mut self,
    commands: &UiCommands,
    env: UiRenderEnv,
    retain: bool,
    sources: Option<&TextSources<'_>>,
    targets: &mut TextTargets<'_>,
    real_time: f32,
  ) {
    let mut scratch = TextTargets {
      raster: &mut *targets.raster,
      sprites: &mut *targets.sprites,
      device: targets.device,
      queue: targets.queue,
      list: &mut self.scratch,
      warned: &mut *targets.warned,
    };
    self.texts.begin(env, retain);
    let texts = &mut self.texts;
    build_ui(
      &mut self.build,
      commands,
      &env,
      sources,
      &mut scratch,
      real_time,
      texts,
    );
    self.texts.end();
  }

  /// The last build's sprite textures, to make resident before
  /// [`finish`](Self::finish)
  pub(crate) fn sprite_textures(&self) -> &[TextureId] {
    self.build.sprite_textures()
  }

  /// The last build's texts: those built, then those reused
  pub(crate) fn text_counts(&self) -> (u32, u32) {
    (self.texts.built(), self.texts.reused())
  }

  /// Finish the last build, its sprite textures now resident: fix its
  /// sprites (dropping those whose texture has no slot), split it into runs
  /// and bind each run's textures, and upload what changed in one write.
  /// The frame draws through the UI camera's `camera` group; a build without
  /// an instance (all of it culled) draws none.
  pub(crate) fn finish<'a>(
    &'a mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    sprites: &SpriteGpuRenderer,
    presentation: Presentation,
    camera: &'a wgpu::BindGroup,
  ) -> UiFrame<'a> {
    self.runs.clear();
    self.build.fix_sprites(sprites, presentation);
    if !self.build.instances.is_empty() {
      let runs = plan_runs(&self.build.needs, |need| slot_of(need, sprites));
      self.frame += 1;
      for run in runs {
        let textures = run
          .textures
          .map(|key| key.and_then(|key| sprites.page_texture(key)));
        let group = self
          .gpu
          .bind_group(device, textures, sprites.samplers(), self.frame);
        self.runs.push((run.range, group.clone()));
      }
      self.gpu.upload(device, queue, &self.build.instances);
      self.gpu.end_frame(self.frame);
    }
    UiFrame {
      pipeline: self.gpu.pipeline(),
      quad: self.gpu.quad(),
      instances: self.gpu.instances(),
      camera,
      runs: &self.runs,
    }
  }
}

/// The slot an instance's texture goes in, and the texture as this frame
/// resolved it (a sprite's: the page its texture sits on)
fn slot_of(need: TexNeed, sprites: &SpriteGpuRenderer) -> Option<(Slot, TexKey)> {
  match need {
    TexNeed::None => None,
    TexNeed::Glyph(page) => {
      let slot = match page.format {
        GlyphFormat::Mask => Slot::Mask,
        GlyphFormat::Color => Slot::Color,
        GlyphFormat::Msdf => Slot::Msdf,
      };
      Some((slot, TexKey::Glyph(page)))
    }
    TexNeed::Sprite(texture) => {
      let (slot, _) = sprites.slot(texture)?;
      Some((Slot::Sprite, TexKey::Sprite(slot.page)))
    }
  }
}

/// The UI renderer's frame, for the `ui` pass: its pipeline and buffers, the
/// UI camera's uniforms and its runs
#[derive(Clone, Copy)]
pub(crate) struct UiFrame<'a> {
  pipeline: &'a wgpu::RenderPipeline,
  quad: &'a wgpu::Buffer,
  instances: &'a wgpu::Buffer,
  camera: &'a wgpu::BindGroup,
  runs: &'a [(Range<u32>, wgpu::BindGroup)],
}

impl UiFrame<'_> {
  /// Its draws: one per run
  pub(crate) fn draw_count(&self) -> usize {
    self.runs.len()
  }

  /// Draw it into `pass` (the UI image): the pipeline and the camera bound
  /// once, then each run with its textures
  pub(crate) fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
    if self.runs.is_empty() {
      return;
    }
    pass.set_pipeline(self.pipeline);
    pass.set_bind_group(0, self.camera, &[]);
    pass.set_vertex_buffer(0, self.quad.slice(..));
    pass.set_vertex_buffer(1, self.instances.slice(..));
    for (range, group) in self.runs {
      pass.set_bind_group(1, group, &[]);
      pass.draw(0..6, range.clone());
    }
  }
}
