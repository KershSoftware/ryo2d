# Audio System

ryo2d includes a built-in audio system powered by [Kira](https://github.com/tesselode/kira), a game audio library designed for games.

## Overview

The audio system provides:
- **Sound effects** - Short sounds loaded into memory for instant playback
- **Music** - Long audio files streamed from disk for memory efficiency
- **Volume control** - Master, SFX, and music volume levels
- **Stereo panning** - Position sounds left/right for spatial awareness
- **Playback rate** - Speed up or slow down sounds (affects pitch)
- **Fading & crossfading** - Smooth music transitions
- **Playback control** - Play, pause, stop, and adjust playing sounds

## Quick Start

```rust
use ryo2d::prelude::*;

struct MyGame {
    audio: Option<AudioManager>,
}

impl Scene for MyGame {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
        // Initialize audio
        match AudioManager::new() {
            Ok(mut audio) => {
                // Load sound effects into memory
                audio.load_sound("jump", "assets/sounds/jump.wav").unwrap();
                audio.load_sound("coin", "assets/sounds/coin.ogg").unwrap();
                self.audio = Some(audio);
            }
            Err(e) => {
                eprintln!("Audio init failed: {}", e);
            }
        }
    }

    fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
        if let Some(audio) = &mut self.audio {
            // Play sound on key press
            if ctx.input.key_pressed(KeyCode::Space) {
                let _ = audio.play_sound("jump");
            }
        }
        SceneAction::Continue
    }
}
```

## AudioManager

The `AudioManager` is the main interface for all audio operations.

### Creating an AudioManager

```rust
// Create with default settings
let audio = AudioManager::new()?;
```

> **Note:** Creating an `AudioManager` initializes the audio device. If no audio device is available (e.g., headless server), this will return an error. Handle this gracefully!

### Loading Sound Effects

Sound effects are loaded into memory for instant playback:

```rust
// Load with a name for later reference
audio.load_sound("explosion", "assets/sounds/explosion.wav")?;
audio.load_sound("laser", "assets/sounds/laser.ogg")?;

// Supported formats: WAV, MP3, OGG, FLAC
```

### Playing Sound Effects

```rust
// Play a loaded sound with default options
let handle = audio.play_sound("explosion")?;

// Play with custom options using the builder pattern
let handle = audio.play_sound_with("explosion", SoundOptions::new()
    .volume(0.5)        // 50% volume
    .panning(-0.3)      // Slightly left
    .playback_rate(1.2) // 20% faster
)?;
```

### Playing Music

Music is streamed from disk, making it memory-efficient for long tracks. By default, music loops.

```rust
// Play looping music with defaults
let music = audio.play_music("assets/music/theme.ogg")?;

// Play once (no loop) with a fade-in effect
let music = audio.play_music_with("assets/music/intro.ogg", MusicOptions::new()
    .once()
    .fade_in(Duration::from_secs(2))
)?;

// Auto-fade-out when the handle is dropped (scene exits)
let music = audio.play_music_with("assets/music/level.ogg", MusicOptions::new()
    .fade_out_on_drop(Duration::from_millis(500))
)?;

// Full customization
let music = audio.play_music_with("assets/music/boss.ogg", MusicOptions::new()
    .volume(0.8)
    .looping(true)
    .fade_in(Duration::from_secs(1))
    .fade_out_on_drop(Duration::from_millis(500))
)?;
```

## Sound Handles

When you play a sound, you get a handle to control it:

### SoundHandle (for sound effects)

```rust
let mut handle = audio.play_sound("laser")?;

// Control playback
handle.pause();
handle.resume();
handle.stop();

// Adjust volume
handle.set_volume(0.5);

// Adjust panning (-1.0 to 1.0)
handle.set_panning(-0.5);  // Pan left

// Adjust playback rate/pitch
handle.set_playback_rate(1.5);  // 50% faster

// Check state
if handle.is_playing() {
    println!("Still playing!");
}

if handle.is_finished() {
    println!("Sound complete");
}
```

### MusicHandle (for music)

```rust
let mut music = audio.play_music("assets/music/boss.ogg")?;

// Same controls as SoundHandle
music.pause();
music.resume();
music.set_volume(0.8);
music.set_panning(0.0);      // Center
music.set_playback_rate(1.0); // Normal speed
music.stop();
```

## Volume Control

The audio system supports three volume levels that multiply together:

| Level | Description | Affects |
|-------|-------------|---------|
| Master | Global volume for all audio | Everything |
| SFX | Volume for sound effects | `play_sound()` |
| Music | Volume for background music | `play_music()` |

```rust
// Set volumes (0.0 - 1.0)
audio.set_master_volume(0.8);
audio.set_sfx_volume(1.0);
audio.set_music_volume(0.6);

// Get current volumes
let master = audio.master_volume();
let sfx = audio.sfx_volume();
let music = audio.music_volume();
```

The effective volume is calculated as: `master × category × sound`

For example, if master is 0.5, SFX is 0.5, and you call:
```rust
audio.play_sound_with("explosion", SoundOptions::new().volume(0.5))?;
```
The final volume is 0.5 × 0.5 × 0.5 = 0.125 (12.5%)

### Updating Music Volume in Real-Time

Music volume changes are applied when tracks start. To update playing music:

```rust
// When music category volume changes, update the active track
if let Some(music) = &mut self.music_handle {
    music.update_category_volume(audio.master_volume(), audio.music_volume());
}
```

This is useful in pause menus or settings screens where users adjust volume sliders.

## Positional Audio (Panning)

For 2D games, you can position sounds in the stereo field using panning. This makes sounds feel like they're coming from specific locations on screen.

### Play with panning

```rust
// Play sound panned to the left (-1.0 = left, 0.0 = center, 1.0 = right)
let handle = audio.play_sound_with("explosion", SoundOptions::new().panning(-0.5))?;

// Calculate pan based on screen position
fn screen_position_to_pan(x: f32, screen_width: f32) -> f32 {
    // Convert screen X to -1.0..1.0 range
    (x / screen_width) * 2.0 - 1.0
}

let pan = screen_position_to_pan(enemy_x, 800.0);
audio.play_sound_with("enemy_hit", SoundOptions::new().panning(pan))?;
```

### Adjust panning on playing sounds

```rust
let mut handle = audio.play_sound("laser")?;

// Move sound left to right over time
handle.set_panning(-1.0);  // Start on left
// ... later
handle.set_panning(0.0);   // Move to center
handle.set_panning(1.0);   // Move to right
```

## Playback Rate (Pitch/Speed)

Adjust the playback rate to change both speed and pitch of sounds:

```rust
// Play at half speed (lower pitch)
let handle = audio.play_sound_with("laser", SoundOptions::new().playback_rate(0.5))?;

// Play at double speed (higher pitch)
let handle = audio.play_sound_with("laser", SoundOptions::new().playback_rate(2.0))?;
```

### Common use cases

- **Engine sounds**: Increase pitch as speed increases
- **Slow motion**: Lower playback rate for all sounds
- **Voice variety**: Slight pitch variations for repeated sounds

```rust
// Randomize pitch slightly for variety
let pitch = 0.9 + rand::random::<f32>() * 0.2;  // 0.9 to 1.1
let handle = audio.play_sound_with("footstep", SoundOptions::new().playback_rate(pitch))?;
```

## SoundOptions and MusicOptions

Use builder-pattern structs for full control:

### SoundOptions

```rust
let options = SoundOptions::new()
    .volume(0.8)        // 0.0-1.0, default: 1.0
    .panning(-0.5)      // -1.0 to 1.0, default: 0.0 (center)
    .playback_rate(1.5); // default: 1.0

audio.play_sound_with("explosion", options)?;
```

### MusicOptions

```rust
let options = MusicOptions::new()
    .volume(0.8)                                    // 0.0-1.0, default: 1.0
    .panning(0.0)                                   // -1.0 to 1.0, default: 0.0
    .playback_rate(1.0)                             // default: 1.0
    .looping(true)                                  // default: true
    .fade_in(Duration::from_secs(2))                // default: None (no fade)
    .fade_out_on_drop(Duration::from_millis(500));  // default: None (no fade)

audio.play_music_with("assets/music/boss.ogg", options)?;

// Shorthand for non-looping
let options = MusicOptions::new().once();  // Same as .looping(false)
```

The `fade_out_on_drop` option automatically fades out music when the handle is dropped (e.g., when a scene exits). This ensures smooth transitions without manual cleanup.

## Music System

Background music requires special handling for smooth transitions between tracks.

### Fade In/Out

Use fading for professional-sounding music transitions:

```rust
use std::time::Duration;

// Start music with a fade-in
let mut music = audio.play_music_with("assets/music/level1.ogg", MusicOptions::new()
    .fade_in(Duration::from_secs(2))
)?;

// Fade out when leaving the level
music.fade_out(Duration::from_secs(1));

// Or fade out and stop completely
music.fade_out_and_stop(Duration::from_secs(1));
```

### Fade to Volume

Smoothly transition to any volume level:

```rust
// Fade to 50% volume over 1 second
music.fade_to(0.5, Duration::from_secs(1));

// Fade back to full volume
music.fade_to(1.0, Duration::from_millis(500));
```

### Crossfading

Smoothly transition between two music tracks:

```rust
// Current music is playing...
let mut current_music = audio.play_music("assets/music/explore.ogg")?;

// Later, crossfade to boss music (2 second transition)
let boss_music = audio.crossfade_music(
    &mut current_music,
    "assets/music/boss.ogg",
    Duration::from_secs(2),
)?;

// With custom options for the new track
let boss_music = audio.crossfade_music_with(
    &mut current_music,
    "assets/music/boss.ogg",
    Duration::from_secs(2),
    MusicOptions::new().volume(0.8),
)?;
```

The crossfade will:
1. Fade out the current track over the specified duration
2. Simultaneously fade in the new track
3. Stop the old track when the fade completes

### Common Music Patterns

**Level transitions:**
```rust
// Fade out current music before scene change
self.music.fade_out_and_stop(Duration::from_millis(500));
// Load new level...
// Start new music with fade-in
self.music = audio.play_music_with("new_level.ogg", MusicOptions::new()
    .fade_in(Duration::from_secs(1))
)?;
```

**Intensity changes:**
```rust
// Combat starts - crossfade to combat music
if combat_started {
    self.music = audio.crossfade_music(
        &mut self.music,
        "combat.ogg",
        Duration::from_secs(1),
    )?;
}

// Combat ends - return to calm
if combat_ended {
    self.music = audio.crossfade_music(
        &mut self.music,
        "explore.ogg",
        Duration::from_secs(2),
    )?;
}
```

**Dynamic volume (ducking):**
```rust
// Reduce music volume during dialogue
music.fade_to(0.3, Duration::from_millis(200));

// Restore after dialogue
music.fade_to(1.0, Duration::from_millis(500));
```

## Memory Management

Sound effects are cached in memory. Manage memory explicitly:

```rust
// Check if a sound is loaded
if audio.has_sound("explosion") {
    audio.play_sound("explosion")?;
}

// Unload a specific sound
audio.unload_sound("explosion");

// Unload all cached sounds
audio.clear_sounds();
```

## Error Handling

Audio operations return `AudioResult<T>`:

```rust
match audio.play_sound("missing") {
    Ok(handle) => { /* playing */ }
    Err(AudioError::NotFound(name)) => {
        eprintln!("Sound '{}' not loaded", name);
    }
    Err(e) => {
        eprintln!("Audio error: {}", e);
    }
}
```

### Error Types

| Error | Cause |
|-------|-------|
| `InitError` | Failed to initialize audio device |
| `LoadError` | Failed to load audio file |
| `PlayError` | Failed to play audio |
| `NotFound` | Sound name not in cache |

## Best Practices

### 1. Load sounds in `on_enter()`
```rust
fn on_enter(&mut self, ctx: &mut SceneContext) {
    // Load all sounds upfront for instant playback
    self.audio.load_sound("jump", "assets/jump.wav");
    self.audio.load_sound("land", "assets/land.wav");
}
```

### 2. Handle missing audio gracefully
```rust
// Don't unwrap - audio might fail in some environments
if let Some(audio) = &mut self.audio {
    let _ = audio.play_sound("jump"); // Ignore errors
}
```

### 3. Use streaming for long audio
```rust
// Short sounds (< 10 seconds): load_sound + play_sound
audio.load_sound("hit", "assets/hit.wav");

// Long audio (music, ambient): play_music (streams from disk)
audio.play_music("assets/music/level1.ogg")?;
```

### 4. Reuse sound handles for control
```rust
// Store handle if you need to control it later
self.music_handle = Some(audio.play_music("theme.ogg")?);

// Later...
if let Some(handle) = &mut self.music_handle {
    handle.set_volume(0.5);
}
```

## Complete Example

```rust
use ryo2d::prelude::*;

struct AudioDemo {
    audio: Option<AudioManager>,
    music: Option<MusicHandle>,
}

impl AudioDemo {
    fn new() -> Self {
        Self { audio: None, music: None }
    }
}

impl Scene for AudioDemo {
    fn on_enter(&mut self, _ctx: &mut SceneContext) {
        if let Ok(mut audio) = AudioManager::new() {
            // Load sound effects
            let _ = audio.load_sound("blip", "assets/blip.wav");
            
            // Start background music
            if let Ok(music) = audio.play_music("assets/music.ogg") {
                self.music = Some(music);
            }
            
            self.audio = Some(audio);
        }
    }

    fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
        let Some(audio) = &mut self.audio else { return SceneAction::Continue };

        // Play sound on click
        if ctx.input.mouse_pressed(MouseButton::Left) {
            let _ = audio.play_sound("blip");
        }

        // Volume controls
        if ctx.input.key_pressed(KeyCode::Minus) {
            let vol = audio.master_volume();
            audio.set_master_volume((vol - 0.1).max(0.0));
        }
        if ctx.input.key_pressed(KeyCode::Equal) {
            let vol = audio.master_volume();
            audio.set_master_volume((vol + 0.1).min(1.0));
        }

        // Toggle music pause
        if ctx.input.key_pressed(KeyCode::KeyM) {
            if let Some(music) = &mut self.music {
                if music.is_playing() {
                    music.pause();
                } else {
                    music.resume();
                }
            }
        }
        SceneAction::Continue
    }
}
```

## Supported Formats

| Format | Extension | Notes |
|--------|-----------|-------|
| WAV | `.wav` | Uncompressed, fast loading |
| MP3 | `.mp3` | Common, lossy compression |
| OGG Vorbis | `.ogg` | Good quality, open format |
| FLAC | `.flac` | Lossless compression |

## See Also

- [Getting Started](../getting-started/quick-start.md) - Basic engine setup
- [Game Loop](./game-loop.md) - Understanding update cycles
- [API Reference](../api/ryo2d/index.html) - Full audio API
