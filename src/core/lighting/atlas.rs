//! The shadow atlas: a depth texture of four rows per shadowed light, one
//! per 90° view around it (right, down, left, up), each texel the nearest
//! occluder's distance over the light's radius at that angle. Drawn from
//! the plan's edges before any camera pass, in a frame whose edges differ
//! from those it holds; otherwise kept as it is.

use super::ShadowQuality;
use super::plan::ShadowEdge;

/// The atlas's height grows in steps of this many rows (16 lights)
const ROW_STEP: u32 = 64;

/// Edges the instance buffer holds at first; it doubles when a frame needs
/// more
const FIRST_EDGES: usize = 1024;

/// The atlas's size as the shadow map shader reads it
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct AtlasSize {
  /// Width and height in texels, then unused
  size: [f32; 4],
}

/// The soft filter's settings as the light shader reads them
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct FilterUniform {
  /// The filter's look-ups (the blocker search takes half), then unused
  taps: [u32; 4],
}

/// The frame's shadow maps and what draws them.
pub(crate) struct ShadowAtlas {
  /// Kept to grow the edge buffer while encoding
  device: wgpu::Device,
  texture: wgpu::Texture,
  view: wgpu::TextureView,
  width: u32,
  height: u32,
  /// The quality the soft filter's look-ups follow
  quality: ShadowQuality,
  /// The filter uniform needs writing (the quality changed)
  filter_stale: bool,
  filter_uniform: wgpu::Buffer,
  /// The size uniform needs writing (the texture was recreated)
  size_stale: bool,
  size_uniform: wgpu::Buffer,
  bind_group: wgpu::BindGroup,
  pipeline: wgpu::RenderPipeline,
  edges: wgpu::Buffer,
  /// In edges
  capacity: usize,
  /// The most edges a buffer the device allows holds
  max_edges: usize,
  compare_sampler: wgpu::Sampler,
  /// The atlas, its comparison sampler and the filter uniform as the light
  /// shader binds them (`batch::shadow_layout`), made again with the
  /// texture
  light_layout: wgpu::BindGroupLayout,
  light_group: wgpu::BindGroup,
  /// GPU resources made after the first ones (a texture and its bind group
  /// count once, as does a new edge buffer); the headless engine reads it,
  /// which the web has not
  #[cfg(not(target_arch = "wasm32"))]
  builds: u64,
  /// The edges last drawn, a copy kept to compare the next frame's with
  /// (what the texture holds while `holds`)
  held: Vec<ShadowEdge>,
  /// The texture holds `held` (false until drawn, and once recreated)
  holds: bool,
  /// Times it was drawn (tests)
  #[cfg(test)]
  encodes: u64,
}

