//! A skeleton's meshes drawn through the world batch: skinned to their
//! bones, exactly as a region of the same image draws, in the draw order,
//! sorted, lit, lifted, mirrored, shadowed, culled and drawn between steps,
//! and moved by their animations' deform keys, read back as display bytes.
//! The rigs and the frame helpers are the skeleton pixel tests' own
//! (`skeleton.rs`).
//!
//! Tests skip when no GPU is available.

use std::f32::consts::FRAC_PI_2;
use std::sync::Arc;

use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

use super::skeleton::{
  BLACK, BLUE, Box4, CELL, GREEN, INVERT, Images, Part, RED, SHADOW_ONCE, SIZE, Stage, WHITE,
  YELLOW, among_trees, assert_boxes, assert_pixels, bone, engine, engine_with, frame, inside, land,
  near, part, part_step, quad_mesh, rig, rig_with, settled, sliding, solid, square, stand,
  step_until, stepped, time, two_tone, white_floor,
};

#[test]
fn an_unweighted_mesh_draws_where_its_bone_puts_it() {
  let Some(mut engine) = engine() else {
    return;
  };
  // The rotated-bone test's rig with its arm a mesh: a body 8 px square on
  // the root, and an arm 10 px along it turned a quarter turn clockwise
  // (pointing down the screen), whose 12 x 4 rectangle, centred 6 px along
  // it, is two triangles. And a fin that no quad draws: a right triangle
  // with its corner at (-24, -24) from the body and legs 8.5 px long, so
  // that no pixel's centre lies on its slanted edge. All three are parts of
  // one image, each its own rectangle of it
  let world = engine.world_mut();
  let corner = vec![Vec2::ZERO, Vec2::new(8.5, 0.0), Vec2::new(0.0, 8.5)];
  let parts = [
    part("body", GREEN, (8, 8)),
    part("arm", RED, (12, 4))
      .under("body", 10.0, 0.0)
      .turned(FRAC_PI_2)
      .at(6.0, 0.0)
      .in_triangles(),
    part("fin", BLUE, (8, 8))
      .under("body", -24.0, -24.0)
      .meshed(
        MeshVertices::Unweighted(corner),
        vec![Vec2::ZERO, Vec2::X, Vec2::Y],
        vec![0, 1, 2],
      ),
  ];
  let data = rig_with(world, &parts, Images::Atlas, |builder| builder);
  let hero = stand(world, data, (32.0, 32.0), Renderable::skeleton());
  let drawn = stepped(&mut engine);
  // The fin's corner is at (8, 8): a pixel is in it when its centre is on
  // the corner's side of the slanted edge, less than 8.5 px along both legs
  // together
  let fin = |x: u32, y: u32| x >= 8 && y >= 8 && (x - 8) + (y - 8) <= 7;
  assert_pixels(
    &drawn,
    |x, y| {
      if inside(x, y, (40, 32, 4, 12)) {
        Some(RED)
      } else if inside(x, y, (28, 28, 8, 8)) {
        Some(GREEN)
      } else {
        fin(x, y).then_some(BLUE)
      }
    },
    "standing at (32, 32)",
  );

  // The entity's own transform carries every vertex, as it carries a
  // region: moved, turned a quarter turn and doubled, the body is 16 px
  // square about the entity and the arm spans x -24..0 and y 16..24 from
  // it (the fin is out of view)
  let mut transform = engine.world_mut().get_mut::<Transform>(hero).unwrap();
  *transform = Transform::from_xy(40.0, 20.0)
    .with_rotation(FRAC_PI_2)
    .with_scale(Vec2::splat(2.0));
  let drawn = settled(&mut engine);
  assert_boxes(
    &drawn,
    &[((32, 12, 16, 16), GREEN), ((16, 36, 24, 8), RED)],
    "turned and doubled",
  );
}

#[test]
fn a_weighted_mesh_bends_between_two_bones() {
  let Some(mut engine) = engine() else {
    return;
  };
  // A strip 16 x 4 px between two bones 16 px apart: its left end rides
  // "near", its right end "far", and the two vertices in its middle are
  // half each (their weights, 3 and 3, are made to sum to 1 as the data is
  // built). A vertex's place is given in the space of each bone that pulls
  // on it
  let world = engine.world_mut();
  let (near, far) = (BoneId(0), BoneId(1));
  let row = |y: f32| {
    [
      vec![SkeletonInfluence::new(near, 0.0, y, 1.0)],
      vec![
        SkeletonInfluence::new(near, 8.0, y, 3.0),
        SkeletonInfluence::new(far, -8.0, y, 3.0),
      ],
      vec![SkeletonInfluence::new(far, 0.0, y, 1.0)],
    ]
  };
  let [a, b, c] = row(-2.0);
  let [d, e, f] = row(2.0);
  let strip = MeshVertices::Weighted(WeightedVertices::new([a, b, c, d, e, f]));
  let uvs = [
    (0.0, 0.0),
    (0.5, 0.0),
    (1.0, 0.0),
    (0.0, 1.0),
    (0.5, 1.0),
    (1.0, 1.0),
  ];
  let uvs = uvs.map(|(u, v)| Vec2::new(u, v)).to_vec();
  let triangles = vec![0, 1, 3, 1, 4, 3, 1, 2, 4, 2, 5, 4];
  let unseen = Part {
    colour: [0, 0, 0, 0],
    ..part("far", WHITE, (1, 1))
  };
  let parts = [
    part("near", RED, (16, 4)).meshed(strip, uvs, triangles),
    unseen.under("near", 16.0, 0.0),
  ];
  let data = rig(world, &parts);
  let hero = stand(world, data, (16.0, 32.0), Renderable::skeleton());
  assert_boxes(
    &stepped(&mut engine),
    &[((16, 30, 16, 4), RED)],
    "the setup pose: a straight strip",
  );

  // The far bone moves 8 px down (then up): the left end stays, the right
  // end goes with it, and the middle, half of each, goes 4 px. So the
  // strip slants evenly from one bone to the other
  let pose = |engine: &mut HeadlessEngine, value: BoneOverride| {
    let mut skeleton = engine.world_mut().get_mut::<Skeleton>(hero).unwrap();
    skeleton.override_bone("far", value);
    settled(engine)
  };
  for rise in [8.0, -8.0] {
    let moved = BoneOverride {
      translation: Some(Vec2::new(16.0, rise)),
      ..BoneOverride::default()
    };
    let drawn = pose(&mut engine, moved);
    assert_pixels(
      &drawn,
      |x, y| {
        if !(16..32).contains(&x) {
          return None;
        }
        // Its top edge where this column's centre crosses it
        let top = 30.0 + rise * (x as f32 + 0.5 - 16.0) / 16.0;
        let centre = y as f32 + 0.5;
        (centre > top && centre < top + 4.0).then_some(RED)
      },
      &format!("the far bone moved {rise} px"),
    );
  }

  // The far bone back in its place and turned a quarter turn clockwise:
  // its end of the strip turns with it about its origin, to span x 14..18
  // at the origin's height, and each middle vertex is half way between
  // where the near bone holds it and where the far bone's turn takes it:
  // (13, -5) and (11, -3) from the near bone. The strip folds
  let turned = BoneOverride {
    rotation: Some(FRAC_PI_2),
    ..BoneOverride::default()
  };
  let drawn = pose(&mut engine, turned);
  let background = drawn.get_pixel(SIZE - 1, SIZE - 1).0;
  let at = |x: u32, y: u32| drawn.get_pixel(x, y).0;
  assert_eq!(at(20, 30), RED, "the near end, where it was");
  assert_eq!(at(30, 29), RED, "the fold, above where the strip lay");
  assert_eq!(
    at(28, 33),
    background,
    "under the fold, where the strip lay"
  );
  assert_eq!(at(31, 31), RED, "the far end, turned about its bone");
  assert_eq!(at(31, 33), background, "nothing below the far bone");
}

/// An image 16 x 12 px whose every texel has a colour of its own
fn patterned(world: &mut World) -> Handle<Image> {
  let data: Vec<u8> = (0..16 * 12u32)
    .flat_map(|i| {
      let (x, y) = (i % 16, i / 16);
      [40 + x * 13, 30 + y * 17, 90 + x * 5 + y * 7, 255].map(|c| c as u8)
    })
    .collect();
  world
    .resource_mut::<Assets<Image>>()
    .add(Image::from_rgba(data, 16, 12))
}

/// A skeleton whose one slot shows an 8 px square of `image` (3 px in and
/// 2 px down from its corner), centred on its root: as a region
/// attachment, or as a mesh of two triangles. Its slot and its attachment
/// each have a colour, and its slot blends as `blend` says
fn one_part(
  world: &mut World,
  image: &Handle<Image>,
  mesh: bool,
  blend: SlotBlend,
) -> Handle<SkeletonData> {
  one_part_on(world, image, mesh, blend, BoneSetup::default())
}

