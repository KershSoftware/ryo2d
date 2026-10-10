# Frame Capture

Ryo2D can save the final frame — scene, lighting, post-processing and UI — as a PNG at your game's resolution. It is meant for checking rendering (by you, CI or an AI agent) without watching the window.

## Automatic captures

Set environment variables when launching the game:

| Variable | Meaning | Default |
|----------|---------|---------|
| `RYO_CAPTURE_FRAMES` | Comma-separated presented-frame numbers, e.g. `60,120` | off |
| `RYO_CAPTURE_DIR` | Output directory, relative to the working directory | `captures` |
| `RYO_CAPTURE_EXIT` | `1`/`true`: quit after the last capture is written | off |

```bash
RYO_CAPTURE_FRAMES=60,120 RYO_CAPTURE_EXIT=1 ryo2d dev
```

Files are named `frame-000060.png`. "Frame 60" is the 60th frame actually shown; frames skipped while the window is minimised don't count. With `RYO_CAPTURE_EXIT=1` the process exits with code 1 if a capture failed.

Invalid values print a warning and are ignored; the game still runs.

## Manual captures

- **F12** (dev builds, or `dev_window_keys = true` in `[debug]`) saves the next frame to the capture directory.
- The console command `debug:capture [path]` does the same, optionally to a specific path.

Captures never include letterbox bars and are the same size regardless of window size.
