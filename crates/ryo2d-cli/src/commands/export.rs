//! Export command - packages a release bundle into a distributable zip

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use super::licenses::write_font_licenses;
use super::{CliError, CliResult, Output, discover_workspace, get_cargo_package_name};

/// Execute the export command
pub fn execute() -> CliResult<()> {
  let workspace = discover_workspace()?;
  let project_name = workspace.name();
  let package_name = get_cargo_package_name(&workspace)?;
  let release_dir = workspace.root().join("release");

  // Check that release folder exists
  if !release_dir.exists() {
    return Err(CliError::NoBuild(release_dir));
  }

  // Verify the executable exists
  let binary_name = if cfg!(windows) {
    format!("{}.exe", package_name)
  } else {
    package_name.to_string()
  };

  let executable = release_dir.join(&binary_name);
  if !executable.exists() {
    return Err(CliError::Other(format!(
      "Release executable not found: {}. Run 'ryo2d build' first.",
      executable.display()
    )));
  }

  // Get version from Ryo.toml first, then Cargo.toml
  let version = get_project_version(&workspace);

  // The UTC date (YYYYMMDD)
  let datestamp = utc_date_stamp(SystemTime::now());

  // Build zip base name: {project-name}-v{version}-{datestamp} or {project-name}-{datestamp}
  let base_name = match version {
    Some(v) => format!("{}-v{}-{}", sanitize_filename(project_name), v, datestamp),
    None => format!("{}-{}", sanitize_filename(project_name), datestamp),
  };

  let zip_path = next_available_zip_path(workspace.root(), &base_name);
  let zip_filename = zip_path.file_name().unwrap().to_string_lossy();

  Output::status("Exporting", &format!("{}...", zip_filename));

  // Create zip file
  create_zip(&release_dir, &zip_path, &binary_name)?;

  Output::success(&format!("created {}", zip_filename));

  // Print file size
  if let Ok(metadata) = fs::metadata(&zip_path) {
    let size_mb = metadata.len() as f64 / (1024.0 * 1024.0);
    Output::status("Size", &format!("{:.2} MB", size_mb));
  }

  Ok(())
}

/// The version: Ryo.toml's `[project] version` as the workspace read it, else
/// Cargo.toml's `[package] version`
fn get_project_version(workspace: &ryo2d::core::config::Workspace) -> Option<String> {
  workspace
    .config()
    .version
    .clone()
    .or_else(|| cargo_package_version(&workspace.root().join("Cargo.toml")))
}

/// `[package] version` of a Cargo.toml
fn cargo_package_version(path: &Path) -> Option<String> {
  let manifest: toml::Table = fs::read_to_string(path).ok()?.parse().ok()?;
  manifest
    .get("package")?
    .get("version")?
    .as_str()
    .map(String::from)
}

/// The UTC date of `now` as `YYYYMMDD`, worked out from the days since the
/// epoch (Howard Hinnant's civil-from-days), so it needs no time crate and
/// no system call
pub fn utc_date_stamp(now: SystemTime) -> String {
  let secs = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
  let z = (secs / 86400) as i64 + 719_468;
  let era = z.div_euclid(146_097);
  let doe = z.rem_euclid(146_097);
  let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
  let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
  let mp = (5 * doy + 2) / 153;
  let day = doy - (153 * mp + 2) / 5 + 1;
  let month = if mp < 10 { mp + 3 } else { mp - 9 };
  let year = yoe + era * 400 + i64::from(month <= 2);
  format!("{year:04}{month:02}{day:02}")
}

/// Find next available zip path, appending -001, -002, etc. if the base name already exists
fn next_available_zip_path(root: &Path, base_name: &str) -> std::path::PathBuf {
  let initial = root.join(format!("{}.zip", base_name));
  if !initial.exists() {
    return initial;
  }

  for i in 1..=999 {
    let path = root.join(format!("{}-{:03}.zip", base_name, i));
    if !path.exists() {
      return path;
    }
  }

  // Fallback: overwrite the base file
  initial
}

/// Sanitize project name for use in filename
fn sanitize_filename(name: &str) -> String {
  name
    .chars()
    .map(|c| {
      if c.is_alphanumeric() || c == '-' || c == '_' {
        c
      } else if c == ' ' {
        '-'
      } else {
        '_'
      }
    })
    .collect()
}

/// Create a zip file from the release directory, with the built-in fonts'
/// licences in its `licenses/` (written into the release first, so a
/// release made without them still ships them). The release's top-level file
/// named `executable` is written with mode 0o755 and every other file 0o644,
/// so the game runs after unzipping on Mac and Linux (a Windows binary gets
/// 0o755 too, which does no harm).
fn create_zip(release_dir: &Path, zip_path: &Path, executable: &str) -> CliResult<()> {
  write_font_licenses(release_dir)?;
  let file = File::create(zip_path)
    .map_err(|e| CliError::Other(format!("Failed to create zip file: {}", e)))?;

  let mut zip = ZipWriter::new(file);
  let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

  // Walk the release directory and add all files
  add_directory_to_zip(&mut zip, release_dir, release_dir, options, executable)?;

  zip
    .finish()
    .map_err(|e| CliError::Other(format!("Failed to finalize zip: {}", e)))?;

  Ok(())
}

