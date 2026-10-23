//! Shaders from `.wgsl` files

use crate::core::asset::{AssetError, AssetLoader, LoadContext};
use crate::core::ecs::Shader;

/// Loads [`Shader`]s from `.wgsl` files: the file's text, named by its asset
/// path, which its errors are reported under. Every registry has it
#[derive(Debug, Default, Clone, Copy)]
pub struct ShaderLoader;

impl AssetLoader for ShaderLoader {
  type Asset = Shader;
  type Settings = ();

  fn extensions(&self) -> &[&'static str] {
    &["wgsl"]
  }

  fn load(&self, bytes: Vec<u8>, _: &(), cx: &mut LoadContext<'_>) -> Result<Shader, AssetError> {
    let path = cx.path();
    let source = String::from_utf8(bytes).map_err(|error| AssetError::decode(path, error))?;
    Ok(Shader::from_wgsl(path.as_str(), source))
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::asset::{AssetServer, Assets, AssetsView, MemorySource, insert_registry};
  use crate::core::ecs::kernel::World;

  const GLOW: &str =
    "fn sprite(in: SpriteIn, params: Params) -> vec4<f32> { return sample(in.uv) * 2.0; }";

  /// An engine's registry over `files`
  fn registry_over(files: MemorySource) -> World {
    let mut world = World::new();
    insert_registry(&mut world, AssetServer::new(files));
    world
  }

  #[test]
  fn a_loaded_shader_takes_its_path_as_name() {
    let files = MemorySource::new();
    files.insert("shaders/glow.wgsl", GLOW);
    let mut world = registry_over(files);
    let handle = match AssetsView::new(&mut world).load_now::<Shader>("shaders/./glow.wgsl") {
      Ok(handle) => handle,
      Err(error) => panic!("glow.wgsl: {}", error.report()),
    };
    let Some(shader) = world.resource::<Assets<Shader>>().get(&handle) else {
      panic!("glow.wgsl is loaded but not in its store");
    };
    assert_eq!(shader.name(), "shaders/glow.wgsl");
    assert_eq!(shader.source(), GLOW);
  }

  #[test]
  fn a_shader_file_that_is_not_text_does_not_decode() {
    let files = MemorySource::new();
    files.insert("bad.wgsl", vec![0xff, 0xfe, 0x00]);
    let mut world = registry_over(files);
    let error = match AssetsView::new(&mut world).load_now::<Shader>("bad.wgsl") {
      Ok(_) => panic!("bytes that are not UTF-8 load"),
      Err(error) => error,
    };
    assert!(
      matches!(*error, AssetError::Decode { .. }),
      "{}",
      error.report()
    );
  }
}
