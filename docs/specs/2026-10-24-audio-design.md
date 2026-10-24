# Audio (5b) Design

## Overview

**Intent.** Ryo2D's audio should be something a professional game developer and audio designer would choose to work in, with nothing anyone could point to and call unprofessional. Audio is authored outside the engine, in a DAW such as Logic or Ableton. The engine's job is to play what the designer hands over the way they mean it to sound, and to let them shape it without a programmer. (Owner, 2026-10-24.)

**Why now.** The engine audit (`docs/research/2026-10-11-engine-audit.md`, Audio) and a fresh survey of `src/core/audio.rs` found a thin wrapper over kira 0.11:

- **Missing mixer features.** There are no buses, effects, spatial sound, musical clocks, loop points or voice limits.
- **Volume settings are baked in.** Category volumes are multiplied in at play time, so a player's volume slider never changes a sound already playing.
- **Engine state never reaches audio.** Pausing, time scale and losing window focus have no effect on it.
- **Paths and errors.** Paths are relative to the working directory, outside the asset registry, with no hot reload, and failures are silent.
- **No web build.** Every load returns an error on the web.
- **No way to test.** Headless runs have no device, and the unit tests are mostly arithmetic.
- **Music cannot be stopped.** `stop_music()` and `SceneCleanup::MUSIC` do not stop it, because a kira 0.11 sound handle does not stop when dropped. Only `play_music_once` stores its handle, and a later call layers a second copy.

**Who it serves.** First, the audio designer. They drop files exported from a DAW into `assets/`, describe how each sound plays in a small data file, balance the mix in one file, and hear every change while the game runs. Second, the game developer, who plays a sound by name with one call and never manages voices, buses or cleanup.

