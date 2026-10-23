//! Scene-start asset loading: what a scene's `on_enter` does today. Forty
//! PNGs (ten each at 32, 64, 128 and 256 pixels square) loaded as sprites and
//! three UI files (the second importing the first), on a fresh
//! `AssetManager` each iteration. This is the baseline the asset registry is
//! measured against.
//!
//! The two fonts a scene start also adds are not measured here: the bench is
//! an external crate and has no public way to build a `Fonts` registry (its
//! constructors are crate-private and the UI harness keeps its fonts inside).
//! The fonts join the bench when it is ported to the registry.

use std::path::Path;
use std::process::Command;

use criterion::{BatchSize, Criterion, criterion_group};
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

/// A UI file of about 50 lines in the vocabulary of the breakout menu; `use_a`
/// makes it import `a.ui.ron` and draw with its template
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

fn scene_start(c: &mut Criterion) {
  let dir = tempfile::TempDir::new().expect("temp dir");
  let pngs = write_pngs(dir.path());
  let uis = ["a.ui.ron", "b.ui.ron", "c.ui.ron"];
  std::fs::write(dir.path().join("a.ui.ron"), ui_source("a", false)).expect("write ui");
  std::fs::write(dir.path().join("b.ui.ron"), ui_source("b", true)).expect("write ui");
  std::fs::write(dir.path().join("c.ui.ron"), ui_source("c", false)).expect("write ui");

  // Images load through the registry (a fresh one each iteration, over
  // the same folder); UI files still through the old manager
  let fresh = || {
    let mut world = ryo2d::ecs::World::new();
    let server = AssetServer::new(DiskSource::new(dir.path()));
    server.register_asset::<Image>(&mut world);
    server.register_loader(ImageLoader);
    world.insert_resource(server);
    (AssetManager::with_base_path(dir.path()), world)
  };
  c.bench_function("scene_start", |b| {
    b.iter_batched(
      fresh,
      |(mut assets, mut world)| {
        for name in &pngs {
          std::hint::black_box(assets.load_sprite(&mut world, name).expect("load image"));
        }
        for name in uis {
          std::hint::black_box(assets.load_ui(name).expect("load ui"));
        }
        (assets, world)
      },
      BatchSize::PerIteration,
    );
  });
}

fn criterion_config() -> Criterion {
  let load = Command::new("uptime")
    .output()
    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    .unwrap_or_else(|_| "uptime unavailable".into());
  println!("machine: {load}");
  println!(
    "assets bench: images and UI files only; fonts are not measured (no public Fonts route)"
  );
  Criterion::default().sample_size(20)
}

criterion_group! {
  name = benches;
  config = criterion_config();
  targets = scene_start
}

fn main() {
  benches();
  Criterion::default().configure_from_args().final_summary();
  print_median();
}

/// The median of the run criterion just wrote, in microseconds
fn print_median() {
  let path =
    Path::new(env!("CARGO_MANIFEST_DIR")).join("target/criterion/scene_start/new/estimates.json");
  let median = std::fs::read_to_string(path).ok().and_then(|text| {
    let at = text.find("\"median\"")?;
    let rest = &text[at..];
    let from = rest.find("\"point_estimate\":")? + "\"point_estimate\":".len();
    let to = rest[from..].find([',', '}'])? + from;
    rest[from..to].trim().parse::<f64>().ok()
  });
  match median {
    Some(ns) => println!("scene_start median: {:.0} us", ns / 1000.0),
    None => println!("scene_start median: unavailable"),
  }
}
