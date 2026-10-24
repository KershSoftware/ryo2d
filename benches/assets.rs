//! The asset registry against its targets.
//!
//! Scene start, what a scene's `on_enter` does, on a fresh registry each
//! iteration:
//! - `scene_start`: forty PNGs (ten each at 32, 64, 128 and 256 pixels
//!   square) and three UI files (the second importing the first) loaded at
//!   once (`load_now`). It loads what the same bench on the old manager
//!   loaded, before the registry, and is measured against that build;
//! - `scene_start_fonts`: the same with two fonts (Press Start 2P and
//!   JetBrains Mono), the spec's whole set. The build before the registry
//!   had no public way to build a `Fonts` registry, so this has no "before";
//! - `scene_start_async`: `scene_start`'s assets through `load`, pumped
//!   until their `progress` is done. Its workers have started already (an
//!   untimed load of a 1x1 image), as a game's have by its second scene.
//!
//! Before criterion runs, the reload guard runs a `HeadlessEngine`
//! (1280x720) drawing a 2048x2048 PNG and a 256 px SVG, read from a
//! `MemorySource` with hot reload on. Over 2,000 frames every 50th inserts
//! a changed PNG and SVG; the source tells the server, the workers decode
//! them, and the pump swaps each new value in, under the same handle, in
//! some later frame. That frame starts its upload: the SVG's is written at
//! once, the PNG's into a new texture a band a frame while the old one
//! draws, until the new one takes its place. Each frame's main-thread time
//! is the wall time of the whole step (the pump, the update, the UI pass
//! and the render step), with the GPU finished before each step, outside
//! the measure, as `benches/ui_render.rs`'s guard does.
//!
//! The frames are paced at 60 Hz, as a game's are. Stepped back to back
//! (under 100 µs each), the PNG took longer to decode than 50 frames, so
//! each change overtook the last and it never landed. The wait spins: when
//! the main thread slept through it, it ran on a slower core and an
//! ordinary frame took about six times as long (716 µs against 122).
//!
//! A reload's hitch is the slowest frame from the one its new value landed
//! in to the one its new pixels were first drawn in. The guard prints, for
//! each image, the median and maximum hitch over an ordinary frame (one
//! outside every reload), how many frames each reload took to be drawn, the
//! frame after, and how often a frame is more than 1.1 ms over an ordinary
//! one, outside reloads and in them. It holds each image's median hitch to within 0.5 ms of
//! an ordinary frame (the spec), and fails a reload whose upload did not
//! begin in the frame it landed in, or that was not drawn by the end. A
//! miss fails the run once criterion's benches have run, printing the
//! command to profile it with. The `reload_hitch` criterion bench steps the
//! same engine back to back, changing the files once the last change is
//! drawn and 50 frames have passed, for that profile; its median is of
//! every frame, most of them ordinary. The guard runs only where criterion
//! measures `reload_hitch`: with no filter or one naming it, and not for
//! `--list`, `--test` or `--profile-time`. What each part needs (the
//! encoded files, the scene folder, the engine) is made only when it runs.
//!
//! Last, the pump with parents waiting on a dependency that does not load:
//! 10, 100 and 1000 UI files importing one whose read is held. It prints
//! the median of a pump with nothing to settle, and of a pump that takes
//! in one more parent. It runs with no filter or one naming
//! `pump_waiting`.
//!
//! Medians are read from where criterion writes its results:
//! `$CRITERION_HOME`, else `$CARGO_TARGET_DIR/criterion`, else the
//! workspace's `target/criterion`.

mod guards;

use std::cell::OnceCell;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use criterion::{BatchSize, Criterion};
use ryo2d::core::asset::pump;
use ryo2d::prelude::*;
use ryo2d::testing::{HeadlessEngine, shared_gpu};

