use bytemuck::Zeroable;
use glam::Vec2;

use super::*;
use crate::core::asset::Handle;
use crate::core::ecs::Entity;
use crate::core::graphics::bind_groups::created_on_this_thread;
use crate::core::graphics::camera_gpu::{CameraGlobals, CameraUniforms};
use crate::core::graphics::{
  Camera2d, CameraView, Image, Presentation, RenderLayers, SpriteGpuRenderer, SpriteInstance,
  place_in_slot,
};
use crate::core::render::batch::{
  Batch2D, BlendMode, Filter, LineLook, PolyPoint, PolyStyle, Sampling, Triangles, line_color,
  push_polyline,
};
use crate::core::render::{
  DrawKind, DrawList, PixelRect, SCENE_FORMAT, SpriteLook, SpriteShading, UI_FORMAT, plan_cameras,
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
  gpu.upload(&device, &queue, &vec![UiInstance::zeroed(); 3000], &[], &[]);
  let stride = std::mem::size_of::<UiInstance>() as u64;
  let slots = (SLOTS * std::mem::size_of::<SlotUniform>()) as u64;
  assert_eq!(gpu.instances().size(), 4096 * stride + slots);

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
  begin_cleared(encoder, view, CLEAR)
}

/// A pass into `view`, cleared to `clear`
fn begin_cleared<'e>(
  encoder: &'e mut wgpu::CommandEncoder,
  view: &wgpu::TextureView,
  clear: wgpu::Color,
) -> wgpu::RenderPass<'e> {
  encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
    label: Some("UI Test Pass"),
    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
      view,
      depth_slice: None,
      resolve_target: None,
      ops: wgpu::Operations {
        load: wgpu::LoadOp::Clear(clear),
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

/// An image of `width` x `height` with varied colour and alpha, in the
/// test thread's store
fn gradient(width: u32, height: u32) -> Handle<Image> {
  crate::core::graphics::test_handle(gradient_image(width, height))
}

/// An image of `width` x `height` with varied colour and alpha
fn gradient_image(width: u32, height: u32) -> Image {
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
  Image::from_rgba(data, width, height)
}

/// A quad's instance at `centre`, `size` pixels, turned by `angle`
fn quad(centre: Vec2, size: Vec2, angle: f32, tint: Color) -> SpriteInstance {
  SpriteInstance::new()
    .with_transform(centre.x, centre.y, angle, size.x, size.y)
    .with_tint(tint, 1.0)
}

/// The UI camera's uniforms for a `side` x `side` image, bound through the
/// batch's camera layout (as the engine binds them)
fn camera_bound(
  device: &wgpu::Device,
  queue: &wgpu::Queue,
  batch: &mut Batch2D,
  entity: Entity,
  side: u32,
) {
  let size = Vec2::splat(side as f32);
  let view = CameraView::compute(size / 2.0, 0.0, &Camera2d::default(), Vec2::ZERO, size);
  let mut uniforms = CameraUniforms::default();
  let globals = CameraGlobals::new(&view, (side, side), 0.0, 0.0, false);
  uniforms.sync(device, queue, &[(entity, globals)]);
  batch.sync_cameras(device, &uniforms);
}

/// Every kind but the linear sprite (a render target) in one image through
/// the UI pipeline, all but one rect clipped by one clip in each instance,
/// in one run: each draws, and no pixel outside the clip changes but the
/// unclipped rect's
#[test]
fn every_kind_draws_inside_its_clip() {
  let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
    return;
  };
  let entity = Entity::PLACEHOLDER;
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
  // The UI's instances in painter's order, and what each samples; the
  // sprites' are made once their textures are placed
  let mut instances = Vec::new();
  let mut needs = Vec::new();

  // A rounded, bordered rect off the pixel grid as `rect_instance` makes
  // one (one pixel of anti-aliasing margin), clipped; a small one unclipped
  let rect = |centre: Vec2, w: f32, h: f32, fill: Color| {
    let p = 1.0;
    let mut rect = quad(centre, Vec2::new(w + 2.0 * p, h + 2.0 * p), 0.0, fill);
    rect.uv_rect = [-p / w, -p / h, 1.0 + p / w, 1.0 + p / h];
    rect.uv_bounds = rect.uv_rect;
    rect.effect_params = [4.0, 1.5, w, h];
    rect.effect_extra = [0.1, 0.9, 0.3, 1.0];
    rect
  };
  for ((centre, w, h), scissor) in [
    ((Vec2::new(14.3, 13.6), 17.2, 13.9), Some(scissor)),
    ((Vec2::new(4.2, 44.4), 7.3, 5.1), None),
  ] {
    let instance = rect(centre, w, h, Color::new(200, 60, 30, 230));
    let clip = ClipRect::of(scissor, (SIDE, SIDE));
    instances.push(UiInstance::quad(UiKind::RECT, &instance, clip, false, 0));
    needs.push(TexNeed::None);
  }
  // The unclipped rect is drawn last
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
  let mut triangles = Triangles::default();
  push_polyline(
    &mut DrawList::default(),
    &mut triangles,
    &points,
    &style,
    look,
  );
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
      UiKind::MASK,
      Vec2::new(30.4, 10.7),
      Vec2::new(9.3, 12.6),
      glyph_image(GlyphFormat::Mask, (6, 8), |x, y| {
        [((x * 37 + y * 91) % 256) as u8, 0, 0, 0]
      }),
    ),
    (
      GlyphFormat::Color,
      UiKind::COLOR,
      Vec2::new(40.2, 12.1),
      Vec2::new(10.5, 10.5),
      glyph_image(GlyphFormat::Color, (8, 8), |x, y| {
        [(x * 30) as u8, (y * 30) as u8, 90, ((x + y) * 16) as u8]
      }),
    ),
    (
      GlyphFormat::Msdf,
      UiKind::MSDF,
      Vec2::new(12.7, 33.3),
      Vec2::new(14.0, 14.0),
      glyph_image(GlyphFormat::Msdf, (8, 8), disc),
    ),
  ];
  for (n, (format, kind, centre, size, image)) in (2..).zip(glyphs) {
    let key = GlyphKey::fill(FontId(0), n, 16.0, 0.0);
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
  let placed: Vec<_> = sprite_quads
    .iter()
    .map(|(sprite, filter, centre, size, angle)| {
      let texture = sprites
        .texture_of(sprite.id(), &crate::core::graphics::test_images())
        .expect("a test image");
      let instance = quad(*centre, *size, *angle, Color::new(255, 230, 210, 250));
      (texture, instance, *filter)
    })
    .collect();
  let textures: Vec<_> = placed.iter().map(|(texture, ..)| *texture).collect();
  sprites.prepare_textures(&device, &queue, &DrawList::default(), &textures);

  // The sprites placed in their slots as `UiBuild::fix_sprites` places them
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
  let runs: Vec<Run> = plan_runs(&needs, |need| slot_of(need, &sprites));
  assert_eq!(runs.len(), 1, "{runs:?}");
  assert!(runs[0].textures.iter().all(Option::is_some), "{runs:?}");
  camera_bound(&device, &queue, &mut batch, entity, SIDE);
  let mut gpu = UiGpu::new(&device, batch.camera_layout());
  gpu.upload(&device, &queue, &instances, &[], &[]);
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

  let image = ui_image(&device);
  let mut encoder = device.create_command_encoder(&Default::default());
  {
    let view = image.create_view(&Default::default());
    let mut pass = begin(&mut encoder, &view);
    pass.set_pipeline(gpu.pipeline());
    pass.set_bind_group(0, camera_group, &[]);
    pass.set_bind_group(2, gpu.slots_group(), &[gpu.slots_offset()]);
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
  let image = crate::core::capture::read_texture_rgba(&device, &queue, &image).expect("read back");

  let cleared = [51, 102, 153, 255];
  // Inside the first rect and the clip: drawn; inside it left of the clip:
  // untouched; the unclipped rect outside the clip: drawn
  assert_ne!(image.get_pixel(15, 14).0, cleared);
  assert_eq!(image.get_pixel(6, 14).0, cleared);
  assert_ne!(image.get_pixel(4, 44).0, cleared);
  let drawn = image.pixels().filter(|pixel| pixel.0 != cleared).count();
  assert!(drawn > 500, "{drawn} pixels drawn");
  // Outside the clip only the unclipped rect (its box, margin included,
  // reaches pixels x <= 8, y >= 41) draws
  let inside = |x: u32, y: u32| (7..41).contains(&x) && (5..43).contains(&y);
  let stray: Vec<_> = image
    .enumerate_pixels()
    .filter(|&(x, y, pixel)| !inside(x, y) && !(x <= 8 && y >= 41) && pixel.0 != cleared)
    .map(|(x, y, pixel)| ((x, y), pixel.0))
    .collect();
  assert!(
    stray.is_empty(),
    "{} pixels outside the clip drawn: {:?}",
    stray.len(),
    &stray[..stray.len().min(8)]
  );
}

/// A render target shown as UI sprites, the linear sprite body: a world
/// camera draws into it through the batch, then the UI pipeline samples it,
/// its instances fixed by `UiBuild::fix_sprites` (linear and nearest, one
/// turned and minified) and clipped. (The UI's own commands draw sprite
/// handles only, so the engine cannot show a target in its UI; this covers
/// the kind.)
#[test]
fn a_render_target_sprite_draws_with_the_linear_body() {
  let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
    return;
  };
  let ui = crate::core::render::ui::UI_CAMERA;
  let world = Entity::from_raw_u32(7).expect("an entity");
  let camera = Camera2d::default();
  let size = Vec2::splat(SIDE as f32);
  let view = CameraView::compute(size / 2.0, 0.0, &camera, Vec2::ZERO, size);
  let target_size = (20, 16);
  let seen = Vec2::new(target_size.0 as f32, target_size.1 as f32);
  let target_view = CameraView::compute(seen / 2.0, 0.0, &camera, Vec2::ZERO, seen);
  let (mut cameras, mut targetless) = (Vec::new(), Vec::new());
  plan_cameras(
    [(world, &camera, &target_view, None)].into_iter(),
    (SIDE, SIDE),
    false,
    None,
    &mut cameras,
    &mut targetless,
  );
  let scissor = PixelRect {
    x: 5,
    y: 6,
    width: 37,
    height: 35,
  };
  let clip = ClipRect::of(Some(scissor), (SIDE, SIDE));
  let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
  let mut sprites = SpriteGpuRenderer::new(&device, &queue);
  let mut targets = crate::core::graphics::RenderTargets::default();
  let target = targets.create(target_size.0, target_size.1);
  sprites.sync_targets(&device, &targets);
  let shown = sprites
    .target_texture(target)
    .expect("the target's texture");
  let mut batch = Batch2D::new(&device, SCENE_FORMAT, Default::default());
  let mut list = DrawList::default();
  let look = |filter| SpriteLook {
    shading: SpriteShading::Plain,
    blend: BlendMode::Alpha,
    filter,
    lit: false,
  };

  // What the world camera draws into the target: a turned gradient
  let gradient = gradient(5, 4);
  let texture = sprites
    .texture_of(gradient.id(), &crate::core::graphics::test_images())
    .expect("a test image");
  let instance = quad(
    Vec2::new(10.2, 7.7),
    Vec2::new(17.0, 12.5),
    0.3,
    Color::WHITE,
  );
  list.push_sprite_in(
    DrawKind::Sprite,
    0,
    texture,
    look(Filter::Linear),
    instance,
    (RenderLayers::all(), None, Some(world)),
  );

  // The target in the UI, linear and magnified, then nearest, turned and
  // minified, both clipped
  let mut build = UiBuild::default();
  for (filter, centre, size, angle) in [
    (
      Filter::Linear,
      Vec2::new(20.3, 19.6),
      Vec2::new(33.1, 25.7),
      0.0,
    ),
    (
      Filter::Nearest,
      Vec2::new(31.2, 33.4),
      Vec2::new(13.0, 9.3),
      0.4,
    ),
  ] {
    let instance = quad(centre, size, angle, Color::new(255, 235, 220, 240));
    build.sprites.push(SpriteFix {
      index: build.instances.len() as u32,
      texture: shown,
      filter,
    });
    build
      .instances
      .push(UiInstance::quad(UiKind::SPRITE, &instance, clip, false, 0));
    build.needs.push(TexNeed::Sprite(shown));
  }
  list.sorted();

  let mut uniforms = CameraUniforms::default();
  let globals = [
    (
      world,
      CameraGlobals::new(&target_view, target_size, 0.0, 0.0, false),
    ),
    (ui, CameraGlobals::new(&view, (SIDE, SIDE), 0.0, 0.0, false)),
  ];
  uniforms.sync(&device, &queue, &globals);
  batch.sync_cameras(&device, &uniforms);
  batch.prepare(&device, &queue, &list, &cameras, &mut sprites, &[shown]);
  build.fix_sprites(&sprites, Presentation::default());
  assert!(
    build
      .instances
      .iter()
      .all(|instance| instance.kind() == UiKind::SPRITE_LINEAR),
    "a render target's sprites draw with the linear body"
  );

  let runs = plan_runs(&build.needs, |need| slot_of(need, &sprites));
  assert_eq!(runs.len(), 1, "{runs:?}");
  let mut gpu = UiGpu::new(&device, batch.camera_layout());
  gpu.upload(&device, &queue, &build.instances, &[], &build.slots);
  let textures = runs[0]
    .textures
    .map(|key| key.and_then(|key| sprites.page_texture(key)));
  let group = gpu
    .bind_group(&device, textures, sprites.samplers(), 0)
    .clone();
  let camera_group = batch.camera_group(ui).expect("the camera is bound");

  let (_, target_image) = sprites.target_image(target).expect("the target's image");
  let new = ui_image(&device);
  let mut encoder = device.create_command_encoder(&Default::default());
  {
    let mut pass = begin(&mut encoder, target_image);
    for draw in batch.draws(world, false) {
      batch.draw(&mut pass, draw, world, &sprites, target_size);
    }
  }
  {
    let view = new.create_view(&Default::default());
    let mut pass = begin(&mut encoder, &view);
    pass.set_pipeline(gpu.pipeline());
    pass.set_bind_group(0, camera_group, &[]);
    pass.set_bind_group(2, gpu.slots_group(), &[gpu.slots_offset()]);
    pass.set_bind_group(1, &group, &[]);
    pass.set_vertex_buffer(0, gpu.quad().slice(..));
    pass.set_vertex_buffer(1, gpu.instances().slice(..));
    pass.draw(0..6, 0..build.instances.len() as u32);
  }
  queue.submit([encoder.finish()]);
  let error = pollster::block_on(scope.pop());
  assert!(error.is_none(), "{error:?}");
  let new = crate::core::capture::read_texture_rgba(&device, &queue, &new).expect("read back");

  let cleared = [51, 102, 153, 255];
  let drawn = new.pixels().filter(|pixel| pixel.0 != cleared).count();
  assert!(drawn > 500, "{drawn} pixels drawn");
  // The target holds the gradient, not one colour
  let colours: std::collections::HashSet<_> = new.pixels().map(|pixel| pixel.0).collect();
  assert!(colours.len() > 50, "{} colours", colours.len());
  // Outside the clip: untouched
  let outside: Vec<_> = new
    .enumerate_pixels()
    .filter(|&(x, y, _)| !(5..42).contains(&x) || !(6..41).contains(&y))
    .map(|(_, _, pixel)| pixel.0)
    .collect();
  assert!(outside.len() > 300);
  assert!(outside.iter().all(|&pixel| pixel == cleared));
}

/// One rect over the whole UI image through the UI pipeline, tinted `tint`,
/// into an image cleared to `clear`: its centre pixel
fn ui_pixel(
  device: &wgpu::Device,
  queue: &wgpu::Queue,
  clear: wgpu::Color,
  tint: [f32; 4],
) -> [u8; 4] {
  let entity = Entity::PLACEHOLDER;
  let mut batch = Batch2D::new(device, SCENE_FORMAT, Default::default());
  camera_bound(device, queue, &mut batch, entity, SIDE);
  let sprites = SpriteGpuRenderer::new(device, queue);
  // A quad far larger than the image, so the rect's edge is out of view
  let mut rect = SpriteInstance::new();
  let centre = SIDE as f32 / 2.0;
  rect.transform = [
    [1000.0, 0.0, 0.0, 0.0],
    [0.0, 1000.0, 0.0, 0.0],
    [centre, centre, 1.0, 0.0],
  ];
  rect.tint = tint;
  rect.effect_params = [0.0, 0.0, 1000.0, 1000.0];
  let clip = ClipRect::of(None, (SIDE, SIDE));
  let mut gpu = UiGpu::new(device, batch.camera_layout());
  gpu.upload(
    device,
    queue,
    &[UiInstance::quad(UiKind::RECT, &rect, clip, false, 0)],
    &[],
    &[],
  );
  let group = gpu
    .bind_group(device, [None; 4], sprites.samplers(), 0)
    .clone();
  let camera_group = batch.camera_group(entity).expect("the camera is bound");
  let image = ui_image(device);
  let mut encoder = device.create_command_encoder(&Default::default());
  {
    let view = image.create_view(&Default::default());
    let mut pass = begin_cleared(&mut encoder, &view, clear);
    pass.set_pipeline(gpu.pipeline());
    pass.set_bind_group(0, camera_group, &[]);
    pass.set_bind_group(2, gpu.slots_group(), &[gpu.slots_offset()]);
    pass.set_bind_group(1, &group, &[]);
    pass.set_vertex_buffer(0, gpu.quad().slice(..));
    pass.set_vertex_buffer(1, gpu.instances().slice(..));
    pass.draw(0..6, 0..1);
  }
  queue.submit([encoder.finish()]);
  let image = crate::core::capture::read_texture_rgba(device, queue, &image).expect("read back");
  image.get_pixel(SIDE / 2, SIDE / 2).0
}

