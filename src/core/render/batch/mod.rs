//! The 2D batch: what the world draws, keyed for merging, planned per camera.

mod gpu;
mod key;
mod lines;
mod pipelines;
mod plan;
mod polyline;

pub(crate) use gpu::Batch2D;
pub(crate) use key::{BatchShader, DrawState, LightStep, PageKind, Sampling, Stream};
pub use key::{BlendMode, Filter};
pub(crate) use lines::{LineLook, line_color, line_points, push_polyline, trail_points};
pub(crate) use pipelines::texture_layout;
pub(crate) use plan::{Draw, Placed, plan_draws};
pub(crate) use polyline::{PolyPoint, PolyStyle, Triangles, Vertex2D};