const SIZES: [u32; 4] = [32, 64, 128, 256];
const PER_SIZE: u32 = 10;
/// The criterion benches whose medians are printed, with what each
/// median is of
const BENCHES: [(&str, &str); 4] = [
  ("scene_start", "an iteration"),
  ("scene_start_fonts", "an iteration"),
  ("scene_start_async", "an iteration"),
  (
    "reload_hitch",
    "every frame of the back-to-back reloads, most of them ordinary (the guard reads the hitch)",
  ),
];

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

/// The scene-start benches' folder: the PNGs, the fonts, the UI files and
/// the image that starts the workers
struct SceneFiles {
  dir: tempfile::TempDir,
  pngs: Vec<String>,
  fonts: [&'static str; 2],
  uis: [&'static str; 3],
}

const WARM: &str = "warm.png";

impl SceneFiles {
  fn new() -> SceneFiles {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let pngs = write_pngs(dir.path());
    let fonts = write_fonts(dir.path());
    let uis = ["a.ui.ron", "b.ui.ron", "c.ui.ron"];
    std::fs::write(dir.path().join("a.ui.ron"), ui_source("a", false)).expect("write ui");
    std::fs::write(dir.path().join("b.ui.ron"), ui_source("b", true)).expect("write ui");
    std::fs::write(dir.path().join("c.ui.ron"), ui_source("c", false)).expect("write ui");
    image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 255, 255, 255]))
      .save(dir.path().join(WARM))
      .expect("write the warm-up png");
    SceneFiles {
      dir,
      pngs,
      fonts,
      uis,
    }
  }

  /// A fresh registry over the folder
  fn registry(&self) -> (AssetServer, ryo2d::ecs::World) {
    let mut world = ryo2d::ecs::World::new();
    let server = AssetServer::new(DiskSource::new(self.dir.path()));
    server.register_asset::<Image>(&mut world);
    server.register_loader(ImageLoader);
    server.register_asset::<Font>(&mut world);
    server.register_loader(FontLoader);
    server.register_asset::<UiFile>(&mut world);
    server.register_loader(UiFileLoader);
    world.insert_resource(server.clone());
    (server, world)
  }

  /// A fresh registry whose workers have started: an image loaded through
  /// them and pumped in
  fn registry_with_workers(&self) -> (AssetServer, ryo2d::ecs::World, Handle<Image>) {
    let (server, mut world) = self.registry();
    let warm = server.load::<Image>(WARM);
    pump_until_done(&server, &mut world, &[warm.id().untyped()]);
    (server, world, warm)
  }
}

/// Pumps until every one of `ids` is loaded
fn pump_until_done(server: &AssetServer, world: &mut ryo2d::ecs::World, ids: &[UntypedAssetId]) {
  loop {
    pump(world);
    let progress = server.progress(ids);
    if progress.is_done() {
      assert_eq!(progress.failed, 0, "every asset loads");
      return;
    }
    std::thread::yield_now();
  }
}

fn scene_start(c: &mut Criterion, files: &OnceCell<SceneFiles>) {
  // The folder is written when criterion first runs one of these, not for
  // a filter that skips them all
  let files = || files.get_or_init(SceneFiles::new);
  for (name, fonts) in [("scene_start", false), ("scene_start_fonts", true)] {
    c.bench_function(name, |b| {
      let files = files();
      b.iter_batched(
        || files.registry(),
        |(server, mut world)| {
          for name in &files.pngs {
            let image = server.load_now::<Image>(&mut world, name);
            std::hint::black_box(image.expect("load image"));
          }
          if fonts {
            for name in files.fonts {
              let font = server.load_now::<Font>(&mut world, name);
              std::hint::black_box(font.expect("load font"));
            }
          }
          for name in files.uis {
            let file = server.load_now::<UiFile>(&mut world, name);
            std::hint::black_box(file.expect("load ui"));
          }
          (server, world)
        },
        BatchSize::PerIteration,
      );
    });
  }
  c.bench_function("scene_start_async", |b| {
    let files = files();
    b.iter_batched(
      || files.registry_with_workers(),
      |(server, mut world, warm)| {
        let images: Vec<Handle<Image>> = files
          .pngs
          .iter()
          .map(|name| server.load::<Image>(name))
          .collect();
        let uis: Vec<Handle<UiFile>> = files
          .uis
          .iter()
          .map(|name| server.load::<UiFile>(name))
          .collect();
        let ids: Vec<UntypedAssetId> = images
          .iter()
          .map(|image| image.id().untyped())
          .chain(uis.iter().map(|file| file.id().untyped()))
          .collect();
        pump_until_done(&server, &mut world, &ids);
        (server, world, warm, images, uis)
      },
      BatchSize::PerIteration,
    );
  });
}