/// The UI image holds gamma-encoded colour, premultiplied, blended in gamma
/// space as on the web
#[test]
fn the_ui_pipeline_gamma_encodes_its_output() {
  let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
    return;
  };
  let near = |got: [u8; 4], want: [u8; 4]| got.iter().zip(want).all(|(g, w)| g.abs_diff(w) <= 1);
  let clear = wgpu::Color::TRANSPARENT;
  // An opaque sRGB 0.5 grey is stored as 128, not the linear 55
  let grey = ui_pixel(&device, &queue, clear, [0.5, 0.5, 0.5, 1.0]);
  assert!(near(grey, [128, 128, 128, 255]), "grey {grey:?}");
  // 50% black over white mixes in gamma space
  let white = wgpu::Color::WHITE;
  let over = ui_pixel(&device, &queue, white, [0.0, 0.0, 0.0, 0.5]);
  assert!(near(over, [128, 128, 128, 255]), "over white {over:?}");
  // A fully transparent quad leaves the destination alone
  let kept = ui_pixel(&device, &queue, CLEAR, [1.0, 0.0, 0.0, 0.0]);
  assert!(near(kept, [51, 102, 153, 255]), "kept {kept:?}");
}

/// UI lines through the batch's line pipeline (the world's lines, drawn into
/// the UI format) and through the UI pipeline: pixel for pixel the same once
/// the batch shares no vertex between triangles
mod lines {
  use glam::Vec2;

  use super::*;
  use crate::core::render::ui::{UI_CAMERA, line_of};
  use crate::core::ui::immediate::UiCommand;

  /// The line images' size
  const IMAGE: (u32, u32) = (200, 150);

  /// The line test's lines, moved by `shift`: straight ones thin and thick at
  /// several slants, a gentle arc (10 degree turns), closed outlines (a
  /// rectangle, a hexagon, a triangle) and zigzags with long miters
  fn test_lines(shift: Vec2) -> Vec<(Vec<Vec2>, f32)> {
    let at = |points: &[(f32, f32)]| -> Vec<Vec2> {
      points
        .iter()
        .map(|&(x, y)| Vec2::new(x, y) + shift)
        .collect()
    };
    let around = |centre: (f32, f32), radius: f32, from: f32, step: f32, n: usize| {
      let points: Vec<(f32, f32)> = (0..=n)
        .map(|i| {
          let a = (from + i as f32 * step).to_radians();
          (centre.0 + a.cos() * radius, centre.1 + a.sin() * radius)
        })
        .collect();
      at(&points)
    };
    let zigzag = |x: f32, y: f32, dx: f32, dy: f32, n: usize| {
      let points: Vec<(f32, f32)> = (0..=n)
        .map(|i| (x + i as f32 * dx, y + if i % 2 == 0 { dy } else { 0.0 }))
        .collect();
      at(&points)
    };
    let mut lines = vec![
      (at(&[(6.3, 6.3), (90.6, 11.4)]), 1.0),
      (at(&[(6.5, 16.2), (90.1, 30.7)]), 1.5),
      (at(&[(100.4, 5.2), (104.9, 60.7)]), 2.0),
      (at(&[(112.2, 6.1), (150.6, 58.3)]), 3.0),
      (at(&[(160.7, 8.8), (192.0, 20.5)]), 6.0),
      (around((60.0, 70.0), 30.0, 200.0, 10.0, 14), 2.0),
      (around((140.0, 75.0), 28.0, 200.0, 10.0, 14), 5.0),
      (
        at(&[
          (8.5, 95.5),
          (58.5, 95.5),
          (58.5, 125.5),
          (8.5, 125.5),
          (8.5, 95.5),
        ]),
        2.0,
      ),
      (around((88.0, 112.0), 16.3, 7.0, 60.0, 6), 3.0),
      (around((128.0, 114.0), 14.7, -90.0, 120.0, 3), 2.0),
    ];
    lines.push((zigzag(150.0, 100.0, 9.0, 18.0, 5), 4.0));
    lines.push((zigzag(8.0, 132.0, 20.0, 14.0, 9), 2.0));
    lines
  }

  /// `lines` drawn as `ui.draw_line` makes them: through the batch with no
  /// vertex shared between its triangles (`old`), and as one UI instance a
  /// triangle through the UI pipeline (`new`)
  fn drawn(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    lines: &[(Vec<Vec2>, f32)],
  ) -> (image::RgbaImage, image::RgbaImage) {
    let (width, height) = IMAGE;
    let camera = Camera2d::default();
    let size = Vec2::new(width as f32, height as f32);
    let view = CameraView::compute(size / 2.0, 0.0, &camera, Vec2::ZERO, size);
    let (mut cameras, mut targetless) = (Vec::new(), Vec::new());
    plan_cameras(
      [(UI_CAMERA, &camera, &view, None)].into_iter(),
      IMAGE,
      false,
      None,
      &mut cameras,
      &mut targetless,
    );
    let mut sprites = SpriteGpuRenderer::new(device, queue);
    // Into the UI image's format, gamma-encoded as the UI pipeline draws
    let mut batch = Batch2D::new(device, UI_FORMAT, Default::default());
    let mut list = DrawList::default();
    let mut points = Vec::new();
    for (z, (line, width)) in (0..).zip(lines) {
      let command = UiCommand::Line {
        points: line.as_slice().into(),
        at: Vec2::ZERO,
        width: *width,
        color: Color::new(240, 236, 250, 230),
        closed: false,
        clip: None,
        layer: 0,
      };
      let (style, look) = line_of(&command, z, &mut points, None).expect("a line");
      push_polyline(&mut list, batch.triangles(), &points, &style, look);
    }
    let triangles = batch.triangles();
    let mut instances = Vec::new();
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
        ClipRect::of(None, IMAGE),
      ));
    }
    // Each index its own vertex: the same triangles, in the same order,
    // drawn by the batch's own pipeline, but sharing no vertex
    let expanded: Vec<_> = triangles
      .indices
      .iter()
      .map(|&i| triangles.vertices[i as usize])
      .collect();
    triangles.indices = (0..expanded.len() as u32).collect();
    triangles.vertices = expanded;
    list.sorted();
    let mut uniforms = CameraUniforms::default();
    let globals = CameraGlobals::new(&view, IMAGE, 0.0, 0.0, false);
    uniforms.sync(device, queue, &[(UI_CAMERA, globals)]);
    batch.sync_cameras(device, &uniforms);
    batch.prepare(device, queue, &list, &cameras, &mut sprites, &[]);

    let mut gpu = UiGpu::new(device, batch.camera_layout());
    gpu.upload(device, queue, &instances, &[], &[]);
    let group = gpu
      .bind_group(device, [None; 4], sprites.samplers(), 0)
      .clone();
    let camera_group = batch.camera_group(UI_CAMERA).expect("the camera is bound");
    let image = || {
      device.create_texture(&wgpu::TextureDescriptor {
        label: Some("UI Line Test Image"),
        size: wgpu::Extent3d {
          width,
          height,
          depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: UI_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
      })
    };
    let (old, new) = (image(), image());
    let mut encoder = device.create_command_encoder(&Default::default());
    {
      let view = old.create_view(&Default::default());
      let mut pass = begin(&mut encoder, &view);
      for draw in batch.draws(UI_CAMERA, false) {
        batch.draw(&mut pass, draw, UI_CAMERA, &sprites, IMAGE);
      }
    }
    {
      let view = new.create_view(&Default::default());
      let mut pass = begin(&mut encoder, &view);
      pass.set_pipeline(gpu.pipeline());
      pass.set_bind_group(0, camera_group, &[]);
      pass.set_bind_group(2, gpu.slots_group(), &[gpu.slots_offset()]);
      pass.set_bind_group(1, &group, &[]);
      pass.set_vertex_buffer(0, gpu.quad().slice(..));
      pass.set_vertex_buffer(1, gpu.instances().slice(..));
      pass.draw(0..6, 0..instances.len() as u32);
    }
    queue.submit([encoder.finish()]);
    let read = |texture| crate::core::capture::read_texture_rgba(device, queue, texture);
    (
      read(&old).expect("read back"),
      read(&new).expect("read back"),
    )
  }

  /// How many pixels two images differ in, and the first few: where, then
  /// each image's
  fn differ(old: &image::RgbaImage, new: &image::RgbaImage) -> (usize, Vec<String>) {
    let differ: Vec<_> = old
      .enumerate_pixels()
      .zip(new.pixels())
      .filter(|((_, _, a), b)| a != b)
      .map(|((x, y, a), b)| format!("({x}, {y}): {:?} {:?}", a.0, b.0))
      .collect();
    (differ.len(), differ.into_iter().take(8).collect())
  }

  /// The proof behind the owner's acceptance (2026-10-20) of UI line pixels
  /// differing between the paths on some GPUs (Apple): drawn by the batch's
  /// own line pipeline with no vertex shared between its triangles, every
  /// line is the UI pipeline's pixel for pixel, at every sub-pixel shift. The
  /// geometry and the arithmetic are the same; what differs, where it does,
  /// is how such a GPU takes the derivative (`fwidth`) across the triangles
  /// of an indexed mesh that share vertices, as the batch draws lines.
  #[test]
  fn lines_draw_as_the_batch_draws_them_with_no_vertex_shared() {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let cleared = [51, 102, 153, 255];
    for k in 0..16u16 {
      let shift = Vec2::new(
        f32::from(k % 4) * 0.25 + 0.0625,
        f32::from(k / 4) * 0.25 + 0.03125,
      );
      let (old, new) = drawn(&device, &queue, &test_lines(shift));
      let painted = new.pixels().filter(|pixel| pixel.0 != cleared).count();
      assert!(painted > 2000, "shift {shift}: {painted} pixels drawn");
      let (count, first) = differ(&old, &new);
      assert_eq!(
        count, 0,
        "shift {shift}: pixels (old, new) differ: {first:?}"
      );
    }
    let error = pollster::block_on(scope.pop());
    assert!(error.is_none(), "{error:?}");
  }
}

/// `build_ui`, command by command: what each command builds against the
/// shared functions that make it (`rect_instance`, `sprite_instance`,
/// `line_of` and the line builder, `text_of` and the text emitters)
mod building {
  use std::collections::HashSet;
  use std::sync::Arc;

  use glam::Vec2;

  use super::super::*;
  use crate::core::asset::{Assets, Handle};
  use crate::core::graphics::{Image, SpriteGpuRenderer};
  use crate::core::render::batch::{Filter, Triangles, push_polyline};
  use crate::core::render::text::{TextSources, TextTargets, emit_text};
  use crate::core::render::ui::{
    line_of, rect_instance, scissor_of, sprite_instance, text_of, ui_z,
  };
  use crate::core::render::ui_retain::{UiRenderEnv, UiRunCache, UiTextCache};
  use crate::core::render::{DrawList, UiSpace};
  use crate::core::text::{
    Fonts, GlyphRasterizer, Icon, PlacedIcon, TextEffects, TextEngine, TextIcons, TextLayout,
  };
  use crate::core::ui::immediate::{ScrollSlot, UiCommand, UiCommands, intersect};
  use crate::core::ui::{Color, Rect};

  /// What the text emitters write into, shared by the build and what it is
  /// checked against (one glyph atlas, so a glyph has one slot)
  struct Writes {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    sprites: SpriteGpuRenderer,
    raster: GlyphRasterizer,
    list: DrawList,
    warned: HashSet<u64>,
  }

  impl Writes {
    fn new() -> Option<Writes> {
      let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
        return None;
      };
      let sprites = SpriteGpuRenderer::new(&device, &queue);
      Some(Writes {
        device,
        queue,
        sprites,
        raster: GlyphRasterizer::new(),
        list: DrawList::default(),
        warned: HashSet::new(),
      })
    }

