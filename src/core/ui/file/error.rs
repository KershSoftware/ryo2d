//! Words for the mistakes a UI file can hold, and the error that lists them

use std::fmt;
use std::sync::Arc;

use crate::core::asset::{AssetError, AssetPath};

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
/// ui/pause.ui.ron:12:5: Button has no option "fil_width"; did you mean "fill_width"?
/// ```
///
/// A failed load's error (`AssetError::Decode` over a `UiFileError`) names
/// the file, so the file's own mistakes have an empty `path` there and
/// print as `line:column: message`; an imported file's keep theirs.
/// `UiFileError::from` the asset error gives every mistake its path.
#[derive(Debug, Clone, PartialEq)]
pub struct UiFileError {
  /// Each mistake, in the order found
  pub issues: Vec<UiFileIssue>,
}

/// One mistake in a UI file. `line` and `column` count from 1; a mistake
/// with no place in the text (a file that cannot be read) has line 0.
#[derive(Debug, Clone, PartialEq)]
pub struct UiFileIssue {
  /// The file the offending value is in: for a template's mistake, the file
  /// that defines the template. Empty for the file itself in its failed
  /// load's `AssetError`, which names it
  pub path: String,
  /// The line of the offending value, from 1; 0 when it has no place
  pub line: u32,
  /// The column of the offending value, from 1; 0 when it has no place
  pub column: u32,
  /// What is wrong, and the nearest name when an unknown one has one
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

  /// The load of the file at `path` failing with these mistakes: the asset
  /// error names the file, so its own mistakes leave their path to it
  pub(crate) fn failing(mut self, path: &AssetPath) -> AssetError {
    for issue in &mut self.issues {
      if issue.path == path.as_str() {
        issue.path.clear();
      }
    }
    AssetError::decode(path, self)
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

impl From<Arc<AssetError>> for UiFileError {
  /// A UI file's mistakes as they are; any other asset error (a file that is
  /// not there, an import that goes round) as one issue with no position
  fn from(error: Arc<AssetError>) -> UiFileError {
    UiFileError {
      issues: issues_of(&error, ""),
    }
  }
}

/// What failed in a dependency: its own error, not the wrapping that names
/// the file that needs it
fn failed(error: &AssetError) -> &AssetError {
  match error {
    AssetError::Dependency { error, .. } => failed(error),
    other => other,
  }
}

/// Why a dependency did not load, on one line
pub(crate) fn reason(error: &AssetError) -> String {
  failed(error).report()
}

/// A file's failure as issues: a UI file's own mistakes, each with its
/// path, anything else as one issue for `path` with no position
pub(crate) fn issues_of(error: &AssetError, path: &str) -> Vec<UiFileIssue> {
  let error = failed(error);
  if let AssetError::Decode { path: file, source } = error
    && let Some(mistakes) = source.downcast_ref::<UiFileError>()
  {
    let with_path = |issue: &UiFileIssue| {
      let mut issue = issue.clone();
      if issue.path.is_empty() {
        issue.path = file.as_str().to_string();
      }
      issue
    };
    return mistakes.issues.iter().map(with_path).collect();
  }
  vec![UiFileIssue {
    path: path.to_string(),
    line: 0,
    column: 0,
    message: error.report(),
  }]
}

impl fmt::Display for UiFileIssue {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    if self.path.is_empty() {
      if self.line == 0 {
        write!(f, "{}", self.message)
      } else {
        write!(f, "{}:{}: {}", self.line, self.column, self.message)
      }
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
  use super::*;
  use crate::core::asset::AssetPath;

  #[test]
  fn an_issue_prints_the_parts_it_has() {
    let issue = |path: &str, line| UiFileIssue {
      path: path.into(),
      line,
      column: 4,
      message: "bad".into(),
    };
    assert_eq!(issue("", 2).to_string(), "2:4: bad");
    assert_eq!(issue("", 0).to_string(), "bad");
    assert_eq!(issue("a.ui.ron", 0).to_string(), "a.ui.ron: bad");
    assert_eq!(issue("a.ui.ron", 2).to_string(), "a.ui.ron:2:4: bad");
  }

  #[test]
  fn an_asset_error_becomes_the_files_mistakes_or_one_issue_without_a_position() {
    let path = AssetPath::parse("ui/menu.ui.ron").unwrap();
    let mistakes = UiFileError::one("ui/menu.ui.ron", "no root");
    let decode = Arc::new(AssetError::decode(&path, mistakes.clone()));
    assert_eq!(UiFileError::from(Arc::clone(&decode)), mistakes);
    let needed = Arc::new(AssetError::Dependency {
      path: AssetPath::parse("ui/game.ui.ron").unwrap(),
      dependency: path.clone(),
      error: decode,
    });
    assert_eq!(UiFileError::from(needed), mistakes, "the import's mistakes");
    let missing = UiFileError::from(Arc::new(AssetError::NotFound { path }));
    assert_eq!(missing.to_string(), "no asset at \"ui/menu.ui.ron\"");
    assert_eq!(missing.issues[0].line, 0);
  }
}
