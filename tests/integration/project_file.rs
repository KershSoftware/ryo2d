//! `ryo2d::project!` reads the calling crate's Ryo.toml at compile time.

use ryo2d::core::config::ProjectFile;

#[test]
fn the_project_macro_reads_a_ryo_toml_beside_the_manifest() {
  let file: ProjectFile = ryo2d::project!("tests/fixtures/project/Ryo.toml");
  assert!(!file.is_none());
  let on_disk = std::fs::read_to_string(
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/project/Ryo.toml"),
  )
  .unwrap();
  assert_eq!(file.text(), Some(on_disk.as_str()));
}

#[test]
fn the_project_macro_is_none_without_a_ryo_toml() {
  let file: ProjectFile = ryo2d::project!();
  assert!(file.is_none());
}