/// The reload guard's frames
const HITCH_FRAMES: usize = 2000;
/// Frames between changed files
const RELOAD_EVERY: usize = 50;
/// The spec's reload hitch: a landing frame within this of an ordinary one
const HITCH_TARGET: Duration = Duration::from_micros(500);
/// A frame this much slower than an ordinary one is counted as an outlier
const OUTLIER: Duration = Duration::from_micros(1_100);
/// A 60 Hz frame: the guard starts a frame no sooner than this after the last
const PACE: Duration = Duration::from_micros(16_667);
const BIG: &str = "big.png";
const BIG_SIDE: u32 = 2048;
const ICON: &str = "icon.svg";

/// A 2048 px PNG in one of two looks, `variant` 0 or 1
fn big_png(variant: u32) -> Vec<u8> {
  let image = image::RgbaImage::from_fn(BIG_SIDE, BIG_SIDE, |x, y| {
    let v = x.wrapping_mul(31) ^ y.wrapping_mul(17) ^ variant.wrapping_mul(97);
    image::Rgba([
      v as u8,
      (v >> 3) as u8,
      (x ^ y) as u8 ^ (variant as u8 * 85),
      255,
    ])
  });
  let mut bytes = Vec::new();
  image
    .write_to(
      &mut std::io::Cursor::new(&mut bytes),
      image::ImageFormat::Png,
    )
    .expect("encode the big png");
  bytes
}

/// A 256 px SVG icon in one of two looks, `variant` 0 or 1
fn icon_svg(variant: u32) -> Vec<u8> {
  let (from, to, mark) = if variant == 0 {
    ("#3060c0", "#80e0ff", "#ffd040")
  } else {
    ("#c03040", "#ffb080", "#40ff90")
  };
  format!(
    r##"<svg xmlns="http://www.w3.org/2000/svg" width="256" height="256" viewBox="0 0 256 256">
  <defs>
    <linearGradient id="g" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0" stop-color="{from}"/>
      <stop offset="1" stop-color="{to}"/>
    </linearGradient>
  </defs>
  <rect x="8" y="8" width="240" height="240" rx="32" fill="url(#g)"/>
  <circle cx="128" cy="128" r="84" fill="none" stroke="#ffffff" stroke-width="12"/>
  <path d="M128 60 L184 168 L72 168 Z" fill="{mark}" stroke="#202020" stroke-width="4"/>
</svg>
"##
  )
  .into_bytes()
}

/// Both looks of both files, encoded once
struct Looks {
  big: [Vec<u8>; 2],
  icon: [Vec<u8>; 2],
}

impl Looks {
  fn new() -> Looks {
    Looks {
      big: [big_png(0), big_png(1)],
      icon: [icon_svg(0), icon_svg(1)],
    }
  }
}

/// Draws the big PNG at a quarter of its size, and the SVG
struct Reloading;

impl Scene for Reloading {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let big = ctx.assets().load_now::<Image>(BIG).expect("the big png");
    let icon = ctx.assets().load_now::<Image>(ICON).expect("the svg");
    ctx.world.spawn((
      Transform::from_xy(400.0, 360.0).with_scale(Vec2::splat(0.25)),
      Renderable::image(big),
    ));
    ctx
      .world
      .spawn((Transform::from_xy(900.0, 360.0), Renderable::image(icon)));
  }
}

