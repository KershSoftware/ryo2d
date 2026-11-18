//! The 2D batch: what the world draws, keyed for merging, planned per camera.

mod chunks;
#[cfg(test)]
mod coverage;
mod gpu;
mod key;
#[cfg(test)]
mod line_cover_tests;
#[cfg(test)]
mod line_gpu_tests;
mod lines;
mod mesh;
mod pipelines;
mod plan;
#[cfg(test)]
mod polyline;

pub(crate) use chunks::{
  Band, ChunkCopy, ChunkKey, ChunkPart, ChunkRun, ChunkStamp, ChunkStore, KEEP_FRAMES, Planted,
  Spill, TileSlot,
};
pub(crate) use gpu::{Batch2D, Bound, unit_quad};
pub(crate) use key::{BatchShader, DrawState, LightStep, PageKind, Sampling, Stream};
pub use key::{BlendMode, Filter};
pub(crate) use lines::{
  LineLook, LineStream, MITER_LIMIT, Piece, PieceKind, PolyPoint, PolyStyle, along_half, edge_code,
  filter_points, line_points, piece_list, piece_triangles, push_polyline, trail_points,
};
pub(crate) use mesh::MeshStream;
pub(crate) use pipelines::{QUAD_LAYOUT, shadow_layout, texture_layout};
pub(crate) use plan::{Draw, DrawSource, Placed, plan_draws};
