//! The pump: where finished loads, reloads and frees reach the world, and
//! their events are sent. Everything that touches a typed store goes through
//! the fn pointers here, made per type where the type is known

use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use super::loader::ErasedLoader;
use super::server::Settled;
use super::{Asset, AssetError, AssetEvent, AssetPath, AssetServer, Assets, UntypedAssetId};
use crate::core::ecs::kernel::{DetectChangesMut, Messages, World};

/// Applies finished loads, reloads and frees, and sends their events. Runs in
/// `Phase::PreUpdate`; with nothing to do it polls a few channels
#[doc(hidden)]
pub fn pump(world: &mut World) {
  let Some(server) = world.get_resource::<AssetServer>().cloned() else {
    return;
  };
  server.run_inline_jobs();
  server.reload_changed();
  while let Some(finished) = server.next_finished() {
    apply(world, &server, finished);
  }
  // Settling one result can let another go: a parent waits on its dependencies
  while let Some(settled) = server.next_settled() {
    match settled {
      Settled::Ready(waiting) => settle(world, &server, waiting),
      Settled::Failed(waiting, error) => {
        let error = explained(&server, &waiting, error);
        let arrived = waiting.arrived;
        drop(waiting);
        fail(world, &server, arrived, error);
      }
      Settled::Stale(waiting) => drop(waiting),
    }
  }
  // Changes made in code since the last pump, before anything is freed (an
  // asset added and dropped is added, then removed)
  let mut index = 0;
  while let Some(send_queued) = server.send_step(index) {
    send_queued(world);
    index += 1;
  }
  free(world, &server);
}

/// A load that finished on a worker (or failed before it started)
pub(crate) struct Finished {
  pub(crate) id: UntypedAssetId,
  /// The job it came from: stale unless it is still the asset's current one
  pub(crate) job: u64,
  pub(crate) outcome: Result<Loaded, Arc<AssetError>>,
}

/// A value a loader made, and what it loaded through its `LoadContext`
pub(crate) struct Loaded {
  pub(crate) value: Box<dyn Any + Send>,
  pub(crate) dependencies: Vec<UntypedAssetId>,
}

/// Whether a value is an asset's first, or a reload replacing the one in its store
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Arrival {
  /// Its first value: `Added`, or `Failed`
  First,
  /// A reload: `Modified`, or `ReloadFailed` with the last good value kept
  Reload,
}

/// A job's result that still counts: its asset, its job, and how it arrives
#[derive(Clone, Copy)]
pub(crate) struct Arrived {
  pub(crate) id: UntypedAssetId,
  pub(crate) job: u64,
  pub(crate) arrival: Arrival,
  pub(crate) typed: Typed,
}

/// A value held back until its dependencies load
pub(crate) struct Waiting {
  pub(crate) arrived: Arrived,
  pub(crate) value: Box<dyn Any + Send>,
  pub(crate) dependencies: Vec<UntypedAssetId>,
}

/// Puts a value in its store; false if the id is stale
type Insert = fn(&mut World, UntypedAssetId, Box<dyn Any + Send>, Arrival) -> bool;
/// Checks a stored value with its loader; None if nothing is stored
type CheckStored =
  fn(&World, UntypedAssetId, &dyn ErasedLoader, &AssetPath) -> Option<Result<(), AssetError>>;

/// What the pump does with an asset of one type, behind its type
#[derive(Clone, Copy)]
pub(crate) struct Typed {
  /// Puts the value in its store and sends `Added`, or `Modified` for a reload
  pub(crate) insert: Insert,
  /// Sends `Failed`, or `ReloadFailed` for a reload
  pub(crate) failed: fn(&mut World, UntypedAssetId, Arc<AssetError>, Arrival),
  /// Checks the stored value with its loader
  pub(crate) check: CheckStored,
  /// Moves the stored value's version on and sends `Modified`
  pub(crate) touch: fn(&mut World, UntypedAssetId),
}

impl Typed {
  pub(crate) fn of<T: Asset>() -> Self {
    Self {
      insert: insert_value::<T>,
      failed: send_failed::<T>,
      check: check_stored::<T>,
      touch: touch::<T>,
    }
  }
}

/// Frees a type's unheld assets, sends `Removed` for each that was in its
/// store and adds their ids to the list
pub(crate) type FreeUnheld = fn(&mut World, &mut Vec<UntypedAssetId>);
/// Sends the events a type's store queued for changes made in code
pub(crate) type SendQueued = fn(&mut World);

fn apply(world: &mut World, server: &AssetServer, finished: Finished) {
  let Finished { id, job, outcome } = finished;
  // Freed since, overtaken by `load_now` or a newer load, or read before a
  // change that loads it again: the value (and the handles it holds) drops here
  let Some(arrived) = server.arrive(id, job) else {
    return;
  };
  match outcome {
    Ok(Loaded {
      value,
      dependencies,
    }) => {
      let waiting = Waiting {
        arrived,
        value,
        dependencies,
      };
      if waiting.dependencies.is_empty() {
        settle(world, server, waiting);
        return;
      }
      // Dependencies that lead back to it fail it, with every other result on the cycle
      for (waiting, error) in server.wait(waiting) {
        let arrived = waiting.arrived;
        drop(waiting);
        fail(world, server, arrived, error);
      }
    }
    Err(error) => fail(world, server, arrived, error),
  }
}

