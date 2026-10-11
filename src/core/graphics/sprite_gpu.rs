//! GPU-based sprite rendering with per-sprite shader effects
//!
//! This module provides a GPU rendering path for sprites that have `Material`
//! components or inline shaders. Unlike the CPU software renderer, this path:
//!
//! - Renders sprites as textured quads on the GPU
//! - Applies per-sprite shader effects (outline, glow, dissolve, etc.)
//! - Supports custom user shaders via `ShaderHandle`
//! - Can batch sprites with the same shader for performance
//!
//! # Architecture
//!
//! ```text
//! Engine present
//!     │
//!     ├── DrawList: entities, `render_gpu`/particle/parallax sprites and
//!     │   trail groups, sorted by z
//!     │
//!     └── SpriteGpuRenderer::prepare_world()
//!         ├── Make used textures resident in the texture atlas
//!         ├── Remap instance UVs into atlas slots (mask, effect growth)
//!         └── One draw per run of consecutive sprites on one atlas page
//!             and pipeline (drawn by the `world` pass)
//! ```
//!
//! Textures live in a texture atlas (`Rgba8Unorm` pages holding raw sRGB
//! bytes). The shader tints and applies effects in sRGB space and converts
//! to linear on output.

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use wgpu::util::DeviceExt;

use super::{AtlasKey, AtlasSlot, SlotPage, TextureAtlas};
use crate::core::asset::SpriteHandle;
use crate::core::ecs::{Material, ShaderHandle, ShaderParams, SpriteEffect, SpriteShader};
use crate::core::render::{
  DrawKind, DrawList, PipelineKind, SpriteShading, TextureRef, WorldRun, plan_runs,
};
use crate::core::shaders::custom::{self, ParamLayout, ShaderError, ShaderKind};
use crate::core::ui::Color;

/// Per-instance data for GPU sprite rendering
///
/// This struct is passed to the shader for each sprite instance.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SpriteInstance {
  /// Transform matrix (2D affine: scale, rotation, translation)
  /// Row-major 3x3 packed into 3 vec4s (last component unused)
  pub transform: [[f32; 4]; 3],
  /// UV rect: x=u_min, y=v_min, z=u_max, w=v_max
  pub uv_rect: [f32; 4],
  /// Tint color (RGBA of the sRGB colour, straight alpha, 0-1), applied in
  /// sRGB space like the CPU renderer
  pub tint: [f32; 4],
  /// Effect parameters (meaning depends on shader)
  /// x=effect_type, y,z,w=effect-specific params
  pub effect_params: [f32; 4],
  /// Additional effect parameters
  /// Used for effects that need more than 4 floats
  pub effect_extra: [f32; 4],
  /// The sprite's own rect in atlas UVs (u_min, v_min, u_max, v_max).
  ///
  /// Set by the renderer at draw time from `uv_rect`; texture samples outside
  /// it are transparent, so effects never pick up atlas neighbours.
  pub uv_bounds: [f32; 4],
}

impl SpriteInstance {
  /// Create a basic instance with identity transform and full UV
  pub fn new() -> Self {
    Self {
      transform: [
        [1.0, 0.0, 0.0, 0.0], // row 0: scale_x, shear_x, translate_x
        [0.0, 1.0, 0.0, 0.0], // row 1: shear_y, scale_y, translate_y
        [0.0, 0.0, 1.0, 0.0], // row 2: (unused in 2D)
      ],
      uv_rect: [0.0, 0.0, 1.0, 1.0],
      tint: [1.0, 1.0, 1.0, 1.0],
      effect_params: [0.0; 4],
      effect_extra: [0.0; 4],
      uv_bounds: [0.0, 0.0, 1.0, 1.0],
    }
  }

  /// Set transform from position, rotation (radians), and scale
  pub fn with_transform(
    mut self,
    x: f32,
    y: f32,
    rotation: f32,
    scale_x: f32,
    scale_y: f32,
  ) -> Self {
    let cos = rotation.cos();
    let sin = rotation.sin();

    // 2D affine transform matrix (column-major for WGSL mat3x3)
    // WGSL mat3x3 constructor takes column vectors:
    // Column 0: X-axis transform [scale_x * cos, scale_x * sin, 0]
    // Column 1: Y-axis transform [-scale_y * sin, scale_y * cos, 0]
    // Column 2: Translation [x, y, 1]
    self.transform[0] = [scale_x * cos, scale_x * sin, 0.0, 0.0];
    self.transform[1] = [-scale_y * sin, scale_y * cos, 0.0, 0.0];
    self.transform[2] = [x, y, 1.0, 0.0];

    self
  }

  /// Set UV rectangle (for sprite sheets/atlases)
  pub fn with_uv(mut self, u_min: f32, v_min: f32, u_max: f32, v_max: f32) -> Self {
    self.uv_rect = [u_min, v_min, u_max, v_max];
    self
  }

  /// Set tint color with alpha
  pub fn with_tint(mut self, color: Color, alpha: f32) -> Self {
    let a = (color.a as f32 / 255.0) * alpha;
    self.tint = [
      color.r as f32 / 255.0,
      color.g as f32 / 255.0,
      color.b as f32 / 255.0,
      a,
    ];
    self
  }

  /// Set effect parameters from a SpriteEffect
  pub fn with_effect(mut self, effect: &SpriteEffect) -> Self {
    match effect {
      SpriteEffect::Outline { color, thickness } => {
        self.effect_params = [1.0, *thickness as f32, 0.0, 0.0];
        self.effect_extra = [
          color.r as f32 / 255.0,
          color.g as f32 / 255.0,
          color.b as f32 / 255.0,
          color.a as f32 / 255.0,
        ];
      }
      SpriteEffect::Glow {
        color,
        radius,
        intensity,
      } => {
        self.effect_params = [2.0, *radius, *intensity, 0.0];
        self.effect_extra = [
          color.r as f32 / 255.0,
          color.g as f32 / 255.0,
          color.b as f32 / 255.0,
          color.a as f32 / 255.0,
        ];
      }
      SpriteEffect::Flash { color, intensity } => {
        self.effect_params = [3.0, *intensity, 0.0, 0.0];
        self.effect_extra = [
          color.r as f32 / 255.0,
          color.g as f32 / 255.0,
          color.b as f32 / 255.0,
          color.a as f32 / 255.0,
        ];
      }
      SpriteEffect::ColorShift { color, amount } => {
        self.effect_params = [4.0, *amount, 0.0, 0.0];
        self.effect_extra = [
          color.r as f32 / 255.0,
          color.g as f32 / 255.0,
          color.b as f32 / 255.0,
          color.a as f32 / 255.0,
        ];
      }
      SpriteEffect::Silhouette { color } => {
        self.effect_params = [5.0, 0.0, 0.0, 0.0];
        self.effect_extra = [
          color.r as f32 / 255.0,
          color.g as f32 / 255.0,
          color.b as f32 / 255.0,
          color.a as f32 / 255.0,
        ];
      }
      SpriteEffect::Pixelate { block_size } => {
        self.effect_params = [6.0, *block_size as f32, 0.0, 0.0];
      }
      SpriteEffect::Wave {
        amplitude,
        frequency,
        speed,
        time,
      } => {
        self.effect_params = [7.0, *amplitude, *frequency, *time];
        self.effect_extra = [*speed, 0.0, 0.0, 0.0];
      }
      SpriteEffect::Dissolve {
        progress,
        edge_color,
      } => {
        self.effect_params = [8.0, *progress, 0.0, 0.0];
        if let Some(color) = edge_color {
          self.effect_extra = [
            color.r as f32 / 255.0,
            color.g as f32 / 255.0,
            color.b as f32 / 255.0,
            color.a as f32 / 255.0,
          ];
        }
      }
      SpriteEffect::Invert => {
        self.effect_params = [9.0, 0.0, 0.0, 0.0];
      }
      SpriteEffect::Grayscale { intensity } => {
        self.effect_params = [10.0, *intensity, 0.0, 0.0];
      }
      SpriteEffect::DirectionalOutline {
        solid_color,
        passable_color,
        thickness,
      } => {
        // The passable (bottom edge) colour is packed two 8-bit channels per
        // float (exact integers), the solid colour goes in effect_extra
        let p = passable_color;
        self.effect_params = [
          11.0,
          *thickness as f32,
          p.r as f32 * 256.0 + p.g as f32,
          p.b as f32 * 256.0 + p.a as f32,
        ];
        self.effect_extra = [
          solid_color.r as f32 / 255.0,
          solid_color.g as f32 / 255.0,
          solid_color.b as f32 / 255.0,
          solid_color.a as f32 / 255.0,
        ];
      }
    }
    self
  }
}

