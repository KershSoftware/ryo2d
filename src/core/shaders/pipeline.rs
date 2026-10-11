//! Post-processing GPU pipeline
//!
//! Manages GPU resources for rendering post-processing effects.
//! Uses ping-pong buffers for effect chaining.

use super::custom::{self, ParamLayout, ShaderError, ShaderKind};
use super::effects::{EffectParams, GodRaysParams, PostProcessEffect};
use super::manager::CustomEffect;
use super::post_process_shaders;
use crate::core::ecs::{ShaderHandle, ShaderParams};
use std::collections::{HashMap, HashSet};

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
/// Custom effects unused for this many frames are forgotten
const CUSTOM_EFFECT_TTL: u64 = 600;

/// A compiled game effect
struct CustomPipeline {
  pipeline: wgpu::RenderPipeline,
  layout: ParamLayout,
}

/// What the cache knows about one effect handle's current version
struct CustomEntry {
  generation: u64,
  /// Its source's key in [`CustomEffectPipelines::compiled`]
  source: u64,
  last_used: u64,
}

/// One compiled source, shared by every handle with that source
struct CompiledSource {
  /// `None` when the source failed to compile; it is not retried
  ready: Option<CustomPipeline>,
  last_used: u64,
}

/// Pipelines of game effects, built on first use and rebuilt when the shader's
/// source changes.
struct CustomEffectPipelines {
  bind_group_layout: wgpu::BindGroupLayout,
  pipeline_layout: wgpu::PipelineLayout,
  format: wgpu::TextureFormat,
  /// By `ShaderHandle::serial()`
  entries: HashMap<u64, CustomEntry>,
  /// Compiled pipelines by [`custom::source_key`]: handles with equal sources
  /// compile once
  compiled: HashMap<u64, CompiledSource>,
  /// Parameter names already warned about, by (serial, generation)
  warned: HashMap<(u64, u64), HashSet<String>>,
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
      bind_group_layouts: &[&bind_group_layout],
      push_constant_ranges: &[],
    });
    Self {
      bind_group_layout,
      pipeline_layout,
      format,
      entries: HashMap::new(),
      compiled: HashMap::new(),
      warned: HashMap::new(),
    }
  }

  /// Make sure the cache holds the shader's current version; the key to run
  /// it with ([`pipeline`](Self::pipeline)), or `None` if it failed.
  fn prepare(&mut self, device: &wgpu::Device, shader: &ShaderHandle, frame: u64) -> Option<u64> {
    let serial = shader.serial();
    if let Some(entry) = self.entries.get_mut(&serial)
      && entry.generation == shader.generation()
      && let Some(compiled) = self.compiled.get_mut(&entry.source)
    {
      entry.last_used = frame;
      compiled.last_used = frame;
      return compiled.ready.is_some().then_some(entry.source);
    }
    let (generation, source) = shader.snapshot();
    let key = custom::source_key(ShaderKind::Effect, &source);
    if !self.compiled.contains_key(&key) {
      let ready = self.build(device, shader, key, &source);
      self.compiled.insert(
        key,
        CompiledSource {
          ready,
          last_used: frame,
        },
      );
    }
    self.entries.insert(
      serial,
      CustomEntry {
        generation,
        source: key,
        last_used: frame,
      },
    );
    let compiled = self.compiled.get_mut(&key)?;
    compiled.last_used = frame;
    compiled.ready.is_some().then_some(key)
  }

  /// Drop effects unused for [`CUSTOM_EFFECT_TTL`] frames and their warnings.
  fn prune(&mut self, frame: u64) {
    self
      .entries
      .retain(|_, e| frame.saturating_sub(e.last_used) < CUSTOM_EFFECT_TTL);
    self
      .compiled
      .retain(|_, c| frame.saturating_sub(c.last_used) < CUSTOM_EFFECT_TTL);
    let entries = &self.entries;
    self.warned.retain(|(serial, generation), _| {
      entries
        .get(serial)
        .is_some_and(|e| e.generation == *generation)
    });
  }

  /// Compile `source` (whose key is `key`) and build its pipeline; any
  /// failure is logged once and leaves the effect skipped.
  fn build(
    &self,
    device: &wgpu::Device,
    shader: &ShaderHandle,
    key: u64,
    source: &str,
  ) -> Option<CustomPipeline> {
    let compiled = match custom::compile(ShaderKind::Effect, shader.id(), source) {
      Ok(compiled) => compiled,
      Err(error) => {
        custom::report_once(key, &error);
        return None;
      }
    };
    if custom::uniform_size(ShaderKind::Effect, &compiled.layout) as u64 > CUSTOM_PARAMS_BYTES {
      let error = ShaderError {
        shader: shader.id().to_string(),
        line: None,
        column: None,
        message: format!("Params is larger than {CUSTOM_PARAMS_BYTES} bytes"),
      };
      custom::report_once(key, &error);
      return None;
    }

    // Anything naga accepted but the device rejects surfaces here instead of
    // taking the engine down
    device.push_error_scope(wgpu::ErrorFilter::Validation);
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some(shader.id()),
      source: wgpu::ShaderSource::Wgsl(compiled.wgsl.into()),
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some(shader.id()),
      layout: Some(&self.pipeline_layout),
      vertex: wgpu::VertexState {
        module: &module,
        entry_point: "vs_main",
        buffers: &[],
        compilation_options: Default::default(),
      },
      fragment: Some(wgpu::FragmentState {
        module: &module,
        entry_point: "fs_main",
        targets: &[Some(wgpu::ColorTargetState {
          format: self.format,
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
      custom::report_once(key, &error);
      return None;
    }
    Some(CustomPipeline {
      pipeline,
      layout: compiled.layout,
    })
  }

  /// Pack `params` for the shader's `Params` into `out`; the number of bytes
  /// to upload. Names the shader doesn't have are warned about once per
  /// shader version.
  fn pack(
    &mut self,
    shader: &ShaderHandle,
    params: &ShaderParams,
    out: &mut [u8; CUSTOM_PARAMS_BYTES as usize],
  ) -> Option<usize> {
    let serial = shader.serial();
    let entry = self.entries.get(&serial)?;
    let ready = self.compiled.get(&entry.source)?.ready.as_ref()?;
    let generation = entry.generation;
    let size = custom::uniform_size(ShaderKind::Effect, &ready.layout);
    let warned = &mut self.warned;
    custom::pack_with(&ready.layout, params, &mut out[..size], |name, expected| {
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
    Some(size)
  }

  /// The pipeline for a key from [`prepare`](Self::prepare)
  fn pipeline(&self, key: u64) -> Option<&wgpu::RenderPipeline> {
    let ready = self.compiled.get(&key)?.ready.as_ref()?;
    Some(&ready.pipeline)
  }
}

/// GPU pipeline for post-processing effects
pub struct PostProcessPipeline {
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
      mipmap_filter: wgpu::FilterMode::Nearest,
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
    self.apply_chain(device, queue, encoder, effects, &[], 0.0)
  }

  /// [`apply_effects`](Self::apply_effects), then the game's custom effects in
  /// order. A custom effect that failed to compile, or whose pipeline isn't
  /// ready, is skipped and the image carries through.
  pub(crate) fn apply_chain(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    effects: &[(&PostProcessEffect, f32)],
    custom: &[&CustomEffect],
    time: f32,
  ) -> OutputBuffer {
    if effects.is_empty() && custom.is_empty() {
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
        device.create_bind_group(&wgpu::BindGroupDescriptor {
          label: Some("God Rays Bind Group"),
          layout: &self.god_rays_bind_group_layout,
          entries: &[
            wgpu::BindGroupEntry {
              binding: 0,
              resource: wgpu::BindingResource::TextureView(input_view),
            },
            wgpu::BindGroupEntry {
              binding: 1,
              resource: wgpu::BindingResource::Sampler(&self.sampler),
            },
            wgpu::BindGroupEntry {
              binding: 2,
              resource: self
                .god_rays_uniforms
                .buffer(uniform_index)
                .as_entire_binding(),
            },
          ],
        })
      } else {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
          label: Some("Post-Process Bind Group"),
          layout: &self.bind_group_layout,
          entries: &[
            wgpu::BindGroupEntry {
              binding: 0,
              resource: wgpu::BindingResource::TextureView(input_view),
            },
            wgpu::BindGroupEntry {
              binding: 1,
              resource: wgpu::BindingResource::Sampler(&self.sampler),
            },
            wgpu::BindGroupEntry {
              binding: 2,
              resource: self
                .effect_uniforms
                .buffer(uniform_index)
                .as_entire_binding(),
            },
          ],
        })
      };

      // Render pass
      {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
          label: Some("Post-Process Pass"),
          color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: output_view,
            resolve_target: None,
            ops: wgpu::Operations {
              load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
              store: wgpu::StoreOp::Store,
            },
          })],
          depth_stencil_attachment: None,
          occlusion_query_set: None,
          timestamp_writes: None,
        });

        render_pass.set_pipeline(pipeline);
        render_pass.set_bind_group(0, &bind_group, &[]);
        render_pass.draw(0..6, 0..1);
      }

      // Swap for next iteration
      use_a_as_input = !use_a_as_input;
    }

    if !custom.is_empty() {
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
    custom: &[&CustomEffect],
    time: f32,
    mut use_a_as_input: bool,
  ) -> bool {
    self.custom_frame += 1;
    let frame = self.custom_frame;
    self.custom_params.reset();
    self.custom_globals.reset();
    let globals = [time, self.width as f32, self.height as f32, 0.0];

    for effect in custom {
      let shader = effect.shader();
      let Some(key) = self.custom.prepare(device, shader, frame) else {
        continue;
      };
      let mut bytes = [0u8; CUSTOM_PARAMS_BYTES as usize];
      let Some(size) = self.custom.pack(shader, effect.params(), &mut bytes) else {
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
      let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Custom Effect Bind Group"),
        layout: &self.custom.bind_group_layout,
        entries: &[
          wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(input_view),
          },
          wgpu::BindGroupEntry {
            binding: 1,
            resource: wgpu::BindingResource::Sampler(&self.sampler),
          },
          wgpu::BindGroupEntry {
            binding: 2,
            resource: self.custom_params.buffer(params_index).as_entire_binding(),
          },
          wgpu::BindGroupEntry {
            binding: 3,
            resource: self
              .custom_globals
              .buffer(globals_index)
              .as_entire_binding(),
          },
        ],
      });
      {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
          label: Some("Custom Effect Pass"),
          color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: output_view,
            resolve_target: None,
            ops: wgpu::Operations {
              load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
              store: wgpu::StoreOp::Store,
            },
          })],
          depth_stencil_attachment: None,
          occlusion_query_set: None,
          timestamp_writes: None,
        });
        render_pass.set_pipeline(pipeline);
        render_pass.set_bind_group(0, &bind_group, &[]);
        render_pass.draw(0..6, 0..1);
      }
      use_a_as_input = !use_a_as_input;
    }
    self.custom.prune(frame);
    use_a_as_input
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
      bind_group_layouts: &[&self.bind_group_layout],
      push_constant_ranges: &[],
    });

    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some(&format!("{} Pipeline", name)),
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
      multiview: None,
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
      bind_group_layouts: &[&self.god_rays_bind_group_layout],
      push_constant_ranges: &[],
    });

    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some("God Rays Pipeline"),
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
      multiview: None,
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

    let extract_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
      label: Some("Bloom Extract Bind Group"),
      layout: &self.bind_group_layout,
      entries: &[
        wgpu::BindGroupEntry {
          binding: 0,
          resource: wgpu::BindingResource::TextureView(scene_view),
        },
        wgpu::BindGroupEntry {
          binding: 1,
          resource: wgpu::BindingResource::Sampler(&self.sampler),
        },
        wgpu::BindGroupEntry {
          binding: 2,
          resource: self
            .effect_uniforms
            .buffer(extract_index)
            .as_entire_binding(),
        },
      ],
    });

    {
      let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Bloom Extract Pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
          view: &self.bloom_view,
          resolve_target: None,
          ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            store: wgpu::StoreOp::Store,
          },
        })],
        depth_stencil_attachment: None,
        occlusion_query_set: None,
        timestamp_writes: None,
      });
      render_pass.set_pipeline(extract_pipeline);
      render_pass.set_bind_group(0, &extract_bind_group, &[]);
      render_pass.draw(0..6, 0..1);
    }

    // --- Pass 2: Blur loop (bloom A -> B -> A) ---
    for &(h_index, v_index) in &blur_indices {
      // Horizontal blur: bloom A -> bloom B
      let h_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Bloom Blur H Bind Group"),
        layout: &self.bind_group_layout,
        entries: &[
          wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(&self.bloom_view),
          },
          wgpu::BindGroupEntry {
            binding: 1,
            resource: wgpu::BindingResource::Sampler(&self.sampler),
          },
          wgpu::BindGroupEntry {
            binding: 2,
            resource: self.effect_uniforms.buffer(h_index).as_entire_binding(),
          },
        ],
      });

      {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
          label: Some("Bloom Blur H Pass"),
          color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &self.bloom_view_b,
            resolve_target: None,
            ops: wgpu::Operations {
              load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
              store: wgpu::StoreOp::Store,
            },
          })],
          depth_stencil_attachment: None,
          occlusion_query_set: None,
          timestamp_writes: None,
        });
        render_pass.set_pipeline(blur_pipeline);
        render_pass.set_bind_group(0, &h_bind_group, &[]);
        render_pass.draw(0..6, 0..1);
      }

      // Vertical blur: bloom B -> bloom A
      let v_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Bloom Blur V Bind Group"),
        layout: &self.bind_group_layout,
        entries: &[
          wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(&self.bloom_view_b),
          },
          wgpu::BindGroupEntry {
            binding: 1,
            resource: wgpu::BindingResource::Sampler(&self.sampler),
          },
          wgpu::BindGroupEntry {
            binding: 2,
            resource: self.effect_uniforms.buffer(v_index).as_entire_binding(),
          },
        ],
      });

      {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
          label: Some("Bloom Blur V Pass"),
          color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &self.bloom_view,
            resolve_target: None,
            ops: wgpu::Operations {
              load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
              store: wgpu::StoreOp::Store,
            },
          })],
          depth_stencil_attachment: None,
          occlusion_query_set: None,
          timestamp_writes: None,
        });
        render_pass.set_pipeline(blur_pipeline);
        render_pass.set_bind_group(0, &v_bind_group, &[]);
        render_pass.draw(0..6, 0..1);
      }
    }

    // --- Pass 3: Composite (scene + upsampled bloom A -> output_view) ---
    let composite_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
      label: Some("Bloom Composite Bind Group"),
      layout: &self.bloom_composite_bind_group_layout,
      entries: &[
        wgpu::BindGroupEntry {
          binding: 0,
          resource: wgpu::BindingResource::TextureView(scene_view),
        },
        wgpu::BindGroupEntry {
          binding: 1,
          resource: wgpu::BindingResource::TextureView(&self.bloom_view),
        },
        wgpu::BindGroupEntry {
          binding: 2,
          resource: wgpu::BindingResource::Sampler(&self.sampler),
        },
        wgpu::BindGroupEntry {
          binding: 3,
          resource: self
            .effect_uniforms
            .buffer(composite_index)
            .as_entire_binding(),
        },
      ],
    });

    {
      let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Bloom Composite Pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
          view: output_view,
          resolve_target: None,
          ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            store: wgpu::StoreOp::Store,
          },
        })],
        depth_stencil_attachment: None,
        occlusion_query_set: None,
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
      bind_group_layouts: &[&self.bloom_composite_bind_group_layout],
      push_constant_ranges: &[],
    });

    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some("Bloom Composite Pipeline"),
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
      multiview: None,
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
  fn effect_handles_with_the_same_source_share_one_pipeline() {
    let Ok((device, _queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let mut effects = CustomEffectPipelines::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb);
    let src = "fn effect(in: EffectIn, params: Params) -> vec4<f32> { return scene(in.uv) * 0.5; }";
    let keys: Vec<Option<u64>> = (0..3)
      .map(|i| {
        let shader = ShaderHandle::from_source(format!("shared-fx-{i}.wgsl"), src);
        effects.prepare(&device, &shader, 1)
      })
      .collect();
    assert!(keys[0].is_some());
    assert!(keys.iter().all(|k| *k == keys[0]), "{keys:?}");
    assert_eq!(effects.compiled.len(), 1);
    assert_eq!(effects.entries.len(), 3);
  }
}
