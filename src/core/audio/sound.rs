//! [`Sound`]: a sound definition (`.sound.ron`). It says how one sound
//! plays: the clips it picks from and how, its bus, level and pitch, how many
//! of its voices may play at once, its priority and cooldown, looping and
//! fades, and where in the world it is heard from

use std::ops::Range;
use std::time::Duration;

use serde::Deserialize;

use super::AudioClip;
use crate::core::asset::{Asset, Handle};

/// A sound definition: how one sound plays, loaded from a `.sound.ron` file
/// (`ctx.assets().load::<Sound>("sfx/step.sound.ron")`, see
/// [`SoundLoader`](crate::core::asset::SoundLoader) for the file) or made in
/// code, and played with `ctx.audio().play(&handle)`.
///
/// Each play picks one of its clips by weight, avoiding the latest picks
/// (`no_repeat`), draws its gain and pitch from their ranges, and starts on
/// its bus, unless its cooldown or a voice limit refuses it. A refused play
/// returns a voice that is already stopped. A sound is ready once every clip
/// is loaded.
///
/// Saved while it plays (hot reload), it applies to the plays after; its
/// loops playing take the change in its volume and pitch at once (each
/// keeps the spread it drew, and a volume or pitch the game set on its
/// [`Voice`](super::Voice) stays on top, relative to the sound's level).
/// Saved onto another bus, its loops move there where they are,
/// crossfading over 10 ms, and its one-shots finish where they are. A save
/// that does not load changes nothing.
///
/// ```
/// use std::time::Duration;
/// use ryo2d::prelude::*;
///
/// fn footstep(clips: &[Handle<AudioClip>]) -> Sound {
///   Sound {
///     clips: clips.iter().cloned().map(SoundClip::new).collect(),
///     volume_db: -6.0,
///     pitch_range: 1.0,
///     no_repeat: 1,
///     limit: Some(VoiceLimit { max: 4, rule: LimitRule::StopOldest }),
///     cooldown: Duration::from_millis(30),
///     ..Default::default()
///   }
/// }
/// ```
#[derive(Debug, Clone)]
pub struct Sound {
  /// The clips a play picks from. A sound with none plays nothing
  pub clips: Vec<SoundClip>,
  /// The bus it plays on: `"sfx"` (the default), another standard bus
  /// (`"music"`, `"ui"`, `"voice"`, `"ambience"`, or `"master"`), or one of
  /// the mixer's under them, such as `"sfx/footsteps"`. A bus the mixer
  /// does not have plays on the nearest bus above it, with a warning
  pub bus: String,
  /// Its gain in dB: 0 is the clips as recorded, -6 about half as loud
  pub volume_db: f32,
  /// Each play's gain is drawn up to this many dB either side of
  /// `volume_db`
  pub volume_range_db: f32,
  /// Its pitch in semitones: 12 is an octave up, and twice as fast
  pub pitch: f32,
  /// Each play's pitch is drawn up to this many semitones either side of
  /// `pitch`
  pub pitch_range: f32,
  /// How many of the latest picks a play avoids, so a variation does not
  /// repeat: 1 never plays the same clip twice in a row. A sound always has
  /// one clip left to pick, so it avoids at most one fewer than its clips
  pub no_repeat: u8,
  /// How many of its voices play at once, and what a play past that does.
  /// A loop past it goes virtual instead of stopping or being refused
  /// ([`Voice::is_virtual`](super::Voice::is_virtual)), and comes back when
  /// there is room
  pub limit: Option<VoiceLimit>,
  /// How important its voices are, from 0 to 255 (higher is more
  /// important; [`Sound::DEFAULT_PRIORITY`] by default). A voice is never
  /// stopped to make room for a less important one, nor for an equally
  /// important one started in the same frame. A virtual loop comes back in
  /// place of a less important voice, or of an equally important one 3 dB
  /// quieter
  pub priority: u8,
  /// The least time between two of its plays: a play sooner is refused
  pub cooldown: Duration,
  /// The most of its voices that start in one frame, counted across every
  /// bus and place it plays at (1 by default, and at least 1). Copies
  /// started together play in step and add up, ten of them 20 dB over one,
  /// so a burst (a volley of shots in one frame) is heard as one start and
  /// the plays past it that frame are refused. Plays that waited for it to
  /// load count as they land, so a loop played twice while it loads starts
  /// once, and a voice stopped in the frame it started is not counted.
  /// Raise it for a sound meant to layer in one frame
  pub per_frame: u8,
  /// It loops until stopped: over each clip's `loop_region`, or the whole
  /// clip (its `region`). A loop played while it is still loading waits for
  /// it, and starts once it is ready. A loop a voice limit has no room for,
  /// or out of range, goes virtual: it keeps its place silently and comes
  /// back when it can be heard ([`Voice::is_virtual`](super::Voice::is_virtual))
  pub looping: bool,
  /// Each play fades in over this
  pub fade_in: Duration,
  /// Its fade when the engine stops it: to make room under a voice limit,
  /// or when the scene that played it exits (a loop, a paused one-shot, or
  /// a one-shot with more than 2 s left, but on `ui`). At least 10 ms
  pub fade_out: Duration,
  /// Played at a place in the world (`ctx.audio().play_at(..)`, or from an
  /// [`AudioEmitter`](super::AudioEmitter)), it is heard from there:
  /// quieter with distance from the listener, panned by its offset, and
  /// muffled with distance if it has a distance low-pass. A one-shot out of
  /// range is culled, and a loop is virtual. Without it, a sound is heard
  /// as if at the listener wherever it plays
  pub spatial: Option<Spatial>,
}

impl Sound {
  /// The priority of a sound that does not set one, and of a clip played
  /// directly: the middle of 0 to 255
  pub const DEFAULT_PRIORITY: u8 = 128;

  /// How many of its voices start in one frame when it does not say, and a
  /// clip played directly
  pub const DEFAULT_PER_FRAME: u8 = 1;
}

impl Asset for Sound {}

/// The empty definition: no clips, on `sfx`, at 0 dB and pitch 0 with no
/// ranges, no repeat avoidance, no limit, the default priority, no cooldown,
/// one start a frame, not looping and no fades. A clip played directly
/// plays with it
impl Default for Sound {
  fn default() -> Self {
    Self {
      clips: Vec::new(),
      bus: "sfx".to_string(),
      volume_db: 0.0,
      volume_range_db: 0.0,
      pitch: 0.0,
      pitch_range: 0.0,
      no_repeat: 0,
      limit: None,
      priority: Self::DEFAULT_PRIORITY,
      cooldown: Duration::ZERO,
      per_frame: Self::DEFAULT_PER_FRAME,
      looping: false,
      fade_in: Duration::ZERO,
      fade_out: Duration::ZERO,
      spatial: None,
    }
  }
}

/// One of a [`Sound`]'s clips, with how often it is picked and what part of
/// it plays
#[derive(Debug, Clone)]
pub struct SoundClip {
  pub clip: Handle<AudioClip>,
  /// How often it is picked against the others: 2 is twice as often as 1,
  /// and 0 never
  pub weight: f32,
  /// The part of the clip that plays, in seconds from its start; all of it
  /// when `None`
  pub region: Option<Range<f64>>,
  /// The part a looping sound repeats, in seconds from the start of
  /// `region`; the whole region when `None`. Before it, the region plays
  /// once, as an intro
  pub loop_region: Option<Range<f64>>,
}

impl SoundClip {
  /// `clip`, all of it, at weight 1
  pub fn new(clip: Handle<AudioClip>) -> Self {
    Self {
      clip,
      weight: 1.0,
      region: None,
      loop_region: None,
    }
  }
}

