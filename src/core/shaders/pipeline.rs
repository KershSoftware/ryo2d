//! Post-processing GPU pipeline
//!
//! Manages GPU resources for rendering post-processing effects.
//! Uses ping-pong buffers for effect chaining.

use std::sync::Arc;

use super::cache::ShaderCache;
use super::compile::{self, Compiler};
use super::custom::{CompiledShader, ShaderError, ShaderKind};
use super::effects::{EffectParams, GodRaysParams, PostProcessEffect};
use super::manager::CustomEffect;
use super::post_process_shaders;
use super::scope::Checked;
use crate::core::asset::{AssetId, Assets};
use crate::core::ecs::{Shader, ShaderParams};
use crate::core::graphics::bind_groups::{BindGroupCache, Bound};

/// Which buffer contains the effect output
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputBuffer {
  A,
  B,
}

/// Uniform buffers handed out one per post-process pass within a frame.
///
/// `queue.write_buffer` calls all land before the frame's commands run, so
/// passes sharing one buffer would all read the last value written.
struct UniformPool {
  label: &'static str,
  size: u64,
  buffers: Vec<wgpu::Buffer>,
  next: usize,
}

impl UniformPool {
  fn new(label: &'static str, size: u64) -> Self {
    Self {
      label,
      size,
      buffers: Vec::new(),
      next: 0,
    }
  }

  /// Start a frame: buffers are handed out again from the first.
  fn reset(&mut self) {
    self.next = 0;
  }

  /// Write `data` into a buffer no other pass uses this frame (creating one
  /// if the pool is exhausted) and return its index.
  fn write(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, data: &[u8]) -> usize {
    if self.next == self.buffers.len() {
      self
        .buffers
        .push(device.create_buffer(&wgpu::BufferDescriptor {
          label: Some(self.label),
          size: self.size,
          usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
          mapped_at_creation: false,
        }));
    }
    let index = self.next;
    self.next += 1;
    queue.write_buffer(&self.buffers[index], 0, data);
    index
  }

  fn buffer(&self, index: usize) -> &wgpu::Buffer {
    &self.buffers[index]
  }
}

/// Largest `Params` uniform a custom effect can have (see `custom::compile`)
/// Bloom's blur sigma per unit of radius, in screen pixels: the spread of the
/// original 9-tap kernel (sigma 1.69 taps, taps `radius` pixels apart)
const BLOOM_SIGMA_PER_RADIUS: f32 = 1.7;

const CUSTOM_PARAMS_BYTES: u64 = 64;
/// Custom effect globals: `time_resolution: vec4<f32>`
const CUSTOM_GLOBALS_BYTES: u64 = 16;
/// Compiled effect sources no effect has used for this many frames are
/// forgotten; an effect shader's own entry, and its pipeline, go when it is
/// freed
const CUSTOM_EFFECT_TTL: u64 = 600;
/// How often freed effect shaders whose events were missed, and unused
/// compiled sources, are looked for
const CUSTOM_EFFECT_PRUNE_EVERY: u64 = 120;

/// Pipelines of game effects, built on first use and rebuilt when the shader's
/// source changes.
struct CustomEffectPipelines {
  bind_group_layout: wgpu::BindGroupLayout,
  pipeline_layout: wgpu::PipelineLayout,
  format: wgpu::TextureFormat,
  /// The effects' shaders, and each source's pipeline while an effect uses it
  /// (pending until the GPU's check comes back; a failed one is not rebuilt)
  cache: ShaderCache<Checked<wgpu::RenderPipeline>>,
  /// Makes the new versions of effects that already run, while the old
  /// ones run
  compiler: Compiler<EffectCompiled>,
}

/// What the shader thread made of an effect's new version
struct EffectCompiled {
  /// Its name, which labels what is made from it
  name: String,
  compiled: Result<Arc<CompiledShader>, ShaderError>,
  /// Its pipeline, or the GPU's message rejecting it; none if it does not
  /// compile
  pipeline: Option<Result<wgpu::RenderPipeline, String>>,
}

