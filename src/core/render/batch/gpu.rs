//! The batch on the GPU: the frame's quad instances and triangles, each
//! camera's draws over them, and the pipelines and camera bind groups they
//! draw with.

use std::collections::HashMap;

use wgpu::util::DeviceExt;

use super::pipelines::{self, PipelineKey, Pipelines};
use super::polyline::{Triangles, Vertex2D};
use super::{
  BatchShader, BlendMode, Draw, DrawState, LightStep, PageKind, Placed, Sampling, Stream,
  plan_draws,
};
use crate::core::ecs::Entity;
use crate::core::graphics::camera_gpu::CameraUniforms;
use crate::core::graphics::{
  AtlasSlot, Presentation, SlotPage, SpriteGpuRenderer, SpriteInstance, TextureId, place_in_slot,
};
use crate::core::render::{CameraDraw, DrawItem, DrawList, DrawPayload, SpriteShading};

/// Instances, vertices and indices the buffers hold at first; each doubles
/// when a frame needs more.
const FIRST_CAPACITY: usize = 1024;

/// The 2D batch: lays out the world's drawables per camera and draws them.
pub(crate) struct Batch2D {
  format: wgpu::TextureFormat,
  presentation: Presentation,
  pipelines: Pipelines,
  // Group 0: each camera's uniform buffer, bound once
  camera_layout: wgpu::BindGroupLayout,
  camera_groups: HashMap<Entity, wgpu::BindGroup>,
  // The unit quad every instance is drawn from
  quad: wgpu::Buffer,
  instances: wgpu::Buffer,
  capacity: usize,
  // This frame's instances, remapped into their atlas slots (each camera's
  // in a range of their own), and each camera's draws over them; reused
  placed: Vec<SpriteInstance>,
  draws: Vec<CameraDraws>,
  // This frame's trails and lines, in world space for every camera; built
  // while the frame is gathered and uploaded once
  triangles: Triangles,
  vertices: GrowingBuffer,
  indices: GrowingBuffer,
}

/// A GPU buffer that doubles when a frame needs more.
struct GrowingBuffer {
  buffer: wgpu::Buffer,
  /// In elements
  capacity: usize,
  element: usize,
  usage: wgpu::BufferUsages,
  label: &'static str,
}

impl GrowingBuffer {
  fn new(
    device: &wgpu::Device,
    label: &'static str,
    element: usize,
    usage: wgpu::BufferUsages,
  ) -> Self {
    Self {
      buffer: buffer(device, label, FIRST_CAPACITY * element, usage),
      capacity: FIRST_CAPACITY,
      element,
      usage,
      label,
    }
  }

  /// Upload `bytes`, growing to the next power of two elements if need be.
  fn write(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, bytes: &[u8]) {
    let needed = bytes.len() / self.element;
    if needed > self.capacity {
      self.capacity = needed.next_power_of_two();
      self.buffer = buffer(device, self.label, self.capacity * self.element, self.usage);
    }
    if !bytes.is_empty() {
      queue.write_buffer(&self.buffer, 0, bytes);
    }
  }
}

/// One camera's draws: before lighting, after it, and its lights (into
/// its light buffer).
#[derive(Default)]
struct CameraDraws {
  camera: Option<Entity>,
  lit: Vec<Draw>,
  unlit: Vec<Draw>,
  lights: Vec<Draw>,
}

impl Batch2D {
  /// A batch drawing into images of `format`, resolving filters for
  /// `presentation`.
  pub(crate) fn new(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    presentation: Presentation,
  ) -> Self {
    let camera_layout = pipelines::camera_layout(device);
    let texture_layout = pipelines::texture_layout(device);
    Self {
      format,
      presentation,
      pipelines: Pipelines::new(device, &camera_layout, &texture_layout),
      camera_layout,
      camera_groups: HashMap::new(),
      quad: unit_quad(device, "Batch Quad"),
      instances: instance_buffer(device, FIRST_CAPACITY),
      capacity: FIRST_CAPACITY,
      placed: Vec::new(),
      draws: Vec::new(),
      triangles: Triangles::default(),
      vertices: GrowingBuffer::new(
        device,
        "Batch Vertices",
        std::mem::size_of::<Vertex2D>(),
        wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
      ),
      indices: GrowingBuffer::new(
        device,
        "Batch Indices",
        std::mem::size_of::<u32>(),
        wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
      ),
    }
  }

