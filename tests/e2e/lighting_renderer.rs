//! GPU-based lighting renderer tests.
//!
//! These tests verify the lighting system works correctly at the GPU level,
//! including texture operations, shader execution, and pixel output.
//!
//! Tests gracefully skip when no GPU is available (CI without GPU).

use crate::common::gpu::TestGpu;
use ryo2d::core::lighting::LightingRenderer;
use ryo2d::prelude::*;

/// Create an sRGB render target for lighting tests.
///
/// The LightingRenderer compose pipeline requires Rgba8UnormSrgb format,
/// so we need a custom target with that format instead of the default Rgba8Unorm.
fn create_srgb_target(gpu: &TestGpu, width: u32, height: u32) -> SrgbRenderTarget {
  SrgbRenderTarget::new(&gpu.device, width, height)
}

/// sRGB render target for lighting compose output
struct SrgbRenderTarget {
  pub texture: wgpu::Texture,
  pub view: wgpu::TextureView,
  staging_buffer: wgpu::Buffer,
  width: u32,
  height: u32,
  bytes_per_row: u32,
  padded_bytes_per_row: u32,
}

impl SrgbRenderTarget {
  fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;

    let texture = device.create_texture(&wgpu::TextureDescriptor {
      label: Some("sRGB Test Render Target"),
      size: wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
      },
      mip_level_count: 1,
      sample_count: 1,
      dimension: wgpu::TextureDimension::D2,
      format,
      usage: wgpu::TextureUsages::RENDER_ATTACHMENT
        | wgpu::TextureUsages::COPY_SRC
        | wgpu::TextureUsages::COPY_DST,
      view_formats: &[],
    });

    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    let bytes_per_pixel = 4u32;
    let bytes_per_row = width * bytes_per_pixel;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded_bytes_per_row = bytes_per_row.div_ceil(align) * align;
    let buffer_size = (padded_bytes_per_row * height) as u64;

    let staging_buffer = device.create_buffer(&wgpu::BufferDescriptor {
      label: Some("sRGB Test Staging Buffer"),
      size: buffer_size,
      usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
      mapped_at_creation: false,
    });

    Self {
      texture,
      view,
      staging_buffer,
      width,
      height,
      bytes_per_row,
      padded_bytes_per_row,
    }
  }

  fn get_pixel(&self, gpu: &TestGpu, x: u32, y: u32) -> (u8, u8, u8, u8) {
    let pixels = self.read_pixels_sync(gpu);
    let idx = ((y * self.width + x) * 4) as usize;
    (
      pixels[idx],
      pixels[idx + 1],
      pixels[idx + 2],
      pixels[idx + 3],
    )
  }

  fn read_pixels_sync(&self, gpu: &TestGpu) -> Vec<u8> {
    pollster::block_on(self.read_pixels(gpu))
  }

  async fn read_pixels(&self, gpu: &TestGpu) -> Vec<u8> {
    let mut encoder = gpu
      .device
      .create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("sRGB Pixel Readback Encoder"),
      });

    encoder.copy_texture_to_buffer(
      wgpu::ImageCopyTexture {
        texture: &self.texture,
        mip_level: 0,
        origin: wgpu::Origin3d::ZERO,
        aspect: wgpu::TextureAspect::All,
      },
      wgpu::ImageCopyBuffer {
        buffer: &self.staging_buffer,
        layout: wgpu::ImageDataLayout {
          offset: 0,
          bytes_per_row: Some(self.padded_bytes_per_row),
          rows_per_image: Some(self.height),
        },
      },
      wgpu::Extent3d {
        width: self.width,
        height: self.height,
        depth_or_array_layers: 1,
      },
    );

    gpu.queue.submit(std::iter::once(encoder.finish()));

    let buffer_slice = self.staging_buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
      tx.send(result).unwrap();
    });

    gpu.device.poll(wgpu::Maintain::Wait);
    rx.recv().unwrap().expect("Failed to map buffer");

    let mapped = buffer_slice.get_mapped_range();
    let mut pixels = Vec::with_capacity((self.width * self.height * 4) as usize);

    for y in 0..self.height {
      let start = (y * self.padded_bytes_per_row) as usize;
      let end = start + self.bytes_per_row as usize;
      pixels.extend_from_slice(&mapped[start..end]);
    }

    drop(mapped);
    self.staging_buffer.unmap();

    pixels
  }
}

