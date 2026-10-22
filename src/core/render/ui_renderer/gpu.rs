//! The UI on the GPU: its one pipeline, the texture bind groups it draws
//! with (cached by the textures they hold), its instance buffer and its
//! scroll slots' uniform, which the instance buffer holds past the
//! instances

use std::ops::Range;

use bytemuck::{Pod, Zeroable};

use super::instance::UiInstance;
use crate::core::hash::FastMap;
use crate::core::render::UI_FORMAT;
use crate::core::render::batch::{QUAD_LAYOUT, unit_quad};

/// A bind group unused for this many frames whose UI was drawn is dropped,
/// so the textures it holds are released (a frame keeping the UI image keeps
/// only the groups it was drawn with, and a frame without UI none)
pub(crate) const KEEP_FRAMES: u64 = 8;

/// Instances the buffer holds at first; it doubles when a frame needs more
const FIRST_CAPACITY: usize = 1024;

/// Instances compared at once while looking for a change: one `memcmp`
/// over many is far cheaper than one per instance
const COMPARE_CHUNK: usize = 32;

/// Unchanged instances between two changed stretches below which they are
/// written as one: copying them costs less than another write (wgpu makes
/// a staging buffer for each, about 2 µs)
const MERGE_GAP: usize = 128;

/// Most writes an upload makes: changed stretches past them go in one
const MAX_WRITES: usize = 4;

/// The scroll slots the shader holds in a frame (slot 0 is no scroll area):
/// what an instance's slot bits can name. Past them a scroll area's
/// contents are built absolute.
pub(crate) const SLOTS: usize = 256;

/// One scroll slot as the shader reads it (`ui.wgsl`'s `Slot`, 16-byte
/// aligned for a uniform array)
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub(crate) struct SlotUniform {
  /// The offset in UI units (render pixels / px_scale), as the CPU path
  /// adds it: added to an instance's corners
  pub(crate) offset: [f32; 2],
  /// The offset in whole render pixels: moves an instance's own clip
  pub(crate) pixels: [i32; 2],
  /// The clip in UI image pixels: x0, y0, x1, y1 (as `ClipRect`)
  pub(crate) clip: [u32; 4],
}

const SLOT_SIZE: usize = std::mem::size_of::<SlotUniform>();

/// What the shader's slots take in the instance buffer
const SLOTS_SIZE: u64 = (SLOTS * SLOT_SIZE) as u64;

/// Where the slots may start in the instance buffer: WebGPU's largest
/// minimum uniform buffer offset alignment
const SLOTS_ALIGN: u64 = 256;

/// Slot 0 (no scroll area) as the shader reads it: a frame without slots
/// still writes it, as the shader moves every instance by its slot's offset
const NO_SLOT: SlotUniform = SlotUniform {
  offset: [0.0; 2],
  pixels: [0; 2],
  clip: [0; 4],
};

const INSTANCE_SIZE: usize = std::mem::size_of::<UiInstance>();

/// Where a frame of `instances` instances keeps its scroll slots in the
/// instance buffer: past them, aligned for a uniform
fn slots_at(instances: usize) -> u64 {
  ((instances * INSTANCE_SIZE) as u64).next_multiple_of(SLOTS_ALIGN)
}

/// A [`UiInstance`]: its eight lanes, then its clip, kind and flags
const INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 9] = wgpu::vertex_attr_array![
  2 => Float32x4, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4,
  6 => Float32x4, 7 => Float32x4, 8 => Float32x4, 9 => Float32x4,
  10 => Uint32x4,
];

const INSTANCE_LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
  array_stride: INSTANCE_SIZE as u64,
  step_mode: wgpu::VertexStepMode::Instance,
  attributes: &INSTANCE_ATTRIBUTES,
};

/// The UI's pipeline, texture bind groups, instance buffer and scroll slots
pub(crate) struct UiGpu {
  /// Group 1: the mask, colour, MSDF and sprite slots, then the linear and
  /// nearest samplers
  textures: wgpu::BindGroupLayout,
  /// Group 2: the frame's scroll slots, [`SLOTS`] of them, read from the
  /// instance buffer past the frame's instances (`slots_at`, the group's
  /// offset)
  slots_layout: wgpu::BindGroupLayout,
  slots_group: wgpu::BindGroup,
  /// The last slots written, and where in the instance buffer
  held_slots: Vec<SlotUniform>,
  slots_at: Option<u64>,
  pipeline: wgpu::RenderPipeline,
  /// The unit quad every instance is drawn from (a line triangle reads only
  /// the vertex index)
  quad: wgpu::Buffer,
  /// What a slot no instance of a run samples holds
  placeholder: wgpu::Texture,
  instances: wgpu::Buffer,
  /// In instances
  capacity: usize,
  /// What the instance buffer holds, from its start: last upload's frame
  held: Vec<UiInstance>,
  /// The ranges an upload writes (kept for its allocation)
  ranges: Vec<Range<usize>>,
  groups: BindCache,
  /// The ranges the last upload wrote
  #[cfg(test)]
  written: Vec<Range<usize>>,
  /// The instances the last upload compared with what the buffer held
  #[cfg(test)]
  compared: usize,
  /// The last upload wrote the scroll slots, and in a write of their own
  #[cfg(test)]
  slots_written: (bool, bool),
}

