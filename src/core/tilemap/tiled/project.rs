//! Tiled's project file: the class and enum types a map's properties name,
//! and the defaults of a class's members. Tiled writes a class property's
//! members, and an object's class members, only where they differ from the
//! class's defaults, so a loader needs the project to read what a file means.

use std::collections::HashMap;

use serde::Deserialize;
use serde_json::{Map, Value};

use super::raw::RawProperty;
use crate::core::asset::{AssetError, AssetPath, LoadContext};
use crate::core::tilemap::{Properties, PropertyValue};
use crate::core::ui::Color;

/// Where a project is looked for when none is configured: the asset root
pub(crate) const DEFAULT_PROJECT: &str = "project.tiled-project";

/// Classes may hold classes; a project that loops is cut off here
const MAX_DEPTH: usize = 16;
/// Most values one thing's properties may take to resolve: classes holding
/// several members of classes multiply, so a project of a few classes can
/// ask for billions
const MAX_VALUES: usize = 1 << 16;
/// Most values the properties of one file's things may take, all together
const MAX_FILE_VALUES: usize = 1 << 22;

/// The values a resolution may still convert
struct Budget {
  left: usize,
  limit: usize,
  /// The file's budget, not the thing's, is the one that binds
  file: bool,
}

impl Budget {
  /// A budget of one thing's properties
  fn thing() -> Budget {
    Budget {
      left: MAX_VALUES,
      limit: MAX_VALUES,
      file: false,
    }
  }

  /// Takes one value, or fails naming the cap
  fn spend(&mut self) -> Result<(), String> {
    if self.left == 0 {
      return Err(if self.file {
        format!(
          "the file's properties take more than {} values to resolve",
          self.limit
        )
      } else {
        format!(
          "the properties take more than {} values to resolve: classes holding members of classes multiply",
          self.limit
        )
      });
    }
    self.left -= 1;
    Ok(())
  }
}

/// One member of a class
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct MemberDef {
  name: String,
  #[serde(rename = "type")]
  ty: String,
  #[serde(rename = "propertyType")]
  property_type: Option<String>,
  value: Value,
}

/// A class type: its members, with their defaults
#[derive(Debug, Default)]
pub(crate) struct ClassDef {
  members: Vec<MemberDef>,
}

/// An enum type
#[derive(Debug, Default)]
pub(crate) struct EnumDef {
  values: Vec<String>,
  /// Several values may be set at once
  flags: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawType {
  name: String,
  #[serde(rename = "type")]
  ty: String,
  members: Vec<MemberDef>,
  values: Vec<String>,
  #[serde(rename = "valuesAsFlags")]
  values_as_flags: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawProject {
  #[serde(rename = "propertyTypes")]
  property_types: Vec<RawType>,
}

/// The types of a Tiled project
#[derive(Debug, Default)]
pub(crate) struct TiledProject {
  classes: HashMap<String, ClassDef>,
  enums: HashMap<String, EnumDef>,
}

impl TiledProject {
  /// The project in `text`, a `.tiled-project` file; the error reads
  /// `line L, column C: what`
  pub(crate) fn parse(bytes: &[u8]) -> Result<TiledProject, String> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let raw: RawProject = serde_json::from_slice(bytes).map_err(|e| {
      let text = e.to_string();
      let what = text
        .rsplit_once(" at line ")
        .map_or(text.as_str(), |(what, _)| what);
      format!("line {}, column {}: {what}", e.line(), e.column())
    })?;
    let mut project = TiledProject::default();
    for t in raw.property_types {
      match t.ty.as_str() {
        "class" => {
          project
            .classes
            .insert(t.name, ClassDef { members: t.members });
        }
        "enum" => {
          project.enums.insert(
            t.name,
            EnumDef {
              values: t.values,
              flags: t.values_as_flags,
            },
          );
        }
        _ => {}
      }
    }
    Ok(project)
  }

  /// The project a loader of the file at `cx.path()` uses: the one at
  /// `configured`, which must exist, else `project.tiled-project` at the
  /// asset root if there is one. It is read with `cx.read`, so saving the
  /// project loads the file again
  pub(crate) fn for_loader(
    cx: &LoadContext<'_>,
    configured: Option<&str>,
  ) -> Result<Option<TiledProject>, AssetError> {
    let name = configured.unwrap_or(DEFAULT_PROJECT);
    let fail =
      |what: String| AssetError::decode(cx.path(), format!("the Tiled project {name:?}: {what}"));
    let path = AssetPath::parse(name).map_err(|e| fail(e.report()))?;
    let bytes = match cx.read(&path) {
      Ok(bytes) => bytes,
      Err(AssetError::NotFound { .. }) if configured.is_none() => return Ok(None),
      Err(e) => return Err(fail(e.report())),
    };
    TiledProject::parse(&bytes).map(Some).map_err(fail)
  }
}

/// Reads properties against a project, and remembers the classes it warned
/// about so each is warned about once per file
pub(crate) struct Props<'a> {
  path: AssetPath,
  project: Option<&'a TiledProject>,
  warned: Vec<String>,
  /// What the file's things may still convert (`MAX_FILE_VALUES` in all)
  left_in_file: usize,
  /// Where the file names its dependencies
  pub(crate) places: super::build::DependencyPlaces,
  /// What the file uses that loads otherwise than Tiled shows it
  pub(crate) unread: super::build::Unread,
}

