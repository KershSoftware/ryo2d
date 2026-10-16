//! What a widget or container reports back when it is shown

use super::{Rect, UiId};

/// What a shown widget or container reports: its id and where it is.
///
/// A top-level container is laid out when its `.show()` returns, so its
/// `rect` is this frame's. Inside another container the layout is not done
/// yet, so `rect` is where it was last frame (empty in its first frame).
#[derive(Debug, Clone, Default)]
pub struct Response {
  /// Its id, made from where it was called (or its `.id(key)`)
  pub id: UiId,
  /// Its rect in UI units
  pub rect: Rect,
}
