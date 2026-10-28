# Audio (Sub-project 5b)

Spec: `docs/specs/2026-10-24-audio-design.md`.

## 5b: measured

Measured on 2026-10-28 on an Apple M5 Pro (18 cores, Metal, macOS 27.0.1), a shared machine: a music application (Ableton Live) was open and took about 95% of a core, and the window server about 25%. Medians move with the machine's load, so every run's 1-minute load is given.

**Method.**
- **Before** is Task 1's commit (64271e9a: kira 0.12.5 under the old audio player). It was exported with `git archive` into a scratch folder, not a worktree, and built into the workspace's own `target/`.
- **The idle and empty frames:** the build before and the branch (at 8faa9486, kira 0.12.5; an idle frame starts no stream, so the stream decoder and kira 0.12.6 do not touch it) ran `benches/ui_render.rs` and `benches/foundation.rs` (the same sources on both sides) in three alternated rounds, the build before first in each. Each run waited until the 1-minute load was 4 or less; the loads at the runs' starts were 3.40 to 4.00.
- **The audio guards** (`benches/audio.rs`) ran three times on the branch, at loads 3.85, 3.23 and 3.27.
- **Their frames are paced at 60 Hz with a spin wait,** so the numbers are taken on a core kept busy: best case for a game whose main thread sleeps in present or vsync.
- **What a headless frame does on the branch.** The build before ran headless engines with no audio at all. The branch runs the audio engine there, on kira's mock backend, which renders the mix inside `audio_system` on the frame's thread (7.1 to 7.2 µs an idle frame), where a device renders it on its own thread. A headless whole step on the branch therefore holds work that a game's main thread does not do. The idle audio frame's own main-thread time is 1.13 to 1.25 µs.
- **The rules the guards hold:** `voices_64`, `play_cost` and `play_cost_streamed` hold their median; `load_hitch` holds every landing, as the spec's "adds at most 0.5 ms to any frame" says: each file's slowest landing frame, less the run's median ordinary frame, and a stream's slowest mid-file landing the same way. A run with a file over the limit is measured again once, and each file's better run is held, so that one preemption on a shared machine cannot fail it. No run needed the second measurement.
- The numbers are criterion's medians, and the guards' figures from their own output.

### The targets

| Target | Before | After | Verdict |
|---|---|---|---|
| Idle frame: empty frame, render step (`ui_render` guard) | 67.1 µs (rounds 67.6, 67.1, 66.5) | 66.4 µs (66.2, 66.4, 67.5) | within 3%: **met**, -1.0% |
| Idle frame: idle settings screen, render step (`ui_render` guard) | 66.6 µs (66.4, 66.6, 67.1) | 66.8 µs (66.8, 66.1, 67.5) | within 3%: **met**, +0.3% |
| Idle frame: empty frame, whole step (`empty_frame`) | 188.8 µs (187.5, 188.8, 189.2) | 187.9 µs (187.9, 188.5, 187.4) | within 3%: **met**, -0.5% |
| Idle frame: idle settings screen, whole step (`settings_idle`) | 187.3 µs (185.0, 187.3, 187.3) | 186.1 µs (187.1, 185.9, 186.1) | within 3%: **met**, -0.6% |
| Idle frame: 10,000 sprites, whole step (`foundation`) | 1,023.1 µs (1,022.8, 1,024.5, 1,023.1) | 1,045.0 µs (1,037.2, 1,045.0, 1,051.5) | within 3%: **met**, +2.1% |
| 64 voices, 32 of them spatial: `audio_system`'s main-thread time | | median 3.6 µs (runs 3.2, 3.7, 3.6) | median at most 50 µs: **met** |
| `play` of a decoded clip | | median 0.71 µs (0.71, 0.71, 0.71) | median at most 5 µs: **met** |
| `play` of a streamed clip (the 48 s OGG): the main thread's part | | median 23.9 µs (23.6, 23.9, 24.2) | median at most 100 µs: **met** |
| No allocation for audio on an idle frame | | 0 in 100 frames, fresh and after sounds played (`tests/audio_alloc.rs`) | **met** |
| A reload: a 1 s WAV playing as a loop (decoded) | | the slowest landing +19.4 to +87.0 µs; median +12.5 to +13.1 µs | every landing at most 0.5 ms: **met** |
| A reload: a 48 s OGG playing as a loop (streamed) | | the slowest landing +57.3 to +118.6 µs, mid-file +57.3 to +118.6 µs; median +45.2 to +51.5 µs | every landing at most 0.5 ms: **met** |
| A reload: a 48 s MP3 playing as music (streamed) | | the slowest landing +62.4 to +64.8 µs, mid-file +56.4 to +64.8 µs; median +46.1 to +51.3 µs | every landing at most 0.5 ms: **met** |
| Output latency, the default desktop configuration | | 512 frames at 48 kHz: 10.67 ms a buffer, heard 12.9 ms after its callback | reported |
| Output latency, `audio_buffer_frames = 256` | | 256 frames at 48 kHz: 5.33 ms a buffer, heard 7.6 ms after its callback | reported |

