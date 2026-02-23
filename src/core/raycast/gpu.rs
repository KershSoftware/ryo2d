//! GPU renderer for raycasting.
//!
//! Manages wgpu pipelines, bind group layouts, buffers, and textures for the
//! DDA wall raycaster and instanced billboard sprite renderer.

use super::systems::{CameraUniform, LightInstance, MapInfoUniform, SpriteInstance};
use super::world::RaycastWorld;

/// Maximum number of sprite instances per frame.
pub const MAX_SPRITES: usize = 1024;

/// Maximum number of dynamic lights.
pub const MAX_LIGHTS: usize = 128;

/// Maximum map cells (256 x 256 grid).
pub const MAX_MAP_CELLS: usize = 256 * 256;

/// GPU resource manager for the raycasting system.
///
/// Owns two render pipelines (walls and sprites), their bind group layouts,
/// all uniform/storage buffers, the depth texture written by the wall pass
/// and read by the sprite pass, and optional texture arrays for walls, floors,
/// and sprites.
pub struct GpuRaycastRenderer {
  wall_pipeline: wgpu::RenderPipeline,
  sprite_pipeline: wgpu::RenderPipeline,
  wall_bind_group_layout: wgpu::BindGroupLayout,
  sprite_bind_group_layout: wgpu::BindGroupLayout,
  camera_buffer: wgpu::Buffer,
  map_buffer: wgpu::Buffer,
  map_info_buffer: wgpu::Buffer,
  light_buffer: wgpu::Buffer,
  light_count_buffer: wgpu::Buffer,
  sprite_buffer: wgpu::Buffer,
  depth_texture: wgpu::Texture,
  depth_texture_view: wgpu::TextureView,
  wall_texture_array: Option<wgpu::Texture>,
  wall_texture_array_view: Option<wgpu::TextureView>,
  floor_texture_array: Option<wgpu::Texture>,
  floor_texture_array_view: Option<wgpu::TextureView>,
  sprite_texture_array: Option<wgpu::Texture>,
  sprite_texture_array_view: Option<wgpu::TextureView>,
  sampler: wgpu::Sampler,
  width: u32,
  height: u32,
  sprite_count: u32,
  world: Option<RaycastWorld>,
  map_info: MapInfoUniform,
  pending_wall_tex: Option<(Vec<Vec<u8>>, u32, u32)>,
  pending_floor_tex: Option<(Vec<Vec<u8>>, u32, u32)>,
  pending_sprite_tex: Option<(Vec<Vec<u8>>, u32, u32)>,
  cached_wall_bind_group: Option<wgpu::BindGroup>,
  cached_sprite_bind_group: Option<wgpu::BindGroup>,
  enabled: bool,
}

