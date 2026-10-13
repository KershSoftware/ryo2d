//! The batch on the GPU: the frame's quad instances, each camera's draws
//! over them, and the pipelines and camera bind groups they draw with.

use std::collections::HashMap;

use wgpu::util::DeviceExt;

use super::pipelines::{self, PipelineKey, Pipelines};
use super::{BatchShader, Draw, DrawState, Placed, Planned, Stream, plan_draws};
use crate::core::ecs::Entity;
use crate::core::graphics::camera_gpu::CameraUniforms;
use crate::core::graphics::{
  AtlasSlot, Presentation, SpriteGpuRenderer, SpriteInstance, TextureId, place_in_slot,
};
use crate::core::render::{CameraDraw, DrawList, DrawPayload, SpriteShading};

/// Instances the buffer holds at first; it doubles when a frame needs more.
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
  draws: Vec<(Entity, Vec<Planned>)>,
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
    // Centred on the origin; the instance transform places and sizes it
    #[rustfmt::skip]
    let corners: [f32; 24] = [
      -0.5, -0.5, 0.0, 0.0,
       0.5, -0.5, 1.0, 0.0,
      -0.5,  0.5, 0.0, 1.0,
       0.5, -0.5, 1.0, 0.0,
       0.5,  0.5, 1.0, 1.0,
      -0.5,  0.5, 0.0, 1.0,
    ];
    let quad = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
      label: Some("Batch Quad"),
      contents: bytemuck::cast_slice(&corners),
      usage: wgpu::BufferUsages::VERTEX,
    });
    Self {
      format,
      presentation,
      pipelines: Pipelines::new(device, &camera_layout, &texture_layout),
      camera_layout,
      camera_groups: HashMap::new(),
      quad,
      instances: instance_buffer(device, FIRST_CAPACITY),
      capacity: FIRST_CAPACITY,
      placed: Vec::new(),
      draws: Vec::new(),
    }
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
  /// Makes the list's textures resident; then, per camera, remaps the
  /// sprites it sees into their atlas slots in draw order, in a range of
  /// the instance buffer of its own, and merges consecutive drawables with
  /// equal draw states into draws. Call once per frame, after
  /// [`sync_cameras`](Self::sync_cameras).
  pub(crate) fn prepare(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    list: &DrawList,
    cameras: &[CameraDraw],
    textures: &mut SpriteGpuRenderer,
  ) {
    textures.prepare_textures(device, queue, list);
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
      ..
    } = self;
    placed.clear();
    draws.resize_with(cameras.len(), || (Entity::PLACEHOLDER, Vec::new()));
    // The previous sprite's texture and where it lives, reused while
    // sprites share it
    let mut last: Option<(TextureId, AtlasSlot, (u32, u32))> = None;
    let mut ensured: Option<PipelineKey> = None;
    for (camera, (entity, planned)) in cameras.iter().zip(draws.iter_mut()) {
      *entity = camera.entity;
      let first = placed.len() as u32;
      plan_draws(list.items_for(camera), planned, first, |item| {
        let index = match item.payload {
          DrawPayload::Sprite(index) => index,
          DrawPayload::Trail { z, layers } => return Some(Placed::External { z, layers }),
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
          format,
        };
        if ensured != Some(key) {
          let custom = match wanted {
            BatchShader::Custom(source) => textures.custom_shader(source),
            _ => None,
          };
          pipelines.ensure(device, key, custom);
          ensured = Some(key);
        }
        // A custom shader draws plain until the GPU accepted its pipeline
        let shader = if pipelines.get(&key).is_some() {
          wanted
        } else {
          BatchShader::Sprite
        };
        if shader != wanted {
          let plain = PipelineKey { shader, ..key };
          pipelines.ensure(device, plain, None);
        }
        // Only built-in effects grow the quad; custom params are not effect ids
        let effects = shader == BatchShader::SpriteEffect;
        placed.push(place_in_slot(&sprite.instance, &slot, page_size, effects));
        Some(Placed::Quad(DrawState {
          page: Some(slot.page),
          shader,
          blend: sprite.blend,
          sampling: sprite.filter.resolve(presentation),
          scissor: None,
        }))
      });
    }

    if self.placed.len() > self.capacity {
      self.capacity = self.placed.len().next_power_of_two();
      self.instances = instance_buffer(device, self.capacity);
    }
    if !self.placed.is_empty() {
      queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(&self.placed));
    }
  }

  /// The draws [`prepare`](Self::prepare) laid out for `camera`, in order.
  pub(crate) fn draws(&self, camera: Entity) -> &[Planned] {
    self
      .draws
      .iter()
      .find(|(entity, _)| *entity == camera)
      .map_or(&[], |(_, planned)| planned.as_slice())
  }

  /// How many draw calls the laid-out frame issues.
  pub(crate) fn draw_count(&self) -> usize {
    self
      .draws
      .iter()
      .flat_map(|(_, planned)| planned)
      .filter(|planned| matches!(planned, Planned::Draw(_)))
      .count()
  }

  /// Issue one of `camera`'s draws inside a render pass, sampling the pages
  /// of `textures`.
  pub(crate) fn draw<'a>(
    &'a self,
    pass: &mut wgpu::RenderPass<'a>,
    draw: &Draw,
    camera: Entity,
    textures: &'a SpriteGpuRenderer,
  ) {
    let Some(page) = draw.state.page else {
      return;
    };
    let key = PipelineKey {
      shader: draw.state.shader,
      blend: draw.state.blend,
      stream: draw.stream,
      format: self.format,
    };
    let (Some(pipeline), Some(camera_group), Some(texture_group)) = (
      self.pipelines.get(&key),
      self.camera_groups.get(&camera),
      textures.page_bind_group(page),
    ) else {
      return;
    };
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, camera_group, &[]);
    pass.set_bind_group(1, texture_group, &[]);
    pass.set_vertex_buffer(0, self.quad.slice(..));
    let size = std::mem::size_of::<SpriteInstance>() as u64;
    let (start, end) = (u64::from(draw.range.start), u64::from(draw.range.end));
    pass.set_vertex_buffer(1, self.instances.slice(start * size..end * size));
    pass.draw(0..6, 0..draw.range.len() as u32);
  }

  /// How many instances the buffer holds.
  #[cfg(test)]
  pub(crate) fn capacity(&self) -> usize {
    self.capacity
  }
}

fn instance_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
  device.create_buffer(&wgpu::BufferDescriptor {
    label: Some("Batch Instances"),
    size: (capacity * std::mem::size_of::<SpriteInstance>()) as u64,
    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
    mapped_at_creation: false,
  })
}
