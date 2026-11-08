//! Game saves.
//!
//! The save file (a header, checksums and the game's data as compressed RON),
//! the game's data versions with their upgrade steps, and the storage layer:
//! slot names, the crash-safe replace and recover order, and the storage
//! backends (a native folder and an in-memory store that can inject faults for
//! tests).

// Until the save service is built on this layer, only the tests use parts of it
#![cfg_attr(not(test), allow(dead_code))]

pub(crate) mod container;
mod error;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod files;
pub(crate) mod memory;
pub(crate) mod replace;
pub(crate) mod slot;
pub(crate) mod storage;
pub(crate) mod versions;

pub use container::SaveHeader;
pub use error::SaveError;
pub use memory::MemorySaves;
pub use versions::SaveVersions;
