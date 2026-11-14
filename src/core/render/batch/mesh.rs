//! Meshes: an entity's own triangles of an image, drawn as a sprite of the
//! image is. Each vertex is a world position and a UV; one sprite instance
//! a mesh carries everything else, and the sprite shader's own fragment
//! stages draw it (`vs_mesh`, `shaders/core/mesh.wgsl`).

use std::ops::Range;

use glam::Vec2;

/// One vertex of the mesh stream, laid out as a corner of the batch's unit
/// quad is ([`QUAD_LAYOUT`](super::QUAD_LAYOUT)): where a quad's vertex is a
/// corner its instance places, a mesh's is the place itself.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct MeshVertex {
  /// World position
  pub(crate) position: [f32; 2],
  /// Across the rectangle of the image its mesh's instance names (its
  /// `uv_rect`): 0 on one edge, 1 on the other
  pub(crate) uv: [f32; 2],
}

/// Where a mesh went in the mesh stream.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MeshRange {
  pub(crate) vertices: Range<u32>,
  pub(crate) indices: Range<u32>,
  /// The world box its vertices stay inside
  pub(crate) min: Vec2,
  pub(crate) max: Vec2,
}

/// The frame's meshes: their vertices and the indices of their triangles,
/// built on the CPU while the frame is gathered and uploaded once.
#[derive(Default)]
pub(crate) struct MeshStream {
  pub(crate) vertices: Vec<MeshVertex>,
  /// Three a triangle, each a vertex of the stream
  pub(crate) indices: Vec<u32>,
}

impl MeshStream {
  /// Forget every mesh, keeping the allocations.
  pub(crate) fn clear(&mut self) {
    self.vertices.clear();
    self.indices.clear();
  }

  /// Add a mesh: each of `vertices` is a world position and a UV across
  /// its rectangle of the image, and `indices` are three a triangle,
  /// counted from the mesh's own first vertex. Where it went, or `None`,
  /// with the stream as it was, for a mesh that draws nothing: one with no
  /// triangle, indices that are not whole triangles or reach past its
  /// vertices, or a position that is not finite.
  pub(crate) fn add(
    &mut self,
    vertices: impl IntoIterator<Item = (Vec2, Vec2)>,
    indices: impl IntoIterator<Item = u32>,
  ) -> Option<MeshRange> {
    let (first_vertex, first_index) = (self.vertices.len(), self.indices.len());
    let added = self.add_whole(vertices, indices);
    if added.is_none() {
      self.vertices.truncate(first_vertex);
      self.indices.truncate(first_index);
    }
    added
  }

  /// Forget `mesh`, which must be the last one added: a mesh a drawer
  /// added and then did not push (no camera sees it).
  pub(crate) fn drop_last(&mut self, mesh: &MeshRange) {
    debug_assert_eq!(
      (mesh.vertices.end as usize, mesh.indices.end as usize),
      (self.vertices.len(), self.indices.len()),
      "only the last mesh added can be taken back"
    );
    self.vertices.truncate(mesh.vertices.start as usize);
    self.indices.truncate(mesh.indices.start as usize);
  }

