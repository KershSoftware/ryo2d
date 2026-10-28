//! Audio: the engine's player ([`AudioEngine`], reached through
//! `ctx.audio()`, playing [`Voice`]s on the buses of its [`Mixer`], at the
//! player's volume settings, heard from places in the world by an
//! [`AudioListener`], and [`Music`] on its one music player), the registry
//! assets it plays ([`Sound`] definitions, [`Music`] tracks, and the
//! [`AudioClip`]s they are made of), and the old player (`legacy`, until the
//! games move over)

mod clip;
mod engine;
mod legacy;
mod log;
mod mix;
mod mixer;
mod music;
mod settings;
mod sound;
mod spatial;
mod system;
mod view;
mod voice;

pub use clip::AudioClip;
pub(crate) use clip::{ClipData, DECODE_THRESHOLD};
pub use engine::AudioEngine;
pub(crate) use engine::{in_scene, release_scene, stop_scene_music};
pub use legacy::*;
#[doc(hidden)]
pub use log::{AudioLogEntry, AudioLogKind};
pub(crate) use mix::OPEN_HZ;
#[doc(hidden)]
pub use mixer::BusReport;
pub use mixer::{
  BusDef, DistortionKind, DuckDef, EffectDef, EqKind, FocusLoss, MIXER_PATH, Mixer, SnapshotBus,
  SnapshotDef,
};
pub(crate) use mixer::{check_bus_name, default_mixer};
pub use music::Music;
pub(crate) use music::{JoinError, MusicData, join};
pub use settings::{VolumeKind, volume_to_db};
pub use sound::{Falloff, LimitRule, Sound, SoundClip, Spatial, VoiceLimit};
pub use spatial::{AudioEmitter, AudioListener, falloff_gain_db, pan_for};
#[doc(hidden)]
pub use system::audio_system;
pub use view::{AudioView, PlayOptions, Playable};
pub use voice::Voice;
