//! The audio engine against its targets, in an audio world
//! (`testing::audio_world`) on kira's mock backend: 48 kHz, 128-frame
//! chunks, rendered on the main thread inside `audio_system`, where a
//! device renders on its own thread. A frame's main-thread audio time is
//! the wall time of `audio_system` less the time the mock spent rendering
//! (`AudioEngine::mock_render_time`). Each frame advances the clock by a
//! 60 Hz frame and ends with the change trackers cleared, as the engine's
//! `Last` clears them; the audio log is off, as in a game.
//!
//! Before criterion runs, three guards (the first printing an idle frame's
//! audio time beside it, with no target):
//! - `voices_64`: 64 looping voices, 32 of them heard from emitters
//!   circling the listener, each with a distance low-pass (the most that
//!   carry one), moved every frame so that each one's gain, pan and cutoff
//!   change every frame; the other 32 plain loops on `sfx` and `ambience`.
//!   Over 2,000 frames paced at 60 Hz, the median main-thread time of
//!   `audio_system` is held to 50 µs (the spec);
//! - `play_cost`: 10,000 `play` calls of a decoded 50 ms clip, eight a
//!   frame (so about sixteen voices play), each timed alone; the median is
//!   held to 5 µs (the spec). The median play of a sound definition (three
//!   clips, a volume and pitch spread, no repeats) is printed beside it,
//!   with no target;
//! - `load_hitch`: three files played while they reload, read from a
//!   `MemorySource`: a 1 s WAV (decoded) as a loop, a 48 s OGG (streamed)
//!   as a loop, and a 46 to 48 s MP3 (streamed) as music. Over 2,000 frames
//!   at 60 Hz each is saved over with its other version every 50 frames
//!   (the three a third of that apart) and reloaded: a worker loads it,
//!   and the pump swaps it in some frames later, in which frame
//!   `audio_system` switches what plays it to the new data where it is (a
//!   stream is opened anew and seeked there, on the main thread). A frame's
//!   time here is the pump's and `audio_system`'s. Each file's median
//!   landing frame is held to within 0.5 ms of an ordinary frame (the
//!   spec), as are a stream's landings 20 s or more into it (mid-file,
//!   where a seek could cost most). The OGG is made by `ffmpeg` from
//!   breakout's music as the guard starts; without `ffmpeg` it is left out,
//!   with a line saying so. The MP3s are breakout's two tracks. No stream
//!   reaches its end in the run (34 s): kira 0.12.5 spins a Vorbis stream's
//!   decoder thread at the end of the file once the stream was started
//!   partway in, as a reload starts it (`docs/backlog/audio.md`). The guard
//!   also prints how long decoding a long file (a 30 s OGG, the MP3) takes
//!   in one job, as the web does in its asset pump (it has no streaming, so
//!   a long clip there is a hitch of that length), with no target.
//!
//! A miss fails the run once criterion's benches have run, printing the
//! command to profile it with. Each guard runs only where criterion
//! measures the bench of its name: with no filter or one naming it, and not
//! for `--list`, `--test` or `--profile-time`. The criterion benches, for
//! the profiles: `voices_64` times `audio_system` over the same 64 voices
//! back to back, `play_cost` one play of the clip, and `load_hitch` the
//! frame each reload of the OGG lands in (the MP3's without `ffmpeg`),
//! from 20 s in, at 60 Hz (keep `--profile-time` under 25 s, or the stream
//! reaches its end).
//!
//! Medians are read from where criterion writes its results:
//! `$CRITERION_HOME`, else `$CARGO_TARGET_DIR/criterion`, else the
//! workspace's `target/criterion`.

mod guards;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime};

use criterion::Criterion;
use ryo2d::core::asset::pump;
use ryo2d::core::audio::{AudioEngine, AudioLogKind, audio_system};
use ryo2d::prelude::*;
use ryo2d::testing::{audio_world, audio_world_over};

/// A 60 Hz frame: the clock's step, and the guards' pace
const FRAME: Duration = Duration::from_nanos(16_666_667);
/// The guards' frames
const FRAMES: usize = 2000;
/// The spec's frame with 64 voices
const VOICES_TARGET: Duration = Duration::from_micros(50);
/// The spec's play
const PLAY_TARGET: Duration = Duration::from_micros(5);
/// The spec's load or reload: a landing frame within this of an ordinary one
const HITCH_TARGET: Duration = Duration::from_micros(500);
/// The plays `play_cost` times
const PLAYS: usize = 10_000;
/// Plays a frame
const PLAYS_A_FRAME: usize = 8;
/// Frames between saves of each file
const RELOAD_EVERY: usize = 50;
/// A stream landing this far in or further is mid-file
const MID_FILE: f64 = 20.0;
/// The criterion benches whose medians are printed, with what each median
/// is of
const BENCHES: [(&str, &str); 3] = [
  (
    "voices_64",
    "a frame's audio_system less the mock's rendering, back to back",
  ),
  ("play_cost", "a play of a decoded clip"),
  (
    "load_hitch",
    "the pump and audio_system of a frame a reload of the OGG (else the MP3) lands in",
  ),
];

