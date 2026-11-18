//! The command line, read by clap: one parser for every command.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use ryo2d::ecs::UVec2;
use ryo2d::testing::SheetOptions;

/// Ryo2D: create, run, build and package games
#[derive(Debug, Parser)]
#[command(name = "ryo2d", version = ryo2d::tooling::VERSION, arg_required_else_help = true)]
pub struct Cli {
  #[command(subcommand)]
  pub command: Command,
}

/// The commands
#[derive(Debug, Subcommand)]
pub enum Command {
  /// Create a new Ryo2D project
  ///
  /// Creates a new Ryo2D game project in a folder of that name. The name is
  /// lowercase letters, digits, hyphens and underscores. When it is not
  /// given, it is asked for.
  #[command(after_help = INIT_EXAMPLES)]
  Init(InitArgs),
  /// Run the game in development mode
  ///
  /// Runs your game as a debug build, for faster compilation, with the
  /// developer tools and with asset hot reload: an edited asset shows in the
  /// running game. Code is not reloaded: rebuild by running the command
  /// again. This is the recommended command during development.
  ///
  /// It builds the game's binary (the package's `default-run`, or its only
  /// one) and runs it from the project folder. The game's own non-zero exit
  /// code is the command's.
  ///
  /// To profile, use --profiler, which builds the engine's profiler feature
  /// and opens the profiler window. Setting `enable_profiler = true` in
  /// Ryo.toml's [debug] section builds the profiler too but does not open
  /// it: press F10 in the game.
  #[command(after_help = DEV_EXAMPLES)]
  Dev(DevArgs),
  /// Build a production release into release/
  ///
  /// Builds a production-optimised release of your game and a distributable
  /// bundle in the release/ folder. The build has no developer tools: no
  /// console, dev windows or debug keys (F5 and the FPS counter setting
  /// stay). --dev-tools keeps them.
  ///
  /// Ryo.toml is not in the bundle: its values (paths, environment
  /// variables) are compiled into the executable at build time. Any other
  /// argument is an error.
  #[command(after_help = BUILD_BUNDLE)]
  Build(BuildArgs),
  /// Run the production build
  ///
  /// Runs the game from release/, the bundle 'ryo2d build' made, with
  /// release/ as its working directory. Without a bundle it runs the release
  /// binary in cargo's target folder (where cargo says it is), from the
  /// project folder, and warns that this is not a bundle. With no build at all
  /// it says to run 'ryo2d build'. The game's own non-zero exit code is the
  /// command's: it says "the game exited with code N" and exits with N (128
  /// plus the signal when a signal ended the game on Unix).
  Run,
  /// Package the release into a distributable zip
  ///
  /// Creates a zip from the release/ folder, named
  /// {project-name}-v{version}-{YYYYMMDD}.zip, where the date is today's UTC
  /// date. The game's executable keeps its run permission (0o755) in the
  /// zip, so it runs after unzipping on Mac and Linux. The version is Ryo.toml's
  /// [project].version, or else Cargo.toml's [package].version; when neither
  /// has one, the -v{version} part is left out. If a zip of that name
  /// exists, a suffix is added: -001, -002 and so on.
  ///
  /// The zip always carries licenses/, the licences of the fonts the engine
  /// builds in (written into release/ first if missing).
  ///
  /// Run 'ryo2d build' first to create the release bundle.
  #[command(after_help = EXPORT_EXAMPLES)]
  Export,
  /// Install Agent Skills for AI assistants
  ///
  /// Writes every Ryo2D skill into the project, for the assistant you
  /// choose: Claude Code (.claude/skills/), GitHub Copilot (.github/skills/)
  /// or both. All the skills are installed, whichever you choose, and
  /// .gitignore is changed to track them.
  ///
  /// It asks which assistant when run in a terminal. With no terminal it
  /// asks nothing and installs nothing unless --skills says which. 'ryo2d
  /// init --skills' does the same for a project it creates.
  #[command(after_help = INSTALL_SKILLS_EXAMPLES)]
  InstallSkills(InstallSkillsArgs),
  /// Check UI files (*.ui.ron) without running the game
  ///
  /// Checks every *.ui.ron file under the project's assets folder, or the
  /// files given, loading each as the game does. Imports and images are read
  /// from the assets folder, and an image that does not load is reported at
  /// its value. Each problem is printed as path:line:column: message, a
  /// file's images after its other problems, and the command exits with an
  /// error when any file has a problem.
  ///
  /// Paths in UI files, and the paths printed, are relative to the assets
  /// folder (ui/menu.ui.ron for assets/ui/menu.ui.ron). FILES must be under
  /// the assets folder: any other is reported as not an asset path.
  #[command(after_help = CHECK_UI_EXAMPLES)]
  CheckUi(CheckUiArgs),
  /// Draw a skeleton's animations as a contact sheet (PNG)
  ///
  /// Draws each animation at evenly spaced moments, k x length / N for k
  /// from 0, each cell labelled with the animation's name and the time. A
  /// moment is reached by stepping the animation from its start, at the
  /// engine's default 60 steps a second unless --hz says otherwise, after
  /// one loop round (none for an animation given to --once, which also gets
  /// a cell at its end), so springs and constraints show as in play. Every
  /// cell has one scale and the skeleton's origin at one place, on a faint
  /// ground line, so root motion reads. A skeleton with no animations shows
  /// its setup pose.
  ///
  /// The file must be in a project's assets folder: its images are paths
  /// from there. A sheet written into the assets folder is written with a
  /// warning, since a release would ship it. The sheet draws with the GPU.
  #[command(after_help = SHEET_EXAMPLES)]
  Sheet(SheetArgs),
  /// Tools for testing multiplayer
  Netplay {
    #[command(subcommand)]
    command: NetplayCommand,
  },
}

