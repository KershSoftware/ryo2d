//! Collider geometry as particles meet it: world colliders gathered into
//! circles, boxes and edges, and the contact of a particle circle with them.

use super::collision::CollisionConfig;
use crate::core::ecs::{ChildOf, Entity, GlobalTransform, Has, QueryState, Vec2, World};
use crate::core::physics::{Collider, ColliderShape, TileColliders, TilePart};

/// Gap left between a bounced particle and the surface so it starts the next step clear of it.
pub(crate) const CONTACT_SKIN: f32 = 0.01;

/// World-space geometry of a collider, as seen by particles.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ColliderGeometry {
  Box {
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
  },
  Circle {
    x: f32,
    y: f32,
    radius: f32,
  },
  /// Edges (a segment, a polyline, or segments such as a tile map's packed
  /// outlines): the system's edges `start..end`, in world space, within the
  /// box `min..max`. `solid` is `Some(reach)` for a tile part's loops,
  /// which bound solid ground: each edge is wound so its left normal,
  /// `(-along.y, along.x)`, points out of it, and a particle up to `reach`
  /// (half a cell) inside goes out (`solid_contact`). Other edges are
  /// two-sided
  Edges {
    start: u32,
    end: u32,
    min: Vec2,
    max: Vec2,
    solid: Option<f32>,
  },
}

/// A world collider snapshot, gathered once per update.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ParticleCollider {
  pub(crate) entity: Entity,
  pub(crate) geometry: ColliderGeometry,
  /// `Collider::layer`, matched against `CollisionConfig::collision_mask`
  pub(crate) layer: u32,
}

/// Where a particle touches a collider.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Contact {
  pub(crate) collider: Entity,
  /// Unit normal pointing from the surface towards the particle
  pub(crate) normal_x: f32,
  pub(crate) normal_y: f32,
  /// Closest point on the collider surface
  pub(crate) point_x: f32,
  pub(crate) point_y: f32,
}

