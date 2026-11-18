//! The command line, read by clap: one parser for every command.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use ryo2d::ecs::UVec2;
use ryo2d::testing::SheetOptions;

/// Ryo2D: create, run, build and package games
#[derive(Debug, Parser)]
#[command(name = "ryo2d", version, arg_required_else_help = true)]
pub struct Cli {
  #[command(subcommand)]
  pub command: Command,
}

/// The commands
#[derive(Debug, Subcommand)]
pub enum Command {
  /// Create a new Ryo2D project
  Init(InitArgs),
  /// Run the game in development mode
  Dev(DevArgs),
  /// Build a production release into release/
  Build(BuildArgs),
  /// Run the production build
  Run,
  /// Package the release into a distributable zip
  Export,
  /// Install Agent Skills for AI assistants
  InstallSkills,
  /// Check UI files (*.ui.ron) without running the game
  CheckUi(CheckUiArgs),
  /// Draw a skeleton's animations as a contact sheet (PNG)
  Sheet(SheetArgs),
  /// Tools for testing multiplayer
  Netplay {
    #[command(subcommand)]
    command: NetplayCommand,
  },
}

/// `ryo2d init`
#[derive(Debug, Args)]
pub struct InitArgs {
  /// The project's name (asked for when not given)
  pub name: Option<String>,
}

/// `ryo2d dev`
#[derive(Debug, Args)]
pub struct DevArgs {
  /// Open the profiler window (builds the engine's profiler feature)
  #[arg(short = 'p', long)]
  pub profiler: bool,
  /// Skip the dev profile optimization check
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
  #[arg(long)]
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
  Start,
  /// Show your LAN and WAN IP addresses
  Ip,
}

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
