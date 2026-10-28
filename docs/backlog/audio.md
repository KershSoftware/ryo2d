# Audio (Sub-project 5b)

Spec: `docs/specs/2026-10-24-audio-design.md`.

## 5b: measured

Measured on 2026-10-28 on an Apple M5 Pro (18 cores, Metal, macOS 27.0.1), a shared machine: a music application (Ableton Live) was open and took about 95% of a core, and the window server about 25%. Medians move with the machine's load, so every run's 1-minute load is given.

**Method.**
- **Before** is Task 1's commit (64271e9a: kira 0.12.5 under the old audio player). It was exported with `git archive` into a scratch folder, not a worktree, and built into the workspace's own `target/`.
- **The idle and empty frames:** the build before and the branch ran `benches/ui_render.rs` and `benches/foundation.rs` (the same sources on both sides) in three alternated rounds, the build before first in each. Each run waited until the 1-minute load was 4 or less; the loads at the runs' starts were 3.40 to 4.00.
- **The audio guards** (`benches/audio.rs`) ran three times on the branch, at loads 3.90, 3.25 and 3.62.
- **What a headless frame does now.** The build before ran headless engines with no audio at all. The branch runs the audio engine there, on kira's mock backend, which renders the mix inside `audio_system` on the frame's thread (6.7 to 6.9 µs an idle frame), where a device renders it on its own thread. A headless whole step on the branch therefore holds work that a game's main thread does not do. The idle audio frame's own main-thread time is 0.92 to 1.00 µs.
- The numbers are criterion's medians, and the guards' medians from their own output.

### The targets

| Target | Before | After | Verdict |
|---|---|---|---|
| Idle frame: empty frame, render step (`ui_render` guard) | 67.1 µs (rounds 67.6, 67.1, 66.5) | 66.4 µs (66.2, 66.4, 67.5) | within 3%: **met**, -1.0% |
| Idle frame: idle settings screen, render step (`ui_render` guard) | 66.6 µs (66.4, 66.6, 67.1) | 66.8 µs (66.8, 66.1, 67.5) | within 3%: **met**, +0.3% |
| Idle frame: empty frame, whole step (`empty_frame`) | 188.8 µs (187.5, 188.8, 189.2) | 187.9 µs (187.9, 188.5, 187.4) | within 3%: **met**, -0.5% |
| Idle frame: idle settings screen, whole step (`settings_idle`) | 187.3 µs (185.0, 187.3, 187.3) | 186.1 µs (187.1, 185.9, 186.1) | within 3%: **met**, -0.6% |
| Idle frame: 10,000 sprites, whole step (`foundation`) | 1,023.1 µs (1,022.8, 1,024.5, 1,023.1) | 1,045.0 µs (1,037.2, 1,045.0, 1,051.5) | within 3%: **met**, +2.1% |
| 64 voices, 32 of them spatial: `audio_system`'s main-thread time | | 2.5 µs (runs 2.5, 2.3, 2.5) | at most 50 µs: **met** |
| `play` of a decoded clip | | 0.71 µs (0.71, 0.71, 0.71) | at most 5 µs: **met** |
| No allocation for audio on an idle frame | | 0 in 100 frames, fresh and after sounds played (`tests/audio_alloc.rs`) | **met** |
| A reload: a 1 s WAV playing as a loop (decoded) | | +11.2 to +11.7 µs on its landing frame | at most 0.5 ms: **met** |
| A reload: a 48 s OGG playing as a loop (streamed) | | +445.5 to +459.3 µs; mid-file +433.3 to +449.8 µs; the slowest landing +505 to +565 µs | at most 0.5 ms: **met** by the median, with about 40 µs to spare; the slowest landing in each run is over |
| A reload: a 48 s MP3 playing as music (streamed) | | +74.6 to +79.0 µs; mid-file +83.7 to +85.6 µs; the slowest +96 to +104 µs | at most 0.5 ms: **met** |
| Output latency, the default desktop configuration | | 512 frames at 48 kHz: 10.67 ms a buffer, heard 12.9 ms after its callback | reported |
| Output latency, `audio_buffer_frames = 256` | | 256 frames at 48 kHz: 5.33 ms a buffer, heard 7.6 ms after its callback | reported |

The guards hold the reload's median landing frame, as 5a's reload guard held the median hitch. The spec says a reload adds at most 0.5 ms to any frame. The OGG's slowest landing missed that in every run. "The reload hitch" below shows where that time goes, and why a long, high-bitrate OGG would miss the median too.

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

- **The unretained idle screen** read slower on the branch in every round. Two more alternated runs of it alone read 171.8 and 172.3 µs before, and 182.7 and 180.1 µs after (+5 to +6%).
  - Its work is the same on both builds. The `ui_render` guard read its render step at +69.2 to +71.5 µs over an empty frame after, and +70.2 to +73.3 µs before. Its UI pass read 24.0 to 24.3 µs after, and 24.1 to 24.9 µs before.
  - The branch's headless frame also renders the mock's mix and runs the audio frame, about 8 µs that the build before skipped. The rest of the gap is not accounted for.
  - Stepped back to back, an unretained screen is paced by the GPU, as `settings_label` and `settings_scrolling` are: each of those two reads one of two values a run on either build. It is not one of the targets' frames, but it is the one bench here that moved by more than 3%.
