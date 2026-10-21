//! The UI on the GPU: its one pipeline, the texture bind groups it draws
//! with (cached by the textures they hold) and its instance buffer

use std::ops::Range;

use super::instance::UiInstance;
use crate::core::hash::FastMap;
use crate::core::render::UI_FORMAT;
use crate::core::render::batch::{QUAD_LAYOUT, unit_quad};

/// A bind group unused for this many frames whose UI built instances is
/// dropped, so the textures it holds are released
pub(crate) const KEEP_FRAMES: u64 = 8;

/// Instances the buffer holds at first; it doubles when a frame needs more
const FIRST_CAPACITY: usize = 1024;

/// Instances compared at once while looking for a change: one `memcmp`
/// over many is far cheaper than one per instance
const COMPARE_CHUNK: usize = 32;

/// A [`UiInstance`]: its eight lanes, then its clip, kind and flags
const INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 9] = wgpu::vertex_attr_array![
  2 => Float32x4, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4,
  6 => Float32x4, 7 => Float32x4, 8 => Float32x4, 9 => Float32x4,
  10 => Uint32x4,
];

const INSTANCE_LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
  array_stride: std::mem::size_of::<UiInstance>() as u64,
  step_mode: wgpu::VertexStepMode::Instance,
  attributes: &INSTANCE_ATTRIBUTES,
};

/// The UI's pipeline, texture bind groups and instance buffer
pub(crate) struct UiGpu {
  /// Group 1: the mask, colour, MSDF and sprite slots, then the linear and
  /// nearest samplers
  textures: wgpu::BindGroupLayout,
  pipeline: wgpu::RenderPipeline,
  /// The unit quad every instance is drawn from (a line triangle reads only
  /// the vertex index)
  quad: wgpu::Buffer,
  /// What a slot no instance of a run samples holds
  placeholder: wgpu::Texture,
  instances: wgpu::Buffer,
  /// In instances
  capacity: usize,
  /// What the instance buffer holds, from its start: last upload's frame
  held: Vec<UiInstance>,
  groups: BindCache,
  /// The instances the last upload wrote
  #[cfg(test)]
  written: Range<usize>,
}

/// What a texture bind group holds: the four slots' textures, then the
/// linear and nearest samplers
type BindKey = ([wgpu::Texture; 4], [wgpu::Sampler; 2]);

/// The UI's texture bind groups, by what they hold (wgpu handles compare by
/// identity, so a recreated texture is a new key), with the frame each was
/// last used; an entry not used for [`KEEP_FRAMES`] frames is dropped, as it
/// holds its textures alive
#[derive(Default)]
struct BindCache {
  groups: FastMap<BindKey, (wgpu::BindGroup, u64)>,
}