/// [`one_part`] whose root bone is set up as `root`
fn one_part_on(
  world: &mut World,
  image: &Handle<Image>,
  mesh: bool,
  blend: SlotBlend,
  root: BoneSetup,
) -> Handle<SkeletonData> {
  let color = Color::new(255, 200, 150, 230);
  let attachment = if mesh {
    SkeletonAttachment::Mesh(MeshAttachment {
      color,
      ..quad_mesh(0, Vec2::ZERO, Vec2::splat(8.0))
    })
  } else {
    SkeletonAttachment::Region(RegionAttachment {
      color,
      ..RegionAttachment::new(0, 8.0, 8.0)
    })
  };
  let shown = SlotSetup {
    color: Color::new(200, 255, 180, 255),
    attachment: Some(Arc::from("body")),
    blend,
  };
  let data = SkeletonData::builder()
    .bone("root", None, root)
    .slot("body", "root", shown)
    .region(SkeletonRegion {
      name: Arc::from("body"),
      image: image.clone(),
      x: 3,
      y: 2,
      width: 8,
      height: 8,
    })
    .attachment(SkeletonData::DEFAULT_SKIN, "body", "body", attachment)
    .build()
    .expect("a valid skeleton");
  world.resource_mut::<Assets<SkeletonData>>().add(data)
}

#[test]
fn a_mesh_draws_as_the_region_of_its_image() {
  // Two skeletons whose one slot shows the same rectangle of a packed
  // image: one as a region attachment, one as a mesh of two triangles.
  // Whatever the renderable and the transform say, the two draw the same
  // pixels. They stand on whole pixels: a quad is snapped to them on a
  // pixel-art target, and a mesh's vertices draw where they are
  let Some(mut engine) = engine() else {
    return;
  };
  let world = engine.world_mut();
  white_floor(world);
  let image = patterned(world);
  world.spawn((
    Transform::from_xy(48.0, 48.0),
    Light2D::point(Color::rgb(255, 240, 200), 40.0),
  ));
  // Neither in view: what the floor alone looks like
  let hidden = Transform::from_xy(-500.0, -500.0);
  let data = [false, true].map(|mesh| one_part(world, &image, mesh, SlotBlend::Normal));
  let [region, mesh] =
    data.map(|data| stand(world, data, (-500.0, -500.0), Renderable::skeleton()));
  // The same two on a bone of their own that is turned a quarter turn and
  // twice as long as it is tall, which none of the places below makes an
  // entity: the square is 8 px wide and 16 px tall, still on whole pixels
  let lying = BoneSetup {
    rotation: FRAC_PI_2,
    scale_x: 2.0,
    ..BoneSetup::default()
  };
  let data = [false, true].map(|mesh| one_part_on(world, &image, mesh, SlotBlend::Normal, lying));
  let on_a_bone = data.map(|data| stand(world, data, (-500.0, -500.0), Renderable::skeleton()));
  engine.step_frames(2).unwrap();
  let empty = frame(&engine);

  type Dress = fn(Renderable) -> Renderable;
  let looks: [(&str, Dress); 9] = [
    ("tinted and faded", |r| {
      r.with_tint(Color::new(255, 128, 64, 200)).with_alpha(0.6)
    }),
    ("flipped left to right", |r| r.with_flip_x(true)),
    ("flipped both ways", |r| {
      r.with_flip_x(true).with_flip_y(true)
    }),
    ("a drop shadow", |r| {
      r.with_drop_shadow(DropShadow::new().offset(5.0, 3.0))
    }),
    ("a squashed drop shadow", |r| {
      r.with_drop_shadow(DropShadow::new().offset(4.0, 6.0).scale(1.0, 0.5))
    }),
    ("unlit", |r| r.with_lighting(false)),
    ("additive", |r| r.with_blend(BlendMode::Additive)),
    ("glowing", |r| r.with_emission(3.0)),
    ("flashing", |r| {
      r.with_shader(SpriteShader::flash(Color::WHITE, 0.5))
    }),
  ];
  let places = [
    ("upright", Transform::from_xy(30.0, 30.0), [region, mesh]),
    (
      "twice as large",
      Transform::from_xy(30.0, 30.0).with_scale(Vec2::splat(2.0)),
      [region, mesh],
    ),
    (
      "a quarter turn round",
      Transform::from_xy(30.0, 30.0).with_rotation(FRAC_PI_2),
      [region, mesh],
    ),
    (
      "mirrored by its scale",
      Transform::from_xy(30.0, 30.0).with_scale(Vec2::new(-1.0, 1.0)),
      [region, mesh],
    ),
    (
      "on a turned, stretched bone",
      Transform::from_xy(30.0, 30.0),
      on_a_bone,
    ),
    (
      "on a turned, stretched bone, turned again",
      Transform::from_xy(30.0, 30.0).with_rotation(FRAC_PI_2),
      on_a_bone,
    ),
  ];
  for (place, transform, [region, mesh]) in places {
    // The pair that stood in the last place is out of view
    for entity in [region, mesh].into_iter().chain(on_a_bone) {
      *engine.world_mut().get_mut::<Transform>(entity).unwrap() = hidden;
    }
    // One of the two dressed and in place, the other out of view
    let shown = |engine: &mut HeadlessEngine, dress: Dress, which: Entity, other: Entity| {
      let world = engine.world_mut();
      *world.get_mut::<Renderable>(which).unwrap() = dress(Renderable::skeleton());
      *world.get_mut::<Transform>(which).unwrap() = transform;
      *world.get_mut::<Transform>(other).unwrap() = hidden;
      engine.step_frames(2).unwrap();
      frame(engine)
    };
    let same = |as_region: &image::RgbaImage, as_mesh: &image::RgbaImage, look: &str| {
      for (x, y, pixel) in as_region.enumerate_pixels() {
        let got = as_mesh.get_pixel(x, y).0;
        assert_eq!(got, pixel.0, "{place}, {look}: pixel ({x}, {y})");
      }
    };
    // Plain first: the region draws, and the mesh draws the same
    let plain = shown(&mut engine, |r| r, region, mesh);
    let drew = plain
      .pixels()
      .zip(empty.pixels())
      .filter(|(drawn, empty)| drawn != empty)
      .count();
    assert!(drew >= 64, "{place}: the region drew ({drew} pixels)");
    same(&plain, &shown(&mut engine, |r| r, mesh, region), "plain");
    for (look, dress) in looks {
      // The look shows on the region, or the comparison would prove nothing
      let as_region = shown(&mut engine, dress, region, mesh);
      assert!(
        as_region != plain,
        "{place}, {look}: the look changes how the region draws"
      );
      same(&as_region, &shown(&mut engine, dress, mesh, region), look);
    }
  }

  // A slot with a blend of its own: the mesh in it adds light as the
  // region in it does, whatever the renderable's blend
  let world = engine.world_mut();
  for entity in [region, mesh].into_iter().chain(on_a_bone) {
    *world.get_mut::<Transform>(entity).unwrap() = hidden;
  }
  let data = [false, true].map(|mesh| one_part(world, &image, mesh, SlotBlend::Additive));
  let adding = data.map(|data| stand(world, data, (-500.0, -500.0), Renderable::skeleton()));
  let upright = Transform::from_xy(30.0, 30.0);
  let frames = [region, adding[0], adding[1]].map(|which| {
    *engine.world_mut().get_mut::<Transform>(which).unwrap() = upright;
    engine.step_frames(2).unwrap();
    let drawn = frame(&engine);
    *engine.world_mut().get_mut::<Transform>(which).unwrap() = hidden;
    drawn
  });
  let [alpha, as_region, as_mesh] = frames;
  assert!(
    as_region != alpha,
    "the slot's blend changes how the region draws"
  );
  for (x, y, pixel) in as_region.enumerate_pixels() {
    let got = as_mesh.get_pixel(x, y).0;
    assert_eq!(got, pixel.0, "a slot that adds light: pixel ({x}, {y})");
  }
}

/// Three parts 16 px square, red, green, then blue, each 8 px right of the
/// one before and drawn over its right half; those `meshes` marks are
/// meshes of two triangles, the others regions. Standing at (32, 32) they
/// are at (16, 24), (24, 24) and (32, 24)
fn overlapping(meshes: [bool; 3]) -> Vec<Part> {
  let colours = [("a", RED), ("b", GREEN), ("c", BLUE)];
  let each = colours.into_iter().zip(meshes).enumerate();
  each
    .map(|(index, ((name, colour), mesh))| {
      let part = part(name, colour, (16, 16)).at(index as f32 * 8.0 - 8.0, 0.0);
      if mesh { part.in_triangles() } else { part }
    })
    .collect()
}

