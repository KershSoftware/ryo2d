//! The lighting renderer: each lit camera's light buffer and the compose
//! pass that lights the scene with it, and the frame's shadow plan and the
//! atlas its maps are drawn into.
//!
//! Lights are batch draws (`lighting::batch`), drawn into the light buffers
//! by the `lighting` pass, each reading its shadow from the atlas.

use glam::Vec2;

use super::atlas::ShadowAtlas;
use super::index::OccluderIndex;
use super::plan::{MAX_SHADOWED_LIGHTS, PlannedOccluder, ShadowPlan, ShadowView, plan_shadows};
use super::{AmbientLight, Atmosphere, FrameLight, ShadowQuality};
use crate::core::ecs::Entity;
use crate::core::graphics::bind_groups::{BindGroupCache, Bound};
use crate::core::render::CameraDraw;
use std::collections::HashMap;

/// Uniforms for the lighting composition pass
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct LightingParams {
  /// Ambient color (RGB) + unused (A)
  pub(crate) color: [f32; 4],
  /// Ambient intensity (x), atmosphere density (y), atmosphere scattering (z), unused (w)
  pub(crate) params: [f32; 4],
  /// Time (x), width (y), height (z), unused (w)
  pub(crate) time_resolution: [f32; 4],
  /// Atmosphere fog color (RGB) + unused (A)
  pub(crate) atmosphere_color: [f32; 4],
}

/// The textures one image is lit with, at its size: the light
/// accumulation buffer, the compose pass's uniforms and, for a camera lit
/// in place (one with an image of its own), the copy of its image the
/// compose pass reads. A camera drawing straight into the scene image is
/// composed from the scene image into the lit image, so it has no copy.
struct LightTargets {
  light_view: wgpu::TextureView,
  scene_copy: Option<(wgpu::Texture, wgpu::TextureView)>,
  compose_uniforms: wgpu::Buffer,
  width: u32,
  height: u32,
}

impl LightTargets {
  fn new(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    in_place: bool,
  ) -> Self {
    let size = wgpu::Extent3d {
      width,
      height,
      depth_or_array_layers: 1,
    };
    // The batch's light draws accumulate into it (their pipelines are
    // made for the scene format); its alpha masks shadowed lights
    let light_texture = device.create_texture(&wgpu::TextureDescriptor {
      label: Some("Light Accumulation Texture"),
      size,
      mip_level_count: 1,
      sample_count: 1,
      dimension: wgpu::TextureDimension::D2,
      format: crate::core::render::SCENE_FORMAT,
      usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
      view_formats: &[],
    });
    // Lit in place, the compose pass reads a copy of the image, so it can
    // write the lit image over the original
    let scene_copy = in_place.then(|| {
      let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Scene Copy Texture"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
      });
      let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
      (texture, view)
    });
    let compose_uniforms = device.create_buffer(&wgpu::BufferDescriptor {
      label: Some("Lighting Compose Uniform Buffer"),
      size: std::mem::size_of::<LightingParams>() as u64,
      usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
      mapped_at_creation: false,
    });
    Self {
      light_view: light_texture.create_view(&wgpu::TextureViewDescriptor::default()),
      scene_copy,
      compose_uniforms,
      width,
      height,
    }
  }
}

/// The lighting renderer manages GPU resources for 2D lighting.
pub(crate) struct LightingRenderer {
  /// The format of the images it lights, which its scene copies share
  format: wgpu::TextureFormat,

  /// The targets of each lit camera, at its image's size
  cameras: HashMap<Entity, LightTargets>,

  /// Each lit camera's compose bind group, made once
  bind_groups: BindGroupCache,

  /// Sampler for textures
  sampler: wgpu::Sampler,

  /// Pipeline for compositing lights with scene
  compose_pipeline: wgpu::RenderPipeline,

  /// Bind group layout for composition
  compose_bind_group_layout: wgpu::BindGroupLayout,

  /// Current ambient light settings
  ambient: AmbientLight,

  /// Current atmosphere settings
  atmosphere: Atmosphere,

  /// Whether lighting is enabled
  enabled: bool,

  /// The frame's shadowed lights and their casting edges
  shadow_plan: ShadowPlan,

  /// The lit cameras' views the plan reads, kept
  shadow_views: Vec<ShadowView>,

  /// The shadowed lights' maps
  atlas: ShadowAtlas,

  /// The run's warning that a frame's edges passed the largest buffer
  /// was logged
  warned_edges: bool,

