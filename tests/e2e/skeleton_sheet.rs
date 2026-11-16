//! The contact sheet (`ryo2d::testing::skeleton_sheet`): a skeleton's
//! animations drawn as a grid of labelled cells, one row an animation and
//! one column a moment, read as pixels. The rigs are built in code, as a
//! game's would be, but for one read from the skeleton demo's files.
//!
//! Tests skip when no GPU is available.

use std::sync::Arc;

use image::RgbaImage;
use ryo2d::prelude::*;
use ryo2d::testing::{
  SHEET_GAP, SHEET_LABEL, SheetError, SheetOptions, SheetSkeleton, skeleton_sheet,
};

const RED: [u8; 4] = [220, 30, 40, 255];
const GREEN: [u8; 4] = [40, 200, 60, 255];
/// The bones overlay's colour for a bone (`colors::BONE`, which the crate
/// does not export)
const BONE: [u8; 3] = [0xff, 0xc0, 0x20];
/// The overlay's bounding box and point colours (`colors::BOUNDING_BOX`,
/// `colors::SKELETON_POINT`)
const BOUNDING_BOX: [u8; 4] = [0x00, 0xff, 0x00, 0xff];
const POINT: [u8; 4] = [0xff, 0x00, 0xff, 0xff];

/// The sheet, or `None` (with a note) on a machine with no GPU
fn sheet(skeleton: SheetSkeleton, options: &SheetOptions) -> Option<RgbaImage> {
  match skeleton_sheet(skeleton, options) {
    Ok(sheet) => Some(sheet),
    Err(SheetError::NoGpu) => {
      eprintln!("skipping: no GPU available for the sheet");
      None
    }
    Err(error) => panic!("the sheet failed: {error}"),
  }
}

fn solid(world: &mut World, size: u32, colour: [u8; 4]) -> Handle<Image> {
  let [r, g, b, a] = colour;
  world
    .resource_mut::<Assets<Image>>()
    .add(Image::solid_rect(size, size, r, g, b, a))
}

/// A square of `colour`, `size` px, centred on bone `bone`, shown by a slot
/// of the bone's name
fn square(
  builder: SkeletonDataBuilder,
  world: &mut World,
  bone: &str,
  colour: [u8; 4],
  size: u32,
) -> SkeletonDataBuilder {
  let image = solid(world, size, colour);
  let shown = SlotSetup {
    attachment: Some(Arc::from(bone)),
    ..SlotSetup::default()
  };
  let side = size as f32;
  let builder = builder.slot(bone, bone, shown).region(SkeletonRegion {
    name: Arc::from(bone),
    image,
    x: 0,
    y: 0,
    width: size,
    height: size,
  });
  let index = builder.region_index(bone).unwrap_or_default();
  builder.attachment(
    SkeletonData::DEFAULT_SKIN,
    bone,
    bone,
    SkeletonAttachment::Region(RegionAttachment::new(index, side, side)),
  )
}

/// A translate key track on `bone`: each key a time and a place, linear
fn moves(bone: BoneId, keys: &[(f32, Vec2)]) -> SkeletonTimeline {
  SkeletonTimeline::Translate {
    bone,
    frames: keys
      .iter()
      .map(|&(time, at)| (time, at, SkeletonCurve::Linear))
      .collect(),
  }
}

/// A red 12 px body on a root bone 20 long, and a green 6 px hand on a
/// bone of its own. "slide" carries the hand 40 to the right over a
/// second; "lift" carries it 30 up over half a second
fn hand_rig(world: &mut World) -> Result<SkeletonData, SkeletonError> {
  let root = BoneSetup {
    length: 20.0,
    ..BoneSetup::default()
  };
  let builder = SkeletonData::builder().bone("body", None, root).bone(
    "hand",
    Some("body"),
    BoneSetup::default(),
  );
  let builder = square(builder, world, "body", RED, 12);
  let builder = square(builder, world, "hand", GREEN, 6);
  builder
    .animation(SkeletonClip {
      name: Arc::from("slide"),
      duration: 1.0,
      timelines: vec![moves(
        BoneId(1),
        &[(0.0, Vec2::ZERO), (1.0, Vec2::new(40.0, 0.0))],
      )],
    })
    .animation(SkeletonClip {
      name: Arc::from("lift"),
      duration: 0.5,
      timelines: vec![moves(
        BoneId(1),
        &[(0.0, Vec2::ZERO), (0.5, Vec2::new(0.0, -30.0))],
      )],
    })
    .build()
}

