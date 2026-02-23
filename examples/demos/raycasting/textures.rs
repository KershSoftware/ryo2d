//! Procedural texture generation for the raycasting demo.

/// Generate a 64x64 RGBA checkerboard texture.
pub fn checkerboard(color_a: [u8; 4], color_b: [u8; 4], cell_size: u32) -> Vec<u8> {
  let size = 64u32;
  let mut data = vec![0u8; (size * size * 4) as usize];
  for y in 0..size {
    for x in 0..size {
      let checker = ((x / cell_size) + (y / cell_size)).is_multiple_of(2);
      let color = if checker { color_a } else { color_b };
      let idx = ((y * size + x) * 4) as usize;
      data[idx..idx + 4].copy_from_slice(&color);
    }
  }
  data
}

/// Generate a 64x64 RGBA brick pattern.
pub fn bricks(mortar: [u8; 4], brick: [u8; 4]) -> Vec<u8> {
  let size = 64u32;
  let mut data = vec![0u8; (size * size * 4) as usize];
  let brick_h = 8u32;
  let brick_w = 16u32;
  let mortar_size = 1u32;

  for y in 0..size {
    for x in 0..size {
      let row = y / brick_h;
      let offset = if row.is_multiple_of(2) {
        0
      } else {
        brick_w / 2
      };
      let lx = (x + offset) % brick_w;
      let ly = y % brick_h;

      let is_mortar = lx < mortar_size || ly < mortar_size;
      let color = if is_mortar { mortar } else { brick };
      let idx = ((y * size + x) * 4) as usize;
      data[idx..idx + 4].copy_from_slice(&color);
    }
  }
  data
}

/// Generate a 64x64 RGBA solid color texture.
pub fn solid(color: [u8; 4]) -> Vec<u8> {
  let size = 64u32;
  let mut data = vec![0u8; (size * size * 4) as usize];
  for pixel in data.chunks_exact_mut(4) {
    pixel.copy_from_slice(&color);
  }
  data
}

/// Generate a 64x64 RGBA barrel/cylinder sprite.
///
/// Draws a rectangular barrel with cylindrical left-right shading,
/// a dark metal band across the middle, and slight bulge highlights
/// to suggest a 3D cylinder viewed from the side.
pub fn barrel(body: [u8; 3], band: [u8; 3]) -> Vec<u8> {
  let size = 64u32;
  let mut data = vec![0u8; (size * size * 4) as usize];
  let margin = 4u32; // transparent border around the barrel
  let left = margin;
  let right = size - margin;
  let top = 2u32; // nearly full height
  let bottom = size - 2;
  let half_w = (right - left) as f32 / 2.0;
  let center_x = (left + right) as f32 / 2.0;

  for y in 0..size {
    for x in 0..size {
      let idx = ((y * size + x) * 4) as usize;

      if x < left || x >= right || y < top || y >= bottom {
        data[idx + 3] = 0; // transparent outside barrel rect
        continue;
      }

      let dx = x as f32 + 0.5 - center_x;
      // Cylindrical shading: darker at left/right edges
      let norm_x = dx / half_w; // -1..1
      let shade = 1.0 - norm_x * norm_x * 0.55;

      // Metal bands: two thin bands near top/bottom, one wider in middle
      let band_mid = (y as f32 - size as f32 * 0.5).abs() < 3.0;
      let band_top = y >= top && y < top + 3;
      let band_bot = y >= bottom - 3 && y < bottom;
      let in_band = band_mid || band_top || band_bot;

      // Slight vertical plank lines
      let plank = (x - left).is_multiple_of(14) && !in_band;

      let base = if in_band {
        band
      } else if plank {
        // Darken plank seams slightly
        [
          (body[0] as f32 * 0.7) as u8,
          (body[1] as f32 * 0.7) as u8,
          (body[2] as f32 * 0.7) as u8,
        ]
      } else {
        body
      };

      data[idx] = (base[0] as f32 * shade).clamp(0.0, 255.0) as u8;
      data[idx + 1] = (base[1] as f32 * shade).clamp(0.0, 255.0) as u8;
      data[idx + 2] = (base[2] as f32 * shade).clamp(0.0, 255.0) as u8;
      data[idx + 3] = 255;
    }
  }
  data
}

