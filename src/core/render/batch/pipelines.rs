//! The batch's render pipelines, made on first use.

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use super::polyline::Vertex2D;
use super::{BatchShader, BlendMode, LightStep, PageKind, Sampling, Stream};
use crate::core::graphics::SpriteInstance;
use crate::core::render::UI_FORMAT;
use crate::core::shaders::scope::Checked;

/// The blend state of `mode`. Every batch shader writes colour premultiplied
/// by alpha.
pub(crate) fn blend_state(mode: BlendMode) -> wgpu::BlendState {
  use wgpu::BlendFactor::{Dst, One, OneMinusDst, OneMinusSrcAlpha, Zero};
  let add = |src_factor, dst_factor| wgpu::BlendComponent {
    src_factor,
    dst_factor,
    operation: wgpu::BlendOperation::Add,
  };
  // Light and shade change the colour below, not how much of it there is
  let keep = add(Zero, One);
  let color = match mode {
    BlendMode::Alpha | BlendMode::Premultiplied => {
      return wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING;
    }
    BlendMode::Additive => add(One, One),
    BlendMode::Multiply => add(Dst, OneMinusSrcAlpha),
    BlendMode::Screen => add(OneMinusDst, One),
  };
  wgpu::BlendState { color, alpha: keep }
}

/// The blend state and colour writes of a draw into a light buffer.
pub(crate) fn light_blend(step: LightStep) -> (wgpu::BlendState, wgpu::ColorWrites) {
  use wgpu::BlendFactor::{One, Zero};
  let add = |src_factor, dst_factor| wgpu::BlendComponent {
    src_factor,
    dst_factor,
    operation: wgpu::BlendOperation::Add,
  };
  match step {
    // Light adds up (each light holds its own shadow back); the alpha is
    // left alone
    LightStep::Add => (
      wgpu::BlendState {
        color: add(One, One),
        alpha: add(Zero, One),
      },
      wgpu::ColorWrites::COLOR,
    ),
  }
}

/// What a pipeline is made for: two draws with equal keys share one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct PipelineKey {
  pub(crate) shader: BatchShader,
  pub(crate) blend: BlendMode,
  pub(crate) stream: Stream,
  pub(crate) page: PageKind,
  /// The draw's sampling for the tile shaders, whose smooth draws read
  /// inside their cells (`fs_tile`); `Sampling::Linear` for every other
  /// shader, so no other pipeline is made twice
  pub(crate) sampling: Sampling,
  pub(crate) format: wgpu::TextureFormat,
}

impl PipelineKey {
  /// The key of a draw of `shader` sampled with `sampling` (kept for the
  /// tile shaders alone)
  pub(crate) fn new(
    shader: BatchShader,
    blend: BlendMode,
    stream: Stream,
    page: PageKind,
    sampling: Sampling,
    format: wgpu::TextureFormat,
  ) -> Self {
    let sampling = match shader {
      BatchShader::Tile | BatchShader::TileObject => sampling,
      _ => Sampling::Linear,
    };
    Self {
      shader,
      blend,
      stream,
      page,
      sampling,
      format,
    }
  }
}

/// Which of the batch's modules a built-in pipeline's stages come from
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Module {
  Sprite,
  Line,
  Light,
}

/// The bind groups a built-in pipeline takes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Groups {
  /// Camera, then texture
  Textured,
  /// Camera, texture, then the tile slot (tile chunks)
  Tile,
  /// Camera only (untextured lines)
  Camera,
  /// Camera, then the shadow atlas (lights)
  Light,
}

