//! `skeleton_sheet`: a skeleton's animations drawn as a contact sheet, a
//! grid of labelled cells, one row an animation and one column a moment.

use std::path::PathBuf;

use glam::{Affine2, UVec2, Vec2};
use image::{Rgba, RgbaImage, imageops};

use super::{HeadlessEngine, HeadlessError};
use crate::core::asset::{AssetServer, Assets, DiskSource, Handle, MemorySource};
use crate::core::ecs::kernel::{Mut, Resource};
use crate::core::ecs::{Entity, Interpolated, Renderable, Transform, World, WorldExt};
use crate::core::engine::EngineBuilder;
use crate::core::graphics::{Camera2d, CameraClear, CameraMut, Image, RenderLayers};
use crate::core::render::{DebugDraw, Tonemap};
use crate::core::scene::{Scene, SceneAction, SceneContext};
use crate::core::skeleton::{
  Part, Parts, Skeleton, SkeletonAnimation, SkeletonData, SkeletonError,
};
use crate::core::time::Time;
use crate::core::ui::{Color, Rect, UiContext};

/// The moments a row has unless [`SheetOptions::moments`] says otherwise
pub const DEFAULT_MOMENTS: u32 = 8;
/// The height of the strip at the top of each cell that holds its label,
/// in pixels
pub const SHEET_LABEL: u32 = 22;
/// The gap round and between the cells, in pixels
pub const SHEET_GAP: u32 = 2;

/// The longer side of the art in a cell the sheet sizes itself
const ART: u32 = 240;
/// The room round the art inside its cell
const MARGIN: u32 = 12;
/// The narrowest cell the sheet sizes itself, so a label fits
const MIN_WIDTH: u32 = 176;
/// The smallest cell asked for: the label and a little art
const MIN_CELL: UVec2 = UVec2::new(32, SHEET_LABEL + 16);
/// The largest cell side, the headless engine's largest frame
const MAX_CELL: u32 = 8192;
/// The most moments a row has
const MAX_MOMENTS: u32 = 1000;
/// How far past its moment, in steps, a cell is aimed
const NUDGE: f32 = 0.01;

/// Behind the art: a mid grey, against which dark outlines and light
/// highlights both show
const BACKGROUND: Color = Color::rgb(118, 124, 136);
/// The ground through the skeleton's origin, faintly
const GROUND: Color = Color::new(255, 255, 255, 56);
const LABEL_STRIP: Color = Color::rgb(30, 32, 38);
const LABEL_TEXT: Color = Color::rgb(236, 238, 244);
const GAP: Rgba<u8> = Rgba([18, 19, 23, 255]);

/// The layer the sheet's camera sees; a cell not being drawn is on none
const SEEN: RenderLayers = RenderLayers::layer(0);

/// What a contact sheet shows
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetOptions {
  /// The animations, a row each in this order; `None` for every animation
  /// in the skeleton's order. A skeleton with none shows its setup pose
  pub animations: Option<Vec<String>>,
  /// The moments of each animation, a column each: `k × length / moments`
  /// for `k` from 0 (8)
  pub moments: u32,
  /// The skins put on, in order, over the default skin
  pub skins: Vec<String>,
  /// Draws the bones overlay over the art: bones, joints, outlines and the
  /// constraints' marks, as `debug:skeletons` draws them
  pub bones: bool,
  /// Each cell's size in pixels, its label strip included; `None` sizes
  /// the cells to fit the poses
  pub cell: Option<UVec2>,
}

impl Default for SheetOptions {
  fn default() -> Self {
    Self {
      animations: None,
      moments: DEFAULT_MOMENTS,
      skins: Vec::new(),
      bones: false,
      cell: None,
    }
  }
}

/// Builds a skeleton's data in the sheet's world, which its images join
type Build = Box<dyn FnOnce(&mut World) -> Result<SkeletonData, SkeletonError>>;

/// The skeleton a sheet draws: a `.skeleton.ron` file read from an asset
/// folder, or data built in code
pub struct SheetSkeleton {
  source: Source,
}

enum Source {
  File { assets: PathBuf, path: String },
  Built(Build),
}

