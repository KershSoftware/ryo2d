//! `audio_system`: the frame's audio work

use super::AudioEngine;
use super::engine::GameTime;
use crate::core::ecs::World;
use crate::core::ecs::kernel::Mut;
use crate::core::time::Time;

/// `PostUpdate`, after the cameras: applies the voices' commands queued this
/// frame (the scene's update has run), builds the bus tree from the mixer
/// when it lands or changes, switches what plays to the clips, sound
/// definitions and music the registry reloaded (read from its
/// `AssetEvent`s), follows the listener and the emitters (their
/// `GlobalTransform`s as this frame left them) and plays the emitters
/// spawned to play, follows the game clock (its pause and time scale;
/// hitstop leaves audio alone), reports the device's stream errors,
/// advances the mock backend by the frame's real time, and retires the
/// voices that ended. A world without an [`AudioEngine`] is left alone
#[doc(hidden)]
pub fn audio_system(world: &mut World) {
  let (real_delta, game) = world
    .get_resource::<Time>()
    .map(|time| (time.real.delta_duration(), GameTime::of(time)))
    .unwrap_or_default();
  // The engine is taken out of the world while it reads the stores
  world.try_resource_scope(|world: &mut World, mut engine: Mut<'_, AudioEngine>| {
    engine.prepare(world);
    engine.update(world, real_delta, game);
  });
}

#[cfg(test)]
mod tests {
  use std::time::{Duration, Instant};

  use kira::sound::PlaybackState;

  use crate::core::asset::{
    Asset, AssetEvent, AssetServer, Assets, AssetsView, Handle, MemorySource,
  };
  use crate::core::audio::engine::MIN_FADE;
  use crate::core::audio::{
    AudioClip, AudioEngine, AudioLogEntry, AudioLogKind, AudioView, BusDef, ClipData, DuckDef,
    EncodedClip, FocusLoss, Mixer, Music, PlayOptions, Sound, SoundClip, Voice,
  };
  use crate::core::dev_log::{self, LogLevel};
  use crate::core::ecs::World;
  use crate::core::ecs::kernel::Messages;
  use crate::core::testing::{audio_step, audio_world, audio_world_over, wav};
  use crate::core::time::Time;

  const FRAME: Duration = Duration::from_nanos(16_666_667);
  /// One of kira's chunks, in seconds: it applies changes once a chunk
  const CHUNK: f64 = 128.0 / 48_000.0;
  /// The standard buses that play voices
  const BUSES: [&str; 5] = ["music", "sfx", "ui", "voice", "ambience"];

  fn clip(world: &mut World, seconds: f32) -> Handle<AudioClip> {
    let frames = vec![[0.1, 0.1]; (48_000.0 * seconds) as usize];
    world
      .resource_mut::<Assets<AudioClip>>()
      .add(AudioClip::from_frames(48_000, &frames))
  }

  /// A loop of `clip` on `bus`
  fn looping(world: &mut World, clip: &Handle<AudioClip>, bus: &str) -> Handle<Sound> {
    world.resource_mut::<Assets<Sound>>().add(Sound {
      clips: vec![SoundClip::new(clip.clone())],
      bus: bus.to_string(),
      looping: true,
      ..Default::default()
    })
  }

  /// A loop playing on each standard bus, by bus
  fn play_on_each_bus(world: &mut World) -> Vec<(&'static str, Voice)> {
    let clip = clip(world, 2.0);
    BUSES
      .iter()
      .map(|&bus| {
        let sound = looping(world, &clip, bus);
        (bus, AudioView::new(world).play(&sound))
      })
      .collect()
  }

