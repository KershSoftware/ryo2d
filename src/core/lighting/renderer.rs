//! The lighting renderer: each lit camera's light buffer and the compose
//! pass that lights the scene with it.
//!
//! Lights and their shadow volumes are batch draws (`lighting::batch`),
//! drawn into the light buffers by the `lighting` pass.

use super::{AmbientLight, Atmosphere};
use crate::core::ecs::Entity;
use crate::core::graphics::bind_groups::{BindGroupCache, Bound};
use crate::core::render::CameraDraw;
use std::collections::HashMap;

/// Uniforms for the lighting composition pass
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct LightingParams {
  /// Ambient color (RGB) + unused (A)
  pub(crate) color: [f32; 4],
  /// Ambient intensity (x), atmosphere density (y), atmosphere scattering (z), unused (w)
  pub(crate) params: [f32; 4],
  /// Time (x), width (y), height (z), unused (w)
  pub(crate) time_resolution: [f32; 4],
  /// Atmosphere fog color (RGB) + unused (A)
  pub(crate) atmosphere_color: [f32; 4],
}

/// The textures one image is lit with, at its size: the light
/// accumulation buffer, the copy of the scene the compose pass reads, and
/// the compose pass's uniforms.
struct LightTargets {
  light_view: wgpu::TextureView,
  scene_copy_texture: wgpu::Texture,
  scene_copy_view: wgpu::TextureView,
  compose_uniforms: wgpu::Buffer,
  width: u32,
  height: u32,
}

impl LightTargets {
  fn new(device: &wgpu::Device, width: u32, height: u32, format: wgpu::TextureFormat) -> Self {
    let size = wgpu::Extent3d {
      width,
      height,
      depth_or_array_layers: 1,
    };
    // The batch's light draws accumulate into it (their pipelines are
    // made for the scene format); its alpha masks shadowed lights
    let light_texture = device.create_texture(&wgpu::TextureDescriptor {
      label: Some("Light Accumulation Texture"),
      size,
      mip_level_count: 1,
      sample_count: 1,
      dimension: wgpu::TextureDimension::D2,
      format: crate::core::render::SCENE_FORMAT,
      usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
      view_formats: &[],
    });
    // The compose pass reads a copy of the scene, so it can write the lit
    // scene over the original
    let scene_copy_texture = device.create_texture(&wgpu::TextureDescriptor {
      label: Some("Scene Copy Texture"),
      size,
      mip_level_count: 1,
      sample_count: 1,
      dimension: wgpu::TextureDimension::D2,
      format,
      usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
      view_formats: &[],
    });
    let compose_uniforms = device.create_buffer(&wgpu::BufferDescriptor {
      label: Some("Lighting Compose Uniform Buffer"),
      size: std::mem::size_of::<LightingParams>() as u64,
      usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
      mapped_at_creation: false,
    });
    Self {
      light_view: light_texture.create_view(&wgpu::TextureViewDescriptor::default()),
      scene_copy_view: scene_copy_texture.create_view(&wgpu::TextureViewDescriptor::default()),
      scene_copy_texture,
      compose_uniforms,
      width,
      height,
    }
  }
}

/// The lighting renderer manages GPU resources for 2D lighting.
pub(crate) struct LightingRenderer {
  /// The format of the images it lights, which its scene copies share
  format: wgpu::TextureFormat,

  /// The targets of each lit camera, at its image's size
  cameras: HashMap<Entity, LightTargets>,

  /// Each lit camera's compose bind group, made once
  bind_groups: BindGroupCache,

  /// Sampler for textures
  sampler: wgpu::Sampler,

  /// Pipeline for compositing lights with scene
  compose_pipeline: wgpu::RenderPipeline,

  /// Bind group layout for composition
  compose_bind_group_layout: wgpu::BindGroupLayout,

  /// Current ambient light settings
  ambient: AmbientLight,

  /// Current atmosphere settings
  atmosphere: Atmosphere,

  /// Whether lighting is enabled
  enabled: bool,
}

