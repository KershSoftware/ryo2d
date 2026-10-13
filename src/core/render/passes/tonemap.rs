//! The look: the scene image tonemapped into the display image

use crate::core::graphics::bind_groups::{BindGroupCache, Bound};
use crate::core::render::context::image_view;
use crate::core::render::exposure::ExposureGpu;
use crate::core::render::look::LookUniforms;
use crate::core::render::{FrameImage, RenderContext, RenderPass, pass_ids};
use crate::core::shaders::OutputBuffer;

/// The LUT on the GPU: a 3D texture of the strip's sRGB colours, and which
/// sprite version it holds.
struct LutTexture {
  view: wgpu::TextureView,
  /// N of the LUT held (2 while it is the identity placeholder)
  size: u32,
  /// The sprite and generation uploaded, or `None` for the placeholder
  source: Option<(usize, u64)>,
  /// The sprite and generation already warned about
  warned: Option<(usize, u64)>,
}

impl LutTexture {
  /// An identity placeholder, so the bind group always has a LUT.
  fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
    let strip = crate::core::render::ColorLut::identity_strip(2);
    Self {
      view: upload_lut(device, queue, strip.data(), 2),
      size: 2,
      source: None,
      warned: None,
    }
  }

  /// Follow `lut`: upload a new or reloaded strip; keep the last good one
  /// (warning once) if it is no longer a strip. Returns the N to apply, 0
  /// without a LUT.
  fn sync(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    lut: Option<&crate::core::render::ColorLut>,
  ) -> u32 {
    let Some(lut) = lut else {
      return 0;
    };
    let handle = lut.sprite();
    let key = (handle.serial(), handle.generation());
    if self.source != Some(key) && self.warned != Some(key) {
      let sprite = handle.sprite();
      match crate::core::render::look::strip_size(sprite.width(), sprite.height()) {
        Some(n) => {
          self.view = upload_lut(device, queue, sprite.data(), n);
          self.size = n;
          self.source = Some(key);
        }
        None => {
          crate::engine_warn!(
            module: "Render",
            "LUT sprite {} is {}x{}, not a strip; keeping the last good LUT",
            handle.path().display(),
            sprite.width(),
            sprite.height()
          );
          self.warned = Some(key);
        }
      }
    }
    if self.source.is_some() { self.size } else { 0 }
  }
}

/// A 3D texture of the `n` x `n` x `n` LUT a strip's `rgba` bytes hold.
fn upload_lut(
  device: &wgpu::Device,
  queue: &wgpu::Queue,
  rgba: &[u8],
  n: u32,
) -> wgpu::TextureView {
  let size = wgpu::Extent3d {
    width: n,
    height: n,
    depth_or_array_layers: n,
  };
  let texture = device.create_texture(&wgpu::TextureDescriptor {
    label: Some("Look LUT"),
    size,
    mip_level_count: 1,
    sample_count: 1,
    dimension: wgpu::TextureDimension::D3,
    format: wgpu::TextureFormat::Rgba8Unorm,
    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
    view_formats: &[],
  });
  // Slice b of the volume is tile b of the strip
  let row = (n * 4) as usize;
  let mut volume = Vec::with_capacity(row * (n * n) as usize);
  for b in 0..n as usize {
    for g in 0..n as usize {
      let start = (g * n as usize * n as usize + b * n as usize) * 4;
      volume.extend_from_slice(&rgba[start..start + row]);
    }
  }
  queue.write_texture(
    texture.as_image_copy(),
    &volume,
    wgpu::TexelCopyBufferLayout {
      offset: 0,
      bytes_per_row: Some(n * 4),
      rows_per_image: Some(n),
    },
    size,
  );
  texture.create_view(&wgpu::TextureViewDescriptor::default())
}

/// The tonemap pipeline for one display format and render size, and the
/// bind groups of the scene images it has read.
struct TonemapGpu {
  format: wgpu::TextureFormat,
  size: (u32, u32),
  layout: wgpu::BindGroupLayout,
  uniforms: wgpu::Buffer,
  lut: LutTexture,
  lut_sampler: wgpu::Sampler,
  exposure: ExposureGpu,
  pipeline: wgpu::RenderPipeline,
  bind_groups: BindGroupCache,
}

