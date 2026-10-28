//! Sound definitions from `.sound.ron` text

use std::ops::Range;
use std::time::Duration;

use ron::extensions::Extensions;
use serde::Deserialize;

use crate::core::asset::{
  AssetError, AssetLoader, AssetServer, Assets, AudioClipSettings, CheckContext, ClipMode,
  LoadContext,
};
use crate::core::audio::{AudioClip, Sound, SoundClip, Spatial, VoiceLimit, check_bus_name};

/// Loads [`Sound`]s from `.sound.ron` files. Every field but `clips` may be
/// left out:
///
/// ```ron
/// (
///   clips: [
///     (path: "sfx/step1.wav", weight: 2.0),
///     (path: "sfx/step2.wav", region: (start: 0.0, end: 0.4)),
///   ],
///   bus: "sfx/footsteps",                 // "sfx"
///   volume: -6.0,                         // 0.0 dB
///   volume_range: 2.0,                    // 0.0 dB either side
///   pitch: 0.0,                           // 0.0 semitones
///   pitch_range: 1.0,                     // 0.0 semitones either side
///   no_repeat: 1,                         // 0
///   limit: (max: 4, rule: StopOldest),    // none; the rule is StopOldest when left out
///   priority: 64,                         // 128
///   cooldown: 0.03,                       // 0.0 s
///   looping: false,                       // false
///   fade_in: 0.0,                         // 0.0 s
///   fade_out: 0.05,                       // 0.0 s (at least 10 ms is used)
///   spatial: (min_distance: 50.0, max_distance: 800.0, curve: Inverse), // none
///   mode: Stream,                         // Auto
/// )
/// ```
///
/// A clip's `weight` is 1 when left out; its `region` (seconds of the clip)
/// and `loop_region` (seconds from the region's start, for a looping sound)
/// are optional. `spatial`'s `curve` (`Linear`, `Inverse`, `Log`) is
/// `Inverse` when left out, `pan_width` is `max_distance`, and
/// `low_pass_at_max` (a cutoff in Hz) is off. `mode` loads every clip as
/// [`ClipMode`] says: `Auto` decodes a clip of up to 10 s and streams a
/// longer one, `Decode` and `Stream` force one or the other (the web decodes
/// every clip). An option is written without
/// `Some(..)`. The bus is `master`, a standard bus (`music`, `sfx`, `ui`,
/// `voice`, `ambience`) or a bus of the mixer's under one (`"sfx/footsteps"`).
///
/// The clips are asset paths, and the definition's dependencies: a sound is
/// ready once each is, a clip that does not load fails it
/// (`AssetError::Dependency`), and a region past the end of its clip fails
/// it too. A mistake fails the load with `AssetError::Decode`, naming the
/// file and what is wrong. Every registry has it
#[derive(Debug, Default, Clone, Copy)]
pub struct SoundLoader;

impl AssetLoader for SoundLoader {
  type Asset = Sound;
  type Settings = ();

  fn extensions(&self) -> &[&'static str] {
    &["sound.ron"]
  }

  fn load(&self, bytes: Vec<u8>, _: &(), cx: &mut LoadContext<'_>) -> Result<Sound, AssetError> {
    let path = cx.path().clone();
    let text = String::from_utf8(bytes).map_err(|error| AssetError::decode(&path, error))?;
    let file: SoundFile = ron::Options::default()
      .with_default_extension(Extensions::IMPLICIT_SOME)
      .from_str(&text)
      .map_err(|error| AssetError::decode(&path, error))?;
    file
      .check()
      .map_err(|error| AssetError::decode(&path, error))?;
    let mut clips = Vec::with_capacity(file.clips.len());
    for clip in file.clips {
      let settings = AudioClipSettings { mode: file.mode };
      let handle =
        cx.load_with::<AudioClip, _>(&clip.path, settings)
          .map_err(|error| match error {
            AssetError::Dependency { .. } => error,
            // A path the registry refused (one that leaves the asset root, or
            // no loader for it) is the definition's mistake
            refused => AssetError::decode(
              &path,
              format!("the clip {:?}: {}", clip.path, refused.report()),
            ),
          })?;
      clips.push(SoundClip {
        clip: handle,
        weight: clip.weight,
        region: clip.region,
        loop_region: clip.loop_region,
      });
    }
    Ok(Sound {
      clips,
      bus: file.bus,
      volume_db: file.volume,
      volume_range_db: file.volume_range,
      pitch: file.pitch,
      pitch_range: file.pitch_range,
      no_repeat: file.no_repeat,
      limit: file.limit,
      priority: file.priority,
      cooldown: seconds(file.cooldown),
      looping: file.looping,
      fade_in: seconds(file.fade_in),
      fade_out: seconds(file.fade_out),
      spatial: file.spatial,
    })
  }

