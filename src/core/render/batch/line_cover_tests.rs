//! The pixels world lines draw, read back as alpha from an image of the UI's
//! format: each pixel's coverage against the model (`coverage.rs`), against
//! the exact area of the line, for thin lines, at translucent corners, and
//! the styles and textures across a line, all through `line.wgsl`'s own
//! vertex and fragment stages.

use glam::Vec2;
use wgpu::util::DeviceExt;

use super::coverage::{area_in_pixel, cover_strip};
use super::line_gpu_tests::LineGpuTest;
use super::lines::LineStream;
use super::lines::{PolyPoint, PolyStyle};
use super::pipelines::{camera_layout, texture_layout};
use super::polyline::Triangles;
use crate::core::graphics::bind_groups;
use crate::core::graphics::camera_gpu::{CameraGlobals, pixels_per_unit};
use crate::core::graphics::{Camera2d, CameraView};
use crate::core::render::UI_FORMAT;
use crate::core::shaders::compose::LINE;
use crate::core::trail::{LineCap, LineEdge, LineJoin};
use crate::core::ui::Color;

const JOINS: [LineJoin; 3] = [LineJoin::Miter, LineJoin::Bevel, LineJoin::Round];
const CAPS: [LineCap; 3] = [LineCap::Butt, LineCap::Square, LineCap::Round];

/// The style bits of a line's first parameter (`lines.rs`)
const SOLID: f32 = 0.0;
const SOFT: f32 = 16.0;
const GLOW: f32 = 32.0;

/// The line pipeline's stages, drawing into an image of the UI's format
/// (alpha straight from coverage), premultiplied over a transparent clear
struct Painter {
  plain: wgpu::RenderPipeline,
  textured: wgpu::RenderPipeline,
  camera_layout: wgpu::BindGroupLayout,
  texture_layout: wgpu::BindGroupLayout,
  lines: LineGpuTest,
  sampler: wgpu::Sampler,
  blank: wgpu::Texture,
}

impl Painter {
  fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
    let camera_layout = camera_layout(device);
    let texture_layout = texture_layout(device);
    let lines = LineGpuTest::new(device);
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("Line Cover Layout"),
      bind_group_layouts: &[
        Some(&camera_layout),
        Some(&texture_layout),
        Some(lines.layout()),
      ],
      immediate_size: 0,
    });
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some("Line Cover Shader"),
      source: wgpu::ShaderSource::Wgsl(LINE.into()),
    });
    let over = wgpu::BlendComponent {
      src_factor: wgpu::BlendFactor::One,
      dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
      operation: wgpu::BlendOperation::Add,
    };
    let pipeline = |fragment: &str| {
      device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Line Cover"),
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
            format: UI_FORMAT,
            blend: Some(wgpu::BlendState {
              color: over,
              alpha: over,
            }),
            write_mask: wgpu::ColorWrites::ALL,
          })],
          compilation_options: wgpu::PipelineCompilationOptions {
            constants: &[("UI_GAMMA", 1.0)],
            ..Default::default()
          },
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
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
      label: Some("Line Cover Sampler"),
      mag_filter: wgpu::FilterMode::Linear,
      min_filter: wgpu::FilterMode::Linear,
      ..Default::default()
    });
    let blank = rows_texture(device, queue, 1, |_| [255, 255, 255, 255]);
    Self {
      plain: pipeline("fs_main"),
      textured: pipeline("fs_textured"),
      camera_layout,
      texture_layout,
      lines,
      sampler,
      blank,
    }
  }

  /// `stream` drawn by a camera centred on `centre` at `ppu` image pixels a
  /// unit, into an image of `size`, textured by `texture` when given (each
  /// line's slot its whole)
  fn paint(
    &self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    stream: &LineStream,
    (centre, ppu): (Vec2, f32),
    size: (u32, u32),
    texture: Option<&wgpu::Texture>,
  ) -> image::RgbaImage {
    self.paint_on(device, queue, stream, (centre, ppu), size, texture, false)
  }

  /// [`paint`](Self::paint) into a pixel-art target when `pixel_art`
  #[allow(clippy::too_many_arguments)]
  fn paint_on(
    &self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    stream: &LineStream,
    (centre, ppu): (Vec2, f32),
    size: (u32, u32),
    texture: Option<&wgpu::Texture>,
    pixel_art: bool,
  ) -> image::RgbaImage {
    let full = Vec2::new(size.0 as f32, size.1 as f32);
    let camera = Camera2d {
      zoom: ppu,
      ..Default::default()
    };
    let view = CameraView::compute(centre, 0.0, &camera, Vec2::ZERO, full);
    assert_eq!(pixels_per_unit(&view, size), ppu);
    let globals = CameraGlobals::new(&view, size, 0.0, 0.0, pixel_art);
    let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
      label: Some("Line Cover Camera"),
      contents: bytemuck::bytes_of(&globals),
      usage: wgpu::BufferUsages::UNIFORM,
    });
    let camera_group = bind_groups::create(
      device,
      &wgpu::BindGroupDescriptor {
        label: Some("Line Cover Camera"),
        layout: &self.camera_layout,
        entries: &[wgpu::BindGroupEntry {
          binding: 0,
          resource: uniform.as_entire_binding(),
        }],
      },
    );
    let view_of = texture
      .unwrap_or(&self.blank)
      .create_view(&Default::default());
    let texture_group = bind_groups::create(
      device,
      &wgpu::BindGroupDescriptor {
        label: Some("Line Cover Texture"),
        layout: &self.texture_layout,
        entries: &[
          wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(&view_of),
          },
          wgpu::BindGroupEntry {
            binding: 1,
            resource: wgpu::BindingResource::Sampler(&self.sampler),
          },
        ],
      },
    );
    let mut stream_copy = LineStream::default();
    stream_copy.points.clone_from(&stream.points);
    stream_copy.indices.clone_from(&stream.indices);
    stream_copy.lines.clone_from(&stream.lines);
    if texture.is_some() {
      for line in &mut stream_copy.lines {
        line.bounds = [0.0, 0.0, 1.0, 1.0];
      }
    }
    let image = device.create_texture(&wgpu::TextureDescriptor {
      label: Some("Line Cover Image"),
      size: wgpu::Extent3d {
        width: size.0,
        height: size.1,
        depth_or_array_layers: 1,
      },
      mip_level_count: 1,
      sample_count: 1,
      dimension: wgpu::TextureDimension::D2,
      format: UI_FORMAT,
      usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
      view_formats: &[],
    });
    let pipeline = if texture.is_some() {
      &self.textured
    } else {
      &self.plain
    };
    self.lines.paint(
      device,
      queue,
      &stream_copy,
      &image,
      pipeline,
      (&camera_group, Some(&texture_group)),
    );
    crate::core::capture::read_texture_rgba(device, queue, &image).expect("read back")
  }
}

