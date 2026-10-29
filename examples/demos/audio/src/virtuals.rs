//! The Virtual voices page: thirty birds sing in a field, each a loop heard
//! from its emitter, and the listener wanders among them (or walks, with
//! WASD). The birds' sound allows six voices at once, so six are heard,
//! the loudest as heard; the rest are virtual: each keeps its place
//! silently, taking no voice, and comes back as the listener nears it,
//! taking the place of a heard one 3 dB quieter. A bird beyond the range
//! ring takes no voice at all. The page draws each bird by what its
//! `Voice` says (`is_virtual`), and counts them with `voices_on` and
//! `virtual_voices_on`. UI units are world units here, as on the Spatial
//! page

use demo_kit::prelude::*;

use crate::shared::{DIM, load, load_mixer};

/// The birds' sound file
const BIRD: &str = "virtual/bird.sound.ron";
/// The bus they play on
pub const BUS: &str = "ambience";
/// How many birds sing
pub const BIRDS: usize = 30;
/// How many the sound's limit hears at once
pub const HEARD: usize = 6;
/// The field the birds sing in: left of the settings panel, below the
/// page's lines
fn field() -> Rect {
  Rect::new(24.0, 150.0, 496.0, 360.0)
}
/// The field's grid of birds, and how far each strays from its cell's
/// centre, as a fraction of the cell
const COLUMNS: usize = 6;
const ROWS: usize = 5;
const STRAY: f32 = 0.35;
/// The listener's walking speed, in units a fixed step
const WALK: f32 = 3.0;
/// How fast the listener wanders: turns a second across, and down
const WANDER: (f32, f32) = (0.045, 0.065);

/// A heard bird
const HEARD_COLOR: Color = Color::rgb(110, 210, 120);
/// A virtual bird in range: waiting for room
const WAITING_COLOR: Color = Color::rgb(255, 200, 90);
/// A virtual bird out of range
const FAR_COLOR: Color = Color::rgb(110, 110, 125);

struct Bird {
  at: Vec2,
  voice: Voice,
}

/// What a bird is doing, as the page draws it
#[derive(Clone, Copy, PartialEq)]
enum State {
  Heard,
  /// Virtual in range: a limit has no room for it
  Waiting,
  /// Virtual, at or beyond its range
  Far,
}

pub(crate) struct VirtualPage {
  listener: Option<Entity>,
  at: Vec2,
  birds: Vec<Bird>,
  wandering: bool,
  /// How far the wander has come, in seconds
  wander: f32,
  speed: f32,
  sound: Option<Handle<Sound>>,
  _mixer: Option<Handle<Mixer>>,
}

impl VirtualPage {
  pub(crate) fn new() -> Self {
    Self {
      listener: None,
      at: Vec2::ZERO,
      birds: Vec::new(),
      wandering: true,
      wander: 0.0,
      speed: 1.0,
      sound: None,
      _mixer: None,
    }
  }

  /// Each bird's state, by its voice and its distance from the listener
  fn state(&self, bird: &Bird, range: f32) -> State {
    match bird.voice.is_virtual() {
      false => State::Heard,
      true if bird.at.distance(self.at) < range => State::Waiting,
      true => State::Far,
    }
  }
}

/// Where the wander has the listener after `seconds`: a slow loop about
/// the field, never quite at its edges
fn wandered(seconds: f32) -> Vec2 {
  let field = field();
  let tau = std::f32::consts::TAU;
  let across = (tau * WANDER.0 * seconds).sin();
  let down = (tau * WANDER.1 * seconds + 0.7).sin();
  let centre = Vec2::new(field.x + field.width / 2.0, field.y + field.height / 2.0);
  centre + Vec2::new(0.42 * field.width * across, 0.40 * field.height * down)
}

