# Audio demo assets

Every `.wav` here is written from code by `src/bin/make-sounds.rs`, so none carries a licence restriction: tones, noise bursts, loops, and a short piece of music in two forms (an intro and its loop at one rate, and one clip with a loop region). Each file is mono, 16-bit, at 44.1 kHz, and the noise is seeded, so the program writes the same bytes on every run on one platform. Another platform's maths library (`sin`, `exp`, `powf`) may round a few samples' last bit differently, so a run there can show a small diff in the WAVs that changes nothing audible. To change a sound, edit the program and run it again:

```sh
cargo run -p audio-demo --bin make-sounds
```

| Folder | What |
|---|---|
| `audio/mixer.mixer.ron` | The mixer the engine loads at start: a bus for each row of the voice limits page, the `muffled` and `quiet` snapshots, and the duck of music under voice |
| `sounds/` | The clips, and the definitions the Buses, Variations and Mix pages play |
| `limits/` | The Voice limits page's bells, one definition per limit rule |
| `spatial/` | The Spatial page's emitters and its ping |
| `virtual/` | The Virtual voices page's bird, thirty of which sing in its field |
| `music/` | The theme (`theme.music.ron`: intro and loop) and the night track (`night.music.ron`: one clip with a loop region) |
| `hot/` | The Hot reload page's drone and beep; save one while the demo runs |

The web build embeds every file here except this one (`src/embedded.rs`); a test checks the list matches the folder.