impl SheetSkeleton {
  /// The skeleton file at asset path `path` (`"hero.skeleton.ron"`) under
  /// the asset folder `assets`, which its images' paths are read from too
  pub fn file(assets: impl Into<PathBuf>, path: impl Into<String>) -> Self {
    Self {
      source: Source::File {
        assets: assets.into(),
        path: path.into(),
      },
    }
  }

  /// A skeleton built in code: `build` is given the sheet's world, to add
  /// the images its regions name (`world.resource_mut::<Assets<Image>>()`),
  /// and returns the data, as `SkeletonDataBuilder::build` does
  pub fn built(
    build: impl FnOnce(&mut World) -> Result<SkeletonData, SkeletonError> + 'static,
  ) -> Self {
    Self {
      source: Source::Built(Box::new(build)),
    }
  }
}

/// Why a sheet could not be drawn
#[derive(Debug, thiserror::Error)]
pub enum SheetError {
  /// This machine has no GPU to draw with
  #[error("no GPU is available to draw the sheet with")]
  NoGpu,
  /// The skeleton did not load, or did not build
  #[error("the skeleton did not load: {0}")]
  Load(String),
  /// An animation asked for is not in the skeleton
  #[error("the skeleton has no animation \"{name}\" (it has: {known})")]
  UnknownAnimation { name: String, known: String },
  /// A skin asked for is not in the skeleton
  #[error("the skeleton has no skin \"{name}\" (it has: {known})")]
  UnknownSkin { name: String, known: String },
  /// The options cannot make a sheet
  #[error("{0}")]
  Options(String),
  /// A frame failed to draw or read back
  #[error("drawing failed: {0}")]
  Render(String),
}

impl From<HeadlessError> for SheetError {
  fn from(error: HeadlessError) -> Self {
    match error {
      HeadlessError::NoGpu => Self::NoGpu,
      other => Self::Render(other.to_string()),
    }
  }
}

/// One row of the sheet
struct Row {
  /// The animation, or `None` for the setup pose
  animation: Option<String>,
  duration: f32,
  label: String,
}

/// One cell: a skeleton of its own, stepped to its moment
struct Cell {
  row: usize,
  time: f32,
  /// Where its animation's entry starts, under one step, so its whole
  /// steps land on its moment
  start: f32,
  /// The steps it takes after it appears at `start`
  steps: u32,
  entity: Option<Entity>,
}

/// The label shown in the strip of the cell being drawn
#[derive(Resource)]
struct SheetLabel {
  text: String,
  width: f32,
}

/// The sheet's scene: colours as they are, and the cell's label
struct SheetScene;

impl Scene for SheetScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.look().tonemap = Tonemap::Clamp;
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
    if let Some(label) = ctx.world.get_resource::<SheetLabel>() {
      let strip = Rect::new(0.0, 0.0, label.width, SHEET_LABEL as f32);
      ui.draw_rect(strip, LABEL_STRIP);
      ui.draw_text(&label.text, 6.0, 4.0, LABEL_TEXT);
    }
    SceneAction::Continue
  }
}

/// A box grown by the points put in it
#[derive(Debug, Clone, Copy)]
struct Bounds {
  min: Vec2,
  max: Vec2,
}

impl Bounds {
  const EMPTY: Self = Self {
    min: Vec2::splat(f32::INFINITY),
    max: Vec2::splat(f32::NEG_INFINITY),
  };

  fn add(&mut self, point: Vec2) {
    if point.is_finite() {
      self.min = self.min.min(point);
      self.max = self.max.max(point);
    }
  }

  fn union(&mut self, other: Bounds) {
    self.add(other.min);
    self.add(other.max);
  }

  fn is_empty(&self) -> bool {
    self.min.x > self.max.x || self.min.y > self.max.y
  }
}

/// How the cells are laid out: their size, the scale, and where the
/// skeleton's origin is in each
#[derive(Debug, Clone, Copy, PartialEq)]
struct Layout {
  cell: UVec2,
  scale: f32,
  origin: Vec2,
}

