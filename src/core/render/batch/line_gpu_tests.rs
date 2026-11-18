//! The GPU's line pieces (`line_pieces.wgsl`, through `line.wgsl`) against
//! the CPU builder (`polyline.rs`), the reference: the vertices they build,
//! triangle for triangle, and the pixels their pushed triangles cover.

use glam::Vec2;
use wgpu::util::DeviceExt;

use super::corners::{CORNER_BYTES, CornerPass, LIMIT_BYTES, MAX_ACROSS};
use super::gpu::line_group;
use super::lines::{LineStream, MITER_LIMIT, PieceKind, piece_template};
use super::lines::{PolyPoint, PolyStyle};
use super::pipelines::{camera_layout, line_layout_for};
use super::polyline::{Triangles, Vertex2D};
use crate::core::graphics::bind_groups;
use crate::core::graphics::camera_gpu::{CameraGlobals, pixels_per_unit};
use crate::core::graphics::{Camera2d, CameraView};
use crate::core::shaders::compose::LINE_TEST;
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
      edge: None,
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
/// outlines, single segments, tapered lines, a long wandering line, one far
/// from the origin, and lines running straight on through a point (along
/// an axis, diagonally, and turning by about 1e-7)
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
    TestLine::even(
      &[(0.0, 0.0), (5.0, 0.0), (10.0, 0.0), (10.0, 5.0)],
      1.0,
      false,
    ),
    TestLine::even(
      &[(0.0, 0.0), (3.0, 3.0), (7.0, 7.0), (12.0, 4.0)],
      1.0,
      false,
    ),
    TestLine::even(
      &[(0.0, 0.0), (5.0, 0.0), (10.0, 5.0e-7), (10.0, 5.0)],
      1.0,
      false,
    ),
  ]
}

/// A vertex as `cs_line_dump` writes it (`LineDump` in `line_test.wgsl`)
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

fn buffer(
  device: &wgpu::Device,
  label: &str,
  bytes: &[u8],
  usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
  // A binding needs a buffer of some size
  let bytes = if bytes.is_empty() {
    &[0u8; 16][..]
  } else {
    bytes
  };
  device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
    label: Some(label),
    contents: bytes,
    usage,
  })
}

/// The tests' one way to draw a line stream: upload it, encode the corner
/// pass as the frame does before its passes, and draw it in a pass whose
/// pipeline and other groups the caller set
pub(super) struct LineGpuTest {
  layout: wgpu::BindGroupLayout,
  corners: CornerPass,
  /// The corner pass's workgroups a row
  across: u32,
  /// Whether the vertices work their corners out in place, the corner pass
  /// skipped, as a small stream's do
  in_place: bool,
}

/// A stream uploaded for drawing
pub(super) struct UploadedLines {
  group: wgpu::BindGroup,
  corner_group: wgpu::BindGroup,
  points: u32,
  /// Its vertex codes, the draw's index buffer (and a storage array the
  /// dump reads)
  indices: wgpu::Buffer,
  /// How many indices it draws
  pub(super) vertices: u32,
}

impl LineGpuTest {
  pub(super) fn new(device: &wgpu::Device) -> Self {
    Self::across(device, MAX_ACROSS)
  }

  /// One whose corner pass goes on in rows past `across` workgroups
  pub(super) fn across(device: &wgpu::Device, across: u32) -> Self {
    Self::with(device, across, false)
  }

  /// One whose vertices work their corners out in place, with no corner
  /// pass
  pub(super) fn in_place(device: &wgpu::Device) -> Self {
    Self::with(device, MAX_ACROSS, true)
  }

  fn with(device: &wgpu::Device, across: u32, in_place: bool) -> Self {
    Self {
      layout: line_layout_for(
        device,
        wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::COMPUTE,
      ),
      corners: CornerPass::new(device),
      across,
      in_place,
    }
  }

  /// The layout of the group the line pipelines bind the stream with
  pub(super) fn layout(&self) -> &wgpu::BindGroupLayout {
    &self.layout
  }