/// The module, bind groups and fragment entry of a built-in shader's
/// pipeline for `key`; `None` for a game shader's (made from its own
/// module) and for keys with no pipeline (sprites are quads, lines
/// triangles). Tiles sampled smoothly read inside their cells at the level
/// they need (`fs_tile` and its variants); tiles sampled nearest keep the
/// sprite's entries
fn built_in(key: &PipelineKey) -> Option<(Module, Groups, &'static str)> {
  // Render targets hold linear premultiplied colour, whatever the blend;
  // premultiplied textures take their colour as it is
  let premultiplied = key.blend == BlendMode::Premultiplied;
  let entry = |linear: &'static str, premultiplied_entry: &'static str, plain: &'static str| {
    if key.page == PageKind::Linear {
      linear
    } else if premultiplied {
      premultiplied_entry
    } else {
      plain
    }
  };
  let sprite = entry("fs_base_linear", "fs_base_premultiplied", "fs_base");
  let tile = match key.sampling {
    Sampling::Nearest => sprite,
    Sampling::Linear => entry("fs_tile_linear", "fs_tile_premultiplied", "fs_tile"),
  };
  let textured = |fragment| (Module::Sprite, Groups::Textured, fragment);
  Some(match (key.shader, key.stream) {
    (BatchShader::Sprite, Stream::Quads) => textured(sprite),
    (BatchShader::SpriteEffect, Stream::Quads) => textured(entry(
      "fs_effect_linear",
      "fs_effect_premultiplied",
      "fs_effect",
    )),
    // Tile objects are sprites; tile chunks are placed by `vs_tile`
    // through their slot
    (BatchShader::TileObject, Stream::Quads) => textured(tile),
    (BatchShader::Tile, Stream::Quads) => (Module::Sprite, Groups::Tile, tile),
    // Repeated image layers are images, never render targets
    (BatchShader::Repeat, Stream::Quads) if premultiplied => textured("fs_repeat_premultiplied"),
    (BatchShader::Repeat, Stream::Quads) => textured("fs_repeat"),
    (BatchShader::Line, Stream::Triangles) => (Module::Line, Groups::Camera, "fs_main"),
    (BatchShader::LineTextured, Stream::Triangles) => {
      (Module::Line, Groups::Textured, "fs_textured")
    }
    (BatchShader::TextMask, Stream::Quads) => textured("fs_text_mask"),
    (BatchShader::TextColor, Stream::Quads) => textured("fs_text_color"),
    (BatchShader::TextMsdf, Stream::Quads) => textured("fs_text_msdf"),
    (BatchShader::Light(step), Stream::Quads) => (Module::Light, Groups::Light, step.fragment()),
    _ => return None,
  })
}

/// The layout sprite textures are bound with (group 1): the texture and its
/// sampler. Atlas pages make their bind groups with it.
pub(crate) fn texture_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
  device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
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
  })
}

/// The layout the shadow atlas is bound with for the light shader (group
/// 1): the atlas's depth texture and its comparison sampler. The atlas
/// makes its bind group with a layout of its own from this, which WebGPU
/// takes as the same.
pub(crate) fn shadow_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
  device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
    label: Some("Shadow Atlas Layout"),
    entries: &[
      wgpu::BindGroupLayoutEntry {
        binding: 0,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
          multisampled: false,
          view_dimension: wgpu::TextureViewDimension::D2,
          sample_type: wgpu::TextureSampleType::Depth,
        },
        count: None,
      },
      wgpu::BindGroupLayoutEntry {
        binding: 1,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
        count: None,
      },
    ],
  })
}

/// The layout a camera's uniforms are bound with (group 0).
pub(crate) fn camera_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
  device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
    label: Some("Batch Camera Layout"),
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
  })
}

/// The unit quad's corners: position, then UV.
const QUAD_ATTRIBUTES: [wgpu::VertexAttribute; 2] =
  wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2];

/// The unit quad ([`unit_quad`](super::unit_quad)), vertex buffer 0 of every
/// quad pipeline (the batch's and the UI's)
pub(crate) const QUAD_LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
  array_stride: 16,
  step_mode: wgpu::VertexStepMode::Vertex,
  attributes: &QUAD_ATTRIBUTES,
};