impl Default for SpriteInstance {
  fn default() -> Self {
    Self::new()
  }
}

/// Global uniforms for sprite rendering
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SpriteGlobals {
  /// View-projection matrix (orthographic 2D)
  pub view_proj: [[f32; 4]; 4],
  /// Time and resolution: x=time, y=width, z=height, w=unused
  pub time_resolution: [f32; 4],
}

impl SpriteGlobals {
  /// Create globals for orthographic 2D rendering
  pub fn new(width: f32, height: f32, time: f32) -> Self {
    // Orthographic projection: maps (0,0)-(width,height) to clip space (-1,-1)-(1,1)
    // Y is flipped so 0 is at top
    Self {
      view_proj: [
        [2.0 / width, 0.0, 0.0, 0.0],
        [0.0, -2.0 / height, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [-1.0, 1.0, 0.0, 1.0],
      ],
      time_resolution: [time, width, height, 0.0],
    }
  }
}

/// Where an uploaded texture's pixels come from, for re-uploading it after
/// the atlas evicted it.
enum TextureSource {
  Pixels(Vec<u8>),
  /// Read from the sprite when (re)uploaded
  Sprite(SpriteHandle),
}

/// A texture uploaded by id.
struct CachedTexture {
  key: AtlasKey,
  /// Size at upload (sprites are not re-read for it every frame)
  width: u32,
  height: u32,
  source: TextureSource,
}

impl CachedTexture {
  fn rgba(&self) -> Vec<u8> {
    match &self.source {
      TextureSource::Pixels(data) => data.clone(),
      TextureSource::Sprite(sprite) => sprite.with_sprite(|s| s.data().to_vec()),
    }
  }
}

/// An entity sprite's current texture: which version of its content is
/// uploaded and under which id.
struct EntitySprite {
  generation: u64,
  texture: TextureRef,
  /// False when this version could not be uploaded; it is not tried again
  /// until the sprite changes
  uploaded: bool,
  /// Atlas frame it was last asked for, to drop sprites that went away
  last_used: u64,
}

/// Entity sprites unused for this many frames are forgotten.
const ENTITY_SPRITE_TTL: u64 = 600;

/// Most entity sprites kept as standalone (over 256 px) textures; the least
/// recently used beyond it are freed, so a game minting large sprite handles
/// every frame can't hold hundreds of them until the TTL.
const MAX_STANDALONE_ENTITY_SPRITES: usize = 64;

/// A compiled game shader: its pipeline and the `Params` layout to pack for it.
struct CustomPipeline {
  pipeline: wgpu::RenderPipeline,
  layout: ParamLayout,
}

/// What the cache knows about one shader version.
struct CustomEntry {
  generation: u64,
  /// `None` when this version failed to compile; it is not retried until the
  /// shader's source changes
  ready: Option<CustomPipeline>,
  /// Atlas frame the shader was last drawn in, to drop unused shaders
  last_used: u64,
}

/// Shaders unused for this many frames are forgotten, so handles created per
/// spawn don't keep their pipelines forever.
const CUSTOM_SHADER_TTL: u64 = 600;

/// Pipelines of game sprite shaders, built on first use and rebuilt when the
/// shader's source changes.
///
/// Built with the effect pipeline's layout, vertex buffers, blend and target,
/// but with the shader's own `fs_custom` entry point.
struct CustomPipelines {
  layout: wgpu::PipelineLayout,
  vertex: wgpu::VertexBufferLayout<'static>,
  instance: wgpu::VertexBufferLayout<'static>,
  format: wgpu::TextureFormat,
  /// By `ShaderHandle::serial()`
  entries: HashMap<u64, CustomEntry>,
  /// Parameter names already warned about, by (serial, generation)
  warned: HashMap<(u64, u64), HashSet<String>>,
}

impl CustomPipelines {
  /// Make sure the cache holds the shader's current version; whether it can
  /// be drawn.
  fn prepare(&mut self, device: &wgpu::Device, shader: &ShaderHandle, frame: u64) -> bool {
    let serial = shader.serial();
    // Cheap check first: this runs for every custom sprite of every frame
    if let Some(entry) = self.entries.get_mut(&serial)
      && entry.generation == shader.generation()
    {
      entry.last_used = frame;
      return entry.ready.is_some();
    }
    let (generation, source) = shader.snapshot();
    let ready = self.build(device, shader, generation, &source);
    let usable = ready.is_some();
    self.entries.insert(
      serial,
      CustomEntry {
        generation,
        ready,
        last_used: frame,
      },
    );
    usable
  }

  /// Drop shaders unused for [`CUSTOM_SHADER_TTL`] frames and their warnings.
  fn prune(&mut self, frame: u64) {
    self
      .entries
      .retain(|_, e| frame.saturating_sub(e.last_used) < CUSTOM_SHADER_TTL);
    let entries = &self.entries;
    self.warned.retain(|(serial, generation), _| {
      entries
        .get(serial)
        .is_some_and(|e| e.generation == *generation)
    });
  }

