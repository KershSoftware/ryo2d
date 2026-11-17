//! The GPU's line pieces (`line_pieces.wgsl`, through `line.wgsl`) against
//! the CPU builder (`polyline.rs`), the reference: the vertices they build,
//! triangle for triangle, and the pixels their pushed triangles cover.

use glam::Vec2;
use wgpu::util::DeviceExt;

use super::gpu::line_group;
use super::lines::LineStream;
use super::pipelines::{camera_layout, line_layout_for};
use super::polyline::{PolyPoint, PolyStyle, Triangles, Vertex2D};
use crate::core::graphics::bind_groups;
use crate::core::graphics::camera_gpu::{CameraGlobals, pixels_per_unit};
use crate::core::graphics::{Camera2d, CameraView};
use crate::core::shaders::compose::LINE;
use crate::core::trail::{LineCap, LineJoin};
use crate::core::ui::Color;

const JOINS: [LineJoin; 3] = [LineJoin::Miter, LineJoin::Bevel, LineJoin::Round];
const CAPS: [LineCap; 3] = [LineCap::Butt, LineCap::Square, LineCap::Round];

/// A line to test: its points, their half widths, and whether it closes
#[derive(Clone)]
struct TestLine {
  points: Vec<Vec2>,
  halves: Vec<f32>,
  closed: bool,
}

impl TestLine {
  fn even(points: &[(f32, f32)], half: f32, closed: bool) -> Self {
    Self {
      points: points.iter().map(|&(x, y)| Vec2::new(x, y)).collect(),
      halves: vec![half; points.len()],
      closed,
    }
  }

  fn tapered(points: &[(f32, f32)], halves: &[f32]) -> Self {
    Self {
      halves: halves.to_vec(),
      ..Self::even(points, 1.0, false)
    }
  }

  fn moved(&self, by: Vec2) -> Self {
    Self {
      points: self.points.iter().map(|&p| p + by).collect(),
      ..self.clone()
    }
  }

  fn poly(&self) -> Vec<PolyPoint> {
    let last = self.points.len().saturating_sub(1).max(1) as f32;
    self
      .points
      .iter()
      .zip(&self.halves)
      .enumerate()
      .map(|(i, (&pos, &half_width))| PolyPoint {
        pos,
        half_width,
        color: Color::new(
          (10 + i * 20 % 240) as u8,
          200,
          (255 - i * 7 % 200) as u8,
          200,
        ),
        u: i as f32 / last,
      })
      .collect()
  }

  fn style(&self, join: LineJoin, cap: LineCap) -> PolyStyle {
    PolyStyle {
      join,
      cap,
      closed: self.closed,
      params: [0.0; 4],
      emission: 1.0,
    }
  }

  /// The pairs of segments (as end points) meeting at a corner whose inner
  /// side splits, or every pair when `all`
  fn split_corners(&self, all: bool) -> Vec<[(Vec2, Vec2); 2]> {
    let n = self.points.len();
    let at = |i: usize| self.points[i % n];
    let corners = if self.closed {
      0..n
    } else {
      1..n.saturating_sub(1)
    };
    corners
      .filter_map(|i| {
        let (a, p, b) = (at(i + n - 1), at(i), at(i + 1));
        let (d0, d1) = ((p - a).normalize(), (b - p).normalize());
        let miter = (d0.perp() + d1.perp()) / (1.0 + d0.perp().dot(d1.perp()));
        let shortest = p.distance(a).min(p.distance(b));
        // (A U-turn's miter is no number: it never shares)
        let shares = miter.length() * self.halves[i % n] <= shortest / 2.0;
        let splits = !shares;
        (all || splits).then_some([(a, p), (p, b)])
      })
      .collect()
  }
}