#[test]
fn meshes_and_regions_keep_the_draw_order_across_images() {
  let Some(mut engine) = engine() else {
    return;
  };
  // Three parts 16 px square over a white floor, red, green, then blue,
  // each 8 px right of the one before and drawn over its right half
  white_floor(engine.world_mut());
  let floor = ((0, 0, SIZE - 1, SIZE), WHITE);
  let (a, b, c) = (
    ((16, 24, 16, 16), RED),
    ((24, 24, 16, 16), GREEN),
    ((32, 24, 16, 16), BLUE),
  );
  let parts = overlapping;
  // Which of the three are meshes: one between two regions, a region
  // between two meshes, a skeleton that is all meshes, and a mesh drawn
  // last and one drawn first. Each on three images, which end a run of
  // regions at every part, and on one, where only a mesh ends one
  let cases = [
    [false, true, false],
    [true, false, true],
    [true, true, true],
    [false, false, true],
    [true, false, false],
  ];
  for (images, packed) in [(Images::Each, "three images"), (Images::Atlas, "one image")] {
    for meshes in cases {
      let world = engine.world_mut();
      let data = rig_with(world, &parts(meshes), images, |builder| builder);
      let hero = stand(world, data, (32.0, 32.0), Renderable::skeleton());
      let what = format!("{packed}, meshes {meshes:?}");
      assert_boxes(&stepped(&mut engine), &[floor, a, b, c], &what);

      // The drop shadow covers every part, and every part is over every
      // shadow: 12 px left and 4 px down, the green part's shadow and the
      // blue part's fall on the red part, which is drawn first and stays
      // red
      let shadow = DropShadow::new()
        .offset(-12.0, 4.0)
        .color(Color::new(0, 0, 0, 255));
      let mut renderable = engine.world_mut().get_mut::<Renderable>(hero).unwrap();
      renderable.drop_shadow = Some(shadow);
      let shadows = [(4, 28, 16, 16), (12, 28, 16, 16), (20, 28, 16, 16)].map(|area| (area, BLACK));
      let [under_a, under_b, under_c] = shadows;
      assert_boxes(
        &stepped(&mut engine),
        &[floor, under_a, under_b, under_c, a, b, c],
        &format!("{what}, with a drop shadow"),
      );
      engine.world_mut().despawn(hero);
    }
  }

  // A draw-order key puts the mesh and the regions the other way round
  let world = engine.world_mut();
  let data = rig_with(
    world,
    &parts([false, true, false]),
    Images::Each,
    |builder| {
      builder.animation(SkeletonClip {
        name: Arc::from("swap"),
        duration: 1.0,
        timelines: vec![SkeletonTimeline::DrawOrder {
          frames: vec![(0.0, Some(vec![SlotId(2), SlotId(1), SlotId(0)]))],
        }],
      })
    },
  );
  let hero = stand(world, data, (32.0, 32.0), Renderable::skeleton());
  let mut animation = SkeletonAnimation::new();
  animation.set_animation(0, "swap", true);
  world.entity_mut(hero).insert(animation);
  assert_boxes(&stepped(&mut engine), &[floor, c, b, a], "the keyed order");
}

#[test]
fn a_shaded_skeleton_keeps_its_mesh_in_the_draw_order() {
  let Some(mut engine) = engine() else {
    return;
  };
  // The three overlapping parts over a white floor, under a game's own
  // sprite shader, which inverts each part's colour. With a game shader
  // every region is a sprite item of its own and a mesh an item among
  // them, and they must still draw in the skeleton's order: each part
  // over the right half of the one before
  white_floor(engine.world_mut());
  let floor = ((0, 0, SIZE - 1, SIZE), WHITE);
  let inverted = |[r, g, b, _]: [u8; 4]| [255 - r, 255 - g, 255 - b, 255];
  let (a, b, c) = (
    ((16, 24, 16, 16), inverted(RED)),
    ((24, 24, 16, 16), inverted(GREEN)),
    ((32, 24, 16, 16), inverted(BLUE)),
  );
  let invert = engine
    .world_mut()
    .resource_mut::<Assets<Shader>>()
    .add(Shader::from_wgsl("skeleton-mesh-order.wgsl", INVERT));
  // The mesh between two regions, drawn first, and drawn last; on three
  // images and on one
  let cases = [
    [false, true, false],
    [true, false, false],
    [false, false, true],
  ];
  for (images, packed) in [(Images::Each, "three images"), (Images::Atlas, "one image")] {
    for meshes in cases {
      let world = engine.world_mut();
      let data = rig_with(world, &overlapping(meshes), images, |builder| builder);
      let mut renderable = Renderable::skeleton();
      renderable.shader = Some(SpriteShader::custom(invert.clone()));
      let hero = stand(world, data, (32.0, 32.0), renderable);
      let what = format!("shaded, {packed}, meshes {meshes:?}");
      // A part draws plain until the shader's pipeline for it is accepted,
      // a mesh's apart from a quad's: wait for every part's own strip
      step_until(
        &mut engine,
        |drawn| {
          let strip = |x: u32, colour: [u8; 4]| drawn.get_pixel(x, 32).0 == colour;
          strip(20, a.1) && strip(28, b.1) && strip(44, c.1)
        },
        &what,
      );
      assert_boxes(&frame(&engine), &[floor, a, b, c], &what);
      engine.world_mut().despawn(hero);
    }
  }
}

#[test]
fn an_effect_on_a_deformed_mesh_shows_where_the_deform_puts_it() {
  let Some(mut engine) = engine() else {
    return;
  };
  // A red flag 16 x 8 px on a pole at (16, 32), reaching right, its right
  // corners held 8 px down by a deform key: it slants evenly from its
  // pole, so part of its setup shape is left bare and part of what it
  // covers lies outside that shape
  let world = engine.world_mut();
  let data = flagged(world, 0.0);
  let flag = stand(world, data, (16.0, 32.0), Renderable::skeleton());
  let mut state = SkeletonAnimation::new();
  state.set_animation(0, "slant", true);
  world.entity_mut(flag).insert(state);
  let slanted = |x: u32, y: u32| {
    let edge = 28.0 + 8.0 * (x as f32 + 0.5 - 16.0) / 16.0;
    let centre = y as f32 + 0.5;
    (16..32).contains(&x) && centre > edge && centre < edge + 8.0
  };
  let drawn = settled(&mut engine);
  assert_pixels(&drawn, |x, y| slanted(x, y).then_some(RED), "slanted");
  // A hit flash whitens the flag where the deform puts it, every pixel of
  // the slanted shape and none of the setup shape's that it has left
  let mut renderable = engine.world_mut().get_mut::<Renderable>(flag).unwrap();
  renderable.shader = Some(SpriteShader::flash(Color::WHITE, 1.0));
  let drawn = stepped(&mut engine);
  assert_pixels(
    &drawn,
    |x, y| slanted(x, y).then_some(WHITE),
    "slanted and flashing",
  );
  assert_ne!(drawn.get_pixel(30, 29).0, WHITE, "the setup shape's corner");
}

