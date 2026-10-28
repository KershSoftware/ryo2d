//! Audio clips from files: WAV, OGG Vorbis, MP3 and FLAC, mono or stereo,
//! decoded to memory or kept encoded to stream

use std::io::Cursor;
use std::time::Duration;

use kira::sound::static_sound::StaticSoundData;
use serde::Deserialize;

use crate::core::asset::{AssetError, AssetLoader, AssetPath, LoadContext};
use crate::core::audio::{AudioClip, ClipData, DECODE_THRESHOLD};
#[cfg(not(target_arch = "wasm32"))]
use crate::core::audio::{ClipBytes, EncodedClip, StreamInfo, open_probed};

/// Whether a clip is decoded to memory or kept encoded for streaming (a
/// sound definition's `mode: Stream` names it)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Deserialize)]
pub enum ClipMode {
  /// Decode a clip of up to 10 s, stream a longer one
  #[default]
  Auto,
  /// Always decode: every play is ready at once, the memory is the whole clip
  Decode,
  /// Keep the file's bytes and stream: small in memory, decoded as it plays.
  /// The web decodes anyway
  Stream,
}

/// How an audio file is loaded, given to `load_with` and `load_now_with`
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct AudioClipSettings {
  /// Decode or stream
  pub mode: ClipMode,
}

/// Loads [`AudioClip`]s from `.wav`, `.ogg`, `.mp3` and `.flac` files. Only
/// mono and stereo files load; any other layout, or a corrupt file, is
/// `AssetError::Decode`. A clip that streams is only probed: its header
/// gives its sample rate, length, codec and channels, so a surround file is
/// refused whether it decodes or streams, and a corrupt packet past the
/// header is found as it plays (and skipped). Every registry has it.
#[derive(Debug, Clone, Copy)]
pub struct AudioClipLoader {
  // The web decodes every clip, whatever its length
  #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
  threshold: Duration,
}

impl Default for AudioClipLoader {
  fn default() -> Self {
    Self {
      threshold: DECODE_THRESHOLD,
    }
  }
}

impl AudioClipLoader {
  /// A loader that streams clips longer than `threshold`, for tests
  #[cfg(test)]
  pub(crate) fn with_threshold(threshold: Duration) -> Self {
    Self { threshold }
  }
}

/// Whether a clip of `duration` is decoded: `Decode` always, `Stream` never,
/// `Auto` up to `threshold`; on the web (`wasm`) there is no streaming, so all
/// decode
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) fn decodes(mode: ClipMode, duration: Duration, threshold: Duration, wasm: bool) -> bool {
  wasm
    || match mode {
      ClipMode::Decode => true,
      ClipMode::Stream => false,
      ClipMode::Auto => duration <= threshold,
    }
}

/// The file `bytes` decoded whole by kira, which reads its header itself
fn decode(bytes: Vec<u8>) -> Result<AudioClip, kira::sound::FromFileError> {
  #[cfg(test)]
  tests::count_header_read();
  StaticSoundData::from_cursor(Cursor::new(bytes)).map(decoded)
}

/// A clip of the frames `data` holds
fn decoded(data: StaticSoundData) -> AudioClip {
  let (duration, rate) = (data.duration(), data.sample_rate);
  AudioClip::new(ClipData::Decoded(Box::new(data)), duration, rate)
}

impl AssetLoader for AudioClipLoader {
  type Asset = AudioClip;
  type Settings = AudioClipSettings;

  fn extensions(&self) -> &[&'static str] {
    &["wav", "ogg", "mp3", "flac"]
  }

  fn load(
    &self,
    bytes: Vec<u8>,
    settings: &AudioClipSettings,
    cx: &mut LoadContext<'_>,
  ) -> Result<AudioClip, AssetError> {
    self.clip_from(bytes, settings, cx.path())
  }
}

impl AudioClipLoader {
  /// The clip in the file `bytes` read from `path`. Its header is read
  /// once: a clip that decodes anyway (`Decode`, and every clip on the web)
  /// by kira's decoder alone; any other by the probe, whose length decides,
  /// and a short one is then decoded from the probe's reader. A streamed
  /// clip keeps the file's bytes where they were read
  fn clip_from(
    &self,
    bytes: Vec<u8>,
    settings: &AudioClipSettings,
    path: &AssetPath,
  ) -> Result<AudioClip, AssetError> {
    if cfg!(target_arch = "wasm32") || settings.mode == ClipMode::Decode {
      return decode(bytes).map_err(|error| AssetError::decode(path, error));
    }
    self.probed(bytes, settings, path)
  }