impl LightingRenderer {
  /// A lighting renderer for images of `format`.
  pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
    // Create sampler
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
      label: Some("Lighting Sampler"),
      address_mode_u: wgpu::AddressMode::ClampToEdge,
      address_mode_v: wgpu::AddressMode::ClampToEdge,
      address_mode_w: wgpu::AddressMode::ClampToEdge,
      mag_filter: wgpu::FilterMode::Linear,
      min_filter: wgpu::FilterMode::Linear,
      mipmap_filter: wgpu::MipmapFilterMode::Nearest,
      ..Default::default()
    });

    let compose_bind_group_layout =
      device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Lighting Compose Bind Group Layout"),
        entries: &[
          // Scene texture
          wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
              multisampled: false,
              view_dimension: wgpu::TextureViewDimension::D2,
              sample_type: wgpu::TextureSampleType::Float { filterable: true },
            },
            count: None,
          },
          // Light texture
          wgpu::BindGroupLayoutEntry {
            binding: 1,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
              multisampled: false,
              view_dimension: wgpu::TextureViewDimension::D2,
              sample_type: wgpu::TextureSampleType::Float { filterable: true },
            },
            count: None,
          },
          // Sampler
          wgpu::BindGroupLayoutEntry {
            binding: 2,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
          },
          // Uniforms
          wgpu::BindGroupLayoutEntry {
            binding: 3,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
              ty: wgpu::BufferBindingType::Uniform,
              has_dynamic_offset: false,
              min_binding_size: None,
            },
            count: None,
          },
        ],
      });

    // Create composition pipeline
    let compose_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some("Lighting Compose Shader"),
      source: wgpu::ShaderSource::Wgsl(crate::core::shaders::compose::LIGHTING_COMPOSE.into()),
    });

    let compose_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("Lighting Compose Pipeline Layout"),
      bind_group_layouts: &[Some(&compose_bind_group_layout)],
      immediate_size: 0,
    });

    let compose_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some("Lighting Compose Pipeline"),
      layout: Some(&compose_pipeline_layout),
      vertex: wgpu::VertexState {
        module: &compose_shader,
        entry_point: Some("vs_main"),
        buffers: &[],
        compilation_options: Default::default(),
      },
      fragment: Some(wgpu::FragmentState {
        module: &compose_shader,
        entry_point: Some("fs_main"),
        targets: &[Some(wgpu::ColorTargetState {
          format,
          blend: Some(wgpu::BlendState::REPLACE),
          write_mask: wgpu::ColorWrites::ALL,
        })],
        compilation_options: Default::default(),
      }),
      primitive: wgpu::PrimitiveState {
        topology: wgpu::PrimitiveTopology::TriangleList,
        strip_index_format: None,
        front_face: wgpu::FrontFace::Ccw,
        cull_mode: None,
        polygon_mode: wgpu::PolygonMode::Fill,
        unclipped_depth: false,
        conservative: false,
      },
      depth_stencil: None,
      multisample: wgpu::MultisampleState {
        count: 1,
        mask: !0,
        alpha_to_coverage_enabled: false,
      },
      multiview_mask: None,
      cache: None,
    });

    Self {
      format,
      cameras: HashMap::new(),
      bind_groups: BindGroupCache::default(),
      sampler,
      compose_pipeline,
      compose_bind_group_layout,
      ambient: AmbientLight::default(),
      atmosphere: Atmosphere::default(),
      enabled: false, // Disabled by default, enabled when Light2D entities exist
    }
  }

  /// Set the ambient light.
  pub(crate) fn set_ambient(&mut self, ambient: AmbientLight) {
    self.ambient = ambient;
  }

  /// Set the atmosphere settings.
  ///
  /// Atmosphere controls fog density, color tinting, and light scattering.
  /// Use presets like `Atmosphere::dusty()` or `Atmosphere::foggy()`.
  pub(crate) fn set_atmosphere(&mut self, atmosphere: Atmosphere) {
    self.atmosphere = atmosphere;
  }

  /// Get the current atmosphere settings.
  pub(crate) fn atmosphere(&self) -> &Atmosphere {
    &self.atmosphere
  }

  /// Enable or disable the lighting system.
  pub(crate) fn set_enabled(&mut self, enabled: bool) {
    self.enabled = enabled;
  }

  /// Check if lighting is enabled.
  pub(crate) fn is_enabled(&self) -> bool {
    self.enabled
  }

  /// Keep light targets for every lit camera of `cameras`, sized to its
  /// image; drop the others.
  pub(crate) fn sync_cameras(&mut self, device: &wgpu::Device, cameras: &[CameraDraw]) {
    let before = self.cameras.len();
    self.cameras.retain(|entity, targets| {
      cameras.iter().any(|camera| {
        camera.entity == *entity
          && camera.lit
          && (targets.width, targets.height) == camera.rect.size()
      })
    });
    let format = self.format;
    let mut changed = self.cameras.len() != before;
    for camera in cameras.iter().filter(|camera| camera.lit) {
      self.cameras.entry(camera.entity).or_insert_with(|| {
        changed = true;
        let (width, height) = camera.rect.size();
        LightTargets::new(device, width, height, format)
      });
    }
    // Bind groups of dropped or recreated targets would keep them alive
    if changed {
      self.bind_groups.clear();
    }
  }

  /// `camera`'s light buffer, which the batch draws its lights into.
  pub(crate) fn light_view(&self, camera: Entity) -> Option<&wgpu::TextureView> {
    self.cameras.get(&camera).map(|targets| &targets.light_view)
  }

  /// How many cameras have light targets.
  #[cfg(test)]
  pub(crate) fn camera_count(&self) -> usize {
    self.cameras.len()
  }

  /// Compose `camera`'s lit image into `output_view` from its scene image
  /// `scene_texture` (which may be the one behind `output_view`: the scene
  /// is copied first) and its light buffer.
  #[allow(clippy::too_many_arguments)]
  pub(crate) fn compose_for_camera(
    &self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    camera: Entity,
    scene_texture: &wgpu::Texture,
    output_view: &wgpu::TextureView,
    time: f32,
  ) {
    let Some(targets) = self.cameras.get(&camera) else {
      return;
    };
    if scene_texture.size().width != targets.width || scene_texture.size().height != targets.height
    {
      return;
    }
    copy_scene(encoder, scene_texture, targets);
    self.compose_targets(device, queue, encoder, targets, output_view, time);
  }

  /// The compose pass: the scene copy in `targets` lit by its light buffer
  /// (plus ambient and atmosphere), into `output_view`.
  fn compose_targets(
    &self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    targets: &LightTargets,
    output_view: &wgpu::TextureView,
    time: f32,
  ) {
    let [ar, ag, ab, _] = self.ambient.color.to_linear();
    let [fr, fg, fb, _] = self.atmosphere.color.to_linear();
    let compose_uniform = LightingParams {
      color: [ar, ag, ab, 1.0],
      params: [
        self.ambient.intensity,
        self.atmosphere.density,
        self.atmosphere.scattering,
        0.0,
      ],
      time_resolution: [time, targets.width as f32, targets.height as f32, 0.0],
      atmosphere_color: [fr, fg, fb, 1.0],
    };
    queue.write_buffer(
      &targets.compose_uniforms,
      0,
      bytemuck::cast_slice(&[compose_uniform]),
    );
    let compose_bind_group = self.bind_groups.get(
      device,
      "Lighting Compose Bind Group",
      &self.compose_bind_group_layout,
      &[
        Bound::View(targets.scene_copy_view.clone()),
        Bound::View(targets.light_view.clone()),
        Bound::Sampler(self.sampler.clone()),
        Bound::Buffer(targets.compose_uniforms.clone()),
      ],
    );
    let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
      label: Some("Lighting Compose Pass"),
      color_attachments: &[Some(wgpu::RenderPassColorAttachment {
        view: output_view,
        depth_slice: None,
        resolve_target: None,
        ops: wgpu::Operations {
          load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
          store: wgpu::StoreOp::Store,
        },
      })],
      depth_stencil_attachment: None,
      occlusion_query_set: None,
      multiview_mask: None,
      timestamp_writes: None,
    });
    render_pass.set_pipeline(&self.compose_pipeline);
    render_pass.set_bind_group(0, &compose_bind_group, &[]);
    render_pass.draw(0..6, 0..1);
  }
}

/// Copy `scene_texture` into the scene copy of `targets` (the same size).
fn copy_scene(
  encoder: &mut wgpu::CommandEncoder,
  scene_texture: &wgpu::Texture,
  targets: &LightTargets,
) {
  encoder.copy_texture_to_texture(
    wgpu::TexelCopyTextureInfo {
      texture: scene_texture,
      mip_level: 0,
      origin: wgpu::Origin3d::ZERO,
      aspect: wgpu::TextureAspect::All,
    },
    wgpu::TexelCopyTextureInfo {
      texture: &targets.scene_copy_texture,
      mip_level: 0,
      origin: wgpu::Origin3d::ZERO,
      aspect: wgpu::TextureAspect::All,
    },
    wgpu::Extent3d {
      width: targets.width,
      height: targets.height,
      depth_or_array_layers: 1,
    },
  );
}