#[test]
fn a_mesh_follows_ysort_light_and_floor_as_its_regions_do() {
  // A character in two halves, each `width` by `height` px, standing on
  // its origin: its left half a region, its right half a mesh of two
  // triangles
  let halves = |world: &mut World, colour: [u8; 4], (width, height): (u32, u32)| {
    let (x, y) = (width as f32 / 2.0, -(height as f32) / 2.0);
    let parts = [
      part("left", colour, (width, height)).at(-x, y),
      part("right", colour, (width, height))
        .at(x, y)
        .in_triangles(),
    ];
    rig(world, &parts)
  };

  // Among two y-sorted trees it sorts by its feet, both halves together
  let Some(mut engine) = engine() else {
    return;
  };
  let world = engine.world_mut();
  let tree = |world: &mut World, colour, (x, y): (f32, f32)| {
    let image = solid(world, (8, 16), colour);
    world.spawn((
      Transform::from_xy(x, y),
      Renderable::image(image)
        .with_anchor(Anchor::TopLeft)
        .with_z_order(1),
      YSort::new(),
    ));
  };
  tree(world, RED, (16.0, 16.0));
  let data = halves(world, BLUE, (8, 16));
  let hero = stand(
    world,
    data,
    (24.0, 40.0),
    Renderable::skeleton().with_z_order(1),
  );
  world.entity_mut(hero).insert(YSort::new());
  tree(world, YELLOW, (24.0, 32.0));
  for feet in [40, 30, 52] {
    let mut transform = engine.world_mut().get_mut::<Transform>(hero).unwrap();
    transform.translation.y = feet as f32;
    let what = format!("feet at {feet}");
    assert_boxes(&settled(&mut engine), &among_trees(feet), &what);
  }

  // On a map whose floor 1 is 32 px up, both halves draw lifted
  let Some(mut engine) = engine_with(|ctx| {
    let mut tiles = TileMap::new(UVec2::splat(SIZE / CELL), UVec2::splat(CELL));
    tiles.add_tile_layer("ground");
    let upper = tiles.add_tile_layer("upper");
    tiles.layers[upper].info.floor = 1;
    tiles.layers[upper].info.offset = Vec2::new(0.0, -32.0);
    let tiles = ctx.assets().add(tiles);
    ctx
      .world
      .spawn((MapRoot::new(tiles).with_z(4), Transform::IDENTITY));
  }) else {
    return;
  };
  land(&mut engine);
  let world = engine.world_mut();
  let data = halves(world, WHITE, (4, 8));
  let hero = stand(
    world,
    data,
    (32.0, 52.0),
    Renderable::skeleton().with_z_order(5),
  );
  world.entity_mut(hero).insert(Floor::On(1));
  engine.step_frames(2).unwrap();
  assert_boxes(
    &frame(&engine),
    &[((28, 12, 4, 8), WHITE), ((32, 12, 4, 8), WHITE)],
    "on floor 1: the region and the mesh 32 px up",
  );
  engine.world_mut().entity_mut(hero).insert(Floor::GROUND);
  engine.step_frames(2).unwrap();
  assert_boxes(
    &frame(&engine),
    &[((28, 44, 4, 8), WHITE), ((32, 44, 4, 8), WHITE)],
    "on the ground: where it stands",
  );

  // In the dark, a light between the halves where they draw on floor 1
  // lights both alike, and a second character across the frame stays
  // dark; unlit, both of that one's halves draw at full brightness
  let world = engine.world_mut();
  world.entity_mut(hero).insert(Floor::On(1));
  world
    .resource_mut::<LightingSettings>()
    .set_ambient(AmbientLight::with_intensity(0.0));
  world.spawn((
    Transform::from_xy(32.0, 16.0),
    Light2D::point(Color::WHITE, 14.0),
  ));
  let data = halves(world, WHITE, (4, 8));
  let far = stand(
    world,
    data,
    (8.0, 56.0),
    Renderable::skeleton().with_z_order(5),
  );
  engine.step_frames(2).unwrap();
  let lit = frame(&engine);
  let (region, mesh) = (lit.get_pixel(30, 16).0, lit.get_pixel(33, 16).0);
  assert!(
    region[0] > 200,
    "the region, lit where it draws: {region:?}"
  );
  assert_eq!(mesh, region, "the mesh, as far from the light, lit as much");
  let (region, mesh) = (lit.get_pixel(6, 52).0, lit.get_pixel(10, 52).0);
  assert!(region[0] < 10, "dark away from the light: {region:?}");
  assert_eq!(mesh, region, "the mesh as dark");
  engine
    .world_mut()
    .get_mut::<Renderable>(far)
    .unwrap()
    .lighting = false;
  engine.step_frames(2).unwrap();
  let unlit = frame(&engine);
  assert_eq!(unlit.get_pixel(6, 52).0, WHITE, "the region, unlit");
  assert_eq!(unlit.get_pixel(10, 52).0, WHITE, "the mesh, unlit");
}

/// A rig with nothing mirrored about it: an arm at (10, -6) pointing
/// right, and on it a flag, a mesh of two triangles 8 x 6 px rightward
/// from the arm's origin showing the two-tone image (a blue top row over
/// a red left half and a green right half), and under the flag's left
/// half a yellow tag 4 px square, a region
fn banner(world: &mut World) -> Handle<SkeletonData> {
  let flag = two_tone(world);
  let tag = solid(world, (4, 4), YELLOW);
  let shown = |name: &str| SlotSetup {
    attachment: Some(Arc::from(name)),
    ..SlotSetup::default()
  };
  let region = |name: &str, image: Handle<Image>, (width, height): (u32, u32)| SkeletonRegion {
    name: Arc::from(name),
    image,
    x: 0,
    y: 0,
    width,
    height,
  };
  let arm = BoneSetup {
    x: 10.0,
    y: -6.0,
    ..BoneSetup::default()
  };
  let skin = SkeletonData::DEFAULT_SKIN;
  let data = SkeletonData::builder()
    .bone("root", None, BoneSetup::default())
    .bone("arm", Some("root"), arm)
    .slot("flag", "arm", shown("flag"))
    .slot("tag", "arm", shown("tag"))
    .region(region("flag", flag, (8, 6)))
    .region(region("tag", tag, (4, 4)))
    .attachment(
      skin,
      "flag",
      "flag",
      SkeletonAttachment::Mesh(quad_mesh(0, Vec2::new(4.0, 0.0), Vec2::new(8.0, 6.0))),
    )
    .attachment(
      skin,
      "tag",
      "tag",
      SkeletonAttachment::Region(RegionAttachment {
        x: 2.0,
        y: 5.0,
        ..RegionAttachment::new(1, 4.0, 4.0)
      }),
    )
    .build()
    .expect("a valid skeleton");
  world.resource_mut::<Assets<SkeletonData>>().add(data)
}

/// What a [`banner`] draws: its flag's blue row, its red half and its
/// green half, and its tag, each where it is given
fn banner_boxes(blue: Box4, red: Box4, green: Box4, tag: Box4) -> [(Box4, [u8; 4]); 4] {
  [(blue, BLUE), (red, RED), (green, GREEN), (tag, YELLOW)]
}

#[test]
fn a_mesh_mirrors_with_its_skeleton_and_under_a_mirrored_bone() {
  let Some(mut engine) = engine() else {
    return;
  };
  let world = engine.world_mut();
  let data = banner(world);
  let hero = stand(world, data, (32.0, 32.0), Renderable::skeleton());
  // As built: the flag right of and above the entity, red before green
  // along the arm, its blue row on top; the tag under its red half
  let as_built = banner_boxes(
    (42, 23, 8, 1),
    (42, 24, 4, 5),
    (46, 24, 4, 5),
    (42, 29, 4, 4),
  );
  assert_boxes(&stepped(&mut engine), &as_built, "unflipped");

  // Flipped left to right about the entity's origin, the mesh mirrors as
  // the region does: its triangles wind the other way and are drawn all
  // the same, its texels mirrored with its vertices
  let flip = |engine: &mut HeadlessEngine, x: bool, y: bool| {
    let mut renderable = engine.world_mut().get_mut::<Renderable>(hero).unwrap();
    renderable.flip_x = x;
    renderable.flip_y = y;
  };
  let flipped = banner_boxes(
    (14, 23, 8, 1),
    (18, 24, 4, 5),
    (14, 24, 4, 5),
    (18, 29, 4, 4),
  );
  flip(&mut engine, true, false);
  assert_boxes(&stepped(&mut engine), &flipped, "flipped left to right");
  // Upside down as well: wound the first way again, the blue row below
  flip(&mut engine, true, true);
  assert_boxes(
    &stepped(&mut engine),
    &banner_boxes(
      (14, 40, 8, 1),
      (18, 35, 4, 5),
      (14, 35, 4, 5),
      (18, 31, 4, 4),
    ),
    "flipped both ways",
  );

  // A negative scale on the entity mirrors it as the flip does
  flip(&mut engine, false, false);
  engine.world_mut().get_mut::<Transform>(hero).unwrap().scale = Vec2::new(-1.0, 1.0);
  assert_boxes(&settled(&mut engine), &flipped, "mirrored by the entity");

  // The arm's own scale mirrored (a character turning round): the flag
  // and the tag run leftward from the arm's origin, the rest as built
  engine.world_mut().get_mut::<Transform>(hero).unwrap().scale = Vec2::ONE;
  let mirrored = BoneOverride {
    scale: Some(Vec2::new(-1.0, 1.0)),
    ..BoneOverride::default()
  };
  let mut skeleton = engine.world_mut().get_mut::<Skeleton>(hero).unwrap();
  skeleton.override_bone("arm", mirrored);
  assert_boxes(
    &settled(&mut engine),
    &banner_boxes(
      (34, 23, 8, 1),
      (38, 24, 4, 5),
      (34, 24, 4, 5),
      (38, 29, 4, 4),
    ),
    "under a mirrored bone",
  );
  // And both at once: mirrored twice, it reads as built, from the arm's
  // origin as the flip puts it
  flip(&mut engine, true, false);
  assert_boxes(
    &stepped(&mut engine),
    &banner_boxes(
      (22, 23, 8, 1),
      (22, 24, 4, 5),
      (26, 24, 4, 5),
      (22, 29, 4, 4),
    ),
    "a mirrored bone on a flipped skeleton",
  );
}