/// Where the cell of row `row` and column `column` starts, for cells of
/// `cell`
fn cell_at(row: u32, column: u32, cell: UVec2) -> (u32, u32) {
  (
    SHEET_GAP + column * (cell.x + SHEET_GAP),
    SHEET_GAP + row * (cell.y + SHEET_GAP),
  )
}

/// The centre of the pixels near `colour` in the rectangle, if any
fn centre_of(
  sheet: &RgbaImage,
  (x0, y0, w, h): (u32, u32, u32, u32),
  colour: [u8; 4],
) -> Option<Vec2> {
  let (mut sum, mut count) = (Vec2::ZERO, 0.0);
  for y in y0..y0 + h {
    for x in x0..x0 + w {
      let pixel = sheet.get_pixel(x, y).0;
      let near = (0..3).all(|c| pixel[c].abs_diff(colour[c]) < 24);
      if near {
        sum += Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
        count += 1.0;
      }
    }
  }
  (count > 0.0).then(|| sum / count)
}

/// The box of the pixels near `colour` in the rectangle: its left, top,
/// right and bottom edges, if any
fn box_of(
  sheet: &RgbaImage,
  (x0, y0, w, h): (u32, u32, u32, u32),
  colour: [u8; 4],
) -> Option<[f32; 4]> {
  let mut found: Option<[f32; 4]> = None;
  for y in y0..y0 + h {
    for x in x0..x0 + w {
      let pixel = sheet.get_pixel(x, y).0;
      if (0..3).all(|c| pixel[c].abs_diff(colour[c]) < 24) {
        let (x, y) = (x as f32, y as f32);
        let [l, t, r, b] = found.unwrap_or([x, y, x + 1.0, y + 1.0]);
        found = Some([l.min(x), t.min(y), r.max(x + 1.0), b.max(y + 1.0)]);
      }
    }
  }
  found
}

/// Pixels of the rectangle at least `light` in each channel
fn light_pixels(sheet: &RgbaImage, (x0, y0, w, h): (u32, u32, u32, u32), light: u8) -> usize {
  (y0..y0 + h)
    .flat_map(|y| (x0..x0 + w).map(move |x| (x, y)))
    .filter(|&(x, y)| sheet.get_pixel(x, y).0[..3].iter().all(|&c| c >= light))
    .count()
}

/// Pixels of the sheet of the bones overlay's bone colour
fn bone_pixels(sheet: &RgbaImage) -> usize {
  sheet
    .pixels()
    .filter(|pixel| (0..3).all(|c| pixel.0[c].abs_diff(BONE[c]) < 12))
    .count()
}