  /// [`add`](Self::add), leaving what it added of a mesh that draws
  /// nothing
  fn add_whole(
    &mut self,
    vertices: impl IntoIterator<Item = (Vec2, Vec2)>,
    indices: impl IntoIterator<Item = u32>,
  ) -> Option<MeshRange> {
    let first_vertex = u32::try_from(self.vertices.len()).ok()?;
    let first_index = u32::try_from(self.indices.len()).ok()?;
    let (mut min, mut max) = (Vec2::INFINITY, Vec2::NEG_INFINITY);
    // Asked of every position: `min` and `max` skip a NaN
    let mut finite = true;
    for (position, uv) in vertices {
      finite &= position.is_finite();
      min = min.min(position);
      max = max.max(position);
      self.vertices.push(MeshVertex {
        position: position.into(),
        uv: uv.into(),
      });
    }
    let last_vertex = u32::try_from(self.vertices.len()).ok()?;
    if !finite {
      return None;
    }
    let count = last_vertex - first_vertex;
    for index in indices {
      if index >= count {
        return None;
      }
      self.indices.push(first_vertex + index);
    }
    let last_index = u32::try_from(self.indices.len()).ok()?;
    let added = last_index - first_index;
    if added == 0 || !added.is_multiple_of(3) {
      return None;
    }
    Some(MeshRange {
      vertices: first_vertex..last_vertex,
      indices: first_index..last_index,
      min,
      max,
    })
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  /// A unit square's corners, each with its position as its UV
  fn square(at: Vec2) -> [(Vec2, Vec2); 4] {
    [Vec2::ZERO, Vec2::X, Vec2::Y, Vec2::ONE].map(|corner| (at + corner, corner))
  }

  #[test]
  fn a_mesh_joins_the_stream_after_what_it_holds() {
    let mut meshes = MeshStream::default();
    let first = meshes.add(square(Vec2::ZERO), [0, 1, 2, 1, 3, 2]);
    let second = meshes.add(square(Vec2::new(10.0, 20.0)), [0, 1, 3]);
    assert_eq!(
      first,
      Some(MeshRange {
        vertices: 0..4,
        indices: 0..6,
        min: Vec2::ZERO,
        max: Vec2::ONE,
      })
    );
    // Its indices count from its own first vertex, and its box is its
    // vertices'
    assert_eq!(
      second,
      Some(MeshRange {
        vertices: 4..8,
        indices: 6..9,
        min: Vec2::new(10.0, 20.0),
        max: Vec2::new(11.0, 21.0),
      })
    );
    assert_eq!(meshes.indices, [0, 1, 2, 1, 3, 2, 4, 5, 7]);
    // Each vertex is its place and its UV, 16 bytes as a unit quad's is
    assert_eq!(
      meshes.vertices[7],
      MeshVertex {
        position: [11.0, 21.0],
        uv: [1.0, 1.0],
      }
    );
    assert_eq!(std::mem::size_of::<MeshVertex>(), 16);

    // The last mesh, added and then not pushed, is taken back
    if let Some(second) = &second {
      meshes.drop_last(second);
    }
    assert_eq!((meshes.vertices.len(), meshes.indices.len()), (4, 6));
    meshes.clear();
    assert!(meshes.vertices.is_empty() && meshes.indices.is_empty());
  }

  #[test]
  #[cfg(debug_assertions)]
  #[should_panic(expected = "only the last mesh added can be taken back")]
  fn only_the_last_mesh_can_be_taken_back() {
    let mut meshes = MeshStream::default();
    let first = meshes.add(square(Vec2::ZERO), [0, 1, 2]);
    meshes.add(square(Vec2::ONE), [0, 1, 2]);
    if let Some(first) = &first {
      meshes.drop_last(first);
    }
  }

  #[test]
  fn a_mesh_that_draws_nothing_leaves_the_stream_as_it_was() {
    let mut meshes = MeshStream::default();
    meshes.add(square(Vec2::ZERO), [0, 1, 2]);
    let held = (meshes.vertices.clone(), meshes.indices.clone());
    let nan = Vec2::new(f32::NAN, 0.0);
    let far = Vec2::new(0.0, f32::INFINITY);
    /// A mesh that draws nothing: what is wrong with it, its vertices and
    /// its indices
    type Case = (&'static str, [(Vec2, Vec2); 4], &'static [u32]);
    let cases: [Case; 6] = [
      ("no triangle", square(Vec2::ZERO), &[]),
      ("half a triangle", square(Vec2::ZERO), &[0, 1, 2, 3]),
      ("an index past its vertices", square(Vec2::ZERO), &[0, 1, 4]),
      ("a NaN position", square(nan), &[0, 1, 2]),
      ("a position at infinity", square(far), &[0, 1, 2]),
      // Its own last vertex is NaN, which a running minimum would skip
      (
        "a NaN last",
        [
          (Vec2::ZERO, Vec2::ZERO),
          (Vec2::X, Vec2::X),
          (Vec2::Y, Vec2::Y),
          (nan, Vec2::ONE),
        ],
        &[0, 1, 2],
      ),
    ];
    for (name, vertices, indices) in cases {
      let added = meshes.add(vertices, indices.iter().copied());
      assert_eq!(added, None, "{name}");
      assert_eq!(meshes.vertices, held.0, "{name}");
      assert_eq!(meshes.indices, held.1, "{name}");
    }
    // The next one starts where the last good one ended
    let next = meshes.add(square(Vec2::ZERO), [0, 1, 2]);
    assert_eq!(
      next.map(|range| (range.vertices, range.indices)),
      Some((4..8, 3..6))
    );
  }
}
