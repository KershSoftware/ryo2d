//! The engine's decoder for streamed clips.
//!
//! A streamed clip (`ClipData::Encoded`) plays through [`ClipDecoder`], a
//! kira [`Decoder`] over the clip's bytes that uses symphonia directly. It
//! opens on kira's decoder thread: making one only stores the bytes and the
//! rate and length the loader's [`probe`] read, and the probe, the codec's
//! setup and the seek to the start position all wait for the first
//! `decode`. Its frames are exact: after a seek to frame `n` they start at
//! the file's frame `n` and run on without a gap. It always gives the clip's
//! frame count: a file that ends early is padded with silence to it, and
//! frames past it are dropped.
//!
//! kira's own decoder opens the file on the thread that calls `play`, 0.3 to
//! 1.2 ms for an OGG, and miscounts frames after a Vorbis seek (the first
//! packet after it decodes to nothing): a stream started partway runs a
//! packet ahead, never gives the file's last frames, and its decoder thread
//! spins at the end instead of ending or looping.

use std::fmt;
use std::io::{Cursor, ErrorKind};
use std::sync::Arc;
use std::time::Duration;

use kira::Frame;
use kira::sound::streaming::Decoder;
use symphonia::core::audio::conv::{FromSample, IntoSample};
use symphonia::core::audio::sample::Sample;
use symphonia::core::audio::{Audio, AudioBuffer, GenericAudioBufferRef};
use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::audio::{AudioCodecParameters, AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::{Error as SymphoniaError, SeekErrorKind};
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatReader, SeekMode, SeekTo, Track, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::units::Timestamp;

/// The most frames of silence one `decode` gives past the file's end
const SILENCE_CHUNK: usize = 4096;

/// How far before its target a seek starts decoding, the frames before the
/// target dropped, so a codec whose frames depend on the ones before (MP3's
/// overlap and bit reservoir) decodes the target exactly. The OGG reader
/// steps back for Vorbis itself
const PREROLL: usize = 2048;

/// How long `decode` waits before saying again why the stream failed: kira
/// asks again at once until the mixer stops the sound, which would
/// otherwise spin a core
const FAILED_RETRY: Duration = Duration::from_millis(1);

/// Why a streamed clip could not be decoded, in the words the engine reports
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StreamError(String);

impl StreamError {
  fn new(why: impl Into<String>) -> Self {
    StreamError(why.into())
  }

  fn channels(count: usize) -> Self {
    StreamError(format!(
      "streams play mono or stereo files, and this one has {count} channels"
    ))
  }
}

impl fmt::Display for StreamError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(&self.0)
  }
}

impl std::error::Error for StreamError {}

impl From<SymphoniaError> for StreamError {
  fn from(error: SymphoniaError) -> Self {
    StreamError(error.to_string())
  }
}

/// A file's sample rate and length in frames, read from its header
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StreamInfo {
  pub(crate) sample_rate: u32,
  pub(crate) frames: usize,
}

/// Read `bytes`' header: its sample rate and length, and whether its codec
/// is one the decoder has. Nothing is decoded
pub(crate) fn probe(bytes: &Arc<[u8]>) -> Result<StreamInfo, StreamError> {
  let reader = open_reader(bytes)?;
  let (track, params) = audio_track(reader.as_ref())?;
  if symphonia::default::get_codecs()
    .get_audio_decoder(params.codec)
    .is_none()
  {
    return Err(StreamError::new("the file's codec is not supported"));
  }
  let sample_rate = params
    .sample_rate
    .ok_or_else(|| StreamError::new("the file does not give its sample rate"))?;
  let frames = track
    .num_frames
    .and_then(|frames| usize::try_from(frames).ok())
    .ok_or_else(|| StreamError::new("the file does not give its length"))?;
  Ok(StreamInfo {
    sample_rate,
    frames,
  })
}

/// A format reader over `bytes`, past the file's header
fn open_reader(bytes: &Arc<[u8]>) -> Result<Box<dyn FormatReader>, StreamError> {
  let source = Cursor::new(Arc::clone(bytes));
  let stream = MediaSourceStream::new(Box::new(source), Default::default());
  let reader = symphonia::default::get_probe().probe(
    &Hint::default(),
    stream,
    Default::default(),
    Default::default(),
  )?;
  Ok(reader)
}

