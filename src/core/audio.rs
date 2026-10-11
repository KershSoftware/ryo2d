//! Audio system for sound effects and music
//!
//! This module provides:
//! - `AudioManager` - Manages audio device and playback
//! - Sound effect playback with volume, panning, and playback rate control
//! - Background music with looping and crossfading
//!
//! # Volume System
//!
//! Volumes are multiplicative across three levels:
//! 1. **Master volume** - Global volume applied to Kira's main track (affects all audio)
//! 2. **Category volume** - Separate volumes for music and SFX (applied per-sound)
//! 3. **Sound volume** - Per-sound volume passed to `play_sound`/`play_music`
//!
//! Example: If master is 0.5, SFX is 0.5, and a sound is played at 0.5,
//! the effective volume is 0.5 × 0.5 × 0.5 = 0.125 (12.5%)
//!
//! Note: Master volume is applied at the main track level for real-time control
//! of all audio (including already-playing sounds). Category and sound volumes
//! are applied per-sound at play time.

use kira::{
  AudioManager as KiraAudioManager, AudioManagerSettings, Decibels, Tween,
  sound::{
    FromFileError, PlaybackState,
    static_sound::{StaticSoundData, StaticSoundHandle, StaticSoundSettings},
    streaming::{StreamingSoundData, StreamingSoundHandle, StreamingSoundSettings},
  },
};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use crate::core::error::{AudioError, AudioResult};

// Implement From<FromFileError> for AudioError here since kira is only available in this module
impl From<FromFileError> for AudioError {
  fn from(err: FromFileError) -> Self {
    AudioError::LoadError(format!("{}", err))
  }
}

/// Convert linear amplitude (0.0 - 1.0) to decibels
fn amplitude_to_db(amplitude: f32) -> Decibels {
  if amplitude <= 0.0 {
    Decibels::SILENCE
  } else {
    // dB = 20 * log10(amplitude)
    Decibels(20.0 * amplitude.log10())
  }
}

/// Options for playing a sound effect.
///
/// All fields are optional and will use sensible defaults if not specified.
#[derive(Debug, Clone)]
pub struct SoundOptions {
  /// Volume multiplier (0.0 - 1.0). Default: 1.0
  ///
  /// This is multiplied with master and SFX category volumes.
  pub volume: f32,
  /// Stereo panning (-1.0 = left, 0.0 = center, 1.0 = right). Default: 0.0
  pub panning: f32,
  /// Playback rate multiplier (1.0 = normal, 0.5 = half speed, 2.0 = double). Default: 1.0
  pub playback_rate: f32,
}

impl Default for SoundOptions {
  fn default() -> Self {
    Self {
      volume: 1.0,
      panning: 0.0,
      playback_rate: 1.0,
    }
  }
}

impl SoundOptions {
  /// Create new sound options with default values.
  pub fn new() -> Self {
    Self::default()
  }

  /// Set the volume multiplier.
  pub fn volume(mut self, volume: f32) -> Self {
    self.volume = volume.clamp(0.0, 1.0);
    self
  }

  /// Set the stereo panning.
  pub fn panning(mut self, panning: f32) -> Self {
    self.panning = panning.clamp(-1.0, 1.0);
    self
  }

  /// Set the playback rate.
  pub fn playback_rate(mut self, rate: f32) -> Self {
    self.playback_rate = rate.max(0.0);
    self
  }
}

/// Options for playing background music.
///
/// All fields are optional and will use sensible defaults if not specified.
#[derive(Debug, Clone)]
pub struct MusicOptions {
  /// Volume multiplier (0.0 - 1.0). Default: 1.0
  ///
  /// This is multiplied with master and music category volumes.
  pub volume: f32,
  /// Stereo panning (-1.0 = left, 0.0 = center, 1.0 = right). Default: 0.0
  pub panning: f32,
  /// Playback rate multiplier (1.0 = normal). Default: 1.0
  pub playback_rate: f32,
  /// Whether to loop the music. Default: true
  pub looping: bool,
  /// Duration to fade in from silence. Default: None (no fade)
  pub fade_in: Option<Duration>,
  /// Duration to fade out when scene exits. Default: None (no auto-fade)
  pub fade_out_on_drop: Option<Duration>,
}