impl CustomEffectPipelines {
  fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
    let uniform = |binding| wgpu::BindGroupLayoutEntry {
      binding,
      visibility: wgpu::ShaderStages::FRAGMENT,
      ty: wgpu::BindingType::Buffer {
        ty: wgpu::BufferBindingType::Uniform,
        has_dynamic_offset: false,
        min_binding_size: None,
      },
      count: None,
    };
    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
      label: Some("Custom Effect Bind Group Layout"),
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
        uniform(2),
        uniform(3),
      ],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("Custom Effect Pipeline Layout"),
      bind_group_layouts: &[Some(&bind_group_layout)],
      immediate_size: 0,
    });
    Self {
      bind_group_layout,
      pipeline_layout,
      format,
      cache: ShaderCache::new(ShaderKind::Effect),
      compiler: Compiler::new(),
    }
  }

  /// Make sure the cache holds the shader's current version and its
  /// pipeline; the key to run it with ([`pipeline`](Self::pipeline)), or
  /// `None` while it is not loaded, failed, or waits on the GPU's check.
  fn prepare(
    &mut self,
    device: &wgpu::Device,
    shader: AssetId<Shader>,
    shaders: &Assets<Shader>,
    frame: u64,
  ) -> Option<u64> {
    let (key, compiled) = self.cache.prepare(shader, shaders, frame)?;
    let ready = compiled.shader.as_ref().ok()?;
    let state = match &mut compiled.made {
      Some(state) => state,
      made => made.insert(build(
        device,
        &self.pipeline_layout,
        self.format,
        &compiled.name,
        ready,
      )),
    };
    state.ready().is_some().then_some(key)
  }

  /// Forget the effect shaders freed since the last frame (`removed`) at
  /// once, with the pipelines no effect uses any more; now and then check
  /// every one against `shaders`, and forget unused compiled sources. Then
  /// the new versions made off the main thread take over, before anything
  /// runs, and the next one wanted is sent
  fn sync(
    &mut self,
    device: &wgpu::Device,
    removed: &[AssetId<Shader>],
    shaders: &Assets<Shader>,
    frame: u64,
  ) {
    for &shader in removed {
      self.cache.remove(shader);
    }
    if frame.is_multiple_of(CUSTOM_EFFECT_PRUNE_EVERY) {
      self.cache.sweep(shaders);
      self.cache.prune(frame, CUSTOM_EFFECT_TTL);
    }
    self.take_compiled();
    self.compile_next(device);
  }

  /// Takes in the new version the shader thread made, if one came back,
  /// and moves the effects waiting for a version to it once its pipeline
  /// is made (or it failed: they run their last good version)
  fn take_compiled(&mut self) {
    if !self.compiler.busy() && !self.cache.waiting() {
      return;
    }
    if let Some((key, done)) = self.compiler.finished() {
      match done {
        Some(done) => {
          let (made, outcome) = match done.pipeline {
            Some(Ok(pipeline)) => (Some(Checked::Ready(pipeline)), Ok(())),
            Some(Err(message)) => (None, Err(message)),
            None => (None, Ok(())),
          };
          self
            .cache
            .landed(key, &done.name, done.compiled, made, outcome);
        }
        None => self.cache.panicked(key),
      }
    }
    self.cache.advance(|_, source, _| {
      source
        .made
        .as_ref()
        .is_some_and(|made| made.ready().is_some())
    });
  }

  /// Sends the next new version an effect waits for to the shader thread,
  /// unless one is out: its pipeline is made there from its compiled source
  fn compile_next(&mut self, device: &wgpu::Device) {
    if self.compiler.busy() || !self.cache.waiting() {
      return;
    }
    // Not one made already or being checked (a shader of its text was
    // compiled at once), nor one that failed
    let Some((key, shader)) = self
      .cache
      .wanted()
      .find(|wanted| {
        wanted
          .compiled
          .is_none_or(|source| source.made.is_none() && !source.failed())
      })
      .map(|wanted| (wanted.key, wanted.shader.clone()))
    else {
      return;
    };
    let (device, layout, format) = (device.clone(), self.pipeline_layout.clone(), self.format);
    self.compiler.start(key, move || {
      let compiled = shader.compiled(ShaderKind::Effect).clone();
      let pipeline = compiled.as_ref().ok().map(|ready| {
        compile::checked(&device, || {
          effect_pipeline(&device, &layout, format, shader.name(), ready)
        })
      });
      EffectCompiled {
        name: shader.name().to_string(),
        compiled,
        pipeline,
      }
    });
  }

  /// Settle pipelines whose GPU checks have come back: an effect whose
  /// pipeline the GPU rejected runs its last good version, or is skipped
  /// (warned once per version, at its next `prepare`).
  fn settle(&mut self) {
    self.cache.settle(Checked::settle);
  }

  /// Pack `params` for the shader's `Params` into `out`; the number of bytes
  /// to upload. Names the shader doesn't have are warned about once per
  /// shader version.
  fn pack(
    &mut self,
    shader: AssetId<Shader>,
    shaders: &Assets<Shader>,
    params: &ShaderParams,
    out: &mut [u8; CUSTOM_PARAMS_BYTES as usize],
  ) -> Option<usize> {
    self.cache.pack(shader, shaders, params, out)
  }

  /// The pipeline for a key from [`prepare`](Self::prepare)
  fn pipeline(&self, key: u64) -> Option<&wgpu::RenderPipeline> {
    self.cache.source(key)?.made.as_ref()?.ready()
  }

  /// How many effect pipelines are kept
  fn pipeline_count(&self) -> usize {
    self.cache.made_count()
  }
}

/// The pipeline of a compiled game effect, checked by the GPU on a later
/// frame instead of waited on
fn build(
  device: &wgpu::Device,
  layout: &wgpu::PipelineLayout,
  format: wgpu::TextureFormat,
  name: &str,
  compiled: &CompiledShader,
) -> Checked<wgpu::RenderPipeline> {
  let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
  let pipeline = effect_pipeline(device, layout, format, name, compiled);
  // Anything naga accepted but the device rejects is found by a later
  // frame's `settle`, without waiting on the GPU here
  Checked::pending(pipeline, scope)
}

/// The pipeline drawing a compiled game effect into `format`
fn effect_pipeline(
  device: &wgpu::Device,
  layout: &wgpu::PipelineLayout,
  format: wgpu::TextureFormat,
  name: &str,
  compiled: &CompiledShader,
) -> wgpu::RenderPipeline {
  let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
    label: Some(name),
    source: wgpu::ShaderSource::Wgsl(compiled.wgsl.as_str().into()),
  });
  device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
    label: Some(name),
    layout: Some(layout),
    vertex: wgpu::VertexState {
      module: &module,
      entry_point: Some("vs_main"),
      buffers: &[],
      compilation_options: Default::default(),
    },
    fragment: Some(wgpu::FragmentState {
      module: &module,
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
      ..Default::default()
    },
    depth_stencil: None,
    multisample: wgpu::MultisampleState::default(),
    multiview_mask: None,
    cache: None,
  })
}

