//! Sheet command - draws a skeleton's animations as a contact sheet

use std::path::{Path, PathBuf};

use image::RgbaImage;
use ryo2d::ecs::UVec2;

use super::{CliError, CliResult, Output};
use ryo2d::core::config::Workspace;
use ryo2d::testing::{SheetError, SheetOptions, SheetSkeleton, skeleton_sheet};

/// The flags the command takes, for the message about one it does not
const FLAGS: &str = "--animations, --moments, --skins, --once, --bones, --cell, --hz, --out";

/// What the command was asked for
#[derive(Debug, Clone, PartialEq)]
struct SheetArgs {
  file: PathBuf,
  options: SheetOptions,
  out: Option<PathBuf>,
}

/// Execute the sheet command
pub fn execute(args: &[String]) -> CliResult<()> {
  if args.iter().any(|a| a == "--help" || a == "-h") {
    print_help();
    return Ok(());
  }
  let cwd = std::env::current_dir().map_err(CliError::Io)?;
  let out = run(args, &cwd, skeleton_sheet)?;
  Output::success(&format!("wrote {}", out.display()));
  Ok(())
}

/// Reads the arguments, finds the file's project and draws its sheet with
/// `draw`, writing the PNG; relative paths are from `cwd`. The path
/// written, or why not
fn run(
  args: &[String],
  cwd: &Path,
  draw: impl FnOnce(SheetSkeleton, &SheetOptions) -> Result<RgbaImage, SheetError>,
) -> CliResult<PathBuf> {
  let parsed = parse(args).map_err(CliError::Other)?;
  let file = cwd.join(&parsed.file);
  let file = file
    .canonicalize()
    .map_err(|error| CliError::Other(format!("cannot read {}: {error}", file.display())))?;
  let (assets, path) = asset_path(&file)?;
  let out = parsed
    .out
    .map_or_else(|| cwd.join(default_out(&file)), |out| cwd.join(out));
  // The folder is made, or found unusable, before anything is drawn
  if let Some(folder) = out.parent().filter(|folder| !folder.as_os_str().is_empty()) {
    std::fs::create_dir_all(folder).map_err(|error| {
      CliError::Other(format!(
        "cannot make the folder {} for the sheet: {error}",
        folder.display()
      ))
    })?;
  }
  if let Some(warning) = in_assets_warning(&out, &assets) {
    Output::warning(&warning);
  }
  Output::status("Drawing", &format!("{path} from {}", assets.display()));
  let sheet = draw(SheetSkeleton::file(&assets, &path), &parsed.options).map_err(|error| {
    CliError::Other(match error {
      SheetError::NoGpu => {
        "no GPU found: `ryo2d sheet` draws with the GPU, and this machine has none it can use"
          .to_owned()
      }
      other => other.to_string(),
    })
  })?;
  sheet
    .save(&out)
    .map_err(|error| CliError::Other(format!("cannot write {}: {error}", out.display())))?;
  Ok(out)
}

/// The asset folder of the project `file` is in, and the file's asset path
/// in it (`/` between folders); an error for a file outside it
fn asset_path(file: &Path) -> CliResult<(PathBuf, String)> {
  let folder = file.parent().unwrap_or(file);
  let workspace = Workspace::discover_from(folder)
    .map_err(|error| CliError::WorkspaceNotFound(error.to_string()))?;
  let assets = workspace.assets_dir();
  let outside = || {
    CliError::Other(format!(
      "{} is not in the project's assets folder ({}): a skeleton's images are paths from there",
      file.display(),
      assets.display()
    ))
  };
  let assets = assets.canonicalize().map_err(|_| outside())?;
  let inside = file.strip_prefix(&assets).map_err(|_| outside())?;
  let path: Vec<String> = inside
    .components()
    .map(|part| part.as_os_str().to_string_lossy().into_owned())
    .collect();
  Ok((assets, path.join("/")))
}

/// A warning for a sheet written into the assets folder `assets` (canonical),
/// where a release would ship it; `out`'s folder exists
fn in_assets_warning(out: &Path, assets: &Path) -> Option<String> {
  let folder = out.parent()?.canonicalize().ok()?;
  folder.starts_with(assets).then(|| {
    format!(
      "{} is in the project's assets folder, so a release would ship it: write sheets elsewhere",
      out.display()
    )
  })
}