  /// Compile `source` and build its pipeline; any failure is logged once and
  /// leaves the shader drawn plain.
  fn build(
    &self,
    device: &wgpu::Device,
    shader: &ShaderHandle,
    generation: u64,
    source: &str,
  ) -> Option<CustomPipeline> {
    let serial = shader.serial();
    let compiled = match custom::compile(ShaderKind::Sprite, shader.id(), source) {
      Ok(compiled) => compiled,
      Err(error) => {
        custom::report_once(serial, generation, &error);
        return None;
      }
    };

    // Anything naga accepted but the device rejects surfaces here instead of
    // taking the engine down
    device.push_error_scope(wgpu::ErrorFilter::Validation);
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some(shader.id()),
      source: wgpu::ShaderSource::Wgsl(compiled.wgsl.into()),
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some(shader.id()),
      layout: Some(&self.layout),
      vertex: wgpu::VertexState {
        module: &module,
        entry_point: "vs_main",
        buffers: &[self.vertex.clone(), self.instance.clone()],
        compilation_options: Default::default(),
      },
      fragment: Some(wgpu::FragmentState {
        module: &module,
        entry_point: "fs_custom",
        targets: &[Some(wgpu::ColorTargetState {
          format: self.format,
          blend: Some(wgpu::BlendState::ALPHA_BLENDING),
          write_mask: wgpu::ColorWrites::ALL,
        })],
        compilation_options: Default::default(),
      }),
      primitive: wgpu::PrimitiveState {
        topology: wgpu::PrimitiveTopology::TriangleList,
        ..Default::default()
      },
      depth_stencil: None,
      multisample: wgpu::MultisampleState::default(),
      multiview: None,
      cache: None,
    });
    if let Some(error) = pollster::block_on(device.pop_error_scope()) {
      let error = ShaderError {
        shader: shader.id().to_string(),
        line: None,
        column: None,
        message: error.to_string(),
      };
      custom::report_once(serial, generation, &error);
      return None;
    }
    Some(CustomPipeline {
      pipeline,
      layout: compiled.layout,
    })
  }

  /// Put `params` into the instance's eight effect floats, in the layout of
  /// the shader's `Params`. Names the shader doesn't have are warned about
  /// once per shader version; correct names allocate nothing.
  fn pack(&mut self, shader: &ShaderHandle, params: &ShaderParams, instance: &mut SpriteInstance) {
    let serial = shader.serial();
    let Some(entry) = self.entries.get(&serial) else {
      return;
    };
    let Some(ready) = entry.ready.as_ref() else {
      return;
    };
    let generation = entry.generation;
    debug_assert_eq!(custom::uniform_size(ShaderKind::Sprite, &ready.layout), 32);
    let mut bytes = [0u8; 32];
    let warned = &mut self.warned;
    custom::pack_with(&ready.layout, params, &mut bytes, |name, expected| {
      let seen = warned
        .get(&(serial, generation))
        .is_some_and(|names| names.contains(name));
      if seen {
        return;
      }
      warned
        .entry((serial, generation))
        .or_default()
        .insert(name.to_string());
      match expected {
        None => crate::engine_warn!(
          module: "Shaders",
          "{}: parameter `{}` is not a field of Params and is ignored",
          shader.id(),
          name
        ),
        Some(kind) => crate::engine_warn!(
          module: "Shaders",
          "{}: parameter `{}` has the wrong type (Params expects {:?}) and is ignored",
          shader.id(),
          name,
          kind
        ),
      }
    });
    let floats: [f32; 8] = bytemuck::cast(bytes);
    instance.effect_params = [floats[0], floats[1], floats[2], floats[3]];
    instance.effect_extra = [floats[4], floats[5], floats[6], floats[7]];
  }

  fn pipeline(&self, serial: u64) -> Option<&wgpu::RenderPipeline> {
    let ready = self.entries.get(&serial)?.ready.as_ref()?;
    Some(&ready.pipeline)
  }
}

/// GPU-based sprite renderer with per-sprite shader effects
pub struct SpriteGpuRenderer {
  // Shader pipelines (one per effect type + base)
  base_pipeline: wgpu::RenderPipeline,
  effect_pipeline: wgpu::RenderPipeline,
  // Game sprite shaders
  custom: CustomPipelines,

  // Global uniforms
  globals_buffer: wgpu::Buffer,
  globals_bind_group: wgpu::BindGroup,

  // Instance buffer (rebuilt each frame)
  instance_buffer: wgpu::Buffer,
  instance_capacity: usize,

  // Vertex buffer (quad)
  vertex_buffer: wgpu::Buffer,

  // Every uploaded texture lives in the atlas under `AtlasKey::Id`
  atlas: TextureAtlas,
  // Pixel sources by texture id, for re-uploading evicted textures
  textures: HashMap<String, CachedTexture>,
  // Entity sprites by handle identity, each under one texture id
  entity_sprites: HashMap<usize, EntitySprite>,
  // Atlas frame counter, advanced by `prepare`
  frame: u64,

  // This frame's instances remapped into atlas slots, and the draws over
  // them; both built by `prepare_world` and reused across frames
  placed: Vec<SpriteInstance>,
  world_runs: Vec<WorldRun>,
  // Textures already made resident this frame (scratch, reused across frames)
  resident_scratch: HashSet<TextureRef>,

  // Timing
  time: f32,
}

impl SpriteGpuRenderer {
  /// Initial instance buffer capacity (grown by `prepare` when needed)
  const MAX_INSTANCES: usize = 10000;