/// What a texture bind group holds: the four slots' textures, then the
/// linear and nearest samplers
type BindKey = ([wgpu::Texture; 4], [wgpu::Sampler; 2]);

/// The UI's texture bind groups, by what they hold (wgpu handles compare by
/// identity, so a recreated texture is a new key), with the frame each was
/// last used; an entry not used for [`KEEP_FRAMES`] frames is dropped, as it
/// holds its textures alive
#[derive(Default)]
struct BindCache {
  groups: FastMap<BindKey, (wgpu::BindGroup, u64)>,
}

impl UiGpu {
  /// The UI pipeline drawing into the UI image (`UI_FORMAT`, gamma-encoded
  /// output), through cameras bound with `camera_layout` (group 0)
  pub(crate) fn new(device: &wgpu::Device, camera_layout: &wgpu::BindGroupLayout) -> UiGpu {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some("UI Shader"),
      source: wgpu::ShaderSource::Wgsl(crate::core::shaders::compose::UI.into()),
    });
    let texture = |binding| wgpu::BindGroupLayoutEntry {
      binding,
      visibility: wgpu::ShaderStages::FRAGMENT,
      ty: wgpu::BindingType::Texture {
        multisampled: false,
        view_dimension: wgpu::TextureViewDimension::D2,
        sample_type: wgpu::TextureSampleType::Float { filterable: true },
      },
      count: None,
    };
    let sampler = |binding| wgpu::BindGroupLayoutEntry {
      binding,
      visibility: wgpu::ShaderStages::FRAGMENT,
      ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
      count: None,
    };
    let textures = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
      label: Some("UI Texture Layout"),
      entries: &[
        texture(0),
        texture(1),
        texture(2),
        texture(3),
        sampler(4),
        sampler(5),
      ],
    });
    // The slots are read by the vertex stage only: it moves an instance by
    // its slot's offset and hands the clip on
    let slots_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
      label: Some("UI Slot Layout"),
      entries: &[wgpu::BindGroupLayoutEntry {
        binding: 0,
        visibility: wgpu::ShaderStages::VERTEX,
        ty: wgpu::BindingType::Buffer {
          ty: wgpu::BufferBindingType::Uniform,
          has_dynamic_offset: true,
          min_binding_size: wgpu::BufferSize::new(SLOTS_SIZE),
        },
        count: None,
      }],
    });
    let instances = instance_buffer(device, FIRST_CAPACITY);
    let slots_group = slots_group(device, &slots_layout, &instances);
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("UI Pipeline Layout"),
      bind_group_layouts: &[Some(camera_layout), Some(&textures), Some(&slots_layout)],
      immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some("UI Pipeline"),
      layout: Some(&layout),
      vertex: wgpu::VertexState {
        module: &module,
        entry_point: Some("vs_ui"),
        buffers: &[Some(QUAD_LAYOUT), Some(INSTANCE_LAYOUT)],
        compilation_options: Default::default(),
      },
      fragment: Some(wgpu::FragmentState {
        module: &module,
        entry_point: Some("fs_ui"),
        targets: &[Some(wgpu::ColorTargetState {
          format: UI_FORMAT,
          blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
          write_mask: wgpu::ColorWrites::ALL,
        })],
        // The UI image holds gamma-encoded colour: encode on output
        compilation_options: wgpu::PipelineCompilationOptions {
          constants: &[("UI_GAMMA", 1.0)],
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
    });
    // The batch's quad, split along the same diagonal: the same pixels
    let quad = unit_quad(device, "UI Quad");
    let placeholder = device.create_texture(&wgpu::TextureDescriptor {
      label: Some("UI Placeholder Texture"),
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
    UiGpu {
      textures,
      slots_layout,
      slots_group,
      held_slots: Vec::new(),
      slots_at: None,
      pipeline,
      quad,
      placeholder,
      instances,
      capacity: FIRST_CAPACITY,
      held: Vec::new(),
      ranges: Vec::new(),
      groups: BindCache::default(),
      #[cfg(test)]
      written: Vec::new(),
      #[cfg(test)]
      compared: 0,
      #[cfg(test)]
      slots_written: (false, false),
    }
  }

  /// The bind group holding `textures` in the mask, colour, MSDF and sprite
  /// slots (`None`: the placeholder) and `samplers` (linear, nearest), made
  /// on first use and marked used in `frame`
  pub(crate) fn bind_group(
    &mut self,
    device: &wgpu::Device,
    textures: [Option<&wgpu::Texture>; 4],
    (linear, nearest): (&wgpu::Sampler, &wgpu::Sampler),
    frame: u64,
  ) -> &wgpu::BindGroup {
    let textures = textures.map(|texture| texture.unwrap_or(&self.placeholder).clone());
    let key = (textures, [linear.clone(), nearest.clone()]);
    let layout = &self.textures;
    let (group, used) =
      self
        .groups
        .groups
        .entry(key)
        .or_insert_with_key(|(textures, [linear, nearest])| {
          let views = textures
            .each_ref()
            .map(|texture| texture.create_view(&wgpu::TextureViewDescriptor::default()));
          let view = |binding: u32, view| wgpu::BindGroupEntry {
            binding,
            resource: wgpu::BindingResource::TextureView(view),
          };
          let group = crate::core::graphics::bind_groups::create(
            device,
            &wgpu::BindGroupDescriptor {
              label: Some("UI Textures"),
              layout,
              entries: &[
                view(0, &views[0]),
                view(1, &views[1]),
                view(2, &views[2]),
                view(3, &views[3]),
                wgpu::BindGroupEntry {
                  binding: 4,
                  resource: wgpu::BindingResource::Sampler(linear),
                },
                wgpu::BindGroupEntry {
                  binding: 5,
                  resource: wgpu::BindingResource::Sampler(nearest),
                },
              ],
            },
          );
          (group, frame)
        });
    *used = frame;
    group
  }

  /// Drop the bind groups not used for [`KEEP_FRAMES`] frames up to `frame`
  pub(crate) fn end_frame(&mut self, frame: u64) {
    self
      .groups
      .groups
      .retain(|_, (_, used)| frame.saturating_sub(*used) < KEEP_FRAMES);
  }

  /// Keep only the bind groups used in `frame`
  pub(crate) fn keep_used_in(&mut self, frame: u64) {
    self.groups.groups.retain(|_, (_, used)| *used == frame);
  }

  /// Drop every bind group
  pub(crate) fn drop_groups(&mut self) {
    self.groups.groups.clear();
  }

  /// Upload the frame's `instances` and its scroll `slots` (at most
  /// [`SLOTS`]; slot 0 alone when there are none). The instances: the
  /// ranges that differ from what the buffer holds (`changed_ranges`), a
  /// write each (nothing when nothing differs); `same` are ranges of them
  /// known to be what it holds there (in order, apart), which are not
  /// compared. The slots go past the instances (`slots_at`): when they
  /// changed or moved, in the last range's write if it comes within
  /// [`MERGE_GAP`] of the instances' end, else in one of their own. An
  /// instance of the frame names only a slot written for it. A buffer too
  /// small is replaced by one of the next power of two instances, and given
  /// the whole frame. The instances written.
  pub(crate) fn upload(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    instances: &[UiInstance],
    mut same: &[Range<usize>],
    slots: &[SlotUniform],
  ) -> u32 {
    let n = instances.len();
    if n > self.capacity {
      self.capacity = n.next_power_of_two();
      self.instances = instance_buffer(device, self.capacity);
      self.slots_group = slots_group(device, &self.slots_layout, &self.instances);
      self.held.clear();
      self.slots_at = None;
      same = &[];
    }
    let slots = match slots.get(..slots.len().min(SLOTS)) {
      Some(slots) if !slots.is_empty() => slots,
      _ => std::slice::from_ref(&NO_SLOT),
    };
    let mut ranges = std::mem::take(&mut self.ranges);
    let _compared = changed_ranges(&self.held, instances, same, &mut ranges);
    let at = slots_at(n);
    let slot_bytes: &[u8] = bytemuck::cast_slice(slots);
    let move_slots = self.slots_at != Some(at)
      || bytemuck::cast_slice::<SlotUniform, u8>(&self.held_slots) != slot_bytes;
    let joined = move_slots
      && match ranges.last_mut() {
        Some(last) if last.end + MERGE_GAP >= n => {
          last.end = n;
          true
        }
        _ => false,
      };
    let mut written = 0;
    let last = ranges.len().saturating_sub(1);
    for (k, range) in ranges.iter().enumerate() {
      let Some(changed) = instances.get(range.clone()) else {
        continue;
      };
      let offset = (range.start * INSTANCE_SIZE) as u64;
      let bytes: &[u8] = bytemuck::cast_slice(changed);
      if joined && k == last {
        write_joined(queue, &self.instances, offset, bytes, at, slot_bytes);
      } else {
        queue.write_buffer(&self.instances, offset, bytes);
      }
      written += changed.len();
    }
    if move_slots {
      if !joined {
        queue.write_buffer(&self.instances, at, slot_bytes);
      }
      self.held_slots.clear();
      self.held_slots.extend_from_slice(slots);
      self.slots_at = Some(at);
    }
    // Keep `held` equal to `instances`, copying only what changed
    self.held.truncate(instances.len());
    let kept = self.held.len();
    for range in &ranges {
      let end = range.end.min(kept);
      if let (Some(held), Some(changed)) = (
        self.held.get_mut(range.start..end),
        instances.get(range.start..end),
      ) {
        held.copy_from_slice(changed);
      }
    }
    self
      .held
      .extend_from_slice(instances.get(kept..).unwrap_or_default());
    #[cfg(test)]
    {
      self.written.clone_from(&ranges);
      self.compared = _compared;
      self.slots_written = (move_slots, move_slots && !joined);
    }
    self.ranges = ranges;
    u32::try_from(written).unwrap_or(u32::MAX)
  }

  /// The scroll slots' bind group (group 2), bound at `slots_offset`
  pub(crate) fn slots_group(&self) -> &wgpu::BindGroup {
    &self.slots_group
  }

  /// Where the last upload put the scroll slots in the instance buffer:
  /// the slots' group's offset
  pub(crate) fn slots_offset(&self) -> u32 {
    self
      .slots_at
      .and_then(|at| u32::try_from(at).ok())
      .unwrap_or(0)
  }

  /// The ranges the last upload wrote (tests)
  #[cfg(test)]
  pub(crate) fn written(&self) -> &[Range<usize>] {
    &self.written
  }

  /// The instances the last upload compared with what the buffer held
  /// (tests)
  #[cfg(test)]
  pub(crate) fn compared(&self) -> usize {
    self.compared
  }

  /// Whether the last upload wrote the scroll slots, and whether in a
  /// write of their own (tests)
  #[cfg(test)]
  pub(crate) fn slots_written(&self) -> (bool, bool) {
    self.slots_written
  }

  pub(crate) fn pipeline(&self) -> &wgpu::RenderPipeline {
    &self.pipeline
  }

  /// The unit quad (vertex buffer 0)
  pub(crate) fn quad(&self) -> &wgpu::Buffer {
    &self.quad
  }

  /// The uploaded instances (vertex buffer 1)
  pub(crate) fn instances(&self) -> &wgpu::Buffer {
    &self.instances
  }

  /// How many bind groups are cached (tests)
  #[cfg(test)]
  pub(crate) fn cached_groups(&self) -> usize {
    self.groups.groups.len()
  }
}

/// The ranges of `frame` to write over a buffer holding `held`, in order,
/// into `out` (cleared first). When the lengths are equal, each stretch of
/// instances that differ: stretches fewer than [`MERGE_GAP`] alike
/// instances apart are one, and those past [`MAX_WRITES`] join the last
/// (none when nothing differs). When they are not, from the first instance
/// that differs to the frame's end. Instances compare bit for bit, as the
/// buffer holds them; those in `same` (in order, apart) are alike without
/// being compared. The instances compared.
pub(crate) fn changed_ranges(
  held: &[UiInstance],
  frame: &[UiInstance],
  same: &[Range<usize>],
  out: &mut Vec<Range<usize>>,
) -> usize {
  out.clear();
  let n = frame.len();
  let mut alike = Alike {
    held,
    frame,
    same,
    next: 0,
    compared: 0,
  };
  if held.len() != n {
    let first = alike.from(0, held.len().min(n));
    if first < n {
      out.push(first..n);
    }
    return alike.compared;
  }
  let mut at = 0;
  while at < n {
    let start = at + alike.from(at, n);
    if start == n {
      break;
    }
    // On to the first changed instance that [`MERGE_GAP`] alike ones (or
    // the frame's end) follow
    let mut end = start + 1;
    loop {
      let reach = (end + MERGE_GAP).min(n);
      let same = alike.from(end, reach);
      if end + same == reach {
        break;
      }
      end += same + 1;
    }
    if out.len() < MAX_WRITES {
      out.push(start..end);
    } else if let Some(last) = out.last_mut() {
      last.end = end;
    }
    at = end;
  }
  alike.compared
}

/// What the buffer held and the frame, compared from place to place, on
/// from the last place asked
struct Alike<'a> {
  held: &'a [UiInstance],
  frame: &'a [UiInstance],
  /// Ranges known alike, in order
  same: &'a [Range<usize>],
  /// The first of `same` that may end past the last place asked
  next: usize,
  /// Instances compared so far
  compared: usize,
}

