//! Shadow planning: once a frame, the shadowed lights some lit camera can
//! see each get a slot of the shadow atlas (four rows, one per 90° view
//! around the light), and each edge that casts their shadows is written
//! once per view it crosses, for the atlas's draw.

use std::f32::consts::{FRAC_PI_2, FRAC_PI_4};

use glam::Vec2;

use super::LightOccluder2D;
use super::batch::{FrameLight, drawn_cone, light_instance};
use super::index::{IndexScratch, OccluderIndex};
use crate::core::ecs::{Entity, Floor, GlobalTransform};
use crate::core::graphics::RenderLayers;

/// The most lights a frame shadows; past it, those nearest a lit camera's
/// centre keep their shadows
pub(crate) const MAX_SHADOWED_LIGHTS: u32 = 512;

/// Atlas rows a shadowed light takes: its right, down, left and up views
const VIEWS: u32 = 4;

/// A light this near a closed occluder's boundary (px) stands on it: it is
/// outside the occluder, and the edges it stands on cast nothing for it
const ON_EDGE: f32 = 1e-4;

/// One casting edge in one view of one light, as the atlas's draw reads
/// it: its end points and the light's position where they stand (ground
/// space, world units), the light's radius in world units, and the atlas
/// row (the light's slot times four plus the view: 0 right, 1 down, 2
/// left, 3 up)
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct ShadowEdge {
  pub(crate) a: [f32; 2],
  pub(crate) b: [f32; 2],
  pub(crate) light: [f32; 2],
  pub(crate) radius: f32,
  pub(crate) row: u32,
}

/// A lit camera's view as the planner reads it: its layers and its world
/// box's min, max and centre
pub(crate) type ShadowView = (RenderLayers, Vec2, Vec2, Vec2);

/// An occluder as the planner reads it: where it is drawn this frame, its
/// shape and its floor
pub(crate) type PlannedOccluder<'w> = (GlobalTransform, &'w LightOccluder2D, Floor);

/// The frame's shadow plan, kept and reused frame to frame
#[derive(Default)]
pub(crate) struct ShadowPlan {
  /// Each light's slot (by its index in the frame's lights), if it has one
  pub(crate) slots: Vec<Option<u32>>,
  /// Every casting edge of every slotted light, per view it crosses
  pub(crate) edges: Vec<ShadowEdge>,
  /// Atlas rows the slots take: four a slot
  pub(crate) rows: u32,
  /// Edges left out for want of room (the farthest lights' first)
  pub(crate) dropped_edges: usize,
  /// The lights that may have a slot, as (distance to the nearest view's
  /// centre, light index)
  candidates: Vec<(f32, u32)>,
  found: Vec<Entity>,
  scratch: IndexScratch,
}

impl ShadowPlan {
  /// No slots and no edges: a frame with no shadow work
  pub(crate) fn clear(&mut self) {
    self.slots.clear();
    self.edges.clear();
    self.rows = 0;
    self.dropped_edges = 0;
  }

  #[cfg(test)]
  fn scratch_capacity(&self) -> usize {
    self.scratch.capacity()
  }
}

