//! What the UI sizes images by: their sizes and content versions, kept apart
//! from the world's image store.

use glam::Vec2;

use crate::core::asset::assets::follower_token;
use crate::core::asset::{AssetId, Assets};
use crate::core::ecs::World;
use crate::core::ecs::kernel::DetectChangesMut;
use crate::core::graphics::Image;

/// The size and content version of every image in a world's
/// `Assets<Image>`, by slot. The engine and `UiHarness` keep it outside the
/// world and bring it in line at the start of each UI pass (`refresh`), so
/// the pass never takes the store from the world: a scene's `ui` reads,
/// adds and loads images as anywhere else. An image added during the pass
/// is sized from the next one.
pub(crate) struct ImageSizes {
  /// By slot index
  slots: Vec<Option<LiveImage>>,
  /// The store followed (`Assets::serial`)
  store: Option<u64>,
  /// Its token as the store's follower (`follower_token`)
  follower: u64,
}

impl Default for ImageSizes {
  fn default() -> Self {
    Self {
      slots: Vec::new(),
      store: None,
      follower: follower_token(),
    }
  }
}

/// One live image, as the UI reads it
#[derive(Clone, Copy)]
struct LiveImage {
  generation: u32,
  version: u64,
  size: Vec2,
}

#[cfg(test)]
static NO_IMAGES: ImageSizes = ImageSizes {
  slots: Vec::new(),
  store: None,
  follower: 0,
};

