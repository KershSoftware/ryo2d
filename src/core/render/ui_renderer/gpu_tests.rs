use bytemuck::Zeroable;
use glam::Vec2;

use super::*;
use crate::core::asset::SpriteHandle;
use crate::core::ecs::Entity;
use crate::core::graphics::bind_groups::created_on_this_thread;
use crate::core::graphics::camera_gpu::{CameraGlobals, CameraUniforms};
use crate::core::graphics::{
  Camera2d, CameraView, Presentation, RenderLayers, Sprite, SpriteGpuRenderer, SpriteInstance,
  place_in_slot,
};
use crate::core::render::batch::{
  Batch2D, BatchShader, BlendMode, Filter, LineLook, PolyPoint, PolyStyle, Sampling, line_color,
  push_polyline,
};
use crate::core::render::{
  DrawGlyph, DrawKind, DrawList, DrawShape, PixelRect, SCENE_FORMAT, SpriteLook, SpriteShading,
  UI_FORMAT, plan_cameras,
};
use crate::core::shaders::compose::UI;
use crate::core::text::{FontId, GlyphFormat, GlyphImage, GlyphKey};
use crate::core::trail::{LineCap, LineJoin};
use crate::core::ui::Color;

#[test]
fn the_ui_module_validates_under_webgpu_rules() {
  let module = naga::front::wgsl::parse_str(UI)
    .unwrap_or_else(|e| panic!("the UI module does not parse: {}", e.emit_to_string(UI)));
  naga::valid::Validator::new(
    naga::valid::ValidationFlags::all(),
    naga::valid::Capabilities::empty(),
  )
  .validate(&module)
  .unwrap_or_else(|e| panic!("the UI module does not validate: {e:?}"));
}

/// A 1x1 texture to bind
fn texture(device: &wgpu::Device) -> wgpu::Texture {
  device.create_texture(&wgpu::TextureDescriptor {
    label: Some("UI Test Texture"),
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
  })
}

#[test]
fn the_pipeline_builds_and_bind_groups_are_cached_by_texture() {
  let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
    return;
  };
  let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
  let batch = Batch2D::new(&device, SCENE_FORMAT, Default::default());
  let sprites = SpriteGpuRenderer::new(&device, &queue);
  let mut gpu = UiGpu::new(&device, batch.camera_layout());
  let (a, b) = (texture(&device), texture(&device));
  let samplers = sprites.samplers();
  let before = created_on_this_thread();

  let first = gpu
    .bind_group(&device, [Some(&a), None, None, None], samplers, 0)
    .clone();
  let again = gpu
    .bind_group(&device, [Some(&a), None, None, None], samplers, 1)
    .clone();
  assert_eq!(first, again, "the same textures share one group");
  let other = gpu
    .bind_group(&device, [Some(&b), None, None, None], samplers, 1)
    .clone();
  assert_ne!(first, other, "another texture, another group");
  assert_eq!(created_on_this_thread() - before, 2);

  // `a` is unused from frame 2 on, `b` used every frame
  for frame in 2..=KEEP_FRAMES {
    gpu.bind_group(&device, [Some(&b), None, None, None], samplers, frame);
    gpu.end_frame(frame);
  }
  assert_eq!(gpu.cached_groups(), 2, "unused for fewer frames: kept");
  let frame = KEEP_FRAMES + 1;
  gpu.bind_group(&device, [Some(&b), None, None, None], samplers, frame);
  gpu.end_frame(frame);
  assert_eq!(
    gpu.cached_groups(),
    1,
    "unused for KEEP_FRAMES frames: dropped"
  );
  gpu.bind_group(&device, [Some(&a), None, None, None], samplers, frame + 1);
  assert_eq!(created_on_this_thread() - before, 3, "made again");

  // The instance buffer doubles to hold the frame's instances
  gpu.upload(&device, &queue, &vec![UiInstance::zeroed(); 3000]);
  let stride = std::mem::size_of::<UiInstance>() as u64;
  assert_eq!(gpu.instances().size(), 4096 * stride);

  let error = pollster::block_on(scope.pop());
  assert!(error.is_none(), "{error:?}");
}

