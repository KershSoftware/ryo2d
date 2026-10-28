//! The mixer from `.mixer.ron` text

use crate::core::asset::{AssetError, AssetLoader, LoadContext};
use crate::core::audio::Mixer;

/// Loads a [`Mixer`] from a `.mixer.ron` file. The engine loads
/// `audio/mixer.mixer.ron` at start if the game has one; without it the
/// standard layout plays. Every field may be left out:
///
/// ```ron
/// (
///   buses: {
///     "master": (effects: [Compressor(threshold_db: -12.0, ratio: 4.0)]),
///     "sfx": (volume: -3.0, limit: 48),
///     "sfx/footsteps": (
///       volume: -6.0,                          // 0.0 dB
///       effects: [HighPass(cutoff: 120.0)],    // none
///       sends: { "sfx/reverb": -12.0 },        // none; level in dB
///       pauses_with_game: true,                // the parent's
///       follows_time_scale: true,              // the parent's
///       limit: 8,                              // none
///     ),
///     "sfx/reverb": (effects: [Reverb(feedback: 0.85, mix: 1.0)]),
///   },
///   on_focus_loss: Duck(-12.0),                // Keep; or Mute
/// )
/// ```
///
/// - **Buses** are `master`, the standard buses (`music`, `sfx`, `ui`,
///   `voice`, `ambience`) and a game's own under them, named by their path
///   (`"sfx/footsteps"`, whose parent `sfx` must exist: a bus under a game's
///   bus names that bus first). The standard buses are always there; the
///   file only adjusts them.
/// - **Volume** is in dB, at most +24.
/// - **Effects** run in order: `LowPass(cutoff, resonance)`,
///   `HighPass(cutoff, resonance)`, `Eq(kind: Bell | LowShelf | HighShelf,
///   frequency, gain_db, q)`, `Reverb(feedback, damping, stereo_width,
///   mix)`, `Delay(seconds, feedback_db, mix)`, `Compressor(threshold_db,
///   ratio, attack, release, makeup_db, mix)` and `Distortion(kind:
///   HardClip | SoftClip, drive_db, mix)`; see
///   [`EffectDef`](crate::core::audio::EffectDef) for each parameter's range
///   and default.
/// - **Sends** go to send buses: a game's bus that another bus sends to.
///   A send bus plays no voices (a sound on it plays on its parent), has no
///   buses under it, sends nowhere and takes no limit or pause settings;
///   `master` sends nowhere. A send is taken after the sending bus's volume
///   and carries the gains of the buses above the sender, so a return
///   follows its sender's sliders: `voice/npc` sending to `sfx/reverb` goes
///   quiet with the voice slider, not the sfx one. The return is heard at
///   the send bus's own gain, then `master`'s. Only the send bus's own
///   effects process it: kira mixes send buses into its output beside
///   `master`'s track, not through it, so `master`'s effects (a limiter or
///   compressor, say) do not reach returns.
/// - **Pause and time scale**: `sfx`, `ambience` and `voice` pause with the
///   game, and `sfx` and `ambience` follow time scale; a game's bus does as
///   its parent unless it says.
/// - **Limit** caps the voices on the bus and every bus under it (1 or
///   more).
///
/// A mistake fails the load with `AssetError::Decode`, naming the file and
/// what is wrong; a reload that fails keeps the mix playing as it was. A
/// saved mixer rebuilds the bus tree: one-shots finish on the old buses,
/// and loops carry on, at their position, on the new ones (a loop that
/// cannot move is warned about once, and plays on where it was). The old
/// buses keep the volumes they had, so a slider moved while a one-shot
/// finishes there does not reach it; this is only while a reload is
/// played out. A game with no file can build one in code with
/// `ctx.audio().set_mixer(..)`. Every registry has it
#[derive(Debug, Default, Clone, Copy)]
pub struct MixerLoader;

impl AssetLoader for MixerLoader {
  type Asset = Mixer;
  type Settings = ();

  fn extensions(&self) -> &[&'static str] {
    &["mixer.ron"]
  }

  fn load(&self, bytes: Vec<u8>, _: &(), cx: &mut LoadContext<'_>) -> Result<Mixer, AssetError> {
    let path = cx.path().clone();
    let text = String::from_utf8(bytes).map_err(|error| AssetError::decode(&path, error))?;
    Mixer::parse(&text).map_err(|error| AssetError::decode(&path, error))
  }
}

#[cfg(test)]
mod tests {
  use crate::core::asset::{AssetServer, AssetsView, MemorySource, insert_registry};
  use crate::core::audio::{BusDef, DistortionKind, EffectDef, EqKind, FocusLoss, Mixer};
  use crate::core::ecs::kernel::World;

  const PATH: &str = "audio/test.mixer.ron";

