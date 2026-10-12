//! A GPU error scope checked on later frames instead of waited on.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};

pub(crate) struct PendingCheck {
  future: Pin<Box<dyn Future<Output = Option<String>>>>,
}

impl PendingCheck {
  pub(crate) fn new(future: impl Future<Output = Option<String>> + 'static) -> Self {
    Self {
      future: Box::pin(future),
    }
  }

  /// `Ready(None)`: the GPU accepted it. `Ready(Some(message))`: it did not.
  pub(crate) fn poll(&mut self) -> Poll<Option<String>> {
    let mut context = Context::from_waker(Waker::noop());
    self.future.as_mut().poll(&mut context)
  }
}

/// A pipeline the GPU may still reject: usable only once its check passed.
pub(crate) enum Checked<P> {
  Pending { pipeline: P, check: PendingCheck },
  Ready(P),
  Failed,
}

impl<P> Checked<P> {
  /// A pipeline just created inside `scope`, checked on a later frame.
  pub(crate) fn pending(pipeline: P, scope: wgpu::ErrorScopeGuard) -> Self {
    let pop = scope.pop();
    Checked::Pending {
      pipeline,
      check: PendingCheck::new(async move { pop.await.map(|error| error.to_string()) }),
    }
  }

  pub(crate) fn ready(&self) -> Option<&P> {
    match self {
      Checked::Ready(pipeline) => Some(pipeline),
      _ => None,
    }
  }

  /// Settle a pending pipeline whose check came back: it becomes ready, or
  /// failed with the GPU's message, which is returned.
  pub(crate) fn settle(&mut self) -> Option<String> {
    let Checked::Pending { check, .. } = self else {
      return None;
    };
    let Poll::Ready(result) = check.poll() else {
      return None;
    };
    match (std::mem::replace(self, Checked::Failed), result) {
      (Checked::Pending { pipeline, .. }, None) => {
        *self = Checked::Ready(pipeline);
        None
      }
      (_, error) => error,
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_ready_check_reports_without_blocking() {
    let mut ok = PendingCheck::new(async { None });
    assert_eq!(ok.poll(), Poll::Ready(None));
    let mut bad = PendingCheck::new(async { Some("invalid".to_string()) });
    assert_eq!(bad.poll(), Poll::Ready(Some("invalid".to_string())));
  }

  #[test]
  fn a_check_that_is_not_ready_stays_pending() {
    let mut never = PendingCheck::new(std::future::pending());
    assert_eq!(never.poll(), Poll::Pending);
  }
}
