//! Install Skills command - installs Agent Skills into project

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use super::{CliError, CliResult, Output};

/// Available skills to install
const AVAILABLE_SKILLS: &[(&str, &str)] = &[
  ("ryo-animation", "Sprite animations, Animator component, AnimationController state machines"),
  ("ryo-audio", "Sound effects and music with Kira audio backend"),
  ("ryo-cli", "Using the ryo2d CLI for building, running, and developing games"),
  ("ryo-config-save", "Project configuration (Ryo.toml), user settings (system.json), and the 3-tier save system"),
  ("ryo-ecs-internals", "Working on the hecs wrapper, adding components, and ECS patterns"),
  ("ryo-engine-dev", "Working on the Ryo2D engine - architecture, systems, public API"),
  ("ryo-game-dev", "Building games with Ryo2D engine - ECS, entities, scenes, input"),
  ("ryo-physics", "Rapier2D physics integration - colliders, rigid bodies, events"),
  ("ryo-renderer", "Working on the wgpu renderer, shaders, and GPU batching"),
  ("ryo-ui", "Immediate-mode UI for HUDs, menus, and overlays"),
];

/// Execute the install-skills command
pub fn execute(args: &[String]) -> CliResult<()> {
  // Handle --help
  if args.iter().any(|a| a == "--help" || a == "-h") {
    print_help();
    return Ok(());
  }

  install_skills_to_dir(&std::env::current_dir().map_err(|e| CliError::Other(format!("Failed to get current directory: {}", e)))?, false)
}

/// Install all skills to a project directory
pub fn install_skills_to_dir(project_dir: &Path, quiet: bool) -> CliResult<()> {
  // Detect IDE/Editor
  let ide = detect_ide(project_dir);
  let skills_dir = match ide {
    Some(Ide::VsCode) => project_dir.join(".github").join("skills"),
    Some(Ide::Claude) => project_dir.join(".github").join("skills"),
    None => {
      if !quiet {
        Output::warning("No supported IDE detected (VS Code, Claude)");
        Output::status("Using", ".github/skills/ (VS Code/Claude format)");
      }
      project_dir.join(".github").join("skills")
    }
  };

  if !quiet {
    println!();
    println!("Installing Ryo2D Agent Skills:");
    println!();
    for (name, desc) in AVAILABLE_SKILLS {
      println!("  • {} - {}", name, desc);
    }
    println!();
  }

  // Install all skills
  let skills_to_install: Vec<&str> = AVAILABLE_SKILLS.iter().map(|(name, _)| *name).collect();

  // Create skills directory
  fs::create_dir_all(&skills_dir)
    .map_err(|e| CliError::Other(format!("Failed to create skills directory: {}", e)))?;

  // Get engine skills directory (bundled with ryo2d)
  let engine_skills_dir = get_engine_skills_dir()?;

  // Install selected skills
  for skill_name in skills_to_install {
    if !quiet {
      Output::status("Installing", skill_name);
    }
    install_skill(&engine_skills_dir, &skills_dir, skill_name)?;
  }

  // Update .gitignore
  update_gitignore(project_dir)?;

  if !quiet {
    println!();
    Output::success("Skills installed");
    println!();
    println!("Skills installed to: {}", skills_dir.display());
    println!();
    println!("Your AI assistant can now use these skills when helping with your project.");
    println!();
  }

  Ok(())
}

/// Detect which IDE/Editor is being used
#[derive(Debug)]
#[allow(dead_code)]
enum Ide {
  VsCode,
  Claude,
}

fn detect_ide(project_dir: &Path) -> Option<Ide> {
  // Check for VS Code
  if project_dir.join(".vscode").exists() {
    return Some(Ide::VsCode);
  }

  // For now, assume VS Code/Claude format (they use the same .github/skills/)
  // In future, we could detect other IDEs like Cursor, Zed, etc.
  None
}

