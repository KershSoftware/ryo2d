//! The save file: a header read without the data, checksums, and the game's
//! data as compressed RON.
//!
//! Format 2, every integer a little-endian u32:
//!
//! | Bytes | Content |
//! |---|---|
//! | 0-4 | `RYO2` |
//! | 4-8 | the format, 2 |
//! | 8-12 | the header's length |
//! | 12-16 | the body's length |
//! | 16-20 | the header's CRC-32 |
//! | 20-24 | the body's CRC-32 |
//! | 24 on | the header ([`SaveHeader`] as JSON), then the body (deflated RON) |
//!
//! Format 1, the old save manager's, is `RYO2`, 1, then the deflated RON of
//! the whole save: the data and its timestamp, name, play time and metadata.

use std::collections::BTreeMap;
use std::io::{Read, Write};

use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use flate2::{Compression, Crc};
use serde::de::IgnoredAny;
use serde::{Deserialize, Serialize};

use super::SaveError;

const MAGIC: &[u8; 4] = b"RYO2";
/// The format this build writes, and the newest it reads
pub(crate) const FORMAT: u32 = 2;
const LEGACY_FORMAT: u32 = 1;
const PREFIX_LEN: usize = 24;
const LEVEL: u32 = 6;

/// What a save says about itself, readable without loading its data: enough
/// to list the slots
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveHeader {
  /// The file's format: 2 for today's, 1 for a save from before headers
  pub format: u32,
  /// The version of the game's data type the save holds
  pub version: u32,
  /// When it was saved, in seconds since the Unix epoch
  pub saved_at: u64,
  /// The play time the game gave, in seconds
  pub play_time: u64,
  /// The display name the game gave
  pub name: Option<String>,
  /// The game's labels
  pub labels: BTreeMap<String, String>,
  /// The version of the engine that wrote it; empty in format 1, which did not
  /// record it
  pub engine_version: String,
}

/// A save's data as RON
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Body {
  /// The RON text
  pub ron: String,
  /// Whether this is a format-1 save, whose RON is the whole old save with the
  /// data in its `data` field
  pub legacy: bool,
}

/// The format-1 save as RON, its data left unread
#[derive(Deserialize)]
struct LegacySave {
  #[serde(rename = "data")]
  _data: IgnoredAny,
  timestamp: u64,
  name: Option<String>,
  play_time: u64,
  #[serde(default)]
  metadata: BTreeMap<String, String>,
}

/// The file bytes of a save: `header` (written with today's format) and the
/// data as RON
pub(crate) fn encode(header: &SaveHeader, body_ron: &str) -> Result<Vec<u8>, SaveError> {
  let header = SaveHeader {
    format: FORMAT,
    ..header.clone()
  };
  let header = serde_json::to_vec(&header).map_err(|e| SaveError::Serialize(Box::new(e)))?;
  let mut encoder = DeflateEncoder::new(Vec::new(), Compression::new(LEVEL));
  encoder
    .write_all(body_ron.as_bytes())
    .map_err(|e| SaveError::Serialize(Box::new(e)))?;
  let body = encoder
    .finish()
    .map_err(|e| SaveError::Serialize(Box::new(e)))?;

  let mut bytes = Vec::with_capacity(PREFIX_LEN + header.len() + body.len());
  bytes.extend_from_slice(MAGIC);
  for word in [
    FORMAT,
    length(&header)?,
    length(&body)?,
    crc(&header),
    crc(&body),
  ] {
    bytes.extend_from_slice(&word.to_le_bytes());
  }
  bytes.extend_from_slice(&header);
  bytes.extend_from_slice(&body);
  Ok(bytes)
}

/// The header of a save, checking only the header: a file whose body is cut
/// off or damaged still gives its header
pub(crate) fn read_header(bytes: &[u8]) -> Result<SaveHeader, SaveError> {
  match readable_format(bytes)? {
    LEGACY_FORMAT => read_legacy(bytes).map(|(header, _)| header),
    _ => split(bytes).map(|parts| parts.header),
  }
}

