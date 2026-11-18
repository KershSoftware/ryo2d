//! Build script that lists the agent skills the CLI ships.

fn main() {
  list_skills();
}

/// Write `embedded_skills.rs` to `OUT_DIR`: every `.claude/skills/<name>/SKILL.md`
/// as `(name, include_str!(..))`, by name, for `ryo2d init` and
/// `ryo2d install-skills` (`commands::install_skills`). Editing a skill
/// rebuilds through `include_str!`; adding or removing one reruns this.
fn list_skills() {
  use std::path::Path;

  let root = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR");
  let skills = Path::new(&root).join("../../.claude/skills");
  println!("cargo:rerun-if-changed={}", skills.display());
  let mut names: Vec<String> = std::fs::read_dir(&skills)
    .expect(".claude/skills is readable")
    .filter_map(Result::ok)
    .filter(|entry| entry.path().join("SKILL.md").is_file())
    .map(|entry| entry.file_name().to_string_lossy().into_owned())
    .collect();
  names.sort();
  let entries: String = names
    .iter()
    .map(|name| {
      let path = format!("/../../.claude/skills/{name}/SKILL.md");
      format!("  ({name:?}, include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), {path:?}))),\n")
    })
    .collect();
  let out = std::env::var("OUT_DIR").expect("cargo sets OUT_DIR");
  std::fs::write(
    Path::new(&out).join("embedded_skills.rs"),
    format!("&[\n{entries}]\n"),
  )
  .expect("OUT_DIR is writable");
}