- **The 10,000-sprite frame** is 22 µs slower. About 8 µs of that is the headless audio frame (the mock's 6.7 to 6.9 µs and the frame's 1 µs).
- **Propagation** ran the same source on both sides, and its rounds swing as much as its change.

### The audio guards (`benches/audio.rs`, three runs)

Each guard runs in an audio world (`testing::audio_world`) on the mock backend (48 kHz, 128-frame chunks). Frames are paced at 60 Hz, with a spin wait, for 2,000 frames. A frame's main-thread time is `audio_system`'s wall time less the mock's rendering (`AudioEngine::mock_render_time`). Each frame ends with the change trackers cleared, as the engine's `Last` clears them, and the audio log is off, as in a game.

| Run (load) | Idle audio frame (no voices); the mock's rendering | 64 voices: median, 95th percentile, max; the mock's rendering | `play`: median, 95th percentile, max; a definition's play |
|---|---|---|---|
| 1 (3.90) | 1.00 µs; 6.71 µs | 2.5, 7.8, 279.4 µs; 453.8 µs | 0.71, 2.00, 31.2 µs; 0.79 µs |
| 2 (3.25) | 0.92 µs; 6.92 µs | 2.3, 7.3, 19.5 µs; 455.5 µs | 0.71, 2.00, 33.2 µs; 0.79 µs |
| 3 (3.62) | 0.92 µs; 6.71 µs | 2.5, 7.6, 21.3 µs; 451.0 µs | 0.71, 2.00, 20.6 µs; 0.79 µs |

- **64 voices.** There are 32 plain loops on `sfx` and `ambience`, and 32 loops heard from `AudioEmitter`s that circle the listener, each with a distance low-pass (the most that carry one). The emitters move nearer and further by turns, so every frame changes each one's gain, pan and cutoff by more than the least change sent. The guard checks this from the audio log for one frame: 32 gains and 32 pans. kira's handles take a parameter change as a write to a triple buffer, so a frame that moves all 32 costs about 2 µs.
- **`play`.** These are 10,000 plays of a decoded 50 ms clip, eight a frame (16 voices playing at the end of a frame), each timed alone. The definition has three clips, a volume and pitch spread, and no repeats.
- **Criterion's medians** (back to back, for profiles): `voices_64` 1.70, 1.69 and 1.69 µs; `play_cost` 0.89 µs in each run; `load_hitch` (an OGG reload's landing frame, from 20 s in) 409, 403 and 406 µs.

The load guard plays three files from a `MemorySource`: a 1 s WAV as a decoded loop, a 48 s OGG as a streamed loop on `ambience`, and breakout's 48 s MP3 as streamed music. Every 50 frames each is saved over with its other version and reloaded, the three a third of 50 frames apart.
- The OGG is breakout's music, made with `ffmpeg`'s own Vorbis encoder at about 160 kbps. Its other version is the same at 90% of the level.
- The MP3's other version is breakout's menu track (46 s).
- A frame's time is the pump's and `audio_system`'s. An ordinary frame's median is 1.7 µs (1.8 in the first run).

| Run | WAV: landing frame, median (pump; audio), max | OGG: median (pump; audio), max; mid-file median and max | MP3: median, max; mid-file median and max |
|---|---|---|---|
| 1 | +11.7 µs (6.2; 7.0), +19.0 | +459.3 µs (6.7; 455.7), +564.5; +449.8, +517.5 | +79.0 µs, +103.8; +85.6, +103.8 |
| 2 | +11.6 µs (6.2; 6.9), +19.6 | +446.5 µs (6.5; 443.1), +511.2; +433.3, +511.2 | +78.8 µs, +102.8; +85.6, +102.8 |
| 3 | +11.2 µs (6.0; 6.7), +20.6 | +445.5 µs (6.2; 439.1), +505.2; +437.2, +505.2 | +74.6 µs, +96.0; +83.7, +96.0 |

Every save landed: 39 WAV, 40 OGG and 40 MP3 reloads in each run. Mid-file means landings 20 s or more into the stream, 17 of each a run.

### The reload hitch

**What the landing frame does.** A reloaded clip lands in the pump: the workers loaded it, and the swap costs about 6 to 7 µs. In the same frame `audio_system` switches each loop playing the clip to the new data where it is, crossfading over 10 ms.
- A decoded clip starts at once (+7 µs).
- A streamed clip is opened again on the main thread, then started at the old position. kira's `StreamingSoundData::from_cursor` probes the file and builds its decoder. `TrackHandle::play` then runs `DecodeScheduler::new`, which seeks the decoder there, and spawns the decoder's thread.

