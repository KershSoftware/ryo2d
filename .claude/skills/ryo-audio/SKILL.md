---
name: ryo-audio
description: Sound effects and music with Kira audio backend.
metadata:
  version: 1.0.0
---

# Ryo2D Audio Specialist

You help users integrate audio using the Kira audio backend.

**For complete API signatures, see the `ryo-api` skill.**

**External Docs**: [Kira audio backend](https://docs.rs/kira)

## AudioManager Access

```rust
// Audio may be unavailable (headless mode, no device)
if let Some(audio) = ctx.audio.as_mut() {
    // Use audio here
}
```

## Loading Sounds

Sounds are cached by name: `load_sound(name, path)` once (e.g. in `on_enter()`), then play by name. Paths are relative to the working directory, so include `assets/`:

```rust
fn on_enter(&mut self, ctx: &mut SceneContext) {
    if let Some(audio) = ctx.audio.as_mut() {
        audio.load_sound("jump", "assets/sounds/jump.wav").ok();
        audio.load_sound("coin", "assets/sounds/coin.wav").ok();
    }
}
```

`audio.has_sound(name)`, `unload_sound(name)` and `clear_sounds()` manage the cache.

## Playing Sounds

```rust
use std::time::Duration;

// Simple playback (returns AudioResult<SoundHandle>)
if ctx.input.key_pressed(KeyCode::Space) {
    if let Some(audio) = ctx.audio.as_mut() {
        audio.play_sound("jump").ok();
    }
}

// With options (builder or struct)
audio.play_sound_with("jump", SoundOptions::new().volume(0.5).playback_rate(1.2).panning(-0.5)).ok();
audio.play_sound_with("jump", SoundOptions {
    volume: 0.5,
    panning: 0.0,        // -1.0 left, 1.0 right
    playback_rate: 1.2,  // pitch/speed
}).ok();
```

The returned `SoundHandle` can `stop()`, `pause()`, `resume()`, `set_volume()`, `set_panning()`, `set_playback_rate()` and report `is_playing()` / `is_finished()`.

## Playing Music

Music is streamed from a file path (no `load_` step):

```rust
let music = audio.play_music("assets/music/theme.ogg")?;   // looping, no fade

let music = audio.play_music_with(
    "assets/music/theme.ogg",
    MusicOptions::new().volume(0.7).looping(true).fade_in(Duration::from_secs(1)),
)?;
```

`MusicOptions` also has `once()`, `panning()`, `playback_rate()` and `fade_out_on_drop(duration)`.

`play_music` returns a `MusicHandle` you own; keep it in your scene struct. Music keeps playing if the handle is dropped (unless `fade_out_on_drop` is set).

For music that persists across scenes, use `play_music_once`, which tracks the track in the manager and does nothing if music is already playing:

```rust
audio.play_music_once("assets/music/theme.ogg", MusicOptions::new().volume(0.3))?;  // Ok(true) if started
```

## Music Controls

```rust
// On a MusicHandle you hold
music.pause();
music.resume();
music.stop();
music.set_volume(0.5);
music.fade_to(0.2, Duration::from_secs(1));
music.fade_out_and_stop(Duration::from_secs(2));

// Manager-level (tracks of play_music_once)
audio.is_music_playing();
audio.stop_music();
audio.stop_music_fade(Duration::from_secs(2));
if let Some(m) = audio.current_music() { m.pause(); }

// Crossfade to another track (returns the new handle)
let music = audio.crossfade_music(&mut music, "assets/music/boss.ogg", Duration::from_secs(2))?;
```

## Volume

Final volume = master x category (SFX or music) x per-sound volume.

```rust
audio.set_master_volume(0.8);  // real-time, affects playing audio
audio.set_sfx_volume(0.5);     // new sounds only
audio.set_music_volume(0.5);   // new music only; use MusicHandle::set_volume for playing music
```

## Common Patterns

### Randomized Variation

```rust
let rate = 0.9 + rand::random::<f32>() * 0.2;  // 0.9 to 1.1
audio.play_sound_with("jump", SoundOptions { playback_rate: rate, ..Default::default() }).ok();
```

### Collision Sounds

```rust
for event in ctx.events.read::<CollisionStarted>() {
    if let Some(other) = event.other(self.player) {
        if ctx.world.has_component::<Coin>(other) {
            if let Some(audio) = ctx.audio.as_mut() {
                audio.play_sound("coin").ok();
            }
        }
    }
}
```

### Pause Menu

```rust
fn pause_game(&mut self, ctx: &mut SceneContext) {
    if let Some(music) = self.music.as_mut() {
        music.set_volume(0.3);  // Lower during pause
    }
}
```

## File Formats

- **WAV** - Best for short effects (no decode latency)
- **OGG** - Recommended for music (good compression)
- **MP3**, **FLAC** - Also supported

## Tips

- Always check `if let Some(audio) = ctx.audio.as_mut()`
- Load sounds by name in `on_enter()`, play them by name
- Add slight pitch variation to repeated sounds
- Fade music during scene transitions (`crossfade_music`, `fade_out_and_stop`)
- Lower music volume during pause/dialogs