/// An engine drawing the two images, and the files it reads
struct ReloadRig {
  files: MemorySource,
  engine: HeadlessEngine,
  big: AssetId<Image>,
  icon: AssetId<Image>,
  /// Changes made so far
  changes: usize,
}

impl ReloadRig {
  /// The engine, warmed up; none without a GPU
  fn new(looks: &Looks) -> Option<ReloadRig> {
    let files = MemorySource::new();
    files.insert(BIG, looks.big[0].clone());
    files.insert(ICON, looks.icon[0].clone());
    let builder = EngineBuilder::new()
      .resolution(1280, 720)
      .asset_source(files.clone())
      .hot_reload(true)
      .start_scene(Reloading);
    let mut engine = HeadlessEngine::new_or_skip(builder)?;
    engine.step_frames(60).expect("the warm-up frames");
    let big = engine.assets().load_now::<Image>(BIG).expect("the big png");
    let icon = engine.assets().load_now::<Image>(ICON).expect("the svg");
    Some(ReloadRig {
      files,
      engine,
      big: big.id(),
      icon: icon.id(),
      changes: 0,
    })
  }

  /// Puts the other look of each file in the source, which tells the server
  fn change(&mut self, looks: &Looks) {
    self.changes += 1;
    let look = self.changes % 2;
    self.files.insert(BIG, looks.big[look].clone());
    self.files.insert(ICON, looks.icon[look].clone());
  }

  /// The two images' versions in the store
  fn versions(&self) -> (Option<u64>, Option<u64>) {
    let images = self.engine.world().resource::<Assets<Image>>();
    (images.version(self.big), images.version(self.icon))
  }
}

/// One frame of the reload guard
struct HitchFrame {
  /// The whole step on the main thread
  step: Duration,
  /// Its render step
  render: Duration,
  /// The PNG's new value landed in it
  big: bool,
  /// The SVG's new value landed in it
  icon: bool,
  /// Image textures made whole in it: uploaded at once, or a new texture
  /// written over frames taking the old one's place
  uploads: u64,
  /// After it, the PNG's new version was still being written into a new
  /// texture (its old pixels drawn meanwhile)
  big_pending: bool,
  /// After it, the SVG's was
  icon_pending: bool,
}

/// One reload: the frame its new value landed in, and the frame its new
/// pixels were first drawn in (none: not within the run)
struct Reload {
  landed: usize,
  drawn: Option<usize>,
  /// Its upload began in the frame it landed in: uploaded at once, or the
  /// new texture started
  began: bool,
}

/// The reloads of the image `landed` and `pending` read
fn reloads(
  frames: &[HitchFrame],
  landed: fn(&HitchFrame) -> bool,
  pending: fn(&HitchFrame) -> bool,
) -> Vec<Reload> {
  frames
    .iter()
    .enumerate()
    .filter(|(_, frame)| landed(frame))
    .map(|(at, frame)| Reload {
      landed: at,
      drawn: (at..frames.len()).find(|&k| !pending(&frames[k])),
      began: frame.uploads > 0 || pending(frame),
    })
    .collect()
}

/// The median and maximum of `times`, or None for none
fn median_max<T: Ord + Copy>(mut times: Vec<T>) -> Option<(T, T)> {
  times.sort_unstable();
  Some((*times.get(times.len() / 2)?, *times.last()?))
}

fn us(time: Duration) -> f64 {
  time.as_secs_f64() * 1e6
}

