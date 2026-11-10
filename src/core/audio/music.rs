//! [`Music`]: a music definition (`.music.ron`), and the state of the
//! engine's one music player

use std::fmt;
use std::time::Duration;

use kira::Frame;
use kira::sound::static_sound::{StaticSoundData, StaticSoundSettings};
use kira::sound::{EndPosition, PlaybackPosition, Region};

use super::voice::VoiceState;
use super::{AudioClip, ClipData, Voice};
use crate::core::asset::{Asset, AssetId, Handle};
use crate::core::hash::FastSet;

/// A music track, loaded from a `.music.ron` file
/// (`ctx.assets().load::<Music>("music/theme.music.ron")`, see
/// [`MusicLoader`](crate::core::asset::MusicLoader) for the file) or made in
/// code, and played by the engine's one music player with
/// `ctx.audio().play_music(&handle)`.
///
/// It is an intro that plays once and then a loop that repeats with no gap
/// ([`Music::intro_and_loop`]: the two are joined into one sound, so the
/// loop starts on the sample after the intro's last), or one clip looping
/// over a region ([`Music::looping`]: a long clip streams). It plays on the
/// `music` bus, or a bus under it, and belongs to the engine: it carries on
/// across scene changes until it is stopped, replaced, or a scene's
/// `SceneCleanup::MUSIC` stops it.
///
/// Saved while it plays (hot reload: its file, its clip, or its intro or
/// loop file), the track switches to what it now says where it is,
/// crossfading over 10 ms: its new clips, loop, volume, bus and crossfade.
/// Partway through its fade-in or a crossfade, it carries on with what is
/// left of it. A save that does not load changes nothing. Changed in code
/// (`Assets::get_mut`), the track playing it switches the same way, as a
/// save does: each change starts it again where it is, so change it when it
/// should switch, not every frame.
///
/// ```
/// use ryo2d::prelude::*;
///
/// struct Level {
///   theme: Handle<Music>,
/// }
///
/// impl Scene for Level {
///   fn on_enter(&mut self, ctx: &mut SceneContext) {
///     self.theme = ctx.assets().load("music/level.music.ron");
///     // Crossfades from the menu's track; nothing changes if this one
///     // already plays. A track still loading starts once it has loaded
///     ctx.audio().play_music(&self.theme);
///   }
/// }
/// ```
#[derive(Debug, Clone)]
pub struct Music {
  pub(crate) data: MusicData,
  /// Its gain in dB: 0 is the clips as recorded, -6 about half as loud
  pub volume_db: f32,
  /// How long it fades in when it starts with no track playing
  pub fade_in: Duration,
  /// How long it takes over from the track playing when it starts over one:
  /// that track fades out and this one fades in over this time. A scene's
  /// `SceneCleanup::MUSIC` fades it out over this too
  pub crossfade: Duration,
  /// Its tempo in beats per minute, kept for later: the player does not use
  /// it yet
  pub tempo: Option<f32>,
  /// Its meter as beats to the bar and the beat's note value (`(4, 4)`),
  /// kept for later: the player does not use it yet
  pub meter: Option<(u8, u8)>,
  /// The bus it plays on: `"music"` (the default) or one of the mixer's
  /// under it, such as `"music/combat"`. A bus the mixer does not have plays
  /// on the nearest bus above it, and one outside `music` on `music`, each
  /// with a warning
  pub bus: String,
}

impl Asset for Music {}

/// What a [`Music`] plays
#[derive(Clone)]
pub(crate) enum MusicData {
  /// An intro and its loop joined into one sound, whose loop region runs
  /// from the intro's end to the end
  Concatenated(Box<StaticSoundData>),
  /// One clip, looping over `loop_start..loop_end` in seconds (to its end
  /// when `None`); what comes before the loop plays once
  Looping {
    clip: Handle<AudioClip>,
    loop_start: f64,
    loop_end: Option<f64>,
  },
}

/// A summary: a joined sound's frames are not listed
impl fmt::Debug for MusicData {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      MusicData::Concatenated(data) => f
        .debug_struct("Concatenated")
        .field("sample_rate", &data.sample_rate)
        .field("frames", &data.frames.len())
        .field("loop_region", &data.settings.loop_region)
        .finish(),
      MusicData::Looping {
        clip,
        loop_start,
        loop_end,
      } => f
        .debug_struct("Looping")
        .field("clip", clip)
        .field("loop_start", loop_start)
        .field("loop_end", loop_end)
        .finish(),
    }
  }
}

/// Why [`Music::intro_and_loop`] cannot join an intro and a loop
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum MusicJoinError {
  /// The intro is streamed, so it has no frames to join
  #[error("the intro is streamed, so it has no frames to join (load it with `ClipMode::Decode`)")]
  StreamedIntro,
  /// The loop is streamed, so it has no frames to join
  #[error("the loop is streamed, so it has no frames to join (load it with `ClipMode::Decode`)")]
  StreamedLoop,
  /// They are at different sample rates, in Hz
  #[error("the intro is at {intro} Hz and the loop at {looped} Hz: they must share a sample rate")]
  Rates {
    /// The intro's
    intro: u32,
    /// The loop's
    looped: u32,
  },
  /// The loop has no frames, so nothing would repeat
  #[error("the loop has no frames")]
  EmptyLoop,
}

