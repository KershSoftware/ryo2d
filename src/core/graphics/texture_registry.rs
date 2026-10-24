//! Every GPU texture the renderer draws has one `TextureId`.

use std::collections::HashMap;
use std::sync::Arc;

use super::atlas::{ImagePixels, PreparedUpload};
use super::{Image, RenderTarget};
use crate::core::asset::AssetId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct TextureId {
  index: u32,
  generation: u32,
}

pub(crate) enum TextureSource {
  /// An image asset's pixels at one version, taken when a frame drew it
  Image {
    id: AssetId<Image>,
    pixels: Arc<Vec<u8>>,
    /// The levels below the first of its own texture, made by its loader
    /// (kept with the image)
    mips: Option<Arc<Vec<Vec<u8>>>>,
    /// The rest of its upload as its loader made it, until written
    prepared: Option<PreparedUpload>,
    version: u64,
  },
  Pixels(Arc<[u8]>),
  /// A render target: GPU-only (cameras draw into it), never uploaded,
  /// evicted or placed in the atlas
  Target,
}

impl TextureSource {
  /// The texture's pixels (RGBA8, sRGB)
  pub(crate) fn rgba(&self) -> &[u8] {
    match self {
      TextureSource::Image { pixels, .. } => pixels,
      TextureSource::Pixels(data) => data,
      TextureSource::Target => &[],
    }
  }

  /// The mip levels below the first its loader made, if any
  pub(crate) fn mips(&self) -> Option<&[Vec<u8>]> {
    match self {
      TextureSource::Image { mips, .. } => mips.as_deref().map(Vec::as_slice),
      TextureSource::Pixels(_) | TextureSource::Target => None,
    }
  }

  /// The upload its loader made, until written
  pub(crate) fn prepared(&self) -> Option<&PreparedUpload> {
    match self {
      TextureSource::Image { prepared, .. } => prepared.as_ref(),
      TextureSource::Pixels(_) | TextureSource::Target => None,
    }
  }

  /// The upload its loader made, taken (to be written over frames)
  pub(crate) fn take_prepared(&mut self) -> Option<PreparedUpload> {
    match self {
      TextureSource::Image { prepared, .. } => prepared.take(),
      TextureSource::Pixels(_) | TextureSource::Target => None,
    }
  }

  /// Its pixels as the atlas takes them, `width`x`height`
  pub(crate) fn pixels(&self, width: u32, height: u32) -> ImagePixels<'_> {
    ImagePixels {
      width,
      height,
      rgba: self.rgba(),
      mips: self.mips(),
      prepared: self.prepared(),
    }
  }
}

pub(crate) struct TextureEntry {
  pub source: TextureSource,
  pub width: u32,
  pub height: u32,
  /// The image's version (or 0 for pixels) last uploaded: what its
  /// texture shows
  pub uploaded_content: Option<u64>,
  /// The image's version being written into a new texture over frames,
  /// which takes the texture's place once whole
  pub writing: Option<u64>,
  pub last_used: u64,
}

#[derive(Default)]
pub(crate) struct TextureRegistry {
  slots: Vec<(u32, Option<TextureEntry>)>,
  free: Vec<u32>,
  by_image: HashMap<AssetId<Image>, TextureId>,
  by_name: HashMap<String, TextureId>,
  by_target: HashMap<RenderTarget, TextureId>,
}

impl TextureId {
  /// An id for tests that need one without a registry
  #[cfg(test)]
  pub(crate) fn for_test(index: u32) -> Self {
    Self {
      index,
      generation: 0,
    }
  }
}

impl TextureRegistry {
  pub(crate) fn new() -> Self {
    Self::default()
  }

  fn insert(&mut self, entry: TextureEntry) -> TextureId {
    if let Some(index) = self.free.pop() {
      let slot = &mut self.slots[index as usize];
      slot.0 = slot.0.wrapping_add(1);
      slot.1 = Some(entry);
      TextureId {
        index,
        generation: slot.0,
      }
    } else {
      let index = self.slots.len() as u32;
      self.slots.push((0, Some(entry)));
      TextureId {
        index,
        generation: 0,
      }
    }
  }

  /// The texture of image `id`, `image` at `version`, registered on first
  /// use. A new version takes its pixels, size and prepared upload, and is
  /// uploaded again
  pub(crate) fn image(
    &mut self,
    id: AssetId<Image>,
    image: &Image,
    version: u64,
    frame: u64,
  ) -> TextureId {
    if let Some(&texture) = self.by_image.get(&id)
      && let Some(entry) = self.get_mut(texture)
    {
      entry.last_used = frame;
      Self::follow(entry, id, image, version);
      return texture;
    }
    let texture = self.insert(TextureEntry {
      source: Self::image_source(id, image, version),
      width: image.width(),
      height: image.height(),
      uploaded_content: None,
      writing: None,
      last_used: frame,
    });
    self.by_image.insert(id, texture);
    texture
  }