impl UiGpu {
  /// The UI pipeline drawing into the UI image (`UI_FORMAT`, gamma-encoded
  /// output), through cameras bound with `camera_layout` (group 0)
  pub(crate) fn new(device: &wgpu::Device, camera_layout: &wgpu::BindGroupLayout) -> UiGpu {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some("UI Shader"),
      source: wgpu::ShaderSource::Wgsl(crate::core::shaders::compose::UI.into()),
    });
    let texture = |binding| wgpu::BindGroupLayoutEntry {
      binding,
      visibility: wgpu::ShaderStages::FRAGMENT,
      ty: wgpu::BindingType::Texture {
        multisampled: false,
        view_dimension: wgpu::TextureViewDimension::D2,
        sample_type: wgpu::TextureSampleType::Float { filterable: true },
      },
      count: None,
    };
    let sampler = |binding| wgpu::BindGroupLayoutEntry {
      binding,
      visibility: wgpu::ShaderStages::FRAGMENT,
      ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
      count: None,
    };
    let textures = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
      label: Some("UI Texture Layout"),
      entries: &[
        texture(0),
        texture(1),
        texture(2),
        texture(3),
        sampler(4),
        sampler(5),
      ],
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("UI Pipeline Layout"),
      bind_group_layouts: &[Some(camera_layout), Some(&textures)],
      immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some("UI Pipeline"),
      layout: Some(&layout),
      vertex: wgpu::VertexState {
        module: &module,
        entry_point: Some("vs_ui"),
        buffers: &[Some(QUAD_LAYOUT), Some(INSTANCE_LAYOUT)],
        compilation_options: Default::default(),
      },
      fragment: Some(wgpu::FragmentState {
        module: &module,
        entry_point: Some("fs_ui"),
        targets: &[Some(wgpu::ColorTargetState {
          format: UI_FORMAT,
          blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
          write_mask: wgpu::ColorWrites::ALL,
        })],
        // The UI image holds gamma-encoded colour: encode on output
        compilation_options: wgpu::PipelineCompilationOptions {
          constants: &[("UI_GAMMA", 1.0)],
          ..Default::default()
        },
      }),
      primitive: wgpu::PrimitiveState {
        topology: wgpu::PrimitiveTopology::TriangleList,
        ..Default::default()
      },
      depth_stencil: None,
      multisample: wgpu::MultisampleState::default(),
      multiview_mask: None,
      cache: None,
    });
    // The batch's quad, split along the same diagonal: the same pixels
    let quad = unit_quad(device, "UI Quad");
    let placeholder = device.create_texture(&wgpu::TextureDescriptor {
      label: Some("UI Placeholder Texture"),
      size: wgpu::Extent3d {
        width: 1,
        height: 1,
        depth_or_array_layers: 1,
      },
      mip_level_count: 1,
      sample_count: 1,
      dimension: wgpu::TextureDimension::D2,
      format: wgpu::TextureFormat::Rgba8Unorm,
      usage: wgpu::TextureUsages::TEXTURE_BINDING,
      view_formats: &[],
    });
    UiGpu {
      textures,
      pipeline,
      quad,
      placeholder,
      instances: instance_buffer(device, FIRST_CAPACITY),
      capacity: FIRST_CAPACITY,
      held: Vec::new(),
      groups: BindCache::default(),
      #[cfg(test)]
      written: 0..0,
    }
  }

  /// The bind group holding `textures` in the mask, colour, MSDF and sprite
  /// slots (`None`: the placeholder) and `samplers` (linear, nearest), made
  /// on first use and marked used in `frame`
  pub(crate) fn bind_group(
    &mut self,
    device: &wgpu::Device,
    textures: [Option<&wgpu::Texture>; 4],
    (linear, nearest): (&wgpu::Sampler, &wgpu::Sampler),
    frame: u64,
  ) -> &wgpu::BindGroup {
    let textures = textures.map(|texture| texture.unwrap_or(&self.placeholder).clone());
    let key = (textures, [linear.clone(), nearest.clone()]);
    let layout = &self.textures;
    let (group, used) =
      self
        .groups
        .groups
        .entry(key)
        .or_insert_with_key(|(textures, [linear, nearest])| {
          let views = textures
            .each_ref()
            .map(|texture| texture.create_view(&wgpu::TextureViewDescriptor::default()));
          let view = |binding: u32, view| wgpu::BindGroupEntry {
            binding,
            resource: wgpu::BindingResource::TextureView(view),
          };
          let group = crate::core::graphics::bind_groups::create(
            device,
            &wgpu::BindGroupDescriptor {
              label: Some("UI Textures"),
              layout,
              entries: &[
                view(0, &views[0]),
                view(1, &views[1]),
                view(2, &views[2]),
                view(3, &views[3]),
                wgpu::BindGroupEntry {
                  binding: 4,
                  resource: wgpu::BindingResource::Sampler(linear),
                },
                wgpu::BindGroupEntry {
                  binding: 5,
                  resource: wgpu::BindingResource::Sampler(nearest),
                },
              ],
            },
          );
          (group, frame)
        });
    *used = frame;
    group
  }

  /// Drop the bind groups not used for [`KEEP_FRAMES`] frames up to `frame`
  pub(crate) fn end_frame(&mut self, frame: u64) {
    self
      .groups
      .groups
      .retain(|_, (_, used)| frame.saturating_sub(*used) < KEEP_FRAMES);
  }

  /// Upload the frame's `instances`: the one range that differs from what
  /// the buffer holds, in one write (nothing when nothing differs). A buffer
  /// too small is replaced by one of the next power of two instances, and
  /// given the whole frame.
  pub(crate) fn upload(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    instances: &[UiInstance],
  ) {
    if instances.len() > self.capacity {
      self.capacity = instances.len().next_power_of_two();
      self.instances = instance_buffer(device, self.capacity);
      self.held.clear();
    }
    let range = changed_range(&self.held, instances);
    if !range.is_empty() {
      let offset = (range.start * std::mem::size_of::<UiInstance>()) as u64;
      let changed = &instances[range.clone()];
      queue.write_buffer(&self.instances, offset, bytemuck::cast_slice(changed));
    }
    // Keep `held` equal to `instances`, copying only what changed
    self.held.truncate(instances.len());
    let kept = self.held.len();
    let end = range.end.min(kept);
    if range.start < end {
      self.held[range.start..end].copy_from_slice(&instances[range.start..end]);
    }
    self.held.extend_from_slice(&instances[kept..]);
    #[cfg(test)]
    {
      self.written = range;
    }
  }

  /// The instances the last upload wrote (tests)
  #[cfg(test)]
  pub(crate) fn written(&self) -> Range<usize> {
    self.written.clone()
  }

  pub(crate) fn pipeline(&self) -> &wgpu::RenderPipeline {
    &self.pipeline
  }

  /// The unit quad (vertex buffer 0)
  pub(crate) fn quad(&self) -> &wgpu::Buffer {
    &self.quad
  }

  /// The uploaded instances (vertex buffer 1)
  pub(crate) fn instances(&self) -> &wgpu::Buffer {
    &self.instances
  }

  /// How many bind groups are cached (tests)
  #[cfg(test)]
  pub(crate) fn cached_groups(&self) -> usize {
    self.groups.groups.len()
  }
}