impl Alike<'_> {
  /// How many instances are alike from `from` on (no earlier than the last
  /// asked), up to `to`
  fn from(&mut self, from: usize, to: usize) -> usize {
    let mut at = from;
    while at < to {
      while self
        .same
        .get(self.next)
        .is_some_and(|known| known.end <= at)
      {
        self.next += 1;
      }
      let known = self.same.get(self.next);
      if let Some(known) = known.filter(|known| known.start <= at) {
        at = known.end.min(to);
        continue;
      }
      let stop = known.map_or(to, |known| known.start.min(to));
      let (Some(a), Some(b)) = (self.held.get(at..stop), self.frame.get(at..stop)) else {
        break;
      };
      let same = same_prefix(a, b);
      self.compared += (same + 1).min(stop - at);
      at += same;
      if at < stop {
        break;
      }
    }
    at - from
  }
}

fn bytes(instances: &[UiInstance]) -> &[u8] {
  bytemuck::cast_slice(instances)
}

fn alike(a: &UiInstance, b: &UiInstance) -> bool {
  bytemuck::bytes_of(a) == bytemuck::bytes_of(b)
}

/// How many instances `a` and `b` (as long as each other) begin with alike
fn same_prefix(a: &[UiInstance], b: &[UiInstance]) -> usize {
  let mut same = 0;
  for (a, b) in a.chunks(COMPARE_CHUNK).zip(b.chunks(COMPARE_CHUNK)) {
    if bytes(a) != bytes(b) {
      return same + a.iter().zip(b).take_while(|(a, b)| alike(a, b)).count();
    }
    same += a.len();
  }
  same
}

