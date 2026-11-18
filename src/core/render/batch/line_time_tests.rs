//! The GPU's time for the line bench's scenes split into vertex and
//! fragment work, measured by hand (`cargo test --release -p ryo2d --lib
//! line_time -- --ignored --nocapture`): the line pipeline drawing them into
//! a 1920 x 1080 scene image as the world pass does, then with a fragment
//! stage that only writes a constant, then with every fragment but one
//! pixel's scissored away. The figures are recorded in
//! `docs/backlog/lines.md`.

use std::time::Duration;

use glam::Vec2;
use wgpu::util::DeviceExt;

use super::line_gpu_tests::LineGpuTest;
use super::lines::{LineStream, PolyPoint, PolyStyle};
use super::pipelines::{camera_layout, texture_layout};
use crate::core::graphics::bind_groups;
use crate::core::graphics::camera_gpu::CameraGlobals;
use crate::core::graphics::{Camera2d, CameraView};
use crate::core::render::SCENE_FORMAT;
use crate::core::shaders::compose::LINE_TEST;
use crate::core::trail::{LineCap, LineJoin};
use crate::core::ui::Color;

const RESOLUTION: (u32, u32) = (1920, 1080);
const WIDTHS: [f32; 6] = [0.5, 1.0, 2.0, 3.0, 6.0, 10.0];
const CAPS: [LineCap; 3] = [LineCap::Butt, LineCap::Round, LineCap::Square];
const JOINS: [LineJoin; 3] = [LineJoin::Miter, LineJoin::Bevel, LineJoin::Round];
const CELL: (f32, f32) = (48.0, 43.2);

/// The bench's shape `index` about `centre`, scaled by `scale`
fn points(index: usize, centre: Vec2, scale: f32) -> Vec<Vec2> {
  let count = 5 + index % 3;
  (0..count)
    .map(|i| {
      let along = i as f32 / (count - 1) as f32;
      let turn = if (i + index).is_multiple_of(2) {
        1.0
      } else {
        -1.0
      };
      let reach = 9.0 + (index % 5) as f32;
      centre
        + scale
          * Vec2::new(
            (along - 0.5) * 36.0,
            turn * reach * (0.4 + 0.6 * ((i % 3) as f32 / 2.0)),
          )
    })
    .collect()
}

fn color(index: usize) -> Color {
  Color::new(
    80 + (index * 37 % 170) as u8,
    80 + (index * 91 % 170) as u8,
    80 + (index * 53 % 170) as u8,
    if index.is_multiple_of(4) { 160 } else { 255 },
  )
}

fn add(
  stream: &mut LineStream,
  points: &[Vec2],
  index: usize,
  half: f32,
  join: LineJoin,
  cap: LineCap,
) {
  let last = (points.len() - 1) as f32;
  let poly: Vec<PolyPoint> = points
    .iter()
    .enumerate()
    .map(|(i, &pos)| PolyPoint {
      pos,
      half_width: half,
      color: color(index),
      u: i as f32 / last,
    })
    .collect();
  let style = PolyStyle {
    join,
    cap,
    closed: false,
    params: [0.0; 4],
    emission: 1.0,
    edge: None,
  };
  stream.polyline(&poly, &style);
}

/// The bench's mixed scene, its lines and trails alike
fn mixed() -> LineStream {
  let mut stream = LineStream::default();
  for index in 0..1000 {
    let centre = Vec2::new(
      ((index % 40) as f32 + 0.5) * CELL.0,
      ((index / 40) as f32 + 0.5) * CELL.1,
    );
    let width = WIDTHS[(index / 3) % WIDTHS.len()];
    add(
      &mut stream,
      &points(index, centre, 1.0),
      index,
      width / 2.0,
      JOINS[index % 3],
      CAPS[(index / 3) % 3],
    );
  }
  stream
}

/// The bench's `count` lines of `join`, filling the view
fn lines_only(join: LineJoin, count: usize) -> LineStream {
  let (width, height) = (RESOLUTION.0 as f32, RESOLUTION.1 as f32);
  let columns = ((count as f32 * width / height).sqrt().ceil() as usize).max(1);
  let rows = count.div_ceil(columns);
  let cell = Vec2::new(width / columns as f32, height / rows as f32);
  let scale = (cell.x / CELL.0).min(cell.y / CELL.1);
  let mut stream = LineStream::default();
  for index in 0..count {
    let centre = Vec2::new(
      ((index % columns) as f32 + 0.5) * cell.x,
      ((index / columns) as f32 + 0.5) * cell.y,
    );
    let line_width = (WIDTHS[(index / 3) % WIDTHS.len()] * scale).max(1.0);
    add(
      &mut stream,
      &points(index, centre, scale),
      index,
      line_width / 2.0,
      join,
      CAPS[index % 3],
    );
  }
  stream
}