### Idle and empty frames (`benches/ui_render.rs` and `benches/foundation.rs`, three rounds)

| Run | Load at `ui_render`'s start | Load at `foundation`'s start |
|---|---|---|
| round 1, before | 3.96 | 4.00 |
| round 1, after | 4.00 | 3.96 |
| round 2, before | 3.40 | 4.00 |
| round 2, after | 3.81 | 3.85 |
| round 3, before | 3.66 | 3.78 |
| round 3, after | 4.00 | 3.98 |

The `ui_render` guard reads the render step: eight engines stepped in turn, with the GPU idle before each step. Both builds met all of the guard's own targets in every round but one each: the one-label target (25 µs) missed by a few µs once on each side (+25.5 µs after, round 1; +26.1 µs before, round 3), as it does at loads near 4.

Criterion's other medians from the same rounds:

| Bench | Before | After | Change |
|---|---|---|---|
| `settings_idle_unretained` | 172.8 µs (173.4, 172.8, 172.7) | 193.5 µs (193.5, 188.4, 195.6) | +12.0% |
| `settings_label` | 222.0 µs (221.1, 222.0, 223.4) | 218.6 µs (221.7, 149.3, 218.6) | -1.6% |
| `settings_scrolling` | 224.7 µs (222.5, 224.7, 225.5) | 143.6 µs (143.6, 218.0, 143.5) | -36.1% |
| propagate 10k, every root moved | 74.3 µs (74.2, 74.3, 75.2) | 78.6 µs (86.9, 78.6, 77.8) | +5.8% |
| propagate 10k, nothing moved | 7.1 µs (5.9, 7.7, 7.1) | 7.0 µs (7.0, 9.5, 6.8) | -1.2% |

- **The unretained idle screen** read slower on the branch in every round (+12.0%; two more alternated runs of it alone read 171.8 and 172.3 µs before, and 182.7 and 180.1 µs after, +5 to +6%), and **the 10,000-sprite frame** read 22 µs slower. Their work is the same on both builds: the `ui_render` guard read the unretained screen's render step at +69.2 to +71.5 µs over an empty frame after and +70.2 to +73.3 µs before, and its UI pass at 24.0 to 24.3 µs after and 24.1 to 24.9 µs before. The branch's headless frame does add the mock's rendering and the audio frame, about 8 µs, but the whole-step empty frame and idle settings screen carry the same 8 µs and read 0.9 and 1.2 µs faster on the branch. The pattern across these benches is unexplained; GPU pacing is the likely cause (stepped back to back, `settings_label` and `settings_scrolling` each read one of two values a run on either build). The unretained screen is not one of the targets' frames, but it is the one bench here that moved by more than 3%.
- **Propagation** ran the same source on both sides, and its rounds swing as much as its change.

### The audio guards (`benches/audio.rs`, three runs)

Each guard runs in an audio world (`testing::audio_world`) on the mock backend (48 kHz, 128-frame chunks). Frames are paced at 60 Hz, with a spin wait. A frame's main-thread time is `audio_system`'s wall time less the mock's rendering (`AudioEngine::mock_render_time`). Each frame ends with the change trackers cleared, as the engine's `Last` clears them (`testing::audio_step` does the same), and the audio log is off, as in a game.

| Run (load) | Idle audio frame (no voices); the mock's rendering | 64 voices: median, 95th percentile, max; the mock's rendering | `play`: median, 95th percentile, max; a definition's play | A streamed `play`: median, 95th percentile, max |
|---|---|---|---|---|
| 1 (3.85) | 1.25 µs; 7.21 µs | 3.2, 9.0, 18.5 µs; 448.0 µs | 0.71, 2.00, 16.1 µs; 0.75 µs | 23.6, 38.4, 46.5 µs |
| 2 (3.23) | 1.25 µs; 7.12 µs | 3.7, 8.8, 56.0 µs; 448.8 µs | 0.71, 2.00, 11.1 µs; 0.75 µs | 23.9, 35.5, 46.7 µs |
| 3 (3.27) | 1.13 µs; 7.17 µs | 3.6, 8.2, 13.9 µs; 453.6 µs | 0.71, 2.04, 32.6 µs; 0.79 µs | 24.2, 39.3, 77.0 µs |