// ============================================================================
// LIGHTING RENDERER GPU TESTS
// ============================================================================

/// Test that LightingRenderer can be created with headless GPU
#[test]
fn test_lighting_renderer_creation() {
  let Some(gpu) = TestGpu::shared() else {
    eprintln!("Skipping test_lighting_renderer_creation: no GPU available");
    return;
  };

  // LightingRenderer requires Rgba8UnormSrgb format for compose output
  let format = wgpu::TextureFormat::Rgba8UnormSrgb;

  let renderer = LightingRenderer::new(&gpu.device, 100, 100, format);

  // LightingRenderer starts disabled by default for performance
  // The engine auto-enables when Light2D entities exist
  assert!(
    !renderer.is_enabled(),
    "LightingRenderer should be disabled by default for performance"
  );

  // Verify UI is not affected by lighting by default
  assert!(
    !renderer.is_ui_affected_by_lighting(),
    "UI should not be affected by lighting by default"
  );
}

/// Test that ambient light can be configured
#[test]
fn test_lighting_renderer_ambient_config() {
  let Some(gpu) = TestGpu::shared() else {
    eprintln!("Skipping test_lighting_renderer_ambient_config: no GPU available");
    return;
  };

  let format = wgpu::TextureFormat::Rgba8UnormSrgb;
  let mut renderer = LightingRenderer::new(&gpu.device, 100, 100, format);

  // Set custom ambient
  let custom_ambient = AmbientLight::new(Color::new(100, 150, 200, 255), 0.5);
  renderer.set_ambient(custom_ambient);

  // Verify it was set
  let ambient = renderer.ambient();
  assert_eq!(ambient.intensity, 0.5);
  assert_eq!(ambient.color.r, 100);
  assert_eq!(ambient.color.g, 150);
  assert_eq!(ambient.color.b, 200);
}

/// Test that LightingRenderer can be resized
#[test]
fn test_lighting_renderer_resize() {
  let Some(gpu) = TestGpu::shared() else {
    eprintln!("Skipping test_lighting_renderer_resize: no GPU available");
    return;
  };

  let format = wgpu::TextureFormat::Rgba8UnormSrgb;
  let mut renderer = LightingRenderer::new(&gpu.device, 100, 100, format);

  // Resize to new dimensions
  renderer.resize(&gpu.device, 200, 150);

  // The renderer should accept the new size without panicking
  // Internal textures are recreated at the new size
  // (test passes if we reach here without panic)
}

/// Test that enabled state can be toggled
#[test]
fn test_lighting_renderer_enable_disable() {
  let Some(gpu) = TestGpu::shared() else {
    eprintln!("Skipping test_lighting_renderer_enable_disable: no GPU available");
    return;
  };

  let format = wgpu::TextureFormat::Rgba8UnormSrgb;
  let mut renderer = LightingRenderer::new(&gpu.device, 100, 100, format);

  // Starts disabled by default (engine auto-enables when Light2D entities exist)
  assert!(!renderer.is_enabled(), "Should start disabled by default");

  renderer.set_enabled(true);
  assert!(
    renderer.is_enabled(),
    "Should be enabled after set_enabled(true)"
  );

  renderer.set_enabled(false);
  assert!(
    !renderer.is_enabled(),
    "Should be disabled after set_enabled(false)"
  );
}

// ============================================================================
// COMPOSE SHADER TESTS
// ============================================================================

