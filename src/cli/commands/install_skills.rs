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

/// Embedded skills: (name, description, SKILL.md content)
const EMBEDDED_SKILLS: &[(&str, &str, &str)] = &[
  (
    "ryo-animation",
    "Sprite animations (Animator, AnimationController), tweens, knockback and squash/stretch.",
    include_str!("../skills/ryo-animation.md"),
  ),
  (
    "ryo-api",
    "Complete API reference for ryo2d::prelude - every exported type with its signatures and use, organised by area.",
    include_str!("../skills/ryo-api.md"),
  ),
  (
    "ryo-audio",
    "Sound effects and music with Kira audio backend.",
    include_str!("../skills/ryo-audio.md"),
  ),
  (
    "ryo-cli",
    "Using the ryo2d CLI for building, running, and developing games.",
    include_str!("../skills/ryo-cli.md"),
  ),
  (
    "ryo-config-save",
    "Project configuration (Ryo.toml), player settings (system.json, SystemState) and game saves (SaveManager, GameSaveData).",
    include_str!("../skills/ryo-config-save.md"),
  ),
  (
    "ryo-console",
    "Developer console system - commands, registration, debugging overlays.",
    include_str!("../skills/ryo-console.md"),
  ),
  (
    "ryo-controller",
    "Gamepad/controller input handling with gilrs - buttons, sticks, triggers.",
    include_str!("../skills/ryo-controller.md"),
  ),
  (
    "ryo-ecs-internals",
    "Working on Ryo2D's ECS (bevy_ecs presented as ryo2d::ecs) - kernel re-exports, components, propagation, interpolation, pooling and engine systems.",
    include_str!("../skills/ryo-ecs-internals.md"),
  ),
  (
    "ryo-engine-dev",
    "Working on the Ryo2D engine - the frame's phases, module map, resources, public API, crates and testing.",
    include_str!("../skills/ryo-engine-dev.md"),
  ),
  (
    "ryo-game-dev",
    "Building games with Ryo2D - scenes, entities, queries, systems, resources, messages, time and input.",
    include_str!("../skills/ryo-game-dev.md"),
  ),
  (
    "ryo-lighting",
    "2D lighting - Light2D and LightOccluder2D entities, ctx.lighting() for ambient, atmosphere and on/off, shadows, volumetric beams, sprite shadows and ambient occlusion.",
    include_str!("../skills/ryo-lighting.md"),
  ),
  (
    "ryo-messages",
    "Messages - typed, buffered notifications between scenes and systems; the engine's message types and how to define, write and read your own.",
    include_str!("../skills/ryo-messages.md"),
  ),
  (
    "ryo-network",
    "Multiplayer networking with Ryo2D - client-server, replication, RPCs.",
    include_str!("../skills/ryo-network.md"),
  ),
  (
    "ryo-particles",
    "Particle system - emitters, attractors, collisions, presets for visual effects.",
    include_str!("../skills/ryo-particles.md"),
  ),
  (
    "ryo-physics",
    "Rapier2D physics integration - colliders, rigid bodies, collision messages.",
    include_str!("../skills/ryo-physics.md"),
  ),
  (
    "ryo-procgen",
    "The optional ryo2d-procgen crate - dungeons (BSP), caves (cellular), noise terrain, random walks, Poisson sampling, WFC and MST corridors.",
    include_str!("../skills/ryo-procgen.md"),
  ),
  (
    "ryo-profiler",
    "Profiling Ryo2D games and engine - puffin integration, profiler window, performance analysis.",
    include_str!("../skills/ryo-profiler.md"),
  ),
  (
    "ryo-renderer",
    "Working on the wgpu renderer, shaders, and GPU batching.",
    include_str!("../skills/ryo-renderer.md"),
  ),
  (
    "ryo-ui",
    "Build game UI in the Scene::ui hook with immediate-mode and retained-mode widgets including buttons, sliders, checkboxes, text inputs, dropdowns, scroll views, and layout containers, and consume the input the UI uses. Use when creating HUDs, menus, settings screens, or any in-game interface.",
    include_str!("../skills/ryo-ui.md"),
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
    for (name, desc, _) in EMBEDDED_SKILLS {
      println!("  • {} - {}", name, desc);
    }
    println!();
  }

  // Create skills directory
  fs::create_dir_all(&skills_dir)
    .map_err(|e| CliError::Other(format!("Failed to create skills directory: {}", e)))?;

  // Install all embedded skills
  for (name, _, content) in EMBEDDED_SKILLS {
    if !quiet {
      Output::status("Installing", name);
    }
    install_skill(&skills_dir, name, content)?;
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

/// Install a single skill from embedded content
fn install_skill(target_dir: &Path, skill_name: &str, content: &str) -> CliResult<()> {
  let target = target_dir.join(skill_name);

  // Create target skill directory
  fs::create_dir_all(&target)
    .map_err(|e| CliError::Other(format!("Failed to create skill directory: {}", e)))?;

  // Write SKILL.md from embedded content
  let target_skill_md = target.join("SKILL.md");
  fs::write(&target_skill_md, content).map_err(|e| {
    CliError::Other(format!(
      "Failed to write SKILL.md for {}: {}",
      skill_name, e
    ))
  })?;

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