#[test]
fn a_sheet_has_a_cell_per_animation_and_moment_each_labelled_and_drawn() {
  let cell = UVec2::new(120, 100);
  let options = SheetOptions {
    moments: 4,
    cell: Some(cell),
    ..SheetOptions::default()
  };
  let Some(sheet) = sheet(SheetSkeleton::built(hand_rig), &options) else {
    return;
  };
  // Two animations of four moments, each cell its size with a gap round it
  assert_eq!(sheet.width(), 4 * cell.x + 5 * SHEET_GAP);
  assert_eq!(sheet.height(), 2 * cell.y + 3 * SHEET_GAP);

  let mut hands = Vec::new();
  let mut bodies = Vec::new();
  for row in 0..2 {
    for column in 0..4 {
      let (x, y) = cell_at(row, column, cell);
      let label = (x, y, cell.x, SHEET_LABEL);
      let art = (x, y + SHEET_LABEL, cell.x, cell.y - SHEET_LABEL);
      assert!(
        light_pixels(&sheet, label, 200) > 20,
        "cell ({row}, {column}) is labelled"
      );
      assert_eq!(light_pixels(&sheet, art, 200), 0, "no label over the art");
      let hand = centre_of(&sheet, art, GREEN);
      let body = box_of(&sheet, art, RED);
      let (Some(hand), Some([left, _, right, bottom])) = (hand, body) else {
        panic!("cell ({row}, {column}) draws the hand and the body");
      };
      let corner = Vec2::new(x as f32, y as f32);
      hands.push(hand - corner);
      // The hand can hide the body's middle and top, not its sides or foot
      bodies.push(Vec2::new((left + right) / 2.0, bottom) - corner);
    }
  }
  // The origin sits at one place in every cell: the body never moves
  for body in &bodies {
    assert!(body.distance(bodies[0]) < 0.5, "{bodies:?}");
  }
  // "slide" at 0, 0.25, 0.5 and 0.75 s: the hand 10 further right each
  // time, at one scale; "lift" at 0, 0.125, 0.25 and 0.375 s: 7.5 higher
  let scale = (hands[3].x - hands[0].x) / 30.0;
  assert!(scale > 0.5, "{hands:?}");
  for (k, hand) in hands[..4].iter().enumerate() {
    let want = hands[0].x + 10.0 * k as f32 * scale;
    assert!((hand.x - want).abs() < 1.0, "slide {k}: {hands:?}");
    assert!((hand.y - hands[0].y).abs() < 1.0);
  }
  for (k, hand) in hands[4..].iter().enumerate() {
    let want = hands[0].y - 7.5 * k as f32 * scale;
    assert!((hand.y - want).abs() < 1.0, "lift {k}: {hands:?}");
    assert!((hand.x - hands[0].x).abs() < 1.0);
  }
}

/// A red 6 px body on a root bone, and a tail 30 long hanging from it with
/// a green 4 px square at its tip, upright whatever the tail does; with a
/// spring on the tail when `sprung`. "carry" takes the body 60 to the
/// right in half a second and holds it there for the other half
fn tail_rig(world: &mut World, sprung: bool) -> Result<SkeletonData, SkeletonError> {
  let tail = BoneSetup {
    length: 30.0,
    rotation: std::f32::consts::FRAC_PI_2,
    ..BoneSetup::default()
  };
  let tip = BoneSetup {
    x: 30.0,
    inherit: BoneInherit::OnlyTranslation,
    ..BoneSetup::default()
  };
  let builder = SkeletonData::builder()
    .bone("body", None, BoneSetup::default())
    .bone("tail", Some("body"), tail)
    .bone("tip", Some("tail"), tip);
  let builder = square(builder, world, "body", RED, 6);
  let mut builder = square(builder, world, "tip", GREEN, 4);
  if sprung {
    let swing = SpringSetup {
      rotate: 1.0,
      ..SpringSetup::default()
    };
    builder = builder.spring("sway", "tail", swing);
  }
  let carry = [
    (0.0, Vec2::ZERO),
    (0.5, Vec2::new(60.0, 0.0)),
    (1.0, Vec2::new(60.0, 0.0)),
  ];
  builder
    .animation(SkeletonClip {
      name: Arc::from("carry"),
      duration: 1.0,
      timelines: vec![moves(BoneId(0), &carry)],
    })
    .build()
}

#[test]
fn a_sheet_shows_springs() {
  let cell = UVec2::new(160, 120);
  let options = SheetOptions {
    moments: 4,
    cell: Some(cell),
    ..SheetOptions::default()
  };
  // The tip's place from the body's in the cell at 0.25 s, the body half
  // way: straight below it without the spring, trailing with it
  let lag = |sprung: bool| {
    let rig = SheetSkeleton::built(move |world: &mut World| tail_rig(world, sprung));
    let sheet = sheet(rig, &options)?;
    let (x, y) = cell_at(0, 1, cell);
    let art = (x, y + SHEET_LABEL, cell.x, cell.y - SHEET_LABEL);
    let body = centre_of(&sheet, art, RED).expect("the body");
    let tip = centre_of(&sheet, art, GREEN).expect("the tip");
    Some(tip - body)
  };
  let (Some(stiff), Some(sprung)) = (lag(false), lag(true)) else {
    return;
  };
  assert!(stiff.x.abs() < 1.0, "straight below: {stiff}");
  assert!(
    sprung.x < -8.0,
    "the spring leaves the tip behind: {sprung}"
  );
}

