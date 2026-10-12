//! Engine-side capture scheduling: which presented frame to capture, where,
//! and when auto-capture has finished.

use std::path::PathBuf;
use std::thread::JoinHandle;

use super::config::AutoCaptureConfig;
use super::writer::{default_file_name, spawn_write};
use super::{CaptureTarget, CapturedFrame};
use crate::core::error::CaptureResult;

/// Outcome of a capture, for logging.
#[derive(Debug, PartialEq, Eq)]
pub enum CaptureReport {
  /// The PNG was written
  Written { frame: u64, path: PathBuf },
  /// Reading back or writing the frame failed
  Failed { frame: u64, error: String },
}

/// The capture requested for the upcoming frame.
struct Pending {
  frame: u64,
  scheduled: bool,
}

struct PendingWrite {
  frame: u64,
  scheduled: bool,
  handle: JoinHandle<CaptureResult<PathBuf>>,
}

/// Tracks presented frames and decides which ones to capture.
///
/// Call [`path_for_next_frame`](Self::path_for_next_frame) before rendering and
/// [`on_presented`](Self::on_presented) once the frame reached the screen.
/// Frames that are never presented do not advance the counter, so "frame N"
/// is always the Nth presented frame.
pub struct CaptureState {
  config: AutoCaptureConfig,
  presented: u64,
  next_scheduled: usize,
  manual: Option<CaptureTarget>,
  pending: Option<Pending>,
  writes: Vec<PendingWrite>,
  scheduled_failed: bool,
}

impl CaptureState {
  /// Create a state with the given auto-capture schedule.
  pub fn new(config: AutoCaptureConfig) -> Self {
    Self {
      config,
      presented: 0,
      next_scheduled: 0,
      manual: None,
      pending: None,
      writes: Vec::new(),
      scheduled_failed: false,
    }
  }

  /// Number of frames presented so far.
  pub fn presented_frames(&self) -> u64 {
    self.presented
  }

  /// Whether `RYO_CAPTURE_EXIT` ends the run after the last scheduled capture.
  pub fn exits_after_last(&self) -> bool {
    self.config.exit_after
  }

  /// Request a capture of the next presented frame (F12, console).
  pub fn request(&mut self, target: CaptureTarget) {
    // Captures are written to disk, which the web does not have
    if cfg!(target_arch = "wasm32") {
      crate::engine_warn!(module: "Capture", "frame capture is not available on the web");
      return;
    }
    self.manual = Some(target);
  }

  /// Where to capture the upcoming frame, if a capture is due.
  ///
  /// A scheduled frame takes priority; a manual request then waits one frame.
  pub fn path_for_next_frame(&mut self) -> Option<PathBuf> {
    let frame = self.presented + 1;
    self.pending = None;

    if self.config.frames.get(self.next_scheduled) == Some(&frame) {
      self.pending = Some(Pending {
        frame,
        scheduled: true,
      });
      return Some(self.config.dir.join(default_file_name(frame)));
    }

    let path = match self.manual.as_ref()? {
      CaptureTarget::Default => self.config.dir.join(default_file_name(frame)),
      CaptureTarget::Path(path) => path.clone(),
    };
    self.pending = Some(Pending {
      frame,
      scheduled: false,
    });
    Some(path)
  }

  /// Record that a frame reached the screen, with the renderer's capture result.
  ///
  /// Successful captures are written on a background thread and reported by
  /// [`poll_writes`](Self::poll_writes); immediate failures are returned here.
  pub fn on_presented(
    &mut self,
    result: Option<CaptureResult<CapturedFrame>>,
  ) -> Vec<CaptureReport> {
    self.presented += 1;
    let Some(pending) = self.pending.take() else {
      return Vec::new();
    };

    if pending.scheduled {
      self.next_scheduled += 1;
    } else {
      self.manual = None;
    }

    match result {
      Some(Ok(captured)) => {
        self.writes.push(PendingWrite {
          frame: pending.frame,
          scheduled: pending.scheduled,
          handle: spawn_write(captured.image, captured.path),
        });
        Vec::new()
      }
      Some(Err(error)) => vec![self.failed(pending.frame, pending.scheduled, error.to_string())],
      None => vec![self.failed(
        pending.frame,
        pending.scheduled,
        "renderer produced no capture".to_string(),
      )],
    }
  }