/// Where bird `index` sings in the field: a cell of a grid, strayed from
/// its centre by a fixed amount, so the layout is the same each time
fn bird_place(index: usize) -> Vec2 {
  let field = field();
  let (column, row) = (index % COLUMNS, index / COLUMNS);
  let cell = Vec2::new(field.width / COLUMNS as f32, field.height / ROWS as f32);
  // From -1 to 1, the same for the same seed
  let stray = |seed: f32| ((seed * 12.9898).sin() * 43_758.547).fract().abs() * 2.0 - 1.0;
  let offset = Vec2::new(stray(index as f32 + 1.0), stray(index as f32 + 41.0)) * STRAY;
  Vec2::new(
    field.x + (column as f32 + 0.5 + offset.x) * cell.x,
    field.y + (row as f32 + 0.5 + offset.y) * cell.y,
  )
}

/// A circle of `radius` about `centre`, as a line's points
fn ring(centre: Vec2, radius: f32) -> Vec<Vec2> {
  (0..=72)
    .map(|i| {
      let angle = std::f32::consts::TAU * i as f32 / 72.0;
      centre + radius * Vec2::new(angle.cos(), angle.sin())
    })
    .collect()
}

impl DemoPage for VirtualPage {
  fn name(&self) -> &str {
    "Virtual voices"
  }

  fn description(&self) -> &str {
    "Loops past a limit or out of range keep their place silently, and come back as they can be heard"
  }

  fn settings(&mut self, s: &mut Settings) {
    s.toggle("Wandering", &mut self.wandering);
    s.float("Wander speed", &mut self.speed, 0.25..=3.0, 0.25);
  }