impl Music {
  /// The crossfade of a track that does not set one
  pub const DEFAULT_CROSSFADE: Duration = Duration::from_secs(1);

  /// `data` with the defaults: on `music` at 0 dB, no fade-in, the default
  /// crossfade, no tempo or meter
  pub(crate) fn with_data(data: MusicData) -> Music {
    Music {
      data,
      volume_db: 0.0,
      fade_in: Duration::ZERO,
      crossfade: Self::DEFAULT_CROSSFADE,
      tempo: None,
      meter: None,
      bus: "music".to_string(),
    }
  }

  /// `clip` looping from `loop_start` seconds to `loop_end` (its end when
  /// `None`), what comes before `loop_start` playing once as an intro. A
  /// clip loaded to stream streams as it plays. A start that is not a number
  /// of seconds from 0 loops from the clip's start, and an end that is not
  /// after the start loops to its end
  pub fn looping(clip: Handle<AudioClip>, loop_start: f64, loop_end: Option<f64>) -> Music {
    Self::with_data(MusicData::Looping {
      clip,
      loop_start,
      loop_end,
    })
  }

  /// `intro` playing once, then `looped` repeating, joined into one sound so
  /// the loop follows the intro with no gap and repeats from the intro's
  /// end. Both must be decoded clips (a streamed clip has no frames to join:
  /// load it with `ClipMode::Decode`) at the same sample rate, and the loop
  /// must not be empty; otherwise the error says which
  pub fn intro_and_loop(intro: &AudioClip, looped: &AudioClip) -> Result<Music, MusicJoinError> {
    let (ClipData::Decoded(intro), ClipData::Decoded(looped)) = (intro.data(), looped.data())
    else {
      return Err(if intro.is_streamed() {
        MusicJoinError::StreamedIntro
      } else {
        MusicJoinError::StreamedLoop
      });
    };
    let joined = join(intro, looped)?;
    Ok(Self::with_data(MusicData::Concatenated(Box::new(joined))))
  }

  pub(crate) fn data(&self) -> &MusicData {
    &self.data
  }
}

/// The frames `data` plays: its slice of them, if it has one
fn played(data: &StaticSoundData) -> &[Frame] {
  match data.slice {
    Some((start, end)) => data.frames.get(start..end).unwrap_or(&[]),
    None => &data.frames,
  }
}

/// `intro` then `looped` as one sound at their sample rate, its loop
/// region from the first frame after the intro's to the end: the loop starts
/// on the sample after the intro's last, and repeats from there. The frames
/// are copied once, into the joined sound
pub(crate) fn join(
  intro: &StaticSoundData,
  looped: &StaticSoundData,
) -> Result<StaticSoundData, MusicJoinError> {
  if intro.sample_rate != looped.sample_rate {
    return Err(MusicJoinError::Rates {
      intro: intro.sample_rate,
      looped: looped.sample_rate,
    });
  }
  let (intro, looped_frames) = (played(intro), played(looped));
  if looped_frames.is_empty() {
    return Err(MusicJoinError::EmptyLoop);
  }
  // A chain of slices knows its length: the `Arc` is allocated once and
  // filled in place
  let frames = intro.iter().chain(looped_frames).copied().collect();
  let settings = StaticSoundSettings::default().loop_region(Region {
    start: PlaybackPosition::Samples(intro.len()),
    end: EndPosition::EndOfAudio,
  });
  Ok(StaticSoundData {
    sample_rate: looped.sample_rate,
    frames,
    settings,
    slice: None,
  })
}

/// The engine's one music player: the track playing, and one asked for
/// while it loads, which starts once it has
#[derive(Default)]
pub(super) struct MusicPlayer {
  /// The track it started last, until it is stopped or replaced: playing or
  /// paused, or (stolen, say) fading out until its voice ends
  pub(super) current: Option<Track>,
  /// A track asked for while it loads: it takes over once loaded
  pub(super) pending: Option<Handle<Music>>,
  /// Tracks warned about once: one that did not load
  pub(super) warned: FastSet<AssetId<Music>>,
  /// Tracks warned about once: one made for a bus outside `music`
  pub(super) warned_buses: FastSet<AssetId<Music>>,
}

/// A track the player started
pub(super) struct Track {
  /// Held, so the definition outlives a game that let it go while it plays
  pub(super) music: Handle<Music>,
  pub(super) voice: Voice,
  /// Its crossfade, which a scene's cleanup fades it out over
  pub(super) crossfade: Duration,
  /// The definition's content version it plays: as it started, or as a
  /// reload last switched it
  pub(super) version: u64,
}

