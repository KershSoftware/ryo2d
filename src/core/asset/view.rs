//! The registry as a scene or a test reaches it: the world's stores and its
//! `AssetServer` behind one borrow of the world

use std::sync::Arc;

use super::event::AssetStats;
use super::{
  Asset, AssetError, AssetId, AssetServer, AssetSettings, Assets, Handle, LoadState, MemorySource,
  Progress, UntypedAssetId,
};
use crate::core::ecs::kernel::World;

/// The asset registry seen from a scene or a test: loads, gets and adds
/// assets of every type. A scene gets one from `ctx.assets()`; it borrows
/// the world, so take it, use it and let it go
///
/// ```
/// use ryo2d::prelude::*;
///
/// struct Note(String);
/// impl Asset for Note {}
///
/// fn remember(ctx: &mut SceneContext<'_>) -> Handle<Note> {
///   let note = ctx.assets().add(Note("pick up the key".into()));
///   assert_eq!(ctx.assets().get(&note).map(|n| n.0.as_str()), Some("pick up the key"));
///   note
/// }
/// ```
pub struct AssetsView<'w> {
  world: &'w mut World,
  server: AssetServer,
}

impl<'w> AssetsView<'w> {
  /// The registry of `world`. A world with no `AssetServer` (one the engine
  /// did not make) is given one over an empty `MemorySource`, with a warning
  pub(crate) fn new(world: &'w mut World) -> Self {
    let server = match world.get_resource::<AssetServer>() {
      Some(server) => server.clone(),
      None => {
        // Once per world: the next view finds this server
        crate::engine_warn!(
          module: "asset",
          "this world has no asset server; it gets one over an empty memory source"
        );
        let server = AssetServer::new(MemorySource::new());
        world.insert_resource(server.clone());
        server
      }
    };
    Self { world, server }
  }

  /// The asset at `path`, loading it on the workers if needed; returns at
  /// once. See [`AssetServer::load`]
  pub fn load<T: Asset>(&self, path: &str) -> Handle<T> {
    self.server.load(path)
  }

  /// The asset at `path` loaded with `settings`. See [`AssetServer::load_with`]
  pub fn load_with<T: Asset, S: AssetSettings>(&self, path: &str, settings: S) -> Handle<T> {
    self.server.load_with(path, settings)
  }

  /// Loads at once on this thread, dependencies too, and puts the asset in
  /// its store: a large image's mip levels too, about 15 to 40 ms for a
  /// 2048 px one, so a large image is better loaded with [`load`](Self::load)
  /// behind a loading screen. See [`AssetServer::load_now`]
  pub fn load_now<T: Asset>(&mut self, path: &str) -> Result<Handle<T>, Arc<AssetError>> {
    self.server.load_now(self.world, path)
  }

  /// Loads at once with `settings`. See [`AssetServer::load_now_with`]
  pub fn load_now_with<T: Asset, S: AssetSettings>(
    &mut self,
    path: &str,
    settings: S,
  ) -> Result<Handle<T>, Arc<AssetError>> {
    self.server.load_now_with(self.world, path, settings)
  }

  /// Adds an asset made in code; it lives while the handle (or a clone)
  /// does. A type not registered yet, or with no store in this world, is
  /// registered first
  pub fn add<T: Asset>(&mut self, value: T) -> Handle<T> {
    if !self.server.is_registered::<T>() || !self.world.contains_resource::<Assets<T>>() {
      self.server.register_asset::<T>(self.world);
    }
    match self.world.get_resource_mut::<Assets<T>>() {
      Some(mut assets) => assets.add(value),
      // `register_asset` has just made sure the store is there
      None => Handle::detached(),
    }
  }

  /// The asset, if it is loaded
  pub fn get<T: Asset>(&self, id: impl Into<AssetId<T>>) -> Option<&T> {
    self.world.get_resource::<Assets<T>>()?.get(id)
  }

  /// The asset for changing in place; its content version moves on, so
  /// what was built from it is built again, and the next pump sends
  /// `AssetEvent::Modified` for it (once, however many times it is changed
  /// before then)
  pub fn get_mut<T: Asset>(&mut self, id: impl Into<AssetId<T>>) -> Option<&mut T> {
    let id = id.into();
    let assets = self.world.get_resource_mut::<Assets<T>>()?;
    // Only an asset that is there marks its store changed
    assets.get(id)?;
    assets.into_inner().get_mut(id)
  }

  /// Where the asset is in loading. See [`AssetServer::state`]
  pub fn state<T: Asset>(&self, id: impl Into<AssetId<T>>) -> LoadState {
    self.server.state(id)
  }

  /// Loaded, with every dependency loaded too
  pub fn is_ready<T: Asset>(&self, id: impl Into<AssetId<T>>) -> bool {
    self.server.is_ready(id)
  }

  /// How far a group of assets has loaded: its handles, or untyped ids for
  /// a group across types. See [`AssetServer::progress`]
  pub fn progress<I>(&self, ids: I) -> Progress
  where
    I: IntoIterator,
    I::Item: Into<UntypedAssetId>,
  {
    self.server.progress(ids)
  }

  /// Loaded, loading and failed counts per registered type
  pub fn stats(&self) -> AssetStats {
    self.server.stats()
  }

  /// The server, for what the view does not cover (`reload`, `path`,
  /// `dependency_state`)
  pub fn server(&self) -> &AssetServer {
    &self.server
  }

  /// The world the view borrows
  pub(crate) fn world_mut(&mut self) -> &mut World {
    self.world
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[derive(Debug, PartialEq)]
  struct Note(&'static str);
  impl Asset for Note {}

  #[test]
  fn add_registers_the_type_and_get_mut_moves_the_version_on() {
    let mut world = World::new();
    let mut assets = AssetsView::new(&mut world);
    let note = assets.add(Note("a"));
    assert_eq!(assets.get(&note), Some(&Note("a")));
    assert!(assets.state(&note).is_loaded());
    assert!(assets.server().is_registered::<Note>());
    let before = world.resource::<Assets<Note>>().version(&note);
    let mut assets = AssetsView::new(&mut world);
    if let Some(note) = assets.get_mut(&note) {
      note.0 = "b";
    }
    assert_eq!(assets.get(&note), Some(&Note("b")));
    assert_ne!(world.resource::<Assets<Note>>().version(&note), before);
  }

  #[test]
  fn add_puts_a_store_in_a_world_that_has_none() {
    let server = AssetServer::new(MemorySource::new());
    let mut first = World::new();
    server.register_asset::<Note>(&mut first);
    // The same server in a second world, which has no store for the type yet
    let mut second = World::new();
    second.insert_resource(server);
    let note = AssetsView::new(&mut second).add(Note("a"));
    assert_eq!(AssetsView::new(&mut second).get(&note), Some(&Note("a")));
  }

  #[test]
  fn a_world_without_a_server_is_given_one() {
    let mut world = World::new();
    let note = AssetsView::new(&mut world).add(Note("a"));
    assert!(world.contains_resource::<AssetServer>());
    // The same server from then on: it knows the type the first view registered
    let assets = AssetsView::new(&mut world);
    assert!(assets.server().is_registered::<Note>());
    assert!(assets.state(&note).is_loaded());
  }
}
