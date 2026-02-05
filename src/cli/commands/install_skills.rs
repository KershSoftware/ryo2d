//! Install Skills command - installs Agent Skills into project

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use super::{CliError, CliResult, Output};

/// AI assistant platform for skills installation
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SkillsPlatform {
  /// Claude Code - installs to .claude/skills/
  Claude,
  /// GitHub Copilot - installs to .github/skills/
  Copilot,
}

impl SkillsPlatform {
  /// Get the relative path for skills directory
  pub fn skills_path(&self) -> PathBuf {
    match self {
      SkillsPlatform::Claude => PathBuf::from(".claude").join("skills"),
      SkillsPlatform::Copilot => PathBuf::from(".github").join("skills"),
    }
  }

  /// Get display name
  pub fn name(&self) -> &'static str {
    match self {
      SkillsPlatform::Claude => "Claude",
      SkillsPlatform::Copilot => "Copilot",
    }
  }
}

/// Available skills to install
const AVAILABLE_SKILLS: &[(&str, &str)] = &[
  (
    "ryo-api",
    "Complete API reference for ryo2d::prelude - all types, methods, and signatures",
  ),
  (
    "ryo-animation",
    "Sprite animations, Animator component, AnimationController state machines",
  ),
  (
    "ryo-audio",
    "Sound effects and music with Kira audio backend",
  ),
  (
    "ryo-cli",
    "Using the ryo2d CLI for building, running, and developing games",
  ),
  (
    "ryo-config-save",
    "Project configuration (Ryo.toml), user settings (system.json), and the 3-tier save system",
  ),
  (
    "ryo-controller",
    "Gamepad/controller input handling with gilrs - buttons, sticks, triggers",
  ),
  (
    "ryo-ecs-internals",
    "Working on the hecs wrapper, adding components, and ECS patterns",
  ),
  (
    "ryo-engine-dev",
    "Working on the Ryo2D engine - architecture, systems, public API",
  ),
  (
    "ryo-events",
    "Event bus system for decoupled inter-system communication",
  ),
  (
    "ryo-game-dev",
    "Building games with Ryo2D engine - ECS, entities, scenes, input",
  ),
  (
    "ryo-network",
    "Multiplayer networking - client-server, replication, RPCs, WebRTC NAT traversal",
  ),
  (
    "ryo-physics",
    "Rapier2D physics integration - colliders, rigid bodies, events",
  ),
  (
    "ryo-procgen",
    "Procedural generation framework for dungeons, caves, terrain, and more",
  ),
  (
    "ryo-renderer",
    "Working on the wgpu renderer, shaders, and GPU batching",
  ),
  (
    "ryo-ui",
    "Hybrid UI system for HUDs, menus, settings screens, and in-game interfaces",
  ),
];

/// Execute the install-skills command
pub fn execute(args: &[String]) -> CliResult<()> {
  // Handle --help
  if args.iter().any(|a| a == "--help" || a == "-h") {
    print_help();
    return Ok(());
  }

  // Discover workspace from current directory
  let workspace = super::discover_workspace()?;
  let project_dir = workspace.root();

  // Prompt for platform selection
  let platform = prompt_platform_selection()?;

  install_skills_to_dir(project_dir, platform, false)
}

