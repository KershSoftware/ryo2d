//! The UI environment: what every builder reads besides a node's own
//! components

/// Everything a UI builder reads that is not a node's own components: the
/// theme, the fonts, the scale and size of the UI and what prompts show. A
/// cache compares two of these to know whether all it recorded is stale.
/// Scale and size are `f32` bit patterns, so equal values compare equal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(dead_code)] // until the node cache reads it
pub(crate) struct UiEnv {
  /// `UiTheme::generation`
  pub(crate) theme: u64,
  /// `TextEngine::generation`
  pub(crate) fonts: u64,
  /// `UiSpace::px_scale`
  pub(crate) scale: u32,
  /// `UiSpace::size`
  pub(crate) size: (u32, u32),
  /// `PromptContext::generation`
  pub(crate) prompts: u64,
}

#[cfg(test)]
mod tests {
  use crate::core::testing::UiHarness;
  use crate::prelude::*;

  fn env(h: &mut UiHarness) -> crate::core::ui::env::UiEnv {
    h.frame(|ui| ui.env())
  }

  #[test]
  fn the_env_is_stable_while_nothing_changes() {
    let mut h = UiHarness::new(640.0, 480.0);
    let first = env(&mut h);
    assert_eq!(env(&mut h), first);
  }

  #[test]
  fn a_theme_change_moves_the_env() {
    let mut h = UiHarness::new(640.0, 480.0);
    let before = env(&mut h);
    h.set_theme(Theme::light());
    assert_ne!(env(&mut h), before);
  }

  #[test]
  fn a_scale_or_size_change_moves_the_env() {
    let mut h = UiHarness::new(640.0, 480.0);
    let before = env(&mut h);
    h.resize(800.0, 600.0);
    assert_ne!(env(&mut h), before);
  }

  #[test]
  fn loading_skins_bumps_the_theme_generation() {
    let mut theme = crate::core::ui::theme::UiTheme::default();
    let before = theme.generation();
    theme.load_skins(|_| None);
    assert_ne!(theme.generation(), before, "skins can change what is drawn");
  }
}
