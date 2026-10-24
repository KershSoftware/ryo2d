# Audio (5b) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace Ryo2D's thin kira 0.11 wrapper with professional-grade audio on kira 0.12.5:
- audio as registry assets, with hot reload;
- a mixer with live player volume settings;
- data-driven sound and music definitions;
- 2D spatial sound;
- pause and time scale;
- snapshots and ducking;
- intro-then-loop music that survives scene changes;
- the web;
- a mock backend that tests can assert on.

**Architecture:**
- **Assets.** Four new asset types go through the 5a registry: `AudioClip`, `Sound` (`.sound.ron`), `Music` (`.music.ron`) and `Mixer` (`.mixer.ron`).
- **Runtime.** An `AudioEngine` world resource owns a kira `AudioManager`, on cpal or on the mock backend, plus the bus tree, a voice table and the music player. A per-frame `audio_system` in PostUpdate applies everything that changes over time.
- **Access.** Scenes reach it through `ctx.audio()`, an `AudioView`, the way they reach assets through `ctx.assets()`.

**Tech Stack:** Rust 2024, bevy_ecs 0.20 (as `ryo2d::ecs`), kira 0.12.5, and the 5a asset registry (`src/core/asset/`).

**Spec:** `docs/specs/2026-10-24-audio-design.md` (b06f418b)

## Global Constraints

**Dependency.** kira is pinned at `0.12.5`, `default-features = false`, with features `cpal`, `wav`, `pcm`, `ogg`, `vorbis`, `mp3` and `flac`.
- `cpal-realtime` is added only if it builds on macOS and Linux without a system library.
- `cpal-realtime-dbus` is never added.
- No other new runtime dependency.

**Formats.** Exactly WAV, OGG Vorbis, MP3 and FLAC. Only mono and stereo files; anything else is `AssetError::Decode`.

**Mixer layout.**
- The standard buses are always present: `master`, then `music`, `sfx`, `ui`, `voice` and `ambience` under it.
- Custom buses are written with slashes, such as `sfx/footsteps`.
- The default mixer path is `audio/mixer.mixer.ron`. When it is absent, the standard layout applies.
- Extensions: `sound.ron`, `music.ron`, `mixer.ron`, and `wav`, `ogg`, `mp3`, `flac`.
- Pause defaults: `sfx`, `ambience` and `voice` pause with the game; `ui` and `music` do not.
- Time-scale defaults: `sfx` and `ambience` follow time scale.
- Focus-loss default: `Keep`.

