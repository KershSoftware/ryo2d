//! Init command - scaffolds a new Ryo2D project

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use super::{CliError, CliResult, Output};

/// Execute the init command
pub fn execute(args: &[String]) -> CliResult<()> {
  // Handle --help
  if args.iter().any(|a| a == "--help" || a == "-h") {
    print_help();
    return Ok(());
  }

  // Get project name from args or prompt
  let project_name = if args.is_empty() {
    prompt_project_name()?
  } else {
    args[0].clone()
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

  // Prompt to install skills
  if let Some(platform) = super::install_skills::prompt_install_skills()? {
    println!();
    super::install_skills::install_skills_to_dir(project_dir, platform, false)?;
  }

  println!();
  println!("Next steps:");
  println!("  cd {}", project_name);
  println!("  cargo run         # Run in development mode");
  println!("  ryo2d dev         # Run with hot reload (coming soon)");
  println!("  ryo2d build       # Build for release");
  println!();

  Ok(())
}

/// Prompt user for project name
fn prompt_project_name() -> CliResult<String> {
  print!("Project name: ");
  io::stdout().flush().map_err(CliError::Io)?;

  let mut input = String::new();
  io::stdin().read_line(&mut input).map_err(CliError::Io)?;

  let name = input.trim().to_string();

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

/// Create the project directory structure and files
fn create_project_structure(project_dir: &Path, project_name: &str) -> CliResult<()> {
  // Create directories
  fs::create_dir_all(project_dir)
    .map_err(|e| CliError::Other(format!("Failed to create project directory: {}", e)))?;

  fs::create_dir_all(project_dir.join("assets"))
    .map_err(|e| CliError::Other(format!("Failed to create assets directory: {}", e)))?;

  fs::create_dir_all(project_dir.join("data"))
    .map_err(|e| CliError::Other(format!("Failed to create data directory: {}", e)))?;

  fs::create_dir_all(project_dir.join("src"))
    .map_err(|e| CliError::Other(format!("Failed to create src directory: {}", e)))?;

  // Create Cargo.toml
  let cargo_toml = generate_cargo_toml(project_name);
  fs::write(project_dir.join("Cargo.toml"), cargo_toml)
    .map_err(|e| CliError::Other(format!("Failed to write Cargo.toml: {}", e)))?;

  // Create Ryo.toml
  let ryo_toml = generate_ryo_toml(project_name);
  fs::write(project_dir.join("Ryo.toml"), ryo_toml)
    .map_err(|e| CliError::Other(format!("Failed to write Ryo.toml: {}", e)))?;

  // Create main.rs
  let main_rs = generate_main_rs();
  fs::write(project_dir.join("main.rs"), main_rs)
    .map_err(|e| CliError::Other(format!("Failed to write main.rs: {}", e)))?;

  // Create build.rs (for Windows icon embedding)
  let build_rs = generate_build_rs();
  fs::write(project_dir.join("build.rs"), build_rs)
    .map_err(|e| CliError::Other(format!("Failed to write build.rs: {}", e)))?;

  // Create README.md
  let readme = generate_readme(project_name);
  fs::write(project_dir.join("README.md"), readme)
    .map_err(|e| CliError::Other(format!("Failed to write README.md: {}", e)))?;

  // Create .gitignore
  let gitignore = generate_gitignore();
  fs::write(project_dir.join(".gitignore"), gitignore)
    .map_err(|e| CliError::Other(format!("Failed to write .gitignore: {}", e)))?;

  Ok(())
}

/// Generate Cargo.toml content
fn generate_cargo_toml(project_name: &str) -> String {
  // Detect installed engine location
  let ryo2d_dep = if let Some(engine_path) = get_installed_engine_path() {
    // Convert backslashes to forward slashes for TOML compatibility
    let path_str = engine_path.display().to_string().replace('\\', "/");
    format!(r#"ryo2d = {{ path = "{}" }}"#, path_str)
  } else {
    // Fallback to git dependency if engine not installed
    r#"# ryo2d = { git = "https://github.com/your-username/ryo2d" }
ryo2d = { path = "../path/to/ryo2d" }  # Update this path"#
      .to_string()
  };

  format!(
    r#"[package]
name = "{}"
version = "0.1.0"
edition = "2024"
description = "A game built with Ryo2D engine"
build = "build.rs"

[[bin]]
name = "{}"
path = "main.rs"

[dependencies]
# Ryo2D engine dependency
{}

# Add your game dependencies here
# serde = {{ version = "1.0", features = ["derive"] }}
# rand = "0.8"

[build-dependencies]
# Windows icon embedding (used by build.rs when building via `ryo2d build`)
winresource = "0.1"

# Optimize dependencies and engine in dev builds for playable performance
# Your game code stays unoptimized for fast compile times
[profile.dev.package."*"]
opt-level = 2

[profile.dev.package.ryo2d]
opt-level = 2
"#,
    project_name, project_name, ryo2d_dep
  )
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

/// Generate Ryo.toml content
fn generate_ryo_toml(project_name: &str) -> String {
  let display_name = project_name
    .split('-')
    .map(|word| {
      let mut c = word.chars();
      match c.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + c.as_str(),
      }
    })
    .collect::<Vec<_>>()
    .join(" ");

  format!(
    r#"# {} - Ryo2D Project Configuration

[project]
name = "{}"
title = "{}"
version = "0.0.1"

[paths]
entry_point = "main.rs"
save_data = "data"
assets = "assets"

# =============================================================================
# USER-FACING DEFAULTS
# Written and prioritized read from system.json when `.with_user_preferences` set
# =============================================================================

[system]
resolution = "1280x720"
vsync = true
display_mode = "windowed"
master_volume = 0.8
music_volume = 1.0
sfx_volume = 1.0
show_fps = false
console = false

# =============================================================================
# DEVELOPER-ONLY SETTINGS
# These are NOT written to system.json - they're for development
# =============================================================================

[debug]
show_colliders = false
show_velocities = false
show_origins = false
show_frame_timing = false
disable_debug_keys = false
enable_profiler = false
logger_window = false
dev_window_keys = true

# =============================================================================
# RELEASE/DISTRIBUTION SETTINGS
# Build-time settings for `ryo2d build`
# =============================================================================

[release]
# logo = "assets/icon.png"

# =============================================================================
# GAME SPECIFIC SETTINGS
# Custom game configurations (not tied to engine)
# =============================================================================

[env]
# my_custom_variable = false
"#,
    display_name, display_name, display_name
  )
}

/// Generate main.rs content
fn generate_main_rs() -> &'static str {
  r#"// Windows: Hide the console window in release builds.
// In debug builds, the console remains visible for println! output.
// Remove this line if you need console output in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! My Ryo2D Game
//!
//! A game built with the Ryo2D engine.

use ryo2d::prelude::*;

/// Main game scene
struct GameScene;

impl Scene for GameScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    // Initialize your game here
    // Spawn entities, load assets, etc.

    // Example: Spawn a simple colored square at center
    let sprite = ctx.assets.create_sprite(
      Sprite::solid_rect(64, 64, 0, 255, 255, 255),
      "square"
    );
    ctx.world.spawn((
      Transform::new(640.0, 360.0),
      Renderable::sprite(sprite).with_z_order(0),
    ));
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    // Update game logic here

    // Example: Quit on Escape
    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Quit;
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction {
    // Draw UI here
    ui.label(
      "My Ryo2D Game",
      10.0,
      10.0,
      Color::WHITE,
      Some(Color::new(0, 0, 0, 180)),
    );
    SceneAction::Continue
  }
}

fn main() {
  let engine = EngineBuilder::new()
    .resolution(1280, 720)
    .with_scene(GameScene)
    .build();

  if let Err(e) = engine.run() {
    eprintln!("Engine error: {}", e);
    std::process::exit(1);
  }
}
"#
}

/// Generate README.md content
fn generate_readme(project_name: &str) -> String {
  let display_name = project_name
    .split('-')
    .map(|word| {
      let mut c = word.chars();
      match c.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + c.as_str(),
      }
    })
    .collect::<Vec<_>>()
    .join(" ");

  format!(
    r#"# {}

A game built with the [Ryo2D](https://github.com/your-username/ryo2d) engine.

## Building and Running

### Development

```bash
# Run in debug mode (faster compilation, slower runtime)
cargo run

# Or use the Ryo2D CLI
ryo2d dev
```

### Release

```bash
# Build optimized release
ryo2d build

# Run the release build
ryo2d run

# Or distribute the ./release/ folder
```

## Project Structure

```
{}/
├── main.rs          # Game entry point
├── Cargo.toml       # Rust package configuration
├── Ryo.toml         # Ryo2D project configuration
├── assets/          # Game assets (images, audio, etc.)
├── data/            # Save data (auto-generated)
└── src/             # Additional source modules (optional)
```

## Controls

Add your game controls here.

## License

Add your license here.
"#,
    display_name, project_name
  )
}

/// Generate .gitignore content
fn generate_gitignore() -> &'static str {
  r#"/target/
/data/
/release/
Cargo.lock
*.swp
*.swo
*~
.DS_Store
"#
}