  pub(super) fn upload(
    &self,
    device: &wgpu::Device,
    _queue: &wgpu::Queue,
    stream: &LineStream,
  ) -> UploadedLines {
    let storage = wgpu::BufferUsages::STORAGE;
    let table = buffer(
      device,
      "Test Line Table",
      bytemuck::cast_slice(&stream.lines),
      storage,
    );
    let points = buffer(
      device,
      "Test Line Points",
      bytemuck::cast_slice(&stream.points),
      storage,
    );
    let room = |label, element: usize| {
      device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: (stream.points.len().max(1) * element) as u64,
        usage: storage,
        mapped_at_creation: false,
      })
    };
    let corners = room("Test Line Corners", CORNER_BYTES);
    let limits = room("Test Line Limits", LIMIT_BYTES);
    let count = stream.points.len() as u32;
    let count_buffer = buffer(
      device,
      "Test Line Count",
      bytemuck::cast_slice(&[count, u32::from(!self.in_place), 0, 0]),
      wgpu::BufferUsages::UNIFORM,
    );
    let indices = buffer(
      device,
      "Test Line Indices",
      bytemuck::cast_slice(&stream.indices),
      wgpu::BufferUsages::INDEX | storage,
    );
    UploadedLines {
      group: line_group(
        device,
        &self.layout,
        [&table, &points, &corners, &limits, &count_buffer],
      ),
      corner_group: self
        .corners
        .group(device, [&table, &points, &corners, &limits, &count_buffer]),
      points: count,
      indices,
      vertices: stream.indices.len() as u32,
    }
  }

  /// What the frame encodes before its passes: the corner pass, unless the
  /// vertices work their corners out in place
  pub(super) fn encode(&self, encoder: &mut wgpu::CommandEncoder, lines: &UploadedLines) {
    if self.in_place {
      return;
    }
    self.corners.encode(
      encoder,
      &lines.corner_group,
      lines.points,
      self.across,
      None,
    );
  }

  /// Draw every line of `lines` in `pass`, binding the stream as group 2
  pub(super) fn draw<'a>(&self, pass: &mut wgpu::RenderPass<'a>, lines: &'a UploadedLines) {
    pass.set_bind_group(2, &lines.group, &[]);
    pass.set_index_buffer(lines.indices.slice(..), wgpu::IndexFormat::Uint32);
    pass.draw_indexed(0..lines.vertices, 0, 0..1);
  }

  /// `stream` drawn into `target` (cleared to transparent first) by
  /// `pipeline`, with the camera's group and `texture` (group 1) when given
  pub(super) fn paint(
    &self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    stream: &LineStream,
    target: &wgpu::Texture,
    pipeline: &wgpu::RenderPipeline,
    groups: (&wgpu::BindGroup, Option<&wgpu::BindGroup>),
  ) {
    let lines = self.upload(device, queue, stream);
    let mut encoder = device.create_command_encoder(&Default::default());
    self.encode(&mut encoder, &lines);
    {
      let view = target.create_view(&Default::default());
      let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Test Line Pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
          view: &view,
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
      pass.set_pipeline(pipeline);
      pass.set_bind_group(0, groups.0, &[]);
      if let Some(texture) = groups.1 {
        pass.set_bind_group(1, texture, &[]);
      }
      self.draw(&mut pass, &lines);
    }
    queue.submit([encoder.finish()]);
  }
}

fn module(device: &wgpu::Device) -> wgpu::ShaderModule {
  device.create_shader_module(wgpu::ShaderModuleDescriptor {
    label: Some("Line Test Shader"),
    source: wgpu::ShaderSource::Wgsl(LINE_TEST.into()),
  })
}

/// Every vertex `stream` draws, three a triangle in its index buffer's
/// order, as the GPU builds it
fn dump(device: &wgpu::Device, queue: &wgpu::Queue, stream: &LineStream) -> Vec<Dumped> {
  dump_across(device, queue, stream, MAX_ACROSS)
}

/// [`dump`], the corner pass going on in rows past `across` workgroups
fn dump_across(
  device: &wgpu::Device,
  queue: &wgpu::Queue,
  stream: &LineStream,
  across: u32,
) -> Vec<Dumped> {
  dump_by(device, queue, stream, &LineGpuTest::across(device, across))
}

/// [`dump`], its vertices working their corners out in place
fn dump_in_place(device: &wgpu::Device, queue: &wgpu::Queue, stream: &LineStream) -> Vec<Dumped> {
  dump_by(device, queue, stream, &LineGpuTest::in_place(device))
}

