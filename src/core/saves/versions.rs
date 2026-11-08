//! The versions of the game's saved types, and the typed steps that upgrade
//! older data.

use std::any::{TypeId, type_name};
use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::SaveError;
use super::container::Body;

type Cause = Box<dyn std::error::Error + Send + Sync>;
type RonStep = Box<dyn Fn(&str, bool) -> Result<String, Cause> + Send + Sync>;
type JsonStep = Box<dyn Fn(Value) -> Result<Value, Cause> + Send + Sync>;

/// One upgrade, from version `from` to the next, on RON text and on JSON values
struct Step {
  from: u32,
  /// Takes the text and whether it is a format-1 save, whose data is in `data`
  ron: RonStep,
  #[cfg_attr(not(test), expect(dead_code, reason = "settings sections'"))]
  json: JsonStep,
}

/// The versions of a saved type `T`: its current version and one typed step
/// for each older version.
///
/// Step 1 turns a version-1 value into version 2, step 2 a version-2 value
/// into version 3, and the last step yields `T`. Loading older data runs the
/// steps from its version on. A type with no `SaveVersions` is version 1, and
/// so is every save written before the engine recorded versions.
///
/// ```
/// # use ryo2d::core::saves::SaveVersions;
/// # use serde::{Deserialize, Serialize};
/// #[derive(Serialize, Deserialize)]
/// struct HeroV1 { hp: u32 }
/// #[derive(Serialize, Deserialize)]
/// struct HeroV2 { hp: u32, mp: u32 }
/// #[derive(Serialize, Deserialize)]
/// struct Hero { health: u32, mana: u32 }
///
/// let versions = SaveVersions::<Hero>::new(3)
///   .step(1, |old: HeroV1| HeroV2 { hp: old.hp, mp: 0 })
///   .step(2, |old: HeroV2| Hero { health: old.hp, mana: old.mp });
/// ```
pub struct SaveVersions<T> {
  current: u32,
  steps: Vec<Step>,
  marker: PhantomData<fn() -> T>,
}

impl<T> SaveVersions<T> {
  /// `T` is version `current`, which starts at 1. Every version below it
  /// needs a step.
  pub fn new(current: u32) -> Self {
    SaveVersions {
      current,
      steps: Vec::new(),
      marker: PhantomData,
    }
  }

  /// Adds the step that turns version `from`'s value (`A`) into version
  /// `from + 1`'s (`B`). Data passes between steps serialized, so `B` and the
  /// next step's `A` need only agree on their serialized shape.
  pub fn step<A: DeserializeOwned + 'static, B: Serialize + 'static>(
    mut self,
    from: u32,
    convert: impl Fn(A) -> B + Send + Sync + 'static,
  ) -> Self {
    let convert = Arc::new(convert);
    let json_convert = Arc::clone(&convert);
    let ron = Box::new(move |text: &str, legacy: bool| -> Result<String, Cause> {
      let old: A = if legacy {
        ron::from_str::<LegacyData<A>>(text)?.data
      } else {
        ron::from_str(text)?
      };
      Ok(ron::to_string(&convert(old))?)
    });
    let json = Box::new(move |value: Value| -> Result<Value, Cause> {
      let old: A = serde_json::from_value(value)?;
      Ok(serde_json::to_value(json_convert(old))?)
    });
    self.steps.push(Step { from, ron, json });
    self
  }
}

impl<T> fmt::Debug for SaveVersions<T> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("SaveVersions")
      .field("type", &type_name::<T>())
      .field("current", &self.current)
      .field(
        "steps_from",
        &self.steps.iter().map(|step| step.from).collect::<Vec<_>>(),
      )
      .finish()
  }
}

/// A format-1 save: the data in its `data` field, beside fields left unread
#[derive(Deserialize)]
struct LegacyData<A> {
  data: A,
}

/// A step failed to read the data it was given, or to write its result
#[derive(Debug, thiserror::Error)]
#[error("the upgrade from version {from} failed")]
struct StepFailed {
  from: u32,
  #[source]
  source: Cause,
}

/// A registered type's current version and its steps, in order from version 1
struct Chain {
  current: u32,
  steps: Vec<Step>,
}

