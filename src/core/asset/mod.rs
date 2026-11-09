//! Assets: the registry.
//!
//! - [`AssetServer`] loads assets by path through [`AssetLoader`]s chosen by
//!   extension, on worker threads, or at once with `load_now`. [`Assets<T>`]
//!   holds every asset of a type, and a [`Handle<T>`] keeps one alive. Each
//!   frame (`PreUpdate`) the pump reloads changed files, inserts finished
//!   loads once their dependencies are loaded, frees unheld assets and sends
//!   their [`AssetEvent`]s. With hot reload on, the engine's [`DiskSource`]
//!   watches the asset folder (native builds), so a saved file reloads once
//!   its writes settle
//! - [`AssetsView`] - the registry as scenes (`ctx.assets()`) and tests reach it

pub mod assets;
pub mod error;
pub mod event;
pub mod handle;
pub mod id;
pub mod loader;
mod loaders;
pub mod path;
mod pump;
pub mod server;
pub mod source;
pub mod view;
#[cfg(not(target_arch = "wasm32"))]
mod watch;
mod workers;

pub use assets::Assets;
pub use error::AssetError;
pub use event::{AssetEvent, AssetStats, LoadState, Progress, TypeStats};
pub use handle::Handle;
pub use id::{AssetId, UntypedAssetId};
pub use loader::{AssetLoader, AssetSettings, CheckContext, LoadContext};
pub use loaders::{
  AudioClipLoader, AudioClipSettings, BitmapFontLoader, ClipMode, FontLoader, FontSettings,
  ImageLoader, ImageSettings, MixerLoader, MusicLoader, ParticleEffectLoader, ShaderLoader,
  SoundLoader, SvgSize, ThemeLoader, UiFileLoader,
};
pub use path::AssetPath;
#[doc(hidden)]
pub use pump::pump;
pub use server::AssetServer;
pub use source::{AssetChanges, AssetSource, DiskSource, MemorySource};
pub use view::AssetsView;

/// A type the registry stores: loaded from files by an `AssetLoader`, or
/// made in code and added with `Assets::add`
pub trait Asset: Send + Sync + 'static {}

/// Puts `server` in `world` as its asset registry. The engine's world, the
/// test worlds and `UiHarness` all start this way, so the engine's own asset
/// types, registered here with their loaders ([`crate::core::graphics::Image`]
/// with [`ImageLoader`], [`crate::core::ecs::Shader`] with [`ShaderLoader`],
/// [`crate::core::text::Font`] with [`FontLoader`],
/// [`crate::core::text::BitmapFont`] with [`BitmapFontLoader`],
/// [`crate::core::ui::UiFile`] with [`UiFileLoader`] and
/// [`crate::core::ui::Theme`] with [`ThemeLoader`],
/// [`crate::core::audio::AudioClip`] with [`AudioClipLoader`],
/// [`crate::core::audio::Sound`] with [`SoundLoader`],
/// [`crate::core::audio::Music`] with [`MusicLoader`] and
/// [`crate::core::audio::Mixer`] with [`MixerLoader`]), have their
/// stores before anything loads and a loader's `check` finds them. The
/// tilemap types, [`crate::core::tilemap::TileMap`] with
/// [`crate::core::tilemap::TiledMapLoader`] and
/// [`crate::core::tilemap::Tileset`] with
/// [`crate::core::tilemap::TiledTilesetLoader`], and
/// [`crate::core::tilemap::LdtkProject`] with
/// [`crate::core::tilemap::LdtkLoader`], and [`crate::core::tilemap::TileWorld`]
/// with [`crate::core::tilemap::TileWorldLoader`], are registered too, and
/// [`crate::core::particles::ParticleEffect`] with [`ParticleEffectLoader`].
/// A game's loaders are added after, so for an extension both take, the
/// game's wins
pub(crate) fn insert_registry(world: &mut crate::core::ecs::World, server: AssetServer) {
  server.register_asset::<crate::core::graphics::Image>(world);
  server.register_loader(ImageLoader);
  server.register_asset::<crate::core::ecs::Shader>(world);
  server.register_loader(ShaderLoader);
  server.register_asset::<crate::core::text::Font>(world);
  server.register_loader(FontLoader);
  server.register_asset::<crate::core::text::BitmapFont>(world);
  server.register_loader(BitmapFontLoader);
  server.register_asset::<crate::core::ui::UiFile>(world);
  server.register_loader(UiFileLoader);
  server.register_asset::<crate::core::ui::Theme>(world);
  server.register_loader(ThemeLoader);
  server.register_asset::<crate::core::audio::AudioClip>(world);
  server.register_loader(AudioClipLoader::default());
  server.register_asset::<crate::core::audio::Sound>(world);
  server.register_loader(SoundLoader);
  server.register_asset::<crate::core::audio::Music>(world);
  server.register_loader(MusicLoader);
  server.register_asset::<crate::core::audio::Mixer>(world);
  server.register_loader(MixerLoader);
  server.register_asset::<crate::core::tilemap::TileMap>(world);
  server.register_asset::<crate::core::tilemap::Tileset>(world);
  server.register_loader(crate::core::tilemap::TiledMapLoader::default());
  server.register_loader(crate::core::tilemap::TiledTilesetLoader::default());
  server.register_asset::<crate::core::tilemap::TileWorld>(world);
  server.register_loader(crate::core::tilemap::TileWorldLoader);
  server.register_asset::<crate::core::tilemap::LdtkProject>(world);
  server.register_loader(crate::core::tilemap::LdtkLoader);
  server.register_asset::<crate::core::particles::ParticleEffect>(world);
  server.register_loader(ParticleEffectLoader);
  world.insert_resource(server);
}
