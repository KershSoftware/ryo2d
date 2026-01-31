//! Bitmap font rendering for text display
//!
//! Provides `BitmapFont` for loading font atlases and rendering text,
//! plus `create_debug_font()` for a built-in fallback font.

use std::collections::HashMap;
use std::path::Path;

use crate::core::error::{SpriteError, SpriteResult};
use crate::core::ui::types::Color;

// ============================================================================
// BitmapFont
// ============================================================================

/// A bitmap font for text rendering.
///
/// Loads a font atlas (image with character grid) and renders text by copying
/// character regions to the frame buffer.
///
/// # Font Atlas Format
///
/// The font atlas should be a grid of characters with uniform cell sizes.
/// The default layout assumes ASCII characters 32-126 (printable ASCII).
///
/// Example 16x6 grid layout (96 characters):
/// ```text
/// Row 0: ' ' ! " # $ % & ' ( ) * + , - . /
/// Row 1: 0 1 2 3 4 5 6 7 8 9 : ; < = > ?
/// Row 2: @ A B C D E F G H I J K L M N O
/// Row 3: P Q R S T U V W X Y Z [ \ ] ^ _
/// Row 4: ` a b c d e f g h i j k l m n o
/// Row 5: p q r s t u v w x y z { | } ~
/// ```
#[derive(Clone)]
pub struct BitmapFont {
    /// Raw RGBA pixel data of the font atlas
    atlas_data: Vec<u8>,
    /// Width of the atlas in pixels
    atlas_width: u32,
    /// Height of the atlas in pixels
    atlas_height: u32,
    /// Width of each character cell
    char_width: u32,
    /// Height of each character cell
    char_height: u32,
    /// Number of characters per row in the atlas
    chars_per_row: u32,
    /// First ASCII character in the atlas (default: 32 = space)
    first_char: u8,
    /// Last ASCII character in the atlas (default: 126 = tilde)
    last_char: u8,
    /// Optional character spacing override
    spacing: i32,
}

impl std::fmt::Debug for BitmapFont {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BitmapFont")
            .field("atlas_size", &(self.atlas_width, self.atlas_height))
            .field("char_size", &(self.char_width, self.char_height))
            .field("chars_per_row", &self.chars_per_row)
            .field("char_range", &(self.first_char, self.last_char))
            .finish()
    }
}

impl BitmapFont {
    /// Create a bitmap font from an image file.
    ///
    /// # Arguments
    /// * `path` - Path to the font atlas image
    /// * `char_width` - Width of each character cell
    /// * `char_height` - Height of each character cell
    ///
    /// The atlas is assumed to contain ASCII 32-126 in row-major order.
    pub fn load<P: AsRef<Path>>(path: P, char_width: u32, char_height: u32) -> SpriteResult<Self> {
        let path_ref = path.as_ref();
        let img = image::open(path_ref).map_err(|e| SpriteError::DecodeError {
            path: path_ref.to_path_buf(),
            reason: e.to_string(),
        })?;

        let rgba = img.into_rgba8();
        let atlas_width = rgba.width();
        let atlas_height = rgba.height();
        let atlas_data = rgba.into_raw();

        let chars_per_row = atlas_width / char_width;

        Ok(Self {
            atlas_data,
            atlas_width,
            atlas_height,
            char_width,
            char_height,
            chars_per_row,
            first_char: 32,
            last_char: 126,
            spacing: 0,
        })
    }

    /// Create a bitmap font from raw RGBA data.
    pub fn from_rgba(
        data: Vec<u8>,
        atlas_width: u32,
        atlas_height: u32,
        char_width: u32,
        char_height: u32,
    ) -> Self {
        let chars_per_row = atlas_width / char_width;
        Self {
            atlas_data: data,
            atlas_width,
            atlas_height,
            char_width,
            char_height,
            chars_per_row,
            first_char: 32,
            last_char: 126,
            spacing: 0,
        }
    }

    /// Set custom character spacing (can be negative for tighter text).
    pub fn with_spacing(mut self, spacing: i32) -> Self {
        self.spacing = spacing;
        self
    }

    /// Set the character range in the atlas.
    pub fn with_char_range(mut self, first: u8, last: u8) -> Self {
        self.first_char = first;
        self.last_char = last;
        self
    }

    /// Get the width of each character.
    pub fn char_width(&self) -> u32 {
        self.char_width
    }

    /// Get the height of each character (line height).
    pub fn char_height(&self) -> u32 {
        self.char_height
    }

