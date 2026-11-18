//! Init command - scaffolds a new Ryo2D project

use std::fs;
use std::path::{Path, PathBuf};

use super::{CliError, CliResult, Output};
use crate::args::{InitArgs, SkillsChoice};
use crate::prompt::Prompt;
use crate::templates;

/// Execute the init command
pub fn execute(args: &InitArgs) -> CliResult<()> {
  let mut prompt = Prompt::from_stdin();
  // Get project name from args or prompt
  let project_name = match &args.name {
    Some(name) => name.clone(),
    None => prompt_project_name(&mut prompt)?,
  };

  // Validate project name
  if !is_valid_project_name(&project_name) {
    return Err(CliError::Other(
      "Invalid project name. Use lowercase letters, numbers, hyphens, and underscores only."
        .to_string(),
    ));
  }

  let project_dir = Path::new(&project_name);

  // Check if directory already exists
  if project_dir.exists() {
    return Err(CliError::Other(format!(
      "Directory '{}' already exists",
      project_name
    )));
  }

  Output::status("Creating", &format!("new Ryo2D project '{}'", project_name));

  // Create project structure
  create_project_structure(project_dir, &project_name)?;

  Output::success(&format!("created project '{}'", project_name));
  println!();

  // The flag, else the person's answer; with no one to ask, none
  let choice = args
    .skills
    .or_else(|| super::install_skills::ask_skills(&mut prompt, true));
  match choice {
    Some(choice) if choice != SkillsChoice::None => {
      println!();
      super::install_skills::install_choice(project_dir, choice, false)?;
    }
    _ => {
      Output::status("Skipped", "Agent Skills: none were installed");
      println!("             Install them with 'ryo2d init --skills claude|copilot|both'");
      if prompt.interactive {
        println!("             for a new project, or 'ryo2d install-skills' in this one.");
      } else {
        println!("             for a new project, or in this one:");
        println!("             ryo2d install-skills --skills <claude|copilot|both|none>");
      }
    }
  }

  println!();
  println!("Next steps:");
  println!("  cd {}", project_name);
  println!("  cargo run         # Run in development mode");
  println!("  ryo2d dev         # Run with developer tools and asset hot reload");
  println!("  ryo2d build       # Build for release");
  println!();

  Ok(())
}

/// Prompt user for project name
fn prompt_project_name(prompt: &mut Prompt) -> CliResult<String> {
  let Some(name) = prompt.ask("Project name: ") else {
    return Err(CliError::Other(
      "no project name was given and there is no one to ask: run 'ryo2d init NAME'".to_string(),
    ));
  };

  if name.is_empty() {
    return Err(CliError::Other("Project name cannot be empty".to_string()));
  }

  Ok(name)
}

