//! Render pipeline orchestration

use std::collections::HashMap;

use super::RenderContext;
use super::pass::RenderPass;

/// A configurable render pipeline that executes passes in dependency order.
///
/// The pipeline manages a collection of render passes and ensures they execute
/// in the correct order based on their declared dependencies.
///
/// # Example
///
/// ```ignore
/// let mut pipeline = RenderPipeline::new();
///
/// // Add passes (order doesn't matter - sorted by dependencies)
/// pipeline.add_pass(ClearPass::new());
/// pipeline.add_pass(UploadPass::new());
/// pipeline.add_pass(PresentPass::new());
///
/// // Execute
/// pipeline.execute(&mut render_context);
/// ```
pub struct RenderPipeline {
  /// All registered passes
  passes: Vec<Box<dyn RenderPass>>,
  /// Execution order (indices into passes vec)
  execution_order: Vec<usize>,
  /// Map from pass ID to index in passes vec
  pass_indices: HashMap<&'static str, usize>,
  /// Whether execution order needs recalculation
  order_dirty: bool,
}

impl Default for RenderPipeline {
  fn default() -> Self {
    Self::new()
  }
}

impl RenderPipeline {
  /// Create an empty pipeline
  pub fn new() -> Self {
    Self {
      passes: Vec::new(),
      execution_order: Vec::new(),
      pass_indices: HashMap::new(),
      order_dirty: true,
    }
  }

  /// Add a pass to the pipeline.
  ///
  /// The pass will be inserted and the execution order recalculated based on
  /// dependencies. Panics if a pass with the same ID already exists.
  pub fn add_pass<P: RenderPass + 'static>(&mut self, pass: P) {
    let id = pass.id();
    if self.pass_indices.contains_key(id) {
      panic!("Render pass '{}' already exists in pipeline", id);
    }

