//! Audio: the engine's player (`legacy`, until the mixer replaces it) and
//! [`AudioClip`], the registry asset sounds are made of

mod clip;
mod legacy;

pub use clip::AudioClip;
pub(crate) use clip::{ClipData, DECODE_THRESHOLD};
pub use legacy::*;