fn us(time: Duration) -> f64 {
  time.as_secs_f64() * 1e6
}

/// The median, 95th percentile and maximum of `times`
fn spread(mut times: Vec<Duration>) -> Option<(Duration, Duration, Duration)> {
  times.sort_unstable();
  let median = *times.get(times.len() / 2)?;
  let p95 = times[times.len() * 95 / 100];
  Some((median, p95, *times.last()?))
}

fn median(times: Vec<Duration>) -> Option<Duration> {
  spread(times).map(|(median, _, _)| median)
}

/// Wait, spinning, until `next`: a thread that sleeps most of each frame
/// runs its frames on a slower core
fn pace(next: Instant) {
  while Instant::now() < next {
    std::hint::spin_loop();
  }
}

/// An audio world as a game's runs: the audio log off, seeded
fn quiet(mut world: World) -> World {
  world.resource_mut::<AudioEngine>().set_logging(false);
  AudioView::new(&mut world).seed(5);
  world
}

/// Start a frame: the clock advances a 60 Hz frame
fn tick(world: &mut World) {
  world.resource_mut::<Time>().advance(FRAME);
}

/// The frame's `audio_system`: its main-thread time, less the mock's
/// rendering, and the mock's rendering
fn timed_audio(world: &mut World) -> (Duration, Duration) {
  let started = Instant::now();
  audio_system(world);
  let wall = started.elapsed();
  let mock = world.resource::<AudioEngine>().mock_render_time();
  (wall.saturating_sub(mock), mock)
}

/// End a frame, as the engine's `Last` does
fn end(world: &mut World) {
  world.clear_trackers();
}

/// An untimed frame: the clock, the pump and the audio
fn step(world: &mut World) {
  tick(world);
  pump(world);
  audio_system(world);
  end(world);
}

/// `seconds` of stereo noise at 48 kHz, quiet, from `seed`
fn noise(seconds: f32, seed: u32) -> Vec<[f32; 2]> {
  let mut state = seed.wrapping_mul(747_796_405).wrapping_add(1);
  (0..(48_000.0 * seconds) as usize)
    .map(|_| {
      state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
      let left = (state >> 8) as f32 / (1u32 << 24) as f32 - 0.5;
      let right = (state.rotate_left(13) >> 8) as f32 / (1u32 << 24) as f32 - 0.5;
      [left * 0.2, right * 0.2]
    })
    .collect()
}

fn add_clip(world: &mut World, seconds: f32, seed: u32) -> Handle<AudioClip> {
  world
    .resource_mut::<Assets<AudioClip>>()
    .add(AudioClip::from_frames(48_000, &noise(seconds, seed)))
}

// voices_64

/// The voices' world: 32 plain loops playing, and 32 emitters about to
fn voices_world() -> (World, Vec<Entity>) {
  let mut world = quiet(audio_world());
  world.spawn((AudioListener, Transform::default()));
  let clip = add_clip(&mut world, 2.0, 1);
  let mut sounds = world.resource_mut::<Assets<Sound>>();
  let plain = ["sfx", "ambience"].map(|bus| {
    sounds.add(Sound {
      clips: vec![SoundClip::new(clip.clone())],
      bus: bus.to_string(),
      looping: true,
      ..Default::default()
    })
  });
  let placed = sounds.add(Sound {
    clips: vec![SoundClip::new(clip.clone())],
    looping: true,
    spatial: Some(Spatial {
      min_distance: 50.0,
      max_distance: 900.0,
      curve: Falloff::Inverse,
      pan_width: 1000.0,
      low_pass_at_max: Some(800.0),
    }),
    ..Default::default()
  });
  for i in 0..32 {
    AudioView::new(&mut world).play(&plain[i % 2]);
  }
  let emitters = (0..32)
    .map(|i| {
      let emitter = AudioEmitter {
        sound: placed.clone(),
        play_on_spawn: true,
      };
      world
        .spawn((emitter, Transform::from_xy(100.0 + i as f32, 0.0)))
        .id()
    })
    .collect();
  (world, emitters)
}

