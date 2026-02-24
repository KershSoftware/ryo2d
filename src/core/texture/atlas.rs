/// Slice raw RGBA image data into individual tile buffers.
///
/// Tiles are returned in row-major order (left-to-right, top-to-bottom).
/// The image dimensions must be evenly divisible by tile dimensions.
///
/// # Panics
/// Panics if `data.len() != img_width * img_height * 4` or if dimensions are not divisible.
pub fn slice_atlas(
  data: &[u8],
  img_width: u32,
  img_height: u32,
  tile_width: u32,
  tile_height: u32,
) -> Vec<Vec<u8>> {
  assert_eq!(data.len(), (img_width * img_height * 4) as usize);
  assert!(tile_width > 0 && tile_height > 0);
  assert!(
    img_width.is_multiple_of(tile_width) && img_height.is_multiple_of(tile_height),
    "Image dimensions ({img_width}x{img_height}) must be evenly divisible by tile dimensions ({tile_width}x{tile_height})"
  );

  let cols = img_width / tile_width;
  let rows = img_height / tile_height;
  let tile_bytes = (tile_width * tile_height * 4) as usize;
  let img_stride = (img_width * 4) as usize;
  let tile_stride = (tile_width * 4) as usize;

  let mut tiles = Vec::with_capacity((cols * rows) as usize);

  for row in 0..rows {
    for col in 0..cols {
      let mut tile = Vec::with_capacity(tile_bytes);
      for y in 0..tile_height {
        let src_y = row * tile_height + y;
        let src_x = col * tile_width;
        let offset = (src_y as usize * img_stride) + (src_x as usize * 4);
        tile.extend_from_slice(&data[offset..offset + tile_stride]);
      }
      tiles.push(tile);
    }
  }

  tiles
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_slice_2x2_atlas() {
    // 4x4 pixel image (RGBA), sliced into 2x2 tiles -> 4 tiles
    // Layout (each letter = one 2x2 tile region):
    //   A A B B
    //   A A B B
    //   C C D D
    //   C C D D
    let mut data = vec![0u8; 4 * 4 * 4];

    // Fill tile A (top-left 2x2) with value 10
    // Fill tile B (top-right 2x2) with value 20
    // Fill tile C (bottom-left 2x2) with value 30
    // Fill tile D (bottom-right 2x2) with value 40
    let img_w = 4usize;
    for y in 0..4usize {
      for x in 0..4usize {
        let val: u8 = match (x / 2, y / 2) {
          (0, 0) => 10, // A
          (1, 0) => 20, // B
          (0, 1) => 30, // C
          (1, 1) => 40, // D
          _ => unreachable!(),
        };
        let offset = (y * img_w + x) * 4;
        data[offset] = val;
        data[offset + 1] = val;
        data[offset + 2] = val;
        data[offset + 3] = val;
      }
    }

    let tiles = slice_atlas(&data, 4, 4, 2, 2);
    assert_eq!(tiles.len(), 4);

    // Tile 0 = A: all pixels should be 10
    assert!(tiles[0].iter().all(|&b| b == 10));
    // Tile 1 = B: all pixels should be 20
    assert!(tiles[1].iter().all(|&b| b == 20));
    // Tile 2 = C: all pixels should be 30
    assert!(tiles[2].iter().all(|&b| b == 30));
    // Tile 3 = D: all pixels should be 40
    assert!(tiles[3].iter().all(|&b| b == 40));

    // Each tile should be 2*2*4 = 16 bytes
    for tile in &tiles {
      assert_eq!(tile.len(), 16);
    }
  }

  #[test]
  fn test_slice_single_tile() {
    // Image same size as tile -> 1 tile with identical data
    let data: Vec<u8> = (0..64).collect(); // 4x4 RGBA = 64 bytes
    let tiles = slice_atlas(&data, 4, 4, 4, 4);
    assert_eq!(tiles.len(), 1);
    assert_eq!(tiles[0], data);
  }

  #[test]
  fn test_slice_row_major_order() {
    // 4x2 image with 2x2 tiles -> 2 tiles
    // First tile is the left half, second is the right half
    let mut data = vec![0u8; 4 * 2 * 4];
    let img_w = 4usize;

    // Left half (x=0..2) filled with 0xAA
    // Right half (x=2..4) filled with 0xBB
    for y in 0..2usize {
      for x in 0..4usize {
        let val: u8 = if x < 2 { 0xAA } else { 0xBB };
        let offset = (y * img_w + x) * 4;
        data[offset] = val;
        data[offset + 1] = val;
        data[offset + 2] = val;
        data[offset + 3] = val;
      }
    }

    let tiles = slice_atlas(&data, 4, 2, 2, 2);
    assert_eq!(tiles.len(), 2);

    // First tile (left half) should be all 0xAA
    assert!(tiles[0].iter().all(|&b| b == 0xAA));
    // Second tile (right half) should be all 0xBB
    assert!(tiles[1].iter().all(|&b| b == 0xBB));
  }

  #[test]
  fn test_slice_preserves_rgba() {
    // Create a 2x1 pixel image (one tile) with distinct R, G, B, A values
    // Pixel (0,0): R=10, G=20, B=30, A=40
    // Pixel (1,0): R=50, G=60, B=70, A=80
    let data = vec![10, 20, 30, 40, 50, 60, 70, 80];
    let tiles = slice_atlas(&data, 2, 1, 2, 1);
    assert_eq!(tiles.len(), 1);
    assert_eq!(tiles[0], vec![10, 20, 30, 40, 50, 60, 70, 80]);
  }

  #[test]
  #[should_panic]
  fn test_slice_panics_not_divisible() {
    // 3x3 image with 2x2 tiles -> should panic
    let data = vec![0u8; 3 * 3 * 4];
    slice_atlas(&data, 3, 3, 2, 2);
  }

  #[test]
  #[should_panic]
  fn test_slice_panics_wrong_data_length() {
    // Claim 4x4 image but provide wrong data length
    let data = vec![0u8; 10]; // wrong length, should be 4*4*4 = 64
    slice_atlas(&data, 4, 4, 2, 2);
  }
}
