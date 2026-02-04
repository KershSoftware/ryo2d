//! Export command - packages a release bundle into a distributable zip

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use super::{CliError, CliResult, Output, discover_workspace, get_cargo_package_name};

/// Execute the export command
pub fn execute(args: &[String]) -> CliResult<()> {
  // Handle --help
  if args.iter().any(|a| a == "--help" || a == "-h") {
    print_help();
    return Ok(());
  }

  let workspace = discover_workspace()?;
  let project_name = workspace.name();
  let package_name = get_cargo_package_name(&workspace)?;
  let release_dir = workspace.root().join("release");

  // Check that release folder exists
  if !release_dir.exists() {
    return Err(CliError::NoBuildExists(release_dir));
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

  // Generate datestamp (YYYYMMDD)
  let datestamp = chrono_free_datestamp();

  // Build zip filename: {project-name}-v{version}-{datestamp} or {project-name}-{datestamp}
  let zip_filename = match version {
    Some(v) => format!(
      "{}-v{}-{}.zip",
      sanitize_filename(project_name),
      v,
      datestamp
    ),
    None => format!("{}-{}.zip", sanitize_filename(project_name), datestamp),
  };

  let zip_path = workspace.root().join(&zip_filename);

  Output::status("Exporting", &format!("{}...", zip_filename));

  // Create zip file
  create_zip(&release_dir, &zip_path, project_name)?;

  Output::success(&format!("created {}", zip_filename));

  // Print file size
  if let Ok(metadata) = fs::metadata(&zip_path) {
    let size_mb = metadata.len() as f64 / (1024.0 * 1024.0);
    Output::status("Size", &format!("{:.2} MB", size_mb));
  }

  Ok(())
}

/// Get version from Ryo.toml [project].version, falling back to Cargo.toml [package].version
fn get_project_version(workspace: &crate::core::config::Workspace) -> Option<String> {
  // Try Ryo.toml first
  if let Some(version) = read_version_from_toml(&workspace.root().join("Ryo.toml"), "project") {
    return Some(version);
  }

  // Fall back to Cargo.toml
  read_version_from_toml(&workspace.root().join("Cargo.toml"), "package")
}

/// Read version field from a TOML file's specified section
fn read_version_from_toml(path: &Path, section: &str) -> Option<String> {
  let contents = fs::read_to_string(path).ok()?;
  let parsed: toml::Value = contents.parse().ok()?;
  parsed
    .get(section)?
    .get("version")?
    .as_str()
    .map(String::from)
}

/// Generate datestamp without external crate (YYYYMMDD format)
fn chrono_free_datestamp() -> String {
  use std::time::{SystemTime, UNIX_EPOCH};

  let duration = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .unwrap_or_default();

  let secs = duration.as_secs();

  // Calculate date from unix timestamp
  // Days since epoch
  let days = secs / 86400;

  // Algorithm to convert days since epoch to year/month/day
  // Based on Howard Hinnant's date algorithms
  let z = days as i64 + 719468;
  let era = if z >= 0 { z } else { z - 146096 } / 146097;
  let doe = (z - era * 146097) as u32;
  let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
  let y = yoe as i64 + era * 400;
  let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
  let mp = (5 * doy + 2) / 153;
  let d = doy - (153 * mp + 2) / 5 + 1;
  let m = if mp < 10 { mp + 3 } else { mp - 9 };
  let year = if m <= 2 { y + 1 } else { y };

  format!("{:04}{:02}{:02}", year, m, d)
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

/// Create a zip file from the release directory
fn create_zip(release_dir: &Path, zip_path: &Path, _project_name: &str) -> CliResult<()> {
  let file = File::create(zip_path)
    .map_err(|e| CliError::Other(format!("Failed to create zip file: {}", e)))?;

  let mut zip = ZipWriter::new(file);
  let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

  // Walk the release directory and add all files
  add_directory_to_zip(&mut zip, release_dir, release_dir, options)?;

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
        .add_directory(format!("{}/", zip_path), options)
        .map_err(|e| CliError::Other(format!("Failed to add directory to zip: {}", e)))?;

      // Recurse into directory
      add_directory_to_zip(zip, base_path, &path, options)?;
    } else {
      // Add file
      zip
        .start_file(&zip_path, options)
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

fn print_help() {
  println!(
    r#"
ryo2d-export - Package release bundle into a distributable zip

USAGE:
    ryo2d export [OPTIONS]

OPTIONS:
    -h, --help    Show this help message

DESCRIPTION:
    Creates a distributable zip file from the release/ directory.
    The zip file is named: {{project-name}}-v{{version}}-{{datestamp}}.zip

    Version is read from:
    1. Ryo.toml [project].version (if present)
    2. Cargo.toml [package].version (fallback)
    3. If no version found, the -v{{version}} part is omitted

    Datestamp format: YYYYMMDD

PREREQUISITES:
    Run 'ryo2d build' first to create the release bundle.

EXAMPLE:
    ryo2d build    # Create release bundle
    ryo2d export   # Package into zip

    # Output: MyGame-v1.0.0-20260203.zip (if version found)
    # Or:     MyGame-20260203.zip (if no version)
"#
  );
}
