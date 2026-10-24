use std::fmt;
use std::sync::Arc;

use super::{Asset, AssetError, AssetId};
use crate::core::ecs::kernel::Message;

/// Where an asset is in loading
#[derive(Debug, Clone)]
pub enum LoadState {
  /// Not asked for, or freed
  NotLoaded,
  /// Asked for; its value is on its way
  Loading,
  /// Its value is in its `Assets<T>`
  Loaded,
  /// It did not load, for this reason. It is loaded again when its file
  /// changes (hot reload, or `AssetServer::reload`)
  Failed(Arc<AssetError>),
}

impl LoadState {
  /// Whether the value is in its store
  pub fn is_loaded(&self) -> bool {
    matches!(self, LoadState::Loaded)
  }

  /// Whether it failed to load
  pub fn is_failed(&self) -> bool {
    matches!(self, LoadState::Failed(_))
  }

  /// Why it failed to load, if it did
  pub fn error(&self) -> Option<&Arc<AssetError>> {
    match self {
      LoadState::Failed(error) => Some(error),
      _ => None,
    }
  }
}

/// Something happened to an asset of type `T` (read with `MessageReader` or
/// `ctx.messages`).
///
/// `load_now` sends its asset's `Added` or `Failed` at once. The pump
/// (`PreUpdate`, each frame) sends the rest, for each type in this order:
/// 1. the events of the loads, reloads and dependency touches it applies
///    (`Added`, `Modified`, `Failed`, `ReloadFailed`), as it applies them;
/// 2. the events of changes made in code since the last pump (`add`,
///    `get_mut`), in the order their assets first changed: once per asset,
///    so one added, then changed, is `Added` only, and one changed many times
///    is `Modified` once; one the pump also loads, reloads or touches in step
///    1 has only that event;
/// 3. `Removed` for each asset it frees that was in its store.
///
/// What an id gets, by cause:
/// - `Added`: a load lands, or code calls `add`;
/// - `Modified`: a reload lands, a dependency reloads, or code calls
///   `get_mut` or `insert`;
/// - `Failed`: a load fails;
/// - `ReloadFailed`: a reload fails, and the last good value stays;
/// - `Removed`: the last handle dropped, and only for an id that was
///   `Added`.
///
/// An id's first event is `Added` or `Failed`. A failed one is loaded again
/// when its file changes, so `Added` or `Failed` can follow `Failed`. While
/// it is in its store it can be `Modified` or `ReloadFailed`. `Removed`
/// comes last. An asset freed without an `Added` (a failed one, or a load
/// dropped before it landed) gets no `Removed`, so a game that keeps
/// failures by id drops them when it drops the handle.
///
/// So the order holds per asset: an asset's events come in the order its
/// changes happened, and `Removed` comes only after `Added`. Between assets
/// it need not, as a change made in code before the pump is sent after the
/// pump's own events
#[derive(Message)]
pub enum AssetEvent<T: Asset> {
  /// It is in its `Assets<T>`: a load finished (sent by the pump that put it
  /// there, or by `load_now` at once), or code added it (`add`, sent at the
  /// next pump)
  Added {
    /// The asset
    id: AssetId<T>,
  },
  /// Its value changed: it reloaded or a dependency did (sent by the pump
  /// that applied it), or code changed it (`get_mut`, sent at the next pump)
  Modified {
    /// The asset
    id: AssetId<T>,
  },
  /// It left its `Assets<T>`: its last handle dropped and the pump freed it
  /// (after any event of the same pump for it). Only an asset that was in
  /// its store is `Removed`: a load dropped before it landed, or one that
  /// failed, is freed with no event
  Removed {
    /// The asset
    id: AssetId<T>,
  },
  /// It did not load (sent by the pump, or by `load_now` at once). Its file
  /// changing, or `AssetServer::reload`, loads it again: `Added`, or
  /// `Failed` again
  Failed {
    /// The asset
    id: AssetId<T>,
    /// Why
    error: Arc<AssetError>,
  },
  /// A reload failed; the last good value stays (sent by the pump)
  ReloadFailed {
    /// The asset
    id: AssetId<T>,
    /// Why
    error: Arc<AssetError>,
  },
}

// By hand, so an event is `Clone` and `Debug` whatever `T` is
impl<T: Asset> Clone for AssetEvent<T> {
  fn clone(&self) -> Self {
    match self {
      AssetEvent::Added { id } => AssetEvent::Added { id: *id },
      AssetEvent::Modified { id } => AssetEvent::Modified { id: *id },
      AssetEvent::Removed { id } => AssetEvent::Removed { id: *id },
      AssetEvent::Failed { id, error } => AssetEvent::Failed {
        id: *id,
        error: Arc::clone(error),
      },
      AssetEvent::ReloadFailed { id, error } => AssetEvent::ReloadFailed {
        id: *id,
        error: Arc::clone(error),
      },
    }
  }
}

impl<T: Asset> fmt::Debug for AssetEvent<T> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      AssetEvent::Added { id } => f.debug_struct("Added").field("id", id).finish(),
      AssetEvent::Modified { id } => f.debug_struct("Modified").field("id", id).finish(),
      AssetEvent::Removed { id } => f.debug_struct("Removed").field("id", id).finish(),
      AssetEvent::Failed { id, error } => f
        .debug_struct("Failed")
        .field("id", id)
        .field("error", error)
        .finish(),
      AssetEvent::ReloadFailed { id, error } => f
        .debug_struct("ReloadFailed")
        .field("id", id)
        .field("error", error)
        .finish(),
    }
  }
}

/// A group's progress
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Progress {
  /// Assets whose value is in its store
  pub loaded: usize,
  /// Assets that will not load
  pub failed: usize,
  /// Assets still loading
  pub pending: usize,
}

impl Progress {
  /// Whether nothing is pending: each asset loaded or failed
  pub fn is_done(&self) -> bool {
    self.pending == 0
  }

  /// The share settled (loaded or failed), from 0 to 1; 1 for an empty group
  pub fn fraction(&self) -> f32 {
    let total = self.loaded + self.failed + self.pending;
    if total == 0 {
      return 1.0;
    }
    (self.loaded + self.failed) as f32 / total as f32
  }
}

/// Per type: loaded, loading and failed counts
#[derive(Debug, Clone, Default)]
pub struct AssetStats {
  /// One entry per registered asset type, in the order they were registered
  pub types: Vec<TypeStats>,
}

/// One asset type's counts, over the assets loaded by path
#[derive(Debug, Clone, Default)]
pub struct TypeStats {
  /// The type's name, without its module path
  pub name: &'static str,
  /// Assets whose value is in the store
  pub loaded: usize,
  /// Assets still loading
  pub loading: usize,
  /// Assets that will not load
  pub failed: usize,
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn progress_counts_settled_assets() {
    let progress = Progress {
      loaded: 2,
      failed: 1,
      pending: 1,
    };
    assert!(!progress.is_done());
    assert!((progress.fraction() - 0.75).abs() < f32::EPSILON);
    assert!(Progress::default().is_done());
    assert!((Progress::default().fraction() - 1.0).abs() < f32::EPSILON);
  }
}