  fn load(text: &str) -> Result<Mixer, String> {
    let files = MemorySource::new();
    files.insert(PATH, text);
    let mut world = World::new();
    insert_registry(&mut world, AssetServer::new(files));
    let mut assets = AssetsView::new(&mut world);
    let mixer = assets.load_now::<Mixer>(PATH).map_err(|e| e.report())?;
    assets
      .get(&mixer)
      .cloned()
      .ok_or_else(|| "not in the store".to_string())
  }

  fn loaded(text: &str) -> Mixer {
    match load(text) {
      Ok(mixer) => mixer,
      Err(error) => panic!("{error}"),
    }
  }

  fn failure(text: &str) -> String {
    match load(text) {
      Ok(mixer) => panic!("{text} loaded: {mixer:?}"),
      Err(error) => error,
    }
  }

  fn bus<'a>(mixer: &'a Mixer, name: &str) -> &'a BusDef {
    match mixer.buses.iter().find(|bus| bus.name == name) {
      Some(bus) => bus,
      None => panic!("no {name:?} in {mixer:?}"),
    }
  }

  #[test]
  fn an_empty_file_is_the_standard_layout() {
    assert_eq!(loaded("()"), Mixer::default());
    assert_eq!(Mixer::default().on_focus_loss, FocusLoss::Keep);
  }

  #[test]
  fn snapshots_and_ducks_read_as_written() {
    let mixer = loaded(
      r#"(
        snapshots: {
          "pause": (blend: 0.5, buses: { "music": (volume: -6.0, low_pass: 800.0), "sfx": () }),
          "plain": (),
        },
        ducks: [(trigger: "voice", target: "music", by: -9.0, attack: 0.2, release: 0.7),
                (trigger: "ui", target: "ambience", by: -3.0)],
      )"#,
    );
    assert_eq!(mixer.snapshots.len(), 2);
    let pause = &mixer.snapshots[0];
    assert_eq!((pause.name.as_str(), pause.blend), ("pause", 0.5));
    assert_eq!(pause.buses[0].0, "music");
    assert_eq!(pause.buses[0].1.volume_db, -6.0);
    assert_eq!(pause.buses[0].1.low_pass, Some(800.0));
    assert_eq!(pause.buses[1].1, Default::default());
    assert_eq!(mixer.snapshots[1].blend, 0.3, "the default blend");
    assert_eq!(mixer.ducks[0].by_db, -9.0);
    assert_eq!((mixer.ducks[0].attack, mixer.ducks[0].release), (0.2, 0.7));
    assert_eq!((mixer.ducks[1].attack, mixer.ducks[1].release), (0.1, 0.6));
    assert!(failure(r#"(snapshots: { "a": (), "a": () })"#).contains("twice"));
  }

  #[test]
  fn every_field_reads_as_written() {
    let mixer = loaded(
      r#"(
        buses: {
          "music": (volume: -3.0, pauses_with_game: true, follows_time_scale: true),
          "sfx": (limit: 48),
          "sfx/footsteps": (
            volume: -6.0,
            effects: [
              LowPass(cutoff: 8000.0, resonance: 0.2),
              HighPass(cutoff: 120.0),
              Eq(kind: HighShelf, frequency: 6000.0, gain_db: -4.0, q: 0.7),
              Delay(seconds: 0.25),
              Compressor(threshold_db: -18.0, ratio: 3.0, attack: 0.005),
              Distortion(kind: SoftClip, drive_db: 6.0),
            ],
            sends: { "sfx/reverb": -12.0, "sfx/echo": -20.0 },
            limit: 8,
          ),
          "sfx/reverb": (volume: -2.0, effects: [Reverb()]),
          "sfx/echo": (),
        },
        on_focus_loss: Duck(-12.0),
      )"#,
    );
    let music = bus(&mixer, "music");
    assert_eq!(music.volume_db, -3.0);
    assert_eq!(music.pauses_with_game, Some(true));
    assert_eq!(music.follows_time_scale, Some(true));
    assert_eq!(bus(&mixer, "sfx").limit, Some(48));
    let footsteps = bus(&mixer, "sfx/footsteps");
    assert_eq!(footsteps.volume_db, -6.0);
    assert_eq!(footsteps.limit, Some(8));
    assert_eq!(footsteps.pauses_with_game, None);
    assert_eq!(
      footsteps.sends,
      [
        ("sfx/reverb".to_string(), -12.0),
        ("sfx/echo".to_string(), -20.0)
      ],
      "in the file's order"
    );
    assert_eq!(
      footsteps.effects,
      [
        EffectDef::LowPass {
          cutoff: 8000.0,
          resonance: 0.2
        },
        EffectDef::HighPass {
          cutoff: 120.0,
          resonance: 0.0
        },
        EffectDef::Eq {
          kind: EqKind::HighShelf,
          frequency: 6000.0,
          gain_db: -4.0,
          q: 0.7
        },
        EffectDef::Delay {
          seconds: 0.25,
          feedback_db: -6.0,
          mix: 0.5
        },
        EffectDef::Compressor {
          threshold_db: -18.0,
          ratio: 3.0,
          attack: 0.005,
          release: 0.1,
          makeup_db: 0.0,
          mix: 1.0
        },
        EffectDef::Distortion {
          kind: DistortionKind::SoftClip,
          drive_db: 6.0,
          mix: 1.0
        },
      ]
    );
    assert_eq!(
      bus(&mixer, "sfx/reverb").effects,
      [EffectDef::Reverb {
        feedback: 0.9,
        damping: 0.1,
        stereo_width: 1.0,
        mix: 0.5
      }],
      "kira's defaults"
    );
    assert_eq!(mixer.on_focus_loss, FocusLoss::Duck(-12.0));
    assert_eq!(
      loaded("(on_focus_loss: Mute)").on_focus_loss,
      FocusLoss::Mute
    );
  }

  #[test]
  fn mistakes_fail_the_load_naming_the_file_and_what_is_wrong() {
    let cases = [
      (r#"(buses: { "sfx": (volum: -3.0) })"#, "volum"),
      (r#"(buses: { "sfx": (), "sfx": () })"#, "\"sfx\" twice"),
      (r#"(buses: { "footsteps": () })"#, "\"footsteps\""),
      (r#"(buses: { "sfx//steps": () })"#, "\"sfx//steps\""),
      (r#"(buses: { "sfx/a/b": () })"#, "its parent \"sfx/a\""),
      (r#"(buses: { "sfx": (volume: 30.0) })"#, "volume"),
      (r#"(buses: { "sfx": (limit: 0) })"#, "limit"),
      (
        r#"(buses: { "sfx": (sends: { "music": -6.0 }) })"#,
        "\"music\"",
      ),
      (
        r#"(buses: { "sfx": (sends: { "sfx/nowhere": -6.0 }) })"#,
        "\"sfx/nowhere\"",
      ),
      (
        r#"(buses: { "sfx/a": (sends: { "sfx/a": -6.0 }) })"#,
        "itself",
      ),
      (
        r#"(buses: { "sfx/a": (sends: { "sfx/verb": -6.0 }), "sfx/verb": (limit: 4) })"#,
        "send bus",
      ),
      (
        r#"(buses: { "sfx/a": (sends: { "sfx/verb": -6.0 }), "sfx/verb": (), "sfx/verb/x": () })"#,
        "send bus",
      ),
      (
        r#"(buses: { "sfx/a": (sends: { "sfx/verb": -6.0 }), "sfx/verb": (sends: { "sfx/b": -3.0 }), "sfx/b": () })"#,
        "send bus",
      ),
      (
        r#"(buses: { "sfx": (effects: [LowPass(cutoff: 5.0)]) })"#,
        "cutoff",
      ),
      (
        r#"(buses: { "sfx": (effects: [LowPass(cutoff: 500.0, resonance: 2.0)]) })"#,
        "resonance",
      ),
      (
        r#"(buses: { "sfx": (effects: [Reverb(feedback: 1.5)]) })"#,
        "feedback",
      ),
      (
        r#"(buses: { "sfx": (effects: [Delay(seconds: 60.0)]) })"#,
        "seconds",
      ),
      (
        r#"(buses: { "sfx": (effects: [Delay(feedback_db: 0.0)]) })"#,
        "feedback_db",
      ),
      (
        r#"(buses: { "sfx": (effects: [Compressor(ratio: 0.5)]) })"#,
        "ratio",
      ),
      (
        r#"(buses: { "sfx": (effects: [Eq(kind: Bell, frequency: 1000.0, gain_db: 3.0, q: 0.0)]) })"#,
        "q",
      ),
      (
        r#"(buses: { "sfx": (effects: [Distortion(mix: 2.0)]) })"#,
        "mix",
      ),
      (
        r#"(buses: { "master": (sends: { "sfx/verb": -6.0 }), "sfx/verb": () })"#,
        "master sends nowhere",
      ),
      (r#"(on_focus_loss: Duck(6.0))"#, "on_focus_loss"),
      (r#"(buses: { "sfx": (volume: NaN) })"#, "volume"),
      (
        r#"(buses: { "sfx": (effects: [LowPass(cutof: 500.0)]) })"#,
        "cutof",
      ),
      (r#"(buses: { "sfx": (effects: [Chorus()]) })"#, "Chorus"),
    ];
    for (text, names) in cases {
      let error = failure(text);
      assert!(error.contains(PATH), "{text}: {error}");
      assert!(error.contains(names), "{text}: {error} (wanted {names:?})");
    }
  }

  #[test]
  fn a_mixer_made_in_code_is_checked_the_same_way() {
    let mut mixer = Mixer::default();
    assert_eq!(mixer.check(), Ok(()));
    mixer.buses.push(BusDef::new("sfx/steps"));
    assert_eq!(mixer.check(), Ok(()));
    mixer.buses.push(BusDef::new("sfx/steps"));
    assert!(mixer.check().is_err_and(|error| error.contains("twice")));
  }
}
