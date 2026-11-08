//! The game's own settings sections: typed values the game registers by key,
//! stored in the settings file with their versions.

use std::any::Any;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::file::{RawSection, bad_value};
use crate::core::saves::versions::VersionRegistry;
use crate::core::saves::{SaveError, SaveVersions};

/// The registered sections, each with its value
#[derive(Default)]
pub(crate) struct SectionRegistry {
  sections: Vec<Section>,
  versions: VersionRegistry,
}

struct Section {
  key: String,
  value: Box<dyn Erased>,
  /// What the file held for the section when it did not read (a newer
  /// build's version, say): written back while the section is at its
  /// default, so it survives this build
  unread: Option<RawSection>,
  /// Whether the section has warned that it does not serialize
  warned: AtomicBool,
}

/// A section's value with its type erased
trait Erased: Send + Sync {
  fn as_any(&self) -> &dyn Any;
  fn as_any_mut(&mut self) -> &mut dyn Any;
  fn reset(&mut self);
  fn json(&self) -> serde_json::Result<Value>;
  fn default_json(&self) -> serde_json::Result<Value>;
  /// The type's current version
  fn current(&self, versions: &VersionRegistry) -> u32;
  /// Replaces the value with `stored`, upgraded from `version`
  fn load(
    &mut self,
    versions: &VersionRegistry,
    key: &str,
    version: u32,
    stored: Value,
  ) -> Result<(), SaveError>;
}

struct Typed<T>(T);

impl<T: Default + Serialize + DeserializeOwned + Send + Sync + 'static> Erased for Typed<T> {
  fn as_any(&self) -> &dyn Any {
    &self.0
  }

  fn as_any_mut(&mut self) -> &mut dyn Any {
    &mut self.0
  }

  fn reset(&mut self) {
    self.0 = T::default();
  }

  fn json(&self) -> serde_json::Result<Value> {
    serde_json::to_value(&self.0)
  }

  fn default_json(&self) -> serde_json::Result<Value> {
    serde_json::to_value(T::default())
  }

  fn current(&self, versions: &VersionRegistry) -> u32 {
    versions.current::<T>()
  }

  fn load(
    &mut self,
    versions: &VersionRegistry,
    key: &str,
    version: u32,
    stored: Value,
  ) -> Result<(), SaveError> {
    self.0 = versions.decode_json::<T>(key, version, stored)?.0;
    Ok(())
  }
}

impl SectionRegistry {
  /// Registers `T` as the section `key`, starting at `T::default()`, with
  /// the versions its older data upgrades through (none: version 1). A key
  /// or a type registered before is replaced.
  ///
  /// # Panics
  ///
  /// When `versions` has a gap or a step out of range.
  pub(crate) fn register<T: Default + Serialize + DeserializeOwned + Send + Sync + 'static>(
    &mut self,
    key: &str,
    versions: Option<SaveVersions<T>>,
  ) {
    self
      .versions
      .register(versions.unwrap_or_else(|| SaveVersions::new(1)));
    self
      .sections
      .retain(|section| section.key != key && !section.value.as_any().is::<T>());
    self.sections.push(Section {
      key: key.to_string(),
      value: Box::new(Typed(T::default())),
      unread: None,
      warned: AtomicBool::new(false),
    });
  }

  pub(crate) fn get<T: 'static>(&self) -> Option<&T> {
    self
      .sections
      .iter()
      .find_map(|section| section.value.as_any().downcast_ref::<T>())
  }

  pub(crate) fn get_mut<T: 'static>(&mut self) -> Option<&mut T> {
    self
      .sections
      .iter_mut()
      .find_map(|section| section.value.as_any_mut().downcast_mut::<T>())
  }

  /// Puts every section back to its type's default
  pub(crate) fn reset_all(&mut self) {
    for section in &mut self.sections {
      section.value.reset();
    }
  }

  /// Loads the sections `stored` holds, upgrading older versions. The
  /// current version holds only the fields that differ from the default, so
  /// they are merged over the default's (unless the entry is whole); an
  /// older version's value goes to the steps as it is. A section that does
  /// not read keeps its value, with a warning.
  pub(crate) fn apply(
    &mut self,
    stored: &BTreeMap<String, RawSection>,
    warnings: &mut Vec<String>,
  ) {
    for section in &mut self.sections {
      let Some(raw) = stored.get(&section.key) else {
        continue;
      };
      let value = if !raw.whole && raw.version == section.value.current(&self.versions) {
        merged(raw.value.clone(), section.value.default_json().ok())
      } else {
        raw.value.clone()
      };
      match section
        .value
        .load(&self.versions, &section.key, raw.version, value)
      {
        Ok(()) => section.unread = None,
        Err(error) => {
          warnings.push(bad_value(&format!("game.{}", section.key), &detail(&error)));
          section.unread = Some(raw.clone());
        }
      }
    }
  }

  /// The sections to store: `base`'s sections that are not registered, and
  /// each registered one that differs from its default, at its current
  /// version (see `stored_value`)
  pub(crate) fn store(&self, base: &BTreeMap<String, RawSection>) -> BTreeMap<String, RawSection> {
    let mut stored: BTreeMap<String, RawSection> = base
      .iter()
      .filter(|(key, _)| !self.sections.iter().any(|section| section.key == **key))
      .map(|(key, raw)| (key.clone(), raw.clone()))
      .collect();
    for section in &self.sections {
      match (section.value.json(), section.value.default_json()) {
        (Ok(value), Ok(default)) => {
          if let Some((value, whole)) = stored_value(value, &default) {
            let version = section.value.current(&self.versions);
            let raw = RawSection {
              version,
              whole,
              value,
            };
            stored.insert(section.key.clone(), raw);
          } else if let Some(unread) = &section.unread {
            stored.insert(section.key.clone(), unread.clone());
          }
        }
        (Err(error), _) | (_, Err(error)) => {
          if !section.warned.swap(true, Ordering::Relaxed) {
            crate::engine_warn!(
              module: "settings",
              "the settings section `{}` does not serialize as JSON ({error}); the file keeps what it held",
              section.key
            );
          }
          if let Some(raw) = base.get(&section.key) {
            stored.insert(section.key.clone(), raw.clone());
          }
        }
      }
    }
    stored
  }
}

