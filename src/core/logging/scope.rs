//! Log scopes: a number each thread holds, carried by its records.
//!
//! A game runs in scope 0. A test capture opens a fresh scope on its thread,
//! and a thread the engine spawns takes its spawner's, so a process holding
//! many engines (a test run) can tell their records apart.

use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};

thread_local! {
  static SCOPE: Cell<u64> = const { Cell::new(0) };
}

static NEXT_SCOPE: AtomicU64 = AtomicU64::new(1);

/// This thread's scope.
pub(crate) fn current_scope() -> u64 {
  SCOPE.with(Cell::get)
}

/// Sets this thread's scope, giving back the one before.
pub(super) fn replace_scope(scope: u64) -> u64 {
  SCOPE.with(|cell| cell.replace(scope))
}

/// Runs `f` with this thread's scope set to `scope`, then puts the earlier
/// one back (on a panic too).
pub(crate) fn in_scope<R>(scope: u64, f: impl FnOnce() -> R) -> R {
  struct Restore(u64);
  impl Drop for Restore {
    fn drop(&mut self) {
      replace_scope(self.0);
    }
  }
  let _restore = Restore(replace_scope(scope));
  f()
}

/// `f`, to run on another thread in this thread's scope: the scope is read
/// here, on the spawning thread. Every thread the engine spawns runs its body
/// through this.
pub(crate) fn scoped<R>(f: impl FnOnce() -> R) -> impl FnOnce() -> R {
  let scope = current_scope();
  move || in_scope(scope, f)
}

/// A scope no one has had: never 0, never repeated.
pub(crate) fn new_scope() -> u64 {
  NEXT_SCOPE.fetch_add(1, Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_new_scope_is_never_zero_and_never_repeats() {
    let a = new_scope();
    let b = new_scope();
    assert_ne!(a, 0);
    assert_ne!(b, 0);
    assert_ne!(a, b);
  }

  #[test]
  fn in_scope_sets_the_scope_for_its_call_only() {
    let scope = new_scope();
    let before = current_scope();
    assert_eq!(in_scope(scope, current_scope), scope);
    assert_eq!(current_scope(), before);
  }

  #[test]
  fn scoped_takes_the_spawning_thread_s_scope() {
    let scope = new_scope();
    let body = in_scope(scope, || scoped(current_scope));
    let seen = std::thread::spawn(body).join().expect("a thread");
    assert_eq!(seen, scope);
  }
}