const INIT_EXAMPLES: &str = "\
The project it creates:
    project-name/
    ├── main.rs          Game entry point with a basic scene
    ├── build.rs         Build script (Windows icon embedding)
    ├── Cargo.toml       Rust package configuration
    ├── Ryo.toml         Ryo2D project settings
    ├── README.md        Project documentation
    ├── .gitignore       Git ignore patterns
    ├── assets/          Game assets (images, audio)
    ├── data/            Save data (made at run time)
    └── src/             Additional modules (create as needed)

Examples:
    ryo2d init my-game
    ryo2d init";

const DEV_EXAMPLES: &str = "\
Examples:
    ryo2d dev              Normal development mode
    ryo2d dev --profiler   With the profiler window";

const BUILD_BUNDLE: &str = "\
The bundle (release/) holds:
    - The executable (with its configuration compiled in)
    - assets/
    - levels/ (if present)
    - data/ (empty, for save files; made only when Ryo.toml has
      [release] portable = true, otherwise saves go to the player's
      own folders)
    - icon.png (the game's icon: [release] logo, else the engine's)
    - README.md (if present)
    - licenses/ (the licences of the fonts the engine builds in)

Example:
    ryo2d build
    Then distribute the whole release/ folder.";

const INSTALL_SKILLS_EXAMPLES: &str = "\
Examples:
    ryo2d install-skills                  Asks which assistant
    ryo2d install-skills --skills both    Claude and Copilot, no questions";

const EXPORT_EXAMPLES: &str = "\
Example:
    ryo2d build    Create the release bundle
    ryo2d export   Package it into a zip

    Writes MyGame-v1.0.0-20260203.zip (MyGame-20260203.zip when no
    version is found).";

const CHECK_UI_EXAMPLES: &str = "\
Examples:
    ryo2d check-ui
    ryo2d check-ui assets/ui/menu.ui.ron";

const SHEET_EXAMPLES: &str = "\
Examples:
    ryo2d sheet assets/hero.skeleton.ron
    ryo2d sheet assets/hero.skeleton.ron --animations walk,run --moments 6 --bones";

/// `ryo2d init`
#[derive(Debug, Args)]
pub struct InitArgs {
  /// The project's name (asked for when not given)
  pub name: Option<String>,
  /// Install the Agent Skills for this assistant, without asking (asked in a
  /// terminal when not given; with no terminal none are installed)
  #[arg(long, value_enum)]
  pub skills: Option<SkillsChoice>,
}

/// `ryo2d install-skills`
#[derive(Debug, Args)]
pub struct InstallSkillsArgs {
  /// The assistant to install for, without asking (required when there is
  /// no terminal)
  #[arg(long, value_enum)]
  pub skills: Option<SkillsChoice>,
}

/// Which assistants' skills folders to write
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SkillsChoice {
  /// Claude Code: .claude/skills/
  Claude,
  /// GitHub Copilot: .github/skills/
  Copilot,
  /// Both folders
  Both,
  /// No skills
  None,
}

/// `ryo2d dev`
#[derive(Debug, Args)]
pub struct DevArgs {
  /// Open the profiler window (builds the engine's profiler feature)
  #[arg(short = 'p', long)]
  pub profiler: bool,
  /// Skip the dev profile optimisation check
  #[arg(long)]
  pub skip_opt_check: bool,
}

/// `ryo2d build`
#[derive(Debug, Clone, Copy, Args)]
pub struct BuildArgs {
  /// Keep the developer tools (the console, the dev windows, the debug keys
  /// and overlays) in the build, for testing a release build. Not for players
  #[arg(long)]
  pub dev_tools: bool,
}