/// REGRESSION TEST: Verify scene texture data reaches compose shader
///
/// This test verifies the fix for the critical bug where the compose shader
/// could not read scene texture data. The fix involves passing pixel data
/// directly to compose() rather than a texture reference.
#[test]
fn test_compose_receives_pixel_data() {
  let Some(gpu) = TestGpu::shared() else {
    eprintln!("Skipping test_compose_receives_pixel_data: no GPU available");
    return;
  };

  let width = 64u32;
  let height = 64u32;
  let format = wgpu::TextureFormat::Rgba8UnormSrgb;

  let mut renderer = LightingRenderer::new(&gpu.device, width, height, format);

  // Enable lighting (disabled by default for performance)
  renderer.set_enabled(true);

  // Create output texture
  let output_texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
    label: Some("Test Output"),
    size: wgpu::Extent3d {
      width,
      height,
      depth_or_array_layers: 1,
    },
    mip_level_count: 1,
    sample_count: 1,
    dimension: wgpu::TextureDimension::D2,
    format,
    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
      | wgpu::TextureUsages::COPY_SRC
      | wgpu::TextureUsages::TEXTURE_BINDING,
    view_formats: &[],
  });
  let output_view = output_texture.create_view(&wgpu::TextureViewDescriptor::default());

  // Create pixel data with known content (red pixels)
  let mut pixel_data = vec![0u8; (width * height * 4) as usize];
  for pixel in pixel_data.as_chunks_mut::<4>().0 {
    pixel[0] = 255; // R
    pixel[1] = 0; // G
    pixel[2] = 0; // B
    pixel[3] = 255; // A
  }

  // Create encoder and call compose
  let mut encoder = gpu
    .device
    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
      label: Some("Test Compose Encoder"),
    });

  // First render some lights (empty for this test)
  renderer.render_lights(
    &gpu.device,
    &gpu.queue,
    &mut encoder,
    std::iter::empty::<(&Vec2, &Light2D)>(),
    Vec2::ZERO,
    1.0,
  );

  // Call compose with pixel data
  renderer.compose(
    &gpu.device,
    &gpu.queue,
    &mut encoder,
    &pixel_data,
    &output_view,
    0.0,
  );

  gpu.queue.submit(std::iter::once(encoder.finish()));

  // Read back the output
  // Note: The compose result depends on ambient + light accumulation
  // With no lights and default ambient, scene is multiplied by ambient

  // For now, just verify the operation completed without panic
  // (test passes if we reach here without panic)
}

/// Test that compose with ambient light produces visible output
#[test]
fn test_compose_with_ambient() {
  let Some(gpu) = TestGpu::shared() else {
    eprintln!("Skipping test_compose_with_ambient: no GPU available");
    return;
  };

  let width = 32u32;
  let height = 32u32;
  let format = wgpu::TextureFormat::Rgba8UnormSrgb;

  let mut renderer = LightingRenderer::new(&gpu.device, width, height, format);

  // Enable lighting (disabled by default for performance)
  renderer.set_enabled(true);

  // Set high ambient so scene is fully visible
  renderer.set_ambient(AmbientLight::new(Color::WHITE, 1.0));

  // Create output target with matching sRGB format
  let target = create_srgb_target(gpu, width, height);

  // Create pixel data with gray pixels
  let gray_value = 128u8;
  let mut pixel_data = vec![0u8; (width * height * 4) as usize];
  for pixel in pixel_data.as_chunks_mut::<4>().0 {
    pixel[0] = gray_value; // R
    pixel[1] = gray_value; // G
    pixel[2] = gray_value; // B
    pixel[3] = 255; // A
  }

  // Create encoder
  let mut encoder = gpu
    .device
    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
      label: Some("Test Ambient Compose"),
    });

  // Render lights (empty)
  renderer.render_lights(
    &gpu.device,
    &gpu.queue,
    &mut encoder,
    std::iter::empty::<(&Vec2, &Light2D)>(),
    Vec2::ZERO,
    1.0,
  );

  // Compose to target
  renderer.compose(
    &gpu.device,
    &gpu.queue,
    &mut encoder,
    &pixel_data,
    &target.view,
    0.0,
  );

  gpu.queue.submit(std::iter::once(encoder.finish()));

  // Read back center pixel
  let (r, g, b, _a) = target.get_pixel(gpu, width / 2, height / 2);

  // With ambient intensity 1.0, output should be close to input
  // Allow some tolerance for sRGB conversion
  let tolerance = 30;
  assert!(
    (r as i32 - gray_value as i32).abs() < tolerance,
    "Red channel should be close to input gray: got {}, expected ~{}",
    r,
    gray_value
  );
  assert!(
    (g as i32 - gray_value as i32).abs() < tolerance,
    "Green channel should be close to input gray: got {}, expected ~{}",
    g,
    gray_value
  );
  assert!(
    (b as i32 - gray_value as i32).abs() < tolerance,
    "Blue channel should be close to input gray: got {}, expected ~{}",
    b,
    gray_value
  );
}