impl<'a> Props<'a> {
  pub(crate) fn new(path: AssetPath, project: Option<&'a TiledProject>) -> Props<'a> {
    Props {
      path,
      project,
      warned: Vec::new(),
      left_in_file: MAX_FILE_VALUES,
      places: Default::default(),
      unread: Default::default(),
    }
  }

  /// A load error at `place`
  pub(crate) fn fail(&self, place: &str, what: impl std::fmt::Display) -> AssetError {
    AssetError::decode(&self.path, format!("{place}: {what}"))
  }

  /// The file being loaded
  pub(crate) fn path(&self) -> &AssetPath {
    &self.path
  }

  /// The properties of something of `class`, with the defaults of the class's
  /// members the file left out; the error names the place and the property
  pub(crate) fn read(
    &mut self,
    place: &str,
    raw: &[RawProperty],
    class: Option<&str>,
  ) -> Result<Properties, AssetError> {
    let mut unknown = Vec::new();
    let mut budget = if self.left_in_file < MAX_VALUES {
      Budget {
        left: self.left_in_file,
        limit: MAX_FILE_VALUES,
        file: true,
      }
    } else {
      Budget::thing()
    };
    let before = budget.left;
    let result = resolve(raw, class, self.project, &mut unknown, &mut budget);
    self.left_in_file -= before - budget.left;
    for name in unknown {
      if self.warned.contains(&name) {
        continue;
      }
      let why = if self.project.is_some() {
        "is not in the Tiled project"
      } else {
        "needs a Tiled project, and there is none"
      };
      crate::engine_warn!(
        module: "Tiled",
        "{}: class `{name}` {why}, so its members keep what the file wrote and its defaults are not filled in (name the project in Ryo.toml [tilemap] tiled_project, or EngineBuilder::tiled_project, or save it as {DEFAULT_PROJECT} at the asset root)",
        self.path
      );
      self.warned.push(name);
    }
    result.map_err(|e| AssetError::decode(&self.path, format!("{place}: {e}")))
  }
}

/// The properties of `raw`, for a thing of class `class`, resolved against
/// `project`: a class member the file omits takes the class's default, nested
/// classes resolve the same way, enums read by name (flags as a `List` of
/// them). Without the project, or for a class it lacks, what is written is
/// kept. The error says what is wrong, naming the property
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn resolve_properties(
  raw: &[RawProperty],
  class: Option<&str>,
  project: Option<&TiledProject>,
) -> Result<Properties, String> {
  resolve(raw, class, project, &mut Vec::new(), &mut Budget::thing())
}

fn resolve(
  raw: &[RawProperty],
  class: Option<&str>,
  project: Option<&TiledProject>,
  unknown: &mut Vec<String>,
  budget: &mut Budget,
) -> Result<Properties, String> {
  let mut out = Properties::default();
  let class_def = class
    .filter(|c| !c.is_empty())
    .and_then(|name| project.and_then(|p| p.classes.get(name)));
  for p in raw {
    let bad = |what: &str| format!("property {:?}: {what}", p.name);
    // a member of the class: what the file wrote lies over the class's default
    let default = class_def
      .and_then(|def| def.members.iter().find(|m| m.name == p.name))
      .map(|m| &m.value);
    let layered = match default {
      Some(default) => merge(default, &p.value),
      None => p.value.clone(),
    };
    if let Some(value) = convert(
      &p.ty,
      p.propertytype.as_deref(),
      &layered,
      (project, unknown, budget),
      0,
    )
    .map_err(|e| bad(&e))?
    {
      out.insert(p.name.clone(), value);
    }
  }
  if let Some(name) = class.filter(|c| !c.is_empty()) {
    match project.and_then(|p| p.classes.get(name)) {
      Some(def) => {
        for member in &def.members {
          if out.get(&member.name).is_some() {
            continue;
          }
          let bad = |what: &str| format!("property {:?}: {what}", member.name);
          let value = convert(
            &member.ty,
            member.property_type.as_deref(),
            &member.value,
            (project, unknown, budget),
            0,
          )
          .map_err(|e| bad(&e))?;
          if let Some(value) = value {
            out.insert(member.name.clone(), value);
          }
        }
      }
      None => note(unknown, name),
    }
  }
  Ok(out)
}

fn note(unknown: &mut Vec<String>, name: &str) {
  if !unknown.iter().any(|u| u == name) {
    unknown.push(name.to_string());
  }
}

/// A color as Tiled writes it: `#AARRGGBB`, or `#RRGGBB`
pub(crate) fn parse_color(text: &str) -> Option<Color> {
  let hex = text.strip_prefix('#').unwrap_or(text);
  if !hex.is_ascii() {
    return None;
  }
  let value = u32::from_str_radix(hex, 16).ok()?;
  match hex.len() {
    6 => Some(Color::rgb(
      (value >> 16) as u8,
      (value >> 8) as u8,
      value as u8,
    )),
    8 => Some(Color::new(
      (value >> 16) as u8,
      (value >> 8) as u8,
      value as u8,
      (value >> 24) as u8,
    )),
    _ => None,
  }
}

/// The value of a class member of a type the file does not give
fn infer(value: &Value) -> PropertyValue {
  match value {
    Value::Bool(b) => PropertyValue::Bool(*b),
    Value::Number(n) => match n.as_i64() {
      Some(i) => PropertyValue::Int(i),
      None => PropertyValue::Float(n.as_f64().unwrap_or(0.0)),
    },
    Value::String(s) => PropertyValue::String(s.clone()),
    // a list member's items are written typed, as a list property's are
    Value::Array(items) => PropertyValue::List(
      items
        .iter()
        .map(|item| typed_item(item).unwrap_or_else(|| infer(item)))
        .collect(),
    ),
    Value::Object(members) => {
      let mut fields = Properties::default();
      for (name, value) in members {
        fields.insert(name.clone(), infer(value));
      }
      PropertyValue::Class {
        ty: String::new(),
        fields,
      }
    }
    Value::Null => PropertyValue::String(String::new()),
  }
}

/// A list item as Tiled writes one: its type (`string` if left out), its
/// custom type (`propertytype` in a map, `propertyType` in the project file)
/// and its value
fn list_item(item: &Value) -> Option<(&str, Option<&str>, &Value)> {
  let Value::Object(fields) = item else {
    return None;
  };
  let ty = match fields.get("type") {
    Some(Value::String(ty)) => ty.as_str(),
    None => "string",
    Some(_) => return None,
  };
  let custom = ["propertytype", "propertyType"]
    .iter()
    .find_map(|key| fields.get(*key))
    .and_then(Value::as_str);
  Some((ty, custom, fields.get("value").unwrap_or(&Value::Null)))
}

/// A typed list item read without a project, for a class member of a class
/// the project lacks; `None` if `item` is not one
fn typed_item(item: &Value) -> Option<PropertyValue> {
  let Value::Object(fields) = item else {
    return None;
  };
  if !fields.contains_key("type") || !fields.contains_key("value") {
    return None;
  }
  let (ty, custom, value) = list_item(item)?;
  let mut budget = Budget::thing();
  convert(ty, custom, value, (None, &mut Vec::new(), &mut budget), 0)
    .ok()
    .flatten()
}

/// `over` laid on `base`: objects merge member by member, anything else is
/// replaced
fn merge(base: &Value, over: &Value) -> Value {
  match (base, over) {
    (Value::Object(a), Value::Object(b)) => {
      let mut out: Map<String, Value> = a.clone();
      for (k, v) in b {
        let merged = match a.get(k) {
          Some(under) => merge(under, v),
          None => v.clone(),
        };
        out.insert(k.clone(), merged);
      }
      Value::Object(out)
    }
    _ => over.clone(),
  }
}

/// An enum's value as the names it holds
fn enum_value(def: Option<&EnumDef>, ty: &str, value: &Value) -> Result<PropertyValue, String> {
  let one = |name: String| PropertyValue::Enum {
    ty: ty.to_string(),
    value: name,
  };
  let Some(def) = def else {
    // not a known enum: what is written
    return Ok(match value {
      Value::Number(n) => one(n.to_string()),
      Value::String(s) => one(s.clone()),
      Value::Null => one(String::new()),
      _ => return Err(format!("not a value of {ty}")),
    });
  };
  match value {
    Value::Number(n) => {
      let n = n
        .as_u64()
        .ok_or_else(|| format!("{n} is not a value of {ty}"))?;
      if def.flags {
        let mut names = Vec::new();
        for (bit, name) in def.values.iter().enumerate() {
          if bit < 64 && (n >> bit) & 1 == 1 {
            names.push(one(name.clone()));
          }
        }
        let known = def.values.len().min(64);
        if known < 64 && n >> known != 0 {
          return Err(format!("{n} sets a flag {ty} does not have"));
        }
        Ok(PropertyValue::List(names))
      } else {
        match def.values.get(usize::try_from(n).unwrap_or(usize::MAX)) {
          Some(name) => Ok(one(name.clone())),
          None => Err(format!("{n} is not a value of {ty}")),
        }
      }
    }
    Value::String(text) => {
      if def.flags {
        Ok(PropertyValue::List(
          text
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(|name| one(name.to_string()))
            .collect(),
        ))
      } else {
        Ok(one(text.clone()))
      }
    }
    Value::Null => Ok(one(String::new())),
    _ => Err(format!("not a value of {ty}")),
  }
}

/// One value of Tiled type `ty` (`propertytype` names a custom type). `None`
/// for a value that means unset
fn convert(
  ty: &str,
  propertytype: Option<&str>,
  value: &Value,
  (project, unknown, budget): (Option<&TiledProject>, &mut Vec<String>, &mut Budget),
  depth: usize,
) -> Result<Option<PropertyValue>, String> {
  budget.spend()?;
  let custom = propertytype.filter(|t| !t.is_empty());
  Ok(Some(match ty {
    "bool" => PropertyValue::Bool(value.as_bool().ok_or("not a bool")?),
    "int" => match (value, custom) {
      (Value::Number(_), Some(name)) => {
        enum_value(project.and_then(|p| p.enums.get(name)), name, value)?
      }
      (Value::Number(n), None) => PropertyValue::Int(
        n.as_i64()
          .or_else(|| n.as_f64().filter(|f| f.fract() == 0.0).map(|f| f as i64))
          .ok_or("not an int")?,
      ),
      _ => return Err("not an int".into()),
    },
    "float" => PropertyValue::Float(value.as_f64().ok_or("not a float")?),
    "" | "string" => {
      if !matches!(value, Value::String(_) | Value::Null) {
        return Err("not a string".into());
      }
      match custom {
        Some(name) => enum_value(project.and_then(|p| p.enums.get(name)), name, value)?,
        None => PropertyValue::String(value.as_str().unwrap_or_default().to_string()),
      }
    }
    "color" => {
      let text = value.as_str().ok_or("not a color")?;
      // an unset color property is written as an empty string
      if text.is_empty() {
        return Ok(None);
      }
      PropertyValue::Color(parse_color(text).ok_or("not a color")?)
    }
    "file" => PropertyValue::File(value.as_str().ok_or("not a path")?.to_string()),
    "object" => PropertyValue::Object(match value {
      Value::Number(n) => n.to_string(),
      Value::String(s) => s.clone(),
      _ => return Err("not an object id".into()),
    }),
    "class" => {
      let Value::Object(written) = value else {
        return Err("not a class".into());
      };
      let name = custom.unwrap_or_default();
      match project.and_then(|p| p.classes.get(name)) {
        Some(def) => {
          if depth >= MAX_DEPTH {
            return Err(format!(
              "class {name} holds itself, more than {MAX_DEPTH} deep"
            ));
          }
          let mut fields = Properties::default();
          for member in &def.members {
            let own = written.get(&member.name);
            let layered = match own {
              Some(own) => merge(&member.value, own),
              None => member.value.clone(),
            };
            let value = convert(
              &member.ty,
              member.property_type.as_deref(),
              &layered,
              (project, &mut *unknown, &mut *budget),
              depth + 1,
            )
            .map_err(|e| format!("{name}.{}: {e}", member.name))?;
            if let Some(value) = value {
              fields.insert(member.name.clone(), value);
            }
          }
          for (key, value) in written {
            if fields.get(key).is_none() {
              fields.insert(key.clone(), infer(value));
            }
          }
          PropertyValue::Class {
            ty: name.to_string(),
            fields,
          }
        }
        None => {
          if !name.is_empty() {
            note(unknown, name);
          }
          let mut fields = Properties::default();
          for (key, value) in written {
            fields.insert(key.clone(), infer(value));
          }
          PropertyValue::Class {
            ty: name.to_string(),
            fields,
          }
        }
      }
    }
    "list" => {
      let items = match value {
        Value::Array(items) => items.as_slice(),
        Value::Null => &[],
        _ => return Err("not a list".into()),
      };
      if depth >= MAX_DEPTH {
        return Err(format!(
          "lists and classes are nested more than {MAX_DEPTH} deep"
        ));
      }
      let mut out = Vec::with_capacity(items.len());
      for (i, item) in items.iter().enumerate() {
        let Some((ty, custom, value)) = list_item(item) else {
          return Err(format!(
            "item {i}: not a list item ({{\"type\", \"value\"}})"
          ));
        };
        let value = convert(
          ty,
          custom,
          value,
          (project, &mut *unknown, &mut *budget),
          depth + 1,
        )
        .map_err(|e| format!("item {i}: {e}"))?;
        // an unset item (an empty color) is left out, as an unset property is
        out.extend(value);
      }
      PropertyValue::List(out)
    }
    other => return Err(format!("unknown type {other:?}")),
  }))
}

#[cfg(test)]
mod tests {
  use super::*;

