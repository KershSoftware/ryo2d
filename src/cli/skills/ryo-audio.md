---
name: ryo-audio
description: "Audio - the workflow from a DAW export through assets/ and a .sound.ron definition to ctx.audio().play, voices and voice limits, the mixer file (buses, effects, sends, snapshots, ducking), the player's volume settings and their curve, 2D spatial sound with listeners and emitters, pause and time scale, music with intros, loops and crossfades, hot reload, the web's first-gesture unlock, latency, and testing with audio_world and the audio log."
metadata:
  version: 2.5.1
---

# Ryo2D Audio

Sound is authored outside the engine, in a DAW (Logic, Ableton, Reaper), and handed over as files. The audio designer describes how each sound plays in a small RON file, balances the mix in one mixer file, and hears every save while the game runs; the game developer plays a sound with one call, and the engine handles its voice, its bus and its cleanup as scenes change. Underneath is a mixer on kira 0.12.

Four registry assets do the work, each loaded by asset path like an image (`ryo-assets`):

| Asset | File | What it is |
|---|---|---|
| `AudioClip` | `.wav`, `.ogg`, `.mp3`, `.flac` | One recording |
| `Sound` | `.sound.ron` | How a sound plays: its clips and variations, bus, level, pitch, limits, fades, spatial settings |
| `Music` | `.music.ron` | A track: an intro into a gapless loop, or one clip with a loop region |
| `Mixer` | `audio/mixer.mixer.ron` | The bus tree, its effects and sends, snapshots, ducking and focus loss |

Every type here is in the prelude unless a path is given; complete signatures are in `ryo-api` (Audio). The worked example is the audio demo, `examples/demos/audio` (`./dev.sh audio`), in eight pages (the buses and volume settings, variations, voice limits, spatial sound, the mix, music, hot reload, virtual voices); its `assets/` folder is a small complete project.

## 1. From the DAW to a play

1. **Export** mono or stereo WAV, OGG Vorbis, MP3 or FLAC, at any sample rate (kira resamples to the device as it plays, so nothing is resampled at load). Surround files are refused (`AssetError::Decode`), as kira plays stereo frames. Export at the level you mean: 0 dB in a definition plays the file as recorded.
   - **Effects and UI sounds:** WAV (or FLAC). A clip of up to 10 s decodes to memory when it loads (on a worker thread, or the calling thread under `load_now`), so a play costs nothing to decode.
   - **Music and long ambience:** OGG Vorbis. A clip over 10 s keeps its compressed bytes and streams as it plays, so a five-minute track does not sit decoded in memory (about 23 MB a minute at 48 kHz stereo).
   - **Loops:** cut the loop to the sample in the DAW, and export it as WAV, OGG or FLAC. An MP3 without a Xing, Info or VBRI header has only an estimated length, so a streamed MP3 loop can wrap after a short gap or cut its end; a FLAC without its total sample count cannot stream, so load it with `ClipMode::Decode`.
   - **Edges:** start and end each one-shot at a zero crossing, or give its definition a short `fade_in`. The engine smooths every stop, pause and change it makes over at least 10 ms, but a start plays the file's first sample at once unless the definition has a `fade_in`, so a file (or a `region`) that starts away from a zero crossing clicks, as does a click inside the file.
2. **Put the files under `assets/`**, for example `assets/sfx/step-1.wav`. Asset paths are relative to `assets/` (`"sfx/step-1.wav"`), inside RON files too.
3. **Describe the sound** in a definition next to them (section 2):

   ```ron
   // assets/sfx/step.sound.ron: four takes, never the same twice in a row
   (
     clips: [
       (path: "sfx/step-1.wav"),
       (path: "sfx/step-2.wav"),
       (path: "sfx/step-3.wav"),
       (path: "sfx/step-4.wav", weight: 2.0),
     ],
     volume: -4.0,
     volume_range: 2.0,
     pitch_range: 1.0,
     no_repeat: 1,
     limit: (max: 4, rule: StopOldest),
   )
   ```

4. **Play it** from the game: load the handle once, keep it, and play it as often as needed.

   ```rust
   use ryo2d::prelude::*;

   #[derive(Default)]
   struct Level {
     step: Option<Handle<Sound>>,
     wind: Voice,
   }

   impl Scene for Level {
     fn on_enter(&mut self, ctx: &mut SceneContext) {
       // At once, its clips too, so the first step is heard
       self.step = ctx.assets().load_now::<Sound>("sfx/step.sound.ron").ok();
       let wind = ctx.assets().load_now::<Sound>("ambience/wind.sound.ron");
       if let Ok(wind) = wind {
         // A looping definition: it plays until stopped, or until this scene exits
         self.wind = ctx.audio().play(&wind);
       }
     }

     fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
       if ctx.input.key_pressed(KeyCode::Space)
         && let Some(step) = &self.step
       {
         ctx.audio().play(step);
       }
       SceneAction::Continue
     }
   }
   ```

- `ctx.audio()` is an `AudioView` that borrows the context: take it, use it, let it go.
- A `Handle<AudioClip>` plays directly too (`ctx.audio().play(&clip)`), with the empty definition: the `sfx` bus, 0 dB, no variation and no limit.
- **A loop played while its sound loads waits for it**: its voice reports playing, takes what the game does to it, and starts with its `fade_in` once the sound is ready, so `load` then `play` of an ambience bed in `on_enter` just works. Its scene's exit ends it as it does any loop, and a sound that fails to load ends it (the registry's one warning says why).
- **A one-shot played while its sound loads is refused**, as it would only be late: logged, and warned about once in a debug build. A definition still loading is not known to loop until it lands, so any play of one waits until then, and a one-shot is refused at that point. Load what a scene plays at once with `load_now` in `on_enter`, or `load` behind a loading screen (`ryo-assets`). A sound that failed to load plays nothing, warned once with its path.
- A play that waited counts against its sound's `per_frame` (section 4) as it lands, so the same loop played twice while it loads starts once.
- A UI theme's sound clicked before it has loaded is refused the same way, never waited for, and not warned about: the theme loads its sounds as it loads, and a game has no way to load them sooner.
- A voice holds its clip while it plays, so a scene may drop its handles while the last sounds play out.

## 2. Sound definitions (`.sound.ron`)