  /// Create a new GPU sprite renderer
  pub fn new(
    device: &wgpu::Device,
    _queue: &wgpu::Queue,
    surface_format: wgpu::TextureFormat,
  ) -> Self {
    // Create bind group layouts
    let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
      label: Some("Sprite Globals Layout"),
      entries: &[wgpu::BindGroupLayoutEntry {
        binding: 0,
        visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
          ty: wgpu::BufferBindingType::Uniform,
          has_dynamic_offset: false,
          min_binding_size: None,
        },
        count: None,
      }],
    });

    let texture_layout = Arc::new(device.create_bind_group_layout(
      &wgpu::BindGroupLayoutDescriptor {
        label: Some("Sprite Texture Layout"),
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
        ],
      },
    ));

    // Nearest filtering for pixel art; the atlas border covers any edge overshoot
    let sampler = Arc::new(device.create_sampler(&wgpu::SamplerDescriptor {
      label: Some("Sprite Sampler"),
      address_mode_u: wgpu::AddressMode::ClampToEdge,
      address_mode_v: wgpu::AddressMode::ClampToEdge,
      address_mode_w: wgpu::AddressMode::ClampToEdge,
      mag_filter: wgpu::FilterMode::Nearest,
      min_filter: wgpu::FilterMode::Nearest,
      mipmap_filter: wgpu::FilterMode::Nearest,
      ..Default::default()
    }));

    // Create globals buffer
    let globals = SpriteGlobals::new(320.0, 180.0, 0.0);
    let globals_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
      label: Some("Sprite Globals Buffer"),
      contents: bytemuck::cast_slice(&[globals]),
      usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });

    let globals_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
      label: Some("Sprite Globals Bind Group"),
      layout: &globals_layout,
      entries: &[wgpu::BindGroupEntry {
        binding: 0,
        resource: globals_buffer.as_entire_binding(),
      }],
    });

    let instance_buffer = Self::create_instance_buffer(device, Self::MAX_INSTANCES);

    // Create quad vertex buffer
    // Each vertex: position (x, y), uv (u, v)
    #[repr(C)]
    #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
    struct Vertex {
      position: [f32; 2],
      uv: [f32; 2],
    }

    // Center-based quad: translation becomes the center position
    // This matches industry-standard GPU sprite conventions
    let vertices = [
      Vertex {
        position: [-0.5, -0.5],
        uv: [0.0, 0.0],
      }, // top-left
      Vertex {
        position: [0.5, -0.5],
        uv: [1.0, 0.0],
      }, // top-right
      Vertex {
        position: [-0.5, 0.5],
        uv: [0.0, 1.0],
      }, // bottom-left
      Vertex {
        position: [0.5, -0.5],
        uv: [1.0, 0.0],
      }, // top-right
      Vertex {
        position: [0.5, 0.5],
        uv: [1.0, 1.0],
      }, // bottom-right
      Vertex {
        position: [-0.5, 0.5],
        uv: [0.0, 1.0],
      }, // bottom-left
    ];

    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
      label: Some("Sprite Vertex Buffer"),
      contents: bytemuck::cast_slice(&vertices),
      usage: wgpu::BufferUsages::VERTEX,
    });

    // Create shader module
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some("Sprite Shader"),
      source: wgpu::ShaderSource::Wgsl(SPRITE_SHADER.into()),
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("Sprite Pipeline Layout"),
      bind_group_layouts: &[&globals_layout, &*texture_layout],
      push_constant_ranges: &[],
    });

    // Vertex buffer layout
    let vertex_layout: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
      array_stride: 16, // 4 floats * 4 bytes
      step_mode: wgpu::VertexStepMode::Vertex,
      attributes: &[
        wgpu::VertexAttribute {
          offset: 0,
          shader_location: 0, // position
          format: wgpu::VertexFormat::Float32x2,
        },
        wgpu::VertexAttribute {
          offset: 8,
          shader_location: 1, // uv
          format: wgpu::VertexFormat::Float32x2,
        },
      ],
    };

    // Instance buffer layout
    let instance_layout: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
      array_stride: std::mem::size_of::<SpriteInstance>() as u64,
      step_mode: wgpu::VertexStepMode::Instance,
      attributes: &[
        // transform row 0
        wgpu::VertexAttribute {
          offset: 0,
          shader_location: 2,
          format: wgpu::VertexFormat::Float32x4,
        },
        // transform row 1
        wgpu::VertexAttribute {
          offset: 16,
          shader_location: 3,
          format: wgpu::VertexFormat::Float32x4,
        },
        // transform row 2
        wgpu::VertexAttribute {
          offset: 32,
          shader_location: 4,
          format: wgpu::VertexFormat::Float32x4,
        },
        // uv_rect
        wgpu::VertexAttribute {
          offset: 48,
          shader_location: 5,
          format: wgpu::VertexFormat::Float32x4,
        },
        // tint
        wgpu::VertexAttribute {
          offset: 64,
          shader_location: 6,
          format: wgpu::VertexFormat::Float32x4,
        },
        // effect_params
        wgpu::VertexAttribute {
          offset: 80,
          shader_location: 7,
          format: wgpu::VertexFormat::Float32x4,
        },
        // effect_extra
        wgpu::VertexAttribute {
          offset: 96,
          shader_location: 8,
          format: wgpu::VertexFormat::Float32x4,
        },
        // uv_bounds
        wgpu::VertexAttribute {
          offset: 112,
          shader_location: 9,
          format: wgpu::VertexFormat::Float32x4,
        },
      ],
    };

    // Base pipeline (no effects, just textured sprites)
    let base_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some("Sprite Base Pipeline"),
      layout: Some(&pipeline_layout),
      vertex: wgpu::VertexState {
        module: &shader,
        entry_point: "vs_main",
        buffers: &[vertex_layout.clone(), instance_layout.clone()],
        compilation_options: Default::default(),
      },
      fragment: Some(wgpu::FragmentState {
        module: &shader,
        entry_point: "fs_base",
        targets: &[Some(wgpu::ColorTargetState {
          format: surface_format,
          blend: Some(wgpu::BlendState::ALPHA_BLENDING),
          write_mask: wgpu::ColorWrites::ALL,
        })],
        compilation_options: Default::default(),
      }),
      primitive: wgpu::PrimitiveState {
        topology: wgpu::PrimitiveTopology::TriangleList,
        ..Default::default()
      },
      depth_stencil: None,
      multisample: wgpu::MultisampleState::default(),
      multiview: None,
      cache: None,
    });

    // Effect pipeline (applies per-sprite effects)
    let effect_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some("Sprite Effect Pipeline"),
      layout: Some(&pipeline_layout),
      vertex: wgpu::VertexState {
        module: &shader,
        entry_point: "vs_main",
        buffers: &[vertex_layout.clone(), instance_layout.clone()],
        compilation_options: Default::default(),
      },
      fragment: Some(wgpu::FragmentState {
        module: &shader,
        entry_point: "fs_effect",
        targets: &[Some(wgpu::ColorTargetState {
          format: surface_format,
          blend: Some(wgpu::BlendState::ALPHA_BLENDING),
          write_mask: wgpu::ColorWrites::ALL,
        })],
        compilation_options: Default::default(),
      }),
      primitive: wgpu::PrimitiveState {
        topology: wgpu::PrimitiveTopology::TriangleList,
        ..Default::default()
      },
      depth_stencil: None,
      multisample: wgpu::MultisampleState::default(),
      multiview: None,
      cache: None,
    });

    Self {
      base_pipeline,
      effect_pipeline,
      custom: CustomPipelines {
        layout: pipeline_layout,
        vertex: vertex_layout,
        instance: instance_layout,
        format: surface_format,
        entries: HashMap::new(),
        warned: HashMap::new(),
      },
      globals_buffer,
      globals_bind_group,
      instance_buffer,
      instance_capacity: Self::MAX_INSTANCES,
      vertex_buffer,
      atlas: TextureAtlas::new(device, texture_layout, sampler),
      textures: HashMap::new(),
      entity_sprites: HashMap::new(),
      frame: 0,
      placed: Vec::new(),
      world_runs: Vec::new(),
      resident_scratch: HashSet::new(),
      time: 0.0,
    }
  }

  fn create_instance_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
      label: Some("Sprite Instance Buffer"),
      size: (capacity * std::mem::size_of::<SpriteInstance>()) as u64,
      usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
      mapped_at_creation: false,
    })
  }

  /// Update time (called each frame)
  pub fn update(&mut self, dt: f32) {
    self.time += dt;
  }

  /// Update globals for current frame
  pub fn set_viewport(&mut self, queue: &wgpu::Queue, width: f32, height: f32) {
    let globals = SpriteGlobals::new(width, height, self.time);
    queue.write_buffer(&self.globals_buffer, 0, bytemuck::cast_slice(&[globals]));
  }

  /// Upload a sprite texture to the GPU (cached by id).
  ///
  /// `data` is RGBA8 in sRGB. The texture goes into the texture atlas;
  /// uploading an id that is already cached does nothing. Data that doesn't
  /// match `width`x`height` is ignored.
  pub fn upload_texture(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    id: &str,
    width: u32,
    height: u32,
    data: &[u8],
  ) {
    if self.textures.contains_key(id) {
      return;
    }
    let key = AtlasKey::Id(id.to_string());
    let placed = self.atlas.slot(
      device,
      queue,
      &key,
      width,
      height,
      || data.to_vec(),
      self.frame,
    );
    if placed.is_some() {
      let source = TextureSource::Pixels(data.to_vec());
      let texture = CachedTexture {
        key,
        width,
        height,
        source,
      };
      self.textures.insert(id.to_string(), texture);
    }
  }

  /// Make the textures `ids` resident for this frame, which draws `sprites`
  /// instances: re-upload what the atlas evicted, mark them used, grow the
  /// instance buffer and run the atlas housekeeping.
  fn make_resident<'a>(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    ids: impl Iterator<Item = &'a str>,
    sprites: usize,
  ) {
    for id in ids {
      let Some(texture) = self.textures.get(id) else {
        continue;
      };
      let rgba = || texture.rgba();
      let (width, height) = (texture.width, texture.height);
      let atlas_key = &texture.key;
      self
        .atlas
        .slot(device, queue, atlas_key, width, height, rgba, self.frame);
    }
    self.atlas.end_frame(self.frame);
    self.prune_entity_sprites();
    self.frame += 1;

    if sprites > self.instance_capacity {
      self.instance_capacity = sprites.next_power_of_two();
      self.instance_buffer = Self::create_instance_buffer(device, self.instance_capacity);
    }
  }

  /// Lay out the z-sorted draw list's sprites for
  /// [`draw_world_run`](Self::draw_world_run).
  ///
  /// Makes the list's textures resident, remaps its sprites into their atlas slots in
  /// draw order and splits the sorted items into draws (sprites merge only
  /// when consecutive on one atlas page and pipeline; trail groups split
  /// them). `list` must be sorted ([`DrawList::sorted`]). Call once per frame,
  /// after [`set_viewport`](Self::set_viewport).
  pub(crate) fn prepare_world(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    list: &DrawList,
  ) {
    let mut seen = std::mem::take(&mut self.resident_scratch);
    seen.clear();
    let textures = (0..list.sprite_count())
      .filter_map(|i| list.sprite(i))
      .map(|sprite| &sprite.texture)
      .filter(|texture| seen.insert(Arc::clone(texture)))
      .map(|texture| &**texture);
    self.make_resident(device, queue, textures, list.sprite_count());
    self.resident_scratch = seen;

    self.placed.clear();
    let mut runs = std::mem::take(&mut self.world_runs);
    plan_runs(list.items(), &mut runs, |index| {
      let sprite = list.sprite(index)?;
      let texture = self.textures.get(&*sprite.texture)?;
      let slot = self.atlas.lookup(&texture.key)?;
      let page = self.atlas.texture(slot.page)?;
      let page_size = (page.width(), page.height());
      let pipeline = match sprite.shading {
        SpriteShading::Plain => PipelineKind::Plain,
        SpriteShading::Effect => PipelineKind::Effect,
        SpriteShading::Custom(serial) if self.custom.pipeline(serial).is_some() => {
          PipelineKind::Custom(serial)
        }
        SpriteShading::Custom(_) => PipelineKind::Plain,
      };
      // Only built-in effects grow the quad; custom params are not effect ids
      let effects = pipeline == PipelineKind::Effect;
      self
        .placed
        .push(place_in_slot(&sprite.instance, &slot, page_size, effects));
      Some((slot.page, pipeline))
    });
    self.world_runs = runs;
  }

  /// Compile (or reuse) the game shader and pack `params` into `instance`.
  ///
  /// Returns the serial to draw the sprite with
  /// ([`SpriteShading::Custom`]), or `None` when the shader failed, in which
  /// case the sprite draws plain. Call at emit time, so nothing per-sprite
  /// has to be kept until the frame is laid out.
  pub(crate) fn shade_custom(
    &mut self,
    device: &wgpu::Device,
    shader: &ShaderHandle,
    params: &ShaderParams,
    instance: &mut SpriteInstance,
  ) -> Option<u64> {
    if !self.custom.prepare(device, shader, self.frame) {
      return None;
    }
    self.custom.pack(shader, params, instance);
    Some(shader.serial())
  }

  /// The draws [`prepare_world`](Self::prepare_world) laid out, in order.
  pub(crate) fn world_runs(&self) -> &[WorldRun] {
    &self.world_runs
  }

  /// Number of sprite draws [`draw_world_run`](Self::draw_world_run) issues
  /// for the laid-out frame.
  pub(crate) fn world_draw_count(&self) -> usize {
    self
      .world_runs
      .iter()
      .filter(|run| matches!(run, WorldRun::Sprites { .. }))
      .count()
  }

  /// Upload the laid-out instances. Call before the render pass that draws
  /// them with [`draw_world_run`](Self::draw_world_run).
  pub(crate) fn upload_world(&self, queue: &wgpu::Queue) {
    if !self.placed.is_empty() {
      queue.write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(&self.placed));
    }
  }

  /// Draw one sprite run inside a render pass, after
  /// [`upload_world`](Self::upload_world). Does nothing for trail runs.
  pub(crate) fn draw_world_run<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, run: &WorldRun) {
    let WorldRun::Sprites {
      page,
      pipeline,
      start,
      count,
    } = *run
    else {
      return;
    };
    let Some(bind_group) = self.atlas.bind_group(page) else {
      return;
    };
    let pipeline = match pipeline {
      PipelineKind::Plain => &self.base_pipeline,
      PipelineKind::Effect => &self.effect_pipeline,
      PipelineKind::Custom(serial) => self.custom.pipeline(serial).unwrap_or(&self.base_pipeline),
    };
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, &self.globals_bind_group, &[]);
    pass.set_bind_group(1, bind_group, &[]);
    pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
    let instance_size = std::mem::size_of::<SpriteInstance>() as u64;
    let offset = start as u64 * instance_size;
    let end = offset + count as u64 * instance_size;
    pass.set_vertex_buffer(1, self.instance_buffer.slice(offset..end));
    pass.draw(0..6, 0..count as u32);
  }

  /// Where a cached texture currently lives in the atlas.
  #[cfg(test)]
  pub(crate) fn lookup(&self, id: &str) -> Option<AtlasSlot> {
    let texture = self.textures.get(id)?;
    self.atlas.lookup(&texture.key)
  }

  /// Upload a sprite from a SpriteHandle to the GPU texture cache
  ///
  /// Convenience method that extracts data from SpriteHandle.
  /// Uses the sprite's ID as the cache key.
  pub fn upload_sprite(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    sprite: &SpriteHandle,
  ) {
    let id = sprite.id();
    if self.textures.contains_key(&id) {
      return;
    }
    let (width, height, data) = sprite.with_sprite(|s| (s.width(), s.height(), s.data().to_vec()));
    let key = AtlasKey::Id(id.clone());
    let placed = self
      .atlas
      .slot(device, queue, &key, width, height, || data, self.frame);
    if placed.is_some() {
      let texture = CachedTexture {
        key,
        width,
        height,
        source: TextureSource::Sprite(sprite.clone()),
      };
      self.textures.insert(id, texture);
    }
  }

  /// Make an entity's sprite resident and return the texture id to draw it by.
  ///
  /// Sprites are keyed by the handle's identity and content generation, so
  /// every entity sharing a handle shares one texture, and a hot-reloaded
  /// sprite is uploaded again. Returns `None` for a sprite that cannot be
  /// uploaded (empty, or larger than the device allows).
  pub(crate) fn upload_entity_sprite(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    sprite: &SpriteHandle,
  ) -> Option<TextureRef> {
    let serial = sprite.serial();
    let generation = sprite.generation();
    if let Some(entry) = self.entity_sprites.get_mut(&serial)
      && entry.generation == generation
    {
      entry.last_used = self.frame;
      return entry.uploaded.then(|| entry.texture.clone());
    }

    // New, or its pixels changed: read the size and generation under the
    // read lock, so they belong together; the pixels are copied only if the
    // atlas needs them
    let (generation, width, height) =
      sprite.with_sprite(|s| (sprite.generation(), s.width(), s.height()));
    let key = AtlasKey::Sprite {
      handle: serial,
      generation,
    };
    let uploaded = self
      .atlas
      .slot(
        device,
        queue,
        &key,
        width,
        height,
        || sprite.with_sprite(|s| s.data().to_vec()),
        self.frame,
      )
      .is_some();

    let texture = match self.entity_sprites.remove(&serial) {
      Some(old) => {
        // The superseded version's rect is free for reuse
        self.atlas.remove(&AtlasKey::Sprite {
          handle: serial,
          generation: old.generation,
        });
        old.texture
      }
      None => TextureRef::from(format!("\u{1}sprite:{serial}")),
    };
    if uploaded {
      self.textures.insert(
        texture.to_string(),
        CachedTexture {
          key,
          width,
          height,
          source: TextureSource::Sprite(sprite.clone()),
        },
      );
    } else {
      self.textures.remove(&*texture);
    }
    self.entity_sprites.insert(
      serial,
      EntitySprite {
        generation,
        texture: texture.clone(),
        uploaded,
        last_used: self.frame,
      },
    );
    uploaded.then_some(texture)
  }

  /// Forget entity sprites nothing asked for in a while, so sprites of
  /// despawned entities don't stay in memory.
  fn prune_entity_sprites(&mut self) {
    self.cap_standalone_entity_sprites();
    if !self.frame.is_multiple_of(120) {
      return;
    }
    self.custom.prune(self.frame);
    let frame = self.frame;
    let stale: Vec<usize> = self
      .entity_sprites
      .iter()
      .filter(|(_, e)| frame.saturating_sub(e.last_used) >= ENTITY_SPRITE_TTL)
      .map(|(serial, _)| *serial)
      .collect();
    for serial in stale {
      if let Some(entry) = self.entity_sprites.remove(&serial) {
        self.textures.remove(&*entry.texture);
        self.atlas.remove(&AtlasKey::Sprite {
          handle: serial,
          generation: entry.generation,
        });
      }
    }
  }

  /// Free the least recently used standalone entity textures beyond
  /// [`MAX_STANDALONE_ENTITY_SPRITES`], sparing any used this frame.
  fn cap_standalone_entity_sprites(&mut self) {
    if self.entity_sprites.len() <= MAX_STANDALONE_ENTITY_SPRITES {
      return;
    }
    let mut standalone = self.standalone_entity_serials();
    let Some(excess) = standalone
      .len()
      .checked_sub(MAX_STANDALONE_ENTITY_SPRITES)
      .filter(|n| *n > 0)
    else {
      return;
    };
    standalone.sort_by_key(|&(_, last_used)| last_used);
    let frame = self.frame;
    let evict = standalone
      .into_iter()
      .filter(|&(_, last_used)| last_used < frame)
      .take(excess);
    let evict: Vec<usize> = evict.map(|(serial, _)| serial).collect();
    for serial in evict {
      if let Some(entry) = self.entity_sprites.remove(&serial) {
        self.textures.remove(&*entry.texture);
        self.atlas.remove(&AtlasKey::Sprite {
          handle: serial,
          generation: entry.generation,
        });
      }
    }
  }

  /// Entity sprites holding a standalone texture, as `(serial, last_used)`.
  fn standalone_entity_serials(&self) -> Vec<(usize, u64)> {
    self
      .entity_sprites
      .iter()
      .filter(|(serial, e)| {
        e.uploaded
          && self
            .atlas
            .lookup(&AtlasKey::Sprite {
              handle: **serial,
              generation: e.generation,
            })
            .is_some_and(|slot| matches!(slot.page, SlotPage::Standalone(_)))
      })
      .map(|(serial, e)| (*serial, e.last_used))
      .collect()
  }

  /// How many textures are cached (for tests that watch for leaks).
  #[cfg(test)]
  pub(crate) fn texture_count(&self) -> usize {
    self.textures.len()
  }

  /// How many entity sprites hold a standalone texture.
  #[cfg(test)]
  pub(crate) fn standalone_entity_count(&self) -> usize {
    self.standalone_entity_serials().len()
  }

  /// Whether the entity sprite is resident (for tests of what gets uploaded).
  #[cfg(test)]
  pub(crate) fn has_entity_sprite(&self, sprite: &SpriteHandle) -> bool {
    self
      .entity_sprites
      .get(&sprite.serial())
      .is_some_and(|e| e.uploaded)
  }

  /// Check if a texture is already cached
  pub fn has_texture(&self, id: &str) -> bool {
    self.textures.contains_key(id)
  }

  /// Clear the texture cache
  pub fn clear_cache(&mut self) {
    self.textures.clear();
    self.entity_sprites.clear();
    self.atlas.clear();
    self.placed.clear();
    self.world_runs.clear();
  }
}

