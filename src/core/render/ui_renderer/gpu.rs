//! The UI on the GPU: its one pipeline, the texture bind groups it draws
//! with (cached by the textures they hold) and its instance buffer

use wgpu::util::DeviceExt;

use super::instance::UiInstance;
use crate::core::hash::FastMap;
use crate::core::render::UI_FORMAT;

/// A bind group unused for this many frames is dropped, so the textures it
/// holds are released
pub(crate) const KEEP_FRAMES: u64 = 8;

/// Instances the buffer holds at first; it doubles when a frame needs more
const FIRST_CAPACITY: usize = 1024;

/// The unit quad's corners: position, then UV (as the batch's)
const QUAD_ATTRIBUTES: [wgpu::VertexAttribute; 2] =
  wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2];

/// A [`UiInstance`]: its eight lanes, then its clip, kind and flags
const INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 9] = wgpu::vertex_attr_array![
  2 => Float32x4, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4,
  6 => Float32x4, 7 => Float32x4, 8 => Float32x4, 9 => Float32x4,
  10 => Uint32x4,
];

const QUAD_LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
  array_stride: 16,
  step_mode: wgpu::VertexStepMode::Vertex,
  attributes: &QUAD_ATTRIBUTES,
};

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
  groups: BindCache,
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
      label: Some("UI Quad"),
      contents: bytemuck::cast_slice(&corners),
      usage: wgpu::BufferUsages::VERTEX,
    });
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
      groups: BindCache::default(),
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

  /// Upload the frame's `instances` in one write, growing the buffer to the
  /// next power of two instances if need be
  pub(crate) fn upload(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    instances: &[UiInstance],
  ) {
    if instances.len() > self.capacity {
      self.capacity = instances.len().next_power_of_two();
      self.instances = instance_buffer(device, self.capacity);
    }
    if !instances.is_empty() {
      queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(instances));
    }
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

fn instance_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
  device.create_buffer(&wgpu::BufferDescriptor {
    label: Some("UI Instances"),
    size: (capacity * std::mem::size_of::<UiInstance>()) as u64,
    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
    mapped_at_creation: false,
  })
}
