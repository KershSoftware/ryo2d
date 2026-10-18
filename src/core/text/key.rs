//! A text's key: its text, style shape, colours and layout options, each
//! hashed once with the fast hasher. The UI makes one per text leaf per frame
//! and passes it to every engine call for that leaf, so the text is never
//! hashed again: the effect seed and the layout cache key combine the parts.

use std::hash::Hasher;

use crate::core::hash::FastHasher;
use crate::core::text::rich::RichText;
use crate::core::text::style::{TextLayoutOptions, TextStyle};

#[cfg(test)]
thread_local! {
  /// Keys made on this thread (the tests count them)
  pub(crate) static KEYS_MADE: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// A text's parts, each hashed once
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct TextKey {
  text: u64,
  /// The style but its colours
  shape: u64,
  colours: u64,
  options: u64,
}

impl TextKey {
  /// Hash `text`, `style` (its shape and its colours apart) and `options`
  pub(crate) fn of(text: &RichText, style: &TextStyle, options: &TextLayoutOptions) -> TextKey {
    #[cfg(test)]
    KEYS_MADE.with(|made| made.set(made.get() + 1));
    let mut h = FastHasher::default();
    text.hash_into(&mut h);
    let text = h.finish();
    let mut h = FastHasher::default();
    style.hash_shape_into(&mut h);
    let shape = h.finish();
    let mut h = FastHasher::default();
    style.hash_colours_into(&mut h);
    TextKey {
      text,
      shape,
      colours: h.finish(),
      options: options_hash(options),
    }
  }

  /// The effect seed: the text, the shape and the options, so it stays the
  /// same whatever else is drawn around it and when a hover changes its
  /// colour
  pub(crate) fn seed(&self) -> u64 {
    combine(&[self.text, self.shape, self.options])
  }

  /// The same text and style laid out with `options` (a leaf is laid out at
  /// the width it gets)
  pub(crate) fn with_options(&self, options: &TextLayoutOptions) -> TextKey {
    TextKey {
      options: options_hash(options),
      ..*self
    }
  }

  /// The layout cache's key: every part, the scale and the font generation
  pub(crate) fn cache_key(&self, scale: f32, generation: u64) -> u64 {
    combine(&[
      self.text,
      self.shape,
      self.colours,
      self.options,
      u64::from(scale.to_bits()),
      generation,
    ])
  }
}

fn options_hash(options: &TextLayoutOptions) -> u64 {
  let mut h = FastHasher::default();
  options.hash_into(&mut h);
  h.finish()
}

/// The parts folded into one hash, a word at a time
fn combine(parts: &[u64]) -> u64 {
  let mut h = FastHasher::default();
  for part in parts {
    h.write_u64(*part);
  }
  h.finish()
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::text::{RichText, TextLayoutOptions, TextStyle};
  use crate::core::ui::Color;

  #[test]
  fn colours_change_the_cache_key_but_not_the_seed() {
    let text = RichText::from("Score: 10");
    let white = TextStyle::default().size(16.0);
    let red = TextStyle::default()
      .size(16.0)
      .color(Color::new(255, 0, 0, 255));
    let options = TextLayoutOptions::default();
    let a = TextKey::of(&text, &white, &options);
    let b = TextKey::of(&text, &red, &options);
    assert_eq!(a.seed(), b.seed(), "the seed ignores colours");
    assert_ne!(
      a.cache_key(1.0, 0),
      b.cache_key(1.0, 0),
      "the cache key does not"
    );
  }

  #[test]
  fn scale_generation_and_options_change_the_cache_key() {
    let text = RichText::from("Hi");
    let style = TextStyle::default();
    let key = TextKey::of(&text, &style, &TextLayoutOptions::default());
    assert_ne!(key.cache_key(1.0, 0), key.cache_key(2.0, 0));
    assert_ne!(key.cache_key(1.0, 0), key.cache_key(1.0, 1));
    let wrapped = TextLayoutOptions {
      wrap: Some(100.0),
      ..TextLayoutOptions::default()
    };
    assert_eq!(
      key.with_options(&wrapped),
      TextKey::of(&text, &style, &wrapped)
    );
  }

  #[test]
  fn shape_and_colour_hashes_together_match_the_full_hash() {
    use crate::core::hash::FastHasher;
    use std::hash::Hasher;
    let style = TextStyle::default()
      .size(20.0)
      .color(Color::new(1, 2, 3, 255));
    let mut full = FastHasher::default();
    style.hash_into(&mut full);
    let mut parts = FastHasher::default();
    style.hash_shape_into(&mut parts);
    style.hash_colours_into(&mut parts);
    assert_eq!(full.finish(), parts.finish());
  }
}
