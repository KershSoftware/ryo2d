//! Audio system for sound effects and music
//!
//! This module provides:
//! - `AudioManager` - Manages audio device and playback
//! - Sound effect playback with volume control
//! - Stereo panning for positional audio
//! - Playback rate control for pitch shifting
//! - Background music with looping and crossfading

use kira::{
  AudioManager as KiraAudioManager, AudioManagerSettings, Tween,
  sound::{
    FromFileError, PlaybackState,
    static_sound::{StaticSoundData, StaticSoundHandle, StaticSoundSettings},
    streaming::{StreamingSoundData, StreamingSoundHandle, StreamingSoundSettings},
  },
};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

/// Result type for audio operations
pub type AudioResult<T> = Result<T, AudioError>;

/// Audio system errors
#[derive(Debug)]
pub enum AudioError {
  /// Failed to initialize the audio backend
  InitError(String),
  /// Failed to load a sound file
  LoadError(String),
  /// Failed to play a sound
  PlayError(String),
  /// Sound not found in cache
  NotFound(String),
}

impl std::fmt::Display for AudioError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      AudioError::InitError(msg) => write!(f, "Audio init error: {}", msg),
      AudioError::LoadError(msg) => write!(f, "Audio load error: {}", msg),
      AudioError::PlayError(msg) => write!(f, "Audio play error: {}", msg),
      AudioError::NotFound(msg) => write!(f, "Sound not found: {}", msg),
    }
  }
}

impl std::error::Error for AudioError {}

impl From<FromFileError> for AudioError {
  fn from(err: FromFileError) -> Self {
    AudioError::LoadError(format!("{}", err))
  }
}

/// Handle to a playing sound effect
#[derive(Debug)]
pub struct SoundHandle {
  handle: StaticSoundHandle,
}

impl SoundHandle {
  /// Stop the sound immediately
  pub fn stop(&mut self) {
    self.handle.stop(Tween::default());
  }

  /// Pause the sound
  pub fn pause(&mut self) {
    self.handle.pause(Tween::default());
  }

  /// Resume a paused sound
  pub fn resume(&mut self) {
    self.handle.resume(Tween::default());
  }

  /// Set the volume (0.0 = silent, 1.0 = full volume)
  /// Note: Uses linear amplitude, not decibels
  pub fn set_volume(&mut self, volume: f32) {
    self.handle.set_volume(volume, Tween::default());
  }

  /// Set the stereo panning (-1.0 = left, 0.0 = center, 1.0 = right)
  ///
  /// Use this for positional audio in 2D games. For example, if an
  /// explosion happens on the left side of the screen, use a negative
  /// panning value.
  pub fn set_panning(&mut self, pan: f32) {
    self
      .handle
      .set_panning(pan.clamp(-1.0, 1.0), Tween::default());
  }

  /// Set the playback rate (1.0 = normal, 0.5 = half speed, 2.0 = double speed)
  ///
  /// This affects both the speed and pitch of the sound.
  pub fn set_playback_rate(&mut self, rate: f32) {
    self.handle.set_playback_rate(rate as f64, Tween::default());
  }

  /// Check if the sound is still playing
  pub fn is_playing(&self) -> bool {
    self.handle.state() == PlaybackState::Playing
  }

  /// Check if the sound has finished
  pub fn is_finished(&self) -> bool {
    self.handle.state() == PlaybackState::Stopped
  }
}

/// Handle to playing background music
#[derive(Debug)]
pub struct MusicHandle {
  handle: StreamingSoundHandle<FromFileError>,
}

impl MusicHandle {
  /// Stop the music immediately
  pub fn stop(&mut self) {
    self.handle.stop(Tween::default());
  }

  /// Pause the music
  pub fn pause(&mut self) {
    self.handle.pause(Tween::default());
  }

  /// Resume paused music
  pub fn resume(&mut self) {
    self.handle.resume(Tween::default());
  }

