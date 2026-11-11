//! The panic hook: a panic is written to the log (all but its stderr output,
//! where the hook that was there prints it), and the file flushed, before the
//! hook that was there runs.

use std::panic::{self, PanicHookInfo};
use std::sync::Once;

use super::file;

static INSTALL: Once = Once::new();

/// Installs the hook, once per process; it chains to the hook that was there.
pub(crate) fn install_hook() {
  INSTALL.call_once(|| {
    let earlier = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
      // A panic that came with this thread holding the file's lock writes
      // nothing to the file, and does not wait for the lock. Not to stderr:
      // the earlier hook prints the panic there
      super::log_panic(describe(info));
      file::flush();
      earlier(info);
    }));
  });
}

/// `panic at <file>:<line>: <message>`
fn describe(info: &PanicHookInfo<'_>) -> String {
  let payload = info.payload();
  let message = payload
    .downcast_ref::<&str>()
    .copied()
    .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
    .unwrap_or("(a panic with no message)");
  match info.location() {
    Some(location) => format!(
      "panic at {}:{}: {message}",
      location.file(),
      location.line()
    ),
    None => format!("panic at an unknown place: {message}"),
  }
}
