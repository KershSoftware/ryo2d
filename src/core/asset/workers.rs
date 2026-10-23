//! The worker pool: threads that read and decode assets off the main thread.
//! On the web there are no threads, and the pump runs the jobs itself

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::OnceLock;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, unbounded};

/// A load to run off the main thread
pub(crate) type Job = Box<dyn FnOnce() + Send>;

/// How long the pump may spend running jobs a frame, where there are no workers
pub(crate) const INLINE_BUDGET: Duration = Duration::from_millis(4);

/// The most worker threads, whatever the machine
#[cfg(not(target_arch = "wasm32"))]
const MAX_THREADS: usize = 4;

pub(crate) struct Workers {
  jobs: Sender<Job>,
  /// The threads' end of `jobs`, which the pump runs itself where none started
  queue: Receiver<Job>,
  /// How many threads started, settled at the first queued job: a server
  /// that only loads at once, or is given its assets, starts none
  threads: OnceLock<usize>,
}

impl Workers {
  /// A pool with no threads yet; they start when the first job is queued
  pub(crate) fn new() -> Self {
    let (jobs, queue) = unbounded::<Job>();
    Self {
      jobs,
      queue,
      threads: OnceLock::new(),
    }
  }

  /// Starts the threads: one fewer than the machine's cores, from one to
  /// four. They stop once the pool is dropped and the queue is empty
  #[cfg(not(target_arch = "wasm32"))]
  fn start(&self) -> usize {
    let count = std::thread::available_parallelism()
      .map_or(1, std::num::NonZero::get)
      .saturating_sub(1)
      .clamp(1, MAX_THREADS);
    let mut started = 0;
    for n in 0..count {
      let queue = self.queue.clone();
      let thread = std::thread::Builder::new()
        .name(format!("ryo2d-assets-{n}"))
        .spawn(move || {
          for job in queue.iter() {
            run(job);
          }
        });
      match thread {
        Ok(_) => started += 1,
        Err(error) => {
          crate::engine_warn!(module: "asset", "could not start an asset worker: {error}")
        }
      }
    }
    started
  }

  /// The web has no threads: the pump runs the jobs
  #[cfg(target_arch = "wasm32")]
  fn start(&self) -> usize {
    0
  }

  /// How many worker threads started
  #[cfg(test)]
  pub(crate) fn threads(&self) -> usize {
    self.threads.get().copied().unwrap_or(0)
  }

  /// Queues a job for the next free worker, starting the threads first if
  /// this is the first job
  pub(crate) fn queue(&self, job: Job) {
    self.threads.get_or_init(|| self.start());
    // The pool holds the receiving end, so the send cannot fail
    let _ = self.jobs.send(job);
  }

  /// Runs queued jobs on this thread until `budget` has passed, when there
  /// are no worker threads. At least one job runs, so a big file still loads
  pub(crate) fn run_inline(&self, budget: Duration) {
    // Nothing queued yet, or threads run the jobs
    if self.threads.get() != Some(&0) || self.queue.is_empty() {
      return;
    }
    let start = web_time::Instant::now();
    while let Ok(job) = self.queue.try_recv() {
      run(job);
      if start.elapsed() >= budget {
        break;
      }
    }
  }
}

/// Runs a job; a panic that escapes it is caught so the thread lives on
fn run(job: Job) {
  let _ = catch_unwind(AssertUnwindSafe(job));
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::sync::Arc;
  use std::sync::atomic::{AtomicUsize, Ordering};

  #[cfg(not(target_arch = "wasm32"))]
  #[test]
  fn jobs_run_and_panicking_ones_do_not_stop_the_rest() {
    let workers = Workers::new();
    let done = Arc::new(AtomicUsize::new(0));
    // More panics than threads: without the catch, every thread would die
    for _ in 0..=MAX_THREADS {
      workers.queue(Box::new(|| panic!("a bad job")));
    }
    for _ in 0..8 {
      let done = Arc::clone(&done);
      workers.queue(Box::new(move || {
        done.fetch_add(1, Ordering::SeqCst);
      }));
    }
    let start = std::time::Instant::now();
    while done.load(Ordering::SeqCst) < 8 && start.elapsed() < Duration::from_secs(5) {
      workers.run_inline(INLINE_BUDGET);
      std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(done.load(Ordering::SeqCst), 8);
  }
}