/// Prompt user if they want to install skills (yes/no)
pub fn prompt_install_skills() -> CliResult<bool> {
  print!("Install Agent Skills for AI assistants? (Y/n): ");
  io::stdout().flush().map_err(CliError::Io)?;

  let mut input = String::new();
  io::stdin().read_line(&mut input).map_err(CliError::Io)?;

  let response = input.trim().to_lowercase();
  Ok(response.is_empty() || response.starts_with('y'))
}

/// Get the engine's skills directory
fn get_engine_skills_dir() -> CliResult<PathBuf> {
  // Try to find the ryo2d repository skills directory
  // This could be:
  // 1. Installed via cargo (in $CARGO_HOME/git/...)
  // 2. Local development (workspace root)
  // 3. Bundled with the binary

  // For now, we'll embed skills at compile time and write them out
  // In the future, we could bundle them differently
  
  // Try to locate ryo2d workspace (for development)
  if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
    let dev_skills = PathBuf::from(manifest_dir).join(".github").join("skills");
    if dev_skills.exists() {
      return Ok(dev_skills);
    }
  }

  // Try relative to binary location (for installed ryo2d)
  if let Ok(exe) = std::env::current_exe()
    && let Some(parent) = exe.parent()
  {
    let bundled = parent.join("skills");
    if bundled.exists() {
      return Ok(bundled);
    }
  }

  Err(CliError::Other(
    "Could not locate Ryo2D skills directory. Are you running from the ryo2d repository?".to_string()
  ))
}

/// Install a single skill
fn install_skill(engine_skills_dir: &Path, target_dir: &Path, skill_name: &str) -> CliResult<()> {
  let source = engine_skills_dir.join(skill_name);
  let target = target_dir.join(skill_name);

  if !source.exists() {
    return Err(CliError::Other(format!(
      "Skill '{}' not found in engine directory",
      skill_name
    )));
  }

  // Create target skill directory
  fs::create_dir_all(&target)
    .map_err(|e| CliError::Other(format!("Failed to create skill directory: {}", e)))?;

  // Copy SKILL.md
  let source_skill_md = source.join("SKILL.md");
  let target_skill_md = target.join("SKILL.md");

  fs::copy(&source_skill_md, &target_skill_md).map_err(|e| {
    CliError::Other(format!("Failed to copy SKILL.md for {}: {}", skill_name, e))
  })?;

  Ok(())
}

/// Update .gitignore to track ryo-* skills
fn update_gitignore(project_dir: &Path) -> CliResult<()> {
  let gitignore_path = project_dir.join(".gitignore");

  // Read existing .gitignore or create new
  let mut gitignore_content = if gitignore_path.exists() {
    fs::read_to_string(&gitignore_path)
      .map_err(|e| CliError::Other(format!("Failed to read .gitignore: {}", e)))?
  } else {
    String::new()
  };

  // Check if skills are already tracked
  if gitignore_content.contains("!.github/skills/ryo-*/") {
    return Ok(()); // Already configured
  }

  // Add rules to track ryo-* skills
  if !gitignore_content.ends_with('\n') && !gitignore_content.is_empty() {
    gitignore_content.push('\n');
  }

  gitignore_content.push_str("\n# Track Ryo2D Agent Skills\n");
  gitignore_content.push_str("!.github/skills/ryo-*/\n");
  gitignore_content.push_str("!.github/skills/ryo-*/**\n");

  fs::write(&gitignore_path, gitignore_content)
    .map_err(|e| CliError::Other(format!("Failed to update .gitignore: {}", e)))?;

  Output::status("Updated", ".gitignore to track ryo-* skills");

  Ok(())
}

fn print_help() {
  println!(
    r#"
ryo2d install-skills - Install Agent Skills for AI assistants

USAGE:
    ryo2d install-skills [OPTIONS]

DESCRIPTION:
    Installs all Ryo2D Agent Skills into your project so AI assistants
    (like GitHub Copilot) can better help you develop your game.

    Skills are installed to .github/skills/ (VS Code/Claude format).
    All 10 skills are installed automatically.

OPTIONS:
    -h, --help    Show this help message

EXAMPLES:
    # Install all skills
    ryo2d install-skills
"#
  );
}
