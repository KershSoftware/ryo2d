//! WGPU renderer for 2D games.
//!
//! This module provides a renderer that:
//! 1. Runs the render pipeline: the world is drawn on the GPU into the game texture
//! 2. Applies lighting and post-processing effects (optional)
//! 3. Composites the CPU-drawn UI layer last
//! 4. Presents the result with proper letterboxing/scaling

use std::path::PathBuf;
use std::sync::Arc;

use image::RgbaImage;

/// Default background color for the game framebuffer.
/// This color matches the UI demo background rgb(25, 25, 35) aka #191923.
/// Values are LINEAR (not sRGB) because wgpu applies sRGB encoding to the texture.
/// sRGB to linear conversion: ((srgb + 0.055) / 1.055)^2.4
pub const DEFAULT_BACKGROUND_COLOR: wgpu::Color = wgpu::Color {
  r: 0.0296, // sRGB 25 -> linear
  g: 0.0296, // sRGB 25 -> linear
  b: 0.0473, // sRGB 35 -> linear
  a: 1.0,
};
use winit::window::Window;

use crate::core::capture::{CapturedFrame, read_texture_rgba};
use crate::core::error::{CaptureError, CaptureResult};
use crate::core::render::{
  DrawList, FrameInfo, FrameScene, FrameTargets, RenderContext, RenderPipeline, TRANSITION_VISIBLE,
};
use crate::core::shaders::{EffectsManager, PostProcessPipeline};
use crate::core::window::WindowMode;

use super::{Compositor, SpriteGpuRenderer};

/// Where finished frames go: the window surface or an offscreen texture.
enum PresentTarget {
  Surface {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
  },
  Offscreen {
    texture: wgpu::Texture,
  },
}

/// The texture an offscreen renderer presents into, read back by `read_frame`
fn offscreen_target(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
  device.create_texture(&wgpu::TextureDescriptor {
    label: Some("Offscreen Target"),
    size: wgpu::Extent3d {
      width,
      height,
      depth_or_array_layers: 1,
    },
    mip_level_count: 1,
    sample_count: 1,
    dimension: wgpu::TextureDimension::D2,
    format: wgpu::TextureFormat::Rgba8UnormSrgb,
    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    view_formats: &[],
  })
}

impl PresentTarget {
  /// The view to draw this frame into, plus the surface texture to present afterwards.
  fn acquire(
    &self,
  ) -> Result<(Option<wgpu::SurfaceTexture>, wgpu::TextureView), wgpu::SurfaceError> {
    match self {
      PresentTarget::Surface { surface, .. } => {
        let output = surface.get_current_texture()?;
        let view = output
          .texture
          .create_view(&wgpu::TextureViewDescriptor::default());
        Ok((Some(output), view))
      }
      PresentTarget::Offscreen { texture } => Ok((
        None,
        texture.create_view(&wgpu::TextureViewDescriptor::default()),
      )),
    }
  }

  fn format(&self) -> wgpu::TextureFormat {
    match self {
      PresentTarget::Surface { config, .. } => config.format,
      PresentTarget::Offscreen { texture } => texture.format(),
    }
  }
}

/// Construction inputs that differ between window and offscreen renderers.
struct RendererParts {
  instance: Option<wgpu::Instance>,
  adapter: Option<wgpu::Adapter>,
  device: Arc<wgpu::Device>,
  queue: Arc<wgpu::Queue>,
  target: PresentTarget,
  window_size: (u32, u32),
  presentation_blocks: bool,
  game_width: u32,
  game_height: u32,
  window_mode: WindowMode,
  max_width: Option<u32>,
  max_height: Option<u32>,
}