  /// Set the volume (0.0 = silent, 1.0 = full volume)
  pub fn set_volume(&mut self, volume: f32) {
    self.handle.set_volume(volume, Tween::default());
  }

  /// Set the stereo panning (-1.0 = left, 0.0 = center, 1.0 = right)
  pub fn set_panning(&mut self, pan: f32) {
    self
      .handle
      .set_panning(pan.clamp(-1.0, 1.0), Tween::default());
  }

  /// Set the playback rate (1.0 = normal, 0.5 = half speed, 2.0 = double speed)
  pub fn set_playback_rate(&mut self, rate: f32) {
    self.handle.set_playback_rate(rate as f64, Tween::default());
  }

  /// Check if the music is still playing
  pub fn is_playing(&self) -> bool {
    self.handle.state() == PlaybackState::Playing
  }

  /// Fade the volume to a target level over a duration
  ///
  /// Use this for smooth volume transitions, like fading out before stopping.
  pub fn fade_to(&mut self, target_volume: f32, duration: Duration) {
    self.handle.set_volume(
      target_volume,
      Tween {
        duration,
        ..Default::default()
      },
    );
  }

  /// Fade in from silence to a target volume
  ///
  /// Note: The music should already be playing (started at volume 0)
  pub fn fade_in(&mut self, target_volume: f32, duration: Duration) {
    self.fade_to(target_volume, duration);
  }

  /// Fade out to silence over a duration
  ///
  /// The music will continue playing at volume 0. Call `stop()` after
  /// the fade completes if you want to fully stop playback.
  pub fn fade_out(&mut self, duration: Duration) {
    self.fade_to(0.0, duration);
  }

  /// Stop the music with a fade out
  ///
  /// Fades the volume to zero over the specified duration, then stops playback.
  pub fn fade_out_and_stop(&mut self, duration: Duration) {
    self.handle.stop(Tween {
      duration,
      ..Default::default()
    });
  }
}

/// Main audio manager for the engine
///
/// Handles loading and playing sound effects and music.
pub struct AudioManager {
  manager: KiraAudioManager,
  /// Cached sound data for quick playback
  sounds: HashMap<String, StaticSoundData>,
  /// Master volume for all audio (0.0 - 1.0)
  master_volume: f32,
  /// Volume for sound effects (0.0 - 1.0)
  sfx_volume: f32,
  /// Volume for music (0.0 - 1.0)
  music_volume: f32,
}

impl AudioManager {
  /// Create a new audio manager
  ///
  /// Returns an error if the audio device cannot be initialized.
  pub fn new() -> AudioResult<Self> {
    let manager = KiraAudioManager::new(AudioManagerSettings::default())
      .map_err(|e| AudioError::InitError(format!("{}", e)))?;

    Ok(AudioManager {
      manager,
      sounds: HashMap::new(),
      master_volume: 1.0,
      sfx_volume: 1.0,
      music_volume: 1.0,
    })
  }

  /// Load a sound effect from a file and cache it for later playback
  ///
  /// Supports: WAV, MP3, OGG, FLAC
  pub fn load_sound(&mut self, name: &str, path: impl AsRef<Path>) -> AudioResult<()> {
    let sound_data = StaticSoundData::from_file(path.as_ref())?;
    self.sounds.insert(name.to_string(), sound_data);
    Ok(())
  }

  /// Calculate the effective volume for sound effects
  fn effective_sfx_volume(&self) -> f32 {
    self.master_volume * self.sfx_volume
  }

  /// Calculate the effective volume for music
  fn effective_music_volume(&self) -> f32 {
    self.master_volume * self.music_volume
  }