/// `ryo2d check-ui`
#[derive(Debug, Args)]
pub struct CheckUiArgs {
  /// The files to check (every *.ui.ron under the assets folder when none)
  pub files: Vec<PathBuf>,
}

/// `ryo2d sheet`
#[derive(Debug, Clone, PartialEq, Args)]
pub struct SheetArgs {
  /// The skeleton file, in the project's assets folder
  pub file: PathBuf,
  /// The animations, a row each (default: every one)
  #[arg(long, value_delimiter = ',')]
  pub animations: Option<Vec<String>>,
  /// The moments of each, a column each
  #[arg(long, default_value_t = SheetOptions::default().moments)]
  pub moments: u32,
  /// The skins to put on, in order
  #[arg(long, value_delimiter = ',')]
  pub skins: Vec<String>,
  /// Animations played once, not looped: no loop round first, and a last
  /// cell at their length
  #[arg(long, value_delimiter = ',')]
  pub once: Vec<String>,
  /// Draw the bones overlay over the art
  #[arg(long)]
  pub bones: bool,
  /// Each cell's size in pixels, label included (default: sized to fit the
  /// poses)
  #[arg(long, value_name = "WxH", value_parser = parse_cell)]
  pub cell: Option<UVec2>,
  /// The fixed rate to step at, as the game's EngineBuilder::fixed_hz
  /// (default: 60)
  #[arg(long, value_name = "N")]
  pub hz: Option<u32>,
  /// Where to write the PNG, its folder made if missing (default:
  /// NAME.sheet.png in the current folder)
  #[arg(long, value_name = "PATH")]
  pub out: Option<PathBuf>,
}

impl SheetArgs {
  /// What to draw: the flags as the contact sheet's options
  pub fn options(&self) -> SheetOptions {
    SheetOptions {
      animations: self.animations.as_deref().map(names),
      moments: self.moments,
      skins: names(&self.skins),
      once: names(&self.once),
      bones: self.bones,
      cell: self.cell,
      fixed_hz: self.hz,
    }
  }
}

/// A comma list's names, trimmed, empty ones dropped
fn names(list: &[String]) -> Vec<String> {
  list
    .iter()
    .map(|name| name.trim())
    .filter(|name| !name.is_empty())
    .map(str::to_owned)
    .collect()
}

/// `WxH` in pixels
fn parse_cell(value: &str) -> Result<UVec2, String> {
  let bad = || format!("expected WIDTHxHEIGHT in pixels (200x240), not {value:?}");
  let (width, height) = value.split_once(['x', 'X']).ok_or_else(bad)?;
  let side = |side: &str| side.trim().parse::<u32>().map_err(|_| bad());
  Ok(UVec2::new(side(width)?, side(height)?))
}

/// `ryo2d netplay`
#[derive(Debug, Subcommand)]
pub enum NetplayCommand {
  /// Start a temporary signaling server for testing with friends
  ///
  /// Creates a temporary tunnel for testing multiplayer with friends over
  /// the internet. The tunnel URL changes each time and closes when you
  /// press Ctrl+C.
  ///
  /// NOT FOR PRODUCTION: for shipped games, deploy your own signalling
  /// server (see docs/architecture/matchbox-server.md).
  #[command(after_help = NETPLAY_REQUIREMENTS)]
  Start,
  /// Show your LAN and WAN IP addresses
  Ip,
}

const NETPLAY_REQUIREMENTS: &str = "\
Requirements:
    - matchbox_server (installed through cargo when missing)
    - cloudflared: https://developers.cloudflare.com/cloudflare-one/connections/connect-apps/install-and-setup/installation/

      Windows:   winget install Cloudflare.cloudflared
      macOS:     brew install cloudflared
      Linux:     See Cloudflare docs for your distro";

#[cfg(test)]
mod tests {
  use super::*;

  fn parse(words: &[&str]) -> Result<Cli, clap::Error> {
    Cli::try_parse_from(std::iter::once("ryo2d").chain(words.iter().copied()))
  }

  #[test]
  fn build_takes_dev_tools_and_nothing_else() {
    let Ok(Cli {
      command: Command::Build(plain),
    }) = parse(&["build"])
    else {
      panic!("build parses");
    };
    assert!(!plain.dev_tools);
    let Ok(Cli {
      command: Command::Build(tools),
    }) = parse(&["build", "--dev-tools"])
    else {
      panic!("build --dev-tools parses");
    };
    assert!(tools.dev_tools);
    // A misspelt flag must not make a plain build
    let said = parse(&["build", "--dev-tool"]).unwrap_err().to_string();
    assert!(said.contains("--dev-tool"), "{said}");
    assert!(said.contains("--dev-tools"), "{said}");
    assert!(parse(&["build", "release"]).is_err());
  }

