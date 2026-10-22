//! What the UI benches' guards share: whether criterion's arguments ask for
//! a guarded bench, the targets missed, and the run's verdict once every
//! bench has run

use std::sync::Mutex;

/// The targets missed so far: the run fails once every bench has run
static MISSED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Criterion's options that take a value (`--name value` or `--name=value`)
const VALUED: [&str; 20] = [
  "--color",
  "--colour",
  "-c",
  "--save-baseline",
  "-s",
  "--baseline",
  "-b",
  "--baseline-lenient",
  "--format",
  "--profile-time",
  "--load-baseline",
  "--sample-size",
  "--warm-up-time",
  "--measurement-time",
  "--nresamples",
  "--noise-threshold",
  "--confidence-level",
  "--significance-level",
  "--plotting-backend",
  "--output-format",
];

/// Whether a guard over `benches` (criterion's names) runs: criterion
/// measures (not `--list`, `--test`, `--ignored` or `--profile-time`, the
/// profile command), and its filter, if it has one, names one of them. A
/// filter is matched as a plain or `^`/`$`-anchored name (criterion reads it
/// as a regular expression; one this cannot read skips the guard), or
/// exactly with `--exact`.
pub fn runs(benches: &[&str]) -> bool {
  let mut args = std::env::args().skip(1);
  let (mut filter, mut exact) = (None, false);
  while let Some(arg) = args.next() {
    let (name, inline) = match arg.split_once('=') {
      Some((name, _)) if name.starts_with('-') => (name.to_string(), true),
      _ => (arg.clone(), false),
    };
    match name.as_str() {
      "--list" | "--test" | "--ignored" | "--profile-time" => return false,
      "--exact" => exact = true,
      name if VALUED.contains(&name) => {
        if !inline {
          args.next();
        }
      }
      name if name.starts_with('-') => {}
      _ => filter = Some(arg),
    }
  }
  let Some(filter) = filter else {
    return true;
  };
  benches.iter().any(|bench| names(&filter, bench, exact))
}

/// Whether `filter` names `bench`
fn names(filter: &str, bench: &str, exact: bool) -> bool {
  if exact {
    return bench == filter;
  }
  let (start, rest) = filter
    .strip_prefix('^')
    .map_or((false, filter), |rest| (true, rest));
  let (end, text) = rest
    .strip_suffix('$')
    .map_or((false, rest), |text| (true, text));
  match (start, end) {
    (true, true) => bench == text,
    (true, false) => bench.starts_with(text),
    (false, true) => bench.ends_with(text),
    (false, false) => bench.contains(text),
  }
}

/// Note a missed target, `what`
pub fn miss(what: String) {
  MISSED
    .lock()
    .unwrap_or_else(|poisoned| poisoned.into_inner())
    .push(what);
}

/// Once every bench has run: with a target missed, print each miss and how
/// to profile it (`profile`), and fail the run
pub fn verdict(bench: &str, profile: &str) {
  let missed = MISSED
    .lock()
    .unwrap_or_else(|poisoned| poisoned.into_inner());
  if !missed.is_empty() {
    eprintln!("{bench} guards missed: {}. {profile}", missed.join("; "));
    std::process::exit(1);
  }
}
