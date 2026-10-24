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
  Encoded(Arc<[u8]>),
}

/// A sound: WAV, OGG Vorbis, MP3 or FLAC, mono or stereo, loaded through the
/// asset registry (`ctx.assets().load::<AudioClip>("sfx/jump.ogg")`).
///
/// A short clip is decoded to memory, a long one (over 10 s) keeps its encoded
/// bytes and streams when played, so music does not sit decoded in memory;
/// [`AudioClipSettings`](crate::core::asset::AudioClipSettings) can force
/// either. The web has no streaming: every clip decodes there.
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
  /// same value for mono)
  pub fn from_frames(sample_rate: u32, frames: &[[f32; 2]]) -> AudioClip {
    let frames: Arc<[Frame]> = frames.iter().map(|&[l, r]| Frame::new(l, r)).collect();
    let duration = Duration::from_secs_f64(frames.len() as f64 / f64::from(sample_rate.max(1)));
    let data = StaticSoundData {
      sample_rate,
      frames,
      settings: StaticSoundSettings::default(),
      slice: None,
    };
    AudioClip::new(ClipData::Decoded(Box::new(data)), duration, sample_rate)
  }

  // Read by the player
  #[allow(dead_code)]
  pub(crate) fn data(&self) -> &ClipData {
    &self.data
  }
}

impl Clone for AudioClip {
  fn clone(&self) -> Self {
    let data = match &self.data {
      ClipData::Decoded(data) => ClipData::Decoded(data.clone()),
      ClipData::Encoded(bytes) => ClipData::Encoded(Arc::clone(bytes)),
    };
    Self::new(data, self.duration, self.sample_rate)
  }
}