/// The lines the tests build: corners both ways, gentle and sharp, inner
/// sides that split (short segments), a U-turn and one nearly, closed
/// outlines, single segments, tapered lines, a long wandering line, and one
/// far from the origin
fn lines() -> Vec<TestLine> {
  let zigzag = [(0.0, 0.0), (10.0, 3.0), (20.0, 0.0), (30.0, 3.0)];
  let mut wander = Vec::new();
  let mut halves = Vec::new();
  let (mut at, mut heading, mut seed) = (Vec2::ZERO, 0.3f32, 12345u32);
  let mut next = || {
    seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    (seed >> 8) as f32 / (1u32 << 24) as f32
  };
  for _ in 0..14 {
    wander.push((at.x, at.y));
    halves.push(0.5 + 2.5 * next());
    heading += (next() - 0.5) * 4.0;
    at += Vec2::from_angle(heading) * (2.0 + 9.0 * next());
  }
  let circle: Vec<(f32, f32)> = (0..24)
    .map(|i| {
      let a = i as f32 * std::f32::consts::TAU / 24.0;
      (20.0 * a.cos(), 20.0 * a.sin())
    })
    .collect();
  vec![
    TestLine::even(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)], 1.0, false),
    TestLine::even(&[(0.0, 0.0), (10.0, 0.0), (10.0, -10.0)], 1.0, false),
    TestLine::even(&zigzag, 1.0, false),
    TestLine::even(&[(0.0, 0.0), (10.0, 0.0), (2.0, 3.0)], 1.0, false),
    TestLine::even(&[(0.0, 0.0), (10.0, 0.0), (0.0, 0.1)], 1.0, false),
    TestLine::even(&[(0.0, 0.0), (10.0, 0.0), (0.0, 0.0)], 1.0, false),
    TestLine::even(
      &[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)],
      1.0,
      true,
    ),
    TestLine::even(&[(0.0, 0.0), (10.0, 0.0), (5.0, 8.0)], 0.5, true),
    TestLine::even(&[(1.0, 2.0), (7.0, 9.0)], 1.5, false),
    TestLine::tapered(
      &[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 12.0)],
      &[1.5, 1.0, 0.75, 0.25],
    ),
    TestLine::tapered(&[(0.0, 0.0), (10.0, 0.0)], &[1.0, 0.0]),
    TestLine::even(
      &[(0.0, 0.0), (3.0, 0.0), (3.0, 2.5), (6.0, 2.0), (6.5, 6.0)],
      1.0,
      false,
    ),
    TestLine::even(&[(0.0, 0.0), (3.0, 0.0), (3.0, 3.0), (0.0, 3.0)], 1.0, true),
    TestLine::tapered(&wander, &halves),
    TestLine::even(&circle, 2.0, true),
    TestLine::even(&zigzag, 1.25, false).moved(Vec2::new(1000.5, 700.25)),
    TestLine::even(
      &[(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (4.0, 2.0), (5.0, 0.5)],
      1.0,
      false,
    ),
  ]
}

/// A vertex as `cs_line_dump` writes it (`LineDump` in `line.wgsl`)
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Dumped {
  position: [f32; 2],
  uv: [f32; 2],
  across: [f32; 2],
  push: [f32; 2],
  color: [f32; 4],
  dist: f32,
  second: f32,
  half: f32,
  limit: f32,
  dist_rate: f32,
  second_rate: f32,
  k: f32,
  flags: u32,
  effect_u: f32,
  _pad: [f32; 3],
}

fn storage(device: &wgpu::Device, label: &str, bytes: &[u8]) -> wgpu::Buffer {
  // A binding needs a buffer of some size
  let bytes = if bytes.is_empty() {
    &[0u8; 16][..]
  } else {
    bytes
  };
  device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
    label: Some(label),
    contents: bytes,
    usage: wgpu::BufferUsages::STORAGE,
  })
}

/// `stream`'s buffers and their bind group, in `layout`
pub(super) fn stream_group(
  device: &wgpu::Device,
  layout: &wgpu::BindGroupLayout,
  stream: &LineStream,
) -> wgpu::BindGroup {
  let table = storage(
    device,
    "Test Line Table",
    bytemuck::cast_slice(&stream.lines),
  );
  let points = storage(
    device,
    "Test Line Points",
    bytemuck::cast_slice(&stream.points),
  );
  let codes = storage(
    device,
    "Test Line Codes",
    bytemuck::cast_slice(&stream.codes),
  );
  line_group(device, layout, [&table, &points, &codes])
}

fn module(device: &wgpu::Device) -> wgpu::ShaderModule {
  device.create_shader_module(wgpu::ShaderModuleDescriptor {
    label: Some("Line Test Shader"),
    source: wgpu::ShaderSource::Wgsl(LINE.into()),
  })
}

