# Audio fixtures

Generated tones, with no licence restriction. Each is at most 20 KB. Made
with `ffmpeg -v error -y ...`, ffmpeg 8 (`tone-3s.ogg` with ffmpeg 9); the
Vorbis files use ffmpeg's built-in encoder, as these builds have no
libvorbis.

```sh
S='sine=frequency=440:duration=0.25:sample_rate=22050'
ffmpeg -f lavfi -i "$S" -ac 1 -ar 22050 tone.wav
ffmpeg -f lavfi -i "$S" -ac 2 -ar 22050 -c:a vorbis -strict -2 tone.ogg
ffmpeg -f lavfi -i "$S" -ac 1 -ar 22050 -c:a libmp3lame -b:a 32k tone.mp3
ffmpeg -f lavfi -i "$S" -ac 1 -ar 22050 tone.flac
ffmpeg -f lavfi -i "$S" -ac 1 -ar 22050 -c:a pcm_u8 tone8.wav
ffmpeg -f lavfi -i "sine=frequency=440:duration=0.1:sample_rate=22050" -ac 2 -ar 22050 -c:a pcm_f32le tonef32.wav
ffmpeg -f lavfi -i "sine=frequency=440:duration=0.05:sample_rate=22050" -ac 6 -ar 22050 -c:a pcm_s16le surround.wav
ffmpeg -f lavfi -i "aevalsrc=0.25*sin(2*PI*(220+110*t)*t)|0.25*sin(2*PI*(330+110*t)*t):s=44100:d=3" -ac 2 -ar 44100 -c:a vorbis -strict -2 -q:a 1 tone-3s.ogg
```

`tone-3s.ogg` is a rising tone, a different one on each side, so a stream
that starts a few frames early or late, or swaps its channels, differs from
the decoded file. It is stereo because ffmpeg's built-in Vorbis encoder only
writes two channels, and at quality 1 because at quality 0 that encoder
writes silence. It is 13.5 KB.

| File | Format | Length |
|------|--------|--------|
| `tone.wav`, `tone.ogg`, `tone.mp3`, `tone.flac` | 22.05 kHz, 16-bit or compressed | 250 ms |
| `tone8.wav` | 8-bit mono | 250 ms |
| `tonef32.wav` | 32-bit float stereo | 100 ms |
| `surround.wav` | 6 channels, which no loader accepts | 50 ms |
| `tone-3s.ogg` | 44.1 kHz stereo Vorbis, rising tones, to stream from partway | 3 s |
