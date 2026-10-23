//! Scene-start asset loading: what a scene's `on_enter` does. Forty PNGs
//! (ten each at 32, 64, 128 and 256 pixels square) and two fonts (Press
//! Start 2P and JetBrains Mono) loaded at once through the asset registry
//! (`load_now::<Image>`, `load_now::<Font>`), and three UI files (the second
//! importing the first) through `AssetManager`, on a fresh registry and
//! manager each iteration. The same bench on the old manager's image loads,
//! before the registry, is what it is measured against.
//!
//! The fonts are measured from the registry's fonts on: before them the
//! bench had no public way to build a `Fonts` registry, so the "before"
//! build loads no fonts.
//!
//! The median is read from where criterion writes its results:
//! `$CRITERION_HOME`, else `$CARGO_TARGET_DIR/criterion`, else the
//! workspace's `target/criterion`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use criterion::{BatchSize, Criterion};
use ryo2d::prelude::*;

const SIZES: [u32; 4] = [32, 64, 128, 256];
const PER_SIZE: u32 = 10;

fn write_pngs(dir: &Path) -> Vec<String> {
  let mut names = Vec::new();
  for size in SIZES {
    for n in 0..PER_SIZE {
      let name = format!("sprite_{size}_{n}.png");
      let img = image::RgbaImage::from_fn(size, size, |x, y| {
        let v = x.wrapping_mul(31) ^ y.wrapping_mul(17) ^ n.wrapping_mul(97) ^ size;
        image::Rgba([v as u8, (v >> 3) as u8, x as u8 ^ y as u8, 255])
      });
      img.save(dir.join(&name)).expect("write bench png");
      names.push(name);
    }
  }
  names
}

/// A UI file of about 50 lines in the vocabulary of the breakout menu;
/// `import_a` makes it import `a.ui.ron` and draw with its template
fn ui_source(tag: &str, import_a: bool) -> String {
  let imports = if import_a {
    "  imports: [\"a.ui.ron\"],\n"
  } else {
    ""
  };
  let borrowed = if import_a {
    "      Line_a(text: \"from a\", color: \"#ffffff\", y: 10),\n"
  } else {
    ""
  };
  let lines: String = (0..6)
    .map(|i| {
      format!(
        "      Line_{tag}(text: \"{tag} line {i}\", color: \"#c0c0c0\", y: {}),\n",
        40 + i * 20
      )
    })
    .collect();
  let blocks: String = (0..8)
    .map(|i| {
      format!(
        "          Block_{tag}(color: \"#{:02x}{:02x}{:02x}\"),\n",
        (i * 37) % 256,
        (i * 91) % 256,
        (i * 53) % 256
      )
    })
    .collect();
  format!(
    r##"// Bench UI file {tag}
(
{imports}  templates: {{
    "Line_{tag}": (
      params: ["text", "color", "y"],
      node: Column(
        anchor: TopCenter, offset: (0, y),
        children: [Text(text: text, color: color)],
      ),
    ),
    "Block_{tag}": (
      params: ["color"],
      node: Stack(width: 60, height: 25, background: color),
    ),
  }},
  root: Stack(
    fill: true, background: "#141423",
    children: [
{borrowed}{lines}      Row(
        anchor: TopCenter, offset: (0, 250), gap: 5,
        children: [
{blocks}        ],
      ),
      Column(
        focus_scope: true, anchor: TopCenter, offset: (0, 350),
        children: [
          Button(label: "START {tag}", action: "start", width: 200, height: 50, default_focus: true),
          Button(label: "QUIT {tag}", action: "quit", width: 200, height: 50),
        ],
      ),
      Column(
        anchor: BottomCenter, offset: (0, -30),
        children: [Text(text: "footer {tag}", color: "#404040")],
      ),
    ],
  ),
)
"##
  )
}