/// Move each emitter along its own circle about the listener, its radius
/// swinging between 80 and 750 px, nearer and further by turns (10% of its
/// radius, and 30 px across), so that every frame changes each one's gain,
/// pan and cutoff by more than the least change sent
fn circle(world: &mut World, emitters: &[Entity], frame: usize) {
  let t = frame as f32 * FRAME.as_secs_f32();
  let side = if frame.is_multiple_of(2) { 1.0 } else { -1.0 };
  for (i, &entity) in emitters.iter().enumerate() {
    let phase = i as f32 * 0.37;
    let radius = (415.0 + 335.0 * (t * 0.9 + phase).sin()) * (1.0 + 0.1 * side);
    let angle = t * (0.5 + i as f32 * 0.03) + phase * 3.0;
    // Out from the listener on the far turns, in on the near ones
    let x = radius * angle.cos() + 30.0 * side * angle.cos().signum();
    let y = radius * angle.sin();
    let place = Transform::from_xy(x, y);
    if let Some(mut global) = world.get_mut::<GlobalTransform>(entity) {
      *global = GlobalTransform::from_transform(&place);
    }
  }
}

/// An idle frame (no voices) at 60 Hz: the median main-thread time of
/// `audio_system`, and of the mock's rendering, which a headless engine's
/// frame also runs on its main thread (no target: the idle frame's is the
/// whole frame's, in `benches/ui_render.rs` and `benches/foundation.rs`)
fn idle_line() {
  let mut world = quiet(audio_world());
  for _ in 0..120 {
    step(&mut world);
  }
  let (mut times, mut mocks) = (Vec::with_capacity(FRAMES), Vec::with_capacity(FRAMES));
  let mut next = Instant::now();
  for _ in 0..FRAMES {
    pace(next);
    next = Instant::now() + FRAME;
    tick(&mut world);
    pump(&mut world);
    let (time, mock) = timed_audio(&mut world);
    end(&mut world);
    times.push(time);
    mocks.push(mock);
  }
  if let (Some(time), Some(mock)) = (median(times), median(mocks)) {
    eprintln!(
      "idle audio frame (no voices), {FRAMES} frames at 60 Hz: audio_system's main-thread time, \
       median {:.2} µs; the mock's rendering, median {:.2} µs (no target)",
      us(time),
      us(mock)
    );
  }
}

/// The 64 voices' frames at 60 Hz: the median main-thread time of
/// `audio_system`, held to the spec's 50 µs
fn voices_guard() {
  let (mut world, emitters) = voices_world();
  for frame in 0..120 {
    circle(&mut world, &emitters, frame);
    step(&mut world);
  }
  let playing = AudioView::new(&mut world).voices_on("master");
  // What a frame changes, read from the audio log for one frame
  world.resource_mut::<AudioEngine>().set_logging(true);
  circle(&mut world, &emitters, 120);
  step(&mut world);
  let log = world.resource::<AudioEngine>().log();
  let changed = |kind: AudioLogKind| log.iter().filter(|entry| entry.kind == kind).count();
  let (gains, pans) = (changed(AudioLogKind::Gain), changed(AudioLogKind::Pan));
  world.resource_mut::<AudioEngine>().set_logging(false);
  let mut times = Vec::with_capacity(FRAMES);
  let mut mocks = Vec::with_capacity(FRAMES);
  let mut next = Instant::now();
  for frame in 121..121 + FRAMES {
    pace(next);
    next = Instant::now() + FRAME;
    tick(&mut world);
    pump(&mut world);
    circle(&mut world, &emitters, frame);
    let (time, mock) = timed_audio(&mut world);
    end(&mut world);
    times.push(time);
    mocks.push(mock);
  }
  let still = AudioView::new(&mut world).voices_on("master");
  let (Some((median, p95, max)), Some(mock)) = (spread(times), median(mocks)) else {
    return;
  };
  let met = median <= VOICES_TARGET;
  eprintln!(
    "voices_64 guard, {FRAMES} frames at 60 Hz, {playing} voices playing ({still} at the \
     end), 32 heard from moving emitters with a low-pass (a frame changes {gains} gains and \
     {pans} pans): audio_system's main-thread time, \
     median {:.1} µs, 95th percentile {:.1}, max {:.1} (the mock's rendering, left out: median \
     {:.1} µs); within {} µs: {}",
    us(median),
    us(p95),
    us(max),
    us(mock),
    VOICES_TARGET.as_micros(),
    if met { "met" } else { "missed" },
  );
  if !met {
    guards::miss(format!("voices_64: median {:.1} µs", us(median)));
  }
  if gains < 32 || pans < 32 {
    guards::miss(format!(
      "voices_64: a frame changed {gains} gains and {pans} pans, not each emitter's"
    ));
  }
  if playing != 64 || still != 64 {
    guards::miss(format!(
      "voices_64: {playing} voices played, {still} at the end, not 64"
    ));
  }
}