  /// Bring image `id`'s texture, `texture`, to `image` at `version`
  /// without marking it used: a new version, seen before a frame draws
  pub(crate) fn refresh(&mut self, texture: TextureId, image: &Image, version: u64) {
    if let Some(entry) = self.get_mut(texture)
      && let TextureSource::Image { id, .. } = entry.source
    {
      Self::follow(entry, id, image, version);
    }
  }

  /// `entry`, image `id`'s, at `version`
  fn follow(entry: &mut TextureEntry, id: AssetId<Image>, image: &Image, version: u64) {
    if !matches!(entry.source, TextureSource::Image { version: held, .. } if held == version) {
      entry.source = Self::image_source(id, image, version);
      entry.width = image.width();
      entry.height = image.height();
    }
  }

  fn image_source(id: AssetId<Image>, image: &Image, version: u64) -> TextureSource {
    TextureSource::Image {
      id,
      pixels: Arc::clone(image.pixels()),
      mips: image.mips().cloned(),
      prepared: image.take_upload(),
      version,
    }
  }

  /// The texture of image `id`, if it has one
  pub(crate) fn image_texture(&self, id: AssetId<Image>) -> Option<TextureId> {
    self.by_image.get(&id).copied()
  }

  /// The images with a texture, for checking them against the store
  pub(crate) fn images(&self) -> impl Iterator<Item = (AssetId<Image>, TextureId)> + '_ {
    self
      .by_image
      .iter()
      .map(|(&image, &texture)| (image, texture))
  }

  pub(crate) fn pixels(
    &mut self,
    name: &str,
    width: u32,
    height: u32,
    data: &[u8],
    frame: u64,
  ) -> TextureId {
    if let Some(&id) = self.by_name.get(name)
      && let Some(entry) = self.get_mut(id)
    {
      entry.last_used = frame;
      return id;
    }
    let id = self.insert(TextureEntry {
      source: TextureSource::Pixels(Arc::from(data)),
      width,
      height,
      uploaded_content: None,
      writing: None,
      last_used: frame,
    });
    self.by_name.insert(name.to_string(), id);
    id
  }

  pub(crate) fn get(&self, id: TextureId) -> Option<&TextureEntry> {
    let (generation, entry) = self.slots.get(id.index as usize)?;
    (*generation == id.generation)
      .then_some(entry.as_ref())
      .flatten()
  }

  pub(crate) fn get_mut(&mut self, id: TextureId) -> Option<&mut TextureEntry> {
    let (generation, entry) = self.slots.get_mut(id.index as usize)?;
    (*generation == id.generation)
      .then_some(entry.as_mut())
      .flatten()
  }

  fn content(entry: &TextureEntry) -> u64 {
    match &entry.source {
      TextureSource::Image { version, .. } => *version,
      TextureSource::Pixels(_) | TextureSource::Target => 0,
    }
  }

  /// Whether `id`'s pixels are to be written: new, or a new version not
  /// already being written into a new texture
  pub(crate) fn needs_upload(&self, id: TextureId) -> bool {
    self.get(id).is_some_and(|entry| {
      let content = Some(Self::content(entry));
      !matches!(entry.source, TextureSource::Target)
        && entry.uploaded_content != content
        && entry.writing != content
    })
  }

  /// The id of `target`'s texture, now `width`x`height`, registered on
  /// first use.
  pub(crate) fn target(&mut self, target: RenderTarget, width: u32, height: u32) -> TextureId {
    if let Some(&id) = self.by_target.get(&target)
      && let Some(entry) = self.get_mut(id)
    {
      (entry.width, entry.height) = (width, height);
      return id;
    }
    let id = self.insert(TextureEntry {
      source: TextureSource::Target,
      width,
      height,
      uploaded_content: None,
      writing: None,
      last_used: 0,
    });
    self.by_target.insert(target, id);
    id
  }

  /// The id of `target`'s texture, if it is registered.
  pub(crate) fn target_id(&self, target: RenderTarget) -> Option<TextureId> {
    self.by_target.get(&target).copied()
  }

  /// Forget the render targets `keep` rejects, returning their ids.
  pub(crate) fn retain_targets(&mut self, keep: impl Fn(RenderTarget) -> bool) -> Vec<TextureId> {
    let gone: Vec<TextureId> = self
      .by_target
      .iter()
      .filter(|(target, _)| !keep(**target))
      .map(|(_, &id)| id)
      .collect();
    for &id in &gone {
      self.remove(id);
    }
    gone
  }

  /// `id`'s texture shows its pixels now; their prepared upload is freed
  pub(crate) fn mark_uploaded(&mut self, id: TextureId) {
    if let Some(entry) = self.get_mut(id) {
      entry.uploaded_content = Some(Self::content(entry));
      entry.writing = None;
      entry.source.take_prepared();
    }
  }

  /// `id`'s pixels are being written into a new texture over frames
  pub(crate) fn mark_writing(&mut self, id: TextureId) {
    if let Some(entry) = self.get_mut(id) {
      entry.writing = Some(Self::content(entry));
    }
  }

  /// The new texture `id`'s pixels were being written into was dropped
  /// unfinished: they are written at once when next drawn
  pub(crate) fn stop_writing(&mut self, id: TextureId) {
    if let Some(entry) = self.get_mut(id) {
      entry.writing = None;
    }
  }

  /// The new texture `id`'s pixels were written into took the old one's
  /// place: it shows the version written
  pub(crate) fn mark_written(&mut self, id: TextureId) {
    if let Some(entry) = self.get_mut(id)
      && let Some(written) = entry.writing.take()
    {
      entry.uploaded_content = Some(written);
    }
  }

  /// Forget the named pixel textures neither uploaded nor drawn for `ttl`
  /// frames, returning their ids. Images live until their asset is freed,
  /// and render targets until they are removed
  pub(crate) fn prune(&mut self, frame: u64, ttl: u64) -> Vec<TextureId> {
    let mut pruned = Vec::new();
    for (index, (generation, entry)) in self.slots.iter_mut().enumerate() {
      if entry.as_ref().is_some_and(|e| {
        matches!(e.source, TextureSource::Pixels(_)) && frame.saturating_sub(e.last_used) >= ttl
      }) {
        *entry = None;
        pruned.push(TextureId {
          index: index as u32,
          generation: *generation,
        });
        self.free.push(index as u32);
      }
    }
    self.by_name.retain(|_, id| !pruned.contains(id));
    pruned
  }

  /// Forget `id` (its image or name registers afresh next time).
  pub(crate) fn remove(&mut self, id: TextureId) {
    let Some((generation, entry)) = self.slots.get_mut(id.index as usize) else {
      return;
    };
    if *generation != id.generation {
      return;
    }
    let Some(removed) = entry.take() else {
      return;
    };
    self.free.push(id.index);
    if let TextureSource::Image { id: image, .. } = removed.source {
      self.by_image.remove(&image);
    }
    self.by_name.retain(|_, held| *held != id);
    self.by_target.retain(|_, held| *held != id);
  }

  /// Every live texture.
  pub(crate) fn iter(&self) -> impl Iterator<Item = (TextureId, &TextureEntry)> {
    self
      .slots
      .iter()
      .enumerate()
      .filter_map(|(index, (generation, entry))| {
        let id = TextureId {
          index: index as u32,
          generation: *generation,
        };
        entry.as_ref().map(|entry| (id, entry))
      })
  }

  pub(crate) fn len(&self) -> usize {
    self
      .slots
      .iter()
      .filter(|(_, entry)| entry.is_some())
      .count()
  }
}

