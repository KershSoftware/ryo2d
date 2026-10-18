//! Words for the mistakes a UI file can hold

use super::reader::ValueKind;

/// What a value is, for a message: "a string", "a number", `` `Name` ``
pub(crate) fn describe(kind: &ValueKind) -> String {
  match kind {
    ValueKind::Str(_) => "a string".into(),
    ValueKind::Num(_) => "a number".into(),
    ValueKind::Bool(_) => "a bool".into(),
    ValueKind::List(_) => "a list".into(),
    ValueKind::Tuple(_) => "a tuple".into(),
    ValueKind::Map(_) => "a map".into(),
    ValueKind::Fields(_) => "fields".into(),
    ValueKind::Named { name, .. } => format!("`{name}`"),
    ValueKind::Any => "a value".into(),
  }
}