/// A texture four texels wide and `height` tall, row `r` of colour `row(r)`
/// (raw bytes, as the atlas holds them)
fn rows_texture(
  device: &wgpu::Device,
  queue: &wgpu::Queue,
  height: u32,
  row: impl Fn(u32) -> [u8; 4],
) -> wgpu::Texture {
  let texture = device.create_texture(&wgpu::TextureDescriptor {
    label: Some("Line Cover Rows"),
    size: wgpu::Extent3d {
      width: 4,
      height,
      depth_or_array_layers: 1,
    },
    mip_level_count: 1,
    sample_count: 1,
    dimension: wgpu::TextureDimension::D2,
    format: wgpu::TextureFormat::Rgba8Unorm,
    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
    view_formats: &[],
  });
  let bytes: Vec<u8> = (0..height).flat_map(|r| row(r).repeat(4)).collect();
  queue.write_texture(
    texture.as_image_copy(),
    &bytes,
    wgpu::TexelCopyBufferLayout {
      offset: 0,
      bytes_per_row: Some(16),
      rows_per_image: Some(height),
    },
    wgpu::Extent3d {
      width: 4,
      height,
      depth_or_array_layers: 1,
    },
  );
  texture
}

/// The points of a line through `points`, every one of half width `half`,
/// in `color`
fn points_of(points: &[Vec2], half: f32, color: Color) -> Vec<PolyPoint> {
  let last = (points.len() - 1).max(1) as f32;
  points
    .iter()
    .enumerate()
    .map(|(i, &pos)| PolyPoint {
      pos,
      half_width: half,
      color,
      u: i as f32 / last,
    })
    .collect()
}

fn style(join: LineJoin, cap: LineCap, closed: bool, look: f32) -> PolyStyle {
  PolyStyle {
    join,
    cap,
    closed,
    params: [look, 0.0, 0.0, 0.0],
    emission: 1.0,
    edge: None,
  }
}

/// A pixel's centre in the world, for a camera at `centre` a pixel a unit
/// drawing an image of `size`
fn world_of(x: u32, y: u32, centre: Vec2, size: (u32, u32)) -> Vec2 {
  Vec2::new(x as f32 + 0.5, y as f32 + 0.5) - Vec2::new(size.0 as f32, size.1 as f32) / 2.0 + centre
}

fn alpha(image: &image::RgbaImage, x: u32, y: u32) -> f32 {
  f32::from(image.get_pixel(x, y).0[3]) / 255.0
}

#[test]
fn straight_lines_cover_each_pixel_as_the_model_does() {
  // Widths of a quarter pixel to six, every 7.5 degrees, at 16 sub-pixel
  // places: away from their ends, every pixel takes the strip's coverage
  // of its centre's distance within 2/255
  let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
    return;
  };
  let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
  let painter = Painter::new(&device, &queue);
  const CELL: u32 = 48;
  const HALF_LENGTH: f32 = 16.0;
  let size = (6 * CELL, 4 * CELL);
  let centre = Vec2::new(size.0 as f32, size.1 as f32) / 2.0;
  let mut worst = (0.0f32, String::new());
  let mut failures = Vec::new();
  let mut checked = 0usize;
  for width in [0.25f32, 0.5, 1.0, 1.5, 2.0, 3.0, 6.0] {
    let hw = width / 2.0;
    for k in 0..16u16 {
      let offset = Vec2::new(
        f32::from(k % 4) * 0.25 + 0.0625,
        f32::from(k / 4) * 0.25 + 0.03125,
      );
      let mut stream = LineStream::default();
      let mut placed = Vec::new();
      for i in 0..24u32 {
        let angle = (i as f32 * 7.5).to_radians();
        let dir = Vec2::from_angle(angle);
        let cell = Vec2::new((i % 6 * CELL) as f32, (i / 6 * CELL) as f32);
        let middle = cell + Vec2::splat(CELL as f32 / 2.0) + offset;
        let ends = [middle - dir * HALF_LENGTH, middle + dir * HALF_LENGTH];
        let points = points_of(&ends, hw, Color::WHITE);
        stream.polyline(
          &points,
          &style(LineJoin::Miter, LineCap::Butt, false, SOLID),
        );
        placed.push((cell, middle, dir));
      }
      let image = painter.paint(&device, &queue, &stream, (centre, 1.0), size, None);
      for (i, &(cell, middle, dir)) in placed.iter().enumerate() {
        for y in cell.y as u32..cell.y as u32 + CELL {
          for x in cell.x as u32..cell.x as u32 + CELL {
            let at = world_of(x, y, centre, size) - middle;
            if at.dot(dir).abs() > HALF_LENGTH - hw - 2.0 {
              continue;
            }
            let model = cover_strip(at.dot(dir.perp()), hw);
            let got = alpha(&image, x, y);
            let off = (got - model).abs();
            checked += 1;
            let what = format!(
              "width {width} angle {} offset {offset} pixel ({x}, {y}): {got} against {model}",
              i as f32 * 7.5
            );
            if off > worst.0 {
              worst = (off, what.clone());
            }
            if off > 2.0 / 255.0 + 1e-4 {
              failures.push(what);
            }
          }
        }
      }
    }
  }
  let error = pollster::block_on(scope.pop());
  assert!(error.is_none(), "{error:?}");
  eprintln!("{checked} pixels; worst {:.4} ({})", worst.0, worst.1);
  assert!(
    failures.is_empty(),
    "{} pixels off:\n{}",
    failures.len(),
    failures[..failures.len().min(30)].join("\n")
  );
}

/// A corner whose segments share their inner vertex: where it is and the
/// two inner edges' lines (a point and a normal each), as
/// `the_pushed_mesh_gives_every_pixel_its_coverage` records its zone
#[derive(Clone, Copy)]
struct Inner {
  at: Vec2,
  edges: [(Vec2, Vec2); 2],
}

/// How far along the inner edges from the inner vertex its zone reaches
const INNER_REACH: f32 = 4.0;

impl Inner {
  fn holds(&self, at: Vec2) -> bool {
    let distance = at.distance(self.at);
    let both = self
      .edges
      .iter()
      .all(|&(q, n)| (at - q).dot(n).abs() <= 1.0);
    distance <= 1.0 || (distance <= INNER_REACH && both)
  }

  /// The corner at `p` between segments along `d0` then `d1` of half width
  /// `hw`, if its inner side shares
  fn of(p: Vec2, d0: Vec2, d1: Vec2, hw: f32, shortest: f32) -> Option<Self> {
    let (n0, n1) = (d0.perp(), d1.perp());
    let side = if d0.perp_dot(d1) > 0.0 { -1.0 } else { 1.0 };
    let miter = (n0 + n1) / (1.0 + n0.dot(n1));
    (miter.length() * hw <= shortest / 2.0).then(|| Self {
      at: p - side * miter * hw,
      edges: [(p - side * n0 * hw, n0), (p - side * n1 * hw, n1)],
    })
  }

  /// A split corner's point `p`, between segments along `d0` then `d1`:
  /// the zone's lines are through `p` across each segment (the directions
  /// themselves as their normals, not the segments' normals), so it holds
  /// the patch about `p` within a pixel along both segments, where the
  /// two segments' cut ends cross
  fn split_point(p: Vec2, d0: Vec2, d1: Vec2) -> Self {
    Self {
      at: p,
      edges: [(p, d0), (p, d1)],
    }
  }
}

/// A split corner's inside, where its two strips overlap, unmeasured
#[derive(Clone, Copy)]
struct Overlap {
  at: Vec2,
  out: Vec2,
  segments: [(Vec2, Vec2); 2],
  hw: f32,
}

