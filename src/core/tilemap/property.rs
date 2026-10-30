//! Typed properties, as Tiled and LDtk attach them to maps, layers, tiles
//! and objects.

use crate::core::ui::Color;

/// One property's value
#[derive(Clone, Debug, PartialEq)]
pub enum PropertyValue {
  Bool(bool),
  Int(i64),
  Float(f64),
  String(String),
  Color(Color),
  /// A path to a file, as written
  File(String),
  /// A reference to a map object by id
  Object(String),
  /// A value of a named enum
  Enum {
    ty: String,
    value: String,
  },
  /// A named class with fields of its own
  Class {
    ty: String,
    fields: Properties,
  },
  List(Vec<PropertyValue>),
}

/// Named properties in the order they were added
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Properties {
  entries: Vec<(String, PropertyValue)>,
}

impl Properties {
  /// The value named `name`
  pub fn get(&self, name: &str) -> Option<&PropertyValue> {
    self.entries.iter().find(|(k, _)| k == name).map(|(_, v)| v)
  }

  /// A bool property
  pub fn bool(&self, name: &str) -> Option<bool> {
    match self.get(name)? {
      PropertyValue::Bool(b) => Some(*b),
      _ => None,
    }
  }

  /// An int property; a float with no fraction counts
  pub fn int(&self, name: &str) -> Option<i64> {
    match self.get(name)? {
      PropertyValue::Int(i) => Some(*i),
      PropertyValue::Float(f) if f.fract() == 0.0 && f.abs() < 9.0e15 => Some(*f as i64),
      _ => None,
    }
  }

  /// A float property; an int counts
  pub fn float(&self, name: &str) -> Option<f64> {
    match self.get(name)? {
      PropertyValue::Float(f) => Some(*f),
      PropertyValue::Int(i) => Some(*i as f64),
      _ => None,
    }
  }

  /// A string property
  pub fn string(&self, name: &str) -> Option<&str> {
    match self.get(name)? {
      PropertyValue::String(s) => Some(s),
      _ => None,
    }
  }

  /// A color property
  pub fn color(&self, name: &str) -> Option<Color> {
    match self.get(name)? {
      PropertyValue::Color(c) => Some(*c),
      _ => None,
    }
  }

  /// Sets `name`, keeping its place if it exists and appending otherwise
  pub fn insert(&mut self, name: impl Into<String>, value: PropertyValue) {
    let name = name.into();
    match self.entries.iter_mut().find(|(k, _)| *k == name) {
      Some(entry) => entry.1 = value,
      None => self.entries.push((name, value)),
    }
  }

  /// The properties in the order they were added
  pub fn iter(&self) -> impl Iterator<Item = (&str, &PropertyValue)> {
    self.entries.iter().map(|(k, v)| (k.as_str(), v))
  }

  /// How many properties there are
  pub fn len(&self) -> usize {
    self.entries.len()
  }

  /// Whether there are none
  pub fn is_empty(&self) -> bool {
    self.entries.is_empty()
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ui::Color;

  #[test]
  fn properties_read_typed_and_keep_order() {
    let mut p = Properties::default();
    p.insert("zeta", PropertyValue::Bool(true));
    p.insert("alpha", PropertyValue::Int(3));
    p.insert("mid", PropertyValue::Float(2.0));
    p.insert("name", PropertyValue::String("x".into()));
    p.insert("tint", PropertyValue::Color(Color::WHITE));
    let keys: Vec<&str> = p.iter().map(|(k, _)| k).collect();
    assert_eq!(keys, ["zeta", "alpha", "mid", "name", "tint"]);
    assert_eq!(p.bool("zeta"), Some(true));
    assert_eq!(p.int("alpha"), Some(3));
    assert_eq!(p.int("mid"), Some(2));
    assert_eq!(p.float("alpha"), Some(3.0));
    assert_eq!(p.string("name"), Some("x"));
    assert_eq!(p.color("tint"), Some(Color::WHITE));
    assert_eq!(p.bool("alpha"), None);
    assert_eq!(p.int("missing"), None);
    p.insert("zeta", PropertyValue::Bool(false));
    assert_eq!(p.bool("zeta"), Some(false));
    assert_eq!(p.iter().count(), 5);
    assert_eq!(p.iter().next().map(|(k, _)| k), Some("zeta"));
    p.insert("frac", PropertyValue::Float(2.5));
    assert_eq!(p.int("frac"), None);
  }
}
