//! The batch on the GPU: the frame's quad instances and triangles, each
//! camera's draws over them, and the pipelines and camera bind groups they
//! draw with.

use std::collections::HashMap;
use std::sync::Arc;

use wgpu::util::DeviceExt;

use super::chunks::{ChunkBuffers, ChunkStore, SLOT_STRIDE};
use super::pipelines::{self, PipelineKey, Pipelines};
use super::polyline::{Triangles, Vertex2D};
use super::{
  BatchShader, BlendMode, Draw, DrawSource, DrawState, Filter, LightStep, PageKind, Placed,
  Sampling, Stream, plan_draws,
};
use crate::core::ecs::Entity;
use crate::core::graphics::camera_gpu::CameraUniforms;
use crate::core::graphics::{
  AtlasSlot, Presentation, SlotPage, SpriteGpuRenderer, SpriteInstance, TextureId, place_in_slot,
};
use crate::core::render::{CameraDraw, DrawItem, DrawList, DrawPayload, SpriteShading};
use crate::core::shaders::compile::{self, Compiler};
use crate::core::shaders::custom::{CompiledShader, ShaderError, ShaderKind};

/// Instances, vertices and indices the buffers hold at first; each doubles
/// when a frame needs more.
const FIRST_CAPACITY: usize = 1024;