impl Overlap {
  fn holds(&self, at: Vec2) -> bool {
    let near = |(a, b): (Vec2, Vec2)| {
      let t = ((at - a).dot(b - a) / (b - a).length_squared()).clamp(0.0, 1.0);
      at.distance(a + (b - a) * t) <= self.hw + 1.0
    };
    (at - self.at).dot(self.out) < 0.0 && self.segments.iter().all(|&s| near(s))
  }
}

/// A line to hold to the truth: its points, and the pixels to look at
struct Case {
  what: String,
  points: Vec<Vec2>,
  hw: f32,
  join: LineJoin,
  cap: LineCap,
  closed: bool,
  around: Vec<Vec2>,
  reach: f32,
  inner: Vec<Inner>,
  overlap: Vec<Overlap>,
  /// A split corner's turn, its figure held apart
  split: Option<f32>,
}

/// The worst error of `case`'s pixels drawn on the GPU against the exact
/// area of its line unpushed, away from its inner corners, and the worst at
/// them; with where
fn worst_against_truth(
  device: &wgpu::Device,
  queue: &wgpu::Queue,
  painter: &Painter,
  case: &Case,
) -> ((f32, String), (f32, String)) {
  let points = points_of(&case.points, case.hw, Color::WHITE);
  let style = style(case.join, case.cap, case.closed, SOLID);
  let (mut stream, mut reference) = (LineStream::default(), Triangles::default());
  stream.polyline(&points, &style);
  reference.polyline(&points, &style);
  // The line's exact shape: its round pieces true discs
  let triangles = reference.true_shape();
  let (mut min, mut max) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
  for &p in &case.around {
    min = min.min(p - case.reach);
    max = max.max(p + case.reach);
  }
  let centre = ((min + max) / 2.0).round();
  let extent = ((max - min) / 2.0).ceil() + 2.0;
  let size = (2 * extent.x as u32, 2 * extent.y as u32);
  let image = painter.paint(device, queue, &stream, (centre, 1.0), size, None);
  let (mut outer, mut inner) = ((0.0f32, String::new()), (0.0f32, String::new()));
  for y in 0..size.1 {
    for x in 0..size.0 {
      let at = world_of(x, y, centre, size);
      if case.around.iter().all(|&p| at.distance(p) > case.reach)
        || case.overlap.iter().any(|o| o.holds(at))
      {
        continue;
      }
      let truth = triangles
        .iter()
        .map(|&tri| area_in_pixel(tri, at))
        .sum::<f32>()
        .min(1.0);
      let got = alpha(&image, x, y);
      let off = (got - truth).abs();
      let worst = if case.inner.iter().any(|corner| corner.holds(at)) {
        &mut inner
      } else {
        &mut outer
      };
      if off > worst.0 {
        *worst = (off, format!("{} at {at}: {got} against {truth}", case.what));
      }
    }
  }
  (outer, inner)
}

/// The cases: joins both ways at every sharpness, corners whose inner side
/// splits, every cap on one segment and two, closed corners
fn truth_cases() -> Vec<Case> {
  let mut cases = Vec::new();
  let offsets = [Vec2::ZERO, Vec2::new(0.5, 0.6), Vec2::new(0.8, 0.35)];
  for hw in [0.75f32, 1.5, 3.0, 6.0] {
    let length = 20.0 * hw + 10.0;
    for rotation in [0, 3, 5] {
      let d0 = Vec2::from_angle(rotation as f32 * std::f32::consts::TAU / 8.0 + 0.1);
      for &offset in &offsets {
        let p = Vec2::new(100.0, 100.0) + offset;
        for degrees in [5.0f32, 15.0, 30.0, 45.0, 60.0, 90.0, 120.0, 150.0, 165.0] {
          for turn in [1.0f32, -1.0] {
            let d1 = Vec2::from_angle(turn * degrees.to_radians()).rotate(d0);
            for join in JOINS {
              cases.push(Case {
                what: format!("{join:?} {degrees}° {turn} hw {hw} rot {rotation} {offset}"),
                points: vec![p - d0 * length, p, p + d1 * length],
                hw,
                join,
                cap: LineCap::Butt,
                closed: false,
                around: vec![p],
                reach: 5.0 * hw + 3.0,
                inner: Inner::of(p, d0, d1, hw, length).into_iter().collect(),
                overlap: Vec::new(),
                split: None,
              });
            }
          }
        }
        for degrees in [60.0f32, 90.0, 120.0, 150.0, 165.0] {
          for turn in [1.0f32, -1.0] {
            let d1 = Vec2::from_angle(turn * degrees.to_radians()).rotate(d0);
            let miter = (d0.perp() + d1.perp()) / (1.0 + d0.perp().dot(d1.perp()));
            let short = 1.9 * miter.length() * hw;
            let (a, b) = (p - d0 * short, p + d1 * short);
            let out = if d0.perp_dot(d1) > 0.0 { -miter } else { miter };
            for join in JOINS {
              cases.push(Case {
                what: format!("split {join:?} {degrees}° {turn} hw {hw} rot {rotation} {offset}"),
                points: vec![a, p, b],
                hw,
                join,
                cap: LineCap::Butt,
                closed: false,
                around: vec![p],
                reach: 3.0 * hw + 3.0,
                inner: vec![Inner::split_point(p, d0, d1)],
                overlap: vec![Overlap {
                  at: p,
                  out,
                  segments: [(a, p), (p, b)],
                  hw,
                }],
                split: Some(degrees),
              });
            }
          }
        }
        for cap in CAPS {
          let two = vec![p - d0 * length, p];
          let d1 = Vec2::from_angle(0.5).rotate(d0);
          let bent = vec![
            p - d0 * length * 2.0,
            p - d0 * length,
            p - d0 * length + d1 * length,
          ];
          for points in [two, bent] {
            let ends = vec![points[0], points[points.len() - 1]];
            cases.push(Case {
              what: format!(
                "{cap:?} {} points hw {hw} rot {rotation} {offset}",
                points.len()
              ),
              points,
              hw,
              join: LineJoin::Miter,
              cap,
              closed: false,
              around: ends,
              reach: 2.0 * hw + 3.0,
              inner: Vec::new(),
              overlap: Vec::new(),
              split: None,
            });
          }
        }
      }
    }
    let square = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
    let triangle = [(0.0, 0.0), (1.0, 0.0), (0.5, 0.8)];
    for shape in [&square[..], &triangle[..]] {
      let scale = 25.0 * hw + 10.0;
      let corners: Vec<Vec2> = shape
        .iter()
        .map(|&(x, y)| Vec2::new(50.0, 50.0) + Vec2::new(x, y) * scale + offsets[2])
        .collect();
      let n = corners.len();
      let inner: Vec<Inner> = (0..n)
        .filter_map(|i| {
          let (a, p, b) = (corners[(i + n - 1) % n], corners[i], corners[(i + 1) % n]);
          let shortest = p.distance(a).min(p.distance(b));
          Inner::of(p, (p - a).normalize(), (b - p).normalize(), hw, shortest)
        })
        .collect();
      for join in JOINS {
        for &corner in &corners {
          cases.push(Case {
            what: format!("closed {n} {join:?} hw {hw} at {corner}"),
            points: corners.clone(),
            hw,
            join,
            cap: LineCap::Butt,
            closed: true,
            around: vec![corner],
            reach: 5.0 * hw + 3.0,
            inner: inner.clone(),
            overlap: Vec::new(),
            split: None,
          });
        }
      }
    }
  }
  cases
}