- **64 voices.** There are 32 plain loops on `sfx` and `ambience`, and 32 loops heard from `AudioEmitter`s that circle the listener, each with a distance low-pass (the most that carry one). The emitters move nearer and further by turns, so every frame changes each one's gain, pan and cutoff by more than the least change sent. The guard checks this from the audio log for one frame: 32 gains and 32 pans. kira's handles take a parameter change as a write to a triple buffer, so a frame that moves all 32 costs a few µs.
- **`play`.** These are 10,000 plays of a decoded 50 ms clip, eight a frame (16 voices playing at the end of a frame), each timed alone. The definition has three clips, a volume and pitch spread, and no repeats.
- **A streamed `play`.** These are 300 plays of the 48 s OGG loaded to stream, one a frame, each stopped the frame after; the last is still playing ten frames on, so the streams decode. The main thread makes the engine's decoder (it stores the clip's bytes, rate and length), and kira's `into_sound` allocates the stream's buffers and spawns its decoder thread; the file is opened, and seeked to its start position, on that thread.
- **The paced medians move with the machine.** Criterion's back-to-back medians, which do not pace, read the same as on Task 14's head: `voices_64` 1.70, 1.68 and 1.67 µs; `play_cost` 0.89, 0.88 and 0.88 µs. `load_hitch` (an OGG reload's landing frame, from 20 s in) reads 30.4, 29.3 and 33.0 µs.

The load guard plays three files from a `MemorySource`: a 1 s WAV as a decoded loop, a 48 s OGG as a streamed loop on `ambience`, and breakout's 48 s MP3 as streamed music. Every 50 frames each is saved over with its other version and reloaded, the three a third of 50 frames apart.
- The OGG is breakout's music (48 s), made with `ffmpeg`'s own Vorbis encoder at about 160 kbps. Its other version is the same at 90% of the level. Without `ffmpeg` the OGG is left out, and the run's last lines say it was not measured.
- The MP3's other version is breakout's menu track (46 s).
- A frame's time is the pump's and `audio_system`'s. A landing's cost is its frame's time less the run's median ordinary frame (2.0, 2.5 and 2.5 µs; the 95th percentile of ordinary frames is +5.5 to +6.0 µs).

| Run | WAV: landing, median (pump; audio), the slowest | OGG: median (pump; audio), the slowest; mid-file median, the slowest | MP3: median (pump; audio), the slowest; mid-file median, the slowest |
|---|---|---|---|
| 1 | +12.5 µs (6.9; 8.0), +87.0 | +51.5 µs (7.3; 46.4), +118.6; +51.5, +118.6 | +51.3 µs (8.0; 45.6), +64.8; +50.7, +64.8 |
| 2 | +13.1 µs (7.3; 8.2), +41.8 | +48.9 µs (6.8; 44.7), +80.4; +48.2, +62.9 | +48.0 µs (7.7; 43.4), +64.2; +48.0, +56.4 |
| 3 | +12.7 µs (7.2; 8.0), +19.4 | +45.2 µs (7.1; 41.4), +57.3; +45.5, +57.3 | +46.1 µs (7.5; 40.7), +62.4; +44.7, +62.4 |

Every save landed: 39 WAV, 40 OGG and 40 MP3 reloads in each run. Mid-file means landings 20 s or more into the stream, 17 of each a run.

### The reload hitch

**What the landing frame does.** A reloaded clip lands in the pump: the workers loaded it, and the swap costs about 7 µs. In the same frame `audio_system` switches each loop playing the clip to the new data where it is, crossfading over 10 ms.
- A decoded clip starts at once (+8 µs).
- A streamed clip starts a new stream at the old position (about 41 to 46 µs, the same for the OGG and the MP3). The engine's decoder (`src/core/audio/stream.rs`) takes only the clip's bytes, rate and length on the main thread; kira allocates the stream's buffers and spawns its decoder thread, and the file is probed, its codec set up and the seek made there, on the first frames that thread decodes. kira pauses a stream until its first frames are decoded, so the open delays the new stream's start by the open's time, well under the 10 ms crossfade, and does not glitch it.

**Why the open is the decoder thread's.** On the calling thread, kira's own decoder made a play of these OGGs cost 0.3 to 0.45 ms and a reload's landing 0.45 to 0.56 ms (55% of it the Vorbis setup header and codebooks, 15% the probe, 23% the seek), and a 5 min 20 s, 376 kbps track 0.9 to 1.2 ms. The main thread's part does not depend on the file.

**Every start of a streamed clip costs the same,** not only a reload: `play` of a streamed loop, `play_music` of a streamed track, a mixer rebuild moving a loop. That is the streamed `play` above, about 24 µs.

### Streams and kira

1. **kira 0.12.6 ends a stream dropped without being stopped:** `StreamingSound`'s `Drop` marks it stopped, so its decoder thread ends, as when an `AudioEngine` (or an audio world) is dropped with a stream playing, or kira refuses a sound for want of room after its decoder started. The engine uses kira 0.12.6.
2. **A Vorbis stream started partway in ends, and loops, at the end of the file.** kira's own decoder miscounts frames after a Vorbis seek: the first packet after the seek decodes to nothing, while it counts from that packet's start, so its stream runs a packet ahead and never gives the file's last frames, and its decoder thread spins there instead of ending or looping. The engine's decoder places each packet's frames by the packet's timestamp, drops those before the seek's target, and pads a file that ends early with silence to the length its header gives, so a stream started anywhere (a reload switch, a mixer rebuild) is frame-exact and reaches the end of the file: a one-shot ends and a loop wraps. The decoder's tests compare a stream started partway with the whole file decoded at once, for OGG, MP3 and FLAC.

### The web: a long clip decodes in one job

On the web every clip decodes, in the asset pump on the main thread. A long clip is therefore a one-off hitch of its whole decode. Natively, decoding in one job (`ClipMode::Decode`, the median of five, each run):
- the first 30 s of breakout's music as OGG: 15.6, 15.3 and 15.5 ms;
- breakout's 48 s MP3: 20.8, 20.7 and 21.4 ms.

An estimate, not measured on the web: wasm runs slower than native, so there a 30 s OGG is likely a hitch of one frame at 60 Hz at best, and two or three on a slower machine. A web game should load its long clips (music, ambience) behind a loading screen, or before the scene's first frame, rather than mid-play. Each decoded minute also holds about 23 MB (48 kHz stereo, 8 bytes a frame).

### Output latency

The default output device here is the MacBook Pro's speakers, through CoreAudio: 48,000 Hz, stereo, f32, with a supported buffer of 15 to 4,096 frames. The engine opens it as kira's cpal backend does (cpal's default configuration), or with its buffer fixed at `audio_buffer_frames`, cut to that range. These were measured with cpal directly, the way the engine opens it, for 1.5 s each:

| Configuration | Frames a callback | One buffer | Callback to playback (cpal's timestamps) | From a `play` call to sound |
|---|---|---|---|---|
| Default (cpal's default buffer) | 512 | 10.67 ms | 12.92 ms | 12.9 to 23.6 ms |
| `audio_buffer_frames = 256` | 256 | 5.33 ms | 7.58 ms | 7.6 to 12.9 ms |

- A play reaches kira at once, and the next callback renders it. That is up to one buffer later; the callback's data is then heard after the "callback to playback" time, which is a buffer plus about 2.25 ms of device latency.
- kira's 128-frame internal chunks add nothing on top: each callback renders its whole buffer.
- A smaller buffer cuts latency, at the cost of more callbacks (281 rather than 141 in 1.5 s), and with them more risk of underruns on a busy machine.

### The idle frame's allocations (`tests/audio_alloc.rs`)

A counting global allocator counts the test thread's allocations: the frame runs there, and so does the mock's rendering. It reads 0 over 100 idle frames of a fresh engine with a mixer set (as every engine sets the game's, which each frame looks up and checks for a change), and 0 over 100 idle frames after a one-shot, a spatial one-shot and a loop stopped with a fade have played and ended.
- An audio world's frame must clear the change trackers, as the engine's `Last` does, and `testing::audio_step` does: `audio_system` takes the engine out of the world each frame, and bevy writes a removal message for it into a buffer that only clearing reuses. Without it, 100 frames allocate 3 times.
