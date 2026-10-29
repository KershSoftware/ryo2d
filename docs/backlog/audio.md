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
| 64 voices, 32 of them spatial: `audio_system`'s main-thread time | | median 3.6 µs (runs 3.2, 3.7, 3.6); after the final fixes 4.2 µs (4.0, 4.2, 5.4) | median at most 50 µs: **met** |
| 256 virtual loops ranked against 64 heard (`virtual_256`): `audio_system`'s main-thread time | | median 8.4 µs (runs 8.4, 8.4, 8.4) | median at most 20 µs: **met** |
| `play` of a decoded clip | | median 0.71 µs (0.71, 0.71, 0.71); after the final fixes 0.75 µs (0.75, 0.75, 0.75); after the deferrals' second wave 0.50 µs (0.50, 0.50, 0.50) | median at most 5 µs: **met** |
| `play` of a streamed clip (the 48 s OGG): the main thread's part | | median 23.9 µs (23.6, 23.9, 24.2) | the spec's `play` at most 5 µs: **not met, an exception** (below); guarded at 100 µs |
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

- **The unretained idle screen** read slower on the branch in every round (+12.0%; two more alternated runs of it alone read 171.8 and 172.3 µs before, and 182.7 and 180.1 µs after, +5 to +6%), and **the 10,000-sprite frame** read 22 µs slower. Their work is the same on both builds: the `ui_render` guard read the unretained screen's render step at +69.2 to +71.5 µs over an empty frame after and +70.2 to +73.3 µs before, and its UI pass at 24.0 to 24.3 µs after and 24.1 to 24.9 µs before. The branch's headless frame does add the mock's rendering and the audio frame, about 8 µs, but the whole-step empty frame and idle settings screen carry the same 8 µs and read 0.9 and 1.2 µs faster on the branch. The pattern across these benches is unexplained; GPU pacing is the likely cause (stepped back to back, `settings_label` and `settings_scrolling` each read one of two values a run on either build). The unretained screen is not one of the targets' frames, but it is the one bench here that moved by more than 3%. What of it a game's own idle frame pays was looked into after the deferrals' second wave (below): moving the engine in and out of the world, removed.
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
- **A streamed `play`.** These are 300 plays of the 48 s OGG loaded to stream, one a frame, each stopped the frame after; the last is still playing ten frames on, so none of the streams failed. The main thread makes the engine's decoder (it stores the clip's bytes, rate and length), and kira's `into_sound` allocates the stream's buffers and spawns its decoder thread; the file is opened, and seeked to its start position, on that thread.
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

**The streamed `play` is an exception to the spec's 5 µs, reported to the owner.** A play of a streamed clip costs the main thread about 24 µs, almost all of it kira's: the buffers it allocates for each stream and the decoder thread it spawns for it (the engine's own decoder stores only the clip's bytes, rate and length). Opening the file, its codec's setup and the seek happen on that thread, not here. The cost is per start of a stream (a streamed loop or one-shot, a streamed music track, a loop moved by a rebuild or a reload), a few a scene at most, so no player hears it, but it is not the 5 µs the spec asks of every play. The bench holds it at 100 µs, a ceiling neither the spec nor the plan set, only to catch a regression; it is not recorded as met.

Every save landed: 39 WAV, 40 OGG and 40 MP3 reloads in each run. Mid-file means landings 20 s or more into the stream, 17 of each a run.

### After the final fixes (`benches/audio.rs`, beside the branch before them)

The final fix wave (ed340557 to a7959c3a) changed the play path and the frame: a play builds a mixer that landed first, counts its start against its sound's `per_frame`, and can start a voice that waited for its sound; the frame starts the loops that waited, and keeps the fade of a voice's own pause or resume. Changes queued in a scene's code are now applied as that code ends. The guards ran again on 2026-10-28 beside the branch before the wave (7d72b6b7, exported with `git archive` and built into the workspace's `target/`), in the order below: three runs before and two after, then one of each. Ableton Live again took about a core, and the 1-minute load was 3.1 to 4.7 across the runs. `play_cost` now plays eight 50 ms clips, one of each a frame, since a clip starts once a frame; the branch before played one clip eight times a frame.

