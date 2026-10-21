//! The UI's own renderer: every UI draw item is one kind of GPU instance,
//! drawn in painter's order through one pipeline that clips in the shader
#![cfg_attr(not(test), allow(dead_code, unused_imports))] // wired in Task 4

mod gpu;
mod instance;
mod runs;

#[cfg(test)]
mod gpu_tests;
#[cfg(test)]
mod tests;

pub(crate) use gpu::{KEEP_FRAMES, UiGpu};
pub(crate) use instance::{ClipRect, UiInstance, UiKind};
pub(crate) use runs::{Run, Slot, TexKey, TexNeed, plan_runs};
