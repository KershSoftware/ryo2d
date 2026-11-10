//! Work for spawned maps done on a thread of its own: a map's first
//! collision and shadow builds (classifying every cell, tracing the
//! outlines, chaining the occluders), so the frame a map lands in only
//! hands them over, and a streamed map's chunks, generated on a thread of
//! their own so a chunk never waits behind a landing. A thread starts with
//! its first job; where none can start (the web), a job runs where it is
//! sent, so its result is there at once.

use std::panic::{AssertUnwindSafe, catch_unwind};

use crossbeam_channel::{Receiver, Sender};

/// A job and its id
type Job<T> = (u64, Box<dyn FnOnce() -> T + Send>);

/// One worker thread for one system's jobs, and the results that came back
pub(crate) struct Worker<T: Send + 'static> {
  /// The thread's name
  name: &'static str,
  jobs: Option<Sender<Job<T>>>,
  /// A thread was tried and would not start: jobs run where they are sent
  inline: bool,
  done_sender: Sender<(u64, Option<T>)>,
  done: Receiver<(u64, Option<T>)>,
  next: u64,
}

impl<T: Send + 'static> Default for Worker<T> {
  fn default() -> Self {
    let (done_sender, done) = crossbeam_channel::unbounded();
    Worker {
      name: "ryo2d map builder",
      jobs: None,
      inline: false,
      done_sender,
      done,
      next: 1,
    }
  }
}

impl<T: Send + 'static> Worker<T> {
  /// A worker whose thread is called `name`
  pub(crate) fn named(name: &'static str) -> Self {
    Worker {
      name,
      ..Worker::default()
    }
  }

  /// A worker with no thread, as on the web
  #[cfg(test)]
  pub(crate) fn without_thread() -> Self {
    Worker {
      inline: true,
      ..Worker::default()
    }
  }

  /// Sends `job` to the thread (or runs it here, with no thread); returns
  /// the id its result comes back with
  pub(crate) fn send(&mut self, job: impl FnOnce() -> T + Send + 'static) -> u64 {
    let id = self.next;
    self.next += 1;
    let job: Job<T> = (id, Box::new(job));
    let job = match self.sender() {
      Some(jobs) => match jobs.send(job) {
        Ok(()) => return id,
        Err(error) => error.into_inner(),
      },
      None => job,
    };
    let _ = self.done_sender.send(run(job));
    id
  }

  /// Whether jobs run where they are sent: no thread would start (the web),
  /// found when the first job is sent
  pub(crate) fn runs_inline(&self) -> bool {
    self.inline
  }

  /// The results that came back since: each job's id and what it made
  /// (`None`: it panicked). Never waits
  pub(crate) fn finished(&self) -> impl Iterator<Item = (u64, Option<T>)> + '_ {
    self.done.try_iter()
  }

  /// The thread's channel, starting the thread with the first job; `None`
  /// if no thread could be started
  fn sender(&mut self) -> Option<&Sender<Job<T>>> {
    if self.jobs.is_none() && !self.inline {
      let (jobs, inbox) = crossbeam_channel::unbounded::<Job<T>>();
      let done = self.done_sender.clone();
      let spawned =
        std::thread::Builder::new()
          .name(self.name.into())
          .spawn(crate::core::logging::scoped(move || {
            for job in inbox {
              if done.send(run(job)).is_err() {
                break;
              }
            }
          }));
      match spawned {
        Ok(_) => self.jobs = Some(jobs),
        // Expected on the web, so not a warning
        Err(error) => {
          log::debug!(
            "no thread for the {} ({error}); its jobs run on the main thread",
            self.name
          );
          self.inline = true;
        }
      }
    }
    self.jobs.as_ref()
  }
}

/// Runs `job`, catching a panic
fn run<T>((id, job): Job<T>) -> (u64, Option<T>) {
  (id, catch_unwind(AssertUnwindSafe(job)).ok())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_job_s_result_comes_back_with_its_id_and_a_panic_as_none() {
    let mut worker = Worker::<u32>::default();
    let first = worker.send(|| 7);
    let second = worker.send(|| panic!("a broken job"));
    let mut results = Vec::new();
    let start = std::time::Instant::now();
    while results.len() < 2 && start.elapsed().as_secs() < 5 {
      results.extend(worker.finished());
      std::thread::yield_now();
    }
    results.sort_by_key(|(id, _)| *id);
    assert_eq!(results, [(first, Some(7)), (second, None)]);
  }

  #[test]
  fn with_no_thread_a_job_s_result_is_there_at_once() {
    let mut worker = Worker::<u32>::without_thread();
    let job = worker.send(|| 3);
    let results: Vec<(u64, Option<u32>)> = worker.finished().collect();
    assert_eq!(results, [(job, Some(3))]);
  }
}