/// A value whose dependencies are all loaded: checked by its loader, then put
/// in its store. A reload then has every asset that holds it checked again
fn settle(world: &mut World, server: &AssetServer, waiting: Waiting) {
  let Waiting {
    arrived,
    value,
    dependencies,
  } = waiting;
  // A value that counts has a path and a loader, and its slot is live (a
  // slot is freed only in the free step, which forgets its info and so makes
  // its values stale). Should either break, the asset fails, so nothing
  // waits on it for good
  let checker = server.checker(arrived.id);
  debug_assert!(checker.is_some(), "a current value has a path and a loader");
  let Some((loader, path)) = checker else {
    let error = AssetError::decode(
      &server.known_path(arrived.id),
      "a broken invariant: the server has no path or loader for its value",
    );
    fail(world, server, arrived, Arc::new(error));
    return;
  };
  if let Err(error) = run_check(&*loader, &*value, world, &path) {
    fail(world, server, arrived, Arc::new(error));
    return;
  }
  let inserted = (arrived.typed.insert)(world, arrived.id, value, arrived.arrival);
  debug_assert!(inserted, "a current value's store takes it");
  if !inserted {
    let error = AssetError::decode(&path, BROKEN_STORE);
    fail(world, server, arrived, Arc::new(error));
    return;
  }
  server.set_loaded(arrived.id, arrived.job, dependencies);
  if arrived.arrival == Arrival::Reload {
    recheck_dependents(world, server, arrived.id);
  }
}

/// A failed dependency's error as the waiting value's loader gives it
/// (`AssetLoader::dependency_failed`), else the server's; a panic in the
/// loader keeps the server's
fn explained(server: &AssetServer, waiting: &Waiting, error: Arc<AssetError>) -> Arc<AssetError> {
  let Some((loader, _)) = server.checker(waiting.arrived.id) else {
    return error;
  };
  let better = catch_unwind(AssertUnwindSafe(|| {
    loader.dependency_failed(&*waiting.value, &error)
  }));
  match better {
    Ok(Some(better)) => Arc::new(better),
    Ok(None) | Err(_) => error,
  }
}

/// Why a value its store would not take fails its asset
pub(crate) const BROKEN_STORE: &str =
  "a broken invariant: its store would not take the value (a stale id, or no store in the world)";

/// A result that failed: a first load's asset fails; a reload keeps the last
/// good value. Either way it warns once per error, and its event is sent
pub(crate) fn fail(
  world: &mut World,
  server: &AssetServer,
  arrived: Arrived,
  error: Arc<AssetError>,
) {
  if arrived.arrival == Arrival::First {
    server.set_failed(arrived.id, arrived.job, Arc::clone(&error));
  }
  server.warn_once(arrived.id, &error);
  (arrived.typed.failed)(world, arrived.id, error, arrived.arrival);
}

/// Checks each loaded asset that holds `id`, directly or through others,
/// nearest first: by the fewest steps from `id`, so one that holds it both
/// directly and through another comes before that other. Checks only read
/// and a pass only moves a version on, so the order changes no outcome, only
/// the order of the events. One that passes is `Modified` (its version moves
/// on, so what was built from it rebuilds); one that fails is `ReloadFailed`
fn recheck_dependents(world: &mut World, server: &AssetServer, id: UntypedAssetId) {
  for dependent in server.dependents_of(id) {
    let checked = (dependent.typed.check)(world, dependent.id, &*dependent.loader, &dependent.path);
    match checked {
      None => {}
      Some(Ok(())) => {
        server.clear_warning(dependent.id);
        (dependent.typed.touch)(world, dependent.id);
      }
      Some(Err(error)) => {
        let error = Arc::new(error);
        server.warn_once(dependent.id, &error);
        (dependent.typed.failed)(world, dependent.id, error, Arrival::Reload);
      }
    }
  }
}

/// Frees every unheld asset, and forgets them
fn free(world: &mut World, server: &AssetServer) {
  let mut freed = Vec::new();
  // A freed asset drops its handles on its dependencies: round again, so
  // those it alone held go in the same frame
  loop {
    let before = freed.len();
    let mut index = 0;
    while let Some(free_unheld) = server.free_step(index) {
      free_unheld(world, &mut freed);
      index += 1;
    }
    if freed.len() == before {
      break;
    }
  }
  if !freed.is_empty() {
    server.forget(&freed);
  }
}

/// A loader's check of `value`; a panic in it fails the check (in native
/// builds: the web build aborts on a panic)
pub(crate) fn run_check(
  loader: &dyn ErasedLoader,
  value: &dyn Any,
  world: &World,
  path: &AssetPath,
) -> Result<(), AssetError> {
  catch_unwind(AssertUnwindSafe(|| loader.check(value, world, path)))
    .unwrap_or_else(|_| Err(AssetError::decode(path, "the loader's check panicked")))
}