  /// Collect background writes that have finished.
  pub fn poll_writes(&mut self) -> Vec<CaptureReport> {
    self.collect_writes(false)
  }

  /// Wait for every in-flight write and report scheduled frames that were
  /// never presented (call when the event loop exits).
  pub fn finish(&mut self) -> Vec<CaptureReport> {
    let mut reports = self.collect_writes(true);
    let missed = self.config.frames[self.next_scheduled..].to_vec();
    self.next_scheduled = self.config.frames.len();
    for frame in missed {
      reports.push(self.failed(
        frame,
        true,
        "game exited before this frame was presented".to_string(),
      ));
    }
    reports
  }

  /// True once `RYO_CAPTURE_EXIT` is set and every scheduled capture is done.
  pub fn exit_requested(&self) -> bool {
    self.config.exit_after
      && self.next_scheduled >= self.config.frames.len()
      && self.writes.is_empty()
  }

  /// True if `RYO_CAPTURE_EXIT` is set and a scheduled capture failed.
  pub fn exit_with_failure(&self) -> bool {
    self.config.exit_after && self.scheduled_failed
  }

  fn collect_writes(&mut self, wait: bool) -> Vec<CaptureReport> {
    let mut reports = Vec::new();
    let mut still_running = Vec::new();
    for write in std::mem::take(&mut self.writes) {
      if !wait && !write.handle.is_finished() {
        still_running.push(write);
        continue;
      }
      let report = match write.handle.join() {
        Ok(Ok(path)) => CaptureReport::Written {
          frame: write.frame,
          path,
        },
        Ok(Err(error)) => self.failed(write.frame, write.scheduled, error.to_string()),
        Err(_) => self.failed(
          write.frame,
          write.scheduled,
          "capture writer thread panicked".to_string(),
        ),
      };
      reports.push(report);
    }
    self.writes = still_running;
    reports
  }

  fn failed(&mut self, frame: u64, scheduled: bool, error: String) -> CaptureReport {
    if scheduled {
      self.scheduled_failed = true;
    }
    CaptureReport::Failed { frame, error }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::error::CaptureError;
  use image::RgbaImage;
  use std::path::Path;

  fn config(frames: &[u64], dir: &Path, exit_after: bool) -> AutoCaptureConfig {
    AutoCaptureConfig {
      frames: frames.to_vec(),
      dir: dir.to_path_buf(),
      exit_after,
    }
  }

  fn captured(path: PathBuf) -> Option<CaptureResult<CapturedFrame>> {
    Some(Ok(CapturedFrame {
      image: RgbaImage::new(2, 2),
      path,
    }))
  }

  #[test]
  fn nothing_due_without_schedule_or_request() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = CaptureState::new(config(&[], dir.path(), false));
    for _ in 0..3 {
      assert_eq!(state.path_for_next_frame(), None);
      assert!(state.on_presented(None).is_empty());
    }
    assert!(!state.exit_requested());
  }

