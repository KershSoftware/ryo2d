---
name: ryo-audio
description: Sound effects and music with Kira audio backend.
metadata:
  version: 1.1.0
---

# Ryo2D Audio Specialist

You help users integrate audio using the Kira audio backend.

**For complete API signatures, see the `ryo-api` skill.**

**External Docs**: [Kira audio backend](https://docs.rs/kira)

## Audio Access

`ctx.audio` is the engine's `Audio`, present in every scene callback. Without an output device (headless runs, no sound card) its methods that return a `Result` return `Err(AudioError::Unavailable)` and the rest do nothing, so games never branch on availability. `ctx.audio.is_available()` says whether a device exists; `ctx.audio.manager()` gives the underlying `AudioManager` when it does.

## Loading Sounds

Sounds are cached by name: `load_sound(name, path)` once (e.g. in `on_enter()`), then play by name. Paths are relative to the working directory, so include `assets/`:

```rust
fn on_enter(&mut self, ctx: &mut SceneContext) {
  ctx.audio.load_sound("jump", "assets/sounds/jump.wav").ok();
  ctx.audio.load_sound("coin", "assets/sounds/coin.wav").ok();
}
```

`ctx.audio.has_sound(name)`, `unload_sound(name)` and `clear_sounds()` manage the cache.

## Playing Sounds

```rust
// Simple playback (returns AudioResult<SoundHandle>)
if ctx.input.key_pressed(KeyCode::Space) {
  ctx.audio.play_sound("jump").ok();
}

// With options (builder or struct)
ctx
  .audio
  .play_sound_with("jump", SoundOptions::new().volume(0.5).playback_rate(1.2).panning(-0.5))
  .ok();
ctx
  .audio
  .play_sound_with(
    "jump",
    SoundOptions {
      volume: 0.5,
      panning: 0.0,       // -1.0 left, 1.0 right
      playback_rate: 1.2, // pitch/speed
    },
  )
  .ok();
```

The returned `SoundHandle` can `stop()`, `pause()`, `resume()`, `set_volume()`, `set_panning()`, `set_playback_rate()` and report `is_playing()` / `is_finished()`.

## Playing Music

Music is streamed from a file path (no `load_` step):

```rust
use std::time::Duration;

// Looping, no fade; keep the handle in the scene (self.music: Option<MusicHandle>)
self.music = ctx.audio.play_music("assets/music/theme.ogg").ok();

self.music = ctx
  .audio
  .play_music_with(
    "assets/music/theme.ogg",
    MusicOptions::new().volume(0.7).looping(true).fade_in(Duration::from_secs(1)),
  )
  .ok();
```

`MusicOptions` also has `once()`, `panning()`, `playback_rate()` and `fade_out_on_drop(duration)`.

`play_music` returns a `MusicHandle` you own; keep it in your scene struct. Music keeps playing if the handle is dropped (unless `fade_out_on_drop` is set).

For music that persists across scenes, use `play_music_once`, which tracks the track in the manager and does nothing if music is already playing:

```rust
ctx
  .audio
  .play_music_once("assets/music/theme.ogg", MusicOptions::new().volume(0.3))
  .ok(); // Ok(true) if it started, Ok(false) if music was already playing
```

Scene changes leave this music playing: `SceneCleanup::MUSIC` is not in `SceneCleanup::DEFAULT`. A scene whose `cleanup_on_exit` includes it (`SceneCleanup::ALL`, or `DEFAULT | SceneCleanup::MUSIC`) stops it on `Switch` and `Reset`.

## Music Controls

```rust
// On a MusicHandle you hold (here `music` is `&mut MusicHandle`)
music.pause();
music.resume();
music.stop();
music.set_volume(0.5);
music.fade_to(0.2, Duration::from_secs(1));
music.fade_out_and_stop(Duration::from_secs(2));

// Manager-level (tracks of play_music_once)
ctx.audio.is_music_playing();
ctx.audio.stop_music();
ctx.audio.stop_music_fade(Duration::from_secs(2));
if let Some(m) = ctx.audio.current_music() {
  m.pause();
}

// Crossfade to another track: fades the current one out, returns the new handle
if let Some(current) = self.music.as_mut()
  && let Ok(next) = ctx.audio.crossfade_music(current, "assets/music/boss.ogg", Duration::from_secs(2))
{
  self.music = Some(next);
}
```

## Volume

Final volume = master x category (SFX or music) x per-sound volume.

```rust
ctx.audio.set_master_volume(0.8); // real-time, affects playing audio
ctx.audio.set_sfx_volume(0.5);    // new sounds only
ctx.audio.set_music_volume(0.5);  // new music only; use MusicHandle::set_volume for playing music
```

## Common Patterns

### Randomized Variation

```rust
let rate = 0.9 + rand::random::<f32>() * 0.2; // 0.9 to 1.1
ctx
  .audio
  .play_sound_with("jump", SoundOptions { playback_rate: rate, ..Default::default() })
  .ok();
```

### Collision Sounds

```rust
for hit in ctx.messages::<CollisionStarted>() {
  if let Some(other) = hit.other(self.player)
    && ctx.world.get::<Coin>(other).is_some()
  {
    ctx.audio.play_sound("coin").ok();
  }
}
```

### Pause Menu

```rust
fn pause_game(&mut self) {
  if let Some(music) = self.music.as_mut() {
    music.set_volume(0.3); // lower during pause
  }
}
```

## File Formats

- **WAV** - Best for short effects (no decode latency)
- **OGG** - Recommended for music (good compression)
- **MP3**, **FLAC** - Also supported

## Tips

- `ctx.audio` works without a device: calls return `Err(AudioError::Unavailable)` or do nothing, so `.ok()` the results you do not need
- Load sounds by name in `on_enter()`, play them by name
- Add slight pitch variation to repeated sounds
- Fade music during scene transitions (`crossfade_music`, `fade_out_and_stop`)
- Lower music volume during pause/dialogs