/// Draws `skeleton`'s animations as a contact sheet: one row an animation,
/// one column a moment, each cell labelled with the animation's name and
/// the time, `SHEET_GAP` pixels round and between them.
///
/// Each cell is a skeleton of its own on a headless engine, drawn as the
/// game draws it, its constraints applied. Its moment, `k × length /
/// moments`, is reached by stepping its animation from its start at the
/// engine's fixed rate, so springs show as they would in play; every
/// animation is played looping and is first run once round, so its springs
/// are warmed up. Every cell has one scale, fitted to the poses of the
/// whole sheet unless [`SheetOptions::cell`] is given, and the skeleton's
/// origin at the same place, on a faint ground line, so root motion reads.
/// Cells are drawn one by one and put together on the CPU, so a sheet has
/// no size limit.
///
/// The bones overlay is the sheet's own drawing: its engine has every
/// overlay of its own off, so it shows in a build without the developer
/// tools as in one with them.
pub fn skeleton_sheet(
  skeleton: SheetSkeleton,
  options: &SheetOptions,
) -> Result<RgbaImage, SheetError> {
  check_options(options)?;
  let canvas = options.cell.unwrap_or(UVec2::new(
    (ART + 2 * MARGIN).max(MIN_WIDTH),
    ART + 2 * MARGIN + SHEET_LABEL,
  ));
  let builder = EngineBuilder::new()
    .resolution(canvas.x, canvas.y)
    .start_scene(SheetScene);
  let (builder, build) = match skeleton.source {
    Source::File { assets, path } => (builder.asset_source(DiskSource::new(assets)), Ok(path)),
    Source::Built(build) => (builder.asset_source(MemorySource::new()), Err(build)),
  };
  let mut engine = HeadlessEngine::new(builder)?;
  quiet_overlays(&mut engine);
  let world = engine.world_mut();
  let handle = match build {
    Ok(path) => {
      let server = world.resource::<AssetServer>().clone();
      server
        .load_now::<SkeletonData>(world, &path)
        .map_err(|error| SheetError::Load(error.to_string()))?
    }
    Err(build) => {
      let data = build(world).map_err(|error| SheetError::Load(error.to_string()))?;
      world.resource_mut::<Assets<SkeletonData>>().add(data)
    }
  };
  let data = world
    .resource::<Assets<SkeletonData>>()
    .get(&handle)
    .ok_or_else(|| SheetError::Load("the skeleton's data is not in its store".to_owned()))?;
  let rows = rows(data, options)?;
  check_skins(data, &options.skins)?;

  let step = engine.world().resource::<Time>().fixed.timestep();
  let mut cells = cells(&rows, options.moments, step);
  run(&mut engine, &handle, &rows, &mut cells, options)?;

  let mut union = Bounds::EMPTY;
  for entity in cells.iter().filter_map(|cell| cell.entity) {
    if let Some(bounds) = pose_bounds(engine.world(), entity, options.bones) {
      union.union(bounds);
    }
  }
  if union.is_empty() {
    union = Bounds {
      min: Vec2::splat(-50.0),
      max: Vec2::splat(50.0),
    };
  }
  let layout = layout(union, options.cell);
  draw(&mut engine, &rows, &cells, layout, options)
}

fn check_options(options: &SheetOptions) -> Result<(), SheetError> {
  if !(1..=MAX_MOMENTS).contains(&options.moments) {
    return Err(SheetError::Options(format!(
      "moments must be 1 to {MAX_MOMENTS}, not {}",
      options.moments
    )));
  }
  if let Some(cell) = options.cell
    && (cell.x < MIN_CELL.x || cell.y < MIN_CELL.y || cell.x > MAX_CELL || cell.y > MAX_CELL)
  {
    return Err(SheetError::Options(format!(
      "a cell must be {}x{} to {MAX_CELL}x{MAX_CELL} pixels, not {}x{}",
      MIN_CELL.x, MIN_CELL.y, cell.x, cell.y
    )));
  }
  if options.animations.as_ref().is_some_and(Vec::is_empty) {
    return Err(SheetError::Options("no animations named".to_owned()));
  }
  Ok(())
}