/// The 64 voices back to back; the world is made once, on the bench's
/// first call (criterion calls it for each sample)
fn voices_bench(c: &mut Criterion) {
  let mut made = None;
  c.bench_function("voices_64", |b| {
    let (world, emitters, frame) = made.get_or_insert_with(|| {
      let (mut world, emitters) = voices_world();
      for frame in 0..120 {
        circle(&mut world, &emitters, frame);
        step(&mut world);
      }
      (world, emitters, 120)
    });
    b.iter_custom(|iters| {
      let mut total = Duration::ZERO;
      for _ in 0..iters {
        tick(world);
        pump(world);
        circle(world, emitters, *frame);
        total += timed_audio(world).0;
        end(world);
        *frame += 1;
      }
      total
    });
  });
}

// play_cost

/// A world with the 50 ms clip to play, and a definition of three clips
fn play_world() -> (World, Handle<AudioClip>, Handle<Sound>) {
  let mut world = quiet(audio_world());
  let clip = add_clip(&mut world, 0.05, 2);
  let clips = (3..6)
    .map(|seed| SoundClip::new(add_clip(&mut world, 0.05, seed)))
    .collect();
  let sound = world.resource_mut::<Assets<Sound>>().add(Sound {
    clips,
    volume_range_db: 2.0,
    pitch_range: 1.0,
    no_repeat: 1,
    ..Default::default()
  });
  (world, clip, sound)
}

/// `PLAYS` plays (`play`), eight a frame, each timed alone; with the plays
/// refused
fn plays(world: &mut World, play: impl Fn(&mut World) -> Voice) -> (Vec<Duration>, usize) {
  let mut times = Vec::with_capacity(PLAYS);
  let mut refused = 0;
  for i in 0..PLAYS {
    let started = Instant::now();
    let voice = play(world);
    times.push(started.elapsed());
    refused += usize::from(voice.is_stopped());
    drop(voice);
    if (i + 1) % PLAYS_A_FRAME == 0 {
      step(world);
    }
  }
  (times, refused)
}

/// The plays: each one's median, held to the spec's 5 µs for the clip
fn play_guard() {
  let (mut world, clip, sound) = play_world();
  let clip = |world: &mut World| AudioView::new(world).play(&clip);
  // Warm: kira's tracks have held a frame's worth of sounds
  plays(&mut world, clip);
  let (times, refused) = plays(&mut world, clip);
  let voices = AudioView::new(&mut world).voices_on("master");
  let (sound_times, sound_refused) = plays(&mut world, |world: &mut World| {
    AudioView::new(world).play(&sound)
  });
  let (Some((median, p95, max)), Some(by_sound)) = (spread(times), median(sound_times)) else {
    return;
  };
  let met = median <= PLAY_TARGET;
  eprintln!(
    "play_cost guard, {PLAYS} plays of a decoded 50 ms clip, {PLAYS_A_FRAME} a frame ({voices} \
     voices playing after the last frame): median {:.2} µs, 95th percentile {:.2}, max {:.1}; \
     within {} µs: {}. A definition of three clips (spread, no repeats): median {:.2} µs",
    us(median),
    us(p95),
    us(max),
    PLAY_TARGET.as_micros(),
    if met { "met" } else { "missed" },
    us(by_sound),
  );
  if !met {
    guards::miss(format!("play_cost: median {:.2} µs", us(median)));
  }
  if refused + sound_refused > 0 {
    guards::miss(format!(
      "play_cost: {refused} plays of the clip and {sound_refused} of the definition refused"
    ));
  }
}

/// One play of the clip at a time, eight a frame; the world is made once,
/// on the bench's first call
fn play_bench(c: &mut Criterion) {
  let mut made = None;
  c.bench_function("play_cost", |b| {
    let (world, clip, played) = made.get_or_insert_with(|| {
      let (mut world, clip, _) = play_world();
      plays(&mut world, |world: &mut World| {
        AudioView::new(world).play(&clip)
      });
      (world, clip, 0usize)
    });
    b.iter_custom(|iters| {
      let mut total = Duration::ZERO;
      for _ in 0..iters {
        let started = Instant::now();
        let voice = AudioView::new(world).play(&*clip);
        total += started.elapsed();
        drop(voice);
        *played += 1;
        if played.is_multiple_of(PLAYS_A_FRAME) {
          step(world);
        }
      }
      total
    });
  });
}

// load_hitch

/// A mono 16-bit WAV at 48 kHz, `seconds` long: a tone of `hz`
fn wav(seconds: f32, hz: f32) -> Vec<u8> {
  let frames = (48_000.0 * seconds) as usize;
  let data = (frames * 2) as u32;
  let mut bytes = Vec::with_capacity(44 + frames * 2);
  bytes.extend_from_slice(b"RIFF");
  bytes.extend_from_slice(&(36 + data).to_le_bytes());
  bytes.extend_from_slice(b"WAVEfmt ");
  bytes.extend_from_slice(&16u32.to_le_bytes());
  // PCM, one channel, 48 kHz, two bytes a frame, 16 bits a sample
  for field in [1u16, 1] {
    bytes.extend_from_slice(&field.to_le_bytes());
  }
  bytes.extend_from_slice(&48_000u32.to_le_bytes());
  bytes.extend_from_slice(&96_000u32.to_le_bytes());
  for field in [2u16, 16] {
    bytes.extend_from_slice(&field.to_le_bytes());
  }
  bytes.extend_from_slice(b"data");
  bytes.extend_from_slice(&data.to_le_bytes());
  for frame in 0..frames {
    let t = frame as f32 / 48_000.0;
    let sample = (t * hz * std::f32::consts::TAU).sin() * 6_000.0;
    bytes.extend_from_slice(&(sample as i16).to_le_bytes());
  }
  bytes
}

