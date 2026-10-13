//! The 2D batch: what the world draws, keyed for merging, planned per camera.

mod gpu;
mod key;
mod pipelines;
mod plan;

pub(crate) use gpu::Batch2D;
pub(crate) use key::{BatchShader, DrawState, Sampling, Stream};
pub use key::{BlendMode, Filter};
pub(crate) use pipelines::texture_layout;
pub(crate) use plan::{Draw, Placed, Planned, plan_draws};
