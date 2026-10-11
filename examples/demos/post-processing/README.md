# Post-Processing Effects Demo

Interactive demo showcasing Ryo2D's post-processing shader system.

## Features

- **6 Post-Processing Effects:**
  - Fade (scene transitions)
  - Color Tint (sepia, color grading)
  - Vignette (edge darkening)
  - Screen Flash (impact effects)
  - CRT (retro scanlines & distortion)
  - Chromatic Aberration (RGB offset)

## Controls

- **Number Keys (1-8):** Switch between effects
  - `1` - None (disable all effects)
  - `2` - Fade
  - `3` - Color Tint (Sepia)
  - `4` - Vignette
  - `5` - Screen Flash
  - `6` - CRT
  - `7` - Chromatic Aberration
  - `8` - Custom WGSL scanlines (`assets/shaders/scanlines.wgsl`)

- **Arrow Keys:** Adjust effect intensity
  - `Up/Down` - Increase/decrease primary parameter
  - `Left/Right` - Adjust secondary parameter (where applicable)

- **Space:** Trigger screen flash

- **ESC:** Quit

## Running

```bash
cargo run
```

## Purpose

This demo helps you understand how to use post-processing effects in your games:
- Fade for scene transitions
- Screen flash for damage/impact effects
- Vignette for atmosphere and focus
- CRT for retro aesthetics
- Color tint for mood/environmental effects
- Chromatic aberration for glitch/distortion effects
