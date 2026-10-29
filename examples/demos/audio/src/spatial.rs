//! The Spatial page: three emitters circle the point the listener starts
//! at, each a loop heard from where it is, on one of the three falloff
//! curves. Walk the listener (an `AudioListener`) towards and away from
//! them: each gets quieter with distance and pans by its offset. Solo one
//! (1, 2, 3; 0 for all three) to hear it alone, and click 150 px to one
//! side of the listener for a ping heard fully on that side. The page uses
//! UI units as world units, so what it draws is where the sounds are; a game
//! with a camera turns a screen position (the pointer's, say) into a world
//! one with `ctx.camera().screen_to_world` before playing there

use std::time::Duration;

use demo_kit::prelude::*;

use crate::shared::{DIM, bar, bus_meters, db_fraction, db_text, load, load_mixer};

/// Where the listener starts, and the emitters circle
const CENTRE: Vec2 = Vec2::new(430.0, 380.0);
/// How far the listener may walk from the centre
const FIELD: Vec2 = Vec2::new(250.0, 150.0);
/// The listener's speed, in units a fixed step
const WALK: f32 = 3.0;
/// How far to the listener's side a sound is heard fully on that side: the
/// ping's (and the emitters') `pan_width`
const FULL_SIDE: f32 = 150.0;
/// How long a solo takes to fade the others out, or back in
const SOLO_FADE: Duration = Duration::from_millis(80);
/// The keys that solo each emitter, in order, and the key for all three
const SOLO_KEYS: [KeyCode; 3] = [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3];
const ALL_KEY: KeyCode = KeyCode::Digit0;

struct Emitter {
  entity: Entity,
  name: &'static str,
  curve: &'static str,
  sound: Handle<Sound>,
  radius: f32,
  /// Turns a second at an orbit speed of 1
  turns: f32,
  /// Where on its circle it starts, in turns
  phase: f32,
  color: Color,
  position: Vec2,
  /// Its loop, played as it spawned
  voice: Voice,
}

pub(crate) struct SpatialPage {
  listener: Option<Entity>,
  at: Vec2,
  emitters: Vec<Emitter>,
  ping: Option<Handle<Sound>>,
  /// The latest ping's place, and the frame it played in
  pinged: Option<(Vec2, u32)>,
  orbit_speed: f32,
  orbiting: bool,
  /// How far round the orbits have come, in seconds at speed 1
  orbit: f32,
  /// The emitter heard alone; all three when `None`
  solo: Option<usize>,
  frame: u32,
  _mixer: Option<Handle<Mixer>>,
}

impl SpatialPage {
  pub(crate) fn new() -> Self {
    Self {
      listener: None,
      at: CENTRE,
      emitters: Vec::new(),
      ping: None,
      pinged: None,
      orbit_speed: 1.0,
      orbiting: true,
      orbit: 0.0,
      solo: None,
      frame: 0,
      _mixer: None,
    }
  }

  /// Move each emitter to its place on its circle
  fn place_emitters(&mut self, ctx: &mut SceneContext) {
    for emitter in &mut self.emitters {
      emitter.position = orbit_point(emitter.radius, emitter.turns, emitter.phase, self.orbit);
      if let Some(mut transform) = ctx.world.get_mut::<Transform>(emitter.entity) {
        *transform = Transform::from_xy(emitter.position.x, emitter.position.y);
      }
    }
  }

  /// Hear emitter `solo` alone, or all three (`None`): the others fade to
  /// silence (-60 dB) on their voices, and back
  fn solo(&mut self, solo: Option<usize>) {
    self.solo = solo;
    for (index, emitter) in self.emitters.iter().enumerate() {
      let heard = solo.is_none_or(|solo| solo == index);
      let db = if heard { 0.0 } else { -60.0 };
      emitter.voice.set_volume_over(db, SOLO_FADE);
    }
  }

  /// Whether emitter `index` is heard: soloed, or nothing is
  fn heard(&self, index: usize) -> bool {
    self.solo.is_none_or(|solo| solo == index)
  }
}

/// Where on its circle an emitter of `radius` is, `orbit` seconds round at
/// `turns` a second from `phase` (in turns)
fn orbit_point(radius: f32, turns: f32, phase: f32, orbit: f32) -> Vec2 {
  let angle = std::f32::consts::TAU * (phase + turns * orbit);
  CENTRE + radius * Vec2::new(angle.cos(), angle.sin())
}

impl DemoPage for SpatialPage {
  fn name(&self) -> &str {
    "Spatial"
  }

  fn description(&self) -> &str {
    "Emitters are heard from where they are: quieter with distance, panned by their offset"
  }