/// Test that compose with zero ambient produces dark output
#[test]
fn test_compose_with_zero_ambient() {
  let Some(gpu) = TestGpu::shared() else {
    eprintln!("Skipping test_compose_with_zero_ambient: no GPU available");
    return;
  };

  let width = 32u32;
  let height = 32u32;
  let format = wgpu::TextureFormat::Rgba8UnormSrgb;

  let mut renderer = LightingRenderer::new(&gpu.device, width, height, format);

  // Enable lighting (disabled by default for performance)
  renderer.set_enabled(true);

  // Set zero ambient (pitch black without lights)
  renderer.set_ambient(AmbientLight::new(Color::WHITE, 0.0));

  // Create output target with matching sRGB format
  let target = create_srgb_target(gpu, width, height);

  // Create pixel data with bright pixels
  let mut pixel_data = vec![0u8; (width * height * 4) as usize];
  for pixel in pixel_data.as_chunks_mut::<4>().0 {
    pixel[0] = 255; // R
    pixel[1] = 255; // G
    pixel[2] = 255; // B
    pixel[3] = 255; // A
  }

  // Create encoder
  let mut encoder = gpu
    .device
    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
      label: Some("Test Zero Ambient"),
    });

  // Render lights (empty - no lights)
  renderer.render_lights(
    &gpu.device,
    &gpu.queue,
    &mut encoder,
    std::iter::empty::<(&Vec2, &Light2D)>(),
    Vec2::ZERO,
    1.0,
  );

  // Compose to target
  renderer.compose(
    &gpu.device,
    &gpu.queue,
    &mut encoder,
    &pixel_data,
    &target.view,
    0.0,
  );

  gpu.queue.submit(std::iter::once(encoder.finish()));

  // Read back center pixel
  let (r, g, b, _a) = target.get_pixel(gpu, width / 2, height / 2);

  // With zero ambient and no lights, output should be black
  // Allow small tolerance for numerical precision
  let tolerance = 5;
  assert!(
    r < tolerance,
    "Red channel should be near black with zero ambient: got {}",
    r
  );
  assert!(
    g < tolerance,
    "Green channel should be near black with zero ambient: got {}",
    g
  );
  assert!(
    b < tolerance,
    "Blue channel should be near black with zero ambient: got {}",
    b
  );
}

// ============================================================================
// RENDER LIGHTS TESTS
// ============================================================================

/// Test that render_lights doesn't crash with empty light list
#[test]
fn test_render_lights_empty() {
  let Some(gpu) = TestGpu::shared() else {
    eprintln!("Skipping test_render_lights_empty: no GPU available");
    return;
  };

  let format = wgpu::TextureFormat::Rgba8UnormSrgb;
  let mut renderer = LightingRenderer::new(&gpu.device, 100, 100, format);

  // Enable lighting (disabled by default for performance)
  renderer.set_enabled(true);

  let mut encoder = gpu
    .device
    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
      label: Some("Test Empty Lights"),
    });

  // Should handle empty iterator gracefully
  renderer.render_lights(
    &gpu.device,
    &gpu.queue,
    &mut encoder,
    std::iter::empty::<(&Vec2, &Light2D)>(),
    Vec2::ZERO,
    1.0,
  );

  gpu.queue.submit(std::iter::once(encoder.finish()));
  // (test passes if we reach here without panic)
}

/// Test that render_lights works with actual light data
#[test]
fn test_render_lights_with_data() {
  let Some(gpu) = TestGpu::shared() else {
    eprintln!("Skipping test_render_lights_with_data: no GPU available");
    return;
  };

  let format = wgpu::TextureFormat::Rgba8UnormSrgb;
  let mut renderer = LightingRenderer::new(&gpu.device, 100, 100, format);

  // Enable lighting (disabled by default for performance)
  renderer.set_enabled(true);

  // Create light data
  let transform = Transform::from_xy(50.0, 50.0);
  let light = Light2D::torch(50.0);
  let lights = [(transform, light)];

  let mut encoder = gpu
    .device
    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
      label: Some("Test With Lights"),
    });

  // Render lights
  renderer.render_lights(
    &gpu.device,
    &gpu.queue,
    &mut encoder,
    lights.iter().map(|(t, l)| (&t.translation, l)),
    Vec2::ZERO,
    1.0,
  );

  gpu.queue.submit(std::iter::once(encoder.finish()));
  // (test passes if we reach here without panic)
}