/// A [`SpriteInstance`]: its transform rows, UV rect, tint, effect
/// parameters and UV bounds.
const INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 8] = wgpu::vertex_attr_array![
  2 => Float32x4, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4,
  6 => Float32x4, 7 => Float32x4, 8 => Float32x4, 9 => Float32x4,
];

const INSTANCE_LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
  array_stride: std::mem::size_of::<SpriteInstance>() as u64,
  step_mode: wgpu::VertexStepMode::Instance,
  attributes: &INSTANCE_ATTRIBUTES,
};

/// A [`Vertex2D`]: position, UV, colour, parameters, perpendicular and the
/// atlas slot it samples.
const VERTEX2D_ATTRIBUTES: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
  0 => Float32x2, 1 => Float32x2, 2 => Float32x4, 3 => Float32x4, 4 => Float32x2, 5 => Float32x4,
];

const VERTEX2D_LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
  array_stride: std::mem::size_of::<Vertex2D>() as u64,
  step_mode: wgpu::VertexStepMode::Vertex,
  attributes: &VERTEX2D_ATTRIBUTES,
};

/// A pipeline and what to report if the GPU rejects it.
struct Built {
  state: Checked<wgpu::RenderPipeline>,
  /// The shader's name and its source key (custom shaders only)
  custom: Option<(String, u64)>,
}

/// The batch's pipelines, by [`PipelineKey`], made on first use. Built-in
/// ones are usable at once; a custom one once the GPU's check passed.
pub(crate) struct Pipelines {
  /// Camera, then texture
  layout: wgpu::PipelineLayout,
  /// Camera, texture, then the tile slot (tile chunks)
  tile_layout: wgpu::PipelineLayout,
  /// Camera only (untextured lines)
  camera_only: wgpu::PipelineLayout,
  /// Camera, then the shadow atlas (lights)
  light_layout: wgpu::PipelineLayout,
  sprite: wgpu::ShaderModule,
  line: wgpu::ShaderModule,
  light: wgpu::ShaderModule,
  built: HashMap<PipelineKey, Built>,
}