  /// Play a cached sound effect
  ///
  /// Returns a handle to control the playing sound.
  pub fn play_sound(&mut self, name: &str) -> AudioResult<SoundHandle> {
    let sound_data = self
      .sounds
      .get(name)
      .ok_or_else(|| AudioError::NotFound(name.to_string()))?
      .clone();

    let settings = StaticSoundSettings::default().volume(self.effective_sfx_volume());
    let sound_with_settings = sound_data.with_settings(settings);

    let handle = self
      .manager
      .play(sound_with_settings)
      .map_err(|e| AudioError::PlayError(format!("{}", e)))?;

    Ok(SoundHandle { handle })
  }

  /// Play a sound effect with custom volume multiplier (0.0 - 1.0)
  pub fn play_sound_with_volume(&mut self, name: &str, volume: f32) -> AudioResult<SoundHandle> {
    let sound_data = self
      .sounds
      .get(name)
      .ok_or_else(|| AudioError::NotFound(name.to_string()))?
      .clone();

    let final_volume = self.effective_sfx_volume() * volume;
    let settings = StaticSoundSettings::default().volume(final_volume);
    let sound_with_settings = sound_data.with_settings(settings);

    let handle = self
      .manager
      .play(sound_with_settings)
      .map_err(|e| AudioError::PlayError(format!("{}", e)))?;

    Ok(SoundHandle { handle })
  }

  /// Play a sound effect with panning for positional audio
  ///
  /// `pan`: Stereo position from -1.0 (left) to 1.0 (right), 0.0 is center
  ///
  /// Useful for making sounds feel like they come from a specific
  /// position on screen.
  pub fn play_sound_panned(&mut self, name: &str, pan: f32) -> AudioResult<SoundHandle> {
    let sound_data = self
      .sounds
      .get(name)
      .ok_or_else(|| AudioError::NotFound(name.to_string()))?
      .clone();

    let settings = StaticSoundSettings::default()
      .volume(self.effective_sfx_volume())
      .panning(pan.clamp(-1.0, 1.0));
    let sound_with_settings = sound_data.with_settings(settings);

    let handle = self
      .manager
      .play(sound_with_settings)
      .map_err(|e| AudioError::PlayError(format!("{}", e)))?;

    Ok(SoundHandle { handle })
  }

  /// Play a sound effect with full customization
  ///
  /// - `volume`: Volume multiplier (0.0 - 1.0)
  /// - `pan`: Stereo position (-1.0 = left, 0.0 = center, 1.0 = right)
  /// - `playback_rate`: Speed/pitch multiplier (1.0 = normal)
  pub fn play_sound_ex(
    &mut self,
    name: &str,
    volume: f32,
    pan: f32,
    playback_rate: f32,
  ) -> AudioResult<SoundHandle> {
    let sound_data = self
      .sounds
      .get(name)
      .ok_or_else(|| AudioError::NotFound(name.to_string()))?
      .clone();

    let final_volume = self.effective_sfx_volume() * volume;
    let settings = StaticSoundSettings::default()
      .volume(final_volume)
      .panning(pan.clamp(-1.0, 1.0))
      .playback_rate(playback_rate as f64);
    let sound_with_settings = sound_data.with_settings(settings);

    let handle = self
      .manager
      .play(sound_with_settings)
      .map_err(|e| AudioError::PlayError(format!("{}", e)))?;

    Ok(SoundHandle { handle })
  }

  /// Play background music from a file (streams from disk)
  ///
  /// For long audio files, this is more memory-efficient than loading into memory.
  /// Set `looping` to true for continuous playback.
  pub fn play_music(&mut self, path: impl AsRef<Path>, looping: bool) -> AudioResult<MusicHandle> {
    let mut settings = StreamingSoundSettings::default().volume(self.effective_music_volume());

    if looping {
      settings = settings.loop_region(..);
    }

    let sound_data = StreamingSoundData::from_file(path.as_ref())
      .map_err(|e| AudioError::LoadError(format!("{}", e)))?
      .with_settings(settings);

    let handle = self
      .manager
      .play(sound_data)
      .map_err(|e| AudioError::PlayError(format!("{}", e)))?;

    Ok(MusicHandle { handle })
  }

