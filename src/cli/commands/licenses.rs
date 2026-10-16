//! The licences of the fonts the engine embeds (feature `default-fonts`, on
//! by default), written into every release: the SIL Open Font License asks
//! that each copy of a font carries its copyright notice and licence. The
//! texts are compiled into the CLI, so it needs no source checkout.

use std::fs;
use std::path::Path;

use super::{CliError, CliResult};

/// The folder in a release that holds the licences
pub(super) const LICENSES_DIR: &str = "licenses";

/// Each licence file's name and text
pub(super) const FONT_LICENSES: [(&str, &str); 4] = [
  ("README.txt", README),
  (
    "OFL-Inter.txt",
    include_str!("../../assets/fonts/OFL-Inter.txt"),
  ),
  (
    "OFL-JetBrainsMono.txt",
    include_str!("../../assets/fonts/OFL-JetBrainsMono.txt"),
  ),
  (
    "m5x7-LICENSE.txt",
    include_str!("../../assets/fonts/m5x7-LICENSE.txt"),
  ),
];

const README: &str = "\
Fonts

This game is made with the Ryo2D engine, which builds these fonts into the
game (subset to Latin, Latin Extended, Greek and Cyrillic):

- Inter (Regular and Bold), by The Inter Project Authors,
  SIL Open Font License 1.1: OFL-Inter.txt
- JetBrains Mono (Regular), by The JetBrains Mono Project Authors,
  SIL Open Font License 1.1: OFL-JetBrainsMono.txt
- m5x7, by Daniel Linssen, CC0 1.0 (public domain): m5x7-LICENSE.txt
";

/// Write the font licences into `release_dir/licenses/`, replacing any
/// that are there
pub(super) fn write_font_licenses(release_dir: &Path) -> CliResult<()> {
  let dir = release_dir.join(LICENSES_DIR);
  fs::create_dir_all(&dir).map_err(|e| {
    CliError::Other(format!(
      "Failed to create licence directory {}: {}",
      dir.display(),
      e
    ))
  })?;
  for (name, text) in FONT_LICENSES {
    let path = dir.join(name);
    fs::write(&path, text)
      .map_err(|e| CliError::Other(format!("Failed to write {}: {}", path.display(), e)))?;
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn every_font_licence_is_written_into_the_release() {
    let release = tempfile::tempdir().unwrap();
    write_font_licenses(release.path()).unwrap();
    let dir = release.path().join(LICENSES_DIR);
    for (name, text) in FONT_LICENSES {
      assert_eq!(std::fs::read_to_string(dir.join(name)).unwrap(), text);
    }
    let ofl = std::fs::read_to_string(dir.join("OFL-Inter.txt")).unwrap();
    assert!(ofl.contains("SIL OPEN FONT LICENSE Version 1.1"));
    let readme = std::fs::read_to_string(dir.join("README.txt")).unwrap();
    for font in ["Inter", "JetBrains Mono", "m5x7"] {
      assert!(readme.contains(font), "the README names {font}");
    }
  }
}