impl Track {
  /// Whether it is playing or paused: not fading out after a stop, nor over
  pub(super) fn is_live(&self) -> bool {
    matches!(self.voice.state(), VoiceState::Playing | VoiceState::Paused)
  }
}

impl MusicPlayer {
  /// The track playing or paused
  pub(super) fn live(&self) -> Option<&Track> {
    self.current.as_ref().filter(|track| track.is_live())
  }

  /// What plays now, or will once it loads: the track waiting to load, or
  /// else the one playing or paused
  pub(super) fn current_music(&self) -> Option<AssetId<Music>> {
    self
      .pending
      .as_ref()
      .map(Handle::id)
      .or_else(|| self.live().map(|track| track.music.id()))
  }

  /// Forget the warnings given once about `music`: its file changed, or it
  /// went
  pub(super) fn forget(&mut self, music: AssetId<Music>) {
    self.warned.remove(&music);
    self.warned_buses.remove(&music);
  }
}

#[cfg(test)]
mod tests {
  use std::time::{Duration, Instant};

  use kira::sound::PlaybackState;

  use super::*;
  use crate::core::asset::{AssetServer, Assets, AssetsView, MemorySource};
  use crate::core::audio::{
    AudioEngine, AudioLogEntry, AudioLogKind, AudioView, ClipBytes, EncodedClip, MusicJoinError,
  };
  use crate::core::ecs::World;
  use crate::core::testing::{audio_step, audio_world, audio_world_over};

  const FRAME: Duration = Duration::from_nanos(16_666_667);
  const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/audio");

  /// A world whose files are each definition in `definitions` (path and
  /// text) and `music/tone.wav`, the 250 ms tone at 22.05 kHz
  fn world_with(definitions: &[(&str, &str)]) -> World {
    let files = MemorySource::new();
    match std::fs::read(format!("{FIXTURES}/tone.wav")) {
      Ok(bytes) => files.insert("music/tone.wav", bytes),
      Err(error) => panic!("tone.wav: {error}"),
    }
    for (path, text) in definitions {
      files.insert(path, *text);
    }
    audio_world_over(files)
  }

  fn music(world: &mut World, path: &str) -> Handle<Music> {
    match AssetsView::new(world).load_now::<Music>(path) {
      Ok(music) => music,
      Err(error) => panic!("{path}: {}", error.report()),
    }
  }

  fn log(world: &World) -> Vec<AudioLogEntry> {
    world.resource::<AudioEngine>().log().to_vec()
  }

  /// What the log says happened to `path`'s voices: each entry's kind and
  /// fade, in order
  fn entries(world: &World, path: &str) -> Vec<(AudioLogKind, Duration)> {
    log(world)
      .iter()
      .filter(|entry| entry.sound == path)
      .map(|entry| (entry.kind, entry.fade))
      .collect()
  }

  fn voices_on_music(world: &mut World) -> usize {
    AudioView::new(world).voices_on("music")
  }

  fn current(world: &mut World) -> Option<AssetId<Music>> {
    AudioView::new(world).current_music()
  }

  fn steps(world: &mut World, count: usize) {
    for _ in 0..count {
      audio_step(world, FRAME);
    }
  }

  /// Where the music's one voice is in what it plays, in seconds
  fn position(world: &World) -> f64 {
    let engine = world.resource::<AudioEngine>();
    let Some(voice) = engine
      .log()
      .iter()
      .rev()
      .find(|entry| entry.kind == AudioLogKind::Start)
    else {
      panic!("nothing started");
    };
    match engine.voice_position(voice.voice) {
      Some(position) => position,
      None => panic!("voice {} is gone", voice.voice),
    }
  }

  #[test]
  fn play_music_crossfades_from_the_current_track() {
    let mut world = world_with(&[
      (
        "music/a.music.ron",
        r#"(clip: "music/tone.wav", crossfade: 0.5)"#,
      ),
      (
        "music/b.music.ron",
        r#"(clip: "music/tone.wav", crossfade: 0.2)"#,
      ),
    ]);
    let (a, b) = (
      music(&mut world, "music/a.music.ron"),
      music(&mut world, "music/b.music.ron"),
    );
    AudioView::new(&mut world).play_music(&a);
    assert_eq!(current(&mut world), Some(a.id()));
    steps(&mut world, 3);
    AudioView::new(&mut world).play_music(&b);
    assert_eq!(current(&mut world), Some(b.id()));
    let crossfade = Duration::from_millis(200);
    assert_eq!(
      entries(&world, "music/a.music.ron"),
      [
        (AudioLogKind::Start, Duration::ZERO),
        (AudioLogKind::Stop, crossfade)
      ],
      "the old track fades out over the new one's crossfade"
    );
    assert_eq!(
      entries(&world, "music/b.music.ron"),
      [(AudioLogKind::Start, crossfade)],
      "and the new one fades in over it"
    );
    let start = log(&world)
      .into_iter()
      .find(|entry| entry.sound == "music/b.music.ron" && entry.kind == AudioLogKind::Start);
    assert_eq!(start.map(|entry| entry.bus).as_deref(), Some("music"));
    // They overlap for the crossfade
    audio_step(&mut world, Duration::from_millis(100));
    assert_eq!(voices_on_music(&mut world), 2);
    audio_step(&mut world, Duration::from_millis(150));
    assert_eq!(voices_on_music(&mut world), 1, "{:#?}", log(&world));
  }

