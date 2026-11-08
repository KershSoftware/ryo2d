//! The saves as a game reaches them: `ctx.saves()` ([`SavesView`]) in scenes
//! and the [`Saves`] parameter in systems, what they take and give, and the
//! [`SaveFinished`] message.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::Serialize;
use serde::de::DeserializeOwned;

use super::service::{JobKind, SaveService};
use super::{SaveError, SaveHeader};
use crate::core::ecs::kernel::{MessageWriter, Res, ResMut, SystemParam};
use crate::core::ecs::{Message, World};
use crate::core::settings::SettingsSaveFailed;
use crate::core::settings::file::FILE_NAME;
use crate::core::settings::persist::SettingsWriter;

/// What a save says about itself besides its data, for a slot list: a display
/// name, the play time and the game's labels
///
/// ```
/// use ryo2d::prelude::*;
///
/// let info = SaveInfo::new()
///   .name("Chapter two")
///   .play_time(5_400)
///   .label("area", "caves");
/// assert_eq!(info.labels["area"], "caves");
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SaveInfo {
  /// The display name (a slot's name is only `a`-`z`, `0`-`9`, `-` and `_`)
  pub name: Option<String>,
  /// The play time, in seconds
  pub play_time: u64,
  /// The game's own labels (an area, a character's level, ...)
  pub labels: BTreeMap<String, String>,
}

impl SaveInfo {
  /// No name, no play time and no labels
  pub fn new() -> Self {
    Self::default()
  }

  /// With the display name `name`
  pub fn name(mut self, name: impl Into<String>) -> Self {
    self.name = Some(name.into());
    self
  }

  /// With `secs` seconds of play time
  pub fn play_time(mut self, secs: u64) -> Self {
    self.play_time = secs;
    self
  }

  /// With the label `key` set to `value`
  pub fn label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
    self.labels.insert(key.into(), value.into());
    self
  }
}

/// A loaded save: the data, its header, whether it came from the backup and
/// the version it was upgraded from
#[derive(Debug, Clone)]
pub struct Loaded<T> {
  /// The data, upgraded to its type's current version
  pub data: T,
  /// What the save says about itself
  pub header: SaveHeader,
  /// The save was damaged, so this is its backup: the save before it
  pub from_backup: bool,
  /// The data's version in the save, when it was older than the type's
  /// current version and upgraded through the steps
  pub upgraded_from: Option<u32>,
}

/// A slot and its save's header, as [`SavesView::list`] gives them
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveSlot {
  /// The slot's name
  pub slot: String,
  /// What its save says about itself
  pub header: SaveHeader,
  /// The save was damaged, so this is its backup's header
  pub from_backup: bool,
}

/// What a [`SaveFinished`] reports on
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveOp {
  /// A `save` or `save_with`
  Save,
  /// A `delete`
  Delete,
}

/// A save or delete has finished: its slot, which it was, and whether it
/// worked. Every save and delete reports once, in a frame after its call.
/// When saves to one slot follow each other faster than they are written,
/// only the last is written, and the ones before it report `Ok`
#[derive(Message, Clone, Debug)]
pub struct SaveFinished {
  /// The slot
  pub slot: String,
  /// A save or a delete
  pub op: SaveOp,
  /// `Ok`, or why it failed (shared, since messages are cloned)
  pub result: Result<(), Arc<SaveError>>,
}

/// The game's saves, as a scene reaches them with `ctx.saves()` (a system
/// takes [`Saves`]). A save goes in a slot, by name, and holds any
/// `Serialize` value. `save` only checks the slot and queues the work: the
/// data is serialized and written on a thread of its own, crash-safe (a save
/// never destroys the last good one), and a [`SaveFinished`] message reports
/// the result. A load is immediate, and sees what is queued.
///
/// ```
/// use ryo2d::prelude::*;
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Serialize, Deserialize)]
/// struct Progress {
///   level: u32,
/// }
///
/// fn save_progress(ctx: &mut SceneContext<'_>, level: u32) {
///   if let Err(error) = ctx.saves().save("slot1", Progress { level }) {
///     game_warn!("could not save: {error}");
///   }
/// }
///
/// fn saved_level(ctx: &mut SceneContext<'_>) -> u32 {
///   ctx.saves().load::<Progress>("slot1").map_or(1, |saved| saved.data.level)
/// }
/// ```
///
/// A world without the engine's saves (one the engine did not make) has no
/// storage: saving and loading give `SaveError::Unavailable`.
pub struct SavesView<'w> {
  service: Option<&'w SaveService>,
}

impl<'w> SavesView<'w> {
  /// The saves of `world`
  pub fn new(world: &'w World) -> Self {
    Self {
      service: world.get_resource::<SaveService>(),
    }
  }