/// Install all skills to a project directory
pub fn install_skills_to_dir(
  project_dir: &Path,
  platform: SkillsPlatform,
  quiet: bool,
) -> CliResult<()> {
  let skills_dir = project_dir.join(platform.skills_path());

  if !quiet {
    println!();
    println!("Installing Ryo2D Agent Skills for {}:", platform.name());
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
  update_gitignore(project_dir, platform)?;

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

/// Prompt user if they want to install skills and which platform
/// Returns None if user declines, Some(platform) if they want to install
pub fn prompt_install_skills() -> CliResult<Option<SkillsPlatform>> {
  print!("Install Agent Skills for AI assistants? (Y/n): ");
  io::stdout().flush().map_err(CliError::Io)?;

  let mut input = String::new();
  io::stdin().read_line(&mut input).map_err(CliError::Io)?;

  let response = input.trim().to_lowercase();
  if !response.is_empty() && !response.starts_with('y') {
    return Ok(None);
  }

  // User wants skills, ask which platform
  let platform = prompt_platform_selection()?;
  Ok(Some(platform))
}

/// Prompt user to select AI assistant platform
fn prompt_platform_selection() -> CliResult<SkillsPlatform> {
  println!();
  println!("Select your AI assistant:");
  println!("  1) Claude    (.claude/skills)");
  println!("  2) Copilot   (.github/skills)");
  println!();
  print!("Choice (1/2): ");
  io::stdout().flush().map_err(CliError::Io)?;

  let mut input = String::new();
  io::stdin().read_line(&mut input).map_err(CliError::Io)?;

  let choice = input.trim();
  match choice {
    "1" | "claude" | "Claude" => Ok(SkillsPlatform::Claude),
    "2" | "copilot" | "Copilot" | "" => Ok(SkillsPlatform::Copilot),
    _ => {
      Output::warning("Invalid choice, defaulting to Copilot");
      Ok(SkillsPlatform::Copilot)
    }
  }
}

/// Get the engine's skills directory
fn get_engine_skills_dir() -> CliResult<PathBuf> {
  // Priority order:
  // 1. Installed location (next to binary) - for production installs
  // 2. Development location (CARGO_MANIFEST_DIR) - for cargo run

  // Try relative to binary location (for installed ryo2d)
  if let Ok(exe) = std::env::current_exe()
    && let Some(parent) = exe.parent()
  {
    // Windows: %LOCALAPPDATA%\ryo2d\bin\ryo2d.exe -> %LOCALAPPDATA%\ryo2d\skills
    // Unix: ~/.ryo2d/bin/ryo2d -> ~/.ryo2d/skills
    let bundled = parent.parent().map(|p| p.join("skills"));
    if let Some(path) = bundled
      && path.exists()
    {
      return Ok(path);
    }

    // Fallback: check directly next to binary (old behavior)
    let bundled = parent.join("skills");
    if bundled.exists() {
      return Ok(bundled);
    }
  }

  // Try to locate ryo2d workspace (for development)
  if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
    let dev_skills = PathBuf::from(manifest_dir).join(".github").join("skills");
    if dev_skills.exists() {
      return Ok(dev_skills);
    }
  }

  Err(CliError::Other(
    "Could not locate Ryo2D skills directory. Please reinstall ryo2d using the install script."
      .to_string(),
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

  fs::copy(&source_skill_md, &target_skill_md)
    .map_err(|e| CliError::Other(format!("Failed to copy SKILL.md for {}: {}", skill_name, e)))?;

  Ok(())
}

/// Update .gitignore to track ryo-* skills
fn update_gitignore(project_dir: &Path, platform: SkillsPlatform) -> CliResult<()> {
  let gitignore_path = project_dir.join(".gitignore");

  // Read existing .gitignore or create new
  let mut gitignore_content = if gitignore_path.exists() {
    fs::read_to_string(&gitignore_path)
      .map_err(|e| CliError::Other(format!("Failed to read .gitignore: {}", e)))?
  } else {
    String::new()
  };

  // Determine the skills path pattern based on platform
  let skills_pattern = match platform {
    SkillsPlatform::Claude => ".claude/skills",
    SkillsPlatform::Copilot => ".github/skills",
  };

  // Check if skills are already tracked for this platform
  let check_pattern = format!("!{}/ryo-*/", skills_pattern);
  if gitignore_content.contains(&check_pattern) {
    return Ok(()); // Already configured
  }

  // Add rules to track ryo-* skills
  if !gitignore_content.ends_with('\n') && !gitignore_content.is_empty() {
    gitignore_content.push('\n');
  }

  gitignore_content.push_str("\n# Track Ryo2D Agent Skills\n");
  gitignore_content.push_str(&format!("!{}/ryo-*/\n", skills_pattern));
  gitignore_content.push_str(&format!("!{}/ryo-*/**\n", skills_pattern));

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
    can better help you develop your game.

    You'll be prompted to choose your AI assistant:
      - Claude:  installs to .claude/skills/
      - Copilot: installs to .github/skills/

    All skills are installed automatically.

OPTIONS:
    -h, --help    Show this help message

EXAMPLES:
    # Install all skills (will prompt for platform)
    ryo2d install-skills
"#
  );
}