  /// Each region inside its clip, and each loop region inside its region
  fn check(&self, sound: &Sound, cx: &CheckContext<'_>) -> Result<(), AssetError> {
    let Some(clips) = cx.world().get_resource::<Assets<AudioClip>>() else {
      return Ok(());
    };
    let server = cx.world().get_resource::<AssetServer>();
    for clip in &sound.clips {
      let Some(length) = clips.get(&clip.clip).map(|c| c.duration().as_secs_f64()) else {
        continue;
      };
      let named = || {
        server
          .and_then(|server| server.path(&clip.clip))
          .map_or_else(|| format!("{:?}", clip.clip.id()), |path| path.to_string())
      };
      let played = match &clip.region {
        Some(region) if region.end > length + TOLERANCE => {
          return Err(AssetError::decode(
            cx.path(),
            format!(
              "the clip {:?}: its region ends at {} s, past the clip's end at {length} s",
              named(),
              region.end
            ),
          ));
        }
        Some(region) => region.end - region.start,
        None => length,
      };
      if let Some(looped) = &clip.loop_region
        && looped.end > played + TOLERANCE
      {
        return Err(AssetError::decode(
          cx.path(),
          format!(
            "the clip {:?}: its loop_region ends at {} s, past the end of what plays ({played} s)",
            named(),
            looped.end
          ),
        ));
      }
    }
    Ok(())
  }
}

/// How far past a clip's end a region may reach, in seconds, for rounding
const TOLERANCE: f64 = 1e-6;

/// A time the file gives in seconds, already checked
fn seconds(value: f64) -> Duration {
  Duration::try_from_secs_f64(value).unwrap_or_default()
}

/// A `.sound.ron` file, as written
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SoundFile {
  clips: Vec<ClipFile>,
  #[serde(default = "default_bus")]
  bus: String,
  #[serde(default)]
  volume: f32,
  #[serde(default)]
  volume_range: f32,
  #[serde(default)]
  pitch: f32,
  #[serde(default)]
  pitch_range: f32,
  #[serde(default)]
  no_repeat: u8,
  #[serde(default)]
  limit: Option<VoiceLimit>,
  #[serde(default = "default_priority")]
  priority: u8,
  #[serde(default)]
  cooldown: f64,
  #[serde(default)]
  looping: bool,
  #[serde(default)]
  fade_in: f64,
  #[serde(default)]
  fade_out: f64,
  #[serde(default)]
  spatial: Option<Spatial>,
  #[serde(default)]
  mode: ClipMode,
}

/// One of a file's clips
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClipFile {
  path: String,
  #[serde(default = "default_weight")]
  weight: f32,
  #[serde(default)]
  region: Option<Range<f64>>,
  #[serde(default)]
  loop_region: Option<Range<f64>>,
}

fn default_bus() -> String {
  Sound::default().bus
}

fn default_priority() -> u8 {
  Sound::DEFAULT_PRIORITY
}

fn default_weight() -> f32 {
  1.0
}

