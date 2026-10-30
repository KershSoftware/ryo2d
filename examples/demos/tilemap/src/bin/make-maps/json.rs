//! A JSON value that keeps its keys in the order they were added, printed
//! the way Tiled (Qt) or LDtk print their files.

/// A JSON value
#[derive(Clone, Debug)]
pub enum J {
  Null,
  Bool(bool),
  Int(i64),
  Num(f64),
  Str(String),
  Arr(Vec<J>),
  /// Keys in insertion order
  Obj(Vec<(String, J)>),
}

impl From<bool> for J {
  fn from(value: bool) -> J {
    J::Bool(value)
  }
}

impl From<i64> for J {
  fn from(value: i64) -> J {
    J::Int(value)
  }
}

impl From<i32> for J {
  fn from(value: i32) -> J {
    J::Int(i64::from(value))
  }
}

impl From<u32> for J {
  fn from(value: u32) -> J {
    J::Int(i64::from(value))
  }
}

impl From<f64> for J {
  fn from(value: f64) -> J {
    J::Num(value)
  }
}

impl From<&str> for J {
  fn from(value: &str) -> J {
    J::Str(value.to_string())
  }
}

impl From<String> for J {
  fn from(value: String) -> J {
    J::Str(value)
  }
}

impl<T: Into<J>> From<Vec<T>> for J {
  fn from(value: Vec<T>) -> J {
    J::Arr(value.into_iter().map(Into::into).collect())
  }
}

impl<T: Into<J>> From<Option<T>> for J {
  fn from(value: Option<T>) -> J {
    value.map_or(J::Null, Into::into)
  }
}