**Smoothing and volume.**
- The minimum smoothing is `MIN_FADE = 10 ms`, and it applies to every volume, pan, pitch, pause, stop and effect-parameter change.
- Volume settings are values from 0 to 1, mapped to bus gain with amplitude = v², that is dB = 40·log10(v).
  - 0 is exact silence (kira's `Decibels::SILENCE`, -60 dB).
  - 1 is 0 dB.
  - Out-of-range or NaN values clamp, and NaN becomes the default.
- Gain layers add in dB: mixer default + player setting + snapshot + duck.

**Clips.**
- The decode threshold is 10 s: shorter clips decode to memory, longer ones stream (native only).
- On wasm every clip decodes.

**Voices.**
- Voice-limit rules: `PreventNew`, `StopOldest`, `StopQuietest`, `StopFarthest`, `StopLowestPriority`. Priority runs from 0 to 255.
- A global voice cap of 96 per bus track, below kira's 128 per track. Past it, the lowest-priority and then quietest voice is stolen with `MIN_FADE`.
- Spatial falloff curves: `Linear`, `Inverse`, `Log`.
- At most 32 voices carry a distance low-pass at once.

**Targets** (hard, as in 5a; a miss goes to the owner with a profile; nothing is loosened):
- an idle frame within 3% of Task 1's build;
- 64 voices (32 of them spatial) ≤ 50 µs of main-thread audio time per frame;
- `play` ≤ 5 µs;
- no allocation for audio on an idle frame;
- a clip load or reload adds ≤ 0.5 ms to any frame.

**Tour.** It stays pixel-exact against Task 1's baseline. The only additions allowed are the new `audio` demo's pages.

**House rules.**
- Paths are relative to the asset root, with no `assets/` prefix.
- Errors are `AssetError` with a path, warned once by the registry.
- No `unwrap` in library code.
- LF line endings.
- No Co-Authored-By or Claude-Session trailers in commits.
- Never name AGENTS.md in commit messages (say "module notes").
- Stage files by name.

## Review Focus

1. **A sound asked for every frame or hundreds of times at once** (bullets, footsteps). Voices stay within their limits and the global cap, nothing distorts, and kira never returns a capacity error. Test: Task 4, `a_thousand_plays_in_one_frame_stay_within_the_limit`.
2. **Rapid scene changes** (switching every frame). No voice leaks, music plays once and never restarts, and the voice count returns to the persistent set. Test: Task 9, `fifty_scene_switches_leak_no_voice_and_keep_one_music`.
3. **A huge file** (leap-king's 24.5 MB, 12.7-minute `rain.mp3`). It streams natively, without a decode spike or a 290 MB decode. Test: Task 2, `a_clip_longer_than_the_threshold_streams_and_is_never_decoded`.
4. **Volume settings at their ends, or corrupted** (`NaN`, `-1`, `5` in `system.json`). 0 is exact silence, 1 is 0 dB, and everything else is clamped. Test: Task 5, `volume_settings_at_their_ends_and_out_of_range`.
5. **No audio device, or a device lost mid-session.** A failed init falls back to the mock backend with one warning, and a stream error mid-session warns once and never panics. Test: Task 3, `a_device_that_fails_falls_back_to_the_mock_with_one_warning` and `a_stream_error_warns_once`.

## Where this plan differs from the spec

The facts behind these are kira 0.12.5's source (`~/.cargo/registry/src/*/kira-0.12.5`) and the engine's code.

1. **The web decodes every clip.** kira's streaming is `cfg(not(wasm32))` and spawns one OS thread per play. On the web, music and long clips therefore decode to memory in the pump; one minute of stereo f32 at 48 kHz is about 23 MB. Native streaming is unchanged.
2. **A gapless intro-then-loop comes from concatenation, not clock scheduling.** kira applies start times and tweens once per internal chunk of 128 frames, so a clock-scheduled loop is only chunk-accurate. So:
   - Two-clip intro-and-loop music decodes both clips and concatenates their frames into one sound, with a loop region at the intro's length. The two clips must share a sample rate.
   - One-clip music with a loop region uses kira's loop region, streaming natively when the clip is long.
   - The tests check this structure, not sample timing on the mock backend.
3. **A mixer reload rebuilds the bus tree.** kira fixes a track's effects and sends at build, and a sound cannot move between tracks. Every bus track is built with `persist_until_sounds_finish(true)`. On a mixer reload:
   - one-shots finish on the retired tracks;
   - loops and music restart on the new tracks at their position, with a `MIN_FADE` crossfade.

   The spec's "voices of a removed bus move to the parent" becomes "restart on the parent".
4. **A distance low-pass needs its own track.** kira's effects are per track, so a voice whose sound has a distance low-pass plays on its own child track with a filter. At most 32 voices have one at a time; beyond that a voice plays unfiltered. Gain and pan stay per voice.
5. **Audio becomes a world resource.** kira's `AudioManager` is Send and Sync, so `AudioEngine` is a `Resource`. Task 12 removes the `&mut Audio` threaded through `SceneManager`, `SceneHost`, cleanup and `SceneContext`.
6. **Time.** The game pause is `time.game.pause()`, `resume()` and `is_paused()`. Hitstop does not change `speed()`, so audio treats `is_hitstop()` as time scale 0.
7. **`Voice` methods queue commands.** The frame's `audio_system` applies them in PostUpdate, the same frame as the scene's update, so a `Voice` can be kept anywhere. `is_playing` reads shared state, which that system updates.
8. **Devices.** A device that fails to open falls back to the mock backend for the session, with one warning. A stream error mid-session warns once, and the engine carries on; it does not rebuild.
9. **The web unlock.** kira hides the browser's AudioContext, so on wasm the engine creates its kira manager inside the first user-gesture event: a key, mouse button or touch. Until then:
   - one-shots are dropped;
   - music and loops are kept and started at the unlock.
10. **The theme's UI sounds** follow the theme's skin pattern: a `path: Option<String>` with a `#[serde(skip)]` `Handle<Sound>`. The theme loader loads them. `Theme` stays serde-derived.

---

## File structure

**New: `src/core/audio/`** (it replaces `src/core/audio.rs` in Task 12):
- `mod.rs`: exports and module docs.
- `clip.rs`: `AudioClip`, `ClipData`, `AudioClipSettings`.
- `sound.rs`: the `Sound` definition and the voice-limit types.
- `music.rs`: the `Music` definition and the music player.
- `mixer.rs`: the `Mixer` definition and the bus tree at runtime, including gain layers, snapshots and ducking.
- `engine.rs`: `AudioEngine` (the resource), the kira manager over `Device` or `Mock`, the unlock, and the voice table.
- `voice.rs`: `Voice`, `VoiceId` and the command queue.
- `spatial.rs`: `AudioListener`, `AudioEmitter` and the spatial math.
- `settings.rs`: `VolumeKind` and the volume curve.
- `log.rs`: the doc-hidden audio log.
- `system.rs`: `audio_system`.
- `view.rs`: `AudioView`.

**New loaders: `src/core/asset/loaders/{audio_clip.rs, sound.rs, music.rs, mixer.rs}`.** They are registered in `insert_registry` (`src/core/asset/mod.rs:60`).

**Modified:**
- `Cargo.toml` (kira).
- `src/core/scene/{context.rs, manager.rs, cleanup.rs, mod.rs}`.
- `src/core/engine/{mod.rs, frame.rs, window.rs, tests.rs}`.
- `src/core/testing/{mod.rs, headless.rs}`.
- `src/core/ui/theme/mod.rs` and `src/core/asset/loaders/theme.rs`.
- The volume-settings plumbing: `src/core/config/{save.rs, resolve.rs, workspace.rs}`, `src/build.rs`, `src/cli/commands/{build.rs, init.rs}` and `tests/integration/config_resolution.rs`.
- `src/lib.rs` (the prelude).
- `src/core/error.rs`.
- The four games: breakout, bullet-hell, flappy and leap-king.

**New:**
- `examples/demos/audio/`;
- `benches/audio.rs`;
- `tests/audio_alloc.rs`;
- `tests/e2e/audio.rs`;
- `src/core/audio/AGENTS.md`;
- the `ryo-audio` skill rewrite.

---

### Task 1: kira 0.12.5 and the baselines

**Files:** Modify `Cargo.toml` (kira line) and `src/core/audio.rs` (only what 0.12 needs). Create in the plan workspace (git-ignored): `tour-base/` and `compare.sh`.

**Interfaces:** Produces the baseline tour that every later task compares against, and the "before" commit for Task 14.

- [ ] **Step 1:** Set kira to `{ version = "0.12.5", default-features = false, features = ["cpal","wav","pcm","ogg","vorbis","mp3","flac"] }`. Try adding `"cpal-realtime"` and keep it only if `cargo build` succeeds on macOS without a new system library. Note the result in the report.
- [ ] **Step 2:** Run `cargo build --workspace` and fix what 0.12 breaks in the old `audio.rs`. Expected: only feature-related changes; the public API is near-identical.
- [ ] **Step 3:** Run the gate: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --no-fail-fast`, and `cargo check -p ryo2d --lib --target wasm32-unknown-unknown`. Expected: green, with test counts unchanged.
- [ ] **Step 4:** Tour baseline. Run `./dev.sh demos:tour` twice, and copy `target/demo-tour` to `<workspace>/tour-base/`.
  - Record the pages that differ between the two runs as nondeterministic. The 5a list was particles pages 1, 2, 4, 5, 7, 8, 9 and 10, and steering page 1.
  - Write `<workspace>/compare.sh <base>`. For every base page not in the list, it prints `DIFF <page> <AE>` using `magick compare -metric AE`, then a summary line `compared N: S same, D differ, M missing`.
- [ ] **Step 5:** Commit: `chore: kira 0.12.5, with its format features named`.

### Task 2: `AudioClip`, an asset decoded or streamed off the main thread

**Files:**
- Create `src/core/audio/mod.rs` (temporarily as `src/core/audio/` beside `audio.rs`: rename `audio.rs` to `audio/legacy.rs` and re-export it unchanged until Task 12), `src/core/audio/clip.rs` and `src/core/asset/loaders/audio_clip.rs`.
- Fixtures: `tests/fixtures/audio/tone.{wav,ogg,mp3,flac}`, `tone8.wav` (8-bit mono), `tonef32.wav` (32-bit float stereo) and `surround.wav` (6 channels), each ≤ 20 KB and generated, plus `tests/fixtures/audio/README.md` giving the generating command and stating that the files are generated tones with no licence restriction.

**Interfaces:**
- Produces:

```rust
pub struct AudioClip { /* data: ClipData, duration: Duration, sample_rate: u32, channels: u16 */ }
impl Asset for AudioClip {}
impl AudioClip {
  pub fn duration(&self) -> Duration;
  pub fn sample_rate(&self) -> u32;
  pub fn is_streamed(&self) -> bool;
  /// A clip made in code from interleaved or mono frames
  pub fn from_frames(sample_rate: u32, frames: &[[f32; 2]]) -> AudioClip;
  pub(crate) fn data(&self) -> &ClipData;
}
pub(crate) enum ClipData { Decoded(kira::sound::static_sound::StaticSoundData), Encoded(Arc<[u8]>) }
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum ClipMode { #[default] Auto, Decode, Stream }
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct AudioClipSettings { pub mode: ClipMode }
pub struct AudioClipLoader; // extensions "wav","ogg","mp3","flac"
pub(crate) const DECODE_THRESHOLD: Duration = Duration::from_secs(10);
```

- [ ] **Step 1: Write the failing tests** in `loaders/audio_clip.rs`. Use a `MemorySource` with the fixtures and `AssetsView::load_now_with`; see the image loader's `images_over` helper pattern.
  - `each_format_loads_its_duration_and_rate`: every fixture `tone.*` loads, its `duration()` is within 5 ms of the generated length, and its `sample_rate` is the generated rate.
  - `eight_bit_mono_and_float_stereo_wavs_load`.
  - `a_surround_file_is_a_decode_error_naming_its_path`: `surround.wav` gives `AssetError::Decode` whose `report()` contains `surround.wav`.
  - `a_corrupt_file_is_a_decode_error`.
  - `a_short_clip_decodes_and_a_forced_stream_keeps_its_bytes`: with `Auto`, `tone.ogg` has `is_streamed() == false`; with `ClipMode::Stream`, it is `true`.
  - `a_clip_longer_than_the_threshold_streams_and_is_never_decoded` (Review Focus 3): use a test-only threshold override (`#[cfg(test)]` setter on the loader) at 50 ms, and check that a 200 ms clip is streamed and holds only its encoded bytes, with the same length as the file.
  - `on_wasm_every_clip_decodes` (`#[cfg(target_arch = "wasm32")]`, compile-checked only).
- [ ] **Step 2:** Run `cargo test -p ryo2d --lib asset::loaders::audio_clip`. Expected: FAIL, with the types undefined.
- [ ] **Step 3: Implement.**
  - The probe uses `StreamingSoundData::from_cursor(Cursor::new(bytes.clone()))?.duration()` on native.
  - Decoding uses `StaticSoundData::from_cursor`.
  - Map kira's `FromFileError` to `AssetError::decode(path, e)`.
  - Register the loader in `insert_registry`.
- [ ] **Step 4:** Run the tests. Expected: PASS. Then run the gate.
- [ ] **Step 5:** Commit: `feat: audio clips are registry assets, decoded or streamed off the main thread`.

### Task 3: `AudioEngine`, `Voice` and `ctx.audio()`, on a device or the mock backend

**Files:**
- Create `src/core/audio/{engine.rs, voice.rs, log.rs, system.rs, view.rs}`.
- Modify `src/core/engine/mod.rs` (insert `AudioEngine`; register `audio_system` in PostUpdate after `PostUpdateSet::Camera`), `src/core/scene/context.rs` (add `pub fn audio(&mut self) -> AudioView<'_>` beside the legacy field), `src/core/testing/{mod.rs, headless.rs}` and `src/lib.rs`.

**Interfaces:**
- Consumes `AudioClip` (Task 2).
- Produces:

```rust
#[derive(Resource)] pub struct AudioEngine { /* manager: Manager (Device(AudioManager<CpalBackend>) | Mock(AudioManager<MockBackend>) | Pending on wasm), buses: BusTree, voices: VoiceTable, log: AudioLog, ... */ }
impl AudioEngine {
  pub(crate) fn new(device: bool) -> AudioEngine;                    // device=false: mock at 48 kHz
  pub(crate) fn unlock(&mut self);                                   // wasm: build the cpal manager; native: no-op
  #[doc(hidden)] pub fn set_logging(&mut self, on: bool);
  #[doc(hidden)] pub fn log(&self) -> &[AudioLogEntry];
}
pub(crate) const MIN_FADE: Duration = Duration::from_millis(10);
#[derive(Clone)] pub struct Voice(/* Arc<VoiceShared>: id, command sender, state */);
impl Voice {
  pub fn stop(&self, fade: Duration); pub fn pause(&self); pub fn resume(&self);
  pub fn set_volume(&self, db: f32); pub fn set_pitch(&self, semitones: f32);
  pub fn is_playing(&self) -> bool; pub fn is_stopped(&self) -> bool;
}
pub struct AudioView<'w> { /* &'w mut World */ }
impl AudioView<'_> {
  pub fn play(&mut self, sound: impl Into<Playable>) -> Voice;      // Playable: &Handle<AudioClip> (Task 4 adds &Handle<Sound>)
  pub fn play_with(&mut self, sound: impl Into<Playable>, options: PlayOptions) -> Voice;
  pub fn voices_on(&self, bus: &str) -> usize;
}
#[derive(Debug, Clone, Copy, Default)] pub struct PlayOptions { pub position: Option<Vec2>, pub persist: bool, pub volume_db: f32, pub pitch: f32 }
#[doc(hidden)] pub fn audio_system(world: &mut World);               // drains Voice commands, updates states, advances the mock backend by Time.real delta
pub mod testing { pub fn audio_world() -> World; pub fn audio_step(world: &mut World, dt: Duration); } // in src/core/testing
#[doc(hidden)] impl HeadlessEngine { pub fn audio_log(&self) -> Vec<AudioLogEntry>; }
#[doc(hidden)] #[derive(Debug, Clone, PartialEq)] pub struct AudioLogEntry { pub kind: AudioLogKind, pub voice: u32, pub source: String, pub bus: String, pub gain_db: f32, pub pan: f32, pub rate: f64, pub fade: Duration }
#[doc(hidden)] #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum AudioLogKind { Start, Stop, Pause, Resume, Gain, Pan, Rate, Refused, Culled, Stolen }
```

  - In this task the bus tree is the fixed standard layout. Task 5 makes it come from the mixer.
  - The mock backend is advanced in whole 128-frame chunks covering `Time.real` delta at 48 kHz, with the remainder carried over.
  - The log is a ring of 4096 entries, off unless `set_logging(true)`.
  - `audio_world()` is `asset_world()` plus `Time` plus `AudioEngine::new(false)`, with logging on.
  - `audio_step` advances `Time.real` by `dt`, pumps the registry and runs `audio_system`.
  - Headless engines (`build_headless`, `RYO_HEADLESS`) and failed devices use the mock backend.

- [ ] **Step 1: Write the failing tests** (`engine.rs` and `voice.rs` tests, using `audio_world`):
  - `a_clip_plays_on_sfx_and_the_log_shows_it`: the first log entry is `Start` on bus `sfx` with gain 0 dB.
  - `stop_fades_and_really_stops`: the log's `Stop` entry has `fade == 30 ms`, and after `audio_step(50 ms)` `is_stopped()` is true.
  - `every_change_uses_at_least_the_minimum_fade`: `stop(Duration::ZERO)`, `pause()`, `set_volume` and `set_pitch` all log a `fade >= MIN_FADE`.
  - `dropping_a_voice_does_not_stop_it`.
  - `voices_on_counts_a_bus`.
  - `a_device_that_fails_falls_back_to_the_mock_with_one_warning`: use a test seam `AudioEngine::new_failing_device()` and check exactly one warning containing `audio device` in the dev log.
  - `a_stream_error_warns_once`: inject two errors through a `#[cfg(test)]` hook, and check for one warning.
  - In `tests/e2e/audio.rs` (`HeadlessEngine::new_or_skip`), `a_headless_engine_plays_on_the_mock`: a scene that plays a clip in `enter` shows `Start` in `audio_log()`.
- [ ] **Step 2:** Run them. Expected: FAIL.
- [ ] **Step 3: Implement** as in Interfaces. A `Voice` holds an `Arc` with an atomic state and a crossbeam `Sender<VoiceCommand>`. `audio_system` drains the commands and applies them as kira tweens with `max(fade, MIN_FADE)`.
- [ ] **Step 4:** Run the tests. Expected: PASS. Run the gate and the tour (`compare.sh`: all pages same).
- [ ] **Step 5:** Commit: `feat: an audio engine plays clips as voices, on a device or the mock backend, through ctx.audio()`.

### Task 4: Sound definitions: variations, limits, priority and cooldown

**Files:** Create `src/core/audio/sound.rs` and `src/core/asset/loaders/sound.rs`. Modify `voice.rs`, `engine.rs` and `view.rs`.

**Interfaces:**
- Consumes Tasks 2 and 3.
- Produces:

```rust
pub struct Sound { /* clips: Vec<SoundClip{clip: Handle<AudioClip>, weight: f32, region: Option<Range<f64>>, loop_region: Option<Range<f64>>}>, bus: String, volume_db: f32, volume_range_db: f32, pitch: f32, pitch_range: f32, no_repeat: u8, limit: Option<VoiceLimit>, priority: u8, cooldown: Duration, looping: bool, fade_in: Duration, fade_out: Duration, spatial: Option<Spatial> */ }
impl Asset for Sound {}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)] pub enum LimitRule { PreventNew, StopOldest, StopQuietest, StopFarthest, StopLowestPriority }
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)] pub struct VoiceLimit { pub max: u16, pub rule: LimitRule }
pub struct SoundLoader; // extension "sound.ron"; clips loaded with cx.load::<AudioClip>, as dependencies
// Playable gains From<&Handle<Sound>>
```

The RON form is:

```ron
(clips: [(path: "sfx/step1.wav", weight: 1.0), (path: "sfx/step2.wav")], bus: "sfx/footsteps", volume: -6.0, volume_range: 2.0, pitch: 0.0, pitch_range: 1.0, no_repeat: 1, limit: (max: 4, rule: StopOldest), priority: 64, cooldown: 0.03, looping: false, fade_in: 0.0, fade_out: 0.05)
```

Every field except `clips` is optional, with the defaults from the spec.

This task defines and parses the spatial data. Task 8 gives it behaviour:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)] pub struct Spatial { pub min_distance: f32, pub max_distance: f32, pub curve: Falloff, pub pan_width: f32, pub low_pass_at_max: Option<f64> }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)] pub enum Falloff { Linear, Inverse, Log }
```

`StopFarthest` uses a voice's distance to the listener, which comes in Task 8. Until then, every voice's distance is 0.

Picking is seeded from a `SmallRng` on the engine, so tests can call `AudioEngine::seed(u64)` (`#[doc(hidden)]`).