#[test]
fn every_pixel_is_within_0_12_of_the_line_s_exact_area() {
  // Lines, miter, bevel and round joins, every cap and closed corners, drawn
  // on the GPU: every pixel within 0.12 of the exact area of the line
  // unpushed (the supersampler's limit, and what the coverage model is held
  // to in `the_pushed_mesh_gives_every_pixel_its_coverage`), but in the
  // recorded zones: at an inner corner within its figure, at a split
  // corner's point within its figure by turn, a split corner's inside
  // overlap unmeasured
  let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
    return;
  };
  const TOLERANCE: f32 = 0.12;
  const INNER: f32 = 0.45;
  // The recorded figures, and a byte's rounding
  const SPLIT: [(f32, f32); 5] = [
    (60.0, 0.12),
    (90.0, 0.13),
    (120.0, 0.22),
    (150.0, 0.33),
    (165.0, 0.38),
  ];
  let slack = 1.0 / 255.0;
  let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
  let painter = Painter::new(&device, &queue);
  let mut failures = Vec::new();
  let mut outer_worst = (0.0f32, String::new());
  let mut inner_worst = (0.0f32, String::new());
  let mut split_worst = SPLIT.map(|(degrees, _)| (degrees, 0.0f32, String::new()));
  for case in truth_cases() {
    let (outer, inner) = worst_against_truth(&device, &queue, &painter, &case);
    if let Some(degrees) = case.split {
      if let Some(worst) = split_worst.iter_mut().find(|w| w.0 == degrees)
        && outer.0 > worst.1
      {
        *worst = (degrees, outer.0, outer.1.clone());
      }
    } else {
      if outer.0 > TOLERANCE {
        failures.push(format!("{:.3}: {}", outer.0, outer.1));
      }
      if outer.0 > outer_worst.0 {
        outer_worst = outer;
      }
    }
    if inner.0 > inner_worst.0 {
      inner_worst = inner;
    }
  }
  let error = pollster::block_on(scope.pop());
  assert!(error.is_none(), "{error:?}");
  eprintln!(
    "worst away from the zones: {:.4} ({})",
    outer_worst.0, outer_worst.1
  );
  eprintln!(
    "worst at an inner corner: {:.4} ({})",
    inner_worst.0, inner_worst.1
  );
  for ((degrees, limit), (_, error, at)) in SPLIT.iter().zip(&split_worst) {
    eprintln!("worst at a split {degrees}° corner: {error:.4} ({at})");
    assert!(
      *error <= limit + slack,
      "a split {degrees}° corner is off by {error}: {at}"
    );
  }
  assert!(
    inner_worst.0 <= INNER + slack,
    "an inner corner is off by {:?}",
    inner_worst
  );
  assert!(
    failures.is_empty(),
    "{} cases off:\n{}",
    failures.len(),
    failures[..failures.len().min(30)].join("\n")
  );
}

/// Each column's summed alpha over the rows (or each row's over the
/// columns, `across_rows` false) of `image`, at `at`
fn summed(image: &image::RgbaImage, at: u32, across_rows: bool) -> f32 {
  let (w, h) = image.dimensions();
  if across_rows {
    (0..h).map(|y| alpha(image, at, y)).sum()
  } else {
    (0..w).map(|x| alpha(image, x, at)).sum()
  }
}

#[test]
fn a_half_pixel_line_draws_at_half_coverage_wherever_it_lies() {
  // Centred on a row of pixel centres it draws them at half coverage; moved
  // by sixteenths of a pixel, the coverage summed across it stays within 2%
  // of its width, along and across the axes
  let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
    return;
  };
  let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
  let painter = Painter::new(&device, &queue);
  let size = (64u32, 64u32);
  let centre = Vec2::new(32.0, 32.0);
  let mut sums = Vec::new();
  for horizontal in [true, false] {
    for k in 0..16u16 {
      let shift = f32::from(k) / 16.0;
      // Through pixel centres at the first step
      let ends = if horizontal {
        [Vec2::new(8.0, 32.5 + shift), Vec2::new(56.0, 32.5 + shift)]
      } else {
        [Vec2::new(32.5 + shift, 8.0), Vec2::new(32.5 + shift, 56.0)]
      };
      let mut stream = LineStream::default();
      let points = points_of(&ends, 0.25, Color::WHITE);
      stream.polyline(
        &points,
        &style(LineJoin::Miter, LineCap::Butt, false, SOLID),
      );
      let image = painter.paint(&device, &queue, &stream, (centre, 1.0), size, None);
      if k == 0 {
        let (x, y) = if horizontal { (30, 32) } else { (32, 30) };
        let half = alpha(&image, x, y);
        assert!(
          (half - 0.5).abs() <= 1.0 / 255.0,
          "horizontal {horizontal}: {half}"
        );
      }
      let sum = summed(&image, 30, horizontal);
      sums.push((horizontal, shift, sum));
    }
  }
  let error = pollster::block_on(scope.pop());
  assert!(error.is_none(), "{error:?}");
  eprintln!("{sums:?}");
  let (low, high) = sums
    .iter()
    .fold((f32::MAX, f32::MIN), |(lo, hi), &(_, _, s)| {
      (lo.min(s), hi.max(s))
    });
  eprintln!("summed coverage {low} to {high}");
  for &(horizontal, shift, sum) in &sums {
    assert!(
      (sum - 0.5).abs() / 0.5 < 0.02,
      "horizontal {horizontal} shift {shift}: {sum}"
    );
  }
  assert!((high - low) / 0.5 < 0.02, "{low} to {high}");
}

/// Each column's spread of its summed alpha over a run of placements, as a
/// fraction of its mean: the worst column's, the least sum seen, and the
/// mean sum over every column and placement
fn column_spread(sums: &[Vec<f32>]) -> (f32, f32, f32) {
  let columns = sums.first().map_or(0, Vec::len);
  let (mut worst, mut least, mut total) = (0.0f32, f32::MAX, 0.0f32);
  for c in 0..columns {
    let (mut lo, mut hi, mut sum) = (f32::MAX, f32::MIN, 0.0f32);
    for run in sums {
      lo = lo.min(run[c]);
      hi = hi.max(run[c]);
      sum += run[c];
    }
    let mean = sum / sums.len() as f32;
    worst = worst.max((hi - lo) / mean.max(1e-6));
    least = least.min(lo);
    total += mean;
  }
  (worst, least, total / columns.max(1) as f32)
}