/// How many voices of a sound play at once, and what a play past that does
/// (`limit: (max: 4, rule: StopOldest)` in a `.sound.ron` file; the rule is
/// `StopOldest` when left out)
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceLimit {
  /// The most voices playing at once (a voice stopped is no longer
  /// counted, even one whose stop was asked for in the frame of the play:
  /// a play applies the stops queued before it)
  pub max: u16,
  #[serde(default)]
  pub rule: LimitRule,
}

/// What a play past a voice limit does. Whichever voice a rule picks, it is
/// never one more important than the new play, nor an equally important one
/// started in the same frame: with none left to pick, the play is refused.
/// A loop picked, or refused, goes virtual instead: it keeps its place
/// silently and comes back when there is room, or in place of a voice it
/// outranks (but under `PreventNew`, which takes no voice's place)
/// ([`Voice::is_virtual`](super::Voice::is_virtual))
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
pub enum LimitRule {
  /// The new play is refused
  PreventNew,
  /// The voice that started first stops
  #[default]
  StopOldest,
  /// The quietest voice stops (the oldest of equals)
  StopQuietest,
  /// The voice farthest from the listener stops (the oldest of equals). A
  /// voice played without a position counts as at the listener, as does
  /// every voice while there is no listener (no `AudioListener`, and no main
  /// camera)
  StopFarthest,
  /// The least important voice stops (the oldest of equals)
  StopLowestPriority,
}

/// How a sound played at a place is heard from the listener
/// (`spatial: (min_distance: 50.0, max_distance: 800.0)` in a `.sound.ron`
/// file, with `curve`, `pan_width` and `low_pass_at_max` optional). The
/// listener is the entity with an `AudioListener`, or else the main camera;
/// [`falloff_gain_db`](super::falloff_gain_db) and
/// [`pan_for`](super::pan_for) give what it hears
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(from = "SpatialFile")]
pub struct Spatial {
  /// Within this distance it plays at full volume
  pub min_distance: f32,
  /// At this distance and beyond it is silent: a one-shot played there is
  /// culled (it never starts), and a loop is virtual, taking no voice, until
  /// the listener comes within 95% of it
  /// ([`Voice::is_virtual`](super::Voice::is_virtual))
  pub max_distance: f32,
  /// How its volume falls between the two (`Inverse` when left out)
  pub curve: Falloff,
  /// The horizontal offset at which it is panned fully to one side
  /// (`max_distance` when left out)
  pub pan_width: f32,
  /// A low-pass filter that closes with distance, to this cutoff in Hz (at
  /// most 20000) at `max_distance`; none when `None`. It is open (20 kHz)
  /// within `min_distance`, and closes in even steps of octaves between.
  /// Each voice with one plays on a kira track of its own, so at most 32
  /// have one at once: a voice past them plays unfiltered
  pub low_pass_at_max: Option<f64>,
}

/// [`Spatial`] as a file writes it
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SpatialFile {
  min_distance: f32,
  max_distance: f32,
  #[serde(default)]
  curve: Falloff,
  #[serde(default)]
  pan_width: Option<f32>,
  #[serde(default)]
  low_pass_at_max: Option<f64>,
}

impl From<SpatialFile> for Spatial {
  fn from(file: SpatialFile) -> Self {
    Self {
      min_distance: file.min_distance,
      max_distance: file.max_distance,
      curve: file.curve,
      pan_width: file.pan_width.unwrap_or(file.max_distance),
      low_pass_at_max: file.low_pass_at_max,
    }
  }
}

/// How a spatial sound's volume falls from full at its minimum distance to
/// silence at its maximum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
pub enum Falloff {
  /// Its amplitude falls evenly over the distance: -6 dB halfway, then fast
  /// to silence near the maximum
  Linear,
  /// Fast near the source and slowly far from it, as sound does in the open
  /// (the inverse-distance law, tapered to silence at the maximum): about
  /// -25 dB halfway
  #[default]
  Inverse,
  /// Evenly in decibels over the distance: -30 dB halfway
  Log,
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use super::*;
  use crate::core::asset::{AssetServer, Assets, AssetsView, MemorySource};
  use crate::core::audio::engine::MIN_FADE;
  use crate::core::audio::{
    AudioEngine, AudioListener, AudioLogEntry, AudioLogKind, AudioView, PlayOptions, Voice,
    VoicePick,
  };
  use crate::core::dev_log::{self, LogLevel};
  use crate::core::ecs::{Vec2, World};
  use crate::core::testing::{audio_step, audio_world, audio_world_over};

  const FRAME: Duration = Duration::from_nanos(16_666_667);
  const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/audio");

  /// The 250 ms tone, at 22.05 kHz
  fn tone() -> Vec<u8> {
    match std::fs::read(format!("{FIXTURES}/tone.wav")) {
      Ok(bytes) => bytes,
      Err(error) => panic!("tone.wav: {error}"),
    }
  }

  /// A clip `seconds` long, made in code
  fn clip(world: &mut World, seconds: f32) -> Handle<AudioClip> {
    let frames = vec![[0.1, 0.1]; (48_000.0 * seconds) as usize];
    world
      .resource_mut::<Assets<AudioClip>>()
      .add(AudioClip::from_frames(48_000, &frames).unwrap())
  }

  /// How the log names a clip made in code
  fn name(clip: &Handle<AudioClip>) -> String {
    format!("{:?}", clip.id())
  }

  /// The empty definition over `clips`
  fn of(clips: &[&Handle<AudioClip>]) -> Sound {
    Sound {
      clips: clips
        .iter()
        .map(|&clip| SoundClip::new(clip.clone()))
        .collect(),
      ..Default::default()
    }
  }

  /// [`of`], every play in a frame starting (up to 255)
  fn burst(clips: &[&Handle<AudioClip>]) -> Sound {
    Sound {
      per_frame: u8::MAX,
      ..of(clips)
    }
  }

  fn add(world: &mut World, sound: Sound) -> Handle<Sound> {
    world.resource_mut::<Assets<Sound>>().add(sound)
  }

  fn play(world: &mut World, sound: &Handle<Sound>) -> Voice {
    AudioView::new(world).play(sound)
  }

  fn play_with(world: &mut World, sound: &Handle<Sound>, options: PlayOptions) -> Voice {
    AudioView::new(world).play_with(sound, options)
  }

  fn log(world: &World) -> Vec<AudioLogEntry> {
    world.resource::<AudioEngine>().log().to_vec()
  }

  fn last(world: &World) -> AudioLogEntry {
    match world.resource::<AudioEngine>().log().last() {
      Some(entry) => entry.clone(),
      None => panic!("the log is empty"),
    }
  }

  fn of_kind(world: &World, kind: AudioLogKind) -> Vec<AudioLogEntry> {
    log(world)
      .into_iter()
      .filter(|entry| entry.kind == kind)
      .collect()
  }

  fn warnings(matching: impl Fn(&str) -> bool) -> usize {
    dev_log::get_entries()
      .iter()
      .filter(|entry| entry.level == LogLevel::Warn && matching(&entry.message))
      .count()
  }

  /// A sound over one 2 s clip limited to two voices by `rule`, fading out
  /// over 30 ms when stopped
  fn limited(world: &mut World, rule: LimitRule) -> Handle<Sound> {
    let long = clip(world, 2.0);
    let sound = Sound {
      limit: Some(VoiceLimit { max: 2, rule }),
      fade_out: Duration::from_millis(30),
      ..of(&[&long])
    };
    add(world, sound)
  }