/// Every vertex of `stream`, three a triangle, as the GPU builds it
fn dump(device: &wgpu::Device, queue: &wgpu::Queue, stream: &LineStream) -> Vec<Dumped> {
  let count = 3 * stream.codes.len();
  let stream_layout = line_layout_for(device, wgpu::ShaderStages::COMPUTE);
  let dump_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
    label: Some("Line Dump Layout"),
    entries: &[wgpu::BindGroupLayoutEntry {
      binding: 0,
      visibility: wgpu::ShaderStages::COMPUTE,
      ty: wgpu::BindingType::Buffer {
        ty: wgpu::BufferBindingType::Storage { read_only: false },
        has_dynamic_offset: false,
        min_binding_size: None,
      },
      count: None,
    }],
  });
  let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
    label: Some("Line Dump Pipeline Layout"),
    bind_group_layouts: &[None, None, Some(&stream_layout), Some(&dump_layout)],
    immediate_size: 0,
  });
  let module = module(device);
  let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
    label: Some("Line Dump"),
    layout: Some(&layout),
    module: &module,
    entry_point: Some("cs_line_dump"),
    compilation_options: Default::default(),
    cache: None,
  });
  let size = (count * std::mem::size_of::<Dumped>()) as u64;
  let out = device.create_buffer(&wgpu::BufferDescriptor {
    label: Some("Line Dump"),
    size,
    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
    mapped_at_creation: false,
  });
  let read = device.create_buffer(&wgpu::BufferDescriptor {
    label: Some("Line Dump Readback"),
    size,
    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
    mapped_at_creation: false,
  });
  let stream_group = stream_group(device, &stream_layout, stream);
  let dump_group = bind_groups::create(
    device,
    &wgpu::BindGroupDescriptor {
      label: Some("Line Dump Group"),
      layout: &dump_layout,
      entries: &[wgpu::BindGroupEntry {
        binding: 0,
        resource: out.as_entire_binding(),
      }],
    },
  );
  let mut encoder = device.create_command_encoder(&Default::default());
  {
    let mut pass = encoder.begin_compute_pass(&Default::default());
    pass.set_pipeline(&pipeline);
    pass.set_bind_group(2, &stream_group, &[]);
    pass.set_bind_group(3, &dump_group, &[]);
    pass.dispatch_workgroups((count as u32).div_ceil(64), 1, 1);
  }
  encoder.copy_buffer_to_buffer(&out, 0, &read, 0, size);
  queue.submit([encoder.finish()]);
  let slice = read.slice(..);
  slice.map_async(wgpu::MapMode::Read, |_| {});
  let _ = device.poll(wgpu::PollType::wait_indefinitely());
  let dumped = {
    let view = slice.get_mapped_range().expect("the dump maps");
    bytemuck::cast_slice::<u8, Dumped>(&view).to_vec()
  };
  read.unmap();
  dumped
}

/// The reference's fields of `v` and the GPU's of `d`, by name, with how
/// far apart they may be
fn compare(v: &Vertex2D, d: &Dumped) -> Vec<(&'static str, f32, f32)> {
  let color = v.color.map(|c| c as f32 / 255.0);
  let mut fields = vec![
    ("position.x", v.position[0], d.position[0]),
    ("position.y", v.position[1], d.position[1]),
    ("uv.x", v.uv[0], d.uv[0]),
    ("uv.y", v.uv[1], d.uv[1]),
    ("across.x", v.across[0], d.across[0]),
    ("across.y", v.across[1], d.across[1]),
    ("push.x", v.push[0], d.push[0]),
    ("push.y", v.push[1], d.push[1]),
    ("dist", v.dist, d.dist),
    ("second", v.second, d.second),
    ("half", v.half, d.half),
    ("limit", v.limit, d.limit),
    ("dist_rate", v.dist_rate, d.dist_rate),
    ("second_rate", v.second_rate, d.second_rate),
    ("k", v.k, d.k),
    ("effect_u", v.effect_u, d.effect_u),
  ];
  for (k, name) in ["color.r", "color.g", "color.b", "color.a"]
    .into_iter()
    .enumerate()
  {
    fields.push((name, color[k], d.color[k]));
  }
  fields
}