const SIDE: u32 = 48;

/// What the UI image holds before the draws (sRGB 51, 102, 153)
const CLEAR: wgpu::Color = wgpu::Color {
  r: 0.2,
  g: 0.4,
  b: 0.6,
  a: 1.0,
};

/// A UI image to draw into and read back
fn ui_image(device: &wgpu::Device) -> wgpu::Texture {
  device.create_texture(&wgpu::TextureDescriptor {
    label: Some("UI Test Image"),
    size: wgpu::Extent3d {
      width: SIDE,
      height: SIDE,
      depth_or_array_layers: 1,
    },
    mip_level_count: 1,
    sample_count: 1,
    dimension: wgpu::TextureDimension::D2,
    format: UI_FORMAT,
    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    view_formats: &[],
  })
}

/// A pass into `view`, cleared to [`CLEAR`]
fn begin<'e>(
  encoder: &'e mut wgpu::CommandEncoder,
  view: &wgpu::TextureView,
) -> wgpu::RenderPass<'e> {
  encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
    label: Some("UI Test Pass"),
    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
      view,
      depth_slice: None,
      resolve_target: None,
      ops: wgpu::Operations {
        load: wgpu::LoadOp::Clear(CLEAR),
        store: wgpu::StoreOp::Store,
      },
    })],
    depth_stencil_attachment: None,
    timestamp_writes: None,
    occlusion_query_set: None,
    multiview_mask: None,
  })
}

/// A glyph image of `format`, `width` x `height`, its texels from `texel`
fn glyph_image(
  format: GlyphFormat,
  (width, height): (u32, u32),
  texel: impl Fn(u32, u32) -> [u8; 4],
) -> GlyphImage {
  let channels = if format == GlyphFormat::Mask { 1 } else { 4 };
  let mut data = Vec::new();
  for y in 0..height {
    for x in 0..width {
      data.extend_from_slice(&texel(x, y)[..channels]);
    }
  }
  GlyphImage {
    format,
    width,
    height,
    left: 0,
    top: height as i32,
    data,
    base: 16.0,
  }
}

/// A sprite of `width` x `height` with varied colour and alpha
fn gradient(width: u32, height: u32) -> SpriteHandle {
  let mut data = Vec::new();
  for y in 0..height {
    for x in 0..width {
      let a = if (x + y) % 3 == 0 { 128 } else { 255 };
      data.extend_from_slice(&[
        (x * 40 % 256) as u8,
        (y * 50 % 256) as u8,
        (255 - x * 15 % 256) as u8,
        a,
      ]);
    }
  }
  SpriteHandle::from_sprite(Sprite::from_rgba(data, width, height), "gradient")
}

/// A quad's instance at `centre`, `size` pixels, turned by `angle`
fn quad(centre: Vec2, size: Vec2, angle: f32, tint: Color) -> SpriteInstance {
  SpriteInstance::new()
    .with_transform(centre.x, centre.y, angle, size.x, size.y)
    .with_tint(tint, 1.0)
}