fn dump_by(
  device: &wgpu::Device,
  queue: &wgpu::Queue,
  stream: &LineStream,
  gpu: &LineGpuTest,
) -> Vec<Dumped> {
  let lines = gpu.upload(device, queue, stream);
  let count = lines.vertices as usize;
  let storage = |binding, read_only| wgpu::BindGroupLayoutEntry {
    binding,
    visibility: wgpu::ShaderStages::COMPUTE,
    ty: wgpu::BindingType::Buffer {
      ty: wgpu::BufferBindingType::Storage { read_only },
      has_dynamic_offset: false,
      min_binding_size: None,
    },
    count: None,
  };
  let dump_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
    label: Some("Line Dump Layout"),
    entries: &[storage(0, false), storage(1, true)],
  });
  let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
    label: Some("Line Dump Pipeline Layout"),
    bind_group_layouts: &[None, None, Some(gpu.layout()), Some(&dump_layout)],
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
  let size = (count.max(1) * std::mem::size_of::<Dumped>()) as u64;
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
  let dump_group = bind_groups::create(
    device,
    &wgpu::BindGroupDescriptor {
      label: Some("Line Dump Group"),
      layout: &dump_layout,
      entries: &[
        wgpu::BindGroupEntry {
          binding: 0,
          resource: out.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
          binding: 1,
          resource: lines.indices.as_entire_binding(),
        },
      ],
    },
  );
  let mut encoder = device.create_command_encoder(&Default::default());
  gpu.encode(&mut encoder, &lines);
  {
    let mut pass = encoder.begin_compute_pass(&Default::default());
    pass.set_pipeline(&pipeline);
    pass.set_bind_group(2, &lines.group, &[]);
    pass.set_bind_group(3, &dump_group, &[]);
    pass.dispatch_workgroups((count as u32).div_ceil(64), 1, 1);
  }
  encoder.copy_buffer_to_buffer(&out, 0, &read, 0, size);
  queue.submit([encoder.finish()]);
  let slice = read.slice(..);
  slice.map_async(wgpu::MapMode::Read, |_| {});
  let _ = device.poll(wgpu::PollType::wait_indefinitely());
  let mut dumped = {
    let view = slice.get_mapped_range().expect("the dump maps");
    bytemuck::cast_slice::<u8, Dumped>(&view).to_vec()
  };
  read.unmap();
  dumped.truncate(count);
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

/// `dumped` in the reference's corner order: each triangle's first vertex
/// is the reference's (flat attributes come from it), and its other two
/// are the reference's in either order, a right turn's triangles winding
/// the other way on the GPU (no face is culled); each pair goes the way
/// that puts its fields nearer the reference's
fn paired(reference: &Triangles, dumped: &[Dumped]) -> Vec<Dumped> {
  let off = |v: &Vertex2D, d: &Dumped| -> f32 {
    compare(v, d)
      .iter()
      .map(|(_, cpu, gpu)| (cpu - gpu).abs())
      .sum()
  };
  let mut out = dumped.to_vec();
  for (tri, gpu) in reference.indices.chunks(3).zip(out.chunks_mut(3)) {
    let [_, b, c] = [0, 1, 2].map(|k| &reference.vertices[tri[k] as usize]);
    if let [_, g1, g2] = gpu
      && off(b, g2) + off(c, g1) < off(b, g1) + off(c, g2)
    {
      std::mem::swap(g1, g2);
    }
  }
  out
}