  #[test]
  fn help_keeps_what_the_hand_written_help_said() {
    let help = |words: &[&str]| {
      let mut all = words.to_vec();
      all.push("--help");
      parse(&all).unwrap_err().to_string()
    };
    assert!(help(&["netplay", "start"]).contains("NOT FOR PRODUCTION"));
    assert!(help(&["netplay", "start"]).contains("brew install cloudflared"));
    assert!(help(&["export"]).contains("{project-name}-v{version}-{YYYYMMDD}.zip"));
    assert!(help(&["build"]).contains("Ryo.toml is not in the bundle"));
    assert!(help(&["sheet"]).contains("one loop round"));
    assert!(help(&["check-ui"]).contains("path:line:column"));
    assert!(help(&["dev"]).contains("F10"));
    assert!(help(&["init"]).contains("assets/"));
    assert!(help(&["init"]).contains("--skills"));
    let skills = help(&["install-skills"]);
    assert!(skills.contains(".claude/skills/") && skills.contains(".github/skills/"));
    assert!(skills.contains("--skills"));
    assert!(help(&["run"]).contains("release/"));
    let build = help(&["build"]);
    assert!(build.contains("portable = true") && build.contains("icon.png"));
    assert!(!help(&["dev"]).contains("coming soon"));
  }

  #[test]
  fn dev_takes_its_flags_in_either_form() {
    let Ok(Cli {
      command: Command::Dev(dev),
    }) = parse(&["dev", "-p", "--skip-opt-check"])
    else {
      panic!("dev parses");
    };
    assert!(dev.profiler && dev.skip_opt_check);
    let Ok(Cli {
      command: Command::Dev(dev),
    }) = parse(&["dev", "--profiler"])
    else {
      panic!("dev parses");
    };
    assert!(dev.profiler && !dev.skip_opt_check);
  }

  fn sheet(words: &[&str]) -> SheetArgs {
    let mut all = vec!["sheet"];
    all.extend_from_slice(words);
    match parse(&all) {
      Ok(Cli {
        command: Command::Sheet(sheet),
      }) => sheet,
      other => panic!("sheet {words:?} does not parse: {other:?}"),
    }
  }

  #[test]
  fn the_arguments_become_the_sheet_s_options() {
    let parsed = sheet(&[
      "hero.skeleton.ron",
      "--animations",
      "walk, run",
      "--moments",
      "6",
      "--skins",
      "dark,cape",
      "--bones",
      "--once",
      "jump",
      "--cell",
      "200x240",
      "--hz",
      "30",
      "--out",
      "sheets/hero.png",
    ]);
    assert_eq!(parsed.file, PathBuf::from("hero.skeleton.ron"));
    assert_eq!(parsed.out, Some(PathBuf::from("sheets/hero.png")));
    assert_eq!(
      parsed.options(),
      SheetOptions {
        animations: Some(vec!["walk".to_owned(), "run".to_owned()]),
        moments: 6,
        skins: vec!["dark".to_owned(), "cape".to_owned()],
        once: vec!["jump".to_owned()],
        bones: true,
        cell: Some(UVec2::new(200, 240)),
        fixed_hz: Some(30),
      }
    );
    // The file alone takes every default
    let plain = sheet(&["hero.skeleton.ron"]);
    assert_eq!(plain.options(), SheetOptions::default());
    assert_eq!(plain.out, None);
  }

  #[test]
  fn arguments_it_cannot_read_say_why() {
    let error = |words: &[&str]| {
      let mut all = vec!["sheet"];
      all.extend_from_slice(words);
      parse(&all).unwrap_err().to_string()
    };
    assert!(error(&[]).contains("<FILE>"));
    assert!(error(&["a.skeleton.ron", "b.skeleton.ron"]).contains("b.skeleton.ron"));
    assert!(error(&["a.skeleton.ron", "--bone"]).contains("--bone"));
    assert!(error(&["a.skeleton.ron", "--moments"]).contains("a value is required"));
    assert!(error(&["a.skeleton.ron", "--moments", "six"]).contains("six"));
    assert!(error(&["a.skeleton.ron", "--cell", "200"]).contains("WIDTHxHEIGHT"));
    assert!(error(&["a.skeleton.ron", "--cell", "200xtall"]).contains("WIDTHxHEIGHT"));
    assert!(error(&["a.skeleton.ron", "--hz", "fast"]).contains("fast"));
  }
}
