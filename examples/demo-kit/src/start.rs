//! `RYO_DEMO_PAGE`: the page a demo starts on

/// Why `RYO_DEMO_PAGE` was not used
#[allow(dead_code)] // used by the runtime from Task 3
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StartPageError {
  /// Not a page number: warn and start on page 1
  Invalid(String),
  /// No such page: report and exit with code 2
  OutOfRange(String),
}

/// The 0-based page to start on for a 1-based `RYO_DEMO_PAGE` value, or
/// `Ok(None)` when it is unset.
#[allow(dead_code)] // used by the runtime from Task 3
pub(crate) fn start_page(
  raw: Option<&str>,
  title: &str,
  pages: usize,
) -> Result<Option<usize>, StartPageError> {
  let Some(raw) = raw else {
    return Ok(None);
  };
  let Ok(page) = raw.trim().parse::<usize>() else {
    return Err(StartPageError::Invalid(format!(
      "[demo] RYO_DEMO_PAGE={raw:?} is not a page number; starting on page 1"
    )));
  };
  if page == 0 || page > pages {
    return Err(StartPageError::OutOfRange(format!(
      "[demo] RYO_DEMO_PAGE={page}, but {title} has {}",
      pages_label(pages)
    )));
  }
  Ok(Some(page - 1))
}

/// `1 page`, `6 pages`
#[allow(dead_code)] // used by the runtime from Task 3
pub(crate) fn pages_label(pages: usize) -> String {
  if pages == 1 {
    "1 page".to_string()
  } else {
    format!("{pages} pages")
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn start_page_handles_every_input() {
    assert_eq!(start_page(None, "Bloom", 3), Ok(None));
    assert_eq!(start_page(Some("1"), "Bloom", 3), Ok(Some(0)));
    assert_eq!(start_page(Some(" 3 "), "Bloom", 3), Ok(Some(2)));
    assert!(matches!(
      start_page(Some("abc"), "Bloom", 3),
      Err(StartPageError::Invalid(_))
    ));
    assert!(matches!(
      start_page(Some("-1"), "Bloom", 3),
      Err(StartPageError::Invalid(_))
    ));
    assert_eq!(
      start_page(Some("4"), "Bloom", 3),
      Err(StartPageError::OutOfRange(
        "[demo] RYO_DEMO_PAGE=4, but Bloom has 3 pages".to_string()
      ))
    );
    assert_eq!(
      start_page(Some("0"), "Fog", 1),
      Err(StartPageError::OutOfRange(
        "[demo] RYO_DEMO_PAGE=0, but Fog has 1 page".to_string()
      ))
    );
  }

  #[test]
  fn pages_are_counted_in_words() {
    assert_eq!(pages_label(1), "1 page");
    assert_eq!(pages_label(6), "6 pages");
  }
}