/// Atlas padding (texels on every side) of the effect in `params`: outline,
/// glow and directional outline grow the sprite as the CPU renderer did.
pub(crate) fn effect_padding(params: &[f32; 4]) -> f32 {
  match params[0] as i32 {
    // Outline, DirectionalOutline: thickness
    1 | 11 => params[1].max(0.0),
    // Glow: the radius in whole texels, at least 1
    2 => params[1].floor().max(1.0),
    _ => 0.0,
  }
}

/// Map an instance into its atlas slot.
///
/// `uv_rect` (normalized over the texture) is remapped into the slot and
/// `uv_bounds` becomes the sprite's own rect, clipped to the slot. When
/// `effects` is set and the effect grows the sprite, the quad and its UVs are
/// enlarged by the effect's padding around the same centre, so the effect can
/// draw outside the sprite. `page_size` is the size of the slot's texture.
fn place_in_slot(
  instance: &SpriteInstance,
  slot: &AtlasSlot,
  page_size: (u32, u32),
  effects: bool,
) -> SpriteInstance {
  let [s0, t0, s1, t1] = slot.uv;
  let [u0, v0, u1, v1] = instance.uv_rect;
  let (du, dv) = (s1 - s0, t1 - t0);
  let mut uv = [s0 + u0 * du, t0 + v0 * dv, s0 + u1 * du, t0 + v1 * dv];
  let mut out = *instance;
  out.uv_bounds = [
    uv[0].min(uv[2]).max(s0),
    uv[1].min(uv[3]).max(t0),
    uv[0].max(uv[2]).min(s1),
    uv[1].max(uv[3]).min(t1),
  ];

  let pad = if effects {
    effect_padding(&instance.effect_params)
  } else {
    0.0
  };
  let texel_u = 1.0 / page_size.0.max(1) as f32;
  let texel_v = 1.0 / page_size.1.max(1) as f32;
  // Sprite size in texels
  let width = (uv[2] - uv[0]).abs() / texel_u;
  let height = (uv[3] - uv[1]).abs() / texel_v;
  if pad > 0.0 && width > 0.0 && height > 0.0 {
    let grow_x = (width + 2.0 * pad) / width;
    let grow_y = (height + 2.0 * pad) / height;
    for c in 0..2 {
      out.transform[0][c] *= grow_x;
      out.transform[1][c] *= grow_y;
    }
    // Grow outward, also when the UVs are flipped
    let pad_u = (uv[2] - uv[0]).signum() * pad * texel_u;
    let pad_v = (uv[3] - uv[1]).signum() * pad * texel_v;
    uv[0] -= pad_u;
    uv[2] += pad_u;
    uv[1] -= pad_v;
    uv[3] += pad_v;
  }
  out.uv_rect = uv;
  out
}