| Run | Build | Idle audio frame | 64 voices: median | `play`: median; a definition's | Streamed `play` | Back to back: `voices_64`, `play_cost`, `load_hitch` |
|---|---|---|---|---|---|---|
| 1 | before | 1.08 µs | 3.3 µs | 0.71; 0.79 µs | 23.8 µs | 1.68, 0.88, 31.0 µs |
| 2 | before | 1.12 µs | 3.7 µs | 0.71; 0.75 µs | 25.6 µs | 1.68, 0.88, 31.0 µs |
| 3 | before | 1.12 µs | 3.5 µs | 0.71; 0.79 µs | 24.6 µs | 1.69, 0.88, 30.5 µs |
| 4 | after | 1.50 µs | 4.0 µs | 0.75; 0.79 µs | 25.4 µs | 1.75, 0.92, 32.2 µs |
| 5 | after | 1.62 µs | 4.2 µs | 0.75; 0.79 µs | 24.9 µs | 1.74, 0.92, 35.9 µs |
| 6 | before | 1.79 µs | 4.8 µs | 0.71; 0.79 µs | 34.3 µs | 1.70, 0.89, 32.0 µs |
| 7 | after | 1.67 µs | 5.4 µs | 0.75; 0.79 µs | 25.5 µs | 1.73, 0.91, 33.3 µs |

- **What moved:** a play of a decoded clip, by about 0.04 µs (0.71 to 0.75 µs paced, 0.88 to 0.91 or 0.92 µs back to back), in every run: the per-frame count and the other checks it now makes. `voices_64` back to back, by about 0.05 µs (1.68 to 1.74 µs). Both are far inside their targets.
- **Within the runs' spread:** the paced idle frame and 64 voices (each build's runs overlap), the streamed play, a definition's play, and every reload landing: the WAV's median +11.5 to +14.0 µs after against +11.7 to +14.0 before, the OGG's +45.6 to +46.8 against +46.2 to +48.5, the MP3's +45.9 to +48.4 against +46.2 to +47.9. The slowest landing of any file in the runs after was +148.5 µs (one MP3 landing in run 4; the next slowest +69.3), against the 0.5 ms each must stay under.
- An earlier pair at load 10 to 12 read every paced median 2 to 4 times higher on both builds (idle 5.5 after and 6.1 before; 64 voices 8.4 and 11.7), and is left out of the table.

### After the deferrals' first wave (`benches/audio.rs`, beside ac1bbe4c)

The first wave of 5b's deferrals changed the play path and the frame: a play applies the voices' queued changes first, plays that waited count against `per_frame` as they land, a voice remembers a fade from silence in progress (so a restart carries it on), and the frame's glides are capped at 100 ms. `voices_64` and `play_cost` ran on 2026-10-28 beside the branch before the wave (ac1bbe4c, exported with `git archive` and built into the workspace's `target/`; each build's binary was kept aside and checked to come from its tree), alternated base, head, head, base, base, head. Ableton Live again took about a core; the 1-minute load was 3.0 to 4.3, but 7.5 at the first run.

| Build | Idle audio frame | 64 voices: median | `play`: median; a definition's | Back to back: `voices_64`, `play_cost` |
|---|---|---|---|---|
| before | 1.04, 1.04, 1.00 µs | 2.7, 2.9, 2.8 µs | 0.75; 0.79 µs (each run) | 1.75, 1.72, 1.71 µs; 0.93, 0.92, 0.93 µs |
| after | 1.00, 1.08, 1.04 µs | 2.8, 3.2, 2.9 µs | 0.75; 0.79 µs (each run) | 1.85, 1.80, 1.79 µs; 0.93, 0.92, 0.93 µs |

- **What moved:** `voices_64` back to back, by about 0.07 µs (1.73 to 1.81 µs, +4%), in every run. A voice now carries its fade-in's start and length (`FadeIn`), so the voice table the frame walks several times is larger, and each placed voice's gain change asks whether a fade rides on its volume. It is far inside its 50 µs target.
- **Within the runs' spread:** the paced idle frame and 64 voices, and `play` paced and back to back (the queued-change check a play now makes is one look at an empty channel).

### After the deferrals' second wave (`benches/audio.rs`, beside 0916d380)

