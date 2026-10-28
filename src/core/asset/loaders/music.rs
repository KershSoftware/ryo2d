//! Music definitions from `.music.ron` text

use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;

use kira::sound::static_sound::StaticSoundData;
use ron::extensions::Extensions;
use serde::Deserialize;

use crate::core::asset::{
  AssetError, AssetLoader, AssetPath, AssetServer, Assets, AudioClipSettings, CheckContext,
  ClipMode, LoadContext,
};
use crate::core::audio::{AudioClip, Music, MusicData, MusicJoinError, check_bus_name, join};

/// Loads [`Music`] from `.music.ron` files: an intro and a loop, or one
/// clip with a loop region. Every field but the clips may be left out:
///
/// ```ron
/// (
///   intro: "music/level_intro.ogg",       // plays once
///   loop: "music/level_loop.ogg",         // then repeats, with no gap
///   volume: -6.0,                         // 0.0 dB
///   fade_in: 0.5,                         // 0.0 s, starting over silence
///   crossfade: 2.0,                       // 1.0 s, taking over from a track
///   tempo: 128.0,                         // none (kept for later)
///   meter: (4, 4),                        // none (kept for later)
///   bus: "music/combat",                  // "music"
/// )
/// ```
///
/// or
///
/// ```ron
/// (
///   clip: "music/boss.ogg",
///   loop_start: 12.5,                     // 0.0 s
///   loop_end: None,                       // the clip's end
///   mode: Stream,                         // Auto
/// )
/// ```
///
/// - **An intro and a loop** are read and decoded on the loader's thread
///   and joined into one sound, so the loop starts on the sample after the
///   intro's last and repeats from there: a seamless loop on kira's clock,
///   which a second sound scheduled at the intro's end could not give (kira
///   starts sounds on its 128-frame chunks). Both must be at one sample
///   rate (a mismatch fails the load, naming both files) and are held
///   decoded, so they do not stream; `mode: Stream` is refused for them.
///   They are read, not loaded as dependencies (nothing else can use them as
///   assets), but saving one reloads the music, as saving the music file
///   does.
/// - **One clip** loops from `loop_start` to `loop_end` (seconds; what comes
///   before `loop_start` plays once), on kira's loop region. The clip is a
///   dependency, loaded as `mode` says ([`ClipMode`]: `Auto` decodes a clip
///   of up to 10 s and streams a longer one, `Decode` and `Stream` force
///   one or the other; the web decodes every clip), and the loop must lie
///   inside it.
/// - **The bus** is `music` or a bus under it, such as `"music/combat"`.
///
/// An option is written without `Some(..)`. A clip that is missing or will
/// not decode fails the music naming both (`AssetError::Dependency`);
/// any other mistake fails the load with `AssetError::Decode`, naming the
/// file and what is wrong. Every registry has it
#[derive(Debug, Default, Clone, Copy)]
pub struct MusicLoader;

impl AssetLoader for MusicLoader {
  type Asset = Music;
  type Settings = ();

  fn extensions(&self) -> &[&'static str] {
    &["music.ron"]
  }

