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
      let texture = sprites.texture_of(sprite);
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

  let image = ui_image(&device);
  let mut encoder = device.create_command_encoder(&Default::default());
  {
    let view = image.create_view(&Default::default());
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
  let texture = sprites.texture_of(&gradient);
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
  gpu.upload(&device, &queue, &build.instances);
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
      let (style, look) = line_of(&command, z, &mut points).expect("a line");
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
    gpu.upload(device, queue, &instances);
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
  use crate::core::asset::SpriteHandle;
  use crate::core::graphics::{Sprite, SpriteGpuRenderer};
  use crate::core::render::batch::{Filter, Triangles, push_polyline};
  use crate::core::render::text::{TextSources, TextTargets, emit_text};
  use crate::core::render::ui::{
    line_of, rect_instance, scissor_of, sprite_instance, text_of, ui_z,
  };
  use crate::core::render::ui_retain::{UiRenderEnv, UiTextCache};
  use crate::core::render::{DrawList, UiSpace};
  use crate::core::text::{
    Fonts, GlyphRasterizer, Icon, PlacedIcon, TextEffects, TextEngine, TextIcons, TextLayout,
  };
  use crate::core::ui::immediate::{UiCommand, UiCommands};
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

  fn sprite(handle: &SpriteHandle, rect: Rect, filter: Filter, clip: Option<Rect>) -> UiCommand {
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
      commands.push(order, command);
    }
    commands
  }

  /// The new path's build of `commands`, retention off
  fn built(
    writes: &mut Writes,
    commands: &UiCommands,
    env: UiRenderEnv,
    sources: Option<&TextSources<'_>>,
  ) -> UiBuild {
    let mut build = UiBuild::default();
    let mut texts = UiTextCache::<UiInstance>::default();
    texts.begin(env, false);
    build_ui(
      &mut build,
      commands,
      &env,
      sources,
      &mut writes.targets(),
      0.0,
      &mut texts,
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
    for (order, command) in commands.as_slice() {
      let scissor = scissor_of(command.clip_rect(), space).expect("a clip that draws");
      let z = ui_z(command.draw_layer(), *order);
      if let Some((layout, place)) = text_of(command, space, z, scissor, 0.0) {
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
    let swatch = SpriteHandle::from_sprite(Sprite::solid_rect(4, 4, 255, 0, 0, 255), "swatch");
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
    commands.push(1, rect_at(10.0, 100.0, None, 1));
    for order in [5, 2, 9] {
      commands.push(order, rect_at(order as f32 * 10.0, 10.0, None, 0));
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
    commands.push(3, line(&[Vec2::new(5.0, 5.0), Vec2::new(60.0, 30.0)], None));
    commands.push(3, rect_at(10.0, 10.0, None, 0));
    let build = built(&mut writes, &commands, env(), None);
    let kinds: Vec<u32> = build.instances.iter().map(UiInstance::kind).collect();
    assert_eq!(kinds[0], UiKind::RECT);
    assert!(kinds.len() > 1 && kinds[1..].iter().all(|&kind| kind == UiKind::LINE));
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
    let (style, look) = line_of(&command, ui_z(0, 0), &mut poly).expect("a line");
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
    let swatch = SpriteHandle::from_sprite(Sprite::solid_rect(4, 4, 255, 0, 0, 255), "swatch");
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
    let sources = NoFonts::new();
    // The sprite's quad, and the icon's as the text emitters lay it
    let quad = sprite_instance(
      Rect::new(10.3, 10.0, 16.0, 12.0),
      [0.0, 0.0, 1.0, 1.0],
      Color::new(255, 230, 210, 250),
    )
    .expect("a sprite");
    let list = laid(&mut writes, &commands, env(), &sources.sources());
    let icon = list.sprite(0).expect("the icon is laid").instance;
    let texture = writes.sprites.texture_of(&swatch);
    let build = built(&mut writes, &commands, env(), Some(&sources.sources()));
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
    let swatch = SpriteHandle::from_sprite(Sprite::solid_rect(4, 4, 255, 0, 0, 255), "swatch");
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
    let texture = writes.sprites.texture_of(&swatch);
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
    let [a, b] = ["a", "b"]
      .map(|name| SpriteHandle::from_sprite(Sprite::solid_rect(4, 4, 255, 0, 0, 255), name));
    let on_layer = |handle: &SpriteHandle, x: f32, layer: u8| match sprite(
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
    let build = built(&mut writes, &commands, env(), None);
    let (a, b) = (writes.sprites.texture_of(&a), writes.sprites.texture_of(&b));
    assert_eq!(build.needs, [TexNeed::Sprite(b), TexNeed::Sprite(a)]);
    assert_eq!(build.sprite_textures(), [a, b, a]);
  }

  /// Built-in fonts: texts as glyphs
  #[cfg(feature = "default-fonts")]
  mod texts {
    use super::*;
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
            Some(sources),
            &mut writes.targets(),
            0.0,
            &mut texts,
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
            Some(sources),
            &mut writes.targets(),
            0.0,
            &mut texts,
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
            // any move of an MSDF glyph (it has none)
            if vertical || msdf {
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
  use crate::core::ui::{Color, Rect};

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
    gpu.upload(&device, &queue, &first);
    // The same 600 first, then more than the buffer holds
    let grown = numbered(0, 3000);
    gpu.upload(&device, &queue, &grown);
    assert_eq!(gpu.written(), 0..3000);
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
      let written = ui.gpu.written();
      partial += usize::from(!written.is_empty() && written.len() < instances.len());
    }
    assert!(partial >= 6, "{partial} frames wrote only what changed");
  }
}
