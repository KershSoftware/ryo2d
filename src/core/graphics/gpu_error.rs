//! GPU setup and surface failures as values, not panics.

use std::sync::{Arc, Mutex};

/// Why the GPU could not be set up or kept running.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GpuError {
  #[error("no compatible GPU adapter: {0}")]
  NoAdapter(String),
  #[error("the GPU device could not be created: {0}")]
  RequestDevice(String),
  #[error("the window surface could not be created: {0}")]
  CreateSurface(String),
  #[error("the GPU device was lost: {0}")]
  DeviceLost(String),
}

/// What the surface reported when asked for the next frame's texture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SurfaceState {
  Success,
  Suboptimal,
  Timeout,
  Occluded,
  Outdated,
  Lost,
  Validation,
}

/// What the frame does about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SurfaceAction {
  Draw { then_reconfigure: bool },
  Skip,
  Reconfigure,
  Recreate,
  Fatal,
}

pub(crate) fn surface_action(state: SurfaceState) -> SurfaceAction {
  match state {
    SurfaceState::Success => SurfaceAction::Draw {
      then_reconfigure: false,
    },
    SurfaceState::Suboptimal => SurfaceAction::Draw {
      then_reconfigure: true,
    },
    SurfaceState::Timeout | SurfaceState::Occluded => SurfaceAction::Skip,
    SurfaceState::Outdated => SurfaceAction::Reconfigure,
    SurfaceState::Lost => SurfaceAction::Recreate,
    SurfaceState::Validation => SurfaceAction::Fatal,
  }
}

/// A GPU error reported by a callback, picked up by the next frame.
#[derive(Clone, Default)]
pub(crate) struct GpuFault(Arc<Mutex<Option<String>>>);

impl GpuFault {
  pub(crate) fn record(&self, message: impl Into<String>) {
    if let Ok(mut slot) = self.0.lock() {
      slot.get_or_insert(message.into());
    }
  }

  pub(crate) fn take(&self) -> Option<String> {
    self.0.lock().ok().and_then(|mut slot| slot.take())
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn every_surface_state_has_an_action() {
    use SurfaceAction::*;
    assert_eq!(
      surface_action(SurfaceState::Success),
      Draw {
        then_reconfigure: false
      }
    );
    assert_eq!(
      surface_action(SurfaceState::Suboptimal),
      Draw {
        then_reconfigure: true
      }
    );
    assert_eq!(surface_action(SurfaceState::Timeout), Skip);
    assert_eq!(surface_action(SurfaceState::Occluded), Skip);
    assert_eq!(surface_action(SurfaceState::Outdated), Reconfigure);
    assert_eq!(surface_action(SurfaceState::Lost), Recreate);
    assert_eq!(surface_action(SurfaceState::Validation), Fatal);
  }

  #[test]
  fn a_fault_is_taken_once() {
    let fault = GpuFault::default();
    assert_eq!(fault.take(), None);
    fault.clone().record("device lost");
    assert_eq!(fault.take().as_deref(), Some("device lost"));
    assert_eq!(fault.take(), None);
  }
}