/// A rig whose hand slides right 20 px a step on a still body with no art,
/// three white 4 px squares riding it: a region on the hand; 8 px below
/// it a mesh of two triangles on a bone of the hand's; and 16 px below it
/// a mesh in a slot on a still bone, every vertex weighted to the hand
fn mesh_slider(world: &mut World) -> Handle<SkeletonData> {
  let hand = BoneId(1);
  let corner = |x: f32, y: f32| vec![SkeletonInfluence::new(hand, x, 16.0 + y, 1.0)];
  let corners = [
    corner(-2.0, -2.0),
    corner(2.0, -2.0),
    corner(-2.0, 2.0),
    corner(2.0, 2.0),
  ];
  let unseen = Part {
    colour: [0, 0, 0, 0],
    ..part("body", WHITE, (1, 1))
  };
  let parts = [
    unseen,
    part("hand", WHITE, (4, 4)).under("body", 0.0, 0.0),
    part("glove", WHITE, (4, 4))
      .under("hand", 0.0, 8.0)
      .in_triangles(),
    part("cuff", WHITE, (4, 4)).under("body", 0.0, 0.0).meshed(
      MeshVertices::Weighted(WeightedVertices::new(corners)),
      vec![Vec2::ZERO, Vec2::X, Vec2::Y, Vec2::ONE],
      vec![0, 1, 2, 1, 3, 2],
    ),
  ];
  rig_with(world, &parts, Images::Each, |builder| {
    builder.animation(SkeletonClip {
      name: Arc::from("slide"),
      duration: 10.0,
      timelines: vec![SkeletonTimeline::Translate {
        bone: hand,
        frames: vec![
          (0.0, Vec2::ZERO, SkeletonCurve::Linear),
          (10.0, Vec2::new(12_000.0, 0.0), SkeletonCurve::Linear),
        ],
      }],
    })
  })
}

#[test]
fn a_mesh_is_interpolated_between_steps() {
  let Some(mut engine) = engine() else {
    return;
  };
  // Two skeletons whose hands slide 20 px a step, each with a region, a
  // mesh and a weighted mesh riding the hand, one under the other. One is
  // interpolated, as a skeleton's entity is; the other has had
  // `Interpolated` taken off it and draws at the step rate
  let world = engine.world_mut();
  let data = mesh_slider(world);
  let smooth = sliding(world, &data, (4.0, 8.0));
  let plain = sliding(world, &data, (4.0, 40.0));
  world.entity_mut(plain).remove::<Interpolated>();
  engine.step_frames(2).unwrap();
  let now = bone(&engine, smooth, "hand").translation.x;
  assert!(near(now, 44.0), "two steps of 20 px from x 4: {now}");
  let rows = [
    (0.0, "the region"),
    (8.0, "the mesh"),
    (16.0, "the weighted mesh"),
  ];
  // Every part of the interpolated one is at `at`, and none at `not`;
  // every part of the plain one is where its hand is
  let check = |drawn: &image::RgbaImage, at: f32, not: f32, what: &str| {
    for (row, part) in rows {
      let seen = square(drawn, at, 8.0 + row);
      assert_eq!(seen, Some(true), "{what}: {part}");
      let seen = square(drawn, not, 8.0 + row);
      assert_eq!(seen, Some(false), "{what}: {part} elsewhere");
      let seen = square(drawn, now, 40.0 + row);
      assert_eq!(seen, Some(true), "{what}: {part}, plain");
      let seen = square(drawn, at.min(now - 10.0), 40.0 + row);
      assert_eq!(seen, Some(false), "{what}: {part}, plain, elsewhere");
    }
  };

  // A frame of whole steps draws the interpolated one where it was the
  // step before, its meshes with its region
  check(&frame(&engine), now - 20.0, now, "a whole step");
  // Half a step later no step has run: all three draw half way
  part_step(&mut engine, 5);
  check(&frame(&engine), now - 10.0, now - 20.0, "half a step on");
  // A teleport: the entity skips interpolation and its meshes skip with
  // it, drawn where they are in that same frame
  engine.world_mut().skip_interpolation(smooth);
  part_step(&mut engine, 2);
  check(&frame(&engine), now, now - 10.0, "after a teleport");
}

#[test]
fn a_mesh_casts_the_drop_shadow_of_its_silhouette() {
  let Some(mut engine) = engine() else {
    return;
  };
  // Over a white floor, a skeleton at (24, 24): a red sail, a right
  // triangle whose corner is 4 px up and left of the origin and whose legs
  // are 16.25 px long (so no pixel's centre lies on its slanted edge); a
  // blue hand 4 px square on a bone at (10, 6); and a yellow hat 4 px
  // square on a bone at (-8, -8). With a black shadow 6 px down and right,
  // the sail casts its own shape, not its box, and every shadow is under
  // every part: the sail's falls on the hand, which stays blue, and the
  // hat's on the sail, which stays red
  let world = engine.world_mut();
  white_floor(world);
  let corner = Vec2::splat(-4.0);
  let sail = vec![
    corner,
    corner + Vec2::new(16.25, 0.0),
    corner + Vec2::new(0.0, 16.25),
  ];
  let parts = [
    part("sail", RED, (16, 16)).meshed(
      MeshVertices::Unweighted(sail),
      vec![Vec2::ZERO, Vec2::X, Vec2::Y],
      vec![0, 1, 2],
    ),
    part("hand", BLUE, (4, 4)).under("sail", 10.0, 6.0),
    part("hat", YELLOW, (4, 4)).under("sail", -8.0, -8.0),
  ];
  let data = rig(world, &parts);
  let shadow = DropShadow::new()
    .offset(6.0, 6.0)
    .color(Color::new(0, 0, 0, 255));
  let hero = stand(
    world,
    data,
    (24.0, 24.0),
    Renderable::skeleton().with_drop_shadow(shadow),
  );
  // The sail with its corner at (left, top)
  let sail_at =
    |x: u32, y: u32, left: u32, top: u32| x >= left && y >= top && (x - left) + (y - top) <= 15;
  // The parts, front to back, over `shadows`, over the floor
  let over = |shadows: &dyn Fn(u32, u32) -> bool, x: u32, y: u32| {
    if x == SIZE - 1 {
      None
    } else if inside(x, y, (14, 14, 4, 4)) {
      Some(YELLOW)
    } else if inside(x, y, (32, 28, 4, 4)) {
      Some(BLUE)
    } else if sail_at(x, y, 20, 20) {
      Some(RED)
    } else if shadows(x, y) {
      Some(BLACK)
    } else {
      Some(WHITE)
    }
  };
  let cast = |x: u32, y: u32| {
    sail_at(x, y, 26, 26) || inside(x, y, (38, 34, 4, 4)) || inside(x, y, (20, 20, 4, 4))
  };
  assert_pixels(
    &stepped(&mut engine),
    |x, y| over(&cast, x, y),
    "a shadow 6 px down and right of each part",
  );

  // Faded with the skeleton: at half alpha the sail's black shadow leaves
  // the white floor mid grey
  let mut renderable = engine.world_mut().get_mut::<Renderable>(hero).unwrap();
  renderable.alpha = 0.5;
  let faded = stepped(&mut engine).get_pixel(28, 38).0;
  assert!(
    faded[0] > 100 && faded[0] < 220 && faded[0] == faded[1] && faded[1] == faded[2],
    "a half shadow: {faded:?}"
  );

  // A shadow's scale shrinks the silhouettes toward where the skeleton
  // stands: at half height, 16 px down, the sail's shadow is a triangle
  // from (20, 38) with legs 16.25 px along and 8.125 px down, the hand's
  // spans x 32..36 and y 42..44, and the hat's x 14..18 and y 35..37
  let mut renderable = engine.world_mut().get_mut::<Renderable>(hero).unwrap();
  renderable.alpha = 1.0;
  renderable.drop_shadow = Some(
    DropShadow::new()
      .offset(0.0, 16.0)
      .scale(1.0, 0.5)
      .color(Color::new(0, 0, 0, 255)),
  );
  let squashed = |x: u32, y: u32| {
    let sail = x >= 20 && y >= 38 && x + 2 * y <= 110;
    sail || inside(x, y, (32, 42, 4, 2)) || inside(x, y, (14, 35, 4, 2))
  };
  assert_pixels(
    &stepped(&mut engine),
    |x, y| over(&squashed, x, y),
    "a squashed shadow",
  );

  // A mesh faded by its slot casts as much less, as a region does
  engine.world_mut().despawn(hero);
  let world = engine.world_mut();
  let parts = [
    part("solid", RED, (8, 8)).at(-10.0, 0.0).in_triangles(),
    part("ghost", RED, (8, 8))
      .at(10.0, 0.0)
      .slot(Color::new(255, 255, 255, 128))
      .in_triangles(),
  ];
  let data = rig(world, &parts);
  let shadow = DropShadow::new()
    .offset(0.0, 16.0)
    .color(Color::new(0, 0, 0, 255));
  stand(
    world,
    data,
    (32.0, 20.0),
    Renderable::skeleton().with_drop_shadow(shadow),
  );
  let drawn = stepped(&mut engine);
  assert_eq!(drawn.get_pixel(22, 36).0, BLACK, "a solid mesh's shadow");
  assert_eq!(
    drawn.get_pixel(42, 36).0,
    [SHADOW_ONCE, SHADOW_ONCE, SHADOW_ONCE, 255],
    "a half transparent mesh's shadow"
  );
}