impl ParticleCollider {
  /// The collider as particles meet it; a segment, polyline or segments
  /// collider's edges are added to `edges`, in world space. `cell` is a
  /// tile part's grid cell, in world pixels (`None` for a game's own
  /// collider). A tile part that is not one-way holds the loops of a map's
  /// outline, which physics winds clockwise on screen round the solid
  /// (holes too: the solid is on the same side of every edge), as segments
  /// or, for one loop, a closed polyline: its edges are turned round so
  /// their left normals point out of the solid (unless the placement
  /// mirrors them, which turns them already). Every other collider's edges
  /// are two-sided, as physics treats them
  pub(crate) fn from_component(
    entity: Entity,
    global: &GlobalTransform,
    (collider, cell): (&Collider, Option<f32>),
    edges: &mut Vec<(Vec2, Vec2)>,
  ) -> Self {
    let affine = global.affine();
    let turn = Vec2::from_angle(collider.rotation);
    // Placed as physics places the shape's points
    let place = |p: Vec2| affine.transform_point2(collider.offset + turn.rotate(p));
    let start = edges.len();
    let solid = cell
      .filter(|_| collider.one_way.is_none())
      .map(|cell| cell * 0.5);
    let turned = solid.is_some() && affine.matrix2.determinant() >= 0.0;
    let edge = |a: Vec2, b: Vec2| if turned { (b, a) } else { (a, b) };
    match &collider.shape {
      ColliderShape::Segment { a, b } => edges.push((place(*a), place(*b))),
      ColliderShape::Polyline { points } => {
        edges.extend(points.windows(2).map(|w| edge(place(w[0]), place(w[1]))));
      }
      ColliderShape::Segments { points, segments } => {
        edges.extend(segments.iter().filter_map(|&[a, b]| {
          Some(edge(
            place(*points.get(a as usize)?),
            place(*points.get(b as usize)?),
          ))
        }));
      }
      _ => {}
    }
    let solid = match collider.shape {
      ColliderShape::Polyline { .. } | ColliderShape::Segments { .. } => solid,
      _ => None,
    };
    let geometry = match collider.shape {
      ColliderShape::Circle { radius } => {
        let center = global.transform_point(collider.offset);
        ColliderGeometry::Circle {
          x: center.x,
          y: center.y,
          radius: radius * global.scale().max_element(),
        }
      }
      ColliderShape::Segment { .. }
      | ColliderShape::Polyline { .. }
      | ColliderShape::Segments { .. } => {
        let placed = &edges[start..];
        let (min, max) = placed.iter().fold(
          (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)),
          |(min, max), &(a, b)| (min.min(a).min(b), max.max(a).max(b)),
        );
        ColliderGeometry::Edges {
          start: u32::try_from(start).unwrap_or(u32::MAX),
          end: u32::try_from(edges.len()).unwrap_or(u32::MAX),
          min,
          max,
          solid,
        }
      }
      // Solid shapes collide by their world bounds
      _ => {
        let (min, max) = collider.world_bounds(global);
        ColliderGeometry::Box {
          min_x: min.x,
          min_y: min.y,
          max_x: max.x,
          max_y: max.y,
        }
      }
    };
    Self {
      entity,
      geometry,
      layer: collider.layer,
    }
  }

  /// Test a particle circle at `(x, y)`, which was at `from` before this
  /// update moved it, against this collider (whose edges, if any, are in
  /// `edges`).
  pub(crate) fn contact(
    &self,
    edges: &[(Vec2, Vec2)],
    from: Vec2,
    (x, y): (f32, f32),
    radius: f32,
  ) -> Option<Contact> {
    let (normal_x, normal_y, point_x, point_y) = match self.geometry {
      ColliderGeometry::Edges {
        start,
        end,
        min,
        max,
        solid,
      } => {
        let to = Vec2::new(x, y);
        // Its move, give or take how far it is met, misses the edges' box:
        // it misses every edge
        let reach = solid.map_or(radius, |reach| reach.max(radius));
        let (low, high) = (to.min(from) - reach, to.max(from) + reach);
        if low.x > max.x || low.y > max.y || high.x < min.x || high.y < min.y {
          return None;
        }
        let edges = edges.get(start as usize..end as usize)?;
        let (_, point, normal) = match solid {
          Some(reach) => solid_contact(edges, (from, to), radius, reach)?,
          None => edges
            .iter()
            .filter_map(|&(a, b)| edge_contact(a, b, from, to, radius))
            .min_by(|a, b| a.0.total_cmp(&b.0))?,
        };
        (normal.x, normal.y, point.x, point.y)
      }
      ColliderGeometry::Box {
        min_x,
        min_y,
        max_x,
        max_y,
      } => {
        // min/max instead of clamp: clamp panics on NaN bounds
        let closest_x = x.max(min_x).min(max_x);
        let closest_y = y.max(min_y).min(max_y);
        let dx = x - closest_x;
        let dy = y - closest_y;
        let dist_sq = dx * dx + dy * dy;
        if dist_sq > 0.0 {
          if dist_sq >= radius * radius {
            return None;
          }
          let dist = dist_sq.sqrt();
          (dx / dist, dy / dist, closest_x, closest_y)
        } else {
          // Centre inside the box: push out through the nearest edge
          let top = y - min_y;
          let bottom = max_y - y;
          let left = x - min_x;
          let right = max_x - x;
          let nearest = top.min(bottom).min(left).min(right);
          if nearest == top {
            (0.0, -1.0, x, min_y)
          } else if nearest == bottom {
            (0.0, 1.0, x, max_y)
          } else if nearest == left {
            (-1.0, 0.0, min_x, y)
          } else {
            (1.0, 0.0, max_x, y)
          }
        }
      }
      ColliderGeometry::Circle {
        x: cx,
        y: cy,
        radius: circle_radius,
      } => {
        let dx = x - cx;
        let dy = y - cy;
        let reach = circle_radius + radius;
        let dist_sq = dx * dx + dy * dy;
        if dist_sq >= reach * reach {
          return None;
        }
        let dist = dist_sq.sqrt();
        // Exactly at the centre: push out upwards
        let (nx, ny) = if dist > 0.0 {
          (dx / dist, dy / dist)
        } else {
          (0.0, -1.0)
        };
        (nx, ny, cx + nx * circle_radius, cy + ny * circle_radius)
      }
    };
    Some(Contact {
      collider: self.entity,
      normal_x,
      normal_y,
      point_x,
      point_y,
    })
  }
}

