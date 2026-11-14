//! Meshes as triangles: a drawer's own textured triangles in the batch's
//! triangle stream, drawn with the sprite's look (`BatchShader::Mesh`,
//! `shaders/core/mesh.wgsl`).

#![cfg_attr(
  not(test),
  expect(
    dead_code,
    reason = "skeletons' mesh attachments draw meshes; until they do, only tests add one"
  )
)]

use std::ops::Range;

use glam::Vec2;

use super::polyline::{Triangles, Vertex2D};
use crate::core::graphics::SpriteInstance;

/// What every vertex of a mesh carries besides its own position and UV.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MeshLanes {
  /// The tint, as a sprite instance's is given: the sRGB colour with
  /// straight alpha
  pub(crate) tint: [f32; 4],
  /// The rectangle of the image the mesh may read (UV min and max, as
  /// fractions of the image): a UV outside it reads nothing
  pub(crate) region: [f32; 4],
  /// Multiplies the light, as a sprite's emission does
  pub(crate) emission: f32,
}

/// Where a mesh went in the triangle stream.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MeshRange {
  pub(crate) vertices: Range<u32>,
  pub(crate) indices: Range<u32>,
  /// The world box its vertices stay inside
  pub(crate) min: Vec2,
  pub(crate) max: Vec2,
}

impl Triangles {
  /// Add a mesh: each of `vertices` is a world position and a UV as a
  /// fraction of its image, and `indices` are three a triangle, counted
  /// from the mesh's own first vertex. Where it went, or `None`, with the
  /// stream as it was, for a mesh that draws nothing: one with no
  /// triangle, indices that are not whole triangles or reach past its
  /// vertices, or a position that is not finite.
  ///
  /// The lanes of a mesh's [`Vertex2D`]: `uv` a fraction of the image,
  /// `color` the tint, `params` the region, `across[0]` the emission less
  /// one (a sprite instance's lane) and `bounds` the image's atlas slot,
  /// set when the mesh is placed.
  pub(crate) fn mesh(
    &mut self,
    lanes: &MeshLanes,
    vertices: impl IntoIterator<Item = (Vec2, Vec2)>,
    indices: impl IntoIterator<Item = u32>,
  ) -> Option<MeshRange> {
    let (first_vertex, first_index) = (self.vertices.len(), self.indices.len());
    let added = self.add_mesh(lanes, vertices, indices);
    if added.is_none() {
      self.vertices.truncate(first_vertex);
      self.indices.truncate(first_index);
    }
    added
  }

  /// Forget `mesh`, the last thing added: a mesh a drawer added and then
  /// did not push (no camera sees it).
  pub(crate) fn drop_mesh(&mut self, mesh: &MeshRange) {
    self.vertices.truncate(mesh.vertices.start as usize);
    self.indices.truncate(mesh.indices.start as usize);
  }

  /// [`mesh`](Self::mesh), leaving what it added of a mesh that draws
  /// nothing
  fn add_mesh(
    &mut self,
    lanes: &MeshLanes,
    vertices: impl IntoIterator<Item = (Vec2, Vec2)>,
    indices: impl IntoIterator<Item = u32>,
  ) -> Option<MeshRange> {
    let first_vertex = u32::try_from(self.vertices.len()).ok()?;
    let first_index = u32::try_from(self.indices.len()).ok()?;
    // The lane a sprite's instance carries its emission in
    let emission = SpriteInstance::new()
      .with_emission(lanes.emission)
      .transform[0][3];
    let (mut min, mut max) = (Vec2::INFINITY, Vec2::NEG_INFINITY);
    // Asked of every position: `min` and `max` skip a NaN
    let mut finite = true;
    for (position, uv) in vertices {
      finite &= position.is_finite();
      min = min.min(position);
      max = max.max(position);
      self.vertices.push(Vertex2D {
        position: position.into(),
        uv: uv.into(),
        color: lanes.tint,
        params: lanes.region,
        across: [emission, 0.0],
        bounds: [0.0; 4],
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

  const LANES: MeshLanes = MeshLanes {
    tint: [0.5, 0.25, 1.0, 0.75],
    region: [0.25, 0.0, 0.5, 1.0],
    emission: 3.0,
  };

  /// A unit square's corners, each with its position as its UV
  fn square(at: Vec2) -> [(Vec2, Vec2); 4] {
    [Vec2::ZERO, Vec2::X, Vec2::Y, Vec2::ONE].map(|corner| (at + corner, corner))
  }

  #[test]
  fn a_mesh_joins_the_stream_after_what_it_holds() {
    let mut triangles = Triangles::default();
    let first = triangles.mesh(&LANES, square(Vec2::ZERO), [0, 1, 2, 1, 3, 2]);
    let second = triangles.mesh(&LANES, square(Vec2::new(10.0, 20.0)), [0, 1, 3]);
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
    assert_eq!(triangles.indices, [0, 1, 2, 1, 3, 2, 4, 5, 7]);
    // Each vertex carries its place and UV, and the mesh's lanes: the
    // emission as a sprite's instance does, less one
    assert_eq!(
      triangles.vertices[7],
      Vertex2D {
        position: [11.0, 21.0],
        uv: [1.0, 1.0],
        color: LANES.tint,
        params: LANES.region,
        across: [2.0, 0.0],
        bounds: [0.0; 4],
      }
    );
    let sprite = SpriteInstance::new().with_emission(LANES.emission);
    assert_eq!(triangles.vertices[7].across[0], sprite.transform[0][3]);

    // A mesh added and then not pushed is taken back
    if let Some(second) = &second {
      triangles.drop_mesh(second);
    }
    assert_eq!((triangles.vertices.len(), triangles.indices.len()), (4, 6));
  }

  #[test]
  fn a_mesh_that_draws_nothing_leaves_the_stream_as_it_was() {
    let mut triangles = Triangles::default();
    triangles.mesh(&LANES, square(Vec2::ZERO), [0, 1, 2]);
    let held = (triangles.vertices.clone(), triangles.indices.clone());
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
      let added = triangles.mesh(&LANES, vertices, indices.iter().copied());
      assert_eq!(added, None, "{name}");
      assert_eq!(triangles.vertices, held.0, "{name}");
      assert_eq!(triangles.indices, held.1, "{name}");
    }
    // The next one starts where the last good one ended
    let next = triangles.mesh(&LANES, square(Vec2::ZERO), [0, 1, 2]);
    assert_eq!(
      next.map(|range| (range.vertices, range.indices)),
      Some((4..8, 3..6))
    );
  }
}