/// The game's effects a chain runs, in order, and the store their shaders
/// are in
#[derive(Clone, Copy)]
pub(crate) struct CustomChain<'a> {
  pub(crate) effects: &'a [&'a CustomEffect],
  pub(crate) shaders: &'a Assets<Shader>,
}

/// GPU pipeline for post-processing effects
pub struct PostProcessPipeline {
  // Every pass's bind group, made once per input and uniform buffer
  bind_groups: BindGroupCache,
  // Ping-pong textures for effect chaining
  texture_a: wgpu::Texture,
  texture_b: wgpu::Texture,
  view_a: wgpu::TextureView,
  view_b: wgpu::TextureView,

  // Sampler for texture sampling
  sampler: wgpu::Sampler,

  // Effect pipelines (created on first use)
  fade_pipeline: Option<EffectPipeline>,
  color_tint_pipeline: Option<EffectPipeline>,
  vignette_pipeline: Option<EffectPipeline>,
  screen_flash_pipeline: Option<EffectPipeline>,
  crt_pipeline: Option<EffectPipeline>,
  chromatic_aberration_pipeline: Option<EffectPipeline>,
  god_rays_pipeline: Option<EffectPipeline>,
  ambient_occlusion_pipeline: Option<EffectPipeline>,

  // Bloom pipelines (multi-pass: extract -> blur -> composite)
  bloom_extract_pipeline: Option<EffectPipeline>,
  bloom_blur_pipeline: Option<EffectPipeline>,
  bloom_composite_pipeline: Option<EffectPipeline>,

  // Half-resolution bloom scratch textures: the extract writes A, and each
  // blur pass goes A -> B -> A
  bloom_view: wgpu::TextureView,
  bloom_view_b: wgpu::TextureView,
  bloom_size: (u32, u32),

  // Per-pass uniform buffers for effect parameters
  effect_uniforms: UniformPool,

  // Per-pass uniform buffers for god rays (larger params)
  god_rays_uniforms: UniformPool,

  // Game-written effects, run after the built-ins
  custom: CustomEffectPipelines,
  custom_params: UniformPool,
  custom_globals: UniformPool,
  custom_frame: u64,

  // Bind group layout shared by most effects
  bind_group_layout: wgpu::BindGroupLayout,

  // Bind group layout for god rays (larger uniform)
  god_rays_bind_group_layout: wgpu::BindGroupLayout,

  // Bind group layout for bloom composite (2 textures + sampler + uniform)
  bloom_composite_bind_group_layout: wgpu::BindGroupLayout,

  // Dimensions
  width: u32,
  height: u32,
  format: wgpu::TextureFormat,
}

/// A single effect's GPU resources
struct EffectPipeline {
  pipeline: wgpu::RenderPipeline,
}

/// Descriptor shared by the two ping-pong textures, so a resize keeps the
/// usages they were created with.
/// Bloom extracts and blurs at half resolution: wide glows stay cheap, and
/// the composite's bilinear upsample smooths the result
fn bloom_size(width: u32, height: u32) -> (u32, u32) {
  (width.div_ceil(2).max(1), height.div_ceil(2).max(1))
}

fn bloom_scratch(
  device: &wgpu::Device,
  (width, height): (u32, u32),
  format: wgpu::TextureFormat,
  label: &str,
) -> wgpu::TextureView {
  device
    .create_texture(&wgpu::TextureDescriptor {
      label: Some(label),
      size: wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
      },
      mip_level_count: 1,
      sample_count: 1,
      dimension: wgpu::TextureDimension::D2,
      format,
      usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
      view_formats: &[],
    })
    .create_view(&wgpu::TextureViewDescriptor::default())
}

fn ping_pong_texture_desc(
  width: u32,
  height: u32,
  format: wgpu::TextureFormat,
) -> wgpu::TextureDescriptor<'static> {
  wgpu::TextureDescriptor {
    label: Some("Post-Process Texture"),
    size: wgpu::Extent3d {
      width,
      height,
      depth_or_array_layers: 1,
    },
    mip_level_count: 1,
    sample_count: 1,
    dimension: wgpu::TextureDimension::D2,
    format,
    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
      | wgpu::TextureUsages::TEXTURE_BINDING
      | wgpu::TextureUsages::COPY_DST
      | wgpu::TextureUsages::COPY_SRC, // Allow copying between ping-pong buffers
    view_formats: &[],
  }
}

