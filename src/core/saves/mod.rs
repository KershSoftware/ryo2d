//! Game saves.
//!
//! The storage layer: slot names, the crash-safe replace and recover order,
//! and the storage backends (a native folder and an in-memory store that can
//! inject faults for tests).

// Until the save service is built on this layer, only the tests use parts of it
#![cfg_attr(not(test), allow(dead_code))]

mod error;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod files;
pub(crate) mod memory;
pub(crate) mod replace;
pub(crate) mod slot;
pub(crate) mod storage;

pub use error::SaveError;
pub use memory::MemorySaves;
