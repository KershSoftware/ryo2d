//! Shared WGSL: include files composed in front of each shader body.
//!
//! Built-in shaders are composed at compile time (`concat!`), so each module
//! constant is a complete WGSL module; game shaders are composed at runtime
//! with [`compose`].

/// The camera uniform every world shader draws through
#[cfg(test)]
pub(crate) const CAMERA: &str = include_str!("include/camera.wgsl");
/// The fullscreen vertex stage (game post-process effects compose it at runtime)
pub(crate) const FULLSCREEN: &str = include_str!("include/fullscreen.wgsl");

/// The sprite shader
pub(crate) const SPRITE: &str = concat!(
  include_str!("include/camera.wgsl"),
  "\n",
  include_str!("include/color.wgsl"),
  "\n",
  include_str!("include/sample.wgsl"),
  "\n",
  include_str!("core/sprite.wgsl"),
  "\n",
  include_str!("core/text.wgsl"),
);

/// Light accumulation
pub(crate) const LIGHT: &str = concat!(
  include_str!("include/camera.wgsl"),
  "\n",
  include_str!("core/light.wgsl"),
);

/// Shadow geometry
pub(crate) const SHADOW: &str = concat!(
  include_str!("include/camera.wgsl"),
  "\n",
  include_str!("core/shadow.wgsl"),
);

/// Trails and lines (plain and textured)
pub(crate) const LINE: &str = concat!(
  include_str!("include/camera.wgsl"),
  "\n",
  include_str!("include/color.wgsl"),
  "\n",
  include_str!("include/sample.wgsl"),
  "\n",
  include_str!("core/line.wgsl"),
);

/// The lit scene composed from the light buffer
pub(crate) const LIGHTING_COMPOSE: &str = concat!(
  include_str!("include/fullscreen.wgsl"),
  "\n",
  include_str!("post-process/lighting.wgsl"),
);

/// The scene transition fade
pub(crate) const TRANSITION: &str = concat!(
  include_str!("include/color.wgsl"),
  "\n",
  include_str!("core/transition.wgsl"),
);

/// The final composite to the window
pub(crate) const COMPOSITE: &str = concat!(include_str!("core/fullscreen_quad.wgsl"),);

/// A camera's own image placed in its viewport
pub(crate) const CAMERA_IMAGE: &str = concat!(include_str!("core/camera_image.wgsl"),);

/// The tonemap pass: the scene image into the display image
pub(crate) const TONEMAP: &str = concat!(
  include_str!("include/fullscreen.wgsl"),
  "\n",
  include_str!("include/color.wgsl"),
  "\n",
  include_str!("core/tonemap.wgsl"),
);

/// Auto-exposure: the scene image's luminance histogram and stored exposure
pub(crate) const EXPOSURE: &str = include_str!("core/exposure.wgsl");

/// The `fade` post-process effect
pub(crate) const FADE: &str = concat!(
  include_str!("include/fullscreen.wgsl"),
  "\n",
  include_str!("post-process/fade.wgsl"),
);

/// The `color_tint` post-process effect
pub(crate) const COLOR_TINT: &str = concat!(
  include_str!("include/fullscreen.wgsl"),
  "\n",
  include_str!("post-process/color_tint.wgsl"),
);

/// The `vignette` post-process effect
pub(crate) const VIGNETTE: &str = concat!(
  include_str!("include/fullscreen.wgsl"),
  "\n",
  include_str!("post-process/vignette.wgsl"),
);

/// The `screen_flash` post-process effect
pub(crate) const SCREEN_FLASH: &str = concat!(
  include_str!("include/fullscreen.wgsl"),
  "\n",
  include_str!("post-process/screen_flash.wgsl"),
);

/// The `crt` post-process effect
pub(crate) const CRT: &str = concat!(
  include_str!("include/fullscreen.wgsl"),
  "\n",
  include_str!("post-process/crt.wgsl"),
);

/// The `chromatic_aberration` post-process effect
pub(crate) const CHROMATIC_ABERRATION: &str = concat!(
  include_str!("include/fullscreen.wgsl"),
  "\n",
  include_str!("post-process/chromatic_aberration.wgsl"),
);

/// The `god_rays` post-process effect
pub(crate) const GOD_RAYS: &str = concat!(
  include_str!("include/fullscreen.wgsl"),
  "\n",
  include_str!("post-process/god_rays.wgsl"),
);

/// The `ambient_occlusion` post-process effect
pub(crate) const AMBIENT_OCCLUSION: &str = concat!(
  include_str!("include/fullscreen.wgsl"),
  "\n",
  include_str!("post-process/ambient_occlusion.wgsl"),
);

/// The `bloom_extract` post-process effect
pub(crate) const BLOOM_EXTRACT: &str = concat!(
  include_str!("include/fullscreen.wgsl"),
  "\n",
  include_str!("post-process/bloom_extract.wgsl"),
);

/// The `bloom_blur` post-process effect
pub(crate) const BLOOM_BLUR: &str = concat!(
  include_str!("include/fullscreen.wgsl"),
  "\n",
  include_str!("post-process/bloom_blur.wgsl"),
);

/// The `bloom_composite` post-process effect
pub(crate) const BLOOM_COMPOSITE: &str = concat!(
  include_str!("include/fullscreen.wgsl"),
  "\n",
  include_str!("post-process/bloom_composite.wgsl"),
);

/// `parts` as one module source.
pub(crate) fn compose(parts: &[&str]) -> String {
  parts.join("\n")
}

/// Every built-in module, composed, by name (checked by the tests).
#[cfg(test)]
pub(crate) fn built_in() -> Vec<(&'static str, String)> {
  [
    ("sprite", SPRITE),
    ("light", LIGHT),
    ("shadow", SHADOW),
    ("line", LINE),
    ("lighting_compose", LIGHTING_COMPOSE),
    ("transition", TRANSITION),
    ("composite", COMPOSITE),
    ("camera_image", CAMERA_IMAGE),
    ("tonemap", TONEMAP),
    ("exposure", EXPOSURE),
    ("fade", FADE),
    ("color_tint", COLOR_TINT),
    ("vignette", VIGNETTE),
    ("screen_flash", SCREEN_FLASH),
    ("crt", CRT),
    ("chromatic_aberration", CHROMATIC_ABERRATION),
    ("god_rays", GOD_RAYS),
    ("ambient_occlusion", AMBIENT_OCCLUSION),
    ("bloom_extract", BLOOM_EXTRACT),
    ("bloom_blur", BLOOM_BLUR),
    ("bloom_composite", BLOOM_COMPOSITE),
  ]
  .into_iter()
  .map(|(name, source)| (name, source.to_string()))
  .collect()
}

#[cfg(test)]
mod tests {
  use super::*;

  fn validate(name: &str, source: &str) {
    let module = naga::front::wgsl::parse_str(source)
      .unwrap_or_else(|e| panic!("{name} does not parse: {}", e.emit_to_string(source)));
    naga::valid::Validator::new(
      naga::valid::ValidationFlags::all(),
      naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .unwrap_or_else(|e| panic!("{name} does not validate: {e:?}"));
  }

  #[test]
  fn every_built_in_shader_parses_and_validates() {
    let shaders = built_in();
    assert!(shaders.len() >= 18, "{} built-in shaders", shaders.len());
    for (name, source) in &shaders {
      validate(name, source);
    }
  }

  #[test]
  fn the_camera_struct_is_declared_once() {
    for (name, source) in built_in() {
      assert!(source.matches("view_proj: mat4x4").count() <= 1, "{name}");
    }
    assert!(CAMERA.contains("view_proj: mat4x4"));
  }
}