    let index = self.passes.len();
    self.passes.push(Box::new(pass));
    self.pass_indices.insert(id, index);
    self.order_dirty = true;
  }

  /// Remove a pass by ID.
  ///
  /// Returns the removed pass, or None if not found.
  pub fn remove_pass(&mut self, id: &str) -> Option<Box<dyn RenderPass>> {
    let index = *self.pass_indices.get(id)?;

    // Remove from passes vec
    let pass = self.passes.remove(index);

    // Rebuild pass_indices (indices shifted after removal)
    self.pass_indices.clear();
    for (i, p) in self.passes.iter().enumerate() {
      self.pass_indices.insert(p.id(), i);
    }

    self.order_dirty = true;
    Some(pass)
  }

  /// Get a reference to a pass by ID
  pub fn get_pass(&self, id: &str) -> Option<&dyn RenderPass> {
    let index = *self.pass_indices.get(id)?;
    Some(self.passes[index].as_ref())
  }

  /// Get a mutable reference to a pass by ID
  pub fn get_pass_mut(&mut self, id: &str) -> Option<&mut dyn RenderPass> {
    let index = *self.pass_indices.get(id)?;
    Some(self.passes[index].as_mut())
  }

  /// Enable or disable a pass by ID.
  ///
  /// Returns false if the pass doesn't exist.
  pub fn set_enabled(&mut self, id: &str, enabled: bool) -> bool {
    if let Some(pass) = self.get_pass_mut(id) {
      pass.set_enabled(enabled);
      true
    } else {
      false
    }
  }

  /// Check if a pass is enabled
  pub fn is_enabled(&self, id: &str) -> bool {
    self.get_pass(id).is_some_and(|p| p.is_enabled())
  }

  /// Get the number of passes in the pipeline
  pub fn pass_count(&self) -> usize {
    self.passes.len()
  }

  /// Get all pass IDs in execution order
  pub fn pass_ids(&mut self) -> Vec<&'static str> {
    self.ensure_order_computed();
    self
      .execution_order
      .iter()
      .map(|&i| self.passes[i].id())
      .collect()
  }

  /// Execute all enabled passes in dependency order.
  ///
  /// Passes are executed if:
  /// 1. They are enabled (`is_enabled()` returns true)
  /// 2. Their runtime check passes (`should_execute()` returns true)
  pub fn execute(&mut self, ctx: &mut RenderContext) {
    self.ensure_order_computed();

    // Execute passes in order
    for &index in &self.execution_order.clone() {
      let pass = &mut self.passes[index];

      // Skip disabled passes
      if !pass.is_enabled() {
        continue;
      }

      // Skip passes that shouldn't run this frame
      if !pass.should_execute(ctx) {
        continue;
      }

      // Execute the pass
      pass.execute(ctx);
    }
  }

  /// Ensure execution order is computed
  fn ensure_order_computed(&mut self) {
    if !self.order_dirty {
      return;
    }

    self.execution_order = self.compute_execution_order();
    self.order_dirty = false;
  }

  /// Compute execution order using topological sort
  fn compute_execution_order(&self) -> Vec<usize> {
    // Build adjacency list for dependency graph
    let mut in_degree: HashMap<usize, usize> = HashMap::new();
    let mut dependents: HashMap<usize, Vec<usize>> = HashMap::new();

    for (i, _pass) in self.passes.iter().enumerate() {
      in_degree.insert(i, 0);
      dependents.insert(i, Vec::new());
    }

    // Count incoming edges (dependencies)
    for (i, pass) in self.passes.iter().enumerate() {
      for dep_id in pass.dependencies() {
        if let Some(&dep_index) = self.pass_indices.get(dep_id) {
          *in_degree.get_mut(&i).unwrap() += 1;
          dependents.get_mut(&dep_index).unwrap().push(i);
        }
        // Silently ignore missing dependencies (pass might be optional)
      }
    }

    // Kahn's algorithm for topological sort
    let mut queue: Vec<usize> = in_degree
      .iter()
      .filter_map(|(&i, &deg)| if deg == 0 { Some(i) } else { None })
      .collect();

    // Sort initial queue for deterministic ordering (descending so pop() yields ascending)
    queue.sort_by(|&a, &b| self.passes[b].id().cmp(self.passes[a].id()));

    let mut result = Vec::with_capacity(self.passes.len());

    while let Some(current) = queue.pop() {
      result.push(current);

      // Process dependents
      let mut new_ready = Vec::new();
      for &dependent in &dependents[&current] {
        let degree = in_degree.get_mut(&dependent).unwrap();
        *degree -= 1;
        if *degree == 0 {
          new_ready.push(dependent);
        }
      }

      // Sort new ready passes for deterministic ordering (descending so pop() yields ascending)
      new_ready.sort_by(|&a, &b| self.passes[b].id().cmp(self.passes[a].id()));
      queue.extend(new_ready);
    }

    // Check for cycles
    if result.len() != self.passes.len() {
      let remaining: Vec<_> = (0..self.passes.len())
        .filter(|i| !result.contains(i))
        .map(|i| self.passes[i].id())
        .collect();
      panic!(
        "Circular dependency detected in render passes: {:?}",
        remaining
      );
    }

    result
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  /// Test pass for unit testing
  struct TestPass {
    id: &'static str,
    deps: Vec<&'static str>,
    enabled: bool,
    executed: std::sync::atomic::AtomicBool,
  }

  impl TestPass {
    fn new(id: &'static str, deps: Vec<&'static str>) -> Self {
      Self {
        id,
        deps,
        enabled: true,
        executed: std::sync::atomic::AtomicBool::new(false),
      }
    }

    #[allow(dead_code)]
    fn was_executed(&self) -> bool {
      self.executed.load(std::sync::atomic::Ordering::SeqCst)
    }
  }

  impl RenderPass for TestPass {
    fn id(&self) -> &'static str {
      self.id
    }

    fn dependencies(&self) -> &[&'static str] {
      &self.deps
    }

    fn execute(&mut self, _ctx: &mut RenderContext) {
      self
        .executed
        .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    fn is_enabled(&self) -> bool {
      self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
      self.enabled = enabled;
    }
  }

  #[test]
  fn test_empty_pipeline() {
    let pipeline = RenderPipeline::new();
    assert_eq!(pipeline.pass_count(), 0);
  }

  #[test]
  fn test_add_pass() {
    let mut pipeline = RenderPipeline::new();
    pipeline.add_pass(TestPass::new("test", vec![]));
    assert_eq!(pipeline.pass_count(), 1);
    assert!(pipeline.get_pass("test").is_some());
  }

  #[test]
  fn test_remove_pass() {
    let mut pipeline = RenderPipeline::new();
    pipeline.add_pass(TestPass::new("test", vec![]));
    let removed = pipeline.remove_pass("test");
    assert!(removed.is_some());
    assert_eq!(pipeline.pass_count(), 0);
  }

  #[test]
  fn test_dependency_order() {
    let mut pipeline = RenderPipeline::new();

    // Add passes in reverse order - should still execute in correct order
    pipeline.add_pass(TestPass::new("c", vec!["b"]));
    pipeline.add_pass(TestPass::new("a", vec![]));
    pipeline.add_pass(TestPass::new("b", vec!["a"]));

    let order = pipeline.pass_ids();
    assert_eq!(order, vec!["a", "b", "c"]);
  }

  #[test]
  #[should_panic(expected = "Circular dependency")]
  fn test_circular_dependency_detection() {
    let mut pipeline = RenderPipeline::new();
    pipeline.add_pass(TestPass::new("a", vec!["b"]));
    pipeline.add_pass(TestPass::new("b", vec!["a"]));

    // This should panic
    pipeline.pass_ids();
  }

  #[test]
  fn test_enable_disable() {
    let mut pipeline = RenderPipeline::new();
    pipeline.add_pass(TestPass::new("test", vec![]));

    assert!(pipeline.is_enabled("test"));
    pipeline.set_enabled("test", false);
    assert!(!pipeline.is_enabled("test"));
  }
}
