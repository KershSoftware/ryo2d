//! Fonts from `.ttf`, `.otf` and `.ttc` files

use crate::core::asset::{AssetError, AssetLoader, LoadContext};
use crate::core::text::Font;

/// Which face of a font file to load: a collection (`.ttc`) holds several,
/// and part of the asset's key, so two faces of one file are two fonts:
///
/// ```
/// use ryo2d::prelude::*;
///
/// fn load_bold(ctx: &mut SceneContext<'_>) -> Option<Handle<Font>> {
///   let face = FontSettings { face: 1 };
///   ctx.assets().load_now_with::<Font, _>("fonts/Family.ttc", face).ok()
/// }
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct FontSettings {
  /// The face's index in its file (0 for a single font)
  pub face: u32,
}

/// Loads [`Font`]s from TrueType and OpenType files (`.ttf`, `.otf`) and
/// collections (`.ttc`, one face per [`FontSettings`]). Every registry has it
#[derive(Debug, Default, Clone, Copy)]
pub struct FontLoader;

impl AssetLoader for FontLoader {
  type Asset = Font;
  type Settings = FontSettings;

  fn extensions(&self) -> &[&'static str] {
    &["ttf", "otf", "ttc"]
  }

  fn load(
    &self,
    bytes: Vec<u8>,
    settings: &FontSettings,
    cx: &mut LoadContext<'_>,
  ) -> Result<Font, AssetError> {
    Font::from_bytes_face(bytes, settings.face)
      .map_err(|error| AssetError::decode(cx.path(), error))
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::asset::{
    AssetError, AssetServer, Assets, AssetsView, MemorySource, insert_registry,
  };
  use crate::core::ecs::kernel::World;
  use crate::core::text::Font;

  const INTER: &[u8] = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");
  const MONO: &[u8] = include_bytes!("../../../assets/fonts/JetBrainsMono-Regular.ttf");

  /// An engine's registry over `files`
  fn registry_over(files: MemorySource) -> World {
    let mut world = World::new();
    insert_registry(&mut world, AssetServer::new(files));
    world
  }

  /// A font collection of `faces` in order: each face's file follows the
  /// collection's header, its table offsets moved to where its tables now sit
  fn collection(faces: &[&[u8]]) -> Vec<u8> {
    let header = 12 + 4 * faces.len();
    let mut file = b"ttcf".to_vec();
    file.extend(0x0001_0000u32.to_be_bytes());
    file.extend((faces.len() as u32).to_be_bytes());
    file.resize(header, 0);
    for (n, face) in faces.iter().enumerate() {
      while !file.len().is_multiple_of(4) {
        file.push(0);
      }
      let base = file.len() as u32;
      file[12 + 4 * n..16 + 4 * n].copy_from_slice(&base.to_be_bytes());
      let mut moved = face.to_vec();
      let tables = u16::from_be_bytes([face[4], face[5]]) as usize;
      for table in 0..tables {
        let at = 12 + 16 * table + 8;
        let offset = u32::from_be_bytes([face[at], face[at + 1], face[at + 2], face[at + 3]]);
        moved[at..at + 4].copy_from_slice(&(offset + base).to_be_bytes());
      }
      file.extend(moved);
    }
    file
  }

  fn load(world: &mut World, path: &str, face: u32) -> Result<Font, std::sync::Arc<AssetError>> {
    let handle = AssetsView::new(world).load_now_with::<Font, _>(path, FontSettings { face })?;
    match world.resource::<Assets<Font>>().get(&handle) {
      Some(font) => Ok(font.clone()),
      None => panic!("{path} is loaded but not in its store"),
    }
  }

  #[test]
  fn a_ttf_loads_with_its_family() {
    let files = MemorySource::new();
    files.insert("fonts/mono.ttf", MONO);
    let mut world = registry_over(files);
    let handle = match AssetsView::new(&mut world).load_now::<Font>("fonts/mono.ttf") {
      Ok(handle) => handle,
      Err(error) => panic!("mono.ttf: {}", error.report()),
    };
    let family = world
      .resource::<Assets<Font>>()
      .get(&handle)
      .map(Font::family);
    assert_eq!(family, Some("JetBrains Mono"));
  }

  #[test]
  fn a_ttc_loads_the_face_its_settings_name() {
    let files = MemorySource::new();
    files.insert("fonts/pair.ttc", collection(&[INTER, MONO]));
    let mut world = registry_over(files);
    let first = load(&mut world, "fonts/pair.ttc", 0).map(|font| font.family().to_string());
    let second = load(&mut world, "fonts/pair.ttc", 1).map(|font| font.family().to_string());
    assert_eq!(first.ok().as_deref(), Some("Inter"), "face 0, the default");
    assert_eq!(second.ok().as_deref(), Some("JetBrains Mono"), "face 1");
    let third = load(&mut world, "fonts/pair.ttc", 2);
    assert!(
      matches!(
        third.as_ref().map_err(|error| &**error),
        Err(AssetError::Decode { .. })
      ),
      "a collection of two has no face 2: {third:?}"
    );
  }

  #[test]
  fn a_corrupt_font_file_does_not_decode() {
    let files = MemorySource::new();
    // A TrueType signature, then nothing a font needs
    files.insert("bad.ttf", vec![0u8, 1, 0, 0, 0, 7, 1, 2, 3]);
    let mut world = registry_over(files);
    let error = match AssetsView::new(&mut world).load_now::<Font>("bad.ttf") {
      Ok(_) => panic!("a corrupt file loads"),
      Err(error) => error,
    };
    assert!(
      matches!(*error, AssetError::Decode { .. }),
      "{}",
      error.report()
    );
  }
}