/// The one range of `frame` to write over a buffer holding `held`: from
/// the first instance that differs to the last that does when the lengths
/// are equal (empty when nothing does), to the frame's end when they are
/// not. Instances compare bit for bit, as the buffer holds them.
pub(crate) fn changed_range(held: &[UiInstance], frame: &[UiInstance]) -> Range<usize> {
  let shared = held.len().min(frame.len());
  let first = same_prefix(&held[..shared], &frame[..shared]);
  if held.len() != frame.len() {
    return first..frame.len();
  }
  if first == frame.len() {
    return first..first;
  }
  first..frame.len() - same_suffix(&held[first..], &frame[first..])
}

fn bytes(instances: &[UiInstance]) -> &[u8] {
  bytemuck::cast_slice(instances)
}

fn alike(a: &UiInstance, b: &UiInstance) -> bool {
  bytemuck::bytes_of(a) == bytemuck::bytes_of(b)
}

/// How many instances `a` and `b` (as long as each other) begin with alike
fn same_prefix(a: &[UiInstance], b: &[UiInstance]) -> usize {
  let mut same = 0;
  for (a, b) in a.chunks(COMPARE_CHUNK).zip(b.chunks(COMPARE_CHUNK)) {
    if bytes(a) != bytes(b) {
      return same + a.iter().zip(b).take_while(|(a, b)| alike(a, b)).count();
    }
    same += a.len();
  }
  same
}

/// How many instances `a` and `b` (as long as each other) end with alike
fn same_suffix(a: &[UiInstance], b: &[UiInstance]) -> usize {
  let mut same = 0;
  for (a, b) in a.rchunks(COMPARE_CHUNK).zip(b.rchunks(COMPARE_CHUNK)) {
    if bytes(a) != bytes(b) {
      let pairs = a.iter().rev().zip(b.iter().rev());
      return same + pairs.take_while(|(a, b)| alike(a, b)).count();
    }
    same += a.len();
  }
  same
}

fn instance_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
  use wgpu::BufferUsages as U;
  // Tests read it back
  let read_back = if cfg!(test) { U::COPY_SRC } else { U::empty() };
  device.create_buffer(&wgpu::BufferDescriptor {
    label: Some("UI Instances"),
    size: (capacity * std::mem::size_of::<UiInstance>()) as u64,
    usage: U::VERTEX | U::COPY_DST | read_back,
    mapped_at_creation: false,
  })
}