impl Pipelines {
  pub(crate) fn new(
    device: &wgpu::Device,
    camera: &wgpu::BindGroupLayout,
    textures: &wgpu::BindGroupLayout,
    tile_slots: &wgpu::BindGroupLayout,
  ) -> Self {
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("Batch Pipeline Layout"),
      bind_group_layouts: &[Some(camera), Some(textures)],
      immediate_size: 0,
    });
    let tile_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("Batch Tile Pipeline Layout"),
      bind_group_layouts: &[Some(camera), Some(textures), Some(tile_slots)],
      immediate_size: 0,
    });
    let camera_only = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("Batch Line Pipeline Layout"),
      bind_group_layouts: &[Some(camera)],
      immediate_size: 0,
    });
    let shadow = shadow_layout(device);
    let light_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("Batch Light Pipeline Layout"),
      bind_group_layouts: &[Some(camera), Some(&shadow)],
      immediate_size: 0,
    });
    let module = |label, source: &'static str| {
      device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(source.into()),
      })
    };
    Self {
      layout,
      tile_layout,
      camera_only,
      light_layout,
      sprite: module("Sprite Shader", crate::core::shaders::compose::SPRITE),
      line: module("Line Shader", crate::core::shaders::compose::LINE),
      light: module("Light Shader", crate::core::shaders::compose::LIGHT),
      built: HashMap::new(),
    }
  }

  /// Make `key`'s pipeline unless it exists. A custom shader's comes from
  /// `custom`, its name and composed WGSL.
  pub(crate) fn ensure(
    &mut self,
    device: &wgpu::Device,
    key: PipelineKey,
    custom: Option<(&str, &str)>,
  ) {
    let Entry::Vacant(slot) = self.built.entry(key) else {
      return;
    };
    if let BatchShader::Custom(source) = key.shader {
      let (Stream::Quads, Some((name, wgsl))) = (key.stream, custom) else {
        return;
      };
      let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
      let module = custom_module(device, name, wgsl);
      let pipeline = custom_pipeline(device, &self.layout, name, &module, key);
      // Anything naga accepted but the device rejects is found by a later
      // frame's `settle`, without waiting on the GPU here
      slot.insert(Built {
        state: Checked::pending(pipeline, scope),
        custom: Some((name.to_string(), source)),
      });
      return;
    }
    let Some((module, groups, fragment)) = built_in(&key) else {
      return;
    };
    let module = match module {
      Module::Sprite => &self.sprite,
      Module::Line => &self.line,
      Module::Light => &self.light,
    };
    let layout = match groups {
      Groups::Textured => &self.layout,
      Groups::Tile => &self.tile_layout,
      Groups::Camera => &self.camera_only,
      Groups::Light => &self.light_layout,
    };
    let pipeline = batch_pipeline(device, layout, fragment, module, fragment, key);
    slot.insert(Built {
      state: Checked::Ready(pipeline),
      custom: None,
    });
  }

  /// `key`'s pipeline, once usable.
  pub(crate) fn get(&self, key: &PipelineKey) -> Option<&wgpu::RenderPipeline> {
    self.built.get(key)?.state.ready()
  }

  /// Whether `key`'s pipeline was made: usable, being checked, or rejected
  pub(crate) fn made(&self, key: &PipelineKey) -> bool {
    self.built.contains_key(key)
  }

  /// The keys of the pipelines made for game shader source `source` that
  /// the GPU has not rejected: a newer version of a shader drawn with it is
  /// made for the same before it takes over
  pub(crate) fn custom_keys(&self, source: u64) -> impl Iterator<Item = PipelineKey> + '_ {
    self
      .built
      .iter()
      .filter(move |(key, built)| {
        key.shader == BatchShader::Custom(source) && !matches!(built.state, Checked::Failed)
      })
      .map(|(key, _)| *key)
  }

  /// Whether a pipeline made on the main thread for game shader source
  /// `source` still has its GPU check out
  pub(crate) fn checking(&self, source: u64) -> bool {
    self.built.iter().any(|(key, built)| {
      key.shader == BatchShader::Custom(source) && matches!(built.state, Checked::Pending { .. })
    })
  }

  /// Takes in `pipeline`, which the shader thread made (and the GPU
  /// accepted) for game shader source `source`, named `name`; one made
  /// meanwhile is kept
  pub(crate) fn take_made(
    &mut self,
    key: PipelineKey,
    pipeline: wgpu::RenderPipeline,
    name: &str,
    source: u64,
  ) {
    self.built.entry(key).or_insert_with(|| Built {
      state: Checked::Ready(pipeline),
      custom: Some((name.to_string(), source)),
    });
  }

  /// The layout game shaders' pipelines are made with: camera, then texture
  pub(crate) fn custom_layout(&self) -> &wgpu::PipelineLayout {
    &self.layout
  }

  /// Settle pipelines whose GPU checks came back, telling `outcome` which
  /// game shader source each was made from and whether the GPU accepted it
  /// (`Ok`) or rejected it (`Err`, its message). A rejected one's drawables
  /// draw plain.
  pub(crate) fn settle(&mut self, mut outcome: impl FnMut(u64, Result<(), String>)) {
    for built in self.built.values_mut() {
      if let Some(settled) = built.state.settle()
        && let Some((_, source)) = &built.custom
      {
        outcome(*source, settled);
      }
    }
  }

  /// Keep only the pipelines `keep` accepts.
  pub(crate) fn retain(&mut self, mut keep: impl FnMut(&PipelineKey) -> bool) {
    self.built.retain(|key, _| keep(key));
  }

  /// How many pipelines game shaders have
  // The headless engine reads it, which the web has not
  #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
  pub(crate) fn custom_count(&self) -> usize {
    self
      .built
      .keys()
      .filter(|key| matches!(key.shader, BatchShader::Custom(_)))
      .count()
  }
}