impl ImageSizes {
  /// A table with no images: every image reads as not loaded
  #[cfg(test)]
  pub(crate) fn none() -> &'static ImageSizes {
    &NO_IMAGES
  }

  /// A table of `images` as they are now (for tests that draw without a world)
  #[cfg(test)]
  pub(crate) fn of(images: &Assets<Image>) -> ImageSizes {
    let mut sizes = ImageSizes::default();
    for (id, image) in images.iter() {
      let version = images.version(id).unwrap_or(0);
      sizes.set(id.index(), Some((id.generation(), version, image)));
    }
    sizes
  }

  /// Brings the table in line with `world`'s image store as it is now. It
  /// costs what changed since the last refresh: nothing when nothing did
  pub(crate) fn refresh(&mut self, world: &mut World) {
    match world.get_resource_mut::<Assets<Image>>() {
      // Following the store changes nothing a reader of the store can see
      Some(mut images) => self.follow(images.bypass_change_detection()),
      None => {
        self.slots.clear();
        self.store = None;
      }
    }
  }

  /// Brings the table in line with `images`: the slots it changed since the
  /// last call, or all of them when it is not the store followed before
  fn follow(&mut self, images: &mut Assets<Image>) {
    let whole = self.store != Some(images.serial()) || !images.is_followed_by(self.follower);
    if whole {
      self.slots.clear();
      self.store = Some(images.serial());
    }
    images.take_changes(self.follower, whole, |index, now| {
      self.set(
        index,
        now.map(|now| (now.generation, now.version, now.value)),
      );
    });
  }

  /// Slot `index` holds `now` (generation, version, image), or nothing
  fn set(&mut self, index: u32, now: Option<(u32, u64, &Image)>) {
    let at = index as usize;
    if self.slots.len() <= at {
      if now.is_none() {
        return;
      }
      self.slots.resize(at + 1, None);
    }
    if let Some(slot) = self.slots.get_mut(at) {
      *slot = now.map(|(generation, version, image)| LiveImage {
        generation,
        version,
        size: Vec2::new(image.width() as f32, image.height() as f32),
      });
    }
  }

  fn get(&self, id: AssetId<Image>) -> Option<&LiveImage> {
    self
      .slots
      .get(id.index() as usize)?
      .as_ref()
      .filter(|slot| slot.generation == id.generation())
  }

  /// The image's size in texels, if it is loaded
  pub(crate) fn size(&self, id: impl Into<AssetId<Image>>) -> Option<Vec2> {
    self.get(id.into()).map(|slot| slot.size)
  }

  /// The image's content version (`Assets::version`), if it is loaded
  pub(crate) fn version(&self, id: impl Into<AssetId<Image>>) -> Option<u64> {
    self.get(id.into()).map(|slot| slot.version)
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::asset::Handle;

  fn solid(width: u32, height: u32) -> Image {
    Image::solid_rect(width, height, 255, 255, 255, 255)
  }

  /// A world with an image store, and its table brought in line
  fn world() -> (World, ImageSizes) {
    let mut world = World::new();
    world.insert_resource(Assets::<Image>::default());
    let mut sizes = ImageSizes::default();
    sizes.refresh(&mut world);
    (world, sizes)
  }

  fn add(world: &mut World, image: Image) -> Handle<Image> {
    world.resource_mut::<Assets<Image>>().add(image)
  }

  /// The table reads as the store does, for `ids`
  fn agrees(world: &World, sizes: &ImageSizes, ids: &[AssetId<Image>]) {
    let store = world.resource::<Assets<Image>>();
    for &id in ids {
      let size = store
        .get(id)
        .map(|image| Vec2::new(image.width() as f32, image.height() as f32));
      assert_eq!(sizes.size(id), size, "{id:?}");
      assert_eq!(sizes.version(id), store.version(id), "{id:?}");
    }
  }

  #[test]
  fn a_refresh_reads_what_was_added_changed_and_freed() {
    let (mut world, mut sizes) = world();
    let a = add(&mut world, solid(4, 2));
    let b = add(&mut world, solid(8, 8));
    sizes.refresh(&mut world);
    assert_eq!(sizes.size(&a), Some(Vec2::new(4.0, 2.0)));
    agrees(&world, &sizes, &[a.id(), b.id()]);

    *world.resource_mut::<Assets<Image>>().get_mut(&a).unwrap() = solid(16, 2);
    let gone = b.id();
    drop(b);
    world
      .resource_mut::<Assets<Image>>()
      .free_unheld(&mut Vec::new());
    sizes.refresh(&mut world);
    assert_eq!(sizes.size(&a), Some(Vec2::new(16.0, 2.0)));
    assert_eq!(sizes.size(gone), None, "freed");
    agrees(&world, &sizes, &[a.id(), gone]);

    // The freed slot's next image is a new generation: the old id stays unloaded
    let c = add(&mut world, solid(3, 3));
    assert_eq!(c.id().index(), gone.index());
    sizes.refresh(&mut world);
    assert_eq!(sizes.size(gone), None);
    assert_eq!(sizes.size(&c), Some(Vec2::new(3.0, 3.0)));
  }

  #[test]
  fn an_image_changed_after_a_refresh_reads_as_before_until_the_next() {
    let (mut world, mut sizes) = world();
    let a = add(&mut world, solid(4, 4));
    sizes.refresh(&mut world);
    let version = sizes.version(&a);
    *world.resource_mut::<Assets<Image>>().get_mut(&a).unwrap() = solid(9, 4);
    assert_eq!(sizes.size(&a), Some(Vec2::new(4.0, 4.0)));
    assert_eq!(sizes.version(&a), version);
    sizes.refresh(&mut world);
    assert_eq!(sizes.size(&a), Some(Vec2::new(9.0, 4.0)));
    assert_ne!(sizes.version(&a), version);
  }

  #[test]
  fn a_replaced_store_is_read_whole() {
    let (mut world, mut sizes) = world();
    let old = add(&mut world, solid(4, 4));
    sizes.refresh(&mut world);
    // A new store: its first image takes the same id as the old store's
    world.insert_resource(Assets::<Image>::default());
    let new = add(&mut world, solid(2, 6));
    assert_eq!(new.id(), old.id());
    sizes.refresh(&mut world);
    assert_eq!(sizes.size(&new), Some(Vec2::new(2.0, 6.0)));
    world.remove_resource::<Assets<Image>>();
    sizes.refresh(&mut world);
    assert_eq!(sizes.size(&new), None, "no store: no images");
  }

  #[test]
  fn an_idle_refresh_leaves_the_store_unchanged_for_its_readers() {
    let (mut world, mut sizes) = world();
    add(&mut world, solid(4, 4));
    sizes.refresh(&mut world);
    world.clear_trackers();
    sizes.refresh(&mut world);
    assert!(!world.is_resource_changed::<Assets<Image>>());
  }
}