  fn settings(&mut self, s: &mut Settings) {
    s.float("Orbit speed", &mut self.orbit_speed, 0.0..=2.0, 0.25);
    s.toggle("Orbiting", &mut self.orbiting);
  }

  fn controls(&self) -> Vec<Control> {
    vec![
      Control::keys(
        "WASD",
        &[KeyCode::KeyW, KeyCode::KeyA, KeyCode::KeyS, KeyCode::KeyD],
        "Walk the listener",
      ),
      Control::key(KeyCode::Space, "Back to the centre"),
      Control::keys("1 2 3", &SOLO_KEYS, "Solo hum, chirp or buzz"),
      Control::key(ALL_KEY, "All three"),
      Control::mouse("Click: a ping there"),
    ]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    self._mixer = load_mixer(ctx);
    self.ping = load(ctx, "spatial/ping.sound.ron");
    self.listener = Some(
      ctx
        .world
        .spawn((AudioListener, Transform::from_xy(CENTRE.x, CENTRE.y)))
        .id(),
    );
    let specs = [
      (
        "hum",
        "Linear",
        "spatial/hum.sound.ron",
        70.0,
        0.11,
        0.0,
        Color::rgb(255, 160, 70),
      ),
      (
        "chirp",
        "Inverse",
        "spatial/chirp.sound.ron",
        125.0,
        -0.07,
        0.35,
        Color::rgb(110, 210, 120),
      ),
      (
        "buzz",
        "Log",
        "spatial/buzz.sound.ron",
        175.0,
        0.05,
        0.7,
        Color::rgb(90, 170, 255),
      ),
    ];
    for (name, curve, path, radius, turns, phase, color) in specs {
      let Some(sound) = load::<Sound>(ctx, path) else {
        continue;
      };
      // Spawned where it starts, and played: its voice follows it (kept
      // to solo it), and stops as the page closes and despawns it
      let emitter = AudioEmitter {
        sound: sound.clone(),
        play_on_spawn: false,
      };
      let at = orbit_point(radius, turns, phase, self.orbit);
      let entity = ctx
        .world
        .spawn((emitter, Transform::from_xy(at.x, at.y)))
        .id();
      let voice = ctx.audio().play_emitter(entity);
      self.emitters.push(Emitter {
        entity,
        name,
        curve,
        sound,
        radius,
        turns,
        phase,
        color,
        position: at,
        voice,
      });
    }
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    self.frame += 1;
    if self.orbiting {
      self.orbit += self.orbit_speed * dt;
    }
    self.place_emitters(ctx);

    let input = &*ctx.input;
    let axis = |minus: KeyCode, plus: KeyCode| {
      f32::from(u8::from(input.key_down(plus))) - f32::from(u8::from(input.key_down(minus)))
    };
    let step = Vec2::new(
      axis(KeyCode::KeyA, KeyCode::KeyD),
      axis(KeyCode::KeyW, KeyCode::KeyS),
    );
    self.at = (self.at + step * WALK).clamp(CENTRE - FIELD, CENTRE + FIELD);
    if input.key_pressed(KeyCode::Space) {
      self.at = CENTRE;
    }
    let soloed = SOLO_KEYS.iter().position(|&key| input.key_pressed(key));
    let solo = match soloed {
      Some(index) => Some(Some(index)),
      None => input.key_pressed(ALL_KEY).then_some(None),
    };
    let click = input
      .mouse_pressed(MouseButton::Left)
      .then(|| Vec2::from(input.mouse_position()));
    if let Some(listener) = self.listener
      && let Some(mut transform) = ctx.world.get_mut::<Transform>(listener)
    {
      *transform = Transform::from_xy(self.at.x, self.at.y);
    }
    if let Some(solo) = solo {
      self.solo(solo);
    }
    // Screen and world units are one here; a game converts the pointer with
    // `ctx.camera().screen_to_world`
    if let (Some(place), Some(ping)) = (click, &self.ping) {
      ctx.audio().play_at(ping, place);
      self.pinged = Some((place, self.frame));
    }
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    let area = content_area(ui);
    let x = area.x + 16.0;
    let mut y = area.y + 10.0;

    // Each emitter as the listener hears it, by the engine's own curves
    let (curve_x, distance_x, gain_x, pan_x) = (x + 70.0, x + 150.0, x + 230.0, x + 400.0);
    ui.draw_text("Emitter", x, y, DIM);
    ui.draw_text("Curve", curve_x, y, DIM);
    ui.draw_text("Distance", distance_x, y, DIM);
    ui.draw_text("Falloff", gain_x, y, DIM);
    ui.draw_text("Pan", pan_x, y, DIM);
    y += 24.0;
    let assets = ctx.assets();
    let mut heard = Vec::with_capacity(self.emitters.len());
    for (index, emitter) in self.emitters.iter().enumerate() {
      let Some(spatial) = assets.get(&emitter.sound).and_then(|sound| sound.spatial) else {
        continue;
      };
      let offset = emitter.position - self.at;
      let distance = offset.length();
      let gain = falloff_gain_db(&spatial, distance);
      let pan = pan_for(&spatial, offset.x);
      // A muted one is drawn but not heard
      let audible = self.heard(index);
      if audible {
        heard.push((emitter.position, gain, emitter.color));
      }
      let name_color = if audible { emitter.color } else { DIM };
      ui.draw_text(emitter.name, x, y, name_color);
      ui.draw_text(emitter.curve, curve_x, y, Color::WHITE);
      ui.draw_text(&format!("{distance:.0}"), distance_x, y, Color::WHITE);
      bar(
        ui,
        Rect::new(gain_x, y + 3.0, 90.0, 12.0),
        db_fraction(gain),
        emitter.color,
      );
      ui.draw_text(&db_text(gain), gain_x + 98.0, y, Color::WHITE);
      // Pan from -1 (left) to 1 (right)
      let track = Rect::new(pan_x, y + 8.0, 80.0, 2.0);
      ui.draw_rect(track, Color::new(255, 255, 255, 60));
      let mark = pan_x + (pan + 1.0) * 0.5 * 80.0;
      ui.draw_rect(Rect::new(mark - 2.0, y + 2.0, 4.0, 14.0), emitter.color);
      ui.draw_text(&format!("{pan:+.2}"), pan_x + 90.0, y, Color::WHITE);
      match self.solo {
        Some(solo) if solo == index => ui.draw_text("solo", pan_x + 150.0, y, emitter.color),
        Some(_) => ui.draw_text("muted", pan_x + 150.0, y, DIM),
        None => {}
      }
      y += 22.0;
    }
    drop(assets);
    ui.draw_text(
      "A ping clicked 150 px left or right of the listener (the marks) is heard fully on that side",
      x,
      y + 6.0,
      DIM,
    );

    // The orbits, and a line from the listener to each emitter as bright
    // as it is heard
    for emitter in &self.emitters {
      let points: Vec<Vec2> = (0..=64)
        .map(|i| {
          let angle = std::f32::consts::TAU * i as f32 / 64.0;
          CENTRE + emitter.radius * Vec2::new(angle.cos(), angle.sin())
        })
        .collect();
      ui.draw_line(&points, 1.0, emitter.color.with_alpha(50));
    }
    for (position, gain, color) in heard {
      let alpha = (db_fraction(gain) * 230.0) as u8;
      if alpha > 4 {
        ui.draw_line(&[self.at, position], 2.0, color.with_alpha(alpha));
      }
      ui.draw_rounded_rect(
        Rect::new(position.x - 9.0, position.y - 9.0, 18.0, 18.0),
        9.0,
        color,
      );
    }
    if let Some((place, frame)) = self.pinged {
      let age = self.frame.saturating_sub(frame);
      if age < 40 {
        let r = 6.0 + age as f32;
        let alpha = (200 - age * 5) as u8;
        ui.draw_rect_outline(
          Rect::new(place.x - r, place.y - r, 2.0 * r, 2.0 * r),
          Color::WHITE.with_alpha(alpha),
          1.0,
        );
      }
    }
    // Where a ping is heard fully to one side: the pan width either way
    for side in [-FULL_SIDE, FULL_SIDE] {
      let mark = Rect::new(self.at.x + side - 1.0, self.at.y - 8.0, 2.0, 16.0);
      ui.draw_rect(mark, Color::WHITE.with_alpha(90));
    }
    // The listener: a head, its right ear marked
    let head = Rect::new(self.at.x - 11.0, self.at.y - 11.0, 22.0, 22.0);
    ui.draw_rounded_rect(head, 11.0, Color::WHITE);
    ui.draw_rect(
      Rect::new(self.at.x + 10.0, self.at.y - 3.0, 4.0, 6.0),
      Color::WHITE,
    );
    ui.draw_text("listener", self.at.x - 26.0, self.at.y + 14.0, DIM);

    // Right of the field, below the settings panel
    bus_meters(
      ui,
      ctx,
      CENTRE.x + FIELD.x + 36.0,
      CENTRE.y + 30.0,
      &["sfx"],
    );
  }
}
