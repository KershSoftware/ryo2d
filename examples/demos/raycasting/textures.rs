//! Procedural texture generation for the raycasting demo.

/// Generate an RGBA checkerboard texture at the given size.
pub fn checkerboard(color_a: [u8; 4], color_b: [u8; 4], cell_size: u32, size: u32) -> Vec<u8> {
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

/// Generate an RGBA brick pattern at the given size.
pub fn bricks(mortar: [u8; 4], brick: [u8; 4], size: u32) -> Vec<u8> {
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

/// Generate an RGBA solid color texture at the given size.
pub fn solid(color: [u8; 4], size: u32) -> Vec<u8> {
  let mut data = vec![0u8; (size * size * 4) as usize];
  for pixel in data.as_chunks_mut::<4>().0 {
    pixel.copy_from_slice(&color);
  }
  data
}

/// Generate an RGBA barrel/cylinder sprite at the given size.
///
/// Draws a rectangular barrel with cylindrical left-right shading,
/// a dark metal band across the middle, and slight bulge highlights
/// to suggest a 3D cylinder viewed from the side.
pub fn barrel(body: [u8; 3], band: [u8; 3], size: u32) -> Vec<u8> {
  let mut data = vec![0u8; (size * size * 4) as usize];
  let s = size as f32 / 64.0;
  let margin = (4.0 * s) as u32;
  let left = margin;
  let right = size - margin;
  let top = (2.0 * s) as u32;
  let bottom = size - (2.0 * s) as u32;
  let half_w = (right - left) as f32 / 2.0;
  let center_x = (left + right) as f32 / 2.0;
  let band_thickness = (3.0 * s).max(1.0);
  let plank_spacing = (14.0 * s).max(2.0) as u32;

  for y in 0..size {
    for x in 0..size {
      let idx = ((y * size + x) * 4) as usize;

      if x < left || x >= right || y < top || y >= bottom {
        data[idx + 3] = 0;
        continue;
      }

      let dx = x as f32 + 0.5 - center_x;
      let norm_x = dx / half_w;
      let shade = 1.0 - norm_x * norm_x * 0.55;

      let band_mid = (y as f32 - size as f32 * 0.5).abs() < band_thickness;
      let band_top = y >= top && y < top + band_thickness as u32;
      let band_bot = y >= bottom - band_thickness as u32 && y < bottom;
      let in_band = band_mid || band_top || band_bot;

      let plank = plank_spacing > 0 && (x - left).is_multiple_of(plank_spacing) && !in_band;

      let base = if in_band {
        band
      } else if plank {
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

/// Generate an RGBA torch sprite at the given size.
///
/// Draws a small wooden handle at the bottom with a flame shape on top.
/// Most of the texture is transparent so the sprite doesn't look blocky.
pub fn torch(handle: [u8; 3], flame: [u8; 3], size: u32) -> Vec<u8> {
  let mut data = vec![0u8; (size * size * 4) as usize];
  let s = size as f32;
  let cx = s / 2.0;

  // All positions expressed as fractions of size
  let handle_top = (s * 44.0 / 64.0) as u32;
  let handle_bot = (s * 62.0 / 64.0) as u32;
  let handle_half_w = s * 3.0 / 64.0;
  let flame_top = (s * 8.0 / 64.0) as u32;
  let flame_bot = (s * 46.0 / 64.0) as u32;
  let flame_span = (flame_bot - flame_top) as f32;
  let flame_max_w = s * 10.0 / 64.0;

  for y in 0..size {
    for x in 0..size {
      let idx = ((y * size + x) * 4) as usize;
      let fx = x as f32 + 0.5;
      let fy = y as f32 + 0.5;

      if y >= handle_top && y < handle_bot && (fx - cx).abs() < handle_half_w {
        let shade = 0.7 + 0.3 * (1.0 - (fx - cx).abs() / handle_half_w);
        data[idx] = (handle[0] as f32 * shade) as u8;
        data[idx + 1] = (handle[1] as f32 * shade) as u8;
        data[idx + 2] = (handle[2] as f32 * shade) as u8;
        data[idx + 3] = 255;
        continue;
      }

      if y >= flame_top && y < flame_bot {
        let t = (fy - flame_top as f32) / flame_span;
        let max_half_w = flame_max_w * (t * 0.6 + 0.4).sqrt();
        let dx = (fx - cx).abs();
        if dx < max_half_w {
          let edge = dx / max_half_w;
          let core = 1.0 - edge * edge;
          let r = (flame[0] as f32 + (255.0 - flame[0] as f32) * core).clamp(0.0, 255.0);
          let g = (flame[1] as f32 * (0.5 + 0.5 * core)).clamp(0.0, 255.0);
          let b = (flame[2] as f32 * core * 0.5).clamp(0.0, 255.0);
          let alpha = ((1.0 - edge) * 255.0).clamp(0.0, 255.0);
          data[idx] = r as u8;
          data[idx + 1] = g as u8;
          data[idx + 2] = b as u8;
          data[idx + 3] = alpha as u8;
          continue;
        }
      }

      data[idx + 3] = 0;
    }
  }
  data
}

/// Generate an RGBA noisy stone texture at the given size.
pub fn stone(base: [u8; 3], size: u32) -> Vec<u8> {
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
    let tex = checkerboard([255, 255, 255, 255], [0, 0, 0, 255], 8, 64);
    assert_eq!(tex.len(), 64 * 64 * 4);
  }

  #[test]
  fn checkerboard_pattern() {
    let tex = checkerboard([255, 0, 0, 255], [0, 255, 0, 255], 32, 64);
    assert_eq!(&tex[0..4], &[255, 0, 0, 255]);
    let idx = (32 * 4) as usize;
    assert_eq!(&tex[idx..idx + 4], &[0, 255, 0, 255]);
  }

  #[test]
  fn bricks_size() {
    let tex = bricks([128, 128, 128, 255], [180, 60, 30, 255], 64);
    assert_eq!(tex.len(), 64 * 64 * 4);
  }

  #[test]
  fn solid_uniform() {
    let color = [42, 128, 200, 255];
    let tex = solid(color, 64);
    assert_eq!(tex.len(), 64 * 64 * 4);
    for pixel in tex.as_chunks::<4>().0 {
      assert_eq!(pixel, &color);
    }
  }

  #[test]
  fn stone_size_and_alpha() {
    let tex = stone([128, 128, 128], 64);
    assert_eq!(tex.len(), 64 * 64 * 4);
    for pixel in tex.as_chunks::<4>().0 {
      assert_eq!(pixel[3], 255);
    }
  }

  #[test]
  fn barrel_size_and_transparency() {
    let tex = barrel([139, 90, 43], [100, 65, 30], 64);
    assert_eq!(tex.len(), 64 * 64 * 4);
    assert_eq!(tex[3], 0);
    let center_idx = (32 * 64 + 32) * 4;
    assert_eq!(tex[center_idx + 3], 255);
  }

  #[test]
  fn torch_size_and_shape() {
    let tex = torch([100, 70, 40], [255, 180, 50], 64);
    assert_eq!(tex.len(), 64 * 64 * 4);
    assert_eq!(tex[3], 0);
    let handle_idx = (50 * 64 + 32) * 4;
    assert_eq!(tex[handle_idx + 3], 255);
  }
}
