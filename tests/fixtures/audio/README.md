# Audio fixtures

Generated sine tones (440 Hz), with no licence restriction. Each is at most
20 KB. Made with ffmpeg 8 (`ffmpeg -v error -y ...`); the Vorbis file uses
ffmpeg's built-in encoder, as this build has no libvorbis.

```sh
S='sine=frequency=440:duration=0.25:sample_rate=22050'
ffmpeg -f lavfi -i "$S" -ac 1 -ar 22050 tone.wav
ffmpeg -f lavfi -i "$S" -ac 2 -ar 22050 -c:a vorbis -strict -2 tone.ogg
ffmpeg -f lavfi -i "$S" -ac 1 -ar 22050 -c:a libmp3lame -b:a 32k tone.mp3
ffmpeg -f lavfi -i "$S" -ac 1 -ar 22050 tone.flac
ffmpeg -f lavfi -i "$S" -ac 1 -ar 22050 -c:a pcm_u8 tone8.wav
ffmpeg -f lavfi -i "sine=frequency=440:duration=0.1:sample_rate=22050" -ac 2 -ar 22050 -c:a pcm_f32le tonef32.wav
ffmpeg -f lavfi -i "sine=frequency=440:duration=0.05:sample_rate=22050" -ac 6 -ar 22050 -c:a pcm_s16le surround.wav
```

| File | Format | Length |
|------|--------|--------|
| `tone.wav`, `tone.ogg`, `tone.mp3`, `tone.flac` | 22.05 kHz, 16-bit or compressed | 250 ms |
| `tone8.wav` | 8-bit mono | 250 ms |
| `tonef32.wav` | 32-bit float stereo | 100 ms |
| `surround.wav` | 6 channels, which no loader accepts | 50 ms |