  /// A clip `clip_from` probes: decoded from the probe's reader if short
  /// enough for its mode, kept encoded to stream otherwise
  #[cfg(not(target_arch = "wasm32"))]
  fn probed(
    &self,
    bytes: Vec<u8>,
    settings: &AudioClipSettings,
    path: &AssetPath,
  ) -> Result<AudioClip, AssetError> {
    let bytes = ClipBytes::new(bytes);
    #[cfg(test)]
    tests::count_header_read();
    let probed = open_probed(&bytes).map_err(|error| AssetError::decode(path, error))?;
    let StreamInfo {
      sample_rate,
      frames,
    } = probed.info;
    let duration = Duration::from_secs_f64(frames as f64 / f64::from(sample_rate.max(1)));
    if decodes(settings.mode, duration, self.threshold, false) {
      return probed
        .decode()
        .map(decoded)
        .map_err(|error| AssetError::decode(path, error));
    }
    let encoded = EncodedClip { bytes, frames };
    Ok(AudioClip::new(
      ClipData::Encoded(encoded),
      duration,
      sample_rate,
    ))
  }

  /// The web has no streaming: every clip decodes, read by kira alone
  #[cfg(target_arch = "wasm32")]
  fn probed(
    &self,
    bytes: Vec<u8>,
    _settings: &AudioClipSettings,
    path: &AssetPath,
  ) -> Result<AudioClip, AssetError> {
    decode(bytes).map_err(|error| AssetError::decode(path, error))
  }
}

#[cfg(test)]
mod tests {
  use std::sync::Arc;
  use std::time::Duration;

  use super::*;
  use crate::core::asset::{AssetServer, Assets, AssetsView, MemorySource};
  use crate::core::audio::{AudioClip, AudioClipError, ClipData};
  use crate::core::ecs::kernel::World;

  const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/audio");

  thread_local! {
    /// The times a clip file's header was read on this thread
    static HEADER_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
  }

  /// A clip file's header is being read (by the probe or a decoder)
  pub(super) fn count_header_read() {
    HEADER_READS.with(|reads| reads.set(reads.get() + 1));
  }

  fn header_reads() -> usize {
    HEADER_READS.with(std::cell::Cell::get)
  }

  /// A clip's file is read once: a short clip under `Auto` is decoded from
  /// the probe that measured it, a forced one by the decoder alone and a
  /// streamed one by the probe alone
  #[test]
  fn a_clip_s_header_is_read_once() {
    let names = ["tone.wav", "tone.ogg", "tone.mp3", "tone.flac"];
    let mut world = clips_over(&names, AudioClipLoader::default());
    for name in names {
      for mode in [ClipMode::Auto, ClipMode::Decode, ClipMode::Stream] {
        let before = header_reads();
        let clip = loaded(&mut world, name, mode);
        assert_eq!(header_reads() - before, 1, "{name}, {mode:?}");
        assert_eq!(
          clip.is_streamed(),
          mode == ClipMode::Stream,
          "{name}, {mode:?}"
        );
      }
    }
  }

  /// A decoded `Auto` clip has the frames kira's own decoding gives
  #[test]
  fn an_auto_clip_decodes_as_a_forced_one_does() {
    let names = [
      "tone.wav",
      "tone.ogg",
      "tone.mp3",
      "tone.flac",
      "tone8.wav",
      "tonef32.wav",
    ];
    let mut world = clips_over(&names, AudioClipLoader::default());
    for name in names {
      let auto = loaded(&mut world, name, ClipMode::Auto);
      let forced = loaded(&mut world, name, ClipMode::Decode);
      let (ClipData::Decoded(auto_data), ClipData::Decoded(forced_data)) =
        (auto.data(), forced.data())
      else {
        panic!("{name}: both decoded");
      };
      assert_eq!(auto_data.sample_rate, forced_data.sample_rate, "{name}");
      assert_eq!(auto_data.frames.len(), forced_data.frames.len(), "{name}");
      assert!(
        auto_data
          .frames
          .iter()
          .zip(forced_data.frames.iter())
          .all(|(a, b)| a == b),
        "{name}: the same frames"
      );
      assert!(
        auto_data.frames.iter().any(|frame| frame.left.abs() > 0.01),
        "{name}: audible"
      );
      assert_eq!(auto.duration(), forced.duration(), "{name}");
    }
  }