impl ShadowAtlas {
  pub(crate) fn new(device: &wgpu::Device) -> Self {
    let width = ShadowQuality::default().texels();
    let height = ROW_STEP;
    let texture = atlas_texture(device, width, height);
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let size_uniform = device.create_buffer(&wgpu::BufferDescriptor {
      label: Some("Shadow Atlas Size"),
      size: std::mem::size_of::<AtlasSize>() as u64,
      usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
      mapped_at_creation: false,
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
      label: Some("Shadow Map Bind Group Layout"),
      entries: &[wgpu::BindGroupLayoutEntry {
        binding: 0,
        visibility: wgpu::ShaderStages::VERTEX,
        ty: wgpu::BindingType::Buffer {
          ty: wgpu::BufferBindingType::Uniform,
          has_dynamic_offset: false,
          min_binding_size: None,
        },
        count: None,
      }],
    });
    let bind_group = crate::core::graphics::bind_groups::create(
      device,
      &wgpu::BindGroupDescriptor {
        label: Some("Shadow Map Bind Group"),
        layout: &layout,
        entries: &[wgpu::BindGroupEntry {
          binding: 0,
          resource: size_uniform.as_entire_binding(),
        }],
      },
    );
    let compare_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
      label: Some("Shadow Atlas Compare Sampler"),
      address_mode_u: wgpu::AddressMode::ClampToEdge,
      address_mode_v: wgpu::AddressMode::ClampToEdge,
      address_mode_w: wgpu::AddressMode::ClampToEdge,
      mag_filter: wgpu::FilterMode::Linear,
      min_filter: wgpu::FilterMode::Linear,
      mipmap_filter: wgpu::MipmapFilterMode::Nearest,
      // Lit where the pixel's distance is at most the stored one
      compare: Some(wgpu::CompareFunction::LessEqual),
      ..Default::default()
    });
    let filter_uniform = device.create_buffer(&wgpu::BufferDescriptor {
      label: Some("Shadow Filter"),
      size: std::mem::size_of::<FilterUniform>() as u64,
      usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
      mapped_at_creation: false,
    });
    let light_layout = crate::core::render::batch::shadow_layout(device);
    let light_group = light_group(
      device,
      &light_layout,
      &view,
      &compare_sampler,
      &filter_uniform,
    );
    Self {
      device: device.clone(),
      texture,
      view,
      width,
      height,
      quality: ShadowQuality::default(),
      filter_stale: true,
      filter_uniform,
      size_stale: true,
      size_uniform,
      pipeline: pipeline(device, &layout),
      bind_group,
      edges: edge_buffer(device, FIRST_EDGES),
      capacity: FIRST_EDGES,
      max_edges: max_edges(device),
      compare_sampler,
      light_layout,
      light_group,
      #[cfg(not(target_arch = "wasm32"))]
      builds: 0,
      held: Vec::new(),
      holds: false,
      #[cfg(test)]
      encodes: 0,
    }
  }

  /// Makes the atlas `quality`'s width and at least `rows` rows tall
  /// (rounded up to a multiple of 64, within the device's largest texture),
  /// recreating it when its width changes or it needs more rows, and gives
  /// the soft filter `quality`'s look-ups. It never shrinks for fewer rows,
  /// so a light count going up and down makes no texture each time.
  pub(crate) fn ensure(&mut self, device: &wgpu::Device, quality: ShadowQuality, rows: u32) {
    if quality != self.quality {
      self.quality = quality;
      self.filter_stale = true;
    }
    let largest = device.limits().max_texture_dimension_2d;
    let width = quality.texels().min(largest);
    let needed = rows
      .div_ceil(ROW_STEP)
      .max(1)
      .saturating_mul(ROW_STEP)
      .min(largest);
    let height = if width == self.width {
      needed.max(self.height)
    } else {
      needed
    };
    if (width, height) == (self.width, self.height) {
      return;
    }
    self.built();
    // A new texture holds nothing yet: the next encode draws it
    self.holds = false;
    self.texture = atlas_texture(device, width, height);
    self.view = self
      .texture
      .create_view(&wgpu::TextureViewDescriptor::default());
    (self.width, self.height) = (width, height);
    self.size_stale = true;
    self.light_group = light_group(
      device,
      &self.light_layout,
      &self.view,
      &self.compare_sampler,
      &self.filter_uniform,
    );
  }

  /// Clears the atlas to 1 (nothing nearer than the radius) and draws
  /// `edges` into it, the nearest kept, in `encoder`, before any pass that
  /// reads it; nothing when the texture already holds exactly `edges` (a
  /// frame whose plan did not change), as the edges and the texture's size
  /// are all a draw reads.
  pub(crate) fn encode(
    &mut self,
    encoder: &mut wgpu::CommandEncoder,
    queue: &wgpu::Queue,
    edges: &[ShadowEdge],
  ) {
    if self.filter_stale {
      let filter = FilterUniform {
        taps: [self.quality.taps(), 0, 0, 0],
      };
      queue.write_buffer(&self.filter_uniform, 0, bytemuck::bytes_of(&filter));
      self.filter_stale = false;
    }
    // The plan keeps no more edges than the largest buffer holds
    let edges = &edges[..edges.len().min(self.max_edges)];
    // Compared as bytes: exact, and an edge differing only in a zero's
    // sign is drawn again, harmlessly
    let same: &[u8] = bytemuck::cast_slice(edges);
    if self.holds && same == bytemuck::cast_slice::<ShadowEdge, u8>(&self.held) {
      return;
    }
    self.held.clear();
    self.held.extend_from_slice(edges);
    self.holds = true;
    #[cfg(test)]
    {
      self.encodes += 1;
    }
    if self.size_stale {
      let size = AtlasSize {
        size: [self.width as f32, self.height as f32, 0.0, 0.0],
      };
      queue.write_buffer(&self.size_uniform, 0, bytemuck::bytes_of(&size));
      self.size_stale = false;
    }
    if edges.len() > self.capacity {
      self.built();
      self.capacity = edges.len().next_power_of_two().min(self.max_edges);
      self.edges = edge_buffer(&self.device, self.capacity);
    }
    if !edges.is_empty() {
      queue.write_buffer(&self.edges, 0, bytemuck::cast_slice(edges));
    }
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
      label: Some("Shadow Atlas Pass"),
      color_attachments: &[],
      depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
        view: &self.view,
        depth_ops: Some(wgpu::Operations {
          load: wgpu::LoadOp::Clear(1.0),
          store: wgpu::StoreOp::Store,
        }),
        stencil_ops: None,
      }),
      occlusion_query_set: None,
      multiview_mask: None,
      timestamp_writes: None,
    });
    let count = u32::try_from(edges.len()).unwrap_or(u32::MAX);
    if count == 0 {
      return;
    }
    pass.set_pipeline(&self.pipeline);
    pass.set_bind_group(0, &self.bind_group, &[]);
    let bytes = u64::from(count) * std::mem::size_of::<ShadowEdge>() as u64;
    pass.set_vertex_buffer(0, self.edges.slice(..bytes));
    pass.draw(0..6, 0..count);
  }

  /// The atlas and its comparison sampler (1 where the distance compared
  /// is at most the stored one, filtered along a row), bound for the light
  /// shader (group 1)
  pub(crate) fn light_group(&self) -> &wgpu::BindGroup {
    &self.light_group
  }

  /// The most edges one frame's draw holds: the device's largest buffer
  pub(crate) fn max_edges(&self) -> usize {
    self.max_edges
  }

  /// Counts a GPU resource made after the first ones
  fn built(&mut self) {
    #[cfg(not(target_arch = "wasm32"))]
    {
      self.builds += 1;
    }
  }

  /// GPU resources made since it was created: a texture (with its bind
  /// group) or an edge buffer, each once
  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn builds(&self) -> u64 {
    self.builds
  }

  /// Texels across a view (tests; the light shader reads the texture's
  /// size)
  #[cfg(test)]
  pub(crate) fn width(&self) -> u32 {
    self.width
  }

  /// Times the atlas was drawn, leaving out the frames it was kept (tests)
  #[cfg(test)]
  pub(crate) fn encodes(&self) -> u64 {
    self.encodes
  }

  /// The stored values of `rows`, row after row, `width` each (tests). A
  /// depth texture is copied whole
  #[cfg(test)]
  pub(crate) fn read_rows(
    &self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    rows: std::ops::Range<u32>,
  ) -> Vec<f32> {
    let row_bytes = self.width * 4;
    let padded =
      row_bytes.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
      label: Some("Shadow Atlas Readback"),
      size: u64::from(padded) * u64::from(self.height),
      usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
      mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.copy_texture_to_buffer(
      wgpu::TexelCopyTextureInfo {
        texture: &self.texture,
        mip_level: 0,
        origin: wgpu::Origin3d::ZERO,
        aspect: wgpu::TextureAspect::DepthOnly,
      },
      wgpu::TexelCopyBufferInfo {
        buffer: &staging,
        layout: wgpu::TexelCopyBufferLayout {
          offset: 0,
          bytes_per_row: Some(padded),
          rows_per_image: Some(self.height),
        },
      },
      wgpu::Extent3d {
        width: self.width,
        height: self.height,
        depth_or_array_layers: 1,
      },
    );
    queue.submit([encoder.finish()]);
    let slice = staging.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    if let Err(error) = device.poll(wgpu::PollType::wait_indefinitely()) {
      panic!("the readback finishes: {error}");
    }
    let mapped = match slice.get_mapped_range() {
      Ok(mapped) => mapped,
      Err(error) => panic!("the readback maps: {error}"),
    };
    let mut values = Vec::new();
    for row in rows {
      let start = row as usize * padded as usize;
      let bytes = &mapped[start..start + row_bytes as usize];
      values.extend(
        bytes
          .as_chunks::<4>()
          .0
          .iter()
          .map(|b| f32::from_le_bytes(*b)),
      );
    }
    values
  }
}