  #[test]
  fn scheduled_frames_use_default_names() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = CaptureState::new(config(&[2], dir.path(), false));
    assert_eq!(state.path_for_next_frame(), None);
    state.on_presented(None);
    assert_eq!(
      state.path_for_next_frame(),
      Some(dir.path().join("frame-000002.png"))
    );
  }

  #[test]
  fn unpresented_frame_keeps_its_number() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = CaptureState::new(config(&[1], dir.path(), false));
    let first = state.path_for_next_frame();
    // Frame skipped (surface lost): on_presented is not called
    assert_eq!(state.path_for_next_frame(), first);
  }

  #[test]
  fn manual_request_is_used_once() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("shot.png");
    let mut state = CaptureState::new(config(&[], dir.path(), false));
    state.request(CaptureTarget::Path(target.clone()));

    assert_eq!(state.path_for_next_frame(), Some(target.clone()));
    state.on_presented(captured(target));
    assert_eq!(state.path_for_next_frame(), None);
    state.finish();
  }

  #[test]
  fn default_manual_request_uses_capture_dir() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = CaptureState::new(config(&[], dir.path(), false));
    state.on_presented(None);
    state.request(CaptureTarget::Default);
    assert_eq!(
      state.path_for_next_frame(),
      Some(dir.path().join("frame-000002.png"))
    );
  }

  #[test]
  fn scheduled_frame_takes_priority_over_manual() {
    let dir = tempfile::tempdir().unwrap();
    let manual = dir.path().join("manual.png");
    let scheduled = dir.path().join("frame-000001.png");
    let mut state = CaptureState::new(config(&[1], dir.path(), false));
    state.request(CaptureTarget::Path(manual.clone()));

    assert_eq!(state.path_for_next_frame(), Some(scheduled.clone()));
    state.on_presented(captured(scheduled));
    assert_eq!(state.path_for_next_frame(), Some(manual));
    state.finish();
  }

  #[test]
  fn written_capture_is_reported_and_exit_follows() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = CaptureState::new(config(&[1], dir.path(), true));
    let path = state.path_for_next_frame().unwrap();
    assert!(!state.exit_requested());

    assert!(state.on_presented(captured(path.clone())).is_empty());
    let reports = state.finish();

    assert_eq!(
      reports,
      vec![CaptureReport::Written {
        frame: 1,
        path: path.clone()
      }]
    );
    assert!(path.exists());
    assert!(state.exit_requested());
    assert!(!state.exit_with_failure());
  }

  #[test]
  fn readback_failure_on_scheduled_frame_fails_exit() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = CaptureState::new(config(&[1], dir.path(), true));
    state.path_for_next_frame();

    let reports = state.on_presented(Some(Err(CaptureError::Readback("lost device".into()))));

    assert!(matches!(
      reports.as_slice(),
      [CaptureReport::Failed { frame: 1, .. }]
    ));
    assert!(state.exit_requested());
    assert!(state.exit_with_failure());
  }

  #[test]
  fn missing_capture_result_counts_as_failure() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = CaptureState::new(config(&[1], dir.path(), false));
    state.path_for_next_frame();

    let reports = state.on_presented(None);

    assert!(matches!(
      reports.as_slice(),
      [CaptureReport::Failed { frame: 1, .. }]
    ));
  }

  #[test]
  fn finish_reports_scheduled_frames_never_presented() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = CaptureState::new(config(&[1, 5], dir.path(), true));
    let path = state.path_for_next_frame().unwrap();
    state.on_presented(captured(path.clone()));

    let reports = state.finish();

    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0], CaptureReport::Written { frame: 1, path });
    assert!(matches!(reports[1], CaptureReport::Failed { frame: 5, .. }));
    assert!(state.exit_with_failure());
    assert!(state.finish().is_empty(), "missed frames are reported once");
  }

  #[test]
  fn write_failure_marks_scheduled_failed() {
    let dir = tempfile::tempdir().unwrap();
    let blocker = dir.path().join("not-a-dir");
    std::fs::write(&blocker, b"x").unwrap();
    let mut state = CaptureState::new(config(&[1], &blocker, true));
    let path = state.path_for_next_frame().unwrap();

    state.on_presented(captured(path));
    let reports = state.finish();

    assert!(matches!(
      reports.as_slice(),
      [CaptureReport::Failed { frame: 1, .. }]
    ));
    assert!(state.exit_with_failure());
  }
}
