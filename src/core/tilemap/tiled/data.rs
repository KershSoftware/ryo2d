//! Layer data: gids and the encodings they come in.

use std::io::Read;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use flate2::read::{GzDecoder, ZlibDecoder};

use super::raw::RawData;
use crate::core::tilemap::TileFlip;

const FLIP_H: u32 = 1 << 31;
const FLIP_V: u32 = 1 << 30;
/// The diagonal flip, or on a hexagonal map a 60 degree turn
const FLIP_D: u32 = 1 << 29;
/// A hexagonal map's 120 degree turn, which Tiled ignores on other grids:
/// the map's build drops it there
const ROTATE_120: u32 = 1 << 28;
const FLAG_BITS: u32 = 0xF000_0000;

/// A gid split into its number (0 for empty) and its flips, all four
pub(crate) fn decode_gid(gid: u32) -> (u32, TileFlip) {
  let mut bits = 0;
  for (flag, flip) in [
    (FLIP_H, TileFlip::HORIZONTAL),
    (FLIP_V, TileFlip::VERTICAL),
    (FLIP_D, TileFlip::DIAGONAL),
    (ROTATE_120, TileFlip::HEX_ROTATE_120),
  ] {
    if gid & flag != 0 {
      bits |= flip.bits();
    }
  }
  (gid & !FLAG_BITS, TileFlip::from_bits(bits))
}

/// The `expected` gids of a layer's data. The error says what is wrong with
/// the data, for the caller to put the layer's place before
pub(crate) fn decode_layer_data(
  data: &RawData,
  encoding: Option<&str>,
  compression: Option<&str>,
  expected: usize,
) -> Result<Vec<u32>, String> {
  // Tiled writes an empty compression for plain base64
  let encoding = encoding.filter(|e| !e.is_empty());
  let compression = compression.filter(|c| !c.is_empty());
  let gids = match (data, encoding) {
    (RawData::Array(gids), None | Some("csv")) => {
      if compression.is_some() {
        return Err("compression needs base64 data".into());
      }
      gids.clone()
    }
    (RawData::Text(text), None | Some("csv")) => {
      if compression.is_some() {
        return Err("compression needs base64 data".into());
      }
      parse_csv(text, expected)?
    }
    (RawData::Text(text), Some("base64")) => decode_base64(text, compression, expected)?,
    (RawData::Array(_), Some("base64")) => {
      return Err("the data is a list of numbers, not base64 text".into());
    }
    (_, Some(other)) => return Err(format!("unknown encoding {other:?}")),
  };
  check_count(gids.len(), expected)?;
  Ok(gids)
}

fn check_count(found: usize, expected: usize) -> Result<(), String> {
  if found == expected {
    Ok(())
  } else {
    Err(format!("has {found} cells, expected {expected}"))
  }
}

fn parse_csv(text: &str, expected: usize) -> Result<Vec<u32>, String> {
  let text = text.trim().trim_end_matches(',');
  let mut gids = Vec::with_capacity(expected.min(1 << 20));
  if text.trim().is_empty() {
    return Ok(gids);
  }
  for (i, part) in text.split(',').enumerate() {
    if gids.len() >= expected {
      return Err(format!("has more than {expected} cells"));
    }
    let part = part.trim();
    let gid = part
      .parse::<u32>()
      .map_err(|_| format!("cell {i}: {part:?} is not a gid"))?;
    gids.push(gid);
  }
  Ok(gids)
}

fn decode_base64(
  text: &str,
  compression: Option<&str>,
  expected: usize,
) -> Result<Vec<u32>, String> {
  let packed: String = text.split_whitespace().collect();
  let raw = STANDARD
    .decode(packed)
    .map_err(|e| format!("the base64 data is broken: {e}"))?;
  let want = expected
    .checked_mul(4)
    .ok_or_else(|| "the layer is too large".to_string())?;
  let bytes = match compression {
    None => raw,
    Some("gzip") => read_limited(GzDecoder::new(raw.as_slice()), want, "gzip")?,
    Some("zlib") => read_limited(ZlibDecoder::new(raw.as_slice()), want, "zlib")?,
    Some("zstd") => {
      let decoder = ruzstd::decoding::StreamingDecoder::new(raw.as_slice())
        .map_err(|e| format!("the zstd data is broken: {e}"))?;
      read_limited(decoder, want, "zstd")?
    }
    Some(other) => return Err(format!("unknown compression {other:?}")),
  };
  if bytes.len() > want {
    return Err(format!("has more than {expected} cells"));
  }

  if bytes.len() % 4 != 0 {
    return Err(format!(
      "the data is {} bytes, not a whole number of 4-byte cells",
      bytes.len()
    ));
  }
  Ok(
    bytes
      .as_chunks::<4>()
      .0
      .iter()
      .map(|c| u32::from_le_bytes(*c))
      .collect(),
  )
}