/// The sheet's engine draws only what the sheet asks for: none of its own
/// overlays, whatever the project it was started in turns on
fn quiet_overlays(engine: &mut HeadlessEngine) {
  let debug = engine.engine.debug_config_mut();
  debug.show_skeletons = false;
  debug.show_colliders = false;
  debug.show_velocities = false;
  debug.show_origins = false;
  debug.show_frame_timing = false;
  debug.show_stats = false;
  debug.show_ui_debug = false;
  debug.show_ui_rebuilds = false;
}

/// The rows: the animations asked for, each checked, else every one, else
/// the setup pose
fn rows(data: &SkeletonData, options: &SheetOptions) -> Result<Vec<Row>, SheetError> {
  let known = || {
    let names: Vec<&str> = data.animations().iter().map(|a| &*a.name).collect();
    if names.is_empty() {
      "none".to_owned()
    } else {
      names.join(", ")
    }
  };
  let row = |name: &str, duration: f32| Row {
    animation: Some(name.to_owned()),
    duration: if duration.is_finite() {
      duration.max(0.0)
    } else {
      0.0
    },
    label: name.to_owned(),
  };
  match &options.animations {
    Some(names) => names
      .iter()
      .map(|name| {
        data
          .animation(name)
          .map(|animation| row(name, animation.duration))
          .ok_or_else(|| SheetError::UnknownAnimation {
            name: name.clone(),
            known: known(),
          })
      })
      .collect(),
    None if data.animations().is_empty() => Ok(vec![Row {
      animation: None,
      duration: 0.0,
      label: "setup pose".to_owned(),
    }]),
    None => Ok(
      data
        .animations()
        .iter()
        .map(|animation| row(&animation.name, animation.duration))
        .collect(),
    ),
  }
}

fn check_skins(data: &SkeletonData, skins: &[String]) -> Result<(), SheetError> {
  for name in skins {
    if data.skin(name).is_none() {
      let known: Vec<&str> = data.skins().iter().map(|skin| &*skin.name).collect();
      return Err(SheetError::UnknownSkin {
        name: name.clone(),
        known: known.join(", "),
      });
    }
  }
  Ok(())
}

/// Each row's moments, `k × duration / moments` (one, the setup pose, for
/// a row without an animation), and the steps that reach each: a loop
/// round first, then the moment, `step` seconds a step, the entry starting
/// part of a step in so that its whole steps land on the moment.
///
/// Each is aimed a hundredth of a step past its moment: the steps' sum
/// falls a little short in `f32`, and a loop's first moment a hair short
/// of a whole loop would show the loop's last pose, not its first
fn cells(rows: &[Row], moments: u32, step: f32) -> Vec<Cell> {
  let mut cells = Vec::new();
  for (index, row) in rows.iter().enumerate() {
    let (count, nudge) = match row.animation {
      Some(_) => (moments, step * NUDGE),
      None => (1, 0.0),
    };
    for k in 0..count {
      let time = row.duration * k as f32 / count as f32;
      let (start, steps) = steps_to(row.duration + time + nudge, step);
      cells.push(Cell {
        row: index,
        time,
        start,
        steps,
        entity: None,
      });
    }
  }
  cells
}

/// The start under one step and the steps that put an animation `time`
/// seconds in, `step` seconds a step: a skeleton appears at its entry's
/// start, and each step moves it on a step
fn steps_to(time: f32, step: f32) -> (f32, u32) {
  if !(step > 0.0 && time.is_finite() && time > 0.0) {
    return (0.0, 0);
  }
  // A moment on a step's edge lands on it, not a step short of it
  let whole = (time / step + 1e-4).floor();
  let start = (time - whole * step).max(0.0);
  (start, whole as u32)
}