/// Checks the whole file: its save time for a whole save this build reads,
/// else `None`
pub(crate) fn verify(bytes: &[u8]) -> Option<u64> {
  match readable_format(bytes).ok()? {
    LEGACY_FORMAT => read_legacy(bytes).ok().map(|(header, _)| header.saved_at),
    _ => {
      let parts = split(bytes).ok()?;
      parts.body().ok()?;
      Some(parts.header.saved_at)
    }
  }
}

/// The header and the data of a whole save
pub(crate) fn decode(bytes: &[u8]) -> Result<(SaveHeader, Body), SaveError> {
  match readable_format(bytes)? {
    LEGACY_FORMAT => {
      let (header, ron) = read_legacy(bytes)?;
      Ok((header, Body { ron, legacy: true }))
    }
    _ => {
      let parts = split(bytes)?;
      let inflated = inflate(parts.body()?)?;
      let ron = String::from_utf8(inflated).map_err(|_| corrupt("the body is not UTF-8"))?;
      Ok((parts.header, Body { ron, legacy: false }))
    }
  }
}

/// The format of a file that starts with the `RYO2` magic, whether this build
/// reads it or not
pub(crate) fn peek_format(bytes: &[u8]) -> Option<u32> {
  if bytes.get(..4)? != MAGIC {
    return None;
  }
  word(bytes, 4)
}

/// The file's format, if this build reads it
fn readable_format(bytes: &[u8]) -> Result<u32, SaveError> {
  match peek_format(bytes) {
    None | Some(0) => Err(corrupt("not a Ryo2D save")),
    Some(found) if found > FORMAT => Err(SaveError::NewerVersion {
      found,
      current: FORMAT,
    }),
    Some(format) => Ok(format),
  }
}

/// A format-2 file with its header read and checked, and its body not yet
struct Parts<'a> {
  header: SaveHeader,
  rest: &'a [u8],
  body_len: u32,
  body_crc: u32,
}

impl<'a> Parts<'a> {
  /// The compressed body, checked against its length and checksum
  fn body(&self) -> Result<&'a [u8], SaveError> {
    let body_len = usize::try_from(self.body_len).unwrap_or(usize::MAX);
    if self.rest.len() < body_len {
      return Err(corrupt("the body is cut off"));
    }
    if self.rest.len() > body_len {
      return Err(corrupt("the file goes on past its body"));
    }
    if crc(self.rest) != self.body_crc {
      return Err(corrupt("the body's checksum does not match"));
    }
    Ok(self.rest)
  }
}

fn split(bytes: &[u8]) -> Result<Parts<'_>, SaveError> {
  let (Some(header_len), Some(body_len), Some(header_crc), Some(body_crc)) = (
    word(bytes, 8),
    word(bytes, 12),
    word(bytes, 16),
    word(bytes, 20),
  ) else {
    return Err(corrupt("the file is cut off before its header"));
  };
  let header_end = usize::try_from(header_len)
    .ok()
    .and_then(|len| PREFIX_LEN.checked_add(len))
    .filter(|end| *end <= bytes.len())
    .ok_or_else(|| corrupt("the header is cut off"))?;
  let (header, rest) = bytes[PREFIX_LEN..].split_at(header_end - PREFIX_LEN);
  if crc(header) != header_crc {
    return Err(corrupt("the header's checksum does not match"));
  }
  let mut header: SaveHeader = serde_json::from_slice(header)
    .map_err(|e| corrupt(format!("the header does not parse: {e}")))?;
  header.format = FORMAT;
  Ok(Parts {
    header,
    rest,
    body_len,
    body_crc,
  })
}

/// A format-1 save's header, from its own fields, and its whole RON
fn read_legacy(bytes: &[u8]) -> Result<(SaveHeader, String), SaveError> {
  let inflated = inflate(bytes.get(8..).unwrap_or_default())?;
  // The old reader took the text lossily, so this does too
  let ron = String::from_utf8_lossy(&inflated).into_owned();
  let save: LegacySave =
    ron::from_str(&ron).map_err(|e| corrupt(format!("the save does not parse: {e}")))?;
  let header = SaveHeader {
    format: LEGACY_FORMAT,
    version: 1,
    saved_at: save.timestamp,
    play_time: save.play_time,
    name: save.name,
    labels: save.metadata,
    engine_version: String::new(),
  };
  Ok((header, ron))
}

