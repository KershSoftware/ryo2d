//! Reusing per-frame buffers whose items borrow from the frame.

/// `vec`, emptied, as a vector of another item type, keeping its
/// allocation when the two types have the same size and alignment (items
/// that differ only in the lifetimes they borrow for).
///
/// A buffer of borrowed items can't outlive the frame it borrows from, but
/// its allocation can: keep it as `Vec<Item<'static>>` and recycle it into
/// the frame's lifetime and back.
pub(crate) fn recycle<T, U>(mut vec: Vec<T>) -> Vec<U> {
  vec.clear();
  // Collecting an empty iterator in place reuses the source allocation
  vec.into_iter().filter_map(|_| None).collect()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_recycled_vector_keeps_its_allocation() {
    let text = String::from("borrowed");
    let mut borrowed: Vec<&str> = Vec::with_capacity(64);
    borrowed.push(&text);
    let pointer = borrowed.as_ptr() as usize;
    let kept: Vec<&'static str> = recycle(borrowed);
    assert!(kept.is_empty());
    assert!(kept.capacity() >= 64);
    assert_eq!(kept.as_ptr() as usize, pointer);
  }
}
