//! Build script for embedding resources (e.g., Windows icon).
//!
//! When building via `ryo2d build`, the RYO_EMBED_ICON environment variable
//! is set to the path of the application icon. This script uses winresource
//! to embed it into the Windows executable.

fn main() {
    // Only embed icon on Windows
    #[cfg(target_os = "windows")]
    {
        // Check if building via ryo2d CLI (which sets RYO_EMBED_ICON)
        if let Ok(icon_path) = std::env::var("RYO_EMBED_ICON") {
            println!("cargo:rerun-if-env-changed=RYO_EMBED_ICON");

            // Check if the icon file exists and is a PNG
            let path = std::path::Path::new(&icon_path);
            if path.exists() {
                // winresource can handle PNG files directly
                let mut res = winresource::WindowsResource::new();
                res.set_icon(&icon_path);

                if let Err(e) = res.compile() {
                    eprintln!("Warning: Failed to embed icon: {}", e);
                }
            }
        }
    }
}
