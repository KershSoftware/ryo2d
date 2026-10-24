//! What the pages load, and what the registry says of each asset

use std::sync::Arc;

use demo_kit::prelude::*;

/// Every image in the demo's folder, the largest files first
const IMAGES: [&str; 10] = [
  "back_decor.png",
  "bg.png",
  "front_decor.png",
  "battleground.png",
  "back_land.png",
  "ground_decor.png",
  "bg-ground.png",
  "ground.png",
  "pipe_cap.png",
  "pipe_body.png",
];

/// The fonts in the demo's folder
const FONTS: [&str; 2] = ["fonts/PressStart2P-Regular.ttf", "fonts/m5x7.ttf"];

/// The UI file in the demo's folder, which the Hot reload page spawns
pub(crate) const UI_FILE: &str = "ui/card.ui.ron";

/// Not in the folder, so it fails to load
const MISSING: &str = "missing.png";

/// The asset types the demo loads
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
  Image,
  Font,
  UiFile,
}

impl Kind {
  pub(crate) fn label(self) -> &'static str {
    match self {
      Kind::Image => "Image",
      Kind::Font => "Font",
      Kind::UiFile => "UI file",
    }
  }
}

/// Each asset the pages load, by asset path (relative to `assets/`), in the
/// order their tables list them: the images, the fonts, the UI file and the
/// missing image
pub(crate) fn listed() -> impl Iterator<Item = (&'static str, Kind)> {
  let images = IMAGES.into_iter().map(|path| (path, Kind::Image));
  let fonts = FONTS.into_iter().map(|path| (path, Kind::Font));
  images
    .chain(fonts)
    .chain([(UI_FILE, Kind::UiFile), (MISSING, Kind::Image)])
}

/// A handle to one of the demo's assets: it keeps the asset loaded
pub(crate) enum Held {
  Image(Handle<Image>),
  Font(Handle<Font>),
  UiFile(Handle<UiFile>),
}

impl Held {
  /// Asks for `path` on the worker threads; returns at once
  pub(crate) fn load(assets: &AssetsView, path: &str, kind: Kind) -> Held {
    match kind {
      Kind::Image => Held::Image(assets.load(path)),
      Kind::Font => Held::Font(assets.load(path)),
      Kind::UiFile => Held::UiFile(assets.load(path)),
    }
  }

  /// Loads `path` at once, or gives why it does not load
  pub(crate) fn load_now(
    assets: &mut AssetsView,
    path: &str,
    kind: Kind,
  ) -> Result<Held, Arc<AssetError>> {
    Ok(match kind {
      Kind::Image => Held::Image(assets.load_now(path)?),
      Kind::Font => Held::Font(assets.load_now(path)?),
      Kind::UiFile => Held::UiFile(assets.load_now(path)?),
    })
  }

  pub(crate) fn id(&self) -> UntypedAssetId {
    match self {
      Held::Image(image) => image.id().untyped(),
      Held::Font(font) => font.id().untyped(),
      Held::UiFile(file) => file.id().untyped(),
    }
  }

  pub(crate) fn state(&self, assets: &AssetsView) -> LoadState {
    match self {
      Held::Image(image) => assets.state(image),
      Held::Font(font) => assets.state(font),
      Held::UiFile(file) => assets.state(file),
    }
  }

  /// What the loaded asset is: an image's size, a font's family; empty
  /// while it is not loaded, and for a UI file
  pub(crate) fn summary(&self, assets: &AssetsView) -> String {
    match self {
      Held::Image(image) => assets
        .get(image)
        .map(|image| format!("{}×{}", image.width(), image.height()))
        .unwrap_or_default(),
      Held::Font(font) => assets
        .get(font)
        .map(|font| font.family().to_string())
        .unwrap_or_default(),
      Held::UiFile(_) => String::new(),
    }
  }
}

/// An error and its causes on one line
pub(crate) fn explained(error: &dyn std::error::Error) -> String {
  let mut text = error.to_string();
  let mut cause = error.source();
  while let Some(next) = cause {
    text.push_str(": ");
    text.push_str(&next.to_string());
    cause = next.source();
  }
  text
}