/// `stored` over `default`, field by field, when both are objects
fn merged(stored: Value, default: Option<Value>) -> Value {
  match (stored, default) {
    (Value::Object(fields), Some(Value::Object(mut merged))) => {
      merged.extend(fields);
      Value::Object(merged)
    }
    (stored, _) => stored,
  }
}

/// What a section stores of `value`, and whether it is the whole value:
/// `None` when it equals the default. An object stores the fields that differ
/// from the default's when merging them back over the default gives `value`
/// again; an object they cannot express (a field skipped when `None`, a key
/// taken out of a map, another enum variant), or whose default is not an
/// object, is stored whole. Any other value is stored as it is, since a load
/// merges only objects.
fn stored_value(value: Value, default: &Value) -> Option<(Value, bool)> {
  if value == *default {
    return None;
  }
  let diff = match (&value, default) {
    (Value::Object(fields), Value::Object(defaults)) => Value::Object(
      fields
        .iter()
        .filter(|(key, field)| defaults.get(*key) != Some(*field))
        .map(|(key, field)| (key.clone(), field.clone()))
        .collect(),
    ),
    _ => {
      let whole = value.is_object();
      return Some((value, whole));
    }
  };
  if merged(diff.clone(), Some(default.clone())) == value {
    Some((diff, false))
  } else {
    Some((value, true))
  }
}

/// Why a section did not read, for its warning
fn detail(error: &SaveError) -> String {
  match error {
    SaveError::NewerVersion { found, current } => {
      format!("version {found} is newer than this build's {current}")
    }
    SaveError::Corrupt { reason, .. } => reason.clone(),
    SaveError::Deserialize { source, .. } => {
      let mut text = source.to_string();
      let mut cause = source.source();
      while let Some(next) = cause {
        text.push_str(": ");
        text.push_str(&next.to_string());
        cause = next.source();
      }
      text
    }
    other => other.to_string(),
  }
}

impl fmt::Debug for SectionRegistry {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_list()
      .entries(self.sections.iter().map(|section| &section.key))
      .finish()
  }
}

#[cfg(test)]
mod tests {
  use serde_json::json;

  use super::*;

  #[test]
  fn a_section_that_is_not_an_object_stores_its_whole_value() {
    let mut sections = SectionRegistry::default();
    sections.register::<Vec<u32>>("unlocked", None);
    assert!(sections.store(&BTreeMap::new()).is_empty());

    sections.get_mut::<Vec<u32>>().unwrap().extend([3, 5]);
    let stored = sections.store(&BTreeMap::new());
    assert_eq!(
      stored.get("unlocked"),
      Some(&RawSection {
        version: 1,
        whole: false,
        value: json!([3, 5]),
      })
    );

    let mut loaded = SectionRegistry::default();
    loaded.register::<Vec<u32>>("unlocked", None);
    let mut warnings = Vec::new();
    loaded.apply(&stored, &mut warnings);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(loaded.get::<Vec<u32>>(), Some(&vec![3, 5]));
  }

  #[test]
  fn a_second_registration_of_a_key_or_a_type_replaces_the_first() {
    let mut sections = SectionRegistry::default();
    sections.register::<u32>("a", None);
    sections.register::<String>("a", None);
    assert_eq!(sections.get::<u32>(), None);
    assert_eq!(sections.get::<String>(), Some(&String::new()));

    sections.register::<String>("b", None);
    *sections.get_mut::<String>().unwrap() = "x".into();
    let stored = sections.store(&BTreeMap::new());
    assert_eq!(stored.keys().collect::<Vec<_>>(), ["b"]);
  }
}
