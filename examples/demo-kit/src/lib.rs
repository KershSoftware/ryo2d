//! Shared shell for the Ryo2D feature demos.
//!
//! Every demo gets the same controls (`[ ]` pages, arrows for settings, `R`
//! reset, `H` help, `Esc` quit), the same layout and a settings panel. See
//! `examples/demos/AGENTS.md`.

mod controls;
mod hold;
mod settings;
mod start;

pub use controls::Control;
pub use settings::Settings;
