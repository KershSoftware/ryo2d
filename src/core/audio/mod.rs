//! Audio: the engine's player ([`AudioEngine`], reached through
//! `ctx.audio()`, playing [`Voice`]s), the registry assets it plays
//! ([`Sound`] definitions, and the [`AudioClip`]s they are made of), and the
//! old player (`legacy`, until the mixer replaces it)

mod clip;
mod engine;
mod legacy;
mod log;
mod sound;
mod system;
mod view;
mod voice;

pub use clip::AudioClip;
pub(crate) use clip::{ClipData, DECODE_THRESHOLD};
pub use engine::AudioEngine;
pub(crate) use engine::STANDARD_BUSES;
pub use legacy::*;
#[doc(hidden)]
pub use log::{AudioLogEntry, AudioLogKind};
pub use sound::{Falloff, LimitRule, Sound, SoundClip, Spatial, VoiceLimit};
#[doc(hidden)]
pub use system::audio_system;
pub use view::{AudioView, PlayOptions, Playable};
pub use voice::Voice;