impl SoundFile {
  /// What RON cannot say is wrong: the numbers' ranges, the regions, the
  /// bus, a loop region on a sound that does not loop
  fn check(&self) -> Result<(), String> {
    if self.clips.is_empty() {
      return Err(r#"a sound needs at least one clip (`clips: [(path: "sfx/jump.wav")]`)"#.into());
    }
    for clip in &self.clips {
      clip
        .check(self.looping)
        .map_err(|error| format!("the clip {:?}: {error}", clip.path))?;
    }
    if self.clips.iter().all(|clip| clip.weight == 0.0) {
      return Err("every clip's weight is 0, so none would ever play".into());
    }
    check_bus_name(&self.bus)?;
    finite("volume", self.volume)?;
    at_least_zero("volume_range", self.volume_range)?;
    finite("pitch", self.pitch)?;
    at_least_zero("pitch_range", self.pitch_range)?;
    for (field, value) in [
      ("cooldown", self.cooldown),
      ("fade_in", self.fade_in),
      ("fade_out", self.fade_out),
    ] {
      if !(value.is_finite() && value >= 0.0) || Duration::try_from_secs_f64(value).is_err() {
        return Err(format!(
          "{field}: {value} is not a time in seconds (0 or more)"
        ));
      }
    }
    if self.limit.is_some_and(|limit| limit.max == 0) {
      return Err("limit: its max is 0, so the sound would never play (it is 1 or more)".into());
    }
    if let Some(spatial) = &self.spatial {
      check_spatial(spatial)?;
    }
    Ok(())
  }
}

impl ClipFile {
  fn check(&self, looping: bool) -> Result<(), String> {
    if !(self.weight.is_finite() && self.weight >= 0.0) {
      return Err(format!("its weight {} is not 0 or more", self.weight));
    }
    if let Some(region) = &self.region {
      check_span("region", region)?;
    }
    if let Some(looped) = &self.loop_region {
      if !looping {
        return Err("it has a loop_region, but the sound does not loop (`looping: true`)".into());
      }
      check_span("loop_region", looped)?;
    }
    Ok(())
  }
}

/// A region from 0 or later, ending after it starts
fn check_span(field: &str, span: &Range<f64>) -> Result<(), String> {
  if span.start.is_finite() && span.end.is_finite() && span.start >= 0.0 && span.end > span.start {
    Ok(())
  } else {
    Err(format!(
      "its {field} {}..{} is not a span of seconds (from 0 or later, ending after it starts)",
      span.start, span.end
    ))
  }
}

fn finite(field: &str, value: f32) -> Result<(), String> {
  if value.is_finite() {
    Ok(())
  } else {
    Err(format!("{field}: {value} is not a number"))
  }
}

fn at_least_zero(field: &str, value: f32) -> Result<(), String> {
  if value.is_finite() && value >= 0.0 {
    Ok(())
  } else {
    Err(format!("{field}: {value} is not 0 or more"))
  }
}

fn check_spatial(spatial: &Spatial) -> Result<(), String> {
  let Spatial {
    min_distance,
    max_distance,
    pan_width,
    low_pass_at_max,
    ..
  } = *spatial;
  if !(min_distance.is_finite() && min_distance >= 0.0) {
    return Err(format!(
      "spatial: min_distance {min_distance} is not 0 or more"
    ));
  }
  if !(max_distance.is_finite() && max_distance > min_distance) {
    return Err(format!(
      "spatial: max_distance {max_distance} is not more than min_distance ({min_distance})"
    ));
  }
  if !(pan_width.is_finite() && pan_width > 0.0) {
    return Err(format!("spatial: pan_width {pan_width} is not more than 0"));
  }
  if let Some(cutoff) = low_pass_at_max
    && !(cutoff.is_finite() && cutoff > 0.0)
  {
    return Err(format!(
      "spatial: low_pass_at_max {cutoff} is not a cutoff in Hz (more than 0)"
    ));
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use std::time::{Duration, Instant};

  use crate::core::asset::{
    AssetPath, AssetServer, AssetSource, Assets, AssetsView, AudioClipSettings, ClipMode,
    MemorySource, insert_registry, pump,
  };
  use crate::core::audio::{AudioClip, Falloff, LimitRule, Sound, Spatial, VoiceLimit};
  use crate::core::ecs::kernel::World;

  const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/audio");

  /// The 250 ms tone, at 22.05 kHz
  fn tone() -> Vec<u8> {
    match std::fs::read(format!("{FIXTURES}/tone.wav")) {
      Ok(bytes) => bytes,
      Err(error) => panic!("tone.wav: {error}"),
    }
  }

  fn registry_over(source: impl AssetSource) -> World {
    let mut world = World::new();
    insert_registry(&mut world, AssetServer::new(source));
    world
  }

  /// The definition `text` at `path`, its clips the tone at each path in
  /// `clips`, loaded at once
  fn load(path: &str, text: &str, clips: &[&str]) -> Result<Sound, String> {
    let files = MemorySource::new();
    files.insert(path, text);
    for clip in clips {
      files.insert(clip, tone());
    }
    let mut world = registry_over(files);
    let mut assets = AssetsView::new(&mut world);
    let sound = assets.load_now::<Sound>(path).map_err(|e| e.report())?;
    assets
      .get(&sound)
      .cloned()
      .ok_or_else(|| "not in the store".to_string())
  }

  fn loaded(path: &str, text: &str, clips: &[&str]) -> Sound {
    match load(path, text, clips) {
      Ok(sound) => sound,
      Err(error) => panic!("{error}"),
    }
  }

  fn failure(path: &str, text: &str, clips: &[&str]) -> String {
    match load(path, text, clips) {
      Ok(sound) => panic!("{text} loaded: {sound:?}"),
      Err(error) => error,
    }
  }

  #[test]
  fn every_field_reads_as_written() {
    let text = r#"(
      clips: [(path: "sfx/step1.wav", weight: 3.0), (path: "sfx/step2.wav", region: (start: 0.05, end: 0.2))],
      bus: "sfx/footsteps",
      volume: -6.0,
      volume_range: 2.0,
      pitch: 1.5,
      pitch_range: 1.0,
      no_repeat: 1,
      limit: (max: 4, rule: StopQuietest),
      priority: 64,
      cooldown: 0.03,
      looping: false,
      fade_in: 0.01,
      fade_out: 0.05,
    )"#;
    let sound = loaded(
      "sfx/step.sound.ron",
      text,
      &["sfx/step1.wav", "sfx/step2.wav"],
    );
    assert_eq!(sound.clips.len(), 2);
    assert_eq!(sound.clips[0].weight, 3.0);
    assert_eq!(sound.clips[0].region, None);
    assert_eq!(sound.clips[1].weight, 1.0, "a weight left out is 1");
    assert_eq!(sound.clips[1].region, Some(0.05..0.2));
    assert_eq!(sound.bus, "sfx/footsteps");
    assert_eq!(sound.volume_db, -6.0);
    assert_eq!(sound.volume_range_db, 2.0);
    assert_eq!(sound.pitch, 1.5);
    assert_eq!(sound.pitch_range, 1.0);
    assert_eq!(sound.no_repeat, 1);
    assert_eq!(
      sound.limit,
      Some(VoiceLimit {
        max: 4,
        rule: LimitRule::StopQuietest
      })
    );
    assert_eq!(sound.priority, 64);
    assert_eq!(sound.cooldown, Duration::from_millis(30));
    assert!(!sound.looping);
    assert_eq!(sound.fade_in, Duration::from_millis(10));
    assert_eq!(sound.fade_out, Duration::from_millis(50));
    assert_eq!(sound.spatial, None);
  }

  #[test]
  fn every_field_but_clips_has_a_default() {
    let sound = loaded(
      "sfx/plain.sound.ron",
      r#"(clips: [(path: "sfx/plain.wav")])"#,
      &["sfx/plain.wav"],
    );
    let empty = Sound::default();
    assert_eq!(sound.clips.len(), 1);
    assert_eq!(sound.clips[0].weight, 1.0);
    assert_eq!(sound.clips[0].loop_region, None);
    assert_eq!(sound.bus, "sfx");
    assert_eq!(sound.bus, empty.bus);
    assert_eq!(
      (sound.volume_db, sound.volume_range_db),
      (empty.volume_db, empty.volume_range_db)
    );
    assert_eq!(
      (sound.pitch, sound.pitch_range),
      (empty.pitch, empty.pitch_range)
    );
    assert_eq!(sound.no_repeat, 0);
    assert_eq!(sound.limit, None);
    assert_eq!(sound.priority, Sound::DEFAULT_PRIORITY);
    assert_eq!(sound.cooldown, Duration::ZERO);
    assert!(!sound.looping);
    assert_eq!(
      (sound.fade_in, sound.fade_out),
      (Duration::ZERO, Duration::ZERO)
    );
    assert_eq!(sound.spatial, None);
    // A limit's rule left out is StopOldest
    let limited = loaded(
      "sfx/limited.sound.ron",
      r#"(clips: [(path: "sfx/limited.wav")], limit: (max: 2))"#,
      &["sfx/limited.wav"],
    );
    assert_eq!(
      limited.limit,
      Some(VoiceLimit {
        max: 2,
        rule: LimitRule::StopOldest
      })
    );
  }

  /// A definition's `mode` loads its clips with that mode: the same clip
  /// assets as loading the files with those settings
  #[test]
  fn a_definition_can_force_its_clips_to_decode_or_stream() {
    let cases = [
      ("", ClipMode::Auto),
      (", mode: Auto", ClipMode::Auto),
      (", mode: Decode", ClipMode::Decode),
      (", mode: Stream", ClipMode::Stream),
    ];
    for (field, mode) in cases {
      let files = MemorySource::new();
      let text = format!(r#"(clips: [(path: "sfx/forced.wav")]{field})"#);
      files.insert("sfx/forced.sound.ron", text);
      files.insert("sfx/forced.wav", tone());
      let mut world = registry_over(files);
      let sound = match AssetsView::new(&mut world).load_now::<Sound>("sfx/forced.sound.ron") {
        Ok(sound) => sound,
        Err(error) => panic!("{field}: {}", error.report()),
      };
      let clip = world
        .resource::<Assets<Sound>>()
        .get(&sound)
        .map(|sound| sound.clips[0].clip.clone());
      let settings = AudioClipSettings { mode };
      let direct = world
        .resource::<AssetServer>()
        .load_with::<AudioClip, _>("sfx/forced.wav", settings);
      assert_eq!(clip.as_ref(), Some(&direct), "{field}");
      // The tone is 250 ms: only Stream streams it (and the web decodes all)
      let streamed = world
        .resource::<Assets<AudioClip>>()
        .get(&direct)
        .map(AudioClip::is_streamed);
      let expected = mode == ClipMode::Stream && !cfg!(target_arch = "wasm32");
      assert_eq!(streamed, Some(expected), "{field}");
    }
  }

  #[test]
  fn spatial_settings_read_with_their_defaults() {
    let sound = loaded(
      "sfx/near.sound.ron",
      r#"(clips: [(path: "sfx/near.wav")], spatial: (min_distance: 50.0, max_distance: 800.0))"#,
      &["sfx/near.wav"],
    );
    assert_eq!(
      sound.spatial,
      Some(Spatial {
        min_distance: 50.0,
        max_distance: 800.0,
        curve: Falloff::Inverse,
        pan_width: 800.0,
        low_pass_at_max: None,
      })
    );
    let full = r#"(clips: [(path: "sfx/far.wav")], looping: true, spatial: (min_distance: 10.0, max_distance: 300.0, curve: Log, pan_width: 120.0, low_pass_at_max: 2000.0))"#;
    let sound = loaded("sfx/far.sound.ron", full, &["sfx/far.wav"]);
    assert_eq!(
      sound.spatial,
      Some(Spatial {
        min_distance: 10.0,
        max_distance: 300.0,
        curve: Falloff::Log,
        pan_width: 120.0,
        low_pass_at_max: Some(2000.0),
      })
    );
    assert!(sound.looping);
  }

  /// Each mistake fails the load as a decode error naming the file and what
  /// is wrong
  #[test]
  fn a_mistake_fails_naming_the_file_and_what_is_wrong() {
    let cases: &[(&str, &str)] = &[
      ("(clips: [])", "at least one clip"),
      (r#"(clips: [(path: "m.wav", weight: -1.0)])"#, "weight"),
      (r#"(clips: [(path: "m.wav", weight: 0.0)])"#, "weight"),
      (
        r#"(clips: [(path: "m.wav", region: (start: 0.2, end: 0.1))])"#,
        "region",
      ),
      (
        r#"(clips: [(path: "m.wav", loop_region: (start: 0.0, end: 0.1))])"#,
        "looping",
      ),
      (r#"(clips: [(path: "m.wav")], bus: "sxf")"#, "bus"),
      (r#"(clips: [(path: "m.wav")], bus: "sfx/")"#, "bus"),
      (r#"(clips: [(path: "m.wav")], volum: -3.0)"#, "volum"),
      (r#"(clips: [(path: "m.wav")], cooldown: -0.1)"#, "cooldown"),
      (r#"(clips: [(path: "m.wav")], fade_out: inf)"#, "fade_out"),
      (
        r#"(clips: [(path: "m.wav")], volume_range: -2.0)"#,
        "volume_range",
      ),
      (r#"(clips: [(path: "m.wav")], pitch: NaN)"#, "pitch"),
      (r#"(clips: [(path: "m.wav")], limit: (max: 0))"#, "max"),
      (r#"(clips: [(path: "m.wav")], mode: Streamed)"#, "Streamed"),
      (
        r#"(clips: [(path: "m.wav")], spatial: (min_distance: 50.0, max_distance: 10.0))"#,
        "max_distance",
      ),
      (
        r#"(clips: [(path: "m.wav")], spatial: (min_distance: 0.0, max_distance: 10.0, pan_width: 0.0))"#,
        "pan_width",
      ),
      (r#"(clips: [(path: "../m.wav")])"#, "../m.wav"),
      ("(clips: [(path: ", "1:"),
    ];
    for (text, names) in cases {
      let error = failure("sfx/mistake.sound.ron", text, &["m.wav"]);
      assert!(
        error.starts_with("could not load \"sfx/mistake.sound.ron\""),
        "{text}: {error}"
      );
      assert!(error.contains(names), "{text}: {error}");
    }
  }

  #[test]
  fn a_region_past_the_clip_s_end_fails_the_check() {
    // The tone is 250 ms
    let past = r#"(clips: [(path: "sfx/short.wav", region: (start: 0.1, end: 0.5))])"#;
    let error = failure("sfx/past.sound.ron", past, &["sfx/short.wav"]);
    assert!(error.contains("sfx/short.wav"), "{error}");
    assert!(error.contains("region"), "{error}");
    let looped = r#"(clips: [(path: "sfx/short.wav", region: (start: 0.0, end: 0.1), loop_region: (start: 0.05, end: 0.2))], looping: true)"#;
    let error = failure("sfx/looped.sound.ron", looped, &["sfx/short.wav"]);
    assert!(error.contains("loop_region"), "{error}");
    // Inside it, it loads
    let inside = r#"(clips: [(path: "sfx/short.wav", region: (start: 0.05, end: 0.25), loop_region: (start: 0.1, end: 0.2))], looping: true)"#;
    assert!(load("sfx/inside.sound.ron", inside, &["sfx/short.wav"]).is_ok());
  }

  #[test]
  fn a_missing_clip_fails_the_sound_naming_both() {
    let error = failure(
      "sfx/lost.sound.ron",
      r#"(clips: [(path: "sfx/nowhere.wav")])"#,
      &[],
    );
    assert!(error.contains("sfx/lost.sound.ron"), "{error}");
    assert!(error.contains("sfx/nowhere.wav"), "{error}");
  }

  /// Files whose clip `held` is not read until the test lets it go
  struct Held {
    files: MemorySource,
    held: &'static str,
    gate: crossbeam_channel::Receiver<()>,
  }

  impl AssetSource for Held {
    fn read(&self, path: &AssetPath) -> Result<Vec<u8>, crate::core::asset::AssetError> {
      // Returns when the test sends, or drops its sender
      if path.as_str() == self.held {
        let _ = self.gate.recv();
      }
      self.files.read(path)
    }
  }

  fn pump_until(world: &mut World, done: impl Fn(&World) -> bool) {
    let start = Instant::now();
    loop {
      pump(world);
      if done(world) {
        return;
      }
      assert!(
        start.elapsed() < Duration::from_secs(5),
        "timed out waiting"
      );
      std::thread::sleep(Duration::from_millis(1));
    }
  }

  #[test]
  fn a_definition_loads_with_its_clips_as_dependencies() {
    let (release, gate) = crossbeam_channel::unbounded();
    let files = MemorySource::new();
    files.insert(
      "sfx/two.sound.ron",
      r#"(clips: [(path: "sfx/one.wav"), (path: "sfx/held.wav")])"#,
    );
    files.insert("sfx/one.wav", tone());
    files.insert("sfx/held.wav", tone());
    let mut world = registry_over(Held {
      files,
      held: "sfx/held.wav",
      gate,
    });
    let server = world.resource::<AssetServer>().clone();
    // One worker: the definition is read, then its clips in order
    server.limit_workers(1);
    let sound = server.load::<Sound>("sfx/two.sound.ron");
    let one = server.load::<AudioClip>("sfx/one.wav");
    pump_until(&mut world, |_| server.is_ready(&one));
    pump_until(&mut world, |_| server.waiting_count() == 1);
    assert!(!server.is_ready(&sound), "a clip is still loading");
    assert!(world.resource::<Assets<Sound>>().get(&sound).is_none());
    let _ = release.send(());
    pump_until(&mut world, |_| server.is_ready(&sound));
    let held = server.load::<AudioClip>("sfx/held.wav");
    assert!(server.is_ready(&held));
    let clips: Vec<_> = world
      .resource::<Assets<Sound>>()
      .get(&sound)
      .map(|sound| sound.clips.iter().map(|clip| clip.clip.clone()).collect())
      .unwrap_or_default();
    assert_eq!(clips, [one, held]);
  }
}
