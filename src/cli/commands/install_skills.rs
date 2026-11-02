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

/// Every `.claude/skills/<name>/SKILL.md` as (name, content), listed by the
/// build script (`src/build.rs`): the skills folder is the one copy
const EMBEDDED_SKILLS: &[(&str, &str)] = include!(concat!(env!("OUT_DIR"), "/embedded_skills.rs"));

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
    for (name, content) in EMBEDDED_SKILLS {
      println!("  • {} - {}", name, skill_description(content));
    }
    println!();
  }

  // Create skills directory
  fs::create_dir_all(&skills_dir)
    .map_err(|e| CliError::Other(format!("Failed to create skills directory: {}", e)))?;

  // Install all embedded skills
  for (name, content) in EMBEDDED_SKILLS {
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

/// A skill's `description:`, from its frontmatter
fn skill_description(skill: &str) -> &str {
  let mut lines = skill.lines();
  if lines.next() != Some("---") {
    return "";
  }
  lines
    .take_while(|line| *line != "---")
    .find_map(|line| line.strip_prefix("description: "))
    .map_or("", |description| description.trim_matches('"'))
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

#[cfg(test)]
mod tests {
  use std::path::Path;

  use super::{EMBEDDED_SKILLS, SkillsPlatform, install_skills_to_dir, skill_description};

  #[test]
  fn a_skills_description_is_its_frontmatters() {
    let skill = "---\nname: ryo-x\ndescription: Does things - and more.\nmetadata:\n  version: 1.0.0\n---\n\n# X\n\ndescription: not this\n";
    assert_eq!(skill_description(skill), "Does things - and more.");
    let quoted = "---\nname: ryo-y\ndescription: \"Quoted: yes\"\n---\n";
    assert_eq!(skill_description(quoted), "Quoted: yes");
    let body_only = "# No frontmatter\n\ndescription: body text\n";
    assert_eq!(
      skill_description(body_only),
      "",
      "only the frontmatter counts"
    );
  }

  #[test]
  fn every_embedded_skill_has_a_description() {
    for (name, content) in EMBEDDED_SKILLS {
      assert!(!skill_description(content).is_empty(), "{name}");
    }
  }

  /// Installing writes every `.claude/skills/<name>/SKILL.md` byte for byte,
  /// and nothing else
  #[test]
  fn installing_writes_every_skill_folder_as_it_is() {
    let project = tempfile::tempdir().unwrap();
    install_skills_to_dir(project.path(), SkillsPlatform::Claude, true).unwrap();
    let installed = project.path().join(".claude/skills");
    let sources = Path::new(env!("CARGO_MANIFEST_DIR")).join(".claude/skills");
    let mut skills = 0;
    for entry in std::fs::read_dir(&sources).unwrap() {
      let entry = entry.unwrap();
      if !entry.file_type().unwrap().is_dir() {
        continue;
      }
      let name = entry.file_name();
      let source = std::fs::read_to_string(entry.path().join("SKILL.md")).unwrap();
      let copy = std::fs::read_to_string(installed.join(&name).join("SKILL.md"))
        .unwrap_or_else(|_| panic!("{name:?} was not installed"));
      assert!(copy == source, "{name:?} was installed as it is");
      skills += 1;
    }
    assert!(skills > 20, "the engine's skills were found: {skills}");
    assert_eq!(std::fs::read_dir(&installed).unwrap().count(), skills);
  }
}