impl GpuRaycastRenderer {
  /// Create a new GPU raycasting renderer.
  pub fn new(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    output_format: wgpu::TextureFormat,
  ) -> Self {
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
      label: Some("Raycast Sampler"),
      address_mode_u: wgpu::AddressMode::Repeat,
      address_mode_v: wgpu::AddressMode::Repeat,
      address_mode_w: wgpu::AddressMode::Repeat,
      mag_filter: wgpu::FilterMode::Nearest,
      min_filter: wgpu::FilterMode::Nearest,
      mipmap_filter: wgpu::FilterMode::Nearest,
      ..Default::default()
    });

    let (depth_texture, depth_texture_view) = Self::create_depth_texture(device, width, height);

    let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
      label: Some("Raycast Camera Buffer"),
      size: 48,
      usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
      mapped_at_creation: false,
    });

    let map_buffer = device.create_buffer(&wgpu::BufferDescriptor {
      label: Some("Raycast Map Buffer"),
      size: (MAX_MAP_CELLS * 3 * 4) as u64,
      usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
      mapped_at_creation: false,
    });

    let map_info_buffer = device.create_buffer(&wgpu::BufferDescriptor {
      label: Some("Raycast MapInfo Buffer"),
      size: 32,
      usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
      mapped_at_creation: false,
    });

    let light_buffer = device.create_buffer(&wgpu::BufferDescriptor {
      label: Some("Raycast Light Buffer"),
      size: (MAX_LIGHTS * 32) as u64,
      usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
      mapped_at_creation: false,
    });

    let light_count_buffer = device.create_buffer(&wgpu::BufferDescriptor {
      label: Some("Raycast Light Count Buffer"),
      size: 16,
      usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
      mapped_at_creation: false,
    });

    let sprite_buffer = device.create_buffer(&wgpu::BufferDescriptor {
      label: Some("Raycast Sprite Buffer"),
      size: (MAX_SPRITES * 32) as u64,
      usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
      mapped_at_creation: false,
    });

    let wall_bind_group_layout =
      device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Raycast Wall Bind Group Layout"),
        entries: &[
          wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
              ty: wgpu::BufferBindingType::Uniform,
              has_dynamic_offset: false,
              min_binding_size: None,
            },
            count: None,
          },
          wgpu::BindGroupLayoutEntry {
            binding: 1,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
              ty: wgpu::BufferBindingType::Storage { read_only: true },
              has_dynamic_offset: false,
              min_binding_size: None,
            },
            count: None,
          },
          wgpu::BindGroupLayoutEntry {
            binding: 2,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
              ty: wgpu::BufferBindingType::Uniform,
              has_dynamic_offset: false,
              min_binding_size: None,
            },
            count: None,
          },
          wgpu::BindGroupLayoutEntry {
            binding: 3,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
              multisampled: false,
              view_dimension: wgpu::TextureViewDimension::D2Array,
              sample_type: wgpu::TextureSampleType::Float { filterable: false },
            },
            count: None,
          },
          wgpu::BindGroupLayoutEntry {
            binding: 4,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
              multisampled: false,
              view_dimension: wgpu::TextureViewDimension::D2Array,
              sample_type: wgpu::TextureSampleType::Float { filterable: false },
            },
            count: None,
          },
          wgpu::BindGroupLayoutEntry {
            binding: 5,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
            count: None,
          },
          wgpu::BindGroupLayoutEntry {
            binding: 6,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
              ty: wgpu::BufferBindingType::Storage { read_only: true },
              has_dynamic_offset: false,
              min_binding_size: None,
            },
            count: None,
          },
          wgpu::BindGroupLayoutEntry {
            binding: 7,
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

    let sprite_bind_group_layout =
      device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Raycast Sprite Bind Group Layout"),
        entries: &[
          wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
              ty: wgpu::BufferBindingType::Uniform,
              has_dynamic_offset: false,
              min_binding_size: None,
            },
            count: None,
          },
          wgpu::BindGroupLayoutEntry {
            binding: 1,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
              ty: wgpu::BufferBindingType::Storage { read_only: true },
              has_dynamic_offset: false,
              min_binding_size: None,
            },
            count: None,
          },
          wgpu::BindGroupLayoutEntry {
            binding: 2,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
              multisampled: false,
              view_dimension: wgpu::TextureViewDimension::D2Array,
              sample_type: wgpu::TextureSampleType::Float { filterable: false },
            },
            count: None,
          },
          wgpu::BindGroupLayoutEntry {
            binding: 3,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
            count: None,
          },
          wgpu::BindGroupLayoutEntry {
            binding: 4,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
              multisampled: false,
              view_dimension: wgpu::TextureViewDimension::D2,
              sample_type: wgpu::TextureSampleType::Float { filterable: false },
            },
            count: None,
          },
        ],
      });

    let wall_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some("Raycast Wall Shader"),
      source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/core/raycast.wgsl").into()),
    });

    let sprite_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some("Raycast Sprite Shader"),
      source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/core/raycast_sprite.wgsl").into()),
    });

    let wall_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("Raycast Wall Pipeline Layout"),
      bind_group_layouts: &[&wall_bind_group_layout],
      push_constant_ranges: &[],
    });

    let sprite_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("Raycast Sprite Pipeline Layout"),
      bind_group_layouts: &[&sprite_bind_group_layout],
      push_constant_ranges: &[],
    });

    let wall_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some("Raycast Wall Pipeline"),
      layout: Some(&wall_pipeline_layout),
      vertex: wgpu::VertexState {
        module: &wall_shader,
        entry_point: "vs_main",
        buffers: &[],
        compilation_options: Default::default(),
      },
      fragment: Some(wgpu::FragmentState {
        module: &wall_shader,
        entry_point: "fs_main",
        targets: &[
          Some(wgpu::ColorTargetState {
            format: output_format,
            blend: Some(wgpu::BlendState::REPLACE),
            write_mask: wgpu::ColorWrites::ALL,
          }),
          Some(wgpu::ColorTargetState {
            format: wgpu::TextureFormat::R32Float,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
          }),
        ],
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
      multiview: None,
      cache: None,
    });

    let sprite_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some("Raycast Sprite Pipeline"),
      layout: Some(&sprite_pipeline_layout),
      vertex: wgpu::VertexState {
        module: &sprite_shader,
        entry_point: "vs_main",
        buffers: &[],
        compilation_options: Default::default(),
      },
      fragment: Some(wgpu::FragmentState {
        module: &sprite_shader,
        entry_point: "fs_main",
        targets: &[Some(wgpu::ColorTargetState {
          format: output_format,
          blend: Some(wgpu::BlendState::ALPHA_BLENDING),
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
      multiview: None,
      cache: None,
    });

    Self {
      wall_pipeline,
      sprite_pipeline,
      wall_bind_group_layout,
      sprite_bind_group_layout,
      camera_buffer,
      map_buffer,
      map_info_buffer,
      light_buffer,
      light_count_buffer,
      sprite_buffer,
      depth_texture,
      depth_texture_view,
      wall_texture_array: None,
      wall_texture_array_view: None,
      floor_texture_array: None,
      floor_texture_array_view: None,
      sprite_texture_array: None,
      sprite_texture_array_view: None,
      sampler,
      width,
      height,
      sprite_count: 0,
      world: None,
      map_info: MapInfoUniform {
        dimensions: [0; 2],
        max_distance: 64.0,
        fog_density: 0.03,
        fog_color: [0.1, 0.1, 0.15],
        _pad: 0.0,
      },
      pending_wall_tex: None,
      pending_floor_tex: None,
      pending_sprite_tex: None,
      cached_wall_bind_group: None,
      cached_sprite_bind_group: None,
      enabled: false,
    }
  }

  fn create_depth_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
  ) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
      label: Some("Raycast Depth Texture"),
      size: wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
      },
      mip_level_count: 1,
      sample_count: 1,
      dimension: wgpu::TextureDimension::D2,
      format: wgpu::TextureFormat::R32Float,
      usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
      view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
  }

  /// Whether the raycasting renderer is currently active.
  pub fn is_enabled(&self) -> bool {
    self.enabled
  }

  /// Enable or disable the raycasting renderer.
  pub fn set_enabled(&mut self, enabled: bool) {
    self.enabled = enabled;
  }

  /// Store the raycasting world map. Called from SceneContext without GPU access.
  pub fn set_world(&mut self, world: RaycastWorld) {
    assert!(
      world.width() * world.height() <= MAX_MAP_CELLS,
      "RaycastWorld exceeds MAX_MAP_CELLS ({} x {} = {}, max = {})",
      world.width(),
      world.height(),
      world.width() * world.height(),
      MAX_MAP_CELLS,
    );
    self.map_info.dimensions = [world.width() as u32, world.height() as u32];
    self.world = Some(world);
  }

  /// Get a mutable reference to the stored world.
  pub fn world_mut(&mut self) -> Option<&mut RaycastWorld> {
    self.world.as_mut()
  }

  /// Get a reference to the stored world.
  pub fn world(&self) -> Option<&RaycastWorld> {
    self.world.as_ref()
  }

  /// Drop the stored world map, freeing its memory.
  pub fn drop_world(&mut self) {
    self.world = None;
  }

  /// Set the map info uniform (fog, max distance, etc).
  pub fn set_map_info(&mut self, info: MapInfoUniform) {
    self.map_info = info;
  }

  /// Queue wall textures for GPU upload. Each Vec<u8> is RGBA data.
  pub fn queue_wall_textures(&mut self, images: Vec<Vec<u8>>, width: u32, height: u32) {
    self.pending_wall_tex = Some((images, width, height));
  }

  /// Queue floor/ceiling textures for GPU upload.
  pub fn queue_floor_textures(&mut self, images: Vec<Vec<u8>>, width: u32, height: u32) {
    self.pending_floor_tex = Some((images, width, height));
  }

  /// Queue sprite textures for GPU upload.
  pub fn queue_sprite_textures(&mut self, images: Vec<Vec<u8>>, width: u32, height: u32) {
    self.pending_sprite_tex = Some((images, width, height));
  }

  /// Flush all pending data to GPU. Called by engine before rendering.
  pub fn flush_pending(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
    // Create textures from pending raw data
    if let Some((images, w, h)) = self.pending_wall_tex.take() {
      let refs: Vec<&[u8]> = images.iter().map(|v| v.as_slice()).collect();
      let (tex, view) = Self::create_texture_array(device, queue, &refs, w, h, "Wall Textures");
      self.set_wall_textures(tex, view);
    }
    if let Some((images, w, h)) = self.pending_floor_tex.take() {
      let refs: Vec<&[u8]> = images.iter().map(|v| v.as_slice()).collect();
      let (tex, view) = Self::create_texture_array(device, queue, &refs, w, h, "Floor Textures");
      self.set_floor_textures(tex, view);
    }
    if let Some((images, w, h)) = self.pending_sprite_tex.take() {
      let refs: Vec<&[u8]> = images.iter().map(|v| v.as_slice()).collect();
      let (tex, view) = Self::create_texture_array(device, queue, &refs, w, h, "Sprite Textures");
      self.set_sprite_textures(tex, view);
    }

    // Upload world map data only when changed
    if let Some(ref mut world) = self.world
      && world.is_dirty()
    {
      let gpu_data = world.pack_for_gpu();
      queue.write_buffer(&self.map_buffer, 0, bytemuck::cast_slice(&gpu_data));
      queue.write_buffer(
        &self.map_info_buffer,
        0,
        bytemuck::cast_slice(&[self.map_info]),
      );
      world.clear_dirty();
    }

    // Rebuild cached bind groups when invalidated
    if self.cached_wall_bind_group.is_none()
      && let (Some(wall_view), Some(floor_view)) = (
        self.wall_texture_array_view.as_ref(),
        self.floor_texture_array_view.as_ref(),
      )
    {
      self.cached_wall_bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Raycast Wall Bind Group (cached)"),
        layout: &self.wall_bind_group_layout,
        entries: &[
          wgpu::BindGroupEntry {
            binding: 0,
            resource: self.camera_buffer.as_entire_binding(),
          },
          wgpu::BindGroupEntry {
            binding: 1,
            resource: self.map_buffer.as_entire_binding(),
          },
          wgpu::BindGroupEntry {
            binding: 2,
            resource: self.map_info_buffer.as_entire_binding(),
          },
          wgpu::BindGroupEntry {
            binding: 3,
            resource: wgpu::BindingResource::TextureView(wall_view),
          },
          wgpu::BindGroupEntry {
            binding: 4,
            resource: wgpu::BindingResource::TextureView(floor_view),
          },
          wgpu::BindGroupEntry {
            binding: 5,
            resource: wgpu::BindingResource::Sampler(&self.sampler),
          },
          wgpu::BindGroupEntry {
            binding: 6,
            resource: self.light_buffer.as_entire_binding(),
          },
          wgpu::BindGroupEntry {
            binding: 7,
            resource: self.light_count_buffer.as_entire_binding(),
          },
        ],
      }));
    }

    if self.cached_sprite_bind_group.is_none()
      && let Some(sprite_view) = self.sprite_texture_array_view.as_ref()
    {
      self.cached_sprite_bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Raycast Sprite Bind Group (cached)"),
        layout: &self.sprite_bind_group_layout,
        entries: &[
          wgpu::BindGroupEntry {
            binding: 0,
            resource: self.camera_buffer.as_entire_binding(),
          },
          wgpu::BindGroupEntry {
            binding: 1,
            resource: self.sprite_buffer.as_entire_binding(),
          },
          wgpu::BindGroupEntry {
            binding: 2,
            resource: wgpu::BindingResource::TextureView(sprite_view),
          },
          wgpu::BindGroupEntry {
            binding: 3,
            resource: wgpu::BindingResource::Sampler(&self.sampler),
          },
          wgpu::BindGroupEntry {
            binding: 4,
            resource: wgpu::BindingResource::TextureView(&self.depth_texture_view),
          },
        ],
      }));
    }
  }

  /// Upload camera parameters to the GPU.
  pub fn update_camera(&self, queue: &wgpu::Queue, camera: &CameraUniform) {
    queue.write_buffer(&self.camera_buffer, 0, bytemuck::cast_slice(&[*camera]));
  }

  /// Upload map cell data and metadata to the GPU.
  pub fn update_map(&self, queue: &wgpu::Queue, world: &RaycastWorld, map_info: &MapInfoUniform) {
    let packed = world.pack_for_gpu();
    queue.write_buffer(&self.map_buffer, 0, bytemuck::cast_slice(&packed));
    queue.write_buffer(&self.map_info_buffer, 0, bytemuck::cast_slice(&[*map_info]));
  }

  /// Upload sprite instance data and update the instance count.
  pub fn update_sprites(&mut self, queue: &wgpu::Queue, sprites: &[SpriteInstance]) {
    self.sprite_count = sprites.len().min(MAX_SPRITES) as u32;
    if !sprites.is_empty() {
      let count = sprites.len().min(MAX_SPRITES);
      queue.write_buffer(
        &self.sprite_buffer,
        0,
        bytemuck::cast_slice(&sprites[..count]),
      );
    }
  }

  /// Upload light instance data and the light count.
  pub fn update_lights(&self, queue: &wgpu::Queue, lights: &[LightInstance]) {
    let count = lights.len().min(MAX_LIGHTS);
    let count_padded: [u32; 4] = [count as u32, 0, 0, 0];
    queue.write_buffer(
      &self.light_count_buffer,
      0,
      bytemuck::cast_slice(&count_padded),
    );
    if count > 0 {
      queue.write_buffer(
        &self.light_buffer,
        0,
        bytemuck::cast_slice(&lights[..count]),
      );
    }
  }

  /// Create a texture_2d_array from a set of RGBA8 images.
  pub fn create_texture_array(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    images: &[&[u8]],
    width: u32,
    height: u32,
    label: &str,
  ) -> (wgpu::Texture, wgpu::TextureView) {
    let layer_count = images.len() as u32;

    let texture = device.create_texture(&wgpu::TextureDescriptor {
      label: Some(label),
      size: wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: layer_count,
      },
      mip_level_count: 1,
      sample_count: 1,
      dimension: wgpu::TextureDimension::D2,
      format: wgpu::TextureFormat::Rgba8UnormSrgb,
      usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
      view_formats: &[],
    });

    let bytes_per_row = 4 * width;
    for (i, image_data) in images.iter().enumerate() {
      queue.write_texture(
        wgpu::ImageCopyTexture {
          texture: &texture,
          mip_level: 0,
          origin: wgpu::Origin3d {
            x: 0,
            y: 0,
            z: i as u32,
          },
          aspect: wgpu::TextureAspect::All,
        },
        image_data,
        wgpu::ImageDataLayout {
          offset: 0,
          bytes_per_row: Some(bytes_per_row),
          rows_per_image: Some(height),
        },
        wgpu::Extent3d {
          width,
          height,
          depth_or_array_layers: 1,
        },
      );
    }

    let view = texture.create_view(&wgpu::TextureViewDescriptor {
      dimension: Some(wgpu::TextureViewDimension::D2Array),
      ..Default::default()
    });

    (texture, view)
  }

  /// Set the wall texture array.
  pub fn set_wall_textures(&mut self, texture: wgpu::Texture, view: wgpu::TextureView) {
    self.wall_texture_array = Some(texture);
    self.wall_texture_array_view = Some(view);
    self.cached_wall_bind_group = None;
  }

  /// Set the floor/ceiling texture array.
  pub fn set_floor_textures(&mut self, texture: wgpu::Texture, view: wgpu::TextureView) {
    self.floor_texture_array = Some(texture);
    self.floor_texture_array_view = Some(view);
    self.cached_wall_bind_group = None;
  }

  /// Set the sprite texture array.
  pub fn set_sprite_textures(&mut self, texture: wgpu::Texture, view: wgpu::TextureView) {
    self.sprite_texture_array = Some(texture);
    self.sprite_texture_array_view = Some(view);
    self.cached_sprite_bind_group = None;
  }

  /// Render the wall (DDA raycasting) pass.
  pub fn render_walls(
    &self,
    _device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    output_view: &wgpu::TextureView,
  ) {
    let bind_group = match self.cached_wall_bind_group.as_ref() {
      Some(bg) => bg,
      None => return,
    };

    {
      let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Raycast Wall Pass"),
        color_attachments: &[
          Some(wgpu::RenderPassColorAttachment {
            view: output_view,
            resolve_target: None,
            ops: wgpu::Operations {
              load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
              store: wgpu::StoreOp::Store,
            },
          }),
          Some(wgpu::RenderPassColorAttachment {
            view: &self.depth_texture_view,
            resolve_target: None,
            ops: wgpu::Operations {
              load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
              store: wgpu::StoreOp::Store,
            },
          }),
        ],
        depth_stencil_attachment: None,
        occlusion_query_set: None,
        timestamp_writes: None,
      });

      render_pass.set_pipeline(&self.wall_pipeline);
      render_pass.set_bind_group(0, bind_group, &[]);
      render_pass.draw(0..6, 0..1);
    }
  }

  /// Render the sprite (billboard) pass.
  pub fn render_sprites(
    &self,
    _device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    output_view: &wgpu::TextureView,
  ) {
    if self.sprite_count == 0 {
      return;
    }

    let bind_group = match self.cached_sprite_bind_group.as_ref() {
      Some(bg) => bg,
      None => return,
    };

    {
      let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Raycast Sprite Pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
          view: output_view,
          resolve_target: None,
          ops: wgpu::Operations {
            load: wgpu::LoadOp::Load,
            store: wgpu::StoreOp::Store,
          },
        })],
        depth_stencil_attachment: None,
        occlusion_query_set: None,
        timestamp_writes: None,
      });

      render_pass.set_pipeline(&self.sprite_pipeline);
      render_pass.set_bind_group(0, bind_group, &[]);
      render_pass.draw(0..6, 0..self.sprite_count);
    }
  }

  /// Recreate the depth texture after a window resize.
  pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
    self.width = width;
    self.height = height;
    let (tex, view) = Self::create_depth_texture(device, width, height);
    self.depth_texture = tex;
    self.depth_texture_view = view;
    // Sprite bind group references the depth texture view
    self.cached_sprite_bind_group = None;
  }
}