  /// The run's warning that a frame's shadowed lights passed the cap was
  /// logged
  warned_cap: bool,
}

impl LightingRenderer {
  /// A lighting renderer for images of `format`.
  pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
    // Create sampler
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
      label: Some("Lighting Sampler"),
      address_mode_u: wgpu::AddressMode::ClampToEdge,
      address_mode_v: wgpu::AddressMode::ClampToEdge,
      address_mode_w: wgpu::AddressMode::ClampToEdge,
      mag_filter: wgpu::FilterMode::Linear,
      min_filter: wgpu::FilterMode::Linear,
      mipmap_filter: wgpu::MipmapFilterMode::Nearest,
      ..Default::default()
    });

    let compose_bind_group_layout =
      device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Lighting Compose Bind Group Layout"),
        entries: &[
          // Scene texture
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
          // Light texture
          wgpu::BindGroupLayoutEntry {
            binding: 1,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
              multisampled: false,
              view_dimension: wgpu::TextureViewDimension::D2,
              sample_type: wgpu::TextureSampleType::Float { filterable: true },
            },
            count: None,
          },
          // Sampler
          wgpu::BindGroupLayoutEntry {
            binding: 2,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
          },
          // Uniforms
          wgpu::BindGroupLayoutEntry {
            binding: 3,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
              ty: wgpu::BufferBindingType::Uniform,
              has_dynamic_offset: false,
              min_binding_size: None,
            },
            count: None,
          },
        ],
      });

    // Create composition pipeline
    let compose_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some("Lighting Compose Shader"),
      source: wgpu::ShaderSource::Wgsl(crate::core::shaders::compose::LIGHTING_COMPOSE.into()),
    });

    let compose_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("Lighting Compose Pipeline Layout"),
      bind_group_layouts: &[Some(&compose_bind_group_layout)],
      immediate_size: 0,
    });

    let compose_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some("Lighting Compose Pipeline"),
      layout: Some(&compose_pipeline_layout),
      vertex: wgpu::VertexState {
        module: &compose_shader,
        entry_point: Some("vs_main"),
        buffers: &[],
        compilation_options: Default::default(),
      },
      fragment: Some(wgpu::FragmentState {
        module: &compose_shader,
        entry_point: Some("fs_main"),
        targets: &[Some(wgpu::ColorTargetState {
          format,
          blend: Some(wgpu::BlendState::REPLACE),
          write_mask: wgpu::ColorWrites::ALL,
        })],
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
      depth_stencil: None,
      multisample: wgpu::MultisampleState {
        count: 1,
        mask: !0,
        alpha_to_coverage_enabled: false,
      },
      multiview_mask: None,
      cache: None,
    });

    Self {
      format,
      cameras: HashMap::new(),
      bind_groups: BindGroupCache::default(),
      sampler,
      compose_pipeline,
      compose_bind_group_layout,
      ambient: AmbientLight::default(),
      atmosphere: Atmosphere::default(),
      enabled: false, // Disabled by default, enabled when Light2D entities exist
      shadow_plan: ShadowPlan::default(),
      shadow_views: Vec::new(),
      atlas: ShadowAtlas::new(device),
      warned_edges: false,
      warned_cap: false,
    }
  }

  /// Set the ambient light.
  pub(crate) fn set_ambient(&mut self, ambient: AmbientLight) {
    self.ambient = ambient;
  }

  /// Set the atmosphere settings.
  ///
  /// Atmosphere controls fog density, color tinting, and light scattering.
  /// Use presets like `Atmosphere::dusty()` or `Atmosphere::foggy()`.
  pub(crate) fn set_atmosphere(&mut self, atmosphere: Atmosphere) {
    self.atmosphere = atmosphere;
  }

  /// Get the current atmosphere settings.
  pub(crate) fn atmosphere(&self) -> &Atmosphere {
    &self.atmosphere
  }

  /// Enable or disable the lighting system.
  pub(crate) fn set_enabled(&mut self, enabled: bool) {
    self.enabled = enabled;
  }

  /// Check if lighting is enabled.
  pub(crate) fn is_enabled(&self) -> bool {
    self.enabled
  }

  /// Keep light targets for every lit camera of `cameras`, sized to its
  /// image (with a scene copy when it is lit in place); drop the others.
  pub(crate) fn sync_cameras(&mut self, device: &wgpu::Device, cameras: &[CameraDraw]) {
    let before = self.cameras.len();
    self.cameras.retain(|entity, targets| {
      cameras.iter().any(|camera| {
        camera.entity == *entity
          && camera.lit
          && (targets.width, targets.height) == camera.rect.size()
          && targets.scene_copy.is_some() != camera.direct
      })
    });
    let format = self.format;
    let mut changed = self.cameras.len() != before;
    for camera in cameras.iter().filter(|camera| camera.lit) {
      self.cameras.entry(camera.entity).or_insert_with(|| {
        changed = true;
        let (width, height) = camera.rect.size();
        LightTargets::new(device, width, height, format, !camera.direct)
      });
    }
    // Bind groups of dropped or recreated targets would keep them alive
    if changed {
      self.bind_groups.clear();
    }
  }

  /// Plans the frame's shadows for the lit `cameras` (`plan::plan_shadows`,
  /// reading occluders from `index` through `occluder`), at most as many
  /// lights as the atlas's largest height holds and as many edges as the
  /// largest buffer holds (each warned once a run past it: the lights past
  /// the cap draw unshadowed), and makes the atlas fit them at `quality`.
  pub(crate) fn plan_shadows<'w>(
    &mut self,
    device: &wgpu::Device,
    quality: ShadowQuality,
    lights: &[FrameLight],
    cameras: &[CameraDraw],
    index: Option<&OccluderIndex>,
    occluder: impl FnMut(Entity) -> Option<PlannedOccluder<'w>>,
  ) {
    let Some(index) = index else {
      self.shadow_plan.clear();
      return;
    };
    self.shadow_views.clear();
    self
      .shadow_views
      .extend(cameras.iter().filter(|camera| camera.lit).map(|camera| {
        let bounds = camera.view.bounds;
        (
          camera.layers,
          Vec2::new(bounds.left, bounds.top),
          Vec2::new(bounds.right, bounds.bottom),
          camera.view.center,
        )
      }));
    // Four rows a light, within the device's tallest texture
    let cap = MAX_SHADOWED_LIGHTS.min(device.limits().max_texture_dimension_2d / 4);
    let dropped = plan_shadows(
      &mut self.shadow_plan,
      lights,
      &self.shadow_views,
      index,
      occluder,
      cap,
      self.atlas.max_edges(),
    );
    if dropped > 0 && !self.warned_cap {
      self.warned_cap = true;
      log::warn!(
        "{dropped} shadowed lights past the cap of {cap} a frame: the farthest from the cameras draw unshadowed"
      );
    }
    let past = self.shadow_plan.dropped_edges;
    if past > 0 && !self.warned_edges {
      self.warned_edges = true;
      log::warn!(
        "a frame's shadows have {past} edges more than the largest GPU buffer holds: the farthest lights draw unshadowed"
      );
    }
    if self.shadow_plan.rows > 0 {
      self.atlas.ensure(device, quality, self.shadow_plan.rows);
    }
  }

  /// Draws the planned shadow maps into the atlas in the frame's
  /// `encoder`, before any camera pass; nothing when no light has a slot.
  pub(crate) fn encode_shadows(&mut self, encoder: &mut wgpu::CommandEncoder, queue: &wgpu::Queue) {
    if self.shadow_plan.rows > 0 {
      self.atlas.encode(encoder, queue, &self.shadow_plan.edges);
    }
  }

  /// The frame's shadow plan (tests)
  #[cfg(test)]
  pub(crate) fn shadow_plan(&self) -> &ShadowPlan {
    &self.shadow_plan
  }

  /// Each of the frame's lights' atlas slot, by its index in the lights the
  /// plan was given (`None`: unshadowed)
  pub(crate) fn shadow_slots(&self) -> &[Option<u32>] {
    &self.shadow_plan.slots
  }

  /// The shadow atlas bound for the light shader
  pub(crate) fn shadow_group(&self) -> &wgpu::BindGroup {
    self.atlas.light_group()
  }

  /// How many GPU resources the shadow atlas has made (tests that watch a
  /// settled frame make none; the headless engine, which the web has not)
  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn shadow_atlas_builds(&self) -> u64 {
    self.atlas.builds()
  }

  /// The shadow atlas (tests)
  #[cfg(test)]
  pub(crate) fn shadow_atlas(&self) -> &ShadowAtlas {
    &self.atlas
  }

  /// `camera`'s light buffer, which the batch draws its lights into.
  pub(crate) fn light_view(&self, camera: Entity) -> Option<&wgpu::TextureView> {
    self.cameras.get(&camera).map(|targets| &targets.light_view)
  }

  /// How many lit cameras keep a scene copy: those lit in place (tests)
  #[cfg(test)]
  pub(crate) fn scene_copies(&self) -> usize {
    self
      .cameras
      .values()
      .filter(|targets| targets.scene_copy.is_some())
      .count()
  }

  /// How many cameras have light targets.
  #[cfg(test)]
  pub(crate) fn camera_count(&self) -> usize {
    self.cameras.len()
  }

  /// Compose `camera`'s lit image into `output_view` from its scene image
  /// (`scene_texture`, viewed by `scene_view`) and its light buffer. A
  /// camera lit in place (`scene_texture` the one behind `output_view`) has
  /// its image copied first and the copy read; a camera drawing straight
  /// into the scene image reads the scene image itself, `output_view`
  /// being the lit image.
  #[allow(clippy::too_many_arguments)]
  pub(crate) fn compose_for_camera(
    &self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    camera: Entity,
    scene_texture: &wgpu::Texture,
    scene_view: &wgpu::TextureView,
    output_view: &wgpu::TextureView,
    time: f32,
  ) {
    let Some(targets) = self.cameras.get(&camera) else {
      return;
    };
    if scene_texture.size().width != targets.width || scene_texture.size().height != targets.height
    {
      return;
    }
    let source = match &targets.scene_copy {
      Some((copy, view)) => {
        copy_scene(encoder, scene_texture, copy, targets);
        view
      }
      None => scene_view,
    };
    self.compose_targets(device, queue, encoder, targets, source, output_view, time);
  }

  /// The compose pass: `scene_view` lit by the light buffer in `targets`
  /// (plus ambient and atmosphere), into `output_view`.
  #[allow(clippy::too_many_arguments)]
  fn compose_targets(
    &self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    targets: &LightTargets,
    scene_view: &wgpu::TextureView,
    output_view: &wgpu::TextureView,
    time: f32,
  ) {
    let [ar, ag, ab, _] = self.ambient.color.to_linear();
    let [fr, fg, fb, _] = self.atmosphere.color.to_linear();
    let compose_uniform = LightingParams {
      color: [ar, ag, ab, 1.0],
      params: [
        self.ambient.intensity,
        self.atmosphere.density,
        self.atmosphere.scattering,
        0.0,
      ],
      time_resolution: [time, targets.width as f32, targets.height as f32, 0.0],
      atmosphere_color: [fr, fg, fb, 1.0],
    };
    queue.write_buffer(
      &targets.compose_uniforms,
      0,
      bytemuck::cast_slice(&[compose_uniform]),
    );
    let compose_bind_group = self.bind_groups.get(
      device,
      "Lighting Compose Bind Group",
      &self.compose_bind_group_layout,
      &[
        Bound::View(scene_view.clone()),
        Bound::View(targets.light_view.clone()),
        Bound::Sampler(self.sampler.clone()),
        Bound::Buffer(targets.compose_uniforms.clone()),
      ],
    );
    let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
      label: Some("Lighting Compose Pass"),
      color_attachments: &[Some(wgpu::RenderPassColorAttachment {
        view: output_view,
        depth_slice: None,
        resolve_target: None,
        ops: wgpu::Operations {
          load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
          store: wgpu::StoreOp::Store,
        },
      })],
      depth_stencil_attachment: None,
      occlusion_query_set: None,
      multiview_mask: None,
      timestamp_writes: None,
    });
    render_pass.set_pipeline(&self.compose_pipeline);
    render_pass.set_bind_group(0, &compose_bind_group, &[]);
    render_pass.draw(0..6, 0..1);
  }
}

/// Copy `scene_texture` into `copy`, the scene copy of `targets` (the same
/// size).
fn copy_scene(
  encoder: &mut wgpu::CommandEncoder,
  scene_texture: &wgpu::Texture,
  copy: &wgpu::Texture,
  targets: &LightTargets,
) {
  encoder.copy_texture_to_texture(
    wgpu::TexelCopyTextureInfo {
      texture: scene_texture,
      mip_level: 0,
      origin: wgpu::Origin3d::ZERO,
      aspect: wgpu::TextureAspect::All,
    },
    wgpu::TexelCopyTextureInfo {
      texture: copy,
      mip_level: 0,
      origin: wgpu::Origin3d::ZERO,
      aspect: wgpu::TextureAspect::All,
    },
    wgpu::Extent3d {
      width: targets.width,
      height: targets.height,
      depth_or_array_layers: 1,
    },
  );
}