/// A game sprite shader's module, from its composed WGSL
pub(crate) fn custom_module(device: &wgpu::Device, name: &str, wgsl: &str) -> wgpu::ShaderModule {
  device.create_shader_module(wgpu::ShaderModuleDescriptor {
    label: Some(name),
    source: wgpu::ShaderSource::Wgsl(wgsl.into()),
  })
}

/// A game sprite shader's pipeline for `key`, from its `module`: the entry
/// for `key`'s page and blend (linear pages, premultiplied textures, or
/// straight colour)
pub(crate) fn custom_pipeline(
  device: &wgpu::Device,
  layout: &wgpu::PipelineLayout,
  name: &str,
  module: &wgpu::ShaderModule,
  key: PipelineKey,
) -> wgpu::RenderPipeline {
  let fragment = if key.page == PageKind::Linear {
    "fs_custom_linear"
  } else if key.blend == BlendMode::Premultiplied {
    "fs_custom_premultiplied"
  } else {
    "fs_custom"
  };
  batch_pipeline(device, layout, name, module, fragment, key)
}

/// A pipeline drawing `key`'s stream (instanced quads or indexed triangles)
/// with `module`'s `vs_main` (`vs_tile` for tile chunks) and `fragment`,
/// blended and formatted for `key`.
fn batch_pipeline(
  device: &wgpu::Device,
  layout: &wgpu::PipelineLayout,
  label: &str,
  module: &wgpu::ShaderModule,
  fragment: &str,
  key: PipelineKey,
) -> wgpu::RenderPipeline {
  let (blend, write_mask) = match key.shader {
    BatchShader::Light(step) => light_blend(step),
    _ => (blend_state(key.blend), wgpu::ColorWrites::ALL),
  };
  // Built for the UI image (gamma-encoded colour), a pipeline encodes on
  // output. The UI draws through its own renderer: this rule now serves the
  // tests alone, chiefly the proof of UI lines against this line pipeline.
  // Light modules do not compose color.wgsl (lights are always in the scene
  // format)
  let ui = key.format == UI_FORMAT && !matches!(key.shader, BatchShader::Light(_));
  let constants: &[(&str, f64)] = if ui { &[("UI_GAMMA", 1.0)] } else { &[] };
  device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
    label: Some(label),
    layout: Some(layout),
    vertex: wgpu::VertexState {
      module,
      entry_point: Some(match key.shader {
        BatchShader::Tile => "vs_tile",
        BatchShader::Repeat => "vs_repeat",
        _ => "vs_main",
      }),
      buffers: match key.stream {
        Stream::Quads => &[Some(QUAD_LAYOUT), Some(INSTANCE_LAYOUT)],
        Stream::Triangles => &[Some(VERTEX2D_LAYOUT)],
      },
      compilation_options: Default::default(),
    },
    fragment: Some(wgpu::FragmentState {
      module,
      entry_point: Some(fragment),
      targets: &[Some(wgpu::ColorTargetState {
        format: key.format,
        blend: Some(blend),
        write_mask,
      })],
      compilation_options: wgpu::PipelineCompilationOptions {
        constants,
        ..Default::default()
      },
    }),
    // No culling: a diagonally flipped tile's quad has its axes swapped,
    // which turns it over
    primitive: wgpu::PrimitiveState {
      topology: wgpu::PrimitiveTopology::TriangleList,
      cull_mode: None,
      ..Default::default()
    },
    depth_stencil: None,
    multisample: wgpu::MultisampleState::default(),
    multiview_mask: None,
    cache: None,
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn every_blend_mode_has_its_state() {
    use wgpu::{BlendComponent as C, BlendFactor as F, BlendOperation as Op};
    let over = C {
      src_factor: F::One,
      dst_factor: F::OneMinusSrcAlpha,
      operation: Op::Add,
    };
    let keep_alpha = C {
      src_factor: F::Zero,
      dst_factor: F::One,
      operation: Op::Add,
    };
    let colour = |mode| blend_state(mode).color;
    assert_eq!(
      blend_state(BlendMode::Premultiplied),
      wgpu::BlendState {
        color: over,
        alpha: over
      }
    );
    assert_eq!(
      colour(BlendMode::Additive),
      C {
        src_factor: F::One,
        dst_factor: F::One,
        operation: Op::Add
      }
    );
    assert_eq!(
      colour(BlendMode::Multiply),
      C {
        src_factor: F::Dst,
        dst_factor: F::OneMinusSrcAlpha,
        operation: Op::Add
      }
    );
    assert_eq!(
      colour(BlendMode::Screen),
      C {
        src_factor: F::OneMinusDst,
        dst_factor: F::One,
        operation: Op::Add
      }
    );
    for mode in [BlendMode::Additive, BlendMode::Multiply, BlendMode::Screen] {
      assert_eq!(blend_state(mode).alpha, keep_alpha, "{mode:?}");
    }
  }

  #[test]
  fn lights_add_up_and_leave_the_alpha_alone() {
    let (blend, writes) = light_blend(LightStep::Add);
    assert_eq!(blend.color.src_factor, wgpu::BlendFactor::One);
    assert_eq!(blend.color.dst_factor, wgpu::BlendFactor::One);
    assert_eq!(writes, wgpu::ColorWrites::COLOR);
  }

  #[test]
  fn nearest_tile_draws_keep_the_sprite_fragment_stages() {
    use crate::core::render::SCENE_FORMAT;
    let key = |shader, blend, page, sampling| {
      PipelineKey::new(shader, blend, Stream::Quads, page, sampling, SCENE_FORMAT)
    };
    let entry = |shader, blend, page, sampling| {
      built_in(&key(shader, blend, page, sampling)).map(|(_, _, entry)| entry)
    };
    let (straight, premultiplied) = (BlendMode::Alpha, BlendMode::Premultiplied);
    let (image, target) = (PageKind::Srgb8, PageKind::Linear);
    for shader in [BatchShader::Tile, BatchShader::TileObject] {
      for (blend, page, nearest, smooth) in [
        (straight, image, "fs_base", "fs_tile"),
        (
          premultiplied,
          image,
          "fs_base_premultiplied",
          "fs_tile_premultiplied",
        ),
        (straight, target, "fs_base_linear", "fs_tile_linear"),
      ] {
        let at = |sampling| entry(shader, blend, page, sampling);
        assert_eq!(at(Sampling::Nearest), Some(nearest), "{shader:?} {blend:?}");
        assert_eq!(at(Sampling::Linear), Some(smooth), "{shader:?} {blend:?}");
      }
    }
    // Chunks bind their tile slot; tile objects are sprites
    let groups = |shader| built_in(&key(shader, straight, image, Sampling::Linear));
    assert_eq!(
      groups(BatchShader::Tile).map(|(m, g, _)| (m, g)),
      Some((Module::Sprite, Groups::Tile))
    );
    assert_eq!(
      groups(BatchShader::TileObject).map(|(m, g, _)| (m, g)),
      Some((Module::Sprite, Groups::Textured))
    );
    // Every other shader has one pipeline for either sampling
    for shader in [
      BatchShader::Sprite,
      BatchShader::SpriteEffect,
      BatchShader::Repeat,
      BatchShader::TextMask,
    ] {
      assert_eq!(
        key(shader, straight, image, Sampling::Nearest),
        key(shader, straight, image, Sampling::Linear),
        "{shader:?}"
      );
    }
    assert_ne!(
      key(BatchShader::Tile, straight, image, Sampling::Nearest),
      key(BatchShader::Tile, straight, image, Sampling::Linear)
    );
  }

  #[test]
  fn alpha_blends_premultiplied_colour() {
    // Same result as straight alpha once the shader multiplies by alpha
    assert_eq!(
      blend_state(BlendMode::Alpha),
      wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING
    );
  }
}
