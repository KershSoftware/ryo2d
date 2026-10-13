//! What a render pass can use while it records its work

use super::FrameInfo;
use super::frame::{CameraDraw, FrameImage, FrameScene};
use crate::core::ecs::Entity;
use crate::core::graphics::camera_images::CameraImages;
use crate::core::graphics::{CameraView, Compositor};
use crate::core::shaders::{OutputBuffer, PostProcessPipeline};

/// The renderer's textures and pipelines the built-in passes draw with.
pub(crate) struct FrameTargets<'a> {
  pub(crate) game: &'a wgpu::Texture,
  pub(crate) game_view: &'a wgpu::TextureView,
  pub(crate) lit: &'a wgpu::Texture,
  pub(crate) lit_view: &'a wgpu::TextureView,
  pub(crate) compositor: &'a Compositor,
  /// The window cameras' own images
  pub(crate) camera_images: &'a CameraImages,
  /// The window surface or offscreen texture this frame is presented to
  pub(crate) output_view: &'a wgpu::TextureView,
  /// The capture texture, only while a capture is pending this frame
  pub(crate) capture: Option<&'a wgpu::Texture>,
}

/// What a [`RenderPass`](super::RenderPass) can use while it records its work.
pub struct RenderContext<'a> {
  device: &'a wgpu::Device,
  queue: &'a wgpu::Queue,
  // Built-in passes borrow it next to the crate-internal fields
  pub(super) encoder: &'a mut wgpu::CommandEncoder,
  frame: FrameInfo,
  pub(crate) targets: FrameTargets<'a>,
  pub(crate) scene: FrameScene<'a>,
  pub(crate) post_process: Option<&'a mut PostProcessPipeline>,
  /// Which texture holds the latest image
  pub(crate) image: FrameImage,
  /// The camera whose passes are running (an index into the scene's
  /// cameras), and the scene image to go back to after them
  camera: Option<usize>,
  scene_image: FrameImage,
  /// Whether the `ui` pass uploaded this frame's UI (the composite only blends it then)
  pub(crate) ui_uploaded: bool,
  /// Whether the `capture` pass drew into the capture texture this frame
  pub(crate) captured: bool,
}

impl<'a> RenderContext<'a> {
  /// A context for one frame; the latest image starts as the game texture.
  pub(crate) fn new(
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    encoder: &'a mut wgpu::CommandEncoder,
    frame: FrameInfo,
    targets: FrameTargets<'a>,
    scene: FrameScene<'a>,
    post_process: Option<&'a mut PostProcessPipeline>,
  ) -> Self {
    Self {
      device,
      queue,
      encoder,
      frame,
      targets,
      scene,
      post_process,
      image: FrameImage::Game,
      camera: None,
      scene_image: FrameImage::Game,
      ui_uploaded: false,
      captured: false,
    }
  }