- [ ] **Step 1: Write the failing tests:**
  - `a_definition_loads_with_its_clips_as_dependencies`: the sound is not ready until each clip is.
  - `weights_pick_in_proportion`: weights 3 and 1 over 4,000 seeded plays give 3,000 ± 150.
  - `no_repeat_never_plays_a_clip_twice_in_a_row`: two clips, `no_repeat: 1`, 200 plays.
  - `volume_and_pitch_ranges_stay_inside_their_ranges`, read from the log's `gain_db` and `rate`.
  - `cooldown_refuses_a_play_too_soon`: the log shows `Refused`.
  - Each rule gets a test with `max: 2` and three plays: `prevent_new_refuses_the_third`, `stop_oldest_stops_the_first`, `stop_quietest_stops_the_quietest`, `stop_lowest_priority_stops_the_lowest` and `stop_farthest_stops_the_farthest_and_treats_no_position_as_zero`.
  - `a_bus_limit_counts_its_subtree`.
  - `a_thousand_plays_in_one_frame_stay_within_the_limit` (Review Focus 1): `max: 4` keeps 4 voices; with no limit, the global cap of 96 holds, the log shows `Stolen`, and no kira error is ever returned.
  - `playing_an_unready_sound_is_a_stopped_voice` and `a_failed_sound_warns_once`.
  - `a_reload_keeps_a_voice_valid`.