#[test]
fn the_gpu_builds_the_reference_s_vertices() {
  // Every vertex of every triangle, in the reference's order, equal within
  // f32's rounding: every join and cap, closed, tapered, split and U-turned
  let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
    return;
  };
  let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
  let (mut stream, mut reference) = (LineStream::default(), Triangles::default());
  let mut names = Vec::new();
  for (index, line) in lines().iter().enumerate() {
    for join in JOINS {
      for cap in CAPS {
        let (points, style) = (line.poly(), line.style(join, cap));
        let gpu = stream.polyline(&points, &style);
        let cpu = reference.polyline(&points, &style);
        assert_eq!(
          gpu, cpu,
          "line {index} {join:?} {cap:?}: the triangles' counts"
        );
        names.push((gpu, format!("line {index} {join:?} {cap:?}")));
      }
    }
  }
  let dumped = dump(&device, &queue, &stream);
  let error = pollster::block_on(scope.pop());
  assert!(error.is_none(), "{error:?}");
  assert_eq!(dumped.len(), reference.indices.len());
  let mut failures = Vec::new();
  let mut worst = (0.0f32, String::new());
  for (corner, (&index, d)) in reference.indices.iter().zip(&dumped).enumerate() {
    let v = &reference.vertices[index as usize];
    let name = names
      .iter()
      .find(|(range, _)| range.contains(&(corner as u32)))
      .map_or("", |(_, name)| name.as_str());
    let what = format!("{name}, triangle {} corner {}", corner / 3, corner % 3);
    if v.flags != d.flags {
      failures.push(format!(
        "{what}: flags {} on the CPU, {} on the GPU",
        v.flags, d.flags
      ));
      continue;
    }
    for (field, cpu, gpu) in compare(v, d) {
      let scale = cpu.abs().max(gpu.abs()).max(1.0);
      let off = (cpu - gpu).abs() / scale;
      if off > worst.0 {
        worst = (off, format!("{what} {field}: {cpu} {gpu}"));
      }
      if off > 2e-4 {
        failures.push(format!(
          "{what} {field}: {cpu} on the CPU, {gpu} on the GPU"
        ));
      }
    }
  }
  eprintln!("{} vertices; worst: {worst:?}", dumped.len());
  assert!(
    failures.is_empty(),
    "{} fields differ:\n{}",
    failures.len(),
    failures[..failures.len().min(30)].join("\n")
  );
}

/// The count pipeline: the line pipeline's vertex stage, each fragment
/// adding one to its pixel
struct Counter {
  pipeline: wgpu::RenderPipeline,
  camera_layout: wgpu::BindGroupLayout,
  stream_layout: wgpu::BindGroupLayout,
}

impl Counter {
  fn new(device: &wgpu::Device) -> Self {
    let camera_layout = camera_layout(device);
    let stream_layout = line_layout_for(device, wgpu::ShaderStages::VERTEX);
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("Line Count Layout"),
      bind_group_layouts: &[Some(&camera_layout), None, Some(&stream_layout)],
      immediate_size: 0,
    });
    let module = module(device);
    let add = wgpu::BlendComponent {
      src_factor: wgpu::BlendFactor::One,
      dst_factor: wgpu::BlendFactor::One,
      operation: wgpu::BlendOperation::Add,
    };
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some("Line Count"),
      layout: Some(&layout),
      vertex: wgpu::VertexState {
        module: &module,
        entry_point: Some("vs_main"),
        buffers: &[],
        compilation_options: Default::default(),
      },
      fragment: Some(wgpu::FragmentState {
        module: &module,
        entry_point: Some("fs_count"),
        targets: &[Some(wgpu::ColorTargetState {
          format: wgpu::TextureFormat::Rgba8Unorm,
          blend: Some(wgpu::BlendState {
            color: add,
            alpha: add,
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
    });
    Self {
      pipeline,
      camera_layout,
      stream_layout,
    }
  }

  /// How many fragments each pixel of a `size` image took, the line in
  /// `stream` drawn through `view`
  fn count(
    &self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    stream: &LineStream,
    view: &CameraView,
    size: (u32, u32),
  ) -> Vec<u8> {
    let globals = CameraGlobals::new(view, size, 0.0, 0.0, false);
    let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
      label: Some("Line Count Camera"),
      contents: bytemuck::bytes_of(&globals),
      usage: wgpu::BufferUsages::UNIFORM,
    });
    let camera = bind_groups::create(
      device,
      &wgpu::BindGroupDescriptor {
        label: Some("Line Count Camera"),
        layout: &self.camera_layout,
        entries: &[wgpu::BindGroupEntry {
          binding: 0,
          resource: uniform.as_entire_binding(),
        }],
      },
    );
    let lines = stream_group(device, &self.stream_layout, stream);
    let texture = device.create_texture(&wgpu::TextureDescriptor {
      label: Some("Line Count Image"),
      size: wgpu::Extent3d {
        width: size.0,
        height: size.1,
        depth_or_array_layers: 1,
      },
      mip_level_count: 1,
      sample_count: 1,
      dimension: wgpu::TextureDimension::D2,
      format: wgpu::TextureFormat::Rgba8Unorm,
      usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
      view_formats: &[],
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
      let target = texture.create_view(&Default::default());
      let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Line Count Pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
          view: &target,
          depth_slice: None,
          resolve_target: None,
          ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            store: wgpu::StoreOp::Store,
          },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
      });
      pass.set_pipeline(&self.pipeline);
      pass.set_bind_group(0, &camera, &[]);
      pass.set_bind_group(2, &lines, &[]);
      pass.draw(0..3 * stream.codes.len() as u32, 0..1);
    }
    queue.submit([encoder.finish()]);
    let image =
      crate::core::capture::read_texture_rgba(device, queue, &texture).expect("read back");
    image.pixels().map(|pixel| pixel.0[0]).collect()
  }
}

