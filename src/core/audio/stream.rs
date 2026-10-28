//! The engine's decoder for streamed clips.
//!
//! A streamed clip (`ClipData::Encoded`) plays through [`ClipDecoder`], a
//! kira [`Decoder`] over the clip's bytes that uses symphonia directly. It
//! opens on kira's decoder thread: making one only stores the bytes, the
//! rate the loader's [`probe`] read and the frames it plays, and the probe,
//! the codec's setup and the seek to the start position all wait for the
//! first `decode`. It plays a window of the file (a definition's region, or
//! all of it) as its whole stream, so kira never seeks it from the file's
//! start. Its frames are exact: after a seek to frame `n` they start at the
//! window's frame `n` and run on without a gap. It always gives the
//! window's frame count: a file that ends early is padded with silence to
//! it, and frames past it are dropped. A packet the codec cannot decode is
//! skipped, its frames silence; a gap in the file's timestamps is silence
//! too, given at most [`SILENCE_CHUNK`] frames a call.
//!
//! kira's own decoder opens the file on the thread that calls `play`, 0.3 to
//! 1.2 ms for an OGG, and miscounts frames after a Vorbis seek (the first
//! packet after it decodes to nothing): a stream started partway runs a
//! packet ahead, never gives the file's last frames, and its decoder thread
//! spins at the end instead of ending or looping.

