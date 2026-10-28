//! The Hot reload page: the drone loop and the beep's definitions, their
//! clips and the mixer, each with a count of its `AssetEvent::Modified`.
//! With hot reload on (a debug build with a window), saving one of them in
//! `assets/` reloads it under the same handle: the drone loop takes a new
//! volume or pitch at once, the next beep plays as saved, and a saved
//! mixer rebuilds the buses with the loop carrying on

use demo_kit::prelude::*;

use crate::shared::{BAD, DIM, HOT, bus_meters, load, load_mixer};

const DRONE: &str = "hot/drone.sound.ron";
const BEEP: &str = "hot/beep.sound.ron";

/// What a row holds
enum Held {
  Sound(Handle<Sound>),
  Clip(Handle<AudioClip>),
  Mixer(Handle<Mixer>),
}

impl Held {
  fn id(&self) -> UntypedAssetId {
    match self {
      Held::Sound(handle) => handle.id().untyped(),
      Held::Clip(handle) => handle.id().untyped(),
      Held::Mixer(handle) => handle.id().untyped(),
    }
  }

  fn kind(&self) -> &'static str {
    match self {
      Held::Sound(_) => "Sound",
      Held::Clip(_) => "AudioClip",
      Held::Mixer(_) => "Mixer",
    }
  }
}

struct Row {
  path: &'static str,
  held: Held,
  modified: u32,
  /// Why its last reload failed (it keeps its last good version), until
  /// it next changes
  error: Option<String>,
}

/// The Hot reload page's counts while it is open: each file's
/// `AssetEvent::Modified` since the page opened. The page keeps them as a
/// resource, so a test (or another system) can read them
#[derive(Resource, Debug, Default)]
pub struct Reloads {
  counts: Vec<(&'static str, u32)>,
}

impl Reloads {
  /// `path`'s count, if the page tracks it
  pub fn modified(&self, path: &str) -> Option<u32> {
    self
      .counts
      .iter()
      .find(|(tracked, _)| *tracked == path)
      .map(|&(_, count)| count)
  }
}

pub(crate) struct HotReloadPage {
  rows: Vec<Row>,
  drone: Option<Handle<Sound>>,
  beep: Option<Handle<Sound>>,
}

impl HotReloadPage {
  pub(crate) fn new() -> Self {
    Self {
      rows: Vec::new(),
      drone: None,
      beep: None,
    }
  }

  /// The counts, as the page keeps them in [`Reloads`]
  fn publish(&self, ctx: &mut SceneContext) {
    let counts = self
      .rows
      .iter()
      .map(|row| (row.path, row.modified))
      .collect();
    ctx.world.insert_resource(Reloads { counts });
  }

  /// Count this step's events for `T`'s assets
  fn count<T: Asset>(&mut self, ctx: &mut SceneContext) {
    let mut changed = false;
    for event in ctx.messages::<AssetEvent<T>>() {
      let (id, error) = match event {
        AssetEvent::Modified { id } => (id.untyped(), None),
        AssetEvent::ReloadFailed { id, error } => (id.untyped(), Some(error.report())),
        _ => continue,
      };
      let Some(row) = self.rows.iter_mut().find(|row| row.held.id() == id) else {
        continue;
      };
      match error {
        None => {
          row.modified += 1;
          row.error = None;
          changed = true;
        }
        Some(error) => row.error = Some(format!("reload failed: {error}")),
      }
    }
    if changed {
      self.publish(ctx);
    }
  }
}

impl DemoPage for HotReloadPage {
  fn name(&self) -> &str {
    "Hot reload"
  }

  fn description(&self) -> &str {
    "Save a definition, a clip or the mixer in assets/: it reloads in place, and its count goes up"
  }