**Shape.** Two sub-projects (owner's choice):
- **5b (this design):** the playback core and the mix.
- **5c (later):** interactive music, meaning transitions on the beat or bar, layered stems, stingers, and game-driven parameters. 5c builds on 5b's clips, buses and clock.

## Goals

- **Registry assets.** Audio clips, sound definitions, music definitions and the mixer are registry assets, loaded by asset path, with hot reload. They follow the same lifetimes, events and error reporting as 5a's images.
- **A mixer.** It has a fixed standard bus layout (master, then music, sfx, ui, voice and ambience), and a game can add buses of its own under it. Bus volumes, effects and sends change live. Player volume settings drive the buses on a perceptual curve and act on sounds already playing.
- **Sound definitions.** Each one names its clips with weights, its bus, and a volume and pitch with random ranges. It also sets:
  - no-repeat;
  - a voice limit with a resolution rule, and a priority;
  - a cooldown;
  - looping;
  - default fades;
  - 2D spatial settings.
- **2D spatial sound.** A listener (by default the camera) hears emitters (entities). Volume falls off with distance on a chosen curve, stereo pan follows the horizontal offset, and an optional low-pass rises with distance. Sounds out of range are culled.
- **Game state reaches audio.**
  - Buses can pause with the game. By default effects, ambience and voice pause, while UI and music carry on.
  - Buses can follow time scale, so slow motion lowers their pitch.
  - Losing window focus does what the mixer says.
- **Snapshots and ducking.**
  - A snapshot is a named mixer preset (bus volumes and filter cutoffs) blended to over a time.
  - Ducking rules lower one bus while another plays, with attack and release.
- **Basic music.**
  - A music definition is an intro followed by a loop, or one file with a loop region.
  - Switching tracks crossfades.
  - Music is played by the engine, not by a scene, so it carries on across scene changes until it is stopped. Long files stream.
- **Clean sound.**
  - No click or pop on any start, stop, pause or volume change.
  - Loops are gapless.
  - Decoding never blocks a frame.
  - No voice is cut off by a less important one.
- **The web.** Audio works there. It loads from the registry's bytes and unlocks on the first user gesture.
- **Testable.** A headless engine runs a real mixer on kira's mock backend and records what played, so tests can assert on it.
- **One API, migrated.** One API replaces the old one. Every game, demo, theme and skill moves to it, and a new audio demo shows each feature.

## Non-goals (5b)

- Interactive music, which is 5c's work:
  - beat- or bar-quantized transitions and a musical clock API for gameplay;
  - transition tables, layered stems and stingers;
  - game-driven parameters (RTPCs), states and switches.
- Middleware integration (FMOD, Wwise), and sound-design or mixing tools. Designers author in their DAW and in text files (owner: not now).
- Virtual voices, a runtime loudness meter, a master limiter, true audio-rate side-chain, and 3D or HRTF audio.
- Recording and microphone input, and voice chat.

## Anti-goals (the professional bar)

Each of these counts as a failure even if every feature works.

**For the player:**
- a click or pop when a sound starts, stops, pauses or changes volume;
- a gap or seam in a loop, or between an intro and its loop;
- a frame hitch or an audio dropout from loading, decoding or reloading;
- a burst of identical sounds stacking into distortion;
- music restarting or layering on a scene change;
- volume lurching between sounds or scenes;
- drifting out of sync after a pause.

**For the developer:**
- a slider whose 50% sounds near-silent;
- a setting that does not reach sounds already playing;
- a silent failure, where a missing or broken sound says nothing;
- a web build with no sound;
- audio behaviour no test can check.

## Decisions

### The owner's decisions

- Professional quality as above.
- Authoring in an external DAW.
- No middleware and no tooling now.
- Two sub-projects, 5b then 5c.
- Upgrade kira (0.11.0 to 0.12.5).

### My calls (the owner may revisit them in review)

- **Data files are RON**, like UI files and themes:
  - sound definitions are `*.sound.ron`;
  - music definitions are `*.music.ron`;
  - the mixer is `*.mixer.ron`. The engine loads `audio/mixer.mixer.ron` if it exists, and otherwise uses the standard layout.
- **Game code** plays a sound with `ctx.audio().play(&handle)`, which returns a `Voice` handle used to stop, fade or adjust that playback.
- **Spatial sound** comes from an `AudioEmitter` component on an entity, heard by an `AudioListener`. The camera is the listener unless an entity carries the component.
- **Custom buses** go in the mixer file, under the standard five.
- **The old API** (`Audio`, `AudioManager`, `SoundOptions`, `MusicOptions`, `SoundHandle`, `MusicHandle`, and the `ctx.audio` field) is replaced outright. Breaking changes are fine.

## Design

### Assets

**`AudioClip`** is one audio file. 5b ships WAV, OGG Vorbis, MP3 and FLAC (kira's `wav`/`pcm`, `ogg`/`vorbis`, `mp3` and `flac` features). No other format is enabled, to keep the binary and the web build small. kira resamples to the device rate as it plays, so clips are not resampled at load. (my call)
- The loader probes the clip's length, sample rate and channels on the loader thread.
- A short clip is decoded to frames on that thread, giving kira's static sound data.
- A long clip keeps its file's bytes and streams from them when played (kira's streaming data from a cursor).
- The threshold is a loader setting with a default of about 10 s. A sound definition or music definition can force either mode.
- Decoding never runs on the main thread, except under `load_now`, which runs on the caller's thread as it does for images.

**`Sound`** (`*.sound.ron`) is a sound definition:
- **clips:** one or more, as asset paths. Each has an optional weight, an optional region (start and end), and an optional loop region.
- **bus:** a name. The default is `sfx`.
- **volume:** in dB, with an optional random range.
- **pitch:** in semitones, with an optional random range. Changing pitch also changes speed, which is kira's playback rate.
- **no-repeat:** how many recent picks to avoid.
- **voice limit:** `max`, plus a rule. The rules are:
  - `PreventNew`;
  - `StopOldest`;
  - `StopQuietest`;
  - `StopFarthest`;
  - `StopLowestPriority`.
- **priority:** from 0 to 255.
- **cooldown:** a minimum time between plays.
- **looping:** on or off.
- **fades:** a fade in, and a fade on stop.
- **spatial:** optional. It sets the minimum and maximum distance, the falloff curve (`Linear`, `Inverse` or `Log`), the pan width, and an optional distance low-pass, from no filter to a cutoff at maximum distance.

The clips are dependencies, so a sound is ready only when every clip is. A `Handle<AudioClip>` can be played directly too, with the defaults of an empty definition: bus `sfx`, 0 dB, no randomization and no limit.

**`Music`** (`*.music.ron`) is a music definition:
- an intro clip and a loop clip, or one clip with a loop region;
- its volume;
- its default fade-in and crossfade times;
- an optional tempo and meter, recorded for 5c and unused in 5b.

It is always on the `music` bus, or on a bus under it.

**`Mixer`** (`*.mixer.ron`) is the bus tree:
- Each bus has:
  - a default volume in dB;
  - effects in order, drawn from kira's filter, EQ, reverb, delay, compressor, distortion and volume and panning controls, with their parameters;
  - sends, which go to send buses with a level;
  - whether it pauses with the game;
  - whether it follows time scale;
  - its own voice limit.
- The file also holds the snapshots, the ducking rules, the focus-loss behaviour and the player-setting curve.
- The standard layout is master, then music, sfx, ui, voice and ambience. It always exists, so a file only adds to it or adjusts it.
- A game may name its own buses under any standard one, such as `sfx/footsteps`.

### The mixer at runtime

- **Buses.** Each bus is a kira sub-track built from the mixer asset, with send tracks for sends. The engine owns them for the life of the engine.
- **Volume.** Changes go through kira tweens, never jumps. The minimum smoothing is a constant, about 5 to 10 ms, and it applies to every volume, pan, pitch, pause, stop and effect-parameter change. This is what rules out clicks.
- **Player volume settings.** Master, music, sfx, ui, voice and ambience are stored in `SystemState`. There, they are values from 0 to 1, mapped to bus gain on the mixer's perceptual curve. The default curve is a dB range from -60 to 0 with a cubic or log taper, which is the builder's call to tune. Changing a setting tweens the bus live.
  - These six setting kinds are fixed, one per standard bus, with master being both a setting and a bus.
  - A game's own buses take their parent's setting. A game sets their gain with `set_bus_volume`, not with a player setting.
  - An old `system.json` without the new fields takes the `Ryo.toml` defaults, or 1.0 when `Ryo.toml` gives none.

  (my call)
- **Gain layers.** A bus's gain is its mixer default, times the player setting, times the active snapshot, times any duck. These are added in dB as separate layers, so a snapshot or a duck never overrides a slider, and a slider moved under a snapshot is heard at once. (my call)
- **Snapshots.** `ctx.audio().snapshot(name)` blends to the snapshot's targets over its blend time. `clear_snapshot()` blends back. One snapshot is active at a time; a new one blends from wherever the mix is.
- **Ducking.** A rule reads, for example: "while any voice plays on `voice`, duck `music` by -9 dB, attack 100 ms, release 600 ms." It acts on bus gain, driven by the engine each frame. It is not audio-rate side-chaining.
- **Pause.** When the game pauses (`Time::pause`), every bus marked to pause with the game pauses its voices, with the minimum fade, and resumes them after. The default marks are:

  | Bus | Pauses with the game |
  |---|---|
  | sfx | yes |
  | ambience | yes |
  | voice | yes |
  | ui | no |
  | music | no |

  A pause-menu snapshot, such as a low-passed and quieter music bus, is the designer's to define and the game's to call.
- **Time scale.** Buses marked to follow time scale (by default sfx and ambience) set their voices' playback rate to the game's time scale, tweened. Hitstop counts as time scale.
  - kira has no per-track playback rate, so the engine keeps a table of live voices and sets each following voice's rate itself.
  - The rate is the voice's own pitch times the time scale. `Voice::set_pitch` sets the voice's own pitch, and the time scale still multiplies it.
  - A time scale at or below a small threshold, such as hitstop's 0, pauses those buses' voices instead of setting a rate kira cannot play.
  - Pausing uses kira's track pause, so streams and loops resume from the same position.

  (my call)
- **Focus loss.** The mixer's `on_focus_loss` is one of `Keep` (the default), `Duck(dB)` or `Mute`. It applies a tween to master.

### Voices

- **Playing.** `ctx.audio().play(&Handle<Sound>)` and `play_at(&handle, position)`:
  1. pick a clip by weight, honouring no-repeat;
  2. draw the volume and pitch;
  3. apply the cooldown and the voice limit;
  4. start the voice on its bus.

  They return a `Voice`.
- **The `Voice` handle.** It offers `stop(fade)`, `set_volume`, `set_pitch` and `pause`/`resume`, all tweened, and `is_playing`. Dropping a `Voice` never stops it.
- **Ownership.**
  - A voice started by a scene belongs to that scene. When the scene exits, its voices stop with their stop fade.
  - A voice marked `persist`, and all music, belong to the engine and outlive scene changes.
  - A voice played from an `AudioEmitter` belongs to its entity. Despawning the entity stops its loops with their fade and lets its one-shots finish.
- **Voice limits.** They are enforced per sound and per bus, with each sound's resolution rule. A refused play returns a `Voice` that is already stopped, with no error.
- **Capacity.** The engine's global voice cap stays below kira's capacities. Above it, the lowest-priority, quietest voice is stolen with a short fade, so kira never returns a capacity error.
- **Unready sounds.** Playing a sound that has not loaded yet, or has failed, is a no-op returning a stopped `Voice`. A failed one warns once per id. The asset error, with its path, is the registry's.
- **Voice details** (my call):
  - **Clips stay alive.** A live voice holds strong handles to its clip, so the registry never frees a clip under playback. A streamed clip's bytes are shared, so the stream keeps them alive.
  - **Stability.** A `Voice` stays valid when its sound or clip reloads.
  - **Bus scope.** `voices_on(bus)` counts the bus's whole subtree, and so does a bus's voice limit.
  - **`StopFarthest`** treats a voice without a position as distance 0, so among those it falls back to `StopOldest`.
  - **A removed bus.** When a mixer reload removes a bus, its voices move to the parent bus.

### 2D spatial sound

- **`AudioListener`** is a component. With none present, the active camera's position is the listener.
- **`AudioEmitter`** is a component holding a sound handle, plus play-on-spawn and loop settings. `ctx.audio().play_at(&sound, pos)` plays a one-shot at a world position.
- **Each frame**, the engine updates every spatial voice's gain, pan and low-pass cutoff from its offset to the listener and its sound's spatial settings. Each update is a tweened parameter change.
- **Range.**
  - A one-shot that starts beyond its maximum distance is culled, so it never starts.
  - A loop beyond range keeps running, silent, and fades back in when it comes into range.
  - kira's 3D spatial tracks are not used, because 2D needs only gain, pan and filter.

### Music

- **The engine's one music player.**
  - `ctx.audio().play_music(&Handle<Music>)` crossfades from the current track over the music's crossfade time. Playing the track already playing is a no-op.
  - `stop_music(fade)` and `pause_music`/`resume_music` control it.
  - `current_music()` reports what is playing.
- **The loop.** An intro plays once and then its loop repeats with no gap: the loop clip is scheduled at the intro's end, sample-accurate on kira's clock. A single clip with a loop region loops inside its region.
- **Streaming.** Music streams by default.
- **Scene changes.** Music carries on across them, because it belongs to the engine.

### Hot reload

Hot reload follows the registry, with the same `Modified` and `ReloadFailed` events and the last good value kept.
- **A reloaded clip.** Playing one-shots finish on their old data. Playing loops and music switch to the new data at the same position, with a short crossfade. New plays use the new data.
- **A reloaded sound definition.** It applies to new plays. Its playing loops take its new volume and pitch live.
- **A reloaded mixer.** It applies live. Bus volumes and parameters tween. Changed effect chains are rebuilt with a short fade on that bus.

### The web

- **Loading.** Clips load from the registry's sources as bytes, so there is no file-system access. Streaming reads from those bytes.
- **The unlock.** The audio context starts suspended until the first user gesture (a key, click or touch). Until then, one-shot plays are dropped, and music and loops are started at the unlock.
- **The worker pool.** It is absent on wasm, so decoding runs in the registry's pump within its budget, as images do.
- **In scope as working code.** The web build is in 5b's scope as working code, not only as a design. If kira 0.12's cpal backend cannot resume a suspended context, a small engine-side wasm shim is acceptable. The final check is by hand in a browser at hand-play. (my call; the owner approved the bar "the web works, with sound")

### Headless and tests

- **No device.** A headless engine, or one with no device, runs the real mixer on kira's mock backend (`kira::backend::mock`). Everything works: no call returns "unavailable", and nothing plays aloud.
- **Driving the mock backend.** The engine processes the mock backend each frame for that frame's elapsed time, in samples at the mock sample rate. A `HeadlessEngine`'s fixed step therefore makes it deterministic, and its clock, tweens and sample positions advance as they would on a device. A device that fails to open falls back to the mock backend for the session, silent, with one warning and no retry. (my call)
- **The audio log.** The engine keeps a `#[doc(hidden)]` log of each voice. It is a bounded ring that is off by default; `HeadlessEngine` and debug tooling turn it on, so release games pay nothing for it. Each entry records:
  - the voice's start and stop, with their fade times;
  - its sound, its clip pick, its bus, its gain, its pan and its pitch.

  `HeadlessEngine` exposes it, so tests can assert:
  - that the 4th of 3 limited plays was refused;
  - that a pause paused sfx and not ui;
  - that a slider moved a playing voice's bus;
  - that a scene exit stopped its loops with a fade.

### Errors

- Every audio failure is an asset error with a path: a missing file, an undecodable file, or a definition naming a missing clip. The registry reports it once, as for images.
- A device that cannot be opened falls back to the mock backend, with one warning.

## Public API and migration

- **`ctx.audio()`** returns an `AudioView`, the way `ctx.assets()` does. It gives:
  - `play`, `play_at`;
  - `play_music`, `stop_music`, `pause_music`, `resume_music`, `current_music`;
  - `snapshot`, `clear_snapshot`;
  - `set_bus_volume(name, dB)`;
  - `set_volume_setting(kind, 0..1)`, `volume_setting`;
  - `voices_on(bus)`.

  The builder names the methods to match the engine's style.
- **Exports.** The prelude exports `AudioClip`, `Sound`, `Music`, `Mixer`, `Voice`, `AudioEmitter`, `AudioListener` and the setting kinds.
- **Migration.**
  - The four games with audio (breakout, bullet-hell, flappy, leap-king) move to sound and music definitions under their `assets/` folders.
  - leap-king's hand-rolled pan-and-volume "spatial" helper becomes an `AudioEmitter`. It also fixes a bug: `.volume(0.05)` overwrote its own distance attenuation.
  - breakout's handle-passing crossfade becomes `play_music`.
  - The pause menus use the volume settings.
- **Theme UI sounds** (`UiSounds` in the theme) become `Handle<Sound>`s by asset path, in code and in `.theme.ron`, on the `ui` bus.
- **Settings.** `SystemState` gains ui, voice and ambience volumes, keeping master, music and sfx. `Ryo.toml` defaults follow.
- **A new demo,** `examples/demos/audio`, follows the demo-kit standard, with pages for:
  - buses with live sliders;
  - variations and no-repeat;
  - voice limits with each rule;
  - spatial emitters around a listener;
  - pause, time scale, snapshots and ducking;
  - music intro and loop, and crossfades;
  - hot reload.

  It ships with small audio files under licences the repo allows.

## Acceptance

### Correctness

Each of these is pinned by a test, headless on the mock backend unless marked.
- `stop_music` and scene exit really stop, with a fade. The old bug never comes back.
- A volume setting changed while a sound plays changes that sound.
- Pausing the game pauses `sfx`, `ambience` and `voice`, but not `ui` or `music`. Resuming continues from the same position.
- Time scale 0.5 halves the playback rate of the buses that follow time scale.
- Each voice-limit rule chooses the right voice. Global capacity steals the lowest-priority voice and never errors.
- With two clips and no-repeat 1, no clip plays twice in a row.
- Spatial gain, pan and cutoff match the falloff curve at given offsets. A one-shot out of range is culled.
- An intro-then-loop has no gap. The loop starts sample-exactly at the intro's end, checked from the mock backend's sample clock.
- A crossfade between two tracks overlaps by the crossfade time.
- Music survives a scene change.
- A reloaded clip, sound definition and mixer each take effect as in Hot reload, with `ReloadFailed` and the last good value kept on a broken save.
- Every stop, pause and volume change uses at least the minimum smoothing, read from the audio log.
- An undecodable file fails with its path. A missing device falls back to the mock backend with one warning.
- The web build loads and plays after the first gesture. This is verified by hand in a browser, because CI has no browser audio.

### Targets

All are measured against main before 5b, as in 5a. They are hard targets with guards, as in 5a: a miss is reported with a profile and goes to the owner, and no target is loosened (my call, following 5a's rule). `play` may allocate. Idle frames must not.
- An idle frame, with no voices, is within 3%.
- A frame with 64 voices playing, 32 of them spatial, costs at most 50 µs of main-thread time for audio.
- A `play` call costs at most 5 µs of main-thread time.
- No frame allocates for audio while idle.
- A clip's load or reload adds at most 0.5 ms to any frame, with decoding off the main thread.
- Output latency is reported for the default desktop configuration, and the buffer size is a `Ryo.toml` setting.

## Documentation

- A new module note for audio.
- The `ryo-audio` skill is rewritten to cover the workflow from DAW export to sound definition, the mix, spatial sound, pause, music and hot reload.
- The `ryo-api` rows, `ryo-assets`' types table and the architecture overview's audio section are updated.
- Each skill's copy stays byte-identical.

## Risks

- **kira 0.12's cpal on the web.** The unlock and suspended-context behaviour needs a browser check. If kira cannot resume the context, the engine needs a small wasm shim.
- **Per-voice parameter tweens.** Their cost with many spatial voices is unknown. The 64-voice target guards it.
- **Gapless intro-to-loop on streams.** Scheduling a streamed loop exactly at a streamed intro's end depends on kira's clock and stream start latency. If streaming cannot meet it, the intro and loop decode to memory.
- **Licensing of demo audio.** It must be free to ship (CC0 or similar).

## Out of scope

- 5c: interactive music, as in Non-goals.
- Later:
  - virtual voices;
  - loudness metering and a master limiter;
  - a dev-time loudness check of assets;
  - middleware;
  - tools;
  - 3D audio.