/// Steps every cell's skeleton to its moment. Each is spawned so that the
/// last step is its last, then a frame with the clock paused poses those
/// that took none: what is drawn after is each cell as its moment left it
fn run(
  engine: &mut HeadlessEngine,
  handle: &Handle<SkeletonData>,
  rows: &[Row],
  cells: &mut [Cell],
  options: &SheetOptions,
) -> Result<(), SheetError> {
  let most = cells.iter().map(|cell| cell.steps).max().unwrap_or(0);
  let skins: Vec<&str> = options.skins.iter().map(String::as_str).collect();
  // Those of no steps are spawned after the last, and appear in a frame
  // with the clock paused
  for step in 0..=most {
    for cell in cells.iter_mut().filter(|cell| most - cell.steps == step) {
      let mut skeleton = Skeleton::new(handle.clone());
      if !skins.is_empty() {
        skeleton.set_skins(&skins);
      }
      let world = engine.world_mut();
      let mut spawned = world.spawn((
        Transform::IDENTITY,
        skeleton,
        Renderable::skeleton(),
        RenderLayers::none(),
      ));
      if let Some(name) = rows.get(cell.row).and_then(|row| row.animation.as_deref()) {
        let mut animation = SkeletonAnimation::new();
        if let Some(id) = animation.set_animation(0, name, true)
          && let Some(entry) = animation.entry_mut(id)
        {
          entry.start = cell.start;
        }
        spawned.insert(animation);
      }
      // Drawn where its step left it, not between its last two
      spawned.remove::<Interpolated>();
      cell.entity = Some(spawned.id());
    }
    // The last frame runs no step: those just spawned appear in it
    if step == most {
      engine.world_mut().resource_mut::<Time>().game.pause();
    }
    engine.step()?;
  }
  Ok(())
}

/// The box of what `entity`'s skeleton draws, in its own space: its
/// regions' corners and its meshes' vertices, and its bones from origin to
/// tip when the overlay is drawn
fn pose_bounds(world: &World, entity: Entity, bones: bool) -> Option<Bounds> {
  let skeleton = world.get::<Skeleton>(entity)?;
  let assets = world.get_resource::<Assets<SkeletonData>>()?;
  let handle = skeleton.data();
  let data = assets.get(handle)?;
  let seen = (handle.id(), assets.version(handle)?);
  let mut parts = Parts::default();
  skeleton.parts(data, seen, Affine2::IDENTITY, None, &mut parts);
  let mut bounds = Bounds::EMPTY;
  for part in &parts.list {
    match part {
      Part::Region(quad) => quad.corners().into_iter().for_each(|c| bounds.add(c)),
      Part::Mesh(mesh) => parts.vertices(mesh).iter().for_each(|&v| bounds.add(v)),
    }
  }
  if bones && skeleton.seen() == Some(seen) {
    let placed = data.bones().iter().zip(&skeleton.pose.skeleton_space);
    for (bone, space) in placed {
      bounds.add(space.translation);
      bounds.add(space.transform_point2(Vec2::new(bone.length, 0.0)));
    }
  }
  (!bounds.is_empty()).then_some(bounds)
}

/// The cells' size, scale and origin for poses inside `union`: fitted into
/// `cell` when given, else the art's longer side `ART` pixels and the cell
/// sized round it
fn layout(union: Bounds, cell: Option<UVec2>) -> Layout {
  let size = (union.max - union.min).max(Vec2::splat(1.0));
  let margin = MARGIN as f32;
  let label = SHEET_LABEL as f32;
  let (cell, scale) = match cell {
    Some(cell) => {
      let room = Vec2::new(
        cell.x as f32 - 2.0 * margin,
        cell.y as f32 - label - 2.0 * margin,
      )
      .max(Vec2::splat(4.0));
      (cell, (room / size).min_element())
    }
    None => {
      let scale = ART as f32 / size.max_element();
      // What rounds a hair over a whole pixel is that pixel
      let art = (size * scale - 1e-3).ceil();
      let cell = UVec2::new(
        (art.x as u32 + 2 * MARGIN).max(MIN_WIDTH),
        art.y as u32 + 2 * MARGIN + SHEET_LABEL,
      );
      (cell, scale)
    }
  };
  let scale = scale.clamp(0.01, 100.0);
  // The poses' box centred in the room under the label
  let room = Vec2::new(cell.x as f32, cell.y as f32 - label);
  let corner = (room - size * scale) / 2.0;
  let origin = Vec2::new(corner.x, label + corner.y) - union.min * scale;
  Layout {
    cell,
    scale,
    origin,
  }
}