#[test]
fn a_sheet_steps_springs_at_the_fixed_rate_asked_for() {
  // The sprung tail's cell at 0.25 s stepped at 30 a second trails
  // otherwise than at the default 60; 60 asked for is the default itself
  let cell = UVec2::new(160, 120);
  let drawn = |fixed_hz: Option<u32>| {
    let options = SheetOptions {
      moments: 4,
      cell: Some(cell),
      fixed_hz,
      ..SheetOptions::default()
    };
    let rig = SheetSkeleton::built(|world: &mut World| tail_rig(world, true));
    sheet(rig, &options)
  };
  let lag = |sheet: &RgbaImage| {
    let (x, y) = cell_at(0, 1, cell);
    let art = (x, y + SHEET_LABEL, cell.x, cell.y - SHEET_LABEL);
    let body = centre_of(sheet, art, RED).expect("the body");
    let tip = centre_of(sheet, art, GREEN).expect("the tip");
    tip - body
  };
  let (Some(default), Some(sixty), Some(thirty)) = (drawn(None), drawn(Some(60)), drawn(Some(30)))
  else {
    return;
  };
  assert!(default == sixty, "60 a second is the default");
  let (at_60, at_30) = (lag(&default), lag(&thirty));
  assert!(
    at_60.distance(at_30) > 1.0,
    "the tip at 60 a second {at_60}, at 30 {at_30}"
  );
}

#[test]
fn a_one_shot_is_not_warmed_and_ends_with_its_last_pose() {
  let cell = UVec2::new(160, 120);
  let options = |once: bool| SheetOptions {
    moments: 4,
    cell: Some(cell),
    once: if once {
      vec!["carry".to_owned()]
    } else {
      Vec::new()
    },
    ..SheetOptions::default()
  };
  let rig = || SheetSkeleton::built(|world: &mut World| tail_rig(world, true));
  let (Some(looped), Some(once)) = (sheet(rig(), &options(false)), sheet(rig(), &options(true)))
  else {
    return;
  };
  // A loop: four cells. A one-shot: a fifth, at its length
  assert_eq!(looped.width(), 4 * cell.x + 5 * SHEET_GAP);
  assert_eq!(once.width(), 5 * cell.x + 6 * SHEET_GAP);
  let art = |column: u32| {
    let (x, y) = cell_at(0, column, cell);
    (x, y + SHEET_LABEL, cell.x, cell.y - SHEET_LABEL)
  };
  // Each from its cell's corner
  let parts = |sheet: &RgbaImage, column: u32| {
    let area = art(column);
    let corner = Vec2::new(area.0 as f32, area.1 as f32);
    let body = centre_of(sheet, area, RED).expect("the body");
    let tip = centre_of(sheet, area, GREEN).expect("the tip");
    (body - corner, tip - corner)
  };
  // At 0 s the loop has just jumped back from its end, and its spring
  // swings from that; the one-shot starts at rest, the tip straight below
  let (body, tip) = parts(&looped, 0);
  assert!((tip.x - body.x).abs() > 2.0, "warmed: {body} {tip}");
  let (start, tip) = parts(&once, 0);
  assert!((tip.x - start.x).abs() < 1.0, "at rest: {start} {tip}");
  // Its end cell shows its last pose: the body carried 60 to the right
  let (half, _) = parts(&once, 2);
  let (end, _) = parts(&once, 4);
  let scale = (half.x - start.x) / 60.0;
  assert!(scale > 0.5, "{start} {half}");
  assert!(
    (end.x - start.x - 60.0 * scale).abs() < 1.0,
    "{start} {end}"
  );
}