**The idle frame, looked into.** `settings_idle_unretained` read 5 to 12% slower on the branch (above). Of what a headless whole step adds on the branch, the mock's rendering (7 to 11 µs an idle frame, on the frame's thread) is a headless step's alone: a device renders on its own thread. The rest is `audio_system`'s own main-thread work, which a game's idle frame pays too. Timed in a tight loop of idle frames (an audio world with a mixer set, its log off, the mock's rendering left out), it was 0.50 µs, and 0.37 µs of that was `try_resource_scope` taking the `AudioEngine` out of the world and putting it back: the resource was the engine's whole state, 4,320 bytes, moved twice (the same scope over an 8-byte resource took 0.06 µs, over a 4,320-byte array 0.29 µs). Every play paid the same, as `AudioView` takes the engine out for each. The resource is now a box of the state (`EngineState`), 8 bytes: the idle loop reads 0.14 µs and the scope 0.06 µs. Nothing else a real idle frame pays came to light: the scenes' `in_scene` brackets and the scene hold check are a resource lookup each (about 5 ns). The rest of the unretained screen's gap is the run-to-run spread the GPU benches show (above), and is not re-measured here: the change is about 0.4 µs of a 180 µs step.

**The bench runs.** `voices_64` and `play_cost` (with `play_cost_streamed`) ran on 2026-10-28 beside the branch before the wave (0916d380, exported with `git archive`). Both builds went into a scratch target directory outside the workspace's (the owner's release build was in use), each tree's sources touched before its build, and each binary kept aside and checked to come from its tree (the base binary holds the export's path, the head's does not, and they differ). They ran alternated base, head, head, base, base, head, the first once the 1-minute load fell to 4.5; the loads at the runs' starts were 3.7 to 5.3, still falling from earlier builds, and the paced medians read 3 to 5 times the first wave's on both builds (the mock's rendering 8.9 to 10.8 µs an idle frame, against 7.1 then).

| Build (runs) | Idle audio frame, paced | 64 voices, paced | `play` paced; a definition's | Back to back: `voices_64`; `play_cost` | Streamed `play` |
|---|---|---|---|---|---|
| before (1, 4, 5) | 3.92, 5.46, 4.50 µs | 9.0, 10.1, 8.4 µs | 0.75, 0.79, 0.79; 0.79, 0.83, 0.79 µs | 1.95, 2.12, 2.73; 0.95, 0.94, 1.02 µs | 38.1, 36.8, 40.0 µs |
| after (2, 3, 6) | 5.12, 5.04, 3.42 µs | 9.2, 9.1, 6.9 µs | 0.50, 0.50, 0.50; 0.54, 0.54, 0.54 µs | 1.81, 1.92, 1.61; 0.71, 0.68, 0.66 µs | 39.2, 39.2, 27.5 µs |

- **What moved:** a play of a decoded clip, 0.75 to 0.79 µs paced before and 0.50 µs after in every run (a definition's 0.79 to 0.83 to 0.54 µs; back to back 0.94 to 1.02 to 0.66 to 0.71 µs): the play no longer moves the engine's 4 KB twice. `voices_64` back to back read lower after (1.61 to 1.92 µs against 1.95 to 2.73 µs), the same saving once a frame.
- **Within the runs' spread at this load:** the paced idle frame and 64 voices, and the streamed play. The tight loop above is the idle frame's measure here. Every guard met its target in every run.

### After the features' third wave (`benches/audio.rs`, beside c84a11d9)

The third wave changed the play path and the frame: a play can start as a voice a system's `Audio` queued (`play_from` takes it), voice ids come from an atomic, a change for a voice not in the table is held for its start, the frame checks two lists (held changes, retired trees ringing out their effects' tails) that are empty in these benches, and an emitter's hold looks up the scene that added it. `voices_64` and `play_cost` (with `play_cost_streamed`) ran on 2026-10-28 beside the branch before the wave (c84a11d9, exported with `git archive`), both built into a scratch target directory outside the workspace's (the owner's release build is in `target/release`), each tree's sources touched before its build, each binary kept aside and checked to come from its tree (the base binary holds the export's path, the head's does not; they differ). They ran alternated base, head, head, base, base, head, the first once the 1-minute load had fallen below 5 after the gate's builds; the loads at the runs' starts were 3.9, 4.0, 1.9, 1.9, 1.4 and 1.6.

| Build (runs) | Idle audio frame, paced | 64 voices, paced | `play` paced; a definition's | Back to back: `voices_64`; `play_cost` | Streamed `play` |
|---|---|---|---|---|---|
| before (1, 4, 5) | 0.54, 0.46, 0.42 µs | 2.1, 1.7, 1.7 µs | 0.50, 0.50, 0.50; 0.54, 0.50, 0.50 µs | 1.28, 1.25, 1.26; 0.64, 0.63, 0.63 µs | 19.0, 24.0, 23.9 µs |
| after (2, 3, 6) | 0.46, 0.42, 0.46 µs | 1.8, 1.8, 1.7 µs | 0.50, 0.50, 0.50; 0.54, 0.54, 0.54 µs | 1.36, 1.35, 1.31; 0.64, 0.63, 0.64 µs | 23.9, 24.5, 23.9 µs |

- **What moved:** `voices_64` back to back, by about 0.07 µs (1.25 to 1.28 µs before, 1.31 to 1.36 µs after, +5%), in every run. With 64 voices playing and none queued, held, ringing or held by a scene, the frame's new work is two checks of empty lists; the engine state's layout changed (three new fields), the likely cause, not measured further. It is far inside its 50 µs target.
- **Within the runs' spread:** the paced idle frame and 64 voices, `play` paced (0.50 µs on both builds; a definition's 0.50 to 0.54 µs on both) and back to back (0.63 to 0.64 µs on both), and the streamed play (the base's first run, at 19.0 µs, the one apart). Every guard met its target in every run.
- At this load the paced medians are back at the first wave's (the idle frame 0.42 to 0.54 µs, the mock's rendering 6.4 to 6.6 µs), and the idle frame reads 0.42 to 0.54 µs against 1.00 to 1.08 µs in the first wave's runs, consistent with the second wave's box of the engine state (0.36 µs a frame in its tight loop).