  /// A streamed clip holds the very bytes the loader read: the file is not
  /// copied
  #[cfg(not(target_arch = "wasm32"))]
  #[test]
  fn a_streamed_clip_keeps_the_file_s_bytes_without_a_copy() {
    let bytes = fixture("tone.ogg");
    let read = bytes.as_ptr();
    let settings = AudioClipSettings {
      mode: ClipMode::Stream,
    };
    let Ok(path) = AssetPath::parse("tone.ogg") else {
      panic!("a path");
    };
    let clip = match AudioClipLoader::default().clip_from(bytes, &settings, &path) {
      Ok(clip) => clip,
      Err(error) => panic!("{}", error.report()),
    };
    let ClipData::Encoded(encoded) = clip.data() else {
      panic!("streamed");
    };
    assert_eq!(encoded.bytes.as_ptr(), read);
  }

  fn fixture(name: &str) -> Vec<u8> {
    match std::fs::read(format!("{FIXTURES}/{name}")) {
      Ok(bytes) => bytes,
      Err(error) => panic!("{name}: {error}"),
    }
  }

  fn clips_over(names: &[&str], loader: AudioClipLoader) -> World {
    let files = MemorySource::new();
    for name in names {
      files.insert(name, fixture(name));
    }
    files.insert(
      "corrupt.ogg",
      b"OggS this is not a vorbis stream at all".to_vec(),
    );
    let mut world = World::new();
    let server = AssetServer::new(files);
    server.register_asset::<AudioClip>(&mut world);
    server.register_loader(loader);
    world.insert_resource(server);
    world
  }

  fn load(world: &mut World, path: &str, mode: ClipMode) -> Result<AudioClip, Arc<AssetError>> {
    let handle =
      AssetsView::new(world).load_now_with::<AudioClip, _>(path, AudioClipSettings { mode })?;
    match world.resource::<Assets<AudioClip>>().get(&handle) {
      Some(clip) => Ok(clip.clone()),
      None => panic!("{path} is loaded but not in its store"),
    }
  }

  fn loaded(world: &mut World, path: &str, mode: ClipMode) -> AudioClip {
    match load(world, path, mode) {
      Ok(clip) => clip,
      Err(error) => panic!("{path}: {}", error.report()),
    }
  }

  fn near(a: Duration, b: Duration) -> bool {
    a.abs_diff(b) <= Duration::from_millis(5)
  }

  #[test]
  fn each_format_loads_its_duration_and_rate() {
    let names = ["tone.wav", "tone.ogg", "tone.mp3", "tone.flac"];
    let mut world = clips_over(&names, AudioClipLoader::default());
    for name in names {
      let clip = loaded(&mut world, name, ClipMode::Auto);
      assert!(
        near(clip.duration(), Duration::from_millis(250)),
        "{name}: {:?}",
        clip.duration()
      );
      assert_eq!(clip.sample_rate(), 22050, "{name}");
    }
  }

  #[test]
  fn eight_bit_mono_and_float_stereo_wavs_load() {
    let mut world = clips_over(&["tone8.wav", "tonef32.wav"], AudioClipLoader::default());
    let eight = loaded(&mut world, "tone8.wav", ClipMode::Auto);
    assert!(near(eight.duration(), Duration::from_millis(250)));
    assert_eq!(eight.sample_rate(), 22050);
    let float = loaded(&mut world, "tonef32.wav", ClipMode::Auto);
    assert!(near(float.duration(), Duration::from_millis(100)));
    assert_eq!(float.sample_rate(), 22050);
  }

  #[test]
  fn a_surround_file_is_a_decode_error_naming_its_path() {
    let mut world = clips_over(&["surround.wav"], AudioClipLoader::default());
    // A clip to stream is probed: its header gives its channels
    for mode in [ClipMode::Auto, ClipMode::Decode, ClipMode::Stream] {
      let Err(error) = load(&mut world, "surround.wav", mode) else {
        panic!("{mode:?}: six channels loaded");
      };
      assert!(matches!(*error, AssetError::Decode { .. }), "{mode:?}");
      assert!(
        error.report().contains("surround.wav"),
        "{}",
        error.report()
      );
    }
  }

  #[test]
  fn a_corrupt_file_is_a_decode_error() {
    let mut world = clips_over(&[], AudioClipLoader::default());
    let Err(error) = load(&mut world, "corrupt.ogg", ClipMode::Auto) else {
      panic!("garbage loaded");
    };
    assert!(
      matches!(*error, AssetError::Decode { .. }),
      "{}",
      error.report()
    );
  }

