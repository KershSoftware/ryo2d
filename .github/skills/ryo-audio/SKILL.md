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

Load in `init()`, store handles:

```rust
struct MyGame {
    jump_sound: Option<SoundHandle>,
    bg_music: Option<MusicHandle>,
}

fn init(&mut self, ctx: &mut GameContext) {
    if let Some(audio) = ctx.audio.as_mut() {
        self.jump_sound = audio.load_sound("sounds/jump.wav").ok();
        self.bg_music = audio.load_music("music/theme.ogg").ok();
    }
}
```

## Playing Sounds

```rust
// Simple playback
if ctx.input.key_pressed(KeyCode::Space) {
    if let Some(ref sound) = self.jump_sound {
        audio.play_sound(sound, SoundOptions::default());
    }
}

// With options
audio.play_sound(sound, SoundOptions {
    volume: 0.5,
    pitch: 1.2,
    panning: 0.0,  // -1.0 left, 1.0 right
});
```

## Playing Music

```rust
if let Some(ref music) = self.bg_music {
    audio.play_music(music, MusicOptions {
        volume: 0.7,
        looping: true,
        fade_in: Some(1.0),  // 1 second fade in
    });
}
```

## Music Controls

```rust
audio.pause_music();
audio.resume_music();
audio.stop_music();
audio.set_music_volume(0.5);
audio.fade_out_music(2.0);  // 2 second fade
```

## Common Patterns

### Randomized Variation

```rust
let pitch = 0.9 + rand::random::<f32>() * 0.2;  // 0.9 to 1.1
audio.play_sound(sound, SoundOptions { pitch, ..Default::default() });
```

### Collision Sounds

```rust
for event in ctx.collisions_started() {
    if event.involves(self.player) {
        let other = event.other_entity(self.player);
        if ctx.world.has_component::<Coin>(other) {
            self.play_sound(ctx, &self.coin_sound);
        }
    }
}
```

### Pause Menu

```rust
fn pause_game(&mut self, ctx: &mut GameContext) {
    if let Some(audio) = ctx.audio.as_mut() {
        audio.set_music_volume(0.3);  // Lower during pause
    }
}
```

## File Formats

- **WAV** - Best for short effects (no decode latency)
- **OGG** - Recommended for music (good compression)
- **MP3**, **FLAC** - Also supported

## Tips

- Always check `if let Some(audio) = ctx.audio.as_mut()`
- Load sounds in `init()`, store handles in game struct
- Add slight pitch variation to repeated sounds
- Fade music during scene transitions
- Lower music volume during pause/dialogs

