# AGENTS.md - Audio Module

The audio engine: kira 0.12.6's manager on the audio device (or on kira's mock backend), the bus tree a mixer asset describes, the live voices and their limits, 2D spatial sound, the one music player, and what follows the game clock, hot reload and the web's first gesture. Clips, sound definitions, music and the mixer are registry assets (asset module notes, The asset types). The spec is `docs/specs/2026-10-24-audio-design.md` (where it and the code differ, the code is current); what it costs and the output latency are measured in `docs/backlog/audio.md`, with the measurements behind the stream decoder. Using it from a game is the `ryo-audio` skill; the worked example is `examples/demos/audio`.

## Module map

| File | What |
|---|---|
| `mod.rs` | The exports; `src/lib.rs`'s prelude takes the game-facing ones |
| `clip.rs` | `AudioClip`: `ClipData::Decoded` (a kira `StaticSoundData`) or `ClipData::Encoded` (the file's bytes and its frame count, `EncodedClip`); `DECODE_THRESHOLD` (10 s) |
| `stream.rs` | `ClipDecoder`, the engine's kira `Decoder` over symphonia for streamed clips, and `probe` (the loader's header read). Native only |
| `sound.rs` | `Sound` (a `.sound.ron` definition), `SoundClip`, `VoiceLimit`, `LimitRule`, `Spatial`, `Falloff` |
| `music.rs` | `Music` (a `.music.ron` definition), `MusicData`, `join` (an intro and its loop as one sound), `MusicJoinError`, and `MusicPlayer`, the engine's one player |
| `mixer.rs` | `Mixer` and its parts (`BusDef`, `EffectDef`, `SnapshotDef`, `SnapshotBus`, `DuckDef`, `FocusLoss`), its parse and check, `MIXER_PATH`, `default_mixer`; `plan` (the build order) and `BusTree` (the kira tracks); `GainLayers`; `BusReport` (doc-hidden) |
| `mix.rs` | The mix's controllers: `Blend` (the active snapshot and the blend toward it) and `DuckState` (a ducking rule's dip) |
| `settings.rs` | `VolumeKind`, `volume_to_db` (the curve), `VolumeSettings` |
| `spatial.rs` | `AudioListener`, `AudioEmitter` and their hooks and observers, the falloff, pan and cutoff maths, `ListenerQuery`, `SpatialVoice`, `VoiceFilter` (a voice's own low-pass track) |
| `voice.rs` | `Voice` (the game's handle), `VoicePick`, and the state and commands it shares with the engine |
| `view.rs` | `AudioView` (`ctx.audio()`), `PlayOptions`, `Playable` |
| `engine.rs` | `AudioEngine`, the resource: the kira `Manager` (device or mock) and the web's `Unlock`, the voice table, the play path and its limits (`Room`), the holds, reloads, scene release (`in_scene`, `release_scene`, `stop_scene_music`) and `unlock` |
| `system.rs` | `audio_system` (doc-hidden), the frame's audio work |
| `log.rs` | The audio log: `AudioLogEntry`, `AudioLogKind` (doc-hidden, for tests and debug tooling) |

The loaders are the asset module's: `asset/loaders/audio_clip.rs` (`AudioClipLoader`, `AudioClipSettings`, `ClipMode`), `sound.rs`, `music.rs` and `mixer.rs`.

## The frame's audio work

`audio_system` runs in `PostUpdate`, after `PostUpdateSet::Camera`: the scenes' updates and their systems have queued their plays and voice commands, and the cameras (the default listener) and transforms are where this frame leaves them. It takes the `AudioEngine` out of the world (`try_resource_scope`) while it reads the stores, and in order:

1. counts the frame and adds the real delta to the engine's clock (cooldowns run on it, so a paused game still cools down);
2. applies the voices' queued commands (stop, pause, resume, volume, pitch), in the order they were sent;
3. builds the bus tree again if the mixer landed or changed (and applies a snapshot asked for before it landed);
4. follows the registry's `AssetEvent`s for clips, sound definitions and music through its own `MessageCursor`s: switches what plays to reloaded data, and forgets what it remembered of a freed asset;
5. steps the music player: a track that finished loading takes over;
6. releases the voices whose emitter went (despawned, disabled, taken away or put over);
7. finds the listener and brings every spatial voice's gain, pan and cutoff in line with its place;
8. plays the emitters queued to play (each entity once a frame);
9. follows the game clock: holds and lets go the buses that pause with the game or follow time scale, and sets the following voices' rates;
10. warns once about an error the device's stream reported;
11. renders the frame's real time on the mock backend, in whole 128-frame chunks at 48 kHz (the remainder carried, at most 1 s a frame);
12. retires the voices that ended (a failed stream warns once here), drops the music track whose voice ended, steps the snapshot blend and the ducks onto the bus gains, and drops a retired tree's send tracks once no voice plays on it.

A `Voice`'s methods only queue a command (a crossbeam channel) and read an atomic state, so a `Voice` is `Send`, cheap to clone, and usable anywhere; what `is_playing` reports changes by the next update. Plays themselves run at once in the caller (`AudioView::play*` takes the engine out of the world for the call), so the voice returned is live, or already stopped if refused.

## Playing

The play path (`AudioEngine::play`): resolve the definition (a clip played directly takes the empty definition: `sfx`, 0 dB, no spread, no limit, priority 128), refuse it if not loaded (a failed one warns once per id), refuse it inside its cooldown, pick a clip by weight avoiding the last `no_repeat` picks (never every clip), draw the gain and pitch from the ranges, resolve the bus (a missing one plays on the nearest bus above it, warned once per name, but not while the mixer that may add it is still loading), hear it from its place (a one-shot at or beyond its maximum distance is culled), plan the room (below), check kira's room on the track, start it (static or streamed), stop the planned victims, and log the start.

- Ranges and clamps: a voice's own gain is -60 to +24 dB (kira is silent at -60), its pitch -48 to 48 semitones; a rate is `2^(semitones/12)`.
- **A live voice holds strong handles to what it plays** (its clip, or its music), so the registry never frees a clip under playback, and a scene may drop its handle while the sound plays out. A streamed clip's bytes are an `Arc` the stream shares.
- A refused play returns `Voice::stopped()`, one shared static, so a refusal allocates nothing; it is logged `Refused` (`Culled` for an out-of-range one-shot) and, by design, not warned about (a burst against a limit is normal).
- Each definition's state (its recent picks, its last start for the cooldown) is kept by id and pruned on its `AssetEvent::Removed`.
- `PlayOptions` adds to the definition: `position`, `persist`, `volume_db`, `pitch`.

## Ownership and scene exits

- A voice belongs to the scene whose code played it. `SceneSlot::call`, the scene systems and the delivery of a scene's UI node events run under `in_scene(world, scope, ..)`, which sets the engine's `scope`. A voice played with `persist`, or outside every scene (a global system or observer), is the engine's; one played from an `AudioEmitter` is its entity's.
- **A scene's exit** (pop, switch, clear, reset; the manager calls `release_scene` at each) stops its loops and its one-shots paused or held by the game's pause, each with its sound's `fade_out` (at least 10 ms), and lets its one-shots that are playing finish. A confirm sound played just before a switch is heard out; a paused one-shot never resumes over the next scene when the clock does. A long one-shot (a stream played as a one-shot) therefore outlives its scene by its length.
- **Music is the engine's**: no scene's exit stops it. `SceneCleanup::MUSIC` (not in `DEFAULT`) stops it through `stop_scene_music`, fading over the track's crossfade, when the scene whose cleanup holds it is switched away from or reset: cleanup runs only in `SceneManager::switch` and `reset`, never on `pop`, so a pushed scene that plays a track of its own stops it in its `on_exit` (`stop_music`), or the scene below asks for its track again in `on_resume`.
- **An emitter's voices** are released by the same rule as its entity is despawned, disabled (a pool putting it back), loses its `AudioEmitter` or has another put over it. The `on_discard` hook lets them go at once (`emitter_gone`, released in the next `audio_system`), so an emitter put over it, or a pooled entity put back and handed out in one frame, starts afresh rather than taking the old voices.

## Voice limits, priority and the cap

- **Priority is 0 to 255, higher more important** (Unity's convention is the opposite), 128 by default (`Sound::DEFAULT_PRIORITY`). Music plays at 255, and only a music play may stop music.
- A play is checked against its definition's limit (its voices on its bus's subtree), then each bus limit from its bus up to `master` (every voice on that bus's subtree), then the cap on its bus's own track. The definition's limit and the bus limits stop voices by the definition's rule; a bus limit when the definition has no limit, and the cap, stop the least important voice, then the quietest (as heard, falloff included), then the oldest. `PreventNew` refuses.
- **Who may be stopped:** never a more important voice than the new play, and never an equally important one started in the same frame. Without the same-frame rule a burst (a hundred plays in one frame) would stop its own starts in turn, so kira starts and stops sounds nobody hears; with it, the burst refuses past the limit. A more important play may still take a lower-priority voice started this frame.
- **Victims are planned before any stop** (`Room`, into `victims`): a play that one limit would make room for but a later limit refuses stops nothing. A rule's victim stops with its own sound's `fade_out` (at least 10 ms), the cap's with 10 ms; each is logged `Stolen`.
- **The cap is 96 voices per bus track** (`VOICE_CAP`). A kira track holds a fixed 128 sounds (`TrackBuilder::sound_capacity`, kira's default), and a stopped sound holds its place until its fade ends, so the 32 above the cap are room for stolen voices to fade out; kira never returns a capacity error in normal play. A voice fading out no longer counts toward a limit or the cap, but it holds its kira place: many long `fade_out`s on one bus can fill the track, and plays there are then refused, warned once per bus.
- `StopFarthest` counts a voice with no position as at the listener (distance 0), as it does every voice while there is no listener, so among those it stops the oldest.
- There are no virtual voices: a loop stolen by a limit or the cap does not come back when room frees.

## The bus tree and the mix

- **A bus is a kira track** (`BusTree`, built from `plan(mixer)`: the standard six first, then the game's by depth). `master` is a sub-track of kira's main track at 0 dB; master's gain goes on the main track, so send returns, which kira mixes into the main track beside `master`, are under it too. The standard buses always exist; a mixer adjusts them and adds up to 64 of its own (`MAX_BUSES`), 16 of them send buses at most (`MAX_SEND_BUSES`; kira's send-track room is set to 64, `SEND_TRACK_CAPACITY`, so retired trees' send tracks fit beside the live one's). kira holds 128 sub-tracks per track (its default), and the engine's tracks nest (each tree's `master` under the main track, the standard buses under `master`, a game's buses under theirs, voice filter tracks under their bus), so no parent nears it: a rebuild adds one `master` under the main track for as long as the retired tree plays out.
- **Gain layers** (`GainLayers`) add in dB: the mixer's volume (or the game's, `set_bus_volume`), the player's setting on the curve (standard buses only; a game's bus inherits through its parent), the active snapshot's, the duck's, and on `master` the focus loss's; the sum is at most +24, and silence if any layer is silent (a setting of 0 is silent whatever the others add). A snapshot or a duck never overrides a slider, and a slider moved under a snapshot is heard at once.
- **Sends** go to send buses (a bus another sends to): a kira send track, which plays no voices (a sound on one plays on its parent, warned), has no children and sends nowhere. kira takes a send after the sender's own volume and before its parent's, so each route's volume is its level plus the gains of the buses above the sender, except `master`'s (on the main track, over every return): a return follows its sender's sliders whichever send bus it reaches, and is tweened with `set_send`. `master`'s effects do not process returns, since kira mixes send tracks beside `master`'s track.
- **Snapshots** (`Blend`): one active at a time, blended from wherever the mix is over the snapshot's `blend` (0.3 s by default). A snapshot is a full preset: a bus it does not name, or names without a `low_pass`, blends back to 0 dB and open. dB blend in a line and cutoffs in a line of octaves (how a sweep sounds even), stepped once a frame with each step tweened over the frame (`glide`: at least 10 ms). A bus any snapshot names a `low_pass` for gets a low-pass on its track at build, open at 20 kHz (`OPEN_HZ`) until a snapshot closes it. The blend is kept by bus name, so the active snapshot survives a rebuilt tree; a rebuild whose mixer lacks it blends back to open. A snapshot asked for before the mixer lands waits for it.
- **Ducks** (`DuckState`): while any voice plays on the trigger's subtree, the target's duck layer moves to `by_db` over `attack`, and back to 0 over `release` after. It is bus gain stepped each frame, not audio-rate side-chaining; two rules on one target add their dips.
- **Focus loss** reaches the engine from `WindowEvent::Focused` (`engine/window.rs`): the mixer's `on_focus_loss` (`Keep` by default, `Duck(dB)`, `Mute`) is a layer on `master`, smoothed over 10 ms.
- **A rebuild** (a mixer landing, changing, or set in code with `AudioView::set_mixer`; the web's unlock) builds a whole new tree and retires the old one. kira cannot move a sound between tracks and fixes a track's effects and sends when it builds it, so loops restart on the new tree at their position, crossfading over 10 ms, onto the bus their definition names (a clip's onto its old bus's name, or the nearest above either), while one-shots play out on the old tracks: every track is built with `persist_until_sounds_finish(true)`, so kira keeps a retired track until its sounds finish once the engine drops its handle. A retired tree keeps the volumes it had (only `master`, on the main track, still reaches it) and its send tracks, held by generation in `retired_sends` until its last voice ends. A loop that cannot move is warned about once and plays on where it was. A mixer that fails its check keeps the tree as it is, with a warning. A rebuild happens only in `audio_system` (`apply_mixer`), so a mixer loaded with `load_now` in `on_enter` is built at the end of that frame: a one-shot played on one of its buses earlier in the frame plays out on the nearest bus above (the bus did not exist yet, and one-shots never move), while a loop moves onto it at the rebuild.
- `voices_on(bus)` and every bus limit count the bus's whole subtree.

## The game clock

- **Holds are per voice**, not kira's track pause: a bus that pauses with the game holds its voices while `time.game` is paused, and one that follows time scale while the scale is at or below 0.01 (`HOLD_SCALE`; pause counts as 0). Each held voice is paused in kira where it is, over 10 ms, and resumed from there. A kira track pause would freeze a game's bus under it that should play on, keep a stopped voice's fade from finishing (a released loop would fade out over the next menu instead of at once), and cannot reach a retired tree's tracks.
- A hold is the engine's and leaves the voice's own `pause` alone: a held voice still reports playing, and one paused by its own call stays paused when the game resumes. The voices' own pauses and resumes reach kira in the same pass, so each voice gets at most one pause or resume a frame.
- **A voice played while its bus is held** starts paused and silent, and its first resume takes its own fade-in (kira's resume would replace a fade-in still to run). That covers the frame the clock resumes in, such as a `Reset` from a pause menu (resumed in `SceneCleanup::TIME`, before the next scene plays), while the buses are still held from the frame before.
- **Time scale**: a voice on a bus that follows it plays at its pitch's rate times the game's speed, capped at 8 (`MAX_SCALE`: the game clock allows 100, which would swamp kira's mixing thread), gliding over the frame. A held voice keeps its rate.
- **Hitstop does not hold audio** (`GameTime::of` leaves it out): it is a visual freeze, and the hit's sound playing through it is the impact.
- Defaults: `sfx`, `ambience` and `voice` pause with the game; `sfx` and `ambience` follow time scale; `ui` and `music` do neither, so menus and music carry on under a pause. A game's bus does as its parent unless the mixer says.

## Spatial sound

- kira's 3D spatial tracks are not used: 2D needs only gain, pan and a filter, each a tweened change on the voice.
- The listener is the first `AudioListener`'s `GlobalTransform`, else the `MainCamera`'s, read in `audio_system` after the cameras, as the fixed steps and propagation left them (uninterpolated). Its rotation turns which way its right is. With none, every voice is heard as if at it. `ListenerQuery` keeps its queries, so a frame's search allocates nothing.
- Each frame a spatial voice's gain, pan and cutoff follow its offset: `falloff_gain_db` (Linear: amplitude in a line, -6 dB halfway; Inverse: `(1 - t) / (1 + 15t)`, the inverse-distance law of a source in the open heard from a fifteenth of the span, tapered to silence, about -25 dB halfway; Log: even in dB, -30 dB halfway; silent at the maximum), `pan_for` (`dx / pan_width`, -1 to 1), and the distance low-pass (open within the minimum distance, closing in even octave steps to `low_pass_at_max` at the maximum). A change smaller than can be heard is not sent (gain, pan and cutoff thresholds), so still voices cost nothing.
- **A distance low-pass needs a kira track of its own** (kira's effects are per track): a `VoiceFilter` sub-track under the bus's track, at most 32 at once (`MAX_FILTERED`); a voice past them plays unfiltered, its gain and pan still following.
- A one-shot at or beyond its maximum distance is culled (never starts); a loop there starts silent and is heard as the listener comes near. A sound with no `spatial` played at a place plays as if at the listener, warned once.
- **Emitters**: `AudioEmitter`'s `on_insert` hook queues a play-on-spawn unless its entity is `Disabled` (a pool's); the world's first emitter adds observers of `Add<Disabled>` (let its voices go) and `Remove<Disabled>` (queue a play-on-spawn). A recycled entity of a `RecycleOldest` pool is disabled and enabled again (ecs module notes, Pooling), so its emitter stops and plays as a released and acquired one does. A queued entity plays once a frame, once its sound has loaded. Emitter voices follow the entity's `GlobalTransform` each frame.

## Music

- One player (`MusicPlayer`): the current track (playing, paused, or fading out until its voice ends) and a track asked for while it loads, which takes over once loaded while the current one plays on. Asking for the track already playing changes nothing (a paused one resumes), so every scene can ask for its track as it enters. A failed request changes nothing, not even a track still loading, and warns once.
- A switch fades the current track out and the new one in over the new track's `crossfade` (1 s by default); with none playing, it fades in over its `fade_in` (0 by default). `stop_music(fade)` fades over at least 10 ms and forgets a track still loading.
- **An intro and its loop are joined into one sound** (`join`, on the loader's thread): kira starts sounds and applies start times once per 128-frame chunk, so a loop scheduled at the intro's end cannot be sample-exact, while one sound whose loop region starts on the frame after the intro's last is. Both are decoded (`mode: Stream` is refused for them) and must share a sample rate. One clip with a loop region loops on kira's loop region and streams when long.
- A track plays on `music` or a bus under it; a definition naming another bus plays on `music`, warned once. `tempo` and `meter` are recorded for interactive music and not read by the player.

## Clips and streams

- The loader decodes a clip of up to 10 s (`ClipMode::Auto`) to a `StaticSoundData` on its thread, so a play costs a clone of an `Arc`. A longer clip keeps its encoded bytes and the sample rate and frame count its header gives (read by `stream::probe`, which also refuses anything but mono or stereo), so music does not sit decoded in memory; each play streams it. A definition's `mode` forces either. Mono and stereo only: kira's frames are stereo and its decoding refuses any other layout (`UnsupportedChannelConfiguration`), so the loader refuses one up front, streamed or decoded, rather than at play.
- **Streams play through the engine's `ClipDecoder`**, not kira's own decoder: making one stores only the bytes, the rate and the frame count, and the probe, the codec's setup and the seek to the start position wait for its first `decode`, on kira's decoder thread. A streamed play therefore costs the main thread only kira's buffers and its thread's spawn (about 24 µs measured), where kira's own decoder opens the file on the calling thread (0.3 to 1.2 ms for an OGG). Its frames are exact after a seek (each packet's frames placed by timestamp, those before the target dropped, with a preroll for MP3), and it always gives the window's frame count (a file that ends early is padded with silence), so a stream started partway (a reload switch, a rebuild) ends, and a loop wraps, at the file's end: kira's decoder miscounts frames after a Vorbis seek and its decoder thread spins at the end instead.
- A streamed clip's `region` is the decoder's window, and kira's slice is left unset: kira seeks a sliced stream's decoder by the slice's frames but asks it for the file's, so a region far into a long file would decode everything before it first.
- **A stream fails as it plays, not at `play`**: a file that cannot open fails on kira's decoder thread, its voice ends (stopped with no fade, as kira marks a failing stream stopped at once), and the clip is warned about once by path and refused until it changes. A packet the codec cannot decode (`DecodeError` or a per-packet `IoError`) is skipped as silence; a format reader's error other than the end of the stream fails it.
- kira is at least 0.12.6, which ends a stream dropped without being stopped (`StreamingSound`'s `Drop` marks it stopped); 0.12.5 leaks its decoder thread, as when an engine or an audio world is dropped with a stream playing.
- A header's length can be an estimate: an MP3 with no Xing, Info or VBRI tag gets one from its bitrate, so such a streamed loop can wrap after a short silence or cut its end; a FLAC without `total_samples` loads only with `ClipMode::Decode`.

## Hot reload

- **A clip** (saved, or changed with `Assets::get_mut`): its playing loops and the music playing it switch to the new data where they are, crossfading over 10 ms (a decoded clip starts at once; a streamed one starts a new stream at the old position); its one-shots finish on the old data; new plays use the new.
- **A sound definition**: applies to the plays after; its playing loops take the change in its volume and pitch as a delta (each keeps the spread it drew, and a volume or pitch the game set on the voice stays the game's) and stay on their bus.
- **Music** (its file, its clip, or its intro or loop file, which the loader reads with `LoadContext::read`, so their change reloads the music): the track switches, where it is, to what the saved definition says, crossfading over 10 ms. Each switch starts it again where it is, so a `get_mut` every frame would restart it every frame.
- **The mixer**: a rebuild (above).
- A save that fails to load or check changes nothing (`ReloadFailed`, the last good value kept).

## The web

- **The device opens at the player's first gesture**: a key pressed (not Escape, which browsers do not count as activation), a mouse button, or a touch lifted, and where the browser reports it only while `navigator.userActivation.isActive`. `window_event` calls `audio::unlock`, which makes kira's manager inside the gesture's event (winit dispatches it in the DOM handler's stack): kira keeps the browser's audio context to itself, so one made outside a gesture could never be resumed. A gamepad never unlocks it (browsers grant no activation for pad input).
- Until then the engine has no manager: one-shots are refused (logged), and loops and music wait as `Playback::Waiting` voices, started at the unlock by the rebuild onto the new manager, each fading in over its own fade-in. `AudioView::waiting_for_gesture()` tells the game, so it can show "click or tap to start".
- The web has no streaming and no worker threads: every clip decodes, inline in the asset pump (one job a frame at least), so a long clip is a one-off hitch of its whole decode (a 30 s OGG is about 15.5 ms natively, more on wasm) and holds its decoded frames (about 23 MB a minute at 48 kHz stereo).
- The web has no files: a web game passes its assets as its source (`EngineBuilder::asset_source`, a `MemorySource` of embedded bytes). The engine looks for the mixer with `AssetSource::exists(MIXER_PATH)`, so the source must hold it there, or the game sets one with `set_mixer`. `audio_buffer_frames` does not apply (the browser picks the buffer).

## The device and the mock backend

- **Native start**: `AudioEngine::open(device, audio_buffer_frames)` opens the default output device through kira's cpal backend (`cpal-realtime` raises its thread's priority), its buffer fixed at `audio_buffer_frames` (`Ryo.toml [system]`) cut to the device's supported range when given; a device that refuses the size opens with its default, warned. A smaller buffer cuts latency and costs more callbacks, with more risk of underruns on a busy machine; the measured trade is in `docs/backlog/audio.md` (512 frames at 48 kHz, the default here, is heard 12.9 ms after its callback; 256 frames, 7.6 ms). When the default device changes, kira reopens its stream with the same fixed buffer, and a new device that refuses that size stops the sound for the session (kira's restart, not the engine's), so a game leaves it unset unless it needs the latency.
- **No device**: a headless engine (`HeadlessEngine`, `RYO_HEADLESS`), or a device that fails to open, runs the same mixer on kira's mock backend at 48 kHz: everything works, nothing plays aloud, and a failed device warns once with no retry. The mock renders inside `audio_system`, on the frame's thread, by the frame's real time, so a headless run's clock, tweens and sample positions advance as on a device (one fixed timestep a step). `mock_render_time()` (doc-hidden) is the last frame's rendering, which the benches leave out of the main thread's time.

## Determinism and testing

- Plays pick clips and draw gains and pitches from a `SmallRng`, seeded from entropy, except headless: `Engine::new` seeds it with a fixed value (`HEADLESS_SEED`), so a headless run's variations repeat run to run. `AudioView::seed(u64)` seeds it for replays, tests and captures. `Voice::pick()` reads what a play picked and drew (the clip's index and id, its gain and pitch as started).
- **The audio log** (`log.rs`, doc-hidden): a ring of the latest 4096 entries (held up to twice that and halved at once, so a push is cheap), off by default so a game pays nothing. `HeadlessEngine` turns it on before its start scene enters (`audio_log()`), as does `testing::audio_world`. Each entry has its kind (`Start`, `Stop`, `Pause`, `Resume`, `Gain`, `Pan`, `Rate`, `Refused`, `Culled`, `Stolen`), the voice's id (0 for a refused play, or a bus's change), its sound's and source's paths, its bus, gain as heard, pan, rate and fade. Tests assert on it that a limit refused, a pause held `sfx` but not `ui`, a slider moved a playing bus, a scene exit stopped its loops with their fade.
- **Test worlds** (`testing/mod.rs`): `audio_world()` is `asset_world()` plus a `Time` and an engine on the mock with its log on, and is not seeded (call `AudioView::seed`); `audio_world_over(files)` reads a `MemorySource` the test keeps, to save over a file and `reload` it; `audio_step(world, dt)` is one frame (clock, pump, `audio_system`, then `clear_trackers`, as the engine's `Last` does: taking the engine out of the world writes a removal message that only clearing reuses).
- **Guards**: `tests/audio_alloc.rs` counts an idle frame's allocations (0, fresh and after sounds played); `benches/audio.rs` holds 64 voices (32 spatial with a low-pass) to 50 µs of main-thread time a frame, `play` to 5 µs, a streamed `play` to 100 µs and every reload landing to 0.5 ms (measured: 3.6 µs, 0.71 µs, 24 µs, at most +119 µs).

## Conventions

- **Every change is smoothed by at least 10 ms** (`MIN_FADE`): a voice's volume, pitch, pause, resume and stop, a bus's gain, a cutoff, a send. Shorter jumps click. The one exception is a stream that fails (it cannot open, or its file's reader errs), which kira stops at once.
- **An idle frame allocates nothing for audio**: the frame's lists (emitters spawned, orphaned voices, victims) are kept and reused, the listener's queries are kept, and a refused play shares one stopped `Voice`.
- **Every failure says so once**: a missing or broken file is the registry's `AssetError` with its path, warned once; the engine warns once per definition that did not load, per bus name that does not exist, per snapshot name, per clip played at a place without spatial settings, per undecodable clip version, per full bus. Refusals by limit, cooldown or culling are logged, not warned.
- **Ids, not handles, in what the engine remembers** of an asset (definition state, warned sets), except what a voice plays, which it holds on purpose; per-definition state is pruned on `Removed`.
- `AudioView` and the emitter hooks do nothing in a world without an `AudioEngine` (one the engine did not make): a play there returns a stopped voice, so code that plays sounds runs in any test world.
- Engine constants live beside their use with the reason in their rustdoc (`MIN_FADE`, `VOICE_CAP`, `HOLD_SCALE`, `MAX_SCALE`, `MAX_FILTERED`, `DECODE_THRESHOLD`); a change to one is a change to the documented behaviour, so the `ryo-audio` skill and the rustdoc change with it.

## Known limits

- No virtual voices, loudness metering, master limiter, audio-rate side-chain or 3D audio (spec, Non-goals). A mixer can put a `Compressor` on `master`, which does not reach send returns.
- Interactive music (beat-quantized transitions, stems, stingers, game parameters) is not here: `Music::tempo` and `meter` are recorded for it.
- A player's `system.json` volumes are read as points on the curve and never rescaled: a file written for a linear volume scale plays its settings below 1 quieter than they were written for.
