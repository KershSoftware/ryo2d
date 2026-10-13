//! Every bind group is made here, so tests can count them, and the ones
//! a pass needs every frame are made once and reused ([`BindGroupCache`]).

use std::collections::HashMap;
use std::sync::Mutex;

#[cfg(test)]
thread_local! {
  static CREATED: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Create a bind group (counted per thread in tests).
pub(crate) fn create(
  device: &wgpu::Device,
  desc: &wgpu::BindGroupDescriptor<'_>,
) -> wgpu::BindGroup {
  #[cfg(test)]
  CREATED.with(|created| created.set(created.get() + 1));
  device.create_bind_group(desc)
}

/// One resource a bind group binds.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum Bound {
  View(wgpu::TextureView),
  Sampler(wgpu::Sampler),
  Buffer(wgpu::Buffer),
}

/// The most bindings a cached bind group has.
const MAX_BINDINGS: usize = 4;

/// More entries than any pass needs means stale views piled up: start over.
const MAX_CACHED: usize = 64;

type Key = (wgpu::BindGroupLayout, [Option<Bound>; MAX_BINDINGS]);

/// Bind groups made once per layout and set of resources, then reused.
///
/// wgpu's handles compare by identity, so a recreated texture view (a
/// resize) is a new key; owners call [`clear`](Self::clear) when they
/// recreate the resources they bind, so old textures are not kept alive.
#[derive(Default)]
pub(crate) struct BindGroupCache {
  groups: Mutex<HashMap<Key, wgpu::BindGroup>>,
}

impl BindGroupCache {
  /// The bind group binding `resources[i]` at binding `i` with `layout`,
  /// made on first use.
  pub(crate) fn get(
    &self,
    device: &wgpu::Device,
    label: &str,
    layout: &wgpu::BindGroupLayout,
    resources: &[Bound],
  ) -> wgpu::BindGroup {
    let mut bound: [Option<Bound>; MAX_BINDINGS] = Default::default();
    for (slot, resource) in bound.iter_mut().zip(resources) {
      *slot = Some(resource.clone());
    }
    let key = (layout.clone(), bound);
    let mut groups = self
      .groups
      .lock()
      .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(group) = groups.get(&key) {
      return group.clone();
    }
    if groups.len() >= MAX_CACHED {
      groups.clear();
    }
    let entries: Vec<wgpu::BindGroupEntry<'_>> = resources
      .iter()
      .enumerate()
      .map(|(binding, resource)| wgpu::BindGroupEntry {
        binding: binding as u32,
        resource: match resource {
          Bound::View(view) => wgpu::BindingResource::TextureView(view),
          Bound::Sampler(sampler) => wgpu::BindingResource::Sampler(sampler),
          Bound::Buffer(buffer) => buffer.as_entire_binding(),
        },
      })
      .collect();
    let group = create(
      device,
      &wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &entries,
      },
    );
    groups.insert(key, group.clone());
    group
  }

  /// Forget every bind group (their resources were recreated).
  pub(crate) fn clear(&self) {
    self
      .groups
      .lock()
      .unwrap_or_else(|poisoned| poisoned.into_inner())
      .clear();
  }
}

/// Bind groups created on this thread so far.
#[cfg(test)]
pub(crate) fn created_on_this_thread() -> u64 {
  CREATED.with(std::cell::Cell::get)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_cached_bind_group_is_made_once_per_set_of_resources() {
    let Ok((device, _queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
      label: None,
      entries: &[wgpu::BindGroupLayoutEntry {
        binding: 0,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
      }],
    });
    let (a, b) = (
      device.create_sampler(&wgpu::SamplerDescriptor::default()),
      device.create_sampler(&wgpu::SamplerDescriptor::default()),
    );
    let cache = BindGroupCache::default();
    let before = created_on_this_thread();
    let first = cache.get(&device, "a", &layout, &[Bound::Sampler(a.clone())]);
    let again = cache.get(&device, "a", &layout, &[Bound::Sampler(a.clone())]);
    assert_eq!(first, again);
    cache.get(&device, "b", &layout, &[Bound::Sampler(b)]);
    assert_eq!(
      created_on_this_thread() - before,
      2,
      "one per distinct sampler"
    );
    cache.clear();
    cache.get(&device, "a", &layout, &[Bound::Sampler(a)]);
    assert_eq!(
      created_on_this_thread() - before,
      3,
      "made again after clear"
    );
  }

  #[test]
  fn every_bind_group_is_created_through_the_counter() {
    fn visit(dir: &std::path::Path, offenders: &mut Vec<String>) {
      for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
          visit(&path, offenders);
        } else if path.extension().is_some_and(|e| e == "rs") && !path.ends_with("bind_groups.rs") {
          let text = std::fs::read_to_string(&path).unwrap_or_default();
          if text.contains(".create_bind_group(") {
            offenders.push(path.display().to_string());
          }
        }
      }
    }
    let mut offenders = Vec::new();
    visit(
      &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
      &mut offenders,
    );
    assert!(offenders.is_empty(), "{offenders:?}");
  }
}