    fn targets(&mut self) -> TextTargets<'_> {
      TextTargets {
        raster: &mut self.raster,
        sprites: &mut self.sprites,
        device: &self.device,
        queue: &self.queue,
        list: &mut self.list,
        warned: &mut self.warned,
      }
    }
  }

  /// No text engine work happens without sources; these make empty ones
  struct NoFonts {
    fonts: Fonts,
    engine: TextEngine,
    effects: TextEffects,
    icons: TextIcons,
  }

  impl NoFonts {
    fn new() -> NoFonts {
      NoFonts {
        fonts: Fonts::new(),
        engine: TextEngine::new(),
        effects: TextEffects::default(),
        icons: TextIcons::default(),
      }
    }

    fn sources(&self) -> TextSources<'_> {
      TextSources {
        fonts: &self.fonts,
        engine: &self.engine,
        effects: &self.effects,
        icons: &self.icons,
        images: crate::core::graphics::no_images(),
      }
    }
  }

  /// A 320 x 240 UI image at `px_scale` image pixels per UI unit
  fn env_at(px_scale: f32, on_target: bool) -> UiRenderEnv {
    UiRenderEnv {
      space: UiSpace {
        size: Vec2::new(320.0, 240.0) / px_scale,
        px_scale,
        image: (320, 240),
        mouse_scale: 1.0,
      },
      on_target,
      text: true,
      fonts: 1,
      bitmaps: 0,
    }
  }

  fn env() -> UiRenderEnv {
    env_at(1.0, false)
  }

  /// A 20 x 10 rounded, bordered rect with its top-left at (x, y)
  fn rect_at(x: f32, y: f32, clip: Option<Rect>, layer: u8) -> UiCommand {
    UiCommand::Rect {
      rect: Rect::new(x, y, 20.0, 10.0),
      fill: Color::new(200, 60, 30, 230),
      radius: 3.0,
      border: Some((1.5, Color::WHITE)),
      clip,
      layer,
    }
  }

  fn line(points: &[Vec2], clip: Option<Rect>) -> UiCommand {
    UiCommand::Line {
      points: points.into(),
      at: Vec2::new(2.0, 1.0),
      width: 3.0,
      color: Color::new(250, 240, 90, 200),
      closed: false,
      clip,
      layer: 0,
    }
  }

  fn sprite(handle: &Handle<Image>, rect: Rect, filter: Filter, clip: Option<Rect>) -> UiCommand {
    UiCommand::Sprite {
      sprite: handle.clone(),
      rect,
      uv: [0.0, 0.0, 1.0, 1.0],
      tint: Color::new(255, 230, 210, 250),
      filter,
      clip,
      layer: 0,
    }
  }

  fn text(layout: &Arc<TextLayout>, origin: Vec2, clip: Option<Rect>) -> UiCommand {
    UiCommand::Text {
      layout: layout.clone(),
      origin,
      reveal: None,
      opacity: 1.0,
      seed: 3,
      clip,
      layer: 0,
    }
  }

  /// `list` in order, each command's order its index
  fn commands_of(list: impl IntoIterator<Item = UiCommand>) -> UiCommands {
    let mut commands = UiCommands::default();
    for (order, command) in (0..).zip(list) {
      commands.push(order, 0, command);
    }
    commands
  }

  /// The new path's build of `commands`, retention off, its images in the
  /// test thread's store
  fn built(
    writes: &mut Writes,
    commands: &UiCommands,
    env: UiRenderEnv,
    sources: Option<&TextSources<'_>>,
  ) -> UiBuild {
    built_with(
      writes,
      commands,
      env,
      sources,
      &crate::core::graphics::test_images(),
    )
  }

  /// [`built`], its images in `images`
  fn built_with(
    writes: &mut Writes,
    commands: &UiCommands,
    env: UiRenderEnv,
    sources: Option<&TextSources<'_>>,
    images: &Assets<Image>,
  ) -> UiBuild {
    let mut build = UiBuild::default();
    let mut texts = UiTextCache::<UiInstance>::default();
    texts.begin(env, false);
    build_ui(
      &mut build,
      commands,
      &env,
      (sources, images),
      &mut writes.targets(),
      0.0,
      &mut texts,
      &mut UiRunCache::default(),
    );
    texts.end();
    build
  }

  /// What the text emitters lay for `commands`' texts, each placed by
  /// `text_of` (as `build_ui` places them, under their clip's scissor), in
  /// the order laid
  fn laid(
    writes: &mut Writes,
    commands: &UiCommands,
    env: UiRenderEnv,
    sources: &TextSources<'_>,
  ) -> DrawList {
    writes.list.clear();
    let space = &env.space;
    for draw in commands.as_slice() {
      let command = &draw.command;
      let scissor = scissor_of(command.clip_rect(), space).expect("a clip that draws");
      let z = ui_z(command.draw_layer(), draw.order);
      if let Some((layout, place)) = text_of(command, space, z, scissor, 0.0, None) {
        emit_text(sources, &mut writes.targets(), layout, &place);
      }
    }
    std::mem::take(&mut writes.list)
  }

  /// An instance's quad centre (a quad's third lane is its translation)
  fn centre(instance: &UiInstance) -> Vec2 {
    Vec2::new(instance.data[2][0], instance.data[2][1])
  }

  #[test]
  fn commands_outside_their_clip_or_the_image_build_nothing() {
    let Some(mut writes) = Writes::new() else {
      return;
    };
    let swatch = crate::core::graphics::test_handle(Image::solid_rect(4, 4, 255, 0, 0, 255));
    let clip = Some(Rect::new(0.0, 0.0, 100.0, 50.0));
    let commands = commands_of([
      // Wholly below its clip
      rect_at(10.0, 80.0, clip, 0),
      // Wholly right of the image (320 wide)
      rect_at(340.0, 10.0, None, 0),
      // An empty clip
      rect_at(10.0, 10.0, Some(Rect::new(0.0, 0.0, 0.0, 50.0)), 0),
      // A line wholly right of its clip, a sprite wholly below the image
      line(&[Vec2::new(150.0, 10.0), Vec2::new(200.0, 40.0)], clip),
      sprite(
        &swatch,
        Rect::new(10.0, 260.0, 8.0, 8.0),
        Filter::Default,
        None,
      ),
      // Seen
      rect_at(10.0, 10.0, clip, 0),
    ]);
    let build = built(&mut writes, &commands, env(), None);
    assert_eq!(build.instances.len(), 1, "{:?}", build.instances);
    assert_eq!(build.instances[0].kind(), UiKind::RECT);
    assert_eq!(centre(&build.instances[0]), Vec2::new(20.0, 15.0));
    assert_eq!(build.needs, [TexNeed::None]);
    assert!(build.sprites.is_empty());
  }

  #[test]
  fn commands_out_of_order_are_built_in_painters_order() {
    let Some(mut writes) = Writes::new() else {
      return;
    };
    // Each rect's x is ten times its order
    let mut commands = UiCommands::default();
    commands.push(1, 0, rect_at(10.0, 100.0, None, 1));
    for order in [5, 2, 9] {
      commands.push(order, 0, rect_at(order as f32 * 10.0, 10.0, None, 0));
    }
    let build = built(&mut writes, &commands, env(), None);
    let painted: Vec<(f32, f32)> = build
      .instances
      .iter()
      .map(|instance| {
        let at = centre(instance) - Vec2::new(10.0, 5.0);
        (at.x / 10.0, at.y)
      })
      .collect();
    // Layer 0 by order, then layer 1
    assert_eq!(
      painted,
      [(2.0, 10.0), (5.0, 10.0), (9.0, 10.0), (1.0, 100.0)]
    );
  }

  #[test]
  fn at_one_order_a_line_draws_after_the_rest() {
    let Some(mut writes) = Writes::new() else {
      return;
    };
    let mut commands = UiCommands::default();
    commands.push(
      3,
      0,
      line(&[Vec2::new(5.0, 5.0), Vec2::new(60.0, 30.0)], None),
    );
    commands.push(3, 0, rect_at(10.0, 10.0, None, 0));
    let build = built(&mut writes, &commands, env(), None);
    let kinds: Vec<u32> = build.instances.iter().map(UiInstance::kind).collect();
    assert_eq!(kinds[0], UiKind::RECT);
    assert!(kinds.len() > 1 && kinds[1..].iter().all(|&kind| kind == UiKind::LINE));
  }

  /// Past the slots the shader holds, a draw in a slot is moved and
  /// clipped on the CPU, as every slot was before the shader held them
  #[test]
  fn draws_past_the_shaders_slots_build_as_the_same_draws_placed_absolute() {
    let Some(mut writes) = Writes::new() else {
      return;
    };
    let swatch = crate::core::graphics::test_handle(Image::solid_rect(4, 4, 255, 0, 0, 255));
    let slot_clip = Rect::new(0.0, 5.0, 100.0, 60.0);
    let inner = Rect::new(4.0, 2.0, 40.0, 30.0);
    // At a fractional scale: 3 and 7 render pixels are 2.4 and 5.6 units
    let env = env_at(1.25, false);
    let by = Vec2::new(3.0, 7.0) / 1.25;
    let draws = |placed: bool| {
      let at = |x: f32, y: f32| {
        if placed {
          Vec2::new(x, y) + by
        } else {
          Vec2::new(x, y)
        }
      };
      let rect = |x, y| Rect::new(at(x, y).x, at(x, y).y, 20.0, 10.0);
      let clip = |own: Option<Rect>| match (placed, own) {
        // Placed absolute: the slot's clip with the draw's own moved there
        (true, Some(own)) => Some(intersect(
          slot_clip,
          Rect::new(own.x + by.x, own.y + by.y, own.width, own.height),
        )),
        (true, None) => Some(slot_clip),
        (false, own) => own,
      };
      [
        UiCommand::Rect {
          rect: rect(0.0, 0.0),
          fill: Color::new(200, 60, 30, 230),
          radius: 3.0,
          border: Some((1.5, Color::WHITE)),
          clip: clip(None),
          layer: 0,
        },
        UiCommand::Sprite {
          sprite: swatch.clone(),
          rect: rect(10.0, 12.0),
          uv: [0.0, 0.0, 1.0, 1.0],
          tint: Color::WHITE,
          filter: Filter::Default,
          clip: clip(Some(inner)),
          layer: 0,
        },
        UiCommand::Line {
          points: [Vec2::new(5.0, 5.0), Vec2::new(60.0, 30.0)].into(),
          at: at(2.0, 1.0),
          width: 3.0,
          color: Color::WHITE,
          closed: false,
          clip: clip(None),
          layer: 0,
        },
      ]
    };
    let mut slotted = UiCommands::default();
    for _ in 1..SLOTS {
      slotted.push_slot(ScrollSlot::NONE);
    }
    let slot = slotted.push_slot(ScrollSlot {
      offset: [3, 7],
      clip: Some(slot_clip),
      cull: None,
    });
    assert_eq!(usize::from(slot), SLOTS);
    for (order, command) in (0..).zip(draws(false)) {
      slotted.push(order, slot, command);
    }
    // Past the slot table: skipped
    slotted.push(9, slot + 1, rect_at(0.0, 0.0, None, 0));
    let mut absolute = UiCommands::default();
    for (order, command) in (0..).zip(draws(true)) {
      absolute.push(order, 0, command);
    }
    let slotted = built(&mut writes, &slotted, env, None);
    let absolute = built(&mut writes, &absolute, env, None);
    assert!(absolute.instances.len() > 2, "{:?}", absolute.instances);
    assert_eq!(
      bytemuck::cast_slice::<UiInstance, u8>(&slotted.instances),
      bytemuck::cast_slice::<UiInstance, u8>(&absolute.instances),
      "data and clips"
    );
    assert_eq!(slotted.needs, absolute.needs);
    assert_eq!(slotted.sprites, absolute.sprites);
  }

  #[test]
  fn a_rect_instance_is_its_rect_quad() {
    let Some(mut writes) = Writes::new() else {
      return;
    };
    // Off the pixel grid at a fractional scale, snapped on the target or not
    for on_target in [false, true] {
      let env = env_at(1.5, on_target);
      let (rect, fill, radius) = (
        Rect::new(10.3, 7.6, 30.2, 12.9),
        Color::new(40, 160, 220, 200),
        4.0,
      );
      let border = Some((1.5, Color::new(250, 250, 250, 180)));
      let command = UiCommand::Rect {
        rect,
        fill,
        radius,
        border,
        clip: Some(Rect::new(5.0, 5.0, 30.0, 40.0)),
        layer: 0,
      };
      let commands = commands_of([command]);
      let (quad, _, _) =
        rect_instance(rect, fill, radius, border, &env.space, on_target).expect("a rect");
      let build = built(&mut writes, &commands, env, None);
      // The clip out to whole image pixels
      let clip = ClipRect {
        x0: 7,
        y0: 7,
        x1: 53,
        y1: 68,
      };
      assert_eq!(
        build.instances,
        [UiInstance::quad(UiKind::RECT, &quad, clip, false, 0)],
        "on target: {on_target}"
      );
      // Every lane, bit for bit
      assert_eq!(
        bytemuck::bytes_of(&build.instances[0].data),
        bytemuck::bytes_of(&quad)
      );
    }
  }

  #[test]
  fn a_line_becomes_its_builders_triangles() {
    let Some(mut writes) = Writes::new() else {
      return;
    };
    let points = [
      Vec2::new(10.0, 30.0),
      Vec2::new(40.5, 12.2),
      Vec2::new(70.0, 33.0),
    ];
    let command = line(&points, Some(Rect::new(0.0, 0.0, 200.0, 100.0)));
    // The line's builder's triangles, as `line_of` asks for them
    let (mut list, mut triangles, mut poly) =
      (DrawList::default(), Triangles::default(), Vec::new());
    let (style, look) = line_of(&command, ui_z(0, 0), &mut poly, None).expect("a line");
    push_polyline(&mut list, &mut triangles, &poly, &style, look);
    let clip = ClipRect {
      x0: 0,
      y0: 0,
      x1: 200,
      y1: 100,
    };
    let commands = commands_of([command]);
    let expected: Vec<UiInstance> = triangles
      .indices
      .as_chunks::<3>()
      .0
      .iter()
      .map(|triangle| {
        let vertex = |i: u32| &triangles.vertices[i as usize];
        let corner = |i: u32| {
          let v = vertex(i);
          (Vec2::from(v.position), Vec2::from(v.uv), v.color)
        };
        UiInstance::triangle(
          [
            corner(triangle[0]),
            corner(triangle[1]),
            corner(triangle[2]),
          ],
          vertex(triangle[0]).params,
          clip,
        )
      })
      .collect();
    // Two segments and the wedge at their corner
    assert!(expected.len() >= 5, "{} triangles", expected.len());
    let build = built(&mut writes, &commands, env(), None);
    assert_eq!(build.instances, expected);
    assert_eq!(build.needs, vec![TexNeed::None; expected.len()]);
  }

  #[test]
  fn sprites_and_icons_become_sprite_instances_fixed_later() {
    let Some(mut writes) = Writes::new() else {
      return;
    };
    // The swatch in a store the text's sources read too
    let mut images = Assets::new();
    let swatch = images.add(Image::solid_rect(4, 4, 255, 0, 0, 255));
    let icons = Arc::new(TextLayout {
      scale: 1.0,
      icons: vec![PlacedIcon {
        icon: Icon::Sprite {
          sprite: swatch.clone(),
          size: None,
        },
        rect: Rect::new(0.0, 0.0, 8.0, 8.0),
        cluster: 0,
      }],
      ..TextLayout::default()
    });
    let commands = commands_of([
      sprite(
        &swatch,
        Rect::new(10.3, 10.0, 16.0, 12.0),
        Filter::Nearest,
        None,
      ),
      text(&icons, Vec2::new(40.0, 10.0), None),
    ]);
    let fonts = NoFonts::new();
    let sources = TextSources {
      images: &images,
      ..fonts.sources()
    };
    // The sprite's quad, and the icon's as the text emitters lay it
    let quad = sprite_instance(
      Rect::new(10.3, 10.0, 16.0, 12.0),
      [0.0, 0.0, 1.0, 1.0],
      Color::new(255, 230, 210, 250),
    )
    .expect("a sprite");
    let list = laid(&mut writes, &commands, env(), &sources);
    let icon = list.sprite(0).expect("the icon is laid").instance;
    let texture = writes
      .sprites
      .texture_of(swatch.id(), &images)
      .expect("a loaded image");
    let build = built_with(&mut writes, &commands, env(), Some(&sources), &images);
    let clip = ClipRect::of(None, env().space.image);
    let expected = [quad, icon].map(|quad| UiInstance::quad(UiKind::SPRITE, &quad, clip, false, 0));
    assert_eq!(build.instances, expected);
    assert_eq!(build.needs, [TexNeed::Sprite(texture); 2]);
    assert_eq!(
      build.sprites,
      [
        SpriteFix {
          index: 0,
          texture,
          filter: Filter::Nearest
        },
        SpriteFix {
          index: 1,
          texture,
          filter: Filter::Default
        },
      ]
    );
  }

  #[test]
  fn sprites_are_placed_in_their_slots_and_those_without_one_dropped() {
    use crate::core::graphics::{
      Presentation, RenderTargets, SpriteInstance, TextureId, place_in_slot,
    };
    let Some(mut writes) = Writes::new() else {
      return;
    };
    let swatch = crate::core::graphics::test_handle(Image::solid_rect(4, 4, 255, 0, 0, 255));
    let commands = commands_of([
      sprite(
        &swatch,
        Rect::new(10.0, 10.0, 16.0, 12.0),
        Filter::Nearest,
        None,
      ),
      sprite(
        &swatch,
        Rect::new(40.0, 10.0, 16.0, 12.0),
        Filter::Linear,
        None,
      ),
    ]);
    let mut build = built(&mut writes, &commands, env(), None);
    let texture = writes
      .sprites
      .texture_of(swatch.id(), &crate::core::graphics::test_images())
      .expect("a test image");
    // Two more sprites: one of a texture with no slot, one of a render
    // target's
    let mut targets = RenderTargets::default();
    let target = targets.create(8, 8);
    writes.sprites.sync_targets(&writes.device, &targets);
    let target = writes.sprites.target_texture(target).expect("the target");
    let unknown = TextureId::for_test(9_999);
    for texture in [unknown, target] {
      let index = build.instances.len() as u32;
      build.instances.push(build.instances[0]);
      build.needs.push(TexNeed::Sprite(texture));
      build.sprites.push(SpriteFix {
        index,
        texture,
        filter: Filter::Default,
      });
    }
    let before: Vec<SpriteInstance> = build
      .instances
      .iter()
      .map(|instance| bytemuck::cast(instance.data))
      .collect();
    let list = DrawList::default();
    let resident = build.sprite_textures().to_vec();
    assert_eq!(resident, [texture; 2]);
    writes
      .sprites
      .prepare_textures(&writes.device, &writes.queue, &list, &resident);
    build.fix_sprites(&writes.sprites, Presentation::default());

    // The swatch's placed as the batch places them, with their samplers
    let (slot, size) = writes.sprites.slot(texture).expect("resident");
    let clip = ClipRect::of(None, env().space.image);
    for (index, nearest) in [(0, true), (1, false)] {
      let placed = place_in_slot(&before[index], &slot, size, false);
      let expected = UiInstance::quad(UiKind::SPRITE, &placed, clip, nearest, 0);
      assert_eq!(build.instances[index], expected, "sprite {index}");
    }
    // The one without a slot dropped, the target's drawn linear
    assert_eq!(build.instances.len(), 3);
    assert_eq!(build.instances[2].kind(), UiKind::SPRITE_LINEAR);
    assert_eq!(
      build.needs,
      [
        TexNeed::Sprite(texture),
        TexNeed::Sprite(texture),
        TexNeed::Sprite(target)
      ]
    );
    assert!(build.sprites.is_empty(), "each fixed once");
  }

  #[test]
  fn sprite_textures_are_made_resident_in_the_order_recorded() {
    let Some(mut writes) = Writes::new() else {
      return;
    };
    let [a, b] =
      [(); 2].map(|()| crate::core::graphics::test_handle(Image::solid_rect(4, 4, 255, 0, 0, 255)));
    let on_layer = |handle: &Handle<Image>, x: f32, layer: u8| match sprite(
      handle,
      Rect::new(x, 10.0, 16.0, 12.0),
      Filter::Default,
      None,
    ) {
      UiCommand::Sprite {
        sprite,
        rect,
        uv,
        tint,
        filter,
        clip,
        ..
      } => UiCommand::Sprite {
        sprite,
        rect,
        uv,
        tint,
        filter,
        clip,
        layer,
      },
      other => other,
    };
    // Recorded a (above), then b: painted b, then a; a culled one (off the
    // image) is still made resident
    let commands = commands_of([
      on_layer(&a, 10.0, 1),
      on_layer(&b, 40.0, 0),
      on_layer(&a, 900.0, 0),
    ]);
    let mut build = built(&mut writes, &commands, env(), None);
    let (a, b) = (
      writes
        .sprites
        .texture_of(a.id(), &crate::core::graphics::test_images())
        .expect("a test image"),
      writes
        .sprites
        .texture_of(b.id(), &crate::core::graphics::test_images())
        .expect("a test image"),
    );
    assert_eq!(build.needs, [TexNeed::Sprite(b), TexNeed::Sprite(a)]);
    assert_eq!(build.sprite_textures(), [a, b, a]);
    // A frame keeping the UI image marks each once
    assert_eq!(build.shown_textures(), [a, b]);
  }

  /// Built-in fonts: texts as glyphs
  #[cfg(feature = "default-fonts")]
  mod texts {
    use super::*;
    use crate::core::graphics::GlyphPage;
    use crate::core::render::PixelRect;
    use crate::core::render::batch::Sampling;
    use crate::core::text::{
      FontRender, FontRole, GlyphFormat, RichText, TextLayoutOptions, TextStyle,
    };

    /// The built-in fonts, and a text engine synced to them
    struct WithFonts(NoFonts);

    impl WithFonts {
      fn new() -> WithFonts {
        let mut sources = NoFonts::new();
        sources.fonts = Fonts::with_builtins();
        sources.engine.sync(&sources.fonts);
        WithFonts(sources)
      }

      fn laid(&mut self, text: &str) -> Arc<TextLayout> {
        self
          .0
          .engine
          .layout(
            &RichText::new(text),
            &TextStyle::default(),
            &TextLayoutOptions::default(),
            1.0,
          )
          .expect("laid out")
      }
    }

    /// The glyphs the text emitters lay for `commands`, as the instances
    /// they become
    fn laid_glyphs(
      writes: &mut Writes,
      commands: &UiCommands,
      sources: &TextSources<'_>,
      clip: ClipRect,
    ) -> (Vec<UiInstance>, Vec<TexNeed>, Vec<(Vec2, Vec2)>) {
      let list = laid(writes, commands, env(), sources);
      let glyphs = list.glyphs_from(0);
      assert!(!glyphs.is_empty());
      assert!(
        glyphs
          .iter()
          .all(|glyph| glyph.page.format == GlyphFormat::Mask)
      );
      let instances = glyphs
        .iter()
        .map(|glyph| {
          let nearest = glyph.sampling == Sampling::Nearest;
          UiInstance::quad(
            UiKind::MASK,
            &glyph.instance,
            clip,
            nearest,
            glyph.page.index,
          )
        })
        .collect();
      let needs = glyphs
        .iter()
        .map(|glyph| TexNeed::Glyph(glyph.page))
        .collect();
      let boxes = glyphs.iter().map(|glyph| (glyph.min, glyph.max)).collect();
      (instances, needs, boxes)
    }

    /// `commands` built `frames` times through one text cache, retention on
    fn rebuilt(
      writes: &mut Writes,
      commands: &UiCommands,
      sources: &TextSources<'_>,
      frames: usize,
    ) -> (Vec<UiBuild>, UiTextCache<UiInstance>) {
      let mut texts = UiTextCache::<UiInstance>::default();
      let builds = (0..frames)
        .map(|_| {
          let mut build = UiBuild::default();
          texts.begin(env(), true);
          build_ui(
            &mut build,
            commands,
            &env(),
            (Some(sources), &crate::core::graphics::test_images()),
            &mut writes.targets(),
            0.0,
            &mut texts,
            &mut UiRunCache::default(),
          );
          texts.end();
          build
        })
        .collect();
      (builds, texts)
    }

    #[test]
    fn a_text_is_its_laid_glyphs_then_one_copy_of_them() {
      let Some(mut writes) = Writes::new() else {
        return;
      };
      let mut fonts = WithFonts::new();
      let layout = fonts.laid("Settings 42");
      let commands = commands_of([text(&layout, Vec2::new(12.3, 20.0), None)]);
      let sources = fonts.0.sources();
      let clip = ClipRect::of(None, env().space.image);
      let (instances, needs, _) = laid_glyphs(&mut writes, &commands, &sources, clip);
      let (builds, texts) = rebuilt(&mut writes, &commands, &sources, 2);
      for (frame, build) in builds.iter().enumerate() {
        assert_eq!(build.instances, instances, "frame {frame}");
        assert_eq!(build.needs, needs, "frame {frame}");
        assert!(build.sprites.is_empty());
      }
      assert_eq!(
        (texts.built(), texts.reused()),
        (0, 1),
        "the second frame reuses the text"
      );
    }

    #[test]
    fn a_text_builds_only_the_glyphs_that_meet_its_clip() {
      let Some(mut writes) = Writes::new() else {
        return;
      };
      let mut fonts = WithFonts::new();
      let layout = fonts.laid("A label cut by its clip");
      let cut = Rect::new(0.0, 0.0, 40.0, 240.0);
      let commands = commands_of([text(&layout, Vec2::new(10.0, 20.0), Some(cut))]);
      let sources = fonts.0.sources();
      let clip = ClipRect::of(
        Some(PixelRect {
          x: 0,
          y: 0,
          width: 40,
          height: 240,
        }),
        env().space.image,
      );
      let (instances, needs, boxes) = laid_glyphs(&mut writes, &commands, &sources, clip);
      // A glyph is kept when its box comes within a pixel of the clip
      let meets = |(min, _): &(Vec2, Vec2)| min.x - 1.0 < 40.0;
      let kept: Vec<UiInstance> = instances
        .iter()
        .zip(&boxes)
        .filter(|(_, b)| meets(b))
        .map(|(instance, _)| *instance)
        .collect();
      let kept_needs: Vec<TexNeed> = needs
        .iter()
        .zip(&boxes)
        .filter(|(_, b)| meets(b))
        .map(|(need, _)| *need)
        .collect();
      assert!(!kept.is_empty() && kept.len() < instances.len());
      let (builds, texts) = rebuilt(&mut writes, &commands, &sources, 2);
      for (frame, build) in builds.iter().enumerate() {
        assert_eq!(build.instances, kept, "frame {frame}");
        assert_eq!(build.needs, kept_needs, "frame {frame}");
      }
      assert_eq!(texts.reused(), 1);
    }

    #[test]
    fn a_text_outside_the_image_builds_nothing_and_is_still_reused() {
      let Some(mut writes) = Writes::new() else {
        return;
      };
      let mut fonts = WithFonts::new();
      let layout = fonts.laid("Far away");
      let commands = commands_of([text(&layout, Vec2::new(10.0, 400.0), None)]);
      let sources = fonts.0.sources();
      let (builds, texts) = rebuilt(&mut writes, &commands, &sources, 2);
      assert!(builds.iter().all(|build| build.instances.is_empty()));
      assert_eq!((texts.built(), texts.reused()), (0, 1));
    }

    /// `text` laid out at `scale` (as the UI lays it out at its own), with
    /// an outline and a shadow, so a glyph is three quads from two kinds of
    /// image
    fn outlined(fonts: &mut WithFonts, text: &str, scale: f32) -> Arc<TextLayout> {
      let style = TextStyle::default()
        .size(15.0)
        .outline(1.0, Color::BLACK)
        .shadow(Vec2::new(1.0, 1.0), Color::new(0, 0, 0, 160), 0.0);
      fonts
        .0
        .engine
        .layout(
          &RichText::new(text),
          &style,
          &TextLayoutOptions::default(),
          scale,
        )
        .expect("laid out")
    }

    /// Each of `frames` built in turn through one text cache, retention on,
    /// with the texts the frame built and reused
    fn retained(
      writes: &mut Writes,
      frames: &[UiCommands],
      sources: &TextSources<'_>,
      env: UiRenderEnv,
    ) -> Vec<(UiBuild, (u32, u32))> {
      let mut texts = UiTextCache::<UiInstance>::default();
      frames
        .iter()
        .map(|commands| {
          let mut build = UiBuild::default();
          texts.begin(env, true);
          build_ui(
            &mut build,
            commands,
            &env,
            (Some(sources), &crate::core::graphics::test_images()),
            &mut writes.targets(),
            0.0,
            &mut texts,
            &mut UiRunCache::default(),
          );
          texts.end();
          (build, (texts.built(), texts.reused()))
        })
        .collect()
    }

    /// Whether `build` is `fresh`'s instances bit for bit (0.0 and -0.0
    /// differ), sampling the same
    fn same_bits(build: &UiBuild, fresh: &UiBuild) -> bool {
      bytemuck::cast_slice::<UiInstance, u8>(&build.instances)
        == bytemuck::cast_slice::<UiInstance, u8>(&fresh.instances)
        && build.needs == fresh.needs
    }

    /// The render modes the moved-text tests run in: raster (on the pixel
    /// grid, its subpixel bins picked by the pen), and MSDF (scaled, off
    /// the grid)
    fn with_render(msdf: bool) -> WithFonts {
      let mut fonts = WithFonts::new();
      if msdf {
        let ui = fonts.0.fonts.role(FontRole::Ui).expect("a UI font");
        fonts.0.fonts.set_render(ui, FontRender::msdf());
        fonts.0.engine.sync(&fonts.0.fonts);
      }
      fonts
    }

    #[test]
    fn a_moved_text_is_reused_as_a_fresh_build_at_its_new_place_lays_it() {
      let Some(mut writes) = Writes::new() else {
        return;
      };
      for msdf in [false, true] {
        let mut fonts = with_render(msdf);
        // Image pixels per UI unit: whole, and not a power of two
        for px_scale in [1.0, 1.5, 1.25] {
          let env = env_at(px_scale, false);
          // Two lines, so each line's baseline rounds on its own
          let layout = outlined(&mut fonts, "Settings 42\nVolume, 7%", px_scale);
          // A clip cutting some glyphs at some places
          let clip = Some(Rect::new(
            4.0 / px_scale,
            30.0 / px_scale,
            120.0 / px_scale,
            70.0 / px_scale,
          ));
          let from = Vec2::new(12.3, 40.2) / px_scale;
          let a_pixel = 1.0 / px_scale;
          // Moves down and up by fractions of a pixel, by whole pixels,
          // across the clip's edge, far (an offset carrying float error),
          // and right by whole pixels
          let moves = [
            (Vec2::new(0.0, 0.37), true),
            (Vec2::new(0.0, -13.71), true),
            (Vec2::new(0.0, 3.0 * a_pixel), true),
            (Vec2::new(0.0, 35.5), true),
            (
              Vec2::new(0.0, (from.y + 100_000.3) - 100_000.0 - from.y),
              true,
            ),
            (Vec2::new(5.0 * a_pixel, 0.0), false),
            (Vec2::new(5.0 * a_pixel, -7.43), false),
          ];
          let sources = fonts.0.sources();
          for (by, vertical) in moves {
            let to = from + by;
            let frames = [from, to].map(|at| commands_of([text(&layout, at, clip)]));
            let builds = retained(&mut writes, &frames, &sources, env);
            let fresh = built(&mut writes, &frames[1], env, Some(&sources));
            let case = format!("msdf {msdf}, scale {px_scale}, moved by {by}");
            assert!(!fresh.instances.is_empty(), "{case}: the text draws");
            assert!(
              same_bits(&builds[1].0, &fresh),
              "{case}: the moved text is what a fresh build lays"
            );
            // A vertical move never changes a glyph's subpixel bin, nor does
            // any move of an MSDF glyph (it has none); on scale 1's grid
            // neither does this text's whole-pixel move along x, which
            // `seeds_fit` checks
            if vertical || msdf || px_scale == 1.0 {
              assert_eq!(builds[1].1, (0, 1), "{case}: reused");
            }
          }
        }
      }
    }

    #[test]
    fn a_text_moved_into_other_subpixel_bins_is_built_again() {
      let Some(mut writes) = Writes::new() else {
        return;
      };
      let mut fonts = with_render(false);
      let layout = outlined(&mut fonts, "Settings 42", 1.0);
      let sources = fonts.0.sources();
      // A quarter pixel moves every pen to the next bin
      let frames = [Vec2::new(12.0, 20.0), Vec2::new(12.25, 20.0)]
        .map(|at| commands_of([text(&layout, at, None)]));
      let builds = retained(&mut writes, &frames, &sources, env());
      let fresh = built(&mut writes, &frames[1], env(), Some(&sources));
      assert!(same_bits(&builds[1].0, &fresh));
      assert_eq!(builds[1].1, (1, 0), "built again");
    }

    /// `frames` built in turn through `texts`, retention on, at `env`,
    /// with `between` run before each frame: each frame's build, and the
    /// texts it built and reused
    fn through(
      writes: &mut Writes,
      texts: &mut UiTextCache<UiInstance>,
      frames: &[UiCommands],
      sources: &TextSources<'_>,
      env: UiRenderEnv,
      mut between: impl FnMut(usize, &mut Writes),
    ) -> Vec<(UiBuild, (u32, u32))> {
      frames
        .iter()
        .enumerate()
        .map(|(frame, commands)| {
          between(frame, writes);
          let mut build = UiBuild::default();
          texts.begin(env, true);
          build_ui(
            &mut build,
            commands,
            &env,
            (Some(sources), &crate::core::graphics::test_images()),
            &mut writes.targets(),
            0.0,
            texts,
            &mut UiRunCache::default(),
          );
          texts.end();
          (build, (texts.built(), texts.reused()))
        })
        .collect()
    }

    #[test]
    fn a_text_kept_where_it_drew_nothing_checks_its_pages_where_it_draws() {
      let Some(mut writes) = Writes::new() else {
        return;
      };
      let mut fonts = WithFonts::new();
      let layout = fonts.laid("Settings 42");
      let sources = fonts.0.sources();
      // Kept where no glyph lands (its centre not a number), then drawn
      // where every glyph does; its glyph page cleared before the last frame
      let nowhere = || commands_of([text(&layout, Vec2::new(12.0, f32::NAN), None)]);
      let here = || commands_of([text(&layout, Vec2::new(12.0, 20.0), None)]);
      let pages: Vec<GlyphPage> = built(&mut writes, &here(), env(), Some(&sources))
        .needs
        .iter()
        .filter_map(|need| match need {
          TexNeed::Glyph(page) => Some(*page),
          _ => None,
        })
        .collect();
      assert!(!pages.is_empty());
      let frames = [nowhere(), here(), nowhere(), here()];
      let mut texts = UiTextCache::<UiInstance>::default();
      let clear = |frame: usize, writes: &mut Writes| {
        if frame == 3 {
          for &page in &pages {
            writes.sprites.clear_glyph_page(page);
          }
        }
      };
      let builds = through(&mut writes, &mut texts, &frames, &sources, env(), clear);
      assert!(builds[0].0.instances.is_empty(), "nothing lands");
      assert_eq!(builds[1].1, (0, 1), "laid where it draws");
      // The pages its glyphs were fetched from are checked, though none
      // landed where it was built: cleared, the text is built again
      assert_eq!(builds[3].1, (1, 0), "built again on a cleared page");
      let fresh = built(&mut writes, &here(), env(), Some(&sources));
      assert!(same_bits(&builds[1].0, &fresh));
      assert!(same_bits(&builds[3].0, &fresh));
    }

    #[test]
    fn a_text_stopped_at_a_new_place_is_kept_there() {
      let Some(mut writes) = Writes::new() else {
        return;
      };
      let mut fonts = WithFonts::new();
      let still = fonts.laid("Title");
      let moving = fonts.laid("A label cut by its clip");
      let sources = fonts.0.sources();
      // At `b` the clip cuts the moving text, so what is kept there differs
      // from what was kept where it was built
      let clip = Some(Rect::new(0.0, 0.0, 120.0, 240.0));
      let at = |x: f32| {
        commands_of([
          text(&still, Vec2::new(8.0, 4.0), None),
          text(&moving, Vec2::new(x, 40.5), clip),
        ])
      };
      // Built at a; laid at b; laid and kept there; copied; laid at a
      let (a, b) = (10.0, 70.0);
      let frames = [a, b, b, b, a].map(at);
      let mut texts = UiTextCache::<UiInstance>::default();
      let builds = through(&mut writes, &mut texts, &frames, &sources, env(), |_, _| {});
      let counts: Vec<(u32, u32)> = builds.iter().map(|(_, counts)| *counts).collect();
      assert_eq!(counts, [(2, 0), (0, 2), (0, 2), (0, 2), (0, 2)]);
      for (frame, (build, _)) in builds.iter().enumerate() {
        let fresh = built(&mut writes, &frames[frame], env(), Some(&sources));
        assert!(same_bits(build, &fresh), "frame {frame}");
      }
    }

    #[test]
    fn a_layout_drawn_twice_keeps_an_entry_for_each() {
      let Some(mut writes) = Writes::new() else {
        return;
      };
      let mut fonts = WithFonts::new();
      let layout = fonts.laid("On");
      let sources = fonts.0.sources();
      let commands = commands_of([
        text(&layout, Vec2::new(10.0, 20.0), None),
        text(&layout, Vec2::new(10.0, 50.0), None),
      ]);
      let (builds, texts) = rebuilt(&mut writes, &commands, &sources, 3);
      let fresh = built(&mut writes, &commands, env(), Some(&sources));
      assert!(builds.iter().all(|build| same_bits(build, &fresh)));
      assert_eq!((texts.built(), texts.reused()), (0, 2));
      assert_eq!(texts.entries(), 2, "each drawing of the layout is kept");
    }

    #[test]
    fn texts_moving_every_frame_are_never_built_again() {
      let Some(mut writes) = Writes::new() else {
        return;
      };
      let mut fonts = with_render(false);
      let env = env_at(1.25, false);
      let row = outlined(&mut fonts, "Setting 7", 1.25);
      let title = outlined(&mut fonts, "Settings", 1.25);
      let clip = Some(Rect::new(0.0, 40.0, 200.0, 120.0));
      // A list easing as a wheel turns it: fractional offsets every frame.
      // One layout drawn twice (two rows), and a still title
      let frames: Vec<UiCommands> = (0..40)
        .map(|frame| {
          let t = frame as f32 / 39.0;
          let offset = -95.3 * (1.0 - (1.0 - t).powi(3));
          commands_of([
            text(&title, Vec2::new(10.0, 8.0), None),
            text(&row, Vec2::new(10.0, 60.0 + offset), clip),
            text(&row, Vec2::new(10.0, 81.7 + offset), clip),
          ])
        })
        .collect();
      let sources = fonts.0.sources();
      let builds = retained(&mut writes, &frames, &sources, env);
      for (frame, (build, counts)) in builds.iter().enumerate() {
        let fresh = built(&mut writes, &frames[frame], env, Some(&sources));
        assert!(same_bits(build, &fresh), "frame {frame}");
        if frame > 0 {
          assert_eq!(*counts, (0, 3), "frame {frame}: every text reused");
        }
      }
    }
  }

  /// Draws in scroll slots through the GPU: the vertex shader moves them by
  /// their slot's offset and the clip comes from the slot, where Task 2 moved
  /// and clipped them on the CPU (`UiDraw::placed`). The two must draw the
  /// same pixels.
  mod slots {
    use super::*;
    use crate::core::graphics::Presentation;
    use crate::core::graphics::camera_gpu::{CameraGlobals, CameraUniforms};
    use crate::core::render::batch::Batch2D;
    use crate::core::render::ui::{UI_CAMERA, ui_view};
    use crate::core::render::{SCENE_FORMAT, UI_FORMAT};

    /// What the UI image holds before the draws (`super::super::CLEAR`)
    const CLEARED: [u8; 4] = [51, 102, 153, 255];

    /// A UI image of `image` pixels at `px_scale` image pixels per UI unit
    fn env_sized(px_scale: f32, image: (u32, u32), on_target: bool) -> UiRenderEnv {
      UiRenderEnv {
        space: UiSpace {
          size: Vec2::new(image.0 as f32, image.1 as f32) / px_scale,
          px_scale,
          image,
          mouse_scale: 1.0,
        },
        on_target,
        text: true,
        fonts: 1,
        bitmaps: 0,
      }
    }

    /// `commands` drawn through the UI renderer (built, its sprites made
    /// resident, finished and drawn as the `ui` pass draws it) into a UI
    /// image of `env`'s size, read back
    fn drawn(
      writes: &mut Writes,
      commands: &UiCommands,
      env: UiRenderEnv,
      sources: Option<&TextSources<'_>>,
    ) -> image::RgbaImage {
      let (device, queue) = (Arc::clone(&writes.device), Arc::clone(&writes.queue));
      let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
      let mut batch = Batch2D::new(&device, SCENE_FORMAT, Default::default());
      let mut uniforms = CameraUniforms::default();
      let globals = CameraGlobals::new(
        &ui_view(&env.space),
        env.space.image,
        0.0,
        0.0,
        env.on_target,
      );
      uniforms.sync(&device, &queue, &[(UI_CAMERA, globals)]);
      batch.sync_cameras(&device, &uniforms);
      let mut ui = UiRenderer::new(&device, batch.camera_layout());
      ui.build(
        commands,
        env,
        false,
        (sources, &crate::core::graphics::test_images()),
        &mut writes.targets(),
        0.0,
      );
      let textures = ui.sprite_textures().to_vec();
      let list = DrawList::default();
      writes
        .sprites
        .prepare_textures(&device, &queue, &list, &textures);
      let camera = batch.camera_group(UI_CAMERA).expect("the camera is bound");
      let frame = ui.finish(
        &device,
        &queue,
        &writes.sprites,
        Presentation::default(),
        camera,
      );
      let (width, height) = env.space.image;
      let image = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("UI Slot Test Image"),
        size: wgpu::Extent3d {
          width,
          height,
          depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: UI_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
      });
      let mut encoder = device.create_command_encoder(&Default::default());
      {
        let view = image.create_view(&Default::default());
        let mut pass = super::super::begin(&mut encoder, &view);
        frame.draw(&mut pass);
      }
      queue.submit([encoder.finish()]);
      let error = pollster::block_on(scope.pop());
      assert!(error.is_none(), "{error:?}");
      crate::core::capture::read_texture_rgba(&device, &queue, &image).expect("read back")
    }

    /// The draws as Task 2 drew them, on the CPU: each moved by its slot's
    /// offset, under its slot's clip and its own (`UiDraw::placed`), in
    /// slot 0
    fn placed(commands: &UiCommands, px_scale: f32) -> UiCommands {
      let mut absolute = UiCommands::default();
      for draw in commands.as_slice() {
        if let Some(command) = draw.placed(commands.slots(), px_scale) {
          absolute.push(draw.order, 0, command.into_owned());
        }
      }
      absolute
    }

    /// How many pixels two images differ in, and the first few: where, then
    /// each image's
    fn differ(a: &image::RgbaImage, b: &image::RgbaImage) -> (usize, Vec<String>) {
      let differ: Vec<_> = a
        .enumerate_pixels()
        .zip(b.pixels())
        .filter(|((_, _, a), b)| a != b)
        .map(|((x, y, a), b)| format!("({x}, {y}): {:?} {:?}", a.0, b.0))
        .collect();
      (differ.len(), differ.into_iter().take(8).collect())
    }

    fn painted(image: &image::RgbaImage) -> usize {
      image.pixels().filter(|pixel| pixel.0 != CLEARED).count()
    }

    #[test]
    fn a_rect_in_a_slot_draws_where_the_slots_offset_puts_it() {
      let Some(mut writes) = Writes::new() else {
        return;
      };
      let env = env_sized(1.0, (64, 64), false);
      let square = |x: f32, y: f32| UiCommand::Rect {
        rect: Rect::new(x, y, 20.0, 20.0),
        fill: Color::new(200, 60, 30, 255),
        radius: 0.0,
        border: None,
        clip: None,
        layer: 0,
      };
      let in_slot = |clip: Rect| {
        let mut commands = UiCommands::default();
        let slot = commands.push_slot(ScrollSlot {
          offset: [5, 9],
          clip: Some(clip),
          cull: None,
        });
        commands.push(0, slot, square(0.0, 0.0));
        commands
      };
      let slotted = drawn(
        &mut writes,
        &in_slot(Rect::new(0.0, 0.0, 64.0, 64.0)),
        env,
        None,
      );
      let absolute = drawn(&mut writes, &commands_of([square(5.0, 9.0)]), env, None);
      assert_eq!(painted(&absolute), 400);
      assert_eq!(differ(&slotted, &absolute), (0, Vec::new()));
      // The slot's clip cuts it to the pixels inside
      let cut = drawn(
        &mut writes,
        &in_slot(Rect::new(0.0, 0.0, 10.0, 10.0)),
        env,
        None,
      );
      let inside: Vec<(u32, u32)> = cut
        .enumerate_pixels()
        .filter(|(_, _, pixel)| pixel.0 != CLEARED)
        .map(|(x, y, _)| (x, y))
        .collect();
      assert_eq!(inside, (5..10).map(|x| (x, 9)).collect::<Vec<_>>());
    }

    /// A slot's clip is tested where the slot draws its contents: what its
    /// offset moves into the clip draws (the shader collapses only what it
    /// moves wholly outside), and what it moves out does not
    #[test]
    fn a_slot_draws_what_its_offset_moves_into_its_clip() {
      let Some(mut writes) = Writes::new() else {
        return;
      };
      let env = env_sized(1.0, (64, 64), false);
      let draws = |at: f32| {
        let square = UiCommand::Rect {
          rect: Rect::new(at, at, 16.0, 16.0),
          fill: Color::new(200, 60, 30, 255),
          radius: 0.0,
          border: None,
          clip: None,
          layer: 0,
        };
        let points = [Vec2::new(at, at + 18.0), Vec2::new(at + 18.0, at + 20.0)];
        let line = UiCommand::Line {
          points: points.as_slice().into(),
          at: Vec2::ZERO,
          width: 2.0,
          color: Color::WHITE,
          closed: false,
          clip: None,
          layer: 0,
        };
        [square, line]
      };
      let in_slot = |at: f32, offset: [i32; 2]| {
        let mut commands = UiCommands::default();
        let slot = commands.push_slot(ScrollSlot {
          offset,
          clip: Some(Rect::new(36.0, 36.0, 28.0, 28.0)),
          cull: None,
        });
        for (order, command) in (0..).zip(draws(at)) {
          commands.push(order, slot, command);
        }
        commands
      };
      // Outside the clip where they are recorded, moved inside it
      let moved_in = in_slot(2.0, [38, 38]);
      let slotted = drawn(&mut writes, &moved_in, env, None);
      let absolute = drawn(&mut writes, &placed(&moved_in, 1.0), env, None);
      assert!(painted(&absolute) > 280, "{} pixels", painted(&absolute));
      assert_eq!(differ(&slotted, &absolute), (0, Vec::new()));
      // Inside it where they are recorded, moved out of it
      let moved_out = drawn(&mut writes, &in_slot(40.0, [-38, -38]), env, None);
      assert_eq!(painted(&moved_out), 0);
    }

    /// Draws as a scroll area's contents are recorded, relative to its slot
    /// at `px_scale`: rows of rounded, bordered rects on whole render pixels
    /// (where resolve puts nodes in a slot), each with an icon and a label,
    /// with `field` a text field (its frame and text under its view's clip,
    /// relative to the slot), and a custom widget's draws off the pixel grid
    /// (a rect, a line and a sprite)
    fn contents(
      px_scale: f32,
      swatch: &Handle<Image>,
      label: Option<&Arc<TextLayout>>,
      field: bool,
    ) -> Vec<UiCommand> {
      let px = |k: i32| k as f32 / px_scale;
      let row = |rect: Rect, clip: Option<Rect>| UiCommand::Rect {
        rect,
        fill: Color::new(40, 160, 220, 200),
        radius: 4.0,
        border: Some((1.5, Color::WHITE)),
        clip,
        layer: 0,
      };
      let mut draws = Vec::new();
      for k in 0..6 {
        let y = 4 + 34 * k;
        draws.push(row(Rect::new(px(6), px(y), px(180), px(30)), None));
        draws.push(sprite(
          swatch,
          Rect::new(px(10), px(y + 7), px(16), px(16)),
          Filter::Default,
          None,
        ));
        if let Some(label) = label {
          draws.push(text(label, Vec2::new(px(32), px(y + 8)), None));
        }
      }
      if field {
        let view = Some(Rect::new(px(100), px(40), px(70), px(22)));
        draws.push(row(Rect::new(px(96), px(36), px(90), px(30)), view));
        if let Some(label) = label {
          draws.push(text(label, Vec2::new(px(92), px(44)), view));
        }
      }
      draws.push(row(Rect::new(20.3, 61.7, 30.2, 12.9), None));
      draws.push(line(
        &[
          Vec2::new(5.0, 5.0),
          Vec2::new(60.0, 30.0),
          Vec2::new(90.0, 12.5),
        ],
        None,
      ));
      draws.push(sprite(
        swatch,
        Rect::new(140.4, 20.6, 13.3, 9.8),
        Filter::Linear,
        None,
      ));
      draws
    }

    /// A page at `px_scale`: a panel in slot 0, a scroll area's contents in
    /// slot 1 (its view from (20, 16) image pixels, drawn at `offset`), and
    /// a strip nested in it in slot 2, drawn at `offset` moved by (30, 12)
    fn page(
      px_scale: f32,
      offset: [i32; 2],
      swatch: &Handle<Image>,
      label: Option<&Arc<TextLayout>>,
      field: bool,
    ) -> UiCommands {
      let px = |k: i32| k as f32 / px_scale;
      let mut commands = UiCommands::default();
      let mut order = 0;
      let mut push = |commands: &mut UiCommands, slot: u16, command: UiCommand| {
        commands.push(order, slot, command);
        order += 1;
      };
      push(&mut commands, 0, rect_at(px(4), px(2), None, 0));
      let area = commands.push_slot(ScrollSlot {
        offset,
        clip: Some(Rect::new(px(20), px(16), px(200), px(150))),
        cull: None,
      });
      for command in contents(px_scale, swatch, label, field) {
        push(&mut commands, area, command);
      }
      let strip = commands.push_slot(ScrollSlot {
        offset: [offset[0] + 30, offset[1] + 12],
        clip: Some(Rect::new(px(60), px(40), px(120), px(70))),
        cull: None,
      });
      for command in contents(px_scale, swatch, label, field).into_iter().take(9) {
        push(&mut commands, strip, command);
      }
      push(&mut commands, 0, rect_at(px(230), px(180), None, 0));
      commands
    }

    /// Fractions of a pixel draws sit at off the grid, halves among them
    const FRACTIONS: [f32; 10] = [0.0, 0.25, 0.5, 0.75, 0.1, 0.3, 0.6, 0.9, 0.45, 0.55];

    /// Draws off the pixel grid, in slot `slot`: a grid of rects, sprites
    /// and (with `label`) texts at fractions of a pixel, halves among them,
    /// as a custom widget may draw or a padded or centred label sits, and a
    /// line
    fn off_grid(
      commands: &mut UiCommands,
      slot: u16,
      px_scale: f32,
      swatch: &Handle<Image>,
      label: Option<&Arc<TextLayout>>,
    ) {
      let mut order = commands.as_slice().len() as u32;
      let mut push = |commands: &mut UiCommands, command: UiCommand| {
        commands.push(order, slot, command);
        order += 1;
      };
      for (i, fx) in FRACTIONS.iter().enumerate() {
        for (j, fy) in FRACTIONS.iter().take(8).enumerate() {
          let x = (i as f32 * 30.0 + 4.0 + fx) / px_scale;
          let y = (j as f32 * 28.0 + 6.0 + fy) / px_scale;
          let (w, h) = ((17.0 + fy) / px_scale, (13.0 + fx) / px_scale);
          let command = match ((i + j) % 3, label) {
            (1, _) => sprite(swatch, Rect::new(x, y, w, h), Filter::Default, None),
            (2, Some(label)) => text(label, Vec2::new(x, y), None),
            _ => UiCommand::Rect {
              rect: Rect::new(x, y, w, h),
              fill: Color::new(40, 160, 220, 200),
              radius: 4.0,
              border: Some((1.5, Color::WHITE)),
              clip: None,
              layer: 0,
            },
          };
          push(commands, command);
        }
      }
      let points = [
        Vec2::new(5.3, 5.7),
        Vec2::new(60.1, 30.4),
        Vec2::new(90.6, 12.5),
        Vec2::new(150.0, 80.25),
      ];
      push(commands, line(&points, None));
    }

    /// `off_grid`'s draws in one slot drawn at `offset`, its clip the image
    fn off_grid_at(
      px_scale: f32,
      offset: [i32; 2],
      swatch: &Handle<Image>,
      label: Option<&Arc<TextLayout>>,
    ) -> UiCommands {
      let mut commands = UiCommands::default();
      let slot = commands.push_slot(ScrollSlot {
        offset,
        clip: None,
        cull: None,
      });
      off_grid(&mut commands, slot, px_scale, swatch, label);
      commands
    }

    /// Scroll offsets: at rest, then whole pixels each way
    const OFFSETS: [[i32; 2]; 8] = [
      [0, 0],
      [3, 7],
      [-5, -38],
      [17, -61],
      [-2, 23],
      [0, -137],
      [-29, 4],
      [11, -1],
    ];

    /// Pages drawn through the shader's slots and placed on the CPU at each
    /// offset: pixel for pixel the same
    fn assert_drawn_as_placed(
      writes: &mut Writes,
      env: UiRenderEnv,
      page: impl Fn([i32; 2]) -> UiCommands,
      sources: Option<&TextSources<'_>>,
    ) {
      let px_scale = env.space.px_scale;
      for offset in OFFSETS {
        let commands = page(offset);
        let slotted = drawn(writes, &commands, env, sources);
        let absolute = drawn(writes, &placed(&commands, px_scale), env, sources);
        assert!(painted(&absolute) > 5_000, "{} pixels", painted(&absolute));
        let (count, first) = differ(&slotted, &absolute);
        assert_eq!(
          count, 0,
          "scale {px_scale}, on target {}, offset {offset:?}: pixels differ (slots, placed): {first:?}",
          env.on_target
        );
      }
    }

    /// Off the pixel-art target nothing is snapped but texts' glyphs: rects,
    /// sprites and lines in a slot draw as Task 2 drew them on the CPU, on
    /// the pixel grid or off it, at whole and fractional UI scales. So does
    /// a text field's clip at scale 1. (At a fractional scale its edges, on
    /// whole pixels, were moved by a fraction of a UI unit on the CPU, which
    /// could tip one over a pixel; through the slot they move by exactly
    /// its offset: `a_slot_moves_texts_and_clips_by_exactly_its_offset`.)
    #[test]
    fn draws_in_slots_draw_as_they_were_placed_on_the_cpu() {
      let Some(mut writes) = Writes::new() else {
        return;
      };
      let swatch = super::super::gradient(6, 5);
      for px_scale in [1.0, 1.25, 1.5, 2.0, 0.75] {
        let env = env_sized(px_scale, (320, 240), false);
        let field = px_scale == 1.0;
        let laid = |offset| page(px_scale, offset, &swatch, None, field);
        assert_drawn_as_placed(&mut writes, env, laid, None);
        let off = |offset| off_grid_at(px_scale, offset, &swatch, None);
        assert_drawn_as_placed(&mut writes, env, off, None);
      }
    }

    /// Ruling R1: on the pixel-art target a quad's corners are rounded to
    /// whole pixels, and one on a half pixel rounds by the last bit of its
    /// position, which the CPU and the shader compute differently when
    /// moving it: there draws in slots are built absolute, as on the CPU
    #[test]
    fn draws_in_slots_on_the_pixel_art_target_draw_as_they_were_placed() {
      let Some(mut writes) = Writes::new() else {
        return;
      };
      let swatch = super::super::gradient(6, 5);
      for px_scale in [1.0, 0.5, 0.25, 1.25] {
        let env = env_sized(px_scale, (320, 240), true);
        let laid = |offset| page(px_scale, offset, &swatch, None, true);
        assert_drawn_as_placed(&mut writes, env, laid, None);
        let off = |offset| off_grid_at(px_scale, offset, &swatch, None);
        assert_drawn_as_placed(&mut writes, env, off, None);
      }
    }

    #[cfg(feature = "default-fonts")]
    mod texts {
      use super::*;
      use crate::core::text::{RichText, TextLayoutOptions, TextStyle};

      fn fonts() -> NoFonts {
        let mut sources = NoFonts::new();
        sources.fonts = Fonts::with_builtins();
        sources.engine.sync(&sources.fonts);
        sources
      }

      fn label(fonts: &mut NoFonts, px_scale: f32) -> Arc<TextLayout> {
        let style = TextStyle::default().size(15.0);
        fonts
          .engine
          .layout(
            &RichText::new("Setting 7: 42%"),
            &style,
            &TextLayoutOptions::default(),
            px_scale,
          )
          .expect("laid out")
      }

      /// Texts in a slot whose origins sit on whole render pixels draw as
      /// they were placed on the CPU at UI scales 1 and 1.25; at scale 1 so
      /// do those off the grid, and a text field's. (At a fractional scale
      /// a text off the grid may not: on the CPU each glyph was snapped
      /// where the slot moved it, a rounding the move by a fraction of a UI
      /// unit could tip; through the slot it moves by exactly the slot's
      /// offset, below.) On the pixel-art target every draw in a slot is
      /// placed on the CPU.
      #[test]
      fn texts_in_slots_draw_as_they_were_placed_on_the_cpu() {
        let Some(mut writes) = Writes::new() else {
          return;
        };
        let mut fonts = fonts();
        let swatch = super::super::super::gradient(6, 5);
        for (px_scale, on_target, off_grid) in [
          (1.0, false, true),
          (1.25, false, false),
          (1.0, true, true),
          (1.25, true, true),
        ] {
          let env = env_sized(px_scale, (320, 240), on_target);
          let layout = label(&mut fonts, px_scale);
          let sources = fonts.sources();
          let laid = |offset| page(px_scale, offset, &swatch, Some(&layout), off_grid);
          assert_drawn_as_placed(&mut writes, env, laid, Some(&sources));
          if off_grid {
            let off = |offset| off_grid_at(px_scale, offset, &swatch, Some(&layout));
            assert_drawn_as_placed(&mut writes, env, off, Some(&sources));
          }
        }
      }

      /// A slot moves its texts and the clips of its own draws by exactly
      /// its offset, on the pixel grid or off it, at whole and fractional UI
      /// scales: drawn at an offset, they are the image drawn at rest moved
      /// by it, pixel for pixel
      #[test]
      fn a_slot_moves_texts_and_clips_by_exactly_its_offset() {
        let Some(mut writes) = Writes::new() else {
          return;
        };
        let mut fonts = fonts();
        for px_scale in [1.0, 1.25, 1.5, 2.0, 0.75] {
          let env = env_sized(px_scale, (320, 240), false);
          let layout = label(&mut fonts, px_scale);
          let sources = fonts.sources();
          // Texts alone, at fractions of a pixel each way
          let texts = |offset: [i32; 2]| {
            let mut commands = UiCommands::default();
            let slot = commands.push_slot(ScrollSlot {
              offset,
              clip: None,
              cull: None,
            });
            let mut order = 0;
            for (i, fx) in FRACTIONS.iter().enumerate() {
              for (j, fy) in FRACTIONS.iter().enumerate() {
                let x = i as f32 * 30.0 + 4.0 + fx;
                let y = j as f32 * 22.0 + 6.0 + fy;
                let origin = Vec2::new(x, y) / px_scale;
                commands.push(order, slot, text(&layout, origin, None));
                order += 1;
              }
            }
            // A text field: its frame and text under its view's clip
            let px = |k: i32| k as f32 / px_scale;
            let view = Some(Rect::new(px(100), px(40), px(70), px(22)));
            let frame = UiCommand::Rect {
              rect: Rect::new(px(96), px(36), px(90), px(30)),
              fill: Color::new(40, 160, 220, 200),
              radius: 4.0,
              border: Some((1.5, Color::WHITE)),
              clip: view,
              layer: 0,
            };
            commands.push(order, slot, frame);
            commands.push(
              order + 1,
              slot,
              text(&layout, Vec2::new(px(92), px(44)), view),
            );
            commands
          };
          let rest = drawn(&mut writes, &texts([0, 0]), env, Some(&sources));
          assert!(painted(&rest) > 5_000, "{} pixels", painted(&rest));
          for offset in OFFSETS.into_iter().skip(1) {
            let moved = drawn(&mut writes, &texts(offset), env, Some(&sources));
            let differ = rest
              .enumerate_pixels()
              .filter(|&(x, y, pixel)| {
                let (mx, my) = (x as i32 + offset[0], y as i32 + offset[1]);
                let inside = (0..320).contains(&mx) && (0..240).contains(&my);
                inside && moved.get_pixel(mx as u32, my as u32) != pixel
              })
              .count();
            assert_eq!(differ, 0, "scale {px_scale}, offset {offset:?}");
          }
        }
      }

      /// A scrolling list's texts are laid once, relative to their slot,
      /// and copied while it scrolls: their instances do not change
      #[test]
      fn a_text_in_a_scrolling_slot_is_one_copy_of_its_instances() {
        let Some(mut writes) = Writes::new() else {
          return;
        };
        let mut fonts = fonts();
        let env = env_sized(1.25, (320, 240), false);
        let layout = label(&mut fonts, 1.25);
        let sources = fonts.sources();
        let frames: Vec<UiCommands> = (0..6)
          .map(|frame| {
            let mut commands = UiCommands::default();
            // A slot pushed before the list's on odd frames: the list's
            // slot changes
            if frame % 2 == 1 {
              commands.push_slot(ScrollSlot::NONE);
            }
            let slot = commands.push_slot(ScrollSlot {
              offset: [0, 10 - 7 * frame],
              clip: Some(Rect::new(0.0, 8.0, 200.0, 100.0)),
              cull: None,
            });
            commands.push(0, slot, text(&layout, Vec2::new(8.0, 12.8), None));
            commands
          })
          .collect();
        let mut texts = UiTextCache::<UiInstance>::default();
        let mut first: Option<Vec<UiInstance>> = None;
        for (frame, commands) in frames.iter().enumerate() {
          let mut build = UiBuild::default();
          texts.begin(env, true);
          build_ui(
            &mut build,
            commands,
            &env,
            (Some(&sources), &crate::core::graphics::test_images()),
            &mut writes.targets(),
            0.0,
            &mut texts,
            &mut UiRunCache::default(),
          );
          texts.end();
          let slot = commands.slots().len() as u8 - 1;
          assert!(!build.instances.is_empty());
          assert!(build.instances.iter().all(|glyph| glyph.slot() == slot));
          // The same glyphs, but for the slot they are in
          let unslotted: Vec<UiInstance> = build
            .instances
            .iter()
            .map(|glyph| glyph.with_slot(0))
            .collect();
          match &first {
            None => first = Some(unslotted),
            Some(first) => {
              assert_eq!(&unslotted, first, "frame {frame}");
              assert_eq!((texts.built(), texts.reused()), (0, 1), "frame {frame}");
            }
          }
        }
      }

      /// A nested list's texts are copied while an outer list scrolls it:
      /// their slot's clip moves with the outer list, but the shader takes
      /// a slot's clip from the slot, so neither their instances nor their
      /// key in the text cache hold it
      #[test]
      fn a_text_in_a_slot_whose_clip_moves_is_one_copy_of_its_instances() {
        let Some(mut writes) = Writes::new() else {
          return;
        };
        let mut fonts = fonts();
        let env = env_sized(1.25, (320, 240), false);
        let layout = label(&mut fonts, 1.25);
        let sources = fonts.sources();
        let mut texts = UiTextCache::<UiInstance>::default();
        let mut first: Option<Vec<UiInstance>> = None;
        for frame in 0..6 {
          let mut commands = UiCommands::default();
          let slot = commands.push_slot(ScrollSlot {
            offset: [0, 10 - 7 * frame],
            clip: Some(Rect::new(0.0, 8.0 - 3.0 * frame as f32, 200.0, 100.0)),
            cull: None,
          });
          commands.push(0, slot, text(&layout, Vec2::new(8.0, 12.8), None));
          let mut build = UiBuild::default();
          texts.begin(env, true);
          build_ui(
            &mut build,
            &commands,
            &env,
            (Some(&sources), &crate::core::graphics::test_images()),
            &mut writes.targets(),
            0.0,
            &mut texts,
            &mut UiRunCache::default(),
          );
          texts.end();
          assert!(!build.instances.is_empty());
          match &first {
            None => first = Some(build.instances.to_vec()),
            Some(first) => {
              assert_eq!(build.instances, *first, "frame {frame}");
              assert_eq!((texts.built(), texts.reused()), (0, 1), "frame {frame}");
            }
          }
        }
      }

      /// A text drawn in slot 0 is culled glyph by glyph; drawn next in a
      /// slot (the same layout, at the same place, under the same clip, as
      /// a scroll area's contents are on the frame its state is made), its
      /// glyphs are not, so the kept ones are not copied there
      #[test]
      fn a_text_kept_outside_a_slot_is_not_copied_into_one() {
        let Some(mut writes) = Writes::new() else {
          return;
        };
        let mut fonts = fonts();
        let env = env_sized(1.0, (320, 240), false);
        let layout = label(&mut fonts, 1.0);
        let sources = fonts.sources();
        // Its clip shows the first few glyphs only
        let clip = Rect::new(0.0, 0.0, 40.0, 100.0);
        let mut loose = UiCommands::default();
        loose.push(0, 0, text(&layout, Vec2::new(8.0, 12.0), Some(clip)));
        let mut slotted = UiCommands::default();
        let slot = slotted.push_slot(ScrollSlot {
          offset: [0, 0],
          clip: Some(clip),
          cull: None,
        });
        slotted.push(0, slot, text(&layout, Vec2::new(8.0, 12.0), None));
        let mut texts = UiTextCache::<UiInstance>::default();
        let mut counts = Vec::new();
        let mut builds = Vec::new();
        for commands in [&loose, &slotted] {
          let mut build = UiBuild::default();
          texts.begin(env, true);
          build_ui(
            &mut build,
            commands,
            &env,
            (Some(&sources), &crate::core::graphics::test_images()),
            &mut writes.targets(),
            0.0,
            &mut texts,
            &mut UiRunCache::default(),
          );
          texts.end();
          counts.push((texts.built(), texts.reused()));
          builds.push(build.instances.len());
        }
        assert_eq!(counts, [(1, 0), (1, 0)]);
        assert!(builds[0] < builds[1], "{builds:?}");
      }
    }
  }
  /// A fragment run's instances, built once per version and copied after
  mod runs {
    use super::*;
    use crate::core::ui::UiId;
    use crate::core::ui::immediate::FragmentRun;

    /// The run's root
    const ID: UiId = UiId(41);

    /// A loose rect, then a fragment's run in `slot` at `version` (three
    /// rects stacked 20 apart from y = 0, a sprite, a line and, with
    /// `label`, a text), then a loose rect in that slot at y = 80; the
    /// frame's slots `slots` (pushed after slot 0)
    fn with_run(
      version: u64,
      slots: &[ScrollSlot],
      slot: u16,
      swatch: &Handle<Image>,
      label: Option<&Arc<TextLayout>>,
    ) -> UiCommands {
      let mut commands = UiCommands::default();
      for &slot in slots {
        commands.push_slot(slot);
      }
      commands.push(0, 0, rect_at(200.0, 4.0, None, 0));
      let start = commands.position();
      let mut order = 10;
      let mut push = |commands: &mut UiCommands, command: UiCommand| {
        commands.push(order, slot, command);
        order += 1;
      };
      for k in 0..3 {
        push(&mut commands, rect_at(4.0, 20.0 * k as f32, None, 0));
      }
      let picture = Rect::new(30.0, 2.0, 16.0, 16.0);
      push(
        &mut commands,
        sprite(swatch, picture, Filter::Default, None),
      );
      let points = [Vec2::new(5.0, 5.0), Vec2::new(60.0, 30.0)];
      push(&mut commands, line(&points, None));
      if let Some(label) = label {
        push(&mut commands, text(label, Vec2::new(50.0, 4.0), None));
      }
      let end = commands.position();
      commands.push_run(FragmentRun {
        id: ID,
        version,
        base: 10,
        layer: 0,
        slot,
        draws: start..end,
        bounds: None,
      });
      commands.push(order, slot, rect_at(4.0, 80.0, None, 0));
      commands
    }

    /// Each of `frames` built in turn through one build (each frame's
    /// instances written over the last's, as the renderer builds them), one
    /// text cache and one run cache in `env`, retention `retain`: what each
    /// frame built, with the runs it copied and built
    fn through(
      writes: &mut Writes,
      frames: &[UiCommands],
      env: UiRenderEnv,
      sources: Option<&TextSources<'_>>,
      retain: bool,
    ) -> Vec<(UiBuild, (u32, u32))> {
      let mut texts = UiTextCache::<UiInstance>::default();
      let mut runs = UiRunCache::default();
      let mut build = UiBuild::default();
      frames
        .iter()
        .map(|commands| {
          texts.begin(env, retain);
          runs.begin(env, retain);
          build_ui(
            &mut build,
            commands,
            &env,
            (sources, &crate::core::graphics::test_images()),
            &mut writes.targets(),
            0.0,
            &mut texts,
            &mut runs,
          );
          texts.end();
          runs.end();
          (build.snapshot(), runs.stats())
        })
        .collect()
    }

    /// Each of `frames` built as it is first built, each through a text and
    /// run cache of its own, retention on: what a copy of its runs must be
    fn fresh(
      writes: &mut Writes,
      frames: &[UiCommands],
      env: UiRenderEnv,
      sources: Option<&TextSources<'_>>,
    ) -> Vec<(UiBuild, (u32, u32))> {
      frames
        .iter()
        .flat_map(|commands| through(writes, std::slice::from_ref(commands), env, sources, true))
        .collect()
    }

    /// Whether two builds are the same instances bit for bit, sampling the
    /// same, with the same sprites and sprite textures
    fn same(a: &UiBuild, b: &UiBuild) -> bool {
      bytemuck::cast_slice::<UiInstance, u8>(&a.instances)
        == bytemuck::cast_slice::<UiInstance, u8>(&b.instances)
        && a.needs == b.needs
        && a.sprites == b.sprites
        && a.sprite_textures() == b.sprite_textures()
    }

    /// The counts of each build in `built`
    fn counts(built: &[(UiBuild, (u32, u32))]) -> Vec<(u32, u32)> {
      built.iter().map(|(_, counts)| *counts).collect()
    }

    #[test]
    fn the_second_frame_copies_the_run() {
      let Some(mut writes) = Writes::new() else {
        return;
      };
      let swatch = super::super::gradient(6, 5);
      let frames = [
        with_run(1, &[], 0, &swatch, None),
        with_run(1, &[], 0, &swatch, None),
      ];
      let built = through(&mut writes, &frames, env(), None, true);
      assert_eq!(counts(&built), [(0, 1), (1, 0)]);
      let (first, second) = (&built[0].0, &built[1].0);
      assert!(same(first, second), "the copy is the build");
      // It holds a sprite, fixed once its texture is resident: copied
      assert!(second.copied > 0);
      // Three rects, the sprite and the line's triangles, the loose rects
      assert!(first.instances.len() >= 7, "{}", first.instances.len());
      assert_eq!(second.sprites.len(), 1);
    }

    #[test]
    fn a_new_version_rebuilds_the_run() {
      let Some(mut writes) = Writes::new() else {
        return;
      };
      let swatch = super::super::gradient(6, 5);
      let frames = [1, 1, 2, 2].map(|version| with_run(version, &[], 0, &swatch, None));
      let built = through(&mut writes, &frames, env(), None, true);
      assert_eq!(counts(&built), [(0, 1), (1, 0), (0, 1), (1, 0)]);
    }

    /// Commands out of the painter's order are sorted and built one by one:
    /// every run is built, and counted so, and none is kept
    #[test]
    fn out_of_order_every_run_is_built_and_counted() {
      let Some(mut writes) = Writes::new() else {
        return;
      };
      let swatch = super::super::gradient(6, 5);
      let frames = [0, 1].map(|_| {
        let mut commands = with_run(1, &[], 0, &swatch, None);
        // Recorded last, drawn first
        commands.push(5, 0, rect_at(260.0, 4.0, None, 0));
        commands
      });
      let built = through(&mut writes, &frames, env(), None, true);
      assert_eq!(counts(&built), [(0, 1), (0, 1)]);
    }

    #[test]
    fn with_retention_off_every_run_is_built() {
      let Some(mut writes) = Writes::new() else {
        return;
      };
      let swatch = super::super::gradient(6, 5);
      let frames = [1, 1, 1].map(|version| with_run(version, &[], 0, &swatch, None));
      let built = through(&mut writes, &frames, env(), None, false);
      assert_eq!(counts(&built), [(0, 1), (0, 1), (0, 1)]);
      let on = through(&mut writes, &frames, env(), None, true);
      for ((off, _), (on, _)) in built.iter().zip(&on) {
        assert!(same(off, on), "built or copied, the same instances");
      }
    }

    /// A slot drawn at `offset`, under a clip from y = 0 that is `high`
    fn slot_at(offset: [i32; 2], high: f32) -> ScrollSlot {
      ScrollSlot {
        offset,
        clip: Some(Rect::new(0.0, 0.0, 300.0, high)),
        cull: None,
      }
    }

    #[test]
    fn a_slots_run_is_built_unculled() {
      let Some(mut writes) = Writes::new() else {
        return;
      };
      let swatch = super::super::gradient(6, 5);
      // The slot's clip shows the first rect only, and not the loose rect
      // at y = 80 after the run
      let commands = with_run(1, &[slot_at([0, 0], 15.0)], 1, &swatch, None);
      // With retention off nothing of it is kept, and it is culled as the
      // loose rect is: the first rect alone
      let culled = through(
        &mut writes,
        std::slice::from_ref(&commands),
        env(),
        None,
        false,
      );
      let rects = culled[0]
        .0
        .instances
        .iter()
        .filter(|instance| instance.slot() == 1 && instance.kind() == UiKind::RECT)
        .count();
      assert_eq!(rects, 1, "the run's first rect");
      let built = through(&mut writes, &[commands], env(), None, true);
      let build = &built[0].0;
      let in_slot: Vec<&UiInstance> = build
        .instances
        .iter()
        .filter(|instance| instance.slot() == 1)
        .collect();
      let rects = in_slot
        .iter()
        .filter(|instance| instance.kind() == UiKind::RECT)
        .count();
      assert_eq!(rects, 3, "the run's three rects; the loose one culled");
      assert_eq!(build.sprites.len(), 1, "the run's sprite");
      assert!(
        in_slot
          .iter()
          .any(|instance| instance.kind() == UiKind::LINE),
        "the run's line"
      );
      // Its commands where the slot puts them, in slot 0: culled
      let mut placed = UiCommands::default();
      for draw in build_commands(&with_run(1, &[slot_at([0, 0], 15.0)], 1, &swatch, None)) {
        placed.push(draw.0, 0, draw.1);
      }
      let culled = through(&mut writes, &[placed], env(), None, false);
      let rects = culled[0]
        .0
        .instances
        .iter()
        .filter(|instance| instance.kind() == UiKind::RECT)
        .count();
      assert_eq!(
        rects, 2,
        "the run's first rect and the loose rect in slot 0"
      );
    }

    #[test]
    fn a_culled_runs_bounds_cull_it_as_its_draws_are_culled() {
      use crate::core::ui::immediate::Bounds;
      let Some(mut writes) = Writes::new() else {
        return;
      };
      // Three rects in a slot drawn at y = 20 under a view 40 tall, and a
      // fourth beside them whose own clip starts half a unit below it (it
      // shows nothing, though it lies within a pixel of that clip): the run
      // below the view, within a pixel of it, across it and inside it; its
      // bounds known or not, retention off (so it is culled), the same
      // instances
      let frame = |top: f32, bounded: bool| {
        let mut commands = UiCommands::default();
        let view = Rect::new(0.0, 0.0, 300.0, 40.0);
        commands.push_slot(ScrollSlot {
          offset: [0, 20],
          clip: Some(Rect::new(0.0, 20.0, 300.0, 40.0)),
          cull: Some(view),
        });
        let own = Some(Rect::new(0.0, top + 14.5, 300.0, 5.0));
        let rects = [
          rect_at(4.0, top, None, 0),
          rect_at(4.0, top + 12.0, None, 0),
          rect_at(4.0, top + 24.0, None, 0),
          rect_at(40.0, top + 4.0, own, 0),
        ];
        let mut bounds = Bounds::default();
        for (k, rect) in (0..).zip(rects) {
          bounds.add(rect.cull_box());
          commands.push(10 + k, 1, rect);
        }
        commands.push_run(FragmentRun {
          id: ID,
          version: 1,
          base: 10,
          layer: 0,
          slot: 1,
          draws: 0..4,
          bounds: bounded.then(|| bounds.edges()).flatten(),
        });
        commands
      };
      // Outside, across (a pixel off, its edge, a unit in, partly), inside
      for top in [300.0, 40.5, 40.0, 39.0, 10.0, 2.0] {
        let culled = [true, false].map(|bounded| {
          let built = through(&mut writes, &[frame(top, bounded)], env(), None, false);
          built.into_iter().next().expect("a build")
        });
        assert_eq!(counts(&culled), [(0, 1), (0, 1)], "top {top}");
        assert!(same(&culled[0].0, &culled[1].0), "top {top}");
        if top == 2.0 {
          let rects = culled[0].0.instances.len();
          assert_eq!(rects, 3, "inside: the three rects, the clipped one culled");
        }
      }
    }

    /// `commands`' draws where they are drawn (moved by their slots), with
    /// their orders
    fn build_commands(commands: &UiCommands) -> Vec<(u32, UiCommand)> {
      commands
        .as_slice()
        .iter()
        .filter_map(|draw| {
          let command = draw.placed(commands.slots(), 1.0)?;
          Some((draw.order, command.into_owned()))
        })
        .collect()
    }

    #[test]
    fn a_copied_run_is_what_building_it_gives_as_its_slot_scrolls() {
      let Some(mut writes) = Writes::new() else {
        return;
      };
      let swatch = super::super::gradient(6, 5);
      // A list scrolling under a clip that moves as an outer list scrolls
      // it (a nested slot), and a slot pushed before it on odd frames, so
      // its index changes
      let frames: Vec<UiCommands> = (0..6)
        .map(|frame: i32| {
          let mut slots = Vec::new();
          if frame % 2 == 1 {
            slots.push(ScrollSlot::NONE);
          }
          slots.push(ScrollSlot {
            offset: [0, 10 - 7 * frame],
            clip: Some(Rect::new(0.0, 8.0 - frame as f32, 200.0, 50.0)),
            cull: None,
          });
          let slot = slots.len() as u16;
          with_run(1, &slots, slot, &swatch, None)
        })
        .collect();
      let kept = through(&mut writes, &frames, env(), None, true);
      let fresh = fresh(&mut writes, &frames, env(), None);
      assert_eq!(counts(&kept)[1..], [(1, 0); 5]);
      for (frame, ((kept, _), (fresh, _))) in kept.iter().zip(&fresh).enumerate() {
        assert!(same(kept, fresh), "frame {frame}");
        let slot = frames[frame].slots().len() as u8 - 1;
        let run = kept.instances.iter().filter(|i| i.slot() != 0);
        assert!(run.clone().count() >= 5, "frame {frame}");
        assert!(run.clone().all(|i| i.slot() == slot), "frame {frame}");
        // Its clip is the slot's, which the shader takes from the slot
        let image = ClipRect::of(None, env().space.image);
        assert!(run.clone().all(|i| i.clip() == image), "frame {frame}");
      }
    }

    #[test]
    fn a_run_left_in_place_is_what_building_it_gives() {
      let Some(mut writes) = Writes::new() else {
        return;
      };
      // A run of rects in a scrolling slot, between loose rects, written over
      // the last frame's instances: where it sat, with its slot's index,
      // it is left in place; where the slot's index moved (a slot pushed
      // before it), or a loose rect before it came and went, it is copied
      let run = |frame: i32| {
        let mut commands = UiCommands::default();
        if frame == 3 {
          commands.push_slot(ScrollSlot::NONE);
        }
        let slot = commands.push_slot(ScrollSlot {
          offset: [0, 10 - 7 * frame],
          clip: Some(Rect::new(0.0, 8.0, 200.0, 50.0)),
          cull: None,
        });
        commands.push(0, 0, rect_at(200.0, 4.0, None, 0));
        if frame == 5 {
          commands.push(1, 0, rect_at(230.0, 4.0, None, 0));
        }
        let start = commands.position();
        for k in 0..4 {
          commands.push(10 + k, slot, rect_at(4.0, 20.0 * k as f32, None, 0));
        }
        let draws = start..commands.position();
        commands.push_run(FragmentRun {
          id: ID,
          version: 1,
          base: 10,
          layer: 0,
          slot,
          draws,
          bounds: None,
        });
        commands.push(20, slot, rect_at(4.0, 90.0, None, 0));
        commands
      };
      let frames: Vec<UiCommands> = (0..7).map(run).collect();
      let kept = through(&mut writes, &frames, env(), None, true);
      let fresh = fresh(&mut writes, &frames, env(), None);
      assert_eq!(counts(&kept)[1..], [(1, 0); 6]);
      for (frame, ((kept, _), (fresh, _))) in kept.iter().zip(&fresh).enumerate() {
        assert!(same(kept, fresh), "frame {frame}: as building it gives");
      }
      let copied: Vec<bool> = kept.iter().map(|(build, _)| build.copied > 0).collect();
      // Built; in place twice; at another index; back at the first (the
      // last frame had the other); after a loose rect more; back where it
      // was two frames before
      assert_eq!(copied, [false, false, false, true, true, true, true]);
    }

    #[test]
    fn a_run_built_absolute_is_never_kept() {
      let Some(mut writes) = Writes::new() else {
        return;
      };
      let swatch = super::super::gradient(6, 5);
      let twice =
        |slots: &[ScrollSlot], slot: u16| [0, 1].map(|_| with_run(1, slots, slot, &swatch, None));
      // Past the shader's slots
      let past = vec![slot_at([0, 3], 100.0); 300];
      let built = through(&mut writes, &twice(&past, 299), env(), None, true);
      assert_eq!(counts(&built), [(0, 1), (0, 1)], "past the slots");
      // Kept in slot 0, then drawn past the slots: moved on the CPU, not
      // copied as it was
      let frames = [
        with_run(1, &past, 0, &swatch, None),
        with_run(1, &past, 299, &swatch, None),
      ];
      let built = through(&mut writes, &frames, env(), None, true);
      assert_eq!(
        counts(&built),
        [(0, 1), (0, 1)],
        "from slot 0 past the slots"
      );
      // On the pixel-art target, in a slot; in slot 0 it is kept there
      let pixel_art = env_at(1.0, true);
      let frames = twice(&[slot_at([0, 3], 100.0)], 1);
      let built = through(&mut writes, &frames, pixel_art, None, true);
      assert_eq!(counts(&built), [(0, 1), (0, 1)], "on the pixel-art target");
      let built = through(&mut writes, &twice(&[], 0), pixel_art, None, true);
      assert_eq!(counts(&built), [(0, 1), (1, 0)], "slot 0 on the target");
      // A run with a draw whose own clip does not fit relative to the slot
      // (a text field 70,000 pixels down a list), after one that fits
      let far = |mut commands: UiCommands| {
        let clip = Some(Rect::new(4.0, 70_000.0, 40.0, 10.0));
        let start = commands.position();
        commands.push(30, 1, rect_at(4.0, 69_995.0, clip, 0));
        commands.push(31, 1, rect_at(4.0, 0.0, None, 0));
        commands.push_run(FragmentRun {
          id: UiId(42),
          version: 1,
          base: 30,
          layer: 0,
          slot: 1,
          draws: start..commands.position(),
          bounds: None,
        });
        commands
      };
      let frames = twice(&[slot_at([0, -69_990], 100.0)], 1).map(far);
      let built = through(&mut writes, &frames, env(), None, true);
      assert_eq!(counts(&built), [(0, 2), (1, 1)], "only the run in reach");
      // Its own clip scrolled outside the slot's view (it draws nothing),
      // then into it at the same version: built in both frames, so it
      // draws in the second as a rebuild draws it
      let frames = [-69_800, -69_990].map(|y| {
        let slots = [slot_at([0, y], 100.0)];
        far(with_run(1, &slots, 1, &swatch, None))
      });
      let built = through(&mut writes, &frames, env(), None, true);
      assert_eq!(counts(&built), [(0, 2), (1, 1)], "scrolled into view");
      let fresh = fresh(&mut writes, &frames, env(), None);
      assert!(
        same(&built[1].0, &fresh[1].0),
        "drawn as a rebuild draws it"
      );
      // The far rect, built absolute under its own clip met with the slot's
      let far_clip = ClipRect {
        x0: 4,
        y0: 10,
        x1: 44,
        y1: 20,
      };
      let drawn = |build: &UiBuild| {
        let far = |instance: &&UiInstance| instance.clip() == far_clip;
        build.instances.iter().filter(far).count()
      };
      assert_eq!((drawn(&built[0].0), drawn(&built[1].0)), (0, 1));
    }

    #[cfg(feature = "default-fonts")]
    #[test]
    fn a_runs_text_is_copied_with_it() {
      use crate::core::text::{RichText, TextLayoutOptions, TextStyle};

      let Some(mut writes) = Writes::new() else {
        return;
      };
      let mut fonts = NoFonts::new();
      fonts.fonts = Fonts::with_builtins();
      fonts.engine.sync(&fonts.fonts);
      let layout = fonts
        .engine
        .layout(
          &RichText::new("Kept 42"),
          &TextStyle::default(),
          &TextLayoutOptions::default(),
          1.0,
        )
        .expect("laid out");
      let sources = fonts.sources();
      let swatch = super::super::gradient(6, 5);
      for (slots, slot) in [(vec![], 0), (vec![slot_at([0, 4], 60.0)], 1)] {
        let frames = [1, 1, 1].map(|_| with_run(1, &slots, slot, &swatch, Some(&layout)));
        let built = through(&mut writes, &frames, env(), Some(&sources), true);
        assert_eq!(counts(&built), [(0, 1), (1, 0), (1, 0)], "slot {slot}");
        assert!(same(&built[0].0, &built[2].0), "slot {slot}");
        let glyphs = built[2]
          .0
          .needs
          .iter()
          .filter(|need| matches!(need, TexNeed::Glyph(_)))
          .count();
        assert!(glyphs >= 5, "slot {slot}: {glyphs} glyphs");
      }
    }

    /// A copied run's texts are drawn with it: the text cache keeps them,
    /// so the run's next version (a hover in it, its list's interest moving
    /// on) reuses them
    #[cfg(feature = "default-fonts")]
    #[test]
    fn a_copied_runs_texts_stay_in_the_text_cache() {
      use crate::core::text::{RichText, TextLayoutOptions, TextStyle};

      let Some(mut writes) = Writes::new() else {
        return;
      };
      let mut fonts = NoFonts::new();
      fonts.fonts = Fonts::with_builtins();
      fonts.engine.sync(&fonts.fonts);
      let layout = fonts
        .engine
        .layout(
          &RichText::new("Held 3"),
          &TextStyle::default(),
          &TextLayoutOptions::default(),
          1.0,
        )
        .expect("laid out");
      let sources = fonts.sources();
      let swatch = super::super::gradient(6, 5);
      for (slots, slot) in [(vec![], 0), (vec![slot_at([0, 4], 60.0)], 1)] {
        let (mut texts, mut runs) = (UiTextCache::<UiInstance>::default(), UiRunCache::default());
        let mut counts = Vec::new();
        for version in [1, 1, 1, 2, 2] {
          let mut build = UiBuild::default();
          texts.begin(env(), true);
          runs.begin(env(), true);
          build_ui(
            &mut build,
            &with_run(version, &slots, slot, &swatch, Some(&layout)),
            &env(),
            (Some(&sources), &crate::core::graphics::test_images()),
            &mut writes.targets(),
            0.0,
            &mut texts,
            &mut runs,
          );
          texts.end();
          runs.end();
          counts.push((runs.stats(), (texts.built(), texts.reused())));
        }
        // A copy draws the text from glyphs built earlier: reused
        let expected = [
          ((0, 1), (1, 0)),
          ((1, 0), (0, 1)),
          ((1, 0), (0, 1)),
          ((0, 1), (0, 1)),
          ((1, 0), (0, 1)),
        ];
        assert_eq!(counts, expected, "slot {slot}");
      }
    }

    /// Two runs draw one layout (two rows' equal labels share it): a copy
    /// of the first holds its text where drawing it would, so the second,
    /// built anew, still finds its own entry
    #[cfg(feature = "default-fonts")]
    #[test]
    fn a_held_text_keeps_its_place_among_texts_alike() {
      use crate::core::text::{RichText, TextLayoutOptions, TextStyle};

      let Some(mut writes) = Writes::new() else {
        return;
      };
      let mut fonts = NoFonts::new();
      fonts.fonts = Fonts::with_builtins();
      fonts.engine.sync(&fonts.fonts);
      let layout = fonts
        .engine
        .layout(
          &RichText::new("On"),
          &TextStyle::default(),
          &TextLayoutOptions::default(),
          1.0,
        )
        .expect("laid out");
      let sources = fonts.sources();
      // Rows 1 and 2, each a run of its label, at their versions
      let frame = |versions: [u64; 2]| {
        let mut commands = UiCommands::default();
        for (row, version) in (0u32..).zip(versions) {
          let start = commands.position();
          let origin = Vec2::new(8.0, 10.0 + 30.0 * row as f32);
          commands.push(10 * row, 0, text(&layout, origin, None));
          commands.push_run(FragmentRun {
            id: UiId(u64::from(row) + 1),
            version,
            base: 10 * row,
            layer: 0,
            slot: 0,
            draws: start..commands.position(),
            bounds: None,
          });
        }
        commands
      };
      let (mut texts, mut runs) = (UiTextCache::<UiInstance>::default(), UiRunCache::default());
      let mut counts = Vec::new();
      for versions in [[1, 1], [1, 2], [3, 2]] {
        let mut build = UiBuild::default();
        texts.begin(env(), true);
        runs.begin(env(), true);
        build_ui(
          &mut build,
          &frame(versions),
          &env(),
          (Some(&sources), &crate::core::graphics::test_images()),
          &mut writes.targets(),
          0.0,
          &mut texts,
          &mut runs,
        );
        texts.end();
        runs.end();
        counts.push((runs.stats(), (texts.built(), texts.reused())));
      }
      let expected = [
        ((0, 2), (2, 0)),
        // The first copied, the second built from its kept text
        ((1, 1), (0, 2)),
        // The first built from its kept text, the second copied
        ((1, 1), (0, 2)),
      ];
      assert_eq!(counts, expected);
    }

    /// A run whose text a full atlas cut short (glyphs refused for the
    /// frame) is not kept: built in the next frame, its text is whole
    #[cfg(feature = "default-fonts")]
    #[test]
    fn a_run_a_full_atlas_cut_short_is_built_again() {
      use crate::core::text::{RichText, TextLayoutOptions, TextStyle};

      let Some(mut writes) = Writes::new() else {
        return;
      };
      let mut fonts = NoFonts::new();
      fonts.fonts = Fonts::with_builtins();
      fonts.engine.sync(&fonts.fonts);
      let mut laid = |text: &str, size: f32| {
        fonts
          .engine
          .layout(
            &RichText::new(text),
            &TextStyle::default().size(size),
            &TextLayoutOptions::default(),
            1.0,
          )
          .expect("laid out")
      };
      // Large glyphs enough to fill every glyph page in one frame, laid
      // below the image (culled, but made), then the label: its glyphs, as
      // wide, find no room left
      let chars = "ABCDEFGHIJKLNOPQRSTUVXYZabcdefghijklmnopqrstuvwxyz0123456789";
      let flood: Vec<_> = [500.0, 480.0, 460.0, 440.0, 420.0]
        .into_iter()
        .map(|size| laid(chars, size))
        .collect();
      let label = laid("MW", 500.0);
      let sources = fonts.sources();
      let env = UiRenderEnv {
        space: UiSpace {
          size: Vec2::new(2048.0, 1024.0),
          px_scale: 1.0,
          image: (2048, 1024),
          mouse_scale: 1.0,
        },
        ..env()
      };
      // The run, after the flood when `flooded`: its label alone
      let frame = |flooded: bool| {
        let mut commands = UiCommands::default();
        if flooded {
          for (order, layout) in (0..).zip(&flood) {
            commands.push(order, 0, text(layout, Vec2::new(0.0, 1500.0), None));
          }
        }
        let start = commands.position();
        commands.push(10, 0, text(&label, Vec2::new(8.0, 8.0), None));
        commands.push_run(FragmentRun {
          id: ID,
          version: 1,
          base: 10,
          layer: 0,
          slot: 0,
          draws: start..commands.position(),
          bounds: None,
        });
        commands
      };
      let (mut texts, mut runs) = (UiTextCache::<UiInstance>::default(), UiRunCache::default());
      let (mut counts, mut glyphs) = (Vec::new(), Vec::new());
      for flooded in [true, false] {
        let mut build = UiBuild::default();
        texts.begin(env, true);
        runs.begin(env, true);
        build_ui(
          &mut build,
          &frame(flooded),
          &env,
          (Some(&sources), &crate::core::graphics::test_images()),
          &mut writes.targets(),
          0.0,
          &mut texts,
          &mut runs,
        );
        texts.end();
        runs.end();
        counts.push(runs.stats());
        // The label's glyphs (the flood's are culled)
        glyphs.push(build.instances.len());
        // The next frame
        let (device, queue) = (Arc::clone(&writes.device), Arc::clone(&writes.queue));
        let list = DrawList::default();
        let textures = build.sprite_textures().to_vec();
        writes
          .sprites
          .prepare_textures(&device, &queue, &list, &textures);
      }
      assert_eq!(glyphs[1], 2, "the whole label");
      assert!(glyphs[0] < glyphs[1], "the label cut short: {glyphs:?}");
      assert_eq!(counts, [(0, 1), (0, 1)], "built again");
    }

    /// A copied run's glyphs were looked up in an earlier frame: the copy
    /// keeps the pages they sit on from eviction for the frame, so a glyph
    /// made later in it cannot clear one under them
    #[cfg(feature = "default-fonts")]
    #[test]
    fn a_copied_run_pins_the_glyph_pages_it_samples() {
      use crate::core::text::{RichText, TextLayoutOptions, TextStyle};

      let Some(mut writes) = Writes::new() else {
        return;
      };
      let mut fonts = NoFonts::new();
      fonts.fonts = Fonts::with_builtins();
      fonts.engine.sync(&fonts.fonts);
      let layout = fonts
        .engine
        .layout(
          &RichText::new("Pinned 7"),
          &TextStyle::default(),
          &TextLayoutOptions::default(),
          1.0,
        )
        .expect("laid out");
      let sources = fonts.sources();
      let swatch = super::super::gradient(6, 5);
      let commands = with_run(1, &[], 0, &swatch, Some(&layout));
      let (mut texts, mut runs) = (UiTextCache::<UiInstance>::default(), UiRunCache::default());
      let mut pages = Vec::new();
      for frame in 0..2 {
        let mut build = UiBuild::default();
        texts.begin(env(), true);
        runs.begin(env(), true);
        build_ui(
          &mut build,
          &commands,
          &env(),
          (Some(&sources), &crate::core::graphics::test_images()),
          &mut writes.targets(),
          0.0,
          &mut texts,
          &mut runs,
        );
        texts.end();
        runs.end();
        pages = build
          .needs
          .iter()
          .filter_map(|need| match need {
            TexNeed::Glyph(page) => Some(*page),
            _ => None,
          })
          .collect();
        if frame == 0 {
          // The next frame
          let (device, queue) = (Arc::clone(&writes.device), Arc::clone(&writes.queue));
          let list = DrawList::default();
          let textures = build.sprite_textures().to_vec();
          writes
            .sprites
            .prepare_textures(&device, &queue, &list, &textures);
          let unpinned = pages
            .iter()
            .all(|&page| !writes.sprites.glyph_page_pinned(page));
          assert!(unpinned, "built, not pinned");
        }
      }
      assert_eq!(runs.stats(), (1, 0), "copied");
      assert!(!pages.is_empty());
      for page in pages {
        assert!(writes.sprites.glyph_page_pinned(page), "{page:?}");
      }
    }
  }
}