  fn time(world: &mut World) -> crate::core::ecs::Mut<'_, Time> {
    world.resource_mut::<Time>()
  }

  fn log(world: &World) -> Vec<AudioLogEntry> {
    world.resource::<AudioEngine>().log().to_vec()
  }

  /// The buses the log says were paused or resumed (`kind`), in order,
  /// with their fades
  fn bus_entries(log: &[AudioLogEntry], kind: AudioLogKind) -> Vec<(String, Duration)> {
    log
      .iter()
      .filter(|entry| entry.kind == kind && entry.voice == 0)
      .map(|entry| (entry.bus.clone(), entry.fade))
      .collect()
  }

  /// What kira says `voice`'s sound is doing
  fn playback(world: &World, voice: &Voice) -> Option<PlaybackState> {
    world.resource::<AudioEngine>().voice_playback(voice.id())
  }

  /// The rates `voice` was started at and given since, in order
  fn rates(world: &World, voice: &Voice) -> Vec<f64> {
    log(world)
      .iter()
      .filter(|entry| entry.voice == voice.id())
      .filter(|entry| matches!(entry.kind, AudioLogKind::Start | AudioLogKind::Rate))
      .map(|entry| entry.rate)
      .collect()
  }

  fn close(actual: &[f64], expected: &[f64]) -> bool {
    actual.len() == expected.len()
      && actual
        .iter()
        .zip(expected)
        .all(|(actual, expected)| (actual - expected).abs() < 1e-9)
  }

  fn position(world: &World, voice: &Voice) -> f64 {
    world
      .resource::<AudioEngine>()
      .voice_position(voice.id())
      .unwrap_or_default()
  }

  fn with_fade(buses: &[&str], fade: Duration) -> Vec<(String, Duration)> {
    buses.iter().map(|bus| (bus.to_string(), fade)).collect()
  }

  #[test]
  fn pausing_the_game_pauses_sfx_ambience_and_voice_but_not_ui_or_music() {
    let mut world = audio_world();
    let voices = play_on_each_bus(&mut world);
    audio_step(&mut world, FRAME);
    time(&mut world).game.pause();
    audio_step(&mut world, FRAME);
    let paused = ["sfx", "voice", "ambience"];
    assert_eq!(
      bus_entries(&log(&world), AudioLogKind::Pause),
      with_fade(&paused, MIN_FADE),
      "{:#?}",
      log(&world)
    );
    for (bus, voice) in &voices {
      let expected = if paused.contains(bus) {
        PlaybackState::Paused
      } else {
        PlaybackState::Playing
      };
      assert_eq!(playback(&world, voice), Some(expected), "{bus}");
      // The game's pause holds it; the voice itself was not paused
      assert!(voice.is_playing(), "{bus}: {voice:?}");
    }
    // Paused frames leave it so
    for _ in 0..5 {
      audio_step(&mut world, FRAME);
    }
    assert_eq!(bus_entries(&log(&world), AudioLogKind::Pause).len(), 3);
    time(&mut world).game.resume();
    audio_step(&mut world, FRAME);
    assert_eq!(
      bus_entries(&log(&world), AudioLogKind::Resume),
      with_fade(&paused, MIN_FADE)
    );
    for (bus, voice) in &voices {
      assert_eq!(
        playback(&world, voice),
        Some(PlaybackState::Playing),
        "{bus}"
      );
    }
  }

  #[test]
  fn resuming_continues_from_the_same_position() {
    let mut world = audio_world();
    let clip = clip(&mut world, 2.0);
    let sound = looping(&mut world, &clip, "sfx");
    let voice = AudioView::new(&mut world).play(&sound);
    for _ in 0..10 {
      audio_step(&mut world, FRAME);
    }
    time(&mut world).game.pause();
    audio_step(&mut world, FRAME);
    let at_pause = position(&world, &voice);
    assert!(at_pause > 0.15, "it has played: {at_pause}");
    // Half a second paused: it does not move
    for _ in 0..30 {
      audio_step(&mut world, FRAME);
    }
    let held = position(&world, &voice);
    assert!((held - at_pause).abs() <= CHUNK, "{at_pause} then {held}");
    time(&mut world).game.resume();
    // The resume reaches kira, and a chunk renders
    audio_step(&mut world, Duration::ZERO);
    audio_step(&mut world, Duration::from_secs_f64(CHUNK));
    let resumed = position(&world, &voice);
    assert!(
      (resumed - at_pause).abs() <= CHUNK,
      "{at_pause} then {resumed}"
    );
    for _ in 0..3 {
      audio_step(&mut world, FRAME);
    }
    let later = position(&world, &voice);
    assert!(
      later > resumed + 0.03,
      "it plays on: {resumed} then {later}"
    );
  }

  #[test]
  fn half_time_scale_halves_following_buses_rates() {
    let mut world = audio_world();
    let clip = clip(&mut world, 2.0);
    let octave_up = PlayOptions {
      pitch: 12.0,
      ..Default::default()
    };
    let ui_sound = looping(&mut world, &clip, "ui");
    let (sfx, ui) = {
      let mut audio = AudioView::new(&mut world);
      (audio.play_with(&clip, octave_up), audio.play(&ui_sound))
    };
    audio_step(&mut world, FRAME);
    assert!(
      close(&rates(&world, &sfx), &[2.0]),
      "{:?}",
      rates(&world, &sfx)
    );
    time(&mut world).game.set_speed(0.5);
    audio_step(&mut world, FRAME);
    assert!(
      close(&rates(&world, &sfx), &[2.0, 1.0]),
      "{:?}",
      rates(&world, &sfx)
    );
    assert!(
      close(&rates(&world, &ui), &[1.0]),
      "{:?}",
      rates(&world, &ui)
    );
    // The voice's own pitch still counts, the time scale multiplying it: an
    // octave down from the octave up it was played at
    sfx.set_pitch(-12.0);
    audio_step(&mut world, FRAME);
    assert!(
      close(&rates(&world, &sfx), &[2.0, 1.0, 0.5]),
      "{:?}",
      rates(&world, &sfx)
    );
    // A voice that starts under the scale starts at the scaled rate
    let late = AudioView::new(&mut world).play_with(&clip, octave_up);
    assert!(
      close(&rates(&world, &late), &[1.0]),
      "{:?}",
      rates(&world, &late)
    );
    time(&mut world).game.set_speed(1.0);
    audio_step(&mut world, FRAME);
    assert!(
      close(&rates(&world, &sfx), &[2.0, 1.0, 0.5, 1.0]),
      "{:?}",
      rates(&world, &sfx)
    );
    assert!(
      close(&rates(&world, &late), &[1.0, 2.0]),
      "{:?}",
      rates(&world, &late)
    );
    assert!(
      close(&rates(&world, &ui), &[1.0]),
      "{:?}",
      rates(&world, &ui)
    );
  }

  /// Hitstop is a visual freeze: the hit's sound plays through it, and no
  /// bus holds or slows
  #[test]
  fn hitstop_leaves_audio_playing() {
    let mut world = audio_world();
    let voices = play_on_each_bus(&mut world);
    let hit_clip = clip(&mut world, 1.0);
    audio_step(&mut world, FRAME);
    time(&mut world).game.hitstop(0.1);
    let hit = AudioView::new(&mut world).play(&hit_clip);
    audio_step(&mut world, FRAME);
    assert!(world.resource::<Time>().game.is_hitstop());
    let log = log(&world);
    assert_eq!(bus_entries(&log, AudioLogKind::Pause), [], "{log:#?}");
    assert!(log.iter().all(|entry| entry.kind != AudioLogKind::Rate));
    for (bus, voice) in &voices {
      assert_eq!(
        playback(&world, voice),
        Some(PlaybackState::Playing),
        "{bus}"
      );
    }
    assert_eq!(playback(&world, &hit), Some(PlaybackState::Playing));
    assert!(position(&world, &hit) > 0.01, "the hit sounds at once");
  }

  /// A time scale at or below 0.01 stands time still: the buses that
  /// follow it hold, and no rate kira cannot play is sent
  #[test]
  fn a_time_scale_at_or_below_0_01_holds_following_buses() {
    let mut world = audio_world();
    let voices = play_on_each_bus(&mut world);
    audio_step(&mut world, FRAME);
    time(&mut world).game.set_speed(0.01);
    audio_step(&mut world, FRAME);
    let following = ["sfx", "ambience"];
    assert_eq!(
      bus_entries(&log(&world), AudioLogKind::Pause),
      with_fade(&following, MIN_FADE),
      "voice does not follow time scale, and the game is not paused"
    );
    for (bus, voice) in &voices {
      let expected = if following.contains(bus) {
        PlaybackState::Paused
      } else {
        PlaybackState::Playing
      };
      assert_eq!(playback(&world, voice), Some(expected), "{bus}");
    }
    assert!(
      log(&world)
        .iter()
        .all(|entry| entry.kind != AudioLogKind::Rate)
    );
    time(&mut world).game.set_speed(0.02);
    audio_step(&mut world, FRAME);
    assert_eq!(
      bus_entries(&log(&world), AudioLogKind::Resume),
      with_fade(&following, MIN_FADE)
    );
    let sfx = &voices[1].1;
    assert_eq!(playback(&world, sfx), Some(PlaybackState::Playing));
    assert!(
      close(&rates(&world, sfx), &[1.0, 0.02]),
      "{:?}",
      rates(&world, sfx)
    );
  }

  /// The game clock runs up to 100 times as fast; audio follows it up to
  /// 8 times
  #[test]
  fn the_time_scale_audio_follows_is_capped_at_8x() {
    let mut world = audio_world();
    let clip = clip(&mut world, 2.0);
    let octave_up = PlayOptions {
      pitch: 12.0,
      ..Default::default()
    };
    let sfx = AudioView::new(&mut world).play_with(&clip, octave_up);
    time(&mut world).game.set_speed(20.0);
    audio_step(&mut world, FRAME);
    time(&mut world).game.set_speed(4.0);
    audio_step(&mut world, FRAME);
    assert!(
      close(&rates(&world, &sfx), &[2.0, 16.0, 8.0]),
      "{:?}",
      rates(&world, &sfx)
    );
  }

  /// A voice that starts held keeps its fade-in: played as the game
  /// resumes (its bus is still held until the frame's audio runs) or
  /// while the game is paused, it fades in over its own fade-in when its
  /// bus lets it go, not over 10 ms. Its start, silent and paused, logs no
  /// fade; its first resume logs the fade-in
  #[test]
  fn a_voice_that_starts_held_keeps_its_fade_in() {
    let mut world = audio_world();
    let clip = clip(&mut world, 2.0);
    let fade_in = Duration::from_millis(500);
    let bed = world.resource_mut::<Assets<Sound>>().add(Sound {
      clips: vec![SoundClip::new(clip)],
      looping: true,
      fade_in,
      ..Default::default()
    });
    time(&mut world).game.pause();
    audio_step(&mut world, FRAME);
    let waiting = AudioView::new(&mut world).play(&bed);
    audio_step(&mut world, FRAME);
    time(&mut world).game.resume();
    let late = AudioView::new(&mut world).play(&bed);
    for _ in 0..3 {
      audio_step(&mut world, FRAME);
    }
    for voice in [&waiting, &late] {
      // Still fading in after 50 ms
      assert_eq!(playback(&world, voice), Some(PlaybackState::Resuming));
      let entries: Vec<(AudioLogKind, Duration)> = log(&world)
        .iter()
        .filter(|entry| entry.voice == voice.id())
        .map(|entry| (entry.kind, entry.fade))
        .collect();
      assert_eq!(
        entries,
        [
          (AudioLogKind::Start, Duration::ZERO),
          (AudioLogKind::Resume, fade_in)
        ]
      );
    }
    for _ in 0..30 {
      audio_step(&mut world, FRAME);
    }
    for voice in [&waiting, &late] {
      assert_eq!(playback(&world, voice), Some(PlaybackState::Playing));
    }
  }

  /// A voice's own resume in the frame its bus starts holding does not
  /// play it through the hold
  #[test]
  fn a_voice_resumed_as_its_bus_starts_holding_stays_held() {
    let mut world = audio_world();
    let clip = clip(&mut world, 2.0);
    let voice = AudioView::new(&mut world).play(&clip);
    voice.pause();
    audio_step(&mut world, FRAME);
    voice.resume();
    time(&mut world).game.pause();
    for _ in 0..3 {
      audio_step(&mut world, FRAME);
    }
    assert!(voice.is_playing(), "{voice:?}");
    assert_eq!(playback(&world, &voice), Some(PlaybackState::Paused));
    time(&mut world).game.resume();
    audio_step(&mut world, FRAME);
    assert_eq!(playback(&world, &voice), Some(PlaybackState::Playing));
  }

  /// A voice played while its bus is held starts held, unheard, and plays
  /// from its start when the game resumes; one stopped while held is over
  /// by then, never heard again
  #[test]
  fn a_voice_played_or_stopped_while_the_game_is_paused_waits_for_it() {
    let mut world = audio_world();
    let clip = clip(&mut world, 2.0);
    let stopped = AudioView::new(&mut world).play(&clip);
    audio_step(&mut world, FRAME);
    time(&mut world).game.pause();
    audio_step(&mut world, FRAME);
    let waiting = AudioView::new(&mut world).play(&clip);
    stopped.stop(Duration::from_millis(30));
    for _ in 0..4 {
      audio_step(&mut world, FRAME);
    }
    assert_eq!(playback(&world, &waiting), Some(PlaybackState::Paused));
    assert!(position(&world, &waiting) < 1e-3, "it has not played");
    assert!(stopped.is_stopped(), "its fade ran, unheard: {stopped:?}");
    time(&mut world).game.resume();
    audio_step(&mut world, FRAME);
    assert_eq!(playback(&world, &waiting), Some(PlaybackState::Playing));
    assert!(position(&world, &waiting) > 0.0);
    assert_eq!(AudioView::new(&mut world).voices_on("sfx"), 1);
  }

  /// The game's pause and a voice's own pause are separate: a voice the
  /// game paused, and the game then paused too, stays paused when the game
  /// resumes; one resumed while the game is paused waits for the game
  #[test]
  fn a_voice_s_own_pause_outlasts_the_game_s() {
    let mut world = audio_world();
    let (one, other) = (clip(&mut world, 2.0), clip(&mut world, 2.0));
    let (own, resumed) = {
      let mut audio = AudioView::new(&mut world);
      (audio.play(&one), audio.play(&other))
    };
    own.pause();
    audio_step(&mut world, FRAME);
    time(&mut world).game.pause();
    audio_step(&mut world, FRAME);
    resumed.pause();
    audio_step(&mut world, FRAME);
    resumed.resume();
    audio_step(&mut world, FRAME);
    assert!(resumed.is_playing(), "{resumed:?}");
    assert_eq!(playback(&world, &resumed), Some(PlaybackState::Paused));
    time(&mut world).game.resume();
    audio_step(&mut world, FRAME);
    assert!(!own.is_playing() && !own.is_stopped(), "{own:?}");
    assert_eq!(playback(&world, &own), Some(PlaybackState::Paused));
    assert_eq!(playback(&world, &resumed), Some(PlaybackState::Playing));
  }

  /// Each bus pauses by its own setting: a game's bus that does not pause
  /// under one that does plays on
  #[test]
  fn a_bus_that_does_not_pause_under_one_that_does_plays_on() {
    let mut world = audio_world();
    let mut menu = BusDef::new("sfx/menu");
    menu.pauses_with_game = Some(false);
    menu.follows_time_scale = Some(false);
    AudioView::new(&mut world).set_mixer(Mixer {
      buses: vec![menu],
      ..Default::default()
    });
    audio_step(&mut world, FRAME);
    let clip = clip(&mut world, 2.0);
    let (menu_sound, steps_sound) = (
      looping(&mut world, &clip, "sfx/menu"),
      looping(&mut world, &clip, "sfx"),
    );
    let (menu, steps) = {
      let mut audio = AudioView::new(&mut world);
      (audio.play(&menu_sound), audio.play(&steps_sound))
    };
    audio_step(&mut world, FRAME);
    time(&mut world).game.pause();
    audio_step(&mut world, FRAME);
    assert_eq!(playback(&world, &menu), Some(PlaybackState::Playing));
    assert_eq!(playback(&world, &steps), Some(PlaybackState::Paused));
    let before = position(&world, &menu);
    audio_step(&mut world, FRAME);
    assert!(position(&world, &menu) > before, "it plays on");
  }

  /// A voice its bus holds is not heard, so it ducks nothing: music comes
  /// back up under the pause, and dips again after it
  #[test]
  fn a_held_voice_ducks_nothing() {
    let mut world = audio_world();
    AudioView::new(&mut world).set_mixer(Mixer {
      ducks: vec![DuckDef {
        trigger: "voice".into(),
        target: "music".into(),
        by_db: -9.0,
        attack: 0.05,
        release: 0.05,
      }],
      ..Default::default()
    });
    audio_step(&mut world, FRAME);
    let clip = clip(&mut world, 2.0);
    let line = looping(&mut world, &clip, "voice");
    AudioView::new(&mut world).play(&line);
    let run = |world: &mut World, frames: usize| {
      for _ in 0..frames {
        audio_step(world, FRAME);
      }
    };
    let duck = |world: &World| {
      let buses = world.resource::<AudioEngine>().buses();
      buses
        .iter()
        .find(|bus| bus.name == "music")
        .map_or(f32::NAN, |bus| bus.duck_db)
    };
    run(&mut world, 6);
    assert_eq!(duck(&world), -9.0);
    time(&mut world).game.pause();
    run(&mut world, 6);
    assert_eq!(duck(&world), 0.0, "the line is held");
    time(&mut world).game.resume();
    run(&mut world, 6);
    assert_eq!(duck(&world), -9.0);
  }

  #[test]
  fn focus_loss_keeps_by_default_and_mutes_when_the_mixer_says() {
    let mut world = audio_world();
    audio_step(&mut world, FRAME);
    let master = |world: &World| {
      let buses = world.resource::<AudioEngine>().buses();
      buses.first().map(|bus| bus.gain_db).unwrap_or(f32::NAN)
    };
    let master_gains = |world: &World| -> Vec<(f32, Duration)> {
      log(world)
        .iter()
        .filter(|entry| entry.kind == AudioLogKind::Gain && entry.voice == 0)
        .filter(|entry| entry.bus == "master")
        .map(|entry| (entry.gain_db, entry.fade))
        .collect()
    };
    let set_focused = |world: &mut World, focused: bool| {
      world.resource_mut::<AudioEngine>().set_focused(focused);
      audio_step(world, FRAME);
    };
    // master at the player's setting
    let open = master(&world);
    // Keep: nothing changes
    set_focused(&mut world, false);
    assert_eq!(master(&world), open);
    assert_eq!(master_gains(&world), []);
    set_focused(&mut world, true);
    // Mute
    AudioView::new(&mut world).set_mixer(Mixer {
      on_focus_loss: FocusLoss::Mute,
      ..Default::default()
    });
    audio_step(&mut world, FRAME);
    set_focused(&mut world, false);
    assert_eq!(master(&world), -60.0);
    assert_eq!(master_gains(&world), [(-60.0, MIN_FADE)]);
    set_focused(&mut world, true);
    assert_eq!(master(&world), open);
    assert_eq!(master_gains(&world), [(-60.0, MIN_FADE), (open, MIN_FADE)]);
    // Duck, from a mixer that lands while the window is unfocused
    set_focused(&mut world, false);
    AudioView::new(&mut world).set_mixer(Mixer {
      on_focus_loss: FocusLoss::Duck(-12.0),
      ..Default::default()
    });
    audio_step(&mut world, FRAME);
    assert!((master(&world) - (open - 12.0)).abs() < 1e-4);
    set_focused(&mut world, true);
    assert_eq!(master(&world), open);
  }

  // Hot reload

  /// A mono WAV file at 48 kHz, `seconds` long, every sample `value`
  fn wav_of(seconds: f64, value: i16) -> Vec<u8> {
    wav(48_000, (48_000.0 * seconds) as usize, value)
  }

  /// An audio world whose files are `files`, kept to save over
  fn world_over(files: &[(&str, &[u8])]) -> (World, MemorySource) {
    let source = MemorySource::new();
    for (path, bytes) in files {
      source.insert(path, *bytes);
    }
    (audio_world_over(source.clone()), source)
  }

  fn load<T: Asset>(world: &mut World, path: &str) -> Handle<T> {
    match AssetsView::new(world).load_now::<T>(path) {
      Ok(handle) => handle,
      Err(error) => panic!("{path}: {}", error.report()),
    }
  }

  /// Save `bytes` over `path` and reload it, stepping with no time passing
  /// (so nothing plays on) until the registry has applied the reload to a
  /// `T`: `Modified`, or `ReloadFailed`. Gives the `T` events sent
  fn save<T: Asset>(
    world: &mut World,
    files: &MemorySource,
    path: &str,
    bytes: impl Into<Vec<u8>>,
  ) -> Vec<AssetEvent<T>> {
    let mut cursor = world
      .resource::<Messages<AssetEvent<T>>>()
      .get_cursor_current();
    files.insert(path, bytes);
    world.resource::<AssetServer>().reload(path);
    let start = Instant::now();
    let mut sent = Vec::new();
    loop {
      audio_step(world, Duration::ZERO);
      sent.extend(
        cursor
          .read(world.resource::<Messages<AssetEvent<T>>>())
          .cloned(),
      );
      let applied = sent.iter().any(|event| {
        matches!(
          event,
          AssetEvent::Modified { .. } | AssetEvent::ReloadFailed { .. }
        )
      });
      if applied {
        return sent;
      }
      assert!(start.elapsed() < Duration::from_secs(5), "{path}: {sent:?}");
      std::thread::sleep(Duration::from_millis(1));
    }
  }

  /// What the log says happened to voice `id`: each entry's kind and fade
  fn entries(world: &World, id: u32) -> Vec<(AudioLogKind, Duration)> {
    log(world)
      .iter()
      .filter(|entry| entry.voice == id)
      .map(|entry| (entry.kind, entry.fade))
      .collect()
  }

  fn steps(world: &mut World, count: usize) {
    for _ in 0..count {
      audio_step(world, FRAME);
    }
  }

  /// The voice playing on `bus`, by the log's latest start there
  fn started_on(world: &World, bus: &str) -> u32 {
    log(world)
      .iter()
      .rev()
      .find(|entry| entry.kind == AudioLogKind::Start && entry.bus == bus)
      .map_or(0, |entry| entry.voice)
  }

  const HUM: &str = r#"(clips: [(path: "sfx/hum.wav")], looping: true)"#;

  #[test]
  fn a_reloaded_clip_switches_a_playing_loop_at_its_position() {
    let hum = wav_of(2.0, 3_000);
    let (mut world, files) =
      world_over(&[("sfx/hum.wav", &hum), ("sfx/hum.sound.ron", HUM.as_bytes())]);
    let sound = load::<Sound>(&mut world, "sfx/hum.sound.ron");
    let voice = AudioView::new(&mut world).play(&sound);
    steps(&mut world, 18);
    let before = position(&world, &voice);
    assert!(before > 0.25, "it has played: {before}");
    save::<AudioClip>(&mut world, &files, "sfx/hum.wav", wav_of(0.5, 6_000));
    assert_eq!(
      entries(&world, voice.id()),
      [
        (AudioLogKind::Start, Duration::ZERO),
        (AudioLogKind::Stop, MIN_FADE),
        (AudioLogKind::Start, MIN_FADE)
      ],
      "{:#?}",
      log(&world)
    );
    let after = position(&world, &voice);
    assert!((after - before).abs() <= CHUNK, "{before} then {after}");
    // The new clip is half a second: the loop wraps there, not at 2 s
    steps(&mut world, 18);
    let later = position(&world, &voice);
    assert!(later < 0.5, "it loops the new clip: {later}");
    assert!(voice.is_playing(), "{voice:?}");
    assert_eq!(AudioView::new(&mut world).voices_on("sfx"), 1);
  }

  #[test]
  fn a_reloaded_clip_lets_one_shots_finish() {
    let beep = wav_of(0.5, 3_000);
    let (mut world, files) = world_over(&[("sfx/beep.wav", &beep)]);
    let clip = load::<AudioClip>(&mut world, "sfx/beep.wav");
    let voice = AudioView::new(&mut world).play(&clip);
    steps(&mut world, 6);
    let before = position(&world, &voice);
    save::<AudioClip>(&mut world, &files, "sfx/beep.wav", wav_of(2.0, 6_000));
    assert_eq!(
      entries(&world, voice.id()),
      [(AudioLogKind::Start, Duration::ZERO)]
    );
    steps(&mut world, 6);
    assert!(position(&world, &voice) > before + 0.05, "it plays on");
    // The old clip's half second is over well before the new one's 2 s
    steps(&mut world, 30);
    assert!(voice.is_stopped(), "{voice:?}");
    assert_eq!(
      entries(&world, voice.id()),
      [(AudioLogKind::Start, Duration::ZERO)],
      "it ended, not stopped"
    );
    // A new play is the new clip
    let fresh = AudioView::new(&mut world).play(&clip);
    steps(&mut world, 60);
    assert!(fresh.is_playing(), "{fresh:?}");
  }

  #[test]
  fn a_reloaded_sound_retunes_its_playing_loops() {
    let hum = wav_of(2.0, 3_000);
    let (mut world, files) = world_over(&[
      ("sfx/hum.wav", &hum),
      (
        "sfx/hum.sound.ron",
        br#"(clips: [(path: "sfx/hum.wav")], looping: true, volume: -6.0)"#,
      ),
    ]);
    let sound = load::<Sound>(&mut world, "sfx/hum.sound.ron");
    let voice = AudioView::new(&mut world).play(&sound);
    steps(&mut world, 3);
    let retuned = r#"(clips: [(path: "sfx/hum.wav")], looping: true, volume: -12.0, pitch: 2.0, bus: "ambience")"#;
    save::<Sound>(&mut world, &files, "sfx/hum.sound.ron", retuned);
    assert_eq!(
      entries(&world, voice.id()),
      [
        (AudioLogKind::Start, Duration::ZERO),
        (AudioLogKind::Gain, MIN_FADE),
        (AudioLogKind::Rate, MIN_FADE)
      ],
      "{:#?}",
      log(&world)
    );
    let octave_sixth = 2f64.powf(2.0 / 12.0);
    let last = log(&world);
    let (gain, rate) = (&last[last.len() - 2], &last[last.len() - 1]);
    assert_eq!(gain.gain_db, -12.0);
    assert!((rate.rate - octave_sixth).abs() < 1e-9, "{}", rate.rate);
    // It stays on its bus; a new play takes everything new
    assert_eq!(rate.bus, "sfx");
    let fresh = AudioView::new(&mut world).play(&sound);
    let start = log(&world)
      .into_iter()
      .find(|entry| entry.voice == fresh.id())
      .map(|entry| (entry.gain_db, entry.rate, entry.bus));
    assert_eq!(start, Some((-12.0, octave_sixth, "ambience".to_string())));
  }

  #[test]
  fn a_broken_save_keeps_everything_playing() {
    let (hum, theme) = (wav_of(2.0, 3_000), wav_of(2.0, 2_000));
    let (mut world, files) = world_over(&[
      ("sfx/hum.wav", &hum),
      ("sfx/hum.sound.ron", HUM.as_bytes()),
      ("music/theme.wav", &theme),
      ("music/theme.music.ron", br#"(clip: "music/theme.wav")"#),
    ]);
    let sound = load::<Sound>(&mut world, "sfx/hum.sound.ron");
    let track = load::<Music>(&mut world, "music/theme.music.ron");
    let hum = AudioView::new(&mut world).play(&sound);
    AudioView::new(&mut world).play_music(&track);
    steps(&mut world, 6);
    let music = started_on(&world, "music");
    let before = (position(&world, &hum), music_position(&world, music));
    let failed = |sent: &[AssetEvent<Sound>]| {
      sent
        .iter()
        .any(|event| matches!(event, AssetEvent::ReloadFailed { .. }))
    };
    let sent = save::<Sound>(&mut world, &files, "sfx/hum.sound.ron", "(clips: [");
    assert!(failed(&sent), "{sent:?}");
    let sent = save::<AudioClip>(&mut world, &files, "sfx/hum.wav", "not audio");
    assert!(matches!(sent.as_slice(), [AssetEvent::ReloadFailed { .. }]));
    let sent = save::<Music>(&mut world, &files, "music/theme.music.ron", "(clip: ");
    assert!(matches!(sent.as_slice(), [AssetEvent::ReloadFailed { .. }]));
    let sent = save::<AudioClip>(&mut world, &files, "music/theme.wav", "RIFF no");
    assert!(matches!(sent.as_slice(), [AssetEvent::ReloadFailed { .. }]));
    for id in [hum.id(), music] {
      assert_eq!(
        entries(&world, id),
        [(AudioLogKind::Start, Duration::ZERO)],
        "{:#?}",
        log(&world)
      );
    }
    steps(&mut world, 6);
    let after = (position(&world, &hum), music_position(&world, music));
    assert!(
      after.0 > before.0 + 0.05 && after.1 > before.1 + 0.05,
      "{before:?} then {after:?}"
    );
    assert!(hum.is_playing());
    assert_eq!(AudioView::new(&mut world).current_music(), Some(track.id()));
  }

  fn music_position(world: &World, id: u32) -> f64 {
    world
      .resource::<AudioEngine>()
      .voice_position(id)
      .unwrap_or_default()
  }

  /// A saved intro or loop file (read, not loaded, by the music) reloads
  /// the music, and the track switches to it where it was
  #[test]
  fn a_saved_intro_or_loop_switches_the_playing_music() {
    let (intro, looped) = (wav_of(0.1, 1_000), wav_of(0.2, 2_000));
    let (mut world, files) = world_over(&[
      ("music/intro.wav", &intro),
      ("music/loop.wav", &looped),
      (
        "music/theme.music.ron",
        br#"(intro: "music/intro.wav", loop: "music/loop.wav")"#,
      ),
    ]);
    let track = load::<Music>(&mut world, "music/theme.music.ron");
    AudioView::new(&mut world).play_music(&track);
    steps(&mut world, 9);
    let voice = started_on(&world, "music");
    let before = music_position(&world, voice);
    save::<Music>(&mut world, &files, "music/loop.wav", wav_of(0.4, 4_000));
    let switch = [
      (AudioLogKind::Start, Duration::ZERO),
      (AudioLogKind::Stop, MIN_FADE),
      (AudioLogKind::Start, MIN_FADE),
    ];
    assert_eq!(entries(&world, voice), switch, "{:#?}", log(&world));
    let after = music_position(&world, voice);
    assert!((after - before).abs() <= CHUNK, "{before} then {after}");
    assert_eq!(AudioView::new(&mut world).current_music(), Some(track.id()));
    // It loops over the new loop: on past the old track's end at 0.3 s
    steps(&mut world, 12);
    let later = music_position(&world, voice);
    assert!(later > 0.3 && later < 0.5, "{later}");
    save::<Music>(&mut world, &files, "music/intro.wav", wav_of(0.2, 1_000));
    assert_eq!(entries(&world, voice).len(), 5, "{:#?}", log(&world));
  }

  /// A saved music definition switches the track where it is to what it
  /// now says; its clip saved does too, once
  #[test]
  fn a_saved_music_definition_switches_the_playing_music() {
    let theme = wav_of(2.0, 2_000);
    let (mut world, files) = world_over(&[
      ("music/theme.wav", &theme),
      ("music/theme.music.ron", br#"(clip: "music/theme.wav")"#),
    ]);
    let track = load::<Music>(&mut world, "music/theme.music.ron");
    AudioView::new(&mut world).play_music(&track);
    steps(&mut world, 6);
    let voice = started_on(&world, "music");
    let before = music_position(&world, voice);
    let quieter = r#"(clip: "music/theme.wav", volume: -6.0, crossfade: 0.5)"#;
    save::<Music>(&mut world, &files, "music/theme.music.ron", quieter);
    assert_eq!(
      entries(&world, voice),
      [
        (AudioLogKind::Start, Duration::ZERO),
        (AudioLogKind::Stop, MIN_FADE),
        (AudioLogKind::Start, MIN_FADE)
      ],
      "{:#?}",
      log(&world)
    );
    let restarted = log(&world).pop().map(|entry| entry.gain_db);
    assert_eq!(restarted, Some(-6.0));
    let after = music_position(&world, voice);
    assert!((after - before).abs() <= CHUNK, "{before} then {after}");
    // Its clip saved switches it once, though the music reloads with it
    save::<AudioClip>(&mut world, &files, "music/theme.wav", wav_of(1.0, 3_000));
    assert_eq!(entries(&world, voice).len(), 5, "{:#?}", log(&world));
    // Its new crossfade is what a scene's cleanup fades it out over
    crate::core::audio::stop_scene_music(&mut world);
    audio_step(&mut world, Duration::ZERO);
    assert_eq!(
      entries(&world, voice).last(),
      Some(&(AudioLogKind::Stop, Duration::from_millis(500)))
    );
  }

  /// A streamed loop switches to its reloaded clip as a decoded one does
  #[test]
  fn a_reloaded_clip_switches_a_streamed_loop() {
    let drone = wav_of(1.0, 3_000);
    let (mut world, files) = world_over(&[
      ("sfx/drone.wav", &drone),
      (
        "sfx/drone.sound.ron",
        br#"(clips: [(path: "sfx/drone.wav")], looping: true, mode: Stream)"#,
      ),
    ]);
    let sound = load::<Sound>(&mut world, "sfx/drone.sound.ron");
    let voice = AudioView::new(&mut world).play(&sound);
    steps(&mut world, 3);
    save::<AudioClip>(&mut world, &files, "sfx/drone.wav", wav_of(0.5, 6_000));
    assert_eq!(
      entries(&world, voice.id()),
      [
        (AudioLogKind::Start, Duration::ZERO),
        (AudioLogKind::Stop, MIN_FADE),
        (AudioLogKind::Start, MIN_FADE)
      ],
      "{:#?}",
      log(&world)
    );
    steps(&mut world, 3);
    assert!(voice.is_playing(), "{voice:?}");
    let about_it = dev_log::get_entries()
      .iter()
      .filter(|entry| entry.level == LogLevel::Warn && entry.message.starts_with("sfx/drone"))
      .count();
    assert_eq!(about_it, 0, "it opened and switched");
  }

  /// What the engine keeps about an asset (a definition's picks and
  /// cooldown, a clip's failed decode, a warning given once) goes when the
  /// asset is freed
  #[test]
  fn a_freed_asset_leaves_nothing_remembered() {
    let mut world = audio_world();
    let clip = clip(&mut world, 0.1);
    // Played at a place with no spatial settings: warned about once
    let sound = world.resource_mut::<Assets<Sound>>().add(Sound {
      clips: vec![SoundClip::new(clip.clone())],
      cooldown: Duration::from_millis(50),
      ..Default::default()
    });
    AudioView::new(&mut world).play_at(&sound, crate::core::ecs::Vec2::new(10.0, 0.0));
    // A track made for a bus outside music: warned about once
    let mut track = Music::looping(clip.clone(), 0.0, None);
    track.bus = "sfx".into();
    let track = world.resource_mut::<Assets<Music>>().add(track);
    AudioView::new(&mut world).play_music(&track);
    AudioView::new(&mut world).stop_music(Duration::ZERO);
    // A clip whose stream will not open: marked
    let garbage: std::sync::Arc<[u8]> = std::sync::Arc::from(&b"not audio at all"[..]);
    let broken = world
      .resource_mut::<Assets<AudioClip>>()
      .add(AudioClip::new(
        ClipData::Encoded(EncodedClip {
          bytes: garbage,
          frames: 22_050,
        }),
        Duration::from_secs(1),
        22_050,
      ));
    // It fails on its decoder thread, and its voice ends
    let failing = AudioView::new(&mut world).play(&broken);
    for _ in 0..500 {
      if failing.is_stopped() {
        break;
      }
      audio_step(&mut world, FRAME);
      std::thread::sleep(Duration::from_millis(1));
    }
    assert!(failing.is_stopped(), "{failing:?}");
    assert_eq!(world.resource::<AudioEngine>().remembered(), 4);
    drop((sound, track, broken));
    audio_step(&mut world, FRAME);
    assert_eq!(world.resource::<AudioEngine>().remembered(), 0);
  }

  /// The Stop and Start of a switch to new data where it is
  const SWITCH: [(AudioLogKind, Duration); 2] = [
    (AudioLogKind::Stop, MIN_FADE),
    (AudioLogKind::Start, MIN_FADE),
  ];

  /// Replace `clip`'s data in code: the same `Modified` a save sends, at the
  /// next pump
  fn replace_clip(world: &mut World, clip: &Handle<AudioClip>, seconds: f32) {
    let frames = vec![[0.2, 0.2]; (48_000.0 * seconds) as usize];
    if let Some(clip) = world.resource_mut::<Assets<AudioClip>>().get_mut(clip) {
      *clip = AudioClip::from_frames(48_000, &frames);
    }
  }

  /// A loop moved to rebuilt buses in the frame its clip changes switches
  /// once, onto the new clip
  #[test]
  fn a_mixer_rebuild_and_a_clip_reload_in_one_frame_switch_a_loop_once() {
    let mut world = audio_world();
    let clip = clip(&mut world, 2.0);
    let sound = looping(&mut world, &clip, "sfx");
    let voice = AudioView::new(&mut world).play(&sound);
    steps(&mut world, 6);
    AudioView::new(&mut world).set_mixer(Mixer::default());
    replace_clip(&mut world, &clip, 0.5);
    audio_step(&mut world, Duration::ZERO);
    let mut once = vec![(AudioLogKind::Start, Duration::ZERO)];
    once.extend(SWITCH);
    assert_eq!(entries(&world, voice.id()), once, "{:#?}", log(&world));
    // On the new half-second clip: it wraps there
    steps(&mut world, 30);
    assert!(
      position(&world, &voice) < 0.5,
      "{}",
      position(&world, &voice)
    );
  }

  /// A track moved to rebuilt buses in the frame its definition changes
  /// switches once, to what it now says
  #[test]
  fn a_mixer_rebuild_and_a_music_change_in_one_frame_switch_the_track_once() {
    let mut world = audio_world();
    let clip = clip(&mut world, 2.0);
    let track = world
      .resource_mut::<Assets<Music>>()
      .add(Music::looping(clip, 0.0, None));
    AudioView::new(&mut world).play_music(&track);
    steps(&mut world, 6);
    let voice = started_on(&world, "music");
    AudioView::new(&mut world).set_mixer(Mixer::default());
    if let Some(track) = world.resource_mut::<Assets<Music>>().get_mut(&track) {
      track.volume_db = -6.0;
    }
    audio_step(&mut world, Duration::ZERO);
    let mut once = vec![(AudioLogKind::Start, Duration::ZERO)];
    once.extend(SWITCH);
    assert_eq!(entries(&world, voice), once, "{:#?}", log(&world));
    assert_eq!(log(&world).pop().map(|entry| entry.gain_db), Some(-6.0));
  }

  /// A track played in the frame its change lands starts on it, and does
  /// not switch again
  #[test]
  fn a_track_played_as_its_change_lands_starts_once() {
    let mut world = audio_world();
    let clip = clip(&mut world, 2.0);
    let track = world
      .resource_mut::<Assets<Music>>()
      .add(Music::looping(clip, 0.0, None));
    audio_step(&mut world, FRAME);
    if let Some(track) = world.resource_mut::<Assets<Music>>().get_mut(&track) {
      track.volume_db = -6.0;
    }
    AudioView::new(&mut world).play_music(&track);
    audio_step(&mut world, Duration::ZERO);
    let voice = started_on(&world, "music");
    assert_eq!(
      entries(&world, voice),
      [(AudioLogKind::Start, Duration::ZERO)],
      "{:#?}",
      log(&world)
    );
    assert_eq!(log(&world).pop().map(|entry| entry.gain_db), Some(-6.0));
  }

  /// A volume or pitch the game sets on a voice is relative to its sound's
  /// level, as `PlayOptions` are: a definition at -12 dB set to -6 plays at
  /// -18, and a reload of the definition still moves the sound beneath the
  /// game's change
  #[test]
  fn a_voice_s_volume_and_pitch_are_relative_to_its_sound() {
    let hum = wav_of(2.0, 3_000);
    let (mut world, files) = world_over(&[
      ("sfx/hum.wav", &hum),
      (
        "sfx/hum.sound.ron",
        br#"(clips: [(path: "sfx/hum.wav")], looping: true, volume: -12.0)"#,
      ),
    ]);
    let sound = load::<Sound>(&mut world, "sfx/hum.sound.ron");
    let voice = AudioView::new(&mut world).play(&sound);
    steps(&mut world, 3);
    voice.set_volume(-6.0);
    voice.set_pitch(2.0);
    audio_step(&mut world, FRAME);
    let heard = |world: &World, kind: AudioLogKind| {
      log(world)
        .iter()
        .rev()
        .find(|entry| entry.voice == voice.id() && entry.kind == kind)
        .map(|entry| (entry.gain_db, entry.rate))
    };
    let semitones = |rate: f64| 12.0 * rate.log2();
    let Some((gain, _)) = heard(&world, AudioLogKind::Gain) else {
      panic!("no gain change: {:#?}", log(&world));
    };
    assert!((gain + 18.0).abs() < 1e-4, "{gain}");
    let Some((_, rate)) = heard(&world, AudioLogKind::Rate) else {
      panic!("no rate change: {:#?}", log(&world));
    };
    assert!((semitones(rate) - 2.0).abs() < 1e-6, "{rate}");
    let louder = r#"(clips: [(path: "sfx/hum.wav")], looping: true, volume: -10.0, pitch: 1.0)"#;
    save::<Sound>(&mut world, &files, "sfx/hum.sound.ron", louder);
    let Some((gain, _)) = heard(&world, AudioLogKind::Gain) else {
      panic!("no gain change: {:#?}", log(&world));
    };
    assert!(
      (gain + 16.0).abs() < 1e-4,
      "the reload moves it beneath: {gain}"
    );
    let Some((_, rate)) = heard(&world, AudioLogKind::Rate) else {
      panic!("no rate change: {:#?}", log(&world));
    };
    assert!((semitones(rate) - 3.0).abs() < 1e-6, "{rate}");
    // Back to 0, it plays at the sound's level
    voice.set_volume(0.0);
    audio_step(&mut world, FRAME);
    let Some((gain, _)) = heard(&world, AudioLogKind::Gain) else {
      panic!("no gain change: {:#?}", log(&world));
    };
    assert!((gain + 10.0).abs() < 1e-4, "{gain}");
  }

  /// A loop paused through its clip's switch stays paused where it was,
  /// and resumes from there on the new clip
  #[test]
  fn a_paused_loop_stays_paused_through_a_clip_switch() {
    let hum = wav_of(2.0, 3_000);
    let (mut world, files) =
      world_over(&[("sfx/hum.wav", &hum), ("sfx/hum.sound.ron", HUM.as_bytes())]);
    let sound = load::<Sound>(&mut world, "sfx/hum.sound.ron");
    let voice = AudioView::new(&mut world).play(&sound);
    steps(&mut world, 12);
    voice.pause();
    steps(&mut world, 3);
    let paused_at = position(&world, &voice);
    save::<AudioClip>(&mut world, &files, "sfx/hum.wav", wav_of(1.0, 6_000));
    assert_eq!(
      entries(&world, voice.id()),
      [
        (AudioLogKind::Start, Duration::ZERO),
        (AudioLogKind::Pause, MIN_FADE),
        (AudioLogKind::Stop, MIN_FADE),
        (AudioLogKind::Start, Duration::ZERO)
      ],
      "it starts again silent and paused: {:#?}",
      log(&world)
    );
    steps(&mut world, 6);
    assert_eq!(playback(&world, &voice), Some(PlaybackState::Paused));
    let held = position(&world, &voice);
    assert!((held - paused_at).abs() <= CHUNK, "{paused_at} then {held}");
    voice.resume();
    steps(&mut world, 6);
    assert_eq!(playback(&world, &voice), Some(PlaybackState::Playing));
    assert!(
      position(&world, &voice) > held + 0.05,
      "it plays on from there"
    );
  }

  /// A loop past the end of its clip saved shorter goes on from the same
  /// place in its loop
  #[test]
  fn a_loop_past_its_clip_s_new_end_wraps_into_it() {
    let hum = wav_of(2.0, 3_000);
    let (mut world, files) =
      world_over(&[("sfx/hum.wav", &hum), ("sfx/hum.sound.ron", HUM.as_bytes())]);
    let sound = load::<Sound>(&mut world, "sfx/hum.sound.ron");
    let voice = AudioView::new(&mut world).play(&sound);
    steps(&mut world, 18);
    let before = position(&world, &voice);
    assert!(before > 0.25 && before < 0.35, "{before}");
    save::<AudioClip>(&mut world, &files, "sfx/hum.wav", wav_of(0.2, 6_000));
    let after = position(&world, &voice);
    let wrapped = before - 0.2;
    assert!(
      (after - wrapped).abs() <= CHUNK,
      "{before} wraps to {after}, not {wrapped}"
    );
    steps(&mut world, 3);
    assert!(voice.is_playing(), "{voice:?}");
    assert!(
      position(&world, &voice) < 0.2,
      "{}",
      position(&world, &voice)
    );
  }
}