#[test]
fn thin_lines_stay_whole_and_steady_wherever_they_lie() {
  // Solid, Soft and Glow lines a quarter of a pixel to one and a half in
  // half width, level and at 30 degrees, moved by sixteenths of a pixel:
  // every column's summed alpha away from the ends is never empty, and its
  // spread over the moves is within 5% level; at 30 degrees within the strip
  // model's own spread for a Solid line of the width covered (a Soft or Glow
  // line covers at least half a pixel of half width), and a byte a pixel
  let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
    return;
  };
  let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
  let painter = Painter::new(&device, &queue);
  let size = (160u32, 96u32);
  let centre = Vec2::new(80.0, 48.0);
  const HALF_LENGTH: f32 = 70.0;
  let mut failures = Vec::new();
  for (name, look) in [("solid", SOLID), ("soft", SOFT), ("glow", GLOW)] {
    for degrees in [0.0f32, 30.0] {
      let dir = Vec2::from_angle(degrees.to_radians());
      // The columns at least 4 px along from either end
      let columns: Vec<u32> = (0..size.0)
        .filter(|&x| ((x as f32 + 0.5 - centre.x) / dir.x).abs() <= HALF_LENGTH - 4.0)
        .collect();
      for hw in [0.25f32, 0.5, 1.0, 1.5] {
        let covered = if look == SOLID { hw } else { hw.max(0.5) };
        let (mut drawn, mut model) = (Vec::new(), Vec::new());
        for k in 0..16u16 {
          let middle = Vec2::new(centre.x, centre.y + 0.5 + f32::from(k) / 16.0);
          let ends = [middle - dir * HALF_LENGTH, middle + dir * HALF_LENGTH];
          let mut stream = LineStream::default();
          let points = points_of(&ends, hw, Color::WHITE);
          stream.polyline(&points, &style(LineJoin::Miter, LineCap::Butt, false, look));
          let image = painter.paint(&device, &queue, &stream, (centre, 1.0), size, None);
          drawn.push(columns.iter().map(|&x| summed(&image, x, true)).collect());
          model.push(
            columns
              .iter()
              .map(|&x| {
                (0..size.1)
                  .map(|y| {
                    let at = world_of(x, y, centre, size) - middle;
                    cover_strip(at.dot(dir.perp()), covered)
                  })
                  .sum::<f32>()
              })
              .collect(),
          );
        }
        let (spread, least, mean) = column_spread(&drawn);
        let (own, _, _) = column_spread(&model);
        let limit = if degrees == 0.0 {
          0.05
        } else {
          own.max(0.05) + 2.0 * (2.0 / 255.0) / mean
        };
        eprintln!(
          "{name} {degrees}° hw {hw}: column sums from {least:.4}, mean {mean:.4}, spread {:.1}% (the model's {:.1}%, limit {:.1}%)",
          spread * 100.0,
          own * 100.0,
          limit * 100.0
        );
        if least <= 0.0 || spread > limit {
          failures.push(format!(
            "{name} {degrees}° hw {hw}: least {least}, spread {spread} over {limit}"
          ));
        }
      }
    }
  }
  let error = pollster::block_on(scope.pop());
  assert!(error.is_none(), "{error:?}");
  assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The value noise of `line.wgsl`, for the displacement a test expects
fn value_noise(p: f32) -> f32 {
  let hash = |p: f32| {
    let mut q = (p * 0.1031).fract();
    q *= q + 33.33;
    q *= q + q;
    q.fract()
  };
  let (i, f) = (p.floor(), p - p.floor());
  let u = f * f * (3.0 - 2.0 * f);
  hash(i) + (hash(i + 1.0) - hash(i)) * u
}

#[test]
fn a_displaced_line_keeps_its_coverage_across_it() {
  // Lightning and Ribbon lines across the image, Solid and a thin Glow,
  // their vertices moved along their perpendicular: away from the ends no
  // column is empty and each column's summed alpha stays within 3% of the
  // undisplaced line's, the fringe moving with the vertices
  let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
    return;
  };
  let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
  let painter = Painter::new(&device, &queue);
  let size = (224u32, 128u32);
  let centre = Vec2::new(112.0, 64.0);
  let count = 97;
  let ends = (16.0f32, 208.0f32);
  let row = 64.5;
  let points: Vec<Vec2> = (0..count)
    .map(|i| {
      Vec2::new(
        ends.0 + (ends.1 - ends.0) * i as f32 / (count - 1) as f32,
        row,
      )
    })
    .collect();
  let columns: Vec<u32> = (24..200).collect();
  // Its displacement at `u` (`lightning_displacement`, `ribbon_displacement`
  // at time 0)
  let lightning = |u: f32, jitter: f32| {
    (0..3)
      .map(|i| {
        let (amp, freq) = (jitter / 2f32.powi(i), 4.0 * 2f32.powi(i));
        (value_noise(u * freq + i as f32 * 17.0) * 2.0 - 1.0) * amp
      })
      .sum::<f32>()
  };
  let ribbon = |u: f32, amplitude: f32, frequency: f32| {
    let tau = std::f32::consts::TAU;
    ((u * frequency * tau).sin() * 0.7 + (u * frequency * 1.618 * tau).sin() * 0.3) * amplitude
  };
  let mut failures = Vec::new();
  for (name, look, hw, effect) in [
    ("lightning solid", SOLID, 2.0f32, [1.0, 6.0, 0.0, 3.0]),
    ("lightning glow", GLOW, 1.0, [1.0, 6.0, 0.0, 3.0]),
    ("ribbon solid", SOLID, 2.0, [4.0, 6.0, 2.0, 0.0]),
    // As steep as the trails demo's ribbon
    ("steep ribbon solid", SOLID, 2.0, [4.0, 12.0, 6.0, 0.0]),
  ] {
    let mut sums = Vec::new();
    for displaced in [false, true] {
      let mut line_style = style(LineJoin::Miter, LineCap::Butt, false, look);
      if displaced {
        line_style.params = [effect[0] + look, effect[1], effect[2], effect[3]];
      }
      let mut stream = LineStream::default();
      stream.polyline(&points_of(&points, hw, Color::WHITE), &line_style);
      let image = painter.paint(&device, &queue, &stream, (centre, 1.0), size, None);
      sums.push(
        columns
          .iter()
          .map(|&x| summed(&image, x, true))
          .collect::<Vec<f32>>(),
      );
    }
    let (plain, moved) = (&sums[0], &sums[1]);
    let mut worst = (0.0f32, 0u32);
    for (c, &x) in columns.iter().enumerate() {
      let off = (moved[c] - plain[c]).abs() / plain[c];
      if off > worst.0 {
        worst = (off, x);
      }
      if moved[c] <= 0.0 || off > 0.03 {
        failures.push(format!(
          "{name} column {x}: {} against {}",
          moved[c], plain[c]
        ));
      }
    }
    // How much the shear between a piece's ends narrows it: the drawn width
    // across the moved line, against the line's
    let shift = |u: f32| {
      if effect[0] == 1.0 {
        lightning(u, effect[1])
      } else {
        ribbon(u, effect[1], effect[2])
      }
    };
    let (mut along, mut path, mut narrowest) = (0.0f32, 0.0f32, 1.0f32);
    for i in 0..count - 1 {
      let (a, b) = (points[i], points[i + 1]);
      if a.x < columns[0] as f32 || b.x > columns[columns.len() - 1] as f32 + 1.0 {
        continue;
      }
      let (u0, u1) = (
        i as f32 / (count - 1) as f32,
        (i + 1) as f32 / (count - 1) as f32,
      );
      let length = b.x - a.x;
      let slanted = length.hypot(shift(u1) - shift(u0));
      along += length;
      path += slanted;
      narrowest = narrowest.min(length / slanted);
    }
    eprintln!(
      "{name}: column sums within {:.2}% of the undisplaced line's (column {}); the moved line drawn {:.0}% as wide on average, {:.0}% at the steepest piece",
      worst.0 * 100.0,
      worst.1,
      along / path * 100.0,
      narrowest * 100.0
    );
  }
  let error = pollster::block_on(scope.pop());
  assert!(error.is_none(), "{error:?}");
  assert!(
    failures.is_empty(),
    "{} columns off:\n{}",
    failures.len(),
    failures[..failures.len().min(30)].join("\n")
  );
}

