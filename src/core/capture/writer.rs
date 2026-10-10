//! PNG output for captured frames.

use std::path::{Path, PathBuf};
use std::thread::JoinHandle;

use image::RgbaImage;

use crate::core::error::{CaptureError, CaptureResult};

/// Default file name for a capture of presented frame `frame`.
pub fn default_file_name(frame: u64) -> String {
  format!("frame-{frame:06}.png")
}

/// Encode `image` as PNG at `path`, creating missing parent directories.
///
/// Existing files are overwritten.
pub fn write_png(image: &RgbaImage, path: &Path) -> CaptureResult<()> {
  if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
    std::fs::create_dir_all(parent).map_err(|source| CaptureError::CreateDir {
      path: parent.to_path_buf(),
      source,
    })?;
  }
  image
    .save_with_format(path, image::ImageFormat::Png)
    .map_err(|source| CaptureError::Write {
      path: path.to_path_buf(),
      source,
    })
}

/// Write the PNG on a background thread so encoding never stalls a frame.
pub fn spawn_write(image: RgbaImage, path: PathBuf) -> JoinHandle<CaptureResult<PathBuf>> {
  std::thread::spawn(move || write_png(&image, &path).map(|()| path))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn default_name_is_zero_padded() {
    assert_eq!(default_file_name(60), "frame-000060.png");
    assert_eq!(default_file_name(1_234_567), "frame-1234567.png");
  }

  #[test]
  fn write_png_round_trips_and_creates_dirs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested/out.png");
    let mut image = RgbaImage::new(3, 2);
    image.put_pixel(2, 1, image::Rgba([10, 20, 30, 40]));

    write_png(&image, &path).unwrap();

    let loaded = image::open(&path).unwrap().to_rgba8();
    assert_eq!(loaded, image);
  }

  #[test]
  fn write_png_reports_unwritable_directory() {
    let dir = tempfile::tempdir().unwrap();
    let blocker = dir.path().join("not-a-dir");
    std::fs::write(&blocker, b"x").unwrap();

    let err = write_png(&RgbaImage::new(1, 1), &blocker.join("out.png")).unwrap_err();

    assert!(matches!(err, CaptureError::CreateDir { .. }));
  }

  #[test]
  fn spawn_write_returns_written_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bg.png");

    let written = spawn_write(RgbaImage::new(2, 2), path.clone())
      .join()
      .unwrap()
      .unwrap();

    assert_eq!(written, path);
    assert!(path.exists());
  }
}