/// The instance buffer after uploads that change part of a frame
mod upload {
  use bytemuck::Zeroable;
  use glam::Vec2;

  use super::super::*;
  use crate::core::engine::EngineBuilder;
  use crate::core::render::SCENE_FORMAT;
  use crate::core::render::batch::Batch2D;
  use crate::core::scene::{Scene, SceneAction, SceneContext};
  use crate::core::testing::HeadlessEngine;
  use crate::core::ui::immediate::UiContext;
  use crate::core::ui::{Color, Rect, UiNode};

  /// What `buffer` holds over its first `len` instances
  fn read_back(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    buffer: &wgpu::Buffer,
    len: usize,
  ) -> Vec<UiInstance> {
    let stride = std::mem::size_of::<UiInstance>();
    let size = (len * stride) as u64;
    if size == 0 {
      return Vec::new();
    }
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
      label: Some("UI Instance Readback"),
      size,
      usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
      mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.copy_buffer_to_buffer(buffer, 0, &staging, 0, size);
    queue.submit([encoder.finish()]);
    let slice = staging.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device
      .poll(wgpu::PollType::wait_indefinitely())
      .expect("the readback finishes");
    let bytes = slice.get_mapped_range().expect("the readback maps");
    bytes
      .chunks_exact(stride)
      .map(bytemuck::pod_read_unaligned)
      .collect()
  }

