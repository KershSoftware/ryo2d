//! The batch's render pipelines, made on first use.

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use super::polyline::Vertex2D;
use super::{BatchShader, BlendMode, LightStep, PageKind, Stream};
use crate::core::graphics::SpriteInstance;
use crate::core::render::UI_FORMAT;
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

/// The blend state and colour writes of a draw into a light buffer.
pub(crate) fn light_blend(step: LightStep) -> (wgpu::BlendState, wgpu::ColorWrites) {
  use wgpu::BlendFactor::{DstAlpha, One, OneMinusDstAlpha, OneMinusSrcAlpha, Zero};
  let add = |src_factor, dst_factor| wgpu::BlendComponent {
    src_factor,
    dst_factor,
    operation: wgpu::BlendOperation::Add,
  };
  let keep = add(Zero, One);
  let (color, alpha, writes) = match step {
    // Light adds up; the alpha, a shadowed light's mask, is left alone
    LightStep::Add => (add(One, One), keep, wgpu::ColorWrites::COLOR),
    LightStep::ResetMask => (keep, add(One, Zero), wgpu::ColorWrites::ALPHA),
    LightStep::Shadow => (keep, add(Zero, OneMinusSrcAlpha), wgpu::ColorWrites::ALPHA),
    LightStep::Masked => (add(DstAlpha, One), keep, wgpu::ColorWrites::COLOR),
    LightStep::Coloured => (add(OneMinusDstAlpha, One), keep, wgpu::ColorWrites::COLOR),
  };
  (wgpu::BlendState { color, alpha }, writes)
}

/// What a pipeline is made for: two draws with equal keys share one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct PipelineKey {
  pub(crate) shader: BatchShader,
  pub(crate) blend: BlendMode,
  pub(crate) stream: Stream,
  pub(crate) page: PageKind,
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
  /// Camera only (untextured lines, lights and shadows)
  camera_only: wgpu::PipelineLayout,
  sprite: wgpu::ShaderModule,
  line: wgpu::ShaderModule,
  light: wgpu::ShaderModule,
  shadow: wgpu::ShaderModule,
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
    let camera_only = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("Batch Line Pipeline Layout"),
      bind_group_layouts: &[Some(camera)],
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
      camera_only,
      sprite: module("Sprite Shader", crate::core::shaders::compose::SPRITE),
      line: module("Line Shader", crate::core::shaders::compose::LINE),
      light: module("Light Shader", crate::core::shaders::compose::LIGHT),
      shadow: module("Shadow Shader", crate::core::shaders::compose::SHADOW),
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
    let linear = key.page == PageKind::Linear;
    let (module, layout, fragment) = match (key.shader, key.stream, premultiplied, linear) {
      // Render targets hold linear premultiplied colour, whatever the blend
      (BatchShader::Sprite, Stream::Quads, _, true) => {
        (&self.sprite, &self.layout, "fs_base_linear")
      }
      (BatchShader::Sprite, Stream::Quads, false, false) => (&self.sprite, &self.layout, "fs_base"),
      (BatchShader::Sprite, Stream::Quads, true, false) => {
        (&self.sprite, &self.layout, "fs_base_premultiplied")
      }
      (BatchShader::SpriteEffect, Stream::Quads, _, true) => {
        (&self.sprite, &self.layout, "fs_effect_linear")
      }
      (BatchShader::SpriteEffect, Stream::Quads, false, false) => {
        (&self.sprite, &self.layout, "fs_effect")
      }
      (BatchShader::SpriteEffect, Stream::Quads, true, false) => {
        (&self.sprite, &self.layout, "fs_effect_premultiplied")
      }
      (BatchShader::Line, Stream::Triangles, _, _) => (&self.line, &self.camera_only, "fs_main"),
      (BatchShader::LineTextured, Stream::Triangles, _, _) => {
        (&self.line, &self.layout, "fs_textured")
      }
      (BatchShader::Custom(source), Stream::Quads, _, _) => {
        let Some((name, wgsl)) = custom else {
          return;
        };
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
          label: Some(name),
          source: wgpu::ShaderSource::Wgsl(wgsl.into()),
        });
        let fragment = if linear {
          "fs_custom_linear"
        } else if premultiplied {
          "fs_custom_premultiplied"
        } else {
          "fs_custom"
        };
        let pipeline = batch_pipeline(device, &self.layout, name, &module, fragment, key);
        // Anything naga accepted but the device rejects is found by a
        // later frame's `settle`, without waiting on the GPU here
        slot.insert(Built {
          state: Checked::pending(pipeline, scope),
          custom: Some((name.to_string(), source)),
        });
        return;
      }
      (BatchShader::TextMask, Stream::Quads, _, _) => (&self.sprite, &self.layout, "fs_text_mask"),
      (BatchShader::TextColor, Stream::Quads, _, _) => {
        (&self.sprite, &self.layout, "fs_text_color")
      }
      (BatchShader::TextMsdf, Stream::Quads, _, _) => (&self.sprite, &self.layout, "fs_text_msdf"),
      (BatchShader::Light(LightStep::Shadow), Stream::Triangles, _, _) => {
        (&self.shadow, &self.camera_only, "fs_shadow")
      }
      (BatchShader::Light(step), Stream::Quads, _, _) => {
        (&self.light, &self.camera_only, step.fragment())
      }
      // No such pipeline: sprites are quads and lines triangles
      _ => return,
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

/// A pipeline drawing `key`'s stream (instanced quads or indexed triangles)
/// with `module`'s `vs_main` and `fragment`, blended and formatted for `key`.
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
      entry_point: Some("vs_main"),
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
  fn lights_add_up_and_leave_the_mask_alone() {
    let (blend, writes) = light_blend(LightStep::Add);
    assert_eq!(blend.color.src_factor, wgpu::BlendFactor::One);
    assert_eq!(blend.color.dst_factor, wgpu::BlendFactor::One);
    assert_eq!(writes, wgpu::ColorWrites::COLOR);
  }

  #[test]
  fn shadowed_lights_mask_their_own_light() {
    use wgpu::BlendFactor as F;
    let (reset, reset_writes) = light_blend(LightStep::ResetMask);
    assert_eq!(
      (reset.alpha.src_factor, reset.alpha.dst_factor),
      (F::One, F::Zero)
    );
    assert_eq!(reset_writes, wgpu::ColorWrites::ALPHA);
    let (shadow, shadow_writes) = light_blend(LightStep::Shadow);
    assert_eq!(
      (shadow.alpha.src_factor, shadow.alpha.dst_factor),
      (F::Zero, F::OneMinusSrcAlpha)
    );
    assert_eq!(shadow_writes, wgpu::ColorWrites::ALPHA);
    let (masked, _) = light_blend(LightStep::Masked);
    assert_eq!(
      (masked.color.src_factor, masked.color.dst_factor),
      (F::DstAlpha, F::One)
    );
    let (coloured, writes) = light_blend(LightStep::Coloured);
    assert_eq!(
      (coloured.color.src_factor, coloured.color.dst_factor),
      (F::OneMinusDstAlpha, F::One)
    );
    assert_eq!(writes, wgpu::ColorWrites::COLOR);
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