**A profile of the OGG's landings.** `sample` took 1 ms samples of the main thread for 12 s, while the criterion `load_hitch` bench reloaded the OGG at 60 Hz. Of the samples in `AudioEngine::start_stream`:

| Where | Share |
|---|---|
| symphonia's `VorbisDecoder::try_new` (the Vorbis setup header and its codebooks) | 55% |
| symphonia's `OggReader::try_new` (the probe) | 15% |
| `DecodeScheduler::new`'s seek (symphonia's OGG seek) | 23% |
| the decoder thread's spawn, and the rest | 7% |

The MP3 opens in about 6 µs, and its start (the seek and the spawn) takes about 60 µs.

**A longer, higher-bitrate OGG would miss.** A 5 min 20 s libvorbis track at 376 kbps (15 MB, a shipped game's soundtrack, read locally and not kept) was measured alone with kira's API. Opening it took 331 µs, and kira's start took 590 to 880 µs (the seek grows with the file), so a reload switch would cost about 0.9 to 1.2 ms. The 30 s and 48 s OGGs here are short and modest by comparison.

**The remedy in the plan was not applied.** The plan's remedy for a miss was to start the new stream at 0, silent, `seek_to` the position (kira seeks on the decoder's thread), and fade in. The guard met its median, so the remedy was not triggered. It also could not reach the cost that matters:
- kira 0.12.5's `DecodeScheduler::new` seeks on the calling thread even to position 0, which is the 590 µs above.
- `seek_to` discards the frames already buffered only for a sound that is paused, not one pausing or playing. A stream started at 0 and then seeked would play its first frames from the start, unless it waited a frame before fading in.
- Opening the file is 70% of the OGG's landing, and the remedy does not touch it.

What would remove it is opening the stream off the main thread. The loader already opens one on a worker to probe the clip, and drops it; a clip could keep it for the next start. Starting a stream partway without the calling thread's seek needs kira's help, or a decoder of the engine's own (kira takes custom decoders).

**Every start of a streamed clip pays the same open,** not only a reload: `play` of a streamed loop, `play_music` of a streamed track. That is about 0.3 to 0.45 ms on the main thread for these OGGs, 0.08 ms for the MP3, and over a millisecond for a long, high-bitrate OGG. The `play` target was measured on a decoded clip, as the plan says.

### Found while measuring: kira 0.12.5's decoder threads that never end

1. **A Vorbis stream started partway in spins its decoder thread forever at the end of the file.**
   - This happens to a stream started at a position: the loop a clip reload switches, or one a mixer rebuild moves to the new buses.
   - Its decoder thread loops at 100% of a core in `DecodeScheduler::frame_at_index`, calling a decoder that has nothing left. It never looks at the sound's state again, so stopping the voice does not end it. By kira's code, the stream also gets no more frames, so the loop falls silent there instead of wrapping.
   - Seen with `ffmpeg`'s Vorbis and with libvorbis files. MP3, FLAC, a stream started at 0, and a music loop wrapping to its `loop_start` all loop correctly.
   - kira likely loses the first packet after seeking a fresh Vorbis decoder (its Vorbis decoder gives no samples for that packet), so its frame count runs short of the file's.
   - The load guard's OGG is 48 s so that no stream reaches its end in the 34 s run. The criterion bench, stepped back to back at first, ran its streams to their ends and left a thread spinning per reload, hundreds within seconds. It is now paced at 60 Hz.
   - Not fixed here.
2. **A streaming sound dropped without being stopped keeps its decoder thread forever, waking every millisecond.** This happens when an `AudioEngine` (or an audio world) is dropped with a stream playing, or when kira refuses a sound for want of room after its decoder started. kira 0.12.6 fixes this: `StreamingSound`'s `Drop` marks it stopped. Not fixed here: kira is pinned at 0.12.5.

### The web: a long clip decodes in one job

On the web every clip decodes, in the asset pump on the main thread. A long clip is therefore a one-off hitch of its whole decode. Natively, decoding in one job (`ClipMode::Decode`, the median of five, each run):
- the first 30 s of breakout's music as OGG: 15.6, 15.7 and 15.7 ms;
- breakout's 48 s MP3: 21.1, 21.3 and 21.5 ms.

wasm runs slower than native, so on the web a 30 s OGG is a hitch of one frame at 60 Hz at best, and two or three on a slower machine. A web game should load its long clips (music, ambience) behind a loading screen, or before the scene's first frame, rather than mid-play. Each decoded minute also holds about 23 MB (48 kHz stereo, 8 bytes a frame).

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

A counting global allocator counts the test thread's allocations: the frame runs there, and so does the mock's rendering. It reads 0 over 100 idle frames of a fresh engine, and 0 over 100 idle frames after a one-shot, a spatial one-shot and a loop stopped with a fade have played and ended.
- The first version of the test read 3 in 100 fresh frames. `audio_system` takes the engine out of the world each frame, and bevy writes a removal message for it into a buffer. A world that never clears its change trackers grows that buffer; the engine's `Last` clears them every frame.
- The test's frame now clears them as the engine does, and reads 0 with no engine change.
