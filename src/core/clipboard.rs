//! The system clipboard. The web build has none: reads and writes fail.

/// Put `text` on the clipboard.
pub(crate) fn set_text(text: &str) -> Result<(), String> {
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