  #[test]
  fn a_short_clip_decodes_and_a_forced_stream_keeps_its_bytes() {
    let mut world = clips_over(&["tone.ogg"], AudioClipLoader::default());
    let auto = loaded(&mut world, "tone.ogg", ClipMode::Auto);
    assert!(!auto.is_streamed());
    let streamed = loaded(&mut world, "tone.ogg", ClipMode::Stream);
    if cfg!(target_arch = "wasm32") {
      assert!(!streamed.is_streamed(), "the web decodes every clip");
    } else {
      assert!(streamed.is_streamed());
      assert!(near(streamed.duration(), auto.duration()));
      assert_eq!(streamed.sample_rate(), auto.sample_rate());
    }
    let decoded = loaded(&mut world, "tone.ogg", ClipMode::Decode);
    assert!(!decoded.is_streamed());
  }

  #[cfg(not(target_arch = "wasm32"))]
  #[test]
  fn a_clip_longer_than_the_threshold_streams_and_is_never_decoded() {
    let loader = AudioClipLoader::with_threshold(Duration::from_millis(50));
    let mut world = clips_over(&["tone.ogg"], loader);
    let clip = loaded(&mut world, "tone.ogg", ClipMode::Auto);
    assert!(clip.is_streamed());
    match clip.data() {
      ClipData::Encoded(encoded) => assert_eq!(encoded.bytes.len(), fixture("tone.ogg").len()),
      ClipData::Decoded(_) => panic!("a clip over the threshold was decoded"),
    }
    // Decode still decodes it
    assert!(!loaded(&mut world, "tone.ogg", ClipMode::Decode).is_streamed());
  }

  /// A streamed clip keeps the sample rate and frame count its file's
  /// header gives, which are what decoding the whole file gives
  #[cfg(not(target_arch = "wasm32"))]
  #[test]
  fn a_streamed_clip_keeps_its_files_exact_rate_and_length() {
    let names = [
      "tone.wav",
      "tone.ogg",
      "tone.mp3",
      "tone.flac",
      "tone-3s.ogg",
    ];
    let mut world = clips_over(&names, AudioClipLoader::default());
    for name in names {
      let streamed = loaded(&mut world, name, ClipMode::Stream);
      let decoded = loaded(&mut world, name, ClipMode::Decode);
      let (ClipData::Encoded(encoded), ClipData::Decoded(data)) = (streamed.data(), decoded.data())
      else {
        panic!("{name}: one streamed and one decoded clip");
      };
      assert_eq!(encoded.frames, data.frames.len(), "{name}");
      assert_eq!(streamed.sample_rate(), data.sample_rate, "{name}");
      assert_eq!(streamed.duration(), decoded.duration(), "{name}");
    }
  }

  #[test]
  fn the_threshold_decides_by_mode_and_the_web_decodes_everything() {
    let (short, long, limit) = (
      Duration::from_secs(1),
      Duration::from_secs(60),
      DECODE_THRESHOLD,
    );
    for wasm in [false, true] {
      assert!(decodes(ClipMode::Decode, long, limit, wasm));
    }
    assert!(decodes(ClipMode::Auto, short, limit, false));
    assert!(decodes(ClipMode::Auto, limit, limit, false));
    assert!(!decodes(ClipMode::Auto, long, limit, false));
    assert!(!decodes(ClipMode::Stream, short, limit, false));
    for mode in [ClipMode::Auto, ClipMode::Decode, ClipMode::Stream] {
      assert!(decodes(mode, long, limit, true), "{mode:?} on the web");
    }
  }

  #[test]
  fn a_clip_made_from_frames_is_decoded() {
    let clip = match AudioClip::from_frames(1000, &[[0.0, 0.5]; 500]) {
      Ok(clip) => clip,
      Err(error) => panic!("{error}"),
    };
    assert!(!clip.is_streamed());
    assert_eq!(clip.sample_rate(), 1000);
    assert_eq!(clip.duration(), Duration::from_millis(500));
  }

  /// A clip at a sample rate of 0 never advances: it would never end, and
  /// its voice would hold its place for good
  #[test]
  fn a_clip_needs_a_sample_rate() {
    let error = AudioClip::from_frames(0, &[[0.1, 0.1]; 10]).err();
    assert_eq!(error, Some(AudioClipError::ZeroSampleRate));
    let message = error.map(|error| error.to_string()).unwrap_or_default();
    assert!(message.contains("sample rate"), "{message}");
  }

  #[test]
  fn every_registry_loads_audio_clips() {
    let files = MemorySource::new();
    files.insert("tone.wav", fixture("tone.wav"));
    let mut world = World::new();
    crate::core::asset::insert_registry(&mut world, AssetServer::new(files));
    let handle = match AssetsView::new(&mut world).load_now::<AudioClip>("tone.wav") {
      Ok(handle) => handle,
      Err(error) => panic!("{}", error.report()),
    };
    assert!(world.resource::<Assets<AudioClip>>().get(&handle).is_some());
  }
}