/// `path` loaded at once
fn load_now<T: Asset>(world: &mut World, path: &str) -> Handle<T> {
  let server = world.resource::<AssetServer>().clone();
  server
    .load_now::<T>(world, path)
    .unwrap_or_else(|error| panic!("{path}: {}", error.report()))
}

/// Breakout's music, the source of the MP3s and the OGG
fn breakout_track(name: &str) -> Vec<u8> {
  let path = Path::new(env!("CARGO_MANIFEST_DIR"))
    .join("examples/games/breakout/assets/audio")
    .join(name);
  std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// Breakout's music as Ogg Vorbis, made by `ffmpeg` (its own Vorbis
/// encoder, about 160 kbps): all of it (48 s), or its first `seconds`, at
/// 90% of its level when `quieter` (a second version to save over the
/// first); none without `ffmpeg`
fn ogg(seconds: Option<u32>, quieter: bool) -> Option<Vec<u8>> {
  let dir = tempfile::TempDir::new().ok()?;
  let out = dir.path().join("music.ogg");
  let source =
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/games/breakout/assets/audio/bgm.mp3");
  let mut ffmpeg = Command::new("ffmpeg");
  ffmpeg.args(["-v", "error", "-y", "-i"]).arg(&source);
  if let Some(seconds) = seconds {
    ffmpeg.args(["-t", &seconds.to_string()]);
  }
  if quieter {
    ffmpeg.args(["-af", "volume=0.9"]);
  }
  let made = ffmpeg
    .args(["-ac", "2", "-c:a", "vorbis", "-strict", "-2"])
    .arg(&out)
    .status()
    .ok()?;
  if !made.success() {
    return None;
  }
  std::fs::read(&out).ok()
}

/// One file the load guard plays and saves over
struct LoadFile {
  name: &'static str,
  path: &'static str,
  /// Its two versions, saved in turn
  versions: [Vec<u8>; 2],
  /// It is saved in the frames this far into each `RELOAD_EVERY`
  offset: usize,
  stream: bool,
}

/// What the load guard plays, and what the web's decode is measured on
struct Fixtures {
  load: Vec<LoadFile>,
  /// The first 30 s of the music as Ogg Vorbis
  ogg_30: Option<Vec<u8>>,
}

/// The files, with the OGGs when `ffmpeg` made them
fn fixtures() -> Fixtures {
  let mut files = vec![LoadFile {
    name: "the 1 s WAV",
    path: "sfx/hum.wav",
    versions: [wav(1.0, 220.0), wav(1.0, 330.0)],
    offset: 0,
    stream: false,
  }];
  match (ogg(None, false), ogg(None, true)) {
    (Some(first), Some(second)) => files.push(LoadFile {
      name: "the 48 s OGG",
      path: "ambience/rain.ogg",
      versions: [first, second],
      offset: RELOAD_EVERY / 3,
      stream: true,
    }),
    _ => eprintln!(
      "load_hitch guard: ffmpeg could not make the OGG (is it installed?): the OGG is not \
       measured"
    ),
  }
  files.push(LoadFile {
    name: "the MP3",
    path: "music/theme.mp3",
    versions: [
      breakout_track("bgm.mp3"),
      breakout_track("bgm-main-menu.mp3"),
    ],
    offset: RELOAD_EVERY * 2 / 3,
    stream: true,
  });
  Fixtures {
    load: files,
    ogg_30: ogg(Some(30), false),
  }
}

/// The world playing the files: the WAV and the OGG as loops, the MP3 as
/// music; and each file's clip
struct LoadRig {
  files: MemorySource,
  world: World,
  clips: Vec<Handle<AudioClip>>,
  /// Saves of each file so far
  saves: Vec<usize>,
  /// What plays them
  _sounds: Vec<Handle<Sound>>,
  _music: Option<Handle<Music>>,
}

impl LoadRig {
  fn new(load: &[LoadFile]) -> LoadRig {
    let files = MemorySource::new();
    for file in load {
      files.insert(file.path, file.versions[0].clone());
    }
    files.insert(
      "sfx/hum.sound.ron",
      r#"(clips: [(path: "sfx/hum.wav")], looping: true)"#,
    );
    files.insert(
      "ambience/rain.sound.ron",
      r#"(clips: [(path: "ambience/rain.ogg")], looping: true, bus: "ambience")"#,
    );
    files.insert("music/theme.music.ron", r#"(clip: "music/theme.mp3")"#);
    let mut world = quiet(audio_world_over(files.clone()));
    let clips = load
      .iter()
      .map(|file| {
        let clip = load_now::<AudioClip>(&mut world, file.path);
        let streamed = world
          .resource::<Assets<AudioClip>>()
          .get(&clip)
          .is_some_and(AudioClip::is_streamed);
        assert_eq!(streamed, file.stream, "{} streams or not", file.path);
        clip
      })
      .collect();
    let (mut sounds, mut music) = (Vec::new(), None);
    for file in load {
      if file.path == "music/theme.mp3" {
        let theme = load_now::<Music>(&mut world, "music/theme.music.ron");
        AudioView::new(&mut world).play_music(&theme);
        music = Some(theme);
      } else {
        let (stem, _) = file.path.rsplit_once('.').unwrap_or((file.path, ""));
        let path = format!("{stem}.sound.ron");
        let sound = load_now::<Sound>(&mut world, &path);
        // A voice let go plays on
        AudioView::new(&mut world).play(&sound);
        sounds.push(sound);
      }
    }
    LoadRig {
      files,
      world,
      clips,
      saves: vec![0; load.len()],
      _sounds: sounds,
      _music: music,
    }
  }

  /// Save file `index`'s other version over it, and reload it
  fn save(&mut self, load: &[LoadFile], index: usize) {
    self.saves[index] += 1;
    let file = &load[index];
    self
      .files
      .insert(file.path, file.versions[self.saves[index] % 2].clone());
    self.world.resource::<AssetServer>().reload(file.path);
  }

  fn versions(&self) -> Vec<Option<u64>> {
    let clips = self.world.resource::<Assets<AudioClip>>();
    self.clips.iter().map(|clip| clips.version(clip)).collect()
  }

  fn duration(&self, index: usize) -> f64 {
    self
      .world
      .resource::<Assets<AudioClip>>()
      .get(&self.clips[index])
      .map_or(f64::MAX, |clip| clip.duration().as_secs_f64())
  }

  /// A timed frame: the pump's time and `audio_system`'s (less the mock's
  /// rendering), and which files' new versions landed in it
  fn frame(&mut self) -> LoadFrame {
    let before = self.versions();
    tick(&mut self.world);
    let started = Instant::now();
    pump(&mut self.world);
    let pumped = started.elapsed();
    let (audio, _) = timed_audio(&mut self.world);
    end(&mut self.world);
    let landed = self
      .versions()
      .iter()
      .zip(&before)
      .map(|(now, was)| now != was)
      .collect();
    LoadFrame {
      pump: pumped,
      audio,
      landed,
    }
  }
}

struct LoadFrame {
  pump: Duration,
  audio: Duration,
  landed: Vec<bool>,
}

impl LoadFrame {
  fn time(&self) -> Duration {
    self.pump + self.audio
  }
}

/// The reloads' frames at 60 Hz: each file's median landing frame (and a
/// stream's mid-file landings) held to within 0.5 ms of an ordinary frame
fn load_guard(load: &[LoadFile]) {
  let mut rig = LoadRig::new(load);
  // Warm: the streams have started, and played a second
  for _ in 0..60 {
    step(&mut rig.world);
  }
  let warm = 60;
  let mut frames = Vec::with_capacity(FRAMES);
  let mut next = Instant::now();
  for frame in 0..FRAMES {
    for index in 0..load.len() {
      let at = frame % RELOAD_EVERY == load[index].offset;
      if frame > 0 && at {
        rig.save(load, index);
      }
    }
    pace(next);
    next = Instant::now() + FRAME;
    frames.push(rig.frame());
  }
  let playing = AudioView::new(&mut rig.world).voices_on("master");
  let ordinary_frames: Vec<&LoadFrame> = frames
    .iter()
    .filter(|frame| !frame.landed.contains(&true))
    .collect();
  let Some((ordinary, ordinary_p95, ordinary_max)) =
    spread(ordinary_frames.iter().map(|frame| frame.time()).collect())
  else {
    guards::miss("load_hitch: no ordinary frame".into());
    return;
  };
  let over = |time: Duration| us(time) - us(ordinary);
  let held = |time: Duration| time.saturating_sub(ordinary) <= HITCH_TARGET;
  let mut lines = Vec::new();
  for (index, file) in load.iter().enumerate() {
    let landings: Vec<(usize, &LoadFrame)> = frames
      .iter()
      .enumerate()
      .filter(|(_, frame)| frame.landed[index])
      .collect();
    let saves = rig.saves[index];
    if landings.len() + 1 < saves {
      guards::miss(format!(
        "load_hitch: of {saves} saves of {} {} landed",
        file.name,
        landings.len()
      ));
    }
    let Some((landing, _, landing_max)) =
      spread(landings.iter().map(|(_, frame)| frame.time()).collect())
    else {
      continue;
    };
    let pump = median(landings.iter().map(|(_, frame)| frame.pump).collect()).unwrap_or_default();
    let audio = median(landings.iter().map(|(_, frame)| frame.audio).collect()).unwrap_or_default();
    let met = held(landing);
    let mut line = format!(
      "{}: {} landings of {saves} saves, the landing frame median {:.1} µs ({:+.1}; pump \
       {:.1}, audio {:.1}), max {:+.1}; within {} µs: {}",
      file.name,
      landings.len(),
      us(landing),
      over(landing),
      us(pump),
      us(audio),
      over(landing_max),
      HITCH_TARGET.as_micros(),
      if met { "met" } else { "missed" },
    );
    if !met {
      guards::miss(format!("load_hitch, {line}"));
    }
    if file.stream {
      // Where in the file each landing came: the stream has played since
      // the warm-up began, looping at its end
      let duration = rig.duration(index);
      let mid: Vec<Duration> = landings
        .iter()
        .filter(|(at, _)| ((warm + at) as f64 * FRAME.as_secs_f64()) % duration >= MID_FILE)
        .map(|(_, frame)| frame.time())
        .collect();
      let count = mid.len();
      match spread(mid) {
        Some((mid, _, mid_max)) => {
          let met = held(mid);
          let part = format!(
            "; mid-file ({MID_FILE} s or more in, {count} of them): median {:+.1} µs, max \
             {:+.1}: {}",
            over(mid),
            over(mid_max),
            if met { "met" } else { "missed" },
          );
          if !met {
            guards::miss(format!("load_hitch, {}{part}", file.name));
          }
          line.push_str(&part);
        }
        None => guards::miss(format!("load_hitch: {} never landed mid-file", file.name)),
      }
    }
    lines.push(line);
  }
  eprintln!(
    "load_hitch guard, the pump and audio_system's main-thread time (less the mock's \
     rendering), {FRAMES} frames at 60 Hz, each file saved and reloaded every {RELOAD_EVERY}th \
     ({playing} voices playing at the end). Ordinary frames ({} of them): median {:.1} µs, 95th \
     percentile {:+.1}, max {:+.1}. {}",
    ordinary_frames.len(),
    us(ordinary),
    over(ordinary_p95),
    over(ordinary_max),
    lines.join("; "),
  );
  if playing != load.len() {
    guards::miss(format!(
      "load_hitch: {playing} voices playing at the end, not {}",
      load.len()
    ));
  }
}

/// How long decoding a long file takes in one job (`ClipMode::Decode`), as
/// the web's asset pump does it: the 30 s OGG and the 48 s MP3, the median
/// of five each
fn web_decode(fixtures: &Fixtures) {
  let mut long: Vec<(&str, &str, &[u8])> = Vec::new();
  if let Some(ogg) = &fixtures.ogg_30 {
    long.push(("the 30 s OGG", "music/a.ogg", ogg));
  }
  if let Some(mp3) = fixtures
    .load
    .iter()
    .find(|file| file.path.ends_with(".mp3"))
  {
    long.push(("the MP3", mp3.path, &mp3.versions[0]));
  }
  let mut lines = Vec::new();
  for (name, path, bytes) in long {
    let times = (0..5)
      .map(|_| {
        let files = MemorySource::new();
        files.insert(path, bytes.to_vec());
        let mut world = audio_world_over(files);
        let settings = AudioClipSettings {
          mode: ClipMode::Decode,
        };
        let server = world.resource::<AssetServer>().clone();
        let started = Instant::now();
        let clip = server.load_now_with::<AudioClip, _>(&mut world, path, settings);
        let time = started.elapsed();
        let clip = clip.unwrap_or_else(|error| panic!("{path}: {}", error.report()));
        let seconds = world
          .resource::<Assets<AudioClip>>()
          .get(&clip)
          .map_or(0.0, |clip| clip.duration().as_secs_f64());
        (time, seconds)
      })
      .collect::<Vec<_>>();
    let seconds = times.first().map_or(0.0, |(_, seconds)| *seconds);
    if let Some(time) = median(times.into_iter().map(|(time, _)| time).collect()) {
      lines.push(format!(
        "{name} ({seconds:.1} s) {:.1} ms",
        time.as_secs_f64() * 1e3
      ));
    }
  }
  eprintln!(
    "web_decode, decoding a long file in one job as the web's asset pump does (no target; \
     natively, median of five): {}",
    lines.join(", ")
  );
}

/// The load bench: the rig is made once, on the bench's first call (not
/// for a filter that skips it), and its frames paced at 60 Hz as the
/// guard's are: stepped back to back, the mock plays the streams hundreds
/// of times faster than their decoders' threads can keep up
fn load_bench(c: &mut Criterion, fixtures: &mut Option<Fixtures>) {
  let mut made: Option<(LoadRig, usize)> = None;
  c.bench_function("load_hitch", |b| {
    let (rig, file) = made.get_or_insert_with(|| {
      let load = &fixtures.get_or_insert_with(self::fixtures).load;
      let mut rig = LoadRig::new(load);
      // The OGG, whose landing costs most; the MP3 without it
      let file = load
        .iter()
        .position(|file| file.path.ends_with(".ogg"))
        .unwrap_or(load.len() - 1);
      // Mid-file: 20 s in
      let mut next = Instant::now();
      for _ in 0..(MID_FILE / FRAME.as_secs_f64()) as usize {
        pace(next);
        next = Instant::now() + FRAME;
        step(&mut rig.world);
      }
      (rig, file)
    });
    let file = *file;
    let load = fixtures
      .as_ref()
      .map_or(&[][..], |fixtures| &fixtures.load[..]);
    b.iter_custom(|iters| {
      let mut total = Duration::ZERO;
      let mut next = Instant::now();
      for _ in 0..iters {
        rig.save(load, file);
        let started = Instant::now();
        loop {
          pace(next);
          next = Instant::now() + FRAME;
          let frame = rig.frame();
          if frame.landed[file] {
            total += frame.time();
            break;
          }
          assert!(
            started.elapsed() < Duration::from_secs(5),
            "the reload landed"
          );
        }
      }
      total
    });
  });
}

/// Where criterion writes its results, and the median is read from
fn results_dir() -> PathBuf {
  if let Some(home) = std::env::var_os("CRITERION_HOME") {
    return PathBuf::from(home);
  }
  std::env::var_os("CARGO_TARGET_DIR")
    .map(PathBuf::from)
    .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("target"))
    .join("criterion")
}

fn main() {
  let started = SystemTime::now();
  let load = Command::new("uptime")
    .output()
    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    .unwrap_or_else(|_| "uptime unavailable".into());
  println!("machine: {load}");
  println!("audio bench: 64 voices, a play and the load hitch");
  let results = results_dir();
  let mut criterion = Criterion::default()
    .sample_size(20)
    .output_directory(&results)
    .configure_from_args();
  if guards::runs(&["voices_64"]) {
    idle_line();
    voices_guard();
  }
  if guards::runs(&["play_cost"]) {
    play_guard();
  }
  // Made only for what runs: the files of a filter that skips them all are
  // not
  let mut made = None;
  if guards::runs(&["load_hitch"]) {
    let fixtures = made.get_or_insert_with(fixtures);
    load_guard(&fixtures.load);
    web_decode(fixtures);
  }
  voices_bench(&mut criterion);
  play_bench(&mut criterion);
  load_bench(&mut criterion, &mut made);
  criterion.final_summary();
  for (bench, of) in BENCHES {
    print_median(&results, bench, of, started);
  }
  guards::verdict(
    "audio",
    "Profile a miss (the bench binary is the one `cargo bench --bench audio --no-run` names; \
     voices_64, play_cost or load_hitch) with\n  xcrun xctrace record --template 'Time \
     Profiler' --output audio.trace --launch -- target/release/deps/audio-<hash> --bench \
     --profile-time 20 '^load_hitch$'\nand charge each sample's time to the nearest ryo2d \
     function from the leaf up (kira's and symphonia's under the ryo2d call that reached \
     them).",
  );
}

/// The median of `bench` (of `of`) from the run criterion just wrote to
/// `results`, in microseconds; nothing for results an earlier run left
/// (this run skipped the bench, as a filter or `--list` does)
fn print_median(results: &Path, bench: &str, of: &str, started: SystemTime) {
  let path = results.join(bench).join("new/estimates.json");
  let written = std::fs::metadata(&path)
    .and_then(|meta| meta.modified())
    .is_ok_and(|modified| modified >= started);
  if !written {
    println!("{bench} median: not measured in this run");
    return;
  }
  let median = std::fs::read_to_string(&path).ok().and_then(|text| {
    let at = text.find("\"median\"")?;
    let rest = &text[at..];
    let from = rest.find("\"point_estimate\":")? + "\"point_estimate\":".len();
    let to = rest[from..].find([',', '}'])? + from;
    rest[from..to].trim().parse::<f64>().ok()
  });
  match median {
    Some(ns) => println!("{bench} median, of {of}: {:.2} us", ns / 1000.0),
    None => println!("{bench} median: unavailable ({})", path.display()),
  }
}