### After the fix round (`benches/audio.rs`, beside f72595b4)

The fix round changed the frame: it plays the emitter plays systems queued for entities not yet emitters (`play_awaited`, one check of an empty list in these benches), a stream that fails is stopped with a fade worked out from its rate, and ringing trees fade out before they go. `play_from` did not change. `voices_64` and `play_cost` (with `play_cost_streamed`) ran on 2026-10-28 beside the branch before the round (f72595b4, exported with `git archive`), each built into its own scratch target directory outside the workspace's (one shared target directory would have kept the base's build for both: cargo hashes a path package relative to its workspace, so the two trees' units share names, and the fingerprint checks the files the first build read), each binary kept aside and checked to come from its tree (the base binary holds the export's path; they differ). They ran alternated base, head, head, base, base, head, then one more base, the first once the 1-minute load had fallen below 5; the fourth run's log was written over by the fifth's under the same name (a fault in the run script), so the extra base run replaces it. The loads at the kept runs' starts were 4.4, 2.6, 2.5, 1.6, 1.5 and 1.4.

| Build (runs) | Idle audio frame, paced | 64 voices, paced | `play` paced; a definition's | Back to back: `voices_64`; `play_cost` | Streamed `play` |
|---|---|---|---|---|---|
| before (1, 5, 7) | 0.54, 0.42, 0.50 µs | 1.9, 1.7, 1.8 µs | 0.50, 0.50, 0.46; 0.50, 0.54, 0.50 µs | 1.37, 1.34, 1.37; 0.64, 0.64, 0.63 µs | 23.6, 24.9, 23.7 µs |
| after (2, 3, 6) | 0.54, 0.50, 0.46 µs | 1.7, 2.0, 1.7 µs | 0.50, 0.50, 0.50; 0.50, 0.50, 0.50 µs | 1.36, 1.32, 1.33; 0.63, 0.63, 0.63 µs | 25.1, 23.7, 24.4 µs |

Nothing moved beyond the runs' spread, and every guard met its target in every run.

### After the features' fourth wave (`benches/audio.rs`, beside 1bb76590)