  /// `n` instances told apart by their first lane, from `from`
  fn numbered(from: usize, n: usize) -> Vec<UiInstance> {
    (from..from + n)
      .map(|i| {
        let mut instance = UiInstance::zeroed();
        instance.data[0][0] = i as f32;
        instance
      })
      .collect()
  }

  #[test]
  fn a_grown_buffer_is_given_the_whole_frame() {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let batch = Batch2D::new(&device, SCENE_FORMAT, Default::default());
    let mut gpu = UiGpu::new(&device, batch.camera_layout());
    let first = numbered(0, 600);
    gpu.upload(&device, &queue, &first, &[], &[]);
    // The same 600 first, then more than the buffer holds
    let grown = numbered(0, 3000);
    gpu.upload(&device, &queue, &grown, &[], &[]);
    let written: Vec<_> = gpu.written().iter().map(|range| range.len()).collect();
    assert_eq!(written, [3000]);
    assert_eq!(gpu.written().first().map(|range| range.start), Some(0));
    let held = read_back(&device, &queue, gpu.instances(), grown.len());
    assert!(held == grown, "the grown buffer holds the frame");
  }

  /// A panel with texts before and after a counter that changes every frame,
  /// a line, and every third frame a text more
  struct Counter {
    frame: u32,
  }