/// The reload hitch: every frame from the one a reloaded image's new value
/// lands in to the one its new pixels are first drawn in, against an
/// ordinary frame. A reload's hitch is the slowest of its frames; each
/// image's median hitch over the target is noted for the run to fail on,
/// as is a reload whose upload did not begin in the frame it landed in or
/// that was not drawn by the end of the run
fn reload_guard(looks: &Looks) {
  let Some(mut rig) = ReloadRig::new(looks) else {
    return;
  };
  let Ok((device, _)) = shared_gpu() else {
    return;
  };
  let mut frames = Vec::with_capacity(HITCH_FRAMES);
  let mut next = Instant::now();
  for frame in 0..HITCH_FRAMES {
    if frame > 0 && frame.is_multiple_of(RELOAD_EVERY) {
      rig.change(looks);
    }
    device
      .poll(wgpu::PollType::wait_indefinitely())
      .expect("the GPU finishes its work");
    // Spun, not slept: a thread that sleeps most of each frame runs its
    // frames on a slower core, nine times as long
    while Instant::now() < next {
      std::hint::spin_loop();
    }
    let (big, icon) = rig.versions();
    let uploads = rig.engine.image_uploads();
    let started = Instant::now();
    next = started + PACE;
    rig.engine.step().expect("a guard frame");
    let step = started.elapsed();
    let (big_now, icon_now) = rig.versions();
    frames.push(HitchFrame {
      step,
      render: rig.engine.render_time(),
      big: big_now != big,
      icon: icon_now != icon,
      uploads: rig.engine.image_uploads() - uploads,
      big_pending: rig.engine.image_upload_pending(rig.big),
      icon_pending: rig.engine.image_upload_pending(rig.icon),
    });
  }
  let kinds: [(&str, Vec<Reload>); 2] = [
    (
      "the PNG",
      reloads(&frames, |frame| frame.big, |frame| frame.big_pending),
    ),
    (
      "the SVG",
      reloads(&frames, |frame| frame.icon, |frame| frame.icon_pending),
    ),
  ];
  // Every frame from a landing to its drawing, of either image
  let mut reloading = vec![false; frames.len()];
  for (_, reloads) in &kinds {
    for reload in reloads {
      let last = reload.drawn.unwrap_or(frames.len() - 1);
      for frame in &mut reloading[reload.landed..=last] {
        *frame = true;
      }
    }
  }
  let ordinary_frames: Vec<&HitchFrame> = frames
    .iter()
    .zip(&reloading)
    .filter(|(_, reloading)| !**reloading)
    .map(|(frame, _)| frame)
    .collect();
  let Some((ordinary, ordinary_max)) =
    median_max(ordinary_frames.iter().map(|frame| frame.step).collect())
  else {
    guards::miss("reload_hitch: no ordinary frame".into());
    return;
  };
  let (ordinary_render, _) =
    median_max(ordinary_frames.iter().map(|frame| frame.render).collect()).unwrap_or_default();
  let ordinary_p95 = {
    let mut steps: Vec<Duration> = ordinary_frames.iter().map(|frame| frame.step).collect();
    steps.sort_unstable();
    steps[steps.len() * 95 / 100]
  };
  // The frame after each reload is drawn, outside every reload
  let after: Vec<Duration> = kinds
    .iter()
    .flat_map(|(_, reloads)| reloads)
    .filter_map(|reload| reload.drawn.map(|drawn| drawn + 1))
    .filter(|&k| k < frames.len() && !reloading[k])
    .map(|k| frames[k].step)
    .collect();
  let over = |time: Duration| us(time) - us(ordinary);
  // How often a frame is an outlier, outside reloads and in them, so a
  // reload's slowest frames can be told from the machine's
  let outliers = |reload: bool| {
    let kept: Vec<&HitchFrame> = frames
      .iter()
      .zip(&reloading)
      .filter(|(_, reloading)| **reloading == reload)
      .map(|(frame, _)| frame)
      .collect();
    let slow = kept
      .iter()
      .filter(|frame| frame.step.saturating_sub(ordinary) > OUTLIER)
      .count();
    format!(
      "{slow} of {} ({:.2}%)",
      kept.len(),
      100.0 * slow as f64 / kept.len().max(1) as f64
    )
  };
  let changes = rig.changes;
  let mut lines = Vec::new();
  let mut missed = Vec::new();
  let mut every = Vec::new();
  for (kind, reloads) in &kinds {
    if reloads.is_empty() {
      missed.push(format!(
        "reload_hitch: of {changes} changes {kind} never landed"
      ));
      continue;
    }
    let unbegun = reloads.iter().filter(|reload| !reload.began).count();
    if unbegun > 0 {
      missed.push(format!(
        "reload_hitch: {unbegun} of {kind}'s landings began no upload in their frame"
      ));
    }
    let undrawn = reloads
      .iter()
      .filter(|reload| reload.drawn.is_none())
      .count();
    if undrawn > 0 {
      missed.push(format!(
        "reload_hitch: {undrawn} of {kind}'s reloads were not drawn by the end of the run"
      ));
    }
    // A reload's hitch: the slowest frame from its landing to its drawing
    let worst: Vec<Duration> = reloads
      .iter()
      .filter_map(|reload| {
        let drawn = reload.drawn?;
        frames[reload.landed..=drawn]
          .iter()
          .map(|frame| frame.step)
          .max()
      })
      .collect();
    every.extend(worst.iter().copied());
    let spans: Vec<usize> = reloads
      .iter()
      .filter_map(|reload| Some(reload.drawn? - reload.landed + 1))
      .collect();
    let renders: Vec<Duration> = reloads
      .iter()
      .filter_map(|reload| Some(&frames[reload.landed..=reload.drawn?]))
      .flatten()
      .map(|frame| frame.render)
      .collect();
    let landing: Vec<Duration> = reloads
      .iter()
      .map(|reload| frames[reload.landed].step)
      .collect();
    let (Some((median, max)), Some((span, span_max)), Some((landing, _))) =
      (median_max(worst), median_max(spans), median_max(landing))
    else {
      continue;
    };
    let (render, _) = median_max(renders).unwrap_or_default();
    let met = median.saturating_sub(ordinary) <= HITCH_TARGET;
    let verdict = if met { "met" } else { "missed" };
    let line = format!(
      "{kind}: {} reloads, drawn {span} frames after landing (median; max {span_max}); the \
       slowest frame of each, median {:.1} µs ({:+.1}), max {:.1} µs ({:+.1}); the landing \
       frame, median {:+.1}; render step of those frames {render:.1?}; within {} µs: \
       {verdict}",
      reloads.len(),
      us(median),
      over(median),
      us(max),
      over(max),
      over(landing),
      HITCH_TARGET.as_micros(),
    );
    if !met {
      missed.push(format!("reload_hitch, {line}"));
    }
    lines.push(line);
  }
  if let Some((median, max)) = median_max(every) {
    let met = median.saturating_sub(ordinary) <= HITCH_TARGET;
    let line = format!(
      "every reload: the slowest frame of each, median {:+.1} µs, max {:+.1}; within {} µs: {}",
      over(median),
      over(max),
      HITCH_TARGET.as_micros(),
      if met { "met" } else { "missed" },
    );
    if !met {
      missed.push(format!("reload_hitch, {line}"));
    }
    lines.push(line);
  }
  let after = median_max(after).map_or("none".to_string(), |(median, max)| {
    format!(
      "median {:.1} µs ({:+.1}), max {:.1} µs ({:+.1})",
      us(median),
      over(median),
      us(max),
      over(max)
    )
  });
  eprintln!(
    "reload_hitch guard, main-thread frame time (the whole step, the GPU finished before \
     each), {HITCH_FRAMES} frames at 60 Hz, a changed PNG and SVG every {RELOAD_EVERY}th \
     ({changes} changes). Ordinary frames ({} of them, outside every reload): median {:.1} µs \
     (render step {:.1}), 95th percentile {:+.1}, max {:+.1}. {}. The frame after a reload is \
     drawn: {after}. Frames more than {} µs over an ordinary one: outside reloads {}, in \
     reloads {}",
    ordinary_frames.len(),
    us(ordinary),
    us(ordinary_render),
    over(ordinary_p95),
    over(ordinary_max),
    lines.join("; "),
    OUTLIER.as_micros(),
    outliers(false),
    outliers(true),
  );
  for miss in missed {
    guards::miss(miss);
  }
}

