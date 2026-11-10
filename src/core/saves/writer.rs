//! The saves' writer: one thread that runs the save jobs in the order they
//! are sent, so the writes to one file land in call order. The thread starts
//! with the first job; where none can start (the web), a job runs where it is
//! sent.

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

use crossbeam_channel::{Receiver, Sender};
use web_time::Instant;

/// A job and its id
type Job<T> = (u64, Box<dyn FnOnce() -> T + Send>);

/// The jobs sent and not yet finished, by id, with the file each writes
#[derive(Default)]
struct InFlight {
  files: Mutex<BTreeMap<u64, String>>,
  idle: Condvar,
}

impl InFlight {
  fn finish(&self, id: u64) {
    let mut files = lock(&self.files);
    files.remove(&id);
    if files.is_empty() {
      self.idle.notify_all();
    }
  }
}

struct State<T> {
  jobs: Option<Sender<Job<T>>>,
  /// A thread was tried and would not start: jobs run where they are sent
  inline: bool,
  next: u64,
}

/// One thread running jobs in order, and the results that came back
pub(crate) struct Writer<T: Send + 'static> {
  /// The thread's name
  name: &'static str,
  state: Mutex<State<T>>,
  in_flight: Arc<InFlight>,
  done_sender: Sender<(u64, Option<T>)>,
  done: Receiver<(u64, Option<T>)>,
}

impl<T: Send + 'static> Writer<T> {
  /// A writer whose thread is called `name`
  pub(crate) fn named(name: &'static str) -> Self {
    let (done_sender, done) = crossbeam_channel::unbounded();
    Writer {
      name,
      state: Mutex::new(State {
        jobs: None,
        inline: false,
        next: 1,
      }),
      in_flight: Arc::default(),
      done_sender,
      done,
    }
  }

  /// A writer with no thread, as on the web
  #[cfg(test)]
  pub(crate) fn without_thread() -> Self {
    let writer = Writer::named("ryo2d-test");
    lock(&writer.state).inline = true;
    writer
  }

  /// Queues `job`, which writes `file`, behind every job sent before it (or
  /// runs it here, with no thread); returns the id its result comes back with
  pub(crate) fn send(&self, file: &str, job: impl FnOnce() -> T + Send + 'static) -> u64 {
    let mut state = lock(&self.state);
    let id = state.next;
    state.next += 1;
    let job: Job<T> = (id, Box::new(job));
    let job = match self.sender(&mut state) {
      Some(jobs) => {
        lock(&self.in_flight.files).insert(id, file.to_string());
        match jobs.send(job) {
          Ok(()) => return id,
          Err(error) => {
            self.in_flight.finish(id);
            error.into_inner()
          }
        }
      }
      None => job,
    };
    drop(state);
    let _ = self.done_sender.send(run(job));
    id
  }

  /// The results that came back since: each job's id and what it made
  /// (`None`: it panicked). Never waits
  pub(crate) fn finished(&self) -> impl Iterator<Item = (u64, Option<T>)> + '_ {
    self.done.try_iter()
  }

  /// Waits until every job sent has finished, or until `deadline`; true when
  /// they all have
  pub(crate) fn wait_idle(&self, deadline: Instant) -> bool {
    let mut files = lock(&self.in_flight.files);
    loop {
      if files.is_empty() {
        return true;
      }
      // Jobs run where they are sent on the web, so this is never reached there
      let left = deadline.saturating_duration_since(Instant::now());
      if left.is_zero() {
        return false;
      }
      files = self
        .in_flight
        .idle
        .wait_timeout(files, left)
        .unwrap_or_else(PoisonError::into_inner)
        .0;
    }
  }

  /// The files of the jobs sent and not yet finished, in the order sent, each
  /// once
  pub(crate) fn unfinished(&self) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for name in lock(&self.in_flight.files).values() {
      if !names.contains(name) {
        names.push(name.clone());
      }
    }
    names
  }

  /// The thread's channel, starting the thread with the first job; `None` if
  /// no thread could be started
  fn sender<'s>(&self, state: &'s mut State<T>) -> Option<&'s Sender<Job<T>>> {
    if state.jobs.is_none() && !state.inline {
      let (jobs, inbox) = crossbeam_channel::unbounded::<Job<T>>();
      let done = self.done_sender.clone();
      let in_flight = Arc::clone(&self.in_flight);
      let spawned =
        std::thread::Builder::new()
          .name(self.name.into())
          .spawn(crate::core::logging::scoped(move || {
            log::debug!("the save writer thread started");
            // Every queued job runs, even once no one reads the results: a save
            // sent before the engine went still lands
            for job in inbox {
              let id = job.0;
              let _ = done.send(run(job));
              in_flight.finish(id);
            }
          }));
      match spawned {
        Ok(_) => state.jobs = Some(jobs),
        // Expected on the web, so not a warning
        Err(error) => {
          log::debug!(
            "no thread for the {} ({error}); saves are written on the main thread",
            self.name
          );
          state.inline = true;
        }
      }
    }
    state.jobs.as_ref()
  }
}

/// Runs `job`, catching a panic
fn run<T>((id, job): Job<T>) -> (u64, Option<T>) {
  (id, catch_unwind(AssertUnwindSafe(job)).ok())
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
  mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
  use std::sync::atomic::{AtomicBool, Ordering};
  use std::time::Duration;

  use super::*;

  #[test]
  fn jobs_run_in_the_order_sent_and_a_panic_comes_back_as_none() {
    let writer = Writer::<u32>::named("ryo2d-test");
    let order = Arc::new(Mutex::new(Vec::new()));
    let ids: Vec<u64> = (0..5)
      .map(|n| {
        let order = Arc::clone(&order);
        writer.send("f", move || {
          std::thread::sleep(Duration::from_millis(5 - n as u64));
          lock(&order).push(n);
          n
        })
      })
      .collect();
    let broken = writer.send("g", || panic!("a broken job"));
    assert!(writer.wait_idle(Instant::now() + Duration::from_secs(5)));
    assert_eq!(*lock(&order), [0, 1, 2, 3, 4]);
    let results: Vec<_> = writer.finished().collect();
    let mut expected: Vec<_> = ids.iter().zip(0..).map(|(id, n)| (*id, Some(n))).collect();
    expected.push((broken, None));
    assert_eq!(results, expected);
  }

  #[test]
  fn waiting_gives_up_at_the_deadline_and_names_the_unfinished_files() {
    let writer = Writer::<()>::named("ryo2d-test");
    let release = Arc::new(AtomicBool::new(false));
    let held = Arc::clone(&release);
    writer.send("slow.ryo-save", move || {
      while !held.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(1));
      }
    });
    writer.send("slow.ryo-save", || {});
    writer.send("other.ryo-save", || {});
    assert!(!writer.wait_idle(Instant::now() + Duration::from_millis(20)));
    assert_eq!(writer.unfinished(), ["slow.ryo-save", "other.ryo-save"]);
    release.store(true, Ordering::SeqCst);
    assert!(writer.wait_idle(Instant::now() + Duration::from_secs(5)));
    assert!(writer.unfinished().is_empty());
  }

  #[test]
  fn with_no_thread_a_job_runs_where_it_is_sent() {
    let writer = Writer::<u32>::without_thread();
    let job = writer.send("f", || 3);
    assert!(writer.unfinished().is_empty());
    assert!(writer.wait_idle(Instant::now()));
    assert_eq!(writer.finished().collect::<Vec<_>>(), [(job, Some(3))]);
  }
}