#[test]
fn a_translucent_corner_is_no_darker_than_its_segments() {
  // Half-transparent lines turning at every sharpness, every join, and a
  // closed outline's corners: no pixel holds more alpha than a segment's
  // middle
  let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
    return;
  };
  let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
  let painter = Painter::new(&device, &queue);
  let color = Color::new(255, 255, 255, 128);
  let size = (128u32, 128u32);
  let centre = Vec2::new(64.0, 64.0);
  let mut failures = Vec::new();
  let mut worst = 0u8;
  for hw in [0.75f32, 3.0] {
    for join in JOINS {
      let mut shapes: Vec<(Vec<Vec2>, bool, String)> = Vec::new();
      for degrees in [15.0f32, 45.0, 90.0, 120.0, 150.0, 165.0] {
        for turn in [1.0f32, -1.0] {
          let d0 = Vec2::from_angle(0.3);
          let d1 = Vec2::from_angle(turn * degrees.to_radians()).rotate(d0);
          let p = Vec2::new(64.3, 64.6);
          shapes.push((
            vec![p - d0 * 60.0, p, p + d1 * 60.0],
            false,
            format!("{join:?} {degrees}° {turn} hw {hw}"),
          ));
        }
      }
      shapes.push((
        vec![
          Vec2::new(20.2, 20.7),
          Vec2::new(100.2, 20.7),
          Vec2::new(100.2, 100.7),
          Vec2::new(20.2, 100.7),
        ],
        true,
        format!("closed {join:?} hw {hw}"),
      ));
      for (points, closed, what) in shapes {
        let mut stream = LineStream::default();
        let points = points_of(&points, hw, color);
        stream.polyline(&points, &style(join, LineCap::Butt, closed, SOLID));
        let image = painter.paint(&device, &queue, &stream, (centre, 1.0), size, None);
        let most = image.pixels().map(|pixel| pixel.0[3]).max().unwrap_or(0);
        worst = worst.max(most);
        if most > 129 {
          failures.push(format!("{what}: alpha {most}"));
        }
      }
    }
  }
  let error = pollster::block_on(scope.pop());
  assert!(error.is_none(), "{error:?}");
  eprintln!("the most alpha: {worst}");
  assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The smoothstep of WGSL
fn smoothstep(low: f32, high: f32, x: f32) -> f32 {
  let t = ((x - low) / (high - low)).clamp(0.0, 1.0);
  t * t * (3.0 - 2.0 * t)
}

/// Soft's or Glow's alpha at `edge`, the distance from the centre line over
/// the half width
fn shape_at(look: f32, edge: f32) -> f32 {
  if look == SOFT {
    1.0 - smoothstep(0.7, 1.0, edge)
  } else {
    let core = 1.0 - smoothstep(0.0, 0.3, edge);
    let halo = 1.0 - smoothstep(0.3, 1.0, edge);
    core + halo * 0.3
  }
}

/// Soft's or Glow's alpha over a pixel `d` pixels from the centre line of a
/// line `hw` pixels in half width: the shape averaged over the part of the
/// pixel's box the line covers (summed in fine steps), toward its mean over
/// the width as the half width falls from 3 pixels to 1
fn shape_over_pixel(look: f32, d: f32, hw: f32) -> f32 {
  let mean = if look == SOFT { 0.85 } else { 0.345 };
  let (lo, hi) = ((d - 0.5).max(-hw), (d + 0.5).min(hw));
  let boxed = if hi > lo {
    const STEPS: usize = 4000;
    let step = (hi - lo) / STEPS as f32;
    (0..STEPS)
      .map(|i| shape_at(look, (lo + (i as f32 + 0.5) * step).abs() / hw))
      .sum::<f32>()
      / STEPS as f32
  } else {
    mean
  };
  mean + (boxed - mean) * ((hw - 1.0) / 2.0).clamp(0.0, 1.0)
}

#[test]
fn soft_and_glow_shape_by_the_distance_over_the_half_width() {
  // A wide straight line, row by row across it: Soft fades over its outer
  // 30% and Glow is a core and a halo, each by the true distance from the
  // centre over the half width, averaged over the part of the pixel the
  // line covers (and toward its mean below 3 pixels of half width), times
  // the pixel's coverage
  let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
    return;
  };
  let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
  let painter = Painter::new(&device, &queue);
  let size = (64u32, 64u32);
  let centre = Vec2::new(32.0, 32.0);
  let mut failures = Vec::new();
  let mut worst = (0.0f32, String::new());
  for (name, look) in [("soft", SOFT), ("glow", GLOW)] {
    for (hw, y_centre) in [(8.0f32, 32.5), (5.25, 31.8), (2.0, 32.3)] {
      let mut stream = LineStream::default();
      let ends = [Vec2::new(4.0, y_centre), Vec2::new(60.0, y_centre)];
      let points = points_of(&ends, hw, Color::WHITE);
      stream.polyline(&points, &style(LineJoin::Miter, LineCap::Butt, false, look));
      let image = painter.paint(&device, &queue, &stream, (centre, 1.0), size, None);
      for y in 0..size.1 {
        let d = (world_of(32, y, centre, size).y - y_centre).abs();
        let want = (shape_over_pixel(look, d, hw) * cover_strip(d, hw)).min(1.0);
        let got = alpha(&image, 32, y);
        let off = (got - want).abs();
        let what = format!("{name} hw {hw} row {y} (d {d}): {got} against {want}");
        if off > worst.0 {
          worst = (off, what.clone());
        }
        if off > 2.0 / 255.0 {
          failures.push(what);
        }
      }
    }
  }
  let error = pollster::block_on(scope.pop());
  assert!(error.is_none(), "{error:?}");
  eprintln!("worst {:.4} ({})", worst.0, worst.1);
  assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn each_line_s_fragments_read_its_own_entry_in_the_line_table() {
  // A vertex hands its fragments only what varies across its line; what
  // the line shares (its style, a two-point line's half length between its
  // caps, a textured line's slot) the fragments read by the line's number.
  // Lines of different looks in one stream each draw as they do alone
  let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
    return;
  };
  let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
  let painter = Painter::new(&device, &queue);
  let size = (64u32, 64u32);
  let centre = Vec2::new(32.0, 32.0);
  let lines = [
    (8.5, SOLID, LineCap::Round, 4.0),
    (24.5, GLOW, LineCap::Butt, 5.0),
    (40.5, SOFT, LineCap::Square, 3.0),
    (55.5, SOLID, LineCap::Square, 2.5),
  ];
  let add = |stream: &mut LineStream, &(y, look, cap, hw): &(f32, f32, LineCap, f32)| {
    // Two points: its caps measured as one strip along it
    let ends = [Vec2::new(10.0 + hw, y), Vec2::new(50.0 - 2.0 * hw, y)];
    let points = points_of(&ends, hw, Color::WHITE);
    stream.polyline(&points, &style(LineJoin::Miter, cap, false, look));
  };
  let mut together = LineStream::default();
  for line in &lines {
    add(&mut together, line);
  }
  let all = painter.paint(&device, &queue, &together, (centre, 1.0), size, None);
  let mut differ = Vec::new();
  for (index, line) in lines.iter().enumerate() {
    let mut alone = LineStream::default();
    add(&mut alone, line);
    let own = painter.paint(&device, &queue, &alone, (centre, 1.0), size, None);
    let rows = (line.0 - line.3 - 2.0) as u32..(line.0 + line.3 + 2.0) as u32;
    for y in rows {
      for x in 0..size.0 {
        if all.get_pixel(x, y) != own.get_pixel(x, y) {
          differ.push(format!(
            "line {index} at ({x}, {y}): {:?} among the others, {:?} alone",
            all.get_pixel(x, y).0,
            own.get_pixel(x, y).0
          ));
        }
      }
    }
  }
  let error = pollster::block_on(scope.pop());
  assert!(error.is_none(), "{error:?}");
  assert!(
    differ.is_empty(),
    "{}",
    differ[..differ.len().min(20)].join("\n")
  );
}