  /// A switch keeps the music at its level: halfway through a 1 s
  /// crossfade, the two tracks' summed power is within 1.5 dB of one
  /// track's. Each track plays in one channel, so the meter reads each
  #[test]
  fn a_crossfade_holds_its_level_at_the_midpoint() {
    let (mut world, meter) = crate::core::testing::metered_audio_world();
    let mut track = |frame: [f32; 2]| {
      let frames = vec![frame; 4 * 48_000];
      let clip = world
        .resource_mut::<Assets<AudioClip>>()
        .add(AudioClip::from_frames(48_000, &frames).unwrap());
      world
        .resource_mut::<Assets<Music>>()
        .add(Music::looping(clip, 0.0, None))
    };
    let (left, right) = (track([0.1, 0.0]), track([0.0, 0.1]));
    AudioView::new(&mut world).play_music(&left);
    steps(&mut world, 30);
    let alone = meter.last().left;
    assert!(alone > 0.01, "the first track is heard: {alone}");
    AudioView::new(&mut world).play_music(&right);
    // Half of the crossfade, to within a chunk
    steps(&mut world, 30);
    let mid = meter.last();
    let power = mid.left * mid.left + mid.right * mid.right;
    let db = 10.0 * (power / (alone * alone)).log10();
    assert!(
      db.abs() <= 1.5,
      "{db:.2} dB at the midpoint (left {}, right {})",
      mid.left,
      mid.right
    );
  }

