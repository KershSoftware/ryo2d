//! RenderPipeline configuration (no GPU needed)

use ryo2d::prelude::*;

/// A pass that does nothing, identified by name.
struct Named(&'static str);

impl RenderPass for Named {
  fn id(&self) -> &str {
    self.0
  }

  fn execute(&mut self, _ctx: &mut RenderContext<'_>) {}
}

/// A pass that only takes part in lit frames.
struct LitOnly;

impl RenderPass for LitOnly {
  fn id(&self) -> &str {
    "lit_only"
  }

  fn should_run(&self, frame: &FrameInfo) -> bool {
    frame.lighting
  }

  fn execute(&mut self, _ctx: &mut RenderContext<'_>) {}
}

fn pipeline(ids: &[&'static str]) -> RenderPipeline {
  let mut pipeline = RenderPipeline::new();
  for &id in ids {
    pipeline.push(Named(id)).unwrap();
  }
  pipeline
}

fn ids(pipeline: &RenderPipeline) -> Vec<&str> {
  pipeline.pass_ids().collect()
}

#[test]
fn passes_keep_the_order_they_were_added_in() {
  assert_eq!(ids(&pipeline(&["a", "b", "c"])), vec!["a", "b", "c"]);
}

#[test]
fn default_is_empty() {
  assert_eq!(ids(&RenderPipeline::default()), Vec::<&str>::new());
}

#[test]
fn passes_go_before_or_after_a_named_pass() {
  let mut pipeline = pipeline(&["a", "c"]);

  pipeline.insert_before("c", Named("b")).unwrap();
  pipeline.insert_after("c", Named("d")).unwrap();

  assert_eq!(ids(&pipeline), vec!["a", "b", "c", "d"]);
}

#[test]
fn duplicate_ids_are_rejected() {
  let mut pipeline = pipeline(&["a", "b"]);

  assert_eq!(
    pipeline.push(Named("b")),
    Err(RenderPipelineError::DuplicatePass("b".to_string()))
  );
  assert_eq!(
    pipeline.insert_after("a", Named("a")),
    Err(RenderPipelineError::DuplicatePass("a".to_string()))
  );
  assert_eq!(ids(&pipeline), vec!["a", "b"]);
}

#[test]
fn unknown_ids_are_rejected() {
  let mut pipeline = pipeline(&["a"]);
  let unknown = Err(RenderPipelineError::UnknownPass("missing".to_string()));

  assert_eq!(pipeline.insert_before("missing", Named("b")), unknown);
  assert_eq!(pipeline.insert_after("missing", Named("b")), unknown);
  assert_eq!(pipeline.set_enabled("missing", false), unknown);
  assert_eq!(pipeline.is_enabled("missing"), None);
  assert!(pipeline.remove("missing").is_none());
  assert_eq!(ids(&pipeline), vec!["a"]);
}

#[test]
fn disabled_passes_keep_their_place_but_do_not_run() {
  let mut pipeline = pipeline(&["a", "b", "c"]);

  pipeline.set_enabled("b", false).unwrap();

  assert_eq!(pipeline.is_enabled("b"), Some(false));
  assert_eq!(ids(&pipeline), vec!["a", "b", "c"]);
  assert_eq!(
    pipeline.passes_to_run(&FrameInfo::default()),
    vec!["a", "c"]
  );

  pipeline.set_enabled("b", true).unwrap();
  assert_eq!(
    pipeline.passes_to_run(&FrameInfo::default()),
    vec!["a", "b", "c"]
  );
}

#[test]
fn remove_hands_the_pass_back() {
  let mut pipeline = pipeline(&["a", "b"]);

  let removed = pipeline.remove("a").unwrap();

  assert_eq!(removed.id(), "a");
  assert!(!pipeline.contains("a"));
  assert_eq!(ids(&pipeline), vec!["b"]);
}

#[test]
fn should_run_decides_per_frame() {
  let mut pipeline = pipeline(&["a"]);
  pipeline.push(LitOnly).unwrap();
  let lit = FrameInfo {
    lighting: true,
    ..FrameInfo::default()
  };

  assert_eq!(pipeline.passes_to_run(&FrameInfo::default()), vec!["a"]);
  assert_eq!(pipeline.passes_to_run(&lit), vec!["a", "lit_only"]);
}

#[test]
fn errors_name_the_pass() {
  assert_eq!(
    RenderPipelineError::DuplicatePass("x".into()).to_string(),
    "a render pass with id `x` is already in the pipeline"
  );
  assert_eq!(
    RenderPipelineError::UnknownPass("x".into()).to_string(),
    "no render pass with id `x`"
  );
}