/// Frames the criterion bench waits for a change to be drawn before it
/// changes the files again
const GIVE_UP: usize = 600;

/// The reload rig stepped back to back by criterion, for the profile of a
/// reload: the files change once the last change's new pixels are drawn
/// and `RELOAD_EVERY` frames have passed (or `GIVE_UP` frames, should one
/// never be). Its median is of every frame, most of them ordinary: the
/// guard reads the hitch
fn reload_bench(c: &mut Criterion, looks: &OnceCell<Looks>) {
  if shared_gpu().is_err() {
    return;
  }
  // Made when criterion first runs the bench, not for a filter that skips it
  let mut rig: Option<ReloadRig> = None;
  let mut since = 0usize;
  // The versions the last change replaced, until both have moved on
  let mut waiting: Option<(Option<u64>, Option<u64>)> = None;
  c.bench_function("reload_hitch", |b| {
    let rig = rig
      .get_or_insert_with(|| ReloadRig::new(looks.get_or_init(Looks::new)).expect("the GPU is up"));
    b.iter(|| {
      since += 1;
      match waiting {
        Some((big, icon)) => {
          let (big_now, icon_now) = rig.versions();
          let drawn = big_now != big
            && icon_now != icon
            && !rig.engine.image_upload_pending(rig.big)
            && !rig.engine.image_upload_pending(rig.icon);
          if drawn || since >= GIVE_UP {
            waiting = None;
            since = 0;
          }
        }
        None if since >= RELOAD_EVERY => {
          waiting = Some(rig.versions());
          rig.change(looks.get_or_init(Looks::new));
          since = 0;
        }
        None => {}
      }
      rig.engine.step()
    })
  });
}