    /// Calculate the pixel width of a string.
    pub fn text_width(&self, text: &str) -> u32 {
        let char_count = text.chars().count() as i32;
        if char_count == 0 {
            return 0;
        }
        let total_spacing = (char_count - 1) * self.spacing;
        (char_count * self.char_width as i32 + total_spacing).max(0) as u32
    }

    /// Calculate the pixel dimensions of a multi-line string.
    pub fn text_size(&self, text: &str) -> (u32, u32) {
        let lines: Vec<&str> = text.lines().collect();
        let max_width = lines.iter().map(|l| self.text_width(l)).max().unwrap_or(0);
        let height = (lines.len() as u32) * self.char_height;
        (max_width, height)
    }

    /// Draw a single character to the frame buffer.
    ///
    /// Returns the character width (for cursor advancement).
    #[allow(clippy::too_many_arguments)]
    pub fn draw_char(
        &self,
        frame: &mut [u8],
        frame_width: u32,
        frame_height: u32,
        ch: char,
        x: i32,
        y: i32,
        color: Color,
    ) -> u32 {
        let ascii = ch as u8;

        // Skip characters outside our range
        if ascii < self.first_char || ascii > self.last_char {
            return self.char_width;
        }

        let char_index = (ascii - self.first_char) as u32;
        let atlas_col = char_index % self.chars_per_row;
        let atlas_row = char_index / self.chars_per_row;

        let src_x = atlas_col * self.char_width;
        let src_y = atlas_row * self.char_height;

        // Bounds checking
        let frame_w = frame_width as i32;
        let frame_h = frame_height as i32;
        let char_w = self.char_width as i32;
        let char_h = self.char_height as i32;

        for cy in 0..char_h {
            let dst_y = y + cy;
            if dst_y < 0 || dst_y >= frame_h {
                continue;
            }

            for cx in 0..char_w {
                let dst_x = x + cx;
                if dst_x < 0 || dst_x >= frame_w {
                    continue;
                }

                // Sample from atlas
                let atlas_px =
                    ((src_y + cy as u32) * self.atlas_width + (src_x + cx as u32)) as usize * 4;
                if atlas_px + 3 >= self.atlas_data.len() {
                    continue;
                }

                let src_a = self.atlas_data[atlas_px + 3];
                if src_a == 0 {
                    continue; // Fully transparent
                }

                // Blend with tint color
                let dst_idx = (dst_y as usize * frame_width as usize + dst_x as usize) * 4;
                if dst_idx + 3 >= frame.len() {
                    continue;
                }

                // Use atlas alpha and apply tint color
                let alpha = src_a as u16 * color.a as u16 / 255;
                if alpha == 0 {
                    continue;
                }

                let inv_alpha = 255 - alpha;
                let dst_r = frame[dst_idx] as u16;
                let dst_g = frame[dst_idx + 1] as u16;
                let dst_b = frame[dst_idx + 2] as u16;

                frame[dst_idx] = ((color.r as u16 * alpha + dst_r * inv_alpha) / 255) as u8;
                frame[dst_idx + 1] = ((color.g as u16 * alpha + dst_g * inv_alpha) / 255) as u8;
                frame[dst_idx + 2] = ((color.b as u16 * alpha + dst_b * inv_alpha) / 255) as u8;
                frame[dst_idx + 3] = 255;
            }
        }

        self.char_width
    }

    /// Draw a string to the frame buffer.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_text(
        &self,
        frame: &mut [u8],
        frame_width: u32,
        frame_height: u32,
        text: &str,
        x: i32,
        y: i32,
        color: Color,
    ) {
        let mut cursor_x = x;
        let advance = self.char_width as i32 + self.spacing;

        for ch in text.chars() {
            if ch == '\n' {
                // Newline handling (if needed for multi-line)
                continue;
            }
            self.draw_char(frame, frame_width, frame_height, ch, cursor_x, y, color);
            cursor_x += advance;
        }
    }
}