- [ ] **Step 2:** Run them. Expected: FAIL.
- [ ] **Step 3: Implement.** The voice table keeps, per voice: sound id, bus, start order, current gain in dB, priority, position, and the kira handle (static or streaming). Live voices hold strong clip handles.
- [ ] **Step 4:** Run the tests. Expected: PASS. Run the gate.
- [ ] **Step 5:** Commit: `feat: sound definitions pick variations and keep their voices within limits`.

### Task 5: The mixer: buses, effects, sends and live player volumes

**Files:**
- Create `src/core/audio/{mixer.rs, settings.rs}` and `src/core/asset/loaders/mixer.rs`.
- Modify `engine.rs`, `view.rs`, `src/core/engine/mod.rs` (load `audio/mixer.mixer.ron` at start with `load`, and apply it when it lands).
- The volume-settings plumbing, adding `ui_volume`, `voice_volume` and `ambience_volume`: `src/core/config/{save.rs, resolve.rs, workspace.rs}`, `src/build.rs`, `src/cli/commands/{build.rs, init.rs}`, `tests/integration/config_resolution.rs`, and the example `Ryo.toml` files listed in the facts.

**Interfaces:**
- Produces:

```rust
pub struct Mixer { /* buses: Vec<BusDef{name, volume_db, effects: Vec<EffectDef>, sends: Vec<(String, f32)>, pauses_with_game: Option<bool>, follows_time_scale: Option<bool>, limit: Option<u16>}>, snapshots, ducks, on_focus_loss: FocusLoss */ }
impl Asset for Mixer {}
pub enum EffectDef { LowPass{cutoff: f64, resonance: f64}, HighPass{..}, Eq{kind, frequency, gain_db, q}, Reverb{feedback, damping, stereo_width, mix}, Delay{seconds, feedback_db, mix}, Compressor{threshold_db, ratio, attack, release, makeup_db, mix}, Distortion{kind, drive_db, mix} }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)] pub enum VolumeKind { Master, Music, Sfx, Ui, Voice, Ambience }
pub fn volume_to_db(v: f32) -> f32;                                  // 40·log10(v), v<=0 -> -60, NaN -> default handled by caller
impl AudioView<'_> {
  pub fn set_volume_setting(&mut self, kind: VolumeKind, value: f32); pub fn volume_setting(&self, kind: VolumeKind) -> f32;
  pub fn set_bus_volume(&mut self, bus: &str, db: f32);
}
pub struct MixerLoader; // "mixer.ron"
```

  - Each bus is a kira sub-track built from the mixer, with `persist_until_sounds_finish(true)`.
  - A send bus is a kira send track.
  - Gain is mixer default, plus setting, plus snapshot, plus duck, in dB, applied with one `set_volume(total, MIN_FADE tween)`.
  - A reloaded mixer rebuilds the tree, as in Differences 3.
  - `SystemState` gains the three fields. An old `system.json` without them takes the `Ryo.toml` defaults, or 1.0.
  - The engine applies the six settings at start and whenever they are set.
  - `Ryo.toml` `[system]` gains `audio_buffer_frames` (optional), along the same plumbing. When set, the device's cpal stream config uses `BufferSize::Fixed(n)`, taken from `default_output_config()` (fact: kira's `CpalBackendSettings.config`). Unset leaves cpal's default.