/// Where a column of `a_textured_line_s_texture_runs_on_across_the_fringe`
/// runs: along a segment, through a join's corner point, through a cap
#[derive(Clone, Copy)]
enum Through {
  Segment,
  Join,
  Cap,
}

#[test]
fn a_textured_line_s_texture_runs_on_across_the_fringe() {
  // A texture whose rows count up drawn across a wide line: every pixel
  // inside the line reads the row of its true distance across (v = 0.5 -
  // 0.5 d ÷ hw), so the fringe does not squeeze the texture into the line,
  // along a segment, through a miter and a round join, and in a round cap
  let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
    return;
  };
  let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
  let painter = Painter::new(&device, &queue);
  let texture = rows_texture(&device, &queue, 256, |r| [r as u8, 0, 0, 255]);
  let size = (64u32, 64u32);
  let centre = Vec2::new(32.0, 32.0);
  let mut failures = Vec::new();
  let mut worst = (0.0f32, String::new());
  for (hw, y_centre) in [(8.0f32, 32.5), (6.5, 31.75)] {
    // A 30° bend at x 32, its join's wedge on the side before the line
    // (d < 0), and a line ending at x 40 in a round cap
    let corner = Vec2::new(32.0, y_centre);
    let bent = vec![
      Vec2::new(-20.0, y_centre),
      corner,
      Vec2::new(84.0, y_centre + 30.0),
    ];
    let end = Vec2::new(40.0, y_centre);
    let capped = vec![Vec2::new(-20.0, y_centre), end];
    for (what, ends, join, cap, columns) in [
      (
        "miter",
        bent.clone(),
        LineJoin::Miter,
        LineCap::Butt,
        vec![(16, Through::Segment), (32, Through::Join)],
      ),
      (
        "round join",
        bent,
        LineJoin::Round,
        LineCap::Butt,
        vec![(16, Through::Segment), (32, Through::Join)],
      ),
      (
        "round cap",
        capped,
        LineJoin::Miter,
        LineCap::Round,
        vec![(44, Through::Cap)],
      ),
    ] {
      let mut stream = LineStream::default();
      let points = points_of(&ends, hw, Color::WHITE);
      stream.polyline(&points, &style(join, cap, false, SOLID));
      let image = painter.paint(
        &device,
        &queue,
        &stream,
        (centre, 1.0),
        size,
        Some(&texture),
      );
      // Left of the bend, where the line runs straight along x: v by the
      // distance across. Through the corner point on the wedge's side,
      // where the join reaches out: by the distance from the corner. Past
      // the end, through the round cap: across, as along a segment. Each
      // round piece's `uv.y` extrapolated over the fringe with its offset
      for (x, through) in columns {
        for y in 0..size.1 {
          let at = world_of(x, y, centre, size);
          let d = at.y - y_centre;
          let (reach, across) = match through {
            Through::Segment => (d.abs(), d),
            Through::Join if d <= -1.0 => (at.distance(corner), -at.distance(corner)),
            Through::Join => continue,
            Through::Cap => (at.distance(end), d),
          };
          if reach > hw - 0.5 {
            continue;
          }
          let v = 0.5 - 0.5 * across / hw;
          let want = (v * 256.0 - 0.5).clamp(0.0, 255.0);
          let got = f32::from(image.get_pixel(x, y).0[0]);
          let off = (got - want).abs();
          let what = format!("hw {hw} {what} column {x} row {y} (d {d}): red {got} against {want}");
          if off > worst.0 {
            worst = (off, what.clone());
          }
          if off > 2.0 {
            failures.push(what);
          }
        }
      }
    }
  }
  let error = pollster::block_on(scope.pop());
  assert!(error.is_none(), "{error:?}");
  eprintln!("worst {:.2} ({})", worst.0, worst.1);
  assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Straight lines `width` pixels wide at `angles` (degrees from the x
/// axis), each in a cell of its own and moved by `offset`, drawn with
/// `edge` into a pixel-art target or not: the image and each line's cell,
/// middle and direction
struct Straight {
  image: image::RgbaImage,
  placed: Vec<(Vec2, Vec2, Vec2)>,
}

const STRAIGHT_CELL: u32 = 48;
const STRAIGHT_HALF_LENGTH: f32 = 16.0;

impl Straight {
  #[allow(clippy::too_many_arguments)]
  fn paint(
    painter: &Painter,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    width: f32,
    angles: &[f32],
    offset: Vec2,
    edge: Option<LineEdge>,
    pixel_art: bool,
  ) -> Self {
    const ACROSS: u32 = 8;
    let rows = (angles.len() as u32).div_ceil(ACROSS);
    let size = (ACROSS * STRAIGHT_CELL, rows * STRAIGHT_CELL);
    let centre = Vec2::new(size.0 as f32, size.1 as f32) / 2.0;
    let mut stream = LineStream::default();
    let mut placed = Vec::new();
    for (i, &angle) in (0u32..).zip(angles) {
      let dir = Vec2::from_angle(angle.to_radians());
      let cell = Vec2::new(
        (i % ACROSS * STRAIGHT_CELL) as f32,
        (i / ACROSS * STRAIGHT_CELL) as f32,
      );
      let middle = cell + Vec2::splat(STRAIGHT_CELL as f32 / 2.0) + offset;
      let ends = [
        middle - dir * STRAIGHT_HALF_LENGTH,
        middle + dir * STRAIGHT_HALF_LENGTH,
      ];
      let points = points_of(&ends, width / 2.0, Color::WHITE);
      let style = PolyStyle {
        edge,
        ..style(LineJoin::Miter, LineCap::Butt, false, SOLID)
      };
      stream.polyline(&points, &style);
      placed.push((cell, middle, dir));
    }
    let image = painter.paint_on(device, queue, &stream, (centre, 1.0), size, None, pixel_art);
    Self { image, placed }
  }

  /// Where each line's drawn pixels per column (per row where it runs
  /// steeper than 45 degrees) are not `per`, away from its ends
  fn miscounted(&self, per: usize) -> Vec<String> {
    let mut wrong = Vec::new();
    for &(cell, middle, dir) in &self.placed {
      let steep = dir.y.abs() > dir.x.abs();
      let (x0, y0) = (cell.x as u32, cell.y as u32);
      for line in 0..STRAIGHT_CELL {
        // The column's (or row's) centre against the middle, along the
        // major axis
        let along = if steep {
          (y0 + line) as f32 + 0.5 - middle.y
        } else {
          (x0 + line) as f32 + 0.5 - middle.x
        };
        let major = if steep { dir.y } else { dir.x };
        if (along / major).abs() > STRAIGHT_HALF_LENGTH - 2.0 {
          continue;
        }
        let drawn = (0..STRAIGHT_CELL)
          .filter(|&k| {
            let (x, y) = if steep {
              (x0 + k, y0 + line)
            } else {
              (x0 + line, y0 + k)
            };
            self.image.get_pixel(x, y).0[3] > 0
          })
          .count();
        if drawn != per {
          let angle = dir.y.atan2(dir.x).to_degrees();
          wrong.push(format!(
            "angle {angle:.1}, middle {middle}: {} {line} has {drawn}",
            if steep { "row" } else { "column" }
          ));
        }
      }
    }
    wrong
  }

  /// How many pixels are drawn only partly
  fn partial(&self) -> usize {
    self
      .image
      .pixels()
      .filter(|pixel| pixel.0[3] != 0 && pixel.0[3] != 255)
      .count()
  }
}

/// Sixteen places on a quarter-pixel grid (whole and half pixels among
/// them, where a band's edge falls on pixel centres) and two off it
fn straight_offsets() -> Vec<Vec2> {
  let mut offsets: Vec<Vec2> = (0..16u16)
    .map(|k| Vec2::new(f32::from(k % 4) * 0.25, f32::from(k / 4) * 0.25))
    .collect();
  offsets.extend([Vec2::new(0.1, 0.37), Vec2::new(0.63, 0.06)]);
  offsets
}

#[test]
fn a_hard_line_is_one_pixel_per_column_at_any_angle() {
  // A pixel wide line on a pixel-art target, every 1.5 degrees from 0 to
  // 90 at 18 places: one whole pixel in each column (each row past 45
  // degrees), with no gap or double, as a line drawn in pixel art
  let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
    return;
  };
  let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
  let painter = Painter::new(&device, &queue);
  let angles: Vec<f32> = (0..=60u16).map(|i| f32::from(i) * 1.5).collect();
  let mut wrong = Vec::new();
  for offset in straight_offsets() {
    let lines = Straight::paint(&painter, &device, &queue, 1.0, &angles, offset, None, true);
    wrong.extend(lines.miscounted(1));
    assert_eq!(lines.partial(), 0, "offset {offset}: pixels partly drawn");
  }
  let error = pollster::block_on(scope.pop());
  assert!(error.is_none(), "{error:?}");
  assert!(
    wrong.is_empty(),
    "{} columns or rows miscounted:\n{}",
    wrong.len(),
    wrong[..wrong.len().min(30)].join("\n")
  );
}