The fourth wave added virtual voices: a frame with a virtual voice ranks them and moves them on, and every frame does a little more for its placed voices (whether each is near enough to come back, whether a loop has left its range) and its voice table (each voice's virtual state where voices are counted, and the virtual ones counted again as ended voices go). A new guard, `virtual_256`, holds a frame with 256 virtual loops ranked against 64 heard to 20 µs: 320 loops at 60 to 1,500 px from the listener, all in range, on a bus the mixer limits to 64 voices, so the 64 nearest are heard and 256 virtual, each weighed each frame and none the 3 dB louder that would move it (the guard checks that a frame moves none, and that 64 and 256 hold from start to end). Beside it the guard prints the median `play` of a decoded clip on `sfx` with the 256 virtual loops in the voice table, whose limit checks look at every voice.

`voices_64` and `play_cost` (with `play_cost_streamed`) ran on 2026-10-29 beside the branch before the wave (1bb76590, exported with `git archive`), each built into its own scratch target directory outside the workspace's, each tree's sources touched before its build, each binary kept aside and checked to come from its tree (the base binary holds the export's path and has no `virtual_256`; they differ), with `CRITERION_HOME` set to a scratch directory per build. They ran alternated base, head, head, base, base, head, the first once the 1-minute load had fallen below 5; the head's runs also ran `virtual_256`. The loads at the runs' starts were 4.8, 2.1, 1.5, 1.5, 1.5 and 1.6.

| Build (runs) | Idle audio frame, paced | 64 voices, paced | `play` paced; a definition's | Back to back: `voices_64`; `play_cost` | Streamed `play` |
|---|---|---|---|---|---|
| before (1, 4, 5) | 0.50, 0.42, 0.46 µs | 1.8, 1.8, 1.8 µs | 0.50, 0.46, 0.50; 0.50, 0.50, 0.50 µs | 1.33, 1.32, 1.33; 0.64, 0.64, 0.63 µs | 25.0, 23.6, 22.8 µs |
| after (2, 3, 6) | 0.50, 0.50, 0.50 µs | 2.0, 1.9, 2.0 µs | 0.50, 0.50, 0.46; 0.50, 0.50, 0.54 µs | 1.46, 1.42, 1.44; 0.64, 0.63, 0.63 µs | 24.7, 23.2, 24.0 µs |

| `virtual_256` (head runs 2, 3, 6) | Paced: median, 95th percentile, max | Back to back | A play beside them, paced |
|---|---|---|---|
| | 8.4, 8.4, 8.4 µs; 10.7, 10.8, 11.1 µs; 29.8, 28.6, 31.2 µs | 7.11, 7.15, 7.09 µs | 0.67, 0.75, 0.75 µs |

- **What moved:** `voices_64` back to back, by about 0.11 µs (1.32 to 1.33 µs before, 1.42 to 1.46 µs after, +8%), in every run, and paced by about as much (1.8 µs before, 1.9 to 2.0 after). With no voice virtual the frame ranks nothing and moves nothing on; what is left is per voice: for each of the 32 placed voices whether it is near enough to come back (one more distance test) and whether its loop has left its range, and for each of the 64 a virtual check where voices are counted and as ended voices go. Far inside 50 µs.
- **Within the runs' spread:** the paced idle frame, `play` paced (0.46 to 0.50 µs on both builds; a definition's 0.50 to 0.54) and back to back (0.63 to 0.64 µs on both), and the streamed play.
- **`virtual_256`** sits at 8.4 µs paced in every run (back to back 7.1 µs), well inside its 20 µs; at a load of 11 (the first run, before this record's) it read 8.5 µs paced and 6.96 µs back to back. A play beside the 256 virtual loops read 0.67 to 0.75 µs: the limit checks' scan of 320 more voices costs about a quarter of a microsecond.

### The reload hitch

**What the landing frame does.** A reloaded clip lands in the pump: the workers loaded it, and the swap costs about 7 µs. In the same frame `audio_system` switches each loop playing the clip to the new data where it is, crossfading over 10 ms.
- A decoded clip starts at once (+8 µs).
- A streamed clip starts a new stream at the old position (about 41 to 46 µs, the same for the OGG and the MP3). The engine's decoder (`src/core/audio/stream.rs`) takes only the clip's bytes, rate and length on the main thread; kira allocates the stream's buffers and spawns its decoder thread, and the file is probed, its codec set up and the seek made there, on the first frames that thread decodes. kira pauses a stream until its first frames are decoded, while its fade-in runs on and the old voice fades out, so a slow open shortens the new stream's audible fade-in by the open's time: negligible here, well under the 10 ms crossfade, and on a device the next callback usually comes after the open.

**Why the open is the decoder thread's.** On the calling thread, kira's own decoder made a play of these OGGs cost 0.3 to 0.45 ms and a reload's landing 0.45 to 0.56 ms (55% of it the Vorbis setup header and codebooks, 15% the probe, 23% the seek), and a 5 min 20 s, 376 kbps track 0.9 to 1.2 ms. The main thread's part does not depend on the file.

**Every start of a streamed clip costs the same,** not only a reload: `play` of a streamed loop, `play_music` of a streamed track, a mixer rebuild moving a loop. That is the streamed `play` above, about 24 µs.

### A retired tree ringing out

A rebuild (a mixer saved while it is tuned, or `set_mixer`) keeps the old tree while its effects ring, up to 10 s after its last sound, and at most four such trees at once, beside the live one (`src/core/audio/AGENTS.md`, The bus tree and the mix). kira processes every kept track each 128-frame chunk, as it does the live tree's, so what a ringing tree costs is what its effects cost. Worked out from kira 0.12.6's sources, not measured:
- **A `Reverb`** holds eight pairs of comb filters and four of all-pass filters, about 100 KB at 48 kHz, and does a few dozen operations a frame on them: negligible.
- **A `Delay`** holds `seconds × rate` frames of 8 bytes, and shifts its whole buffer by the chunk's length each chunk (`copy_within`, in its `process`). A 10 s delay at 48 kHz holds 3.84 MB and moves it 375 times a second: about 1.4 GB/s of memmove on the mixing thread. The live tree and four ringing copies of it, after rapid saves, come to about 7 GB/s for up to 10 s, which risks underruns on a slower machine. A delay under a second costs a tenth of that or less.
- **Not done:** counting long delays against the four ringing trees (a 10 s delay as several), or a delay of the engine's own over a ring buffer. A designer tuning a long delay hears the cost only while saving it repeatedly; a game's `set_mixer` rebuilds rarely.

### Streams and kira

1. **kira 0.12.6 ends a stream dropped without being stopped:** `StreamingSound`'s `Drop` marks it stopped, so its decoder thread ends, as when an `AudioEngine` (or an audio world) is dropped with a stream playing, or kira refuses a sound for want of room after its decoder started. The engine uses kira 0.12.6.
2. **A Vorbis stream started partway in ends, and loops, at the end of the file.** kira's own decoder miscounts frames after a Vorbis seek: the first packet after the seek decodes to nothing, while it counts from that packet's start, so its stream runs a packet ahead and never gives the file's last frames, and its decoder thread spins there instead of ending or looping. The engine's decoder places each packet's frames by the packet's timestamp, drops those before the seek's target, and pads a file that ends early with silence to the length its header gives, so a stream started anywhere (a reload switch, a mixer rebuild) is frame-exact and reaches the end of the file: a one-shot ends and a loop wraps. The decoder's tests compare streams started at the start (OGG, MP3, FLAC and WAV) and partway (OGG, MP3 and FLAC) with the whole file decoded at once.
3. **A streamed clip's region is its decoder's whole stream.** The engine gives the decoder a definition's `region` and leaves kira's own slice unset: kira seeks a sliced stream's decoder by the slice's frames but asks it for the file's, so a region far into a long file would be decoded from the file's start first. Its frames, its loop and its position count from the region's start, as a decoded clip's slice does.
4. **A header's length can be an estimate.** An MP3 with no Xing, Info or VBRI tag gets symphonia's estimate from its bitrate (close for a constant bitrate, possibly seconds off for a variable one), so such a streamed loop wraps after a short silence or cuts its end. A FLAC without `total_samples` has no length, so it loads only with `ClipMode::Decode`.

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

A counting global allocator counts the test thread's allocations: the frame runs there, and so does the mock's rendering. It reads 0 over 100 idle frames of a fresh engine with a mixer set (as every engine sets the game's, which each frame looks up and checks for a change), 0 over 100 idle frames after a one-shot, a spatial one-shot and a loop stopped with a fade have played and ended, and 0 over 100 frames with four loops virtual (three a limit keeps out, one out of range), ranked and moved on each frame.
- The ranking's lists are taken out of the engine while it runs; what stands in for them must be empty lists (`Vec::new` allocates nothing): with lists made with room, 100 frames allocated 400 times.
- An audio world's frame must clear the change trackers, as the engine's `Last` does, and `testing::audio_step` does: `audio_system` takes the engine out of the world each frame, and bevy writes a removal message for it into a buffer that only clearing reuses. Without it, 100 frames allocate 3 times.
