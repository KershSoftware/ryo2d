//! The pump: where finished loads and frees reach the world, and their
//! events are sent. Everything that touches a typed store goes through the
//! fn pointers here, made per type where the type is known

use std::any::Any;
use std::sync::Arc;

use super::{Asset, AssetError, AssetEvent, AssetServer, Assets, UntypedAssetId};
use crate::core::ecs::kernel::{DetectChangesMut, Messages, World};

/// Applies finished loads, reloads and frees, and sends their events. Runs in
/// `Phase::PreUpdate`; with nothing to do it polls a few channels
#[doc(hidden)]
pub fn pump(world: &mut World) {
  let Some(server) = world.get_resource::<AssetServer>().cloned() else {
    return;
  };
  server.run_inline_jobs();
  server.drain_changed();
  while let Some(finished) = server.next_finished() {
    apply(world, &server, finished);
  }
  let mut freed = Vec::new();
  let mut index = 0;
  while let Some(free_unheld) = server.free_step(index) {
    free_unheld(world, &mut freed);
    index += 1;
  }
  if !freed.is_empty() {
    server.forget(&freed);
  }
}

/// A load that finished on a worker (or failed before it started)
pub(crate) struct Finished {
  pub(crate) id: UntypedAssetId,
  /// The job it came from: stale unless it is still the asset's current one
  pub(crate) job: u64,
  pub(crate) outcome: Result<Loaded, Arc<AssetError>>,
  pub(crate) typed: Typed,
}

/// A value a loader made, and what it loaded through its `LoadContext`
pub(crate) struct Loaded {
  pub(crate) value: Box<dyn Any + Send>,
  pub(crate) dependencies: Vec<UntypedAssetId>,
}

/// What the pump does with an asset of one type, behind its type
#[derive(Clone, Copy)]
pub(crate) struct Typed {
  /// Puts the value in its store and sends `Added`; false if the id is stale
  pub(crate) insert: fn(&mut World, UntypedAssetId, Box<dyn Any + Send>) -> bool,
  /// Sends `Failed`
  pub(crate) failed: fn(&mut World, UntypedAssetId, Arc<AssetError>),
}

impl Typed {
  pub(crate) fn of<T: Asset>() -> Self {
    Self {
      insert: insert_value::<T>,
      failed: send_failed::<T>,
    }
  }
}

/// Frees a type's unheld assets, sends `Removed` for each and adds their ids to the list
pub(crate) type FreeUnheld = fn(&mut World, &mut Vec<UntypedAssetId>);

fn apply(world: &mut World, server: &AssetServer, finished: Finished) {
  let Finished {
    id,
    job,
    outcome,
    typed,
  } = finished;
  // Freed since, or replaced by `load_now`: the value (and the handles it holds) drops here
  if !server.is_current(id, job) {
    return;
  }
  match outcome {
    Ok(Loaded {
      value,
      dependencies,
    }) => {
      if (typed.insert)(world, id, value) {
        server.set_loaded(id, job, dependencies);
      }
    }
    Err(error) => {
      server.set_failed(id, job, Arc::clone(&error));
      crate::engine_warn!(module: "asset", "{}", error.report());
      (typed.failed)(world, id, error);
    }
  }
}

pub(crate) fn insert_value<T: Asset>(
  world: &mut World,
  id: UntypedAssetId,
  value: Box<dyn Any + Send>,
) -> bool {
  let (Some(id), Ok(value)) = (id.typed::<T>(), value.downcast::<T>()) else {
    return false;
  };
  let inserted = world
    .get_resource_mut::<Assets<T>>()
    .is_some_and(|mut assets| assets.insert(id, *value));
  if inserted {
    send(world, AssetEvent::Added { id });
  }
  inserted
}

pub(crate) fn send_failed<T: Asset>(world: &mut World, id: UntypedAssetId, error: Arc<AssetError>) {
  if let Some(id) = id.typed::<T>() {
    send(world, AssetEvent::Failed { id, error });
  }
}

pub(crate) fn free_unheld<T: Asset>(world: &mut World, freed: &mut Vec<UntypedAssetId>) {
  let mut ids = Vec::new();
  {
    let Some(mut assets) = world.get_resource_mut::<Assets<T>>() else {
      return;
    };
    // Most frames free nothing; only a real free marks the store changed
    assets.bypass_change_detection().free_unheld(&mut ids);
    if ids.is_empty() {
      return;
    }
    assets.set_changed();
  }
  freed.extend(ids.iter().map(|id| id.untyped()));
  if let Some(mut events) = world.get_resource_mut::<Messages<AssetEvent<T>>>() {
    for id in ids {
      events.write(AssetEvent::Removed { id });
    }
  }
}

fn send<T: Asset>(world: &mut World, event: AssetEvent<T>) {
  if let Some(mut events) = world.get_resource_mut::<Messages<AssetEvent<T>>>() {
    events.write(event);
  }
}