/// Handles wgpu rendering of the game texture to screen.
pub struct WgpuRenderer {
  // Only set for window renderers (dev windows create extra surfaces from them)
  instance: Option<wgpu::Instance>,
  adapter: Option<wgpu::Adapter>,
  target: PresentTarget,
  device: std::sync::Arc<wgpu::Device>,
  queue: std::sync::Arc<wgpu::Queue>,
  // The game texture the world passes draw into
  texture: wgpu::Texture,
  texture_view: wgpu::TextureView,
  // Final composite (scene + UI overlay to the target) and its shared resources
  compositor: Compositor,
  // Lighting output, kept between frames and recreated with the game texture
  lit_texture: wgpu::Texture,
  lit_texture_view: wgpu::TextureView,
  bind_group: wgpu::BindGroup,
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
  // Post-processing pipeline
  post_process: Option<PostProcessPipeline>,
  // Whether presentation blocks until vsync (Fifo mode)
  // When true, manual frame limiting should be skipped to avoid double-waiting
  presentation_blocks: bool,
  // Offscreen target for frame capture, created on first capture
  capture_texture: Option<wgpu::Texture>,
  // Destination for the capture requested for the next presented frame
  pending_capture: Option<PathBuf>,
  // Result of the latest capture, collected by the engine
  capture_result: Option<CaptureResult<CapturedFrame>>,
}

