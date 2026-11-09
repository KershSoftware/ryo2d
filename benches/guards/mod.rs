//! What the benches' guards share: whether criterion's arguments ask for a
//! guarded bench, the targets missed, the run's verdict once every bench has
//! run, and criterion's medians read back from where it writes them

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

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
// Only the benches that have targets use it
#[allow(dead_code)]
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

/// Where criterion writes its results, and the medians are read from:
/// `$CRITERION_HOME`, else `$CARGO_TARGET_DIR/criterion`, else the
/// workspace's `target/criterion`
// Only the benches that print criterion's medians use it
#[allow(dead_code)]
pub fn results_dir() -> PathBuf {
  if let Some(home) = std::env::var_os("CRITERION_HOME") {
    return PathBuf::from(home);
  }
  std::env::var_os("CARGO_TARGET_DIR")
    .map(PathBuf::from)
    .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("target"))
    .join("criterion")
}

/// Print the median of `bench` (of `of`) from the run criterion just wrote
/// to `results`, in microseconds to `decimals` places; nothing for results
/// an earlier run left (this run skipped the bench, as a filter or `--list`
/// does)
// Only the benches that print criterion's medians use it
#[allow(dead_code)]
pub fn print_median(results: &Path, bench: &str, of: &str, started: SystemTime, decimals: usize) {
  let path = results.join(bench).join("new/estimates.json");
  let written = std::fs::metadata(&path)
    .and_then(|meta| meta.modified())
    .is_ok_and(|modified| modified >= started);
  if !written {
    println!("{bench} median: not measured in this run");
    return;
  }
  let median = std::fs::read_to_string(&path).ok().and_then(|text| {
    let at = text.find("\"median\"")?;
    let rest = &text[at..];
    let from = rest.find("\"point_estimate\":")? + "\"point_estimate\":".len();
    let to = rest[from..].find([',', '}'])? + from;
    rest[from..to].trim().parse::<f64>().ok()
  });
  match median {
    Some(ns) => println!("{bench} median, of {of}: {:.*} us", decimals, ns / 1000.0),
    None => println!("{bench} median: unavailable ({})", path.display()),
  }
}