/// Write `instances` (their bytes) at `offset` and `slots` at `at`, past
/// them, in one write; the bytes between are never read
fn write_joined(
  queue: &wgpu::Queue,
  buffer: &wgpu::Buffer,
  offset: u64,
  instances: &[u8],
  at: u64,
  slots: &[u8],
) {
  let gap = usize::try_from(at.saturating_sub(offset)).unwrap_or(usize::MAX);
  let Some(size) = gap
    .checked_add(slots.len())
    .and_then(|size| wgpu::BufferSize::new(size as u64))
  else {
    return;
  };
  if gap < instances.len() {
    return;
  }
  let Some(mut view) = queue.write_buffer_with(buffer, offset, size) else {
    return;
  };
  view.slice(..instances.len()).copy_from_slice(instances);
  view.slice(gap..).copy_from_slice(slots);
}

/// The instance buffer for `capacity` instances, and the scroll slots past
/// them (`slots_at`)
fn instance_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
  use wgpu::BufferUsages as U;
  // Tests read it back
  let read_back = if cfg!(test) { U::COPY_SRC } else { U::empty() };
  device.create_buffer(&wgpu::BufferDescriptor {
    label: Some("UI Instances"),
    size: slots_at(capacity) + SLOTS_SIZE,
    usage: U::VERTEX | U::UNIFORM | U::COPY_DST | read_back,
    mapped_at_creation: false,
  })
}

/// The scroll slots' bind group over `instances`: the uniform's offset is
/// given when it is bound
fn slots_group(
  device: &wgpu::Device,
  layout: &wgpu::BindGroupLayout,
  instances: &wgpu::Buffer,
) -> wgpu::BindGroup {
  crate::core::graphics::bind_groups::create(
    device,
    &wgpu::BindGroupDescriptor {
      label: Some("UI Slots"),
      layout,
      entries: &[wgpu::BindGroupEntry {
        binding: 0,
        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
          buffer: instances,
          offset: 0,
          size: wgpu::BufferSize::new(SLOTS_SIZE),
        }),
      }],
    },
  )
}