Every field but `clips` may be left out; an option is written without `Some(..)`, and an unknown field fails the load.

| Field | Default | Meaning |
|---|---|---|
| `clips` | (required) | `(path: "..", weight: 1.0, region: (start: 0.0, end: 0.4), loop_region: (start: .., end: ..))`; one is picked per play by weight (2.0 twice as often as 1.0, 0 never). `region` is the seconds of the file that play; `loop_region`, for a looping sound, the seconds from the region's start that repeat, what comes before playing once as an intro |
| `bus` | `"sfx"` | `"master"`, `"music"`, `"sfx"`, `"ui"`, `"voice"`, `"ambience"`, or a mixer bus under one (`"sfx/footsteps"`) |
| `volume`, `volume_range` | 0.0, 0.0 | Gain in dB (-6 is about half the amplitude), and each play's spread either side |
| `pitch`, `pitch_range` | 0.0, 0.0 | Semitones (12 is an octave up and twice as fast, as on a turntable), and each play's spread either side |
| `no_repeat` | 0 | How many of the latest picks a play avoids (1: never the same clip twice in a row); always at most one fewer than the clips |
| `limit` | none | `(max: 4, rule: StopOldest)`: how many of its voices play at once and what a play past that does (section 4) |
| `priority` | 128 | 0 to 255, **higher is more important** (section 4) |
| `cooldown` | 0.0 | Seconds: a play sooner than this after the last is refused |
| `per_frame` | 1 | The most of its voices that start in one frame, wherever they play (section 4). 0 fails the load |
| `looping` | false | Loops until stopped, over each clip's `loop_region` or its whole region |
| `fade_in`, `fade_out` | 0.0, 0.0 | Seconds. `fade_out` is used whenever the engine stops it (a limit, a scene exit, an emitter going), at least 10 ms |
| `spatial` | none | `(min_distance: 50.0, max_distance: 800.0, curve: Inverse, pan_width: 400.0, low_pass_at_max: 2000.0)`: heard from a place (section 7) |
| `mode` | `Auto` | How its clips load: `Auto` decodes up to 10 s and streams longer, `Decode` and `Stream` force one (the web decodes every clip) |

- The clips are the definition's dependencies: it is ready once every clip is, and a clip that fails fails it (`AssetError::Dependency`, naming both). A region past its clip's end, a weight below 0, a loop region on a sound that does not loop, a `limit` or `per_frame` of 0, or a bus name that is not a path under a standard bus fails the load with `AssetError::Decode`, naming the file and the mistake.
- A bus the mixer does not have plays on the nearest bus above it, warned once (`sfx/footstep` typed for `sfx/footsteps` plays on `sfx`).
- A definition can be made in code (`Sound { clips: vec![SoundClip::new(clip)], volume_db: -6.0, ..Default::default() }`) and added with `ctx.assets().add(sound)`.

## 3. Voices

`play`, `play_at`, `play_with` and `play_emitter` return a `Voice`, a handle to that playback:

```rust
let engine = ctx.audio().play_with(&self.engine_loop, PlayOptions { volume_db: -3.0, ..Default::default() });
engine.set_pitch(2.0);                     // semitones above the sound's pitch
engine.set_volume(-12.0);                  // dB below the sound's level
engine.set_volume_over(-24.0, Duration::from_secs(2)); // a slow fade down
engine.pause_over(Duration::from_millis(200)); // fades out, keeps its place
engine.resume();
engine.stop(Duration::from_millis(300));   // fades out, then ends for good
let over = engine.is_stopped();            // ended, or never started
```

