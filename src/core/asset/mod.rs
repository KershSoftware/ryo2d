//! Assets: the registry, and the old manager it is replacing.
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
//! - [`AssetManager`] - the old manager (UI files), until everything loads
//!   through the registry
//! - [`HotReloadWatcher`] - the old manager's file watcher

pub mod assets;
pub mod error;
pub mod event;
pub mod handle;
pub mod hot_reload;
pub mod id;
pub mod loader;
mod loaders;
pub mod manager;
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
pub use loader::{AssetLoader, AssetSettings, LoadContext};
pub use loaders::{ImageLoader, ImageSettings, ShaderLoader, SvgSize};
pub use path::AssetPath;
#[doc(hidden)]
pub use pump::pump;
pub use server::AssetServer;
pub use source::{AssetSource, DiskSource, MemorySource};
pub use view::AssetsView;

/// A type the registry stores: loaded from files by an `AssetLoader`, or
/// made in code and added with `Assets::add`
pub trait Asset: Send + Sync + 'static {}

/// Puts `server` in `world` as its asset registry. The engine's world, the
/// test worlds and `UiHarness` all start this way, so the engine's own asset
/// types, registered here with their loaders ([`crate::core::graphics::Image`]
/// with [`ImageLoader`], [`crate::core::ecs::Shader`] with [`ShaderLoader`]),
/// have their stores before anything loads and a loader's `check` finds
/// them. A game's loaders are added after, so for an extension both take,
/// the game's wins
pub(crate) fn insert_registry(world: &mut crate::core::ecs::World, server: AssetServer) {
  server.register_asset::<crate::core::graphics::Image>(world);
  server.register_loader(ImageLoader);
  server.register_asset::<crate::core::ecs::Shader>(world);
  server.register_loader(ShaderLoader);
  world.insert_resource(server);
}

pub use hot_reload::{HotReloadConfig, HotReloadEvent, HotReloadWatcher};
pub use manager::AssetManager;
