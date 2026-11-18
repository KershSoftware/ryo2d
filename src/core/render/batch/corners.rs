//! The corner pass: once a frame, before the first camera pass and after
//! the line stream's upload, one compute dispatch works out every point's
//! corner (`core/line_corners.wgsl`, the one corner function `lp_joint`)
//! into a record a point and a limits array, which every line vertex at the
//! point reads (`line.wgsl`) instead of working out three corners itself.
//!
//! - **A record a point** (`CORNER_BYTES`: the turn's vectors and figures,
//!   its limits and its decisions as bits; `LineCorner` in
//!   `line_stream.wgsl`), and the limits again, two floats a point
//!   (`LIMIT_BYTES`), for the vertices of the corners beside it. Both are
//!   storage bindings, so the stream's points are capped by what one holds
//!   (`StreamCap`).
//! - **Shared vertices agree bit for bit:** each corner is worked out once
//!   and read by every piece at it, the vertex stage builds every slot from
//!   it by the one shader's arithmetic, and the position is `@invariant`
//!   (`the_gpu_builds_the_reference_s_vertices`, its copies;
//!   `a_translucent_line_drawn_on_the_gpu_counts_every_pixel_once`).
//! - **One thread a point**, 64 a workgroup; past 65,535 workgroups across
//!   they go on in further rows of the grid (`grid`;
//!   `the_corner_pass_goes_on_in_rows_past_its_width`).
//! - **Skipped with no lines, and for a small stream** (under
//!   `CORNER_PASS_FROM` points: `builds_corners`,
//!   `a_small_stream_skips_the_corner_pass`), whose vertices work their
//!   corners out in place with the same function the UI uses
//!   (`lp_corner_here`), told so by the stream's uniform (`line_frame.y`, 0),
//!   and build the same vertices bit for bit
//!   (`vertices_working_their_corners_out_in_place_equal_the_corner_pass_s`;
//!   the count draw draws both ways). It binds the stream's own buffers, so
//!   it needs no copy (`Batch2D::encode_corners`).

/// The fewest points the corner pass runs for: below it each vertex works
/// its corners out in place (`lp_corner_here`, as the UI does), which costs
/// a small stream less than a dispatch and its wait (measured: 6,000 points
/// draw 3% to 11% faster in place, 60,000 and 600,000 up to 8% slower; the
/// threshold lies between, not measured closer)
pub(crate) const CORNER_PASS_FROM: usize = 16_384;

/// Whether the corner pass runs for a stream of `points` points
pub(crate) fn builds_corners(points: usize) -> bool {
  points >= CORNER_PASS_FROM
}

/// The bytes of a point's corner record (`LineCorner`)
pub(crate) const CORNER_BYTES: usize = 64;
/// The bytes of a point's limits (before and after)
pub(crate) const LIMIT_BYTES: usize = 8;
/// Threads a workgroup (`cs_line_corners`)
const WORKGROUP: u32 = 64;
/// The most workgroups a dispatch has along one axis (WebGPU's default
/// `maxComputeWorkgroupsPerDimension`)
pub(crate) const MAX_ACROSS: u32 = 65_535;

/// The workgroups across and down that give each of `points` a thread, at
/// most `across` workgroups a row
pub(crate) fn grid(points: u32, across: u32) -> (u32, u32) {
  let groups = points.div_ceil(WORKGROUP);
  let across = across.max(1);
  if groups <= across {
    (groups, 1)
  } else {
    (across, groups.div_ceil(across))
  }
}

/// The corner pass's pipeline and the layout of its group: the line table,
/// the points, the corner records and limits it writes, and the point count
pub(crate) struct CornerPass {
  pipeline: wgpu::ComputePipeline,
  layout: wgpu::BindGroupLayout,
}