  fn load(&self, bytes: Vec<u8>, _: &(), cx: &mut LoadContext<'_>) -> Result<Music, AssetError> {
    let path = cx.path().clone();
    let text = String::from_utf8(bytes).map_err(|error| AssetError::decode(&path, error))?;
    let file: MusicFile = ron::Options::default()
      .with_default_extension(Extensions::IMPLICIT_SOME)
      .from_str(&text)
      .map_err(|error| AssetError::decode(&path, error))?;
    let form = file
      .check()
      .map_err(|error| AssetError::decode(&path, error))?;
    let data = match form {
      Form::Joined { intro, looped } => {
        let (intro_path, intro) = decode(cx, &path, intro)?;
        let (loop_path, looped) = decode(cx, &path, looped)?;
        let joined = join(&intro, &looped).map_err(|error| {
          let why = match error {
            MusicJoinError::Rates { intro, looped } => format!(
              "the intro {intro_path:?} is at {intro} Hz and the loop {loop_path:?} at {looped} Hz: an intro and its loop must share a sample rate to join with no gap"
            ),
            MusicJoinError::EmptyLoop => format!("the loop {loop_path:?} has no frames"),
            // Both were decoded here, so neither is streamed
            other => other.to_string(),
          };
          AssetError::decode(&path, why)
        })?;
        MusicData::Concatenated(Box::new(joined))
      }
      Form::Looping {
        clip,
        loop_start,
        loop_end,
      } => {
        let settings = AudioClipSettings { mode: file.mode };
        let clip = cx
          .load_with::<AudioClip, _>(clip, settings)
          .map_err(|error| match error {
            AssetError::Dependency { .. } => error,
            // A path the registry refused (one that leaves the asset root,
            // or no loader for it) is the definition's mistake
            refused => {
              AssetError::decode(&path, format!("the clip {clip:?}: {}", refused.report()))
            }
          })?;
        MusicData::Looping {
          clip,
          loop_start,
          loop_end,
        }
      }
    };
    Ok(Music {
      data,
      volume_db: file.volume,
      fade_in: seconds(file.fade_in),
      crossfade: seconds(file.crossfade),
      tempo: file.tempo,
      meter: file.meter,
      bus: file.bus,
    })
  }

  /// A one-clip loop inside its clip
  fn check(&self, music: &Music, cx: &CheckContext<'_>) -> Result<(), AssetError> {
    let MusicData::Looping {
      clip,
      loop_start,
      loop_end,
    } = music.data()
    else {
      return Ok(());
    };
    let Some(clips) = cx.world().get_resource::<Assets<AudioClip>>() else {
      return Ok(());
    };
    let Some(length) = clips.get(clip).map(|clip| clip.duration().as_secs_f64()) else {
      return Ok(());
    };
    let named = || {
      cx.world()
        .get_resource::<AssetServer>()
        .and_then(|server| server.path(clip))
        .map_or_else(|| format!("{:?}", clip.id()), |path| path.to_string())
    };
    if *loop_start >= length {
      return Err(AssetError::decode(
        cx.path(),
        format!(
          "the clip {:?}: its loop_start {loop_start} s is not before the clip's end at {length} s",
          named()
        ),
      ));
    }
    if let Some(end) = loop_end
      && *end > length + TOLERANCE
    {
      return Err(AssetError::decode(
        cx.path(),
        format!(
          "the clip {:?}: its loop_end {end} s is past the clip's end at {length} s",
          named()
        ),
      ));
    }
    Ok(())
  }
}

/// How far past a clip's end a loop may reach, in seconds, for rounding
const TOLERANCE: f64 = 1e-6;

/// The file at `clip`, read and decoded on the loader's thread for joining:
/// its path and its frames. A file that is missing or will not decode fails
/// `music` naming both, as a dependency would; a path that is not one (it
/// leaves the asset root, say) is the definition's mistake
fn decode(
  cx: &LoadContext<'_>,
  music: &AssetPath,
  clip: &str,
) -> Result<(AssetPath, StaticSoundData), AssetError> {
  let path = AssetPath::parse(clip)
    .map_err(|error| AssetError::decode(music, format!("the clip {clip:?}: {}", error.report())))?;
  let needs = |error: AssetError| AssetError::Dependency {
    path: music.clone(),
    dependency: path.clone(),
    error: Arc::new(error),
  };
  let bytes = cx.read(&path).map_err(needs)?;
  let data = StaticSoundData::from_cursor(Cursor::new(bytes))
    .map_err(|error| needs(AssetError::decode(&path, error)))?;
  Ok((path, data))
}

/// A time the file gives in seconds, already checked
fn seconds(value: f64) -> Duration {
  Duration::try_from_secs_f64(value).unwrap_or_default()
}

