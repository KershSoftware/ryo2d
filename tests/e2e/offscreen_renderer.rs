//! E2E tests for the offscreen WgpuRenderer target.

use std::sync::Arc;

use crate::common::gpu::TestGpu;
use ryo2d::core::graphics::WgpuRenderer;

fn within_one(a: u8, b: u8) -> bool {
  a.abs_diff(b) <= 1
}

#[test]
fn offscreen_frame_matches_cpu_pixels() {
  let Some(gpu) = TestGpu::shared() else {
    eprintln!("Skipping offscreen_frame_matches_cpu_pixels: no GPU available");
    return;
  };
  let (width, height) = (37, 9);
  let mut renderer = WgpuRenderer::new_offscreen(
    Arc::clone(&gpu.device),
    Arc::clone(&gpu.queue),
    width,
    height,
  );

  // Opaque pattern: each pixel encodes its position
  let pixels = renderer.frame_mut();
  for (i, px) in pixels.chunks_exact_mut(4).enumerate() {
    let (x, y) = ((i as u32 % width) as u8, (i as u32 / width) as u8);
    px.copy_from_slice(&[x * 6, y * 20, 128, 255]);
  }
  let expected = renderer.frame_mut().to_vec();

  renderer.render().unwrap();
  let frame = renderer.read_frame().unwrap();

  assert_eq!(frame.dimensions(), (width, height));
  for (got, want) in frame.as_raw().chunks_exact(4).zip(expected.chunks_exact(4)) {
    assert!(
      got.iter().zip(want).all(|(g, w)| within_one(*g, *w)),
      "got {got:?}, want {want:?}"
    );
  }
}