/// Where a particle that moved from `from` to `to` meets the two-sided edge
/// `a`-`b`: a rank (lower meets first), the point on the edge, and the unit
/// normal pointing to the side it came from. An edge its move crossed is
/// met where it crossed it, before any it ends near (ranked by how far
/// along its move, below 0); otherwise one whose nearest point lies within
/// `radius` of `to` (ranked by distance over radius, 0 to 1), its normal
/// the edge's along its length and from its end at an end
pub(crate) fn edge_contact(
  a: Vec2,
  b: Vec2,
  from: Vec2,
  to: Vec2,
  radius: f32,
) -> Option<(f32, Vec2, Vec2)> {
  let along = b - a;
  let length = along.length();
  if !(length > 0.0 && length.is_finite()) {
    return None;
  }
  let normal = Vec2::new(-along.y, along.x) / length;
  let (before, after) = (normal.dot(from - a), normal.dot(to - a));
  // Crossed the edge's line this update, within the edge
  if (before > 0.0 && after < 0.0) || (before < 0.0 && after > 0.0) {
    let t = before / (before - after);
    let crossing = from + (to - from) * t;
    let u = (crossing - a).dot(along) / (length * length);
    if (0.0..=1.0).contains(&u) {
      return Some((t - 1.0, crossing, normal * before.signum()));
    }
  }
  let u = ((to - a).dot(along) / (length * length)).clamp(0.0, 1.0);
  let nearest = a + along * u;
  let gap = to - nearest;
  let distance = gap.length();
  if distance.is_nan() || distance >= radius {
    return None;
  }
  // The side it is on (and was on: it did not cross)
  let side = if after != 0.0 {
    after.signum()
  } else if before != 0.0 {
    before.signum()
  } else {
    1.0
  };
  let facing = if (u > 0.0 && u < 1.0) || distance == 0.0 {
    normal * side
  } else {
    gap / distance
  };
  Some((distance / radius, nearest, facing))
}