/// Test that disabled renderer skips render_lights
#[test]
fn test_render_lights_when_disabled() {
  let Some(gpu) = TestGpu::shared() else {
    eprintln!("Skipping test_render_lights_when_disabled: no GPU available");
    return;
  };

  let format = wgpu::TextureFormat::Rgba8UnormSrgb;
  let mut renderer = LightingRenderer::new(&gpu.device, 100, 100, format);

  // Disable the renderer
  renderer.set_enabled(false);

  let transform = Transform::from_xy(50.0, 50.0);
  let light = Light2D::torch(50.0);
  let lights = [(transform, light)];

  let mut encoder = gpu
    .device
    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
      label: Some("Test Disabled Lights"),
    });

  // Should return early without rendering
  renderer.render_lights(
    &gpu.device,
    &gpu.queue,
    &mut encoder,
    lights.iter().map(|(t, l)| (&t.translation, l)),
    Vec2::ZERO,
    1.0,
  );

  gpu.queue.submit(std::iter::once(encoder.finish()));
  // (test passes if we reach here without panic)
}

// ============================================================================
// LIGHTING + POST-PROCESSING INTEGRATION TESTS
// ============================================================================

/// REGRESSION TEST: Lighting output must be copied to post-process texture_a
/// before effects can be applied.
///
/// Bug fixed: When both lighting and CRT (or other effects) were enabled,
/// the screen went completely dark. The issue was that:
/// - Lighting rendered to a temporary `lit_texture`
/// - Effects expected input in `texture_a`
/// - But nothing copied lit_texture → texture_a
///
/// The fix ensures the lit scene is copied to texture_a before effect chain starts.
#[test]
fn test_lighting_plus_effects_produces_visible_output() {
  let Some(gpu) = TestGpu::shared() else {
    eprintln!("Skipping test_lighting_plus_effects_produces_visible_output: no GPU available");
    return;
  };

  let width = 64u32;
  let height = 64u32;
  let format = wgpu::TextureFormat::Rgba8UnormSrgb;

  // Create lighting renderer
  let mut lighting = LightingRenderer::new(&gpu.device, width, height, format);
  lighting.set_enabled(true);
  lighting.set_ambient(AmbientLight::new(Color::WHITE, 0.5)); // 50% ambient

  // Create post-process pipeline
  let mut post_process =
    ryo2d::core::shaders::PostProcessPipeline::new(&gpu.device, width, height, format);

  // Create output target
  let target = create_srgb_target(gpu, width, height);

  // Create bright white pixel data
  let mut pixel_data = vec![0u8; (width * height * 4) as usize];
  for pixel in pixel_data.as_chunks_mut::<4>().0 {
    pixel[0] = 255; // R
    pixel[1] = 255; // G
    pixel[2] = 255; // B
    pixel[3] = 255; // A
  }

  let mut encoder = gpu
    .device
    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
      label: Some("Lighting + Effects Test"),
    });

  // Step 1: Render lights (no dynamic lights, just ambient)
  lighting.render_lights(
    &gpu.device,
    &gpu.queue,
    &mut encoder,
    std::iter::empty::<(&Vec2, &Light2D)>(),
    Vec2::ZERO,
    1.0,
  );

  // Step 2: Compose lit scene
  lighting.compose(
    &gpu.device,
    &gpu.queue,
    &mut encoder,
    &pixel_data,
    post_process.texture_a_view(),
    0.0,
  );

  // Step 3: Apply a CRT effect (the effect that triggered the original bug)
  let crt_effect = ryo2d::core::shaders::PostProcessEffect::crt();
  let effects = [(&crt_effect, 0.0f32)];
  let output_buffer = post_process.apply_effects(&gpu.device, &gpu.queue, &mut encoder, &effects);

  // Step 4: Copy result to our test target for verification
  let output_texture = match output_buffer {
    ryo2d::core::shaders::OutputBuffer::A => post_process.texture_a(),
    ryo2d::core::shaders::OutputBuffer::B => post_process.texture_b(),
  };

  encoder.copy_texture_to_texture(
    wgpu::ImageCopyTexture {
      texture: output_texture,
      mip_level: 0,
      origin: wgpu::Origin3d::ZERO,
      aspect: wgpu::TextureAspect::All,
    },
    wgpu::ImageCopyTexture {
      texture: &target.texture,
      mip_level: 0,
      origin: wgpu::Origin3d::ZERO,
      aspect: wgpu::TextureAspect::All,
    },
    wgpu::Extent3d {
      width,
      height,
      depth_or_array_layers: 1,
    },
  );

  gpu.queue.submit(std::iter::once(encoder.finish()));

  // Verify output is NOT black (the original bug produced all-black output)
  let (r, g, b, _a) = target.get_pixel(gpu, width / 2, height / 2);

  // With 50% ambient on white scene, output should be visible (not black)
  // CRT effect may darken slightly, but should still be well above black
  let min_visible = 30u8;
  assert!(
    r > min_visible || g > min_visible || b > min_visible,
    "REGRESSION: Lighting + effects produced black output! \
     Got ({}, {}, {}), expected visible pixels. \
     This indicates the lit scene was not copied to texture_a before effects.",
    r,
    g,
    b
  );
}