  impl Scene for Counter {
    fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
      SceneAction::Continue
    }

    fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
      self.frame += 1;
      ui.draw_rect(Rect::new(4.0, 4.0, 200.0, 120.0), Color::new(0, 0, 0, 160));
      ui.draw_text("Before the counter", 8.0, 8.0, Color::WHITE);
      ui.draw_text(&format!("Frame {}", self.frame), 8.0, 30.0, Color::WHITE);
      ui.draw_text("After the counter", 8.0, 52.0, Color::WHITE);
      if self.frame.is_multiple_of(3) {
        ui.draw_text("Now and then", 8.0, 74.0, Color::WHITE);
      }
      let points = [Vec2::new(8.0, 100.0), Vec2::new(120.0, 110.0)];
      ui.draw_line(&points, 2.0, Color::WHITE);
      SceneAction::Continue
    }
  }

  #[test]
  fn the_instance_buffer_holds_each_frame_as_one_label_changes() {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let builder = EngineBuilder::new()
      .resolution(320, 240)
      .start_scene(Counter { frame: 0 });
    let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
      return;
    };
    let mut partial = 0;
    // Past "Frame 9" to "Frame 10", and the text more coming and going
    for frame in 1..=12 {
      engine.step().expect("a frame");
      let ui = engine.ui_renderer().expect("a UI renderer");
      let instances = &ui.build.instances;
      let held = read_back(&device, &queue, ui.gpu.instances(), instances.len());
      assert!(
        held == *instances,
        "frame {frame}: the buffer holds the frame"
      );
      let written: usize = ui.gpu.written().iter().map(|range| range.len()).sum();
      partial += usize::from(written > 0 && written < instances.len());
    }
    assert!(partial >= 6, "{partial} frames wrote only what changed");
  }

  /// Two swatches whose shade changes every frame, one before and one after
  /// a column of still labels
  struct Swatches {
    frame: u8,
  }

  impl Scene for Swatches {
    fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
      SceneAction::Continue
    }

    fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
      self.frame = self.frame.wrapping_add(1);
      let shade = Color::new(self.frame, 40, 40, 255);
      ui.draw_rect(Rect::new(4.0, 4.0, 20.0, 20.0), shade);
      for row in 0..20 {
        let y = 4.0 + 10.0 * row as f32;
        ui.draw_text(&format!("A still label, row {row}"), 30.0, y, Color::WHITE);
      }
      ui.draw_rect(Rect::new(4.0, 216.0, 20.0, 20.0), shade);
      SceneAction::Continue
    }
  }

  #[test]
  fn changes_far_apart_are_written_apart() {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let builder = EngineBuilder::new()
      .resolution(320, 240)
      .start_scene(Swatches { frame: 0 });
    let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
      return;
    };
    let _ = engine.step_frames(3);
    for frame in 0..5 {
      engine.step().expect("a frame");
      let ui = engine.ui_renderer().expect("a UI renderer");
      let instances = &ui.build.instances;
      assert!(instances.len() > 200, "{} instances", instances.len());
      let held = read_back(&device, &queue, ui.gpu.instances(), instances.len());
      assert!(
        held == *instances,
        "frame {frame}: the buffer holds the frame"
      );
      // The swatches, and not the labels between them
      let written = engine.ui_render_stats().instances_written;
      assert_eq!(written, 2, "frame {frame}");
    }
  }

  /// A node tree, `tree`, and a label changing every frame over it
  struct NodeLabel {
    tree: fn() -> UiNode,
    frame: u32,
  }

  impl Scene for NodeLabel {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
      ctx.spawn_ui((self.tree)());
    }

    fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
      SceneAction::Continue
    }

    fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
      self.frame += 1;
      ui.draw_text(&format!("Frame {}", self.frame), 8.0, 8.0, Color::WHITE);
      SceneAction::Continue
    }
  }

  /// `tree` and a changing label at 1280x720, warmed up; the debug spot
  /// check off, so every clean fragment is copied
  fn node_label(tree: fn() -> UiNode) -> Option<HeadlessEngine> {
    let builder = EngineBuilder::new()
      .resolution(1280, 720)
      .start_scene(NodeLabel { tree, frame: 0 });
    let mut engine = HeadlessEngine::new_or_skip(builder)?;
    engine.set_ui_spot_check(false);
    let _ = engine.step_frames(10);
    Some(engine)
  }

  /// Step `engine` once: the buffer holds the frame. The instances, and
  /// those compared with what the buffer held
  fn held_step(
    engine: &mut HeadlessEngine,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    what: &str,
  ) -> (usize, usize) {
    engine.step().expect("a frame");
    let ui = engine.ui_renderer().expect("a UI renderer");
    let instances = &ui.build.instances;
    let held = read_back(device, queue, ui.gpu.instances(), instances.len());
    assert!(held == *instances, "{what}: the buffer holds the frame");
    (instances.len(), ui.gpu.compared())
  }

  #[test]
  fn runs_copied_where_they_sat_are_not_compared_again() {
    // The screen the render targets were set on: its rows' runs are copied
    // where they sat last frame, and only what was built (the label, the
    // loose draws) is compared with what the buffer held
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let tree = crate::core::testing::fixtures::settings_tree_overflowing;
    let Some(mut engine) = node_label(tree) else {
      return;
    };
    for frame in 0..5 {
      let (instances, compared) = held_step(&mut engine, &device, &queue, "the label");
      let stats = engine.ui_render_stats();
      assert!(stats.runs_copied >= 40, "frame {frame}: {stats:?}");
      assert!(
        compared * 10 < instances,
        "frame {frame}: {compared} of {instances} compared"
      );
    }
  }

  #[test]
  fn a_scrolling_lists_slots_are_written_with_its_instances() {
    // On the pixel-art target every draw in a scroll slot is built
    // absolute, and no instance is in one of the shader's slots: the list's
    // offset is not written, and slot 0 moves past the instances only with
    // them. Off the target its offset is written every frame it moves,
    // past the instances, with the scrollbar's thumb (the last instance)
    for target in [true, false] {
      let builder = if target {
        EngineBuilder::new().pixel_art(640, 480)
      } else {
        EngineBuilder::new().resolution(640, 480)
      };
      let tree = crate::core::testing::fixtures::settings_tree;
      let builder = builder.start_scene(NodeLabel { tree, frame: 0 });
      let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
        return;
      };
      engine.mouse_move(200.0, 118.0);
      let _ = engine.step_frames(10);
      let (mut written, mut apart) = (0, 0);
      for _ in 0..10 {
        engine.wheel(0.0, -1.0);
        engine.step().expect("a frame");
        let ui = engine.ui_renderer().expect("a UI renderer");
        let (slots, alone) = ui.gpu.slots_written();
        (written, apart) = (written + usize::from(slots), apart + usize::from(alone));
      }
      assert_eq!(apart, 0, "on the target: {target}");
      if !target {
        assert_eq!(written, 10);
      }
    }
  }

  #[test]
  fn runs_copied_where_they_sat_are_left_in_place() {
    // The screen the render targets were set on, a label changing over it:
    // its rows' runs sit where they sat last frame, and are not copied
    // there again; the frame is as building it gives
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let tree = crate::core::testing::fixtures::settings_tree_overflowing;
    let Some(mut engine) = node_label(tree) else {
      return;
    };
    for frame in 0..5 {
      held_step(&mut engine, &device, &queue, "the label");
      let ui = engine.ui_renderer().expect("a UI renderer");
      let stats = engine.ui_render_stats();
      assert!(stats.runs_copied >= 40, "frame {frame}: {stats:?}");
      assert_eq!(ui.build.copied, 0, "frame {frame}: instances copied");
    }
  }

  /// A column of rows, each a swatch beside a label
  fn swatch_rows() -> UiNode {
    let swatch = super::gradient(6, 5);
    UiNode::column().children((0..20).map(|row| {
      UiNode::row()
        .child(UiNode::image(swatch.clone()).image_size(16.0, 16.0))
        .child(UiNode::text(format!("Row {row}")))
    }))
  }

  #[test]
  fn the_instance_buffer_holds_each_frame_of_kept_runs_moving() {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    // The settings screen's list turned down and up by the wheel under
    // the pointer: its rows hovered and split into runs, their runs copied
    // where they sat or elsewhere, its interest moving on
    let Some(mut engine) = node_label(crate::core::testing::fixtures::settings_tree) else {
      return;
    };
    engine.mouse_move(520.0, 238.0);
    for frame in 0..60 {
      engine.wheel(0.0, if frame < 30 { -1.0 } else { 1.0 });
      held_step(
        &mut engine,
        &device,
        &queue,
        &format!("scrolling, frame {frame}"),
      );
    }
    assert!(engine.ui_render_stats().runs_copied > 0);
    // Rows holding sprites, whose instances are placed once their textures
    // are resident
    let Some(mut engine) = node_label(swatch_rows) else {
      return;
    };
    for frame in 0..5 {
      held_step(
        &mut engine,
        &device,
        &queue,
        &format!("swatches, frame {frame}"),
      );
      assert!(engine.ui_render_stats().runs_copied > 0);
    }
  }
}