/// Draws each cell alone, with the clock paused, and puts it in its place
fn draw(
  engine: &mut HeadlessEngine,
  rows: &[Row],
  cells: &[Cell],
  layout: Layout,
  options: &SheetOptions,
) -> Result<RgbaImage, SheetError> {
  let cell = layout.cell;
  let columns = cells.iter().fold(Vec::<u32>::new(), |mut counts, item| {
    if counts.len() <= item.row {
      counts.resize(item.row + 1, 0);
    }
    if let Some(count) = counts.get_mut(item.row) {
      *count += 1;
    }
    counts
  });
  let wide = columns.iter().copied().max().unwrap_or(1);
  let size = |count: u32, side: u32| -> Result<u32, SheetError> {
    count
      .checked_mul(side + SHEET_GAP)
      .and_then(|size| size.checked_add(SHEET_GAP))
      .ok_or_else(|| SheetError::Options("the sheet is too large".to_owned()))
  };
  let rows_count = u32::try_from(rows.len()).unwrap_or(u32::MAX);
  let mut sheet = RgbaImage::from_pixel(size(wide, cell.x)?, size(rows_count, cell.y)?, GAP);

  place_camera(engine.world_mut(), layout);
  ground(engine.world_mut(), layout);
  let mut column = 0u32;
  let mut last_row = None;
  let mut shown: Option<Entity> = None;
  for item in cells {
    let Some(entity) = item.entity else {
      continue;
    };
    if last_row != Some(item.row) {
      column = 0;
      last_row = Some(item.row);
    }
    let world = engine.world_mut();
    if let Some(before) = shown.replace(entity) {
      world.entity_mut(before).insert(RenderLayers::none());
    }
    world.entity_mut(entity).insert(SEEN);
    let text = match rows.get(item.row) {
      Some(row) if row.animation.is_some() => format!("{}  {:.3}s", row.label, item.time),
      Some(row) => row.label.clone(),
      None => String::new(),
    };
    world.insert_resource(SheetLabel {
      text,
      width: cell.x as f32,
    });
    if options.bones {
      let pixel = 1.0 / layout.scale;
      world.resource_scope(|world, mut debug: Mut<DebugDraw>| {
        crate::core::skeleton::debug::draw(world, 0.0, pixel, &mut debug);
      });
    }
    engine.step()?;
    let frame = engine.frame()?;
    let drawn = imageops::crop_imm(&frame, 0, 0, cell.x, cell.y).to_image();
    let row = u32::try_from(item.row).unwrap_or(u32::MAX);
    let x = SHEET_GAP + column * (cell.x + SHEET_GAP);
    let y = SHEET_GAP + row * (cell.y + SHEET_GAP);
    imageops::replace(&mut sheet, &drawn, i64::from(x), i64::from(y));
    column += 1;
  }
  Ok(sheet)
}

/// The camera sees the skeletons' origin, where they all stand, at the
/// layout's origin in a cell, at its scale, over the background
fn place_camera(world: &mut World, layout: Layout) {
  let frame = world
    .get_resource::<crate::core::graphics::ScreenMapping>()
    .map_or(Vec2::ONE, crate::core::graphics::ScreenMapping::screen_size);
  let centre = (frame / 2.0 - layout.origin) / layout.scale;
  let mut camera = CameraMut::main_or_spawn(world);
  camera.set_zoom(layout.scale);
  camera.set_position(centre.x, centre.y);
  let entity = camera.entity();
  if let Some(mut camera) = world.get_mut::<Camera2d>(entity) {
    camera.clear = CameraClear::Color(BACKGROUND);
  }
  world.skip_interpolation(entity);
}

