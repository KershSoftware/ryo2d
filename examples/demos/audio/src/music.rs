//! The Music page: two tracks on the engine's one music player. The theme
//! is an intro and a loop in two files, joined so the loop follows the
//! intro with no gap; the night track is one clip whose first second plays
//! once before the rest loops. Asking for the other track crossfades to it
//! over its crossfade, both playing meanwhile

use demo_kit::prelude::*;

use crate::shared::{DIM, HOT, bar, bus_meters, load, steady_start};

/// Where the night track's loop starts, in seconds, as its file says
const NIGHT_LOOP_START: f32 = 1.0;

/// One of the two tracks, and what the page knows of its playing
struct Track {
  name: &'static str,
  file: &'static str,
  form: &'static str,
  music: Handle<Music>,
  /// Seconds played once before the loop
  intro: f32,
  /// Seconds the loop lasts
  looped: f32,
  /// Seconds it has played (it plays from the start each time it is asked
  /// for)
  played: f32,
  /// Seconds left of its fade-out, after the other took over or a stop
  fading: Option<f32>,
}

pub(crate) struct MusicPage {
  tracks: Vec<Track>,
  /// The track asked for last, if the music is not stopped
  current: Option<usize>,
  paused: bool,
  _mixer: Option<Handle<Mixer>>,
}

impl MusicPage {
  pub(crate) fn new() -> Self {
    Self {
      tracks: Vec::new(),
      current: None,
      paused: false,
      _mixer: None,
    }
  }

  /// Play track `index`, crossfading from the one playing
  fn play(&mut self, ctx: &mut SceneContext, index: usize) {
    let crossfade = ctx
      .assets()
      .get(&self.tracks[index].music)
      .map_or(1.0, |music| music.crossfade.as_secs_f32());
    if let Some(old) = self.current.filter(|&old| old != index) {
      self.tracks[old].fading = Some(crossfade);
    }
    let track = &mut self.tracks[index];
    track.played = 0.0;
    track.fading = None;
    ctx.audio().play_music(&track.music);
    self.current = Some(index);
    self.paused = false;
  }
}

/// A clip's length in seconds, 0 if it did not load
fn seconds(ctx: &mut SceneContext, path: &str) -> f32 {
  let clip = load::<AudioClip>(ctx, path);
  let assets = ctx.assets();
  clip
    .and_then(|clip| assets.get(&clip))
    .map_or(0.0, |clip| clip.duration().as_secs_f32())
}

impl DemoPage for MusicPage {
  fn name(&self) -> &str {
    "Music"
  }

  fn description(&self) -> &str {
    "An intro then a seamless loop, and a crossfade from one track to the other"
  }

