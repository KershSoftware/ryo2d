//! The registry as a scene or a test reaches it: the world's stores and its
//! `AssetServer` behind one borrow of the world

use std::sync::Arc;

use super::event::AssetStats;
use super::{
  Asset, AssetError, AssetId, AssetPath, AssetServer, AssetSettings, Assets, Handle, LoadState,
  MemorySource, Progress, UntypedAssetId,
};
use crate::core::ecs::kernel::{Mut, World};

/// Runs `f` with the world's `Assets<T>` lent out beside the rest of the
/// world, as the UI pass reads the images (an empty store for a world
/// without one). Meanwhile the registry adds nothing to the store and
/// loads nothing into it at once: those calls fail, with a warning
pub(crate) fn lend_store<T: Asset, R>(
  world: &mut World,
  f: impl FnOnce(&mut World, &Assets<T>) -> R,
) -> R {
  let server = world.get_resource::<AssetServer>().cloned();
  if let Some(server) = &server {
    server.lend::<T>();
  }
  let result = if world.contains_resource::<Assets<T>>() {
    world.resource_scope(|world, store: Mut<Assets<T>>| f(world, &store))
  } else {
    f(world, &Assets::default())
  };
  if let Some(server) = &server {
    server.give_back::<T>();
  }
  result
}

/// The error a load at once into a lent store gives
fn lent_error<T: Asset>(path: &str) -> Arc<AssetError> {
  let path = match AssetPath::parse(path) {
    Ok(path) => path,
    Err(error) => return Arc::new(error),
  };
  Arc::new(AssetError::Io {
    path,
    source: std::io::Error::other(format!(
      "{}s cannot be loaded at once while the UI pass reads them; load it before (in enter or update)",
      super::server::short_name::<T>()
    )),
  })
}

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
  /// its store. See [`AssetServer::load_now`]
  pub fn load_now<T: Asset>(&mut self, path: &str) -> Result<Handle<T>, Arc<AssetError>> {
    if self.server.is_lent::<T>() {
      return Err(lent_error::<T>(path));
    }
    self.server.load_now(self.world, path)
  }

  /// Loads at once with `settings`. See [`AssetServer::load_now_with`]
  pub fn load_now_with<T: Asset, S: AssetSettings>(
    &mut self,
    path: &str,
    settings: S,
  ) -> Result<Handle<T>, Arc<AssetError>> {
    if self.server.is_lent::<T>() {
      return Err(lent_error::<T>(path));
    }
    self.server.load_now_with(self.world, path, settings)
  }

  /// Adds an asset made in code; it lives while the handle (or a clone)
  /// does. A type not registered yet, or with no store in this world, is
  /// registered first.
  ///
  /// Images cannot be added while the UI pass reads them (a scene's `ui`):
  /// the value is dropped with a warning and the handle finds nothing. Add
  /// them before (in `enter` or `update`)
  pub fn add<T: Asset>(&mut self, value: T) -> Handle<T> {
    if self.server.is_lent::<T>() {
      crate::engine_warn!(
        module: "asset",
        "{}s cannot be added while the UI pass reads them; add it before (in enter or update)",
        super::server::short_name::<T>()
      );
      drop(value);
      return Handle::detached();
    }
    if !self.server.is_registered::<T>() || !self.world.contains_resource::<Assets<T>>() {
      self.server.register_asset::<T>(self.world);
    }
    match self.world.get_resource_mut::<Assets<T>>() {
      Some(mut assets) => assets.add(value),
      // `register_asset` has just made sure the store is there
      None => Handle::detached(),
    }
  }

  /// The asset, if it is loaded (images are not, while the UI pass reads
  /// them: see `Scene::ui`)
  pub fn get<T: Asset>(&self, id: impl Into<AssetId<T>>) -> Option<&T> {
    self.world.get_resource::<Assets<T>>()?.get(id)
  }

  /// The asset for changing in place; its content version moves on, so
  /// what was built from it is built again
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

  /// How far a group of assets has loaded. See [`AssetServer::progress`]
  pub fn progress(&self, ids: &[UntypedAssetId]) -> Progress {
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