/// The registered [`SaveVersions`], by type
#[derive(Default)]
pub(crate) struct VersionRegistry {
  chains: HashMap<TypeId, Chain>,
}

impl VersionRegistry {
  /// Registers `T`'s versions, replacing any registered before.
  ///
  /// # Panics
  ///
  /// When the current version is 0, a step is missing or out of range, or two
  /// steps start at one version.
  pub(crate) fn register<T: 'static>(&mut self, versions: SaveVersions<T>) {
    let name = type_name::<T>();
    let SaveVersions {
      current, mut steps, ..
    } = versions;
    assert!(
      current != 0,
      "SaveVersions<{name}>: the current version is 0; versions start at 1"
    );
    steps.sort_by_key(|step| step.from);
    for pair in steps.windows(2) {
      if pair[0].from == pair[1].from {
        panic!(
          "SaveVersions<{name}>: two steps from version {}",
          pair[0].from
        );
      }
    }
    if let Some(step) = steps
      .iter()
      .find(|step| step.from == 0 || step.from >= current)
    {
      panic!(
        "SaveVersions<{name}>: no step can start at version {} when the current version is {current}",
        step.from
      );
    }
    // Sorted, distinct and in range: the first version without its step is missing
    let mut next = 1;
    for step in &steps {
      if step.from != next {
        break;
      }
      next += 1;
    }
    if next < current {
      panic!("SaveVersions<{name}>: missing step from version {next}");
    }
    self
      .chains
      .insert(TypeId::of::<T>(), Chain { current, steps });
  }

  /// `T`'s current version: 1 for a type never registered
  pub(crate) fn current<T: 'static>(&self) -> u32 {
    self.chain::<T>().map_or(1, |chain| chain.current)
  }

  /// A save's data as `T`, upgraded from `version`: the value and the version
  /// it was upgraded from, if it was. Errors leave the slot for the caller to
  /// name.
  pub(crate) fn decode_ron<T: DeserializeOwned + 'static>(
    &self,
    version: u32,
    body: &Body,
  ) -> Result<(T, Option<u32>), SaveError> {
    let current = self.current::<T>();
    check_version(version, current, "")?;
    let mut text = Cow::Borrowed(body.ron.as_str());
    let mut legacy = body.legacy;
    for step in self.steps_from::<T>(version) {
      let upgraded = (step.ron)(&text, legacy).map_err(|source| failed("", step.from, source))?;
      text = Cow::Owned(upgraded);
      legacy = false;
    }
    let data = if legacy {
      ron::from_str::<LegacyData<T>>(&text).map(|wrapped| wrapped.data)
    } else {
      ron::from_str(&text)
    }
    .map_err(|e| deserialize("", e))?;
    Ok((data, (version < current).then_some(version)))
  }

  /// A settings section `key` as `T`, upgraded from `version`: the value and
  /// the version it was upgraded from, if it was. Errors name `key` as their
  /// slot.
  #[cfg_attr(not(test), expect(dead_code, reason = "settings sections'"))]
  pub(crate) fn decode_json<T: DeserializeOwned + 'static>(
    &self,
    key: &str,
    version: u32,
    value: Value,
  ) -> Result<(T, Option<u32>), SaveError> {
    let current = self.current::<T>();
    check_version(version, current, key)?;
    let mut value = value;
    for step in self.steps_from::<T>(version) {
      value = (step.json)(value).map_err(|source| failed(key, step.from, source))?;
    }
    let data = serde_json::from_value(value).map_err(|e| deserialize(key, e))?;
    Ok((data, (version < current).then_some(version)))
  }

  fn chain<T: 'static>(&self) -> Option<&Chain> {
    self.chains.get(&TypeId::of::<T>())
  }

  /// The steps that bring `version` up to `T`'s current version
  fn steps_from<T: 'static>(&self, version: u32) -> &[Step] {
    let start = usize::try_from(version.saturating_sub(1)).unwrap_or(usize::MAX);
    self
      .chain::<T>()
      .and_then(|chain| chain.steps.get(start..))
      .unwrap_or_default()
  }
}

