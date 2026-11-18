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
  if let Err(problem) = check_project_name(&project_name) {
    return Err(CliError::Other(format!("Invalid project name: {problem}")));
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
      "no project name was given: run 'ryo2d init NAME'".to_string(),
    ));
  };

  if name.is_empty() {
    return Err(CliError::Other("Project name cannot be empty".to_string()));
  }

  Ok(name)
}

/// Words cargo refuses as a binary package's name: Rust's keywords, `test`
/// (it conflicts with the built-in test library) and the names of cargo's own
/// folders in `target/` (`ryo2d build` writes `target/release/<name>`)
const REFUSED_NAMES: [&str; 51] = [
  "abstract", "as", "async", "await", "become", "box", "break", "const", "continue", "crate", "do",
  "dyn", "else", "enum", "extern", "false", "final", "fn", "for", "if", "impl", "in", "let",
  "loop", "macro", "match", "mod", "move", "mut", "override", "priv", "pub", "ref", "return",
  "self", "static", "struct", "super", "test", "trait", "true", "try", "type", "typeof", "unsafe",
  "unsized", "use", "virtual", "where", "while", "yield",
];

/// Names that conflict with the folders cargo makes in `target/`
const CARGO_FOLDERS: [&str; 4] = ["deps", "examples", "build", "incremental"];

/// What is wrong with a project name for cargo (which names the package
/// after it), or `Ok` when it is fine
fn check_project_name(name: &str) -> Result<(), String> {
  if name.is_empty() {
    return Err("the name cannot be empty".to_string());
  }
  if !name
    .chars()
    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
  {
    return Err("use lowercase letters, numbers, hyphens and underscores only".to_string());
  }
  if name.starts_with(|c: char| c.is_ascii_digit()) {
    return Err(format!("`{name}` cannot start with a digit"));
  }
  if CARGO_FOLDERS.contains(&name) {
    return Err(format!(
      "`{name}` conflicts with the names of cargo's build folders, so cargo refuses it as a package name"
    ));
  }
  if REFUSED_NAMES.contains(&name) {
    return Err(format!(
      "`{name}` is a Rust keyword or conflicts with a built-in library, so cargo refuses it as a package name"
    ));
  }
  Ok(())
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
  for folder in ["assets", "data", "src"] {
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
  use super::{check_project_name, project_files};

  #[test]
  fn a_name_cargo_refuses_is_refused_before_anything_is_written() {
    for name in [
      "my-game", "_x", "game2", "a", "union", "gen", "con", "nul", "core",
    ] {
      assert!(check_project_name(name).is_ok(), "{name}");
    }
    for name in [
      "",
      "2048",
      "9a",
      "fn",
      "test",
      "deps",
      "examples",
      "build",
      "incremental",
      "Upper",
      "has space",
      "dot.name",
    ] {
      assert!(check_project_name(name).is_err(), "{name}");
    }
    assert!(check_project_name("2048").unwrap_err().contains("digit"));
    assert!(check_project_name("fn").unwrap_err().contains("keyword"));
  }

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
    assert!(!files.iter().any(|(_, text)| text.contains("your-username")));
    for (path, text) in &files {
      assert!(!text.is_empty() && !text.contains("{{"), "{path}");
    }
    let cargo = files
      .iter()
      .find(|(path, _)| *path == "Cargo.toml")
      .map(|(_, text)| text.as_str())
      .unwrap_or_default();
    assert!(cargo.contains("name = \"my-game\""));
    assert!(cargo.contains("ryo2d = { path = \"/engine\" }"));
    assert!(!cargo.contains("your-username") && !cargo.contains("rand = "));
  }

  /// A game is a binary: its lock file is committed for reproducible builds
  #[test]
  fn a_new_game_s_gitignore_keeps_cargo_lock() {
    let files = project_files("my-game", "ryo2d = { path = \"/engine\" }", "/engine");
    let ignore = files
      .unwrap_or_default()
      .into_iter()
      .find(|(path, _)| *path == ".gitignore")
      .map(|(_, text)| text)
      .unwrap_or_default();
    assert!(ignore.contains("/target/"), "{ignore}");
    assert!(!ignore.contains("Cargo.lock"), "{ignore}");
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
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Ryo.toml"), ryo_toml()).unwrap();
    let read = ryo2d::core::config::Workspace::from_path(dir.path());
    assert!(read.is_ok());
    let warnings = log.warnings();
    assert!(warnings.is_empty(), "{warnings:?}");
  }
}