  /// `count` plays of `sound`, each in a frame of its own
  fn plays_over_frames(world: &mut World, sound: &Handle<Sound>, count: usize) -> Vec<Voice> {
    (0..count)
      .map(|_| {
        let voice = play(world, sound);
        audio_step(world, FRAME);
        voice
      })
      .collect()
  }

  fn stolen(world: &World) -> Vec<(u32, Duration)> {
    of_kind(world, AudioLogKind::Stolen)
      .iter()
      .map(|entry| (entry.voice, entry.fade))
      .collect()
  }

  #[test]
  fn weights_pick_in_proportion() {
    let mut world = audio_world();
    world.resource_mut::<AudioEngine>().seed(7);
    let (three, one) = (clip(&mut world, 0.001), clip(&mut world, 0.001));
    let mut sound = of(&[&three, &one]);
    sound.clips[0].weight = 3.0;
    sound.clips[1].weight = 1.0;
    let sound = add(&mut world, sound);
    let mut threes = 0;
    for _ in 0..4000 {
      let voice = play(&mut world, &sound);
      assert!(voice.is_playing(), "{voice:?}");
      let start = last(&world);
      assert_eq!(start.kind, AudioLogKind::Start);
      if start.source == name(&three) {
        threes += 1;
      }
      audio_step(&mut world, Duration::from_millis(5));
    }
    assert!((2850..=3150).contains(&threes), "{threes} of 4000");
  }

  #[test]
  fn no_repeat_never_plays_a_clip_twice_in_a_row() {
    let mut world = audio_world();
    world.resource_mut::<AudioEngine>().seed(11);
    let (a, b) = (clip(&mut world, 0.001), clip(&mut world, 0.001));
    let sound = add(
      &mut world,
      Sound {
        no_repeat: 1,
        ..of(&[&a, &b])
      },
    );
    let mut picks = Vec::new();
    for _ in 0..200 {
      assert!(play(&mut world, &sound).is_playing());
      picks.push(last(&world).source);
      audio_step(&mut world, Duration::from_millis(5));
    }
    for pair in picks.windows(2) {
      assert_ne!(pair[0], pair[1], "{picks:?}");
    }
  }

