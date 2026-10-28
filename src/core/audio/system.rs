//! `audio_system`: the frame's audio work

use super::AudioEngine;
use super::engine::GameTime;
use crate::core::ecs::World;
use crate::core::ecs::kernel::Mut;
use crate::core::time::Time;

/// `PostUpdate`, after the cameras: applies the voices' commands queued this
/// frame (the scene's update has run), builds the bus tree from the mixer
/// when it lands or changes, follows the game clock (its pause and time
/// scale; hitstop leaves audio alone), reports the device's stream errors,
/// advances the mock
/// backend by the frame's real time, and retires the voices that ended. A
/// world without an [`AudioEngine`] is left alone
#[doc(hidden)]
pub fn audio_system(world: &mut World) {
  let (real_delta, game) = world
    .get_resource::<Time>()
    .map(|time| (time.real.delta_duration(), GameTime::of(time)))
    .unwrap_or_default();
  // The engine is taken out of the world while it reads the stores
  world.try_resource_scope(|world: &mut World, mut engine: Mut<'_, AudioEngine>| {
    engine.update(world, real_delta, game);
  });
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use kira::sound::PlaybackState;

  use crate::core::asset::{Assets, Handle};
  use crate::core::audio::engine::MIN_FADE;
  use crate::core::audio::{
    AudioClip, AudioEngine, AudioLogEntry, AudioLogKind, AudioView, BusDef, DuckDef, FocusLoss,
    Mixer, PlayOptions, Sound, SoundClip, Voice,
  };
  use crate::core::ecs::World;
  use crate::core::testing::{audio_step, audio_world};
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
    // The voice's own pitch still counts, the time scale multiplying it
    sfx.set_pitch(0.0);
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
    let clip = clip(&mut world, 2.0);
    let (own, resumed) = {
      let mut audio = AudioView::new(&mut world);
      (audio.play(&clip), audio.play(&clip))
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
}