/// Where the GPU's `dumped` vertices (paired) differ from the reference's
/// by more than f32's rounding, naming each by the line in `names` whose
/// range holds it, and the worst difference
fn differences(
  reference: &Triangles,
  dumped: &[Dumped],
  names: &[(std::ops::Range<u32>, String)],
) -> (Vec<String>, (f32, String)) {
  let mut failures = Vec::new();
  let mut worst = (0.0f32, String::new());
  for (corner, (&index, d)) in reference.indices.iter().zip(dumped).enumerate() {
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
  (failures, worst)
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
  // The corner pass in rows of three workgroups, as it goes on past
  // 65,535 (`the_corner_pass_goes_on_in_rows_past_its_width`)
  assert!(stream.points.len() > 3 * 64 * 4, "rows to go on in");
  let dumped = paired(&reference, &dump_across(&device, &queue, &stream, 3));
  let error = pollster::block_on(scope.pop());
  assert!(error.is_none(), "{error:?}");
  assert_eq!(dumped.len(), reference.indices.len());
  let (failures, worst) = differences(&reference, &dumped, &names);
  eprintln!("{} vertices; worst: {worst:?}", dumped.len());
  assert!(
    failures.is_empty(),
    "{} fields differ:\n{}",
    failures.len(),
    failures[..failures.len().min(30)].join("\n")
  );
  // The GPU's copies of each of the reference's vertices (a line's
  // vertices of some width that agree bit for bit in place, push, limit and
  // what effects move them by) agree bit for bit too. A split corner's
  // elbows, where the reference's copies push two ways, are two vertices
  // here, and so are vertices of two pieces that only meet (a U-turn's)
  type Bits = ([u32; 2], [u32; 2], u32, [u32; 2], u32);
  let bits = |position: [f32; 2], push: [f32; 2], limit: f32, across: [f32; 2], u: f32| -> Bits {
    (
      position.map(f32::to_bits),
      push.map(f32::to_bits),
      limit.to_bits(),
      across.map(f32::to_bits),
      u.to_bits(),
    )
  };
  let mut copies: std::collections::HashMap<(u32, Bits), Vec<Bits>> =
    std::collections::HashMap::new();
  for (&index, d) in reference.indices.iter().zip(&dumped) {
    let v = &reference.vertices[index as usize];
    if v.half <= 0.0 {
      continue;
    }
    let cpu = bits(v.position, v.push, v.limit, v.across, v.effect_u);
    let gpu = bits(d.position, d.push, d.limit, d.across, d.effect_u);
    let list = copies.entry((v.line, cpu)).or_default();
    if !list.contains(&gpu) {
      list.push(gpu);
    }
  }
  let shared = copies.values().filter(|gpu| !gpu.is_empty()).count();
  let apart: Vec<String> = copies
    .iter()
    .filter(|(_, gpu)| gpu.len() > 1)
    .map(|((line, cpu), gpu)| {
      format!(
        "line entry {line} at {:?}: the GPU's copies differ: {gpu:?}",
        cpu.0.map(f32::from_bits)
      )
    })
    .collect();
  eprintln!("{shared} places");
  assert!(
    apart.is_empty(),
    "{} places:\n{}",
    apart.len(),
    apart[..apart.len().min(30)].join("\n")
  );
}

#[test]
fn vertices_working_their_corners_out_in_place_equal_the_corner_pass_s() {
  // A small stream skips the corner pass and each vertex works its corners
  // out in place (`builds_corners`): every vertex it builds is the one the
  // corner pass's records build, bit for bit, so a stream draws the same
  // pixels on either side of the threshold, and both equal the reference
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
        reference.polyline(&points, &style);
        names.push((gpu, format!("line {index} {join:?} {cap:?}")));
      }
    }
  }
  let built = dump(&device, &queue, &stream);
  let worked = dump_in_place(&device, &queue, &stream);
  let error = pollster::block_on(scope.pop());
  assert!(error.is_none(), "{error:?}");
  assert_eq!(built.len(), worked.len());
  let apart: Vec<String> = built
    .iter()
    .zip(&worked)
    .enumerate()
    .filter(|(_, (a, b))| bytemuck::bytes_of(*a) != bytemuck::bytes_of(*b))
    .map(|(i, (a, b))| format!("vertex {i}: {a:?} from the records, {b:?} in place"))
    .collect();
  assert!(
    apart.is_empty(),
    "{} of {} vertices differ:\n{}",
    apart.len(),
    built.len(),
    apart[..apart.len().min(10)].join("\n")
  );
  let (failures, worst) = differences(&reference, &paired(&reference, &worked), &names);
  eprintln!("{} vertices; worst {worst:?}", worked.len());
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
  lines: LineGpuTest,
}

impl Counter {
  fn new(device: &wgpu::Device) -> Self {
    Self::drawing(device, LineGpuTest::new(device))
  }

  /// One whose vertices work their corners out in place
  fn in_place(device: &wgpu::Device) -> Self {
    Self::drawing(device, LineGpuTest::in_place(device))
  }