  #[test]
  fn playing_the_track_already_playing_does_nothing() {
    let mut world = world_with(&[("music/a.music.ron", r#"(clip: "music/tone.wav")"#)]);
    let a = music(&mut world, "music/a.music.ron");
    AudioView::new(&mut world).play_music(&a);
    steps(&mut world, 3);
    for _ in 0..3 {
      AudioView::new(&mut world).play_music(&a);
      audio_step(&mut world, FRAME);
    }
    assert_eq!(
      entries(&world, "music/a.music.ron"),
      [(AudioLogKind::Start, Duration::ZERO)]
    );
    assert_eq!(voices_on_music(&mut world), 1);
    assert_eq!(current(&mut world), Some(a.id()));
  }

  /// Dropping a kira handle never stops its sound: `stop_music` stops it,
  /// with its fade, and kira lets it go
  #[test]
  fn stop_music_really_stops() {
    let mut world = world_with(&[("music/a.music.ron", r#"(clip: "music/tone.wav")"#)]);
    let a = music(&mut world, "music/a.music.ron");
    AudioView::new(&mut world).play_music(&a);
    steps(&mut world, 3);
    AudioView::new(&mut world).stop_music(Duration::from_millis(100));
    assert_eq!(current(&mut world), None);
    assert_eq!(
      entries(&world, "music/a.music.ron"),
      [
        (AudioLogKind::Start, Duration::ZERO),
        (AudioLogKind::Stop, Duration::from_millis(100))
      ]
    );
    audio_step(&mut world, Duration::from_millis(50));
    assert_eq!(voices_on_music(&mut world), 1, "still fading out");
    assert_eq!(world.resource::<AudioEngine>().sounds_on("music"), 1);
    audio_step(&mut world, Duration::from_millis(80));
    audio_step(&mut world, FRAME);
    assert_eq!(voices_on_music(&mut world), 0);
    assert_eq!(
      world.resource::<AudioEngine>().sounds_on("music"),
      0,
      "kira's music track holds no sound"
    );
    // A stop with no track playing does nothing
    AudioView::new(&mut world).stop_music(Duration::ZERO);
    audio_step(&mut world, FRAME);
    assert_eq!(entries(&world, "music/a.music.ron").len(), 2);
  }

  /// A clip with a loop region plays from its start, and then repeats only
  /// the region: past the tone's end (250 ms), it is back inside the loop
  /// from 0.1 s, not at the clip's start
  #[test]
  fn a_one_clip_loop_region_is_kira_s_loop_region() {
    let mut world = world_with(&[(
      "music/b.music.ron",
      r#"(clip: "music/tone.wav", loop_start: 0.1)"#,
    )]);
    let b = music(&mut world, "music/b.music.ron");
    AudioView::new(&mut world).play_music(&b);
    // 18 frames: 112 of kira's chunks, 298.7 ms
    steps(&mut world, 18);
    let at = position(&world);
    assert!(
      (0.13..0.17).contains(&at),
      "0.1 s plus the 48.7 ms past the end, not 48.7 ms from the start: {at}"
    );
    assert_eq!(current(&mut world), Some(b.id()));
  }

  /// An intro and loop joined: the intro plays once and the loop repeats
  /// from the intro's end
  #[test]
  fn a_joined_track_repeats_its_loop_not_its_intro() {
    let mut world = audio_world();
    // A 200 ms intro and a 100 ms loop at 48 kHz
    let intro = AudioClip::from_frames(48_000, &vec![[0.1, 0.1]; 9_600]).unwrap();
    let looped = AudioClip::from_frames(48_000, &vec![[0.2, 0.2]; 4_800]).unwrap();
    let track = match Music::intro_and_loop(&intro, &looped) {
      Ok(track) => track,
      Err(error) => panic!("{error}"),
    };
    let track = world.resource_mut::<Assets<Music>>().add(track);
    AudioView::new(&mut world).play_music(&track);
    // 21 frames: 131 chunks, 349.3 ms
    steps(&mut world, 21);
    let at = position(&world);
    assert!(
      (0.24..0.26).contains(&at),
      "in the loop's second pass, 49.3 ms in: {at}"
    );
  }

  #[test]
  fn intro_and_loop_joins_decoded_clips() {
    let clip =
      |rate: u32, frames: usize| AudioClip::from_frames(rate, &vec![[0.1, 0.1]; frames]).unwrap();
    let joined = Music::intro_and_loop(&clip(48_000, 10), &clip(48_000, 20));
    let Ok(joined) = joined else {
      panic!("{joined:?}");
    };
    let MusicData::Concatenated(data) = joined.data() else {
      panic!("{joined:?}");
    };
    assert_eq!(data.frames.len(), 30);
  }

  #[test]
  fn pause_and_resume_music() {
    let mut world = world_with(&[("music/a.music.ron", r#"(clip: "music/tone.wav")"#)]);
    let a = music(&mut world, "music/a.music.ron");
    AudioView::new(&mut world).play_music(&a);
    steps(&mut world, 2);
    AudioView::new(&mut world).pause_music();
    steps(&mut world, 2);
    let paused = |world: &World| {
      let engine = world.resource::<AudioEngine>();
      let start = engine
        .log()
        .iter()
        .find(|entry| entry.kind == AudioLogKind::Start);
      start.and_then(|entry| engine.voice_playback(entry.voice))
    };
    assert_eq!(paused(&world), Some(PlaybackState::Paused));
    assert_eq!(
      current(&mut world),
      Some(a.id()),
      "a paused track is current"
    );
    AudioView::new(&mut world).resume_music();
    steps(&mut world, 2);
    assert_eq!(paused(&world), Some(PlaybackState::Playing));
    let kinds: Vec<AudioLogKind> = entries(&world, "music/a.music.ron")
      .into_iter()
      .map(|(kind, _)| kind)
      .collect();
    assert_eq!(
      kinds,
      [
        AudioLogKind::Start,
        AudioLogKind::Pause,
        AudioLogKind::Resume
      ]
    );
    // Playing a paused track resumes it
    AudioView::new(&mut world).pause_music();
    audio_step(&mut world, FRAME);
    AudioView::new(&mut world).play_music(&a);
    steps(&mut world, 2);
    assert_eq!(paused(&world), Some(PlaybackState::Playing));
    assert_eq!(voices_on_music(&mut world), 1);
  }

  /// A track asked for while it loads takes over once it has loaded
  #[test]
  fn a_track_still_loading_starts_once_it_has_loaded() {
    let mut world = world_with(&[
      (
        "music/a.music.ron",
        r#"(clip: "music/tone.wav", crossfade: 0.1)"#,
      ),
      (
        "music/late.music.ron",
        r#"(clip: "music/tone.wav", crossfade: 0.3)"#,
      ),
    ]);
    let a = music(&mut world, "music/a.music.ron");
    AudioView::new(&mut world).play_music(&a);
    audio_step(&mut world, FRAME);
    let late = AssetsView::new(&mut world).load::<Music>("music/late.music.ron");
    AudioView::new(&mut world).play_music(&late);
    assert_eq!(
      current(&mut world),
      Some(late.id()),
      "it is what plays next"
    );
    assert_eq!(entries(&world, "music/a.music.ron").len(), 1, "a plays on");
    let started = Instant::now();
    while entries(&world, "music/late.music.ron").is_empty() {
      assert!(
        started.elapsed() < Duration::from_secs(5),
        "it never started"
      );
      audio_step(&mut world, FRAME);
      std::thread::sleep(Duration::from_millis(1));
    }
    let crossfade = Duration::from_millis(300);
    assert_eq!(
      entries(&world, "music/late.music.ron"),
      [(AudioLogKind::Start, crossfade)]
    );
    assert_eq!(
      entries(&world, "music/a.music.ron").last(),
      Some(&(AudioLogKind::Stop, crossfade))
    );
    assert_eq!(current(&mut world), Some(late.id()));
  }

  /// A track that did not load is warned about once, and what plays carries
  /// on
  #[test]
  fn a_track_that_failed_warns_once_and_leaves_the_music_alone() {
    let captured = crate::core::testing::capture_log();
    let mut world = world_with(&[
      ("music/a.music.ron", r#"(clip: "music/tone.wav")"#),
      (
        "music/broken-track.music.ron",
        r#"(clip: "music/missing.wav")"#,
      ),
    ]);
    let a = music(&mut world, "music/a.music.ron");
    AudioView::new(&mut world).play_music(&a);
    let broken = AssetsView::new(&mut world).load::<Music>("music/broken-track.music.ron");
    let started = Instant::now();
    while !world
      .resource::<crate::core::asset::AssetServer>()
      .state(&broken)
      .is_failed()
    {
      assert!(
        started.elapsed() < Duration::from_secs(5),
        "it never failed"
      );
      audio_step(&mut world, FRAME);
      std::thread::sleep(Duration::from_millis(1));
    }
    for _ in 0..3 {
      AudioView::new(&mut world).play_music(&broken);
      audio_step(&mut world, FRAME);
    }
    assert_eq!(current(&mut world), Some(a.id()));
    assert_eq!(entries(&world, "music/a.music.ron").len(), 1);
    let warned = captured
      .records()
      .iter()
      .filter(|entry| {
        entry.level == log::Level::Warn
          && entry.message.starts_with("music/broken-track.music.ron:")
          && entry.message.contains("did not load")
      })
      .count();
    assert_eq!(warned, 1);
  }

  /// Music plays on `music` or under it: a track made in code for another
  /// bus plays on `music`, with a warning
  #[test]
  fn a_track_for_a_bus_outside_music_plays_on_music() {
    let mut world = audio_world();
    let clip = world
      .resource_mut::<Assets<AudioClip>>()
      .add(AudioClip::from_frames(48_000, &vec![[0.1, 0.1]; 4_800]).unwrap());
    let mut track = Music::looping(clip, 0.0, None);
    track.bus = "sfx".to_string();
    let track = world.resource_mut::<Assets<Music>>().add(track);
    AudioView::new(&mut world).play_music(&track);
    assert_eq!(voices_on_music(&mut world), 1);
    assert_eq!(AudioView::new(&mut world).voices_on("sfx"), 0);
  }

  /// A rebuilt bus tree (a saved mixer) restarts a joined track on the new
  /// music bus where it was, crossfading over 10 ms, and it stays the
  /// player's track
  #[test]
  fn a_joined_track_moves_to_rebuilt_buses_where_it_was() {
    use crate::core::audio::Mixer;
    use crate::core::audio::engine::MIN_FADE;
    let mut world = audio_world();
    let intro = AudioClip::from_frames(48_000, &vec![[0.1, 0.1]; 9_600]).unwrap();
    let looped = AudioClip::from_frames(48_000, &vec![[0.2, 0.2]; 4_800]).unwrap();
    let track = match Music::intro_and_loop(&intro, &looped) {
      Ok(track) => track,
      Err(error) => panic!("{error}"),
    };
    let track = world.resource_mut::<Assets<Music>>().add(track);
    AudioView::new(&mut world).play_music(&track);
    steps(&mut world, 6);
    let before = position(&world);
    let voice = log(&world)[0].voice;
    AudioView::new(&mut world).set_mixer(Mixer::default());
    audio_step(&mut world, Duration::ZERO);
    let kinds: Vec<(AudioLogKind, Duration)> = log(&world)
      .iter()
      .filter(|entry| entry.voice == voice)
      .map(|entry| (entry.kind, entry.fade))
      .collect();
    assert_eq!(
      kinds,
      [
        (AudioLogKind::Start, Duration::ZERO),
        (AudioLogKind::Stop, MIN_FADE),
        (AudioLogKind::Start, MIN_FADE)
      ]
    );
    assert_eq!(current(&mut world), Some(track.id()));
    assert!(
      (position(&world) - before).abs() < 0.01,
      "{before} then {}",
      position(&world)
    );
    assert_eq!(world.resource::<AudioEngine>().sounds_on("music"), 1);
  }

  /// Review M1: a play that is not music never takes the music's place,
  /// even a priority-255 sound under a limit on `master`, which counts the
  /// music too
  #[test]
  fn a_sound_never_steals_the_music() {
    use crate::core::audio::{LimitRule, Sound, SoundClip, VoiceLimit};
    let mut world = audio_world();
    let clip = world
      .resource_mut::<Assets<AudioClip>>()
      .add(AudioClip::from_frames(48_000, &vec![[0.1, 0.1]; 48_000]).unwrap());
    let track = world
      .resource_mut::<Assets<Music>>()
      .add(Music::looping(clip.clone(), 0.0, None));
    let loud = world.resource_mut::<Assets<Sound>>().add(Sound {
      clips: vec![SoundClip::new(clip)],
      looping: true,
      priority: u8::MAX,
      limit: Some(VoiceLimit {
        max: 8,
        rule: LimitRule::StopOldest,
      }),
      ..Default::default()
    });
    world
      .resource_mut::<AudioEngine>()
      .set_bus_limit("master", Some(2));
    AudioView::new(&mut world).play_music(&track);
    audio_step(&mut world, FRAME);
    let first = AudioView::new(&mut world).play(&loud);
    audio_step(&mut world, FRAME);
    let second = AudioView::new(&mut world).play(&loud);
    assert!(second.is_playing(), "{second:?}");
    let music = log(&world)[0].voice;
    let stopped: Vec<u32> = log(&world)
      .iter()
      .filter(|entry| matches!(entry.kind, AudioLogKind::Stolen | AudioLogKind::Stop))
      .map(|entry| entry.voice)
      .collect();
    assert_eq!(
      stopped,
      [first.id()],
      "the oldest sound gives way, not the music {music}"
    );
    assert_eq!(current(&mut world), Some(track.id()));
  }

  /// When only the music could give way, a one-shot that is not music is
  /// refused, however important, and a loop goes virtual and stays so: the
  /// track plays on
  #[test]
  fn a_play_whose_only_victim_is_the_music_is_refused() {
    use crate::core::audio::{Sound, SoundClip};
    let mut world = audio_world();
    let clip = world
      .resource_mut::<Assets<AudioClip>>()
      .add(AudioClip::from_frames(48_000, &vec![[0.1, 0.1]; 96_000]).unwrap());
    let track = world
      .resource_mut::<Assets<Music>>()
      .add(Music::looping(clip.clone(), 0.0, None));
    let loud = world.resource_mut::<Assets<Sound>>().add(Sound {
      clips: vec![SoundClip::new(clip.clone())],
      priority: u8::MAX,
      ..Default::default()
    });
    let bed = world.resource_mut::<Assets<Sound>>().add(Sound {
      clips: vec![SoundClip::new(clip)],
      looping: true,
      priority: u8::MAX,
      ..Default::default()
    });
    world
      .resource_mut::<AudioEngine>()
      .set_bus_limit("master", Some(1));
    AudioView::new(&mut world).play_music(&track);
    steps(&mut world, 2);
    let refused = AudioView::new(&mut world).play(&loud);
    assert!(refused.is_stopped(), "{refused:?}");
    let entries: Vec<(AudioLogKind, &str)> = log(&world)
      .iter()
      .map(|entry| (entry.kind, entry.reason))
      .collect();
    let [(AudioLogKind::Start, _), (AudioLogKind::Refused, reason)] = entries[..] else {
      panic!("the music started and the play was refused: {entries:?}");
    };
    assert!(reason.starts_with("limit"), "{reason}");
    steps(&mut world, 2);
    assert_eq!(current(&mut world), Some(track.id()));
    assert_eq!(voices_on_music(&mut world), 1, "the track plays on");
    let kept = AudioView::new(&mut world).play(&bed);
    assert!(kept.is_playing() && kept.is_virtual(), "{kept:?}");
    steps(&mut world, 30);
    assert!(kept.is_virtual(), "it never takes the music's place");
    assert_eq!(current(&mut world), Some(track.id()));
    assert_eq!(voices_on_music(&mut world), 1, "the track plays on");
  }

  /// Review M2 (ruling P14): a track that did not load changes nothing: the
  /// track playing plays on, and one still loading still takes over
  #[test]
  fn a_failed_track_keeps_the_track_still_loading() {
    let mut world = world_with(&[
      ("music/a.music.ron", r#"(clip: "music/tone.wav")"#),
      ("music/next.music.ron", r#"(clip: "music/tone.wav")"#),
      (
        "music/broken-next.music.ron",
        r#"(clip: "music/missing.wav")"#,
      ),
    ]);
    let a = music(&mut world, "music/a.music.ron");
    AudioView::new(&mut world).play_music(&a);
    let broken = AssetsView::new(&mut world).load::<Music>("music/broken-next.music.ron");
    wait_for(&mut world, |world| {
      world.resource::<AssetServer>().state(&broken).is_failed()
    });
    let next = AssetsView::new(&mut world).load::<Music>("music/next.music.ron");
    AudioView::new(&mut world).play_music(&next);
    AudioView::new(&mut world).play_music(&broken);
    assert_eq!(current(&mut world), Some(next.id()));
    wait_for(&mut world, |world| {
      !entries(world, "music/next.music.ron").is_empty()
    });
    assert_eq!(current(&mut world), Some(next.id()));
  }

  /// Review M3: a track that cannot start (its stream already failed to
  /// decode) leaves the track playing alone
  #[cfg(not(target_arch = "wasm32"))]
  #[test]
  fn a_refused_track_leaves_the_old_one_playing() {
    let mut world = world_with(&[("music/a.music.ron", r#"(clip: "music/tone.wav")"#)]);
    let garbage = ClipBytes::from(&b"not audio at all"[..]);
    let encoded = EncodedClip {
      bytes: garbage,
      frames: 22_050,
    };
    let bad = AudioClip::new(ClipData::Encoded(encoded), Duration::from_secs(1), 22_050);
    let bad = world.resource_mut::<Assets<AudioClip>>().add(bad);
    let broken = world
      .resource_mut::<Assets<Music>>()
      .add(Music::looping(bad, 0.0, None));
    // Its stream fails on its decoder thread the first time, which marks
    // its clip, and the track ends
    AudioView::new(&mut world).play_music(&broken);
    let started = Instant::now();
    while current(&mut world).is_some() {
      assert!(started.elapsed() < Duration::from_secs(5), "the track ends");
      audio_step(&mut world, FRAME);
      std::thread::sleep(Duration::from_millis(1));
    }
    let a = music(&mut world, "music/a.music.ron");
    AudioView::new(&mut world).play_music(&a);
    audio_step(&mut world, FRAME);
    AudioView::new(&mut world).play_music(&broken);
    audio_step(&mut world, FRAME);
    assert_eq!(
      entries(&world, "music/a.music.ron"),
      [(AudioLogKind::Start, Duration::ZERO)],
      "the old track was never stopped"
    );
    assert_eq!(current(&mut world), Some(a.id()));
    assert_eq!(voices_on_music(&mut world), 1);
    let refused = log(&world)
      .iter()
      .filter(|entry| entry.kind == AudioLogKind::Refused)
      .count();
    assert_eq!(refused, 1);
  }

  /// Review M5: a newer request replaces a track still loading, which then
  /// never starts; a stop forgets one still loading
  #[test]
  fn a_newer_request_or_a_stop_replaces_a_track_still_loading() {
    let mut world = world_with(&[
      ("music/a.music.ron", r#"(clip: "music/tone.wav")"#),
      ("music/first.music.ron", r#"(clip: "music/tone.wav")"#),
      ("music/second.music.ron", r#"(clip: "music/tone.wav")"#),
      ("music/stopped.music.ron", r#"(clip: "music/tone.wav")"#),
    ]);
    let a = music(&mut world, "music/a.music.ron");
    AudioView::new(&mut world).play_music(&a);
    let first = AssetsView::new(&mut world).load::<Music>("music/first.music.ron");
    AudioView::new(&mut world).play_music(&first);
    let second = AssetsView::new(&mut world).load::<Music>("music/second.music.ron");
    AudioView::new(&mut world).play_music(&second);
    assert_eq!(current(&mut world), Some(second.id()));
    wait_for(&mut world, |world| {
      !entries(world, "music/second.music.ron").is_empty()
        && world.resource::<AssetServer>().is_ready(&first)
    });
    steps(&mut world, 3);
    assert_eq!(entries(&world, "music/first.music.ron"), []);
    let stopped = AssetsView::new(&mut world).load::<Music>("music/stopped.music.ron");
    AudioView::new(&mut world).play_music(&stopped);
    AudioView::new(&mut world).stop_music(Duration::ZERO);
    assert_eq!(current(&mut world), None);
    wait_for(&mut world, |world| {
      world.resource::<AssetServer>().is_ready(&stopped)
    });
    steps(&mut world, 3);
    assert_eq!(entries(&world, "music/stopped.music.ron"), []);
    assert_eq!(current(&mut world), None);
  }

  /// Review M4: why an intro and a loop cannot be joined is a typed error
  #[test]
  fn a_join_that_fails_says_why_in_its_error() {
    let clip =
      |rate: u32, frames: usize| AudioClip::from_frames(rate, &vec![[0.1, 0.1]; frames]).unwrap();
    let encoded = EncodedClip {
      bytes: ClipBytes::from(&b"bytes"[..]),
      frames: 48_000,
    };
    let streamed = AudioClip::new(ClipData::Encoded(encoded), Duration::from_secs(1), 48_000);
    let error = |result: Result<Music, MusicJoinError>| match result {
      Ok(music) => panic!("joined: {music:?}"),
      Err(error) => error,
    };
    assert_eq!(
      error(Music::intro_and_loop(&streamed, &clip(48_000, 10))),
      MusicJoinError::StreamedIntro
    );
    assert_eq!(
      error(Music::intro_and_loop(&clip(48_000, 10), &streamed)),
      MusicJoinError::StreamedLoop
    );
    let rates = error(Music::intro_and_loop(&clip(48_000, 10), &clip(44_100, 10)));
    assert_eq!(
      rates,
      MusicJoinError::Rates {
        intro: 48_000,
        looped: 44_100
      }
    );
    assert!(rates.to_string().contains("44100"), "{rates}");
    assert_eq!(
      error(Music::intro_and_loop(&clip(48_000, 10), &clip(48_000, 0))),
      MusicJoinError::EmptyLoop
    );
  }

  /// Step until `done`, which a load on the workers makes true
  fn wait_for(world: &mut World, done: impl Fn(&World) -> bool) {
    let started = Instant::now();
    while !done(world) {
      assert!(started.elapsed() < Duration::from_secs(5), "timed out");
      audio_step(world, FRAME);
      std::thread::sleep(Duration::from_millis(1));
    }
  }
}