- [ ] **Step 1: Write the failing tests:**
  - `the_standard_layout_exists_without_a_mixer_file`.
  - `a_custom_bus_takes_its_parents_setting`.
  - `a_setting_changes_a_playing_voices_bus`: play a loop, `set_volume_setting(Sfx, 0.5)`, step, and check that the bus `Gain` entry is -12.04 dB ± 0.01.
  - `volume_settings_at_their_ends_and_out_of_range` (Review Focus 4): 0 gives -60 (silence), 1 gives 0, NaN gives the default, and -1 and 5 clamp.
  - `gain_layers_add_in_db`.
  - `effects_and_sends_are_built_from_the_file`: a log entry, or an accessor counting the tree's effects and sends.
  - `a_mixer_reload_restarts_loops_on_the_new_tree_and_lets_one_shots_finish`.
  - `an_old_system_json_without_the_new_volumes_takes_the_defaults` (config tests).
  - `the_audio_buffer_setting_resolves_from_ryo_toml` (config tests).
- [ ] **Step 2:** Run them. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4:** Run the tests. Expected: PASS. Run the gate, including `tests/integration/config_resolution.rs`.
- [ ] **Step 5:** Commit: `feat: a mixer of buses, effects and sends, with player volumes that reach playing sounds`.

### Task 6: Snapshots and ducking

**Files:** Modify `mixer.rs`, `system.rs` and `view.rs`.

**Interfaces:**
- Produces `AudioView::{snapshot(&mut self, name: &str), clear_snapshot(&mut self)}`.
- The mixer RON gains:
  - `snapshots: { "pause": (blend: 0.3, buses: { "music": (volume: -6.0, low_pass: 800.0) }) }`;
  - `ducks: [(trigger: "voice", target: "music", by: -9.0, attack: 0.1, release: 0.6)]`.
- A snapshot's `low_pass` needs a filter on that bus, so the tree builds a low-pass, open at 20 kHz, on every bus any snapshot names a `low_pass` for.

- [ ] **Step 1: Write the failing tests:**
  - `a_snapshot_blends_to_its_targets_over_its_blend`: after 0.15 s the music gain is between 0 and -6; after 0.3 s it is -6 ± 0.1; the cutoff is 800.
  - `clear_snapshot_blends_back`.
  - `a_new_snapshot_blends_from_where_the_mix_is`.
  - `a_slider_moved_under_a_snapshot_is_heard_at_once`.
  - `music_ducks_while_a_voice_plays_and_releases_after`: depth -9 within the attack, back within the release after the voice ends.
