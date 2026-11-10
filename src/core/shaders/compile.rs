//! The thread a game shader's new version compiles on while its old one
//! draws.
//!
//! Each renderer that draws game shaders (the sprite batch, and each effect
//! stage) has a [`Compiler`]: a thread of its own, started with its first
//! job, that makes a new version's shader module and pipelines through the
//! shared wgpu `Device`. Metal compiles a pipeline in the call that makes
//! it, 70 to 130 ms for a new shader, so on the main thread a save stalled
//! the frame it landed in.
//!
//! - One job is out at a time. The renderer keeps the versions it waits
//!   for in its shader cache, at most one per shader, and sends the next
//!   only when the last came back, so nothing queues up under repeated
//!   saves; the result of a version saved over meanwhile is dropped.
//! - The thread ends once its renderer is dropped and its job is done:
//!   nothing waits for it, so the engine shuts down without waiting on a
//!   compile.
//! - A job that panics comes back as `None`.
//! - The web has no threads to compile on: there every version compiles
//!   at once on the main thread, and nothing is sent here.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
#[cfg(test)]
use std::sync::{Condvar, Mutex, PoisonError};

use crossbeam_channel::{Receiver, Sender};

/// A job and the source key it compiles
type Job<D> = (u64, Box<dyn FnOnce() -> D + Send>);

/// Whether a renderer compiles the new versions of shaders it already draws
/// off the main thread: everywhere but the web
pub(crate) const OFF_THREAD: bool = cfg!(not(target_arch = "wasm32"));

/// A thread compiling one job at a time, its results read without waiting
pub(crate) struct Compiler<D> {
  /// To the thread, made with the first job
  jobs: Option<Sender<Job<D>>>,
  /// The thread's results: the source key, and what it made (`None`: the
  /// job panicked)
  done: Receiver<(u64, Option<D>)>,
  done_sender: Sender<(u64, Option<D>)>,
  /// The source key of the job out
  out: Option<u64>,
  /// Holds the thread's next job until released (tests)
  gate: Arc<CompileGate>,
  /// Jobs sent so far (tests)
  #[cfg(test)]
  started: usize,
}

impl<D: Send + 'static> Compiler<D> {
  pub(crate) fn new() -> Self {
    let (done_sender, done) = crossbeam_channel::unbounded();
    Self {
      jobs: None,
      done,
      done_sender,
      out: None,
      gate: Arc::default(),
      #[cfg(test)]
      started: 0,
    }
  }

  /// Whether a job is out
  pub(crate) fn busy(&self) -> bool {
    self.out.is_some()
  }

  /// Sends `job`, compiling the source under `key`, to the thread; it is
  /// started with the first. Call only when not [`busy`](Self::busy)
  pub(crate) fn start(&mut self, key: u64, job: impl FnOnce() -> D + Send + 'static) {
    debug_assert!(self.out.is_none(), "one compile at a time");
    #[cfg(test)]
    {
      self.started += 1;
    }
    self.out = Some(key);
    let job: Job<D> = (key, Box::new(job));
    let job = match self.sender() {
      Some(jobs) => match jobs.send(job) {
        Ok(()) => return,
        Err(error) => error.into_inner(),
      },
      None => job,
    };
    // No thread to send it to: it runs here, and its result is read as one
    // from the thread
    let _ = self.done_sender.send(run(job));
  }

  /// The job that came back, if one did: its source key and what it made
  /// (`None`: it panicked). Never waits
  pub(crate) fn finished(&mut self) -> Option<(u64, Option<D>)> {
    self.out?;
    let done = self.done.try_recv().ok()?;
    self.out = None;
    Some(done)
  }

  /// The thread's channel, starting the thread with the first job; `None`
  /// if no thread could be started
  fn sender(&mut self) -> Option<&Sender<Job<D>>> {
    if self.jobs.is_none() {
      let (jobs, inbox) = crossbeam_channel::unbounded::<Job<D>>();
      let done = self.done_sender.clone();
      let gate = self.gate.clone();
      let spawned = std::thread::Builder::new()
        .name("ryo2d shader compiler".into())
        .spawn(crate::core::logging::scoped(move || {
          log::debug!("the shader compiler thread started");
          for job in inbox {
            gate.wait();
            if done.send(run(job)).is_err() {
              break;
            }
          }
        }));
      match spawned {
        Ok(_) => self.jobs = Some(jobs),
        Err(error) => {
          crate::engine_warn!(
            module: "Shaders",
            "no thread to compile shaders on ({error}); they compile on the main thread"
          );
          return None;
        }
      }
    }
    self.jobs.as_ref()
  }

  /// The gate the thread waits at before each job (tests)
  #[cfg(test)]
  pub(crate) fn gate(&self) -> Arc<CompileGate> {
    self.gate.clone()
  }

  /// Jobs sent so far (tests)
  #[cfg(test)]
  pub(crate) fn started(&self) -> usize {
    self.started
  }
}