/// An object from `(key, value)` pairs, in order
pub fn obj<const N: usize>(pairs: [(&str, J); N]) -> J {
  J::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

/// A JSON string literal
fn quote(text: &str, escape_slash: bool) -> String {
  let mut out = String::with_capacity(text.len() + 2);
  out.push('"');
  for c in text.chars() {
    match c {
      '"' => out.push_str("\\\""),
      '\\' => out.push_str("\\\\"),
      '/' if escape_slash => out.push_str("\\/"),
      '\n' => out.push_str("\\n"),
      '\r' => out.push_str("\\r"),
      '\t' => out.push_str("\\t"),
      c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
      c => out.push(c),
    }
  }
  out.push('"');
  out
}

/// A number as both editors print one: whole numbers without a fraction
fn number(value: f64) -> String {
  if value.fract() == 0.0 && value.abs() < 1e15 {
    format!("{}", value as i64)
  } else {
    format!("{value}")
  }
}

fn scalar(value: &J) -> Option<String> {
  Some(match value {
    J::Null => "null".to_string(),
    J::Bool(b) => b.to_string(),
    J::Int(i) => i.to_string(),
    J::Num(n) => number(*n),
    J::Str(s) => quote(s, false),
    J::Arr(_) | J::Obj(_) => return None,
  })
}

/// As Qt's `QJsonDocument::toJson` prints it, which Tiled uses for its
/// project and world files: four spaces, keys sorted, an empty array split
/// over two lines
pub fn qt(value: &J) -> String {
  let mut out = String::new();
  qt_into(value, 0, &mut out);
  out.push('\n');
  out
}

fn qt_into(value: &J, depth: usize, out: &mut String) {
  let pad = |depth: usize| "    ".repeat(depth);
  match value {
    J::Arr(items) => {
      out.push_str("[\n");
      for (i, item) in items.iter().enumerate() {
        out.push_str(&pad(depth + 1));
        qt_into(item, depth + 1, out);
        if i + 1 < items.len() {
          out.push(',');
        }
        out.push('\n');
      }
      out.push_str(&pad(depth));
      out.push(']');
    }
    J::Obj(pairs) => {
      let mut sorted: Vec<&(String, J)> = pairs.iter().collect();
      sorted.sort_by(|a, b| a.0.cmp(&b.0));
      out.push_str("{\n");
      for (i, (key, item)) in sorted.iter().enumerate() {
        out.push_str(&pad(depth + 1));
        out.push_str(&quote(key, false));
        out.push_str(": ");
        qt_into(item, depth + 1, out);
        if i + 1 < sorted.len() {
          out.push(',');
        }
        out.push('\n');
      }
      out.push_str(&pad(depth));
      out.push('}');
    }
    other => out.push_str(&scalar(other).unwrap_or_default()),
  }
}

/// As LDtk prints its projects: tabs, keys in order, arrays of plain values
/// and small objects of plain values on one line, and a layer's
/// `intGridCsv` in rows of the layer's width
pub fn ldtk(value: &J) -> String {
  let mut out = String::new();
  ldtk_into(value, 0, None, &mut out);
  out
}

/// An array of plain values, or an object of at most eight plain values or
/// arrays of plain values, short enough for one line
fn inline(value: &J) -> Option<String> {
  match value {
    J::Arr(items) => {
      let parts: Option<Vec<String>> = items.iter().map(scalar).collect();
      Some(format!("[{}]", parts?.join(",")))
    }
    J::Obj(pairs) if pairs.len() <= 8 => {
      let mut parts = Vec::with_capacity(pairs.len());
      for (key, item) in pairs {
        let text = match item {
          J::Arr(_) => inline(item)?,
          J::Obj(_) => return None,
          other => scalar(other)?,
        };
        parts.push(format!("{}: {text}", quote(key, false)));
      }
      let line = format!("{{ {} }}", parts.join(", "));
      (line.len() <= 140).then_some(line)
    }
    _ => None,
  }
}

fn ldtk_into(value: &J, depth: usize, row: Option<usize>, out: &mut String) {
  let pad = |depth: usize| "\t".repeat(depth);
  match value {
    J::Arr(items) if items.is_empty() => out.push_str("[]"),
    J::Arr(items) => {
      if let Some(width) = row.filter(|w| *w > 0) {
        // rows of numbers, LDtk's intGridCsv
        out.push_str("[\n");
        let parts: Vec<String> = items.iter().filter_map(scalar).collect();
        let chunks: Vec<&[String]> = parts.chunks(width).collect();
        for (i, chunk) in chunks.iter().enumerate() {
          out.push_str(&pad(depth + 1));
          out.push_str(&chunk.join(","));
          if i + 1 < chunks.len() {
            out.push(',');
          }
          out.push('\n');
        }
        out.push_str(&pad(depth));
        out.push(']');
        return;
      }
      if let Some(line) = inline(value) {
        out.push_str(&line);
        return;
      }
      out.push_str("[\n");
      for (i, item) in items.iter().enumerate() {
        out.push_str(&pad(depth + 1));
        match inline(item) {
          Some(line) => out.push_str(&line),
          None => ldtk_into(item, depth + 1, None, out),
        }
        if i + 1 < items.len() {
          out.push(',');
        }
        out.push('\n');
      }
      out.push_str(&pad(depth));
      out.push(']');
    }
    J::Obj(pairs) => {
      out.push_str("{\n");
      // LDtk's level layers print intGridCsv as rows of the layer's width
      let width = pairs
        .iter()
        .find_map(|(key, item)| match (key.as_str(), item) {
          ("__cWid", J::Int(w)) => usize::try_from(*w).ok(),
          _ => None,
        });
      for (i, (key, item)) in pairs.iter().enumerate() {
        out.push_str(&pad(depth + 1));
        out.push_str(&quote(key, false));
        out.push_str(": ");
        let rows = (key == "intGridCsv").then_some(width).flatten();
        match (item, inline(item)) {
          (J::Obj(_), Some(line)) => out.push_str(&line),
          _ => ldtk_into(item, depth + 1, rows, out),
        }
        if i + 1 < pairs.len() {
          out.push(',');
        }
        out.push('\n');
      }
      out.push_str(&pad(depth));
      out.push('}');
    }
    other => out.push_str(&scalar(other).unwrap_or_default()),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn qt_sorts_keys_and_splits_empty_arrays() {
    let value = obj([
      ("type", "world".into()),
      ("maps", J::Arr(vec![])),
      ("a", 1.5.into()),
    ]);
    assert_eq!(
      qt(&value),
      "{\n    \"a\": 1.5,\n    \"maps\": [\n    ],\n    \"type\": \"world\"\n}\n"
    );
  }

  #[test]
  fn ldtk_keeps_order_and_inlines_plain_values() {
    let value = obj([
      ("b", J::Arr(vec![1.into(), 2.into()])),
      ("a", obj([("x", 1.into()), ("y", "z".into())])),
    ]);
    assert_eq!(
      ldtk(&value),
      "{\n\t\"b\": [1,2],\n\t\"a\": { \"x\": 1, \"y\": \"z\" }\n}"
    );
  }
}