/// `hero.skeleton.ron` writes `hero.sheet.png`, in the current folder
fn default_out(file: &Path) -> PathBuf {
  let name = file
    .file_name()
    .map(|name| name.to_string_lossy().into_owned())
    .unwrap_or_default();
  let stem = name
    .strip_suffix(".skeleton.ron")
    .or_else(|| name.strip_suffix(".ron"))
    .unwrap_or(&name);
  PathBuf::from(format!("{stem}.sheet.png"))
}

fn parse(args: &[String]) -> Result<SheetArgs, String> {
  let mut file = None;
  let mut options = SheetOptions::default();
  let mut out = None;
  let mut words = args.iter();
  while let Some(word) = words.next() {
    let mut value = |flag: &str| {
      words
        .next()
        .cloned()
        .ok_or_else(|| format!("{flag} needs a value"))
    };
    match word.as_str() {
      "--animations" => options.animations = Some(list(&value(word)?)),
      "--skins" => options.skins = list(&value(word)?),
      "--once" => options.once = list(&value(word)?),
      "--moments" => {
        let given = value(word)?;
        options.moments = given
          .parse()
          .map_err(|_| format!("--moments takes a whole number, not {given:?}"))?;
      }
      "--cell" => options.cell = Some(cell(&value(word)?)?),
      "--hz" => {
        let given = value(word)?;
        let hz = given
          .parse()
          .map_err(|_| format!("--hz takes a whole number of steps a second, not {given:?}"))?;
        options.fixed_hz = Some(hz);
      }
      "--out" => out = Some(PathBuf::from(value(word)?)),
      "--bones" => options.bones = true,
      flag if flag.starts_with('-') => {
        return Err(format!("unknown option {flag} (the options are {FLAGS})"));
      }
      path if file.is_none() => file = Some(PathBuf::from(path)),
      extra => return Err(format!("one skeleton file at a time: {extra} is a second")),
    }
  }
  let file = file.ok_or("which skeleton? ryo2d sheet <file.skeleton.ron>")?;
  Ok(SheetArgs { file, options, out })
}

/// `a,b` as names, empty ones dropped
fn list(value: &str) -> Vec<String> {
  value
    .split(',')
    .map(str::trim)
    .filter(|name| !name.is_empty())
    .map(str::to_owned)
    .collect()
}

/// `WxH` in pixels
fn cell(value: &str) -> Result<UVec2, String> {
  let bad = || format!("--cell takes WIDTHxHEIGHT in pixels (200x240), not {value:?}");
  let (width, height) = value.split_once(['x', 'X']).ok_or_else(bad)?;
  let side = |side: &str| side.trim().parse::<u32>().map_err(|_| bad());
  Ok(UVec2::new(side(width)?, side(height)?))
}

fn print_help() {
  println!(
    r#"
ryo2d-sheet - Draw a skeleton's animations as a contact sheet

USAGE:
    ryo2d sheet <FILE.skeleton.ron> [OPTIONS]

OPTIONS:
    --animations a,b   The animations, a row each (default: every one)
    --moments N        The moments of each, a column each (default: 8)
    --skins x,y        The skins to put on, in order
    --once a,b         Animations played once, not looped: no loop round
                       first, and a last cell at their length
    --bones            Draw the bones overlay over the art
    --cell WxH         Each cell's size in pixels, label included
                       (default: sized to fit the poses)
    --hz N             The fixed rate to step at, as the game's
                       EngineBuilder::fixed_hz (default: 60)
    --out PATH         Where to write the PNG, its folder made if missing
                       (default: NAME.sheet.png in the current folder)
    -h, --help         Show this help message

DESCRIPTION:
    Draws each animation at evenly spaced moments, k x length / N for k
    from 0, each cell labelled with the animation's name and the time. A
    moment is reached by stepping the animation from its start, at the
    engine's default 60 steps a second unless --hz says otherwise, after
    one loop round (none for an animation given to --once, which also gets
    a cell at its end), so springs and constraints show as in play. Every
    cell has one scale and the skeleton's origin at one place, on a faint
    ground line, so root motion reads. A skeleton with no animations shows
    its setup pose.

    The file must be in a project's assets folder: its images are paths
    from there. A sheet written into the assets folder is written with a
    warning, since a release would ship it. The sheet draws with the GPU.

EXAMPLE:
    ryo2d sheet assets/hero.skeleton.ron
    ryo2d sheet assets/hero.skeleton.ron --animations walk,run \
        --moments 6 --bones
"#
  );
}