/// Generate build.rs for Windows icon embedding
fn generate_build_rs() -> &'static str {
  r#"//! Build script for embedding resources (e.g., Windows icon).
//!
//! When building via `ryo2d build`, the RYO_EMBED_ICON environment variable
//! is set to the path of the application icon. This script uses winresource
//! to embed it into the Windows executable.

fn main() {
    // Only embed icon on Windows
    #[cfg(target_os = "windows")]
    {
        // Check if building via ryo2d CLI (which sets RYO_EMBED_ICON)
        if let Ok(icon_path) = std::env::var("RYO_EMBED_ICON") {
            println!("cargo:rerun-if-env-changed=RYO_EMBED_ICON");

            // Check if the icon file exists and is a PNG
            let path = std::path::Path::new(&icon_path);
            if path.exists() {
                // winresource can handle PNG files directly
                let mut res = winresource::WindowsResource::new();
                res.set_icon(&icon_path);

                if let Err(e) = res.compile() {
                    eprintln!("Warning: Failed to embed icon: {}", e);
                }
            }
        }
    }
}
"#
}

fn print_help() {
  println!(
    r#"
ryo2d-init - Create a new Ryo2D project

USAGE:
    ryo2d init [PROJECT_NAME]

ARGUMENTS:
    PROJECT_NAME    Name of the project (lowercase, hyphens, underscores)
                    If not provided, you will be prompted

DESCRIPTION:
    Creates a new Ryo2D game project with the following structure:

    project-name/
    ├── main.rs          # Game entry point with basic scene
    ├── build.rs         # Build script (Windows icon embedding)
    ├── Cargo.toml       # Rust package configuration
    ├── Ryo.toml         # Ryo2D project settings
    ├── README.md        # Project documentation
    ├── .gitignore       # Git ignore patterns
    ├── assets/          # Game assets (images, audio)
    ├── data/            # Save data (auto-generated at runtime)
    └── src/             # Additional modules (create as needed)

EXAMPLES:
    ryo2d init my-game
    ryo2d init
"#
  );
}