- [ ] **Step 2:** Run them. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4:** Run the tests. Expected: PASS. Run the gate.
- [ ] **Step 5:** Commit: `feat: mixer snapshots blend the mix, and ducking dips one bus under another`.

### Task 7: Game state: pause, time scale, hitstop, focus and scene ownership

**Files:** Modify `system.rs`, `engine.rs`, `voice.rs`, `src/core/engine/window.rs:678` (pass focus to `AudioEngine`) and `src/core/scene/{manager.rs, cleanup.rs}` (on a scene's exit, stop its scope's voices).

**Interfaces:**
- Consumes Tasks 3 to 5, `Time` (`time.game.is_paused()`, `speed()`, `is_hitstop()`) and `SceneContext`'s scope id.
- Produces `pub(crate) fn AudioEngine::set_focused(&mut self, bool)` and `pub(crate) fn AudioEngine::release_scope(&mut self, scope: ScopeId)`.
- Voices remember the scope they were played from. `PlayOptions.persist` and music are engine-owned.

- [ ] **Step 1: Write the failing tests:**
  - `pausing_the_game_pauses_sfx_ambience_and_voice_but_not_ui_or_music`, with each paused bus logging `Pause` with `MIN_FADE`.
  - `resuming_continues_from_the_same_position`: the position after resume equals the position at pause, ± one chunk.
  - `half_time_scale_halves_following_buses_rates`: an sfx voice with pitch +12 semitones has rate 2.0, and 1.0 at time scale 0.5; a ui voice stays 1.0.
  - `hitstop_pauses_following_buses`.
  - `focus_loss_keeps_by_default_and_mutes_when_the_mixer_says`.
  - `a_scene_exit_stops_its_voices_with_their_fade_and_keeps_persistent_ones`.
- [ ] **Step 2:** Run them. Expected: FAIL.
- [ ] **Step 3: Implement.** The effective scale is 0 if paused or in hitstop, and `speed()` otherwise. A following voice's rate is its own pitch times that scale. A scale at or below 0.01 pauses the following buses.
- [ ] **Step 4:** Run the tests. Expected: PASS. Run the gate.
- [ ] **Step 5:** Commit: `feat: audio pauses with the game, slows with time scale, and stops with the scene that played it`.

### Task 8: 2D spatial sound

**Files:** Create `src/core/audio/spatial.rs`. Modify `sound.rs`, `system.rs`, `view.rs` and `src/lib.rs`.

**Interfaces:**
- Produces:

```rust
// Spatial and Falloff: defined in Task 4
#[derive(Component, Debug, Clone, Copy, Default)] pub struct AudioListener;
#[derive(Component, Debug, Clone)] pub struct AudioEmitter { pub sound: Handle<Sound>, pub play_on_spawn: bool }
impl AudioView<'_> { pub fn play_at(&mut self, sound: impl Into<Playable>, position: Vec2) -> Voice; }
pub fn falloff_gain_db(spatial: &Spatial, distance: f32) -> f32;  // 0 dB inside min, SILENCE at max, curve between
pub fn pan_for(spatial: &Spatial, dx: f32) -> f32;                 // clamp(dx / pan_width, -1, 1)
```

  - The listener is the `AudioListener` entity's `GlobalTransform`, or else the main camera's.
  - An emitter's voices follow its entity's `GlobalTransform`. Despawning the entity stops its loops with their fade and lets its one-shots finish.
  - The low-pass applies as in Differences 4: at most 32 at once, past which a voice plays unfiltered.

- [ ] **Step 1: Write the failing tests:**
  - `falloff_curves_match_at_min_mid_and_max`, checking each curve at min, (min+max)/2 and max.
  - `pan_follows_the_horizontal_offset`.
  - `an_emitter_voice_updates_its_gain_and_pan_as_it_moves`, from log `Gain` and `Pan` entries over 3 steps.
  - `a_one_shot_beyond_range_is_culled`, logged as `Culled`.
  - `a_loop_out_of_range_runs_silent_and_returns`.
  - `the_camera_is_the_listener_without_an_audio_listener`.
  - `despawning_an_emitter_stops_its_loops`.
  - `the_33rd_filtered_voice_plays_unfiltered`.
- [ ] **Step 2:** Run them. Expected: FAIL.
- [ ] **Step 3: Implement.** `audio_system` reads the listener and emitter transforms after the PostUpdate camera set.
- [ ] **Step 4:** Run the tests. Expected: PASS. Run the gate.
- [ ] **Step 5:** Commit: `feat: 2D spatial sound falls off and pans from a listener, with an optional distance low-pass`.

### Task 9: Music: intro then loop, crossfades, and surviving scenes

**Files:** Modify `src/core/audio/music.rs`, `src/core/asset/loaders/music.rs`, `engine.rs`, `view.rs` and `src/core/scene/cleanup.rs` (`SceneCleanup::MUSIC` calls `stop_music(fade)`).

**Interfaces:**
- Produces:

```rust
pub struct Music { /* sound: MusicData (Concatenated(StaticSoundData) | Looping{clip: Handle<AudioClip>, loop_region: Range<f64>}), volume_db, fade_in, crossfade, tempo: Option<f32>, meter: Option<(u8, u8)> */ }
impl Asset for Music {}
pub struct MusicLoader; // "music.ron": (intro: "music/a_intro.ogg", loop: "music/a_loop.ogg") | (clip: "music/b.ogg", loop_start: 12.5, loop_end: None), volume, fade_in, crossfade, tempo, meter, bus (default "music")
impl AudioView<'_> {
  pub fn play_music(&mut self, music: &Handle<Music>); pub fn stop_music(&mut self, fade: Duration);
  pub fn pause_music(&mut self); pub fn resume_music(&mut self); pub fn current_music(&self) -> Option<AssetId<Music>>;
}
```

  - Two-clip music decodes both clips and concatenates their frames, as in Differences 2. Mismatched sample rates are a decode error naming both paths.
  - Music is engine-owned and plays on the music bus.