  /// The GPU device.
  pub fn device(&self) -> &'a wgpu::Device {
    self.device
  }

  /// The GPU queue (buffer and texture writes land before the frame's commands).
  pub fn queue(&self) -> &'a wgpu::Queue {
    self.queue
  }

  /// The frame's command encoder.
  pub fn encoder(&mut self) -> &mut wgpu::CommandEncoder {
    self.encoder
  }

  /// What this frame contains.
  pub fn frame(&self) -> &FrameInfo {
    &self.frame
  }

  /// The size the scene renders at, in pixels: every scene image (game, lit
  /// and post-process output) has it. A camera with an image of its own
  /// draws on its viewport's part of it.
  pub fn render_size(&self) -> (u32, u32) {
    (self.frame.render_width, self.frame.render_height)
  }

  /// The size of screen space, which the world's projection and the UI span.
  pub fn screen_size(&self) -> glam::Vec2 {
    glam::Vec2::new(self.frame.screen_width, self.frame.screen_height)
  }

  /// The UI image's size in pixels: the screen size rounded up.
  pub(crate) fn ui_size(&self) -> (u32, u32) {
    let size = self.screen_size().ceil();
    ((size.x as u32).max(1), (size.y as u32).max(1))
  }

  /// The format of [`current_view`](Self::current_view), for the render
  /// pipelines a pass builds. Every game-size image (game, lit and
  /// post-process output) shares it.
  pub fn format(&self) -> wgpu::TextureFormat {
    image_texture(&self.targets, self.post_process.as_deref(), self.image).format()
  }

  /// The texture holding the frame's latest image (the game texture, the lit
  /// scene or the post-process output), at game resolution. In a camera pass
  /// it is the camera's image.
  ///
  /// Only passes before `composite` reach the window: it copies this image to
  /// the screen, so drawing on it from a pass placed after `composite` is not
  /// shown. Insert game passes before `composite` (or after `post_process`).
  pub fn current_view(&self) -> &wgpu::TextureView {
    image_view(&self.targets, self.post_process.as_deref(), self.image)
  }

  /// The encoder and the latest image together, to draw onto the image.
  ///
  /// Only passes before `composite` reach the window: a pass placed after
  /// `composite` draws on an image that is no longer presented.
  pub fn encoder_and_current_view(&mut self) -> (&mut wgpu::CommandEncoder, &wgpu::TextureView) {
    let view = image_view(&self.targets, self.post_process.as_deref(), self.image);
    (&mut *self.encoder, view)
  }

  /// The view of the camera whose passes are running (`None` in a frame
  /// pass).
  pub fn camera(&self) -> Option<&CameraView> {
    self.current_camera().map(|camera| &camera.view)
  }

  /// The entity of the camera whose passes are running (`None` in a frame
  /// pass).
  pub fn camera_entity(&self) -> Option<Entity> {
    self.current_camera().map(|camera| camera.entity)
  }

  pub(crate) fn current_camera(&self) -> Option<&'a CameraDraw> {
    self.camera.and_then(|index| self.scene.cameras.get(index))
  }

  /// Run the following passes for the scene's `index`th camera, on its image.
  pub(super) fn enter_camera(&mut self, index: usize) {
    let Some(camera) = self.scene.cameras.get(index) else {
      return;
    };
    self.camera = Some(index);
    self.scene_image = self.image;
    if !camera.direct {
      self.image = FrameImage::Camera(camera.entity);
    }
  }

  /// Back to the scene image. A camera that drew straight into it leaves
  /// its latest image (lit or not) as the scene's; a texture camera with an
  /// image of its own places it in its rectangle of the target.
  pub(super) fn leave_camera(&mut self) {
    if let Some(camera) = self.current_camera() {
      if !camera.direct {
        self.image = self.scene_image;
      }
      if camera.own_image
        && let Some((_, view)) = camera
          .target
          .and_then(|target| self.scene.sprites.target_image(target))
      {
        self.targets.camera_images.place(self.encoder, view, camera);
      }
    }
    self.camera = None;
  }
}

/// The view of the texture that holds `image`.
pub(super) fn image_view<'t>(
  targets: &'t FrameTargets<'_>,
  post_process: Option<&'t PostProcessPipeline>,
  image: FrameImage,
) -> &'t wgpu::TextureView {
  match (image, post_process) {
    (FrameImage::Lit, _) => targets.lit_view,
    (FrameImage::PostProcess(buffer), Some(post_process)) => post_process.get_output_view(buffer),
    (FrameImage::Camera(camera), _) => targets
      .camera_images
      .view(camera)
      .unwrap_or(targets.game_view),
    // Post-process output without a post-process pipeline can't happen; fall back to the scene
    (FrameImage::Game, _) | (FrameImage::PostProcess(_), None) => targets.game_view,
  }
}

/// The texture that holds `image`.
pub(super) fn image_texture<'t>(
  targets: &'t FrameTargets<'_>,
  post_process: Option<&'t PostProcessPipeline>,
  image: FrameImage,
) -> &'t wgpu::Texture {
  match (image, post_process) {
    (FrameImage::Lit, _) => targets.lit,
    (FrameImage::PostProcess(OutputBuffer::A), Some(post_process)) => post_process.texture_a(),
    (FrameImage::PostProcess(OutputBuffer::B), Some(post_process)) => post_process.texture_b(),
    (FrameImage::Camera(camera), _) => targets
      .camera_images
      .texture(camera)
      .unwrap_or(targets.game),
    (FrameImage::Game, _) | (FrameImage::PostProcess(_), None) => targets.game,
  }
}