impl Default for MusicOptions {
  fn default() -> Self {
    Self {
      volume: 1.0,
      panning: 0.0,
      playback_rate: 1.0,
      looping: true,
      fade_in: None,
      fade_out_on_drop: None,
    }
  }
}

impl MusicOptions {
  /// Create new music options with default values.
  pub fn new() -> Self {
    Self::default()
  }

  /// Set the volume multiplier.
  pub fn volume(mut self, volume: f32) -> Self {
    self.volume = volume.clamp(0.0, 1.0);
    self
  }

  /// Set the stereo panning.
  pub fn panning(mut self, panning: f32) -> Self {
    self.panning = panning.clamp(-1.0, 1.0);
    self
  }

  /// Set the playback rate.
  pub fn playback_rate(mut self, rate: f32) -> Self {
    self.playback_rate = rate.max(0.0);
    self
  }

  /// Set whether to loop.
  pub fn looping(mut self, looping: bool) -> Self {
    self.looping = looping;
    self
  }

  /// Set fade-in duration.
  pub fn fade_in(mut self, duration: Duration) -> Self {
    self.fade_in = Some(duration);
    self
  }

  /// Disable looping (play once).
  pub fn once(mut self) -> Self {
    self.looping = false;
    self
  }

  /// Set fade-out duration when the handle is dropped (scene exit).
  pub fn fade_out_on_drop(mut self, duration: Duration) -> Self {
    self.fade_out_on_drop = Some(duration);
    self
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
  pub fn set_volume(&mut self, volume: f32) {
    self
      .handle
      .set_volume(amplitude_to_db(volume), Tween::default());
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
  /// Original volume multiplier (before category/master volumes)
  reference_volume: f32,
  /// Fade-out duration when dropped (if set)
  fade_out_on_drop: Option<Duration>,
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
  ///
  /// This sets the absolute volume, bypassing category volumes.
  /// Use `update_category_volume()` to apply category volume changes.
  pub fn set_volume(&mut self, volume: f32) {
    self
      .handle
      .set_volume(amplitude_to_db(volume), Tween::default());
  }

  /// Update the volume based on current music category volume.
  ///
  /// Call this when music category volume changes.
  /// Note: master_volume is applied at the main track level, not here.
  pub fn update_category_volume(&mut self, music_volume: f32) {
    let final_volume = music_volume * self.reference_volume;
    self.set_volume(final_volume);
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
  pub fn fade_to(&mut self, target_volume: f32, duration: Duration) {
    self.handle.set_volume(
      amplitude_to_db(target_volume),
      Tween {
        duration,
        ..Default::default()
      },
    );
  }

  /// Fade out to silence over a duration
  pub fn fade_out(&mut self, duration: Duration) {
    self.fade_to(0.0, duration);
  }

  /// Stop the music with a fade out
  pub fn fade_out_and_stop(&mut self, duration: Duration) {
    self.handle.stop(Tween {
      duration,
      ..Default::default()
    });
  }
}

impl Drop for MusicHandle {
  fn drop(&mut self) {
    // Auto-fade-out if configured
    if let Some(fade_duration) = self.fade_out_on_drop {
      self.fade_out_and_stop(fade_duration);
    }
  }
}

/// Main audio manager for the engine
///
/// Handles loading and playing sound effects and music.
///
/// # Volume Hierarchy
///
/// The final volume of any sound is calculated as:
/// `master_volume × category_volume × sound_volume`
///
/// Where:
/// - `master_volume` affects all audio (applied to Kira's main track for real-time control)
/// - `category_volume` is either `sfx_volume` or `music_volume` (applied per-sound)
/// - `sound_volume` is the volume passed to `play_sound`/`play_music` (applied per-sound)
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
  /// Currently playing music track (if any)
  active_music: Option<MusicHandle>,
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
      active_music: None,
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

  /// Calculate the effective volume for a sound effect.
  /// Note: master_volume is applied at the main track level, not here.
  fn effective_sfx_volume(&self, sound_volume: f32) -> f32 {
    self.sfx_volume * sound_volume
  }

  /// Calculate the effective volume for music.
  /// Note: master_volume is applied at the main track level, not here.
  fn effective_music_volume(&self, music_volume: f32) -> f32 {
    self.music_volume * music_volume
  }

  // =========================================================================
  // Sound Effects
  // =========================================================================

  /// Play a cached sound effect with default options.
  ///
  /// Returns a handle to control the playing sound.
  ///
  /// # Example
  /// ```ignore
  /// audio.load_sound("explosion", "assets/sounds/boom.wav")?;
  /// audio.play_sound("explosion")?;
  /// ```
  pub fn play_sound(&mut self, name: &str) -> AudioResult<SoundHandle> {
    self.play_sound_with(name, SoundOptions::default())
  }

  /// Play a cached sound effect with custom options.
  ///
  /// # Example
  /// ```ignore
  /// audio.load_sound("explosion", "assets/sounds/boom.wav")?;
  ///
  /// // Play at half volume, panned left
  /// audio.play_sound_with("explosion", SoundOptions::new()
  ///     .volume(0.5)
  ///     .panning(-0.5)
  /// )?;
  /// ```
  pub fn play_sound_with(&mut self, name: &str, options: SoundOptions) -> AudioResult<SoundHandle> {
    let sound_data = self
      .sounds
      .get(name)
      .ok_or_else(|| AudioError::NotFound(name.to_string()))?
      .clone();

    let final_volume = self.effective_sfx_volume(options.volume);
    let volume_db = amplitude_to_db(final_volume);

    let settings = StaticSoundSettings::default()
      .volume(volume_db)
      .panning(options.panning)
      .playback_rate(options.playback_rate as f64);

    let sound_with_settings = sound_data.with_settings(settings);

    let handle = self
      .manager
      .play(sound_with_settings)
      .map_err(|e| AudioError::PlayError(format!("{}", e)))?;

    Ok(SoundHandle { handle })
  }

  // =========================================================================
  // Music
  // =========================================================================

  /// Play background music from a file with default options (looping, no fade).
  ///
  /// Music is streamed from disk, making it memory-efficient for long audio files.
  ///
  /// # Example
  /// ```ignore
  /// let music = audio.play_music("assets/music/theme.mp3")?;
  /// ```
  pub fn play_music(&mut self, path: impl AsRef<Path>) -> AudioResult<MusicHandle> {
    self.play_music_with(path, MusicOptions::default())
  }

  /// Play background music from a file with custom options.
  ///
  /// # Example
  /// ```ignore
  /// // Play once (no loop) with fade-in
  /// let music = audio.play_music_with("assets/music/intro.mp3", MusicOptions::new()
  ///     .once()
  ///     .fade_in(Duration::from_secs(2))
  /// )?;
  /// ```
  pub fn play_music_with(
    &mut self,
    path: impl AsRef<Path>,
    options: MusicOptions,
  ) -> AudioResult<MusicHandle> {
    let final_volume = self.effective_music_volume(options.volume);

    // Build settings based on fade_in option
    let mut settings = if let Some(fade_duration) = options.fade_in {
      // Start at silence and fade in
      StreamingSoundSettings::default()
        .volume(Decibels::SILENCE)
        .fade_in_tween(Tween {
          duration: fade_duration,
          ..Default::default()
        })
    } else {
      StreamingSoundSettings::default().volume(amplitude_to_db(final_volume))
    };

    settings = settings
      .panning(options.panning)
      .playback_rate(options.playback_rate as f64);

    if options.looping {
      settings = settings.loop_region(..);
    }

    let sound_data = StreamingSoundData::from_file(path.as_ref())
      .map_err(|e| AudioError::LoadError(format!("{}", e)))?
      .with_settings(settings);

    let mut handle = self
      .manager
      .play(sound_data)
      .map_err(|e| AudioError::PlayError(format!("{}", e)))?;

    // If fading in, set the target volume
    if let Some(fade_duration) = options.fade_in {
      handle.set_volume(
        amplitude_to_db(final_volume),
        Tween {
          duration: fade_duration,
          ..Default::default()
        },
      );
    }

    Ok(MusicHandle {
      handle,
      reference_volume: options.volume,
      fade_out_on_drop: options.fade_out_on_drop,
    })
  }

  /// Crossfade from current music to new music.
  ///
  /// Fades out the current music while fading in the new music.
  /// Returns the handle to the new music track.
  ///
  /// The new music will automatically fade out on drop (500ms).
  ///
  /// # Example
  /// ```ignore
  /// let new_music = audio.crossfade_music(
  ///     &mut current_music,
  ///     "assets/music/boss.ogg",
  ///     Duration::from_secs(2),
  /// )?;
  /// ```
  pub fn crossfade_music(
    &mut self,
    current: &mut MusicHandle,
    new_path: impl AsRef<Path>,
    fade_duration: Duration,
  ) -> AudioResult<MusicHandle> {
    // Fade out the current track
    current.fade_out_and_stop(fade_duration);

    // Start the new track with fade in and auto-fade-out on drop
    self.play_music_with(
      new_path,
      MusicOptions::new()
        .fade_in(fade_duration)
        .fade_out_on_drop(Duration::from_millis(500)),
    )
  }

  /// Crossfade from current music to new music with custom options.
  pub fn crossfade_music_with(
    &mut self,
    current: &mut MusicHandle,
    new_path: impl AsRef<Path>,
    fade_duration: Duration,
    options: MusicOptions,
  ) -> AudioResult<MusicHandle> {
    // Fade out the current track
    current.fade_out_and_stop(fade_duration);

    // Merge fade_in into options
    let options = MusicOptions {
      fade_in: Some(fade_duration),
      ..options
    };

    self.play_music_with(new_path, options)
  }

  // =========================================================================
  // Volume Controls
  // =========================================================================

  /// Set master volume (affects all audio, 0.0 - 1.0)
  ///
  /// This is applied to Kira's main track for real-time effect on playing sounds.
  pub fn set_master_volume(&mut self, volume: f32) {
    self.master_volume = volume.clamp(0.0, 1.0);
    // Update Kira's main track volume for real-time effect
    self
      .manager
      .main_track()
      .set_volume(amplitude_to_db(self.master_volume), Tween::default());
  }

  /// Get the current master volume
  pub fn master_volume(&self) -> f32 {
    self.master_volume
  }

  /// Set sound effects volume (0.0 - 1.0)
  ///
  /// Note: This affects new sounds. Already-playing sounds keep their original volume.
  pub fn set_sfx_volume(&mut self, volume: f32) {
    self.sfx_volume = volume.clamp(0.0, 1.0);
  }

  /// Get the current sound effects volume
  pub fn sfx_volume(&self) -> f32 {
    self.sfx_volume
  }

  /// Set music volume (0.0 - 1.0)
  ///
  /// Note: This affects new music. Already-playing music keeps its original volume.
  /// Use `MusicHandle::set_volume()` to change volume of playing music.
  pub fn set_music_volume(&mut self, volume: f32) {
    self.music_volume = volume.clamp(0.0, 1.0);
  }

  /// Get the current music volume
  pub fn music_volume(&self) -> f32 {
    self.music_volume
  }

  // =========================================================================
  // Sound Management
  // =========================================================================

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

  // =========================================================================
  // Music Management
  // =========================================================================

  /// Check if music is currently playing
  pub fn is_music_playing(&self) -> bool {
    self.active_music.as_ref().is_some_and(|m| m.is_playing())
  }

  /// Stop the currently playing music (if any)
  pub fn stop_music(&mut self) {
    self.active_music = None;
  }

  /// Stop music with fade out
  pub fn stop_music_fade(&mut self, duration: Duration) {
    if let Some(music) = &mut self.active_music {
      music.fade_out_and_stop(duration);
    }
    self.active_music = None;
  }

  /// Play music only if no music is currently playing.
  ///
  /// This is useful for persistent background music that should continue
  /// across scene transitions. Returns `Ok(true)` if music was started,
  /// `Ok(false)` if music was already playing.
  ///
  /// # Example
  /// ```ignore
  /// // In menu and game scenes - music persists across both
  /// if let Some(audio) = &mut ctx.audio {
  ///   audio.play_music_once(
  ///     "assets/music/theme.mp3",
  ///     MusicOptions::new().volume(0.3).looping(true)
  ///   )?;
  /// }
  /// ```
  pub fn play_music_once(
    &mut self,
    path: impl AsRef<Path>,
    options: MusicOptions,
  ) -> AudioResult<bool> {
    // If music is already playing, don't start a new track
    if self.is_music_playing() {
      return Ok(false);
    }

    let handle = self.play_music_with(path, options)?;
    self.active_music = Some(handle);
    Ok(true)
  }

  /// Get a mutable reference to the currently playing music
  pub fn current_music(&mut self) -> Option<&mut MusicHandle> {
    self.active_music.as_mut()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn audio_error_display() {
    let err = AudioError::NotFound("boom.wav".to_string());
    assert!(err.to_string().contains("boom.wav"));

    let err2 = AudioError::LoadError("decode failed".to_string());
    assert!(err2.to_string().contains("decode failed"));
  }

  #[test]
  fn volume_clamping() {
    let volume = 1.5_f32.clamp(0.0, 1.0);
    assert_eq!(volume, 1.0);

    let volume = (-0.5_f32).clamp(0.0, 1.0);
    assert_eq!(volume, 0.0);
  }

  #[test]
  fn effective_volume_is_multiplicative() {
    // Test that volumes multiply correctly: master × category × sound
    let master = 0.5;
    let sfx = 0.5;
    let sound = 0.5;

    let effective = master * sfx * sound;
    assert_eq!(effective, 0.125); // 0.5 × 0.5 × 0.5 = 0.125
  }

  #[test]
  fn sound_options_builder() {
    let opts = SoundOptions::new()
      .volume(0.5)
      .panning(-0.5)
      .playback_rate(1.5);

    assert_eq!(opts.volume, 0.5);
    assert_eq!(opts.panning, -0.5);
    assert_eq!(opts.playback_rate, 1.5);
  }

  #[test]
  fn sound_options_defaults() {
    let opts = SoundOptions::default();
    assert_eq!(opts.volume, 1.0);
    assert_eq!(opts.panning, 0.0);
    assert_eq!(opts.playback_rate, 1.0);
  }

  #[test]
  fn sound_options_volume_clamps() {
    let high = SoundOptions::new().volume(2.0);
    assert_eq!(high.volume, 1.0);

    let low = SoundOptions::new().volume(-0.5);
    assert_eq!(low.volume, 0.0);
  }

  #[test]
  fn sound_options_panning_clamps() {
    let far_left = SoundOptions::new().panning(-5.0);
    assert_eq!(far_left.panning, -1.0);

    let far_right = SoundOptions::new().panning(5.0);
    assert_eq!(far_right.panning, 1.0);
  }

  #[test]
  fn sound_options_playback_rate_clamps() {
    let negative = SoundOptions::new().playback_rate(-0.5);
    assert_eq!(negative.playback_rate, 0.0);

    let normal = SoundOptions::new().playback_rate(2.0);
    assert_eq!(normal.playback_rate, 2.0);
  }

  #[test]
  fn music_options_builder() {
    let opts = MusicOptions::new()
      .volume(0.8)
      .once()
      .fade_in(Duration::from_secs(2));

    assert_eq!(opts.volume, 0.8);
    assert!(!opts.looping);
    assert_eq!(opts.fade_in, Some(Duration::from_secs(2)));
  }

  #[test]
  fn music_options_defaults() {
    let opts = MusicOptions::default();
    assert_eq!(opts.volume, 1.0);
    assert_eq!(opts.panning, 0.0);
    assert_eq!(opts.playback_rate, 1.0);
    assert!(opts.looping); // default is to loop
    assert!(opts.fade_in.is_none());
    assert!(opts.fade_out_on_drop.is_none());
  }

  #[test]
  fn music_options_volume_clamps() {
    let high = MusicOptions::new().volume(1.5);
    assert_eq!(high.volume, 1.0);

    let low = MusicOptions::new().volume(-0.2);
    assert_eq!(low.volume, 0.0);
  }

  #[test]
  fn music_options_panning_clamps() {
    let far_left = MusicOptions::new().panning(-10.0);
    assert_eq!(far_left.panning, -1.0);

    let far_right = MusicOptions::new().panning(10.0);
    assert_eq!(far_right.panning, 1.0);
  }

  #[test]
  fn music_options_playback_rate_clamps() {
    let negative = MusicOptions::new().playback_rate(-1.0);
    assert_eq!(negative.playback_rate, 0.0);
  }

  #[test]
  fn music_options_looping() {
    let looping = MusicOptions::new().looping(true);
    assert!(looping.looping);

    let once = MusicOptions::new().looping(false);
    assert!(!once.looping);
  }

  #[test]
  fn music_options_once_convenience() {
    let opts = MusicOptions::new().once();
    assert!(!opts.looping);
  }

  #[test]
  fn music_options_fade_in() {
    let opts = MusicOptions::new().fade_in(Duration::from_millis(500));
    assert_eq!(opts.fade_in, Some(Duration::from_millis(500)));
  }

  #[test]
  fn music_options_fade_out_on_drop() {
    let opts = MusicOptions::new().fade_out_on_drop(Duration::from_secs(1));
    assert_eq!(opts.fade_out_on_drop, Some(Duration::from_secs(1)));
  }

  #[test]
  fn options_clamp_values() {
    let sound = SoundOptions::new().volume(2.0).panning(5.0);
    assert_eq!(sound.volume, 1.0);
    assert_eq!(sound.panning, 1.0);

    let music = MusicOptions::new().volume(-0.5).panning(-10.0);
    assert_eq!(music.volume, 0.0);
    assert_eq!(music.panning, -1.0);
  }

  #[test]
  fn amplitude_to_db_zero() {
    let db = amplitude_to_db(0.0);
    assert_eq!(db, Decibels::SILENCE);
  }

  #[test]
  fn amplitude_to_db_one() {
    let db = amplitude_to_db(1.0);
    // 20 * log10(1.0) = 0.0 dB
    assert!((db.0 - 0.0).abs() < 0.001);
  }

  #[test]
  fn amplitude_to_db_half() {
    let db = amplitude_to_db(0.5);
    // 20 * log10(0.5) ≈ -6.02 dB
    assert!((db.0 + 6.02).abs() < 0.1);
  }

  #[test]
  fn amplitude_to_db_negative_is_silence() {
    let db = amplitude_to_db(-0.5);
    assert_eq!(db, Decibels::SILENCE);
  }

  #[test]
  fn music_options_chaining() {
    let opts = MusicOptions::new()
      .volume(0.7)
      .panning(0.3)
      .playback_rate(1.1)
      .once()
      .fade_in(Duration::from_millis(200))
      .fade_out_on_drop(Duration::from_millis(100));

    assert_eq!(opts.volume, 0.7);
    assert_eq!(opts.panning, 0.3);
    assert_eq!(opts.playback_rate, 1.1);
    assert!(!opts.looping);
    assert_eq!(opts.fade_in, Some(Duration::from_millis(200)));
    assert_eq!(opts.fade_out_on_drop, Some(Duration::from_millis(100)));
  }

  #[test]
  fn sound_options_chaining() {
    let opts = SoundOptions::new()
      .volume(0.8)
      .panning(-0.4)
      .playback_rate(1.2);

    assert_eq!(opts.volume, 0.8);
    assert_eq!(opts.panning, -0.4);
    assert_eq!(opts.playback_rate, 1.2);
  }
}