  fn controls(&self) -> Vec<Control> {
    vec![
      Control::key(KeyCode::Space, "Crossfade to the other track"),
      Control::key(KeyCode::KeyP, "Pause or resume"),
      Control::key(KeyCode::KeyS, "Stop"),
    ]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    self._mixer = steady_start(ctx);
    let theme_intro = seconds(ctx, "music/theme-intro.wav");
    let theme_loop = seconds(ctx, "music/theme-loop.wav");
    let night = seconds(ctx, "music/night.wav");
    let specs = [
      (
        "theme",
        "music/theme.music.ron",
        "an intro and a loop, two files joined",
        theme_intro,
        theme_loop,
      ),
      (
        "night",
        "music/night.music.ron",
        "one clip, looping from 1 s to its end",
        NIGHT_LOOP_START,
        night - NIGHT_LOOP_START,
      ),
    ];
    for (name, file, form, intro, looped) in specs {
      if let Some(music) = load(ctx, file) {
        self.tracks.push(Track {
          name,
          file,
          form,
          music,
          intro,
          looped,
          played: 0.0,
          fading: None,
        });
      }
    }
    if !self.tracks.is_empty() {
      self.play(ctx, 0);
    }
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    if self.tracks.len() < 2 {
      return;
    }
    if ctx.input.key_pressed(KeyCode::Space) {
      let next = self
        .current
        .map_or(0, |current| (current + 1) % self.tracks.len());
      self.play(ctx, next);
    }
    if ctx.input.key_pressed(KeyCode::KeyP) && self.current.is_some() {
      self.paused = !self.paused;
      if self.paused {
        ctx.audio().pause_music();
      } else {
        ctx.audio().resume_music();
      }
    }
    if ctx.input.key_pressed(KeyCode::KeyS)
      && let Some(current) = self.current.take()
    {
      ctx.audio().stop_music(std::time::Duration::from_secs(1));
      self.tracks[current].fading = Some(1.0);
      self.paused = false;
    }
    for (index, track) in self.tracks.iter_mut().enumerate() {
      let playing = self.current == Some(index) && !self.paused;
      if playing || track.fading.is_some() {
        track.played += dt;
      }
      if let Some(left) = &mut track.fading {
        *left -= dt;
        if *left <= 0.0 {
          track.fading = None;
        }
      }
    }
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    let area = content_area(ui);
    let x = area.x + 16.0;
    let mut y = area.y + 10.0;
    let current = ctx.audio().current_music();
    let timeline_w = 420.0;

    for (index, track) in self.tracks.iter().enumerate() {
      let is_current = current == Some(track.music.id());
      let state = if is_current && self.paused {
        "paused"
      } else if is_current {
        "playing"
      } else if track.fading.is_some() {
        "fading out"
      } else {
        "stopped"
      };
      let color = if is_current { HOT } else { Color::WHITE };
      ui.draw_text(&format!("{}: {state}", track.name), x, y, color);
      ui.draw_text(track.file, x + 200.0, y, DIM);
      y += 22.0;
      let assets = ctx.assets();
      let crossfade = assets
        .get(&track.music)
        .map_or(0.0, |music| music.crossfade.as_secs_f32());
      ui.draw_text(
        &format!(
          "{}: {:.1} s once, then {:.1} s looped; crossfade {crossfade:.1} s",
          track.form, track.intro, track.looped
        ),
        x,
        y,
        Color::LIGHT_GRAY,
      );
      y += 26.0;

      // The intro, then the loop, with where the track is
      let total = (track.intro + track.looped).max(0.001);
      let intro_w = timeline_w * track.intro / total;
      let track_color = if index == 0 {
        Color::rgb(170, 130, 255)
      } else {
        Color::rgb(90, 170, 255)
      };
      bar(
        ui,
        Rect::new(x, y, intro_w, 18.0),
        1.0,
        track_color.with_alpha(90),
      );
      bar(
        ui,
        Rect::new(x + intro_w, y, timeline_w - intro_w, 18.0),
        1.0,
        track_color.with_alpha(170),
      );
      ui.draw_text("intro", x + 4.0, y + 1.0, Color::WHITE);
      ui.draw_text("loop", x + intro_w + 4.0, y + 1.0, Color::WHITE);
      let sounding = is_current || track.fading.is_some();
      if sounding {
        let at = if track.played < track.intro || track.looped <= 0.0 {
          track.played
        } else {
          track.intro + (track.played - track.intro) % track.looped
        };
        let head = x + timeline_w * (at / total).min(1.0);
        ui.draw_rect(Rect::new(head - 1.0, y - 4.0, 3.0, 26.0), Color::WHITE);
        ui.draw_text(
          &format!("{at:.1} s"),
          x + timeline_w + 12.0,
          y + 1.0,
          Color::WHITE,
        );
      }
      y += 48.0;
    }

    ui.draw_text(
      "Asking for the track playing changes nothing; asking for the other crossfades:",
      x,
      y,
      Color::LIGHT_GRAY,
    );
    y += 20.0;
    ui.draw_text(
      "the old fades out as the new fades in, so the music bus has two voices.",
      x,
      y,
      Color::LIGHT_GRAY,
    );
    y += 34.0;
    bus_meters(ui, ctx, x, y, &["music"]);
  }
}
