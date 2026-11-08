//! Game saves.
//!
//! What a game uses: `ctx.saves()` ([`SavesView`]) in scenes and the [`Saves`]
//! parameter in systems, with [`SaveFinished`] reporting each write. Behind
//! them, the save service queues saves on one writer thread, the save file
//! carries a header, checksums and the game's data as compressed RON, the
//! game's data versions upgrade older data through typed steps, and the
//! storage layer keeps slot names, the crash-safe replace and recover order,
//! and the storage backends (a native folder and an in-memory store that can
//! inject faults for tests).

use std::time::Duration;

pub(crate) mod container;
mod error;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod files;
pub(crate) mod memory;
pub(crate) mod replace;
mod service;
pub(crate) mod slot;
pub(crate) mod storage;
pub(crate) mod versions;
mod view;
mod writer;

pub use container::SaveHeader;
pub use error::SaveError;
pub use memory::MemorySaves;
pub(crate) use service::SaveService;
pub use versions::SaveVersions;
pub(crate) use view::drain_save_results;
pub use view::{Loaded, SaveFinished, SaveInfo, SaveOp, SaveSlot, Saves, SavesView};

/// How long the quit flush waits for pending writes before it warns, and
/// before it gives up
pub(crate) const QUIT_FLUSH_WARN: Duration = Duration::from_secs(3);
pub(crate) const QUIT_FLUSH_GIVE_UP: Duration = Duration::from_secs(10);
