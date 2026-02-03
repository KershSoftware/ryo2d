//! WGPU-based software renderer for 2D games.
//!
//! This module provides a renderer that:
//! 1. Maintains a CPU-side pixel buffer (like a framebuffer)
//! 2. Uploads it to a GPU texture each frame
//! 3. Applies post-processing effects (optional)
//! 4. Renders the texture to screen with proper letterboxing/scaling

use std::sync::Arc;
use wgpu::util::DeviceExt;
use winit::window::Window;

use crate::core::engine::WindowMode;
use crate::core::shaders::{EffectsManager, PostProcessPipeline};

/// Handles wgpu rendering of a software pixel buffer to screen.
pub struct WgpuRenderer {
  surface: wgpu::Surface<'static>,
  device: wgpu::Device,
  queue: wgpu::Queue,
  config: wgpu::SurfaceConfiguration,
  // The game texture (GPU sprites render here first, then CPU content blended on top)
  texture: wgpu::Texture,
  texture_view: wgpu::TextureView,
  sampler: wgpu::Sampler,
  // Staging texture for CPU pixel buffer (used for alpha-blending onto game texture)
  cpu_texture: wgpu::Texture,
  #[allow(dead_code)] // View is used indirectly through cpu_bind_group
  cpu_texture_view: wgpu::TextureView,
  cpu_bind_group: wgpu::BindGroup,
  // Pipeline for rendering texture to screen (REPLACE blend)
  render_pipeline: wgpu::RenderPipeline,
  // Pipeline for alpha-blending overlay onto surface (for UI compositing)
  blend_pipeline: wgpu::RenderPipeline,
  // Pipeline for alpha-blending CPU content onto game texture (Rgba8UnormSrgb format)
  game_blend_pipeline: wgpu::RenderPipeline,
  bind_group: wgpu::BindGroup,
  bind_group_layout: wgpu::BindGroupLayout,
  // Uniform buffer for viewport transform
  uniform_buffer: wgpu::Buffer,
  // Game resolution
  game_width: u32,
  game_height: u32,
  // Current window size
  window_width: u32,
  window_height: u32,
  // Window mode for scaling
  window_mode: WindowMode,
  // Max viewport constraints
  max_width: Option<u32>,
  max_height: Option<u32>,
  // CPU-side pixel buffer (RGBA)
  pixel_buffer: Vec<u8>,
  // Post-processing pipeline
  post_process: Option<PostProcessPipeline>,
  // Whether presentation blocks until vsync (Fifo mode)
  // When true, manual frame limiting should be skipped to avoid double-waiting
  presentation_blocks: bool,
}

/// Viewport uniform data sent to shader
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct ViewportUniform {
  // Scale and offset for rendering the quad
  // x: scale_x, y: scale_y, z: offset_x, w: offset_y
  transform: [f32; 4],
}