  /// The frame's trail and line triangles, for gathering them (clear first).
  pub(crate) fn triangles(&mut self) -> &mut Triangles {
    &mut self.triangles
  }

  /// The layout cameras' uniforms are bound with (group 0)
  pub(crate) fn camera_layout(&self) -> &wgpu::BindGroupLayout {
    &self.camera_layout
  }

  /// `camera`'s uniform bind group, once [`sync_cameras`](Self::sync_cameras)
  /// made it
  pub(crate) fn camera_group(&self, camera: Entity) -> Option<&wgpu::BindGroup> {
    self.camera_groups.get(&camera)
  }

  /// Bind every camera's uniform buffer (made by `CameraUniforms::sync`),
  /// dropping the bind groups of cameras that are gone.
  pub(crate) fn sync_cameras(&mut self, device: &wgpu::Device, uniforms: &CameraUniforms) {
    self
      .camera_groups
      .retain(|camera, _| uniforms.buffer(*camera).is_some());
    for camera in uniforms.cameras() {
      if self.camera_groups.contains_key(&camera) {
        continue;
      }
      let Some(buffer) = uniforms.buffer(camera) else {
        continue;
      };
      let group = crate::core::graphics::bind_groups::create(
        device,
        &wgpu::BindGroupDescriptor {
          label: Some("Batch Camera Bind Group"),
          layout: &self.camera_layout,
          entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
          }],
        },
      );
      self.camera_groups.insert(camera, group);
    }
  }

  /// Lay out the sorted `list` for each of `cameras` and upload it once.
  ///
  /// Makes the list's textures resident, and `extra_textures` (the UI
  /// renderer's sprites) with them; then, per camera, remaps the sprites it
  /// sees into their atlas slots in draw order, in a range of the instance
  /// buffer of its own, and merges consecutive drawables with equal draw
  /// states into draws (trails and lines by their index ranges). Call once
  /// per frame, after [`sync_cameras`](Self::sync_cameras) and after the
  /// frame's triangles are built.
  pub(crate) fn prepare(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    list: &DrawList,
    cameras: &[CameraDraw],
    textures: &mut SpriteGpuRenderer,
    extra_textures: &[TextureId],
  ) {
    textures.prepare_textures(device, queue, list, extra_textures);
    let textures: &SpriteGpuRenderer = textures;
    self.pipelines.settle();
    // Shaders the sprite renderer forgot take their pipelines with them
    self.pipelines.retain(|key| match key.shader {
      BatchShader::Custom(source) => textures.custom_shader(source).is_some(),
      _ => true,
    });

    let (format, presentation) = (self.format, self.presentation);
    let Self {
      pipelines,
      placed,
      draws,
      triangles,
      ..
    } = self;
    placed.clear();
    draws.resize_with(cameras.len(), CameraDraws::default);
    // The previous sprite's texture and where it lives, reused while
    // sprites share it
    let mut last: Option<(TextureId, AtlasSlot, (u32, u32))> = None;
    // The last pipeline asked for and the shader it resolved to, reused
    // while drawables share it (a map lookup per sprite otherwise)
    let mut ensured: Option<(PipelineKey, BatchShader)> = None;
    for (camera, drawn) in cameras.iter().zip(draws.iter_mut()) {
      drawn.camera = Some(camera.entity);
      let first = placed.len() as u32;
      let mut place = |item: &DrawItem| {
        let index = match item.payload {
          DrawPayload::Sprite(index) => index,
          DrawPayload::Glyph(index) => {
            let glyph = list.glyph(index)?;
            let key = PipelineKey {
              shader: glyph.shader,
              blend: glyph.blend,
              stream: Stream::Quads,
              page: PageKind::Srgb8,
              format,
            };
            if ensured.is_none_or(|(last, _)| last != key) {
              pipelines.ensure(device, key, None);
              ensured = Some((key, glyph.shader));
            }
            // The instance already carries its glyph page's UVs
            placed.push(glyph.instance);
            return Some(Placed::Quad(DrawState {
              page: Some(SlotPage::Glyph(glyph.page)),
              shader: glyph.shader,
              blend: glyph.blend,
              sampling: glyph.sampling,
            }));
          }
          DrawPayload::Triangles(index) => {
            let line = list.triangles(index)?;
            // A textured one is told where its texture lives
            let (page, sampling, shader) = match line.texture {
              None => (None, Sampling::Nearest, BatchShader::Line),
              Some(texture) => {
                let (slot, _) = textures.slot(texture)?;
                let range = line.vertices.start as usize..line.vertices.end as usize;
                for vertex in triangles.vertices.get_mut(range)? {
                  vertex.bounds = slot.uv;
                }
                let sampling = line.filter.resolve(presentation);
                (Some(slot.page), sampling, BatchShader::LineTextured)
              }
            };
            let key = PipelineKey {
              shader,
              blend: line.blend,
              stream: Stream::Triangles,
              page: PageKind::Srgb8,
              format,
            };
            if ensured.is_none_or(|(last, _)| last != key) {
              pipelines.ensure(device, key, None);
              ensured = Some((key, shader));
            }
            let state = DrawState {
              page,
              shader,
              blend: line.blend,
              sampling,
            };
            let indices = line.indices.clone();
            return Some(Placed::Triangles { state, indices });
          }
        };
        let sprite = list.sprite(index)?;
        let (slot, page_size) = match last {
          Some((texture, slot, size)) if texture == sprite.texture => (slot, size),
          _ => {
            let (slot, size) = textures.slot(sprite.texture)?;
            last = Some((sprite.texture, slot, size));
            (slot, size)
          }
        };
        let wanted = match sprite.shading {
          SpriteShading::Plain => BatchShader::Sprite,
          SpriteShading::Effect => BatchShader::SpriteEffect,
          SpriteShading::Custom(source) => BatchShader::Custom(source),
        };
        let key = PipelineKey {
          shader: wanted,
          blend: sprite.blend,
          stream: Stream::Quads,
          page: PageKind::of(Some(slot.page)),
          format,
        };
        let shader = match ensured {
          Some((last, shader)) if last == key => shader,
          _ => {
            let custom = match wanted {
              BatchShader::Custom(source) => textures.custom_shader(source),
              _ => None,
            };
            pipelines.ensure(device, key, custom);
            // A custom shader draws plain until the GPU accepted its pipeline
            let shader = if pipelines.get(&key).is_some() {
              wanted
            } else {
              let plain = PipelineKey {
                shader: BatchShader::Sprite,
                ..key
              };
              pipelines.ensure(device, plain, None);
              BatchShader::Sprite
            };
            ensured = Some((key, shader));
            shader
          }
        };
        // Only built-in effects grow the quad; custom params are not effect ids
        let effects = shader == BatchShader::SpriteEffect;
        placed.push(place_in_slot(&sprite.instance, &slot, page_size, effects));
        Some(Placed::Quad(DrawState {
          page: Some(slot.page),
          shader,
          blend: sprite.blend,
          sampling: sprite.filter.resolve(presentation),
        }))
      };
      let next = plan_draws(list.items_for(camera), &mut drawn.lit, first, &mut place);
      plan_draws(
        list.unlit_items_for(camera),
        &mut drawn.unlit,
        next,
        &mut place,
      );
      // A lit camera's lights, into its light buffer
      drawn.lights.clear();
      if camera.lit {
        for step in [
          LightStep::Add,
          LightStep::ResetMask,
          LightStep::Shadow,
          LightStep::Masked,
          LightStep::Coloured,
        ] {
          pipelines.ensure(device, light_key(step, format), None);
        }
        // Lights without shadow volumes add up in one draw
        let start = placed.len() as u32;
        placed.extend(
          list
            .lights_for(camera)
            .filter(|light| light.shadows.is_empty())
            .map(|light| light.instance),
        );
        let end = placed.len() as u32;
        if end > start {
          drawn.lights.push(light_draw(LightStep::Add, start..end));
        }
        // Each shadowed light masks itself: full in its quad, cut by its
        // shadows, then let through (and through a coloured shadow, tinted)
        for light in list
          .lights_for(camera)
          .filter(|light| !light.shadows.is_empty())
        {
          let at = placed.len() as u32;
          placed.push(light.instance);
          drawn
            .lights
            .push(light_draw(LightStep::ResetMask, at..at + 1));
          drawn
            .lights
            .push(light_draw(LightStep::Shadow, light.shadows.clone()));
          drawn.lights.push(light_draw(LightStep::Masked, at..at + 1));
          if light.coloured {
            drawn
              .lights
              .push(light_draw(LightStep::Coloured, at..at + 1));
          }
        }
      }
    }

    if self.placed.len() > self.capacity {
      self.capacity = self.placed.len().next_power_of_two();
      self.instances = instance_buffer(device, self.capacity);
    }
    if !self.placed.is_empty() {
      queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(&self.placed));
    }
    let triangles = &self.triangles;
    let vertices = bytemuck::cast_slice(&triangles.vertices);
    self.vertices.write(device, queue, vertices);
    let indices = bytemuck::cast_slice(&triangles.indices);
    self.indices.write(device, queue, indices);
  }

  /// The draws [`prepare`](Self::prepare) laid out for `camera`, in order:
  /// before lighting (`lit`) or after it.
  pub(crate) fn draws(&self, camera: Entity, lit: bool) -> &[Draw] {
    let Some(drawn) = self.draws.iter().find(|d| d.camera == Some(camera)) else {
      return &[];
    };
    if lit { &drawn.lit } else { &drawn.unlit }
  }

  /// The light draws [`prepare`](Self::prepare) laid out for `camera`,
  /// into its light buffer (none for a camera that is not lit).
  pub(crate) fn light_draws(&self, camera: Entity) -> &[Draw] {
    self
      .draws
      .iter()
      .find(|d| d.camera == Some(camera))
      .map_or(&[], |drawn| &drawn.lights)
  }

  /// How many draw calls the laid-out frame issues.
  pub(crate) fn draw_count(&self) -> usize {
    self
      .draws
      .iter()
      .map(|drawn| drawn.lit.len() + drawn.unlit.len() + drawn.lights.len())
      .sum()
  }

  /// Issue one of `camera`'s draws inside a render pass, sampling the pages
  /// of `textures`.
  pub(crate) fn draw<'a>(
    &'a self,
    pass: &mut wgpu::RenderPass<'a>,
    draw: &Draw,
    camera: Entity,
    textures: &'a SpriteGpuRenderer,
    image: (u32, u32),
  ) {
    let page = match draw.stream {
      Stream::Quads => PageKind::of(draw.state.page),
      Stream::Triangles => PageKind::Srgb8,
    };
    let key = PipelineKey {
      shader: draw.state.shader,
      blend: draw.state.blend,
      stream: draw.stream,
      page,
      format: self.format,
    };
    let (Some(pipeline), Some(camera_group)) =
      (self.pipelines.get(&key), self.camera_groups.get(&camera))
    else {
      return;
    };
    // Untextured lines and lights bind no texture; everything else samples
    // a page
    let texture_group = match draw.state.page {
      Some(page) => match textures.page_bind_group(page, draw.state.sampling) {
        Some(group) => Some(group),
        None => return,
      },
      None
        if draw.stream == Stream::Quads && !matches!(draw.state.shader, BatchShader::Light(_)) =>
      {
        return;
      }
      None => None,
    };
    // Every draw covers the whole image
    pass.set_scissor_rect(0, 0, image.0.max(1), image.1.max(1));
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, camera_group, &[]);
    if let Some(group) = texture_group {
      pass.set_bind_group(1, group, &[]);
    }
    match draw.stream {
      Stream::Quads => {
        pass.set_vertex_buffer(0, self.quad.slice(..));
        let size = std::mem::size_of::<SpriteInstance>() as u64;
        let (start, end) = (u64::from(draw.range.start), u64::from(draw.range.end));
        pass.set_vertex_buffer(1, self.instances.slice(start * size..end * size));
        pass.draw(0..6, 0..draw.range.len() as u32);
      }
      Stream::Triangles => {
        pass.set_vertex_buffer(0, self.vertices.buffer.slice(..));
        pass.set_index_buffer(self.indices.buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(draw.range.clone(), 0, 0..1);
      }
    }
  }

  /// Build one pipeline and say whether it exists (tests)
  #[cfg(test)]
  pub(crate) fn ensure_pipeline_for_test(
    &mut self,
    device: &wgpu::Device,
    shader: BatchShader,
    blend: BlendMode,
    stream: Stream,
    format: wgpu::TextureFormat,
  ) -> bool {
    let key = PipelineKey {
      shader,
      blend,
      stream,
      page: PageKind::Srgb8,
      format,
    };
    self.pipelines.ensure(device, key, None);
    self.pipelines.get(&key).is_some()
  }

  /// Build a custom shader's pipeline, wait for the GPU's check and say
  /// whether it passed (tests)
  #[cfg(test)]
  pub(crate) fn ensure_custom_for_test(
    &mut self,
    device: &wgpu::Device,
    wgsl: &str,
    blend: BlendMode,
    format: wgpu::TextureFormat,
  ) -> bool {
    let key = PipelineKey {
      shader: BatchShader::Custom(1),
      blend,
      stream: Stream::Quads,
      page: PageKind::Srgb8,
      format,
    };
    self.pipelines.ensure(device, key, Some(("test", wgsl)));
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    self.pipelines.settle();
    self.pipelines.get(&key).is_some()
  }

  /// How many instances the buffer holds.
  #[cfg(test)]
  pub(crate) fn capacity(&self) -> usize {
    self.capacity
  }
}