  fn drawing(device: &wgpu::Device, lines: LineGpuTest) -> Self {
    let camera_layout = camera_layout(device);
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("Line Count Layout"),
      bind_group_layouts: &[Some(&camera_layout), None, Some(lines.layout())],
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
      lines,
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
    self.lines.paint(
      device,
      queue,
      stream,
      &texture,
      &self.pipeline,
      (&camera, None),
    );
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
  // (measured and printed). Vertices that work their corners out in place
  // (a small stream's) draw the same counts
  let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
    return;
  };
  let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
  let counter = Counter::new(&device);
  let in_place = Counter::in_place(&device);
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
  // The wandering line and the circle are long
  let chosen = all
    .iter()
    .enumerate()
    .filter(|(i, _)| ![13, 14].contains(i));
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
            if in_place.count(&device, &queue, &stream, &view, size) != counts {
              failures.push(format!(
                "line {index} {join:?} {cap:?} ppu {ppu} offset {offset}: drawn otherwise with corners worked out in place"
              ));
            }
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

#[test]
fn the_slot_layouts_agree() {
  // The slots each triangle names, as the CPU writes them into the index
  // buffer (`piece_template`) and as the UI's unindexed instances ask the
  // GPU for them (`line_piece_slot`), for every kind, join and cap
  let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
    return;
  };
  let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
  let count = 4 * 864;
  let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
    label: Some("Slot Dump Layout"),
    entries: &[wgpu::BindGroupLayoutEntry {
      binding: 2,
      visibility: wgpu::ShaderStages::COMPUTE,
      ty: wgpu::BindingType::Buffer {
        ty: wgpu::BufferBindingType::Storage { read_only: false },
        has_dynamic_offset: false,
        min_binding_size: None,
      },
      count: None,
    }],
  });
  let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
    label: Some("Slot Dump Pipeline Layout"),
    bind_group_layouts: &[None, None, None, Some(&layout)],
    immediate_size: 0,
  });
  let module = module(&device);
  let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
    label: Some("Slot Dump"),
    layout: Some(&pipeline_layout),
    module: &module,
    entry_point: Some("cs_slot_dump"),
    compilation_options: Default::default(),
    cache: None,
  });
  let size = (count * 4) as u64;
  let out = device.create_buffer(&wgpu::BufferDescriptor {
    label: Some("Slot Dump"),
    size,
    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
    mapped_at_creation: false,
  });
  let read = device.create_buffer(&wgpu::BufferDescriptor {
    label: Some("Slot Dump Readback"),
    size,
    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
    mapped_at_creation: false,
  });
  let group = bind_groups::create(
    &device,
    &wgpu::BindGroupDescriptor {
      label: Some("Slot Dump Group"),
      layout: &layout,
      entries: &[wgpu::BindGroupEntry {
        binding: 2,
        resource: out.as_entire_binding(),
      }],
    },
  );
  let mut encoder = device.create_command_encoder(&Default::default());
  {
    let mut pass = encoder.begin_compute_pass(&Default::default());
    pass.set_pipeline(&pipeline);
    pass.set_bind_group(3, &group, &[]);
    pass.dispatch_workgroups((count as u32).div_ceil(64), 1, 1);
  }
  encoder.copy_buffer_to_buffer(&out, 0, &read, 0, size);
  queue.submit([encoder.finish()]);
  let slice = read.slice(..);
  slice.map_async(wgpu::MapMode::Read, |_| {});
  let _ = device.poll(wgpu::PollType::wait_indefinitely());
  let slots = {
    let view = slice.get_mapped_range().expect("the dump maps");
    bytemuck::cast_slice::<u8, u32>(&view).to_vec()
  };
  read.unmap();
  let error = pollster::block_on(scope.pop());
  assert!(error.is_none(), "{error:?}");
  let kinds = [
    PieceKind::Segment,
    PieceKind::Join,
    PieceKind::StartCap,
    PieceKind::EndCap,
  ];
  let mut differ = Vec::new();
  let mut checked = 0;
  for (k, kind) in kinds.into_iter().enumerate() {
    for (j, join) in JOINS.into_iter().enumerate() {
      for (c, cap) in [LineCap::Butt, LineCap::Square, LineCap::Round]
        .into_iter()
        .enumerate()
      {
        for (tri, corners) in piece_template(kind, join, cap).iter().enumerate() {
          for (corner, &slot) in corners.iter().enumerate() {
            let at = k * 864 + j * 288 + c * 96 + tri * 3 + corner;
            checked += 1;
            if slots[at] != slot {
              differ.push(format!(
                "{kind:?} {join:?} {cap:?} triangle {tri} corner {corner}: {slot} on the CPU, {} on the GPU",
                slots[at]
              ));
            }
          }
        }
      }
    }
  }
  assert!(checked > 300, "{checked} slots checked");
  assert!(differ.is_empty(), "{}", differ.join("\n"));
}

/// The decisions the GPU takes at a corner of `line`: which way it turns,
/// whether its inner side is shared and, for a miter, whether it stays
/// within the limit; or that it runs straight on or turns back
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Decision {
  Turn {
    left: bool,
    shared: bool,
    mitered: bool,
  },
  Straight,
  UTurn,
}

