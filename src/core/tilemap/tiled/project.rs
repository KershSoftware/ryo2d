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
}

impl<'a> Props<'a> {
  pub(crate) fn new(path: AssetPath, project: Option<&'a TiledProject>) -> Props<'a> {
    Props {
      path,
      project,
      warned: Vec::new(),
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
    let result = resolve(raw, class, self.project, &mut unknown);
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
  resolve(raw, class, project, &mut Vec::new())
}

fn resolve(
  raw: &[RawProperty],
  class: Option<&str>,
  project: Option<&TiledProject>,
  unknown: &mut Vec<String>,
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
      project,
      unknown,
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
            project,
            unknown,
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
    Value::Array(items) => PropertyValue::List(items.iter().map(infer).collect()),
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
  project: Option<&TiledProject>,
  unknown: &mut Vec<String>,
  depth: usize,
) -> Result<Option<PropertyValue>, String> {
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
              project,
              unknown,
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
