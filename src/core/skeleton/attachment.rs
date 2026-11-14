//! What a slot shows: a region of an image, a mesh of one, a point, or a
//! bounding box.

use super::data::BoneId;
use crate::core::ecs::Vec2;
use crate::core::ui::Color;

/// Something a skin puts in a slot. Its offsets are in its bone's space
#[derive(Debug, Clone, PartialEq)]
pub enum Attachment {
  Region(RegionAttachment),
  Mesh(MeshAttachment),
  Point(PointAttachment),
  BoundingBox(BoundingBoxAttachment),
}

/// A textured quad from one of the skeleton's image regions, centred on its
/// offset from the bone
#[derive(Debug, Clone, PartialEq)]
pub struct RegionAttachment {
  /// An index into [`SkeletonData::regions`](super::SkeletonData::regions)
  pub region: usize,
  pub x: f32,
  pub y: f32,
  /// Radians
  pub rotation: f32,
  pub scale_x: f32,
  pub scale_y: f32,
  /// The quad's size before scale, in the bone's units
  pub width: f32,
  pub height: f32,
  /// Multiplied with the slot's colour
  pub color: Color,
}

impl RegionAttachment {
  /// Region `region` drawn `width` by `height`, centred on the bone, with
  /// no rotation, unit scale and white colour
  pub fn new(region: usize, width: f32, height: f32) -> Self {
    Self {
      region,
      x: 0.0,
      y: 0.0,
      rotation: 0.0,
      scale_x: 1.0,
      scale_y: 1.0,
      width,
      height,
      color: Color::WHITE,
    }
  }
}

/// A position and direction on a bone, for spawning things (a muzzle, a
/// hand)
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PointAttachment {
  pub x: f32,
  pub y: f32,
  /// Radians
  pub rotation: f32,
}

/// A polygon on a bone that the game can hit-test
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BoundingBoxAttachment {
  /// At least three, in the bone's space
  pub vertices: Vec<Vec2>,
}

/// A triangle mesh textured from one of the skeleton's image regions, its
/// vertices placed in one bone's space or weighted over several bones.
///
/// It is drawn in its slot's place in the draw order, as a region of the
/// same image would be: tinted, blended, lit, shadowed, flipped and
/// shaded with its skeleton's renderable, its vertices following their
/// bones between fixed steps. Each mesh shown is a draw of its own, so a
/// character is better made of a few meshes than of many. See
/// [`Renderable::skeleton`](crate::core::ecs::Renderable::skeleton) for
/// what does not reach a mesh (an effect that draws round its sprite, the
/// pixel-art snap)
#[derive(Debug, Clone, PartialEq)]
pub struct MeshAttachment {
  /// An index into [`SkeletonData::regions`](super::SkeletonData::regions)
  pub region: usize,
  /// One per vertex: the place in the region, 0 to 1 across it
  pub uvs: Vec<Vec2>,
  /// Vertex indices, three to a triangle
  pub triangles: Vec<u16>,
  /// How many of the first vertices form the outline, in order, for the
  /// debug overlay; 0 for no outline, else 3 or more
  pub hull: u16,
  pub vertices: MeshVertices,
  /// Multiplied with the slot's colour
  pub color: Color,
}

impl MeshAttachment {
  /// A mesh of region `region` with these vertices, UVs and triangles, no
  /// hull and white colour
  pub fn new(region: usize, vertices: MeshVertices, uvs: Vec<Vec2>, triangles: Vec<u16>) -> Self {
    Self {
      region,
      uvs,
      triangles,
      hull: 0,
      vertices,
      color: Color::WHITE,
    }
  }

  /// How many vertices the mesh has
  pub fn vertex_count(&self) -> usize {
    self.vertices.vertex_count()
  }

  /// How many positions the mesh holds, which a deform key must match: a
  /// vertex each when unweighted, an influence each when weighted
  pub fn positions(&self) -> usize {
    self.vertices.positions()
  }
}

/// Where a mesh's vertices are
#[derive(Debug, Clone, PartialEq)]
pub enum MeshVertices {
  /// Positions in the slot's bone space
  Unweighted(Vec<Vec2>),
  /// Each vertex a weighted sum over bones
  Weighted(WeightedVertices),
}

impl MeshVertices {
  /// How many vertices there are
  pub fn vertex_count(&self) -> usize {
    match self {
      Self::Unweighted(positions) => positions.len(),
      Self::Weighted(weighted) => weighted.vertex_count(),
    }
  }

  /// How many positions there are: one a vertex, or one an influence
  pub fn positions(&self) -> usize {
    match self {
      Self::Unweighted(positions) => positions.len(),
      Self::Weighted(weighted) => weighted.positions(),
    }
  }
}

/// Vertices each placed by the weighted sum of several bones' influences,
/// held in one flat list with each vertex's range in it
#[derive(Debug, Clone, PartialEq)]
pub struct WeightedVertices {
  influences: Vec<Influence>,
  /// Vertex `i` owns `influences[starts[i]..starts[i + 1]]`; one longer than
  /// the vertex count
  starts: Vec<u32>,
}

impl WeightedVertices {
  /// The vertices with these influences each. Each vertex's weights are
  /// normalised to sum to 1, unless they do not sum to a positive finite
  /// number, which validation then refuses
  pub fn new(vertices: impl IntoIterator<Item = Vec<Influence>>) -> Self {
    let mut influences = Vec::new();
    let mut starts = vec![0];
    for mut vertex in vertices {
      let sum: f32 = vertex.iter().map(|influence| influence.weight).sum();
      if sum.is_finite() && sum > 0.0 {
        for influence in &mut vertex {
          influence.weight /= sum;
        }
      }
      influences.extend(vertex);
      starts.push(u32::try_from(influences.len()).unwrap_or(u32::MAX));
    }
    Self { influences, starts }
  }

  /// How many vertices there are
  pub fn vertex_count(&self) -> usize {
    self.starts.len() - 1
  }

  /// The influences of vertex `vertex`; none past the last vertex
  pub fn influences(&self, vertex: usize) -> &[Influence] {
    match (self.starts.get(vertex), self.starts.get(vertex + 1)) {
      (Some(&start), Some(&end)) => &self.influences[start as usize..end as usize],
      _ => &[],
    }
  }

  /// How many influence positions there are in all
  pub fn positions(&self) -> usize {
    self.influences.len()
  }

  /// Each vertex's influences, vertex by vertex: the flat list in order,
  /// cut where each vertex's begin
  pub(super) fn each(&self) -> impl Iterator<Item = &[Influence]> {
    self.starts.windows(2).map(|ends| {
      let range = ends[0] as usize..ends[1] as usize;
      self.influences.get(range).unwrap_or(&[])
    })
  }
}

/// One bone's pull on a vertex: where the vertex sits in the bone's space
/// and how much that bone counts
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Influence {
  pub bone: BoneId,
  pub x: f32,
  pub y: f32,
  pub weight: f32,
}

impl Influence {
  pub fn new(bone: BoneId, x: f32, y: f32, weight: f32) -> Self {
    Self { bone, x, y, weight }
  }
}