/// A `Depth32Float` atlas of `width` x `height` texels
fn atlas_texture(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
  let usage = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
  // Tests read it back
  #[cfg(test)]
  let usage = usage | wgpu::TextureUsages::COPY_SRC;
  device.create_texture(&wgpu::TextureDescriptor {
    label: Some("Shadow Atlas"),
    size: wgpu::Extent3d {
      width,
      height,
      depth_or_array_layers: 1,
    },
    mip_level_count: 1,
    sample_count: 1,
    dimension: wgpu::TextureDimension::D2,
    format: wgpu::TextureFormat::Depth32Float,
    usage,
    view_formats: &[],
  })
}

/// The light shader's bind group of `view`, `sampler` and `filter`
fn light_group(
  device: &wgpu::Device,
  layout: &wgpu::BindGroupLayout,
  view: &wgpu::TextureView,
  sampler: &wgpu::Sampler,
  filter: &wgpu::Buffer,
) -> wgpu::BindGroup {
  crate::core::graphics::bind_groups::create(
    device,
    &wgpu::BindGroupDescriptor {
      label: Some("Shadow Atlas Light Bind Group"),
      layout,
      entries: &[
        wgpu::BindGroupEntry {
          binding: 0,
          resource: wgpu::BindingResource::TextureView(view),
        },
        wgpu::BindGroupEntry {
          binding: 1,
          resource: wgpu::BindingResource::Sampler(sampler),
        },
        wgpu::BindGroupEntry {
          binding: 2,
          resource: filter.as_entire_binding(),
        },
      ],
    },
  )
}

