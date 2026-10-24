//! Shaders from `.wgsl` files

use crate::core::asset::{AssetError, AssetLoader, LoadContext};
use crate::core::ecs::Shader;

/// Loads [`Shader`]s from `.wgsl` files: the file's text, named by its asset
/// path, which its errors are reported under. The text is compiled as the
/// renderer will compile it (a sprite shader if it defines `fn sprite`, an
/// effect if it defines `fn effect`; naga parses and validates it), and the
/// renderer takes that compile rather than make its own. So a file that
/// does not compile fails its load with `AssetError::Decode`, at its line
/// and column: a first load is `Failed`, and a reload is `ReloadFailed` and
/// keeps the last good shader.
///
/// A `Shader` is a sprite shader or an effect, so a file that defines
/// neither `fn sprite` nor `fn effect` fails its load. WGSL for a render
/// pass of your own is not a `Shader`: give it an asset type of your own
/// with a loader that takes `wgsl` (loaders are picked by the type asked
/// for, then the extension), so it loads and reloads from `assets/` too,
/// or embed it with `include_str!`. Every registry has it
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
    // Compiled here, off the main thread, and kept with the shader for the
    // renderer. The error names the file already: the shader's own report
    // gives the place in it
    let shader = Shader::from_wgsl(path.as_str(), source);
    shader
      .check()
      .map_err(|error| AssetError::decode(path, error.located()))?;
    Ok(shader)
  }
}

#[cfg(test)]
mod tests {
  use std::time::{Duration, Instant};

  use super::*;
  use crate::core::asset::{
    AssetEvent, AssetServer, Assets, AssetsView, LoadState, MemorySource, insert_registry, pump,
  };
  use crate::core::dev_log::{self, LogLevel};
  use crate::core::ecs::kernel::{Messages, World};

  const GLOW: &str =
    "fn sprite(in: SpriteIn, params: Params) -> vec4<f32> { return sample(in.uv) * 2.0; }";

  /// A sprite shader with a comma missing on its second line
  const BROKEN: &str = "fn sprite(in: SpriteIn, params: Params) -> vec4<f32> {
  return vec4<f32>(1.0, 0.0 0.0, 1.0);
}
";

  /// The `Failed` state's report once `path` has loaded on a worker, or
  /// why it did not fail
  fn failure(files: MemorySource, path: &str) -> Result<String, String> {
    let mut world = registry_over(files);
    let handle = AssetsView::new(&mut world).load::<Shader>(path);
    let start = Instant::now();
    loop {
      pump(&mut world);
      match world.resource::<AssetServer>().state(&handle) {
        LoadState::Failed(error) => return Ok(error.report()),
        LoadState::Loaded => return Err(format!("{path} loaded")),
        _ if start.elapsed() > Duration::from_secs(5) => return Err("timed out".into()),
        _ => std::thread::sleep(Duration::from_millis(1)),
      }
    }
  }

  #[test]
  fn a_shader_that_does_not_compile_fails_its_load_at_its_line_and_column() {
    let files = MemorySource::new();
    files.insert("shaders/broken.wgsl", BROKEN);
    assert_eq!(
      failure(files, "shaders/broken.wgsl"),
      Ok("could not load \"shaders/broken.wgsl\": 2:29: expected `)`, found \"0.0\"".into())
    );
  }

  #[test]
  fn an_effect_is_checked_as_an_effect() {
    let files = MemorySource::new();
    let good = "fn effect(in: EffectIn, params: Params) -> vec4<f32> { return scene(in.uv); }";
    files.insert("fx/good.wgsl", good);
    files.insert(
      "fx/broken.wgsl",
      "fn effect(in: EffectIn, params: Params) -> vec4<f32> {\n  return scene(in.uv) * nope;\n}\n",
    );
    let mut world = registry_over(files.clone());
    let loaded = AssetsView::new(&mut world).load_now::<Shader>("fx/good.wgsl");
    assert!(loaded.is_ok(), "{:?}", loaded.err().map(|e| e.report()));
    assert_eq!(
      failure(files, "fx/broken.wgsl"),
      Ok(
        "could not load \"fx/broken.wgsl\": 2:25: no definition in scope for identifier: `nope`"
          .into()
      )
    );
  }

  #[test]
  fn a_file_with_neither_function_fails() {
    let files = MemorySource::new();
    files.insert("shaders/none.wgsl", "fn helper() -> f32 { return 1.0; }");
    assert_eq!(
      failure(files, "shaders/none.wgsl"),
      Ok(
        "could not load \"shaders/none.wgsl\": no `sprite` or `effect` function; expected \
         `fn sprite(in: SpriteIn, params: Params) -> vec4<f32>` or \
         `fn effect(in: EffectIn, params: Params) -> vec4<f32>`"
          .into()
      )
    );
  }

  #[test]
  fn a_save_that_does_not_compile_fails_its_reload_and_keeps_the_last_good_shader() {
    let files = MemorySource::new();
    files.insert("shaders/kept.wgsl", GLOW);
    let mut world = registry_over(files.clone());
    world.resource::<AssetServer>().start_watching();
    let handle = match AssetsView::new(&mut world).load_now::<Shader>("shaders/kept.wgsl") {
      Ok(handle) => handle,
      Err(error) => panic!("kept.wgsl: {}", error.report()),
    };
    let mut events = world
      .resource::<Messages<AssetEvent<Shader>>>()
      .get_cursor_current();
    files.insert("shaders/kept.wgsl", BROKEN);
    let mut sent = Vec::new();
    let start = Instant::now();
    while !sent
      .iter()
      .any(|e| matches!(e, AssetEvent::ReloadFailed { .. }))
    {
      assert!(start.elapsed() < Duration::from_secs(5), "{sent:?}");
      std::thread::sleep(Duration::from_millis(1));
      pump(&mut world);
      sent.extend(
        events
          .read(world.resource::<Messages<AssetEvent<Shader>>>())
          .cloned(),
      );
    }
    assert!(
      matches!(sent.as_slice(), [AssetEvent::ReloadFailed { id, .. }] if *id == handle.id()),
      "ReloadFailed, not Modified: {sent:?}"
    );
    let kept = world
      .resource::<Assets<Shader>>()
      .get(&handle)
      .map(Shader::source);
    assert_eq!(kept, Some(GLOW), "the last good shader stays");
    let warned: Vec<_> = dev_log::get_entries()
      .into_iter()
      .filter(|entry| entry.message.contains("shaders/kept.wgsl"))
      .map(|entry| (entry.level, entry.module, entry.message))
      .collect();
    assert_eq!(
      warned,
      [(
        LogLevel::Warn,
        Some("asset".to_string()),
        "could not load \"shaders/kept.wgsl\": 2:29: expected `)`, found \"0.0\"".to_string()
      )]
    );
  }

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
