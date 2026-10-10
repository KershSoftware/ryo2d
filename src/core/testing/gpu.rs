//! One GPU device for every headless engine in the process.
//!
//! Creating devices concurrently deadlocks on some platforms (seen on Windows),
//! so tests running in parallel share this one.

use std::sync::{Arc, OnceLock};

use crate::core::error::HeadlessError;

type Gpu = (Arc<wgpu::Device>, Arc<wgpu::Queue>);

static SHARED: OnceLock<Option<Gpu>> = OnceLock::new();

/// The process-wide headless device and queue, created on first use.
///
/// Returns [`HeadlessError::NoGpu`] when the machine has no usable adapter.
pub fn shared_gpu() -> Result<Gpu, HeadlessError> {
  SHARED
    .get_or_init(|| pollster::block_on(create()))
    .as_ref()
    .map(|(device, queue)| (Arc::clone(device), Arc::clone(queue)))
    .ok_or(HeadlessError::NoGpu)
}

async fn create() -> Option<Gpu> {
  let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
    backends: wgpu::Backends::all(),
    ..Default::default()
  });
  let adapter = instance
    .request_adapter(&wgpu::RequestAdapterOptions {
      power_preference: wgpu::PowerPreference::default(),
      compatible_surface: None,
      force_fallback_adapter: false,
    })
    .await?;
  let (device, queue) = adapter
    .request_device(
      &wgpu::DeviceDescriptor {
        label: Some("Ryo2D Headless Device"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        memory_hints: Default::default(),
      },
      None,
    )
    .await
    .ok()?;
  Some((Arc::new(device), Arc::new(queue)))
}