/// A `.music.ron` file, as written
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MusicFile {
  #[serde(default)]
  intro: Option<String>,
  #[serde(default, rename = "loop")]
  looped: Option<String>,
  #[serde(default)]
  clip: Option<String>,
  #[serde(default)]
  loop_start: Option<f64>,
  #[serde(default)]
  loop_end: Option<f64>,
  #[serde(default)]
  volume: f32,
  #[serde(default)]
  fade_in: f64,
  #[serde(default = "default_crossfade")]
  crossfade: f64,
  #[serde(default)]
  tempo: Option<f32>,
  #[serde(default)]
  meter: Option<(u8, u8)>,
  #[serde(default = "default_bus")]
  bus: String,
  #[serde(default)]
  mode: ClipMode,
}

fn default_crossfade() -> f64 {
  Music::DEFAULT_CROSSFADE.as_secs_f64()
}

fn default_bus() -> String {
  "music".to_string()
}

/// What a file plays, checked
enum Form<'a> {
  Joined {
    intro: &'a str,
    looped: &'a str,
  },
  Looping {
    clip: &'a str,
    loop_start: f64,
    loop_end: Option<f64>,
  },
}

impl MusicFile {
  /// What RON cannot say is wrong: which clips it names, the loop, the
  /// numbers' ranges, the bus
  fn check(&self) -> Result<Form<'_>, String> {
    let form = self.form()?;
    if !self.volume.is_finite() {
      return Err(format!("volume: {} is not a number", self.volume));
    }
    for (field, value) in [("fade_in", self.fade_in), ("crossfade", self.crossfade)] {
      if !(value.is_finite() && value >= 0.0) || Duration::try_from_secs_f64(value).is_err() {
        return Err(format!(
          "{field}: {value} is not a time in seconds (0 or more)"
        ));
      }
    }
    if let Some(tempo) = self.tempo
      && !(tempo.is_finite() && tempo > 0.0)
    {
      return Err(format!(
        "tempo: {tempo} is not a number of beats per minute above 0"
      ));
    }
    if let Some((beats, unit)) = self.meter
      && (beats == 0 || unit == 0)
    {
      return Err(format!(
        "meter: ({beats}, {unit}) is not beats to the bar and a note value, each 1 or more"
      ));
    }
    check_bus_name(&self.bus)?;
    if self.bus != "music" && !self.bus.starts_with("music/") {
      return Err(format!(
        "the bus {:?} is not music or a bus under it, such as \"music/combat\"",
        self.bus
      ));
    }
    Ok(form)
  }

  /// An intro and a loop, or one clip with its loop
  fn form(&self) -> Result<Form<'_>, String> {
    match (&self.intro, &self.looped, &self.clip) {
      (Some(intro), Some(looped), None) => {
        if self.loop_start.is_some() || self.loop_end.is_some() {
          return Err(
            "loop_start and loop_end are for one clip (`clip: ..`): an intro and a loop loop the whole loop clip".into(),
          );
        }
        if self.mode == ClipMode::Stream {
          return Err(
            "mode: Stream is for one clip (`clip: ..`): an intro and a loop are joined, so they decode".into(),
          );
        }
        Ok(Form::Joined { intro, looped })
      }
      (None, None, Some(clip)) => {
        let loop_start = self.loop_start.unwrap_or(0.0);
        if !(loop_start.is_finite() && loop_start >= 0.0) {
          return Err(format!(
            "loop_start: {loop_start} is not a time in seconds (0 or more)"
          ));
        }
        if let Some(end) = self.loop_end
          && !(end.is_finite() && end > loop_start)
        {
          return Err(format!(
            "loop_end: {end} is not a time in seconds after loop_start ({loop_start})"
          ));
        }
        Ok(Form::Looping {
          clip,
          loop_start,
          loop_end: self.loop_end,
        })
      }
      (Some(_), None, None) => {
        Err(r#"an intro needs a loop to follow it (`loop: "music/theme_loop.ogg"`)"#.into())
      }
      (None, Some(_), None) => Err(
        r#"a loop needs an intro (`intro: "music/theme_intro.ogg"`); a track that only loops is one clip (`clip: "music/theme.ogg"`)"#.into(),
      ),
      (None, None, None) => Err(
        r#"music needs a clip (`clip: "music/theme.ogg"`), or an intro and a loop (`intro: "music/a_intro.ogg", loop: "music/a_loop.ogg"`)"#.into(),
      ),
      _ => Err(
        "music is one clip (`clip: ..`) or an intro and a loop (`intro: .., loop: ..`), not both".into(),
      ),
    }
  }
}

