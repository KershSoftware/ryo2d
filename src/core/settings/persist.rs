//! Saving the settings: the settings file is written a second after the last
//! change to what it keeps, and at once when the game quits or is hidden.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use web_time::Instant;

use super::file::{self, FILE_NAME, SettingsFile};
use super::{EngineValues, Settings};
use crate::core::ecs::{Message, Resource};
use crate::core::saves::{SaveError, SaveService};

/// How long after the last change the settings file is written
pub(crate) const DEBOUNCE: Duration = Duration::from_secs(1);

/// The settings file could not be written. The settings still apply, and the
/// engine tries again at the next change and when the game quits; it warns
/// once a run, and sends this for every failed write
#[derive(Message, Clone, Debug)]
pub struct SettingsSaveFailed {
  /// Why (shared, since messages are cloned)
  pub error: Arc<SaveError>,
}

/// Writes the settings file when what it keeps changes: a second after the
/// last change (`tick`), or at once (`flush_now`). It compares the file's
/// text, so a change to a setting the file does not keep, or one undone
/// before it is written, writes nothing
#[derive(Resource)]
pub(crate) struct SettingsWriter {
  /// The file was not fully read at start-up (a newer format, a damaged file
  /// that could not be set aside, or one that could not be read), so it is
  /// never written this session
  pub(crate) read_only: bool,
  /// A failure or the read-only file has been warned about this run
  warned: bool,
  /// The file as read, whose unknown keys and sections a write carries
  /// over, with the player's bindings as last set
  base: SettingsFile,
  /// The file's text as last seen by `observe`
  noted: Vec<u8>,
  /// The engine values `noted` was rendered from; `None` once the bindings
  /// changed since
  seen: Option<EngineValues>,
  /// When what the file keeps last changed, while that is not written
  changed_at: Option<Instant>,
  /// The file's text as it stands: as read at start-up, or as the last
  /// write that landed left it
  stored: Vec<u8>,
  /// The latest write queued, until its result comes back
  queued: Option<Queued>,
}

struct Queued {
  id: u64,
  text: Vec<u8>,
}

impl SettingsWriter {
  /// A writer over `base`, the file as read (empty without one), for
  /// `settings` as they start
  pub(crate) fn new(base: SettingsFile, read_only: bool, settings: &Settings) -> SettingsWriter {
    let text = file::render(&settings.to_file(&base));
    SettingsWriter {
      read_only,
      warned: false,
      base,
      noted: text.clone(),
      seen: Some(*settings.values()),
      changed_at: None,
      stored: text,
      queued: None,
    }
  }

  /// What the file keeps changed at `now`: it is written a second later,
  /// unless it changes again
  pub(crate) fn note_change(&mut self, now: Instant) {
    self.changed_at = Some(now);
  }

  /// `settings` changed: notes a change when the file they give differs
  /// from the one last seen. With no game section, engine values equal to
  /// those last seen are no change, so a game that sets a setting every
  /// frame costs a comparison, not a render
  pub(crate) fn observe(&mut self, now: Instant, settings: &Settings) {
    let values = *settings.values();
    if self.seen == Some(values) && !settings.has_sections() {
      return;
    }
    self.seen = Some(values);
    let text = self.render(settings);
    if text != self.noted {
      self.noted = text;
      self.note_change(now);
    }
  }

  /// The player's bindings that differ from the defaults, for the next
  /// write; `observe` then sees whether the file changed
  pub(crate) fn set_bindings(&mut self, map: BTreeMap<String, Vec<String>>) {
    self.base.bindings = (!map.is_empty()).then_some(map);
    self.seen = None;
  }

  /// Whether the last change is a second old and not yet written
  pub(crate) fn is_due(&self, now: Instant) -> bool {
    self
      .changed_at
      .is_some_and(|at| now.saturating_duration_since(at) >= DEBOUNCE)
  }

  /// Writes the file once the last change is a second old; true when a
  /// write was queued
  pub(crate) fn tick(&mut self, now: Instant, settings: &Settings, saves: &SaveService) -> bool {
    self.is_due(now) && self.write(settings, saves)
  }

  /// Writes the file now if it differs from the one stored (the game quits
  /// or is hidden): built from `settings` as they are, so a change no frame
  /// has seen is kept too
  pub(crate) fn flush_now(&mut self, settings: &Settings, saves: &SaveService) {
    self.write(settings, saves);
  }