impl WgpuRenderer {
  /// Create a new renderer.
  ///
  /// # Arguments
  /// - `window`: The window to render to
  /// - `game_width`: Internal game resolution width
  /// - `game_height`: Internal game resolution height
  /// - `window_mode`: How to scale the game viewport to window size
  /// - `max_width`: Optional maximum viewport width constraint
  /// - `max_height`: Optional maximum viewport height constraint
  /// - `vsync`: VSync preference (None = Mailbox→Immediate, Some(true) = Mailbox→Fifo, Some(false) = Immediate)
  pub async fn new(
    window: Arc<Window>,
    game_width: u32,
    game_height: u32,
    window_mode: WindowMode,
    max_width: Option<u32>,
    max_height: Option<u32>,
    vsync: Option<bool>,
  ) -> Self {
    let size = window.inner_size();

    // Create wgpu instance
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
      backends: wgpu::Backends::all(),
      ..Default::default()
    });

    // Create surface
    let surface = instance.create_surface(window).unwrap();

    // Request adapter
    let adapter = instance
      .request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        compatible_surface: Some(&surface),
        force_fallback_adapter: false,
      })
      .await
      .expect("Failed to find an appropriate adapter");

    // Request device and queue
    let (device, queue) = adapter
      .request_device(
        &wgpu::DeviceDescriptor {
          label: Some("Ryo2D Device"),
          required_features: wgpu::Features::empty(),
          required_limits: wgpu::Limits::default(),
          memory_hints: Default::default(),
        },
        None,
      )
      .await
      .expect("Failed to create device");

    // Configure surface
    let surface_caps = surface.get_capabilities(&adapter);
    let surface_format = surface_caps
      .formats
      .iter()
      .find(|f| f.is_srgb())
      .copied()
      .unwrap_or(surface_caps.formats[0]);

    // Choose present mode based on vsync preference
    let present_mode = match vsync {
      // Not configured: Mailbox (best of both worlds) → Immediate (no tearing, max perf)
      None => {
        if surface_caps
          .present_modes
          .contains(&wgpu::PresentMode::Mailbox)
        {
          wgpu::PresentMode::Mailbox
        } else {
          wgpu::PresentMode::Immediate
        }
      }
      // VSync on: Mailbox → Fifo (traditional vsync)
      Some(true) => {
        if surface_caps
          .present_modes
          .contains(&wgpu::PresentMode::Mailbox)
        {
          wgpu::PresentMode::Mailbox
        } else {
          wgpu::PresentMode::Fifo
        }
      }
      // VSync off: Immediate (maximum performance, may tear)
      Some(false) => wgpu::PresentMode::Immediate,
    };

    let config = wgpu::SurfaceConfiguration {
      usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
      format: surface_format,
      width: size.width.max(1),
      height: size.height.max(1),
      present_mode,
      alpha_mode: surface_caps.alpha_modes[0],
      view_formats: vec![],
      desired_maximum_frame_latency: 2,
    };
    surface.configure(&device, &config);

    // Create the game texture (where we upload our pixel buffer)
    let texture = device.create_texture(&wgpu::TextureDescriptor {
      label: Some("Game Texture"),
      size: wgpu::Extent3d {
        width: game_width,
        height: game_height,
        depth_or_array_layers: 1,
      },
      mip_level_count: 1,
      sample_count: 1,
      dimension: wgpu::TextureDimension::D2,
      format: wgpu::TextureFormat::Rgba8UnormSrgb,
      usage: wgpu::TextureUsages::TEXTURE_BINDING
        | wgpu::TextureUsages::COPY_DST
        | wgpu::TextureUsages::COPY_SRC // Allow copying from this texture for post-processing
        | wgpu::TextureUsages::RENDER_ATTACHMENT, // Allow GPU sprite rendering to this texture
      view_formats: &[],
    });

    let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    // Create CPU staging texture (for uploading CPU pixel buffer and blending onto game texture)
    let cpu_texture = device.create_texture(&wgpu::TextureDescriptor {
      label: Some("CPU Staging Texture"),
      size: wgpu::Extent3d {
        width: game_width,
        height: game_height,
        depth_or_array_layers: 1,
      },
      mip_level_count: 1,
      sample_count: 1,
      dimension: wgpu::TextureDimension::D2,
      format: wgpu::TextureFormat::Rgba8UnormSrgb,
      usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
      view_formats: &[],
    });
    let cpu_texture_view = cpu_texture.create_view(&wgpu::TextureViewDescriptor::default());

    // Create sampler (nearest neighbor for pixel art crispness)
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
      label: Some("Game Sampler"),
      address_mode_u: wgpu::AddressMode::ClampToEdge,
      address_mode_v: wgpu::AddressMode::ClampToEdge,
      address_mode_w: wgpu::AddressMode::ClampToEdge,
      mag_filter: wgpu::FilterMode::Nearest,
      min_filter: wgpu::FilterMode::Nearest,
      mipmap_filter: wgpu::FilterMode::Nearest,
      ..Default::default()
    });

    // Create uniform buffer for viewport transform (changes based on window size/mode)
    let uniform = ViewportUniform {
      transform: [1.0, 1.0, 0.0, 0.0],
    };
    let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
      label: Some("Viewport Uniform Buffer"),
      contents: bytemuck::cast_slice(&[uniform]),
      usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });

    // Create identity uniform buffer for game texture blending (always 1:1 scale)
    let identity_uniform = ViewportUniform {
      transform: [1.0, 1.0, 0.0, 0.0],
    };
    let identity_uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
      label: Some("Identity Uniform Buffer"),
      contents: bytemuck::cast_slice(&[identity_uniform]),
      usage: wgpu::BufferUsages::UNIFORM, // No COPY_DST needed - never changes
    });

    // Create bind group layout
    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
      label: Some("Texture Bind Group Layout"),
      entries: &[
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
        wgpu::BindGroupLayoutEntry {
          binding: 1,
          visibility: wgpu::ShaderStages::FRAGMENT,
          ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
          count: None,
        },
        wgpu::BindGroupLayoutEntry {
          binding: 2,
          visibility: wgpu::ShaderStages::VERTEX,
          ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
          },
          count: None,
        },
      ],
    });

    // Create bind group for main game texture
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
      label: Some("Texture Bind Group"),
      layout: &bind_group_layout,
      entries: &[
        wgpu::BindGroupEntry {
          binding: 0,
          resource: wgpu::BindingResource::TextureView(&texture_view),
        },
        wgpu::BindGroupEntry {
          binding: 1,
          resource: wgpu::BindingResource::Sampler(&sampler),
        },
        wgpu::BindGroupEntry {
          binding: 2,
          resource: uniform_buffer.as_entire_binding(),
        },
      ],
    });

    // Create bind group for CPU staging texture (for blending onto game texture)
    // Uses identity_uniform_buffer for 1:1 scale (viewport scaling only on final screen render)
    let cpu_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
      label: Some("CPU Texture Bind Group"),
      layout: &bind_group_layout,
      entries: &[
        wgpu::BindGroupEntry {
          binding: 0,
          resource: wgpu::BindingResource::TextureView(&cpu_texture_view),
        },
        wgpu::BindGroupEntry {
          binding: 1,
          resource: wgpu::BindingResource::Sampler(&sampler),
        },
        wgpu::BindGroupEntry {
          binding: 2,
          resource: identity_uniform_buffer.as_entire_binding(),
        },
      ],
    });

    // Create shader module
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some("Fullscreen Quad Shader"),
      source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/core/fullscreen_quad.wgsl").into()),
    });

    // Create pipeline layout
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("Render Pipeline Layout"),
      bind_group_layouts: &[&bind_group_layout],
      push_constant_ranges: &[],
    });

    // Create render pipeline (REPLACE blend - for drawing scene)
    let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some("Render Pipeline"),
      layout: Some(&pipeline_layout),
      vertex: wgpu::VertexState {
        module: &shader,
        entry_point: "vs_main",
        buffers: &[],
        compilation_options: Default::default(),
      },
      fragment: Some(wgpu::FragmentState {
        module: &shader,
        entry_point: "fs_main",
        targets: &[Some(wgpu::ColorTargetState {
          format: surface_format,
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
      multiview: None,
      cache: None,
    });

    // Create blend pipeline (ALPHA_BLENDING - for UI overlay onto surface)
    let blend_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some("Blend Pipeline"),
      layout: Some(&pipeline_layout),
      vertex: wgpu::VertexState {
        module: &shader,
        entry_point: "vs_main",
        buffers: &[],
        compilation_options: Default::default(),
      },
      fragment: Some(wgpu::FragmentState {
        module: &shader,
        entry_point: "fs_main",
        targets: &[Some(wgpu::ColorTargetState {
          format: surface_format,
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

    // Create game blend pipeline (ALPHA_BLENDING - for CPU content onto game texture)
    // Uses Rgba8UnormSrgb format to match the game texture
    let game_blend_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some("Game Blend Pipeline"),
      layout: Some(&pipeline_layout),
      vertex: wgpu::VertexState {
        module: &shader,
        entry_point: "vs_main",
        buffers: &[],
        compilation_options: Default::default(),
      },
      fragment: Some(wgpu::FragmentState {
        module: &shader,
        entry_point: "fs_main",
        targets: &[Some(wgpu::ColorTargetState {
          format: wgpu::TextureFormat::Rgba8UnormSrgb, // Game texture format
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

    // Create CPU-side pixel buffer pre-filled with dark charcoal background
    // This color is presented immediately to prevent flash on startup
    let mut pixel_buffer = vec![0u8; (game_width * game_height * 4) as usize];
    for pixel in pixel_buffer.chunks_exact_mut(4) {
      pixel[0] = 0x2d; // R: 45
      pixel[1] = 0x2d; // G: 45
      pixel[2] = 0x2d; // B: 45
      pixel[3] = 0xff; // A: 255
    }

    // Upload initial pixel buffer to GPU texture immediately
    queue.write_texture(
      wgpu::ImageCopyTexture {
        texture: &texture,
        mip_level: 0,
        origin: wgpu::Origin3d::ZERO,
        aspect: wgpu::TextureAspect::All,
      },
      &pixel_buffer,
      wgpu::ImageDataLayout {
        offset: 0,
        bytes_per_row: Some(4 * game_width),
        rows_per_image: Some(game_height),
      },
      wgpu::Extent3d {
        width: game_width,
        height: game_height,
        depth_or_array_layers: 1,
      },
    );

    // Create post-process pipeline
    let post_process = PostProcessPipeline::new(
      &device,
      game_width,
      game_height,
      wgpu::TextureFormat::Rgba8UnormSrgb,
    );

    // Fifo mode blocks until vsync - no manual frame limiting needed
    let presentation_blocks = present_mode == wgpu::PresentMode::Fifo;

    let mut renderer = WgpuRenderer {
      surface,
      device,
      queue,
      config,
      texture,
      texture_view,
      sampler,
      cpu_texture,
      cpu_texture_view,
      cpu_bind_group,
      render_pipeline,
      blend_pipeline,
      game_blend_pipeline,
      bind_group,
      bind_group_layout,
      uniform_buffer,
      game_width,
      game_height,
      window_width: size.width,
      window_height: size.height,
      window_mode,
      max_width,
      max_height,
      pixel_buffer,
      post_process: Some(post_process),
      presentation_blocks,
    };

    // Initialize viewport transform
    renderer.update_viewport_transform();

    renderer
  }

  /// Get a mutable reference to the pixel buffer for rendering.
  pub fn frame_mut(&mut self) -> &mut [u8] {
    &mut self.pixel_buffer
  }

  /// Get the game width.
  pub fn game_width(&self) -> u32 {
    self.game_width
  }

  /// Get the game height.
  pub fn game_height(&self) -> u32 {
    self.game_height
  }

  /// Check if presentation blocks until vsync (Fifo mode).
  ///
  /// When this returns true, manual frame limiting should be skipped
  /// to avoid double-waiting (vsync already limits framerate).
  pub fn presentation_blocks(&self) -> bool {
    self.presentation_blocks
  }

  /// Get a reference to the wgpu device.
  ///
  /// Used by GPU sprite renderer for creating resources.
  pub fn device(&self) -> &wgpu::Device {
    &self.device
  }

  /// Get a reference to the wgpu queue.
  ///
  /// Used by GPU sprite renderer for uploading data.
  pub fn queue(&self) -> &wgpu::Queue {
    &self.queue
  }

  /// Get the main game texture view.
  ///
  /// GPU sprite rendering draws directly to this texture.
  pub fn game_texture_view(&self) -> &wgpu::TextureView {
    &self.texture_view
  }

  /// Get the surface format.
  pub fn surface_format(&self) -> wgpu::TextureFormat {
    self.config.format
  }

  /// Get the format of the game texture (where GPU sprites render to)
  pub fn game_texture_format(&self) -> wgpu::TextureFormat {
    wgpu::TextureFormat::Rgba8UnormSrgb
  }

  /// Resize the surface when window size changes.
  pub fn resize(&mut self, new_width: u32, new_height: u32) {
    if new_width > 0 && new_height > 0 {
      self.window_width = new_width;
      self.window_height = new_height;
      self.config.width = new_width;
      self.config.height = new_height;
      self.surface.configure(&self.device, &self.config);
      self.update_viewport_transform();
    }
  }

  /// Resize the internal pixel buffer (for stretch mode).
  pub fn resize_buffer(&mut self, new_width: u32, new_height: u32) {
    self.game_width = new_width;
    self.game_height = new_height;
    self.pixel_buffer = vec![0u8; (new_width * new_height * 4) as usize];

    // Recreate texture with new size
    self.texture = self.device.create_texture(&wgpu::TextureDescriptor {
      label: Some("Game Texture"),
      size: wgpu::Extent3d {
        width: new_width,
        height: new_height,
        depth_or_array_layers: 1,
      },
      mip_level_count: 1,
      sample_count: 1,
      dimension: wgpu::TextureDimension::D2,
      format: wgpu::TextureFormat::Rgba8UnormSrgb,
      usage: wgpu::TextureUsages::TEXTURE_BINDING
        | wgpu::TextureUsages::COPY_DST
        | wgpu::TextureUsages::COPY_SRC
        | wgpu::TextureUsages::RENDER_ATTACHMENT,
      view_formats: &[],
    });

    self.texture_view = self
      .texture
      .create_view(&wgpu::TextureViewDescriptor::default());

    // Recreate bind group with new texture view
    let bind_group_layout = self.render_pipeline.get_bind_group_layout(0);
    self.bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
      label: Some("Texture Bind Group"),
      layout: &bind_group_layout,
      entries: &[
        wgpu::BindGroupEntry {
          binding: 0,
          resource: wgpu::BindingResource::TextureView(&self.texture_view),
        },
        wgpu::BindGroupEntry {
          binding: 1,
          resource: wgpu::BindingResource::Sampler(&self.sampler),
        },
        wgpu::BindGroupEntry {
          binding: 2,
          resource: self.uniform_buffer.as_entire_binding(),
        },
      ],
    });

    self.update_viewport_transform();
  }

  /// Update the viewport uniform based on current window mode and sizes.
  fn update_viewport_transform(&mut self) {
    let (scale_x, scale_y, offset_x, offset_y) = self.calculate_viewport();

    let uniform = ViewportUniform {
      transform: [scale_x, scale_y, offset_x, offset_y],
    };

    self
      .queue
      .write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&[uniform]));
  }

  /// Calculate viewport scale and offset based on window mode.
  fn calculate_viewport(&self) -> (f32, f32, f32, f32) {
    let game_width = self.game_width as f32;
    let game_height = self.game_height as f32;
    let window_width = self.window_width as f32;
    let window_height = self.window_height as f32;

    match self.window_mode {
      WindowMode::Fixed | WindowMode::ResizableAspectRatio => {
        let window_aspect = window_width / window_height;
        let game_aspect = game_width / game_height;

        let mut scale = if window_aspect > game_aspect {
          window_height / game_height
        } else {
          window_width / game_width
        };

        // Apply max constraints
        if let Some(max_width) = self.max_width {
          let max_scale_width = max_width as f32 / game_width;
          scale = scale.min(max_scale_width);
        }
        if let Some(max_height) = self.max_height {
          let max_scale_height = max_height as f32 / game_height;
          scale = scale.min(max_scale_height);
        }

        let scaled_width = game_width * scale;
        let scaled_height = game_height * scale;

        // Convert to normalized device coordinates (-1 to 1)
        let scale_x = scaled_width / window_width;
        let scale_y = scaled_height / window_height;

        (scale_x, scale_y, 0.0, 0.0)
      }
      WindowMode::ResizableIntegerScale => {
        let window_aspect = window_width / window_height;
        let game_aspect = game_width / game_height;

        let mut scale = if window_aspect > game_aspect {
          window_height / game_height
        } else {
          window_width / game_width
        };

        // Integer scaling
        scale = scale.floor().max(1.0);

        // Apply max constraints
        if let Some(max_width) = self.max_width {
          let max_scale_width = max_width as f32 / game_width;
          scale = scale.min(max_scale_width.floor().max(1.0));
        }
        if let Some(max_height) = self.max_height {
          let max_scale_height = max_height as f32 / game_height;
          scale = scale.min(max_scale_height.floor().max(1.0));
        }

        let scaled_width = game_width * scale;
        let scaled_height = game_height * scale;

        let scale_x = scaled_width / window_width;
        let scale_y = scaled_height / window_height;

        (scale_x, scale_y, 0.0, 0.0)
      }
      WindowMode::ResizableStretch => {
        // Fill entire window
        (1.0, 1.0, 0.0, 0.0)
      }
    }
  }

  /// Render the pixel buffer to the screen.
  pub fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
    self.render_with_gpu_sprites::<fn(&wgpu::Queue, &mut wgpu::CommandEncoder, &wgpu::TextureView)>(
      None, None,
    )
  }

  /// Render the pixel buffer with post-processing effects.
  pub fn render_with_effects(
    &mut self,
    effects: Option<&EffectsManager>,
  ) -> Result<(), wgpu::SurfaceError> {
    self.render_with_gpu_sprites::<fn(&wgpu::Queue, &mut wgpu::CommandEncoder, &wgpu::TextureView)>(
      effects, None,
    )
  }

  /// Render with optional GPU sprites and post-processing effects.
  ///
  /// GPU sprites and CPU content are properly z-ordered:
  /// - GPU sprites with negative z-order (e.g., parallax backgrounds) render BEHIND CPU content
  /// - CPU content renders on top with alpha blending
  ///
  /// # Render Order
  /// 1. Game texture is cleared
  /// 2. `gpu_sprite_callback` draws GPU sprites (backgrounds, parallax)
  /// 3. CPU pixel buffer is alpha-blended on top
  /// 4. Post-processing effects are applied (if provided)
  /// 5. Final result is rendered to screen with letterboxing
  pub fn render_with_gpu_sprites<F>(
    &mut self,
    effects: Option<&EffectsManager>,
    gpu_sprite_callback: Option<F>,
  ) -> Result<(), wgpu::SurfaceError>
  where
    F: FnOnce(&wgpu::Queue, &mut wgpu::CommandEncoder, &wgpu::TextureView),
  {
    // Step 1: Upload CPU pixel buffer to staging texture (not game texture)
    self.queue.write_texture(
      wgpu::ImageCopyTexture {
        texture: &self.cpu_texture,
        mip_level: 0,
        origin: wgpu::Origin3d::ZERO,
        aspect: wgpu::TextureAspect::All,
      },
      &self.pixel_buffer,
      wgpu::ImageDataLayout {
        offset: 0,
        bytes_per_row: Some(4 * self.game_width),
        rows_per_image: Some(self.game_height),
      },
      wgpu::Extent3d {
        width: self.game_width,
        height: self.game_height,
        depth_or_array_layers: 1,
      },
    );

    // Get surface texture
    let output = self.surface.get_current_texture()?;
    let view = output
      .texture
      .create_view(&wgpu::TextureViewDescriptor::default());

    // Create command encoder
    let mut encoder = self
      .device
      .create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("Render Encoder"),
      });

    // Step 2: Clear game texture (required to prevent ghosting from previous frame)
    {
      let _clear_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Clear Game Texture Pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
          view: &self.texture_view,
          resolve_target: None,
          ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color {
              r: 0.0,
              g: 0.0,
              b: 0.0,
              a: 1.0, // Opaque black as default background
            }),
            store: wgpu::StoreOp::Store,
          },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
      });
    }

    // Step 3: Render GPU sprites (backgrounds, parallax) if callback provided
    if let Some(callback) = gpu_sprite_callback {
      callback(&self.queue, &mut encoder, &self.texture_view);
    }

    // Step 3: Blend CPU content on top of GPU sprites
    // CPU pixel buffer has alpha=0 for transparent areas, alpha=255 for opaque content
    {
      let mut blend_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("CPU Content Blend Pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
          view: &self.texture_view,
          resolve_target: None,
          ops: wgpu::Operations {
            load: wgpu::LoadOp::Load, // Preserve GPU sprite content
            store: wgpu::StoreOp::Store,
          },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
      });

      // Use game blend pipeline to alpha-composite CPU content onto GPU sprites
      // (game_blend_pipeline uses Rgba8UnormSrgb format to match game texture)
      blend_pass.set_pipeline(&self.game_blend_pipeline);
      blend_pass.set_bind_group(0, &self.cpu_bind_group, &[]);
      blend_pass.draw(0..6, 0..1);
    }

    // Check if we have active effects to apply
    let has_effects = effects.map(|e| e.has_active_effects()).unwrap_or(false);

    // Apply post-processing if we have effects
    let use_post_process = if has_effects {
      if let (Some(effects_mgr), Some(post_process)) = (effects, self.post_process.as_mut()) {
        // Collect active effects
        let active: Vec<_> = effects_mgr
          .active_effects()
          .map(|e| (e, effects_mgr.time()))
          .collect();

        if !active.is_empty() {
          // Copy game texture to post-process texture A first
          encoder.copy_texture_to_texture(
            wgpu::ImageCopyTexture {
              texture: &self.texture,
              mip_level: 0,
              origin: wgpu::Origin3d::ZERO,
              aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyTexture {
              texture: post_process.texture_a(),
              mip_level: 0,
              origin: wgpu::Origin3d::ZERO,
              aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
              width: self.game_width,
              height: self.game_height,
              depth_or_array_layers: 1,
            },
          );

          // Apply effects chain
          let output_buffer =
            post_process.apply_effects(&self.device, &self.queue, &mut encoder, &active);

          Some(output_buffer)
        } else {
          None
        }
      } else {
        None
      }
    } else {
      None
    };

    // Final render pass to screen
    {
      // Create bind group for the appropriate source texture
      let final_bind_group = if let Some(output_buffer) = use_post_process {
        let post_process = self.post_process.as_ref().unwrap();
        let output_view = post_process.get_output_view(output_buffer);

        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
          label: Some("Post-Process Output Bind Group"),
          layout: &self.bind_group_layout,
          entries: &[
            wgpu::BindGroupEntry {
              binding: 0,
              resource: wgpu::BindingResource::TextureView(output_view),
            },
            wgpu::BindGroupEntry {
              binding: 1,
              resource: wgpu::BindingResource::Sampler(&self.sampler),
            },
            wgpu::BindGroupEntry {
              binding: 2,
              resource: self.uniform_buffer.as_entire_binding(),
            },
          ],
        })
      } else {
        // Clone isn't available, so recreate the bind group
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
          label: Some("Game Texture Bind Group"),
          layout: &self.bind_group_layout,
          entries: &[
            wgpu::BindGroupEntry {
              binding: 0,
              resource: wgpu::BindingResource::TextureView(&self.texture_view),
            },
            wgpu::BindGroupEntry {
              binding: 1,
              resource: wgpu::BindingResource::Sampler(&self.sampler),
            },
            wgpu::BindGroupEntry {
              binding: 2,
              resource: self.uniform_buffer.as_entire_binding(),
            },
          ],
        })
      };

      let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Render Pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
          view: &view,
          resolve_target: None,
          ops: wgpu::Operations {
            // Clear to black (letterbox color)
            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            store: wgpu::StoreOp::Store,
          },
        })],
        depth_stencil_attachment: None,
        occlusion_query_set: None,
        timestamp_writes: None,
      });

      render_pass.set_pipeline(&self.render_pipeline);
      render_pass.set_bind_group(0, &final_bind_group, &[]);
      // Draw fullscreen quad (6 vertices, generated in shader)
      render_pass.draw(0..6, 0..1);
    }

    // Submit commands
    self.queue.submit(std::iter::once(encoder.finish()));
    output.present();

    Ok(())
  }

  /// Render with GPU sprites, lighting, UI overlay, and post-processing effects.
  ///
  /// This is the most comprehensive render method that supports the full pipeline:
  /// 1. CPU pixel buffer is uploaded to game texture
  /// 2. GPU sprite callback renders on top
  /// 3. Lighting callback processes the scene (render_lights + compose)
  /// 4. UI buffer is composited on top of lit scene (if provided)
  /// 5. Post-processing effects are applied
  /// 6. Final result is rendered to screen with letterboxing
  ///
  /// # Arguments
  /// - `effects`: Optional post-processing effects manager
  /// - `gpu_sprite_callback`: Optional callback for rendering GPU sprites
  /// - `lighting_callback`: Optional callback for lighting. Receives (device, queue, encoder,
  ///   scene_view, output_view). Should render lights and compose to output_view.
  /// - `ui_buffer`: Optional UI buffer to composite after lighting. Should be RGBA, same dimensions
  ///   as game resolution. Non-zero alpha pixels are blended on top of the lit scene.
  ///
  /// # Lighting Flow
  /// When a lighting callback is provided:
  /// - scene_view: The game texture (unlit scene after CPU + GPU sprites)
  /// - output_view: Texture A from post-process pipeline (write lit result here)
  /// - After callback, texture A becomes the source for effects/final render
  /// - If ui_buffer is provided, it's alpha-blended onto the lit scene
  pub fn render_with_lighting<F, L>(
    &mut self,
    effects: Option<&EffectsManager>,
    gpu_sprite_callback: Option<F>,
    lighting_callback: Option<L>,
    ui_buffer: Option<&[u8]>,
  ) -> Result<(), wgpu::SurfaceError>
  where
    F: FnOnce(&wgpu::Queue, &mut wgpu::CommandEncoder, &wgpu::TextureView),
    L: FnOnce(&wgpu::Device, &wgpu::Queue, &mut wgpu::CommandEncoder, &[u8], &wgpu::TextureView),
  {
    // Step 1: Upload CPU pixel buffer to staging texture (not game texture)
    self.queue.write_texture(
      wgpu::ImageCopyTexture {
        texture: &self.cpu_texture,
        mip_level: 0,
        origin: wgpu::Origin3d::ZERO,
        aspect: wgpu::TextureAspect::All,
      },
      &self.pixel_buffer,
      wgpu::ImageDataLayout {
        offset: 0,
        bytes_per_row: Some(4 * self.game_width),
        rows_per_image: Some(self.game_height),
      },
      wgpu::Extent3d {
        width: self.game_width,
        height: self.game_height,
        depth_or_array_layers: 1,
      },
    );

    // Get surface texture
    let output = self.surface.get_current_texture()?;
    let view = output
      .texture
      .create_view(&wgpu::TextureViewDescriptor::default());

    // Create command encoder
    let mut encoder = self
      .device
      .create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("Render Encoder"),
      });

    // Step 2: Clear game texture (required to prevent ghosting from previous frame)
    {
      let _clear_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Clear Game Texture Pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
          view: &self.texture_view,
          resolve_target: None,
          ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color {
              r: 0.0,
              g: 0.0,
              b: 0.0,
              a: 1.0, // Opaque black as default background
            }),
            store: wgpu::StoreOp::Store,
          },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
      });
    }

    // Step 3: Render GPU sprites (backgrounds, parallax) if callback provided
    if let Some(callback) = gpu_sprite_callback {
      callback(&self.queue, &mut encoder, &self.texture_view);
    }

    // Step 4: Blend CPU content on top of GPU sprites
    {
      let mut blend_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("CPU Content Blend Pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
          view: &self.texture_view,
          resolve_target: None,
          ops: wgpu::Operations {
            load: wgpu::LoadOp::Load, // Preserve GPU sprite content
            store: wgpu::StoreOp::Store,
          },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
      });

      // Use game blend pipeline (Rgba8UnormSrgb format to match game texture)
      blend_pass.set_pipeline(&self.game_blend_pipeline);
      blend_pass.set_bind_group(0, &self.cpu_bind_group, &[]);
      blend_pass.draw(0..6, 0..1);
    }

    // Track whether lighting was applied (changes the source texture)
    // Create temporary texture for lit output (like the original working implementation)
    let (lighting_applied, lit_texture_view) = if let Some(callback) = lighting_callback {
      // Create a temporary texture for the lit result
      let lit_texture = self.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Lit Scene Texture"),
        size: wgpu::Extent3d {
          width: self.game_width,
          height: self.game_height,
          depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
      });
      let lit_view = lit_texture.create_view(&wgpu::TextureViewDescriptor::default());

      // Lighting callback receives pixel data directly, writes lit scene to temp texture
      callback(
        &self.device,
        &self.queue,
        &mut encoder,
        &self.pixel_buffer,
        &lit_view,
      );
      (true, Some(lit_view))
    } else {
      (false, None)
    };

    // Check if we have active effects to apply
    let has_effects = effects.map(|e| e.has_active_effects()).unwrap_or(false);

    // Apply post-processing if we have effects
    let use_post_process = if has_effects && !lighting_applied {
      // Normal path: copy game texture to texture_a, then apply effects
      if let (Some(effects_mgr), Some(post_process)) = (effects, self.post_process.as_mut()) {
        let active: Vec<_> = effects_mgr
          .active_effects()
          .map(|e| (e, effects_mgr.time()))
          .collect();

        if !active.is_empty() {
          // Copy game texture to post-process texture A first
          encoder.copy_texture_to_texture(
            wgpu::ImageCopyTexture {
              texture: &self.texture,
              mip_level: 0,
              origin: wgpu::Origin3d::ZERO,
              aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyTexture {
              texture: post_process.texture_a(),
              mip_level: 0,
              origin: wgpu::Origin3d::ZERO,
              aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
              width: self.game_width,
              height: self.game_height,
              depth_or_array_layers: 1,
            },
          );

          // Apply effects chain
          let output_buffer =
            post_process.apply_effects(&self.device, &self.queue, &mut encoder, &active);

          Some(output_buffer)
        } else {
          None
        }
      } else {
        None
      }
    } else if has_effects && lighting_applied {
      // Lighting already wrote to texture_a, apply effects starting from there
      if let (Some(effects_mgr), Some(post_process)) = (effects, self.post_process.as_mut()) {
        let active: Vec<_> = effects_mgr
          .active_effects()
          .map(|e| (e, effects_mgr.time()))
          .collect();

        if !active.is_empty() {
          // Effects start from texture_a (already has lit scene)
          let output_buffer =
            post_process.apply_effects(&self.device, &self.queue, &mut encoder, &active);

          Some(output_buffer)
        } else {
          None
        }
      } else {
        None
      }
    } else {
      None
    };

    // Check if we have UI to overlay (non-zero ui_buffer)
    let has_ui_overlay = ui_buffer
      .as_ref()
      .map(|buf| buf.iter().any(|&b| b != 0))
      .unwrap_or(false);

    // If we have UI overlay AND lighting was applied, upload UI to game_texture
    // (We can reuse game_texture because the scene is now in texture_a from compose)
    // If lighting was NOT applied, we can't overwrite game_texture - scene is still there!
    if has_ui_overlay
      && lighting_applied
      && let Some(ui_data) = ui_buffer
    {
      self.queue.write_texture(
        wgpu::ImageCopyTexture {
          texture: &self.texture,
          mip_level: 0,
          origin: wgpu::Origin3d::ZERO,
          aspect: wgpu::TextureAspect::All,
        },
        ui_data,
        wgpu::ImageDataLayout {
          offset: 0,
          bytes_per_row: Some(4 * self.game_width),
          rows_per_image: Some(self.game_height),
        },
        wgpu::Extent3d {
          width: self.game_width,
          height: self.game_height,
          depth_or_array_layers: 1,
        },
      );
    }

    // Final render pass to screen
    {
      // Create bind group for the appropriate source texture
      let final_bind_group = if let Some(output_buffer) = use_post_process {
        let post_process = self.post_process.as_ref().unwrap();
        let output_view = post_process.get_output_view(output_buffer);

        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
          label: Some("Post-Process Output Bind Group"),
          layout: &self.bind_group_layout,
          entries: &[
            wgpu::BindGroupEntry {
              binding: 0,
              resource: wgpu::BindingResource::TextureView(output_view),
            },
            wgpu::BindGroupEntry {
              binding: 1,
              resource: wgpu::BindingResource::Sampler(&self.sampler),
            },
            wgpu::BindGroupEntry {
              binding: 2,
              resource: self.uniform_buffer.as_entire_binding(),
            },
          ],
        })
      } else if lighting_applied {
        // Use lit scene from temporary texture
        let lit_view = lit_texture_view.as_ref().unwrap();
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
          label: Some("Lit Scene Bind Group"),
          layout: &self.bind_group_layout,
          entries: &[
            wgpu::BindGroupEntry {
              binding: 0,
              resource: wgpu::BindingResource::TextureView(lit_view),
            },
            wgpu::BindGroupEntry {
              binding: 1,
              resource: wgpu::BindingResource::Sampler(&self.sampler),
            },
            wgpu::BindGroupEntry {
              binding: 2,
              resource: self.uniform_buffer.as_entire_binding(),
            },
          ],
        })
      } else {
        // Use original game texture
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
          label: Some("Game Texture Bind Group"),
          layout: &self.bind_group_layout,
          entries: &[
            wgpu::BindGroupEntry {
              binding: 0,
              resource: wgpu::BindingResource::TextureView(&self.texture_view),
            },
            wgpu::BindGroupEntry {
              binding: 1,
              resource: wgpu::BindingResource::Sampler(&self.sampler),
            },
            wgpu::BindGroupEntry {
              binding: 2,
              resource: self.uniform_buffer.as_entire_binding(),
            },
          ],
        })
      };

      let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Render Pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
          view: &view,
          resolve_target: None,
          ops: wgpu::Operations {
            // Clear to black (letterbox color)
            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            store: wgpu::StoreOp::Store,
          },
        })],
        depth_stencil_attachment: None,
        occlusion_query_set: None,
        timestamp_writes: None,
      });

      // Draw the scene
      render_pass.set_pipeline(&self.render_pipeline);
      render_pass.set_bind_group(0, &final_bind_group, &[]);
      render_pass.draw(0..6, 0..1);

      // If we have UI overlay, draw it on top with alpha blending
      if has_ui_overlay {
        // Create bind group for UI texture (game_texture now holds UI data)
        let ui_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
          label: Some("UI Overlay Bind Group"),
          layout: &self.bind_group_layout,
          entries: &[
            wgpu::BindGroupEntry {
              binding: 0,
              resource: wgpu::BindingResource::TextureView(&self.texture_view),
            },
            wgpu::BindGroupEntry {
              binding: 1,
              resource: wgpu::BindingResource::Sampler(&self.sampler),
            },
            wgpu::BindGroupEntry {
              binding: 2,
              resource: self.uniform_buffer.as_entire_binding(),
            },
          ],
        });

        render_pass.set_pipeline(&self.blend_pipeline);
        render_pass.set_bind_group(0, &ui_bind_group, &[]);
        render_pass.draw(0..6, 0..1);
      }
    }

    // Submit commands
    self.queue.submit(std::iter::once(encoder.finish()));
    output.present();

    Ok(())
  }

  /// Set the window mode.
  pub fn set_window_mode(&mut self, mode: WindowMode) {
    self.window_mode = mode;
    self.update_viewport_transform();
  }

  /// Set max width constraint.
  pub fn set_max_width(&mut self, max: Option<u32>) {
    self.max_width = max;
    self.update_viewport_transform();
  }

  /// Set max height constraint.
  pub fn set_max_height(&mut self, max: Option<u32>) {
    self.max_height = max;
    self.update_viewport_transform();
  }
}