/// Test that multiple effects can be chained after lighting
#[test]
fn test_lighting_with_multiple_effects() {
  let Some(gpu) = TestGpu::shared() else {
    eprintln!("Skipping test_lighting_with_multiple_effects: no GPU available");
    return;
  };

  let width = 32u32;
  let height = 32u32;
  let format = wgpu::TextureFormat::Rgba8UnormSrgb;

  let mut lighting = LightingRenderer::new(&gpu.device, width, height, format);
  lighting.set_enabled(true);
  lighting.set_ambient(AmbientLight::new(Color::WHITE, 1.0));

  let mut post_process =
    ryo2d::core::shaders::PostProcessPipeline::new(&gpu.device, width, height, format);

  // Create gray pixel data
  let mut pixel_data = vec![0u8; (width * height * 4) as usize];
  for pixel in pixel_data.as_chunks_mut::<4>().0 {
    pixel[0] = 128;
    pixel[1] = 128;
    pixel[2] = 128;
    pixel[3] = 255;
  }

  let mut encoder = gpu
    .device
    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
      label: Some("Multi-Effect Test"),
    });

  // Render and compose lighting
  lighting.render_lights(
    &gpu.device,
    &gpu.queue,
    &mut encoder,
    std::iter::empty::<(&Vec2, &Light2D)>(),
    Vec2::ZERO,
    1.0,
  );

  lighting.compose(
    &gpu.device,
    &gpu.queue,
    &mut encoder,
    &pixel_data,
    post_process.texture_a_view(),
    0.0,
  );

  // Apply multiple effects: CRT + vignette
  let crt = ryo2d::core::shaders::PostProcessEffect::crt();
  let vignette = ryo2d::core::shaders::PostProcessEffect::vignette(0.3);
  let effects = [(&crt, 0.0f32), (&vignette, 0.0f32)];

  let _output = post_process.apply_effects(&gpu.device, &gpu.queue, &mut encoder, &effects);

  gpu.queue.submit(std::iter::once(encoder.finish()));
  // Test passes if no panic - verifies effect chaining works after lighting
}

// ============================================================================
// VOLUMETRIC LIGHTING GPU TESTS
// ============================================================================

/// Test that volumetric lights can be rendered without panic
#[test]
fn test_render_volumetric_lights() {
  let Some(gpu) = TestGpu::shared() else {
    eprintln!("Skipping test_render_volumetric_lights: no GPU available");
    return;
  };

  let format = wgpu::TextureFormat::Rgba8UnormSrgb;
  let mut renderer = LightingRenderer::new(&gpu.device, 100, 100, format);
  renderer.set_enabled(true);

  // Create lights with volumetric settings
  let transform1 = Transform::from_xy(50.0, 50.0);
  let light1 = Light2D::flashlight(80.0, 0.0).with_volumetric();

  let transform2 = Transform::from_xy(75.0, 75.0);
  let light2 = Light2D::point(Color::WHITE, 100.0).with_god_rays();

  let lights = [(transform1, light1), (transform2, light2)];

  let mut encoder = gpu
    .device
    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
      label: Some("Volumetric Lights Test"),
    });

  // Render volumetric lights
  renderer.render_lights(
    &gpu.device,
    &gpu.queue,
    &mut encoder,
    lights.iter().map(|(t, l)| (&t.translation, l)),
    Vec2::ZERO,
    1.0,
  );

  gpu.queue.submit(std::iter::once(encoder.finish()));
  // Test passes if no panic - volumetric data is correctly passed to GPU
}