  /// `no_repeat` avoids that many of the latest picks, and never so many
  /// that nothing is left
  #[test]
  fn no_repeat_avoids_its_count_of_recent_picks_and_leaves_one() {
    for (no_repeat, distinct) in [(2, 3), (9, 3)] {
      let mut world = audio_world();
      world.resource_mut::<AudioEngine>().seed(5);
      let clips = [
        clip(&mut world, 0.001),
        clip(&mut world, 0.001),
        clip(&mut world, 0.001),
      ];
      let sound = add(
        &mut world,
        Sound {
          no_repeat,
          ..of(&[&clips[0], &clips[1], &clips[2]])
        },
      );
      let mut picks = Vec::new();
      for _ in 0..90 {
        assert!(
          play(&mut world, &sound).is_playing(),
          "no_repeat {no_repeat}"
        );
        picks.push(last(&world).source);
        audio_step(&mut world, Duration::from_millis(5));
      }
      for window in picks.windows(distinct) {
        let mut seen = window.to_vec();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), distinct, "no_repeat {no_repeat}: {picks:?}");
      }
    }
  }

  /// A sound of four clips with gain and pitch ranges, and what forty
  /// plays of it pick after `seed`
  fn picks_after(seed: u64) -> Vec<Option<VoicePick>> {
    let mut world = audio_world();
    AudioView::new(&mut world).seed(seed);
    let clips: Vec<Handle<AudioClip>> = (0..4).map(|_| clip(&mut world, 0.001)).collect();
    let sound = add(
      &mut world,
      Sound {
        volume_range_db: 3.0,
        pitch_range: 2.0,
        no_repeat: 1,
        ..of(&clips.iter().collect::<Vec<_>>())
      },
    );
    (0..40)
      .map(|_| {
        let pick = play(&mut world, &sound).pick();
        audio_step(&mut world, Duration::from_millis(5));
        pick
      })
      .collect()
  }

  #[test]
  fn a_seed_repeats_the_picks_and_draws() {
    let first = picks_after(9);
    assert!(first.iter().all(Option::is_some), "{first:?}");
    assert_eq!(first, picks_after(9), "the same seed picks the same");
    assert_ne!(first, picks_after(10), "another seed picks otherwise");
  }

  #[test]
  fn a_voice_says_what_it_picked() {
    let mut world = audio_world();
    let clips: Vec<Handle<AudioClip>> = (0..3).map(|_| clip(&mut world, 0.001)).collect();
    let sound = add(
      &mut world,
      Sound {
        volume_db: -6.0,
        volume_range_db: 2.0,
        pitch: 1.0,
        pitch_range: 1.0,
        ..of(&clips.iter().collect::<Vec<_>>())
      },
    );
    let options = PlayOptions {
      volume_db: -3.0,
      pitch: 2.0,
      ..Default::default()
    };
    for _ in 0..30 {
      let voice = play_with(&mut world, &sound, options);
      let start = last(&world);
      assert_eq!(start.kind, AudioLogKind::Start);
      let Some(pick) = voice.pick() else {
        panic!("a voice that started picked: {voice:?}");
      };
      // As the engine started it: the clip, and the gain and pitch drawn
      // with the play's options added
      assert_eq!(name(&clips[pick.clip_index]), start.source);
      assert_eq!(pick.clip, clips[pick.clip_index].id());
      assert!(
        (pick.volume_db - start.gain_db).abs() < 1e-5,
        "{pick:?} {start:?}"
      );
      assert!((-11.0..=-7.0).contains(&pick.volume_db), "{pick:?}");
      let rate = 2f64.powf(f64::from(pick.pitch) / 12.0);
      assert!((rate - start.rate).abs() < 1e-6, "{pick:?} {start:?}");
      assert!((2.0..=4.0).contains(&pick.pitch), "{pick:?}");
      // The game's own changes later do not rewrite what was drawn
      voice.set_volume(-30.0);
      voice.set_pitch(-12.0);
      audio_step(&mut world, Duration::from_millis(5));
      assert_eq!(voice.pick(), Some(pick));
    }

    // A clip played directly is its only clip, at the play's options
    let direct = AudioView::new(&mut world).play_with(&clips[1], options);
    let pick = direct.pick();
    assert_eq!(
      pick.map(|pick| (pick.clip_index, pick.clip, pick.volume_db, pick.pitch)),
      Some((0, clips[1].id(), -3.0, 2.0))
    );

    // A refused play picked nothing
    let long = clip(&mut world, 1.0);
    let one = add(
      &mut world,
      Sound {
        limit: Some(VoiceLimit {
          max: 1,
          rule: LimitRule::PreventNew,
        }),
        ..of(&[&long])
      },
    );
    assert!(play(&mut world, &one).pick().is_some());
    let refused = play(&mut world, &one);
    assert!(refused.is_stopped());
    assert_eq!(refused.pick(), None);
  }

  #[test]
  fn volume_and_pitch_ranges_stay_inside_their_ranges() {
    let mut world = audio_world();
    world.resource_mut::<AudioEngine>().seed(3);
    let short = clip(&mut world, 0.001);
    let sound = add(
      &mut world,
      Sound {
        volume_db: -6.0,
        volume_range_db: 2.0,
        pitch: 0.0,
        pitch_range: 1.0,
        ..of(&[&short])
      },
    );
    let semitone = 2f64.powf(1.0 / 12.0);
    let (mut gains, mut rates) = (Vec::new(), Vec::new());
    for _ in 0..300 {
      assert!(play(&mut world, &sound).is_playing());
      let start = last(&world);
      gains.push(start.gain_db);
      rates.push(start.rate);
      audio_step(&mut world, Duration::from_millis(5));
    }
    for &gain in &gains {
      assert!((-8.0..=-4.0).contains(&gain), "{gain}");
    }
    for &rate in &rates {
      assert!(
        rate >= 1.0 / semitone - 1e-9 && rate <= semitone + 1e-9,
        "{rate}"
      );
    }
    // Spread across both sides of the range, not stuck at one value
    let (low, high) = gains
      .iter()
      .fold((f32::MAX, f32::MIN), |(lo, hi), &g| (lo.min(g), hi.max(g)));
    assert!(low < -7.5 && high > -4.5, "{low}..{high}");
    let (low, high) = rates
      .iter()
      .fold((f64::MAX, f64::MIN), |(lo, hi), &r| (lo.min(r), hi.max(r)));
    assert!(low < 0.96 && high > 1.04, "{low}..{high}");
    // A play's options add to what is drawn
    let options = PlayOptions {
      volume_db: -10.0,
      pitch: 12.0,
      ..Default::default()
    };
    assert!(play_with(&mut world, &sound, options).is_playing());
    let start = last(&world);
    assert!(
      (-18.0..=-14.0).contains(&start.gain_db),
      "{}",
      start.gain_db
    );
    assert!(start.rate > 1.8 && start.rate < 2.2, "{}", start.rate);
  }

  #[test]
  fn cooldown_refuses_a_play_too_soon() {
    let mut world = audio_world();
    let long = clip(&mut world, 1.0);
    let sound = add(
      &mut world,
      Sound {
        cooldown: Duration::from_millis(30),
        ..of(&[&long])
      },
    );
    let other = add(&mut world, of(&[&long]));
    assert!(play(&mut world, &sound).is_playing());
    let again = play(&mut world, &sound);
    assert!(again.is_stopped(), "{again:?}");
    assert_eq!(last(&world).kind, AudioLogKind::Refused);
    assert_eq!(last(&world).sound, format!("{:?}", sound.id()));
    // Another sound has a cooldown of its own
    assert!(play(&mut world, &other).is_playing());
    audio_step(&mut world, Duration::from_millis(20));
    assert!(play(&mut world, &sound).is_stopped(), "20 ms is too soon");
    assert_eq!(last(&world).kind, AudioLogKind::Refused);
    audio_step(&mut world, Duration::from_millis(20));
    assert!(play(&mut world, &sound).is_playing(), "40 ms is not");
  }

  #[test]
  fn prevent_new_refuses_the_third() {
    let mut world = audio_world();
    let sound = limited(&mut world, LimitRule::PreventNew);
    let voices = plays_over_frames(&mut world, &sound, 3);
    assert!(voices[0].is_playing() && voices[1].is_playing());
    assert!(voices[2].is_stopped(), "{:?}", voices[2]);
    assert_eq!(of_kind(&world, AudioLogKind::Refused).len(), 1);
    assert!(stolen(&world).is_empty());
  }

  #[test]
  fn stop_oldest_stops_the_first() {
    let mut world = audio_world();
    let sound = limited(&mut world, LimitRule::StopOldest);
    let voices = plays_over_frames(&mut world, &sound, 3);
    // Stopped with the sound's fade-out
    assert_eq!(
      stolen(&world),
      [(voices[0].id(), Duration::from_millis(30))]
    );
    assert!(voices[1].is_playing() && voices[2].is_playing());
    audio_step(&mut world, Duration::from_millis(50));
    assert!(voices[0].is_stopped(), "{:?}", voices[0]);
    assert_eq!(AudioView::new(&mut world).voices_on("sfx"), 2);
  }

  #[test]
  fn stop_quietest_stops_the_quietest() {
    let mut world = audio_world();
    let sound = limited(&mut world, LimitRule::StopQuietest);
    let mut voices = Vec::new();
    for volume_db in [-3.0, -12.0, 0.0] {
      let options = PlayOptions {
        volume_db,
        ..Default::default()
      };
      voices.push(play_with(&mut world, &sound, options));
      audio_step(&mut world, FRAME);
    }
    assert_eq!(
      stolen(&world),
      [(voices[1].id(), Duration::from_millis(30))]
    );
    assert!(voices[0].is_playing() && voices[2].is_playing());
  }

  #[test]
  fn stop_lowest_priority_stops_the_lowest() {
    let mut world = audio_world();
    let sound = limited(&mut world, LimitRule::StopLowestPriority);
    let mut voices = Vec::new();
    for priority in [100, 50, 200] {
      if let Some(sound) = world.resource_mut::<Assets<Sound>>().get_mut(&sound) {
        sound.priority = priority;
      }
      voices.push(play(&mut world, &sound));
      audio_step(&mut world, FRAME);
    }
    assert_eq!(
      stolen(&world),
      [(voices[1].id(), Duration::from_millis(30))]
    );
    assert!(voices[0].is_playing() && voices[2].is_playing());
    // A less important play cuts neither: it is refused
    if let Some(sound) = world.resource_mut::<Assets<Sound>>().get_mut(&sound) {
      sound.priority = 10;
    }
    assert!(play(&mut world, &sound).is_stopped());
    assert_eq!(last(&world).kind, AudioLogKind::Refused);
    assert_eq!(stolen(&world).len(), 1);
  }

  #[test]
  fn stop_farthest_stops_the_farthest_and_treats_no_position_as_zero() {
    let mut world = audio_world();
    world.spawn(AudioListener);
    let sound = limited(&mut world, LimitRule::StopFarthest);
    let at = |x: f32| PlayOptions {
      position: Some(Vec2::new(x, 0.0)),
      ..Default::default()
    };
    let nowhere = play(&mut world, &sound);
    audio_step(&mut world, FRAME);
    let far = play_with(&mut world, &sound, at(100.0));
    audio_step(&mut world, FRAME);
    let near = play_with(&mut world, &sound, at(10.0));
    audio_step(&mut world, FRAME);
    assert_eq!(stolen(&world), [(far.id(), Duration::from_millis(30))]);
    // The voice with no position counts as at the listener, so the one 10
    // away goes, though the other is older
    let last_one = play(&mut world, &sound);
    audio_step(&mut world, FRAME);
    assert_eq!(
      stolen(&world),
      [
        (far.id(), Duration::from_millis(30)),
        (near.id(), Duration::from_millis(30))
      ]
    );
    assert!(nowhere.is_playing() && last_one.is_playing());

    // With every voice at distance 0 (no listener yet), the oldest goes
    let mut world = audio_world();
    let sound = limited(&mut world, LimitRule::StopFarthest);
    let voices: Vec<Voice> = [10.0, 500.0, 20.0]
      .into_iter()
      .map(|x| {
        let voice = play_with(&mut world, &sound, at(x));
        audio_step(&mut world, FRAME);
        voice
      })
      .collect();
    assert_eq!(
      stolen(&world),
      [(voices[0].id(), Duration::from_millis(30))]
    );
  }

  #[test]
  fn a_bus_limit_counts_its_subtree() {
    let mut world = audio_world();
    world
      .resource_mut::<AudioEngine>()
      .set_bus_limit("master", Some(2));
    let long = clip(&mut world, 2.0);
    let effect = add(&mut world, of(&[&long]));
    let button = add(
      &mut world,
      Sound {
        bus: "ui".to_string(),
        ..of(&[&long])
      },
    );
    let first = play(&mut world, &effect);
    audio_step(&mut world, FRAME);
    let click = play(&mut world, &button);
    audio_step(&mut world, FRAME);
    // master holds the sfx voice and the ui voice: the oldest goes
    let third = play(&mut world, &effect);
    audio_step(&mut world, FRAME);
    assert_eq!(stolen(&world), [(first.id(), MIN_FADE)]);
    assert!(click.is_playing() && third.is_playing());
    audio_step(&mut world, Duration::from_millis(50));
    assert_eq!(AudioView::new(&mut world).voices_on("master"), 2);
    // A limit on ui counts only ui's voices
    world
      .resource_mut::<AudioEngine>()
      .set_bus_limit("ui", Some(1));
    let again = play(&mut world, &button);
    assert!(again.is_playing());
    assert_eq!(stolen(&world)[1], (click.id(), MIN_FADE));
    assert!(third.is_playing());
  }

  /// Review focus 1: a sound asked for a thousand times in one frame starts
  /// as often as its `per_frame` allows (once by default), and the rest
  /// are refused, saying why: copies started together play in step and
  /// add up, so a burst would stack into distortion
  #[test]
  fn a_thousand_plays_in_one_frame_stay_within_the_limit() {
    let mut world = audio_world();
    let long = clip(&mut world, 2.0);
    let shot = add(&mut world, of(&[&long]));
    for _ in 0..1000 {
      play(&mut world, &shot);
    }
    assert_eq!(AudioView::new(&mut world).voices_on("sfx"), 1);
    assert_eq!(of_kind(&world, AudioLogKind::Start).len(), 1);
    let refused = of_kind(&world, AudioLogKind::Refused);
    assert_eq!(refused.len(), 999);
    assert!(
      refused
        .iter()
        .all(|entry| entry.reason.contains("per_frame")),
      "{:#?}",
      refused.first()
    );
    audio_step(&mut world, FRAME);
    assert_eq!(AudioView::new(&mut world).voices_on("sfx"), 1);

    // Raised, exactly that many start
    let mut world = audio_world();
    let long = clip(&mut world, 2.0);
    let layered = add(
      &mut world,
      Sound {
        per_frame: 5,
        ..of(&[&long])
      },
    );
    for _ in 0..1000 {
      play(&mut world, &layered);
    }
    assert_eq!(of_kind(&world, AudioLogKind::Start).len(), 5);
    assert_eq!(of_kind(&world, AudioLogKind::Refused).len(), 995);

    // Raised past them, a voice limit still holds, and the cap of 96 a bus
    // holds: a voice is not stolen in the frame it started by an equally
    // important play, so a burst never stops its own starts
    let mut world = audio_world();
    let long = clip(&mut world, 2.0);
    let limited = add(
      &mut world,
      Sound {
        per_frame: u8::MAX,
        limit: Some(VoiceLimit {
          max: 4,
          rule: LimitRule::StopOldest,
        }),
        ..of(&[&long])
      },
    );
    for _ in 0..1000 {
      play(&mut world, &limited);
    }
    assert_eq!(AudioView::new(&mut world).voices_on("sfx"), 4);
    assert_eq!(of_kind(&world, AudioLogKind::Start).len(), 4);
    audio_step(&mut world, FRAME);
    assert_eq!(AudioView::new(&mut world).voices_on("sfx"), 4);

    let mut world = audio_world();
    let long = clip(&mut world, 2.0);
    let unlimited = add(
      &mut world,
      Sound {
        per_frame: u8::MAX,
        ..of(&[&long])
      },
    );
    for _ in 0..1000 {
      play(&mut world, &unlimited);
    }
    assert_eq!(AudioView::new(&mut world).voices_on("sfx"), 96);
    assert_eq!(of_kind(&world, AudioLogKind::Refused).len(), 904);
    audio_step(&mut world, FRAME);
    // A frame later, the new plays steal the older voices while kira has
    // room for the stolen ones to fade out: its 128 a track, less the 96
    for _ in 0..1000 {
      play(&mut world, &unlimited);
    }
    let stolen = stolen(&world);
    assert_eq!(stolen.len(), 32);
    assert!(
      stolen.iter().all(|&(_, fade)| fade == MIN_FADE),
      "{stolen:?}"
    );
    assert_eq!(of_kind(&world, AudioLogKind::Start).len(), 96 + 32);
    assert_eq!(of_kind(&world, AudioLogKind::Refused).len(), 904 + 968);
    audio_step(&mut world, FRAME);
    assert_eq!(AudioView::new(&mut world).voices_on("sfx"), 96);
    assert_eq!(world.resource::<AudioEngine>().kira_full(), 0);
  }

  /// A definition's `per_frame` is how many of its voices start in one
  /// frame, counted across its buses and places; a clip played directly
  /// starts once a frame, however it is played
  #[test]
  fn per_frame_raises_the_starts_in_one_frame() {
    let mut world = audio_world();
    let long = clip(&mut world, 2.0);
    let three = add(
      &mut world,
      Sound {
        per_frame: 3,
        ..of(&[&long])
      },
    );
    for index in 0..10 {
      let options = PlayOptions {
        position: (index % 2 == 0).then(|| Vec2::new(index as f32 * 10.0, 0.0)),
        volume_db: -(index as f32),
        ..Default::default()
      };
      play_with(&mut world, &three, options);
    }
    assert_eq!(of_kind(&world, AudioLogKind::Start).len(), 3);
    let refused = of_kind(&world, AudioLogKind::Refused);
    assert_eq!(refused.len(), 7);
    assert!(
      refused
        .iter()
        .all(|entry| entry.reason.contains("per_frame")),
      "{refused:#?}"
    );
    // A clip played directly, on its own and as a UI sound, at a place
    let bare = clip(&mut world, 2.0);
    let mut audio = AudioView::new(&mut world);
    let first = audio.play(&bare);
    let again = audio.play_at(&bare, Vec2::new(40.0, 0.0));
    let louder = audio.play_with(
      &bare,
      PlayOptions {
        volume_db: 6.0,
        ..Default::default()
      },
    );
    assert!(first.is_playing(), "{first:?}");
    assert!(again.is_stopped() && louder.is_stopped());
    assert_eq!(of_kind(&world, AudioLogKind::Start).len(), 4);
  }

  /// The count is a frame's: the next frame the sound starts again
  #[test]
  fn a_sound_starts_again_on_the_next_frame() {
    let mut world = audio_world();
    let long = clip(&mut world, 2.0);
    let shot = add(&mut world, of(&[&long]));
    let bare = clip(&mut world, 2.0);
    let mut starts = Vec::new();
    for _ in 0..3 {
      let (first, second, clip_first, clip_second) = {
        let mut audio = AudioView::new(&mut world);
        (
          audio.play(&shot),
          audio.play(&shot),
          audio.play(&bare),
          audio.play(&bare),
        )
      };
      starts.push((
        first.is_playing(),
        second.is_playing(),
        clip_first.is_playing(),
        clip_second.is_playing(),
      ));
      audio_step(&mut world, FRAME);
    }
    assert_eq!(starts, [(true, false, true, false); 3]);
    assert_eq!(AudioView::new(&mut world).voices_on("sfx"), 6);
  }

  /// Past the cap, the least important voice goes, then the quietest; a
  /// less important play is refused
  #[test]
  fn the_cap_steals_the_least_important_then_the_quietest() {
    let mut world = audio_world();
    let long = clip(&mut world, 2.0);
    let sound = add(&mut world, burst(&[&long]));
    let mut voices = Vec::new();
    for index in 0..96 {
      let options = PlayOptions {
        volume_db: if index == 40 { -20.0 } else { 0.0 },
        ..Default::default()
      };
      voices.push(play_with(&mut world, &sound, options));
    }
    audio_step(&mut world, FRAME);
    assert!(play(&mut world, &sound).is_playing());
    assert_eq!(stolen(&world), [(voices[40].id(), MIN_FADE)]);
    if let Some(sound) = world.resource_mut::<Assets<Sound>>().get_mut(&sound) {
      sound.priority = 0;
    }
    assert!(play(&mut world, &sound).is_stopped());
    assert_eq!(last(&world).kind, AudioLogKind::Refused);
  }

  /// When voices fading out hold all of kira's room on a bus, the plays it
  /// refuses warn once, naming the bus, not only in the audio log. The bus
  /// is this test's alone, so the process's log holds its warning only
  #[test]
  fn a_full_track_warns_once_naming_its_bus() {
    let mut world = audio_world();
    AudioView::new(&mut world).set_mixer(crate::core::audio::Mixer {
      buses: vec![crate::core::audio::BusDef::new("ambience/full-track")],
      ..Default::default()
    });
    let long = clip(&mut world, 2.0);
    let rain = add(
      &mut world,
      Sound {
        bus: "ambience/full-track".to_string(),
        ..burst(&[&long])
      },
    );
    for _ in 0..96 {
      play(&mut world, &rain);
    }
    // Each frame after, 32 plays steal older voices, whose fades fill
    // kira's room, and the rest are refused for want of it
    for _ in 0..2 {
      audio_step(&mut world, FRAME);
      for _ in 0..100 {
        play(&mut world, &rain);
      }
    }
    assert_eq!(stolen(&world).len(), 64);
    let about_it = warnings(|message| {
      message.contains("\"ambience/full-track\" bus is full") && message.contains("fading")
    });
    assert_eq!(about_it, 1);
  }

  /// The cap takes the least important voice first, however loud, and
  /// only then the quietest of the rest
  #[test]
  fn the_cap_takes_the_least_important_before_the_quietest() {
    let mut world = audio_world();
    let long = clip(&mut world, 2.0);
    let normal = add(&mut world, burst(&[&long]));
    let minor = add(
      &mut world,
      Sound {
        priority: 64,
        ..burst(&[&long])
      },
    );
    for _ in 0..94 {
      play(&mut world, &normal);
    }
    let loud_but_minor = play(&mut world, &minor);
    let quiet = play_with(
      &mut world,
      &normal,
      PlayOptions {
        volume_db: -20.0,
        ..Default::default()
      },
    );
    assert_eq!(AudioView::new(&mut world).voices_on("sfx"), 96);
    audio_step(&mut world, FRAME);
    assert!(play(&mut world, &normal).is_playing());
    assert_eq!(stolen(&world), [(loud_but_minor.id(), MIN_FADE)]);
    assert!(play(&mut world, &normal).is_playing());
    assert_eq!(
      stolen(&world),
      [(loud_but_minor.id(), MIN_FADE), (quiet.id(), MIN_FADE)]
    );
  }

  /// Victims are planned before any stops: a play the sound's own limit
  /// would make room for, but a bus limit then refuses, stops nothing
  #[test]
  fn a_play_a_later_limit_refuses_stops_nothing() {
    let mut world = audio_world();
    let long = clip(&mut world, 2.0);
    let important = add(
      &mut world,
      Sound {
        priority: 200,
        ..of(&[&long])
      },
    );
    let single = add(
      &mut world,
      Sound {
        limit: Some(VoiceLimit {
          max: 1,
          rule: LimitRule::StopOldest,
        }),
        ..of(&[&long])
      },
    );
    let kept = play(&mut world, &important);
    let first = play(&mut world, &single);
    audio_step(&mut world, FRAME);
    world
      .resource_mut::<AudioEngine>()
      .set_bus_limit("master", Some(1));
    // The sound's limit would stop `first`, but master's would then have to
    // stop the more important voice too
    let refused = play(&mut world, &single);
    assert!(refused.is_stopped(), "{refused:?}");
    assert_eq!(last(&world).kind, AudioLogKind::Refused);
    assert!(stolen(&world).is_empty(), "{:?}", stolen(&world));
    audio_step(&mut world, FRAME);
    assert!(kept.is_playing() && first.is_playing());
  }

  #[test]
  fn a_region_plays_its_part_and_a_loop_loops() {
    let mut world = audio_world();
    let long = clip(&mut world, 1.0);
    let mut part = of(&[&long]);
    part.clips[0].region = Some(0.2..0.25);
    let part = add(&mut world, part);
    let looped = add(
      &mut world,
      Sound {
        looping: true,
        fade_in: Duration::from_millis(20),
        ..of(&[&long])
      },
    );
    let short = play(&mut world, &part);
    let forever = play(&mut world, &looped);
    assert_eq!(last(&world).fade, Duration::from_millis(20), "the fade-in");
    for _ in 0..6 {
      audio_step(&mut world, FRAME);
    }
    assert!(short.is_stopped(), "50 ms of the clip: {short:?}");
    for _ in 0..90 {
      audio_step(&mut world, FRAME);
    }
    assert!(forever.is_playing(), "a 1 s clip looping: {forever:?}");
  }

  /// kira reads past a slice that runs off its clip, and loops forever on
  /// an empty loop: a definition made in code with such regions plays
  /// inside the clip instead
  #[test]
  fn regions_are_kept_inside_the_clip() {
    let mut world = audio_world();
    let long = clip(&mut world, 0.25);
    // Each case is a sound of its own, so every run plays every one
    let cases = [
      ("a region past the clip's end", Some(0.1..10.0), None),
      ("an empty loop region", None, Some(0.1..0.1)),
      ("a backwards loop region", None, Some(0.2..0.05)),
      ("a region that is not a span", Some(f64::NAN..-1.0), None),
      (
        "a loop region past what plays",
        Some(0.0..0.1),
        Some(5.0..9.0),
      ),
    ];
    let mut voices = Vec::new();
    for (case, region, loop_region) in cases {
      let mut wild = Sound {
        looping: true,
        ..of(&[&long])
      };
      wild.clips[0].region = region;
      wild.clips[0].loop_region = loop_region;
      let wild = add(&mut world, wild);
      let voice = play(&mut world, &wild);
      assert!(voice.is_playing(), "{case}: {voice:?}");
      voices.push((case, voice));
    }
    let empty = clip(&mut world, 0.0);
    let silent = add(
      &mut world,
      Sound {
        looping: true,
        ..of(&[&empty])
      },
    );
    let nothing = play(&mut world, &silent);
    for _ in 0..30 {
      audio_step(&mut world, FRAME);
    }
    for (case, voice) in &voices {
      assert!(voice.is_playing(), "{case} loops on: {voice:?}");
    }
    assert!(
      nothing.is_stopped(),
      "an empty clip does not loop: {nothing:?}"
    );
  }

  /// Step, letting the workers load, until `done`
  fn step_until(world: &mut World, done: impl Fn(&World) -> bool) {
    for _ in 0..500 {
      if done(world) {
        return;
      }
      audio_step(world, Duration::from_millis(10));
      std::thread::sleep(Duration::from_millis(2));
    }
    assert!(done(world), "it never happened");
  }

  fn ready(world: &World, sound: &Handle<Sound>) -> bool {
    world.resource::<AssetServer>().is_ready(sound)
  }

  /// A loop played while its sound loads waits as a voice that reports
  /// playing, with what the game does to it meanwhile, and starts with its
  /// fade-in once the sound is ready
  #[test]
  fn a_loop_played_while_loading_starts_when_ready() {
    let files = MemorySource::new();
    files.insert(
      "sfx/rain-waits.sound.ron",
      r#"(clips: [(path: "sfx/rain-waits.wav")], looping: true, volume: -6.0, fade_in: 0.2)"#,
    );
    files.insert("sfx/rain-waits.wav", tone());
    let mut world = audio_world_over(files);
    let sound = AssetsView::new(&mut world).load::<Sound>("sfx/rain-waits.sound.ron");
    let rain = play(&mut world, &sound);
    assert!(rain.is_playing(), "it waits, playing: {rain:?}");
    assert_ne!(rain.id(), 0);
    rain.set_volume(-3.0);
    assert!(of_kind(&world, AudioLogKind::Start).is_empty());
    step_until(&mut world, |world| ready(world, &sound));
    audio_step(&mut world, FRAME);
    let starts = of_kind(&world, AudioLogKind::Start);
    let [start] = &starts[..] else {
      panic!("one start: {:#?}", log(&world));
    };
    assert_eq!(start.voice, rain.id(), "its voice is the one played");
    assert_eq!(start.sound, "sfx/rain-waits.sound.ron");
    assert_eq!(start.fade, Duration::from_millis(200), "with its fade-in");
    assert_eq!(start.gain_db, -9.0, "with the game's change on top");
    assert!(rain.is_playing(), "{rain:?}");
    assert_eq!(rain.pick().map(|pick| pick.clip_index), Some(0));
    for _ in 0..10 {
      audio_step(&mut world, FRAME);
    }
    assert!(rain.is_playing(), "it loops: {rain:?}");
    assert_eq!(AudioView::new(&mut world).voices_on("sfx"), 1);
  }

  /// A voice whose stop is queued, or that is fading out, does not count
  /// against its sound's limit or its `per_frame`: a sound limited to one
  /// voice (`PreventNew`) stopped and played again in one frame starts the
  /// new voice, as does one played, stopped and played again in a single
  /// frame. A stopped voice still holds its place on kira's track until
  /// its fade ends
  #[test]
  fn a_stopped_voice_does_not_hold_its_sound_s_limit() {
    let mut world = audio_world();
    let long = clip(&mut world, 2.0);
    let sound = add(
      &mut world,
      Sound {
        limit: Some(VoiceLimit {
          max: 1,
          rule: LimitRule::PreventNew,
        }),
        ..of(&[&long])
      },
    );
    let first = play(&mut world, &sound);
    audio_step(&mut world, FRAME);
    first.stop(Duration::from_millis(100));
    let second = play(&mut world, &sound);
    assert!(second.is_playing(), "the limit: {:#?}", log(&world));
    let engine = world.resource::<AudioEngine>();
    assert_eq!(engine.sounds_on("sfx"), 2, "kira's room holds both");
    audio_step(&mut world, FRAME);
    second.stop(MIN_FADE);
    let third = play(&mut world, &sound);
    third.stop(MIN_FADE);
    let fourth = play(&mut world, &sound);
    assert!(
      third.id() != 0 && fourth.is_playing(),
      "per_frame: {:#?}",
      log(&world)
    );
    assert!(of_kind(&world, AudioLogKind::Refused).is_empty());
  }

  /// A frame of the audio engine with no pump: a sound loading stays
  /// loading through it
  fn frame_without_pump(world: &mut World) {
    world
      .resource_mut::<crate::core::time::Time>()
      .advance(FRAME);
    crate::core::audio::audio_system(world);
    world.clear_trackers();
  }

  /// Plays that waited for their sound count against its `per_frame` as
  /// they land, as plays made that frame do: the same loop played twice
  /// while it loads (in two frames) starts once, and the other is refused
  /// with the per-frame reason
  #[test]
  fn plays_that_waited_count_against_per_frame_as_they_land() {
    let files = MemorySource::new();
    files.insert(
      "sfx/twice-waits.sound.ron",
      r#"(clips: [(path: "sfx/twice-waits.wav")], looping: true)"#,
    );
    files.insert("sfx/twice-waits.wav", tone());
    let mut world = audio_world_over(files);
    let sound = AssetsView::new(&mut world).load::<Sound>("sfx/twice-waits.sound.ron");
    let first = play(&mut world, &sound);
    frame_without_pump(&mut world);
    let second = play(&mut world, &sound);
    assert!(first.is_playing() && second.is_playing(), "both wait");
    step_until(&mut world, |world| ready(world, &sound));
    audio_step(&mut world, FRAME);
    let starts: Vec<u32> = of_kind(&world, AudioLogKind::Start)
      .iter()
      .map(|entry| entry.voice)
      .collect();
    assert_eq!(starts, [first.id()], "{:#?}", log(&world));
    let refused = of_kind(&world, AudioLogKind::Refused);
    let [refused] = &refused[..] else {
      panic!("one refused: {refused:#?}");
    };
    assert!(refused.reason.starts_with("per_frame"), "{refused:?}");
    assert!(second.is_stopped(), "{second:?}");
    assert!(first.is_playing(), "{first:?}");
  }

  /// A waiting loop paused by the game starts paused, silent, and its
  /// resume takes its fade-in; one that starts while the game is paused is
  /// held as any voice on its bus is (the two land together, which its
  /// `per_frame` of 2 allows)
  #[test]
  fn a_waiting_loop_keeps_its_pause_and_the_game_s() {
    let files = MemorySource::new();
    files.insert(
      "sfx/hum-waits.sound.ron",
      r#"(clips: [(path: "sfx/hum-waits.wav")], looping: true, fade_in: 0.1, per_frame: 2)"#,
    );
    files.insert("sfx/hum-waits.wav", tone());
    let mut world = audio_world_over(files);
    let sound = AssetsView::new(&mut world).load::<Sound>("sfx/hum-waits.sound.ron");
    let paused = play(&mut world, &sound);
    paused.pause();
    audio_step(&mut world, Duration::ZERO);
    assert!(!paused.is_playing() && !paused.is_stopped(), "{paused:?}");
    let held = play(&mut world, &sound);
    world.resource_mut::<crate::core::time::Time>().game.pause();
    step_until(&mut world, |world| ready(world, &sound));
    audio_step(&mut world, FRAME);
    let engine = world.resource::<AudioEngine>();
    use kira::sound::PlaybackState;
    assert_eq!(
      engine.voice_playback(paused.id()),
      Some(PlaybackState::Paused)
    );
    assert_eq!(
      engine.voice_playback(held.id()),
      Some(PlaybackState::Paused)
    );
    assert!(held.is_playing(), "a hold leaves it playing: {held:?}");
    world
      .resource_mut::<crate::core::time::Time>()
      .game
      .resume();
    paused.resume();
    audio_step(&mut world, FRAME);
    let resumes: Vec<(u32, Duration)> = of_kind(&world, AudioLogKind::Resume)
      .iter()
      .filter(|entry| entry.voice != 0)
      .map(|entry| (entry.voice, entry.fade))
      .collect();
    let fade = Duration::from_millis(100);
    assert!(resumes.contains(&(paused.id(), fade)), "{resumes:?}");
    assert!(resumes.contains(&(held.id(), fade)), "{resumes:?}");
  }

  /// A loop whose sound fails to load ends, with one warning, which a play
  /// after does not repeat
  #[test]
  fn a_loop_whose_load_fails_ends_with_one_warning() {
    let files = MemorySource::new();
    files.insert(
      "sfx/never-waits.sound.ron",
      r#"(clips: [(path: "sfx/never-there.wav")], looping: true)"#,
    );
    let mut world = audio_world_over(files);
    let sound = AssetsView::new(&mut world).load::<Sound>("sfx/never-waits.sound.ron");
    let waiting = play(&mut world, &sound);
    assert!(waiting.is_playing(), "it waits: {waiting:?}");
    step_until(&mut world, |world| {
      world.resource::<AssetServer>().state(&sound).is_failed()
    });
    audio_step(&mut world, FRAME);
    assert!(waiting.is_stopped(), "{waiting:?}");
    assert!(play(&mut world, &sound).is_stopped());
    audio_step(&mut world, FRAME);
    assert!(of_kind(&world, AudioLogKind::Start).is_empty());
    // The registry's, saying why: the engine adds none
    let about_it = warnings(|message| message.contains("sfx/never-waits.sound.ron"));
    assert_eq!(about_it, 1);
  }

  /// A waiting loop belongs to the scene that played it: the scene's exit
  /// ends it, and it never starts
  #[test]
  fn a_waiting_loop_leaves_with_its_scene() {
    let files = MemorySource::new();
    files.insert(
      "sfx/bed-waits.sound.ron",
      r#"(clips: [(path: "sfx/bed-waits.wav")], looping: true)"#,
    );
    files.insert("sfx/bed-waits.wav", tone());
    let mut world = audio_world_over(files);
    let sound = AssetsView::new(&mut world).load::<Sound>("sfx/bed-waits.sound.ron");
    let (bed, kept) = crate::core::audio::in_scene(&mut world, 7, |world| {
      let persist = PlayOptions {
        persist: true,
        ..Default::default()
      };
      let bed = play(world, &sound);
      audio_step(world, Duration::ZERO);
      (bed, play_with(world, &sound, persist))
    });
    crate::core::audio::release_scene(&mut world, 7);
    assert!(bed.is_stopped(), "{bed:?}");
    step_until(&mut world, |world| ready(world, &sound));
    audio_step(&mut world, FRAME);
    let starts: Vec<u32> = of_kind(&world, AudioLogKind::Start)
      .iter()
      .map(|entry| entry.voice)
      .collect();
    assert_eq!(starts, [kept.id()], "only the persistent one starts");
    assert!(bed.is_stopped(), "{bed:?}");
  }

  /// A one-shot played while its sound loads would only be late: it is
  /// refused (a clip at once, a definition once it is known to be a
  /// one-shot), logged with why, and warned about once per sound
  #[test]
  fn a_one_shot_played_while_loading_is_refused_and_logged() {
    let files = MemorySource::new();
    files.insert(
      "sfx/later.sound.ron",
      r#"(clips: [(path: "sfx/later.wav")])"#,
    );
    files.insert("sfx/later.wav", tone());
    files.insert("sfx/later-bare.wav", tone());
    let mut world = audio_world_over(files);
    let sound = AssetsView::new(&mut world).load::<Sound>("sfx/later.sound.ron");
    let bare = AssetsView::new(&mut world).load::<AudioClip>("sfx/later-bare.wav");
    let early = play(&mut world, &sound);
    let (clip_early, clip_again) = {
      let mut audio = AudioView::new(&mut world);
      (audio.play(&bare), audio.play(&bare))
    };
    assert!(clip_early.is_stopped() && clip_again.is_stopped());
    let refused = of_kind(&world, AudioLogKind::Refused);
    assert_eq!(refused.len(), 2, "{refused:#?}");
    assert!(
      refused
        .iter()
        .all(|entry| entry.source == "sfx/later-bare.wav" && entry.reason.contains("loading")),
      "{refused:#?}"
    );
    step_until(&mut world, |world| ready(world, &sound));
    audio_step(&mut world, FRAME);
    assert!(early.is_stopped(), "{early:?}");
    let refused = of_kind(&world, AudioLogKind::Refused);
    let about_sound: Vec<&AudioLogEntry> = refused
      .iter()
      .filter(|entry| entry.sound == "sfx/later.sound.ron")
      .collect();
    assert_eq!(about_sound.len(), 1, "{refused:#?}");
    assert!(about_sound[0].reason.contains("loading"));
    assert!(of_kind(&world, AudioLogKind::Start).is_empty());
    let warned = |path: &str| {
      let named = format!("{path}:");
      warnings(|message| message.starts_with(&named) && message.contains("still loading"))
    };
    let once = usize::from(cfg!(debug_assertions));
    assert_eq!(warned("sfx/later-bare.wav"), once);
    assert_eq!(warned("sfx/later.sound.ron"), once);
    // Loaded, it plays
    let voice = play(&mut world, &sound);
    assert!(voice.is_playing(), "{voice:?}");
    let start = last(&world);
    assert_eq!(start.sound, "sfx/later.sound.ron");
    assert_eq!(start.source, "sfx/later.wav");
    assert_eq!(start.bus, "sfx");
  }

  #[test]
  fn a_failed_sound_warns_once() {
    let files = MemorySource::new();
    files.insert(
      "sfx/broken-for-warning.sound.ron",
      r#"(clips: [(path: "sfx/nowhere-for-warning.wav")])"#,
    );
    let mut world = audio_world_over(files);
    let sound = AssetsView::new(&mut world).load::<Sound>("sfx/broken-for-warning.sound.ron");
    for _ in 0..500 {
      if world.resource::<AssetServer>().state(&sound).is_failed() {
        break;
      }
      audio_step(&mut world, Duration::from_millis(10));
      std::thread::sleep(Duration::from_millis(2));
    }
    assert!(world.resource::<AssetServer>().state(&sound).is_failed());
    for _ in 0..3 {
      assert!(play(&mut world, &sound).is_stopped());
      audio_step(&mut world, FRAME);
    }
    let about_it = warnings(|message| {
      message.starts_with("sfx/broken-for-warning.sound.ron:") && message.contains("plays nothing")
    });
    assert_eq!(about_it, 1);
  }

  #[test]
  fn a_reload_keeps_a_voice_valid() {
    let files = MemorySource::new();
    files.insert(
      "sfx/hum.sound.ron",
      r#"(clips: [(path: "sfx/hum.wav")], looping: true)"#,
    );
    files.insert("sfx/hum.wav", tone());
    let mut world = audio_world_over(files.clone());
    let sound = match AssetsView::new(&mut world).load_now::<Sound>("sfx/hum.sound.ron") {
      Ok(sound) => sound,
      Err(error) => panic!("{}", error.report()),
    };
    let voice = play(&mut world, &sound);
    assert!(voice.is_playing());
    audio_step(&mut world, FRAME);
    // The definition reloads, then its clip
    let reloads = [
      (
        "sfx/hum.sound.ron",
        br#"(clips: [(path: "sfx/hum.wav")], looping: true, volume: -6.0)"#.to_vec(),
      ),
      ("sfx/hum.wav", tone()),
    ];
    for (path, bytes) in reloads {
      let version = |world: &World| {
        if path.ends_with(".wav") {
          world
            .resource::<Assets<Sound>>()
            .get(&sound)
            .and_then(|sound| {
              world
                .resource::<Assets<AudioClip>>()
                .version(&sound.clips[0].clip)
            })
        } else {
          world.resource::<Assets<Sound>>().version(&sound)
        }
      };
      let before = version(&world);
      files.insert(path, bytes);
      world.resource::<AssetServer>().reload(path);
      for _ in 0..500 {
        if version(&world) != before {
          break;
        }
        audio_step(&mut world, Duration::from_millis(10));
        std::thread::sleep(Duration::from_millis(2));
      }
      assert_ne!(version(&world), before, "{path} reloaded");
      assert!(voice.is_playing(), "after {path} reloaded: {voice:?}");
      voice.set_volume(-3.0);
      audio_step(&mut world, FRAME);
      let gain = last(&world);
      assert_eq!((gain.kind, gain.voice), (AudioLogKind::Gain, voice.id()));
      assert_eq!(AudioView::new(&mut world).voices_on("sfx"), 1);
    }
    assert_eq!(
      world
        .resource::<Assets<Sound>>()
        .get(&sound)
        .map(|sound| sound.volume_db),
      Some(-6.0)
    );
  }
}