/// `reader`'s default audio track, and its codec parameters
fn audio_track(reader: &dyn FormatReader) -> Result<(&Track, &AudioCodecParameters), StreamError> {
  let no_track = || StreamError::new("the file has no audio track");
  let track = reader
    .default_track(TrackType::Audio)
    .ok_or_else(no_track)?;
  match &track.codec_params {
    Some(CodecParameters::Audio(params)) => Ok((track, params)),
    _ => Err(no_track()),
  }
}

/// A streamed clip's decoder: see the module docs
pub(crate) struct ClipDecoder {
  bytes: Arc<[u8]>,
  sample_rate: u32,
  num_frames: usize,
  /// The file frame the next `decode` starts at
  next: usize,
  /// The file has no more packets: `decode` pads silence from `next`
  ended: bool,
  state: State,
}

enum State {
  /// Not opened yet: the first `decode` opens the file
  Closed,
  Open(Box<Opened>),
  /// It failed to open or decode: every `decode` says why
  Failed(StreamError),
}

/// An open file: its reader, its codec's decoder and the track they read
struct Opened {
  reader: Box<dyn FormatReader>,
  decoder: Box<dyn AudioDecoder>,
  track: u32,
}

impl ClipDecoder {
  /// A decoder over a clip's `bytes`, of `num_frames` frames at
  /// `sample_rate` as the loader's probe read them. Nothing is opened
  pub(crate) fn new(bytes: Arc<[u8]>, sample_rate: u32, num_frames: usize) -> Self {
    Self {
      bytes,
      sample_rate,
      num_frames,
      next: 0,
      ended: false,
      state: State::Closed,
    }
  }

  /// Open the file and move it to `next`, unless that was done
  fn open(&mut self) -> Result<(), StreamError> {
    match &self.state {
      State::Open(_) => return Ok(()),
      State::Failed(error) => {
        std::thread::sleep(FAILED_RETRY);
        return Err(error.clone());
      }
      State::Closed => {}
    }
    let opened = Opened::new(&self.bytes).and_then(|mut opened| {
      if self.next > 0 {
        self.ended = opened.seek(self.next, self.num_frames)?;
      }
      Ok(opened)
    });
    self.state = match opened {
      Ok(opened) => State::Open(Box::new(opened)),
      Err(error) => return Err(self.fail(error)),
    };
    Ok(())
  }

  /// Fail the stream with `error`, which every later `decode` gives again
  fn fail(&mut self, error: StreamError) -> StreamError {
    self.state = State::Failed(error.clone());
    error
  }

  /// Up to [`SILENCE_CHUNK`] frames of silence, to the clip's end
  fn silence(&mut self) -> Vec<Frame> {
    let count = SILENCE_CHUNK.min(self.num_frames.saturating_sub(self.next));
    self.next += count;
    vec![Frame::ZERO; count]
  }

  /// The next frames from `next`: a packet's frames, cut to `next..` and to
  /// the clip's length, after silence for any frames the file skipped. Empty
  /// only once the clip has ended
  fn next_frames(&mut self) -> Result<Vec<Frame>, StreamError> {
    let mut frames = Vec::new();
    loop {
      if self.next >= self.num_frames {
        return Ok(frames);
      }
      if self.ended {
        return Ok(self.silence());
      }
      let State::Open(opened) = &mut self.state else {
        return Ok(frames);
      };
      let packet = match opened.reader.next_packet() {
        Ok(Some(packet)) => packet,
        // A file cut short ends where it is cut
        Ok(None) => {
          self.ended = true;
          continue;
        }
        Err(SymphoniaError::IoError(error)) if error.kind() == ErrorKind::UnexpectedEof => {
          self.ended = true;
          continue;
        }
        Err(error) => return Err(error.into()),
      };
      if packet.track_id != opened.track {
        continue;
      }
      let buffer = match opened.decoder.decode(&packet) {
        Ok(buffer) => buffer,
        // A corrupt packet is skipped, and its frames are silence
        Err(SymphoniaError::DecodeError(_)) => continue,
        Err(error) => return Err(error.into()),
      };
      // The decoder trims the packet's start, so its first frame is the
      // packet's timestamp plus that trim. A codec that gives no frames for
      // a packet (Vorbis, for the first one after a seek) moves nothing
      let first = packet
        .pts
        .get()
        .saturating_add_unsigned(packet.trim_start.get());
      let count = buffer.frames();
      let last = first.saturating_add_unsigned(count as u64);
      let next = self.next as i64;
      if count == 0 || last <= next {
        continue;
      }
      let end = (self.num_frames as i64).min(last);
      if first > next {
        let gap = (end.min(first) - next) as usize;
        frames.resize(gap, Frame::ZERO);
      }
      let skip = (next - first).max(0) as usize;
      let take = (end - first.max(next)).max(0) as usize;
      push_frames(&buffer, skip, take, &mut frames)?;
      self.next = end as usize;
      return Ok(frames);
    }
  }
}