pub(crate) fn insert_value<T: Asset>(
  world: &mut World,
  id: UntypedAssetId,
  value: Box<dyn Any + Send>,
  arrival: Arrival,
) -> bool {
  let (Some(id), Ok(value)) = (id.typed::<T>(), value.downcast::<T>()) else {
    return false;
  };
  // Sent here, so not queued as well
  let inserted = world
    .get_resource_mut::<Assets<T>>()
    .is_some_and(|mut assets| {
      let inserted = assets.insert(id, *value);
      if inserted {
        assets.unqueue(id);
      }
      inserted
    });
  if inserted {
    send(
      world,
      match arrival {
        Arrival::First => AssetEvent::Added { id },
        Arrival::Reload => AssetEvent::Modified { id },
      },
    );
  }
  inserted
}

fn send_failed<T: Asset>(
  world: &mut World,
  id: UntypedAssetId,
  error: Arc<AssetError>,
  arrival: Arrival,
) {
  let Some(id) = id.typed::<T>() else {
    return;
  };
  send(
    world,
    match arrival {
      Arrival::First => AssetEvent::Failed { id, error },
      Arrival::Reload => AssetEvent::ReloadFailed { id, error },
    },
  );
}

fn check_stored<T: Asset>(
  world: &World,
  id: UntypedAssetId,
  loader: &dyn ErasedLoader,
  path: &AssetPath,
) -> Option<Result<(), AssetError>> {
  let value: &dyn Any = world.get_resource::<Assets<T>>()?.get(id.typed::<T>()?)?;
  Some(run_check(loader, value, world, path))
}

fn touch<T: Asset>(world: &mut World, id: UntypedAssetId) {
  let Some(id) = id.typed::<T>() else {
    return;
  };
  // Sent here, so not queued as well
  let touched = world
    .get_resource_mut::<Assets<T>>()
    .is_some_and(|mut assets| {
      let touched = assets.get_mut(id).is_some();
      if touched {
        assets.unqueue(id);
      }
      touched
    });
  if touched {
    send(world, AssetEvent::Modified { id });
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
  freed.extend(ids.iter().map(|(id, _)| id.untyped()));
  // Only what was in the store leaves it: a slot that never held a value
  // (a load dropped before it landed, or a failed one) sends nothing
  if let Some(mut events) = world.get_resource_mut::<Messages<AssetEvent<T>>>() {
    for (id, _) in ids.into_iter().filter(|&(_, held)| held) {
      events.write(AssetEvent::Removed { id });
    }
  }
}

/// Sends the events `T`'s store queued for changes made in code
pub(crate) fn send_queued<T: Asset>(world: &mut World) {
  let mut events = Vec::new();
  {
    let Some(mut assets) = world.get_resource_mut::<Assets<T>>() else {
      return;
    };
    // Most pumps find nothing; taking the queue changes no asset
    if !assets.has_events() {
      return;
    }
    assets
      .bypass_change_detection()
      .take_events(|event| events.push(event));
  }
  if let Some(mut messages) = world.get_resource_mut::<Messages<AssetEvent<T>>>() {
    for event in events {
      messages.write(event);
    }
  }
}

fn send<T: Asset>(world: &mut World, event: AssetEvent<T>) {
  if let Some(mut events) = world.get_resource_mut::<Messages<AssetEvent<T>>>() {
    events.write(event);
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::asset::MemorySource;
  use crate::core::ecs::kernel::MessageCursor;

  struct Text(#[allow(dead_code)] String);
  impl Asset for Text {}

  /// A world whose registry stores texts, and a cursor over their events
  fn world() -> (World, MessageCursor<AssetEvent<Text>>) {
    let mut world = World::new();
    let server = AssetServer::new(MemorySource::new());
    server.register_asset::<Text>(&mut world);
    world.insert_resource(server);
    let cursor = world
      .resource::<Messages<AssetEvent<Text>>>()
      .get_cursor_current();
    (world, cursor)
  }

  fn sent(world: &World, cursor: &mut MessageCursor<AssetEvent<Text>>) -> Vec<AssetEvent<Text>> {
    cursor
      .read(world.resource::<Messages<AssetEvent<Text>>>())
      .cloned()
      .collect()
  }

  #[test]
  fn a_value_refused_for_a_stale_id_leaves_the_new_assets_event() {
    let (mut world, mut events) = world();
    let old = world
      .resource_mut::<Assets<Text>>()
      .add(Text("old".into()))
      .id();
    pump(&mut world);
    sent(&world, &mut events);
    let new = world.resource_mut::<Assets<Text>>().add(Text("new".into()));
    assert_eq!(new.id().index(), old.index(), "the slot is reused");
    let stale = Box::new(Text("stale".into()));
    assert!(!insert_value::<Text>(
      &mut world,
      old.untyped(),
      stale,
      Arrival::First
    ));
    pump(&mut world);
    let sent = sent(&world, &mut events);
    assert!(
      matches!(sent.as_slice(), [AssetEvent::Added { id }] if *id == new.id()),
      "{sent:?}"
    );
  }
}