/// Create the persistent lighting output texture at game resolution.
fn create_lit_texture(
  device: &wgpu::Device,
  width: u32,
  height: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
  let texture = device.create_texture(&wgpu::TextureDescriptor {
    label: Some("Lit Scene Texture"),
    size: wgpu::Extent3d {
      width,
      height,
      depth_or_array_layers: 1,
    },
    mip_level_count: 1,
    sample_count: 1,
    dimension: wgpu::TextureDimension::D2,
    format: wgpu::TextureFormat::Rgba8UnormSrgb,
    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
      | wgpu::TextureUsages::TEXTURE_BINDING
      | wgpu::TextureUsages::COPY_SRC, // Allow copying to post-process texture
    view_formats: &[],
  });
  let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
  (texture, view)
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
  /// - `vsync`: `Some(true)` or `None` = Fifo (waits for the display); `Some(false)` = Mailbox, else Immediate
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

    // Request device and queue (wrapped in Arc for sharing with profiler window)
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
    let device = std::sync::Arc::new(device);
    let queue = std::sync::Arc::new(queue);

    // Configure surface
    let surface_caps = surface.get_capabilities(&adapter);
    let surface_format = surface_caps
      .formats
      .iter()
      .find(|f| f.is_srgb())
      .copied()
      .unwrap_or(surface_caps.formats[0]);

    // VSync on: Fifo, which waits for the display (every surface supports
    // it). Off: Mailbox, else Immediate, paced to target_fps by the engine
    let supports = |mode| surface_caps.present_modes.contains(&mode);
    let present_mode = if vsync.unwrap_or(true) {
      wgpu::PresentMode::Fifo
    } else if supports(wgpu::PresentMode::Mailbox) {
      wgpu::PresentMode::Mailbox
    } else if supports(wgpu::PresentMode::Immediate) {
      wgpu::PresentMode::Immediate
    } else {
      wgpu::PresentMode::Fifo
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
    Self::from_parts(RendererParts {
      instance: Some(instance),
      adapter: Some(adapter),
      device,
      queue,
      target: PresentTarget::Surface { surface, config },
      window_size: (size.width, size.height),
      presentation_blocks: present_mode == wgpu::PresentMode::Fifo,
      game_width,
      game_height,
      window_mode,
      max_width,
      max_height,
    })
  }

  /// Create a renderer that draws into an offscreen texture instead of a window.
  ///
  /// The target is `width × height` (the game resolution), so frames are never
  /// letterboxed. Read them back with [`read_frame`](Self::read_frame).
  pub fn new_offscreen(
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    width: u32,
    height: u32,
  ) -> Self {
    let texture = offscreen_target(&device, width, height);
    Self::from_parts(RendererParts {
      instance: None,
      adapter: None,
      device,
      queue,
      target: PresentTarget::Offscreen { texture },
      window_size: (width, height),
      presentation_blocks: false,
      game_width: width,
      game_height: height,
      window_mode: WindowMode::default(),
      max_width: None,
      max_height: None,
    })
  }

  /// Read the last frame drawn to an offscreen target.
  ///
  /// Window renderers can't read their surface back; use
  /// [`request_capture`](Self::request_capture) instead.
  pub fn read_frame(&self) -> CaptureResult<RgbaImage> {
    match &self.target {
      PresentTarget::Offscreen { texture } => read_texture_rgba(&self.device, &self.queue, texture),
      PresentTarget::Surface { .. } => Err(CaptureError::Readback(
        "window surfaces can't be read back; use request_capture".to_string(),
      )),
    }
  }

  /// Build the textures, pipelines and buffers shared by every target type.
  fn from_parts(parts: RendererParts) -> Self {
    let RendererParts {
      instance,
      adapter,
      device,
      queue,
      target,
      window_size,
      presentation_blocks,
      game_width,
      game_height,
      window_mode,
      max_width,
      max_height,
    } = parts;
    let target_format = target.format();

    // Create the game texture (where the world is drawn)
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

    let compositor = Compositor::new(&device, target_format, game_width, game_height);
    let (lit_texture, lit_texture_view) = create_lit_texture(&device, game_width, game_height);

    // Create bind group for main game texture
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
      label: Some("Texture Bind Group"),
      layout: &compositor.bind_group_layout,
      entries: &[
        wgpu::BindGroupEntry {
          binding: 0,
          resource: wgpu::BindingResource::TextureView(&texture_view),
        },
        wgpu::BindGroupEntry {
          binding: 1,
          resource: wgpu::BindingResource::Sampler(&compositor.sampler),
        },
        wgpu::BindGroupEntry {
          binding: 2,
          resource: compositor.uniform_buffer.as_entire_binding(),
        },
      ],
    });

    // Fill the game texture with a dark charcoal background, which is presented
    // immediately to prevent a flash on startup
    let mut pixel_buffer = vec![0u8; (game_width * game_height * 4) as usize];
    for pixel in pixel_buffer.as_chunks_mut::<4>().0 {
      pixel[0] = 0x2d; // R: 45
      pixel[1] = 0x2d; // G: 45
      pixel[2] = 0x2d; // B: 45
      pixel[3] = 0xff; // A: 255
    }

    // Upload the initial colour to the game texture immediately
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

    let mut renderer = WgpuRenderer {
      instance,
      adapter,
      device,
      queue,
      target,
      texture,
      texture_view,
      compositor,
      lit_texture,
      lit_texture_view,
      bind_group,
      game_width,
      game_height,
      window_width: window_size.0,
      window_height: window_size.1,
      window_mode,
      max_width,
      max_height,
      post_process: Some(post_process),
      presentation_blocks,
      capture_texture: None,
      pending_capture: None,
      capture_result: None,
    };

    // Initialize viewport transform
    renderer.update_viewport_transform();

    renderer
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

  /// Get a shared reference to the wgpu device.
  ///
  /// Used for creating additional rendering contexts (e.g., profiler window).
  pub fn device_arc(&self) -> std::sync::Arc<wgpu::Device> {
    std::sync::Arc::clone(&self.device)
  }

  /// Get a reference to the wgpu queue.
  ///
  /// Used by GPU sprite renderer for uploading data.
  pub fn queue(&self) -> &wgpu::Queue {
    &self.queue
  }

  /// Get a shared reference to the wgpu queue.
  ///
  /// Used for creating additional rendering contexts (e.g., profiler window).
  pub fn queue_arc(&self) -> std::sync::Arc<wgpu::Queue> {
    std::sync::Arc::clone(&self.queue)
  }

  /// Get a reference to the wgpu adapter (window renderers only).
  ///
  /// Used for creating additional surfaces (e.g., profiler window).
  pub fn adapter(&self) -> Option<&wgpu::Adapter> {
    self.adapter.as_ref()
  }

  /// Get a reference to the wgpu instance (window renderers only).
  ///
  /// Used for creating additional surfaces (e.g., profiler window).
  pub fn instance(&self) -> Option<&wgpu::Instance> {
    self.instance.as_ref()
  }

  /// Get the main game texture view.
  ///
  /// GPU sprite rendering draws directly to this texture.
  pub fn game_texture_view(&self) -> &wgpu::TextureView {
    &self.texture_view
  }

  /// Get the format frames are presented in (surface or offscreen texture).
  pub fn surface_format(&self) -> wgpu::TextureFormat {
    self.target.format()
  }

  /// Get the format of the game texture (where the world renders to)
  pub fn game_texture_format(&self) -> wgpu::TextureFormat {
    wgpu::TextureFormat::Rgba8UnormSrgb
  }

  /// Resize the surface when window size changes (no-op for offscreen targets).
  pub fn resize(&mut self, new_width: u32, new_height: u32) {
    if new_width == 0 || new_height == 0 {
      return;
    }
    match &mut self.target {
      PresentTarget::Surface { surface, config } => {
        config.width = new_width;
        config.height = new_height;
        surface.configure(&self.device, config);
      }
      // An offscreen target stands in for the window
      PresentTarget::Offscreen { texture } => {
        *texture = offscreen_target(&self.device, new_width, new_height);
      }
    }
    self.window_width = new_width;
    self.window_height = new_height;
    self.update_viewport_transform();
  }

  /// Resize the internal game textures (for stretch mode).
  ///
  /// A zero width or height (a minimised window) is ignored: the buffer keeps
  /// its size, since zero-size textures are invalid.
  pub fn resize_buffer(&mut self, new_width: u32, new_height: u32) {
    if new_width == 0 || new_height == 0 {
      return;
    }
    self.game_width = new_width;
    self.game_height = new_height;

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
    self.bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
      label: Some("Texture Bind Group"),
      layout: &self.compositor.bind_group_layout,
      entries: &[
        wgpu::BindGroupEntry {
          binding: 0,
          resource: wgpu::BindingResource::TextureView(&self.texture_view),
        },
        wgpu::BindGroupEntry {
          binding: 1,
          resource: wgpu::BindingResource::Sampler(&self.compositor.sampler),
        },
        wgpu::BindGroupEntry {
          binding: 2,
          resource: self.compositor.uniform_buffer.as_entire_binding(),
        },
      ],
    });

    (self.lit_texture, self.lit_texture_view) =
      create_lit_texture(&self.device, new_width, new_height);
    self
      .compositor
      .resize_ui(&self.device, new_width, new_height);
    if let Some(post_process) = &mut self.post_process {
      post_process.resize(&self.device, new_width, new_height);
    }

    self.update_viewport_transform();

    // Recreated at the new size on the next capture
    self.capture_texture = None;
  }

  /// Update the viewport uniform based on current window mode and sizes.
  fn update_viewport_transform(&mut self) {
    let (scale_x, scale_y, offset_x, offset_y) = self.calculate_viewport();

    self
      .compositor
      .set_viewport_transform(&self.queue, [scale_x, scale_y, offset_x, offset_y]);
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

  /// Render a frame by running `pipeline`'s passes on one encoder.
  pub(crate) fn render_frame(
    &mut self,
    pipeline: &mut RenderPipeline,
    scene: FrameScene<'_>,
  ) -> Result<(), wgpu::SurfaceError> {
    let (output, view) = self.target.acquire()?;
    let capture_path = self.prepare_capture();
    let frame = FrameInfo {
      game_width: self.game_width,
      game_height: self.game_height,
      world: !scene.draw_list.items().is_empty(),
      lighting: scene.lighting.is_some(),
      effects: self.post_process.is_some() && scene.effects.has_active_effects(),
      ui: scene.ui_buffer.iter().any(|&b| b != 0),
      transition: scene.overlay.1 > TRANSITION_VISIBLE,
      capture: capture_path.is_some(),
    };
    let mut encoder = self
      .device
      .create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("Frame Encoder"),
      });
    let captured;
    {
      let targets = FrameTargets {
        game: &self.texture,
        game_view: &self.texture_view,
        lit: &self.lit_texture,
        lit_view: &self.lit_texture_view,
        compositor: &self.compositor,
        output_view: &view,
        capture: capture_path.as_ref().and(self.capture_texture.as_ref()),
      };
      let mut ctx = RenderContext::new(
        &self.device,
        &self.queue,
        &mut encoder,
        frame,
        targets,
        scene,
        self.post_process.as_mut(),
      );
      pipeline.execute(&mut ctx);
      captured = ctx.captured;
    }
    self.queue.submit(std::iter::once(encoder.finish()));
    if let Some(output) = output {
      output.present();
    }
    // Only read back what the capture pass drew; otherwise the texture is stale or blank
    if captured {
      self.finish_capture(capture_path);
    } else if capture_path.is_some() {
      self.capture_result = Some(Err(CaptureError::Readback(
        "capture pass did not run".to_string(),
      )));
    }
    Ok(())
  }

  /// Render a frame with no sprites, trails, lights or effects, and the given UI.
  ///
  /// Used to put something on screen before the engine's renderers exist.
  pub(crate) fn render_blank_frame(
    &mut self,
    pipeline: &mut RenderPipeline,
    ui_buffer: &[u8],
  ) -> Result<(), wgpu::SurfaceError> {
    let sprites = SpriteGpuRenderer::new(&self.device, &self.queue, self.target.format());
    let effects = EffectsManager::new();
    let draw_list = DrawList::default();
    let scene = FrameScene {
      sprites: &sprites,
      draw_list: &draw_list,
      trails: None,
      lighting: None,
      effects: &effects,
      ui_buffer,
      camera_pos: glam::Vec2::ZERO,
      camera_zoom: 1.0,
      time: 0.0,
      overlay: ([0; 3], 0.0),
    };
    self.render_frame(pipeline, scene)
  }

  /// Capture the next presented frame to `path` at game resolution.
  ///
  /// Replaces any pending request. Collect the outcome with
  /// [`take_capture_result`](Self::take_capture_result) after rendering.
  pub fn request_capture(&mut self, path: PathBuf) {
    self.pending_capture = Some(path);
  }

  /// Take the result of the latest capture, if one ran since the last call.
  pub fn take_capture_result(&mut self) -> Option<CaptureResult<CapturedFrame>> {
    self.capture_result.take()
  }

  /// Take the pending request and make sure the capture texture matches the game resolution.
  fn prepare_capture(&mut self) -> Option<PathBuf> {
    let path = self.pending_capture.take()?;
    let up_to_date = self
      .capture_texture
      .as_ref()
      .is_some_and(|t| t.width() == self.game_width && t.height() == self.game_height);
    if !up_to_date {
      self.capture_texture = Some(self.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Frame Capture Texture"),
        size: wgpu::Extent3d {
          width: self.game_width,
          height: self.game_height,
          depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        // Must match the output target's format (window surface or offscreen
        // texture): the final-composite pipelines target it
        format: self.target.format(),
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
      }));
    }
    Some(path)
  }

  /// Read the capture texture back once the frame's commands were submitted.
  fn finish_capture(&mut self, path: Option<PathBuf>) {
    let Some(path) = path else {
      return;
    };
    let result = match &self.capture_texture {
      Some(texture) => read_texture_rgba(&self.device, &self.queue, texture)
        .map(|image| CapturedFrame { image, path }),
      None => Err(CaptureError::Readback(
        "capture texture missing".to_string(),
      )),
    };
    self.capture_result = Some(result);
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
#[cfg(test)]
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
  fn resize_buffer_resizes_every_game_resolution_texture() {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return; // no GPU on this machine
    };
    let mut renderer = WgpuRenderer::new_offscreen(device, queue, 64, 48);
    renderer.resize_buffer(96, 80);

    let size = |t: &wgpu::Texture| (t.width(), t.height());
    assert_eq!(size(&renderer.texture), (96, 80));
    assert_eq!(size(&renderer.lit_texture), (96, 80));
    assert_eq!(size(renderer.compositor.ui_texture()), (96, 80));
    let post = renderer.post_process.as_ref().expect("post process");
    assert_eq!(size(post.texture_a()), (96, 80));
    assert_eq!(size(post.texture_b()), (96, 80));

    // Resizing must keep the usages the textures were created with
    assert!(
      post
        .texture_a()
        .usage()
        .contains(wgpu::TextureUsages::COPY_SRC)
    );
    assert!(
      post
        .texture_b()
        .usage()
        .contains(wgpu::TextureUsages::COPY_SRC)
    );

    let ui = vec![255u8; 96 * 80 * 4];
    renderer
      .render_blank_frame(&mut RenderPipeline::default_2d(), &ui)
      .expect("render after resize");
  }

  #[test]
  fn resize_buffer_ignores_a_zero_size() {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return; // no GPU on this machine
    };
    let mut renderer = WgpuRenderer::new_offscreen(device, queue, 64, 48);
    renderer.resize_buffer(0, 0);
    renderer.resize_buffer(0, 10);
    renderer.resize_buffer(10, 0);

    assert_eq!((renderer.game_width, renderer.game_height), (64, 48));
    assert_eq!(
      (renderer.texture.width(), renderer.texture.height()),
      (64, 48)
    );
  }

  #[test]
  fn offscreen_frame_matches_ui_pixels() {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return; // no GPU on this machine
    };
    let (width, height) = (37, 9);
    let mut renderer = WgpuRenderer::new_offscreen(device, queue, width, height);

    // Opaque UI pattern: each pixel encodes its position
    let mut expected = vec![0u8; (width * height * 4) as usize];
    for (i, px) in expected.as_chunks_mut::<4>().0.iter_mut().enumerate() {
      let (x, y) = ((i as u32 % width) as u8, (i as u32 / width) as u8);
      px.copy_from_slice(&[x * 6, y * 20, 128, 255]);
    }

    renderer
      .render_blank_frame(&mut RenderPipeline::default_2d(), &expected)
      .expect("render");
    let frame = renderer.read_frame().expect("read frame");

    assert_eq!(frame.dimensions(), (width, height));
    for (got, want) in frame
      .as_raw()
      .as_chunks::<4>()
      .0
      .iter()
      .zip(expected.as_chunks::<4>().0)
    {
      assert!(
        got.iter().zip(want).all(|(g, w)| g.abs_diff(*w) <= 1),
        "got {got:?}, want {want:?}"
      );
    }
  }

  #[test]
  fn capture_without_the_capture_pass_reports_an_error() {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return; // no GPU on this machine
    };
    let mut renderer = WgpuRenderer::new_offscreen(device, queue, 32, 32);
    let mut pipeline = RenderPipeline::default_2d();
    pipeline
      .set_enabled(crate::core::render::pass_ids::CAPTURE, false)
      .expect("capture pass exists");

    renderer.request_capture(std::env::temp_dir().join("ryo2d-capture-disabled.png"));
    renderer
      .render_blank_frame(&mut pipeline, &[])
      .expect("render");

    let result = renderer.take_capture_result().expect("a capture result");
    assert!(
      matches!(result, Err(CaptureError::Readback(_))),
      "capture should report an error, got ok: {}",
      result.is_ok()
    );
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