/// Recursively add a directory to the zip
fn add_directory_to_zip(
  zip: &mut ZipWriter<File>,
  base_path: &Path,
  current_path: &Path,
  options: SimpleFileOptions,
  executable: &str,
) -> CliResult<()> {
  for entry in fs::read_dir(current_path)
    .map_err(|e| CliError::Other(format!("Failed to read directory: {}", e)))?
  {
    let entry =
      entry.map_err(|e| CliError::Other(format!("Failed to read directory entry: {}", e)))?;
    let path = entry.path();

    // Get relative path from base
    let relative_path = path
      .strip_prefix(base_path)
      .map_err(|e| CliError::Other(format!("Path error: {}", e)))?;

    // Convert to forward slashes for zip compatibility
    let zip_path = relative_path
      .to_string_lossy()
      .replace(std::path::MAIN_SEPARATOR, "/");

    if path.is_dir() {
      // Add directory entry (with trailing slash)
      zip
        .add_directory(format!("{}/", zip_path), options.unix_permissions(0o755))
        .map_err(|e| CliError::Other(format!("Failed to add directory to zip: {}", e)))?;

      // Recurse into directory
      add_directory_to_zip(zip, base_path, &path, options, executable)?;
    } else {
      // Add file
      let mode = if zip_path == executable { 0o755 } else { 0o644 };
      zip
        .start_file(&zip_path, options.unix_permissions(mode))
        .map_err(|e| CliError::Other(format!("Failed to start file in zip: {}", e)))?;

      let mut file = File::open(&path)
        .map_err(|e| CliError::Other(format!("Failed to open file {}: {}", path.display(), e)))?;

      let mut buffer = Vec::new();
      file
        .read_to_end(&mut buffer)
        .map_err(|e| CliError::Other(format!("Failed to read file {}: {}", path.display(), e)))?;

      zip
        .write_all(&buffer)
        .map_err(|e| CliError::Other(format!("Failed to write to zip: {}", e)))?;
    }
  }

  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::commands::licenses::{FONT_LICENSES, LICENSES_DIR};

  #[test]
  fn the_version_comes_from_the_workspace_s_ryo_toml() {
    // Read as the workspace read it, not from a file named Ryo.toml on disk
    let dir = tempfile::tempdir().unwrap();
    let workspace = ryo2d::core::config::Workspace::from_toml_str(
      "[project]\nversion = \"1.2.3\"\n",
      dir.path().to_path_buf(),
    )
    .unwrap();
    assert_eq!(get_project_version(&workspace).as_deref(), Some("1.2.3"));
  }

  #[test]
  fn without_a_project_version_the_cargo_version_is_used() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
      dir.path().join("Cargo.toml"),
      "[package]\nname = \"g\"\nversion = \"0.4.0\"\n",
    )
    .unwrap();
    let workspace =
      ryo2d::core::config::Workspace::from_toml_str("", dir.path().to_path_buf()).unwrap();
    assert_eq!(get_project_version(&workspace).as_deref(), Some("0.4.0"));
  }

  #[test]
  fn the_exported_zip_carries_the_font_licences() {
    // A release made before builds wrote the licences
    let dir = tempfile::tempdir().unwrap();
    let release = dir.path().join("release");
    fs::create_dir_all(&release).unwrap();
    fs::write(release.join("g"), b"a game").unwrap();
    let zip_path = dir.path().join("g.zip");
    create_zip(&release, &zip_path, "g").unwrap();
    let mut archive = zip::ZipArchive::new(File::open(&zip_path).unwrap()).unwrap();
    for (name, text) in FONT_LICENSES {
      let path = format!("{LICENSES_DIR}/{name}");
      let mut read = String::new();
      let found = archive
        .by_name(&path)
        .map(|mut entry| entry.read_to_string(&mut read));
      assert!(found.is_ok(), "{path} is in the zip");
      assert_eq!(read, text, "{path}");
    }
  }

  #[test]
  fn the_exported_game_is_executable_and_its_assets_are_not() {
    let dir = tempfile::tempdir().unwrap();
    let release = dir.path().join("release");
    fs::create_dir_all(release.join("assets")).unwrap();
    fs::write(release.join("g"), b"a game").unwrap();
    fs::write(release.join("assets").join("a.txt"), b"an asset").unwrap();
    let zip_path = dir.path().join("g.zip");
    create_zip(&release, &zip_path, "g").unwrap();
    let mut archive = zip::ZipArchive::new(File::open(&zip_path).unwrap()).unwrap();
    let mode =
      |archive: &mut zip::ZipArchive<File>, name: &str| archive.by_name(name).unwrap().unix_mode();
    assert_eq!(mode(&mut archive, "g"), Some(0o100755));
    assert_eq!(mode(&mut archive, "assets/a.txt"), Some(0o100644));
  }

  #[test]
  fn the_date_stamp_is_the_utc_date() {
    use std::time::{Duration, UNIX_EPOCH};
    let at = |secs: u64| utc_date_stamp(UNIX_EPOCH + Duration::from_secs(secs));
    assert_eq!(utc_date_stamp(UNIX_EPOCH), "19700101");
    assert_eq!(at(951_868_799), "20000229"); // 2000-02-29T23:59:59Z
    assert_eq!(at(951_868_800), "20000301");
    assert_eq!(at(1_798_761_599), "20261231"); // 2026-12-31T23:59:59Z
    assert_eq!(at(1_798_761_600), "20270101");
    assert_eq!(at(4_107_542_399), "21000228"); // 2100 is not a leap year
    assert_eq!(at(4_107_542_400), "21000301");
  }
}