#[cfg(test)]
mod tests {
  #[test]
  fn render_targets_live_until_removed() {
    let mut registry = TextureRegistry::new();
    let (a, b) = (RenderTarget(1), RenderTarget(2));
    let id = registry.target(a, 16, 8);
    assert_eq!(registry.target(a, 32, 8), id, "one id per target");
    assert_eq!(registry.get(id).map(|e| (e.width, e.height)), Some((32, 8)));
    registry.target(b, 4, 4);
    assert!(!registry.needs_upload(id), "nothing to upload");
    assert!(registry.prune(1_000_000, 1).is_empty(), "never pruned");
    assert_eq!(registry.retain_targets(|target| target == b), vec![id]);
    assert!(registry.get(id).is_none() && registry.target_id(a).is_none());
    assert!(registry.target_id(b).is_some());
  }

  use super::*;
  use crate::core::asset::{Assets, Handle};

  fn image(images: &mut Assets<Image>) -> Handle<Image> {
    images.add(Image::solid_rect(4, 2, 255, 0, 0, 255))
  }

  /// The texture of `handle` as a frame draws it
  fn texture(
    registry: &mut TextureRegistry,
    images: &Assets<Image>,
    handle: &Handle<Image>,
    frame: u64,
  ) -> TextureId {
    let image = images.get(handle).unwrap();
    let version = images.version(handle).unwrap();
    registry.image(handle.id(), image, version, frame)
  }

