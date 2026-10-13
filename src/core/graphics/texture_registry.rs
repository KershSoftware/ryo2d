//! Every GPU texture the renderer draws has one `TextureId`.

use std::collections::HashMap;
use std::sync::Arc;

use super::RenderTarget;
use crate::core::asset::SpriteHandle;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct TextureId {
  index: u32,
  generation: u32,
}

pub(crate) enum TextureSource {
  Sprite(SpriteHandle),
  Pixels(Arc<[u8]>),
  /// A render target: GPU-only (cameras draw into it), never uploaded,
  /// evicted or placed in the atlas
  Target,
}

impl TextureSource {
  /// The texture's pixels (RGBA8, sRGB), read from the sprite if it is one
  pub(crate) fn rgba(&self) -> Vec<u8> {
    match self {
      TextureSource::Sprite(sprite) => sprite.with_sprite(|s| s.data().to_vec()),
      TextureSource::Pixels(data) => data.to_vec(),
      TextureSource::Target => Vec::new(),
    }
  }
}

pub(crate) struct TextureEntry {
  pub source: TextureSource,
  pub width: u32,
  pub height: u32,
  /// The sprite content generation (or 0 for pixels) last uploaded
  pub uploaded_content: Option<u64>,
  pub last_used: u64,
}

#[derive(Default)]
pub(crate) struct TextureRegistry {
  slots: Vec<(u32, Option<TextureEntry>)>,
  free: Vec<u32>,
  by_sprite: HashMap<usize, TextureId>,
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

  pub(crate) fn sprite(&mut self, sprite: &SpriteHandle, frame: u64) -> TextureId {
    if let Some(&id) = self.by_sprite.get(&sprite.serial())
      && let Some(entry) = self.get_mut(id)
    {
      entry.last_used = frame;
      if entry.uploaded_content != Some(sprite.generation()) {
        let (width, height) = sprite.with_sprite(|s| (s.width(), s.height()));
        entry.width = width;
        entry.height = height;
      }
      return id;
    }
    let (width, height) = sprite.with_sprite(|s| (s.width(), s.height()));
    let id = self.insert(TextureEntry {
      source: TextureSource::Sprite(sprite.clone()),
      width,
      height,
      uploaded_content: None,
      last_used: frame,
    });
    self.by_sprite.insert(sprite.serial(), id);
    id
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
      TextureSource::Sprite(sprite) => sprite.generation(),
      TextureSource::Pixels(_) | TextureSource::Target => 0,
    }
  }

  pub(crate) fn needs_upload(&self, id: TextureId) -> bool {
    self.get(id).is_some_and(|entry| {
      !matches!(entry.source, TextureSource::Target)
        && entry.uploaded_content != Some(Self::content(entry))
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

  pub(crate) fn mark_uploaded(&mut self, id: TextureId) {
    if let Some(entry) = self.get_mut(id) {
      entry.uploaded_content = Some(Self::content(entry));
    }
  }

  pub(crate) fn prune(&mut self, frame: u64, ttl: u64) -> Vec<TextureId> {
    let mut pruned = Vec::new();
    for (index, (generation, entry)) in self.slots.iter_mut().enumerate() {
      // Render targets live until they are removed
      if entry.as_ref().is_some_and(|e| {
        !matches!(e.source, TextureSource::Target) && frame.saturating_sub(e.last_used) >= ttl
      }) {
        *entry = None;
        pruned.push(TextureId {
          index: index as u32,
          generation: *generation,
        });
        self.free.push(index as u32);
      }
    }
    self.by_sprite.retain(|_, id| !pruned.contains(id));
    self.by_name.retain(|_, id| !pruned.contains(id));
    pruned
  }

  /// Forget `id` (its handle or name registers afresh next time).
  pub(crate) fn remove(&mut self, id: TextureId) {
    let Some((generation, entry)) = self.slots.get_mut(id.index as usize) else {
      return;
    };
    if *generation != id.generation || entry.take().is_none() {
      return;
    }
    self.free.push(id.index);
    self.by_sprite.retain(|_, held| *held != id);
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
  use crate::core::graphics::Sprite;

  fn sprite(name: &str) -> SpriteHandle {
    SpriteHandle::from_sprite(Sprite::solid_rect(4, 2, 255, 0, 0, 255), name)
  }

  #[test]
  fn a_handle_and_its_clones_share_one_id() {
    let mut registry = TextureRegistry::new();
    let a = sprite("a");
    let id = registry.sprite(&a, 0);
    assert_eq!(registry.sprite(&a.clone(), 1), id);
    assert_ne!(
      registry.sprite(&sprite("a"), 1),
      id,
      "a different handle, even with the same name"
    );
    let entry = registry.get(id).unwrap();
    assert_eq!((entry.width, entry.height, entry.last_used), (4, 2, 1));
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
  fn a_changed_sprite_needs_uploading_again() {
    let mut registry = TextureRegistry::new();
    let handle = sprite("hot");
    let id = registry.sprite(&handle, 0);
    assert!(registry.needs_upload(id), "never uploaded");
    registry.mark_uploaded(id);
    assert!(!registry.needs_upload(id));
    handle.update(Sprite::solid_rect(8, 8, 0, 255, 0, 255));
    assert!(
      registry.needs_upload(id),
      "hot reload bumped its generation"
    );
    registry.sprite(&handle, 1);
    assert_eq!(
      registry.get(id).unwrap().width,
      8,
      "the size follows the new content"
    );
  }

  #[test]
  fn a_removed_texture_is_forgotten_by_handle_and_name() {
    let mut registry = TextureRegistry::new();
    let handle = sprite("gone");
    let sprite_id = registry.sprite(&handle, 0);
    let pixels_id = registry.pixels("dot", 1, 1, &[1, 2, 3, 4], 0);
    registry.remove(sprite_id);
    registry.remove(pixels_id);
    assert!(registry.get(sprite_id).is_none() && registry.get(pixels_id).is_none());
    assert_eq!(registry.iter().count(), 0);
    assert_ne!(registry.sprite(&handle, 1), sprite_id, "registered afresh");
    assert_ne!(registry.pixels("dot", 1, 1, &[1, 2, 3, 4], 1), pixels_id);
  }

  #[test]
  fn a_source_reads_its_pixels() {
    let mut registry = TextureRegistry::new();
    let id = registry.pixels("dot", 1, 1, &[1, 2, 3, 4], 0);
    assert_eq!(registry.get(id).unwrap().source.rgba(), vec![1, 2, 3, 4]);
    let id = registry.sprite(&sprite("red"), 0);
    let rgba = registry.get(id).unwrap().source.rgba();
    assert_eq!((rgba.len(), &rgba[..4]), (4 * 2 * 4, &[255, 0, 0, 255][..]));
  }

  #[test]
  fn a_pruned_id_is_never_valid_again() {
    let mut registry = TextureRegistry::new();
    let old = registry.sprite(&sprite("old"), 0);
    let kept = registry.sprite(&sprite("kept"), 0);
    registry.sprite(&sprite("kept-again"), 0);
    registry.get_mut(kept).unwrap().last_used = 10;
    let pruned = registry.prune(10, 5);
    assert!(pruned.contains(&old));
    assert!(registry.get(old).is_none());
    assert!(registry.get(kept).is_some());
    let reused = registry.sprite(&sprite("new"), 11);
    assert_ne!(reused, old, "a reused slot gets a new generation");
    assert!(registry.get(old).is_none());
    assert_eq!(registry.len(), 2);
  }
}