#[test]
fn a_mesh_off_the_screen_is_culled_by_its_skinned_bounds() {
  let Some(mut engine) = engine() else {
    return;
  };
  let draws = |engine: &HeadlessEngine| engine.debug_stats().draw_calls;
  engine.step().unwrap();
  let nothing = draws(&engine);
  // A green body on the root, and a red kite 8 px square drawn after it,
  // every vertex weighted to a bone 1000 px to the right: where the kite
  // is drawn is where that bone carries it, not where its skeleton stands
  // or its slot's bone is
  let world = engine.world_mut();
  let far = BoneId(1);
  let corner = |x: f32, y: f32| vec![SkeletonInfluence::new(far, x, y, 1.0)];
  let corners = [
    corner(-4.0, -4.0),
    corner(4.0, -4.0),
    corner(-4.0, 4.0),
    corner(4.0, 4.0),
  ];
  // The far bone's own slot shows nothing a frame could draw
  let unseen = part("far", WHITE, (1, 1)).slot(Color::new(255, 255, 255, 0));
  let parts = [
    part("body", GREEN, (8, 8)),
    unseen.under("body", 1000.0, 0.0),
    part("kite", RED, (8, 8)).under("body", 0.0, 0.0).meshed(
      MeshVertices::Weighted(WeightedVertices::new(corners)),
      vec![Vec2::ZERO, Vec2::X, Vec2::Y, Vec2::ONE],
      vec![0, 1, 2, 1, 3, 2],
    ),
  ];
  let data = rig(world, &parts);
  let hero = stand(world, data, (16.0, 32.0), Renderable::skeleton());

  // The skeleton in view and its kite far out of it: the body's one draw,
  // and none for the kite
  let body = ((12, 28, 8, 8), GREEN);
  assert_boxes(&settled(&mut engine), &[body], "the kite out of view");
  assert_eq!(draws(&engine), nothing + 1, "the body alone is drawn");

  // The far bone brought 32 px from the body: the kite is in view, a draw
  // of its own
  let mut skeleton = engine.world_mut().get_mut::<Skeleton>(hero).unwrap();
  let brought = BoneOverride {
    translation: Some(Vec2::new(32.0, 0.0)),
    ..BoneOverride::default()
  };
  skeleton.override_bone("far", brought);
  assert_boxes(
    &settled(&mut engine),
    &[body, ((44, 28, 8, 8), RED)],
    "the kite brought into view",
  );
  assert_eq!(draws(&engine), nothing + 2, "the body, then the kite");

  // The bone back 1000 px out, and the skeleton moved as far the other
  // way: its entity and its body are out of view, and the kite, which its
  // bone carries back into view, is drawn
  let world = engine.world_mut();
  world
    .get_mut::<Skeleton>(hero)
    .unwrap()
    .clear_override("far");
  world.get_mut::<Transform>(hero).unwrap().translation = Vec2::new(-968.0, 32.0);
  assert_boxes(
    &settled(&mut engine),
    &[((28, 28, 8, 8), RED)],
    "the skeleton out of view, its kite in it",
  );
  assert_eq!(draws(&engine), nothing + 1, "the kite alone is drawn");

  // With its left edge on the frame's last column of pixels the kite
  // still draws there; two pixels further on nothing of it is in view,
  // and nothing is drawn
  for (x, seen) in [
    (-1000.0 + SIZE as f32 + 3.0, true),
    (-1000.0 + SIZE as f32 + 5.0, false),
  ] {
    let world = engine.world_mut();
    world.get_mut::<Transform>(hero).unwrap().translation.x = x;
    let drawn = settled(&mut engine);
    let background = drawn.get_pixel(0, 0).0;
    let edge = drawn.get_pixel(SIZE - 1, 32).0;
    assert_eq!(edge == RED, seen, "the kite at the frame's edge: {edge:?}");
    assert!(seen || edge == background, "nothing at the edge: {edge:?}");
    let expected = nothing + u32::from(seen);
    assert_eq!(draws(&engine), expected, "the skeleton at x {x}");
  }
}

/// A skeleton of two red 8 px squares of one image, 4 px apart: left of
/// its origin a region, right of it a mesh of two triangles. Standing at
/// (32, 32) they are [`PAIR`]
fn pair(world: &mut World) -> Handle<SkeletonData> {
  let parts = [
    part("left", RED, (8, 8)).at(-6.0, 0.0),
    part("right", RED, (8, 8)).at(6.0, 0.0).in_triangles(),
  ];
  rig_with(world, &parts, Images::Atlas, |builder| builder)
}

/// Where a [`pair`] standing at (32, 32) draws its region and its mesh
const PAIR: [Box4; 2] = [(22, 28, 8, 8), (34, 28, 8, 8)];

#[test]
fn a_built_in_effect_changes_a_skeleton_s_region_and_mesh_alike() {
  let Some(mut engine) = engine() else {
    return;
  };
  let world = engine.world_mut();
  let data = pair(world);
  let hero = stand(world, data, (32.0, 32.0), Renderable::skeleton());
  let [region, mesh] = PAIR;
  assert_boxes(
    &stepped(&mut engine),
    &[(region, RED), (mesh, RED)],
    "plain",
  );
  let shaded = |engine: &mut HeadlessEngine, effect: SpriteEffect| {
    let mut renderable = engine.world_mut().get_mut::<Renderable>(hero).unwrap();
    renderable.shader = Some(SpriteShader::BuiltIn(effect));
    stepped(engine)
  };

  // A hit flash whitens the mesh with the region
  let drawn = shaded(&mut engine, SpriteEffect::flash(Color::WHITE, 1.0));
  assert_boxes(&drawn, &[(region, WHITE), (mesh, WHITE)], "flashing");
  // And every effect that works in place gives the two parts one colour,
  // which is not the plain one
  let effects = [
    ("half a flash", SpriteEffect::flash(Color::WHITE, 0.5)),
    (
      "a silhouette",
      SpriteEffect::silhouette(Color::rgb(40, 200, 60)),
    ),
    ("grey", SpriteEffect::grayscale(1.0)),
    ("inverted", SpriteEffect::invert()),
    (
      "shifted to blue",
      SpriteEffect::color_shift(Color::rgb(30, 60, 230), 0.5),
    ),
  ];
  for (name, effect) in effects {
    let drawn = shaded(&mut engine, effect);
    let colour = drawn.get_pixel(26, 32).0;
    assert_ne!(colour, RED, "{name}: the effect shows on the region");
    assert_boxes(&drawn, &[(region, colour), (mesh, colour)], name);
  }
}

#[test]
fn a_game_s_sprite_shader_changes_a_skeleton_s_region_and_mesh_alike() {
  let Some(mut engine) = engine() else {
    return;
  };
  let world = engine.world_mut();
  let data = pair(world);
  let hero = stand(world, data, (32.0, 32.0), Renderable::skeleton());
  let [region, mesh] = PAIR;
  assert_boxes(
    &stepped(&mut engine),
    &[(region, RED), (mesh, RED)],
    "plain",
  );

  // A game's own shader draws the mesh through it as it draws the region.
  // Each draws plain until its pipeline has compiled
  let inverted = [255 - RED[0], 255 - RED[1], 255 - RED[2], 255];
  let world = engine.world_mut();
  let invert = world
    .resource_mut::<Assets<Shader>>()
    .add(Shader::from_wgsl("skeleton-mesh-invert.wgsl", INVERT));
  world.get_mut::<Renderable>(hero).unwrap().shader = Some(SpriteShader::custom(invert.clone()));
  step_until(
    &mut engine,
    |drawn| drawn.get_pixel(26, 32).0 == inverted && drawn.get_pixel(38, 32).0 == inverted,
    "both parts inverted",
  );
  let both = [(region, inverted), (mesh, inverted)];
  assert_boxes(&frame(&engine), &both, "inverted");

  // A material's shader wins over the renderable's, for the mesh too
  let world = engine.world_mut();
  world.get_mut::<Renderable>(hero).unwrap().shader = Some(SpriteShader::flash(Color::WHITE, 1.0));
  world
    .entity_mut(hero)
    .insert(Material::new(SpriteShader::custom(invert)));
  assert_boxes(&stepped(&mut engine), &both, "the material's shader");
}