/// The most edges a buffer `device` allows holds
fn max_edges(device: &wgpu::Device) -> usize {
  let largest = usize::try_from(device.limits().max_buffer_size).unwrap_or(usize::MAX);
  largest / std::mem::size_of::<ShadowEdge>()
}

/// A buffer of `capacity` edges, read as instances
fn edge_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
  device.create_buffer(&wgpu::BufferDescriptor {
    label: Some("Shadow Edges"),
    size: (capacity * std::mem::size_of::<ShadowEdge>()) as u64,
    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
    mapped_at_creation: false,
  })
}

/// The pipeline drawing edges into the atlas: depth only, the nearest kept
fn pipeline(device: &wgpu::Device, layout: &wgpu::BindGroupLayout) -> wgpu::RenderPipeline {
  let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
    label: Some("Shadow Map Shader"),
    source: wgpu::ShaderSource::Wgsl(crate::core::shaders::compose::SHADOW_MAP.into()),
  });
  let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
    label: Some("Shadow Map Pipeline Layout"),
    bind_group_layouts: &[Some(layout)],
    immediate_size: 0,
  });
  const ATTRIBUTES: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![
    0 => Float32x2,
    1 => Float32x2,
    2 => Float32x2,
    3 => Float32,
    4 => Uint32,
  ];
  device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
    label: Some("Shadow Map Pipeline"),
    layout: Some(&pipeline_layout),
    vertex: wgpu::VertexState {
      module: &module,
      entry_point: Some("vs_edge"),
      buffers: &[Some(wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<ShadowEdge>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &ATTRIBUTES,
      })],
      compilation_options: Default::default(),
    },
    fragment: Some(wgpu::FragmentState {
      module: &module,
      entry_point: Some("fs_edge"),
      targets: &[],
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
    depth_stencil: Some(wgpu::DepthStencilState {
      format: wgpu::TextureFormat::Depth32Float,
      depth_write_enabled: Some(true),
      depth_compare: Some(wgpu::CompareFunction::Less),
      stencil: wgpu::StencilState::default(),
      bias: wgpu::DepthBiasState::default(),
    }),
    multisample: wgpu::MultisampleState {
      count: 1,
      mask: !0,
      alpha_to_coverage_enabled: false,
    },
    multiview_mask: None,
    cache: None,
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  /// The shared GPU, or `None` (the test skips) without one
  fn gpu() -> Option<(std::sync::Arc<wgpu::Device>, std::sync::Arc<wgpu::Queue>)> {
    let gpu = crate::core::testing::shared_gpu().ok();
    if gpu.is_none() {
      eprintln!("skipping: no GPU");
    }
    gpu
  }

  /// An edge of a light at the origin of radius 100, in `row`
  fn edge(a: [f32; 2], b: [f32; 2], row: u32) -> ShadowEdge {
    ShadowEdge {
      a,
      b,
      light: [0.0, 0.0],
      radius: 100.0,
      row,
    }
  }

  /// An atlas of `Low` quality (256 texels a view) for `rows`, with `edges`
  /// drawn, and its rows read back
  fn draw(rows: u32, edges: &[ShadowEdge]) -> Option<(ShadowAtlas, Vec<f32>)> {
    let (device, queue) = gpu()?;
    let mut atlas = ShadowAtlas::new(&device);
    atlas.ensure(&device, ShadowQuality::Low, rows);
    assert_eq!(atlas.width(), 256);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    atlas.encode(&mut encoder, &queue, edges);
    queue.submit([encoder.finish()]);
    let values = atlas.read_rows(&device, &queue, 0..rows);
    Some((atlas, values))
  }

  fn row(values: &[f32], row: u32) -> &[f32] {
    &values[row as usize * 256..(row as usize + 1) * 256]
  }

  #[test]
  fn an_edge_writes_its_distance_in_its_view_s_row() {
    let Some((atlas, values)) = draw(4, &[edge([50.0, -10.0], [50.0, 10.0], 0)]) else {
      return;
    };
    assert_eq!(atlas.encodes(), 1);
    let right = row(&values, 0);
    // The middle texels see the edge at 50 px, half the radius
    for &value in &right[126..130] {
      assert!((value - 0.5).abs() < 0.005, "{value}");
    }
    // Its ends lie at tangents of ±0.2: outside them nothing is near
    assert_eq!(right[0], 1.0);
    assert_eq!(right[255], 1.0);
    assert_eq!(right[90], 1.0);
    assert_eq!(right[165], 1.0);
    // Within them each texel holds the edge's distance at its angle
    for (i, &value) in right.iter().enumerate().take(150).skip(105) {
      let tangent = (i as f32 + 0.5) / 128.0 - 1.0;
      let expected = 50.0 * (1.0 + tangent * tangent).sqrt() / 100.0;
      assert!((value - expected).abs() < 0.005, "texel {i}: {value}");
    }
    // The other views see nothing
    for view in 1..4 {
      assert!(row(&values, view).iter().all(|&v| v == 1.0), "view {view}");
    }
  }

  #[test]
  fn an_oblique_edge_writes_its_true_distance_at_each_texel() {
    // From (30, -30) at -45° to (80, 30) at 20.6°: its distance changes
    // along the row as no linear interpolation of it would
    let (a, b) = (glam::Vec2::new(30.0, -30.0), glam::Vec2::new(80.0, 30.0));
    let Some((_, values)) = draw(4, &[edge(a.into(), b.into(), 0)]) else {
      return;
    };
    let right = row(&values, 0);
    let mut checked = 0;
    for (i, &value) in right.iter().enumerate() {
      let tangent = (i as f32 + 0.5) / 128.0 - 1.0;
      // Where the ray at this texel's angle meets the edge's line
      let ray = glam::Vec2::new(1.0, tangent);
      let along = (b - a).perp_dot(a) / (b - a).perp_dot(ray);
      let hit = ray * along;
      let t = (hit - a).dot(b - a) / (b - a).length_squared();
      // Clear of the ends, where padding reaches past them
      if !(0.01..=0.99).contains(&t) {
        continue;
      }
      let expected = hit.length() / 100.0;
      assert!(
        (value - expected).abs() < 0.005,
        "texel {i}: {value} against {expected}"
      );
      checked += 1;
    }
    assert!(checked > 150, "{checked} texels");
  }

  #[test]
  fn the_nearest_edge_wins() {
    let edges = [
      edge([60.0, -10.0], [60.0, 10.0], 0),
      edge([30.0, -10.0], [30.0, 10.0], 0),
      edge([60.0, -10.0], [60.0, 10.0], 0),
    ];
    let Some((_, values)) = draw(4, &edges) else {
      return;
    };
    let middle = row(&values, 0)[128];
    assert!((middle - 0.3).abs() < 0.005, "{middle}");
  }

  #[test]
  fn an_edge_across_two_views_writes_both_rows() {
    // From -56° to 56°: all of the right view and the ends of the up and
    // down views next to it, as the plan draws it
    let (a, b) = ([40.0, -60.0], [40.0, 60.0]);
    let edges = [edge(a, b, 3), edge(a, b, 0), edge(a, b, 1)];
    let Some((_, values)) = draw(4, &edges) else {
      return;
    };
    let right = row(&values, 0);
    assert!(
      right.iter().all(|&v| v < 1.0),
      "every texel of the right view"
    );
    // Its ends meet the edge at 45°, 56.6 px away
    let seam = 40.0 * std::f32::consts::SQRT_2 / 100.0;
    for value in [right[0], right[255]] {
      assert!((value - seam).abs() < 0.01, "{value}");
    }
    // Down's start (its -45°, the right view's 45°) and up's end
    let (down, up) = (row(&values, 1), row(&values, 3));
    assert!((down[0] - seam).abs() < 0.01, "{}", down[0]);
    assert!((up[255] - seam).abs() < 0.01, "{}", up[255]);
    // The edge reaches 56° in each: tangents from ±1 to ±2/3
    assert!(
      down[40] < 1.0 && down[45] == 1.0,
      "{} {}",
      down[40],
      down[45]
    );
    assert!(up[215] < 1.0 && up[210] == 1.0, "{} {}", up[215], up[210]);
    assert!(row(&values, 2).iter().all(|&v| v == 1.0), "left");
  }

  #[test]
  fn an_edge_through_the_light_writes_no_nan() {
    let mut edges = Vec::new();
    for view in 0..4 {
      edges.push(edge([-50.0, -50.0], [50.0, 50.0], view));
      edges.push(edge([-50.0, 0.0], [50.0, 0.0], view));
      edges.push(edge([0.0, -50.0], [0.0, 50.0], view));
      // Ending at the light
      edges.push(edge([0.0, 0.0], [30.0, 20.0], view));
      edges.push(edge([0.0, 0.0], [0.0, 0.0], view));
    }
    let Some((_, values)) = draw(4, &edges) else {
      return;
    };
    for (i, &value) in values.iter().enumerate() {
      assert!(
        value.is_finite() && (0.0..=1.0).contains(&value),
        "texel {i}: {value}"
      );
    }
    // Each lies along a ray from the light, so it covers no angle; an edge
    // passing 1 px from the light covers the whole view, and is near
    let mut near = edges.clone();
    near.push(edge([1.0, -5.0], [1.0, 5.0], 0));
    let Some((_, values)) = draw(4, &near) else {
      return;
    };
    let right = row(&values, 0);
    assert!(
      right.iter().all(|&v| v.is_finite() && v < 0.06),
      "{right:?}"
    );
    assert!((right[128] - 0.01).abs() < 0.002, "{}", right[128]);
  }

  #[test]
  fn each_light_writes_only_its_own_rows() {
    // Slot 0's edge in its right view; slot 1's in its left view (row 6)
    let mut theirs = edge([-80.0, 10.0], [-80.0, -10.0], 6);
    theirs.light = [0.0, 0.0];
    let edges = [edge([50.0, -10.0], [50.0, 10.0], 0), theirs];
    let Some((_, values)) = draw(8, &edges) else {
      return;
    };
    for r in [1, 2, 3, 4, 5, 7] {
      assert!(row(&values, r).iter().all(|&v| v == 1.0), "row {r}");
    }
    assert!((row(&values, 0)[128] - 0.5).abs() < 0.005);
    assert!((row(&values, 6)[128] - 0.8).abs() < 0.005);
    // Nothing of one row spills into its neighbours: the texels next to
    // row 6's written ones stay clear
    assert!(row(&values, 5)[128] == 1.0 && row(&values, 7)[128] == 1.0);
  }

  #[test]
  fn the_atlas_grows_in_steps_of_64_rows_and_never_shrinks() {
    let Some((device, _)) = gpu() else {
      return;
    };
    let mut atlas = ShadowAtlas::new(&device);
    atlas.ensure(&device, ShadowQuality::High, 4);
    assert_eq!((atlas.width(), atlas.height), (1024, 64));
    atlas.ensure(&device, ShadowQuality::High, 65);
    assert_eq!(atlas.height, 128);
    atlas.ensure(&device, ShadowQuality::High, 8);
    assert_eq!(atlas.height, 128, "kept for fewer rows");
    atlas.ensure(&device, ShadowQuality::Ultra, 8);
    assert_eq!((atlas.width(), atlas.height), (2048, 64));
  }
}