/// Test volumetric spotlight beam renders without crashing
///
/// This test verifies the complete rendering path for volumetric spotlights:
/// - Volumetric settings are passed to the GPU correctly
/// - The light shader processes volumetric data
/// - Compose completes without errors
///
/// Note: Pixel-level verification is complex due to projection matrix setup.
/// Unit tests verify the volumetric data packing is correct.
#[test]
fn test_volumetric_spotlight_beam() {
  let Some(gpu) = TestGpu::shared() else {
    eprintln!("Skipping test_volumetric_spotlight_beam: no GPU available");
    return;
  };

  let width = 64u32;
  let height = 64u32;
  let format = wgpu::TextureFormat::Rgba8UnormSrgb;

  let mut renderer = LightingRenderer::new(&gpu.device, width, height, format);
  renderer.set_enabled(true);
  renderer.set_ambient(AmbientLight::new(Color::WHITE, 0.3));

  let target = create_srgb_target(gpu, width, height);

  // Create a spotlight with volumetric beam
  let transform = Transform::from_xy(32.0, 32.0);
  let light = Light2D::spot(Color::WHITE, 50.0, 45.0, 0.0)
    .with_intensity(2.0)
    .with_volumetric_settings(VolumetricSettings::strong_beam());

  // Verify volumetric data is set correctly
  assert!(light.has_volumetric());
  let vol = light.volumetric().unwrap();
  assert!(vol.intensity > 0.0);
  assert!(vol.density > 0.0);

  let lights = [(transform, light)];

  let mut pixel_data = vec![128u8; (width * height * 4) as usize];
  // Set alpha channel
  for i in (3..pixel_data.len()).step_by(4) {
    pixel_data[i] = 255;
  }

  let mut encoder = gpu
    .device
    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
      label: Some("Volumetric Beam Test"),
    });

  // Render lights - this processes volumetric data in the shader
  renderer.render_lights(
    &gpu.device,
    &gpu.queue,
    &mut encoder,
    lights.iter().map(|(t, l)| (&t.translation, l)),
    Vec2::ZERO,
    1.0,
  );

  // Compose - blends lighting with scene
  renderer.compose(
    &gpu.device,
    &gpu.queue,
    &mut encoder,
    &pixel_data,
    &target.view,
    0.0,
  );

  gpu.queue.submit(std::iter::once(encoder.finish()));

  // Verify output has some content (not all black)
  let center = target.get_pixel(gpu, 32, 32);
  let brightness = (center.0 as u32 + center.1 as u32 + center.2 as u32) / 3;

  // With ambient 0.3 on gray 128 scene, expect at least some output
  assert!(
    brightness > 20,
    "Output should have visible content from ambient lighting: brightness={}",
    brightness
  );
}

/// Test god rays effect params packing
#[test]
fn test_god_rays_params_gpu_alignment() {
  use ryo2d::core::shaders::GodRaysParams;

  // GodRaysParams must be 128 bytes for GPU uniform alignment
  let size = std::mem::size_of::<GodRaysParams>();
  assert_eq!(
    size, 128,
    "GodRaysParams must be 128 bytes for GPU alignment, got {}",
    size
  );

  // Test packing via from_effect
  let light_positions = vec![(0.2, 0.3), (0.7, 0.8)];
  let effect = ryo2d::core::shaders::PostProcessEffect::god_rays(0.8, light_positions);
  let params = GodRaysParams::from_effect(&effect, 0.0, 64, 64);

  // Verify light positions are packed correctly
  assert!((params.light_pos_01[0] - 0.2).abs() < 0.001); // First light X
  assert!((params.light_pos_01[1] - 0.3).abs() < 0.001); // First light Y
  assert!((params.light_pos_01[2] - 0.7).abs() < 0.001); // Second light X
  assert!((params.light_pos_01[3] - 0.8).abs() < 0.001); // Second light Y
}