/// Calculate viewport scale for a given configuration.
///
/// This is a pure function used for testing viewport math without GPU context.
/// Returns (scale_x, scale_y, offset_x, offset_y) for the fullscreen quad shader.
#[allow(dead_code)]
fn calculate_viewport_scale(
  game_width: u32,
  game_height: u32,
  window_width: u32,
  window_height: u32,
  window_mode: WindowMode,
  max_width: Option<u32>,
  max_height: Option<u32>,
) -> (f32, f32, f32, f32) {
  let game_w = game_width as f32;
  let game_h = game_height as f32;
  let window_w = window_width as f32;
  let window_h = window_height as f32;

  match window_mode {
    WindowMode::Fixed | WindowMode::ResizableAspectRatio => {
      let window_aspect = window_w / window_h;
      let game_aspect = game_w / game_h;

      let mut scale = if window_aspect > game_aspect {
        window_h / game_h
      } else {
        window_w / game_w
      };

      if let Some(max_w) = max_width {
        let max_scale_w = max_w as f32 / game_w;
        scale = scale.min(max_scale_w);
      }
      if let Some(max_h) = max_height {
        let max_scale_h = max_h as f32 / game_h;
        scale = scale.min(max_scale_h);
      }

      let scaled_w = game_w * scale;
      let scaled_h = game_h * scale;

      (scaled_w / window_w, scaled_h / window_h, 0.0, 0.0)
    }
    WindowMode::ResizableIntegerScale => {
      let window_aspect = window_w / window_h;
      let game_aspect = game_w / game_h;

      let mut scale = if window_aspect > game_aspect {
        window_h / game_h
      } else {
        window_w / game_w
      };

      scale = scale.floor().max(1.0);

      if let Some(max_w) = max_width {
        let max_scale_w = max_w as f32 / game_w;
        scale = scale.min(max_scale_w.floor().max(1.0));
      }
      if let Some(max_h) = max_height {
        let max_scale_h = max_h as f32 / game_h;
        scale = scale.min(max_scale_h.floor().max(1.0));
      }

      let scaled_w = game_w * scale;
      let scaled_h = game_h * scale;

      (scaled_w / window_w, scaled_h / window_h, 0.0, 0.0)
    }
    WindowMode::ResizableStretch => (1.0, 1.0, 0.0, 0.0),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn viewport_uniform_is_pod() {
    // ViewportUniform must be Pod for bytemuck::cast_slice
    let uniform = ViewportUniform {
      transform: [1.0, 1.0, 0.0, 0.0],
    };
    let bytes = bytemuck::bytes_of(&uniform);
    assert_eq!(bytes.len(), 16); // 4 floats * 4 bytes
  }

  #[test]
  fn stretch_mode_fills_window() {
    let (scale_x, scale_y, _, _) = calculate_viewport_scale(
      800,
      600,
      1920,
      1080,
      WindowMode::ResizableStretch,
      None,
      None,
    );
    assert_eq!(scale_x, 1.0);
    assert_eq!(scale_y, 1.0);
  }

  #[test]
  fn aspect_ratio_letterbox_wider_window() {
    // 800x600 game (4:3) in 1920x1080 window (16:9) = pillarboxing
    let (scale_x, scale_y, _, _) = calculate_viewport_scale(
      800,
      600,
      1920,
      1080,
      WindowMode::ResizableAspectRatio,
      None,
      None,
    );
    // Window is wider than game, so height is limiting factor
    // scale = 1080 / 600 = 1.8, scaled = 1440x1080
    // scale_x = 1440/1920 = 0.75, scale_y = 1080/1080 = 1.0
    assert!((scale_x - 0.75).abs() < 0.001);
    assert!((scale_y - 1.0).abs() < 0.001);
  }

  #[test]
  fn aspect_ratio_letterbox_taller_window() {
    // 800x600 game (4:3) in 800x1000 window = letterboxing (black bars top/bottom)
    let (scale_x, scale_y, _, _) = calculate_viewport_scale(
      800,
      600,
      800,
      1000,
      WindowMode::ResizableAspectRatio,
      None,
      None,
    );
    // Window is taller than game, so width is limiting factor
    // scale = 800 / 800 = 1.0, scaled = 800x600
    // scale_x = 800/800 = 1.0, scale_y = 600/1000 = 0.6
    assert!((scale_x - 1.0).abs() < 0.001);
    assert!((scale_y - 0.6).abs() < 0.001);
  }

  #[test]
  fn aspect_ratio_exact_fit() {
    // Same aspect ratio, no letterboxing needed
    let (scale_x, scale_y, _, _) = calculate_viewport_scale(
      400,
      300,
      800,
      600,
      WindowMode::ResizableAspectRatio,
      None,
      None,
    );
    assert!((scale_x - 1.0).abs() < 0.001);
    assert!((scale_y - 1.0).abs() < 0.001);
  }

  #[test]
  fn integer_scale_rounds_down() {
    // 320x240 game in 1000x800 window
    // Max scale that fits: 1000/320 = 3.125, 800/240 = 3.33
    // Integer: floor(3.125) = 3
    let (scale_x, scale_y, _, _) = calculate_viewport_scale(
      320,
      240,
      1000,
      800,
      WindowMode::ResizableIntegerScale,
      None,
      None,
    );
    // scaled = 960x720, scale_x = 960/1000 = 0.96, scale_y = 720/800 = 0.9
    assert!((scale_x - 0.96).abs() < 0.001);
    assert!((scale_y - 0.9).abs() < 0.001);
  }

  #[test]
  fn integer_scale_minimum_is_one() {
    // Small window, should still be at least 1x scale
    let (scale_x, scale_y, _, _) = calculate_viewport_scale(
      800,
      600,
      400,
      300,
      WindowMode::ResizableIntegerScale,
      None,
      None,
    );
    // scale = floor(400/800) = 0 -> clamped to 1
    // scaled = 800x600, scale_x = 800/400 = 2.0, scale_y = 600/300 = 2.0
    assert!((scale_x - 2.0).abs() < 0.001);
    assert!((scale_y - 2.0).abs() < 0.001);
  }

  #[test]
  fn max_width_constrains_scale() {
    // 400x300 game in 1600x1200 window, max_width = 800
    // Without constraint: scale = 4x, scaled = 1600x1200
    // With max_width 800: scale = 800/400 = 2x, scaled = 800x600
    let (scale_x, scale_y, _, _) = calculate_viewport_scale(
      400,
      300,
      1600,
      1200,
      WindowMode::ResizableAspectRatio,
      Some(800),
      None,
    );
    // scaled = 800x600, scale_x = 800/1600 = 0.5, scale_y = 600/1200 = 0.5
    assert!((scale_x - 0.5).abs() < 0.001);
    assert!((scale_y - 0.5).abs() < 0.001);
  }

  #[test]
  fn max_height_constrains_scale() {
    // 400x300 game in 1600x1200 window, max_height = 600
    // Without constraint: scale = 4x
    // With max_height 600: scale = 600/300 = 2x, scaled = 800x600
    let (scale_x, scale_y, _, _) = calculate_viewport_scale(
      400,
      300,
      1600,
      1200,
      WindowMode::ResizableAspectRatio,
      None,
      Some(600),
    );
    // scaled = 800x600, scale_x = 800/1600 = 0.5, scale_y = 600/1200 = 0.5
    assert!((scale_x - 0.5).abs() < 0.001);
    assert!((scale_y - 0.5).abs() < 0.001);
  }

  #[test]
  fn fixed_mode_same_as_aspect_ratio() {
    let aspect = calculate_viewport_scale(
      800,
      600,
      1920,
      1080,
      WindowMode::ResizableAspectRatio,
      None,
      None,
    );
    let fixed = calculate_viewport_scale(800, 600, 1920, 1080, WindowMode::Fixed, None, None);
    assert_eq!(aspect, fixed);
  }
}