use std::fmt;
use std::io::{Cursor, ErrorKind};
use std::ops::Range;
use std::sync::Arc;
#[cfg(test)]
use std::sync::atomic::AtomicUsize;
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
/// is one the decoder has and its channels one or two. Nothing is decoded
pub(crate) fn probe(bytes: &Arc<[u8]>) -> Result<StreamInfo, StreamError> {
  let reader = open_reader(bytes)?;
  let (track, params) = audio_track(reader.as_ref())?;
  check_channels(params)?;
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

/// Only mono and stereo play: more channels than that, where the codec
/// gives them, is an error
fn check_channels(params: &AudioCodecParameters) -> Result<(), StreamError> {
  match &params.channels {
    Some(channels) if channels.count() > 2 => Err(StreamError::channels(channels.count())),
    _ => Ok(()),
  }
}

/// A streamed clip's decoder: see the module docs
pub(crate) struct ClipDecoder {
  bytes: Arc<[u8]>,
  sample_rate: u32,
  /// The file's frames it plays (a clip's region, or all of it): kira's
  /// frame `i` is the file's `start + i`
  start: usize,
  end: usize,
  /// The file frame the next `decode` starts at
  next: usize,
  /// The file has no more packets: `decode` pads silence from `next`
  ended: bool,
  /// A packet's frames, from the file frame given, waiting for the silence
  /// before them to be given first
  pending: Option<(usize, Vec<Frame>)>,
  state: State,
  /// Where the frames the codec decodes are counted (tests)
  #[cfg(test)]
  decoded: Option<Arc<AtomicUsize>>,
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
  /// A decoder over a clip's `bytes`, at `sample_rate` as the loader's probe
  /// read it, playing the file's frames `window` as its whole stream.
  /// Nothing is opened
  pub(crate) fn new(bytes: Arc<[u8]>, sample_rate: u32, window: Range<usize>) -> Self {
    let end = window.end;
    let start = window.start.min(end);
    Self {
      bytes,
      sample_rate,
      start,
      end,
      next: start,
      ended: false,
      pending: None,
      state: State::Closed,
      #[cfg(test)]
      decoded: None,
    }
  }

  /// Count every frame the codec decodes into `decoded` (tests)
  #[cfg(test)]
  pub(crate) fn counting(mut self, decoded: Arc<AtomicUsize>) -> Self {
    self.decoded = Some(decoded);
    self
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
        self.ended = opened.seek(self.next, self.end)?;
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

  /// Up to [`SILENCE_CHUNK`] frames of silence from `next`, to `until`
  fn silence(&mut self, until: usize) -> Vec<Frame> {
    let count = SILENCE_CHUNK.min(until.saturating_sub(self.next));
    self.next += count;
    vec![Frame::ZERO; count]
  }

  /// The next frames from `next`: a packet's frames, cut to `next..` and to
  /// the window's end, after silence for any frames the file skipped (at
  /// most [`SILENCE_CHUNK`] a call, the packet kept for a later one). Empty
  /// only once the window has ended
  fn next_frames(&mut self) -> Result<Vec<Frame>, StreamError> {
    loop {
      if let Some((first, _)) = &self.pending {
        let first = *first;
        if first > self.next {
          return Ok(self.silence(first));
        }
        if let Some((_, frames)) = self.pending.take() {
          self.next += frames.len();
          return Ok(frames);
        }
      }
      if self.next >= self.end {
        return Ok(Vec::new());
      }
      if self.ended {
        return Ok(self.silence(self.end));
      }
      let State::Open(opened) = &mut self.state else {
        return Ok(Vec::new());
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
        Err(error) if skips_packet(&error) => continue,
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
      #[cfg(test)]
      if let Some(decoded) = &self.decoded {
        decoded.fetch_add(count, std::sync::atomic::Ordering::Relaxed);
      }
      let last = first.saturating_add_unsigned(count as u64);
      let next = self.next as i64;
      if count == 0 || last <= next {
        continue;
      }
      // Past the window's end: all that is left of it is silence
      let end = self.end as i64;
      if first >= end {
        self.ended = true;
        continue;
      }
      let from = first.max(next);
      let skip = (from - first) as usize;
      let take = (end.min(last) - from) as usize;
      let mut frames = Vec::with_capacity(take);
      push_frames(&buffer, skip, take, &mut frames)?;
      if from > next {
        self.pending = Some((from as usize, frames));
        continue;
      }
      self.next += frames.len();
      return Ok(frames);
    }
  }
}

impl Opened {
  /// Probe `bytes` and make its codec's decoder. Only mono and stereo play
  fn new(bytes: &Arc<[u8]>) -> Result<Opened, StreamError> {
    let reader = open_reader(bytes)?;
    let (track, params) = audio_track(reader.as_ref())?;
    // Probed already, but for a clip made in code
    check_channels(params)?;
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
  /// window, which ends at `end`, is silence
  fn seek(&mut self, index: usize, end: usize) -> Result<bool, StreamError> {
    if index >= end {
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
    self.end - self.start
  }

  fn decode(&mut self) -> Result<Vec<Frame>, StreamError> {
    self.open()?;
    self.next_frames().map_err(|error| self.fail(error))
  }

  /// Exact, in the window's frames: an open file seeks now, one not yet
  /// open on its first `decode`
  fn seek(&mut self, index: usize) -> Result<usize, StreamError> {
    let index = index.min(self.end - self.start);
    let target = self.start + index;
    match &mut self.state {
      State::Open(opened) if target != self.next => {
        self.pending = None;
        match opened.seek(target, self.end) {
          Ok(ended) => self.ended = ended,
          Err(error) => return Err(self.fail(error)),
        }
      }
      State::Open(_) => {}
      State::Closed | State::Failed(_) => self.ended = false,
    }
    self.next = target;
    Ok(index)
  }
}

/// Whether a codec's `error` decoding a packet is that packet's alone:
/// symphonia documents a decode error, and an IO error (a codec reads only
/// the packet, so one is a corrupt packet read past its end), as leaving the
/// codec able to go on with the next
fn skips_packet(error: &SymphoniaError) -> bool {
  matches!(
    error,
    SymphoniaError::DecodeError(_) | SymphoniaError::IoError(_)
  )
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
      Ok(info) => ClipDecoder::new(bytes, info.sample_rate, 0..info.frames),
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

  /// The loudest sample a reference must reach to prove a match: a silent
  /// one matches any silence
  const AUDIBLE: f32 = 0.01;

  /// `got` is `want` within 1e-4 a sample; `from` is `want`'s first frame
  /// in the file. `want` must be audible (a peak above [`AUDIBLE`]), so a
  /// silent fixture fails every check made with it, `starts_exactly`'s too
  fn assert_same(name: &str, got: &[Frame], want: &[Frame], from: usize) {
    let peak = want
      .iter()
      .map(|frame| frame.left.abs().max(frame.right.abs()))
      .fold(0.0, f32::max);
    assert!(
      peak > AUDIBLE,
      "{name}: the reference frames from {from} are silent (peak {peak}), so they prove nothing"
    );
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

  /// The accuracy checks compare against sound: a silent reference would
  /// match anything that is silent too, as a broken fixture once did
  #[test]
  #[should_panic(expected = "silent")]
  fn the_accuracy_checks_refuse_a_silent_reference() {
    let silence = [Frame::ZERO; 64];
    assert_same("silence", &silence, &silence, 0);
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
    let mut decoder = ClipDecoder::new(Arc::from(&b"not audio at all"[..]), 44_100, 0..44_100);
    assert_eq!(decoder.sample_rate(), 44_100);
    assert_eq!(decoder.num_frames(), 44_100);
    assert_eq!(decoder.seek(22_050), Ok(22_050));
    assert!(decoder.decode().is_err());
    assert!(decoder.decode().is_err(), "it stays failed");
  }

  /// Streamed from the start, with no seek, each format gives the decoded
  /// file's frames: the Vorbis packet that decodes to nothing and MP3's
  /// encoder delay included
  #[test]
  fn streams_started_at_zero_match_the_decoded_files() {
    for name in ["tone-3s.ogg", "tone.mp3", "tone.flac", "tone.wav"] {
      starts_exactly(name, 0);
    }
  }

  /// A decoder over `name` playing only the file's frames `window`
  fn windowed(name: &str, window: Range<usize>) -> ClipDecoder {
    let bytes = fixture(name);
    match probe(&bytes) {
      Ok(info) => ClipDecoder::new(bytes, info.sample_rate, window),
      Err(error) => panic!("{name} probes: {error}"),
    }
  }

  /// A clip's region is the decoder's whole stream: seeked to 0 it gives the
  /// file's frames from the region's start, and a seek into it lands as far
  /// into the region
  #[test]
  fn a_windowed_stream_seeked_to_zero_gives_its_region() {
    let file = decoded("tone-3s.ogg");
    let mut decoder = windowed("tone-3s.ogg", 88_200..file.len());
    assert_eq!(decoder.num_frames(), file.len() - 88_200);
    assert_eq!(seek(&mut decoder, 0), 0);
    let frames = collect(&mut decoder, 4096);
    assert_same("tone-3s.ogg", &frames, &file[88_200..92_296], 88_200);
    assert_eq!(seek(&mut decoder, 22_050), 22_050);
    let frames = collect(&mut decoder, 4096);
    assert_same("tone-3s.ogg", &frames, &file[110_250..114_346], 110_250);
  }

  /// A region that ends before the file does gives exactly its own frames,
  /// the last of them the file's, then nothing
  #[test]
  fn a_windowed_stream_reaches_exactly_its_region_then_ends() {
    let file = decoded("tone-3s.ogg");
    let mut decoder = windowed("tone-3s.ogg", 88_200..110_250);
    assert_eq!(decoder.num_frames(), 22_050);
    seek(&mut decoder, 0);
    let mut frames = Vec::new();
    loop {
      let chunk = decode(&mut decoder);
      if chunk.is_empty() {
        break;
      }
      frames.extend(chunk);
      assert!(frames.len() <= 22_050, "{} frames", frames.len());
    }
    assert_eq!(frames.len(), 22_050);
    assert_same(
      "tone-3s.ogg",
      &frames[21_050..],
      &file[109_250..110_250],
      109_250,
    );
    for _ in 0..3 {
      assert!(decode(&mut decoder).is_empty());
    }
  }

  /// An Ogg page as written: its 27-byte header, its lacing values and its
  /// body
  struct OggPage {
    header: Vec<u8>,
    lacing: Vec<u8>,
    body: Vec<u8>,
  }

  fn ogg_pages(bytes: &[u8]) -> Vec<OggPage> {
    let mut pages = Vec::new();
    let mut at = 0;
    while at + 27 <= bytes.len() {
      let header = bytes[at..at + 27].to_vec();
      let start = at + 27 + usize::from(header[26]);
      let lacing = bytes[at + 27..start].to_vec();
      let size: usize = lacing.iter().map(|&value| usize::from(value)).sum();
      let body = bytes[start..start + size].to_vec();
      pages.push(OggPage {
        header,
        lacing,
        body,
      });
      at = start + size;
    }
    pages
  }

  /// `pages` written out, each with its lacing count and checksum set again
  fn ogg_bytes(pages: &[OggPage]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for page in pages {
      let mut raw = page.header.clone();
      raw[26] = page.lacing.len() as u8;
      raw[22..26].fill(0);
      raw.extend_from_slice(&page.lacing);
      raw.extend_from_slice(&page.body);
      // Ogg's CRC-32: polynomial 0x04c11db7, unreflected, from 0
      let mut crc = 0u32;
      for &byte in &raw {
        crc ^= u32::from(byte) << 24;
        for _ in 0..8 {
          crc = if crc & 0x8000_0000 == 0 {
            crc << 1
          } else {
            (crc << 1) ^ 0x04c1_1db7
          };
        }
      }
      raw[22..26].copy_from_slice(&crc.to_le_bytes());
      bytes.extend(raw);
    }
    bytes
  }

  /// The first packet `bytes`' codec fails on, read with symphonia itself:
  /// its first frame, its length, and the error
  fn first_failure(bytes: &Arc<[u8]>) -> (usize, usize, SymphoniaError) {
    let Ok(mut opened) = Opened::new(bytes) else {
      panic!("the file opens");
    };
    loop {
      let packet = match opened.reader.next_packet() {
        Ok(Some(packet)) => packet,
        other => panic!("no packet failed: {other:?}"),
      };
      if let Err(error) = opened.decoder.decode(&packet) {
        let first = packet.pts.get() + packet.trim_start.get() as i64;
        return (first as usize, packet.dur.get() as usize, error);
      }
    }
  }

  /// A packet the codec cannot decode is skipped, its frames silence, and
  /// the stream plays on to its end: here an MP3 frame whose side
  /// information is all ones (its `big_values` over 288)
  #[test]
  fn a_packet_the_codec_cannot_decode_is_skipped() {
    let mut mp3 = fixture("tone.mp3").to_vec();
    // The seventh audio frame, after the ID3 tag (44 bytes), the Info frame
    // (182) and six frames of 104 or 105 bytes; its 9 bytes of side
    // information follow its 4-byte header
    mp3[853 + 4..853 + 13].fill(0xff);
    let bytes: Arc<[u8]> = Arc::from(mp3);
    let (failed, length, error) = first_failure(&bytes);
    assert!(matches!(error, SymphoniaError::DecodeError(_)), "{error}");
    let file = decoded("tone.mp3");
    let mut decoder = decoder_over("bad-frame.mp3", bytes);
    let frames = collect(&mut decoder, usize::MAX);
    assert_eq!(frames.len(), decoder.num_frames());
    assert_same("bad-frame.mp3", &frames[..failed], &file[..failed], 0);
    assert!(
      frames[failed..failed + length]
        .iter()
        .all(|frame| *frame == Frame::ZERO)
    );
    assert!(
      frames[failed + 2 * length..]
        .iter()
        .any(|frame| frame.left.abs() > 0.05)
    );
  }

  /// The codec's errors that are the packet's alone, as symphonia documents
  /// them (a decode error, and an IO error from reading past the packet's
  /// end), skip it; the rest end the stream
  #[test]
  fn a_codec_error_skips_only_its_packet() {
    let io = || SymphoniaError::IoError(std::io::Error::from(ErrorKind::UnexpectedEof));
    assert!(skips_packet(&SymphoniaError::DecodeError("bad")));
    assert!(skips_packet(&io()));
    assert!(!skips_packet(&SymphoniaError::Unsupported("feature")));
    assert!(!skips_packet(&SymphoniaError::LimitError("limit")));
    assert!(!skips_packet(&SymphoniaError::ResetRequired));
  }

  /// A packet whose timestamp jumps forward (here the page before it says it
  /// ends 10,000 frames later than it does) leaves silence up to it, given
  /// at most 4096 frames a call
  #[test]
  fn a_forward_jump_is_silence_in_chunks_of_at_most_4096() {
    let mut pages = ogg_pages(&fixture("tone-3s.ogg"));
    let mut granule = [0; 8];
    granule.copy_from_slice(&pages[3].header[6..14]);
    let ends = i64::from_le_bytes(granule);
    pages[3].header[6..14].copy_from_slice(&(ends + 10_000).to_le_bytes());
    let file = decoded("tone-3s.ogg");
    let mut decoder = decoder_over("jump.ogg", Arc::from(ogg_bytes(&pages)));
    let (mut frames, mut largest) = (Vec::new(), 0);
    loop {
      let chunk = decode(&mut decoder);
      if chunk.is_empty() {
        break;
      }
      largest = largest.max(chunk.len());
      frames.extend(chunk);
      assert!(
        frames.len() <= decoder.num_frames(),
        "{} frames",
        frames.len()
      );
    }
    assert_eq!(frames.len(), decoder.num_frames());
    assert!(largest <= 4096, "a chunk of {largest} frames");
    let at = ends as usize;
    assert_same("jump.ogg", &frames[..at], &file[..at], 0);
    assert!(
      frames[at..at + 10_000]
        .iter()
        .all(|frame| *frame == Frame::ZERO)
    );
    assert!(
      frames[at + 11_000..]
        .iter()
        .any(|frame| frame.left.abs() > 0.1)
    );
  }

  #[test]
  fn the_probe_refuses_more_than_two_channels() {
    let Err(error) = probe(&fixture("surround.wav")) else {
      panic!("six channels probed");
    };
    assert!(error.to_string().contains("6 channels"), "{error}");
  }

  /// The decoder checks the channels again as it opens (a clip made in code
  /// is not probed)
  #[test]
  fn a_surround_stream_is_an_error() {
    let mut decoder = ClipDecoder::new(fixture("surround.wav"), 22_050, 0..1_102);
    let Err(error) = decoder.decode() else {
      panic!("six channels decoded");
    };
    assert!(error.to_string().contains("6 channels"), "{error}");
  }
}