  #[test]
  fn an_image_has_one_texture() {
    let mut images = Assets::<Image>::default();
    let mut registry = TextureRegistry::new();
    let a = image(&mut images);
    let id = texture(&mut registry, &images, &a, 0);
    assert_eq!(texture(&mut registry, &images, &a.clone(), 1), id);
    let b = images.add(Image::solid_rect(4, 2, 255, 0, 0, 255));
    assert_ne!(
      texture(&mut registry, &images, &b, 1),
      id,
      "another image, even with the same pixels"
    );
    let entry = registry.get(id).unwrap();
    assert_eq!((entry.width, entry.height, entry.last_used), (4, 2, 1));
    assert_eq!(registry.image_texture(a.id()), Some(id));
  }

  #[test]
  fn named_pixels_share_one_id() {
    let mut registry = TextureRegistry::new();
    let data = [255u8; 16];
    let id = registry.pixels("ellipse", 2, 2, &data, 0);
    assert_eq!(registry.pixels("ellipse", 2, 2, &data, 3), id);
    assert_eq!(registry.get(id).unwrap().last_used, 3);
  }

  #[test]
  fn a_changed_image_needs_uploading_again() {
    let mut images = Assets::<Image>::default();
    let mut registry = TextureRegistry::new();
    let handle = image(&mut images);
    let id = texture(&mut registry, &images, &handle, 0);
    assert!(registry.needs_upload(id), "never uploaded");
    registry.mark_uploaded(id);
    assert!(!registry.needs_upload(id));
    *images.get_mut(&handle).unwrap() = Image::solid_rect(8, 8, 0, 255, 0, 255);
    texture(&mut registry, &images, &handle, 1);
    assert!(
      registry.needs_upload(id),
      "the new version is uploaded again"
    );
    assert_eq!(
      registry.get(id).unwrap().width,
      8,
      "the size follows the new content"
    );
  }

  #[test]
  fn a_removed_texture_is_forgotten_by_image_and_name() {
    let mut images = Assets::<Image>::default();
    let mut registry = TextureRegistry::new();
    let handle = image(&mut images);
    let image_id = texture(&mut registry, &images, &handle, 0);
    let pixels_id = registry.pixels("dot", 1, 1, &[1, 2, 3, 4], 0);
    registry.remove(image_id);
    registry.remove(pixels_id);
    assert!(registry.get(image_id).is_none() && registry.get(pixels_id).is_none());
    assert_eq!(registry.iter().count(), 0);
    assert_eq!(registry.image_texture(handle.id()), None);
    assert_ne!(
      texture(&mut registry, &images, &handle, 1),
      image_id,
      "registered afresh"
    );
    assert_ne!(registry.pixels("dot", 1, 1, &[1, 2, 3, 4], 1), pixels_id);
  }

  #[test]
  fn a_source_reads_its_pixels() {
    let mut images = Assets::<Image>::default();
    let mut registry = TextureRegistry::new();
    let id = registry.pixels("dot", 1, 1, &[1, 2, 3, 4], 0);
    assert_eq!(registry.get(id).unwrap().source.rgba(), &[1, 2, 3, 4]);
    let red = image(&mut images);
    let id = texture(&mut registry, &images, &red, 0);
    let rgba = registry.get(id).unwrap().source.rgba();
    assert_eq!((rgba.len(), &rgba[..4]), (4 * 2 * 4, &[255, 0, 0, 255][..]));
  }

  #[test]
  fn images_outlive_the_ttl_and_pruned_pixels_are_never_valid_again() {
    let mut images = Assets::<Image>::default();
    let mut registry = TextureRegistry::new();
    let old = registry.pixels("old", 1, 1, &[1, 2, 3, 4], 0);
    let kept = registry.pixels("kept", 1, 1, &[1, 2, 3, 4], 0);
    let held = image(&mut images);
    let image_id = texture(&mut registry, &images, &held, 0);
    registry.get_mut(kept).unwrap().last_used = 10;
    let pruned = registry.prune(10, 5);
    assert_eq!(pruned, vec![old]);
    assert!(registry.get(old).is_none());
    assert!(registry.get(kept).is_some());
    assert!(
      registry.get(image_id).is_some(),
      "an image waits for its asset to go"
    );
    let reused = registry.pixels("new", 1, 1, &[1, 2, 3, 4], 11);
    assert_ne!(reused, old, "a reused slot gets a new generation");
    assert!(registry.get(old).is_none());
    assert_eq!(registry.len(), 3);
  }
}
