//! The batch's render pipelines, made on first use.

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use super::{BatchShader, BlendMode, Stream};
use crate::core::graphics::SpriteInstance;
use crate::core::shaders::custom::{self, ShaderError};
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

/// What a pipeline is made for: two draws with equal keys share one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct PipelineKey {
  pub(crate) shader: BatchShader,
  pub(crate) blend: BlendMode,
  pub(crate) stream: Stream,
  pub(crate) format: wgpu::TextureFormat,
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

/// A [`SpriteInstance`]: its transform rows, UV rect, tint, effect
/// parameters and UV bounds.
const INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 8] = wgpu::vertex_attr_array![
  2 => Float32x4, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4,
  6 => Float32x4, 7 => Float32x4, 8 => Float32x4, 9 => Float32x4,
];

const QUAD_LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
  array_stride: 16,
  step_mode: wgpu::VertexStepMode::Vertex,
  attributes: &QUAD_ATTRIBUTES,
};

const INSTANCE_LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
  array_stride: std::mem::size_of::<SpriteInstance>() as u64,
  step_mode: wgpu::VertexStepMode::Instance,
  attributes: &INSTANCE_ATTRIBUTES,
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
  layout: wgpu::PipelineLayout,
  sprite: wgpu::ShaderModule,
  built: HashMap<PipelineKey, Built>,
}

impl Pipelines {
  pub(crate) fn new(
    device: &wgpu::Device,
    camera: &wgpu::BindGroupLayout,
    textures: &wgpu::BindGroupLayout,
  ) -> Self {
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("Batch Pipeline Layout"),
      bind_group_layouts: &[Some(camera), Some(textures)],
      immediate_size: 0,
    });
    let sprite = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some("Sprite Shader"),
      source: wgpu::ShaderSource::Wgsl(crate::core::shaders::compose::SPRITE.into()),
    });
    Self {
      layout,
      sprite,
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
    // Premultiplied textures take their colour as it is
    let premultiplied = key.blend == BlendMode::Premultiplied;
    let entry = match (key.shader, key.stream, premultiplied) {
      (BatchShader::Sprite, Stream::Quads, false) => "fs_base",
      (BatchShader::Sprite, Stream::Quads, true) => "fs_base_premultiplied",
      (BatchShader::SpriteEffect, Stream::Quads, false) => "fs_effect",
      (BatchShader::SpriteEffect, Stream::Quads, true) => "fs_effect_premultiplied",
      (BatchShader::Custom(source), Stream::Quads, _) => {
        let Some((name, wgsl)) = custom else {
          return;
        };
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
          label: Some(name),
          source: wgpu::ShaderSource::Wgsl(wgsl.into()),
        });
        let fragment = if premultiplied {
          "fs_custom_premultiplied"
        } else {
          "fs_custom"
        };
        let pipeline = quad_pipeline(device, &self.layout, name, &module, fragment, key);
        // Anything naga accepted but the device rejects is found by a
        // later frame's `settle`, without waiting on the GPU here
        slot.insert(Built {
          state: Checked::pending(pipeline, scope),
          custom: Some((name.to_string(), source)),
        });
        return;
      }
      // Triangle shaders arrive with the triangle stream
      _ => return,
    };
    let pipeline = quad_pipeline(device, &self.layout, entry, &self.sprite, entry, key);
    slot.insert(Built {
      state: Checked::Ready(pipeline),
      custom: None,
    });
  }

  /// `key`'s pipeline, once usable.
  pub(crate) fn get(&self, key: &PipelineKey) -> Option<&wgpu::RenderPipeline> {
    self.built.get(key)?.state.ready()
  }

  /// Settle pipelines whose GPU checks came back; a rejected one is
  /// reported once and its drawables fall back to plain sprites.
  pub(crate) fn settle(&mut self) {
    for built in self.built.values_mut() {
      if let Some(message) = built.state.settle()
        && let Some((shader, source)) = &built.custom
      {
        let error = ShaderError {
          shader: shader.clone(),
          line: None,
          column: None,
          message,
        };
        custom::report_once(*source, &error);
      }
    }
  }

  /// Keep only the pipelines `keep` accepts.
  pub(crate) fn retain(&mut self, mut keep: impl FnMut(&PipelineKey) -> bool) {
    self.built.retain(|key, _| keep(key));
  }
}

/// A pipeline drawing instanced quads with `module`'s `vs_main` and
/// `fragment`, blended and formatted for `key`.
fn quad_pipeline(
  device: &wgpu::Device,
  layout: &wgpu::PipelineLayout,
  label: &str,
  module: &wgpu::ShaderModule,
  fragment: &str,
  key: PipelineKey,
) -> wgpu::RenderPipeline {
  device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
    label: Some(label),
    layout: Some(layout),
    vertex: wgpu::VertexState {
      module,
      entry_point: Some("vs_main"),
      buffers: &[Some(QUAD_LAYOUT), Some(INSTANCE_LAYOUT)],
      compilation_options: Default::default(),
    },
    fragment: Some(wgpu::FragmentState {
      module,
      entry_point: Some(fragment),
      targets: &[Some(wgpu::ColorTargetState {
        format: key.format,
        blend: Some(blend_state(key.blend)),
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
  fn alpha_blends_premultiplied_colour() {
    // Same result as straight alpha once the shader multiplies by alpha
    assert_eq!(
      blend_state(BlendMode::Alpha),
      wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING
    );
  }
}
