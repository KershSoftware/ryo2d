//! The images cameras draw into when they do not draw straight into the
//! scene image (a window camera's own image, a texture camera's target),
//! and placing window cameras' images in their viewports

use std::collections::HashMap;

use super::RenderTarget;
use crate::core::ecs::Entity;
use crate::core::render::CameraDraw;

/// One camera's image.
struct CameraImage {
  texture: wgpu::Texture,
  view: wgpu::TextureView,
  /// For placing a window camera's image in its viewport (a texture
  /// camera's target is not placed)
  bind_group: Option<wgpu::BindGroup>,
}

/// An image per window camera that does not draw straight into the scene
/// image, sized to its viewport in render pixels, and the pipeline that
/// places them in the scene image; and each texture camera's target (owned
/// by the sprite renderer, shared here).
pub(crate) struct CameraImages {
  images: HashMap<Entity, CameraImage>,
  format: wgpu::TextureFormat,
  layout: wgpu::BindGroupLayout,
  sampler: wgpu::Sampler,
  pipeline: wgpu::RenderPipeline,
}

impl CameraImages {
  /// No images yet; they will have the scene image's `format`.
  pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
      label: Some("Camera Image Layout"),
      entries: &[
        wgpu::BindGroupLayoutEntry {
          binding: 0,
          visibility: wgpu::ShaderStages::FRAGMENT,
          ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
          },
          count: None,
        },
        wgpu::BindGroupLayoutEntry {
          binding: 1,
          visibility: wgpu::ShaderStages::FRAGMENT,
          ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
          count: None,
        },
      ],
    });
    // Images are as big as their viewports: one texel per pixel
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
      label: Some("Camera Image Sampler"),
      mag_filter: wgpu::FilterMode::Nearest,
      min_filter: wgpu::FilterMode::Nearest,
      ..Default::default()
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some("Camera Image Shader"),
      source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/core/camera_image.wgsl").into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("Camera Image Pipeline Layout"),
      bind_group_layouts: &[Some(&layout)],
      immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some("Camera Image Pipeline"),
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
          // Cameras that do not clear show the ones below through
          blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
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
    Self {
      images: HashMap::new(),
      format,
      layout,
      sampler,
      pipeline,
    }
  }

  /// Keep an image for every window camera in `cameras` that does not draw
  /// straight into the scene image, at its rectangle's size, and take each
  /// texture camera's target from `target_image`; drop the rest.
  pub(crate) fn sync<'t>(
    &mut self,
    device: &wgpu::Device,
    cameras: &[CameraDraw],
    target_image: impl Fn(RenderTarget) -> Option<(&'t wgpu::Texture, &'t wgpu::TextureView)>,
  ) {
    self.images.retain(|entity, image| {
      cameras.iter().any(|camera| {
        camera.entity == *entity
          && camera.target.is_none()
          && !camera.direct
          && (image.texture.width(), image.texture.height()) == camera.rect.size()
      })
    });
    for camera in cameras {
      match camera.target {
        // Taken afresh each frame: a resized target is a new texture
        Some(target) => {
          if let Some((texture, view)) = target_image(target) {
            let image = CameraImage {
              texture: texture.clone(),
              view: view.clone(),
              bind_group: None,
            };
            self.images.insert(camera.entity, image);
          }
        }
        None if !camera.direct && !self.images.contains_key(&camera.entity) => {
          let image = self.create(device, camera.rect.size());
          self.images.insert(camera.entity, image);
        }
        None => {}
      }
    }
  }

  fn create(&self, device: &wgpu::Device, (width, height): (u32, u32)) -> CameraImage {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
      label: Some("Camera Image"),
      size: wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
      },
      mip_level_count: 1,
      sample_count: 1,
      dimension: wgpu::TextureDimension::D2,
      format: self.format,
      usage: wgpu::TextureUsages::TEXTURE_BINDING
        | wgpu::TextureUsages::RENDER_ATTACHMENT
        | wgpu::TextureUsages::COPY_SRC,
      view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
      label: Some("Camera Image Bind Group"),
      layout: &self.layout,
      entries: &[
        wgpu::BindGroupEntry {
          binding: 0,
          resource: wgpu::BindingResource::TextureView(&view),
        },
        wgpu::BindGroupEntry {
          binding: 1,
          resource: wgpu::BindingResource::Sampler(&self.sampler),
        },
      ],
    });
    CameraImage {
      texture,
      view,
      bind_group: Some(bind_group),
    }
  }

  pub(crate) fn view(&self, camera: Entity) -> Option<&wgpu::TextureView> {
    self.images.get(&camera).map(|image| &image.view)
  }

  pub(crate) fn texture(&self, camera: Entity) -> Option<&wgpu::Texture> {
    self.images.get(&camera).map(|image| &image.texture)
  }

  /// How many images there are.
  #[cfg(test)]
  pub(crate) fn len(&self) -> usize {
    self.images.len()
  }

  /// Draw the images of the window cameras of `cameras` over `target` (the
  /// scene image) in their rectangles, in order.
  pub(crate) fn composite(
    &self,
    encoder: &mut wgpu::CommandEncoder,
    target: &wgpu::TextureView,
    cameras: &[CameraDraw],
  ) {
    let mut images = cameras.iter().filter_map(|camera| {
      if camera.direct || camera.target.is_some() {
        return None;
      }
      let bind_group = self.images.get(&camera.entity)?.bind_group.as_ref()?;
      Some((camera.rect, bind_group))
    });
    let Some(first) = images.next() else {
      return;
    };
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
      label: Some("Camera Images Pass"),
      color_attachments: &[Some(wgpu::RenderPassColorAttachment {
        view: target,
        depth_slice: None,
        resolve_target: None,
        ops: wgpu::Operations {
          load: wgpu::LoadOp::Load,
          store: wgpu::StoreOp::Store,
        },
      })],
      depth_stencil_attachment: None,
      timestamp_writes: None,
      occlusion_query_set: None,
      multiview_mask: None,
    });
    pass.set_pipeline(&self.pipeline);
    for (rect, bind_group) in std::iter::once(first).chain(images) {
      pass.set_viewport(
        rect.x as f32,
        rect.y as f32,
        rect.width as f32,
        rect.height as f32,
        0.0,
        1.0,
      );
      pass.set_bind_group(0, bind_group, &[]);
      pass.draw(0..3, 0..1);
    }
  }
}