/// A body and hand as `hand_rig`'s, whose "long" animation of 20 s holds
/// the hand still and moves it 40 to the right on a stepped key at 12.5 s
fn long_rig(world: &mut World) -> Result<SkeletonData, SkeletonError> {
  let builder = SkeletonData::builder()
    .bone("body", None, BoneSetup::default())
    .bone("hand", Some("body"), BoneSetup::default());
  let builder = square(builder, world, "body", RED, 12);
  let builder = square(builder, world, "hand", GREEN, 6);
  let frames = vec![
    (0.0, Vec2::ZERO, SkeletonCurve::Stepped),
    (12.5, Vec2::new(40.0, 0.0), SkeletonCurve::Stepped),
  ];
  builder
    .animation(SkeletonClip {
      name: Arc::from("long"),
      duration: 20.0,
      timelines: vec![SkeletonTimeline::Translate {
        bone: BoneId(1),
        frames,
      }],
    })
    .build()
}

#[test]
fn a_long_animation_lands_on_its_moments() {
  // Eight moments of 20 s, after a loop round: the sixth is 32.5 s of
  // steps in, on the key that moves the hand, and shows it moved
  let cell = UVec2::new(120, 100);
  let options = SheetOptions {
    moments: 8,
    cell: Some(cell),
    ..SheetOptions::default()
  };
  let Some(sheet) = sheet(SheetSkeleton::built(long_rig), &options) else {
    return;
  };
  let hand = |column: u32| {
    let (x, y) = cell_at(0, column, cell);
    let art = (x, y + SHEET_LABEL, cell.x, cell.y - SHEET_LABEL);
    centre_of(&sheet, art, GREEN).expect("the hand").x - x as f32
  };
  let body = {
    let (x, y) = cell_at(0, 0, cell);
    let art = (x, y + SHEET_LABEL, cell.x, cell.y - SHEET_LABEL);
    box_of(&sheet, art, RED).expect("the body")
  };
  let centre = (body[0] + body[2]) / 2.0 - SHEET_GAP as f32;
  assert!((hand(4) - centre).abs() < 1.0, "10 s: still {}", hand(4));
  assert!(hand(5) - centre > 10.0, "12.5 s: moved {}", hand(5));
}

#[test]
fn with_bones_the_cell_fits_bounding_boxes_and_points() {
  // A bounding box reaching 150 to the left and a point 150 to the right,
  // far past the art and the bones: the overlay's box and cross are drawn
  // whole inside the cell
  let rig = |world: &mut World| {
    let builder = SkeletonData::builder().bone("body", None, BoneSetup::default());
    let builder = square(builder, world, "body", RED, 12);
    let shown = |name: &str| SlotSetup {
      attachment: Some(Arc::from(name)),
      ..SlotSetup::default()
    };
    let corners = vec![
      Vec2::new(-150.0, -10.0),
      Vec2::new(10.0, -10.0),
      Vec2::new(10.0, 10.0),
      Vec2::new(-150.0, 10.0),
    ];
    builder
      .slot("hit", "body", shown("hit"))
      .slot("muzzle", "body", shown("muzzle"))
      .attachment(
        SkeletonData::DEFAULT_SKIN,
        "hit",
        "hit",
        SkeletonAttachment::BoundingBox(BoundingBoxAttachment { vertices: corners }),
      )
      .attachment(
        SkeletonData::DEFAULT_SKIN,
        "muzzle",
        "muzzle",
        SkeletonAttachment::Point(PointAttachment {
          x: 150.0,
          y: 0.0,
          rotation: 0.0,
        }),
      )
      .build()
  };
  let cell = UVec2::new(200, 100);
  let options = SheetOptions {
    moments: 1,
    bones: true,
    cell: Some(cell),
    ..SheetOptions::default()
  };
  let Some(sheet) = sheet(SheetSkeleton::built(rig), &options) else {
    return;
  };
  let art = (
    SHEET_GAP,
    SHEET_GAP + SHEET_LABEL,
    cell.x,
    cell.y - SHEET_LABEL,
  );
  let [left, _, right, _] = box_of(&sheet, art, BOUNDING_BOX).expect("the bounding box");
  let point = box_of(&sheet, art, POINT).expect("the point's cross");
  let inside = |x: f32| x > (SHEET_GAP + 2) as f32 && x < (SHEET_GAP + cell.x - 2) as f32;
  assert!(
    inside(left) && inside(right),
    "the box whole: {left} to {right}"
  );
  assert!(
    inside(point[0]) && inside(point[2]),
    "the cross whole: {point:?}"
  );
  assert!(point[0] > right, "the point right of the box");
}

