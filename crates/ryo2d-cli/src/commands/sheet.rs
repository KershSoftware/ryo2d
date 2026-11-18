//! Sheet command - draws a skeleton's animations as a contact sheet

use std::path::{Path, PathBuf};

use image::RgbaImage;

use super::{CliError, CliResult, Output};
use crate::args::SheetArgs;
use ryo2d::core::config::Workspace;
use ryo2d::testing::{SheetError, SheetOptions, SheetSkeleton, skeleton_sheet};

/// Execute the sheet command
pub fn execute(args: &SheetArgs) -> CliResult<()> {
  let cwd =
    std::env::current_dir().map_err(|e| CliError::io("could not find the current folder", e))?;
  let out = run(args, &cwd, skeleton_sheet)?;
  Output::success(&format!("wrote {}", out.display()));
  Ok(())
}

/// Finds the file's project and draws its sheet with `draw`, writing the
/// PNG; relative paths are from `cwd`. The path written, or why not
fn run(
  args: &SheetArgs,
  cwd: &Path,
  draw: impl FnOnce(SheetSkeleton, &SheetOptions) -> Result<RgbaImage, SheetError>,
) -> CliResult<PathBuf> {
  let file = cwd.join(&args.file);
  let file = file
    .canonicalize()
    .map_err(|error| CliError::Other(format!("cannot read {}: {error}", file.display())))?;
  let (assets, path) = asset_path(&file)?;
  let out = args
    .out
    .as_ref()
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
  let sheet = draw(SheetSkeleton::file(&assets, &path), &args.options()).map_err(|error| {
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
  let workspace = Workspace::discover_from(folder).map_err(CliError::Workspace)?;
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

#[cfg(test)]
mod tests {
  use super::*;

  /// The arguments `ryo2d sheet` is run with, read by the real parser
  fn args(words: &[&str]) -> SheetArgs {
    use clap::Parser;
    let cli =
      crate::args::Cli::try_parse_from(["ryo2d", "sheet"].into_iter().chain(words.iter().copied()));
    match cli.map(|cli| cli.command) {
      Ok(crate::args::Command::Sheet(args)) => args,
      other => panic!("sheet {words:?} does not parse: {other:?}"),
    }
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
