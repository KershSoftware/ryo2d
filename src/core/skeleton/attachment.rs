//! What a slot shows: a region of an image, a mesh of one, a point, or a
//! bounding box.

use super::data::{BoneId, Region};
use crate::core::ecs::{Affine2, Vec2};
use crate::core::ui::Color;

/// Something a skin puts in a slot. Its offsets are in its bone's space
#[derive(Debug, Clone, PartialEq)]
pub enum Attachment {
  Region(RegionAttachment),
  Mesh(MeshAttachment),
  Point(PointAttachment),
  BoundingBox(BoundingBoxAttachment),
}

/// A textured quad from one of the skeleton's image regions, placed on its
/// bone by its pivot (its centre unless one is given).
///
/// `x`, `y` and `rotation` place the pivot: the pivot's pixel lands at
/// `(x, y)` in the bone's space, and the image turns about it. With
/// `along`, the image is first turned so the line from its pivot to that
/// pixel lies along the bone's x axis, so a limb is placed by naming the
/// pixel of its joint and the pixel of the next joint. Scale and mirroring
/// act about the pivot, and the line is laid along the bone after them, so
/// a mirrored limb still lies along its bone
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
  /// The pixel of the region that `x`, `y` and `rotation` place, from its
  /// top-left (x right, y down, as the image is drawn); `None` for its
  /// centre. A region drawn larger than its pixels keeps its pivot on the
  /// same pixel: pixels become the bone's units by `width` over the
  /// region's pixel width, and `height` over its pixel height
  pub pivot: Option<Vec2>,
  /// A second pixel of the region, in the same pixels as `pivot`: the image
  /// is turned so the line from the pivot to it lies along the bone's x
  /// axis, and `rotation` is added to that. `None` for no such turn
  pub along: Option<Vec2>,
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
      pivot: None,
      along: None,
      color: Color::WHITE,
    }
  }

  /// Its quad in its bone's space, drawn from `region` (whose pixel size
  /// turns `pivot` and `along` into the bone's units): the unit quad (-0.5
  /// to 0.5 on each axis, its top-left the image's) scaled to its size,
  /// turned, and moved so its pivot is at its offset
  pub(crate) fn quad(&self, region: &Region) -> Affine2 {
    let (mut sin, mut cos) = self.rotation.sin_cos();
    let scale = Vec2::new(self.scale_x, self.scale_y);
    let size = scale * Vec2::new(self.width, self.height);
    let mut at = Vec2::new(self.x, self.y);
    // Placed by its centre, exactly as before pivots, unless it has one;
    // validation refuses an empty region, so its pixels have a size
    if (self.pivot.is_some() || self.along.is_some()) && region.width > 0 && region.height > 0 {
      let pixels = Vec2::new(region.width as f32, region.height as f32);
      let per_pixel = Vec2::new(self.width, self.height) / pixels;
      let centre = pixels / 2.0;
      let pivot = self.pivot.unwrap_or(centre);
      if let Some(along) = self.along {
        // Turned back by the line's own direction after the scale, then by
        // `rotation`: the line lies along the bone's x axis, turned by it
        let line = (along - pivot) * per_pixel * scale;
        let length = line.length();
        if length > 0.0 && length.is_finite() {
          let back = line / length;
          (sin, cos) = (sin * back.x - cos * back.y, cos * back.x + sin * back.y);
        }
      }
      // The centre is the pivot's offset less where the scale and turn put
      // the pivot from the centre
      let from_centre = (pivot - centre) * per_pixel * scale;
      at -= Vec2::new(
        from_centre.x * cos - from_centre.y * sin,
        from_centre.x * sin + from_centre.y * cos,
      );
    }
    Affine2::from_cols(
      Vec2::new(cos, sin) * size.x,
      Vec2::new(-sin, cos) * size.y,
      at,
    )
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

  /// How many positions the mesh holds: a vertex each when unweighted, an
  /// influence each when weighted. A key of a deform timeline
  /// ([`Timeline::Deform`](super::Timeline::Deform)) holds an x and a y
  /// for each, in their order, and moves the mesh only if it does
  pub fn position_count(&self) -> usize {
    self.vertices.position_count()
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
  pub fn position_count(&self) -> usize {
    match self {
      Self::Unweighted(positions) => positions.len(),
      Self::Weighted(weighted) => weighted.position_count(),
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
  pub fn position_count(&self) -> usize {
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