  fn controls(&self) -> Vec<Control> {
    vec![
      Control::keys(
        "WASD",
        &[KeyCode::KeyW, KeyCode::KeyA, KeyCode::KeyS, KeyCode::KeyD],
        "Walk the listener (stops the wander)",
      ),
      Control::key(KeyCode::Space, "Wander again from the centre"),
    ]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    self._mixer = load_mixer(ctx);
    self.sound = load(ctx, BIRD);
    self.at = wandered(0.0);
    self.listener = Some(
      ctx
        .world
        .spawn((AudioListener, Transform::from_xy(self.at.x, self.at.y)))
        .id(),
    );
    let Some(sound) = self.sound.clone() else {
      return;
    };
    // Nearest first: the six nearest are heard from the start, and the
    // rest go virtual (the sound's limit takes no voice started in the
    // same frame)
    let mut places: Vec<Vec2> = (0..BIRDS).map(bird_place).collect();
    places.sort_by(|a, b| a.distance(self.at).total_cmp(&b.distance(self.at)));
    for at in places {
      let emitter = AudioEmitter {
        sound: sound.clone(),
        play_on_spawn: false,
      };
      let entity = ctx
        .world
        .spawn((emitter, Transform::from_xy(at.x, at.y)))
        .id();
      let voice = ctx.audio().play_emitter(entity);
      self.birds.push(Bird { at, voice });
    }
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    let input = &*ctx.input;
    let axis = |minus: KeyCode, plus: KeyCode| {
      f32::from(u8::from(input.key_down(plus))) - f32::from(u8::from(input.key_down(minus)))
    };
    let step = Vec2::new(
      axis(KeyCode::KeyA, KeyCode::KeyD),
      axis(KeyCode::KeyW, KeyCode::KeyS),
    );
    if step != Vec2::ZERO {
      self.wandering = false;
    }
    if input.key_pressed(KeyCode::Space) {
      self.wander = 0.0;
      self.wandering = true;
    }
    if self.wandering {
      self.wander += self.speed * dt;
      self.at = wandered(self.wander);
    } else {
      let field = field();
      let min = Vec2::new(field.x, field.y);
      let max = min + Vec2::new(field.width, field.height);
      self.at = (self.at + step * WALK).clamp(min, max);
    }
    if let Some(listener) = self.listener
      && let Some(mut transform) = ctx.world.get_mut::<Transform>(listener)
    {
      *transform = Transform::from_xy(self.at.x, self.at.y);
    }
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    let area = content_area(ui);
    let (x, y) = (area.x + 16.0, area.y + 10.0);
    ui.draw_text(
      &format!(
        "{BIRDS} birds sing, each a loop heard from its emitter. Their sound's limit hears {HEARD} at once, the loudest as heard:"
      ),
      x,
      y,
      Color::LIGHT_GRAY,
    );
    ui.draw_text(
      "the rest are virtual, keeping their place with no voice, and come back as the listener nears them.",
      x,
      y + 20.0,
      Color::LIGHT_GRAY,
    );
    // What a bird's mark means
    let mut key_x = x;
    for (color, label) in [
      (HEARD_COLOR, "heard"),
      (WAITING_COLOR, "virtual, in range: waiting for room"),
      (FAR_COLOR, "virtual, out of range"),
    ] {
      ui.draw_rounded_rect(Rect::new(key_x, y + 48.0, 12.0, 12.0), 6.0, color);
      ui.draw_text(label, key_x + 18.0, y + 45.0, Color::LIGHT_GRAY);
      key_x += 34.0 + ui.text_size(label).0 as f32;
    }

    let range = self
      .sound
      .as_ref()
      .and_then(|sound| ctx.assets().get(sound).and_then(|sound| sound.spatial))
      .map_or(0.0, |spatial| spatial.max_distance);
    let field = field();
    ui.draw_rect(field, Color::new(255, 255, 255, 10));
    // The field's drawing stays in the field
    ui.push_clip(field);
    ui.draw_line(&ring(self.at, range), 1.0, Color::WHITE.with_alpha(70));
    let mut counts = [0usize; 3];
    for bird in &self.birds {
      let state = self.state(bird, range);
      counts[state as usize] += 1;
      let at = bird.at;
      match state {
        State::Heard => {
          ui.draw_line(&[self.at, at], 2.0, HEARD_COLOR.with_alpha(120));
          ui.draw_rounded_rect(
            Rect::new(at.x - 7.0, at.y - 7.0, 14.0, 14.0),
            7.0,
            HEARD_COLOR,
          );
        }
        State::Waiting => {
          ui.draw_line(&ring(at, 6.0), 2.0, WAITING_COLOR);
        }
        State::Far => {
          ui.draw_rounded_rect(Rect::new(at.x - 3.0, at.y - 3.0, 6.0, 6.0), 3.0, FAR_COLOR);
        }
      }
    }
    // The listener
    let head = Rect::new(self.at.x - 9.0, self.at.y - 9.0, 18.0, 18.0);
    ui.draw_rounded_rect(head, 9.0, Color::WHITE);
    ui.draw_text("listener", self.at.x - 26.0, self.at.y + 12.0, DIM);
    ui.pop_clip();
    ui.draw_text(
      &format!("the ring: {range:.0} px, the birds' max_distance"),
      field.x,
      field.y + field.height + 6.0,
      DIM,
    );

    // The counts, right of the field and below the settings panel
    let mut audio = ctx.audio();
    let (heard, kept) = (audio.voices_on(BUS), audio.virtual_voices_on(BUS));
    let column = field.x + field.width + 20.0;
    let mut row = area.y + settings_panel_height(ui, 2) + 20.0;
    for (text, color) in [
      (format!("voices_on(\"{BUS}\") = {heard}"), HEARD_COLOR),
      (
        format!("virtual_voices_on(\"{BUS}\") = {kept}"),
        WAITING_COLOR,
      ),
      (
        format!(
          "in range, waiting for room: {}",
          counts[State::Waiting as usize]
        ),
        Color::LIGHT_GRAY,
      ),
      (
        format!("out of range: {}", counts[State::Far as usize]),
        Color::LIGHT_GRAY,
      ),
    ] {
      ui.draw_text(&text, column, row, color);
      row += 22.0;
    }
    ui.draw_text(
      "A waiting bird comes back 3 dB louder",
      column,
      row + 10.0,
      DIM,
    );
    ui.draw_text(
      "than the quietest heard, after 250 ms",
      column,
      row + 30.0,
      DIM,
    );
  }
}
