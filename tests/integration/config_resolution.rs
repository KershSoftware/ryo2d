//! Integration tests for the "WxH" resolution format. The settings file's own
//! format is tested in `settings::file` and `settings::resource`.

use ryo2d::prelude::*;

// ==========================================================================
// Resolution parsing integration
// ==========================================================================

#[test]
fn parse_resolution_standard_formats() {
  assert_eq!(parse_resolution("1280x720"), Some((1280, 720)));
  assert_eq!(parse_resolution("800x600"), Some((800, 600)));
  assert_eq!(parse_resolution("1920x1080"), Some((1920, 1080)));
  assert_eq!(parse_resolution("3840x2160"), Some((3840, 2160)));
}

#[test]
fn parse_resolution_rejects_invalid() {
  assert_eq!(parse_resolution(""), None);
  assert_eq!(parse_resolution("1280"), None);
  assert_eq!(parse_resolution("axb"), None);
  assert_eq!(parse_resolution("1280x720x60"), None);
}