fn decisions(line: &TestLine, join: LineJoin) -> Vec<Decision> {
  let n = line.points.len();
  let at = |i: usize| line.points[i % n];
  let corners = if line.closed {
    0..n
  } else {
    1..n.saturating_sub(1)
  };
  corners
    .map(|i| {
      let (a, p, b) = (at(i + n - 1), at(i), at(i + 1));
      let (d0, d1) = ((p - a).normalize(), (b - p).normalize());
      let turn = d0.perp_dot(d1);
      let cos = d0.dot(d1);
      if turn.abs() < 1e-6 && cos > 0.0 {
        return Decision::Straight;
      }
      if 1.0 + cos <= 1e-6 {
        return Decision::UTurn;
      }
      let miter = (d0.perp() + d1.perp()) / (1.0 + cos);
      let shortest = p.distance(a).min(p.distance(b));
      Decision::Turn {
        left: turn > 0.0,
        shared: miter.length() * line.halves[i % n] <= shortest / 2.0,
        mitered: join == LineJoin::Miter && miter.length() <= MITER_LIMIT,
      }
    })
    .collect()
}

#[test]
fn every_slot_builds_its_role_under_every_decision() {
  // Each kind's fixed slots, drawn indexed, build the triangles the
  // reference builds (each triangle's first vertex the same, its other two
  // as a pair) under every decision at a corner: either turn, the inner
  // side shared or split, a miter within its limit or bevelled past it,
  // straight on and a U-turn, with every cap at both ends and closed
  let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
    return;
  };
  let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
  let long = |turn: f32| {
    [
      (0.0, 0.0),
      (20.0, 0.0),
      (20.0 + 20.0 * turn.cos(), 20.0 * turn.sin()),
    ]
  };
  let short = |turn: f32| {
    [
      (0.0, 0.0),
      (1.5, 0.0),
      (1.5 + 1.5 * turn.cos(), 1.5 * turn.sin()),
    ]
  };
  let mut cases = Vec::new();
  for degrees in [40.0f32, -40.0, 160.0, -160.0] {
    let turn = degrees.to_radians();
    cases.push(TestLine::even(&long(turn), 1.0, false));
    cases.push(TestLine::even(&short(turn), 1.0, false));
  }
  cases.push(TestLine::even(
    &[(0.0, 0.0), (10.0, 0.0), (20.0, 0.0)],
    1.0,
    false,
  ));
  cases.push(TestLine::even(
    &[(0.0, 0.0), (10.0, 0.0), (0.0, 0.0)],
    1.0,
    false,
  ));
  cases.push(TestLine::even(
    &[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)],
    1.0,
    true,
  ));
  let mut seen = Vec::new();
  let (mut stream, mut reference) = (LineStream::default(), Triangles::default());
  let mut names = Vec::new();
  for (index, line) in cases.iter().enumerate() {
    for join in JOINS {
      for decision in decisions(line, join) {
        seen.push((join, decision));
      }
      for cap in CAPS {
        let (points, style) = (line.poly(), line.style(join, cap));
        let gpu = stream.polyline(&points, &style);
        assert_eq!(gpu, reference.polyline(&points, &style));
        names.push((gpu, format!("case {index} {join:?} {cap:?}")));
      }
    }
  }
  // Every decision is met, for every join it can be met with
  for join in JOINS {
    for left in [false, true] {
      for shared in [false, true] {
        for mitered in [false, true] {
          if mitered && join != LineJoin::Miter {
            continue;
          }
          let decision = Decision::Turn {
            left,
            shared,
            mitered,
          };
          assert!(
            seen.contains(&(join, decision)),
            "{join:?} {decision:?} met"
          );
        }
      }
    }
    assert!(seen.contains(&(join, Decision::Straight)));
    assert!(seen.contains(&(join, Decision::UTurn)));
  }
  let dumped = paired(&reference, &dump(&device, &queue, &stream));
  let error = pollster::block_on(scope.pop());
  assert!(error.is_none(), "{error:?}");
  let (failures, worst) = differences(&reference, &dumped, &names);
  eprintln!("{} vertices; worst {worst:?}", dumped.len());
  assert!(
    failures.is_empty(),
    "{} fields differ:\n{}",
    failures.len(),
    failures[..failures.len().min(30)].join("\n")
  );
}