/// Create a simple built-in debug font (8x8 monospace).
///
/// This provides a fallback font for debugging when no font atlas is loaded.
/// The font is procedurally generated and contains basic ASCII characters.
pub fn create_debug_font() -> BitmapFont {
    // 8x8 pixel font, 16 chars per row, 6 rows (ASCII 32-127)
    let char_width = 8u32;
    let char_height = 8u32;
    let chars_per_row = 16u32;
    let rows = 6u32;
    let atlas_width = chars_per_row * char_width;
    let atlas_height = rows * char_height;

    // Simple bitmap font data - each character is 8 bytes (8x8 bits)
    // This is a minimal readable font for debug purposes
    #[rustfmt::skip]
    let font_data: HashMap<u8, [u8; 8]> = HashMap::from([
        // Space
        (b' ', [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]),
        // Numbers
        (b'0', [0x3C, 0x66, 0x6E, 0x76, 0x66, 0x66, 0x3C, 0x00]),
        (b'1', [0x18, 0x38, 0x18, 0x18, 0x18, 0x18, 0x7E, 0x00]),
        (b'2', [0x3C, 0x66, 0x06, 0x1C, 0x30, 0x60, 0x7E, 0x00]),
        (b'3', [0x3C, 0x66, 0x06, 0x1C, 0x06, 0x66, 0x3C, 0x00]),
        (b'4', [0x0C, 0x1C, 0x3C, 0x6C, 0x7E, 0x0C, 0x0C, 0x00]),
        (b'5', [0x7E, 0x60, 0x7C, 0x06, 0x06, 0x66, 0x3C, 0x00]),
        (b'6', [0x1C, 0x30, 0x60, 0x7C, 0x66, 0x66, 0x3C, 0x00]),
        (b'7', [0x7E, 0x06, 0x0C, 0x18, 0x30, 0x30, 0x30, 0x00]),
        (b'8', [0x3C, 0x66, 0x66, 0x3C, 0x66, 0x66, 0x3C, 0x00]),
        (b'9', [0x3C, 0x66, 0x66, 0x3E, 0x06, 0x0C, 0x38, 0x00]),
        // Uppercase letters
        (b'A', [0x18, 0x3C, 0x66, 0x66, 0x7E, 0x66, 0x66, 0x00]),
        (b'B', [0x7C, 0x66, 0x66, 0x7C, 0x66, 0x66, 0x7C, 0x00]),
        (b'C', [0x3C, 0x66, 0x60, 0x60, 0x60, 0x66, 0x3C, 0x00]),
        (b'D', [0x78, 0x6C, 0x66, 0x66, 0x66, 0x6C, 0x78, 0x00]),
        (b'E', [0x7E, 0x60, 0x60, 0x7C, 0x60, 0x60, 0x7E, 0x00]),
        (b'F', [0x7E, 0x60, 0x60, 0x7C, 0x60, 0x60, 0x60, 0x00]),
        (b'G', [0x3C, 0x66, 0x60, 0x6E, 0x66, 0x66, 0x3E, 0x00]),
        (b'H', [0x66, 0x66, 0x66, 0x7E, 0x66, 0x66, 0x66, 0x00]),
        (b'I', [0x3C, 0x18, 0x18, 0x18, 0x18, 0x18, 0x3C, 0x00]),
        (b'J', [0x1E, 0x0C, 0x0C, 0x0C, 0x6C, 0x6C, 0x38, 0x00]),
        (b'K', [0x66, 0x6C, 0x78, 0x70, 0x78, 0x6C, 0x66, 0x00]),
        (b'L', [0x60, 0x60, 0x60, 0x60, 0x60, 0x60, 0x7E, 0x00]),
        (b'M', [0x63, 0x77, 0x7F, 0x6B, 0x63, 0x63, 0x63, 0x00]),
        (b'N', [0x66, 0x76, 0x7E, 0x7E, 0x6E, 0x66, 0x66, 0x00]),
        (b'O', [0x3C, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3C, 0x00]),
        (b'P', [0x7C, 0x66, 0x66, 0x7C, 0x60, 0x60, 0x60, 0x00]),
        (b'Q', [0x3C, 0x66, 0x66, 0x66, 0x6A, 0x6C, 0x36, 0x00]),
        (b'R', [0x7C, 0x66, 0x66, 0x7C, 0x6C, 0x66, 0x66, 0x00]),
        (b'S', [0x3C, 0x66, 0x60, 0x3C, 0x06, 0x66, 0x3C, 0x00]),
        (b'T', [0x7E, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x00]),
        (b'U', [0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3C, 0x00]),
        (b'V', [0x66, 0x66, 0x66, 0x66, 0x66, 0x3C, 0x18, 0x00]),
        (b'W', [0x63, 0x63, 0x63, 0x6B, 0x7F, 0x77, 0x63, 0x00]),
        (b'X', [0x66, 0x66, 0x3C, 0x18, 0x3C, 0x66, 0x66, 0x00]),
        (b'Y', [0x66, 0x66, 0x66, 0x3C, 0x18, 0x18, 0x18, 0x00]),
        (b'Z', [0x7E, 0x06, 0x0C, 0x18, 0x30, 0x60, 0x7E, 0x00]),
        // Lowercase letters
        (b'a', [0x00, 0x00, 0x3C, 0x06, 0x3E, 0x66, 0x3E, 0x00]),
        (b'b', [0x60, 0x60, 0x7C, 0x66, 0x66, 0x66, 0x7C, 0x00]),
        (b'c', [0x00, 0x00, 0x3C, 0x66, 0x60, 0x66, 0x3C, 0x00]),
        (b'd', [0x06, 0x06, 0x3E, 0x66, 0x66, 0x66, 0x3E, 0x00]),
        (b'e', [0x00, 0x00, 0x3C, 0x66, 0x7E, 0x60, 0x3C, 0x00]),
        (b'f', [0x1C, 0x36, 0x30, 0x7C, 0x30, 0x30, 0x30, 0x00]),
        (b'g', [0x00, 0x00, 0x3E, 0x66, 0x66, 0x3E, 0x06, 0x3C]),
        (b'h', [0x60, 0x60, 0x7C, 0x66, 0x66, 0x66, 0x66, 0x00]),
        (b'i', [0x18, 0x00, 0x38, 0x18, 0x18, 0x18, 0x3C, 0x00]),
        (b'j', [0x0C, 0x00, 0x1C, 0x0C, 0x0C, 0x6C, 0x6C, 0x38]),
        (b'k', [0x60, 0x60, 0x66, 0x6C, 0x78, 0x6C, 0x66, 0x00]),
        (b'l', [0x38, 0x18, 0x18, 0x18, 0x18, 0x18, 0x3C, 0x00]),
        (b'm', [0x00, 0x00, 0x76, 0x7F, 0x6B, 0x63, 0x63, 0x00]),
        (b'n', [0x00, 0x00, 0x7C, 0x66, 0x66, 0x66, 0x66, 0x00]),
        (b'o', [0x00, 0x00, 0x3C, 0x66, 0x66, 0x66, 0x3C, 0x00]),
        (b'p', [0x00, 0x00, 0x7C, 0x66, 0x66, 0x7C, 0x60, 0x60]),
        (b'q', [0x00, 0x00, 0x3E, 0x66, 0x66, 0x3E, 0x06, 0x06]),
        (b'r', [0x00, 0x00, 0x7C, 0x66, 0x60, 0x60, 0x60, 0x00]),
        (b's', [0x00, 0x00, 0x3E, 0x60, 0x3C, 0x06, 0x7C, 0x00]),
        (b't', [0x30, 0x30, 0x7C, 0x30, 0x30, 0x36, 0x1C, 0x00]),
        (b'u', [0x00, 0x00, 0x66, 0x66, 0x66, 0x66, 0x3E, 0x00]),
        (b'v', [0x00, 0x00, 0x66, 0x66, 0x66, 0x3C, 0x18, 0x00]),
        (b'w', [0x00, 0x00, 0x63, 0x63, 0x6B, 0x7F, 0x36, 0x00]),
        (b'x', [0x00, 0x00, 0x66, 0x3C, 0x18, 0x3C, 0x66, 0x00]),
        (b'y', [0x00, 0x00, 0x66, 0x66, 0x66, 0x3E, 0x06, 0x3C]),
        (b'z', [0x00, 0x00, 0x7E, 0x0C, 0x18, 0x30, 0x7E, 0x00]),
        // Punctuation
        (b'!', [0x18, 0x18, 0x18, 0x18, 0x18, 0x00, 0x18, 0x00]),
        (b'"', [0x6C, 0x6C, 0x24, 0x00, 0x00, 0x00, 0x00, 0x00]),
        (b'#', [0x6C, 0x6C, 0xFE, 0x6C, 0xFE, 0x6C, 0x6C, 0x00]),
        (b'$', [0x18, 0x3E, 0x60, 0x3C, 0x06, 0x7C, 0x18, 0x00]),
        (b'%', [0x00, 0xC6, 0xCC, 0x18, 0x30, 0x66, 0xC6, 0x00]),
        (b'&', [0x38, 0x6C, 0x38, 0x76, 0xDC, 0xCC, 0x76, 0x00]),
        (b'\'', [0x18, 0x18, 0x30, 0x00, 0x00, 0x00, 0x00, 0x00]),
        (b'(', [0x0C, 0x18, 0x30, 0x30, 0x30, 0x18, 0x0C, 0x00]),
        (b')', [0x30, 0x18, 0x0C, 0x0C, 0x0C, 0x18, 0x30, 0x00]),
        (b'*', [0x00, 0x66, 0x3C, 0xFF, 0x3C, 0x66, 0x00, 0x00]),
        (b'+', [0x00, 0x18, 0x18, 0x7E, 0x18, 0x18, 0x00, 0x00]),
        (b',', [0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x30]),
        (b'-', [0x00, 0x00, 0x00, 0x7E, 0x00, 0x00, 0x00, 0x00]),
        (b'.', [0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x00]),
        (b'/', [0x06, 0x0C, 0x18, 0x30, 0x60, 0xC0, 0x80, 0x00]),
        (b':', [0x00, 0x18, 0x18, 0x00, 0x00, 0x18, 0x18, 0x00]),
        (b';', [0x00, 0x18, 0x18, 0x00, 0x00, 0x18, 0x18, 0x30]),
        (b'<', [0x0C, 0x18, 0x30, 0x60, 0x30, 0x18, 0x0C, 0x00]),
        (b'=', [0x00, 0x00, 0x7E, 0x00, 0x00, 0x7E, 0x00, 0x00]),
        (b'>', [0x30, 0x18, 0x0C, 0x06, 0x0C, 0x18, 0x30, 0x00]),
        (b'?', [0x3C, 0x66, 0x0C, 0x18, 0x18, 0x00, 0x18, 0x00]),
        (b'@', [0x3C, 0x66, 0x6E, 0x6A, 0x6E, 0x60, 0x3C, 0x00]),
        (b'[', [0x3C, 0x30, 0x30, 0x30, 0x30, 0x30, 0x3C, 0x00]),
        (b'\\', [0xC0, 0x60, 0x30, 0x18, 0x0C, 0x06, 0x02, 0x00]),
        (b']', [0x3C, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x3C, 0x00]),
        (b'^', [0x18, 0x3C, 0x66, 0x00, 0x00, 0x00, 0x00, 0x00]),
        (b'_', [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF]),
        (b'`', [0x30, 0x18, 0x0C, 0x00, 0x00, 0x00, 0x00, 0x00]),
        (b'{', [0x0E, 0x18, 0x18, 0x70, 0x18, 0x18, 0x0E, 0x00]),
        (b'|', [0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x00]),
        (b'}', [0x70, 0x18, 0x18, 0x0E, 0x18, 0x18, 0x70, 0x00]),
        (b'~', [0x76, 0xDC, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]),
    ]);

    // Create atlas image
    let mut atlas_data = vec![0u8; (atlas_width * atlas_height * 4) as usize];

    for ascii in 32u8..=127u8 {
        let char_index = ascii - 32;
        let col = char_index as u32 % chars_per_row;
        let row = char_index as u32 / chars_per_row;

        if let Some(bitmap) = font_data.get(&ascii) {
            for (y, &row_bits) in bitmap.iter().enumerate() {
                for x in 0..8 {
                    if (row_bits >> (7 - x)) & 1 == 1 {
                        let px = col * char_width + x;
                        let py = row * char_height + y as u32;
                        let idx = ((py * atlas_width + px) * 4) as usize;
                        atlas_data[idx] = 255; // R
                        atlas_data[idx + 1] = 255; // G
                        atlas_data[idx + 2] = 255; // B
                        atlas_data[idx + 3] = 255; // A
                    }
                }
            }
        }
    }

    BitmapFont {
        atlas_data,
        atlas_width,
        atlas_height,
        char_width,
        char_height,
        chars_per_row,
        first_char: 32,
        last_char: 127,
        spacing: 0,
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_font_creates_valid_atlas() {
        let font = create_debug_font();
        assert_eq!(font.char_width, 8);
        assert_eq!(font.char_height, 8);
        assert_eq!(font.chars_per_row, 16);
        assert!(!font.atlas_data.is_empty());
    }

    #[test]
    fn font_text_width() {
        let font = create_debug_font();
        assert_eq!(font.text_width(""), 0);
        assert_eq!(font.text_width("A"), 8);
        assert_eq!(font.text_width("Hello"), 40);
    }

    #[test]
    fn font_text_size_multiline() {
        let font = create_debug_font();
        let (w, h) = font.text_size("Hello\nWorld");
        assert_eq!(w, 40); // "Hello" and "World" are same length
        assert_eq!(h, 16); // 2 lines * 8 pixels
    }
}