/// Twice the signed area of `a b c`, in f64
fn orient(a: Vec2, b: Vec2, c: Vec2) -> f64 {
  let (a, b, c) = (a.as_dvec2(), b.as_dvec2(), c.as_dvec2());
  (b - a).perp_dot(c - a)
}

fn strictly_inside([a, b, c]: &[Vec2; 3], at: Vec2) -> bool {
  let sides = [orient(*a, *b, at), orient(*b, *c, at), orient(*c, *a, at)];
  sides.iter().all(|&s| s > 0.0) || sides.iter().all(|&s| s < 0.0)
}

fn distance_to_segment(at: Vec2, (a, b): (Vec2, Vec2)) -> f32 {
  let t = ((at - a).dot(b - a) / (b - a).length_squared()).clamp(0.0, 1.0);
  at.distance(a + (b - a) * t)
}

#[test]
fn a_translucent_line_drawn_on_the_gpu_counts_every_pixel_once() {
  // Every join and cap, at two zooms and four sub-pixel places (some
  // putting edges and corners on pixel centres), drawn with each fragment
  // counted: every pixel the reference's mesh, pushed a pixel, covers is
  // drawn exactly as often as that mesh covers it, so the pieces leave no
  // crack and cover nothing twice. A pixel on the mesh's outer edge may go
  // either way (the fill rule decides). The mesh covers a pixel twice only
  // where a split corner's inner side overlaps, as it does unpushed: within
  // its segments' pushed quads, which is the one place a pixel counts two
  // (measured and printed)
  let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
    return;
  };
  let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
  let counter = Counter::new(&device);
  let offsets = [
    Vec2::ZERO,
    Vec2::new(0.5, 0.5),
    Vec2::new(0.25, 0.125),
    Vec2::new(0.6123, 0.3377),
  ];
  // Samples on a small ring about each pixel's centre: a pixel on an edge
  // or a corner between triangles counts one in each, and one on the
  // mesh's outline anything from its least to its most
  let nudges: [Vec2; 8] = std::array::from_fn(|k| {
    Vec2::from_angle(0.3 + k as f32 * std::f32::consts::TAU / 8.0) * 0.0025
  });
  let (mut drawn, mut doubled, mut cracks) = (0usize, 0usize, 0usize);
  let mut failures = Vec::new();
  let all = lines();
  // The wandering line and the circle are long; the far one is the zigzag
  let chosen = all
    .iter()
    .enumerate()
    .filter(|(i, _)| ![13, 14, 15].contains(i));
  for (index, line) in chosen {
    for join in JOINS {
      for cap in CAPS {
        for ppu in [1.0f32, 2.5] {
          for &offset in &offsets {
            let line = line.moved(offset / ppu);
            let (points, style) = (line.poly(), line.style(join, cap));
            let (mut stream, mut reference) = (LineStream::default(), Triangles::default());
            stream.polyline(&points, &style);
            reference.polyline(&points, &style);
            let (mut min, mut max) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
            for &p in &line.points {
              min = min.min(p);
              max = max.max(p);
            }
            let reach = 4.0 * line.halves.iter().fold(0.0f32, |a, &b| a.max(b)) + 2.0;
            let centre = ((min + max) / 2.0 - offset / ppu).round();
            let extent = ((max - min) / 2.0 + reach) * ppu;
            let size = (
              2 * (extent.x.ceil() as u32 + 2),
              2 * (extent.y.ceil() as u32 + 2),
            );
            let full = Vec2::new(size.0 as f32, size.1 as f32);
            let camera = Camera2d {
              zoom: ppu,
              ..Default::default()
            };
            let view = CameraView::compute(centre, 0.0, &camera, Vec2::ZERO, full);
            assert_eq!(pixels_per_unit(&view, size), ppu);
            let counts = counter.count(&device, &queue, &stream, &view, size);
            // The reference pushed a pixel, in the image's pixels
            let pixel = 1.0 / ppu;
            let to_image = |world: Vec2| (world - centre) * ppu + full / 2.0;
            let triangles: Vec<[Vec2; 3]> = reference
              .indices
              .chunks(3)
              .map(|tri| {
                [0, 1, 2].map(|k| {
                  let v = &reference.vertices[tri[k] as usize];
                  to_image(Vec2::from(v.position) + Vec2::from(v.push) * pixel.min(v.limit))
                })
              })
              .filter(|[a, b, c]| orient(*a, *b, *c).abs() > 1e-9)
              .collect();
            // Each sample's count over the mesh
            let mut sampled = vec![[0u8; 8]; counts.len()];
            for tri in &triangles {
              let lo = tri[0].min(tri[1]).min(tri[2]).floor().max(Vec2::ZERO);
              let hi = tri[0].max(tri[1]).max(tri[2]).ceil().min(full);
              for y in lo.y as u32..hi.y as u32 {
                for x in lo.x as u32..hi.x as u32 {
                  let at = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                  let i = (y * size.0 + x) as usize;
                  for (count, nudge) in sampled[i].iter_mut().zip(&nudges) {
                    *count += u8::from(strictly_inside(tri, at + *nudge));
                  }
                }
              }
            }
            let splits = line.split_corners(false);
            for y in 0..size.1 {
              for x in 0..size.0 {
                let i = (y * size.0 + x) as usize;
                let count = counts[i];
                let low = sampled[i].iter().copied().min().unwrap_or(0);
                let high = sampled[i].iter().copied().max().unwrap_or(0);
                drawn += usize::from(count > 0);
                let what = format!(
                  "line {index} {join:?} {cap:?} ppu {ppu} offset {offset} pixel ({x}, {y})"
                );
                if count < low || count > high {
                  cracks += 1;
                  failures.push(format!(
                    "{what}: drawn {count} times, the mesh {low} to {high}"
                  ));
                }
                if high > 1 {
                  // Only at a split corner's inner overlap
                  let world =
                    (Vec2::new(x as f32 + 0.5, y as f32 + 0.5) - full / 2.0) / ppu + centre;
                  let overlap = splits.iter().any(|segments| {
                    // A segment's pushed quad, its corners included (a cap's
                    // reach diagonally across both edges)
                    let half = line.halves.iter().fold(0.0f32, |a, &b| a.max(b));
                    let reach = (half + pixel) * std::f32::consts::SQRT_2;
                    segments
                      .iter()
                      .all(|&segment| distance_to_segment(world, segment) <= reach)
                  });
                  if overlap {
                    doubled += 1;
                  } else {
                    let far: Vec<f32> = splits
                      .iter()
                      .flat_map(|segments| {
                        segments.map(|segment| distance_to_segment(world, segment))
                      })
                      .collect();
                    failures.push(format!(
                      "{what}: the mesh covers it {high} times (at {world}, {far:?})"
                    ));
                  }
                }
              }
            }
          }
        }
      }
    }
  }
  let error = pollster::block_on(scope.pop());
  assert!(error.is_none(), "{error:?}");
  eprintln!(
    "{drawn} pixels drawn; {doubled} at split corners' inner overlaps; {cracks} not as the mesh"
  );
  assert!(drawn > 10_000, "{drawn} pixels drawn");
  assert!(
    failures.is_empty(),
    "{} pixels wrong:\n{}",
    failures.len(),
    failures[..failures.len().min(30)].join("\n")
  );
}