/// Plans the frame's shadows into `plan`: of `lights`, those that cast
/// shadows, would draw and whose box (lifted on a storey) meets a view in
/// `views` they share layers with each get a slot, at most `cap` (and
/// `MAX_SHADOWED_LIGHTS`) of them, those nearest a sharing view's centre
/// first (ties by light index), numbered in light order. Each slotted
/// light's edges come from the occluders `index` finds in its box (ground
/// space), read through `occluder`: those on a floor meeting its own,
/// enabled and finite, not a closed one holding the light, whose edges
/// face away from it (and lie near a spot's cone) and are longer than
/// 1e-4. At most `max_edges` edges are kept: the nearest lights' whole,
/// the rest counted in `dropped_edges` (their lights draw unshadowed).
/// Returns how many lights the cap left without a slot.
pub(crate) fn plan_shadows<'w>(
  plan: &mut ShadowPlan,
  lights: &[FrameLight],
  views: &[ShadowView],
  index: &OccluderIndex,
  mut occluder: impl FnMut(Entity) -> Option<PlannedOccluder<'w>>,
  cap: u32,
  max_edges: usize,
) -> u32 {
  let ShadowPlan {
    slots,
    edges,
    rows,
    dropped_edges,
    candidates,
    found,
    scratch,
  } = plan;
  *dropped_edges = 0;
  slots.clear();
  slots.resize(lights.len(), None);
  edges.clear();
  candidates.clear();
  for (i, frame) in lights.iter().enumerate() {
    if let Some(distance) = nearest_view(frame, views) {
      candidates.push((distance, i as u32));
    }
  }
  let cap = cap.min(MAX_SHADOWED_LIGHTS) as usize;
  let dropped = candidates.len().saturating_sub(cap);
  if dropped > 0 {
    // The nearest `cap`, ties to the earlier light, then back in light order
    if cap > 0 {
      candidates.select_nth_unstable_by(cap - 1, |a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    }
    candidates.truncate(cap);
    candidates.sort_unstable_by_key(|&(_, i)| i);
  }
  for (slot, &(_, i)) in candidates.iter().enumerate() {
    slots[i as usize] = Some(slot as u32);
  }
  *rows = candidates.len() as u32 * VIEWS;
  // Edges nearest light first, so a frame past the edge limit keeps the
  // nearest lights' shadows whole
  candidates.sort_unstable_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
  for &(_, i) in candidates.iter() {
    let Some(slot) = slots[i as usize] else {
      continue;
    };
    let light = &lights[i as usize];
    let kept = edges.len();
    index.query_into(
      light.position - light.light.radius(),
      light.position + light.light.radius(),
      scratch,
      found,
    );
    for &entity in found.iter() {
      if let Some(placed) = occluder(entity) {
        plan_occluder(light, slot, placed, edges);
      }
    }
    if *dropped_edges > 0 || edges.len() > max_edges {
      *dropped_edges += edges.len() - kept;
      edges.truncate(kept);
    }
  }
  dropped as u32
}

/// How far `frame`'s light (where it is drawn) is from the centre of the
/// nearest view that sees it; `None` if it casts no shadows, would not
/// draw, or no view sharing its layers meets its box
fn nearest_view(frame: &FrameLight, views: &[ShadowView]) -> Option<f32> {
  let drawn = frame.position + frame.lift;
  if !frame.light.casts_shadows() || light_instance(drawn, &frame.light).is_none() {
    return None;
  }
  let reach = Vec2::splat(frame.light.radius());
  let (min, max) = (drawn - reach, drawn + reach);
  views
    .iter()
    .filter(|(layers, low, high, _)| {
      // As a camera's view tests a box
      frame.layers.intersects(layers)
        && max.x > low.x
        && min.x < high.x
        && max.y > low.y
        && min.y < high.y
    })
    .map(|(.., centre)| drawn.distance(*centre))
    .min_by(f32::total_cmp)
}

/// Pushes the edges of `placed` that cast `frame`'s light's shadow, per
/// view each crosses, for the light's `slot`
fn plan_occluder(
  frame: &FrameLight,
  slot: u32,
  (transform, occluder, floor): PlannedOccluder,
  edges: &mut Vec<ShadowEdge>,
) {
  if !frame.floor.meets(floor) || !occluder.is_enabled() {
    return;
  }
  let light = frame.position;
  let spot = drawn_cone(&frame.light);
  let at = transform.translation();
  let corners = occluder.placed(at.x, at.y, transform.rotation());
  let closed = occluder.is_closed();
  let least = if closed { 3 } else { 2 };
  if corners.len() < least || !corners.clone().all(|corner| corner.is_finite()) {
    return;
  }
  // A polygon's last edge returns to its first vertex; a chain's does not
  let closing = if closed { corners.clone().next() } else { None };
  let ends = corners.clone().skip(1).chain(closing);
  if closed && contains(corners.clone().zip(ends.clone()), light) {
    return;
  }
  for (a, b) in corners.zip(ends) {
    // An edge the light stands on covers no angle round it
    if casts(a, b, light, spot) && a.distance(b) > 1e-4 && off_edge(a, b, light) {
      push_views(a, b, frame, slot, edges);
    }
  }
}

/// Whether `point` is farther than `ON_EDGE` from the segment `a`-`b`
fn off_edge(a: Vec2, b: Vec2, point: Vec2) -> bool {
  let along = b - a;
  let t = ((point - a).dot(along) / along.length_squared().max(f32::MIN_POSITIVE)).clamp(0.0, 1.0);
  point.distance(a + along * t) > ON_EDGE
}

/// Whether the closed polygon of `edges` holds `point` (even-odd). A point
/// on its boundary (within `ON_EDGE` of an edge) is outside: a light on an
/// occluder's edge or corner lights the side it faces and is shadowed by
/// the rest
fn contains(edges: impl Iterator<Item = (Vec2, Vec2)> + Clone, point: Vec2) -> bool {
  if edges.clone().any(|(a, b)| !off_edge(a, b, point)) {
    return false;
  }
  let mut inside = false;
  for (previous, corner) in edges {
    if (corner.y > point.y) != (previous.y > point.y) {
      let x =
        previous.x + (point.y - previous.y) * (corner.x - previous.x) / (corner.y - previous.y);
      if point.x < x {
        inside = !inside;
      }
    }
  }
  inside
}

/// Pushes the edge `a`-`b` once for each of the light's views it crosses:
/// those between its ends' angles, the short way round the light, an end
/// on a boundary entering both views
fn push_views(a: Vec2, b: Vec2, frame: &FrameLight, slot: u32, edges: &mut Vec<ShadowEdge>) {
  let light = frame.position;
  // Each angle in quarter turns from the right view's start at -45°: view
  // n covers n to n + 1
  let quarters = |to: Vec2| {
    let d = to - light;
    (d.y.atan2(d.x) + FRAC_PI_4) / FRAC_PI_2
  };
  let (at_a, at_b) = (quarters(a), quarters(b));
  // The way round from a to b is the side of the light b lies on, not the
  // rounded angles' difference: angles increase with a positive cross
  // product
  let (low, high) = if (a - light).perp_dot(b - light) >= 0.0 {
    (at_a, at_b)
  } else {
    (at_b, at_a)
  };
  let mut high = if high < low { high + 4.0 } else { high };
  // No casting edge spans half a turn round the light: past three
  // quarters, its ends' angles rounded past each other along a ray
  if high - low >= 3.0 {
    high = low;
  }
  let first = low.ceil() as i32 - 1;
  let last = (high.floor() as i32).min(first + VIEWS as i32 - 1);
  for view in first..=last {
    edges.push(ShadowEdge {
      a: a.into(),
      b: b.into(),
      light: light.into(),
      radius: frame.light.radius(),
      row: slot * VIEWS + view.rem_euclid(VIEWS as i32) as u32,
    });
  }
}

/// Whether the edge `v0`-`v1` faces away from a light at `light` (and, for
/// a spot of `(angle, direction)` in degrees, lies near its cone).
pub(super) fn casts(v0: Vec2, v1: Vec2, light: Vec2, spot: Option<(f32, f32)>) -> bool {
  let to_edge = (v0 + v1) * 0.5 - light;
  let edge = v1 - v0;
  // Edge normal for counter-clockwise winding: positive means it faces away
  if to_edge.dot(Vec2::new(-edge.y, edge.x)) <= 0.0 {
    return false;
  }
  let Some((angle, direction)) = spot else {
    return true;
  };
  let length = to_edge.length();
  if length <= 0.001 {
    return true;
  }
  let facing = Vec2::from_angle(direction.to_radians());
  let off_axis = facing.dot(to_edge / length).clamp(-1.0, 1.0).acos();
  // A margin past the cone keeps soft edges shadowed
  off_axis <= (angle / 2.0).to_radians() + 0.2
}

/// The edges `occluders` cast for `light` (slot 0), as the planner reads
/// them (tests that place occluders without an index)
#[cfg(test)]
pub(crate) fn casting_edges(light: &FrameLight, occluders: &[PlannedOccluder]) -> Vec<ShadowEdge> {
  let mut edges = Vec::new();
  for &placed in occluders {
    plan_occluder(light, 0, placed, &mut edges);
  }
  edges
}

/// Whether `point` (ground space) lies in `light`'s shadow of `occluders`:
/// within its radius, past an edge they cast for it, as the atlas and the
/// light shader find it (tests)
#[cfg(test)]
pub(crate) fn shadowed(point: Vec2, light: &FrameLight, occluders: &[PlannedOccluder]) -> bool {
  let from = light.position;
  if from.distance(point) > light.light.radius() {
    return false;
  }
  casting_edges(light, occluders).iter().any(|edge| {
    let (a, b) = (Vec2::from(edge.a), Vec2::from(edge.b));
    // The segment from the light to the point crosses the edge
    let side = |p: Vec2, q: Vec2, r: Vec2| (q - p).perp_dot(r - p);
    let (d1, d2) = (side(a, b, from), side(a, b, point));
    let (d3, d4) = (side(from, point, a), side(from, point, b));
    d1 * d2 < 0.0 && d3 * d4 < 0.0
  })
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::kernel::QueryState;
  use crate::core::ecs::{Transform, World};
  use crate::core::lighting::{Light2D, insert_occluder_index, update_occluder_index};
  use crate::core::ui::Color;

  type OccluderQuery = QueryState<(
    &'static GlobalTransform,
    &'static LightOccluder2D,
    Option<&'static Floor>,
  )>;

  /// A world with the occluder index, and a plan
  struct Stage {
    world: World,
    query: OccluderQuery,
    plan: ShadowPlan,
  }

  impl Stage {
    fn new() -> Stage {
      let mut world = World::new();
      insert_occluder_index(&mut world);
      let query = world.query();
      Stage {
        world,
        query,
        plan: ShadowPlan::default(),
      }
    }

    fn wall(&mut self, x: f32, y: f32, occluder: LightOccluder2D) -> Entity {
      self.world.spawn((Transform::from_xy(x, y), occluder)).id()
    }

    /// Updates the index, then plans `lights` for `views` under `cap`
    fn plan_capped(&mut self, lights: &[FrameLight], views: &[ShadowView], cap: u32) -> u32 {
      self.plan_with(lights, views, cap, usize::MAX)
    }

    /// `plan` with room for `max_edges` edges
    fn plan_limited(&mut self, lights: &[FrameLight], views: &[ShadowView], max_edges: usize) {
      self.plan_with(lights, views, MAX_SHADOWED_LIGHTS, max_edges);
    }

    fn plan_with(
      &mut self,
      lights: &[FrameLight],
      views: &[ShadowView],
      cap: u32,
      max_edges: usize,
    ) -> u32 {
      if let Err(error) = self.world.run_system_cached(update_occluder_index) {
        panic!("the index updates: {error}");
      }
      self.world.clear_trackers();
      let world = &self.world;
      let query = &mut self.query;
      plan_shadows(
        &mut self.plan,
        lights,
        views,
        world.resource::<OccluderIndex>(),
        |entity| {
          let (global, occluder, floor) = query.get(world, entity).ok()?;
          Some((*global, occluder, floor.copied().unwrap_or(Floor::GROUND)))
        },
        cap,
        max_edges,
      )
    }

    fn plan(&mut self, lights: &[FrameLight], views: &[ShadowView]) -> u32 {
      self.plan_capped(lights, views, MAX_SHADOWED_LIGHTS)
    }

    /// The planned edges as (a, b, row), sorted
    fn edges(&self) -> Vec<([u32; 2], [u32; 2], u32)> {
      let bits = |v: [f32; 2]| v.map(f32::to_bits);
      let mut edges: Vec<_> = self
        .plan
        .edges
        .iter()
        .map(|edge| (bits(edge.a), bits(edge.b), edge.row))
        .collect();
      edges.sort();
      edges
    }
  }

  /// A shadowed point light of `radius` at `at` on the ground floor
  fn lamp(at: Vec2, radius: f32) -> FrameLight {
    FrameLight {
      position: at,
      light: Light2D::point(Color::WHITE, radius).with_shadows(),
      layers: RenderLayers::default(),
      floor: Floor::GROUND,
      lift: Vec2::ZERO,
    }
  }

  /// A view of the default layers over the box from `min` to `max`
  fn view(min: Vec2, max: Vec2) -> ShadowView {
    (RenderLayers::default(), min, max, (min + max) * 0.5)
  }

  /// A view of 2,000 px around the origin
  fn wide() -> ShadowView {
    view(Vec2::splat(-1000.0), Vec2::splat(1000.0))
  }

  fn v(x: f32, y: f32) -> [f32; 2] {
    [x, y]
  }

  #[test]
  fn an_unseen_light_gets_no_slot() {
    let mut stage = Stage::new();
    let screen = view(Vec2::ZERO, Vec2::new(800.0, 600.0));
    let seen = lamp(Vec2::new(400.0, 300.0), 100.0);
    // Its box meets no view
    let off = lamp(Vec2::new(2000.0, 300.0), 100.0);
    // Its box meets the view, but on layers the camera does not show
    let mut hidden = lamp(Vec2::new(400.0, 300.0), 100.0);
    hidden.layers = RenderLayers::layer(5);
    // Seen, but casting no shadows
    let mut unshadowed = lamp(Vec2::new(400.0, 300.0), 100.0);
    unshadowed.light.set_cast_shadows(false);
    let dropped = stage.plan(&[off, seen, hidden, unshadowed], &[screen]);
    assert_eq!(dropped, 0);
    assert_eq!(stage.plan.slots, vec![None, Some(0), None, None]);
    assert_eq!(stage.plan.rows, 4);
    // Its box reaching into the view is enough
    let edge = lamp(Vec2::new(850.0, 300.0), 100.0);
    stage.plan(&[edge], &[screen]);
    assert_eq!(stage.plan.slots, vec![Some(0)]);
    // No view, no slots
    stage.plan(&[lamp(Vec2::new(400.0, 300.0), 100.0)], &[]);
    assert_eq!(stage.plan.slots, vec![None]);
    assert_eq!(stage.plan.rows, 0);
  }

  #[test]
  fn a_light_seen_by_two_cameras_gets_one_slot() {
    let mut stage = Stage::new();
    stage.wall(50.0, 0.0, LightOccluder2D::rect(20.0, 20.0));
    let light = lamp(Vec2::ZERO, 200.0);
    stage.plan(std::slice::from_ref(&light), &[wide()]);
    let once = stage.edges();
    assert!(!once.is_empty());
    let left = view(Vec2::new(-800.0, -300.0), Vec2::new(100.0, 300.0));
    let right = view(Vec2::new(-100.0, -300.0), Vec2::new(800.0, 300.0));
    stage.plan(&[light], &[left, right]);
    assert_eq!(stage.plan.slots, vec![Some(0)]);
    assert_eq!(stage.plan.rows, 4);
    assert_eq!(stage.edges(), once, "its edges planned once");
  }

  #[test]
  fn only_edges_facing_away_cast() {
    let mut stage = Stage::new();
    stage.wall(50.0, 0.0, LightOccluder2D::rect(20.0, 20.0));
    stage.plan(&[lamp(Vec2::ZERO, 200.0)], &[wide()]);
    // The corners, counter-clockwise on screen: (40, -10), (40, 10),
    // (60, 10), (60, -10). The near side faces the light: the far side and
    // the two ends cast
    let mut expected = vec![
      (v(40.0, 10.0), v(60.0, 10.0)),
      (v(60.0, 10.0), v(60.0, -10.0)),
      (v(60.0, -10.0), v(40.0, -10.0)),
    ];
    expected.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
    let mut cast: Vec<_> = stage.plan.edges.iter().map(|e| (e.a, e.b)).collect();
    cast.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
    assert_eq!(cast, expected);
    for edge in &stage.plan.edges {
      assert_eq!((edge.light, edge.radius, edge.row), ([0.0, 0.0], 200.0, 0));
    }
  }

  #[test]
  fn a_chain_casts_as_the_polygon_it_bounds() {
    // An L, counter-clockwise on screen as polygons are
    let ring = vec![
      Vec2::new(0.0, 0.0),
      Vec2::new(0.0, 30.0),
      Vec2::new(30.0, 30.0),
      Vec2::new(30.0, 20.0),
      Vec2::new(10.0, 20.0),
      Vec2::new(10.0, 0.0),
    ];
    let at = Vec2::new(40.0, 10.0);
    let (first, last) = (ring[0] + at, ring[5] + at);
    for from in [
      Vec2::ZERO,
      Vec2::new(-60.0, 15.0),
      Vec2::new(100.0, 100.0),
      Vec2::new(55.0, -40.0),
      Vec2::new(60.0, 45.0),
    ] {
      let mut polygon = Stage::new();
      polygon.wall(at.x, at.y, LightOccluder2D::polygon(ring.clone()));
      polygon.plan(&[lamp(from, 200.0)], &[wide()]);
      let mut chain = Stage::new();
      chain.wall(at.x, at.y, LightOccluder2D::chain(ring.clone()));
      chain.plan(&[lamp(from, 200.0)], &[wide()]);
      assert!(!polygon.plan.edges.is_empty(), "from {from}");
      // The polygon's edges but its closing one, from its last vertex to
      // its first
      let bits = |p: Vec2| p.to_array().map(f32::to_bits);
      let expected: Vec<_> = polygon
        .edges()
        .into_iter()
        .filter(|&(a, b, _)| (a, b) != (bits(last), bits(first)))
        .collect();
      assert_eq!(chain.edges(), expected, "from {from}");
    }
  }

  #[test]
  fn a_closed_occluder_round_the_light_casts_nothing_for_it() {
    let mut stage = Stage::new();
    stage.wall(0.0, 0.0, LightOccluder2D::rect(100.0, 100.0));
    let inside = lamp(Vec2::new(10.0, -5.0), 300.0);
    let outside = lamp(Vec2::new(200.0, 0.0), 300.0);
    stage.plan(&[inside.clone(), outside.clone()], &[wide()]);
    assert_eq!(stage.plan.slots, vec![Some(0), Some(1)]);
    assert!(
      stage.plan.edges.iter().all(|edge| edge.row / 4 == 1),
      "only the light outside casts it"
    );
    assert!(!stage.plan.edges.is_empty());
    // The same ring as a chain has no inside: it casts round the light
    let mut chain = Stage::new();
    let corners = LightOccluder2D::rect(100.0, 100.0).vertices().to_vec();
    let mut closed = corners.clone();
    closed.push(corners[0]);
    chain.wall(0.0, 0.0, LightOccluder2D::chain(closed));
    chain.plan(&[inside], &[wide()]);
    let views: std::collections::BTreeSet<u32> =
      chain.plan.edges.iter().map(|edge| edge.row).collect();
    assert_eq!(views.len(), 4, "all round it: {views:?}");
  }

  #[test]
  fn a_light_on_a_closed_occluder_s_boundary_stands_outside_it() {
    // A block from (-50, -50) to (50, 50): a light on any edge or corner is
    // outside it, so its far edges cast and it lights the side it faces
    for at in [
      Vec2::new(50.0, 0.0),
      Vec2::new(-50.0, 0.0),
      Vec2::new(0.0, 50.0),
      Vec2::new(0.0, -50.0),
      Vec2::new(50.0, 50.0),
      Vec2::new(-50.0, -50.0),
      Vec2::new(-50.0, 50.0),
      Vec2::new(50.0, -50.0),
      // Off the edge by less than its tolerance
      Vec2::new(-50.0 + 5e-5, 10.0),
    ] {
      let mut stage = Stage::new();
      stage.wall(0.0, 0.0, LightOccluder2D::rect(100.0, 100.0));
      stage.plan(&[lamp(at, 300.0)], &[wide()]);
      assert!(!stage.plan.edges.is_empty(), "a light at {at} casts it");
      // The far edges cast, never the one it stands on
      for edge in &stage.plan.edges {
        let (a, b) = (Vec2::from(edge.a), Vec2::from(edge.b));
        assert!((b - a).perp_dot(at - a).abs() > 1.0, "{at}: {a} to {b}");
      }
    }
    // Just inside, it holds the light
    let mut stage = Stage::new();
    stage.wall(0.0, 0.0, LightOccluder2D::rect(100.0, 100.0));
    stage.plan(&[lamp(Vec2::new(-49.9, 10.0), 300.0)], &[wide()]);
    assert!(stage.plan.edges.is_empty());
  }

  #[test]
  fn past_the_edge_limit_the_nearest_lights_keep_their_edges() {
    // Three lights at 0, 300 and 600 px from the view's centre, each with
    // a wall to its right casting 3 edges in its right view
    let mut stage = Stage::new();
    let lights: Vec<FrameLight> = [600.0, 0.0, 300.0]
      .into_iter()
      .map(|x| {
        stage.wall(x + 50.0, 0.0, LightOccluder2D::rect(20.0, 20.0));
        lamp(Vec2::new(x, 0.0), 100.0)
      })
      .collect();
    stage.plan(&lights, &[wide()]);
    assert_eq!(stage.plan.edges.len(), 9);
    assert_eq!(stage.plan.dropped_edges, 0);
    // Room for 7: the two nearest lights' 6 edges, not part of the third's
    stage.plan_limited(&lights, &[wide()], 7);
    assert_eq!(stage.plan.slots, vec![Some(0), Some(1), Some(2)]);
    let mut slots: Vec<u32> = stage.plan.edges.iter().map(|e| e.row / 4).collect();
    slots.sort_unstable();
    assert_eq!(slots, [1, 1, 1, 2, 2, 2]);
    assert_eq!(stage.plan.dropped_edges, 3);
  }

  #[test]
  fn an_edge_spanning_two_views_is_planned_in_both() {
    let rows = |stage: &Stage| -> Vec<u32> { stage.plan.edges.iter().map(|e| e.row).collect() };
    let cast = |a: Vec2, b: Vec2| {
      let mut stage = Stage::new();
      stage.wall(0.0, 0.0, LightOccluder2D::chain(vec![a, b]));
      // A light before it takes slot 0, so this one's rows are 4 to 7
      let first = lamp(Vec2::new(-900.0, -900.0), 10.0);
      stage.plan(&[first, lamp(Vec2::ZERO, 400.0)], &[wide()]);
      assert_eq!(stage.plan.slots, vec![Some(0), Some(1)]);
      assert!(stage.plan.edges.iter().all(|e| e.a == a.to_array()));
      let mut rows = rows(&stage);
      rows.sort_unstable();
      rows
    };
    // From 56° (down) to 27° (right) round the light
    assert_eq!(
      cast(Vec2::new(100.0, 150.0), Vec2::new(100.0, 50.0)),
      [4, 5]
    );
    // Within the right view only
    assert_eq!(cast(Vec2::new(100.0, 40.0), Vec2::new(100.0, -40.0)), [4]);
    // An end on the boundary at 45° enters both views
    assert_eq!(
      cast(Vec2::new(100.0, 100.0), Vec2::new(100.0, 50.0)),
      [4, 5]
    );
    // So does one at -45°, the arc's other end
    assert_eq!(
      cast(Vec2::new(100.0, 0.0), Vec2::new(100.0, -100.0)),
      [4, 7]
    );
    // Behind the light, across ±180°: the left view only
    assert_eq!(cast(Vec2::new(-100.0, -50.0), Vec2::new(-100.0, 50.0)), [6]);
    // From the right view (-27°) into up (-117°)
    assert_eq!(
      cast(Vec2::new(200.0, -100.0), Vec2::new(-50.0, -100.0)),
      [4, 7]
    );
    // Wound towards the light, it casts in no view
    assert!(cast(Vec2::new(-50.0, -100.0), Vec2::new(200.0, -100.0)).is_empty());
    // A wall below the light, from 166° (left) round through down to 14°
    // (right): three views
    assert_eq!(
      cast(Vec2::new(-200.0, 50.0), Vec2::new(200.0, 50.0)),
      [4, 5, 6]
    );
    // One passing a thousandth of a pixel from the light keeps them
    assert_eq!(
      cast(Vec2::new(-390.0, 0.001), Vec2::new(390.0, 0.001)),
      [4, 5, 6]
    );
  }

  #[test]
  fn a_spot_skips_edges_outside_its_cone() {
    let mut stage = Stage::new();
    let ahead = stage.wall(100.0, 0.0, LightOccluder2D::rect(4.0, 4.0));
    stage.wall(0.0, 100.0, LightOccluder2D::rect(4.0, 4.0));
    stage.wall(-100.0, 0.0, LightOccluder2D::rect(4.0, 4.0));
    // 20° off its axis: past its 15° half-angle, within the margin
    let turn = 20f32.to_radians();
    let edge = stage.wall(
      100.0 * turn.cos(),
      100.0 * turn.sin(),
      LightOccluder2D::rect(4.0, 4.0),
    );
    let spot = FrameLight {
      light: Light2D::spot(Color::WHITE, 300.0, 30.0, 0.0).with_shadows(),
      ..lamp(Vec2::ZERO, 300.0)
    };
    stage.plan(&[spot], &[wide()]);
    let near = |e: &ShadowEdge, at: Entity| {
      let Some(transform) = stage.world.get::<GlobalTransform>(at) else {
        return false;
      };
      let middle = (Vec2::from(e.a) + Vec2::from(e.b)) * 0.5;
      middle.distance(transform.translation()) < 4.0
    };
    assert!(!stage.plan.edges.is_empty());
    assert!(stage.plan.edges.iter().any(|e| near(e, ahead)));
    assert!(
      stage.plan.edges.iter().any(|e| near(e, edge)),
      "at its edge"
    );
    assert!(
      stage
        .plan
        .edges
        .iter()
        .all(|e| near(e, ahead) || near(e, edge)),
      "none from the walls outside it"
    );
  }

  #[test]
  fn a_floor_light_reads_only_its_floor_s_occluders() {
    let mut stage = Stage::new();
    // On the ground floor (no `Floor`), floor 1, every floor and floor 2
    stage.wall(50.0, 0.0, LightOccluder2D::rect(4.0, 4.0));
    let own = stage
      .world
      .spawn((
        Transform::from_xy(0.0, 50.0),
        LightOccluder2D::rect(4.0, 4.0),
        Floor::On(1),
      ))
      .id();
    let every = stage
      .world
      .spawn((
        Transform::from_xy(-50.0, 0.0),
        LightOccluder2D::rect(4.0, 4.0),
        Floor::Every,
      ))
      .id();
    stage.world.spawn((
      Transform::from_xy(0.0, -50.0),
      LightOccluder2D::rect(4.0, 4.0),
      Floor::On(2),
    ));
    let mut light = lamp(Vec2::ZERO, 200.0);
    light.floor = Floor::On(1);
    light.lift = Vec2::new(0.0, -32.0);
    stage.plan(&[light.clone()], &[wide()]);
    let mut from: Vec<Entity> = stage
      .plan
      .edges
      .iter()
      .map(|e| {
        let middle = (Vec2::from(e.a) + Vec2::from(e.b)) * 0.5;
        if middle.distance(Vec2::new(0.0, 50.0)) < 4.0 {
          own
        } else if middle.distance(Vec2::new(-50.0, 0.0)) < 4.0 {
          every
        } else {
          Entity::PLACEHOLDER
        }
      })
      .collect();
    from.sort();
    from.dedup();
    let mut expected = vec![own, every];
    expected.sort();
    assert_eq!(from, expected);
    // Cast where it stands, not where it is drawn
    assert!(stage.plan.edges.iter().all(|e| e.light == [0.0, 0.0]));
    // Seen where it is drawn: a view over its lifted box only
    let mut small = light;
    small.light = Light2D::point(Color::WHITE, 10.0).with_shadows();
    let above = view(Vec2::new(-100.0, -1000.0), Vec2::new(100.0, -20.0));
    stage.plan(&[small.clone()], &[above]);
    assert_eq!(stage.plan.slots, vec![Some(0)]);
    small.lift = Vec2::ZERO;
    stage.plan(&[small], &[above]);
    assert_eq!(stage.plan.slots, vec![None]);
  }

  #[test]
  fn past_the_cap_the_nearest_lights_keep_their_slots() {
    let mut stage = Stage::new();
    // 600 lights at distances 0 to 599 from the view's centre, out of order
    let distance = |i: u32| (i * 7) % 600;
    let lights: Vec<FrameLight> = (0..600)
      .map(|i| lamp(Vec2::new(distance(i) as f32, 0.0), 4.0))
      .collect();
    let dropped = stage.plan(&lights, &[wide()]);
    assert_eq!(dropped, 88);
    assert_eq!(stage.plan.rows, 512 * 4);
    let mut next = 0;
    for (i, slot) in stage.plan.slots.iter().enumerate() {
      let near = distance(i as u32) < 512;
      assert_eq!(slot.is_some(), near, "light {i}");
      // Numbered in light order
      if let Some(slot) = slot {
        assert_eq!(*slot, next);
        next += 1;
      }
    }
    // A lower cap; ties go to the earlier light
    let twins = [
      lamp(Vec2::new(30.0, 0.0), 4.0),
      lamp(Vec2::new(0.0, 30.0), 4.0),
      lamp(Vec2::new(-30.0, 0.0), 4.0),
    ];
    assert_eq!(stage.plan_capped(&twins, &[wide()], 2), 1);
    assert_eq!(stage.plan.slots, vec![Some(0), Some(1), None]);
    // The nearest of a light's views counts
    let far = view(Vec2::new(5000.0, -10.0), Vec2::new(5020.0, 10.0));
    let lights = [
      lamp(Vec2::new(100.0, 0.0), 4.0),
      lamp(Vec2::new(5010.0, 0.0), 4.0),
    ];
    assert_eq!(stage.plan_capped(&lights, &[wide(), far], 1), 1);
    assert_eq!(stage.plan.slots, vec![None, Some(0)]);
  }

  #[test]
  fn degenerate_edges_and_lights_plan_nothing() {
    let mut stage = Stage::new();
    // A zero-length chain edge, a polygon of one point, a vertex that is
    // not finite
    stage.wall(
      0.0,
      0.0,
      LightOccluder2D::chain(vec![Vec2::new(100.0, 0.0); 2]),
    );
    stage.wall(
      0.0,
      0.0,
      LightOccluder2D::polygon(vec![Vec2::new(0.0, 100.0); 3]),
    );
    stage.wall(
      0.0,
      0.0,
      LightOccluder2D::polygon(vec![
        Vec2::new(-100.0, 0.0),
        Vec2::new(f32::NAN, 10.0),
        Vec2::new(-120.0, 10.0),
      ]),
    );
    stage.plan(&[lamp(Vec2::ZERO, 300.0)], &[wide()]);
    assert_eq!(stage.plan.slots, vec![Some(0)]);
    assert!(stage.plan.edges.is_empty(), "{:?}", stage.plan.edges);
    // Lights that cannot light anything
    let mut flat = lamp(Vec2::ZERO, 300.0);
    if let Light2D::Point { radius, .. } = &mut flat.light {
      *radius = 0.0;
    }
    let mut endless = lamp(Vec2::ZERO, 300.0);
    if let Light2D::Point { radius, .. } = &mut endless.light {
      *radius = f32::INFINITY;
    }
    let lost = lamp(Vec2::new(f32::NAN, 0.0), 300.0);
    let mut lifted = lamp(Vec2::ZERO, 300.0);
    lifted.lift = Vec2::new(0.0, f32::NAN);
    let mut dark = lamp(Vec2::ZERO, 300.0);
    dark.light.set_intensity(0.0);
    stage.wall(50.0, 0.0, LightOccluder2D::rect(10.0, 10.0));
    let dropped = stage.plan(&[flat, endless, lost, lifted, dark], &[wide()]);
    assert_eq!(dropped, 0);
    assert_eq!(stage.plan.slots, vec![None; 5]);
    assert!(stage.plan.edges.is_empty());
    assert_eq!(stage.plan.rows, 0);
    // A view that is not finite sees nothing
    let lost_view = view(Vec2::splat(f32::NAN), Vec2::splat(f32::NAN));
    stage.plan(&[lamp(Vec2::ZERO, 300.0)], &[lost_view]);
    assert_eq!(stage.plan.slots, vec![None]);
  }

  #[test]
  fn planning_a_static_frame_allocates_nothing() {
    let mut stage = Stage::new();
    for i in 0..40 {
      let x = (i % 8) as f32 * 120.0 - 480.0;
      let y = (i / 8) as f32 * 120.0 - 300.0;
      stage.wall(x, y, LightOccluder2D::rect(30.0, 30.0));
    }
    let lights: Vec<FrameLight> = (0..12)
      .map(|i| {
        lamp(
          Vec2::new(i as f32 * 80.0 - 480.0, (i % 3) as f32 * 90.0),
          250.0,
        )
      })
      .collect();
    let capacities = |plan: &ShadowPlan| {
      (
        plan.slots.capacity(),
        plan.edges.capacity(),
        plan.candidates.capacity(),
        plan.found.capacity(),
        plan.scratch_capacity(),
      )
    };
    stage.plan(&lights, &[wide()]);
    let edges = stage.plan.edges.clone();
    assert!(!edges.is_empty());
    stage.plan(&lights, &[wide()]);
    let warm = capacities(&stage.plan);
    for _ in 0..3 {
      stage.plan(&lights, &[wide()]);
      assert_eq!(capacities(&stage.plan), warm);
      assert_eq!(stage.plan.edges, edges, "the same plan");
    }
  }
}