#[test]
fn the_bones_overlay_draws_only_when_asked() {
  // The sheet's engine has every overlay of its own off: the bones are the
  // sheet's drawing (`tests/no_tools.rs` draws them in a build without the
  // tools)
  let bones = |bones: bool| {
    let options = SheetOptions {
      moments: 2,
      bones,
      cell: Some(UVec2::new(120, 100)),
      ..SheetOptions::default()
    };
    sheet(SheetSkeleton::built(hand_rig), &options)
  };
  let (Some(off), Some(on)) = (bones(false), bones(true)) else {
    return;
  };
  assert_eq!(bone_pixels(&off), 0, "no bones unless asked for");
  // The body's bone in each of the four cells
  assert!(bone_pixels(&on) > 4 * 8, "{} bone pixels", bone_pixels(&on));
}

#[test]
fn a_sheet_reads_a_skeleton_file_from_its_asset_folder() {
  // The demo's reacher has no animations: its setup pose, as its
  // constraints place it, in one cell
  let assets = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/examples/demos/skeleton/assets"
  );
  let reacher = SheetSkeleton::file(assets, "reacher.skeleton.ron");
  let Some(sheet) = sheet(reacher, &SheetOptions::default()) else {
    return;
  };
  let cell = UVec2::new(
    sheet.width() - 2 * SHEET_GAP,
    sheet.height() - 2 * SHEET_GAP,
  );
  assert!(cell.x >= 160 && cell.y > SHEET_LABEL + 100, "{cell}");
  let label = (SHEET_GAP, SHEET_GAP, cell.x, SHEET_LABEL);
  assert!(light_pixels(&sheet, label, 200) > 20, "labelled");
  // The art: more than the background's one colour
  let background = sheet
    .get_pixel(SHEET_GAP + 1, SHEET_GAP + SHEET_LABEL + 1)
    .0;
  let (top, bottom) = (SHEET_GAP + SHEET_LABEL, SHEET_GAP + cell.y);
  let drawn =
    |y: u32| (SHEET_GAP..SHEET_GAP + cell.x).any(|x| sheet.get_pixel(x, y).0 != background);
  let rows: Vec<u32> = (top..bottom).filter(|&y| drawn(y)).collect();
  // Fitted to the cell: the stand, 280 tall with the arm above it, is
  // drawn the art's full 240 px, not a part of it
  let (first, last) = (rows.first().copied(), rows.last().copied());
  let span = last.zip(first).map_or(0, |(last, first)| last - first);
  assert!(span > 230 && span < 260, "the art spans {span} px");

  // A name the file lacks, or a skin, is an error naming it
  let missing = SheetOptions {
    animations: Some(vec!["nope".to_owned()]),
    ..SheetOptions::default()
  };
  match skeleton_sheet(
    SheetSkeleton::file(assets, "reacher.skeleton.ron"),
    &missing,
  ) {
    Err(SheetError::NoGpu) => {}
    Err(error) => assert!(error.to_string().contains("nope"), "{error}"),
    Ok(_) => panic!("an animation the skeleton lacks"),
  }
  let missing = SheetOptions {
    skins: vec!["plaid".to_owned()],
    ..SheetOptions::default()
  };
  match skeleton_sheet(
    SheetSkeleton::file(assets, "reacher.skeleton.ron"),
    &missing,
  ) {
    Err(SheetError::NoGpu) => {}
    Err(error) => assert!(error.to_string().contains("plaid"), "{error}"),
    Ok(_) => panic!("a skin the skeleton lacks"),
  }
}