impl CornerPass {
  pub(crate) fn new(device: &wgpu::Device) -> Self {
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
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
      label: Some("Line Corner Layout"),
      entries: &[
        storage(0, true),
        storage(1, true),
        storage(2, false),
        storage(3, false),
        wgpu::BindGroupLayoutEntry {
          binding: 4,
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
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("Line Corner Pipeline Layout"),
      bind_group_layouts: &[Some(&layout)],
      immediate_size: 0,
    });
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some("Line Corner Shader"),
      source: wgpu::ShaderSource::Wgsl(crate::core::shaders::compose::LINE_CORNERS.into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
      label: Some("Line Corners"),
      layout: Some(&pipeline_layout),
      module: &module,
      entry_point: Some("cs_line_corners"),
      compilation_options: Default::default(),
      cache: None,
    });
    Self { pipeline, layout }
  }

  /// The pass's group over the stream's line table and points, its corner
  /// records and limits, and the uniform holding its point count
  pub(crate) fn group(
    &self,
    device: &wgpu::Device,
    buffers: [&wgpu::Buffer; 5],
  ) -> wgpu::BindGroup {
    let entries: Vec<wgpu::BindGroupEntry> = (0u32..)
      .zip(buffers)
      .map(|(binding, buffer)| wgpu::BindGroupEntry {
        binding,
        resource: buffer.as_entire_binding(),
      })
      .collect();
    crate::core::graphics::bind_groups::create(
      device,
      &wgpu::BindGroupDescriptor {
        label: Some("Line Corner Bind Group"),
        layout: &self.layout,
        entries: &entries,
      },
    )
  }

  /// Work out the corners of `points` points through `group`, at most
  /// `across` workgroups a row, timestamped at the pass's start and end in
  /// `stamps` from its index when given
  pub(crate) fn encode(
    &self,
    encoder: &mut wgpu::CommandEncoder,
    group: &wgpu::BindGroup,
    points: u32,
    across: u32,
    stamps: Option<(&wgpu::QuerySet, u32)>,
  ) {
    let (x, y) = grid(points, across);
    if x == 0 {
      return;
    }
    let timestamp_writes = stamps.map(|(query_set, at)| wgpu::ComputePassTimestampWrites {
      query_set,
      beginning_of_pass_write_index: Some(at),
      end_of_pass_write_index: Some(at + 1),
    });
    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
      label: Some("Line Corners"),
      timestamp_writes,
    });
    pass.set_pipeline(&self.pipeline);
    pass.set_bind_group(0, group, &[]);
    pass.dispatch_workgroups(x, y, 1);
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_small_stream_skips_the_corner_pass() {
    // Below the threshold, and with no lines, the vertices work their
    // corners out in place (`Batch2D::encode_corners` dispatches nothing;
    // the vertices' path is `vertices_working_their_corners_out_in_place_
    // equal_the_corner_pass_s`)
    assert!(!builds_corners(0));
    assert!(!builds_corners(1));
    assert!(!builds_corners(6_000));
    assert!(builds_corners(60_000));
    assert!(builds_corners(CORNER_PASS_FROM));
    assert!(!builds_corners(CORNER_PASS_FROM - 1));
  }

  #[test]
  fn the_corner_pass_goes_on_in_rows_past_its_width() {
    // A thread a point, 64 a workgroup, rows of at most `across`
    assert_eq!(grid(0, MAX_ACROSS), (0, 1));
    assert_eq!(grid(1, MAX_ACROSS), (1, 1));
    assert_eq!(grid(64, MAX_ACROSS), (1, 1));
    assert_eq!(grid(65, MAX_ACROSS), (2, 1));
    let full = 64 * MAX_ACROSS;
    assert_eq!(grid(full, MAX_ACROSS), (MAX_ACROSS, 1));
    assert_eq!(grid(full + 1, MAX_ACROSS), (MAX_ACROSS, 2));
    assert_eq!(grid(3 * full, MAX_ACROSS), (MAX_ACROSS, 3));
    assert_eq!(grid(1000, 3), (3, 6));
    // The GPU's rows are read back by the reference test with a narrow grid
    // (`the_gpu_builds_the_reference_s_vertices`)
  }
}