#[cfg(test)]
mod tests {
  use super::*;

  fn args(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| (*word).to_owned()).collect()
  }

  fn message(result: CliResult<PathBuf>) -> String {
    match result {
      Err(error) => error.to_string(),
      Ok(path) => panic!("expected an error, wrote {}", path.display()),
    }
  }

  /// A project in a temporary folder: Ryo.toml, and an assets folder with
  /// a skeleton file in it
  fn project() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
      root.path().join("Ryo.toml"),
      "[project]\nname = \"sheet\"\n",
    )
    .unwrap();
    let rigs = root.path().join("assets").join("rigs");
    std::fs::create_dir_all(&rigs).unwrap();
    std::fs::write(rigs.join("hero.skeleton.ron"), "()").unwrap();
    root
  }

  #[test]
  fn help_prints_and_succeeds() {
    assert!(execute(&args(&["--help"])).is_ok());
  }

  #[test]
  fn the_arguments_become_the_sheet_s_options() {
    let parsed = parse(&args(&[
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
    ]))
    .unwrap();
    assert_eq!(
      parsed,
      SheetArgs {
        file: PathBuf::from("hero.skeleton.ron"),
        options: SheetOptions {
          animations: Some(vec!["walk".to_owned(), "run".to_owned()]),
          moments: 6,
          skins: vec!["dark".to_owned(), "cape".to_owned()],
          once: vec!["jump".to_owned()],
          bones: true,
          cell: Some(UVec2::new(200, 240)),
          fixed_hz: Some(30),
        },
        out: Some(PathBuf::from("sheets/hero.png")),
      }
    );
    // The file alone takes every default
    let plain = parse(&args(&["hero.skeleton.ron"])).unwrap();
    assert_eq!(plain.options, SheetOptions::default());
    assert_eq!(plain.out, None);
  }

  #[test]
  fn arguments_it_cannot_read_say_why() {
    let error = |words: &[&str]| parse(&args(words)).unwrap_err();
    assert!(error(&[]).contains("which skeleton"));
    assert!(error(&["a.skeleton.ron", "b.skeleton.ron"]).contains("one skeleton file"));
    assert!(error(&["a.skeleton.ron", "--bone"]).contains("unknown option --bone"));
    assert!(error(&["a.skeleton.ron", "--moments"]).contains("needs a value"));
    assert!(error(&["a.skeleton.ron", "--moments", "six"]).contains("whole number"));
    assert!(error(&["a.skeleton.ron", "--cell", "200"]).contains("WIDTHxHEIGHT"));
    assert!(error(&["a.skeleton.ron", "--cell", "200xtall"]).contains("WIDTHxHEIGHT"));
    assert!(error(&["a.skeleton.ron", "--hz", "fast"]).contains("--hz takes"));
  }

  #[test]
  fn a_file_outside_the_project_s_assets_folder_is_refused() {
    let root = project();
    let outside = root.path().join("hero.skeleton.ron");
    std::fs::write(&outside, "()").unwrap();
    let never = |_: SheetSkeleton, _: &SheetOptions| -> Result<RgbaImage, SheetError> {
      panic!("nothing is drawn for a file outside the assets folder")
    };
    let refused = message(run(&args(&["hero.skeleton.ron"]), root.path(), never));
    assert!(
      refused.contains("not in the project's assets folder"),
      "{refused}"
    );
    // A file that is not there
    let missing = message(run(&args(&["gone.skeleton.ron"]), root.path(), never));
    assert!(missing.contains("cannot read"), "{missing}");
  }

  #[test]
  fn no_gpu_is_a_message_not_a_crash() {
    let root = project();
    let file = "assets/rigs/hero.skeleton.ron";
    let no_gpu = |_: SheetSkeleton, _: &SheetOptions| Err(SheetError::NoGpu);
    let said = message(run(&args(&[file]), root.path(), no_gpu));
    assert!(said.contains("no GPU"), "{said}");
  }

  #[test]
  fn the_sheet_is_drawn_from_the_file_s_asset_path_and_written() {
    let root = project();
    let file = "assets/rigs/hero.skeleton.ron";
    let mut asked = None;
    let draw = |_: SheetSkeleton, options: &SheetOptions| {
      asked = Some(options.clone());
      Ok(RgbaImage::new(4, 3))
    };
    let written = run(&args(&[file, "--moments", "3"]), root.path(), draw).unwrap();
    assert_eq!(written, root.path().join("hero.sheet.png"));
    assert_eq!(image::open(&written).unwrap().width(), 4);
    assert_eq!(asked.map(|options| options.moments), Some(3));
    // Its asset path, with `/` between folders, from the canonical folder
    let canonical = root.path().join(file).canonicalize().unwrap();
    let (assets, path) = asset_path(&canonical).unwrap();
    assert_eq!(path, "rigs/hero.skeleton.ron");
    assert_eq!(assets, root.path().join("assets").canonicalize().unwrap());
    // --out is from the current folder
    let draw = |_: SheetSkeleton, _: &SheetOptions| Ok(RgbaImage::new(2, 2));
    let out = run(&args(&[file, "--out", "walk.png"]), root.path(), draw).unwrap();
    assert_eq!(out, root.path().join("walk.png"));
    assert!(out.exists());
  }

  #[test]
  fn a_missing_out_folder_is_made_before_drawing() {
    let root = project();
    let file = "assets/rigs/hero.skeleton.ron";
    let folder = root.path().join("sheets").join("hero");
    let mut found = None;
    let draw = |_: SheetSkeleton, _: &SheetOptions| {
      found = Some(folder.is_dir());
      Ok(RgbaImage::new(2, 2))
    };
    let out = run(
      &args(&[file, "--out", "sheets/hero/walk.png"]),
      root.path(),
      draw,
    )
    .unwrap();
    assert_eq!(found, Some(true), "the folder is there before drawing");
    assert!(out.exists());
    // A folder that cannot be made (a file is in its way) is said before
    // anything is drawn
    std::fs::write(root.path().join("blocked"), "").unwrap();
    let never = |_: SheetSkeleton, _: &SheetOptions| -> Result<RgbaImage, SheetError> {
      panic!("nothing is drawn when the sheet cannot be written")
    };
    let said = message(run(
      &args(&[file, "--out", "blocked/walk.png"]),
      root.path(),
      never,
    ));
    assert!(said.contains("cannot make the folder"), "{said}");
  }

  #[test]
  fn a_sheet_written_into_the_assets_folder_is_warned_of_and_written() {
    let root = project();
    let file = "assets/rigs/hero.skeleton.ron";
    let assets = root.path().join("assets").canonicalize().unwrap();
    let draw = |_: SheetSkeleton, _: &SheetOptions| Ok(RgbaImage::new(2, 2));
    let out = run(
      &args(&[file, "--out", "assets/rigs/hero.png"]),
      root.path(),
      draw,
    )
    .unwrap();
    assert!(out.exists(), "written all the same");
    let warning = in_assets_warning(&out, &assets).unwrap_or_default();
    assert!(warning.contains("a release would ship it"), "{warning}");
    // Through a path that leaves and comes back in
    std::fs::create_dir_all(root.path().join("sheets")).unwrap();
    let back = root
      .path()
      .join("sheets")
      .join("..")
      .join("assets")
      .join("a.png");
    assert!(in_assets_warning(&back, &assets).is_some());
    // Beside the assets folder, or in the current folder, no warning
    let beside = root.path().join("assets-sheets");
    std::fs::create_dir_all(&beside).unwrap();
    assert_eq!(in_assets_warning(&beside.join("a.png"), &assets), None);
    assert_eq!(in_assets_warning(&root.path().join("a.png"), &assets), None);
  }
}
