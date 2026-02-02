//! Build script that captures Ryo.toml configuration from environment variables.
//!
//! When building via `ryo2d build`, the CLI passes RYO_EMBED_* environment variables
//! which are then embedded into the binary. This allows games to use Ryo.toml as
//! the source of truth without needing the file in the release bundle.

fn main() {
  // Re-run if any of the embedded config values change
  println!("cargo:rerun-if-env-changed=RYO_EMBED_RESOLUTION_WIDTH");
  println!("cargo:rerun-if-env-changed=RYO_EMBED_RESOLUTION_HEIGHT");
  println!("cargo:rerun-if-env-changed=RYO_EMBED_VSYNC");
  println!("cargo:rerun-if-env-changed=RYO_EMBED_FULLSCREEN");
  println!("cargo:rerun-if-env-changed=RYO_EMBED_MASTER_VOLUME");
  println!("cargo:rerun-if-env-changed=RYO_EMBED_MUSIC_VOLUME");
  println!("cargo:rerun-if-env-changed=RYO_EMBED_SFX_VOLUME");
  println!("cargo:rerun-if-env-changed=RYO_EMBED_SHOW_FPS");
  println!("cargo:rerun-if-env-changed=RYO_EMBED_TITLE");

  // Pass through environment variables as rustc-env so they're available via env!()
  // Only set them if they exist (when building via ryo2d CLI)
  if let Ok(val) = std::env::var("RYO_EMBED_RESOLUTION_WIDTH") {
    println!("cargo:rustc-env=RYO_EMBED_RESOLUTION_WIDTH={}", val);
  }
  if let Ok(val) = std::env::var("RYO_EMBED_RESOLUTION_HEIGHT") {
    println!("cargo:rustc-env=RYO_EMBED_RESOLUTION_HEIGHT={}", val);
  }
  if let Ok(val) = std::env::var("RYO_EMBED_VSYNC") {
    println!("cargo:rustc-env=RYO_EMBED_VSYNC={}", val);
  }
  if let Ok(val) = std::env::var("RYO_EMBED_FULLSCREEN") {
    println!("cargo:rustc-env=RYO_EMBED_FULLSCREEN={}", val);
  }
  if let Ok(val) = std::env::var("RYO_EMBED_MASTER_VOLUME") {
    println!("cargo:rustc-env=RYO_EMBED_MASTER_VOLUME={}", val);
  }
  if let Ok(val) = std::env::var("RYO_EMBED_MUSIC_VOLUME") {
    println!("cargo:rustc-env=RYO_EMBED_MUSIC_VOLUME={}", val);
  }
  if let Ok(val) = std::env::var("RYO_EMBED_SFX_VOLUME") {
    println!("cargo:rustc-env=RYO_EMBED_SFX_VOLUME={}", val);
  }
  if let Ok(val) = std::env::var("RYO_EMBED_SHOW_FPS") {
    println!("cargo:rustc-env=RYO_EMBED_SHOW_FPS={}", val);
  }
  if let Ok(val) = std::env::var("RYO_EMBED_TITLE") {
    println!("cargo:rustc-env=RYO_EMBED_TITLE={}", val);
  }
}