  fn project(json: &str) -> TiledProject {
    match TiledProject::parse(json.as_bytes()) {
      Ok(p) => p,
      Err(e) => panic!("{e}"),
    }
  }

  fn prop(name: &str, ty: &str, propertytype: Option<&str>, value: Value) -> RawProperty {
    RawProperty {
      name: name.into(),
      ty: ty.into(),
      propertytype: propertytype.map(str::to_string),
      value,
    }
  }

  const PROJECT: &str = r#"{"propertyTypes":[
    {"name":"Enemy","type":"class","members":[{"name":"hp","type":"int","value":3},{"name":"boss","type":"bool","value":false}]},
    {"name":"Loop","type":"class","members":[{"name":"again","type":"class","propertyType":"Loop","value":{}}]},
    {"name":"Dir","type":"enum","storageType":"int","values":["up","down"],"valuesAsFlags":false}
  ]}"#;

  #[test]
  fn a_class_fills_the_members_a_file_omits() {
    let p = project(PROJECT);
    let raw = [prop("boss", "bool", None, Value::Bool(true))];
    let out = resolve_properties(&raw, Some("Enemy"), Some(&p)).expect("resolves");
    assert_eq!((out.bool("boss"), out.int("hp")), (Some(true), Some(3)));
    // no class given: just what was written
    let out = resolve_properties(&raw, None, Some(&p)).expect("resolves");
    assert_eq!(out.len(), 1);
  }

  #[test]
  fn each_unknown_class_is_warned_about_once_per_file() {
    let none = Props::new(AssetPath::parse("a.tmj").expect("path"), None);
    let mut px = none;
    for class in ["X", "X", "Y", "X"] {
      assert!(px.read("map", &[], Some(class)).is_ok());
    }
    assert_eq!(px.warned, ["X", "Y"]);
    // a class the project has is not warned about
    let p = project(PROJECT);
    let mut px = Props::new(AssetPath::parse("a.tmj").expect("path"), Some(&p));
    px.read("map", &[], Some("Enemy")).expect("reads");
    assert!(px.warned.is_empty());
    // another file starts fresh
    let mut other = Props::new(AssetPath::parse("b.tmj").expect("path"), None);
    other.read("map", &[], Some("X")).expect("reads");
    assert_eq!(other.warned, ["X"]);
  }

  #[test]
  fn a_class_that_holds_itself_is_an_error_not_a_loop() {
    let p = project(PROJECT);
    let raw = [prop("l", "class", Some("Loop"), serde_json::json!({}))];
    let error = resolve_properties(&raw, None, Some(&p)).expect_err("resolved");
    assert!(error.contains("holds itself"), "{error}");
  }

  #[test]
  fn a_list_that_is_not_one_or_nests_too_deep_fails_naming_the_property() {
    let raw = [prop("l", "list", None, serde_json::json!(4))];
    let error = resolve_properties(&raw, None, None).expect_err("resolved");
    assert!(
      error.contains("property \"l\"") && error.contains("not a list"),
      "{error}"
    );
    let mut deep = serde_json::json!([]);
    for _ in 0..20 {
      deep = serde_json::json!([{"type": "list", "value": deep}]);
    }
    let raw = [prop("l", "list", None, deep)];
    let error = resolve_properties(&raw, None, None).expect_err("resolved");
    assert!(error.contains("nested more than 16 deep"), "{error}");
    // an item of a class the project lacks keeps its typed items
    let raw = [prop(
      "k",
      "class",
      Some("Unknown"),
      serde_json::json!({"items": [{"type": "int", "value": 3}]}),
    )];
    let out = resolve_properties(&raw, None, None).expect("resolves");
    let Some(PropertyValue::Class { fields, .. }) = out.get("k") else {
      panic!("not a class")
    };
    assert_eq!(
      fields.get("items"),
      Some(&PropertyValue::List(vec![PropertyValue::Int(3)]))
    );
  }

  /// A project whose class `C0` holds `fan` members of class `C1`, each
  /// holding `fan` of `C2`, and so on `depth` deep: `fan^depth` values
  fn fan_out(fan: usize, depth: usize) -> TiledProject {
    let mut types = Vec::new();
    for level in 0..depth {
      let members: Vec<String> = (0..fan)
        .map(|m| {
          if level + 1 == depth {
            format!(r#"{{"name":"m{m}","type":"int","value":1}}"#)
          } else {
            let next = level + 1;
            format!(r#"{{"name":"m{m}","type":"class","propertyType":"C{next}","value":{{}}}}"#)
          }
        })
        .collect();
      types.push(format!(
        r#"{{"name":"C{level}","type":"class","members":[{}]}}"#,
        members.join(",")
      ));
    }
    project(&format!(r#"{{"propertyTypes":[{}]}}"#, types.join(",")))
  }

  #[test]
  fn classes_fanning_out_past_the_cap_fail_naming_the_cap() {
    // 4^9 = 262,144 values, past the 65,536 one thing's properties may take
    let p = fan_out(4, 9);
    let error = resolve_properties(&[], Some("C0"), Some(&p)).expect_err("resolved");
    assert!(error.contains("65536"), "{error}");
    // 4^7 = 16,384 values resolve
    let p = fan_out(4, 7);
    assert!(resolve_properties(&[], Some("C0"), Some(&p)).is_ok());
  }

  #[test]
  fn a_file_s_things_share_a_cap_on_the_values_they_resolve() {
    // 4^5 = 1,024 values a thing; the file's budget runs out on the fifth
    let p = fan_out(4, 5);
    let mut px = Props::new(AssetPath::parse("a.tmj").expect("path"), Some(&p));
    px.left_in_file = 4 * 1_400;
    for _ in 0..4 {
      assert!(px.read("object 1", &[], Some("C0")).is_ok());
    }
    let error = px.read("object 5", &[], Some("C0")).expect_err("read");
    assert!(error.report().contains("object 5"), "{}", error.report());
  }

  #[test]
  fn an_enum_value_outside_its_enum_is_an_error_naming_the_property() {
    let p = project(PROJECT);
    let raw = [prop("d", "int", Some("Dir"), serde_json::json!(5))];
    let error = resolve_properties(&raw, None, Some(&p)).expect_err("resolved");
    assert!(
      error.contains("property \"d\"") && error.contains("Dir"),
      "{error}"
    );
  }
}