impl Opened {
  /// Probe `bytes` and make its codec's decoder. Only mono and stereo play
  fn new(bytes: &Arc<[u8]>) -> Result<Opened, StreamError> {
    let reader = open_reader(bytes)?;
    let (track, params) = audio_track(reader.as_ref())?;
    if let Some(channels) = &params.channels
      && channels.count() > 2
    {
      return Err(StreamError::channels(channels.count()));
    }
    let decoder = symphonia::default::get_codecs()
      .make_audio_decoder(params, &AudioDecoderOptions::default())?;
    let track = track.id;
    Ok(Opened {
      reader,
      decoder,
      track,
    })
  }

  /// Move to file frame `index`, [`PREROLL`] before it, with the codec
  /// reset. Whether the file ends before there, so all that is left of the
  /// clip's `num_frames` is silence
  fn seek(&mut self, index: usize, num_frames: usize) -> Result<bool, StreamError> {
    if index >= num_frames {
      return Ok(true);
    }
    let from = index.saturating_sub(PREROLL);
    let to = SeekTo::Timestamp {
      ts: Timestamp::new(i64::try_from(from).unwrap_or(i64::MAX)),
      track_id: self.track,
    };
    match self.reader.seek(SeekMode::Accurate, to) {
      Ok(_) => {
        self.decoder.reset();
        Ok(false)
      }
      Err(SymphoniaError::SeekError(SeekErrorKind::OutOfRange)) => Ok(true),
      Err(error) => Err(error.into()),
    }
  }
}

impl Decoder for ClipDecoder {
  type Error = StreamError;

  fn sample_rate(&self) -> u32 {
    self.sample_rate
  }

  fn num_frames(&self) -> usize {
    self.num_frames
  }

  fn decode(&mut self) -> Result<Vec<Frame>, StreamError> {
    self.open()?;
    self.next_frames().map_err(|error| self.fail(error))
  }

  /// Exact: an open file seeks now, one not yet open on its first `decode`
  fn seek(&mut self, index: usize) -> Result<usize, StreamError> {
    let index = index.min(self.num_frames);
    match &mut self.state {
      State::Open(opened) if index != self.next => match opened.seek(index, self.num_frames) {
        Ok(ended) => self.ended = ended,
        Err(error) => return Err(self.fail(error)),
      },
      State::Open(_) => {}
      State::Closed | State::Failed(_) => self.ended = false,
    }
    self.next = index;
    Ok(index)
  }
}

/// Push `take` frames of `buffer`, from its frame `skip`, onto `out`
fn push_frames(
  buffer: &GenericAudioBufferRef<'_>,
  skip: usize,
  take: usize,
  out: &mut Vec<Frame>,
) -> Result<(), StreamError> {
  match buffer {
    GenericAudioBufferRef::U8(buffer) => push_samples(buffer, skip, take, out),
    GenericAudioBufferRef::U16(buffer) => push_samples(buffer, skip, take, out),
    GenericAudioBufferRef::U24(buffer) => push_samples(buffer, skip, take, out),
    GenericAudioBufferRef::U32(buffer) => push_samples(buffer, skip, take, out),
    GenericAudioBufferRef::S8(buffer) => push_samples(buffer, skip, take, out),
    GenericAudioBufferRef::S16(buffer) => push_samples(buffer, skip, take, out),
    GenericAudioBufferRef::S24(buffer) => push_samples(buffer, skip, take, out),
    GenericAudioBufferRef::S32(buffer) => push_samples(buffer, skip, take, out),
    GenericAudioBufferRef::F32(buffer) => push_samples(buffer, skip, take, out),
    GenericAudioBufferRef::F64(buffer) => push_samples(buffer, skip, take, out),
  }
}