/// Every kind but the linear sprite (a render target) in one image, all but
/// one rect clipped by one scissor: through the batch's pipelines with the
/// scissor, and through the UI pipeline with the clip in each instance and
/// one bind group per run. The two images are the same, pixel for pixel.
#[test]
fn every_kind_draws_as_the_batch_draws_it() {
  let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
    return;
  };
  let entity = Entity::PLACEHOLDER;
  let camera = Camera2d::default();
  let size = Vec2::splat(SIDE as f32);
  let view = CameraView::compute(size / 2.0, 0.0, &camera, Vec2::ZERO, size);
  let (mut cameras, mut targetless) = (Vec::new(), Vec::new());
  plan_cameras(
    [(entity, &camera, &view, None)].into_iter(),
    (SIDE, SIDE),
    false,
    None,
    &mut cameras,
    &mut targetless,
  );
  cameras[0].ui = true;
  let scissor = PixelRect {
    x: 7,
    y: 5,
    width: 34,
    height: 38,
  };
  let clip = ClipRect::of(Some(scissor), (SIDE, SIDE));
  let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
  let mut sprites = SpriteGpuRenderer::new(&device, &queue);
  let mut batch = Batch2D::new(&device, SCENE_FORMAT, Default::default());
  let mut list = DrawList::default();
  // The UI's instances in painter's order, and what each samples; the
  // sprites' are made once their textures are placed
  let mut instances = Vec::new();
  let mut needs = Vec::new();

  // A rounded, bordered rect off the pixel grid as `emit_ui` makes one (one
  // pixel of anti-aliasing margin), clipped; a small one unclipped
  let rect = |centre: Vec2, w: f32, h: f32, fill: Color| {
    let p = 1.0;
    let mut rect = quad(centre, Vec2::new(w + 2.0 * p, h + 2.0 * p), 0.0, fill);
    rect.uv_rect = [-p / w, -p / h, 1.0 + p / w, 1.0 + p / h];
    rect.uv_bounds = rect.uv_rect;
    rect.effect_params = [4.0, 1.5, w, h];
    rect.effect_extra = [0.1, 0.9, 0.3, 1.0];
    let half = Vec2::new(w / 2.0 + p, h / 2.0 + p);
    (rect, centre - half, centre + half)
  };
  for (z, (centre, w, h), scissor) in [
    (0, (Vec2::new(14.3, 13.6), 17.2, 13.9), Some(scissor)),
    (7, (Vec2::new(4.2, 44.4), 7.3, 5.1), None),
  ] {
    let (instance, min, max) = rect(centre, w, h, Color::new(200, 60, 30, 230));
    list.push_shape(
      DrawKind::Sprite,
      z,
      false,
      Some(entity),
      DrawShape {
        instance,
        blend: BlendMode::Alpha,
        layers: RenderLayers::all(),
        min,
        max,
        scissor,
      },
    );
    let clip = ClipRect::of(scissor, (SIDE, SIDE));
    instances.push(UiInstance::quad(UiKind::RECT, &instance, clip, false, 0));
    needs.push(TexNeed::None);
  }
  // The rect at z 7 is drawn last: keep the UI's order the batch's
  let unclipped = (instances.pop(), needs.pop());

  // A slanted, mitred line over the first rect, one instance per triangle
  let points: Vec<PolyPoint> = [(3.0, 27.5), (16.2, 9.4), (29.0, 22.0)]
    .iter()
    .enumerate()
    .map(|(i, &(x, y))| PolyPoint {
      pos: Vec2::new(x, y),
      half_width: 1.6,
      color: line_color(Color::new(250, 240, 90, 200), 1.0),
      u: i as f32 / 2.0,
    })
    .collect();
  let style = PolyStyle {
    join: LineJoin::Miter,
    cap: LineCap::Butt,
    closed: false,
    params: [0.0; 4],
  };
  let look = LineLook {
    lit: false,
    z: 1,
    texture: None,
    blend: BlendMode::Alpha,
    filter: Filter::Default,
    layers: RenderLayers::all(),
    only_camera: Some(entity),
  };
  push_polyline(&mut list, batch.triangles(), &points, &style, look);
  list.set_last_triangles_scissor(Some(scissor));
  let triangles = batch.triangles();
  assert!(!triangles.indices.is_empty());
  for triangle in triangles.indices.as_chunks::<3>().0 {
    let vertex = |i: u32| &triangles.vertices[i as usize];
    let corner = |i: u32| {
      let v = vertex(i);
      (Vec2::from(v.position), Vec2::from(v.uv), v.color)
    };
    instances.push(UiInstance::triangle(
      [
        corner(triangle[0]),
        corner(triangle[1]),
        corner(triangle[2]),
      ],
      vertex(triangle[0]).params,
      clip,
    ));
    needs.push(TexNeed::None);
  }

  // A raster, a colour and an MSDF glyph (a disc's distances, outlined),
  // each magnified so filtering shows
  let disc = |x: u32, y: u32| {
    let d = (Vec2::new(x as f32, y as f32) - Vec2::splat(3.5)).length();
    let v = ((3.0 - d) / 4.0 + 0.5).clamp(0.0, 1.0);
    let c = (v * 255.0) as u8;
    [c, c.saturating_add(6), c.saturating_sub(5), 255]
  };
  let glyphs = [
    (
      GlyphFormat::Mask,
      BatchShader::TextMask,
      UiKind::MASK,
      Vec2::new(30.4, 10.7),
      Vec2::new(9.3, 12.6),
      glyph_image(GlyphFormat::Mask, (6, 8), |x, y| {
        [((x * 37 + y * 91) % 256) as u8, 0, 0, 0]
      }),
    ),
    (
      GlyphFormat::Color,
      BatchShader::TextColor,
      UiKind::COLOR,
      Vec2::new(40.2, 12.1),
      Vec2::new(10.5, 10.5),
      glyph_image(GlyphFormat::Color, (8, 8), |x, y| {
        [(x * 30) as u8, (y * 30) as u8, 90, ((x + y) * 16) as u8]
      }),
    ),
    (
      GlyphFormat::Msdf,
      BatchShader::TextMsdf,
      UiKind::MSDF,
      Vec2::new(12.7, 33.3),
      Vec2::new(14.0, 14.0),
      glyph_image(GlyphFormat::Msdf, (8, 8), disc),
    ),
  ];
  for (z, (format, shader, kind, centre, size, image)) in (2..).zip(glyphs) {
    let key = GlyphKey::fill(FontId(0), z as u16, 16.0, 0.0);
    let slot = sprites
      .glyph(&device, &queue, key, || Some(image))
      .expect("the glyph is placed");
    assert_eq!(slot.page.format, format);
    let mut instance = quad(centre, size, 0.0, Color::new(230, 220, 250, 240));
    instance.uv_rect = slot.uv;
    if format == GlyphFormat::Msdf {
      instance.effect_params = [0.0, 0.6, 0.0, 4.0];
      instance.effect_extra = [0.9, 0.2, 0.1, 1.0];
    }
    list.push_glyph(
      DrawKind::Text,
      z,
      false,
      Some(entity),
      DrawGlyph {
        page: slot.page,
        instance,
        shader,
        blend: BlendMode::Alpha,
        sampling: Sampling::Linear,
        layers: RenderLayers::all(),
        min: centre - size,
        max: centre + size,
        scissor: Some(scissor),
      },
    );
    instances.push(UiInstance::quad(
      kind,
      &instance,
      clip,
      false,
      slot.page.index,
    ));
    needs.push(TexNeed::Glyph(slot.page));
  }

  // Two sprites of one atlas page, nearest and linear, one turned, both
  // minified on one axis so the mip levels show
  let sprite_quads = [
    (
      gradient(5, 4),
      Filter::Nearest,
      Vec2::new(28.6, 30.2),
      Vec2::new(11.3, 9.1),
      0.3,
    ),
    (
      gradient(17, 15),
      Filter::Linear,
      Vec2::new(35.3, 37.9),
      Vec2::new(13.0, 6.5),
      0.0,
    ),
  ];
  let mut placed = Vec::new();
  for (z, (sprite, filter, centre, size, angle)) in (5..).zip(sprite_quads) {
    let texture = sprites.texture_of(&sprite);
    let instance = quad(centre, size, angle, Color::new(255, 230, 210, 250));
    let look = SpriteLook {
      shading: SpriteShading::Plain,
      blend: BlendMode::Alpha,
      filter,
      lit: false,
    };
    list.push_sprite_in(
      DrawKind::Sprite,
      z,
      texture,
      look,
      instance,
      (RenderLayers::all(), None, Some(entity)),
    );
    list.set_last_sprite_scissor(Some(scissor));
    placed.push((texture, instance, filter));
  }
  list.sorted();

  let mut uniforms = CameraUniforms::default();
  let globals = CameraGlobals::new(&view, (SIDE, SIDE), 0.0, 0.0, false);
  uniforms.sync(&device, &queue, &[(entity, globals)]);
  batch.sync_cameras(&device, &uniforms);
  batch.prepare(&device, &queue, &list, &cameras, &mut sprites);

  // The sprites remapped into their slots as the batch does
  for (texture, instance, filter) in placed {
    let (slot, page_size) = sprites.slot(texture).expect("the sprite is resident");
    let instance = place_in_slot(&instance, &slot, page_size, false);
    let nearest = filter.resolve(Presentation::default()) == Sampling::Nearest;
    instances.push(UiInstance::quad(
      UiKind::SPRITE,
      &instance,
      clip,
      nearest,
      0,
    ));
    needs.push(TexNeed::Sprite(texture));
  }
  if let (Some(instance), Some(need)) = unclipped {
    instances.push(instance);
    needs.push(need);
  }

  // One run: every slot holds one texture
  let runs: Vec<Run> = plan_runs(&needs, |need| match need {
    TexNeed::None => None,
    TexNeed::Glyph(page) => {
      let slot = match page.format {
        GlyphFormat::Mask => Slot::Mask,
        GlyphFormat::Color => Slot::Color,
        GlyphFormat::Msdf => Slot::Msdf,
      };
      Some((slot, TexKey::Glyph(page)))
    }
    TexNeed::Sprite(texture) => {
      let (slot, _) = sprites.slot(texture)?;
      Some((Slot::Sprite, TexKey::Sprite(slot.page)))
    }
  });
  assert_eq!(runs.len(), 1, "{runs:?}");
  assert!(runs[0].textures.iter().all(Option::is_some), "{runs:?}");
  let mut gpu = UiGpu::new(&device, batch.camera_layout());
  gpu.upload(&device, &queue, &instances);
  let groups: Vec<_> = runs
    .iter()
    .map(|run| {
      let textures = run
        .textures
        .map(|key| key.and_then(|key| sprites.page_texture(key)));
      assert!(textures.iter().all(Option::is_some));
      let group = gpu.bind_group(&device, textures, sprites.samplers(), 0);
      (run.range.clone(), group.clone())
    })
    .collect();
  let camera_group = batch.camera_group(entity).expect("the camera is bound");

  let (old, new) = (ui_image(&device), ui_image(&device));
  let mut encoder = device.create_command_encoder(&Default::default());
  {
    let view = old.create_view(&Default::default());
    let mut pass = begin(&mut encoder, &view);
    for draw in batch.draws(entity, false) {
      batch.draw(&mut pass, draw, entity, &sprites, (SIDE, SIDE));
    }
  }
  {
    let view = new.create_view(&Default::default());
    let mut pass = begin(&mut encoder, &view);
    pass.set_pipeline(gpu.pipeline());
    pass.set_bind_group(0, camera_group, &[]);
    pass.set_vertex_buffer(0, gpu.quad().slice(..));
    pass.set_vertex_buffer(1, gpu.instances().slice(..));
    for (range, group) in &groups {
      pass.set_bind_group(1, group, &[]);
      pass.draw(0..6, range.clone());
    }
  }
  queue.submit([encoder.finish()]);
  let error = pollster::block_on(scope.pop());
  assert!(error.is_none(), "{error:?}");
  let old = crate::core::capture::read_texture_rgba(&device, &queue, &old).expect("read back");
  let new = crate::core::capture::read_texture_rgba(&device, &queue, &new).expect("read back");

  let cleared = [51, 102, 153, 255];
  // Inside the first rect and the clip: drawn; inside it left of the clip:
  // untouched; the unclipped rect outside the clip: drawn
  assert_ne!(new.get_pixel(15, 14).0, cleared);
  assert_eq!(new.get_pixel(6, 14).0, cleared);
  assert_ne!(new.get_pixel(4, 44).0, cleared);
  let drawn = new.pixels().filter(|pixel| pixel.0 != cleared).count();
  assert!(drawn > 500, "{drawn} pixels drawn");
  let differ: Vec<_> = old
    .enumerate_pixels()
    .zip(new.pixels())
    .filter(|((_, _, a), b)| a != b)
    .map(|((x, y, a), b)| ((x, y), a.0, b.0))
    .collect();
  assert!(
    differ.is_empty(),
    "{} pixels (old, new) differ: {:?}",
    differ.len(),
    &differ[..differ.len().min(8)]
  );
}