/// The shader that applies to a sprite: its `Material`'s, else the inline one.
pub(crate) fn effective_shader<'a>(
  material: Option<&'a Material>,
  inline_shader: Option<&'a SpriteShader>,
) -> Option<&'a SpriteShader> {
  material.map(|m| &m.shader).or(inline_shader)
}

/// Get the shader effect from a Material or inline shader
pub fn get_sprite_effect<'a>(
  material: Option<&'a Material>,
  inline_shader: Option<&'a SpriteShader>,
) -> Option<&'a SpriteEffect> {
  effective_shader(material, inline_shader).and_then(SpriteShader::as_builtin)
}

/// The WGSL shader for sprite rendering (loaded from external file)
pub const SPRITE_SHADER: &str = include_str!("../shaders/core/sprite.wgsl");

// =============================================================================
// Sprite Batching System
// =============================================================================

/// Collects the frame's `render_gpu`, particle and parallax sprites.
///
/// Sprites are kept in submission order and join the frame's z-sorted draw
/// list, where equal-z sprites draw in submission order:
///
/// ```ignore
/// let mut collector = BatchCollector::new();
/// collector.add(sprite.id(), has_effect, instance, z_order);
/// ```
#[derive(Debug, Default)]
pub struct BatchCollector {
  /// Every sprite in submission order
  items: Vec<CollectedSprite>,
  /// Shared texture ids, so sprites of one texture hold one allocation.
  /// Kept across frames; it only grows with the distinct textures used.
  textures: HashMap<String, TextureRef>,
}