#[test]
fn a_hard_line_is_its_width_in_pixels_per_column() {
  // Hard set on a smooth target: lines of 2 and 3 pixels are 2 and 3
  // pixels a column (or row), and ones narrower than a pixel one
  let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
    return;
  };
  let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
  let painter = Painter::new(&device, &queue);
  let angles: Vec<f32> = (0..=12u16).map(|i| f32::from(i) * 7.5).collect();
  let mut wrong = Vec::new();
  for (width, per) in [(0.25, 1), (0.5, 1), (2.0, 2), (3.0, 3)] {
    for offset in straight_offsets() {
      let edge = Some(LineEdge::Hard);
      let lines = Straight::paint(
        &painter, &device, &queue, width, &angles, offset, edge, false,
      );
      wrong.extend(
        lines
          .miscounted(per)
          .into_iter()
          .map(|what| format!("width {width}, {what}")),
      );
      assert_eq!(lines.partial(), 0, "width {width}, offset {offset}");
    }
  }
  let error = pollster::block_on(scope.pop());
  assert!(error.is_none(), "{error:?}");
  assert!(
    wrong.is_empty(),
    "{} columns or rows miscounted:\n{}",
    wrong.len(),
    wrong[..wrong.len().min(30)].join("\n")
  );
}

#[test]
fn a_line_on_a_pixel_art_target_draws_whole_pixels_unless_smooth() {
  // Zigzags of every join and cap at three widths: on a pixel-art target
  // every pixel is drawn whole or not at all, unless the line is Smooth;
  // elsewhere pixels are partly covered, unless the line is Hard
  let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
    return;
  };
  let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
  let painter = Painter::new(&device, &queue);
  let size = (240u32, 240u32);
  let centre = Vec2::new(size.0 as f32, size.1 as f32) / 2.0;
  let stream_of = |edge: Option<LineEdge>| {
    let mut stream = LineStream::default();
    for (i, (join, cap)) in JOINS
      .iter()
      .flat_map(|&join| CAPS.iter().map(move |&cap| (join, cap)))
      .enumerate()
    {
      for (j, width) in [1.0f32, 2.5, 5.0].into_iter().enumerate() {
        let at = Vec2::new(12.3 + 76.0 * j as f32, 10.6 + 25.0 * i as f32);
        let zigzag = [
          at,
          at + Vec2::new(14.2, 13.1),
          at + Vec2::new(30.7, 1.4),
          at + Vec2::new(52.1, 15.8),
        ];
        let style = PolyStyle {
          edge,
          ..style(join, cap, false, SOLID)
        };
        stream.polyline(&points_of(&zigzag, width / 2.0, Color::WHITE), &style);
      }
    }
    stream
  };
  for (edge, pixel_art, whole) in [
    (None, true, true),
    (Some(LineEdge::Smooth), true, false),
    (None, false, false),
    (Some(LineEdge::Hard), false, true),
  ] {
    let image = painter.paint_on(
      &device,
      &queue,
      &stream_of(edge),
      (centre, 1.0),
      size,
      None,
      pixel_art,
    );
    let drawn = image.pixels().filter(|pixel| pixel.0[3] != 0).count();
    let partial = image
      .pixels()
      .filter(|pixel| pixel.0[3] != 0 && pixel.0[3] != 255)
      .count();
    let what = format!("{edge:?} on a pixel-art target {pixel_art}");
    assert!(drawn > 2000, "{what}: {drawn} pixels drawn");
    if whole {
      assert_eq!(
        partial, 0,
        "{what}: {partial} of {drawn} pixels partly drawn"
      );
    } else {
      assert!(
        partial > 500,
        "{what}: {partial} of {drawn} pixels partly drawn"
      );
    }
  }
  let error = pollster::block_on(scope.pop());
  assert!(error.is_none(), "{error:?}");
}