fn inflate(compressed: &[u8]) -> Result<Vec<u8>, SaveError> {
  let mut inflated = Vec::new();
  DeflateDecoder::new(compressed)
    .read_to_end(&mut inflated)
    .map_err(|e| corrupt(format!("the body does not inflate: {e}")))?;
  Ok(inflated)
}

fn word(bytes: &[u8], at: usize) -> Option<u32> {
  let word = bytes.get(at..at.checked_add(4)?)?;
  Some(u32::from_le_bytes(word.try_into().ok()?))
}

fn length(part: &[u8]) -> Result<u32, SaveError> {
  u32::try_from(part.len())
    .map_err(|_| SaveError::Serialize("the save is larger than 4 GiB".into()))
}

fn crc(bytes: &[u8]) -> u32 {
  let mut crc = Crc::new();
  crc.update(bytes);
  crc.sum()
}

/// A damaged-file error; the caller names the slot
fn corrupt(reason: impl Into<String>) -> SaveError {
  SaveError::Corrupt {
    slot: String::new(),
    reason: reason.into(),
  }
}

#[cfg(test)]
pub(crate) mod fixtures {
  use serde::{Deserialize, Serialize};

  /// A format-1 save written by the old save manager (see
  /// `tests/fixtures/saves/README.md`)
  pub(crate) const LEGACY_SAVE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/saves/legacy.ryo-save"
  ));

  /// The data in [`LEGACY_SAVE`]
  #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
  pub(crate) struct LegacyFixture {
    pub(crate) level: u32,
    pub(crate) name: String,
    pub(crate) items: Vec<String>,
  }

  impl LegacyFixture {
    /// The values the fixture was written with
    pub(crate) fn written() -> Self {
      LegacyFixture {
        level: 3,
        name: "Ada".into(),
        items: vec!["key".into(), "lamp".into()],
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;

  use serde::Deserialize;

  use super::fixtures::{LEGACY_SAVE, LegacyFixture};
  use super::*;
  use crate::core::saves::SaveError;

  const BODY: &str = "(hp:7,items:[\"key\",\"lamp\"])";
  const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x10\0\0\0\x10\x08\x06\0\0\0";

  fn header() -> SaveHeader {
    SaveHeader {
      format: 2,
      version: 3,
      saved_at: 1_800_000_000,
      play_time: 42,
      name: Some("Chapter two".into()),
      labels: BTreeMap::from([("area".into(), "caves".into())]),
      engine_version: env!("CARGO_PKG_VERSION").into(),
    }
  }

  fn word(bytes: &[u8], at: usize) -> usize {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize
  }

  fn assert_corrupt<T: std::fmt::Debug>(result: Result<T, SaveError>, expected: &str) {
    match result {
      Err(SaveError::Corrupt { reason, .. }) => assert_eq!(reason, expected),
      other => panic!("expected Corrupt ({expected}), got {other:?}"),
    }
  }

  #[test]
  fn a_save_round_trips_with_its_header() {
    let bytes = encode(&header(), BODY).unwrap();
    assert_eq!(&bytes[..4], b"RYO2");
    assert_eq!(word(&bytes, 4), 2);
    assert_eq!(read_header(&bytes).unwrap(), header());
    assert_eq!(verify(&bytes), Some(1_800_000_000));

    let (read, body) = decode(&bytes).unwrap();
    assert_eq!(read, header());
    assert_eq!(body.ron, BODY);
    assert!(!body.legacy);
  }

  #[test]
  fn the_header_reads_without_the_body() {
    let bytes = encode(&header(), BODY).unwrap();
    let header_end = 24 + word(&bytes, 8);
    for cut in [header_end, header_end + 3, bytes.len() - 1] {
      let cut = &bytes[..cut];
      assert_eq!(read_header(cut).unwrap(), header());
      assert_eq!(verify(cut), None);
      assert_corrupt(decode(cut), "the body is cut off");
    }
    assert_corrupt(
      read_header(&bytes[..header_end - 1]),
      "the header is cut off",
    );
    assert_corrupt(
      read_header(&bytes[..20]),
      "the file is cut off before its header",
    );
  }

  #[test]
  fn a_flipped_byte_in_the_header_or_the_body_fails_verification() {
    let bytes = encode(&header(), BODY).unwrap();
    let header_end = 24 + word(&bytes, 8);

    let mut damaged = bytes.clone();
    damaged[24 + 10] ^= 0x20;
    assert_eq!(verify(&damaged), None);
    let reason = "the header's checksum does not match";
    assert_corrupt(read_header(&damaged), reason);
    assert_corrupt(decode(&damaged), reason);

    let mut damaged = bytes.clone();
    damaged[header_end + 1] ^= 0x20;
    assert_eq!(verify(&damaged), None);
    assert_eq!(read_header(&damaged).unwrap(), header());
    assert_corrupt(decode(&damaged), "the body's checksum does not match");

    let mut longer = bytes.clone();
    longer.push(0);
    assert_eq!(verify(&longer), None);
    assert_corrupt(decode(&longer), "the file goes on past its body");
  }

  #[test]
  fn a_file_of_another_kind_is_not_a_save() {
    for bytes in [PNG, b"RYO", b""] {
      assert_corrupt(read_header(bytes), "not a Ryo2D save");
      assert_corrupt(decode(bytes), "not a Ryo2D save");
      assert_eq!(verify(bytes), None);
    }
  }

  #[test]
  fn a_newer_container_format_is_refused() {
    let mut bytes = encode(&header(), BODY).unwrap();
    bytes[4..8].copy_from_slice(&3u32.to_le_bytes());
    assert!(matches!(
      read_header(&bytes),
      Err(SaveError::NewerVersion {
        found: 3,
        current: 2
      })
    ));
    assert!(matches!(
      decode(&bytes),
      Err(SaveError::NewerVersion {
        found: 3,
        current: 2
      })
    ));
    assert_eq!(verify(&bytes), None);
  }

  #[test]
  fn the_legacy_fixture_reads_with_its_header() {
    let header = read_header(LEGACY_SAVE).unwrap();
    assert_eq!(header.format, 1);
    assert_eq!(header.version, 1);
    assert_eq!(header.saved_at, 1_700_000_000);
    assert_eq!(header.name.as_deref(), Some("Before the update"));
    assert_eq!(header.play_time, 754);
    assert_eq!(
      header.labels,
      BTreeMap::from([("chapter".to_string(), "2".to_string())])
    );
    assert_eq!(header.engine_version, "");
    assert_eq!(verify(LEGACY_SAVE), Some(1_700_000_000));

    let (decoded, body) = decode(LEGACY_SAVE).unwrap();
    assert_eq!(decoded, header);
    assert!(body.legacy);
    #[derive(Deserialize)]
    struct Wrapped {
      data: LegacyFixture,
    }
    let wrapped: Wrapped = ron::from_str(&body.ron).unwrap();
    assert_eq!(wrapped.data, LegacyFixture::written());

    let cut = &LEGACY_SAVE[..LEGACY_SAVE.len() - 10];
    assert_eq!(verify(cut), None);
    assert!(matches!(decode(cut), Err(SaveError::Corrupt { .. })));
  }

  #[test]
  fn peek_format_reads_the_format_of_any_ryo2_file() {
    assert_eq!(peek_format(LEGACY_SAVE), Some(1));
    assert_eq!(peek_format(&encode(&header(), BODY).unwrap()), Some(2));
    let mut made = b"RYO2".to_vec();
    made.extend_from_slice(&9u32.to_le_bytes());
    assert_eq!(peek_format(&made), Some(9));
    assert_eq!(peek_format(PNG), None);
    assert_eq!(peek_format(b"RYO2\x02\0"), None);
  }
}
