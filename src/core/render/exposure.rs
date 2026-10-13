//! Auto-exposure on the GPU: measure the scene image, keep the exposure the
//! tonemap pass reads

use crate::core::graphics::bind_groups::{BindGroupCache, Bound};
use crate::core::render::look::{AutoExposure, num};

/// `exposure.wgsl`'s parameters.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ExposureParams {
  ev_range: [f32; 4],
  speed: [f32; 4],
}

/// `settings` for the shader, sanitized, with `dt` seconds since the last frame.
fn params(settings: &AutoExposure, dt: f32) -> ExposureParams {
  let min_ev = num(settings.min_ev, -4.0, -16.0, 16.0);
  let max_ev = num(settings.max_ev, 4.0, -16.0, 16.0).max(min_ev);
  ExposureParams {
    ev_range: [
      min_ev,
      max_ev,
      num(settings.compensation, 0.0, -16.0, 16.0),
      0.0,
    ],
    speed: [
      num(settings.speed_up, 3.0, 0.0, 100.0),
      num(settings.speed_down, 1.0, 0.0, 100.0),
      num(dt, 0.0, 0.0, 0.25),
      0.0,
    ],
  }
}

/// The histogram and stored exposure, and the two compute pipelines that
/// fill them.
pub(crate) struct ExposureGpu {
  layout: wgpu::BindGroupLayout,
  histogram_pipeline: wgpu::ComputePipeline,
  average_pipeline: wgpu::ComputePipeline,
  histogram: wgpu::Buffer,
  state: wgpu::Buffer,
  params: wgpu::Buffer,
  bind_groups: BindGroupCache,
  /// It measured last frame: off and on again, it starts at its target
  was_on: bool,
}