/// The file the waiting parents import, which cannot be read until opened
const GATE: &str = "gate.ui.ron";
/// Parents taken in, one at a time, after the waiting ones
const ARRIVALS: usize = 50;
/// Pumps with nothing to settle
const STILL_PUMPS: usize = 1000;

/// A source whose `GATE` read waits until it is opened
#[derive(Clone)]
struct Gated {
  files: MemorySource,
  open: Arc<(Mutex<bool>, Condvar)>,
}

impl Gated {
  fn open(&self) {
    let (open, opened) = &*self.open;
    *open.lock().unwrap_or_else(PoisonError::into_inner) = true;
    opened.notify_all();
  }
}

impl AssetSource for Gated {
  fn read(&self, path: &AssetPath) -> Result<Vec<u8>, AssetError> {
    if path.as_str() == GATE {
      let (open, opened) = &*self.open;
      let mut open = open.lock().unwrap_or_else(PoisonError::into_inner);
      while !*open {
        open = opened.wait(open).unwrap_or_else(PoisonError::into_inner);
      }
    }
    self.files.read(path)
  }
}

/// A parent UI file importing the gate
fn parent_source(n: usize) -> String {
  format!(
    "(\n  imports: [\"{GATE}\"],\n  root: Column(children: [Text(text: \"parent {n}\")]),\n)\n"
  )
}

/// Pumps until `count` values wait, or ten seconds pass
fn pump_until_waiting(server: &AssetServer, world: &mut ryo2d::ecs::World, count: usize) {
  let start = Instant::now();
  while server.waiting_count() < count {
    assert!(
      start.elapsed() < Duration::from_secs(10),
      "{count} parents wait (only {})",
      server.waiting_count()
    );
    pump(world);
    std::thread::yield_now();
  }
}

