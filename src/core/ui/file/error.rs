//! Words for the mistakes a UI file can hold, and the error that lists them

use std::fmt;

use crate::core::error::AssetError;

use super::reader::{Issue, ValueKind};

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
    ValueKind::InScope { value, .. } => describe(&value.kind),
  }
}

/// Every mistake found in a UI file (and the files it imports), each with
/// its position.
///
/// `Display` prints one per line, as `path:line:column: message`:
///
/// ```text
/// assets/ui/pause.ui.ron:12:5: Button has no option "fil_width"; did you mean "fill_width"?
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct UiFileError {
  pub issues: Vec<UiFileIssue>,
}

/// One mistake in a UI file. `line` and `column` count from 1; a mistake
/// with no place in the text (a file that cannot be read) has line 0.
#[derive(Debug, Clone, PartialEq)]
pub struct UiFileIssue {
  pub path: String,
  pub line: u32,
  pub column: u32,
  pub message: String,
}

impl UiFileIssue {
  pub(crate) fn at(path: &str, issue: Issue) -> UiFileIssue {
    UiFileIssue {
      path: path.to_string(),
      line: issue.pos.line,
      column: issue.pos.column,
      message: issue.message,
    }
  }
}

impl UiFileError {
  /// An error of one issue with no position
  pub(crate) fn one(path: &str, message: impl Into<String>) -> UiFileError {
    UiFileError {
      issues: vec![UiFileIssue {
        path: path.to_string(),
        line: 0,
        column: 0,
        message: message.into(),
      }],
    }
  }

  /// `Ok` when there are no issues; each issue is kept once, in order
  pub(crate) fn check(issues: Vec<UiFileIssue>) -> Result<(), UiFileError> {
    if issues.is_empty() {
      return Ok(());
    }
    // A template body is built once per use, so its mistakes can repeat
    let mut unique: Vec<UiFileIssue> = Vec::with_capacity(issues.len());
    for issue in issues {
      if !unique.contains(&issue) {
        unique.push(issue);
      }
    }
    Err(UiFileError { issues: unique })
  }
}

impl From<AssetError> for UiFileError {
  /// A UI file's mistakes as they are; any other asset error (a file that
  /// cannot be found or read) as one issue with no position
  fn from(error: AssetError) -> UiFileError {
    match error {
      AssetError::UiFile(error) => error,
      AssetError::NotFound(path) => UiFileError::one(&path.display().to_string(), "file not found"),
      AssetError::LoadError { path, reason } => {
        UiFileError::one(&path.display().to_string(), reason)
      }
      other => UiFileError::one("", other.to_string()),
    }
  }
}

impl fmt::Display for UiFileIssue {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    if self.path.is_empty() {
      write!(f, "{}", self.message)
    } else if self.line == 0 {
      write!(f, "{}: {}", self.path, self.message)
    } else {
      write!(
        f,
        "{}:{}:{}: {}",
        self.path, self.line, self.column, self.message
      )
    }
  }
}

impl fmt::Display for UiFileError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    for (i, issue) in self.issues.iter().enumerate() {
      if i > 0 {
        writeln!(f)?;
      }
      write!(f, "{issue}")?;
    }
    Ok(())
  }
}

impl std::error::Error for UiFileError {}

#[cfg(test)]
mod tests {
  use std::path::PathBuf;

  use super::*;

  #[test]
  fn an_asset_error_becomes_the_files_mistakes_or_one_issue_without_a_position() {
    let mistakes = UiFileError::one("menu.ui.ron", "no root");
    assert_eq!(
      UiFileError::from(AssetError::UiFile(mistakes.clone())),
      mistakes
    );
    let missing = UiFileError::from(AssetError::NotFound(PathBuf::from("ui/gone.ui.ron")));
    assert_eq!(missing.to_string(), "ui/gone.ui.ron: file not found");
    assert_eq!(missing.issues[0].line, 0);
    let other = UiFileError::from(AssetError::WatchError("no watcher".into()));
    assert_eq!(
      other.to_string(),
      "Failed to set up file watcher: no watcher"
    );
  }
}