/// One collected sprite, as the draw list takes it.
#[derive(Debug)]
struct CollectedSprite {
  texture: TextureRef,
  shading: SpriteShading,
  z_order: i32,
  instance: SpriteInstance,
}

impl BatchCollector {
  /// Create a new, empty collector.
  pub fn new() -> Self {
    Self::default()
  }

  /// Create a collector with room for `sprites` sprites.
  pub fn with_capacity(sprites: usize) -> Self {
    Self {
      items: Vec::with_capacity(sprites),
      ..Self::default()
    }
  }

  /// Add a sprite. `texture_id` is the id the texture was uploaded under.
  pub fn add(
    &mut self,
    texture_id: impl AsRef<str>,
    has_effects: bool,
    instance: SpriteInstance,
    z_order: i32,
  ) {
    let shading = if has_effects {
      SpriteShading::Effect
    } else {
      SpriteShading::Plain
    };
    self.add_shaded(texture_id, shading, instance, z_order);
  }

  /// Add a sprite with its shading already decided.
  pub(crate) fn add_shaded(
    &mut self,
    texture_id: impl AsRef<str>,
    shading: SpriteShading,
    instance: SpriteInstance,
    z_order: i32,
  ) {
    let id = texture_id.as_ref();
    let texture = match self.textures.get(id) {
      Some(texture) => texture.clone(),
      None => {
        let texture = TextureRef::from(id);
        self.textures.insert(id.to_string(), texture.clone());
        texture
      }
    };
    self.items.push(CollectedSprite {
      texture,
      shading,
      z_order,
      instance,
    });
  }

  /// Push every collected sprite onto `list` as `kind`, in submission order.
  pub(crate) fn push_to(&self, list: &mut DrawList, kind: DrawKind) {
    for item in &self.items {
      list.push_sprite(
        kind,
        item.z_order,
        item.texture.clone(),
        item.shading,
        item.instance,
      );
    }
  }

  /// Get the total number of sprites collected
  pub fn sprite_count(&self) -> usize {
    self.items.len()
  }

