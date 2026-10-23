//! Named icons for inline text (`[icon=name]`, `RichText::icon_named`)

use std::collections::HashMap;

use crate::core::asset::Handle;
use crate::core::ecs::Resource;
use crate::core::graphics::Image;
use crate::core::ui::Rect;

/// A named icon: an image, or a region of one (pixels on the image)
#[derive(Debug, Clone, PartialEq)]
pub struct IconSource {
  pub sprite: Handle<Image>,
  pub region: Option<Rect>,
}

/// The icons markup and `RichText::icon_named` can name. Swap a set (say
/// keyboard keys for gamepad buttons) by inserting the same names again.
#[derive(Resource, Debug, Default, Clone)]
pub struct TextIcons {
  icons: HashMap<String, IconSource>,
}

impl TextIcons {
  /// Name a whole sprite
  pub fn insert(&mut self, name: impl Into<String>, sprite: Handle<Image>) {
    self.icons.insert(
      name.into(),
      IconSource {
        sprite,
        region: None,
      },
    );
  }

  /// Name a pixel region of a sprite (an icon sheet)
  pub fn insert_region(&mut self, name: impl Into<String>, sprite: Handle<Image>, region: Rect) {
    self.icons.insert(
      name.into(),
      IconSource {
        sprite,
        region: Some(region),
      },
    );
  }

  pub fn get(&self, name: &str) -> Option<&IconSource> {
    self.icons.get(name)
  }

  pub fn remove(&mut self, name: &str) {
    self.icons.remove(name);
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn icons_are_found_by_name_and_replaced() {
    let mut images = crate::core::asset::Assets::default();
    let mut sprite = |_name: &str| images.add(Image::solid_rect(4, 4, 255, 0, 0, 255));
    let mut icons = TextIcons::default();
    icons.insert("a_button", sprite("a"));
    assert!(icons.get("a_button").is_some_and(|i| i.region.is_none()));
    icons.insert_region("a_button", sprite("sheet"), Rect::new(0.0, 0.0, 2.0, 2.0));
    assert_eq!(
      icons.get("a_button").and_then(|i| i.region),
      Some(Rect::new(0.0, 0.0, 2.0, 2.0))
    );
    icons.remove("a_button");
    assert!(icons.get("a_button").is_none());
  }
}