  fn controls(&self) -> Vec<Control> {
    vec![Control::key(KeyCode::Space, "Beep")]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    let mixer = load_mixer(ctx);
    self.drone = load(ctx, DRONE);
    self.beep = load(ctx, BEEP);
    let clips = ["sounds/drone.wav", "sounds/beep.wav"].map(|path| (path, load(ctx, path)));
    let sounds = [(DRONE, self.drone.clone()), (BEEP, self.beep.clone())];
    for (path, sound) in sounds {
      if let Some(sound) = sound {
        self.rows.push(Row {
          path,
          held: Held::Sound(sound),
          modified: 0,
          error: None,
        });
      }
    }
    for (path, clip) in clips {
      if let Some(clip) = clip {
        self.rows.push(Row {
          path,
          held: Held::Clip(clip),
          modified: 0,
          error: None,
        });
      }
    }
    if let Some(mixer) = mixer {
      self.rows.push(Row {
        path: MIXER_PATH,
        held: Held::Mixer(mixer),
        modified: 0,
        error: None,
      });
    }
    self.publish(ctx);
    if let Some(drone) = &self.drone {
      ctx.audio().play(drone);
    }
  }

  fn exit(&mut self, ctx: &mut SceneContext) {
    // A resource outlives the page unless taken away
    ctx.world.remove_resource::<Reloads>();
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    self.count::<Sound>(ctx);
    self.count::<AudioClip>(ctx);
    self.count::<Mixer>(ctx);
    if ctx.input.key_pressed(KeyCode::Space)
      && let Some(beep) = &self.beep
    {
      ctx.audio().play(beep);
    }
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    let area = content_area(ui);
    let x = area.x + 16.0;
    let mut y = area.y + 10.0;
    for line in [
      "Save a file in examples/demos/audio/assets/ while the demo runs: it reloads",
      "and its count goes up. Change the drone's volume or pitch to hear the loop",
      "follow at once. Debug builds with a window watch the folder; release,",
      "headless and web builds do not.",
    ] {
      ui.draw_text(line, x, y, Color::LIGHT_GRAY);
      y += 20.0;
    }
    y += 16.0;

    let (count_x, now_x) = (x + 240.0, x + 360.0);
    ui.draw_text("File", x, y, DIM);
    ui.draw_text("AssetEvent::Modified", count_x, y, DIM);
    y += 26.0;
    let assets = ctx.assets();
    for row in &self.rows {
      ui.draw_text(row.path, x, y, Color::WHITE);
      let color = if row.modified > 0 {
        HOT
      } else {
        Color::LIGHT_GRAY
      };
      ui.draw_text(&format!("modified {}", row.modified), count_x, y, color);
      // What it says now
      let now = match &row.held {
        Held::Sound(sound) => assets
          .get(sound)
          .map(|sound| format!("{:+.1} dB, pitch {:+.1}", sound.volume_db, sound.pitch)),
        Held::Clip(clip) => assets.get(clip).map(|clip| {
          format!(
            "{:.2} s at {} Hz",
            clip.duration().as_secs_f32(),
            clip.sample_rate()
          )
        }),
        Held::Mixer(mixer) => assets.get(mixer).map(|mixer| {
          [
            (mixer.buses.len(), "bus", "buses"),
            (mixer.snapshots.len(), "snapshot", "snapshots"),
            (mixer.ducks.len(), "duck", "ducks"),
          ]
          .map(|(n, one, many)| format!("{n} {}", if n == 1 { one } else { many }))
          .join(", ")
        }),
      };
      ui.draw_text(
        &format!("{}  {}", row.held.kind(), now.unwrap_or_default()),
        now_x,
        y,
        DIM,
      );
      y += 24.0;
      if let Some(error) = &row.error {
        let size = ui
          .text(error.as_str())
          .at(x + 16.0, y)
          .wrap(560.0)
          .color(BAD)
          .draw();
        y += size.y + 6.0;
      }
    }
    drop(assets);
    y += 16.0;
    bus_meters(ui, ctx, x, y, &["sfx", "ambience"]);
  }
}