impl ExposureGpu {
  pub(crate) fn new(device: &wgpu::Device) -> Self {
    let storage = |binding| wgpu::BindGroupLayoutEntry {
      binding,
      visibility: wgpu::ShaderStages::COMPUTE,
      ty: wgpu::BindingType::Buffer {
        ty: wgpu::BufferBindingType::Storage { read_only: false },
        has_dynamic_offset: false,
        min_binding_size: None,
      },
      count: None,
    };
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
      label: Some("Exposure Bind Group Layout"),
      entries: &[
        wgpu::BindGroupLayoutEntry {
          binding: 0,
          visibility: wgpu::ShaderStages::COMPUTE,
          ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
          },
          count: None,
        },
        storage(1),
        storage(2),
        wgpu::BindGroupLayoutEntry {
          binding: 3,
          visibility: wgpu::ShaderStages::COMPUTE,
          ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
          },
          count: None,
        },
      ],
    });
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some("Exposure Shader"),
      source: wgpu::ShaderSource::Wgsl(crate::core::shaders::compose::EXPOSURE.into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("Exposure Pipeline Layout"),
      bind_group_layouts: &[Some(&layout)],
      immediate_size: 0,
    });
    let pipeline = |entry: &str| {
      device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(entry),
        layout: Some(&pipeline_layout),
        module: &module,
        entry_point: Some(entry),
        compilation_options: Default::default(),
        cache: None,
      })
    };
    let buffer = |label, size, usage| {
      device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage,
        mapped_at_creation: false,
      })
    };
    use wgpu::BufferUsages as U;
    Self {
      histogram_pipeline: pipeline("histogram_main"),
      average_pipeline: pipeline("average_main"),
      histogram: buffer("Exposure Histogram", 64 * 4, U::STORAGE | U::COPY_DST),
      state: buffer("Exposure State", 16, U::STORAGE | U::COPY_DST | U::COPY_SRC),
      params: buffer(
        "Exposure Params",
        std::mem::size_of::<ExposureParams>() as u64,
        U::UNIFORM | U::COPY_DST,
      ),
      layout,
      bind_groups: BindGroupCache::default(),
      was_on: false,
    }
  }

  /// The stored exposure (`exposure.wgsl`'s `State`), which the tonemap
  /// pass reads.
  pub(crate) fn state(&self) -> &wgpu::Buffer {
    &self.state
  }

  /// Measure `scene` and move the stored exposure toward its target; with
  /// `settings` off, forget it so it starts at its target when back on.
  #[allow(clippy::too_many_arguments)]
  pub(crate) fn measure(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    scene: &wgpu::TextureView,
    (width, height): (u32, u32),
    settings: Option<&AutoExposure>,
    dt: f32,
  ) {
    let Some(settings) = settings else {
      if std::mem::replace(&mut self.was_on, false) {
        queue.write_buffer(&self.state, 0, &[0u8; 16]);
      }
      return;
    };
    self.was_on = true;
    queue.write_buffer(&self.params, 0, bytemuck::bytes_of(&params(settings, dt)));
    encoder.clear_buffer(&self.histogram, 0, None);
    let group = self.bind_groups.get(
      device,
      "Exposure Bind Group",
      &self.layout,
      &[
        Bound::View(scene.clone()),
        Bound::Buffer(self.histogram.clone()),
        Bound::Buffer(self.state.clone()),
        Bound::Buffer(self.params.clone()),
      ],
    );
    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
      label: Some("Exposure Pass"),
      timestamp_writes: None,
    });
    pass.set_bind_group(0, &group, &[]);
    pass.set_pipeline(&self.histogram_pipeline);
    pass.dispatch_workgroups(width.div_ceil(16), height.div_ceil(16), 1);
    pass.set_pipeline(&self.average_pipeline);
    pass.dispatch_workgroups(1, 1, 1);
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  /// The f16 bits of 2^e, a normal power of two.
  fn f16_power_of_two(e: i32) -> u16 {
    ((e + 15) as u16) << 10
  }

  /// A 16x16 float image: the left half grey 2^-6, the right half 2^-4.
  fn two_greys(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::TextureView {
    let mut texels: Vec<u16> = Vec::new();
    for _ in 0..16 {
      for x in 0..16 {
        let v = f16_power_of_two(if x < 8 { -6 } else { -4 });
        texels.extend([v, v, v, f16_power_of_two(0)]);
      }
    }
    let size = wgpu::Extent3d {
      width: 16,
      height: 16,
      depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
      label: Some("Two Greys"),
      size,
      mip_level_count: 1,
      sample_count: 1,
      dimension: wgpu::TextureDimension::D2,
      format: wgpu::TextureFormat::Rgba16Float,
      usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
      view_formats: &[],
    });
    queue.write_texture(
      texture.as_image_copy(),
      bytemuck::cast_slice(&texels),
      wgpu::TexelCopyBufferLayout {
        offset: 0,
        bytes_per_row: Some(16 * 8),
        rows_per_image: Some(16),
      },
      size,
    );
    texture.create_view(&wgpu::TextureViewDescriptor::default())
  }

  /// What `average_main` computes, on the CPU: the target EV of `lumas`.
  fn cpu_target(lumas: &[f32], settings: &AutoExposure) -> f32 {
    let mut bins = [0u32; 64];
    for &luma in lumas {
      let t = ((luma.max(1e-12).log2() + 12.0) / 16.0).clamp(0.0, 1.0);
      bins[((t * 64.0) as usize).min(63)] += 1;
    }
    let total = lumas.len() as f32;
    let (low, high) = (total * 0.05, total * 0.95);
    let (mut seen, mut sum, mut weight) = (0.0_f32, 0.0_f32, 0.0_f32);
    for (i, &count) in bins.iter().enumerate() {
      let count = count as f32;
      let kept = ((seen + count).min(high) - seen.max(low)).max(0.0);
      sum += kept * (-12.0 + (i as f32 + 0.5) * 16.0 / 64.0);
      weight += kept;
      seen += count;
    }
    (0.18_f32.log2() - sum / weight + settings.compensation).clamp(settings.min_ev, settings.max_ev)
  }

  /// The exposure the GPU stored.
  fn read_ev(device: &wgpu::Device, queue: &wgpu::Queue, gpu: &ExposureGpu) -> f32 {
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
      label: Some("Exposure Readback"),
      size: 16,
      usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
      mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.copy_buffer_to_buffer(gpu.state(), 0, &staging, 0, 16);
    queue.submit([encoder.finish()]);
    let slice = staging.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let bytes = slice.get_mapped_range().unwrap();
    f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
  }

  fn measure_once(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    gpu: &mut ExposureGpu,
    scene: &wgpu::TextureView,
    settings: &AutoExposure,
    dt: f32,
  ) {
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    gpu.measure(
      device,
      queue,
      &mut encoder,
      scene,
      (16, 16),
      Some(settings),
      dt,
    );
    queue.submit([encoder.finish()]);
  }

  #[test]
  fn the_first_measure_is_the_histogram_target() {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let scene = two_greys(&device, &queue);
    let settings = AutoExposure::default();
    let mut gpu = ExposureGpu::new(&device);
    measure_once(&device, &queue, &mut gpu, &scene, &settings, 0.0);
    let lumas: Vec<f32> = (0..256)
      .map(|i| {
        if i % 16 < 8 {
          2f32.powi(-6)
        } else {
          2f32.powi(-4)
        }
      })
      .collect();
    let ev = read_ev(&device, &queue, &gpu);
    assert!((ev - cpu_target(&lumas, &settings)).abs() < 1e-3, "{ev}");
    // The exact log average is 2^-5: within half a bin of its target
    assert!((ev - (0.18_f32.log2() + 5.0)).abs() <= 0.13, "{ev}");
  }

  #[test]
  fn later_measures_move_at_the_speed_limit() {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let scene = two_greys(&device, &queue);
    let mut gpu = ExposureGpu::new(&device);
    let low = AutoExposure {
      compensation: -2.0,
      ..AutoExposure::default()
    };
    measure_once(&device, &queue, &mut gpu, &scene, &low, 0.0);
    let start = read_ev(&device, &queue, &gpu);
    // Two stops up wanted; 3 EV/s for a tenth of a second allows 0.3
    measure_once(
      &device,
      &queue,
      &mut gpu,
      &scene,
      &AutoExposure::default(),
      0.1,
    );
    let moved = read_ev(&device, &queue, &gpu) - start;
    assert!((moved - 0.3).abs() < 1e-3, "{moved}");
  }
}