- [ ] **Step 1: Write the failing tests:**
  - `intro_and_loop_concatenate_with_the_loop_region_at_the_intro_s_end`: the frame count is the sum of both, and the loop start is the intro's length in samples.
  - `mismatched_rates_fail_naming_both`.
  - `a_one_clip_loop_region_is_kira_s_loop_region`.
  - `play_music_crossfades_from_the_current_track`: the old track logs `Stop` with fade = crossfade, and the new one `Start` with fade-in = crossfade.
  - `playing_the_track_already_playing_does_nothing`.
  - `stop_music_really_stops`, guarding against the old bug coming back.
  - `music_survives_a_scene_change`.
  - `scene_cleanup_music_stops_it`.
  - `fifty_scene_switches_leak_no_voice_and_keep_one_music` (Review Focus 2), driven through the scene manager over `audio_world` plus a `SceneManager`, or `HeadlessEngine` in `tests/e2e/audio.rs`.
- [ ] **Step 2:** Run them. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4:** Run the tests. Expected: PASS. Run the gate.
- [ ] **Step 5:** Commit: `feat: music plays an intro into a gapless loop, crossfades, and carries on across scenes`.

### Task 10: Hot reload of clips and sounds

**Files:** Modify `system.rs` (read `AssetEvent<AudioClip>` and `AssetEvent<Sound>` with a `MessageCursor`; do not take the stores' change lists) and `engine.rs`.

**Interfaces:**
- Consumes the registry's events.
- Produces this behaviour:
  - A reloaded clip lets its playing one-shots finish on the old data. Its playing loops and music restart on the new data at the same position, with a `MIN_FADE` crossfade.
  - A reloaded sound gives its playing loops its new volume and pitch, and applies everything to new plays.
  - A `ReloadFailed` keeps all of this unchanged.

- [ ] **Step 1: Write the failing tests:**
  - `a_reloaded_clip_switches_a_playing_loop_at_its_position`: log a `Stop` and `Start` pair, with the new start position equal to the old position ± one chunk.
  - `a_reloaded_clip_lets_one_shots_finish`.
  - `a_reloaded_sound_retunes_its_playing_loops`.
  - `a_broken_save_keeps_everything_playing`.

  Each uses `MemorySource::insert` plus `AssetServer::reload`.
- [ ] **Step 2:** Run them. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4:** Run the tests. Expected: PASS. Run the gate.
- [ ] **Step 5:** Commit: `feat: a saved audio file or sound definition reloads into what is playing`.

### Task 11: The web

**Files:** Modify `engine.rs` (the `Manager::Pending` state on wasm, `unlock`, and the queued music and loops) and `src/core/engine/window.rs` (on wasm, the first `KeyboardInput`, `MouseInput` or `Touch` calls `AudioEngine::unlock`).

**Interfaces:** Produces this behaviour on wasm:
- the manager is created inside the first gesture event;
- one-shots before it are dropped and logged as `Refused`;
- music and loops before it start at the unlock;
- clips always decode, in the pump.

- [ ] **Step 1: Write the failing tests:**
  - `before_the_unlock_one_shots_drop_and_music_waits`. This is testable natively through a `#[cfg(test)]` constructor that starts `Pending`, then `unlock()` builds the mock.
  - `the_unlock_happens_once`.
- [ ] **Step 2:** Run them. Expected: FAIL.
- [ ] **Step 3: Implement.** Then run `cargo check -p ryo2d --lib --target wasm32-unknown-unknown` and `cargo clippy -p ryo2d --lib --target wasm32-unknown-unknown -- -D warnings`.
- [ ] **Step 4:** Run the tests and the gate. Write the manual browser check for the hand-play in the report: run a demo's web build (`ryo2d build --web` or the repo's web recipe, per the `ryo-cli` skill), check there is silence until the first click, then sound.
- [ ] **Step 5:** Commit: `feat: audio works on the web, starting at the first user gesture`.

### Task 12: Migration: remove the old audio, and move the theme and the games

**Files:**
- Delete `src/core/audio/legacy.rs`.
- Modify `src/core/scene/{context.rs, manager.rs, cleanup.rs, mod.rs}`: remove the `audio` field and every `&mut Audio` parameter (manager.rs `call`, `enter_initial`, `enter_top`, `push`, `pop`, `switch`, `clear`, `reset`, `update`, `handle_action`, `update_transition`, `execute_pending_action`, `ui_pass`; `execute_cleanup`; about 60 test call sites).
- Modify `src/core/engine/{mod.rs, frame.rs, tests.rs}`: remove `SceneHost.audio` and the old init at mod.rs:547-566. The UI sounds (frame.rs:486-527) play `Handle<Sound>`s through `AudioEngine` on the `ui` bus.
- Modify `src/core/ui/theme/mod.rs` (`UiSounds` takes the skin pattern: `path: Option<String>` plus a `#[serde(skip)]` handle for each sound), `src/core/asset/loaders/theme.rs` (load them with `cx.load::<Sound>`, or as an `AudioClip` played with defaults when the path is not a `.sound.ron`) and `src/core/error.rs` (remove `AudioError` and `AudioResult`).
- Modify `src/lib.rs` (the prelude exports the new types; remove `Audio`, `AudioManager`, `SoundOptions`, `MusicOptions`, `SoundHandle`, `MusicHandle`), `src/core/testing/mod.rs:26` (docs), and `src/core/ui/widgets/slider.rs:42` (the doc example).
- Games:
  - **breakout** (`scenes/{menu.rs, game.rs, pause.rs}`): sound definitions under `assets/audio/` and a `music.ron` for each BGM. `play_music` replaces the handle passing. The pause sliders use `set_volume_setting`.
  - **bullet-hell** (`scenes/menu.rs`, `game/{mod.rs, combat.rs, enemies.rs}`): enemy spawns and bullets become `AudioEmitter`s or `play_at`, in place of the hard-coded pan.
  - **flappy** (`scenes/{menu.rs, game.rs}`).
  - **leap-king** (`game.rs`, `menu.rs`): the remote players' jump and land become `play_at` with a `Spatial` in their `.sound.ron`, deleting `spatial_sound_options` and its bug. `rain.mp3` streams as `Music`, or as an ambience loop with `persist`.

**Interfaces:** Consumes everything above. Produces the final public surface.

