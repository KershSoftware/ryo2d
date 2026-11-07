//! Slot names and the file names the storage keeps for them.

use std::sync::atomic::{AtomicU64, Ordering};

use super::SaveError;

const MAX_SLOT_LEN: usize = 64;

/// Names Windows keeps for devices, whatever follows them
const DEVICE_NAMES: [&str; 22] = [
  "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
  "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Makes each temporary name of this process unique
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Checks a name for a new save: 1 to 64 characters of `a`-`z`, `0`-`9`, `-`
/// and `_`, and not a Windows device name, so the same name is the same slot on
/// every disk and as a browser key.
pub(crate) fn check_new(slot: &str) -> Result<(), SaveError> {
  let allowed = (1..=MAX_SLOT_LEN).contains(&slot.len())
    && slot
      .bytes()
      .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_'))
    && !DEVICE_NAMES.contains(&slot);
  if allowed {
    Ok(())
  } else {
    Err(SaveError::InvalidSlot(slot.to_string()))
  }
}

/// True for a name the old save manager accepted that the new rule refuses:
/// such a save can still be loaded, listed and deleted, but not written.
pub(crate) fn is_legacy(slot: &str) -> bool {
  let old_rule = !slot.is_empty()
    && !slot.contains(['/', '\\'])
    && !slot.contains("..")
    && !slot.starts_with('.');
  old_rule && check_new(slot).is_err()
}

/// The file a slot is kept in
pub(crate) fn file_name(slot: &str) -> String {
  format!("{slot}.ryo-save")
}

/// The previous verified copy of a file
pub(crate) fn backup_name(name: &str) -> String {
  format!("{name}.bak")
}

/// A new temporary name for writing a file, unique across processes (by the
/// process id) and within one (by a counter)
pub(crate) fn temp_name(name: &str) -> String {
  let n = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
  format!("{name}.{}-{n}.tmp", process_id())
}

/// True when `candidate` is a temporary name [`temp_name`] makes for `name`
pub(crate) fn is_temp_of(candidate: &str, name: &str) -> bool {
  let Some(middle) = candidate
    .strip_prefix(name)
    .and_then(|rest| rest.strip_prefix('.'))
    .and_then(|rest| rest.strip_suffix(".tmp"))
  else {
    return false;
  };
  let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
  middle
    .split_once('-')
    .is_some_and(|(pid, n)| digits(pid) && digits(n))
}

#[cfg(not(target_arch = "wasm32"))]
fn process_id() -> u32 {
  std::process::id()
}

// The browser has no process ids; one page is one writer
#[cfg(target_arch = "wasm32")]
fn process_id() -> u32 {
  0
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn new_slot_names_follow_the_rule() {
    let longest = "a".repeat(64);
    for good in ["slot1", "auto-save_2", longest.as_str()] {
      assert!(check_new(good).is_ok(), "{good:?} should be accepted");
    }
    let too_long = "a".repeat(65);
    for bad in [
      "",
      "Slot1",
      "a b",
      "a.b",
      "../x",
      too_long.as_str(),
      "con",
      "lpt9",
    ] {
      match check_new(bad) {
        Err(SaveError::InvalidSlot(name)) => assert_eq!(name, bad),
        other => panic!("{bad:?} should be refused, got {other:?}"),
      }
    }
  }

  #[test]
  fn legacy_names_are_recognised() {
    assert!(is_legacy("My Save"));
    assert!(is_legacy("Hero.v2"));
    assert!(!is_legacy("../x"));
    assert!(!is_legacy("slot1"));
  }

  #[test]
  fn temp_names_are_unique_and_belong_to_their_file() {
    let file = file_name("slot1");
    assert_eq!(file, "slot1.ryo-save");
    assert_eq!(backup_name(&file), "slot1.ryo-save.bak");

    let first = temp_name(&file);
    let second = temp_name(&file);
    assert_ne!(first, second);
    assert!(first.starts_with("slot1.ryo-save.") && first.ends_with(".tmp"));
    assert!(is_temp_of(&first, &file));
    assert!(is_temp_of("slot1.ryo-save.12-0.tmp", &file));

    for other in [
      "slot1.ryo-save",
      "slot1.ryo-save.bak",
      "slot1.ryo-save.tmp",
      "slot1.ryo-save.x-0.tmp",
      "slot1.ryo-save.12-.tmp",
      "slot1.ryo-save.ryo-save.12-0.tmp",
      "slot10.ryo-save.12-0.tmp",
    ] {
      assert!(
        !is_temp_of(other, &file),
        "{other:?} is not a temp of {file:?}"
      );
    }
  }
}