/// Runs `job`, catching a panic
fn run<D>((key, job): Job<D>) -> (u64, Option<D>) {
  (key, catch_unwind(AssertUnwindSafe(job)).ok())
}

/// Holds a compile thread's next job while closed. Open but in tests
#[derive(Default)]
pub(crate) struct CompileGate {
  #[cfg(test)]
  held: Mutex<bool>,
  #[cfg(test)]
  opened: Condvar,
}

impl CompileGate {
  /// Waits while the gate is held
  fn wait(&self) {
    #[cfg(test)]
    {
      let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
      while *held {
        held = self
          .opened
          .wait(held)
          .unwrap_or_else(PoisonError::into_inner);
      }
    }
  }

  /// Holds the thread's next job, or lets it go (tests)
  #[cfg(test)]
  pub(crate) fn hold(&self, hold: bool) {
    *self.held.lock().unwrap_or_else(PoisonError::into_inner) = hold;
    self.opened.notify_all();
  }
}

/// Runs `make` inside a validation error scope and waits for the scope: what
/// `make` made, or the GPU's message rejecting it. On native the scope's
/// outcome is known as it is popped, so the wait is a poll; it gives up
/// after about a second rather than hang the thread
pub(crate) fn checked<T>(device: &wgpu::Device, make: impl FnOnce() -> T) -> Result<T, String> {
  use std::future::Future;
  use std::task::{Context, Poll, Waker};
  let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
  let made = make();
  let mut popped = std::pin::pin!(scope.pop());
  let mut context = Context::from_waker(Waker::noop());
  for _ in 0..1000 {
    match popped.as_mut().poll(&mut context) {
      Poll::Ready(None) => return Ok(made),
      Poll::Ready(Some(error)) => return Err(error.to_string()),
      Poll::Pending => std::thread::sleep(std::time::Duration::from_millis(1)),
    }
  }
  Err("the GPU's check of the pipeline did not come back".to_string())
}

#[cfg(test)]
mod tests {
  use std::time::{Duration, Instant};

  use super::*;

  /// Waits for `compiler`'s job, at most two seconds
  fn wait<D: Send + 'static>(compiler: &mut Compiler<D>) -> (u64, Option<D>) {
    let start = Instant::now();
    loop {
      if let Some(done) = compiler.finished() {
        return done;
      }
      assert!(
        start.elapsed() < Duration::from_secs(2),
        "the job came back"
      );
      std::thread::sleep(Duration::from_millis(1));
    }
  }

  #[test]
  fn a_job_runs_on_the_thread_and_comes_back_by_its_key() {
    let mut compiler = Compiler::new();
    assert!(compiler.finished().is_none(), "nothing out");
    let main = std::thread::current().id();
    compiler.start(7, move || std::thread::current().id() != main);
    assert!(compiler.busy());
    assert_eq!(wait(&mut compiler), (7, Some(true)), "off the main thread");
    assert!(!compiler.busy());
    compiler.start(8, || false);
    assert_eq!(
      wait(&mut compiler),
      (8, Some(false)),
      "the same thread again"
    );
    assert_eq!(compiler.started(), 2);
  }

  #[test]
  fn a_job_that_panics_comes_back_empty() {
    let mut compiler = Compiler::<u32>::new();
    compiler.start(1, || panic!("a broken job"));
    assert_eq!(wait(&mut compiler), (1, None));
    compiler.start(2, || 5);
    assert_eq!(wait(&mut compiler), (2, Some(5)), "the thread lives on");
  }

  #[test]
  fn dropping_a_compiler_waits_for_no_job() {
    let mut compiler = Compiler::new();
    let gate = compiler.gate();
    gate.hold(true);
    compiler.start(1, || 1);
    let start = Instant::now();
    drop(compiler);
    assert!(start.elapsed() < Duration::from_millis(100));
    gate.hold(false);
  }
}