#[test]
fn an_effect_that_grows_a_quad_reaches_the_regions_and_leaves_the_mesh_plain() {
  let log = ryo2d::testing::capture_log();
  let Some(mut engine) = engine() else {
    return;
  };
  let world = engine.world_mut();
  let data = pair(world);
  let hero = stand(world, data, (32.0, 32.0), Renderable::skeleton());
  let [region, mesh] = PAIR;
  let plain = stepped(&mut engine);
  assert_boxes(&plain, &[(region, RED), (mesh, RED)], "plain");
  assert_eq!(log.containing("grows"), 0, "{:?}", log.warnings());
  let background = plain.get_pixel(SIZE - 1, SIZE - 1).0;

  // An outline or a glow is drawn round its sprite, in a quad grown to
  // hold it. A region's quad grows; a mesh has its own shape, and draws
  // as it does with no effect
  let (white, black) = (Color::WHITE, Color::rgb(0, 0, 0));
  let effects = [
    ("an outline", SpriteEffect::outline(white, 2)),
    ("a glow", SpriteEffect::glow(Color::rgb(255, 255, 0), 3.0)),
    (
      "a directional outline",
      SpriteEffect::directional_outline(white, black, 2),
    ),
  ];
  for (name, effect) in effects {
    let mut renderable = engine.world_mut().get_mut::<Renderable>(hero).unwrap();
    renderable.shader = Some(SpriteShader::BuiltIn(effect));
    engine.step().unwrap();
    let drawn = stepped(&mut engine);
    // Round the region, just outside its box: left of it and above it
    for (x, y) in [(21, 32), (26, 27)] {
      let ring = drawn.get_pixel(x, y).0;
      assert_ne!(ring, background, "{name}: round the region at ({x}, {y})");
    }
    // The mesh and all that is round it but the gap between the two parts
    // (where the region's effect reaches): as with no effect
    for y in 0..SIZE {
      for x in mesh.0..SIZE {
        let (got, want) = (drawn.get_pixel(x, y).0, plain.get_pixel(x, y).0);
        assert_eq!(got, want, "{name}: the mesh's side at ({x}, {y})");
      }
    }
  }
  // The log is told once for them all, and whose mesh it was
  let told: Vec<String> = log
    .warnings()
    .into_iter()
    .filter(|warning| warning.contains("grows"))
    .collect();
  assert_eq!(told.len(), 1, "{told:?}");
  assert!(told[0].contains(&format!("{hero:?}")), "{told:?}");
}

#[test]
fn a_mesh_skinned_to_no_finite_place_draws_nothing() {
  let Some(mut engine) = engine() else {
    return;
  };
  // A region and a mesh over a white floor, with a drop shadow
  let world = engine.world_mut();
  white_floor(world);
  let data = pair(world);
  let shadow = DropShadow::new()
    .offset(0.0, 12.0)
    .color(Color::new(0, 0, 0, 255));
  let hero = stand(
    world,
    data,
    (32.0, 32.0),
    Renderable::skeleton().with_drop_shadow(shadow),
  );
  let floor = ((0, 0, SIZE - 1, SIZE), WHITE);
  let [region, mesh] = PAIR;
  let whole = [
    floor,
    ((22, 40, 8, 8), BLACK),
    ((34, 40, 8, 8), BLACK),
    (region, RED),
    (mesh, RED),
  ];
  assert_boxes(&settled(&mut engine), &whole, "as it stands");

  // Its entity scaled past what a float can carry a vertex through, and
  // then by no number at all: every skinned vertex is infinite or not a
  // number. Neither part is drawn, nor a shadow, and nothing panics
  for scale in [f32::MAX, f32::INFINITY, f32::NAN] {
    engine.world_mut().get_mut::<Transform>(hero).unwrap().scale = Vec2::splat(scale);
    let what = format!("scaled by {scale}");
    assert_boxes(&settled(&mut engine), &[floor], &what);
    engine.world_mut().get_mut::<Transform>(hero).unwrap().scale = Vec2::ONE;
    let what = format!("at its own size after {scale}");
    engine.step().unwrap();
    assert_boxes(&settled(&mut engine), &whole, &what);
  }
}

#[test]
fn a_mesh_whose_pose_is_stale_draws_nothing_not_even_its_shadow() {
  let Some(mut engine) = engine() else {
    return;
  };
  // Over a white floor, a red body that is a mesh, with a black shadow
  // 12 px down
  let world = engine.world_mut();
  white_floor(world);
  let data = rig(world, &[part("body", RED, (8, 8)).in_triangles()]);
  let shadow = DropShadow::new()
    .offset(0.0, 12.0)
    .color(Color::new(0, 0, 0, 255));
  let hero = stand(
    world,
    data.clone(),
    (32.0, 32.0),
    Renderable::skeleton().with_drop_shadow(shadow),
  );
  let floor = ((0, 0, SIZE - 1, SIZE), WHITE);
  let whole = [floor, ((28, 40, 8, 8), BLACK), ((28, 28, 8, 8), RED)];
  assert_boxes(&stepped(&mut engine), &whole, "the body and its shadow");

  // Its data changes while it is disabled, and it is enabled again while
  // no step runs: nothing has made it a pose for the data as it now is.
  // It draws no mesh and no shadow of one until its next step
  engine.world_mut().entity_mut(hero).insert(Disabled);
  let mut assets = engine.world_mut().resource_mut::<Assets<SkeletonData>>();
  let again = assets.get(&data).expect("the data").clone();
  *assets.get_mut(&data).expect("the data") = again;
  engine.step().unwrap();
  time(&mut engine).game.pause();
  engine.world_mut().entity_mut(hero).remove::<Disabled>();
  assert_boxes(&stepped(&mut engine), &[floor], "a stale pose");
  time(&mut engine).game.resume();
  assert_boxes(&stepped(&mut engine), &whole, "posed again");
}

#[test]
fn a_mesh_whose_image_is_missing_draws_what_its_region_draws() {
  let builder = EngineBuilder::new()
    .pixel_art(SIZE, SIZE)
    .asset_source(MemorySource::new())
    .start_scene(Stage(None));
  let Some(mut engine) = HeadlessEngine::new_or_skip(builder) else {
    return;
  };
  // An image that will never load (its file is missing), shown by one
  // skeleton as a region and by another as a mesh
  let world = engine.world_mut();
  let ghost = world.resource::<AssetServer>().load::<Image>("ghost.png");
  let hidden = Transform::from_xy(-500.0, -500.0);
  let data = [false, true].map(|mesh| one_part(world, &ghost, mesh, SlotBlend::Normal));
  let [region, mesh] =
    data.map(|data| stand(world, data, (-500.0, -500.0), Renderable::skeleton()));
  engine.step_frames(3).unwrap();
  let empty = frame(&engine);
  let shown = |engine: &mut HeadlessEngine, which: Entity, other: Entity| {
    let world = engine.world_mut();
    *world.get_mut::<Transform>(which).unwrap() = Transform::from_xy(32.0, 32.0);
    *world.get_mut::<Transform>(other).unwrap() = hidden;
    engine.step_frames(2).unwrap();
    frame(engine)
  };
  let as_region = shown(&mut engine, region, mesh);
  let as_mesh = shown(&mut engine, mesh, region);
  // A debug build draws the checker that stands in for a missing image,
  // all of it over the part; a release build draws nothing. The mesh as
  // the region, either way
  assert_eq!(
    as_region != empty,
    cfg!(debug_assertions),
    "the checker shows in a debug build alone"
  );
  for (x, y, pixel) in as_region.enumerate_pixels() {
    assert_eq!(as_mesh.get_pixel(x, y).0, pixel.0, "pixel ({x}, {y})");
  }
}