/// What a render pass has bound so far, so each of its draws
/// ([`Batch2D::draw`]) sets only the state that differs from the draw
/// before: a run of tile chunk draws shares its pipeline, camera, page and
/// tile slot, and sets only its instances. One per render pass, each
/// starting with nothing bound
#[derive(Default)]
pub(crate) struct Bound<'a> {
  scissor: Option<(u32, u32)>,
  pipeline: Option<&'a wgpu::RenderPipeline>,
  /// Each bind group set, and its dynamic offset
  groups: [Option<(&'a wgpu::BindGroup, Option<u32>)>; 3],
  /// The stream whose buffers vertex slot 0 (and the index buffer) hold
  stream: Option<Stream>,
}

impl<'a> Bound<'a> {
  /// Sets bind group `index` to `group` at its dynamic offset, unless the
  /// pass has it there already
  fn bind(
    &mut self,
    pass: &mut wgpu::RenderPass<'a>,
    index: usize,
    (group, offset): (&'a wgpu::BindGroup, Option<u32>),
  ) {
    if self.groups[index].is_some_and(|(set, at)| std::ptr::eq(set, group) && at == offset) {
      return;
    }
    let offsets: &[u32] = match &offset {
      Some(offset) => std::slice::from_ref(offset),
      None => &[],
    };
    pass.set_bind_group(index as u32, group, offsets);
    self.groups[index] = Some((group, offset));
  }
}

/// The 2D batch: lays out the world's drawables per camera and draws them.
pub(crate) struct Batch2D {
  format: wgpu::TextureFormat,
  presentation: Presentation,
  pipelines: Pipelines,
  // Group 0: each camera's uniform buffer, bound once
  camera_layout: wgpu::BindGroupLayout,
  camera_groups: HashMap<Entity, wgpu::BindGroup>,
  // The unit quad every instance is drawn from
  quad: wgpu::Buffer,
  instances: wgpu::Buffer,
  capacity: usize,
  // This frame's instances, remapped into their atlas slots (each camera's
  // in a range of their own), and each camera's draws over them; reused
  placed: Vec<SpriteInstance>,
  draws: Vec<CameraDraws>,
  // This frame's trails and lines, in world space for every camera; built
  // while the frame is gathered and uploaded once
  triangles: Triangles,
  // Tile chunks: built while the frame is gathered, written when they
  // change, drawn through the frame's tile slots
  chunks: ChunkStore,
  chunk_buffers: ChunkBuffers,
  vertices: GrowingBuffer,
  indices: GrowingBuffer,
  // Makes the new versions of game sprite shaders already drawn, while the
  // old ones draw
  compiler: Compiler<SpriteCompiled>,
}

/// What the shader thread made of a game sprite shader's new version
struct SpriteCompiled {
  /// Its name, which labels what is made from it
  name: String,
  compiled: Result<Arc<CompiledShader>, ShaderError>,
  /// Its pipelines by key, or the GPU's message rejecting them (none if it
  /// does not compile)
  pipelines: Result<Vec<(PipelineKey, wgpu::RenderPipeline)>, String>,
}

/// A GPU buffer that doubles when a frame needs more.
struct GrowingBuffer {
  buffer: wgpu::Buffer,
  /// In elements
  capacity: usize,
  element: usize,
  usage: wgpu::BufferUsages,
  label: &'static str,
}

impl GrowingBuffer {
  fn new(
    device: &wgpu::Device,
    label: &'static str,
    element: usize,
    usage: wgpu::BufferUsages,
  ) -> Self {
    Self {
      buffer: buffer(device, label, FIRST_CAPACITY * element, usage),
      capacity: FIRST_CAPACITY,
      element,
      usage,
      label,
    }
  }

  /// Upload `bytes`, growing to the next power of two elements if need be.
  fn write(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, bytes: &[u8]) {
    let needed = bytes.len() / self.element;
    if needed > self.capacity {
      self.capacity = needed.next_power_of_two();
      self.buffer = buffer(device, self.label, self.capacity * self.element, self.usage);
    }
    if !bytes.is_empty() {
      queue.write_buffer(&self.buffer, 0, bytes);
    }
  }
}

/// One camera's draws: before lighting, after it, and its lights (into
/// its light buffer).
#[derive(Default)]
struct CameraDraws {
  camera: Option<Entity>,
  lit: Vec<Draw>,
  unlit: Vec<Draw>,
  lights: Vec<Draw>,
}

impl Batch2D {
  /// A batch drawing into images of `format`, resolving filters for
  /// `presentation`.
  pub(crate) fn new(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    presentation: Presentation,
  ) -> Self {
    let camera_layout = pipelines::camera_layout(device);
    let texture_layout = pipelines::texture_layout(device);
    let chunk_buffers = ChunkBuffers::new(device);
    let mut pipelines = Pipelines::new(
      device,
      &camera_layout,
      &texture_layout,
      chunk_buffers.slot_layout(),
    );
    // Tile chunks' pipeline for a plain layer, made now rather than in the
    // frame the first map lands (about 0.85 ms there, measured headless)
    let key = PipelineKey::new(
      BatchShader::Tile,
      BlendMode::default(),
      Stream::Quads,
      PageKind::Srgb8,
      Filter::Default.resolve(presentation),
      format,
    );
    pipelines.ensure(device, key, None);
    Self {
      format,
      presentation,
      pipelines,
      camera_layout,
      camera_groups: HashMap::new(),
      quad: unit_quad(device, "Batch Quad"),
      instances: instance_buffer(device, FIRST_CAPACITY),
      capacity: FIRST_CAPACITY,
      placed: Vec::new(),
      draws: Vec::new(),
      triangles: Triangles::default(),
      // The arena holds no more than the device's largest buffer
      chunks: ChunkStore::for_device(device.limits().max_buffer_size),
      chunk_buffers,
      vertices: GrowingBuffer::new(
        device,
        "Batch Vertices",
        std::mem::size_of::<Vertex2D>(),
        wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
      ),
      indices: GrowingBuffer::new(
        device,
        "Batch Indices",
        std::mem::size_of::<u32>(),
        wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
      ),
      compiler: Compiler::new(),
    }
  }

  /// Game sprite shaders' new versions made off the main thread take over,
  /// before anything draws: the pipelines that came back are taken in, and
  /// each shader waiting for a version moves to it once every pipeline its
  /// old one has is made for it (or the new one failed: it draws its last
  /// good version). Then the next version wanted is sent. Call once a
  /// frame, before the frame's sprites are gathered
  pub(crate) fn sync_shaders(&mut self, device: &wgpu::Device, textures: &mut SpriteGpuRenderer) {
    if !self.compiler.busy() && !textures.custom_shaders_waiting() {
      return;
    }
    if let Some((key, done)) = self.compiler.finished() {
      match done {
        Some(done) => {
          let outcome = done.pipelines.map(|made| {
            // Not for a version saved over meanwhile, or a shader freed
            if textures.custom_shader_in_use(key) {
              for (pipeline_key, pipeline) in made {
                self
                  .pipelines
                  .take_made(pipeline_key, pipeline, &done.name, key);
              }
            }
          });
          // A twin compiled at once, or a blend mode first drawn, may have
          // a check of its own out on the same source
          let checking = self.pipelines.checking(key);
          textures.custom_shader_landed(key, &done.name, done.compiled, outcome, checking);
        }
        None => textures.custom_shader_panicked(key),
      }
    }
    let pipelines = &self.pipelines;
    textures.advance_custom_shaders(|next, _, drawn| {
      pipelines.custom_keys(drawn).all(|key| {
        pipelines
          .get(&PipelineKey {
            shader: BatchShader::Custom(next),
            ..key
          })
          .is_some()
      })
    });
    self.compile_next(device, textures);
  }

  /// Sends the next new version a game sprite shader waits for to the
  /// shader thread, unless one is out: the pipelines the source it draws
  /// meanwhile has that the new one lacks, made from its compiled source
  fn compile_next(&mut self, device: &wgpu::Device, textures: &SpriteGpuRenderer) {
    if self.compiler.busy() || !textures.custom_shaders_waiting() {
      return;
    }
    let pipelines = &self.pipelines;
    let job = textures.wanted_custom_shaders().find_map(|wanted| {
      if wanted.compiled.is_some_and(|source| source.failed()) {
        return None;
      }
      let keys: Vec<PipelineKey> = pipelines
        .custom_keys(wanted.drawn)
        .map(|key| PipelineKey {
          shader: BatchShader::Custom(wanted.key),
          ..key
        })
        .filter(|key| !pipelines.made(key))
        .collect();
      // A source no shader compiled yet comes back even with nothing to
      // make, so its shaders can move to it
      (!keys.is_empty() || wanted.compiled.is_none())
        .then(|| (wanted.key, wanted.shader.clone(), keys))
    });
    let Some((key, shader, keys)) = job else {
      return;
    };
    let (device, layout) = (device.clone(), self.pipelines.custom_layout().clone());
    self.compiler.start(key, move || {
      let compiled = shader.compiled(ShaderKind::Sprite).clone();
      let pipelines = match &compiled {
        Ok(ready) => compile::checked(&device, || {
          let module = pipelines::custom_module(&device, shader.name(), &ready.wgsl);
          keys
            .iter()
            .map(|&key| {
              let made = pipelines::custom_pipeline(&device, &layout, shader.name(), &module, key);
              (key, made)
            })
            .collect()
        }),
        Err(_) => Ok(Vec::new()),
      };
      SpriteCompiled {
        name: shader.name().to_string(),
        compiled,
        pipelines,
      }
    });
  }

  /// The shader thread's gate (tests)
  #[cfg(test)]
  pub(crate) fn compile_gate(&self) -> Arc<compile::CompileGate> {
    self.compiler.gate()
  }

  /// Compiles sent to the shader thread, and whether one is out (tests)
  #[cfg(test)]
  pub(crate) fn compiles(&self) -> (usize, bool) {
    (self.compiler.started(), self.compiler.busy())
  }

  /// The frame's trail and line triangles, for gathering them (clear first).
  pub(crate) fn triangles(&mut self) -> &mut Triangles {
    &mut self.triangles
  }

  /// The tile chunks, for gathering the frame's
  pub(crate) fn chunks_mut(&mut self) -> &mut ChunkStore {
    &mut self.chunks
  }

  /// The tile chunks (tests, and the headless engine's reads)
  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn chunks(&self) -> &ChunkStore {
    &self.chunks
  }

  /// Whether the chunk arena has a GPU buffer (tests)
  #[cfg(test)]
  pub(crate) fn has_chunk_buffer(&self) -> bool {
    self.chunk_buffers.instances().is_some()
  }

  /// How many instances the chunk arena's GPU buffer holds, 0 with none
  /// (tests)
  #[cfg(test)]
  pub(crate) fn chunk_buffer_capacity(&self) -> usize {
    self.chunk_buffers.capacity()
  }

  /// The layout cameras' uniforms are bound with (group 0)
  pub(crate) fn camera_layout(&self) -> &wgpu::BindGroupLayout {
    &self.camera_layout
  }

  /// `camera`'s uniform bind group, once [`sync_cameras`](Self::sync_cameras)
  /// made it
  pub(crate) fn camera_group(&self, camera: Entity) -> Option<&wgpu::BindGroup> {
    self.camera_groups.get(&camera)
  }

  /// Bind every camera's uniform buffer (made by `CameraUniforms::sync`),
  /// dropping the bind groups of cameras that are gone.
  pub(crate) fn sync_cameras(&mut self, device: &wgpu::Device, uniforms: &CameraUniforms) {
    self
      .camera_groups
      .retain(|camera, _| uniforms.buffer(*camera).is_some());
    for camera in uniforms.cameras() {
      if self.camera_groups.contains_key(&camera) {
        continue;
      }
      let Some(buffer) = uniforms.buffer(camera) else {
        continue;
      };
      let group = crate::core::graphics::bind_groups::create(
        device,
        &wgpu::BindGroupDescriptor {
          label: Some("Batch Camera Bind Group"),
          layout: &self.camera_layout,
          entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
          }],
        },
      );
      self.camera_groups.insert(camera, group);
    }
  }

  /// Lay out the sorted `list` for each of `cameras` and upload it once.
  ///
  /// Makes the list's textures resident, and `extra_textures` (the UI
  /// renderer's sprites) with them, and writes the tile chunks built while
  /// the frame was gathered and the frame's tile slots; then, per camera,
  /// remaps the sprites it sees into their atlas slots in draw order (a
  /// run's quads one after another, its slot found once), in a range of the
  /// instance buffer of its own, and merges consecutive drawables with equal
  /// draw states into draws (trails and lines by their index ranges; each
  /// chunk a draw of its own, from the chunk arena).
  /// Call once per frame, after [`sync_cameras`](Self::sync_cameras) and
  /// after the frame's triangles and chunks are built.
  pub(crate) fn prepare(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    list: &DrawList,
    cameras: &[CameraDraw],
    textures: &mut SpriteGpuRenderer,
    extra_textures: &[TextureId],
  ) {
    textures.prepare_textures(device, queue, list, extra_textures);
    // The chunks built while the frame was gathered, and the frame's tile
    // slots with their images' places
    self
      .chunk_buffers
      .upload(device, queue, &mut self.chunks, textures);
    // A game shader whose pipeline the GPU rejected draws its last good
    // version from the next frame
    self
      .pipelines
      .settle(|source, outcome| textures.custom_shader_settled(source, outcome));
    let textures: &SpriteGpuRenderer = textures;
    // A source no game shader uses any more (its shaders freed, or changed)
    // takes its pipelines with it, in the frame the last one went
    self.pipelines.retain(|key| match key.shader {
      BatchShader::Custom(source) => textures.custom_shader_in_use(source),
      _ => true,
    });

    let (format, presentation) = (self.format, self.presentation);
    let Self {
      pipelines,
      placed,
      draws,
      triangles,
      chunks,
      ..
    } = self;
    let chunks: &ChunkStore = chunks;
    placed.clear();
    draws.resize_with(cameras.len(), CameraDraws::default);
    // The previous sprite's texture and where it lives, reused while
    // sprites share it
    let mut last: Option<(TextureId, AtlasSlot, (u32, u32))> = None;
    // The last pipeline asked for and the shader it resolved to, reused
    // while drawables share it (a map lookup per sprite otherwise)
    let mut ensured: Option<(PipelineKey, BatchShader)> = None;
    for (camera, drawn) in cameras.iter().zip(draws.iter_mut()) {
      drawn.camera = Some(camera.entity);
      let first = placed.len() as u32;
      let mut place = |item: &DrawItem| {
        let index = match item.payload {
          DrawPayload::Sprite(index) => index,
          DrawPayload::Run(index) => {
            // An emitter's particles: its texture's slot found once, each
            // quad placed in it, all drawn with one state
            let run = list.run(index)?;
            let quads = list.run_quads(run);
            let count = u32::try_from(quads.len()).ok()?;
            let (slot, page_size) = match last {
              Some((texture, slot, size)) if texture == run.texture => (slot, size),
              _ => {
                let (slot, size) = textures.slot(run.texture)?;
                last = Some((run.texture, slot, size));
                (slot, size)
              }
            };
            let shader = match run.shading {
              SpriteShading::Effect => BatchShader::SpriteEffect,
              _ => BatchShader::Sprite,
            };
            let sampling = run.filter.resolve(presentation);
            let key = PipelineKey::new(
              shader,
              run.blend,
              Stream::Quads,
              PageKind::of(Some(slot.page)),
              sampling,
              format,
            );
            if ensured.is_none_or(|(last, _)| last != key) {
              pipelines.ensure(device, key, None);
              ensured = Some((key, shader));
            }
            // Only built-in effects grow the quad
            let effects = shader == BatchShader::SpriteEffect;
            placed.extend(
              quads
                .iter()
                .map(|quad| place_in_slot(quad, &slot, page_size, effects)),
            );
            return Some(Placed::Quads {
              state: DrawState {
                page: Some(slot.page),
                shader,
                blend: run.blend,
                sampling,
              },
              count,
            });
          }
          DrawPayload::Chunk(index) => {
            // Its instances are in the chunk arena; its slot places them
            // and maps their UVs into the image's place
            let chunk = list.chunk(index)?;
            // All of the key's instances, or a row of them
            let range = chunks.range(chunk.key)?;
            let start = range.start.checked_add(chunk.first)?;
            let end = start.checked_add(chunk.count)?;
            if end > range.end {
              return None;
            }
            let instances = start..end;
            let (slot, _) = textures.slot(chunk.texture)?;
            let sampling = chunk.filter.resolve(presentation);
            let key = PipelineKey::new(
              BatchShader::Tile,
              chunk.blend,
              Stream::Quads,
              PageKind::of(Some(slot.page)),
              sampling,
              format,
            );
            if ensured.is_none_or(|(last, _)| last != key) {
              pipelines.ensure(device, key, None);
              ensured = Some((key, BatchShader::Tile));
            }
            return Some(Placed::Chunk {
              state: DrawState {
                page: Some(slot.page),
                shader: BatchShader::Tile,
                blend: chunk.blend,
                sampling,
              },
              instances,
              slot: chunk.slot,
            });
          }
          DrawPayload::Glyph(index) => {
            let glyph = list.glyph(index)?;
            let key = PipelineKey::new(
              glyph.shader,
              glyph.blend,
              Stream::Quads,
              PageKind::Srgb8,
              glyph.sampling,
              format,
            );
            if ensured.is_none_or(|(last, _)| last != key) {
              pipelines.ensure(device, key, None);
              ensured = Some((key, glyph.shader));
            }
            // The instance already carries its glyph page's UVs
            placed.push(glyph.instance);
            return Some(Placed::Quad(DrawState {
              page: Some(SlotPage::Glyph(glyph.page)),
              shader: glyph.shader,
              blend: glyph.blend,
              sampling: glyph.sampling,
            }));
          }
          DrawPayload::Triangles(index) => {
            let line = list.triangles(index)?;
            // A textured one is told where its texture lives
            let (page, sampling, shader) = match line.texture {
              None => (None, Sampling::Nearest, BatchShader::Line),
              Some(texture) => {
                let (slot, _) = textures.slot(texture)?;
                let range = line.vertices.start as usize..line.vertices.end as usize;
                for vertex in triangles.vertices.get_mut(range)? {
                  vertex.bounds = slot.uv;
                }
                let sampling = line.filter.resolve(presentation);
                (Some(slot.page), sampling, BatchShader::LineTextured)
              }
            };
            let key = PipelineKey::new(
              shader,
              line.blend,
              Stream::Triangles,
              PageKind::Srgb8,
              sampling,
              format,
            );
            if ensured.is_none_or(|(last, _)| last != key) {
              pipelines.ensure(device, key, None);
              ensured = Some((key, shader));
            }
            let state = DrawState {
              page,
              shader,
              blend: line.blend,
              sampling,
            };
            let indices = line.indices.clone();
            return Some(Placed::Triangles { state, indices });
          }
        };
        let sprite = list.sprite(index)?;
        let (slot, page_size) = match last {
          Some((texture, slot, size)) if texture == sprite.texture => (slot, size),
          _ => {
            let (slot, size) = textures.slot(sprite.texture)?;
            last = Some((sprite.texture, slot, size));
            (slot, size)
          }
        };
        let wanted = match sprite.shading {
          SpriteShading::Plain => BatchShader::Sprite,
          SpriteShading::Effect => BatchShader::SpriteEffect,
          SpriteShading::Custom(source) => BatchShader::Custom(source),
          SpriteShading::Repeat => BatchShader::Repeat,
          SpriteShading::TileObject => BatchShader::TileObject,
        };
        let sampling = sprite.filter.resolve(presentation);
        let key = PipelineKey::new(
          wanted,
          sprite.blend,
          Stream::Quads,
          PageKind::of(Some(slot.page)),
          sampling,
          format,
        );
        let shader = match ensured {
          Some((last, shader)) if last == key => shader,
          _ => {
            let custom = match wanted {
              BatchShader::Custom(source) => textures.custom_shader(source),
              _ => None,
            };
            pipelines.ensure(device, key, custom);
            // A custom shader draws plain until the GPU accepted its pipeline
            let shader = if pipelines.get(&key).is_some() {
              wanted
            } else {
              let plain = PipelineKey {
                shader: BatchShader::Sprite,
                ..key
              };
              pipelines.ensure(device, plain, None);
              BatchShader::Sprite
            };
            ensured = Some((key, shader));
            shader
          }
        };
        if shader == BatchShader::Repeat {
          // Its UVs count copies; the shader wraps them into the image's place
          let mut instance = sprite.instance;
          instance.uv_bounds = slot.uv;
          placed.push(instance);
        } else if shader == BatchShader::TileObject {
          // It reads inside its tile's cell, not just the tile's texels
          let mut instance = place_in_slot(&sprite.instance, &slot, page_size, false);
          instance.uv_bounds = in_slot(sprite.instance.uv_bounds, &slot);
          placed.push(instance);
        } else {
          // Only built-in effects grow the quad; custom params are not effect ids
          let effects = shader == BatchShader::SpriteEffect;
          placed.push(place_in_slot(&sprite.instance, &slot, page_size, effects));
        }
        Some(Placed::Quad(DrawState {
          page: Some(slot.page),
          shader,
          blend: sprite.blend,
          sampling,
        }))
      };
      let next = plan_draws(list.items_for(camera), &mut drawn.lit, first, &mut place);
      plan_draws(
        list.unlit_items_for(camera),
        &mut drawn.unlit,
        next,
        &mut place,
      );
      // A lit camera's lights, into its light buffer, in one draw: each
      // reads its own shadow from the atlas
      drawn.lights.clear();
      if camera.lit {
        pipelines.ensure(device, light_key(format), None);
        let start = placed.len() as u32;
        placed.extend(list.lights_for(camera).map(|light| light.instance));
        let end = placed.len() as u32;
        if end > start {
          drawn.lights.push(light_draw(start..end));
        }
      }
    }

    if self.placed.len() > self.capacity {
      self.capacity = self.placed.len().next_power_of_two();
      self.instances = instance_buffer(device, self.capacity);
    }
    if !self.placed.is_empty() {
      queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(&self.placed));
    }
    let triangles = &self.triangles;
    let vertices = bytemuck::cast_slice(&triangles.vertices);
    self.vertices.write(device, queue, vertices);
    let indices = bytemuck::cast_slice(&triangles.indices);
    self.indices.write(device, queue, indices);
    // A new version this frame's sprites found waits for its pipelines,
    // made for what was made this frame: sent now, it starts a frame sooner
    self.compile_next(device, textures);
  }

  /// The draws [`prepare`](Self::prepare) laid out for `camera`, in order:
  /// before lighting (`lit`) or after it.
  pub(crate) fn draws(&self, camera: Entity, lit: bool) -> &[Draw] {
    let Some(drawn) = self.draws.iter().find(|d| d.camera == Some(camera)) else {
      return &[];
    };
    if lit { &drawn.lit } else { &drawn.unlit }
  }

  /// The light draws [`prepare`](Self::prepare) laid out for `camera`,
  /// into its light buffer (none for a camera that is not lit).
  pub(crate) fn light_draws(&self, camera: Entity) -> &[Draw] {
    self
      .draws
      .iter()
      .find(|d| d.camera == Some(camera))
      .map_or(&[], |drawn| &drawn.lights)
  }

  /// How many pipelines game sprite shaders have: one per blend mode and
  /// page kind each is drawn with (tests that watch them go)
  // The headless engine reads it, which the web has not
  #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
  pub(crate) fn custom_pipeline_count(&self) -> usize {
    self.pipelines.custom_count()
  }

  /// How many draw calls the laid-out frame issues.
  pub(crate) fn draw_count(&self) -> usize {
    self
      .draws
      .iter()
      .map(|drawn| drawn.lit.len() + drawn.unlit.len() + drawn.lights.len())
      .sum()
  }

  /// Issue one of `camera`'s draws inside a render pass, sampling the pages
  /// of `textures`, setting only the state the pass has not `bound` already
  pub(crate) fn draw<'a>(
    &'a self,
    pass: &mut wgpu::RenderPass<'a>,
    draw: &Draw,
    camera: Entity,
    textures: &'a SpriteGpuRenderer,
    image: (u32, u32),
    bound: &mut Bound<'a>,
  ) {
    self.draw_with(pass, draw, camera, Some(textures), None, image, bound);
  }

  /// Issue `camera`'s light draws inside a render pass into its light
  /// buffer, reading the shadow atlas through `shadows` (the atlas's light
  /// bind group)
  pub(crate) fn draw_lights<'a>(
    &'a self,
    pass: &mut wgpu::RenderPass<'a>,
    camera: Entity,
    shadows: &'a wgpu::BindGroup,
    image: (u32, u32),
  ) {
    let mut bound = Bound::default();
    for draw in self.light_draws(camera) {
      self.draw_with(pass, draw, camera, None, Some(shadows), image, &mut bound);
    }
  }

  /// `draw`, binding a page of `textures` for a textured draw or `shadows`
  /// for a light
  #[allow(clippy::too_many_arguments)]
  fn draw_with<'a>(
    &'a self,
    pass: &mut wgpu::RenderPass<'a>,
    draw: &Draw,
    camera: Entity,
    textures: Option<&'a SpriteGpuRenderer>,
    shadows: Option<&'a wgpu::BindGroup>,
    image: (u32, u32),
    bound: &mut Bound<'a>,
  ) {
    let page = match draw.stream {
      Stream::Quads => PageKind::of(draw.state.page),
      Stream::Triangles => PageKind::Srgb8,
    };
    let key = PipelineKey::new(
      draw.state.shader,
      draw.state.blend,
      draw.stream,
      page,
      draw.state.sampling,
      self.format,
    );
    let (Some(pipeline), Some(camera_group)) =
      (self.pipelines.get(&key), self.camera_groups.get(&camera))
    else {
      return;
    };
    // A chunk's instances are in the chunk arena, placed by its slot
    let (instances, slot) = match draw.source {
      DrawSource::Frame => (&self.instances, None),
      DrawSource::Chunk { slot } => {
        let (Some(instances), Some(group)) = (
          self.chunk_buffers.instances(),
          self.chunk_buffers.slot_group(),
        ) else {
          return;
        };
        (instances, Some((group, slot * SLOT_STRIDE)))
      }
    };
    // Untextured lines bind nothing more; lights bind the shadow atlas;
    // everything else samples a page
    let texture_group = match (draw.state.shader, draw.state.page) {
      (BatchShader::Light(_), _) => match shadows {
        Some(group) => Some(group),
        None => return,
      },
      (_, Some(page)) => {
        match textures.and_then(|t| t.page_bind_group(page, draw.state.sampling)) {
          Some(group) => Some(group),
          None => return,
        }
      }
      (_, None) if draw.stream == Stream::Quads => return,
      (_, None) => None,
    };
    // Every draw covers the whole image
    if bound.scissor != Some(image) {
      pass.set_scissor_rect(0, 0, image.0.max(1), image.1.max(1));
      bound.scissor = Some(image);
    }
    if !bound
      .pipeline
      .is_some_and(|set| std::ptr::eq(set, pipeline))
    {
      pass.set_pipeline(pipeline);
      bound.pipeline = Some(pipeline);
      // A pipeline of another layout may leave the groups unusable
      bound.groups = [None; 3];
    }
    bound.bind(pass, 0, (camera_group, None));
    if let Some(group) = texture_group {
      bound.bind(pass, 1, (group, None));
    }
    if let Some((group, offset)) = slot {
      bound.bind(pass, 2, (group, Some(offset)));
    }
    match draw.stream {
      Stream::Quads => {
        if bound.stream != Some(Stream::Quads) {
          pass.set_vertex_buffer(0, self.quad.slice(..));
          bound.stream = Some(Stream::Quads);
        }
        let size = std::mem::size_of::<SpriteInstance>() as u64;
        let (start, end) = (u64::from(draw.range.start), u64::from(draw.range.end));
        pass.set_vertex_buffer(1, instances.slice(start * size..end * size));
        pass.draw(0..6, 0..draw.range.len() as u32);
      }
      Stream::Triangles => {
        if bound.stream != Some(Stream::Triangles) {
          pass.set_vertex_buffer(0, self.vertices.buffer.slice(..));
          pass.set_index_buffer(self.indices.buffer.slice(..), wgpu::IndexFormat::Uint32);
          bound.stream = Some(Stream::Triangles);
        }
        pass.draw_indexed(draw.range.clone(), 0, 0..1);
      }
    }
  }

  /// Build one pipeline and say whether it exists (tests)
  #[cfg(test)]
  pub(crate) fn ensure_pipeline_for_test(
    &mut self,
    device: &wgpu::Device,
    (shader, sampling): (BatchShader, Sampling),
    blend: BlendMode,
    stream: Stream,
    format: wgpu::TextureFormat,
  ) -> bool {
    let key = PipelineKey::new(shader, blend, stream, PageKind::Srgb8, sampling, format);
    self.pipelines.ensure(device, key, None);
    self.pipelines.get(&key).is_some()
  }

  /// Build a custom shader's pipeline, wait for the GPU's check and say
  /// whether it passed (tests)
  #[cfg(test)]
  pub(crate) fn ensure_custom_for_test(
    &mut self,
    device: &wgpu::Device,
    wgsl: &str,
    blend: BlendMode,
    format: wgpu::TextureFormat,
  ) -> bool {
    let key = PipelineKey::new(
      BatchShader::Custom(1),
      blend,
      Stream::Quads,
      PageKind::Srgb8,
      Sampling::Linear,
      format,
    );
    self.pipelines.ensure(device, key, Some(("test", wgsl)));
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    self.pipelines.settle(|_, _| {});
    self.pipelines.get(&key).is_some()
  }

  /// How many instances the buffer holds.
  #[cfg(test)]
  pub(crate) fn capacity(&self) -> usize {
    self.capacity
  }
}