impl TonemapGpu {
  fn new(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    format: wgpu::TextureFormat,
    size: (u32, u32),
  ) -> Self {
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
      label: Some("Tonemap Bind Group Layout"),
      entries: &[
        wgpu::BindGroupLayoutEntry {
          binding: 0,
          visibility: wgpu::ShaderStages::FRAGMENT,
          ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
          },
          count: None,
        },
        wgpu::BindGroupLayoutEntry {
          binding: 1,
          visibility: wgpu::ShaderStages::FRAGMENT,
          ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
          },
          count: None,
        },
        wgpu::BindGroupLayoutEntry {
          binding: 2,
          visibility: wgpu::ShaderStages::FRAGMENT,
          ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D3,
            multisampled: false,
          },
          count: None,
        },
        wgpu::BindGroupLayoutEntry {
          binding: 3,
          visibility: wgpu::ShaderStages::FRAGMENT,
          ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
          count: None,
        },
        wgpu::BindGroupLayoutEntry {
          binding: 4,
          visibility: wgpu::ShaderStages::FRAGMENT,
          ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
          },
          count: None,
        },
      ],
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some("Tonemap Shader"),
      source: wgpu::ShaderSource::Wgsl(crate::core::shaders::compose::TONEMAP.into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("Tonemap Pipeline Layout"),
      bind_group_layouts: &[Some(&layout)],
      immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some("Tonemap Pipeline"),
      layout: Some(&pipeline_layout),
      vertex: wgpu::VertexState {
        module: &shader,
        entry_point: Some("vs_main"),
        buffers: &[],
        compilation_options: Default::default(),
      },
      fragment: Some(wgpu::FragmentState {
        module: &shader,
        entry_point: Some("fs_main"),
        targets: &[Some(wgpu::ColorTargetState {
          format,
          blend: None,
          write_mask: wgpu::ColorWrites::ALL,
        })],
        compilation_options: Default::default(),
      }),
      primitive: wgpu::PrimitiveState::default(),
      depth_stencil: None,
      multisample: wgpu::MultisampleState::default(),
      multiview_mask: None,
      cache: None,
    });
    let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
      label: Some("Tonemap Uniforms"),
      size: std::mem::size_of::<LookUniforms>() as u64,
      usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
      mapped_at_creation: false,
    });
    let lut_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
      label: Some("Look LUT Sampler"),
      mag_filter: wgpu::FilterMode::Linear,
      min_filter: wgpu::FilterMode::Linear,
      address_mode_u: wgpu::AddressMode::ClampToEdge,
      address_mode_v: wgpu::AddressMode::ClampToEdge,
      address_mode_w: wgpu::AddressMode::ClampToEdge,
      ..Default::default()
    });
    Self {
      format,
      size,
      layout,
      uniforms,
      lut: LutTexture::new(device, queue),
      lut_sampler,
      exposure: ExposureGpu::new(device),
      pipeline,
      bind_groups: BindGroupCache::default(),
    }
  }
}

/// Writes the display image (the display stage's buffer `A`) from the
/// latest scene image; the transition, display effects, UI and composite
/// work on it.
#[derive(Default)]
pub(super) struct Tonemap {
  gpu: Option<TonemapGpu>,
  /// The real time of the last frame, for the exposure's speed
  last_time: Option<f32>,
}

impl RenderPass for Tonemap {
  fn id(&self) -> &str {
    pass_ids::TONEMAP
  }

  fn execute(&mut self, ctx: &mut RenderContext<'_>) {
    // Already the display image: nothing to map (and a pass can't read the
    // image it writes)
    if matches!(ctx.image, FrameImage::Display(_)) {
      return;
    }
    let device = ctx.device();
    let queue = ctx.queue();
    let size = ctx.render_size();
    let format = ctx.stages.display.texture_a().format();
    // A resize recreates the images the cached bind groups point at
    let gpu = match &mut self.gpu {
      Some(gpu) if gpu.format == format && gpu.size == size => gpu,
      slot => slot.insert(TonemapGpu::new(device, queue, format, size)),
    };
    let time = ctx.scene.time;
    let dt = self
      .last_time
      .map_or(0.0, |last| (time - last).clamp(0.0, 0.25));
    self.last_time = Some(time);
    let lut_size = gpu
      .lut
      .sync(device, queue, ctx.scene.look.grading.lut.as_ref());
    queue.write_buffer(
      &gpu.uniforms,
      0,
      bytemuck::bytes_of(&ctx.scene.look.uniforms(lut_size)),
    );
    let look = &ctx.scene.look;
    let RenderContext {
      encoder,
      targets,
      stages,
      image,
      ..
    } = ctx;
    let source = image_view(targets, stages, *image).clone();
    gpu.exposure.measure(
      device,
      queue,
      encoder,
      &source,
      size,
      look.auto_exposure.as_ref(),
      dt,
    );
    let group = gpu.bind_groups.get(
      device,
      "Tonemap Bind Group",
      &gpu.layout,
      &[
        Bound::View(source),
        Bound::Buffer(gpu.uniforms.clone()),
        Bound::View(gpu.lut.view.clone()),
        Bound::Sampler(gpu.lut_sampler.clone()),
        Bound::Buffer(gpu.exposure.state().clone()),
      ],
    );
    {
      let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Tonemap Pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
          view: stages.display.get_output_view(OutputBuffer::A),
          depth_slice: None,
          resolve_target: None,
          ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            store: wgpu::StoreOp::Store,
          },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
      });
      pass.set_pipeline(&gpu.pipeline);
      pass.set_bind_group(0, &group, &[]);
      pass.draw(0..6, 0..1);
    }
    *image = FrameImage::Display(OutputBuffer::A);
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::asset::SpriteHandle;
  use crate::core::graphics::Sprite;
  use crate::core::render::ColorLut;

  #[test]
  fn a_lut_reloaded_at_the_wrong_size_keeps_the_last_good_one() {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let handle = SpriteHandle::from_sprite(ColorLut::identity_strip(4), "lut-reload");
    let Ok(lut) = ColorLut::from_sprite(&handle) else {
      panic!("a 16x4 strip is a LUT");
    };
    let mut texture = LutTexture::new(&device, &queue);
    assert_eq!(texture.sync(&device, &queue, Some(&lut)), 4);
    let before = texture.view.clone();
    handle.update(Sprite::solid_rect(5, 5, 0, 0, 0, 255));
    assert_eq!(
      texture.sync(&device, &queue, Some(&lut)),
      4,
      "keeps the last good LUT"
    );
    assert_eq!(texture.view, before);
    assert_eq!(
      texture.sync(&device, &queue, None),
      0,
      "no LUT, nothing applied"
    );
  }
}