/// The pump's cost with 10, 100 and 1000 parents waiting on a dependency:
/// a pump with nothing to settle, and one taking in another parent
fn pump_waiting() {
  let mut lines = Vec::new();
  for parents in [10, 100, 1000] {
    let files = MemorySource::new();
    files.insert(GATE, "(root: Column(children: []))");
    for n in 0..parents + ARRIVALS {
      files.insert(&format!("p{n}.ui.ron"), parent_source(n));
    }
    let source = Gated {
      files,
      open: Arc::default(),
    };
    let mut world = ryo2d::ecs::World::new();
    let server = AssetServer::new(source.clone());
    server.register_asset::<UiFile>(&mut world);
    server.register_loader(UiFileLoader);
    world.insert_resource(server.clone());
    let mut held: Vec<Handle<UiFile>> = (0..parents)
      .map(|n| server.load::<UiFile>(&format!("p{n}.ui.ron")))
      .collect();
    pump_until_waiting(&server, &mut world, parents);
    let mut still = Vec::with_capacity(STILL_PUMPS);
    for _ in 0..STILL_PUMPS {
      let started = Instant::now();
      pump(&mut world);
      still.push(started.elapsed());
    }
    assert_eq!(server.waiting_count(), parents, "nothing settled");
    // Each new parent's result is in once a pump takes it in
    let mut taking = Vec::with_capacity(ARRIVALS);
    for n in parents..parents + ARRIVALS {
      held.push(server.load::<UiFile>(&format!("p{n}.ui.ron")));
      let start = Instant::now();
      loop {
        let started = Instant::now();
        pump(&mut world);
        let time = started.elapsed();
        if server.waiting_count() > n {
          taking.push(time);
          break;
        }
        assert!(
          start.elapsed() < Duration::from_secs(10),
          "parent {n} waits"
        );
        std::thread::yield_now();
      }
    }
    source.open();
    let ids: Vec<UntypedAssetId> = held.iter().map(|file| file.id().untyped()).collect();
    pump_until_done(&server, &mut world, &ids);
    let (still, _) = median_max(still).unwrap_or_default();
    let (taking, taking_max) = median_max(taking).unwrap_or_default();
    lines.push(format!(
      "{parents} waiting: a still pump {:.2} µs, a pump taking in one more {:.2} µs (max {:.1})",
      us(still),
      us(taking),
      us(taking_max)
    ));
  }
  eprintln!(
    "pump_waiting, medians of {STILL_PUMPS} still pumps and {ARRIVALS} taking in a parent: {}",
    lines.join("; ")
  );
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
  println!("assets bench: scene start, the reload hitch and the waiting pump");
  let results = results_dir();
  let mut criterion = Criterion::default()
    .sample_size(20)
    .output_directory(&results)
    .configure_from_args();
  // Made only for what runs: a profile of `reload_hitch` makes no folder
  let looks = OnceCell::new();
  if guards::runs(&["reload_hitch"]) {
    reload_guard(looks.get_or_init(Looks::new));
  }
  scene_start(&mut criterion, &OnceCell::new());
  reload_bench(&mut criterion, &looks);
  if guards::runs(&["pump_waiting"]) {
    pump_waiting();
  }
  criterion.final_summary();
  for (bench, of) in BENCHES {
    print_median(&results, bench, of, started);
  }
  guards::verdict(
    "assets",
    "Profile the reload (the bench binary is the one `cargo bench --bench assets --no-run` \
     names) with\n  xcrun xctrace record --template 'Time Profiler' --output reload_hitch.trace \
     --launch -- target/release/deps/assets-<hash> --bench --profile-time 20 \
     '^reload_hitch$'\nand charge each sample's time to the nearest ryo2d function from the \
     leaf up; the frames between reloads are the baseline.",
  );
}

/// The median of `bench` (of `of`) from the run criterion just wrote to
/// `results`, in microseconds; nothing for results an earlier run left
/// (this run skipped the bench, as a filter or `--list` does)
fn print_median(results: &Path, bench: &str, of: &str, started: SystemTime) {
  let path = results.join(bench).join("new/estimates.json");
  let written = std::fs::metadata(&path)
    .and_then(|meta| meta.modified())
    .is_ok_and(|modified| modified >= started);
  if !written {
    println!("{bench} median: not measured in this run");
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
    Some(ns) => println!("{bench} median, of {of}: {:.0} us", ns / 1000.0),
    None => println!("{bench} median: unavailable ({})", path.display()),
  }
}