/// `bounds` (0..1 over an image) where the image sits in `slot`
fn in_slot([u0, v0, u1, v1]: [f32; 4], slot: &AtlasSlot) -> [f32; 4] {
  let [s0, t0, s1, t1] = slot.uv;
  let (du, dv) = (s1 - s0, t1 - t0);
  [s0 + u0 * du, t0 + v0 * dv, s0 + u1 * du, t0 + v1 * dv]
}

/// The pipeline key of a light-buffer draw.
fn light_key(format: wgpu::TextureFormat) -> PipelineKey {
  PipelineKey::new(
    BatchShader::Light(LightStep::Add),
    BlendMode::Additive,
    Stream::Quads,
    PageKind::Srgb8,
    Sampling::Nearest,
    format,
  )
}

/// A light-buffer draw of the light instances in `range`.
fn light_draw(range: std::ops::Range<u32>) -> Draw {
  Draw {
    source: DrawSource::Frame,
    stream: Stream::Quads,
    state: DrawState {
      page: None,
      shader: BatchShader::Light(LightStep::Add),
      blend: BlendMode::Additive,
      sampling: Sampling::Nearest,
    },
    range,
  }
}

/// The unit quad every quad instance is drawn from (laid out as
/// [`QUAD_LAYOUT`](super::QUAD_LAYOUT)): two triangles, split along the
/// diagonal from (0.5, -0.5) to (-0.5, 0.5), centred on the origin; the
/// instance transform places and sizes it
pub(crate) fn unit_quad(device: &wgpu::Device, label: &str) -> wgpu::Buffer {
  #[rustfmt::skip]
  let corners: [f32; 24] = [
    -0.5, -0.5, 0.0, 0.0,
     0.5, -0.5, 1.0, 0.0,
    -0.5,  0.5, 0.0, 1.0,
     0.5, -0.5, 1.0, 0.0,
     0.5,  0.5, 1.0, 1.0,
    -0.5,  0.5, 0.0, 1.0,
  ];
  device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
    label: Some(label),
    contents: bytemuck::cast_slice(&corners),
    usage: wgpu::BufferUsages::VERTEX,
  })
}

fn instance_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
  let size = capacity * std::mem::size_of::<SpriteInstance>();
  let usage = wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST;
  buffer(device, "Batch Instances", size, usage)
}

fn buffer(
  device: &wgpu::Device,
  label: &str,
  size: usize,
  usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
  device.create_buffer(&wgpu::BufferDescriptor {
    label: Some(label),
    size: size as u64,
    usage,
    mapped_at_creation: false,
  })
}
