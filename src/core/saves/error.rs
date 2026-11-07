/// Why a save, a load or a slot operation failed. Each variant keeps what
/// caused it as its `source()`.
#[derive(Debug, thiserror::Error)]
pub enum SaveError {
  /// The storage could not read or write a file
  #[error("could not access save file \"{path}\"")]
  Io {
    /// The file, as the storage names it
    path: String,
    /// What the storage failed with
    #[source]
    source: std::io::Error,
  },
  /// The save and its backup are damaged
  #[error("save \"{slot}\" is damaged: {reason}")]
  Corrupt {
    /// The slot
    slot: String,
    /// What is wrong with it
    reason: String,
  },
  /// The save was written by a newer build than this one
  #[error("save is version {found}, newer than this build's version {current}")]
  NewerVersion {
    /// The version in the save
    found: u32,
    /// The newest version this build reads
    current: u32,
  },
  /// No save has this slot name
  #[error("no save in slot \"{0}\"")]
  NotFound(String),
  /// The slot name breaks the naming rule: 1 to 64 characters of `a`-`z`,
  /// `0`-`9`, `-` and `_`, and not a Windows device name
  #[error("\"{0}\" is not a slot name: use 1 to 64 of a-z, 0-9, - and _")]
  InvalidSlot(String),
  /// The storage has no room for the save
  #[error("the save storage is full")]
  StorageFull,
  /// There is no storage to save to (blocked browser storage, for one)
  #[error("save storage is unavailable")]
  Unavailable,
  /// The data could not be serialized
  #[error("could not serialize the save data")]
  Serialize(#[source] Box<dyn std::error::Error + Send + Sync>),
  /// The save's data does not deserialize as the type asked for
  #[error("save \"{slot}\" could not be read as the requested type")]
  Deserialize {
    /// The slot
    slot: String,
    /// What deserializing failed with
    #[source]
    source: Box<dyn std::error::Error + Send + Sync>,
  },
}