/// Validate project name (similar to cargo package names)
fn is_valid_project_name(name: &str) -> bool {
  !name.is_empty()
    && name
      .chars()
      .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// The files of a new project, as (path, contents), from the templates
fn project_files(
  project_name: &str,
  engine_dependency: &str,
  engine_root: &str,
) -> CliResult<Vec<(&'static str, String)>> {
  let title = display_name(project_name);
  let values = [
    ("name", project_name),
    ("title", title.as_str()),
    ("engine_dependency", engine_dependency),
    ("engine_root", engine_root),
  ];
  Ok(vec![
    (
      "Cargo.toml",
      templates::render(templates::CARGO_TOML, &values)?,
    ),
    ("Ryo.toml", templates::render(templates::RYO_TOML, &values)?),
    ("main.rs", templates::render(templates::MAIN_RS, &values)?),
    ("build.rs", templates::render(templates::BUILD_RS, &values)?),
    ("README.md", templates::render(templates::README, &values)?),
    (
      ".gitignore",
      templates::render(templates::GITIGNORE, &values)?,
    ),
  ])
}

/// The engine as a dependency line and the folder its crates are under
fn engine_dependency() -> (String, String) {
  match get_installed_engine_path() {
    Some(engine_path) => {
      // Forward slashes, for TOML
      let path = engine_path.display().to_string().replace('\\', "/");
      (format!(r#"ryo2d = {{ path = "{path}" }}"#), path)
    }
    None => (
      r#"ryo2d = { path = "../path/to/ryo2d" }  # Update this path"#.to_string(),
      "../path/to/ryo2d".to_string(),
    ),
  }
}

/// Create the project directory structure and files
fn create_project_structure(project_dir: &Path, project_name: &str) -> CliResult<()> {
  for folder in ["", "assets", "data", "src"] {
    fs::create_dir_all(project_dir.join(folder))
      .map_err(|e| CliError::Other(format!("Failed to create the '{folder}' folder: {e}")))?;
  }
  let (dependency, root) = engine_dependency();
  for (path, contents) in project_files(project_name, &dependency, &root)? {
    fs::write(project_dir.join(path), contents)
      .map_err(|e| CliError::Other(format!("Failed to write {path}: {e}")))?;
  }
  Ok(())
}

/// "my-game" as "My Game"
fn display_name(project_name: &str) -> String {
  project_name
    .split('-')
    .map(|word| {
      let mut c = word.chars();
      match c.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + c.as_str(),
      }
    })
    .collect::<Vec<_>>()
    .join(" ")
}

/// Get the installed engine path
fn get_installed_engine_path() -> Option<PathBuf> {
  // Check for installed engine in standard locations
  #[cfg(target_os = "windows")]
  {
    let engine_path = std::env::var("LOCALAPPDATA")
      .ok()
      .map(PathBuf::from)
      .map(|p| p.join("ryo2d").join("engine"));

    if let Some(ref path) = engine_path
      && path.exists()
    {
      return engine_path;
    }
  }

  #[cfg(not(target_os = "windows"))]
  {
    let engine_path = std::env::var("HOME")
      .ok()
      .map(PathBuf::from)
      .map(|p| p.join(".ryo2d").join("engine"));

    if let Some(ref path) = engine_path
      && path.exists()
    {
      return engine_path;
    }
  }

  None
}

#[cfg(test)]
mod tests {
  use super::project_files;

  fn ryo_toml() -> String {
    match project_files("game", "ryo2d = { path = \"/engine\" }", "/engine") {
      Ok(files) => files
        .into_iter()
        .find(|(path, _)| *path == "Ryo.toml")
        .map(|(_, text)| text)
        .unwrap_or_default(),
      Err(error) => panic!("the templates render: {error}"),
    }
  }

  #[test]
  fn every_template_renders_with_init_s_values() {
    let files = match project_files("my-game", "ryo2d = { path = \"/engine\" }", "/engine") {
      Ok(files) => files,
      Err(error) => panic!("the templates render: {error}"),
    };
    assert_eq!(files.len(), 6);
    for (path, text) in &files {
      assert!(!text.is_empty() && !text.contains("{{"), "{path}");
    }
    let cargo = &files[0].1;
    assert!(cargo.contains("name = \"my-game\""));
    assert!(cargo.contains("ryo2d = { path = \"/engine\" }"));
    assert!(!cargo.contains("your-username") && !cargo.contains("rand"));
  }

  /// A new game starts at the mix as designed: master at 1, which is the
  /// engine's own default (the engine's tests hold that)
  #[test]
  fn a_new_game_s_master_volume_is_the_mix_as_designed() {
    let toml: toml::Value = match toml::from_str(&ryo_toml()) {
      Ok(toml) => toml,
      Err(error) => panic!("the template is TOML: {error}"),
    };
    let master = toml
      .get("system")
      .and_then(|system| system.get("master_volume"))
      .and_then(toml::Value::as_float);
    assert_eq!(master, Some(1.0));
  }

  #[test]
  fn a_new_game_s_ryo_toml_reads_without_a_warning() {
    let log = ryo2d::logging::capture_log();
    let read =
      ryo2d::core::config::Workspace::from_toml_str(&ryo_toml(), std::path::PathBuf::from("game"));
    assert!(read.is_ok());
    let warnings = log.warnings();
    assert!(warnings.is_empty(), "{warnings:?}");
  }
}