#[cfg(test)]
mod tests {
  use std::io::Cursor;
  use std::time::Duration;

  use kira::sound::static_sound::StaticSoundData;
  use kira::sound::{EndPosition, PlaybackPosition, Region};

  use crate::core::asset::{
    AssetServer, Assets, AssetsView, AudioClipSettings, ClipMode, MemorySource, insert_registry,
  };
  use crate::core::audio::{AudioClip, Music, MusicData};
  use crate::core::ecs::kernel::World;

  const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/audio");

  fn fixture(name: &str) -> Vec<u8> {
    match std::fs::read(format!("{FIXTURES}/{name}")) {
      Ok(bytes) => bytes,
      Err(error) => panic!("{name}: {error}"),
    }
  }

  /// A mono 16-bit WAV file of `frames` frames at `rate`, every sample
  /// `value`
  fn wav(rate: u32, frames: usize, value: i16) -> Vec<u8> {
    let data = u32::try_from(frames * 2).unwrap_or(u32::MAX);
    let mut bytes = Vec::with_capacity(44 + frames * 2);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    // PCM, one channel
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * 2).to_le_bytes());
    // Two bytes a frame, 16 bits a sample
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data.to_le_bytes());
    for _ in 0..frames {
      bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
  }

  /// `files` (paths and bytes), the definition `text` at `path` among them,
  /// loaded at once
  fn load(path: &str, text: &str, files: &[(&str, Vec<u8>)]) -> Result<(World, Music), String> {
    let source = MemorySource::new();
    source.insert(path, text);
    for (name, bytes) in files {
      source.insert(name, bytes.clone());
    }
    let mut world = World::new();
    insert_registry(&mut world, AssetServer::new(source));
    let mut assets = AssetsView::new(&mut world);
    let music = assets.load_now::<Music>(path).map_err(|e| e.report())?;
    let music = assets
      .get(&music)
      .cloned()
      .ok_or_else(|| "not in the store".to_string())?;
    Ok((world, music))
  }

  fn loaded(path: &str, text: &str, files: &[(&str, Vec<u8>)]) -> (World, Music) {
    match load(path, text, files) {
      Ok(loaded) => loaded,
      Err(error) => panic!("{error}"),
    }
  }

  fn failure(path: &str, text: &str, files: &[(&str, Vec<u8>)]) -> String {
    match load(path, text, files) {
      Ok((_, music)) => panic!("{text} loaded: {music:?}"),
      Err(error) => error,
    }
  }

  fn joined(music: &Music) -> &StaticSoundData {
    match music.data() {
      MusicData::Concatenated(data) => data,
      other => panic!("not joined: {other:?}"),
    }
  }

  fn decoded(bytes: Vec<u8>) -> StaticSoundData {
    match StaticSoundData::from_cursor(Cursor::new(bytes)) {
      Ok(data) => data,
      Err(error) => panic!("{error}"),
    }
  }

  /// The intro's frames, then the loop's, in one sound whose loop region
  /// starts on the frame after the intro's last: the loop follows it with no
  /// gap, and repeats from there
  #[test]
  fn intro_and_loop_concatenate_with_the_loop_region_at_the_intro_s_end() {
    let text = r#"(intro: "music/a_intro.wav", loop: "music/a_loop.wav")"#;
    let files = [
      ("music/a_intro.wav", wav(22_050, 3_000, 8_000)),
      ("music/a_loop.wav", wav(22_050, 5_000, -8_000)),
    ];
    let (_, music) = loaded("music/a.music.ron", text, &files);
    let data = joined(&music);
    assert_eq!(data.sample_rate, 22_050);
    assert_eq!(data.frames.len(), 8_000, "the sum of both");
    assert_eq!(
      data.settings.loop_region,
      Some(Region {
        start: PlaybackPosition::Samples(3_000),
        end: EndPosition::EndOfAudio,
      })
    );
    assert_eq!(data.slice, None);
    let level = 8_000.0 / 32_768.0;
    assert!(
      (data.frames[2_999].left - level).abs() < 1e-4,
      "the intro's last"
    );
    assert!(
      (data.frames[3_000].left + level).abs() < 1e-4,
      "the loop's first"
    );
    assert!(
      (data.frames[7_999].left + level).abs() < 1e-4,
      "the loop's last"
    );
    // Files of two formats, mono and stereo, at one rate
    let text = r#"(intro: "music/b_intro.wav", loop: "music/b_loop.ogg")"#;
    let files = [
      ("music/b_intro.wav", fixture("tone.wav")),
      ("music/b_loop.ogg", fixture("tone.ogg")),
    ];
    let (_, music) = loaded("music/b.music.ron", text, &files);
    let (intro, looped) = (decoded(fixture("tone.wav")), decoded(fixture("tone.ogg")));
    let data = joined(&music);
    assert_eq!(data.frames.len(), intro.frames.len() + looped.frames.len());
    assert_eq!(
      data.settings.loop_region.map(|region| region.start),
      Some(PlaybackPosition::Samples(intro.frames.len()))
    );
  }

  #[test]
  fn mismatched_rates_fail_naming_both() {
    let text = r#"(intro: "music/m_intro.wav", loop: "music/m_loop.wav")"#;
    let files = [
      ("music/m_intro.wav", wav(22_050, 2_000, 100)),
      ("music/m_loop.wav", wav(44_100, 2_000, 100)),
    ];
    let error = failure("music/m.music.ron", text, &files);
    assert!(
      error.starts_with("could not load \"music/m.music.ron\""),
      "{error}"
    );
    for named in ["music/m_intro.wav", "music/m_loop.wav", "22050", "44100"] {
      assert!(error.contains(named), "{named}: {error}");
    }
  }

  #[test]
  fn one_clip_music_loops_its_clip_from_loop_start() {
    let text = r#"(clip: "music/b.wav", loop_start: 0.1, loop_end: None)"#;
    let (world, music) = loaded(
      "music/b.music.ron",
      text,
      &[("music/b.wav", fixture("tone.wav"))],
    );
    let MusicData::Looping {
      clip,
      loop_start,
      loop_end,
    } = music.data()
    else {
      panic!("not one clip: {music:?}");
    };
    assert_eq!(*loop_start, 0.1);
    assert_eq!(*loop_end, None);
    let direct = world
      .resource::<AssetServer>()
      .load_with::<AudioClip, _>("music/b.wav", AudioClipSettings::default());
    assert_eq!(clip, &direct, "its clip is a dependency, loaded with Auto");
    let text = r#"(clip: "music/b.wav", loop_start: 0.05, loop_end: 0.2)"#;
    let (_, music) = loaded(
      "music/c.music.ron",
      text,
      &[("music/b.wav", fixture("tone.wav"))],
    );
    assert!(
      matches!(music.data(), MusicData::Looping { loop_start, loop_end: Some(end), .. } if *loop_start == 0.05 && *end == 0.2),
      "{music:?}"
    );
  }

  #[test]
  fn every_field_reads_as_written() {
    let text = r#"(
      intro: "music/f_intro.wav",
      loop: "music/f_loop.wav",
      volume: -6.0,
      fade_in: 0.5,
      crossfade: 2.0,
      tempo: 128.0,
      meter: (3, 4),
      bus: "music/combat",
      mode: Decode,
    )"#;
    let files = [
      ("music/f_intro.wav", wav(48_000, 100, 1)),
      ("music/f_loop.wav", wav(48_000, 100, 1)),
    ];
    let (_, music) = loaded("music/f.music.ron", text, &files);
    assert_eq!(music.volume_db, -6.0);
    assert_eq!(music.fade_in, Duration::from_millis(500));
    assert_eq!(music.crossfade, Duration::from_secs(2));
    assert_eq!(music.tempo, Some(128.0));
    assert_eq!(music.meter, Some((3, 4)));
    assert_eq!(music.bus, "music/combat");
    assert_eq!(joined(&music).frames.len(), 200);
  }

  #[test]
  fn every_field_but_the_clips_has_a_default() {
    let (_, music) = loaded(
      "music/plain.music.ron",
      r#"(clip: "music/plain.wav")"#,
      &[("music/plain.wav", fixture("tone.wav"))],
    );
    assert_eq!(music.volume_db, 0.0);
    assert_eq!(music.fade_in, Duration::ZERO);
    assert_eq!(music.crossfade, Music::DEFAULT_CROSSFADE);
    assert_eq!(music.tempo, None);
    assert_eq!(music.meter, None);
    assert_eq!(music.bus, "music");
    assert!(
      matches!(
        music.data(),
        MusicData::Looping {
          loop_start,
          loop_end: None,
          ..
        } if *loop_start == 0.0
      ),
      "{music:?}"
    );
  }

  /// A one-clip music's `mode` loads its clip with that mode: the same clip
  /// asset as loading the file with those settings
  #[test]
  fn a_definition_can_force_its_clip_to_decode_or_stream() {
    for (field, mode) in [
      ("", ClipMode::Auto),
      (", mode: Decode", ClipMode::Decode),
      (", mode: Stream", ClipMode::Stream),
    ] {
      let text = format!(r#"(clip: "music/forced.wav"{field})"#);
      let (world, music) = loaded(
        "music/forced.music.ron",
        &text,
        &[("music/forced.wav", fixture("tone.wav"))],
      );
      let MusicData::Looping { clip, .. } = music.data() else {
        panic!("{field}: not one clip");
      };
      let direct = world
        .resource::<AssetServer>()
        .load_with::<AudioClip, _>("music/forced.wav", AudioClipSettings { mode });
      assert_eq!(clip, &direct, "{field}");
      let streamed = world
        .resource::<Assets<AudioClip>>()
        .get(&direct)
        .map(AudioClip::is_streamed);
      let expected = mode == ClipMode::Stream && !cfg!(target_arch = "wasm32");
      assert_eq!(streamed, Some(expected), "{field}");
    }
  }

  /// Each mistake fails the load as a decode error naming the file and what
  /// is wrong
  #[test]
  fn a_mistake_fails_naming_the_file_and_what_is_wrong() {
    let cases: &[(&str, &str)] = &[
      ("()", "clip"),
      (r#"(intro: "i.wav")"#, "loop"),
      (r#"(loop: "l.wav")"#, "intro"),
      (r#"(intro: "i.wav", loop: "l.wav", clip: "c.wav")"#, "clip"),
      (
        r#"(intro: "i.wav", loop: "l.wav", loop_start: 1.0)"#,
        "loop_start",
      ),
      (r#"(intro: "i.wav", loop: "l.wav", mode: Stream)"#, "Stream"),
      (r#"(clip: "c.wav", loop_start: -1.0)"#, "loop_start"),
      (
        r#"(clip: "c.wav", loop_start: 0.2, loop_end: 0.1)"#,
        "loop_end",
      ),
      (r#"(clip: "c.wav", bus: "sfx")"#, "bus"),
      (r#"(clip: "c.wav", bus: "music/")"#, "bus"),
      (r#"(clip: "c.wav", volume: NaN)"#, "volume"),
      (r#"(clip: "c.wav", fade_in: -0.5)"#, "fade_in"),
      (r#"(clip: "c.wav", crossfade: inf)"#, "crossfade"),
      (r#"(clip: "c.wav", tempo: 0.0)"#, "tempo"),
      (r#"(clip: "c.wav", meter: (0, 4))"#, "meter"),
      (r#"(clip: "c.wav", volum: -3.0)"#, "volum"),
      (r#"(clip: "c.wav", mode: Streamed)"#, "Streamed"),
      (r#"(clip: "../c.wav")"#, "../c.wav"),
      (r#"(intro: "../i.wav", loop: "l.wav")"#, "../i.wav"),
      ("(clip: ", "1:"),
    ];
    let files = [
      ("c.wav", fixture("tone.wav")),
      ("i.wav", fixture("tone.wav")),
      ("l.wav", fixture("tone.wav")),
    ];
    for (text, names) in cases {
      let error = failure("music/mistake.music.ron", text, &files);
      assert!(
        error.starts_with("could not load \"music/mistake.music.ron\""),
        "{text}: {error}"
      );
      assert!(error.contains(names), "{text}: {error}");
    }
  }

  #[test]
  fn a_loop_past_the_clip_s_end_fails_the_check() {
    // The tone is 250 ms
    let files = [("music/short.wav", fixture("tone.wav"))];
    for text in [
      r#"(clip: "music/short.wav", loop_start: 0.3)"#,
      r#"(clip: "music/short.wav", loop_start: 0.1, loop_end: 0.5)"#,
    ] {
      let error = failure("music/past.music.ron", text, &files);
      assert!(error.contains("music/short.wav"), "{text}: {error}");
      assert!(error.contains("loop_"), "{text}: {error}");
    }
    let inside = r#"(clip: "music/short.wav", loop_start: 0.1, loop_end: 0.25)"#;
    assert!(load("music/inside.music.ron", inside, &files).is_ok());
  }

  #[test]
  fn a_missing_clip_fails_naming_both() {
    for text in [
      r#"(clip: "music/nowhere.wav")"#,
      r#"(intro: "music/nowhere.wav", loop: "music/here.wav")"#,
      r#"(intro: "music/here.wav", loop: "music/nowhere.wav")"#,
    ] {
      let error = failure(
        "music/lost.music.ron",
        text,
        &[("music/here.wav", fixture("tone.wav"))],
      );
      assert!(error.contains("music/lost.music.ron"), "{text}: {error}");
      assert!(error.contains("music/nowhere.wav"), "{text}: {error}");
    }
  }

  #[test]
  fn an_undecodable_intro_fails_naming_both() {
    let text = r#"(intro: "music/bad.wav", loop: "music/here.wav")"#;
    let files = [
      ("music/bad.wav", b"RIFF not a wave file".to_vec()),
      ("music/here.wav", fixture("tone.wav")),
    ];
    let error = failure("music/broken.music.ron", text, &files);
    assert!(error.contains("music/broken.music.ron"), "{error}");
    assert!(error.contains("music/bad.wav"), "{error}");
  }

  /// Review M5: a surround intro fails the music as a dependency, naming
  /// the file
  #[test]
  fn a_surround_intro_fails_as_a_dependency_naming_it() {
    let text = r#"(intro: "music/surround.wav", loop: "music/here.wav")"#;
    let files = [
      ("music/surround.wav", fixture("surround.wav")),
      ("music/here.wav", fixture("tone.wav")),
    ];
    let error = failure("music/wide.music.ron", text, &files);
    assert!(
      error.starts_with("\"music/wide.music.ron\" needs \"music/surround.wav\""),
      "{error}"
    );
  }

  /// Review M5: an intro and loop loaded on the workers (`load`, not
  /// `load_now`) are read and joined there
  #[test]
  fn an_intro_and_loop_load_and_join_on_the_workers() {
    let files = MemorySource::new();
    files.insert(
      "music/w.music.ron",
      r#"(intro: "music/w_intro.wav", loop: "music/w_loop.wav")"#,
    );
    files.insert("music/w_intro.wav", wav(22_050, 1_000, 10));
    files.insert("music/w_loop.wav", wav(22_050, 2_000, 20));
    let mut world = World::new();
    insert_registry(&mut world, AssetServer::new(files));
    let server = world.resource::<AssetServer>().clone();
    let music = server.load::<Music>("music/w.music.ron");
    let started = std::time::Instant::now();
    while !server.is_ready(&music) {
      assert!(started.elapsed() < Duration::from_secs(5), "timed out");
      crate::core::asset::pump(&mut world);
      std::thread::sleep(Duration::from_millis(1));
    }
    let loaded = world.resource::<Assets<Music>>().get(&music).cloned();
    let Some(loaded) = loaded else {
      panic!("not in the store");
    };
    let data = joined(&loaded);
    assert_eq!(data.frames.len(), 3_000);
    assert_eq!(
      data.settings.loop_region.map(|region| region.start),
      Some(PlaybackPosition::Samples(1_000))
    );
  }
}