  /// Queues `data` to be saved in `slot`, a name of 1 to 64 characters of
  /// `a`-`z`, `0`-`9`, `-` and `_`. It fails at once only for a slot name
  /// that breaks that rule (`InvalidSlot`) or when there is nowhere to save
  /// (`Unavailable`); the write's own result comes as a [`SaveFinished`].
  /// Saves to one slot are written in call order
  pub fn save<T: Serialize + Send + 'static>(&self, slot: &str, data: T) -> Result<(), SaveError> {
    save(self.service, slot, data, SaveInfo::default())
  }

  /// [`save`](Self::save) with a display name, play time and labels for the
  /// slot list
  pub fn save_with<T: Serialize + Send + 'static>(
    &self,
    slot: &str,
    data: T,
    info: SaveInfo,
  ) -> Result<(), SaveError> {
    save(self.service, slot, data, info)
  }

  /// The save in `slot` as a `T`, upgraded from an older version through the
  /// steps registered with `EngineBuilder::save_versions`. A save still
  /// queued loads as it was queued. A damaged save loads its backup
  /// (`from_backup`, with a warning); when both are damaged it is
  /// `Corrupt`. A save written by a newer build is `NewerVersion`, and one
  /// that does not read as a `T` is `Deserialize`
  pub fn load<T: DeserializeOwned + 'static>(&self, slot: &str) -> Result<Loaded<T>, SaveError> {
    service(self.service)?.load(slot)
  }

  /// The header of the save in `slot`, read without its data
  pub fn info(&self, slot: &str) -> Result<SaveHeader, SaveError> {
    service(self.service)?.info(slot)
  }

  /// Every slot with its header, sorted by slot name, those queued included.
  /// A slot with no header to show (damaged, or written by a newer build) is
  /// left out
  pub fn list(&self) -> Result<Vec<SaveSlot>, SaveError> {
    service(self.service)?.list()
  }

  /// True when `slot` holds a save, or a save of it is queued
  pub fn exists(&self, slot: &str) -> bool {
    self.service.is_some_and(|service| service.exists(slot))
  }

  /// Queues the removal of the save in `slot` and its backup, after any save
  /// of it queued before; a [`SaveFinished`] reports the result
  pub fn delete(&self, slot: &str) -> Result<(), SaveError> {
    service(self.service)?.delete(slot)
  }

  /// True from a save or delete of `slot` until its [`SaveFinished`] is sent
  pub fn is_writing(&self, slot: &str) -> bool {
    self.service.is_some_and(|service| service.is_writing(slot))
  }
}

/// The game's saves, as a system reaches them: take it as a parameter
/// (`saves: Saves`) for what [`SavesView`] does in a scene. It reads the
/// engine's saves and changes nothing in the world, so systems that save run
/// beside each other
///
/// ```
/// use ryo2d::prelude::*;
/// use serde::Serialize;
///
/// #[derive(Resource, Serialize, Clone)]
/// struct Score(u32);
///
/// fn autosave(saves: Saves, score: Res<Score>) {
///   if score.is_changed() {
///     let _ = saves.save("autosave", score.clone());
///   }
/// }
/// ```
#[derive(SystemParam)]
pub struct Saves<'w> {
  service: Option<Res<'w, SaveService>>,
}

impl Saves<'_> {
  /// Queues `data` to be saved in `slot`, as [`SavesView::save`] does
  pub fn save<T: Serialize + Send + 'static>(&self, slot: &str, data: T) -> Result<(), SaveError> {
    save(self.service.as_deref(), slot, data, SaveInfo::default())
  }

  /// [`save`](Self::save) with a display name, play time and labels, as
  /// [`SavesView::save_with`] does
  pub fn save_with<T: Serialize + Send + 'static>(
    &self,
    slot: &str,
    data: T,
    info: SaveInfo,
  ) -> Result<(), SaveError> {
    save(self.service.as_deref(), slot, data, info)
  }

  /// The save in `slot` as a `T`, as [`SavesView::load`] gives it
  pub fn load<T: DeserializeOwned + 'static>(&self, slot: &str) -> Result<Loaded<T>, SaveError> {
    service(self.service.as_deref())?.load(slot)
  }

  /// The header of the save in `slot`, as [`SavesView::info`] gives it
  pub fn info(&self, slot: &str) -> Result<SaveHeader, SaveError> {
    service(self.service.as_deref())?.info(slot)
  }

  /// Every slot with its header, as [`SavesView::list`] gives them
  pub fn list(&self) -> Result<Vec<SaveSlot>, SaveError> {
    service(self.service.as_deref())?.list()
  }

  /// True when `slot` holds a save, or a save of it is queued
  pub fn exists(&self, slot: &str) -> bool {
    self
      .service
      .as_deref()
      .is_some_and(|service| service.exists(slot))
  }

  /// Queues the removal of the save in `slot`, as [`SavesView::delete`] does
  pub fn delete(&self, slot: &str) -> Result<(), SaveError> {
    service(self.service.as_deref())?.delete(slot)
  }

  /// True from a save or delete of `slot` until its [`SaveFinished`] is sent
  pub fn is_writing(&self, slot: &str) -> bool {
    self
      .service
      .as_deref()
      .is_some_and(|service| service.is_writing(slot))
  }
}

fn service(service: Option<&SaveService>) -> Result<&SaveService, SaveError> {
  service.ok_or(SaveError::Unavailable)
}

fn save<T: Serialize + Send + 'static>(
  service: Option<&SaveService>,
  slot: &str,
  data: T,
  info: SaveInfo,
) -> Result<(), SaveError> {
  self::service(service)?.save(slot, data, info)
}

/// Sends a [`SaveFinished`] for each save and delete the writer has finished,
/// and gives the settings file's results to the settings writer, which may
/// send a [`SettingsSaveFailed`] (`Phase::PreUpdate`)
pub(crate) fn drain_save_results(
  service: Option<Res<SaveService>>,
  mut settings_writer: Option<ResMut<SettingsWriter>>,
  mut messages: MessageWriter<SaveFinished>,
  mut settings_failed: MessageWriter<SettingsSaveFailed>,
) {
  let Some(service) = service else {
    return;
  };
  for finished in service.drain() {
    if let Some(message) = finished.message() {
      messages.write(message);
    } else if let JobKind::Named { name } = &finished.kind
      && name == FILE_NAME
      && let Some(writer) = settings_writer.as_mut()
      && let Some(failed) = writer.finished(finished.id, &finished.result)
    {
      settings_failed.write(failed);
    }
  }
}