/// Where a particle that moved from `from` to `to` meets a tile part's
/// loops `edges`, which bound solid ground with each edge's left normal
/// pointing out: a rank (lower meets first), the point, and the unit
/// normal out of the ground. A move into the ground across an edge is met
/// where it crossed in (ranked by how far along its move, below 0); a move
/// out is not met. Otherwise the part's nearest point to `to` decides,
/// locally, since the loops round a place may lie in several parts (a room
/// carved from a wall has its own loop, likely in another chunk's part): if
/// it is within `reach` and `to` is on its inner side (by the edge's
/// normal, or at a corner the sum of its two edges' normals), the particle
/// is in the ground and goes out there (ranked 0); if it is on the outer
/// side within `radius`, it touches (ranked by distance over radius).
/// Ground deeper than `reach` from this part's edges is not judged
pub(crate) fn solid_contact(
  edges: &[(Vec2, Vec2)],
  (from, to): (Vec2, Vec2),
  radius: f32,
  reach: f32,
) -> Option<(f32, Vec2, Vec2)> {
  let outward = |a: Vec2, b: Vec2| {
    let along = b - a;
    let length = along.length();
    (length > 0.0 && length.is_finite()).then(|| Vec2::new(-along.y, along.x) / length)
  };
  // Crossed into the ground this update
  let mut crossed: Option<(f32, Vec2, Vec2)> = None;
  for &(a, b) in edges {
    let Some(normal) = outward(a, b) else {
      continue;
    };
    let (before, after) = (normal.dot(from - a), normal.dot(to - a));
    if before > 0.0 && after < 0.0 {
      let t = before / (before - after);
      let crossing = from + (to - from) * t;
      let u = (crossing - a).dot(b - a) / (b - a).length_squared();
      if (0.0..=1.0).contains(&u) && crossed.is_none_or(|c| t - 1.0 < c.0) {
        crossed = Some((t - 1.0, crossing, normal));
      }
    }
  }
  if crossed.is_some() {
    return crossed;
  }
  // The nearest point, and the outer normal there: an edge's, or at a
  // corner its edges' together
  let nearest = |a: Vec2, b: Vec2| {
    let along = b - a;
    let u = ((to - a).dot(along) / along.length_squared()).clamp(0.0, 1.0);
    a + along * u
  };
  let mut best: Option<(f32, Vec2)> = None;
  for &(a, b) in edges {
    if outward(a, b).is_none() {
      continue;
    }
    let point = nearest(a, b);
    let distance = to.distance(point);
    if best.is_none_or(|(d, _)| distance < d) {
      best = Some((distance, point));
    }
  }
  let (distance, point) = best?;
  if distance.is_nan() || distance >= reach.max(radius) {
    return None;
  }
  let mut normal = Vec2::ZERO;
  for &(a, b) in edges {
    if let Some(n) = outward(a, b)
      && nearest(a, b).distance(point) <= 1e-3
    {
      normal += n;
    }
  }
  let normal = normal.normalize_or_zero();
  if normal == Vec2::ZERO {
    return None;
  }
  if normal.dot(to - point) < 0.0 && distance < reach {
    // In the ground: out through its nearest edge
    return Some((0.0, point, normal));
  }
  if distance >= radius {
    return None;
  }
  let facing = if distance > 0.0 {
    (to - point) / distance
  } else {
    normal
  };
  Some((distance / radius, point, facing))
}

/// Colliders a particle can hit, in gather order.
///
/// Skips the collider on the particle's own emitter entity, which particles spawn inside.
pub(crate) fn candidate_colliders<'a>(
  colliders: &'a [ParticleCollider],
  emitter: Entity,
  config: &'a CollisionConfig,
) -> impl Iterator<Item = &'a ParticleCollider> {
  colliders.iter().filter(move |collider| {
    collider.entity != emitter && collider.layer & config.collision_mask != 0
  })
}

/// The query `gather_colliders` runs; keep it between runs.
pub(crate) type ColliderQuery = QueryState<(
  Entity,
  &'static GlobalTransform,
  &'static Collider,
  Option<&'static ChildOf>,
  Has<TilePart>,
)>;

/// Gathers the world's solid colliders into `colliders`, their edges into `edges`.
pub(crate) fn gather_colliders(
  world: &World,
  query: &mut ColliderQuery,
  colliders: &mut Vec<ParticleCollider>,
  edges: &mut Vec<(Vec2, Vec2)>,
) {
  colliders.extend(
    query
      .iter(world)
      .filter(|(_, _, collider, ..)| !collider.is_trigger)
      .filter_map(|(entity, global, collider, parent, tile_part)| {
        // A tile part's grid, the one it was built from
        let grid = parent
          .filter(|_| tile_part)
          .and_then(|parent| world.get::<TileColliders>(parent.parent()));
        // A tile map storey's edge, which keeps characters on their
        // storey, is no surface
        if grid.is_some_and(|grid| !grid.surface) {
          return None;
        }
        // The part's cell, in world pixels
        let cell = grid
          .map(|grid| {
            let scale = global.scale().abs().min_element();
            grid.tile_size.min_element() * scale
          })
          .filter(|cell| cell.is_finite() && *cell > 0.0);
        Some(ParticleCollider::from_component(
          entity,
          global,
          (collider, cell),
          edges,
        ))
      }),
  );
}
