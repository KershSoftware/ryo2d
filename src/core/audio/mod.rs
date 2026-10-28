//! Audio: the engine's player ([`AudioEngine`], reached through
//! `ctx.audio()`, playing [`Voice`]s on the buses of its [`Mixer`], at the
//! player's volume settings), the registry assets it plays ([`Sound`]
//! definitions, and the [`AudioClip`]s they are made of), and the old
//! player (`legacy`, until the games move over)

mod clip;
mod engine;
mod legacy;
mod log;
mod mix;
mod mixer;
mod settings;
mod sound;
mod system;
mod view;
mod voice;

pub use clip::AudioClip;
pub(crate) use clip::{ClipData, DECODE_THRESHOLD};
pub use engine::AudioEngine;
pub use legacy::*;
#[doc(hidden)]
pub use log::{AudioLogEntry, AudioLogKind};
#[doc(hidden)]
pub use mixer::BusReport;
pub use mixer::{
  BusDef, DistortionKind, DuckDef, EffectDef, EqKind, FocusLoss, MIXER_PATH, Mixer, SnapshotBus,
  SnapshotDef,
};
pub(crate) use mixer::{check_bus_name, default_mixer};
pub use settings::{VolumeKind, volume_to_db};
pub use sound::{Falloff, LimitRule, Sound, SoundClip, Spatial, VoiceLimit};
#[doc(hidden)]
pub use system::audio_system;
pub use view::{AudioView, PlayOptions, Playable};
pub use voice::Voice;