- **`set_volume` and `set_pitch` are relative to the sound's level**: a game layer added to its definition's volume, the spread its play drew and its `PlayOptions`, as `PlayOptions` are. `set_volume(0.0)` puts it back at the sound's level, and a reload of its definition still moves the sound's level beneath the game's change, so a designer's balance survives game code.
- **Every change is smoothed by at least 10 ms**, so no change clicks: the plain forms over 10 ms, and `set_volume_over(db, fade)`, `set_pitch_over(semitones, fade)`, `pause_over(fade)` and `resume_over(fade)` over a fade of their own (at least 10 ms), as `stop(fade)` takes one. A change to a playing voice ramps from where it is.
- **Fades to and from silence are shaped** (a `fade_in`, a stop, a pause or resume, a music crossfade): a fade-out eases in and a fade-in eases out, each about -3.75 dB halfway, so two sounds crossfading hold their level as an equal-power crossfade does (a fade in a line of dB would dip about 27 dB in its middle). A 10 ms smoothing stays a line, too short to hear its curve, and a change between two levels (`set_volume`, a bus's gain) moves in a line of dB.
- **When changes apply:** as the scene code that made them ends (a callback, one of its systems, an observer of its UI nodes), and the rest once a frame after the scenes' updates, so `is_playing` reflects them by the next update. A change to a voice a system's `Audio` played, made before that play applies, applies with its start. A plain change made in the frame a voice started, which nothing has heard yet, applies at once, and an `_over` change made then ramps from the level its play started at; still, the level and pitch it starts at belong in its `PlayOptions`, as on a device its first milliseconds may already be playing.
- **Dropping a `Voice` never stops it**; clones control the same playback, and `Voice::default()` is a voice that never started (for a field). A play refused by its `per_frame`, a limit, a cooldown, a one-shot's sound still loading or distance returns a voice that is already stopped, on which every method does nothing: no error to handle. A loop a limit has no room for, or out of range, is not refused but virtual (section 4): it reports playing, and `voice.is_virtual()` says so.
- `PlayOptions { position, persist, volume_db, pitch }` changes one play: a place in the world, ownership by the engine, and gain and pitch added to the definition's.
- `voice.pick()` says what a play picked and drew: `VoicePick { clip_index, clip, volume_db, pitch }` (a subtitle for a line picked at random, say). It is `None` until the play starts: for a system's `Audio` play, until its buffer applies (read it in a later system or the next frame), and for a loop waiting for its sound, until it lands. `ctx.audio().seed(n)` makes the picks and draws repeat from the same seed, for replays and captures; headless runs and test worlds (`audio_world`) start seeded.
- `ctx.audio().voices_on("sfx")` counts the voices playing on a bus and every bus under it, each taking a voice; `virtual_voices_on("sfx")` counts the virtual loops there. Both build a mixer that has landed first, so they take the view mutably (`let mut audio = ctx.audio();` to call them on a binding).

**Who owns a voice decides when it stops on its own:**

| Played | Belongs to | Stops by itself when |
|---|---|---|
| From a scene's callbacks, systems or UI node observers | That scene | The scene exits (popped, switched away, reset): its loops stop with their `fade_out`, as do its one-shots paused or held and its one-shots with more than 2 s left; its one-shots nearly over, and those on `ui`, finish, so a click just before a switch is heard out while a 20 s voice-over does not run on into the menu |
| With `PlayOptions { persist: true, .. }`, or from a global system | The engine | Never: stop it yourself. Rain that carries on into the next scene needs `persist` |
| From an `AudioEmitter` | Its entity | The entity is despawned, disabled (a pool putting it back) or loses its emitter: its loops and its paused or held one-shots stop with their `fade_out`, and its one-shots playing play out whatever their length (a death cry as it despawns) |
| `play_music` | The engine's music player | It is replaced or stopped, or a scene whose `cleanup_on_exit` includes `SceneCleanup::MUSIC` is switched away from or reset (cleanup does not run on `Pop`) |

**From a system**, take `Audio` as a parameter. It has `play`, `play_with`, `play_at` and `play_emitter`, and `snapshot`, `clear_snapshot`, `set_bus_volume`, `set_volume_setting` and `volume_setting`:

```rust
#[derive(Component)]
struct Footsteps {
  sound: Handle<Sound>,
  next: f32,
}

/// Each walker steps every 0.4 s, heard from where it is
fn footsteps(mut audio: Audio, time: Res<Time>, mut walkers: Query<(&mut Footsteps, &Transform)>) {
  for (mut steps, transform) in &mut walkers {
    steps.next -= time.delta();
    if steps.next <= 0.0 {
      steps.next += 0.4;
      audio.play_at(&steps.sound, transform.translation);
    }
  }
}

// In the level's on_enter: ctx.add_systems(Phase::FixedUpdate, footsteps).ok();
```

- **Its plays are deferred.** `Audio` reads the engine without borrowing it mutably, so systems that play sound run beside each other with no conflict. Each play is queued and applied where the system's commands apply (the end of its schedule, or a sync point before a system ordered after it).
- **Each play returns its `Voice` at once.** Until the play applies, the voice reports playing. Changes made to it meanwhile apply after its start, so a plain `set_volume` in the same system is heard from its first audio. A play refused as it applies leaves the voice stopped.
- **`play_emitter` of an entity the system spawns works in either order.** bevy applies a system's `Audio` and `Commands` in the order they are declared, so with `Audio` first the entity does not exist yet as the play applies: such a play waits for the engine's next audio update and plays there. One whose entity is still no emitter then (gone, disabled, no `AudioEmitter`) plays nothing, logged as refused (`emitter`) and warned once in debug builds.
- **Ownership is the scene rule:** a play applied in a scene's section (a system the scene added with `ctx.add_systems`) belongs to that scene; one from a system added to a phase on the `EngineBuilder` is the engine's. An observer with `Audio` applies its plays at the world's next flush, which may be outside the scene that triggered it, so a loop it plays can outlive that scene: play scene-owned loops from the scene's code. `volume_setting` reads back what the system set at once.
- An exclusive system (`&mut World`) plays at once with `AudioView::new(world).play(&handle)`; music, `set_mixer` and `seed` are `AudioView`'s.

## 4. Voice limits and priority

**Priority runs from 0 to 255, and higher is more important.** This is the opposite of Unity's convention, where 0 is the most important. The default is 128; music plays at 255.

| `rule` | A play past `max` stops | A loop it stops or refuses |
|---|---|---|
| `StopOldest` (the default) | the voice that started first | stops for good |
| `StopQuietest` | the quietest as heard (its distance falloff counts), the oldest of equals | goes virtual |
| `StopFarthest` | the farthest from the listener, the oldest of equals; a voice played without a position counts as at the listener | goes virtual |
| `StopLowestPriority` | the least important, the oldest of equals | goes virtual |
| `PreventNew` | nothing: the new play is refused | is refused (its voice reports stopped) |

`StopOldest` and `PreventNew` end loops as they end one-shots, for the idioms that need it: one bed at a time on a bus (`StopOldest`: the newest replaces the last, which does not come back), one alarm at a time (`PreventNew`: a second play is refused, so the voice a game keeps is the one heard). The rules that rank keep a loop they stop or refuse as a virtual voice, to come back when it ranks again (below).

- **Three layers of limits** apply to each play, in order: its definition's `limit` (its voices on its bus and the buses under it), each bus `limit` in the mixer from its bus up to `master` (every voice on that bus's subtree), and the engine's cap of 96 voices on a bus's own track. A bus limit uses the playing sound's rule (the cap's order when the sound has no limit); the cap stops the least important voice, then the quietest, then the oldest.
- **A sound starts at most `per_frame` voices a frame** (1 by default), counted across every bus and place it plays at, before any limit. Copies started together play in step and add up (ten are 20 dB over one), so a hundred identical plays in one frame start one voice and the rest are refused, logged `Refused` with the reason `per_frame`. A bare clip counts as its own sound. Raise `per_frame` for a sound meant to layer in one frame (a shotgun's pellets). Plays that waited for their sound to load count as they land. Emitters of one sound spawned together take turns instead, starting as many a frame as it allows.
- **A voice is never stopped for a less important play, nor for an equally important one in the frame it started**, so many starts in one frame fill a limit and refuse the rest, instead of stopping their own starts in turn. With nothing it may stop, the play is refused. Only music stops music. Under a rule that ranks (and the cap), a loop stopped or refused this way goes virtual instead (below).
- A voice a rule stops fades over its own `fade_out` (at least 10 ms); one the cap stops fades over 10 ms.
- **A stopped voice holds no room** under its sound's `limit`, the bus limits or its `per_frame`, from the `stop` call on, so a `PreventNew` sound stopped and played again in the same frame starts the new voice. It still holds its place on kira's track (128 sounds a track, which the cap of 96 leaves room in) until its fade ends. Many long `fade_out`s on one busy bus can fill it; plays there are then refused, with one warning naming the bus. Keep the `fade_out`s of sounds stopped by limits short.
- `cooldown` refuses a repeat sooner than its time, before any limit: a coin pickup triggered by two colliders in one step plays once.

**Virtual voices (loops only).** A loop that has no room, or is out of range, does not stop: it goes virtual, keeping its place silently with no voice, and comes back when it can be heard.

- **What goes virtual:** a loop (played plainly, at a place, or from an emitter; decoded or streamed) that its bus's cap of 96, or a limit whose rule ranks (`StopQuietest`, `StopFarthest`, `StopLowestPriority`, or a bus `limit` on a sound with no limit of its own, which takes the cap's order), would stop or refuse; that finds kira's room on its track held by voices fading out; or that is at or beyond its `max_distance`, where it takes no voice at all: a field of a hundred crickets costs the few near the listener. `PreventNew` and `StopOldest` end loops (above). One-shots are stopped, refused or culled as before, and music is never virtual. `per_frame` and `cooldown` still refuse a loop.
- **While virtual** its `Voice` reports playing (`voice.is_virtual()` is true) and takes every change: `set_volume`, `set_pitch`, `pause` and `resume` are kept for when it is heard, and `stop` ends it without its coming back. It moves on through its clip as if heard, at its pitch and its bus's time scale, so it comes back where it would be. The game's pause, its bus's hold and its scene's hold freeze it, as they freeze a heard voice, and its owner's rules apply: its scene's exit and its emitter's despawn end it, `persist` keeps it. It counts against no limit or cap.
- **Coming back:** each frame with a virtual voice, the engine weighs each virtual loop against the voices heard under each limit it is under (its sound's `limit`, the bus limits above it, its bus's cap): by priority, then by gain as heard (its distance falloff included). It comes back when there is room, or in place of the weakest voice heard that it outranks (more important, or 3 dB louder at equal priority; never under `PreventNew` or `StopOldest`, which take no voice's place for it, and never in place of music), starting where it has got to and fading in over 50 ms (a loop never heard, virtual from its play, over its own `fade_in` if longer; a streamed one once its decoder has its first audio, so the fade is heard). The voice it takes the place of goes virtual if it loops (fading out over the same 50 ms) and stops with its `fade_out` if not. A loop out of range comes back once the listener is inside 95% of its `max_distance`.
- **No flapping:** besides the 3 dB, a voice that started, went virtual or came back stays so for at least 250 ms, and the 95% keeps a loop at the edge of its range from coming and going. A loop never heard is exempt: one played as a level enters, judged out of range from a camera that has not moved there yet, or kept out by a limit for a frame, is heard as soon as it may be, with its own swell.
- **At most 1024 are virtual**; past them a loop that would go virtual is refused (logged with the reason `virtual`), and one a play takes the place of stops.
- Beds and emitter loops still give way to more important plays: give them a higher priority than one-shots to keep them heard. A play's limits look at every voice, virtual ones too, so plays cost a little more with hundreds virtual.
- **See it:** the audio demo's Virtual voices page has thirty birds singing in a field under a limit of six, and draws each by its `Voice` (heard, virtual waiting for room, virtual out of range) as the listener wanders among them.

## 5. The mixer (`audio/mixer.mixer.ron`)

The standard buses always exist, so a game without a mixer file plays on them:

```
master
├─ music      (does not pause with the game)
├─ sfx        (pauses with the game, follows time scale)
├─ ui         (does not pause)
├─ voice      (pauses with the game)
└─ ambience   (pauses with the game, follows time scale)
```

The engine loads `audio/mixer.mixer.ron` (`MIXER_PATH`) at start if the game has one; it adjusts the standard buses and adds the game's own under them. Every field may be left out:

```ron
(
  buses: {
    "master": (effects: [Compressor(threshold_db: -6.0, ratio: 4.0)]),
    "sfx": (volume: -3.0, limit: 48),
    "sfx/footsteps": (volume: -6.0, effects: [HighPass(cutoff: 120.0)], sends: { "sfx/reverb": -12.0 }, limit: 8),
    "sfx/reverb": (effects: [Reverb(feedback: 0.85, mix: 1.0)]),
    "voice/npc": (sends: { "sfx/reverb": -18.0 }),
  },
  snapshots: {
    "paused": (blend: 0.5, buses: {
      "music": (volume: -6.0, low_pass: 700.0),
      "sfx": (low_pass: 900.0),
    }),
  },
  ducks: [(trigger: "voice", target: "music", by: -9.0, attack: 0.1, release: 0.6)],
  on_focus_loss: Duck(-12.0),
)
```

- **A bus** is named by its path (`"sfx/footsteps"`; its parent must exist, so a bus under a game's bus needs that bus in the file too, in any order). Each has `volume` (dB, at most +24), `effects` in order, `sends`, `pauses_with_game` and `follows_time_scale` (the parent's when left out) and `limit`. A mixer adds at most 64 buses, 16 of them send buses.
- **Effects:** `LowPass(cutoff, resonance)`, `HighPass(cutoff, resonance)`, `Eq(kind: Bell | LowShelf | HighShelf, frequency, gain_db, q)`, `Reverb(feedback, damping, stereo_width, mix)`, `Delay(seconds, feedback_db, mix)`, `Compressor(threshold_db, ratio, attack, release, makeup_db, mix)`, `Distortion(kind: HardClip | SoftClip, drive_db, mix)`. Each parameter's range and default are in `EffectDef`'s rustdoc; one out of range fails the load naming it.
- **Sends** feed send buses (a bus another sends to) at a level in dB, after the sender's own volume. A send carries the gains of the buses above its sender, so a return follows its sender's sliders: `voice/npc`'s reverb goes quiet with the voice slider, not the sfx one. A send bus plays no voices of its own (a sound on it plays on its parent, warned), has no buses under it and sends nowhere. kira mixes send buses beside `master`'s track rather than through it, so `master`'s effects (a compressor or limiter) do not process returns; the returns are still under `master`'s volume.
- **There is no limiter on the output** unless the mixer adds one: voices that sum past full scale clip at the device. Leave headroom in the mix, or put a `Compressor` on `master` (and on loud send buses).
- **A game sets a bus's volume** with `ctx.audio().set_bus_volume("sfx/footsteps", -9.0)`, in place of the mixer's for the session (through reloads); the player's setting, a snapshot and ducking still apply over it. Bus volume is for the game's own mix decisions; the player's sliders are the volume settings (section 6).
- **Gain layers add in dB:** the mixer's (or the game's) volume, the player's setting, the active snapshot and the duck (and the focus loss, on `master`). A snapshot never overrides a slider, and a slider moved under a snapshot is heard at once.
- **`on_focus_loss`** is `Keep` (the default), `Duck(dB)` or `Mute`: what `master` does while the window is not focused, smoothed over 10 ms.
- **A mixer made in code**, for a game with no file: `ctx.audio().set_mixer(Mixer { buses: vec![BusDef { volume_db: -3.0, ..BusDef::new("sfx") }], ..Default::default() })`. Its snapshots and ducks are `SnapshotDef`, `SnapshotBus` and `DuckDef`.
- The engine loads the mixer on a worker, so its buses exist a few frames after start. A scene that plays on a game bus, or calls a snapshot, from its first frame loads it at once with `ctx.assets().load_now::<Mixer>(MIXER_PATH)` (the demo's pages do). A mixer that has landed, or one set with `set_mixer`, is built before the next play, snapshot, `set_bus_volume` or `voices_on`, so the scene's first sounds play on their own buses. A bus built while the game is paused holds its voices at once.

## 6. The player's volume settings

Six settings, one per standard bus, are what a settings menu shows: `VolumeKind::Master`, `Music`, `Sfx`, `Ui`, `Voice` (dialogue) and `Ambience`. A game's own bus takes the setting of the standard bus it is under.

```rust
ctx.audio().set_volume_setting(VolumeKind::Music, 0.5); // playing sounds follow at once
let music = ctx.audio().volume_setting(VolumeKind::Music);
```

**The curve.** A setting runs from 0 to 1 and is the square root of the amplitude, so its gain is dB = 40·log10(v) (`volume_to_db`):

| Setting | 1.0 | 0.89 | 0.71 | 0.5 | 0.25 | 0.1 | 0 |
|---|---|---|---|---|---|---|---|
| Gain | 0 dB | -2 dB | -6 dB | -12 dB | -24 dB | -40 dB | silent |

A linear slider leaves 50% at only -6 dB and puts everything below -12 dB in its bottom quarter; on this curve 50% is about -12 dB, near half as loud (loudness halves at about -10 dB), and each halving of the setting is the same 12 dB step. 0 is exact silence; a value outside 0 to 1 is clamped, and one that is not a number is the kind's default.

- **Defaults** come from `Ryo.toml` (`[system] master_volume = 1.0`, `music_volume`, `sfx_volume`, `ui_volume`, `voice_volume`, `ambience_volume`); without them every setting is 1, the mix as designed, and `ryo2d init` writes 1.0. A game that wants headroom on master sets less there (0.89 is about -2 dB).
- **Saving:** `SystemState` keeps the six as `master_volume`, `music_volume`, `sfx_volume`, `ui_volume`, `voice_volume` and `ambience_volume` (`ryo-config-save`). With `EngineBuilder::with_user_preferences` allowing them (`UserPreference::MasterVolume` .. `AmbienceVolume`), the engine applies the saved values at start. A saved value is a point on this curve. A `system.json` from before its `format_version` (an engine whose volumes were amplitudes, 20·log10(v) dB) has each volume converted once as it loads to √v, which is heard at the same dB, and is saved with the version, so a player's settings keep their loudness. A setting the file lacks is the game's `[system]` default.

A settings screen:

```rust
struct AudioSettings {
  saves: SaveManager,
  state: SystemState,
}

impl Scene for AudioSettings {
  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
    let state = &mut self.state;
    let mut moved = Vec::new();
    ui.panel().focus_scope().anchor(Anchor::Center).width(320.0).gap(8.0).show(|ui| {
      for (kind, label, value) in [
        (VolumeKind::Master, "Master", &mut state.master_volume),
        (VolumeKind::Music, "Music", &mut state.music_volume),
        (VolumeKind::Sfx, "Effects", &mut state.sfx_volume),
        (VolumeKind::Voice, "Dialogue", &mut state.voice_volume),
      ] {
        if ui.slider(value, 0.0..=1.0).id(label).step(0.05).label(label).show_value().show().changed() {
          moved.push((kind, *value));
        }
      }
    });
    for &(kind, value) in &moved {
      ctx.audio().set_volume_setting(kind, value);
    }
    if !moved.is_empty() {
      self.saves.save_system(&self.state).ok();
    }
    if ui.back() {
      return SceneAction::Pop;
    }
    SceneAction::Continue
  }
}
```

## 7. Spatial sound

A definition with `spatial` is heard from a place: quieter with distance from the listener, panned by its horizontal offset, and muffled with distance if it has a low-pass.

| `spatial` field | Default | Meaning |
|---|---|---|
| `min_distance` | (required) | Full volume within it |
| `max_distance` | (required) | Silent at it and beyond |
| `curve` | `Inverse` | `Linear`: amplitude falls in a line (-6 dB halfway). `Inverse`: fast near the source and slow far off, as sound does in the open (about -25 dB halfway). `Log`: even in dB (-30 dB halfway) |
| `pan_width` | `max_distance` | The horizontal offset at which it is fully to one side |
| `low_pass_at_max` | none | A cutoff in Hz the filter closes to at `max_distance`, from open (20 kHz) within `min_distance`, in even octave steps between |

- **The listener** is the entity with an `AudioListener` (spawn it on the player when the camera leads or lags), else the main camera. Its rotation turns which way is right.
- `ctx.audio().play_at(&boom, position)` plays at a fixed place. **A one-shot at or beyond its maximum distance is culled**: it never starts, so a distant explosion costs no voice. **A loop there is virtual** (section 4): it takes no voice, moves on through its clip, and comes back fading in over 50 ms once the listener is inside 95% of that distance.
- **An emitter** follows an entity: `ctx.world.spawn((AudioEmitter { sound: roar.clone(), play_on_spawn: true }, Transform::from_xy(640.0, 200.0)))`. With `play_on_spawn` it plays as it is added (once its sound has loaded) and again each time its disabled entity is enabled; `ctx.audio().play_emitter(entity)` plays it at any time (a loop whose sound is still loading waits for it as `play`'s does, then starts where the entity is). Its voices follow the entity's position each frame and stop by the scene rule as it goes. A pooled entity works: a pool putting it back stops its loops, and handing it out (a `RecycleOldest` pool's recycle included) plays it again.
- At most 32 voices carry a distance low-pass at once (each needs a kira track of its own); a voice past them plays unfiltered, its gain and pan still following.
- A sound without `spatial` played at a place is heard as if at the listener, warned once.
- Positions are read after the frame's fixed steps and cameras, without interpolation. `falloff_gain_db(&spatial, distance)` and `pan_for(&spatial, dx)` give what the listener hears, for a meter or a test.
- **Hear it on headphones or speakers:** the audio demo's Spatial page solos each circling emitter (1, 2, 3; 0 for all three) and marks the two points, 150 px (its `pan_width`) either side of the listener, where a clicked ping is heard fully on that side. A sound panned fully to one side is silent on the other and 3 dB louder on its own (kira's constant-power pan); headphones make the balance easiest to judge.

## 8. Pause, time scale and focus

- **The game clock drives audio.** While `ctx.time.game` is paused, every bus that pauses with the game (`sfx`, `voice`, `ambience`) holds its voices where they are (a 10 ms fade) and resumes them from there; `ui` and `music` carry on, so menus click and the music plays under a pause. A bus in the mixer decides with `pauses_with_game`.
- **A scene that is not updated is not heard.** A pushed scene that does not update the scenes below (`updates_below` false, the default; the engine's pause idiom, which pauses their physics too) holds their voices on the buses that pause with the game, and the voices of the emitters they added (the world's entities stop with them, as physics does), with the same 10 ms fade. An emitter belongs to the scene whose code or systems added it, so a pause menu's own emitter plays on above the level it holds; one added outside every scene, or whose scene has gone while its entity lives on, is held while any scene is. A `Pop` lets them go, and they resume where they were. The pushed scene's own voices, the music, voices played with `persist`, and voices on `ui` (or any bus that does not pause with the game) play on. A loop that finishes loading while its scene is held starts held.
- **The holds add up**: a voice held by its scene and by the game's pause resumes only once both let it go, so a pause menu that also pauses the clock (to stop the sprites' animation) works the same.
- **`Scene::holds_audio_below`** (true by default) is the top scene's choice: a dialogue box over a level whose ambience should carry on returns false. Only the top scene's answer counts, and a scene that `updates_below` leaves the scenes below audible anyway.

  ```rust
  struct PauseMenu;

  impl Scene for PauseMenu {
    fn is_transparent(&self) -> bool {
      true // the level shows under the menu; it is not updated, so not heard
    }

    fn on_enter(&mut self, ctx: &mut SceneContext) {
      ctx.audio().snapshot("paused"); // the mixer's muffled preset for the music (section 9)
    }

    fn on_exit(&mut self, ctx: &mut SceneContext) {
      ctx.audio().clear_snapshot();
    }

    fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
      if ui.back() {
        return SceneAction::Pop; // the level's sounds resume where they were
      }
      SceneAction::Continue
    }
  }
  ```

- **A voice played while held** waits, unheard, and fades in over its own `fade_in` when it is let go. The engine's holds leave a voice's own `pause()` alone: a held voice still reports playing, and a voice the game paused stays paused when the hold ends.
- **Time scale:** on a bus that follows it (`sfx`, `ambience`), a voice plays at its pitch's rate times `time.game.speed()`, so slow motion lowers pitch and tempo together, as tape does. Audio follows at most 8 times speed (a faster game clock would swamp the audio thread). A speed of 0.01 or less holds those buses, as a pause does.
- **Hitstop does not hold audio.** It is a visual freeze, and the hit's sound playing through it is what sells the impact.
- **Focus:** the mixer's `on_focus_loss` (section 5).

## 9. Snapshots and ducking

- **A snapshot** is a named mixer preset of bus volumes (dB, a layer over the mixer's and the player's) and low-pass cutoffs (Hz). `ctx.audio().snapshot("paused")` blends to it over its `blend` (0.3 s by default), from wherever the mix is; `clear_snapshot()` blends back. One is active at a time, and calling another blends from the current mix to it.
- A snapshot is a full preset: a bus it does not name, or names without a `low_pass`, blends back to its own level and open. Cutoffs sweep evenly in octaves.
- A name the mixer does not have warns once and changes nothing. A snapshot called before the mixer has loaded applies once it does (the last call wins); if the mixer fails to load, one warning names the snapshot that waited.
- The mix moves once a frame, each step gliding over the frame, so a long frame (a hitch) glides over at most 100 ms and the mix does not trail it; a blend lands on the frame its time runs out. A mixer reload that drops the active snapshot blends back to open over its whole blend.
- **A duck** dips one bus while another plays: `(trigger: "voice", target: "music", by: -9.0, attack: 0.1, release: 0.6)` turns `music` down 9 dB over 0.1 s while any voice plays on `voice` or a bus under it, and back over 0.6 s after. It is bus gain moved each frame, not an audio-rate side-chain, so it suits dialogue over music rather than pumping a kick. Two ducks on one target add their dips.

## 10. Music

```ron
// assets/music/level.music.ron: a bar of intro, then a loop with no gap
(
  intro: "music/level-intro.wav",
  loop: "music/level-loop.wav",
  volume: -6.0,       // dB
  fade_in: 0.5,       // seconds, when it starts over silence (0 by default)
  crossfade: 2.0,     // seconds, when it takes over from a track (1 by default)
  bus: "music",       // or a bus under it, such as "music/combat"
)
```

or one file looping over a region, which streams when long:

```ron
(clip: "music/boss.ogg", loop_start: 12.5, loop_end: None, mode: Stream)
```

```rust
fn on_enter(&mut self, ctx: &mut SceneContext) {
  self.theme = ctx.assets().load("music/level.music.ron");
  // Crossfades from the track playing; changes nothing if this one already plays
  ctx.audio().play_music(&self.theme);
}
```

- **The engine has one music player**, and the music belongs to it, not to a scene: it carries on across scene changes until replaced or stopped, so every scene can ask for its track as it enters without restarting it. `SceneCleanup::MUSIC` (not in `SceneCleanup::DEFAULT`) stops it, over its crossfade, when a scene whose `cleanup_on_exit` includes it is switched away from or reset. Cleanup does not run on `Pop`, so a pushed scene that plays a track of its own (a boss fight over the level) stops it in its `on_exit` with `ctx.audio().stop_music(fade)`, or the scene below asks for its own track again in `on_resume`, which crossfades back to it.
- A track still loading takes over once it has loaded, while the current one plays on; one that fails to load warns once and changes nothing.
- `stop_music(fade)`, `pause_music()`, `resume_music()` and `current_music() -> Option<AssetId<Music>>` (compare with `handle.id()`).
- **An intro and its loop are joined into one sound as they load**, so the loop starts on the sample after the intro's last: kira starts separate sounds only on its 128-frame boundaries, which would leave a gap or overlap. Both are decoded (they cannot stream) and must share a sample rate (a mismatch fails the load naming both files). A track longer than a few minutes is better as one streamed clip with a loop region, as a joined track holds its decoded frames (about 23 MB a minute).
- `tempo` and `meter` (`tempo: 128.0, meter: (4, 4)`) are recorded for interactive music; the player does not read them.
- Only music stops music: no sound takes its room, whatever its priority.

## 11. Hot reload

In debug builds with a window, saving a file in `assets/` changes what is playing, about 100 ms after the last write:

| Saved | What happens |
|---|---|
| A clip | Its loops and the music playing it switch to the new file where they are, crossfading over 10 ms; its one-shots finish as they were; the next plays use the new file |
| A sound definition | The next plays use it; its playing loops take the new volume and pitch at once, each keeping its own spread and its play's options, with the game's `set_volume` and `set_pitch` still on top (a definition at -12 dB given `set_volume(-6.0)` plays at -18 dB, and saved at -10 dB plays at -16). Saved with a new `bus`, its loops move there where they are, crossfading over 10 ms, and its one-shots finish where they are |
| A music definition, its clip, or its intro or loop file | The track switches, where it is, to what the saved definition says, crossfading over 10 ms |
| The mixer | The buses are built again: loops carry on, at their position, on the new buses; one-shots finish on the old ones, whose reverbs and delays then ring out their tails (up to 10 s, from the effect's settings) before they go, so a save while tuning a reverb cuts nothing mid-ring. The old buses keep the volumes they had: a slider (but master) or snapshot moved meanwhile does not reach what finishes or rings there. At most four old trees ring at once; a fifth save lets the oldest go, fading it out over 10 ms with anything still playing on it, as a tail capped at 10 s fades out |

- A switch partway through a `fade_in` or a music crossfade carries on with what is left of it rather than jumping to full level.
- Loops a reload cannot switch (their bus out of room, as when one save switches many loops on one bus) play on as they were, with one warning saying how many.
- A save that does not load (a typo) changes nothing and warns once with the file and, for RON, the line and column; fix it and save again.
- A file that failed (a definition or music that did not load, a streamed clip that failed as it played) plays once it is saved fixed, and is warned about again should it break again.
- The same happens for a change made in code: `ctx.assets().get_mut(&clip)` or `get_mut(&music)` switches what plays, so change them when they should switch, not every frame.
- The audio demo's Hot reload page shows it: save `assets/hot/drone.sound.ron` while it plays.

## 12. The web

- **Audio starts at the player's first click, tap or key press** (not Escape): browsers let a page start sound only inside a gesture they count as the player's. A gamepad never counts, so a player with only a pad hears nothing until they click, tap or press a key.
- Until then one-shots are dropped, and loops and music wait and start at that gesture, each fading in. **A web game opens on a "click or tap to start" screen**, shown while `ctx.audio().waiting_for_gesture()`:

  ```rust
  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
    if ctx.audio().waiting_for_gesture() {
      ui.draw_text("Click or tap to start", 400.0, 300.0, Color::WHITE);
    }
    SceneAction::Continue
  }
  ```

- **The web has no files**, so a web build passes its assets as its source, the mixer at `MIXER_PATH` among them:

  ```rust
  #[cfg(target_arch = "wasm32")]
  fn web_assets() -> MemorySource {
    let files = MemorySource::new();
    files.insert(MIXER_PATH, include_bytes!("../assets/audio/mixer.mixer.ron").as_slice());
    files.insert("sfx/step.sound.ron", include_bytes!("../assets/sfx/step.sound.ron").as_slice());
    files.insert("sfx/step-1.wav", include_bytes!("../assets/sfx/step-1.wav").as_slice());
    files
  }
  // main: builder.asset_source(web_assets())
  ```

- **Every clip decodes on the web**, in the asset pump on the main thread (there are no worker threads or streams), so a long clip is a one-off hitch of its whole decode: a 30 s OGG takes about 15 ms natively, and wasm runs slower, so likely one to three frames there (an estimate). Load music and ambience behind a loading screen or before the scene's first frame, never mid-play, and budget their memory decoded (about 23 MB a minute).
- The audio demo builds for the web: `trunk serve --release --open` in `examples/demos/audio`.

## 13. Latency and the device

- The engine opens the default output device. A play is heard when the device's next buffer renders it, plus the device's own latency: measured on a MacBook Pro at 48 kHz, the default 512-frame buffer is heard 12.9 ms after its callback (12.9 to 23.6 ms from a `play` call), and 256 frames 7.6 ms (7.6 to 12.9 ms).
- `Ryo.toml [system] audio_buffer_frames = 256` fixes the buffer, cut to what the device supports. A smaller buffer cuts latency at the cost of more callbacks and more risk of underruns (crackles) on a busy machine. When the default device changes (headphones plugged in), kira reopens its stream with the same fixed size, and a device that refuses it stops the sound for the session, so leave it unset unless a rhythm game needs the latency. The web picks its own buffer.
- With no device (headless runs, or a device that fails to open, warned once) the same mixer runs silently on kira's mock backend: every call works, nothing plays aloud.

## 14. Testing

- **`ryo2d::testing::HeadlessEngine`** plays on the mock backend, rendered by each step's time, its picks seeded so runs repeat. `engine.audio_log()` (doc-hidden, for tests and debug tooling) lists what the engine did with each voice: `AudioLogEntry { kind, voice, sound, source, bus, gain_db, pan, rate, fade, reason }`, with `kind` one of `AudioLogKind::{Start, Stop, Pause, Resume, Gain, Pan, Rate, Refused, Culled, Stolen, Virtual, Real}` (`use ryo2d::core::audio::{AudioLogEntry, AudioLogKind}`). On a `Refused` entry, `reason` names the rule that refused the play first (`per_frame`, `cooldown`, `limit`, `full`, `undecodable`, `locked`, `unlock`, `loading`, `emitter` or `virtual`), then says what it means; on a `Virtual` entry why the loop went virtual (`range`, `limit`, `full`, `stolen` or `displaced`), and on a `Real` entry why it came back (`room` or `outranks`); it is empty on every other kind. The ranking itself is not logged, only the voices it moves.
- **A test without an engine** takes `ryo2d::testing::audio_world()`: the asset registry, a clock and the audio engine on the mock backend with its log on. `audio_world_over(files)` reads a `MemorySource` the test keeps (to save over a file and `reload` it); `audio_step(&mut world, dt)` is one frame. `audio_world()` starts seeded, as a headless engine does, so a test's picks and draws repeat run to run; `AudioView::new(&mut world).seed(n)` picks another sequence.
- **A clip made in code:** `AudioClip::from_frames(rate, &frames)` returns `Result<AudioClip, AudioClipError>`; a rate of 0 is `AudioClipError::ZeroSampleRate`, as such a clip would never play to its end.

```rust
use std::time::Duration;
use ryo2d::core::audio::{AudioEngine, AudioLogKind};
use ryo2d::prelude::*;
use ryo2d::testing::{audio_step, audio_world};

#[test]
fn a_fourth_step_is_refused_and_the_pause_holds_sfx() {
  let mut world = audio_world();
  let frames = vec![[0.2_f32, 0.2]; 48_000]; // a second at 48 kHz
  let clip = AudioClip::from_frames(48_000, &frames).expect("a sample rate");
  let clip = world.resource_mut::<Assets<AudioClip>>().add(clip);
  let step = world.resource_mut::<Assets<Sound>>().add(Sound {
    clips: vec![SoundClip::new(clip)],
    limit: Some(VoiceLimit { max: 3, rule: LimitRule::PreventNew }),
    per_frame: 4, // four starts in this frame, for the limit to refuse one
    ..Default::default()
  });
  let mut audio = AudioView::new(&mut world);
  let voices: Vec<Voice> = (0..4).map(|_| audio.play(&step)).collect();
  assert!(voices[..3].iter().all(Voice::is_playing));
  assert!(voices[3].is_stopped(), "PreventNew refuses the fourth");

  world.resource_mut::<Time>().game.pause();
  audio_step(&mut world, Duration::from_millis(16));
  let log = world.resource::<AudioEngine>().log(); // doc-hidden, for tests
  assert!(log.iter().any(|e| e.kind == AudioLogKind::Pause && e.voice == 0 && e.bus == "sfx"));
  assert!(!log.iter().any(|e| e.kind == AudioLogKind::Pause && e.bus == "ui"));
}
```

- A bus's own change is logged with `voice` 0; a voice its scene holds logs its own `Pause` and `Resume`. Assert on what a player would notice: a voice refused, a bus held, a gain that moved, a stop with its fade.

## 15. When something sounds wrong

| Symptom | Look at |
|---|---|
| Nothing plays | A one-shot played while its sound loads (refused; use `load_now` or check `is_ready`), a load failure (warned once with its path), a volume setting at 0, a bus held by the game's pause, its scene held under a pushed scene, a one-shot culled beyond its `max_distance`, its `per_frame`, a cooldown or a limit refusing it (`Refused` in the audio log, with the reason), or on the web the first gesture not yet given |
| A streamed file is silent or stops early | A bad streamed file shows up when it plays, not at `play`: its voice fades out (its `fade_out`, at least 10 ms, but ending before the silence the failure leaves reaches playback: about 0.3 s at most at 48 kHz, as the stream is decoded that far ahead) and the failure is warned once, naming the file, and the clip is refused until it is saved again. A packet that will not decode plays as silence |
| A sound plays on the wrong bus | A bus name the mixer lacks plays on the nearest bus above it, warned once; a send bus plays on its parent |
| A burst sounds like one sound | Its `per_frame` (1 by default) starts one copy a frame; raise it for a sound meant to layer |
| A sound cuts out | Its `limit`, a bus `limit`, or the cap of 96 per bus stopping it (a loop goes virtual and comes back when there is room); check `priority` (higher is more important) |
| I stopped a loop and it keeps playing | Another play of it was virtual (a limit whose rule ranks kept it out, or the cap) and came back in its place. Stop every voice you played, or give the sound `PreventNew` (a second play is refused) or `StopOldest` (the newest replaces the last for good) |
| A loop is silent, though `is_playing` | It is virtual (`voice.is_virtual()`; `virtual_voices_on` counts them): a limit or its bus's cap has no room for it, more important or louder voices holding it, or it is at or beyond its `max_distance`. The log's `Virtual` entry says why; it comes back in range, with room |
| A long line stops at a scene change | A one-shot with more than 2 s left stops at its scene's exit (on `ui` it finishes); play it with `persist` to keep it |
| Plays refused on a busy bus with "the bus is full" | Long `fade_out`s holding kira's room on that bus |
| Loops keep playing under the pause menu | The menu `updates_below`, or returns false from `holds_audio_below`; or the loop is the engine's (`persist`), or on a bus that does not pause with the game (section 8) |
| A rain loop stops at a scene change | It belonged to the scene: play it with `persist` |
| Music restarts on a scene change | Something called `stop_music`, or the scene switched away from (or reset) has `SceneCleanup::MUSIC` in its cleanup; `play_music` of the track already playing changes nothing |
| A pushed scene's music plays on after it pops | Cleanup does not run on `Pop`: stop the track in the pushed scene's `on_exit` (`stop_music(fade)`), or play the level's own track again in the level's `on_resume` |
| A loop has a gap or seam | The file's loop points (cut to the sample), an MP3 without a length header, or an intro and loop at different rates (fails at load) |
| Crackle | Clipping past full scale (headroom, or a `Compressor` on `master`), or underruns from too small an `audio_buffer_frames` |
| No sound in a web build | No gesture yet (`waiting_for_gesture`), or the files are not in its asset source |