/// A red flag 16 x 8 px on a pole turned `turned`, reaching from the pole
/// along its x axis: a mesh of two triangles whose corners are, in order,
/// its top left, top right, bottom left and bottom right. "stretch" holds
/// its two right corners 8 px further along the pole's x axis, and "slant"
/// holds them 8 px further along its y axis
fn flagged(world: &mut World, turned: f32) -> Handle<SkeletonData> {
  let flag = part("flag", RED, (16, 8))
    .at(8.0, 0.0)
    .turned(turned)
    .in_triangles();
  rig_with(world, &[flag], Images::Each, |builder| {
    let held = |name: &str, (x, y): (f32, f32)| SkeletonClip {
      name: Arc::from(name),
      duration: 1.0,
      timelines: vec![SkeletonTimeline::Deform {
        slot: SlotId(0),
        attachment: Arc::from("flag"),
        frames: vec![(
          0.0,
          vec![0.0, 0.0, x, y, 0.0, 0.0, x, y],
          SkeletonCurve::Linear,
        )],
      }],
    };
    builder
      .animation(held("stretch", (8.0, 0.0)))
      .animation(held("slant", (0.0, 8.0)))
  })
}

#[test]
fn a_deform_key_moves_a_mesh_s_pixels() {
  let Some(mut engine) = engine() else {
    return;
  };
  // Over a white floor, two flags: one on a pole at (16, 32), reaching
  // right, with a black drop shadow 12 px below it; and one on a pole at
  // (52, 8) turned a quarter turn, reaching down the screen
  let world = engine.world_mut();
  white_floor(world);
  let shadow = DropShadow::new()
    .offset(0.0, 12.0)
    .color(Color::new(0, 0, 0, 255));
  let flat = flagged(world, 0.0);
  let hung = flagged(world, FRAC_PI_2);
  let flat = stand(
    world,
    flat,
    (16.0, 32.0),
    Renderable::skeleton().with_drop_shadow(shadow),
  );
  let hung = stand(world, hung, (52.0, 8.0), Renderable::skeleton());
  for flag in [flat, hung] {
    world.entity_mut(flag).insert(SkeletonAnimation::new());
  }
  let play = |engine: &mut HeadlessEngine, flag: Entity, name: &str, alpha: f32| {
    let mut state = engine
      .world_mut()
      .get_mut::<SkeletonAnimation>(flag)
      .unwrap();
    let entry = state.set_animation(0, name, true).expect("an animation");
    state.entry_mut(entry).expect("a live entry").alpha = alpha;
  };
  // Every pixel: the flags red where `flags` says, the flat one's shadow
  // black where `cast` says, and the floor
  type Area<'a> = &'a dyn Fn(u32, u32) -> bool;
  let check = |drawn: &image::RgbaImage, flags: Area, cast: Area, what: &str| {
    let want = |x: u32, y: u32| {
      if x == SIZE - 1 {
        None
      } else if flags(x, y) {
        Some(RED)
      } else if cast(x, y) {
        Some(BLACK)
      } else {
        Some(WHITE)
      }
    };
    assert_pixels(drawn, want, what);
  };
  // The flat flag `long` px long with its shadow, and the hung one `tall`
  let boxes = |engine: &mut HeadlessEngine, long: u32, tall: u32, what: &str| {
    let drawn = settled(engine);
    let flags = |x, y| inside(x, y, (16, 28, long, 8)) || inside(x, y, (48, 8, 8, tall));
    check(
      &drawn,
      &flags,
      &|x, y| inside(x, y, (16, 40, long, 8)),
      what,
    );
  };
  boxes(&mut engine, 16, 16, "the setup shapes");

  // The key moves the right corners 8 px along the pole's x axis: the flag
  // is half as long again, its drop shadow with it. The offsets are in
  // the pole's space, so the hung flag grows down the screen
  play(&mut engine, flat, "stretch", 1.0);
  play(&mut engine, hung, "stretch", 1.0);
  boxes(&mut engine, 24, 24, "stretched");
  // At half alpha the corners go half as far
  play(&mut engine, flat, "stretch", 0.5);
  boxes(&mut engine, 20, 24, "half stretched");
  // The right corners 8 px down: the flag slants evenly from its pole, and
  // so does its shadow, drawn under it
  play(&mut engine, flat, "slant", 1.0);
  let slanted = |x: u32, y: u32, top: f32| {
    let edge = top + 8.0 * (x as f32 + 0.5 - 16.0) / 16.0;
    let centre = y as f32 + 0.5;
    (16..32).contains(&x) && centre > edge && centre < edge + 8.0
  };
  let drawn = settled(&mut engine);
  let flags = |x, y| slanted(x, y, 28.0) || inside(x, y, (48, 8, 8, 24));
  check(&drawn, &flags, &|x, y| slanted(x, y, 40.0), "slanted");
  // With nothing playing, the setup shapes again
  for flag in [flat, hung] {
    let mut state = engine
      .world_mut()
      .get_mut::<SkeletonAnimation>(flag)
      .unwrap();
    state.clear(0);
  }
  boxes(&mut engine, 16, 16, "the setup shapes again");
}

/// A rig with no bone that moves, whose two white 4 px squares slide right
/// 20 px a step by deform keys alone: a mesh of two triangles on the root,
/// and 8 px below it a mesh whose every corner is weighted to the root
fn deform_slider(world: &mut World) -> Handle<SkeletonData> {
  let root = BoneId(0);
  let corner = |x: f32, y: f32| vec![SkeletonInfluence::new(root, x, 8.0 + y, 1.0)];
  let corners = [
    corner(-2.0, -2.0),
    corner(2.0, -2.0),
    corner(-2.0, 2.0),
    corner(2.0, 2.0),
  ];
  let parts = [
    part("flag", WHITE, (4, 4)).in_triangles(),
    part("cuff", WHITE, (4, 4)).under("flag", 0.0, 0.0).meshed(
      MeshVertices::Weighted(WeightedVertices::new(corners)),
      vec![Vec2::ZERO, Vec2::X, Vec2::Y, Vec2::ONE],
      vec![0, 1, 2, 1, 3, 2],
    ),
  ];
  rig_with(world, &parts, Images::Each, |builder| {
    let slide = |slot: u8, attachment: &str| SkeletonTimeline::Deform {
      slot: SlotId(slot),
      attachment: Arc::from(attachment),
      frames: vec![
        (0.0, Vec::new(), SkeletonCurve::Linear),
        (10.0, [12_000.0, 0.0].repeat(4), SkeletonCurve::Linear),
      ],
    };
    builder.animation(SkeletonClip {
      name: Arc::from("slide"),
      duration: 10.0,
      timelines: vec![slide(0, "flag"), slide(1, "cuff")],
    })
  })
}

#[test]
fn a_deform_is_interpolated_between_steps() {
  let Some(mut engine) = engine() else {
    return;
  };
  // Two skeletons whose bones stand still while deform keys slide their
  // meshes 20 px a step, an unweighted one over a weighted one. One is
  // interpolated, as a skeleton's entity is; the other has had
  // `Interpolated` taken off it and draws at the step rate
  let world = engine.world_mut();
  let data = deform_slider(world);
  let smooth = sliding(world, &data, (4.0, 8.0));
  let plain = sliding(world, &data, (4.0, 40.0));
  world.entity_mut(plain).remove::<Interpolated>();
  engine.step_frames(2).unwrap();
  let corners = ryo2d::testing::skeleton_mesh(engine.world(), smooth, "flag");
  let now = corners[0].x + 2.0;
  assert!(near(now, 44.0), "two steps of 20 px from x 4: {now}");
  assert_eq!(bone(&engine, smooth, "flag").translation.x, 4.0);
  let rows = [(0.0, "the mesh"), (8.0, "the weighted mesh")];
  // Both meshes of the interpolated one are at `at`, and neither at
  // `not`; both of the plain one are where the last step's keys put them
  let check = |drawn: &image::RgbaImage, at: f32, not: f32, what: &str| {
    for (row, part) in rows {
      let seen = square(drawn, at, 8.0 + row);
      assert_eq!(seen, Some(true), "{what}: {part}");
      let seen = square(drawn, not, 8.0 + row);
      assert_eq!(seen, Some(false), "{what}: {part} elsewhere");
      let seen = square(drawn, now, 40.0 + row);
      assert_eq!(seen, Some(true), "{what}: {part}, plain");
      let seen = square(drawn, at.min(now - 10.0), 40.0 + row);
      assert_eq!(seen, Some(false), "{what}: {part}, plain, elsewhere");
    }
  };

  // A frame of whole steps draws the interpolated one in the shape the
  // step before left it
  check(&frame(&engine), now - 20.0, now, "a whole step");
  // Half a step later no step has run: its offsets are half way between
  // the last two steps'
  part_step(&mut engine, 5);
  check(&frame(&engine), now - 10.0, now - 20.0, "half a step on");
  // A teleport: the entity skips interpolation and its meshes' shapes
  // skip with it, drawn as the last step left them in that same frame
  engine.world_mut().skip_interpolation(smooth);
  part_step(&mut engine, 2);
  check(&frame(&engine), now, now - 10.0, "after a teleport");
}
