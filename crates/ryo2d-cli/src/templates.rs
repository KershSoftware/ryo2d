//! The files `init` writes, read from `templates/` and filled in.

use crate::error::CliError;

pub const MAIN_RS: &str = include_str!("../templates/main.rs.tmpl");
pub const CARGO_TOML: &str = include_str!("../templates/Cargo.toml.tmpl");
pub const RYO_TOML: &str = include_str!("../templates/Ryo.toml.tmpl");
pub const README: &str = include_str!("../templates/README.md.tmpl");
pub const GITIGNORE: &str = include_str!("../templates/gitignore.tmpl");
pub const BUILD_RS: &str = include_str!("../templates/build.rs.tmpl");

/// Replaces each `{{key}}` in `template` with its value, in one pass (a value
/// is never read for placeholders). A `{{` with no value is an error naming it.
pub fn render(template: &str, values: &[(&str, &str)]) -> Result<String, CliError> {
  let mut text = String::with_capacity(template.len());
  let mut rest = template;
  while let Some(at) = rest.find("{{") {
    text.push_str(&rest[..at]);
    let after = &rest[at + 2..];
    let Some(end) = after.find("}}") else {
      let shown: String = rest[at..].chars().take(20).collect();
      return Err(unreplaced(&shown));
    };
    let key = &after[..end];
    let Some((_, value)) = values.iter().find(|(name, _)| *name == key) else {
      return Err(unreplaced(&format!("{{{{{key}}}}}")));
    };
    text.push_str(value);
    rest = &after[end + 2..];
  }
  text.push_str(rest);
  Ok(text)
}

fn unreplaced(placeholder: &str) -> CliError {
  CliError::Other(format!(
    "the template has a placeholder with no value: {placeholder}"
  ))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_placeholder_left_unreplaced_is_an_error() {
    let error = render("a {{name}} and {{other}}", &[("name", "x")]).unwrap_err();
    assert!(error.to_string().contains("{{other}}"), "{error}");
    assert_eq!(render("a {{name}}", &[("name", "x")]).unwrap(), "a x");
  }

  #[test]
  fn an_unclosed_placeholder_before_multibyte_text_is_an_error_not_a_panic() {
    let error = render("{{ab├──├──├──├──├──├──├──", &[]).unwrap_err();
    assert!(error.to_string().contains("{{ab"), "{error}");
  }

  #[test]
  fn a_value_is_not_read_for_placeholders() {
    let text = render("{{a}} {{b}}", &[("a", "{{b}}"), ("b", "y")]).unwrap();
    assert_eq!(text, "{{b}} y");
  }
}