  /// A write's result came back: a success is the file as it stands; a
  /// failure warns once a run and gives the message to send, and the next
  /// change or flush writes again
  pub(crate) fn finished(
    &mut self,
    id: u64,
    result: &Result<(), Arc<SaveError>>,
  ) -> Option<SettingsSaveFailed> {
    let latest = self.queued.as_ref().is_some_and(|queued| queued.id == id);
    let queued = if latest { self.queued.take() } else { None };
    match result {
      Ok(()) => {
        if let Some(queued) = queued {
          self.stored = queued.text;
        }
        None
      }
      Err(error) => {
        if !std::mem::replace(&mut self.warned, true) {
          crate::engine_warn!(
            module: "Settings",
            "could not save the settings ({}); they apply, and saving is tried again at the next change and at quit. This warns once",
            error_chain(error.as_ref())
          );
        }
        Some(SettingsSaveFailed {
          error: Arc::clone(error),
        })
      }
    }
  }

  fn render(&self, settings: &Settings) -> Vec<u8> {
    file::render(&settings.to_file(&self.base))
  }

  /// Queues the file `settings` give, when it differs from the latest
  /// queued or stored; true when it was queued
  fn write(&mut self, settings: &Settings, saves: &SaveService) -> bool {
    self.changed_at = None;
    let text = self.render(settings);
    self.noted.clone_from(&text);
    self.seen = Some(*settings.values());
    let latest = self.queued.as_ref().map_or(&self.stored, |queued| &queued.text);
    if text == *latest {
      return false;
    }
    if self.read_only {
      if !std::mem::replace(&mut self.warned, true) {
        crate::engine_warn!(
          module: "Settings",
          "{FILE_NAME} was not fully read at start-up (it is newer, damaged or unreadable), so the settings changed this session are not saved"
        );
      }
      return false;
    }
    let id = saves.write_named(FILE_NAME, text.clone(), file::verify);
    self.queued = Some(Queued { id, text });
    true
  }
}

/// An error and its causes: "outer: inner: innermost"
fn error_chain(error: &dyn std::error::Error) -> String {
  let mut text = error.to_string();
  let mut source = error.source();
  while let Some(cause) = source {
    text.push_str(": ");
    text.push_str(&cause.to_string());
    source = cause.source();
  }
  text
}

#[cfg(test)]
mod tests {
  use serde_json::{Value, json};

  use super::*;
  use crate::core::audio::VolumeKind;
  use crate::core::config::UserPreferenceFilter;
  use crate::core::saves::MemorySaves;
  use crate::core::saves::memory::FaultOp;
  use crate::core::saves::versions::VersionRegistry;
  use crate::core::settings::EngineValues;
  use crate::core::settings::sections::SectionRegistry;

  const FRAME: Duration = Duration::from_millis(16);

  fn settings() -> Settings {
    Settings::new(
      EngineValues::default(),
      UserPreferenceFilter::all(),
      SectionRegistry::default(),
    )
  }

  fn saves(store: &MemorySaves) -> SaveService {
    let saves = SaveService::new(VersionRegistry::default());
    saves.set_storage(Arc::new(store.clone()));
    saves
  }

  /// Waits for the queued writes
  fn settle(saves: &SaveService) {
    let unfinished = saves.flush(Duration::from_secs(5), Duration::from_secs(10));
    assert!(unfinished.is_empty(), "still writing {unfinished:?}");
  }

  fn written(store: &MemorySaves) -> Value {
    serde_json::from_slice(&store.get("system.json").expect("system.json is written")).unwrap()
  }

  #[test]
  fn a_slider_drag_over_sixty_frames_writes_the_file_once() {
    let store = MemorySaves::new();
    let saves = saves(&store);
    let mut settings = settings();
    let mut writer = SettingsWriter::new(SettingsFile::default(), false, &settings);
    let start = Instant::now();
    let mut now = start;
    for frame in 0..60 {
      now = start + FRAME * frame;
      settings.set_volume(VolumeKind::Music, 1.0 - frame as f32 / 100.0);
      writer.observe(now, &settings);
      assert!(!writer.tick(now, &settings, &saves), "frame {frame}");
    }
    assert!(!writer.tick(now + DEBOUNCE - FRAME, &settings, &saves));
    settle(&saves);
    assert_eq!(store.writes(), 0, "not while the slider moves");

    assert!(writer.tick(now + DEBOUNCE, &settings, &saves));
    assert!(!writer.tick(now + DEBOUNCE * 2, &settings, &saves));
    settle(&saves);
    assert_eq!(store.writes(), 1);
    assert_eq!(
      written(&store),
      json!({"format_version": 3, "music_volume": 0.41})
    );
  }