/// Reads `reader` to its end, but one byte past `want` at most, so a
/// small file cannot inflate into memory
fn read_limited(reader: impl Read, want: usize, what: &str) -> Result<Vec<u8>, String> {
  let mut out = Vec::with_capacity(want.min(1 << 22));
  reader
    .take(want as u64 + 1)
    .read_to_end(&mut out)
    .map_err(|e| format!("the {what} data is broken: {e}"))?;
  Ok(out)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn text(s: &str) -> RawData {
    RawData::Text(s.to_string())
  }

  #[test]
  fn csv_text_and_arrays_decode_alike() {
    let want = vec![1, 0, 3, 2_147_483_649];
    let from_text = decode_layer_data(&text("1,0,\n3,2147483649,\n"), Some("csv"), None, 4);
    assert_eq!(from_text.as_deref(), Ok(want.as_slice()));
    let array = RawData::Array(want.clone());
    assert_eq!(decode_layer_data(&array, None, None, 4), Ok(want));
    assert!(decode_layer_data(&text("1,2,x"), Some("csv"), None, 3).is_err());
    let error = decode_layer_data(&text("1,2,3,4,5"), Some("csv"), None, 4);
    assert!(error.is_err_and(|e| e.contains("more than 4")));
  }

  #[test]
  fn base64_decodes_plain_and_compressed() {
    let raw: Vec<u8> = [1u32, 2, 3, 4]
      .iter()
      .flat_map(|g| g.to_le_bytes())
      .collect();
    let b64 = STANDARD.encode(&raw);
    let got = decode_layer_data(&text(&b64), Some("base64"), Some(""), 4);
    assert_eq!(got, Ok(vec![1, 2, 3, 4]));
    let got = decode_layer_data(&text(&format!("\n  {b64}\n")), Some("base64"), None, 4);
    assert_eq!(got, Ok(vec![1, 2, 3, 4]));
    assert!(decode_layer_data(&text(&b64), Some("base64"), None, 5).is_err());
    assert!(decode_layer_data(&text("!!!"), Some("base64"), None, 1).is_err());
    let error = decode_layer_data(&text(&b64), Some("base64"), Some("lz4"), 4);
    assert!(error.is_err_and(|e| e.contains("lz4")));
    let error = decode_layer_data(&text(&b64), Some("rot13"), None, 4);
    assert!(error.is_err_and(|e| e.contains("rot13")));
  }

  #[test]
  fn a_small_file_cannot_inflate_past_its_layer() {
    use std::io::Write;
    let zeros = vec![0u8; 8 << 20];
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    z.write_all(&zeros).expect("write");
    let packed = STANDARD.encode(z.finish().expect("finish"));
    assert!(packed.len() < 20_000);
    let error = decode_layer_data(&text(&packed), Some("base64"), Some("zlib"), 4);
    assert!(error.is_err_and(|e| e.contains("more than 4")));
  }

  /// `bytes` compressed as `how` ("gzip", "zlib" or "zstd")
  fn pack(bytes: &[u8], how: &str) -> Vec<u8> {
    use std::io::Write;
    match how {
      "gzip" => {
        let mut z = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
        z.write_all(bytes).expect("write");
        z.finish().expect("finish")
      }
      "zlib" => {
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
        z.write_all(bytes).expect("write");
        z.finish().expect("finish")
      }
      _ => ruzstd::encoding::compress_to_vec(bytes, ruzstd::encoding::CompressionLevel::Fastest),
    }
  }

  /// A layer of `n` cells, gids 1 to `n`, as little-endian bytes
  fn cells(n: u32) -> Vec<u8> {
    (1..=n).flat_map(|g| g.to_le_bytes()).collect()
  }

  #[test]
  fn every_compression_decodes_its_layer() {
    for how in ["gzip", "zlib", "zstd"] {
      let packed = STANDARD.encode(pack(&cells(64), how));
      let got = decode_layer_data(&text(&packed), Some("base64"), Some(how), 64);
      assert_eq!(got, Ok((1..=64).collect::<Vec<u32>>()), "{how}");
    }
  }

  #[test]
  fn truncated_or_corrupt_compressed_data_is_an_error_naming_it() {
    for how in ["gzip", "zlib", "zstd"] {
      let whole = pack(&cells(256), how);
      // cut short: the stream ends early (zlib's last four bytes are only
      // its checksum, which flate2 does not miss once the cells are whole)
      for keep in [whole.len() / 2, whole.len() - 8, 3] {
        let packed = STANDARD.encode(&whole[..keep]);
        let error = decode_layer_data(&text(&packed), Some("base64"), Some(how), 256);
        assert!(error.is_err(), "{how} cut to {keep} bytes decoded");
      }
      // bytes overwritten past the header
      let mut broken = whole.clone();
      let start = broken.len() / 3;
      for b in &mut broken[start..start + 8] {
        *b ^= 0xA5;
      }
      let packed = STANDARD.encode(&broken);
      let decoded = decode_layer_data(&text(&packed), Some("base64"), Some(how), 256);
      if how == "zstd" {
        // Tiled writes zstd frames without a checksum, so bytes changed
        // inside one may decode to other cells: never more than the layer's
        assert!(
          decoded.as_ref().map_or(true, |gids| gids.len() == 256),
          "{how}: {decoded:?}"
        );
      } else {
        assert!(decoded.is_err(), "{how} corrupted decoded");
      }
      // not the format at all
      let packed = STANDARD.encode(b"plainly not compressed data, long enough");
      let error = decode_layer_data(&text(&packed), Some("base64"), Some(how), 4);
      assert!(
        error.as_ref().is_err_and(|e| e.contains(how)),
        "{how}: {error:?}"
      );
    }
  }

  #[test]
  fn no_compression_inflates_a_small_file_past_its_layer() {
    let zeros = vec![0u8; 8 << 20];
    for how in ["gzip", "zlib", "zstd"] {
      let packed = STANDARD.encode(pack(&zeros, how));
      assert!(packed.len() < 400_000, "{how}: {} bytes", packed.len());
      let error = decode_layer_data(&text(&packed), Some("base64"), Some(how), 4);
      assert!(error.is_err_and(|e| e.contains("more than 4")), "{how}");
    }
  }

  #[test]
  fn gids_lose_their_flag_bits() {
    assert_eq!(
      decode_gid(0xF000_0000),
      (
        0,
        TileFlip::HORIZONTAL | TileFlip::VERTICAL | TileFlip::DIAGONAL | TileFlip::HEX_ROTATE_120
      )
    );
  }
}
