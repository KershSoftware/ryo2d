//! [`AudioClip`]: a sound's samples, decoded to memory or kept encoded to
//! stream

use std::sync::Arc;
use std::time::Duration;

use kira::Frame;
use kira::sound::static_sound::{StaticSoundData, StaticSoundSettings};

use crate::core::asset::Asset;

/// Clips up to this long decode to memory; longer ones stream (native
/// builds: the web decodes every clip)
pub(crate) const DECODE_THRESHOLD: Duration = Duration::from_secs(10);

/// What a clip holds
pub(crate) enum ClipData {
  /// All its frames, ready to play; cheap to clone
  Decoded(Box<StaticSoundData>),
  /// The file's bytes, decoded as they play: a stream is made for each play
  Encoded(EncodedClip),
}

/// A streamed clip's file, and its length in frames as its header gives it
/// (at the clip's sample rate)
#[derive(Clone)]
// The web never streams, so it never reads one
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) struct EncodedClip {
  pub(crate) bytes: Arc<[u8]>,
  pub(crate) frames: usize,
}

/// Why [`AudioClip::from_frames`] cannot make a clip
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum AudioClipError {
  /// Its sample rate is 0, so it would never advance
  #[error("the clip's sample rate is 0: it is how many frames play a second (48000, say)")]
  ZeroSampleRate,
}

/// A sound: WAV, OGG Vorbis, MP3 or FLAC, mono or stereo, loaded through the
/// asset registry (`ctx.assets().load::<AudioClip>("sfx/jump.ogg")`).
///
/// A short clip is decoded to memory, a long one (over 10 s) keeps its encoded
/// bytes and streams when played, so music does not sit decoded in memory;
/// [`AudioClipSettings`](crate::core::asset::AudioClipSettings) can force
/// either. The web has no streaming: every clip decodes there.
///
/// Saved while it plays (hot reload), its loops and the music playing it
/// switch to the new file where they are, crossfading over 10 ms, and its
/// one-shots finish on the old one; a save that does not decode changes
/// nothing. Changed in code (`Assets::get_mut`), it switches them the same
/// way, as a save does.
pub struct AudioClip {
  data: ClipData,
  duration: Duration,
  sample_rate: u32,
}

impl Asset for AudioClip {}

impl AudioClip {
  pub(crate) fn new(data: ClipData, duration: Duration, sample_rate: u32) -> Self {
    Self {
      data,
      duration,
      sample_rate,
    }
  }

  /// How long the clip plays at its normal rate
  pub fn duration(&self) -> Duration {
    self.duration
  }

  /// The file's sample rate in Hz
  pub fn sample_rate(&self) -> u32 {
    self.sample_rate
  }

  /// Whether the clip keeps its encoded bytes and streams, rather than
  /// holding its decoded frames
  pub fn is_streamed(&self) -> bool {
    matches!(self.data, ClipData::Encoded(_))
  }

  /// A clip made in code from stereo frames (`[left, right]`; give both the
  /// same value for mono) at `sample_rate` frames a second. A rate of 0 is
  /// an error: such a clip would never play on to its end
  pub fn from_frames(sample_rate: u32, frames: &[[f32; 2]]) -> Result<AudioClip, AudioClipError> {
    if sample_rate == 0 {
      return Err(AudioClipError::ZeroSampleRate);
    }
    let frames: Arc<[Frame]> = frames.iter().map(|&[l, r]| Frame::new(l, r)).collect();
    let duration = Duration::from_secs_f64(frames.len() as f64 / f64::from(sample_rate));
    let data = StaticSoundData {
      sample_rate,
      frames,
      settings: StaticSoundSettings::default(),
      slice: None,
    };
    Ok(AudioClip::new(
      ClipData::Decoded(Box::new(data)),
      duration,
      sample_rate,
    ))
  }

  pub(crate) fn data(&self) -> &ClipData {
    &self.data
  }
}

impl Clone for AudioClip {
  fn clone(&self) -> Self {
    let data = match &self.data {
      ClipData::Decoded(data) => ClipData::Decoded(data.clone()),
      ClipData::Encoded(encoded) => ClipData::Encoded(encoded.clone()),
    };
    Self::new(data, self.duration, self.sample_rate)
  }
}