/// The pipeline key of a light-buffer draw.
fn light_key(step: LightStep, format: wgpu::TextureFormat) -> PipelineKey {
  PipelineKey {
    shader: BatchShader::Light(step),
    blend: BlendMode::Additive,
    stream: step.stream(),
    page: PageKind::Srgb8,
    format,
  }
}

/// A light-buffer draw of `range` (instances or indices, by the step's stream).
fn light_draw(step: LightStep, range: std::ops::Range<u32>) -> Draw {
  Draw {
    stream: step.stream(),
    state: DrawState {
      page: None,
      shader: BatchShader::Light(step),
      blend: BlendMode::Additive,
      sampling: Sampling::Nearest,
    },
    range,
  }
}

/// The unit quad every quad instance is drawn from (laid out as
/// [`QUAD_LAYOUT`](super::QUAD_LAYOUT)): two triangles, split along the
/// diagonal from (0.5, -0.5) to (-0.5, 0.5), centred on the origin; the
/// instance transform places and sizes it
pub(crate) fn unit_quad(device: &wgpu::Device, label: &str) -> wgpu::Buffer {
  #[rustfmt::skip]
  let corners: [f32; 24] = [
    -0.5, -0.5, 0.0, 0.0,
     0.5, -0.5, 1.0, 0.0,
    -0.5,  0.5, 0.0, 1.0,
     0.5, -0.5, 1.0, 0.0,
     0.5,  0.5, 1.0, 1.0,
    -0.5,  0.5, 0.0, 1.0,
  ];
  device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
    label: Some(label),
    contents: bytemuck::cast_slice(&corners),
    usage: wgpu::BufferUsages::VERTEX,
  })
}

fn instance_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
  let size = capacity * std::mem::size_of::<SpriteInstance>();
  let usage = wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST;
  buffer(device, "Batch Instances", size, usage)
}

fn buffer(
  device: &wgpu::Device,
  label: &str,
  size: usize,
  usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
  device.create_buffer(&wgpu::BufferDescriptor {
    label: Some(label),
    size: size as u64,
    usage,
    mapped_at_creation: false,
  })
}