/// The bind groups the renderer holds, and so the textures they hold alive,
/// as the UI changes, holds still and goes away
mod groups_held {
  use super::super::*;
  use super::gradient_image;
  use crate::core::asset::Handle;
  use crate::core::engine::EngineBuilder;
  use crate::core::scene::{Scene, SceneAction, SceneContext};
  use crate::core::testing::HeadlessEngine;
  use crate::core::ui::immediate::UiContext;
  use crate::core::ui::{Color, Rect};

  /// What the UI draws in a frame
  #[derive(Clone, Copy)]
  enum Shows {
    /// One of the images (each a standalone texture, being over 256 px)
    Image(usize),
    /// The image, and a rect over it
    ImageAndRect(usize),
    /// The image, outside the UI image: all of it culled
    Outside(usize),
  }

  /// A UI drawing `shows` frame by frame, then nothing
  struct Script {
    images: Vec<Handle<Image>>,
    shows: Vec<Shows>,
    frame: usize,
  }

  impl Scene for Script {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
      self.images = vec![
        ctx.assets().add(gradient_image(300, 300)),
        ctx.assets().add(gradient_image(300, 280)),
      ];
    }

    fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
      SceneAction::Continue
    }

    fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
      let shows = self.shows.get(self.frame).copied();
      self.frame += 1;
      match shows {
        Some(Shows::Image(image)) => ui.draw_sprite(&self.images[image], 0.0, 0.0),
        Some(Shows::ImageAndRect(image)) => {
          ui.draw_sprite(&self.images[image], 0.0, 0.0);
          ui.draw_rect(Rect::new(8.0, 8.0, 40.0, 20.0), Color::WHITE);
        }
        Some(Shows::Outside(image)) => ui.draw_sprite(&self.images[image], 1000.0, 0.0),
        None => {}
      }
      SceneAction::Continue
    }
  }

  fn engine(shows: Vec<Shows>) -> Option<HeadlessEngine> {
    crate::core::testing::shared_gpu().ok()?;
    let scene = Script {
      images: Vec::new(),
      shows,
      frame: 0,
    };
    let builder = EngineBuilder::new().resolution(320, 240).start_scene(scene);
    HeadlessEngine::new_or_skip(builder)
  }

  fn renderer(engine: &HeadlessEngine) -> &UiRenderer {
    engine.ui_renderer().expect("a UI renderer")
  }

  #[test]
  fn a_ui_that_records_nothing_holds_no_bind_group() {
    let Some(mut engine) = engine(vec![Shows::Image(0), Shows::Image(0)]) else {
      return;
    };
    engine.step().expect("a frame");
    engine.step().expect("a frame");
    assert!(
      engine.ui_render_stats().kept,
      "the second frame keeps the image"
    );
    assert_eq!(
      renderer(&engine).gpu.cached_groups(),
      1,
      "the image's group"
    );
    // No UI from here on, as a scene without one after a title screen
    for _ in 0..3 {
      engine.step().expect("a frame");
    }
    let ui = renderer(&engine);
    assert_eq!(
      ui.gpu.cached_groups(),
      0,
      "no UI: no group holds the image's texture"
    );
    assert!(ui.runs.is_empty(), "nor does a run");
  }

  #[test]
  fn a_kept_ui_holds_only_the_groups_its_image_was_drawn_with() {
    let shows = vec![
      Shows::Image(0),
      Shows::Image(1),
      Shows::Image(1),
      Shows::ImageAndRect(1),
    ];
    let Some(mut engine) = engine(shows) else {
      return;
    };
    engine.step().expect("a frame");
    engine.step().expect("a frame");
    let ui = renderer(&engine);
    assert_eq!(
      ui.gpu.cached_groups(),
      2,
      "the first image's group and the second's"
    );
    assert_eq!(ui.runs.len(), 1);
    let drawn_with = ui.runs[0].1.clone();

    engine.step().expect("a frame");
    assert!(
      engine.ui_render_stats().kept,
      "the third frame keeps the image"
    );
    assert_eq!(
      renderer(&engine).gpu.cached_groups(),
      1,
      "kept: only the group the image was drawn with"
    );

    // The UI changes again, and draws with the group it kept
    engine.step().expect("a frame");
    assert!(!engine.ui_render_stats().kept);
    let ui = renderer(&engine);
    assert_eq!(ui.runs.len(), 1);
    assert!(ui.runs[0].1 == drawn_with, "the kept group, not a new one");
    assert_eq!(ui.gpu.cached_groups(), 1);
  }

  #[test]
  fn a_ui_kept_with_all_of_it_culled_holds_no_bind_group() {
    let shows = vec![Shows::Image(0), Shows::Outside(0), Shows::Outside(0)];
    let Some(mut engine) = engine(shows) else {
      return;
    };
    engine.step().expect("a frame");
    engine.step().expect("a frame");
    assert!(renderer(&engine).build.instances.is_empty(), "all culled");
    engine.step().expect("a frame");
    assert!(engine.ui_render_stats().kept);
    let ui = renderer(&engine);
    assert_eq!(ui.gpu.cached_groups(), 0, "its image was drawn with none");
    assert!(ui.runs.is_empty());
  }
}