/// A faint line along the ground through the skeletons' origin, a pixel
/// thick, across the cell, under the art
fn ground(world: &mut World, layout: Layout) {
  let dot = world
    .resource_mut::<Assets<Image>>()
    .add(Image::solid_rect(1, 1, 255, 255, 255, 255));
  let across = layout.cell.x as f32 * 4.0 / layout.scale;
  let thick = 1.0 / layout.scale;
  world.spawn((
    Transform {
      scale: Vec2::new(across, thick),
      ..Transform::IDENTITY
    },
    Renderable::image(dot).with_tint(GROUND).with_z_order(-1),
    SEEN,
  ));
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_moment_is_reached_in_whole_steps_from_a_start_under_one() {
    let step = 1.0 / 60.0;
    // At 0, no step: the skeleton appears at its entry's start
    assert_eq!(steps_to(0.0, step), (0.0, 0));
    // 0.125 s is seven and a half steps: half a step in, then seven
    let (start, steps) = steps_to(0.125, step);
    assert_eq!(steps, 7);
    assert!((start - step / 2.0).abs() < 1e-5, "{start}");
    // On a step's edge: no start, and the steps land on it
    let (start, steps) = steps_to(0.5, step);
    assert_eq!(steps, 30);
    assert!(start.abs() < 1e-5, "{start}");
    // What cannot be stepped is the start alone
    assert_eq!(steps_to(f32::NAN, step), (0.0, 0));
    assert_eq!(steps_to(1.0, 0.0), (0.0, 0));
  }

  #[test]
  fn each_animation_is_run_once_round_before_its_moments() {
    let rows = [
      Row {
        animation: Some("walk".to_owned()),
        duration: 1.0,
        label: "walk".to_owned(),
      },
      Row {
        animation: None,
        duration: 0.0,
        label: "setup pose".to_owned(),
      },
    ];
    let cells = cells(&rows, 4, 0.25);
    // Four moments of the walk, a second round first; the setup pose once
    let times: Vec<(usize, f32, u32)> = cells.iter().map(|c| (c.row, c.time, c.steps)).collect();
    assert_eq!(
      times,
      vec![
        (0, 0.0, 4),
        (0, 0.25, 5),
        (0, 0.5, 6),
        (0, 0.75, 7),
        (1, 0.0, 0)
      ]
    );
    // Each a hundredth of a step past its moment, the setup pose at its own
    let starts: Vec<f32> = cells.iter().map(|c| c.start).collect();
    let want = [0.0025, 0.0025, 0.0025, 0.0025, 0.0];
    assert!(
      starts
        .iter()
        .zip(want)
        .all(|(start, want)| (start - want).abs() < 1e-6),
      "{starts:?}"
    );
  }

  #[test]
  fn the_layout_fits_every_pose_with_the_origin_in_one_place() {
    // Poses from 100 left of the origin to 300 right, and 200 above it
    let union = Bounds {
      min: Vec2::new(-100.0, -200.0),
      max: Vec2::new(300.0, 0.0),
    };
    // Sized by the sheet: the longer side `ART` pixels, centred in its room
    let fitted = layout(union, None);
    assert_eq!(fitted.scale, ART as f32 / 400.0);
    assert_eq!(
      fitted.cell,
      UVec2::new(ART + 2 * MARGIN, 120 + 2 * MARGIN + SHEET_LABEL)
    );
    let at = |layout: Layout, point: Vec2| layout.origin + point * layout.scale;
    let corner = Vec2::new(MARGIN as f32, (SHEET_LABEL + MARGIN) as f32);
    assert!(at(fitted, union.min).abs_diff_eq(corner, 1e-3));
    // A cell given: the scale the room allows, the box centred across it
    let given = layout(union, Some(UVec2::new(224, 200)));
    assert_eq!(given.cell, UVec2::new(224, 200));
    assert_eq!(given.scale, 0.5);
    assert!((at(given, union.min).x - MARGIN as f32).abs() < 1e-3);
    let top = at(given, union.min).y;
    let bottom = at(given, union.max).y;
    assert!((top - SHEET_LABEL as f32 - (200.0 - bottom)).abs() < 1e-3);
  }

  #[test]
  fn options_that_cannot_make_a_sheet_say_why() {
    let message = |options: SheetOptions| match check_options(&options) {
      Err(error) => error.to_string(),
      Ok(()) => String::new(),
    };
    assert!(
      message(SheetOptions {
        moments: 0,
        ..SheetOptions::default()
      })
      .contains("moments")
    );
    assert!(
      message(SheetOptions {
        cell: Some(UVec2::new(10, 10)),
        ..SheetOptions::default()
      })
      .contains("cell")
    );
    assert!(
      message(SheetOptions {
        animations: Some(Vec::new()),
        ..SheetOptions::default()
      })
      .contains("no animations")
    );
    assert!(message(SheetOptions::default()).is_empty());
  }
}
