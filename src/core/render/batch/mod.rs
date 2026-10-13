//! The 2D batch: what the world draws, keyed for merging, planned per camera.

mod key;
mod plan;

pub(crate) use key::{BatchShader, DrawState, Stream};
pub use key::{BlendMode, Filter};
pub(crate) use plan::{Draw, Placed, Planned, plan_draws};