/// Generate a 64x64 RGBA torch sprite.
///
/// Draws a small wooden handle at the bottom with a flame shape on top.
/// Most of the texture is transparent so the sprite doesn't look blocky.
pub fn torch(handle: [u8; 3], flame: [u8; 3]) -> Vec<u8> {
  let size = 64u32;
  let mut data = vec![0u8; (size * size * 4) as usize];
  let cx = size as f32 / 2.0;

  for y in 0..size {
    for x in 0..size {
      let idx = ((y * size + x) * 4) as usize;
      let fx = x as f32 + 0.5;
      let fy = y as f32 + 0.5;

      // Handle: narrow rectangle at the bottom (y 44..62, 3px wide)
      if (44..62).contains(&y) && (x as f32 - cx).abs() < 3.0 {
        let shade = 0.7 + 0.3 * (1.0 - (fx - cx).abs() / 3.0);
        data[idx] = (handle[0] as f32 * shade) as u8;
        data[idx + 1] = (handle[1] as f32 * shade) as u8;
        data[idx + 2] = (handle[2] as f32 * shade) as u8;
        data[idx + 3] = 255;
        continue;
      }

      // Flame: teardrop shape from y=8 to y=46
      if (8..46).contains(&y) {
        let t = (fy - 8.0) / 38.0; // 0 at top, 1 at bottom
        // Wider at bottom, narrow at top
        let max_half_w = 10.0 * (t * 0.6 + 0.4).sqrt();
        let dx = (fx - cx).abs();
        if dx < max_half_w {
          let edge = dx / max_half_w; // 0=center, 1=edge
          // Color gradient: white-yellow core, orange-red edge
          let core = 1.0 - edge * edge;
          let r = (flame[0] as f32 + (255.0 - flame[0] as f32) * core).clamp(0.0, 255.0);
          let g = (flame[1] as f32 * (0.5 + 0.5 * core)).clamp(0.0, 255.0);
          let b = (flame[2] as f32 * core * 0.5).clamp(0.0, 255.0);
          // Alpha fades at edges
          let alpha = ((1.0 - edge) * 255.0).clamp(0.0, 255.0);
          data[idx] = r as u8;
          data[idx + 1] = g as u8;
          data[idx + 2] = b as u8;
          data[idx + 3] = alpha as u8;
          continue;
        }
      }

      // Everything else: transparent
      data[idx + 3] = 0;
    }
  }
  data
}

/// Generate a 64x64 RGBA noisy stone texture.
pub fn stone(base: [u8; 3]) -> Vec<u8> {
  let size = 64u32;
  let mut data = vec![0u8; (size * size * 4) as usize];
  // Simple hash-based noise
  for y in 0..size {
    for x in 0..size {
      let hash = (x
        .wrapping_mul(374761393)
        .wrapping_add(y.wrapping_mul(668265263))
        ^ 0x5bd1e995)
        .wrapping_mul(0x27d4eb2d);
      let noise = ((hash >> 15) & 0x1F) as i32 - 16; // -16 to +15
      let r = (base[0] as i32 + noise).clamp(0, 255) as u8;
      let g = (base[1] as i32 + noise).clamp(0, 255) as u8;
      let b = (base[2] as i32 + noise).clamp(0, 255) as u8;
      let idx = ((y * size + x) * 4) as usize;
      data[idx] = r;
      data[idx + 1] = g;
      data[idx + 2] = b;
      data[idx + 3] = 255;
    }
  }
  data
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn checkerboard_size() {
    let tex = checkerboard([255, 255, 255, 255], [0, 0, 0, 255], 8);
    assert_eq!(tex.len(), 64 * 64 * 4);
  }

  #[test]
  fn checkerboard_pattern() {
    let tex = checkerboard([255, 0, 0, 255], [0, 255, 0, 255], 32);
    // Top-left pixel should be color_a (red)
    assert_eq!(&tex[0..4], &[255, 0, 0, 255]);
    // Pixel at (32, 0) should be color_b (green)
    let idx = (32 * 4) as usize;
    assert_eq!(&tex[idx..idx + 4], &[0, 255, 0, 255]);
  }

  #[test]
  fn bricks_size() {
    let tex = bricks([128, 128, 128, 255], [180, 60, 30, 255]);
    assert_eq!(tex.len(), 64 * 64 * 4);
  }

  #[test]
  fn solid_uniform() {
    let color = [42, 128, 200, 255];
    let tex = solid(color);
    assert_eq!(tex.len(), 64 * 64 * 4);
    for pixel in tex.chunks_exact(4) {
      assert_eq!(pixel, &color);
    }
  }

  #[test]
  fn stone_size_and_alpha() {
    let tex = stone([128, 128, 128]);
    assert_eq!(tex.len(), 64 * 64 * 4);
    for pixel in tex.chunks_exact(4) {
      assert_eq!(pixel[3], 255);
    }
  }

  #[test]
  fn barrel_size_and_transparency() {
    let tex = barrel([139, 90, 43], [100, 65, 30]);
    assert_eq!(tex.len(), 64 * 64 * 4);
    assert_eq!(tex[3], 0);
    let center_idx = (32 * 64 + 32) * 4;
    assert_eq!(tex[center_idx + 3], 255);
  }

  #[test]
  fn torch_size_and_shape() {
    let tex = torch([100, 70, 40], [255, 180, 50]);
    assert_eq!(tex.len(), 64 * 64 * 4);
    assert_eq!(tex[3], 0);
    let handle_idx = (50 * 64 + 32) * 4;
    assert_eq!(tex[handle_idx + 3], 255);
  }
}
