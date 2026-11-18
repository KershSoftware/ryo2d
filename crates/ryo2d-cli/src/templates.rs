//! The files `init` writes, read from `templates/` and filled in.

use crate::error::CliError;

pub const MAIN_RS: &str = include_str!("../templates/main.rs.tmpl");
pub const CARGO_TOML: &str = include_str!("../templates/Cargo.toml.tmpl");
pub const RYO_TOML: &str = include_str!("../templates/Ryo.toml.tmpl");
pub const README: &str = include_str!("../templates/README.md.tmpl");
pub const GITIGNORE: &str = include_str!("../templates/gitignore.tmpl");
pub const BUILD_RS: &str = include_str!("../templates/build.rs.tmpl");

/// Replaces each `{{key}}` in `template` with its value. A `{{` left after
/// that is a placeholder with no value, which is an error naming it.
pub fn render(template: &str, values: &[(&str, &str)]) -> Result<String, CliError> {
  let mut text = template.to_string();
  for (key, value) in values {
    text = text.replace(&format!("{{{{{key}}}}}"), value);
  }
  if let Some(at) = text.find("{{") {
    let rest = &text[at..];
    let end = rest.find("}}").map_or(rest.len().min(20), |end| end + 2);
    return Err(CliError::Other(format!(
      "the template has a placeholder with no value: {}",
      &rest[..end]
    )));
  }
  Ok(text)
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
}