/// The two fonts, written to `dir`: a game's own and a second distinct one
fn write_fonts(dir: &Path) -> [&'static str; 2] {
  let root = Path::new(env!("CARGO_MANIFEST_DIR"));
  let fonts = [
    (
      "PressStart2P-Regular.ttf",
      root.join("examples/games/breakout/assets/fonts/PressStart2P-Regular.ttf"),
    ),
    (
      "JetBrainsMono-Regular.ttf",
      root.join("src/assets/fonts/JetBrainsMono-Regular.ttf"),
    ),
  ];
  for (name, from) in &fonts {
    std::fs::copy(from, dir.join(name)).expect("copy a bench font");
  }
  fonts.map(|(name, _)| name)
}

fn scene_start(c: &mut Criterion) {
  let dir = tempfile::TempDir::new().expect("temp dir");
  let pngs = write_pngs(dir.path());
  let fonts = write_fonts(dir.path());
  let uis = ["a.ui.ron", "b.ui.ron", "c.ui.ron"];
  std::fs::write(dir.path().join("a.ui.ron"), ui_source("a", false)).expect("write ui");
  std::fs::write(dir.path().join("b.ui.ron"), ui_source("b", true)).expect("write ui");
  std::fs::write(dir.path().join("c.ui.ron"), ui_source("c", false)).expect("write ui");

  // A fresh registry each iteration, over the same folder; UI files still
  // load through the old manager
  let fresh = || {
    let mut world = ryo2d::ecs::World::new();
    let server = AssetServer::new(DiskSource::new(dir.path()));
    server.register_asset::<Image>(&mut world);
    server.register_loader(ImageLoader);
    server.register_asset::<Font>(&mut world);
    server.register_loader(FontLoader);
    world.insert_resource(server.clone());
    (server, world, AssetManager::with_base_path(dir.path()))
  };
  c.bench_function("scene_start", |b| {
    b.iter_batched(
      fresh,
      |(server, mut world, mut manager)| {
        for name in &pngs {
          let image = server.load_now::<Image>(&mut world, name);
          std::hint::black_box(image.expect("load image"));
        }
        for name in fonts {
          let font = server.load_now::<Font>(&mut world, name);
          std::hint::black_box(font.expect("load font"));
        }
        for name in uis {
          std::hint::black_box(manager.load_ui(name).expect("load ui"));
        }
        (server, world, manager)
      },
      BatchSize::PerIteration,
    );
  });
}

/// Where criterion writes its results, and the median is read from
fn results_dir() -> PathBuf {
  if let Some(home) = std::env::var_os("CRITERION_HOME") {
    return PathBuf::from(home);
  }
  std::env::var_os("CARGO_TARGET_DIR")
    .map(PathBuf::from)
    .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("target"))
    .join("criterion")
}

fn main() {
  let started = SystemTime::now();
  let load = Command::new("uptime")
    .output()
    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    .unwrap_or_else(|_| "uptime unavailable".into());
  println!("machine: {load}");
  println!("assets bench: images, fonts and UI files");
  let results = results_dir();
  let mut criterion = Criterion::default()
    .sample_size(20)
    .output_directory(&results)
    .configure_from_args();
  scene_start(&mut criterion);
  criterion.final_summary();
  print_median(&results, started);
}

/// The median of the run criterion just wrote to `results`, in
/// microseconds; nothing for results an earlier run left (this run skipped
/// the bench, as a filter or `--list` does)
fn print_median(results: &Path, started: SystemTime) {
  let path = results.join("scene_start/new/estimates.json");
  let written = std::fs::metadata(&path)
    .and_then(|meta| meta.modified())
    .is_ok_and(|modified| modified >= started);
  if !written {
    println!("scene_start median: not measured in this run");
    return;
  }
  let median = std::fs::read_to_string(&path).ok().and_then(|text| {
    let at = text.find("\"median\"")?;
    let rest = &text[at..];
    let from = rest.find("\"point_estimate\":")? + "\"point_estimate\":".len();
    let to = rest[from..].find([',', '}'])? + from;
    rest[from..to].trim().parse::<f64>().ok()
  });
  match median {
    Some(ns) => println!("scene_start median: {:.0} us", ns / 1000.0),
    None => println!("scene_start median: unavailable ({})", path.display()),
  }
}