  /// Forget this frame's sprites, keeping the allocations.
  pub fn clear(&mut self) {
    self.items.clear();
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::graphics::SlotPage;

  #[test]
  fn sprite_instance_size() {
    // Verify instance size matches expected (for GPU buffer layout)
    assert_eq!(std::mem::size_of::<SpriteInstance>(), 128);
  }

  #[test]
  fn sprite_globals_size() {
    assert_eq!(std::mem::size_of::<SpriteGlobals>(), 80);
  }

  #[test]
  fn sprite_instance_default() {
    let instance = SpriteInstance::new();
    assert_eq!(instance.transform[0][0], 1.0);
    assert_eq!(instance.transform[1][1], 1.0);
    assert_eq!(instance.uv_rect, [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(instance.tint, [1.0, 1.0, 1.0, 1.0]);
  }

  #[test]
  fn sprite_instance_with_transform() {
    let instance = SpriteInstance::new().with_transform(100.0, 200.0, 0.0, 2.0, 3.0);
    // Column-major layout: translation is in column 2
    assert_eq!(instance.transform[2][0], 100.0); // x translation
    assert_eq!(instance.transform[2][1], 200.0); // y translation
    assert_eq!(instance.transform[0][0], 2.0); // scale_x (column 0, row 0)
    assert_eq!(instance.transform[1][1], 3.0); // scale_y (column 1, row 1)
  }

  #[test]
  fn sprite_instance_with_effect_flash() {
    let effect = SpriteEffect::flash(Color::RED, 0.8);
    let instance = SpriteInstance::new().with_effect(&effect);
    assert_eq!(instance.effect_params[0], 3.0); // Flash effect type
    assert_eq!(instance.effect_params[1], 0.8); // intensity
  }

  #[test]
  fn directional_outline_packs_both_colours() {
    let effect =
      SpriteEffect::directional_outline(Color::new(1, 2, 3, 4), Color::new(120, 121, 130, 255), 2);
    let instance = SpriteInstance::new().with_effect(&effect);
    assert_eq!(
      instance.effect_params,
      [11.0, 2.0, 120.0 * 256.0 + 121.0, 130.0 * 256.0 + 255.0]
    );
    assert_eq!(
      instance.effect_extra,
      [1.0 / 255.0, 2.0 / 255.0, 3.0 / 255.0, 4.0 / 255.0]
    );
  }

  // =========================================================================
  // Atlas placement tests
  // =========================================================================

  const PAGE: (u32, u32) = (2048, 2048);

  fn texels(v: [f32; 4]) -> [f32; 4] {
    v.map(|c| c * 2048.0)
  }

  /// A 4x4 image at texel (8, 8) of a 2048 page.
  fn slot_4x4() -> AtlasSlot {
    AtlasSlot {
      page: SlotPage::Atlas(0),
      uv: [8.0, 8.0, 12.0, 12.0].map(|c| c / 2048.0),
    }
  }

  #[test]
  fn uvs_are_remapped_into_the_slot() {
    let sub = SpriteInstance::new().with_uv(0.5, 0.0, 1.0, 0.5);
    let placed = place_in_slot(&sub, &slot_4x4(), PAGE, false);
    assert_eq!(texels(placed.uv_rect), [10.0, 8.0, 12.0, 10.0]);
    assert_eq!(placed.uv_bounds, placed.uv_rect);
    assert_eq!(placed.transform, sub.transform);
  }

  #[test]
  fn flipped_uvs_keep_ordered_bounds() {
    let flipped = SpriteInstance::new().with_uv(1.0, 0.0, 0.0, 1.0);
    let placed = place_in_slot(&flipped, &slot_4x4(), PAGE, false);
    assert_eq!(texels(placed.uv_rect), [12.0, 8.0, 8.0, 12.0]);
    assert_eq!(texels(placed.uv_bounds), [8.0, 8.0, 12.0, 12.0]);
  }

  #[test]
  fn bounds_are_clipped_to_the_slot() {
    let beyond = SpriteInstance::new().with_uv(-1.0, 0.0, 2.0, 1.0);
    let placed = place_in_slot(&beyond, &slot_4x4(), PAGE, false);
    assert_eq!(texels(placed.uv_bounds), [8.0, 8.0, 12.0, 12.0]);
  }

  #[test]
  fn growing_effects_enlarge_quad_and_uvs_around_the_centre() {
    let outlined = SpriteInstance::new()
      .with_transform(32.0, 32.0, 0.0, 16.0, 16.0)
      .with_effect(&SpriteEffect::outline(Color::WHITE, 2));
    let placed = place_in_slot(&outlined, &slot_4x4(), PAGE, true);
    // 4 texels + 2 on each side = 8 texels, 4 px each
    assert_eq!(placed.transform[0][0], 32.0);
    assert_eq!(placed.transform[1][1], 32.0);
    assert_eq!(placed.transform[2], outlined.transform[2]);
    assert_eq!(texels(placed.uv_rect), [6.0, 6.0, 14.0, 14.0]);
    assert_eq!(texels(placed.uv_bounds), [8.0, 8.0, 12.0, 12.0]);
  }

  #[test]
  fn growth_follows_flipped_uvs() {
    let outlined = SpriteInstance::new()
      .with_uv(1.0, 0.0, 0.0, 1.0)
      .with_effect(&SpriteEffect::outline(Color::WHITE, 1));
    let placed = place_in_slot(&outlined, &slot_4x4(), PAGE, true);
    assert_eq!(texels(placed.uv_rect), [13.0, 7.0, 7.0, 13.0]);
  }

  #[test]
  fn nothing_grows_without_the_effect_pipeline_or_a_growing_effect() {
    let outlined = SpriteInstance::new().with_effect(&SpriteEffect::outline(Color::WHITE, 2));
    let base = place_in_slot(&outlined, &slot_4x4(), PAGE, false);
    assert_eq!(texels(base.uv_rect), [8.0, 8.0, 12.0, 12.0]);
    let flashed = SpriteInstance::new().with_effect(&SpriteEffect::flash(Color::WHITE, 1.0));
    let flash = place_in_slot(&flashed, &slot_4x4(), PAGE, true);
    assert_eq!(flash.transform, flashed.transform);
  }

  #[test]
  fn effect_padding_matches_the_cpu_growth() {
    let pad =
      |e: SpriteEffect| effect_padding(&SpriteInstance::new().with_effect(&e).effect_params);
    assert_eq!(pad(SpriteEffect::outline(Color::WHITE, 3)), 3.0);
    let directional = SpriteEffect::directional_outline(Color::WHITE, Color::BLACK, 2);
    assert_eq!(pad(directional), 2.0);
    // The CPU grows a glow by `radius as usize`, at least 1
    assert_eq!(
      pad(SpriteEffect::glow_with_intensity(Color::WHITE, 2.7, 1.0)),
      2.0
    );
    assert_eq!(pad(SpriteEffect::glow(Color::WHITE, 1.0)), 1.0);
    assert_eq!(pad(SpriteEffect::invert()), 0.0);
  }

  fn world_list(entries: &[(&str, i32)]) -> DrawList {
    let mut collector = BatchCollector::new();
    for (id, z) in entries {
      collector.add(id, false, SpriteInstance::new(), *z);
    }
    let mut list = DrawList::default();
    collector.push_to(&mut list, DrawKind::Sprite);
    list.sorted();
    list
  }

  #[test]
  fn prepare_world_reuploads_evicted_textures() {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let mut renderer = SpriteGpuRenderer::new(&device, &queue, format);
    renderer.upload_texture(&device, &queue, "a", 2, 2, &[255; 16]);
    // As if the atlas had evicted it
    renderer.atlas.clear();
    assert!(renderer.lookup("a").is_none());

    renderer.prepare_world(&device, &queue, &world_list(&[("a", 0)]));
    assert!(renderer.lookup("a").is_some());
  }

  #[test]
  fn mismatched_texture_data_is_not_cached() {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let mut renderer = SpriteGpuRenderer::new(&device, &queue, format);
    renderer.upload_texture(&device, &queue, "short", 2, 2, &[255; 15]);
    assert!(!renderer.has_texture("short"));
  }

  #[test]
  fn prepare_world_grows_the_instance_buffer() {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let mut renderer = SpriteGpuRenderer::new(&device, &queue, format);
    let entries = vec![("a", 0); SpriteGpuRenderer::MAX_INSTANCES + 1];
    renderer.prepare_world(&device, &queue, &world_list(&entries));
    assert_eq!(renderer.instance_capacity, 16384);
    let size = 16384 * std::mem::size_of::<SpriteInstance>() as u64;
    assert_eq!(renderer.instance_buffer.size(), size);
  }

  #[test]
  fn shading_is_copy_so_sprites_carry_no_heap_data() {
    fn assert_copy<T: Copy>() {}
    assert_copy::<SpriteShading>();
    assert!(std::mem::size_of::<SpriteShading>() <= 16);
  }

  #[test]
  fn packing_correct_names_reports_nothing() {
    let src = "struct Params { a: f32 }\nfn sprite(in: SpriteIn, params: Params) -> vec4<f32> { return vec4<f32>(params.a); }";
    let layout = custom::compile(ShaderKind::Sprite, "pack.wgsl", src)
      .map(|c| c.layout)
      .unwrap_or_else(|e| panic!("{e}"));
    let mut bytes = [0u8; 32];
    let mut problems = 0;
    let params = ShaderParams::new().with("a", 2.0);
    custom::pack_with(&layout, &params, &mut bytes, |_, _| problems += 1);
    assert_eq!(problems, 0);
    assert_eq!(&bytes[..4], &2.0f32.to_le_bytes());

    let params = params.with("typo", 1.0);
    custom::pack_with(&layout, &params, &mut bytes, |_, _| problems += 1);
    assert_eq!(problems, 1);
  }

  #[test]
  fn unused_custom_shaders_are_forgotten() {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let mut renderer = SpriteGpuRenderer::new(&device, &queue, format);
    let src = "fn sprite(in: SpriteIn, params: Params) -> vec4<f32> { return sample(in.uv); }";
    let shader = ShaderHandle::from_source("prune.wgsl", src);
    let mut instance = SpriteInstance::new();
    let serial = renderer.shade_custom(&device, &shader, &ShaderParams::new(), &mut instance);
    assert_eq!(serial, Some(shader.serial()));
    assert_eq!(renderer.custom.entries.len(), 1);

    renderer.custom.prune(CUSTOM_SHADER_TTL - 1);
    assert_eq!(renderer.custom.entries.len(), 1);
    renderer.custom.prune(CUSTOM_SHADER_TTL);
    assert!(renderer.custom.entries.is_empty());
  }

  // =========================================================================
  // Collector tests
  // =========================================================================

  #[test]
  fn collector_keeps_submission_order_and_counts() {
    let mut collector = BatchCollector::new();
    collector.add("tex_c", false, SpriteInstance::new(), 100);
    collector.add("tex_a", true, SpriteInstance::new(), 0);
    collector.add(String::from("tex_a"), false, SpriteInstance::new(), 50);
    assert_eq!(collector.sprite_count(), 3);

    let mut list = DrawList::default();
    collector.push_to(&mut list, DrawKind::Sprite);
    // One shared allocation per texture id
    assert!(Arc::ptr_eq(
      &list.sprite(1).unwrap().texture,
      &list.sprite(2).unwrap().texture
    ));
    collector.clear();
    assert_eq!(collector.sprite_count(), 0);
  }
}
