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
  DrawList, EffectStages, FrameInfo, FrameScene, FrameTargets, RenderContext, RenderPipeline,
  TRANSITION_VISIBLE,
};
use crate::core::shaders::{EffectStage, EffectsManager, PostProcessPipeline};

use super::camera_images::CameraImages;
use super::gpu_error::{GpuError, GpuFault, SurfaceAction, SurfaceState, surface_action};
use super::screen::{Presentation, ScreenMapping};
use super::{Compositor, SpriteGpuRenderer};

/// Where finished frames go: the window surface or an offscreen texture.
/// Why a frame was not presented.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FrameError {
  /// Nothing to draw into this frame (timeout, occluded window)
  Skip,
  /// The surface must be reconfigured before the next frame
  Reconfigure,
  /// The surface was lost and must be recreated
  Recreate,
  /// Rendering cannot continue
  Fatal(String),
}

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
fn offscreen_target(
  device: &wgpu::Device,
  width: u32,
  height: u32,
  format: wgpu::TextureFormat,
) -> wgpu::Texture {
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
    format,
    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    view_formats: &[],
  })
}

impl PresentTarget {
  /// The view to draw this frame into, plus the surface texture to present afterwards.
  /// The next frame's texture and view, and whether to reconfigure the
  /// surface after presenting it.
  fn acquire(&self) -> Result<(Option<wgpu::SurfaceTexture>, wgpu::TextureView, bool), FrameError> {
    match self {
      PresentTarget::Surface { surface, .. } => {
        let (state, output) = match surface.get_current_texture() {
          wgpu::CurrentSurfaceTexture::Success(output) => (SurfaceState::Success, Some(output)),
          wgpu::CurrentSurfaceTexture::Suboptimal(output) => {
            (SurfaceState::Suboptimal, Some(output))
          }
          wgpu::CurrentSurfaceTexture::Timeout => (SurfaceState::Timeout, None),
          wgpu::CurrentSurfaceTexture::Occluded => (SurfaceState::Occluded, None),
          wgpu::CurrentSurfaceTexture::Outdated => (SurfaceState::Outdated, None),
          wgpu::CurrentSurfaceTexture::Lost => (SurfaceState::Lost, None),
          wgpu::CurrentSurfaceTexture::Validation => (SurfaceState::Validation, None),
        };
        match (surface_action(state), output) {
          (SurfaceAction::Draw { then_reconfigure }, Some(output)) => {
            let view = output
              .texture
              .create_view(&wgpu::TextureViewDescriptor::default());
            Ok((Some(output), view, then_reconfigure))
          }
          (SurfaceAction::Draw { .. } | SurfaceAction::Skip, _) => Err(FrameError::Skip),
          (SurfaceAction::Reconfigure, _) => Err(FrameError::Reconfigure),
          (SurfaceAction::Recreate, _) => Err(FrameError::Recreate),
          (SurfaceAction::Fatal, _) => {
            Err(FrameError::Fatal("surface validation error".to_string()))
          }
        }
      }
      PresentTarget::Offscreen { texture } => Ok((
        None,
        texture.create_view(&wgpu::TextureViewDescriptor::default()),
        false,
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
  mapping: ScreenMapping,
  presentation_blocks: bool,
}

/// How the window renderer is set up.
#[derive(Debug, Clone, Copy)]
pub struct RendererSettings {
  pub presentation: Presentation,
  pub vsync: Option<bool>,
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
  // The window cameras' own images, placed in the scene image each frame
  camera_images: CameraImages,
  // Lighting output, kept between frames and recreated with the game texture
  lit_texture: wgpu::Texture,
  lit_texture_view: wgpu::TextureView,
  bind_group: wgpu::BindGroup,
  // Screen space and the window: the scene renders at `render_size()`
  mapping: ScreenMapping,
  // The display effect stage; its buffer A is the display image
  display_effects: PostProcessPipeline,
  // The HDR effect stage, in the scene format
  hdr_effects: PostProcessPipeline,
  // Whether presentation blocks until vsync (Fifo mode)
  // When true, manual frame limiting should be skipped to avoid double-waiting
  presentation_blocks: bool,
  // Offscreen target for frame capture, created on first capture
  capture_texture: Option<wgpu::Texture>,
  // Composite every frame into `capture_texture` for `read_scene` (headless)
  keep_scene: bool,
  // Whether `capture_texture` holds the last frame's scene
  scene_current: bool,
  // Destination for the capture requested for the next presented frame
  pending_capture: Option<PathBuf>,
  // Result of the latest capture, collected by the engine
  capture_result: Option<CaptureResult<CapturedFrame>>,
  // Errors reported by the device's callbacks, picked up by the next frame
  fault: GpuFault,
  // The surface was suboptimal: reconfigure it after this frame
  needs_reconfigure: bool,
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
    format: crate::core::render::SCENE_FORMAT,
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
  /// - `presentation`: how the world reaches the window
  /// - `vsync`: `Some(true)` or `None` = Fifo (waits for the display); `Some(false)` = Mailbox, else Immediate
  pub async fn new(window: Arc<Window>, settings: RendererSettings) -> Result<Self, GpuError> {
    let RendererSettings {
      presentation,
      vsync,
    } = settings;
    let size = window.inner_size();
    let mapping = ScreenMapping::new(
      presentation,
      (size.width, size.height),
      window.scale_factor(),
    );

    // Create wgpu instance
    let mut descriptor =
      wgpu::InstanceDescriptor::new_with_display_handle(Box::new(Arc::clone(&window)));
    descriptor.backends = wgpu::Backends::all();
    let instance = wgpu::Instance::new(descriptor);

    let surface = instance
      .create_surface(window)
      .map_err(|e| GpuError::CreateSurface(e.to_string()))?;

    // Request adapter
    let adapter = instance
      .request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        compatible_surface: Some(&surface),
        force_fallback_adapter: false,
        apply_limit_buckets: false,
      })
      .await
      .map_err(|e| GpuError::NoAdapter(e.to_string()))?;

    // Request device and queue (wrapped in Arc for sharing with profiler window)
    let (device, queue) = adapter
      .request_device(&wgpu::DeviceDescriptor {
        label: Some("Ryo2D Device"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        memory_hints: Default::default(),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        trace: wgpu::Trace::Off,
      })
      .await
      .map_err(|e| GpuError::RequestDevice(e.to_string()))?;
    // Errors no scope catches, and a lost device, end the run at the next frame
    let fault = GpuFault::default();
    let uncaptured = fault.clone();
    device.on_uncaptured_error(Arc::new(move |error: wgpu::Error| {
      uncaptured.record(error.to_string());
    }));
    let lost = fault.clone();
    device.set_device_lost_callback(move |_reason, message| {
      lost.record(GpuError::DeviceLost(message).to_string());
    });
    let device = std::sync::Arc::new(device);
    let queue = std::sync::Arc::new(queue);

    // Configure surface
    let surface_caps = surface.get_capabilities(&adapter);
    let surface_format = surface_caps
      .formats
      .iter()
      .find(|f| f.is_srgb())
      .copied()
      .or_else(|| surface_caps.formats.first().copied())
      .ok_or_else(|| GpuError::CreateSurface("the surface supports no format".to_string()))?;

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
      alpha_mode: surface_caps
        .alpha_modes
        .first()
        .copied()
        .unwrap_or(wgpu::CompositeAlphaMode::Auto),
      view_formats: vec![],
      desired_maximum_frame_latency: 2,
      color_space: wgpu::SurfaceColorSpace::Auto,
    };
    surface.configure(&device, &config);
    let mut renderer = Self::from_parts(RendererParts {
      instance: Some(instance),
      adapter: Some(adapter),
      device,
      queue,
      target: PresentTarget::Surface { surface, config },
      mapping,
      presentation_blocks: present_mode == wgpu::PresentMode::Fifo,
    });
    renderer.fault = fault;
    Ok(renderer)
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
    Self::new_offscreen_with_format(
      device,
      queue,
      width,
      height,
      wgpu::TextureFormat::Rgba8UnormSrgb,
    )
  }

  /// [`new_offscreen`](Self::new_offscreen) with a chosen target format
  /// (tests of non-sRGB output).
  pub fn new_offscreen_with_format(
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
  ) -> Self {
    let mapping = ScreenMapping::new(Presentation::ResolutionIndependent, (width, height), 1.0);
    Self::new_offscreen_mapped(device, queue, mapping, format)
  }

  /// An offscreen renderer standing in for a window of `mapping`'s physical
  /// size.
  pub(crate) fn new_offscreen_mapped(
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    mapping: ScreenMapping,
    format: wgpu::TextureFormat,
  ) -> Self {
    let (width, height) = mapping.physical_size();
    let texture = offscreen_target(&device, width, height, format);
    Self::from_parts(RendererParts {
      instance: None,
      adapter: None,
      device,
      queue,
      target: PresentTarget::Offscreen { texture },
      mapping,
      presentation_blocks: false,
    })
  }

  /// Read the last frame drawn to an offscreen target: the window, bars
  /// included.
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
      mapping,
      presentation_blocks,
    } = parts;
    let target_format = target.format();
    let (game_width, game_height) = mapping.render_size();
    let (ui_width, ui_height) = mapping.ui_size();

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
      format: crate::core::render::SCENE_FORMAT,
      usage: wgpu::TextureUsages::TEXTURE_BINDING
        | wgpu::TextureUsages::COPY_DST
        | wgpu::TextureUsages::COPY_SRC // Allow copying from this texture for post-processing
        | wgpu::TextureUsages::RENDER_ATTACHMENT, // Allow GPU sprite rendering to this texture
      view_formats: &[],
    });

    let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    let compositor = Compositor::new(&device, target_format, ui_width, ui_height);
    let camera_images = CameraImages::new(&device, crate::core::render::SCENE_FORMAT);
    let (lit_texture, lit_texture_view) = create_lit_texture(&device, game_width, game_height);

    // Create bind group for main game texture
    let bind_group = crate::core::graphics::bind_groups::create(
      &device,
      &wgpu::BindGroupDescriptor {
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
      },
    );

    // Fill the game texture with a dark charcoal background, which is presented
    // immediately to prevent a flash on startup
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
      label: Some("Initial Game Texture Clear"),
    });
    {
      let _clear = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Initial Game Texture Clear"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
          view: &texture_view,
          depth_slice: None,
          resolve_target: None,
          ops: wgpu::Operations {
            // sRGB 45 grey in linear light
            load: wgpu::LoadOp::Clear(wgpu::Color {
              r: 0.0262,
              g: 0.0262,
              b: 0.0262,
              a: 1.0,
            }),
            store: wgpu::StoreOp::Store,
          },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
      });
    }
    queue.submit(std::iter::once(encoder.finish()));

    // Create the display effect stage
    let display_effects = PostProcessPipeline::new(
      &device,
      game_width,
      game_height,
      wgpu::TextureFormat::Rgba8UnormSrgb,
    );

    let hdr_effects = PostProcessPipeline::new(
      &device,
      game_width,
      game_height,
      crate::core::render::SCENE_FORMAT,
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
      camera_images,
      lit_texture,
      lit_texture_view,
      bind_group,
      mapping,
      display_effects,
      hdr_effects,
      presentation_blocks,
      capture_texture: None,
      keep_scene: false,
      scene_current: false,
      pending_capture: None,
      capture_result: None,
      fault: GpuFault::default(),
      needs_reconfigure: false,
    };

    // Initialize viewport transform
    renderer.update_viewport_transform();

    renderer
  }

  /// How screen space maps to the window.
  pub fn mapping(&self) -> ScreenMapping {
    self.mapping
  }

  /// The size the scene renders at, in pixels.
  pub fn render_size(&self) -> (u32, u32) {
    self.mapping.render_size()
  }

  /// The size of screen space (what the world's projection and the UI span).
  pub fn screen_size(&self) -> glam::Vec2 {
    self.mapping.screen_size()
  }

  /// The UI's size in pixels: the screen size rounded up.
  pub fn ui_size(&self) -> (u32, u32) {
    self.mapping.ui_size()
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

  /// The format the world renders in: linear HDR
  pub fn game_texture_format(&self) -> wgpu::TextureFormat {
    crate::core::render::SCENE_FORMAT
  }

  /// Resize the window target, keeping the presentation and scale factor.
  /// A zero side (a minimised window) is ignored.
  pub fn resize(&mut self, new_width: u32, new_height: u32) {
    if new_width == 0 || new_height == 0 {
      return;
    }
    self.set_mapping(self.mapping.with_physical((new_width, new_height)));
  }

  /// Follow a new mapping: reconfigure the window target when its physical
  /// size changed, recreate the scene-sized textures when the render size
  /// changed and the UI texture when the UI size changed, and move the
  /// composite rectangle.
  pub fn set_mapping(&mut self, mapping: ScreenMapping) {
    let (width, height) = mapping.physical_size();
    if mapping.physical_size() != self.mapping.physical_size() {
      match &mut self.target {
        PresentTarget::Surface { surface, config } => {
          config.width = width;
          config.height = height;
          surface.configure(&self.device, config);
        }
        // An offscreen target stands in for the window
        PresentTarget::Offscreen { texture } => {
          *texture = offscreen_target(&self.device, width, height, texture.format());
        }
      }
    } else if let PresentTarget::Surface { surface, config } = &mut self.target {
      // Same size: reconfiguring is how a lost or outdated surface comes back
      surface.configure(&self.device, config);
    }
    let old = std::mem::replace(&mut self.mapping, mapping);
    if old.render_size() != mapping.render_size() {
      let (width, height) = mapping.render_size();
      self.recreate_scene_targets(width, height);
    }
    if old.ui_size() != mapping.ui_size() {
      let (width, height) = mapping.ui_size();
      self.compositor.resize_ui(&self.device, width, height);
    }
    self.update_viewport_transform();
  }

  /// Recreate every texture the scene renders into at `width`x`height`.
  fn recreate_scene_targets(&mut self, new_width: u32, new_height: u32) {
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
      format: crate::core::render::SCENE_FORMAT,
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
    self.bind_group = crate::core::graphics::bind_groups::create(
      &self.device,
      &wgpu::BindGroupDescriptor {
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
      },
    );

    (self.lit_texture, self.lit_texture_view) =
      create_lit_texture(&self.device, new_width, new_height);
    self
      .display_effects
      .resize(&self.device, new_width, new_height);
    self.hdr_effects.resize(&self.device, new_width, new_height);
    self.compositor.forget_sources();

    // Recreated at the new size on the next capture
    self.capture_texture = None;
    self.scene_current = false;
  }

  /// Point the composite at the screen rectangle in the window.
  fn update_viewport_transform(&mut self) {
    self
      .compositor
      .set_viewport_transform(&self.queue, self.mapping.ndc_rect());
  }

  /// Keep every frame's scene image (scene and UI before the letterbox, at
  /// [`render_size`](Self::render_size)) for [`read_scene`](Self::read_scene).
  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn keep_scene_image(&mut self) {
    self.keep_scene = true;
  }

  /// The last frame's scene image, kept by
  /// [`keep_scene_image`](Self::keep_scene_image).
  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn read_scene(&self) -> CaptureResult<RgbaImage> {
    match &self.capture_texture {
      Some(texture) if self.scene_current => read_texture_rgba(&self.device, &self.queue, texture),
      _ => Err(CaptureError::Readback(
        "no scene image kept for the last frame".to_string(),
      )),
    }
  }

  /// Render a frame by running `pipeline`'s passes on one encoder.
  pub(crate) fn render_frame(
    &mut self,
    pipeline: &mut RenderPipeline,
    scene: FrameScene<'_>,
  ) -> Result<(), FrameError> {
    let (output, view, then_reconfigure) = self.target.acquire()?;
    self.needs_reconfigure |= then_reconfigure;
    let capture_path = self.prepare_capture();
    let screen = self.mapping.screen_size();
    let (render_width, render_height) = self.mapping.render_size();
    let frame = FrameInfo {
      screen_width: screen.x,
      screen_height: screen.y,
      render_width,
      render_height,
      world: !scene.draw_list.items().is_empty(),
      lighting: scene.lighting.is_some(),
      hdr_effects: scene.effects.has_active_effects_in(EffectStage::Hdr),
      effects: scene.effects.has_active_effects_in(EffectStage::Display),
      ui: scene.ui_buffer.iter().any(|&b| b != 0),
      transition: scene.overlay.1 > TRANSITION_VISIBLE,
      capture: capture_path.is_some() || self.keep_scene,
    };
    let sprites = scene.sprites;
    self
      .camera_images
      .sync(&self.device, scene.cameras, |target| {
        sprites.target_image(target)
      });
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
        camera_images: &self.camera_images,
        output_view: &view,
        capture: (capture_path.is_some() || self.keep_scene)
          .then_some(self.capture_texture.as_ref())
          .flatten(),
      };
      let mut ctx = RenderContext::new(
        &self.device,
        &self.queue,
        &mut encoder,
        frame,
        targets,
        scene,
        EffectStages {
          hdr: &mut self.hdr_effects,
          display: &mut self.display_effects,
        },
      );
      pipeline.execute(&mut ctx);
      captured = ctx.captured;
    }
    self.queue.submit(std::iter::once(encoder.finish()));
    if let Some(output) = output {
      self.queue.present(output);
    }
    self.scene_current = captured;
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
  ) -> Result<(), FrameError> {
    let sprites = SpriteGpuRenderer::new(&self.device, &self.queue);
    let uniforms = super::camera_gpu::CameraUniforms::default();
    let effects = EffectsManager::new();
    let look = crate::core::render::Look::default();
    let draw_list = DrawList::default();
    let scene = FrameScene {
      sprites: &sprites,
      batch: None,
      draw_list: &draw_list,
      lighting: None,
      effects: &effects,
      look: &look,
      ui_buffer,
      cameras: &[],
      camera_uniforms: &uniforms,
      time: 0.0,
      overlay: ([0; 3], 0.0),
    };
    self.render_frame(pipeline, scene)
  }

  /// Capture the next presented frame (scene and UI before the letterbox, at
  /// the render size) to `path`.
  ///
  /// Replaces any pending request. Collect the outcome with
  /// [`take_capture_result`](Self::take_capture_result) after rendering.
  /// Whether the surface asked to be reconfigured (and forget the request).
  pub(crate) fn take_needs_reconfigure(&mut self) -> bool {
    std::mem::take(&mut self.needs_reconfigure)
  }

  /// How many window cameras have an image of their own.
  #[cfg(test)]
  pub(crate) fn camera_image_count(&self) -> usize {
    self.camera_images.len()
  }

  /// A GPU error a device callback reported since the last call.
  pub(crate) fn take_fault(&self) -> Option<String> {
    self.fault.take()
  }

  pub fn request_capture(&mut self, path: PathBuf) {
    self.pending_capture = Some(path);
  }

  /// Take the result of the latest capture, if one ran since the last call.
  pub fn take_capture_result(&mut self) -> Option<CaptureResult<CapturedFrame>> {
    self.capture_result.take()
  }

  /// Take the pending request and make sure the capture texture matches the
  /// render size (also when the scene image is kept every frame).
  fn prepare_capture(&mut self) -> Option<PathBuf> {
    let path = self.pending_capture.take();
    if path.is_none() && !self.keep_scene {
      return None;
    }
    let (width, height) = self.mapping.render_size();
    let up_to_date = self
      .capture_texture
      .as_ref()
      .is_some_and(|t| t.width() == width && t.height() == height);
    if !up_to_date {
      self.capture_texture = Some(self.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Frame Capture Texture"),
        size: wgpu::Extent3d {
          width,
          height,
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
    path
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
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_new_render_size_recreates_every_scene_texture() {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return; // no GPU on this machine
    };
    let mut renderer = WgpuRenderer::new_offscreen(device, queue, 64, 48);
    renderer.resize(96, 80);

    let size = |t: &wgpu::Texture| (t.width(), t.height());
    assert_eq!(size(&renderer.texture), (96, 80));
    assert_eq!(size(&renderer.lit_texture), (96, 80));
    assert_eq!(size(renderer.compositor.ui_texture()), (96, 80));
    let post = &renderer.display_effects;
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
  fn a_zero_size_resize_is_ignored() {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return; // no GPU on this machine
    };
    let mut renderer = WgpuRenderer::new_offscreen(device, queue, 64, 48);
    renderer.resize(0, 0);
    renderer.resize(0, 10);
    renderer.resize(10, 0);

    assert_eq!(renderer.render_size(), (64, 48));
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
}