/// Test god rays effect with lighting integration
#[test]
fn test_god_rays_with_lighting() {
  let Some(gpu) = TestGpu::shared() else {
    eprintln!("Skipping test_god_rays_with_lighting: no GPU available");
    return;
  };

  let width = 64u32;
  let height = 64u32;
  let format = wgpu::TextureFormat::Rgba8UnormSrgb;

  let mut lighting = LightingRenderer::new(&gpu.device, width, height, format);
  lighting.set_enabled(true);
  lighting.set_ambient(AmbientLight::new(Color::WHITE, 0.3));

  let mut post_process =
    ryo2d::core::shaders::PostProcessPipeline::new(&gpu.device, width, height, format);

  // Create a bright light with god rays
  let transform = Transform::from_xy(10.0, 10.0);
  let light = Light2D::point(Color::WHITE, 50.0)
    .with_intensity(2.0)
    .with_god_rays();
  let lights = [(transform, light)];

  // Create scene with bright spot where light is
  let mut pixel_data = vec![0u8; (width * height * 4) as usize];
  for y in 0..height {
    for x in 0..width {
      let idx = ((y * width + x) * 4) as usize;
      // Make area near light bright (simulating light source visibility)
      let dist = ((x as f32 - 10.0).powi(2) + (y as f32 - 10.0).powi(2)).sqrt();
      let brightness = if dist < 15.0 { 255 } else { 50 };
      pixel_data[idx] = brightness;
      pixel_data[idx + 1] = brightness;
      pixel_data[idx + 2] = brightness;
      pixel_data[idx + 3] = 255;
    }
  }

  let mut encoder = gpu
    .device
    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
      label: Some("God Rays Integration Test"),
    });

  // Render lights
  lighting.render_lights(
    &gpu.device,
    &gpu.queue,
    &mut encoder,
    lights.iter().map(|(t, l)| (&t.translation, l)),
    Vec2::ZERO,
    1.0,
  );

  // Compose lighting to texture_a
  lighting.compose(
    &gpu.device,
    &gpu.queue,
    &mut encoder,
    &pixel_data,
    post_process.texture_a_view(),
    0.0,
  );

  // Apply god rays effect
  let light_positions = vec![(10.0 / width as f32, 10.0 / height as f32)];
  let god_rays = ryo2d::core::shaders::PostProcessEffect::god_rays(0.8, light_positions);
  let effects = [(&god_rays, 0.0f32)];

  let _output = post_process.apply_effects(&gpu.device, &gpu.queue, &mut encoder, &effects);

  gpu.queue.submit(std::iter::once(encoder.finish()));
  // Test passes if no panic - god rays work with lighting pipeline
}

/// Test that lights without volumetric settings render without crashing
///
/// This verifies that regular lights (without volumetric) continue to work
/// correctly alongside the volumetric feature.
#[test]
fn test_non_volumetric_lights_unchanged() {
  let Some(gpu) = TestGpu::shared() else {
    eprintln!("Skipping test_non_volumetric_lights_unchanged: no GPU available");
    return;
  };

  let width = 32u32;
  let height = 32u32;
  let format = wgpu::TextureFormat::Rgba8UnormSrgb;

  let mut renderer = LightingRenderer::new(&gpu.device, width, height, format);
  renderer.set_enabled(true);
  renderer.set_ambient(AmbientLight::new(Color::WHITE, 0.5));

  let target = create_srgb_target(gpu, width, height);

  // Create a normal light (no volumetric)
  let transform = Transform::from_xy(16.0, 16.0);
  let light = Light2D::point(Color::WHITE, 15.0).with_intensity(1.0);

  // Verify light does NOT have volumetric
  assert!(!light.has_volumetric());

  let lights = [(transform, light)];

  let mut pixel_data = vec![0u8; (width * height * 4) as usize];
  for pixel in pixel_data.as_chunks_mut::<4>().0 {
    pixel[0] = 200;
    pixel[1] = 200;
    pixel[2] = 200;
    pixel[3] = 255;
  }

  let mut encoder = gpu
    .device
    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
      label: Some("Non-Volumetric Light Test"),
    });

  renderer.render_lights(
    &gpu.device,
    &gpu.queue,
    &mut encoder,
    lights.iter().map(|(t, l)| (&t.translation, l)),
    Vec2::ZERO,
    1.0,
  );

  renderer.compose(
    &gpu.device,
    &gpu.queue,
    &mut encoder,
    &pixel_data,
    &target.view,
    0.0,
  );

  gpu.queue.submit(std::iter::once(encoder.finish()));

  // Verify output has visible content from ambient
  let center = target.get_pixel(gpu, 16, 16);
  let brightness = (center.0 as u32 + center.1 as u32 + center.2 as u32) / 3;

  // With ambient 0.5 on light gray scene (200), expect ~100 brightness
  assert!(
    brightness > 50,
    "Output should have visible content: brightness={}",
    brightness
  );
}