  #[test]
  fn a_setting_set_again_every_frame_writes_a_second_after_it_last_changed() {
    let store = MemorySaves::new();
    let saves = saves(&store);
    let mut settings = settings();
    let mut writer = SettingsWriter::new(SettingsFile::default(), false, &settings);
    let start = Instant::now();
    settings.set_volume(VolumeKind::Sfx, 0.5);
    writer.observe(start, &settings);
    // A game that sets the same value each frame changes nothing
    for frame in 1..=120 {
      let now = start + FRAME * frame;
      settings.set_volume(VolumeKind::Sfx, 0.5);
      writer.observe(now, &settings);
      writer.tick(now, &settings, &saves);
    }
    settle(&saves);
    assert_eq!(store.writes(), 1);
    assert_eq!(written(&store)["sfx_volume"], json!(0.5));
  }

  #[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
  struct Video {
    subtitles: bool,
  }

  #[test]
  fn a_game_section_change_is_written_like_any_other() {
    let store = MemorySaves::new();
    let saves = saves(&store);
    let mut sections = SectionRegistry::default();
    sections.register::<Video>("video", None);
    let mut settings = Settings::new(
      EngineValues::default(),
      UserPreferenceFilter::none(),
      sections,
    );
    let mut writer = SettingsWriter::new(SettingsFile::default(), false, &settings);
    let start = Instant::now();
    writer.observe(start, &settings);
    assert!(!writer.tick(start + DEBOUNCE, &settings, &saves), "unchanged");
    settings.game_mut::<Video>().subtitles = true;
    writer.observe(start + DEBOUNCE, &settings);
    assert!(writer.tick(start + DEBOUNCE * 2, &settings, &saves));
    settle(&saves);
    assert_eq!(
      written(&store),
      json!({"format_version": 3, "game": {"video": {"version": 1, "value": {"subtitles": true}}}})
    );
  }

  #[test]
  fn a_change_undone_within_the_second_writes_nothing() {
    let store = MemorySaves::new();
    let saves = saves(&store);
    let mut settings = settings();
    let mut writer = SettingsWriter::new(SettingsFile::default(), false, &settings);
    let start = Instant::now();
    settings.set_vsync(false);
    writer.observe(start, &settings);
    settings.set_vsync(true);
    writer.observe(start + FRAME, &settings);
    assert!(!writer.tick(start + FRAME + DEBOUNCE, &settings, &saves));
    writer.flush_now(&settings, &saves);
    settle(&saves);
    assert_eq!(store.writes(), 0);
    assert_eq!(store.get("system.json"), None);
  }

  #[test]
  fn a_failed_write_is_tried_again_at_the_next_flush() {
    let store = MemorySaves::new();
    let saves = saves(&store);
    let mut settings = settings();
    let mut writer = SettingsWriter::new(SettingsFile::default(), false, &settings);
    // Warned already this run: the engine's test counts that warning
    writer.warned = true;
    store.fail(FaultOp::Write, 0);
    let start = Instant::now();
    settings.set_volume(VolumeKind::Ui, 0.5);
    writer.observe(start, &settings);
    assert!(writer.tick(start + DEBOUNCE, &settings, &saves));
    settle(&saves);
    let failed: Vec<_> = saves
      .drain()
      .iter()
      .filter_map(|finished| writer.finished(finished.id, &finished.result))
      .collect();
    assert_eq!(failed.len(), 1);
    assert!(
      matches!(&*failed[0].error, SaveError::Io { path, .. } if path == "system.json"),
      "{:?}",
      failed[0].error
    );
    assert_eq!(store.get("system.json"), None);

    // Quitting tries again, and a write that landed is not written again
    writer.flush_now(&settings, &saves);
    settle(&saves);
    for finished in saves.drain() {
      assert!(writer.finished(finished.id, &finished.result).is_none());
    }
    assert_eq!(written(&store)["ui_volume"], json!(0.5));
    writer.flush_now(&settings, &saves);
    settle(&saves);
    assert_eq!(store.writes(), 2);
  }
}
