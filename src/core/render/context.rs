//! Render context providing access to rendering resources

use crate::core::graphics::sprite_gpu::{BatchCollector, SpriteGpuRenderer};
use crate::core::graphics::Camera;
use crate::core::lighting::LightingRenderer;
use crate::core::shaders::EffectsManager;
use crate::core::trail::TrailRenderer;

/// Which texture is currently the "source" for the next pass
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureSlot {
  /// Main game texture (CPU pixel buffer uploaded here)
  Game,
  /// Post-process scratch buffer A
  ScratchA,
  /// Post-process scratch buffer B
  ScratchB,
}

impl TextureSlot {
  /// Get the alternate scratch buffer (for ping-pong rendering)
  pub fn alternate(&self) -> Self {
    match self {
      Self::Game => Self::ScratchA,
      Self::ScratchA => Self::ScratchB,
      Self::ScratchB => Self::ScratchA,
    }
  }
}

/// Context provided to render passes during execution.
///
/// Contains all the resources needed for rendering. Passes should use this
/// to access textures, buffers, and GPU resources rather than storing their
/// own references.
pub struct RenderContext<'a> {
  // === GPU Resources ===
  /// The wgpu device for creating GPU resources
  pub device: &'a wgpu::Device,
  /// The wgpu queue for submitting commands
  pub queue: &'a wgpu::Queue,
  /// Command encoder for recording GPU commands
  pub encoder: &'a mut wgpu::CommandEncoder,

  // === Textures ===
  /// Main game texture (CPU content uploaded here)
  pub game_texture: &'a wgpu::Texture,
  /// View of the game texture for rendering
  pub game_texture_view: &'a wgpu::TextureView,
  /// Scratch texture A for post-processing
  pub scratch_a: Option<&'a wgpu::Texture>,
  /// View of scratch A
  pub scratch_a_view: Option<&'a wgpu::TextureView>,
  /// Scratch texture B for post-processing
  pub scratch_b: Option<&'a wgpu::Texture>,
  /// View of scratch B
  pub scratch_b_view: Option<&'a wgpu::TextureView>,
  /// Final output view (swapchain texture)
  pub output_view: &'a wgpu::TextureView,

  // === CPU Buffer ===
  /// CPU pixel buffer for software rendering
  pub pixel_buffer: &'a mut [u8],

  // === Frame Info ===
  /// Game resolution width
  pub width: u32,
  /// Game resolution height
  pub height: u32,
  /// Time in seconds since engine start
  pub time: f32,
  /// Delta time for this frame
  pub delta_time: f32,

  // === Camera ===
  /// Camera for world-to-screen transforms
  pub camera: &'a Camera,

  // === Renderers ===
  /// GPU sprite renderer with texture cache
  pub sprite_gpu: &'a mut SpriteGpuRenderer,
  /// Sprite batch collector
  pub batch_collector: &'a mut BatchCollector,
  /// Trail renderer
  pub trail_renderer: Option<&'a mut TrailRenderer>,
  /// Lighting renderer
  pub lighting: Option<&'a mut LightingRenderer>,
  /// Post-processing effects manager
  pub effects: Option<&'a EffectsManager>,

  // === State ===
  /// Which texture currently holds the render result
  pub current_source: TextureSlot,
  /// Whether lighting was applied this frame
  pub lighting_applied: bool,
  /// Whether UI should be rendered after lighting
  pub ui_after_lighting: bool,
}

impl<'a> RenderContext<'a> {
  /// Get the texture view for a given slot
  pub fn get_texture_view(&self, slot: TextureSlot) -> Option<&wgpu::TextureView> {
    match slot {
      TextureSlot::Game => Some(self.game_texture_view),
      TextureSlot::ScratchA => self.scratch_a_view,
      TextureSlot::ScratchB => self.scratch_b_view,
    }
  }

  /// Get the texture for a given slot
  pub fn get_texture(&self, slot: TextureSlot) -> Option<&wgpu::Texture> {
    match slot {
      TextureSlot::Game => Some(self.game_texture),
      TextureSlot::ScratchA => self.scratch_a,
      TextureSlot::ScratchB => self.scratch_b,
    }
  }

  /// Get the current source texture view
  pub fn current_texture_view(&self) -> Option<&wgpu::TextureView> {
    self.get_texture_view(self.current_source)
  }

  /// Advance to the next texture slot (for ping-pong rendering)
  pub fn advance_slot(&mut self) {
    self.current_source = self.current_source.alternate();
  }
}