/// [`push_frames`] for one sample format, converted as kira converts it
fn push_samples<S: Sample>(
  buffer: &AudioBuffer<S>,
  skip: usize,
  take: usize,
  out: &mut Vec<Frame>,
) -> Result<(), StreamError>
where
  f32: FromSample<S>,
{
  let plane = |index: usize| {
    buffer
      .plane(index)
      .and_then(|plane| plane.get(skip..skip + take))
      .ok_or_else(|| StreamError::new("the decoder gave fewer frames than it said"))
  };
  match buffer.num_planes() {
    1 => {
      let mono = plane(0)?;
      out.extend(
        mono
          .iter()
          .map(|&sample| Frame::from_mono(sample.into_sample())),
      );
    }
    2 => {
      let (left, right) = (plane(0)?, plane(1)?);
      out.extend(
        left
          .iter()
          .zip(right)
          .map(|(&left, &right)| Frame::new(left.into_sample(), right.into_sample())),
      );
    }
    count => return Err(StreamError::channels(count)),
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use kira::sound::static_sound::StaticSoundData;

  use super::*;

  const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/audio");

  fn fixture(name: &str) -> Arc<[u8]> {
    match std::fs::read(format!("{FIXTURES}/{name}")) {
      Ok(bytes) => Arc::from(bytes),
      Err(error) => panic!("{name}: {error}"),
    }
  }

  /// A decoder over `bytes`, with the rate and length the loader's probe
  /// reads
  fn decoder_over(name: &str, bytes: Arc<[u8]>) -> ClipDecoder {
    match probe(&bytes) {
      Ok(info) => ClipDecoder::new(bytes, info.sample_rate, info.frames),
      Err(error) => panic!("{name} probes: {error}"),
    }
  }

  fn decoder(name: &str) -> ClipDecoder {
    decoder_over(name, fixture(name))
  }

  /// Every frame of `name`, decoded in one go by kira
  fn decoded(name: &str) -> Vec<Frame> {
    match StaticSoundData::from_cursor(Cursor::new(fixture(name))) {
      Ok(data) => data.frames.to_vec(),
      Err(error) => panic!("{name} decodes: {error}"),
    }
  }

  fn decode(decoder: &mut ClipDecoder) -> Vec<Frame> {
    match decoder.decode() {
      Ok(frames) => frames,
      Err(error) => panic!("decode: {error}"),
    }
  }

  fn seek(decoder: &mut ClipDecoder, index: usize) -> usize {
    match decoder.seek(index) {
      Ok(index) => index,
      Err(error) => panic!("seek to {index}: {error}"),
    }
  }

  /// The next `count` frames `decoder` gives, or fewer if it ends first
  fn collect(decoder: &mut ClipDecoder, count: usize) -> Vec<Frame> {
    let mut frames = Vec::new();
    while frames.len() < count {
      let chunk = decode(decoder);
      if chunk.is_empty() {
        break;
      }
      frames.extend(chunk);
    }
    frames.truncate(count);
    frames
  }

  /// `got` is `want` within 1e-4 a sample; `from` is `want`'s first frame
  /// in the file
  fn assert_same(name: &str, got: &[Frame], want: &[Frame], from: usize) {
    assert_eq!(got.len(), want.len(), "{name}: frames from {from}");
    for (offset, (got, want)) in got.iter().zip(want).enumerate() {
      let off = (got.left - want.left)
        .abs()
        .max((got.right - want.right).abs());
      assert!(
        off <= 1e-4,
        "{name}: frame {} is {got:?}, decoded {want:?}",
        from + offset
      );
    }
  }

  /// `name` streamed from frame `start` gives the decoded file's frames
  /// from there: 4096 of them, or up to the end of a shorter file
  fn starts_exactly(name: &str, start: usize) {
    let file = decoded(name);
    let count = 4096.min(file.len() - start);
    let mut decoder = decoder(name);
    assert_eq!(seek(&mut decoder, start), start);
    let frames = collect(&mut decoder, count);
    assert_same(name, &frames, &file[start..start + count], start);
  }

  #[test]
  fn a_stream_started_partway_matches_the_decoded_file() {
    starts_exactly("tone-3s.ogg", 88_200);
  }

  #[test]
  fn an_mp3_stream_started_partway_matches_the_decoded_file() {
    starts_exactly("tone.mp3", 3_000);
  }

  #[test]
  fn a_flac_stream_started_partway_matches_the_decoded_file() {
    starts_exactly("tone.flac", 3_000);
  }

  #[test]
  fn a_stream_started_partway_reaches_exactly_num_frames_then_ends() {
    let mut decoder = decoder("tone-3s.ogg");
    let (start, end) = (88_200, decoder.num_frames());
    seek(&mut decoder, start);
    let mut total = 0;
    loop {
      let chunk = decode(&mut decoder);
      if chunk.is_empty() {
        break;
      }
      total += chunk.len();
      assert!(total <= end, "{total} frames, past the file's {end}");
    }
    assert_eq!(total, end - start);
    for _ in 0..3 {
      assert!(decode(&mut decoder).is_empty());
    }
  }

  #[test]
  fn a_stream_seeks_back_and_stays_accurate() {
    let file = decoded("tone-3s.ogg");
    let mut decoder = decoder("tone-3s.ogg");
    let played = collect(&mut decoder, 100_000);
    assert_eq!(played.len(), 100_000, "it decodes past 2 s");
    assert_eq!(seek(&mut decoder, 22_050), 22_050);
    let frames = collect(&mut decoder, 4096);
    assert_same("tone-3s.ogg", &frames, &file[22_050..22_050 + 4096], 22_050);
  }

  /// A file that ends before the length its header gives is padded with
  /// silence to that length, in chunks of at most 4096 frames
  #[test]
  fn a_stream_cut_short_is_padded_with_silence_to_num_frames() {
    // Ten of the WAV reader's 1,152-frame packets are kept; the header
    // still says 22,050 frames
    const KEPT: usize = 11_520;
    let wav = crate::core::testing::wav(22_050, 22_050, 8_000);
    let cut: Arc<[u8]> = Arc::from(&wav[..44 + KEPT * 2]);
    let mut decoder = decoder_over("cut.wav", cut);
    assert_eq!(decoder.num_frames(), 22_050);
    seek(&mut decoder, 5_000);
    let mut frames = Vec::new();
    loop {
      let chunk = decode(&mut decoder);
      if chunk.is_empty() {
        break;
      }
      assert!(chunk.len() <= 4096, "a chunk of {}", chunk.len());
      frames.extend(chunk);
      assert!(frames.len() <= 17_050, "{} frames", frames.len());
    }
    assert_eq!(frames.len(), 22_050 - 5_000);
    let sound = KEPT - 5_000;
    assert!(frames[..sound].iter().all(|frame| frame.left > 0.2));
    assert!(frames[sound..].iter().all(|frame| *frame == Frame::ZERO));
  }

  /// Nothing is probed until kira's decoder thread asks for frames
  #[test]
  fn opening_waits_for_the_first_decode() {
    let mut decoder = ClipDecoder::new(Arc::from(&b"not audio at all"[..]), 44_100, 44_100);
    assert_eq!(decoder.sample_rate(), 44_100);
    assert_eq!(decoder.num_frames(), 44_100);
    assert_eq!(decoder.seek(22_050), Ok(22_050));
    assert!(decoder.decode().is_err());
    assert!(decoder.decode().is_err(), "it stays failed");
  }

  #[test]
  fn a_surround_stream_is_an_error() {
    let mut decoder = decoder("surround.wav");
    let Err(error) = decoder.decode() else {
      panic!("six channels decoded");
    };
    assert!(error.to_string().contains("6 channels"), "{error}");
  }
}
