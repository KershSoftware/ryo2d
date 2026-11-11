//! The system clipboard. The web build has none: reads and writes fail.

// The fake's text while a `FakeClipboard` is alive on the thread (`None`
// inside is a clipboard whose read fails)
#[cfg(test)]
thread_local! {
  static FAKE: std::cell::RefCell<Option<Option<String>>> = const { std::cell::RefCell::new(None) };
}

/// A fake clipboard for tests: while one is alive on a thread, that thread's
/// reads and writes use it and the system's is never touched. Removed on drop.
#[cfg(test)]
pub(crate) struct FakeClipboard;

#[cfg(test)]
impl FakeClipboard {
  pub(crate) fn holding(text: Option<&str>) -> Self {
    FAKE.with(|f| *f.borrow_mut() = Some(text.map(str::to_string)));
    FakeClipboard
  }

  /// Change what the fake holds
  pub(crate) fn set(&self, text: Option<&str>) {
    FAKE.with(|f| *f.borrow_mut() = Some(text.map(str::to_string)));
  }

  /// What the fake holds now
  pub(crate) fn text(&self) -> Option<String> {
    FAKE.with(|f| f.borrow().clone().flatten())
  }
}

#[cfg(test)]
impl Drop for FakeClipboard {
  fn drop(&mut self) {
    FAKE.with(|f| *f.borrow_mut() = None);
  }
}

/// Put `text` on the clipboard.
pub(crate) fn set_text(text: &str) -> Result<(), String> {
  #[cfg(test)]
  if FAKE.with(|f| {
    f.borrow_mut()
      .as_mut()
      .map(|held| *held = Some(text.to_string()))
      .is_some()
  }) {
    return Ok(());
  }
  #[cfg(not(target_arch = "wasm32"))]
  {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    clipboard.set_text(text).map_err(|e| e.to_string())
  }
  #[cfg(target_arch = "wasm32")]
  {
    let _ = text;
    Err("there is no clipboard on the web".to_string())
  }
}

/// The clipboard's text.
pub(crate) fn get_text() -> Result<String, String> {
  #[cfg(test)]
  if let Some(held) = FAKE.with(|f| f.borrow().clone()) {
    return held.ok_or_else(|| "the fake clipboard is empty".to_string());
  }
  #[cfg(not(target_arch = "wasm32"))]
  {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    clipboard.get_text().map_err(|e| e.to_string())
  }
  #[cfg(target_arch = "wasm32")]
  {
    Err("there is no clipboard on the web".to_string())
  }
}