fn check_version(version: u32, current: u32, slot: &str) -> Result<(), SaveError> {
  if version == 0 {
    return Err(SaveError::Corrupt {
      slot: slot.to_string(),
      reason: "its data version is 0".to_string(),
    });
  }
  if version > current {
    return Err(SaveError::NewerVersion {
      found: version,
      current,
    });
  }
  Ok(())
}

fn failed(slot: &str, from: u32, source: Cause) -> SaveError {
  SaveError::Deserialize {
    slot: slot.to_string(),
    source: Box::new(StepFailed { from, source }),
  }
}

fn deserialize(slot: &str, source: impl std::error::Error + Send + Sync + 'static) -> SaveError {
  SaveError::Deserialize {
    slot: slot.to_string(),
    source: Box::new(source),
  }
}

#[cfg(test)]
mod tests {
  use serde::{Deserialize, Serialize};
  use serde_json::json;

  use super::*;
  use crate::core::saves::SaveError;
  use crate::core::saves::container::fixtures::{LEGACY_SAVE, LegacyFixture};
  use crate::core::saves::container::{self, Body};

  #[derive(Debug, PartialEq, Serialize, Deserialize)]
  struct V1 {
    hp: u32,
  }

  #[derive(Debug, PartialEq, Serialize, Deserialize)]
  struct V2 {
    hp: u32,
    mp: u32,
  }

  #[derive(Debug, PartialEq, Serialize, Deserialize)]
  struct V3 {
    health: u32,
    mana: u32,
  }

  fn registry() -> VersionRegistry {
    let mut registry = VersionRegistry::default();
    registry.register(
      SaveVersions::<V3>::new(3)
        .step(1, |v: V1| V2 { hp: v.hp, mp: 0 })
        .step(2, |v: V2| V3 {
          health: v.hp,
          mana: v.mp,
        }),
    );
    registry
  }

  fn body<T: Serialize>(data: &T) -> Body {
    Body {
      ron: ron::to_string(data).unwrap(),
      legacy: false,
    }
  }

  #[test]
  fn a_version_one_save_upgrades_through_two_steps() {
    let registry = registry();
    assert_eq!(registry.current::<V3>(), 3);

    let (data, upgraded_from) = registry.decode_ron::<V3>(1, &body(&V1 { hp: 7 })).unwrap();
    assert_eq!(data, V3 { health: 7, mana: 0 });
    assert_eq!(upgraded_from, Some(1));

    let (data, upgraded_from) = registry
      .decode_ron::<V3>(2, &body(&V2 { hp: 5, mp: 9 }))
      .unwrap();
    assert_eq!(data, V3 { health: 5, mana: 9 });
    assert_eq!(upgraded_from, Some(2));

    let current = V3 { health: 1, mana: 2 };
    let (data, upgraded_from) = registry.decode_ron::<V3>(3, &body(&current)).unwrap();
    assert_eq!(data, current);
    assert_eq!(upgraded_from, None);
  }

  #[test]
  fn a_newer_save_is_refused() {
    let registry = registry();
    let newer = body(&V3 { health: 1, mana: 2 });
    assert!(matches!(
      registry.decode_ron::<V3>(4, &newer),
      Err(SaveError::NewerVersion {
        found: 4,
        current: 3
      })
    ));
    assert!(matches!(
      registry.decode_json::<V3>("hero", 4, json!({"health": 1, "mana": 2})),
      Err(SaveError::NewerVersion {
        found: 4,
        current: 3
      })
    ));
  }

  #[test]
  #[should_panic(expected = "missing step from version 2")]
  fn a_gap_in_the_steps_panics_at_registration() {
    let mut registry = VersionRegistry::default();
    registry.register(SaveVersions::<V3>::new(3).step(1, |v: V1| V2 { hp: v.hp, mp: 0 }));
  }

  #[test]
  #[should_panic(expected = "two steps from version 1")]
  fn two_steps_from_one_version_panic_at_registration() {
    let mut registry = VersionRegistry::default();
    registry.register(
      SaveVersions::<V2>::new(2)
        .step(1, |v: V1| V2 { hp: v.hp, mp: 0 })
        .step(1, |v: V1| V2 { hp: v.hp, mp: 1 }),
    );
  }