- [ ] **Step 1: Write the failing tests:**
  - `every_game_audio_file_loads` (`tests/integration/audio_files.rs`): every `.wav`, `.mp3`, `.sound.ron` and `.music.ron` under each game's `assets/` loads through a `DiskSource`.
  - `a_theme_ui_sound_plays_on_the_ui_bus`.
  - Each migrated game's existing tests stay green.
- [ ] **Step 2:** Run them. Expected: FAIL.
- [ ] **Step 3: Implement the removals and the moves.** Then run `grep -rn "AudioManager\|SoundOptions\|MusicOptions\|SoundHandle\|MusicHandle\|AudioError\|AudioResult\|ctx\.audio\.\|legacy" src examples tests benches` (excluding `src/cli/skills` and `.claude/skills`). Expected: no hits.
- [ ] **Step 4:** Run the gate and the tour (`compare.sh`: all same).
- [ ] **Step 5:** Commit: `refactor: the old audio wrapper is gone; scenes, the theme and the games play through the audio engine`.

### Task 13: The audio demo

**Files:**
- Create `examples/demos/audio/`, following `examples/demos/AGENTS.md`: `Cargo.toml` (`audio-demo`, a lib and a bin), `Ryo.toml`, `src/{main.rs, lib.rs}`, a page module each, and `tests/pages.rs`.
- Create `assets/{audio/mixer.mixer.ron, sounds/*.sound.ron, music/*.music.ron}` and generated `.wav` files.
- Create `src/bin/make-sounds.rs`, which writes the WAVs (tones, noise bursts, chord loops, and an intro and loop at one rate) from code, so they carry no licence restriction. Document this in `assets/README.md`.

**Interfaces:** Consumes the public API.

The pages are:
1. buses, with live sliders for the six settings;
2. variations and no-repeat;
3. voice limits, one rule per row;
4. spatial: emitters orbiting a listener, moved with the arrow keys;
5. pause, time scale, a snapshot and ducking;
6. music: intro then loop, and a crossfade between two tracks;
7. hot reload: save a file and its reload count goes up.

Each page shows its state as text and bars from `voices_on` and the volume settings, so the headless tour captures something meaningful.

- [ ] **Step 1: Write the failing test** `tests/pages.rs`: every page enters, steps 120 frames under `HeadlessEngine::new_or_skip` with no warning in the dev log, and page 3's limits hold (`voices_on("sfx")` ≤ the page's max).
- [ ] **Step 2:** Run it. Expected: FAIL.
- [ ] **Step 3: Implement.** Run `cargo run -p audio-demo --bin make-sounds` once and commit its output.
- [ ] **Step 4:** Run the tests and the tour. The new `audio/page-*.png` files are deterministic across two runs. Every other page is the same as `tour-base`.
- [ ] **Step 5:** Commit: `feat: an audio demo of buses, variations, limits, spatial sound, the mix, music and hot reload`.

### Task 14: Measuring the targets

**Files:** Create `benches/audio.rs` (`[[bench]] name = "audio" harness = false`, guards as in `benches/guards/mod.rs`), `tests/audio_alloc.rs` (the counting allocator, as in `tests/asset_pump_alloc.rs`) and `docs/backlog/audio.md` (a "5b: measured" section).

**Interfaces:** Consumes everything above.

- [ ] **Step 1: Write the guards and the alloc test:**
  - `voices_64`: 64 looping voices, 32 of them emitters moving each frame, at 60 Hz pacing over 2,000 frames. The median main-thread time of `audio_system` must be ≤ 50 µs.
  - `play_cost`: 10,000 `play` calls of a decoded clip, with a median ≤ 5 µs.
  - `load_hitch`: a 30 s OGG and a 1 s WAV reloaded every 50 frames. The extra time on the frame the reload lands must be ≤ 0.5 ms.
  - `idle_frame_audio_allocates_nothing` (`tests/audio_alloc.rs`): after warm-up, an idle frame with the engine running allocates 0 times.
- [ ] **Step 2: Measure** on a quiet machine, with `uptime` before each run, three alternating rounds each.
  - The idle frame and the empty frame (the `ui_render` guard and `foundation`) against Task 1's commit, built in a scratch worktree with its own target dir. Each must be within 3%.
  - The audio guards on the branch.
- [ ] **Step 3: Record** in `docs/backlog/audio.md`:
  - every median, every load and every verdict;
  - the output latency of the default desktop configuration, as cpal's buffer size and sample rate, in ms;
  - the latency with `audio_buffer_frames = 256`.

  On a miss, stop and report with a profile.
- [ ] **Step 4:** Run the gate.
- [ ] **Step 5:** Commit: `test: the audio engine's frame, play and reload benches, and their measurements`.

### Task 15: Docs and skills

**Files:**
- Create `src/core/audio/AGENTS.md`.
- Rewrite `.claude/skills/ryo-audio/SKILL.md`, with a byte-identical `src/cli/skills/ryo-audio.md` and its description in `src/cli/commands/install_skills.rs:54-56`.
- Update `ryo-api`, `ryo-assets`, `ryo-config-save`, `ryo-game-dev`, `ryo-ui` (theme sounds) and `ryo-engine-dev`, with version bumps and both copies.
- Update `src/core/asset/AGENTS.md` (audio is now registry assets; `insert_registry`'s list), `src/core/AGENTS.md`, `docs/architecture/overview.md` (lines 35, 124, 263, 409, 428) and `README.md` if it names the old API.

**Interfaces:** Consumes the final API.

The docs cover:
- the workflow from DAW export to `assets/`, then to a `.sound.ron`, then to `ctx.audio().play`;
- the mixer file;
- volume settings and their curve;
- the voice-limit rules;
- spatial sound;
- pause and time scale;
- snapshots and ducking;
- music;
- hot reload;
- the web unlock;
- testing with `audio_world` and the audio log.

State each fact with its reason, with no history wording.

- [ ] **Step 1: Write.** Check every name against the code, and check every skill pair with `cmp`.
- [ ] **Step 2:** Run `cargo test -p ryo2d --lib cli::` and the gate. Then grep the skills for `AudioManager|SoundOptions|MusicOptions|SoundHandle|MusicHandle|play_music_once|load_sound`. Expected: no hits.
- [ ] **Step 3:** Commit: `docs: the audio engine in the module notes, the architecture overview and the skills`.