impl PostProcessPipeline {
  /// Create a new post-process pipeline
  pub fn new(device: &wgpu::Device, width: u32, height: u32, format: wgpu::TextureFormat) -> Self {
    // Create ping-pong textures
    let texture_desc = ping_pong_texture_desc(width, height, format);

    let texture_a = device.create_texture(&wgpu::TextureDescriptor {
      label: Some("Post-Process Texture A"),
      ..texture_desc
    });
    let texture_b = device.create_texture(&wgpu::TextureDescriptor {
      label: Some("Post-Process Texture B"),
      ..texture_desc
    });

    let view_a = texture_a.create_view(&wgpu::TextureViewDescriptor::default());
    let view_b = texture_b.create_view(&wgpu::TextureViewDescriptor::default());

    // Create sampler
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
      label: Some("Post-Process Sampler"),
      address_mode_u: wgpu::AddressMode::ClampToEdge,
      address_mode_v: wgpu::AddressMode::ClampToEdge,
      address_mode_w: wgpu::AddressMode::ClampToEdge,
      mag_filter: wgpu::FilterMode::Linear,
      min_filter: wgpu::FilterMode::Linear,
      mipmap_filter: wgpu::MipmapFilterMode::Nearest,
      ..Default::default()
    });

    // Create uniform buffer
    // Create bind group layout (shared by most effects)
    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
      label: Some("Post-Process Bind Group Layout"),
      entries: &[
        // Input texture
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
        // Sampler
        wgpu::BindGroupLayoutEntry {
          binding: 1,
          visibility: wgpu::ShaderStages::FRAGMENT,
          ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
          count: None,
        },
        // Uniform buffer
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
      ],
    });

    // Create bind group layout for god rays (same structure, but with larger uniform)
    let god_rays_bind_group_layout =
      device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("God Rays Bind Group Layout"),
        entries: &[
          // Input texture
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
          // Sampler
          wgpu::BindGroupLayoutEntry {
            binding: 1,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
          },
          // God rays uniform buffer (128 bytes)
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
        ],
      });

    let bloom_size = bloom_size(width, height);
    let bloom_view = bloom_scratch(device, bloom_size, format, "Bloom Scratch A");
    let bloom_view_b = bloom_scratch(device, bloom_size, format, "Bloom Scratch B");

    // Bloom composite bind group layout: 2 textures + sampler + uniform
    let bloom_composite_bind_group_layout =
      device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Bloom Composite Bind Group Layout"),
        entries: &[
          // Scene texture (binding 0)
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
          // Bloom texture (binding 1)
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
          // Sampler (binding 2)
          wgpu::BindGroupLayoutEntry {
            binding: 2,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
          },
          // Uniform buffer (binding 3)
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

    Self {
      bind_groups: BindGroupCache::default(),
      texture_a,
      texture_b,
      view_a,
      view_b,
      sampler,
      fade_pipeline: None,
      color_tint_pipeline: None,
      vignette_pipeline: None,
      screen_flash_pipeline: None,
      crt_pipeline: None,
      chromatic_aberration_pipeline: None,
      god_rays_pipeline: None,
      ambient_occlusion_pipeline: None,
      bloom_extract_pipeline: None,
      bloom_blur_pipeline: None,
      bloom_composite_pipeline: None,
      bloom_view,
      bloom_view_b,
      bloom_size,
      effect_uniforms: UniformPool::new(
        "Post-Process Uniform Buffer",
        std::mem::size_of::<EffectParams>() as u64,
      ),
      god_rays_uniforms: UniformPool::new(
        "God Rays Uniform Buffer",
        std::mem::size_of::<GodRaysParams>() as u64,
      ),
      custom: CustomEffectPipelines::new(device, format),
      custom_params: UniformPool::new("Custom Effect Params", CUSTOM_PARAMS_BYTES),
      custom_globals: UniformPool::new("Custom Effect Globals", CUSTOM_GLOBALS_BYTES),
      custom_frame: 0,
      bind_group_layout,
      god_rays_bind_group_layout,
      bloom_composite_bind_group_layout,
      width,
      height,
      format,
    }
  }

  /// Resize the ping-pong textures
  pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
    if width == self.width && height == self.height {
      return;
    }

    self.width = width;
    self.height = height;
    // The bind groups point at the old views
    self.bind_groups.clear();

    let texture_desc = ping_pong_texture_desc(width, height, self.format);

    self.texture_a = device.create_texture(&wgpu::TextureDescriptor {
      label: Some("Post-Process Texture A"),
      ..texture_desc
    });
    self.texture_b = device.create_texture(&wgpu::TextureDescriptor {
      label: Some("Post-Process Texture B"),
      ..texture_desc
    });

    self.view_a = self
      .texture_a
      .create_view(&wgpu::TextureViewDescriptor::default());
    self.view_b = self
      .texture_b
      .create_view(&wgpu::TextureViewDescriptor::default());

    self.bloom_size = bloom_size(width, height);
    self.bloom_view = bloom_scratch(device, self.bloom_size, self.format, "Bloom Scratch A");
    self.bloom_view_b = bloom_scratch(device, self.bloom_size, self.format, "Bloom Scratch B");
  }

  /// Apply a chain of effects to the source in texture A.
  ///
  /// The source must already be copied to texture A before calling this.
  /// Returns which buffer contains the final result.
  pub fn apply_effects(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    effects: &[(&PostProcessEffect, f32)], // (effect, time)
  ) -> OutputBuffer {
    let custom = CustomChain {
      effects: &[],
      shaders: crate::core::ecs::no_shaders(),
    };
    self.apply_chain(device, queue, encoder, effects, custom, 0.0)
  }

  /// [`apply_effects`](Self::apply_effects), then the game's custom effects in
  /// order. A custom effect whose shader is not loaded or failed to compile,
  /// or whose pipeline isn't ready, is skipped and the image carries through.
  pub(crate) fn apply_chain(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    effects: &[(&PostProcessEffect, f32)],
    custom: CustomChain<'_>,
    time: f32,
  ) -> OutputBuffer {
    // Effects built on an earlier frame become usable once the GPU accepted them
    self.custom.settle();
    if effects.is_empty() && custom.effects.is_empty() {
      return OutputBuffer::A;
    }

    // Pre-create all needed pipelines to avoid borrow conflicts
    for (effect, _) in effects {
      self.ensure_pipeline_exists(device, effect);
    }

    self.effect_uniforms.reset();
    self.god_rays_uniforms.reset();

    let mut use_a_as_input = true;

    for (effect, time) in effects {
      // Bloom is a multi-pass effect — handle it separately
      if matches!(effect, PostProcessEffect::Bloom { .. }) {
        use_a_as_input =
          self.apply_bloom_effect(device, queue, encoder, effect, *time, use_a_as_input);
        continue;
      }

      // Write this pass's params first; each pass owns its buffer for the frame
      let is_god_rays = matches!(effect, PostProcessEffect::GodRays { .. });
      let uniform_index = if is_god_rays {
        let params = GodRaysParams::from_effect(effect, *time, self.width, self.height);
        self
          .god_rays_uniforms
          .write(device, queue, bytemuck::bytes_of(&params))
      } else {
        let params = EffectParams::from_effect(effect, *time, self.width, self.height);
        self
          .effect_uniforms
          .write(device, queue, bytemuck::bytes_of(&params))
      };

      // Get the pipeline (guaranteed to exist after ensure_pipeline_exists)
      let pipeline = self.get_pipeline(effect);

      // Create bind group for this pass
      let (input_view, output_view) = if use_a_as_input {
        (&self.view_a, &self.view_b)
      } else {
        (&self.view_b, &self.view_a)
      };

      // Handle god rays specially (different uniform buffer size)
      let bind_group = if is_god_rays {
        self.bind_groups.get(
          device,
          "God Rays Bind Group",
          &self.god_rays_bind_group_layout,
          &[
            Bound::View(input_view.clone()),
            Bound::Sampler(self.sampler.clone()),
            Bound::Buffer(self.god_rays_uniforms.buffer(uniform_index).clone()),
          ],
        )
      } else {
        self.bind_groups.get(
          device,
          "Post-Process Bind Group",
          &self.bind_group_layout,
          &[
            Bound::View(input_view.clone()),
            Bound::Sampler(self.sampler.clone()),
            Bound::Buffer(self.effect_uniforms.buffer(uniform_index).clone()),
          ],
        )
      };

      // Render pass
      {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
          label: Some("Post-Process Pass"),
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

        render_pass.set_pipeline(pipeline);
        render_pass.set_bind_group(0, &bind_group, &[]);
        render_pass.draw(0..6, 0..1);
      }

      // Swap for next iteration
      use_a_as_input = !use_a_as_input;
    }

    if !custom.effects.is_empty() {
      use_a_as_input = self.apply_custom(device, queue, encoder, custom, time, use_a_as_input);
    }

    // After all effects, use_a_as_input indicates what the NEXT input would be
    // If use_a_as_input is FALSE, last write was to B (we just flipped from true)
    // If use_a_as_input is TRUE, last write was to A (we just flipped from false)
    // So the output is the OPPOSITE of use_a_as_input
    if use_a_as_input {
      OutputBuffer::A
    } else {
      OutputBuffer::B
    }
  }

  /// Run the game's effects in order; the new `use_a_as_input`.
  fn apply_custom(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    custom: CustomChain<'_>,
    time: f32,
    mut use_a_as_input: bool,
  ) -> bool {
    let CustomChain { effects, shaders } = custom;
    let frame = self.custom_frame;
    self.custom_params.reset();
    self.custom_globals.reset();
    let globals = [time, self.width as f32, self.height as f32, 0.0];

    for effect in effects {
      let shader = effect.shader().id();
      let Some(key) = self.custom.prepare(device, shader, shaders, frame) else {
        continue;
      };
      let mut bytes = [0u8; CUSTOM_PARAMS_BYTES as usize];
      let Some(size) = self
        .custom
        .pack(shader, shaders, effect.params(), &mut bytes)
      else {
        continue;
      };
      let params_index = self.custom_params.write(device, queue, &bytes[..size]);
      let globals_index = self
        .custom_globals
        .write(device, queue, bytemuck::bytes_of(&globals));
      let Some(pipeline) = self.custom.pipeline(key) else {
        continue;
      };

      let (input_view, output_view) = if use_a_as_input {
        (&self.view_a, &self.view_b)
      } else {
        (&self.view_b, &self.view_a)
      };
      let bind_group = self.bind_groups.get(
        device,
        "Custom Effect Bind Group",
        &self.custom.bind_group_layout,
        &[
          Bound::View(input_view.clone()),
          Bound::Sampler(self.sampler.clone()),
          Bound::Buffer(self.custom_params.buffer(params_index).clone()),
          Bound::Buffer(self.custom_globals.buffer(globals_index).clone()),
        ],
      );
      {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
          label: Some("Custom Effect Pass"),
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
        render_pass.set_pipeline(pipeline);
        render_pass.set_bind_group(0, &bind_group, &[]);
        render_pass.draw(0..6, 0..1);
      }
      use_a_as_input = !use_a_as_input;
    }
    // A new version this frame found waits for its pipeline: sent now, it
    // starts a frame sooner
    self.custom.compile_next(device);
    use_a_as_input
  }

  /// Bring the custom effects in line with the shader store: the shaders
  /// freed since the last frame (`removed`, from `AssetEvent::Removed`) are
  /// forgotten at once, with the pipelines no effect uses any more, and the
  /// new versions made off the main thread take over. Call once a frame,
  /// whether or not effects run, before any runs
  pub(crate) fn sync_shaders(
    &mut self,
    device: &wgpu::Device,
    removed: &[AssetId<Shader>],
    shaders: &Assets<Shader>,
  ) {
    self.custom_frame += 1;
    self
      .custom
      .sync(device, removed, shaders, self.custom_frame);
  }

  /// Whether the effect shader `id` waits to run at its current version
  /// (tests and benches that measure a reload until it runs)
  #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
  pub(crate) fn custom_pending(&self, id: AssetId<Shader>, shaders: &Assets<Shader>) -> bool {
    self.custom.cache.pending(id, shaders)
  }

  /// The custom effects' shader thread's gate (tests)
  #[cfg(test)]
  pub(crate) fn compile_gate(&self) -> Arc<compile::CompileGate> {
    self.custom.compiler.gate()
  }

  /// Compiles sent to the custom effects' shader thread, and whether one
  /// is out (tests)
  #[cfg(test)]
  pub(crate) fn compiles(&self) -> (usize, bool) {
    (self.custom.compiler.started(), self.custom.compiler.busy())
  }

  /// How many pipelines the custom effects have (tests that watch them go)
  // The headless engine reads it, which the web has not
  #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
  pub(crate) fn custom_pipeline_count(&self) -> usize {
    self.custom.pipeline_count()
  }

  /// Get the view for a specific output buffer
  pub fn get_output_view(&self, buffer: OutputBuffer) -> &wgpu::TextureView {
    match buffer {
      OutputBuffer::A => &self.view_a,
      OutputBuffer::B => &self.view_b,
    }
  }

  /// Ensure the pipeline for an effect exists (creates it if needed)
  fn ensure_pipeline_exists(&mut self, device: &wgpu::Device, effect: &PostProcessEffect) {
    match effect {
      PostProcessEffect::Fade { .. } => {
        if self.fade_pipeline.is_none() {
          self.fade_pipeline =
            Some(self.create_effect_pipeline(device, "Fade", post_process_shaders::FADE));
        }
      }
      PostProcessEffect::ColorTint { .. } => {
        if self.color_tint_pipeline.is_none() {
          self.color_tint_pipeline = Some(self.create_effect_pipeline(
            device,
            "Color Tint",
            post_process_shaders::COLOR_TINT,
          ));
        }
      }
      PostProcessEffect::Vignette { .. } => {
        if self.vignette_pipeline.is_none() {
          self.vignette_pipeline =
            Some(self.create_effect_pipeline(device, "Vignette", post_process_shaders::VIGNETTE));
        }
      }
      PostProcessEffect::ScreenFlash { .. } => {
        if self.screen_flash_pipeline.is_none() {
          self.screen_flash_pipeline = Some(self.create_effect_pipeline(
            device,
            "Screen Flash",
            post_process_shaders::SCREEN_FLASH,
          ));
        }
      }
      PostProcessEffect::Crt { .. } => {
        if self.crt_pipeline.is_none() {
          self.crt_pipeline =
            Some(self.create_effect_pipeline(device, "CRT", post_process_shaders::CRT));
        }
      }
      PostProcessEffect::ChromaticAberration { .. } => {
        if self.chromatic_aberration_pipeline.is_none() {
          self.chromatic_aberration_pipeline = Some(self.create_effect_pipeline(
            device,
            "Chromatic Aberration",
            post_process_shaders::CHROMATIC_ABERRATION,
          ));
        }
      }
      PostProcessEffect::GodRays { .. } => {
        if self.god_rays_pipeline.is_none() {
          self.god_rays_pipeline = Some(self.create_god_rays_pipeline(device));
        }
      }
      PostProcessEffect::AmbientOcclusion { .. } => {
        if self.ambient_occlusion_pipeline.is_none() {
          self.ambient_occlusion_pipeline = Some(self.create_effect_pipeline(
            device,
            "Ambient Occlusion",
            post_process_shaders::AMBIENT_OCCLUSION,
          ));
        }
      }
      PostProcessEffect::Bloom { .. } => {
        if self.bloom_extract_pipeline.is_none() {
          self.bloom_extract_pipeline = Some(self.create_effect_pipeline(
            device,
            "Bloom Extract",
            post_process_shaders::BLOOM_EXTRACT,
          ));
        }
        if self.bloom_blur_pipeline.is_none() {
          self.bloom_blur_pipeline = Some(self.create_effect_pipeline(
            device,
            "Bloom Blur",
            post_process_shaders::BLOOM_BLUR,
          ));
        }
        if self.bloom_composite_pipeline.is_none() {
          self.bloom_composite_pipeline = Some(self.create_bloom_composite_pipeline(device));
        }
      }
    }
  }

  /// Get the pipeline for an effect (must already exist)
  fn get_pipeline(&self, effect: &PostProcessEffect) -> &wgpu::RenderPipeline {
    match effect {
      PostProcessEffect::Fade { .. } => &self.fade_pipeline.as_ref().unwrap().pipeline,
      PostProcessEffect::ColorTint { .. } => &self.color_tint_pipeline.as_ref().unwrap().pipeline,
      PostProcessEffect::Vignette { .. } => &self.vignette_pipeline.as_ref().unwrap().pipeline,
      PostProcessEffect::ScreenFlash { .. } => {
        &self.screen_flash_pipeline.as_ref().unwrap().pipeline
      }
      PostProcessEffect::Crt { .. } => &self.crt_pipeline.as_ref().unwrap().pipeline,
      PostProcessEffect::ChromaticAberration { .. } => {
        &self
          .chromatic_aberration_pipeline
          .as_ref()
          .unwrap()
          .pipeline
      }
      PostProcessEffect::GodRays { .. } => &self.god_rays_pipeline.as_ref().unwrap().pipeline,
      PostProcessEffect::AmbientOcclusion { .. } => {
        &self.ambient_occlusion_pipeline.as_ref().unwrap().pipeline
      }
      // Bloom uses multi-pass rendering and is handled by apply_bloom_effect()
      PostProcessEffect::Bloom { .. } => {
        unreachable!("Bloom uses apply_bloom_effect(), not get_pipeline()")
      }
    }
  }

  /// Create a render pipeline for an effect shader
  fn create_effect_pipeline(
    &self,
    device: &wgpu::Device,
    name: &str,
    shader_source: &str,
  ) -> EffectPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some(&format!("{} Shader", name)),
      source: wgpu::ShaderSource::Wgsl(shader_source.into()),
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some(&format!("{} Pipeline Layout", name)),
      bind_group_layouts: &[Some(&self.bind_group_layout)],
      immediate_size: 0,
    });

    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some(&format!("{} Pipeline", name)),
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
          format: self.format,
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

    EffectPipeline { pipeline }
  }

  /// Create the god rays pipeline (uses different bind group layout)
  fn create_god_rays_pipeline(&self, device: &wgpu::Device) -> EffectPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some("God Rays Shader"),
      source: wgpu::ShaderSource::Wgsl(post_process_shaders::GOD_RAYS.into()),
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("God Rays Pipeline Layout"),
      bind_group_layouts: &[Some(&self.god_rays_bind_group_layout)],
      immediate_size: 0,
    });

    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some("God Rays Pipeline"),
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
          format: self.format,
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

    EffectPipeline { pipeline }
  }

  /// Apply the multi-pass bloom effect.
  ///
  /// Flow: extract bright pixels -> blur H/V loop -> composite with scene.
  /// Returns the new `use_a_as_input` value for the caller's ping-pong state.
  fn apply_bloom_effect(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    effect: &PostProcessEffect,
    time: f32,
    use_a_as_input: bool,
  ) -> bool {
    let PostProcessEffect::Bloom {
      blur_passes,
      threshold,
      intensity,
      radius,
      ..
    } = effect
    else {
      return use_a_as_input;
    };

    // Write every pass's params up front: the writes need `&mut self`, and each
    // pass needs its own buffer (see `UniformPool`).
    let params_for = |spread: f32, w: f32, (width, height): (u32, u32)| EffectParams {
      color: [0.0, 0.0, 0.0, 1.0],
      params: [*threshold, *intensity, spread, w],
      time_resolution: [time, width as f32, height as f32, 0.0],
      _padding: [0.0; 4],
    };
    let full = (self.width, self.height);
    // Each pass blurs with sigma = 1.7 x radius screen pixels, the spread of
    // the original 9-tap kernel, given in half-resolution texels
    let sigma = BLOOM_SIGMA_PER_RADIUS * *radius / 2.0;
    let extract_params = params_for(*radius, *blur_passes as f32, full);
    let h_params = params_for(sigma, 0.0, self.bloom_size); // w=0.0 = horizontal
    let v_params = params_for(sigma, 1.0, self.bloom_size); // w=1.0 = vertical
    let composite_params = params_for(*radius, *blur_passes as f32, full);

    let extract_index =
      self
        .effect_uniforms
        .write(device, queue, bytemuck::bytes_of(&extract_params));
    let blur_indices: Vec<(usize, usize)> = (0..*blur_passes)
      .map(|_| {
        (
          self
            .effect_uniforms
            .write(device, queue, bytemuck::bytes_of(&h_params)),
          self
            .effect_uniforms
            .write(device, queue, bytemuck::bytes_of(&v_params)),
        )
      })
      .collect();
    let composite_index =
      self
        .effect_uniforms
        .write(device, queue, bytemuck::bytes_of(&composite_params));

    let (Some(extract), Some(blur), Some(composite)) = (
      self.bloom_extract_pipeline.as_ref(),
      self.bloom_blur_pipeline.as_ref(),
      self.bloom_composite_pipeline.as_ref(),
    ) else {
      return use_a_as_input;
    };
    let extract_pipeline = &extract.pipeline;
    let blur_pipeline = &blur.pipeline;
    let composite_pipeline = &composite.pipeline;

    // Input buffer = scene, output buffer = composite destination
    let (scene_view, output_view) = if use_a_as_input {
      (&self.view_a, &self.view_b)
    } else {
      (&self.view_b, &self.view_a)
    };

    // --- Pass 1: Extract bright pixels (scene -> bloom A, half resolution) ---

    let extract_bind_group = self.bind_groups.get(
      device,
      "Bloom Extract Bind Group",
      &self.bind_group_layout,
      &[
        Bound::View(scene_view.clone()),
        Bound::Sampler(self.sampler.clone()),
        Bound::Buffer(self.effect_uniforms.buffer(extract_index).clone()),
      ],
    );

    {
      let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Bloom Extract Pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
          view: &self.bloom_view,
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
      render_pass.set_pipeline(extract_pipeline);
      render_pass.set_bind_group(0, &extract_bind_group, &[]);
      render_pass.draw(0..6, 0..1);
    }

    // --- Pass 2: Blur loop (bloom A -> B -> A) ---
    for &(h_index, v_index) in &blur_indices {
      // Horizontal blur: bloom A -> bloom B
      let h_bind_group = self.bind_groups.get(
        device,
        "Bloom Blur H Bind Group",
        &self.bind_group_layout,
        &[
          Bound::View(self.bloom_view.clone()),
          Bound::Sampler(self.sampler.clone()),
          Bound::Buffer(self.effect_uniforms.buffer(h_index).clone()),
        ],
      );

      {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
          label: Some("Bloom Blur H Pass"),
          color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &self.bloom_view_b,
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
        render_pass.set_pipeline(blur_pipeline);
        render_pass.set_bind_group(0, &h_bind_group, &[]);
        render_pass.draw(0..6, 0..1);
      }

      // Vertical blur: bloom B -> bloom A
      let v_bind_group = self.bind_groups.get(
        device,
        "Bloom Blur V Bind Group",
        &self.bind_group_layout,
        &[
          Bound::View(self.bloom_view_b.clone()),
          Bound::Sampler(self.sampler.clone()),
          Bound::Buffer(self.effect_uniforms.buffer(v_index).clone()),
        ],
      );

      {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
          label: Some("Bloom Blur V Pass"),
          color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &self.bloom_view,
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
        render_pass.set_pipeline(blur_pipeline);
        render_pass.set_bind_group(0, &v_bind_group, &[]);
        render_pass.draw(0..6, 0..1);
      }
    }

    // --- Pass 3: Composite (scene + upsampled bloom A -> output_view) ---
    let composite_bind_group = self.bind_groups.get(
      device,
      "Bloom Composite Bind Group",
      &self.bloom_composite_bind_group_layout,
      &[
        Bound::View(scene_view.clone()),
        Bound::View(self.bloom_view.clone()),
        Bound::Sampler(self.sampler.clone()),
        Bound::Buffer(self.effect_uniforms.buffer(composite_index).clone()),
      ],
    );

    {
      let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Bloom Composite Pass"),
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
      render_pass.set_pipeline(composite_pipeline);
      render_pass.set_bind_group(0, &composite_bind_group, &[]);
      render_pass.draw(0..6, 0..1);
    }

    // Bloom consumed one slot: input X -> output Y, flip the ping-pong
    !use_a_as_input
  }

  /// Create the bloom composite pipeline (uses custom 2-texture bind group layout)
  fn create_bloom_composite_pipeline(&self, device: &wgpu::Device) -> EffectPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some("Bloom Composite Shader"),
      source: wgpu::ShaderSource::Wgsl(post_process_shaders::BLOOM_COMPOSITE.into()),
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("Bloom Composite Pipeline Layout"),
      bind_group_layouts: &[Some(&self.bloom_composite_bind_group_layout)],
      immediate_size: 0,
    });

    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some("Bloom Composite Pipeline"),
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
          format: self.format,
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

    EffectPipeline { pipeline }
  }

  /// Get texture A view (for copying source to)
  pub fn texture_a_view(&self) -> &wgpu::TextureView {
    &self.view_a
  }

  /// Get texture A (for copying source to)
  pub fn texture_a(&self) -> &wgpu::Texture {
    &self.texture_a
  }

  /// Get texture B view (for lighting output)
  pub fn texture_b_view(&self) -> &wgpu::TextureView {
    &self.view_b
  }

  /// Get texture B (for lighting output)
  pub fn texture_b(&self) -> &wgpu::Texture {
    &self.texture_b
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn effect_shaders_of_one_source_share_a_pipeline_until_the_last_goes() {
    let Ok((device, _queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let mut effects = CustomEffectPipelines::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb);
    let mut shaders = Assets::<Shader>::new();
    let src = "fn effect(in: EffectIn, params: Params) -> vec4<f32> { return scene(in.uv) * 0.5; }";
    let handles: Vec<_> = (0..3)
      .map(|i| shaders.add(Shader::from_wgsl(format!("shared-fx-{i}.wgsl"), src)))
      .collect();
    let ids: Vec<AssetId<Shader>> = handles.iter().map(|handle| handle.id()).collect();
    assert!(
      effects.prepare(&device, ids[0], &shaders, 1).is_none(),
      "pending"
    );
    effects.settle();
    let keys: Vec<Option<u64>> = ids
      .iter()
      .map(|&id| effects.prepare(&device, id, &shaders, 1))
      .collect();
    assert!(keys[0].is_some());
    assert!(keys.iter().all(|k| *k == keys[0]), "{keys:?}");
    assert_eq!(effects.pipeline_count(), 1, "one pipeline for the source");
    effects.sync(&device, &ids[..2], &shaders, 1);
    assert_eq!(effects.pipeline_count(), 1, "an effect still runs it");
    effects.sync(&device, &ids[2..], &shaders, 1);
    assert_eq!(effects.pipeline_count(), 0, "gone with its last shader");
    assert!(keys[0].is_some_and(|key| effects.pipeline(key).is_none()));
  }
}