  #[test]
  #[should_panic(expected = "versions start at 1")]
  fn a_current_version_of_zero_panics_at_registration() {
    VersionRegistry::default().register(SaveVersions::<V1>::new(0));
  }

  #[test]
  #[should_panic(expected = "no step can start at version 2 when the current version is 2")]
  fn a_step_from_the_current_version_panics_at_registration() {
    VersionRegistry::default().register(
      SaveVersions::<V2>::new(2)
        .step(1, |v: V1| V2 { hp: v.hp, mp: 0 })
        .step(2, |v: V2| v),
    );
  }

  #[test]
  fn an_unregistered_type_is_version_one() {
    let registry = VersionRegistry::default();
    assert_eq!(registry.current::<V1>(), 1);

    let (data, upgraded_from) = registry.decode_ron::<V1>(1, &body(&V1 { hp: 3 })).unwrap();
    assert_eq!(data, V1 { hp: 3 });
    assert_eq!(upgraded_from, None);
    assert!(matches!(
      registry.decode_ron::<V1>(2, &body(&V1 { hp: 3 })),
      Err(SaveError::NewerVersion {
        found: 2,
        current: 1
      })
    ));

    let (header, legacy) = container::decode(LEGACY_SAVE).unwrap();
    let (data, upgraded_from) = registry
      .decode_ron::<LegacyFixture>(header.version, &legacy)
      .unwrap();
    assert_eq!(data, LegacyFixture::written());
    assert_eq!(upgraded_from, None);
  }

  #[test]
  fn the_legacy_wrapper_upgrades_from_version_one() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Hero2 {
      level: u32,
      name: String,
      items: Vec<String>,
      gold: u32,
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Hero3 {
      level: u32,
      title: String,
      inventory: Vec<String>,
      gold: u32,
    }

    let mut registry = VersionRegistry::default();
    registry.register(
      SaveVersions::<Hero3>::new(3)
        .step(1, |v: LegacyFixture| Hero2 {
          level: v.level,
          name: v.name,
          items: v.items,
          gold: 10,
        })
        .step(2, |v: Hero2| Hero3 {
          level: v.level,
          title: format!("Sir {}", v.name),
          inventory: v.items,
          gold: v.gold,
        }),
    );

    let (header, legacy) = container::decode(LEGACY_SAVE).unwrap();
    let (data, upgraded_from) = registry
      .decode_ron::<Hero3>(header.version, &legacy)
      .unwrap();
    assert_eq!(
      data,
      Hero3 {
        level: 3,
        title: "Sir Ada".into(),
        inventory: vec!["key".into(), "lamp".into()],
        gold: 10,
      }
    );
    assert_eq!(upgraded_from, Some(1));
  }

  #[test]
  fn json_sections_upgrade_with_the_same_steps() {
    let registry = registry();

    let (data, upgraded_from) = registry
      .decode_json::<V3>("hero", 1, json!({"hp": 7}))
      .unwrap();
    assert_eq!(data, V3 { health: 7, mana: 0 });
    assert_eq!(upgraded_from, Some(1));

    let (data, upgraded_from) = registry
      .decode_json::<V3>("hero", 3, json!({"health": 2, "mana": 4}))
      .unwrap();
    assert_eq!(data, V3 { health: 2, mana: 4 });
    assert_eq!(upgraded_from, None);
  }

  #[test]
  fn data_that_does_not_fit_its_version_is_a_deserialize_error() {
    let registry = registry();
    let wrong = Body {
      ron: "(name:\"x\")".into(),
      legacy: false,
    };
    for version in [1, 3] {
      assert!(matches!(
        registry.decode_ron::<V3>(version, &wrong),
        Err(SaveError::Deserialize { .. })
      ));
    }
    match registry.decode_json::<V3>("hero", 1, json!({"hp": "seven"})) {
      Err(SaveError::Deserialize { slot, .. }) => assert_eq!(slot, "hero"),
      other => panic!("expected Deserialize, got {other:?}"),
    }
    assert!(matches!(
      registry.decode_ron::<V3>(0, &wrong),
      Err(SaveError::Corrupt { .. })
    ));
  }
}