  /// Play background music with a fade-in effect
  ///
  /// Starts the music at volume 0 and fades to the target volume over the specified duration.
  pub fn play_music_with_fade_in(
    &mut self,
    path: impl AsRef<Path>,
    looping: bool,
    fade_duration: Duration,
  ) -> AudioResult<MusicHandle> {
    // Start at zero volume with a fade-in tween
    let mut settings = StreamingSoundSettings::default()
      .volume(0.0)
      .fade_in_tween(Tween {
        duration: fade_duration,
        ..Default::default()
      });

    if looping {
      settings = settings.loop_region(..);
    }

    let sound_data = StreamingSoundData::from_file(path.as_ref())
      .map_err(|e| AudioError::LoadError(format!("{}", e)))?
      .with_settings(settings);

    let mut handle = self
      .manager
      .play(sound_data)
      .map_err(|e| AudioError::PlayError(format!("{}", e)))?;

    // Fade to the effective music volume
    handle.set_volume(
      self.effective_music_volume(),
      Tween {
        duration: fade_duration,
        ..Default::default()
      },
    );

    Ok(MusicHandle { handle })
  }

  /// Crossfade from current music to new music
  ///
  /// Fades out the current music while fading in the new music.
  /// Returns the handle to the new music track.
  ///
  /// # Example
  /// ```ignore
  /// // Crossfade to new track over 2 seconds
  /// let new_music = audio.crossfade_music(
  ///     &mut current_music,
  ///     "assets/music/boss.ogg",
  ///     true,
  ///     Duration::from_secs(2),
  /// )?;
  /// ```
  pub fn crossfade_music(
    &mut self,
    current: &mut MusicHandle,
    new_path: impl AsRef<Path>,
    looping: bool,
    fade_duration: Duration,
  ) -> AudioResult<MusicHandle> {
    // Fade out the current track
    current.fade_out_and_stop(fade_duration);

    // Start the new track with fade in
    self.play_music_with_fade_in(new_path, looping, fade_duration)
  }

  /// Set master volume (affects all audio, 0.0 - 1.0)
  pub fn set_master_volume(&mut self, volume: f32) {
    self.master_volume = volume.clamp(0.0, 1.0);
  }

  /// Get the current master volume
  pub fn master_volume(&self) -> f32 {
    self.master_volume
  }

  /// Set sound effects volume (0.0 - 1.0)
  pub fn set_sfx_volume(&mut self, volume: f32) {
    self.sfx_volume = volume.clamp(0.0, 1.0);
  }

  /// Get the current sound effects volume
  pub fn sfx_volume(&self) -> f32 {
    self.sfx_volume
  }

  /// Set music volume (0.0 - 1.0)
  pub fn set_music_volume(&mut self, volume: f32) {
    self.music_volume = volume.clamp(0.0, 1.0);
  }

  /// Get the current music volume
  pub fn music_volume(&self) -> f32 {
    self.music_volume
  }

  /// Check if a sound is loaded
  pub fn has_sound(&self, name: &str) -> bool {
    self.sounds.contains_key(name)
  }

  /// Unload a cached sound to free memory
  pub fn unload_sound(&mut self, name: &str) {
    self.sounds.remove(name);
  }

  /// Unload all cached sounds
  pub fn clear_sounds(&mut self) {
    self.sounds.clear();
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn audio_error_display() {
    let err = AudioError::NotFound("boom.wav".to_string());
    assert!(err.to_string().contains("boom.wav"));
  }

  #[test]
  fn volume_clamping() {
    // We can't create a real AudioManager without audio device,
    // but we can test the clamping logic conceptually
    let volume = 1.5_f32.clamp(0.0, 1.0);
    assert_eq!(volume, 1.0);

    let volume = (-0.5_f32).clamp(0.0, 1.0);
    assert_eq!(volume, 0.0);
  }
}