/// How a run draws
#[derive(Clone, Copy, Debug)]
enum Run {
  /// The world pass's fragment stage
  Full,
  /// A fragment stage writing a constant
  Constant,
  /// The world pass's, every fragment but one pixel's scissored away
  OnePixel,
  /// An empty pass, its load and store alone (and what is encoded
  /// before the passes)
  Empty,
}

#[test]
#[ignore = "a measurement, run by hand"]
fn line_time_splits_into_vertex_and_fragment() {
  let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
    return;
  };
  if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
    eprintln!("no timestamps on this device");
    return;
  }
  let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
    label: Some("Line Time Shader"),
    source: wgpu::ShaderSource::Wgsl(LINE_TEST.into()),
  });
  let lines = LineGpuTest::new(&device);
  let camera_layout = camera_layout(&device);
  let texture_layout = texture_layout(&device);
  let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
    label: Some("Line Time Layout"),
    bind_group_layouts: &[
      Some(&camera_layout),
      Some(&texture_layout),
      Some(lines.layout()),
    ],
    immediate_size: 0,
  });
  let over = wgpu::BlendComponent {
    src_factor: wgpu::BlendFactor::One,
    dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
    operation: wgpu::BlendOperation::Add,
  };
  let pipeline = |fragment: &str| {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some("Line Time"),
      layout: Some(&layout),
      vertex: wgpu::VertexState {
        module: &module,
        entry_point: Some("vs_main"),
        buffers: &[],
        compilation_options: Default::default(),
      },
      fragment: Some(wgpu::FragmentState {
        module: &module,
        entry_point: Some(fragment),
        targets: &[Some(wgpu::ColorTargetState {
          format: SCENE_FORMAT,
          blend: Some(wgpu::BlendState {
            color: over,
            alpha: over,
          }),
          write_mask: wgpu::ColorWrites::ALL,
        })],
        compilation_options: Default::default(),
      }),
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
  };
  let (full, constant) = (pipeline("fs_main"), pipeline("fs_count"));
  let size = Vec2::new(RESOLUTION.0 as f32, RESOLUTION.1 as f32);
  let view = CameraView::compute(size / 2.0, 0.0, &Camera2d::default(), Vec2::ZERO, size);
  let globals = CameraGlobals::new(&view, RESOLUTION, 0.0, 0.0, false);
  let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
    label: Some("Line Time Camera"),
    contents: bytemuck::bytes_of(&globals),
    usage: wgpu::BufferUsages::UNIFORM,
  });
  let camera_group = bind_groups::create(
    &device,
    &wgpu::BindGroupDescriptor {
      label: Some("Line Time Camera"),
      layout: &camera_layout,
      entries: &[wgpu::BindGroupEntry {
        binding: 0,
        resource: uniform.as_entire_binding(),
      }],
    },
  );
  let blank = device.create_texture(&wgpu::TextureDescriptor {
    label: Some("Line Time Blank"),
    size: wgpu::Extent3d {
      width: 1,
      height: 1,
      depth_or_array_layers: 1,
    },
    mip_level_count: 1,
    sample_count: 1,
    dimension: wgpu::TextureDimension::D2,
    format: wgpu::TextureFormat::Rgba8Unorm,
    usage: wgpu::TextureUsages::TEXTURE_BINDING,
    view_formats: &[],
  });
  let blank_view = blank.create_view(&Default::default());
  let sampler = device.create_sampler(&Default::default());
  let texture_group = bind_groups::create(
    &device,
    &wgpu::BindGroupDescriptor {
      label: Some("Line Time Texture"),
      layout: &texture_layout,
      entries: &[
        wgpu::BindGroupEntry {
          binding: 0,
          resource: wgpu::BindingResource::TextureView(&blank_view),
        },
        wgpu::BindGroupEntry {
          binding: 1,
          resource: wgpu::BindingResource::Sampler(&sampler),
        },
      ],
    },
  );
  let target = device.create_texture(&wgpu::TextureDescriptor {
    label: Some("Line Time Image"),
    size: wgpu::Extent3d {
      width: RESOLUTION.0,
      height: RESOLUTION.1,
      depth_or_array_layers: 1,
    },
    mip_level_count: 1,
    sample_count: 1,
    dimension: wgpu::TextureDimension::D2,
    format: SCENE_FORMAT,
    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
    view_formats: &[],
  });
  let target_view = target.create_view(&Default::default());
  let set = device.create_query_set(&wgpu::QuerySetDescriptor {
    label: Some("Line Time Stamps"),
    ty: wgpu::QueryType::Timestamp,
    count: 2,
  });
  let resolved = device.create_buffer(&wgpu::BufferDescriptor {
    label: Some("Line Time Resolved"),
    size: 16,
    usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
    mapped_at_creation: false,
  });
  let read = device.create_buffer(&wgpu::BufferDescriptor {
    label: Some("Line Time Read"),
    size: 16,
    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
    mapped_at_creation: false,
  });
  let period = f64::from(queue.get_timestamp_period());
  let scenes = [
    ("mixed", mixed()),
    ("miter 1k", lines_only(LineJoin::Miter, 1_000)),
    ("miter 10k", lines_only(LineJoin::Miter, 10_000)),
    ("round 1k", lines_only(LineJoin::Round, 1_000)),
    ("round 10k", lines_only(LineJoin::Round, 10_000)),
    ("miter 100k", lines_only(LineJoin::Miter, 100_000)),
  ];
  for (name, stream) in &scenes {
    let uploaded = lines.upload(&device, &queue, stream);
    let mut figures = Vec::new();
    for run in [Run::Full, Run::Constant, Run::OnePixel, Run::Empty] {
      let mut times = Vec::new();
      for _ in 0..120 {
        let mut encoder = device.create_command_encoder(&Default::default());
        // Stamped from empty passes on the image, before what the frame
        // encodes ahead of its passes and after the draw (Metal stamps a
        // pass with work in it unreliably)
        let stamp = |encoder: &mut wgpu::CommandEncoder, index: u32| {
          drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Line Time Stamp"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
              view: &target_view,
              depth_slice: None,
              resolve_target: None,
              ops: wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
              },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: Some(wgpu::RenderPassTimestampWrites {
              query_set: &set,
              beginning_of_pass_write_index: (index == 1).then_some(1),
              end_of_pass_write_index: (index == 0).then_some(0),
            }),
            occlusion_query_set: None,
            multiview_mask: None,
          }));
        };
        stamp(&mut encoder, 0);
        lines.encode(&mut encoder, &uploaded);
        {
          let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Line Time Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
              view: &target_view,
              depth_slice: None,
              resolve_target: None,
              ops: wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
              },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
          });
          if !matches!(run, Run::Empty) {
            pass.set_pipeline(if matches!(run, Run::Constant) {
              &constant
            } else {
              &full
            });
            if matches!(run, Run::OnePixel) {
              pass.set_scissor_rect(960, 540, 1, 1);
            }
            pass.set_bind_group(0, &camera_group, &[]);
            pass.set_bind_group(1, &texture_group, &[]);
            lines.draw(&mut pass, &uploaded);
          }
        }
        stamp(&mut encoder, 1);
        encoder.resolve_query_set(&set, 0..2, &resolved, 0);
        encoder.copy_buffer_to_buffer(&resolved, 0, &read, 0, 16);
        queue.submit([encoder.finish()]);
        let slice = read.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        let ticks = {
          let bytes = slice.get_mapped_range().expect("the stamps map");
          let stamp = |at: usize| {
            let mut word = [0u8; 8];
            word.copy_from_slice(&bytes[at..at + 8]);
            u64::from_le_bytes(word)
          };
          (stamp(0), stamp(8))
        };
        read.unmap();
        if let Some(ticks) = ticks.1.checked_sub(ticks.0)
          && ticks > 0
        {
          times.push(Duration::from_nanos((ticks as f64 * period) as u64));
        }
      }
      times.sort();
      let median = times
        .get(times.len() / 2)
        .map_or(0.0, |t| t.as_secs_f64() * 1e6);
      figures.push(format!("{run:?} {median:.1} ({} timed)", times.len()));
    }
    eprintln!(
      "{name}: {} vertices; {}",
      uploaded.vertices,
      figures.join(", ")
    );
  }
}
