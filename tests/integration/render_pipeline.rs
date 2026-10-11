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
fn a_removed_pass_can_be_inserted_again() {
  let mut pipeline = RenderPipeline::default_2d();
  let original: Vec<String> = pipeline.pass_ids().map(str::to_string).collect();

  let removed = pipeline.remove(pass_ids::POST_PROCESS).unwrap();
  assert!(!pipeline.contains(pass_ids::POST_PROCESS));
  pipeline
    .insert_after(pass_ids::UNLIT_TRAILS, removed)
    .unwrap();

  let restored: Vec<String> = pipeline.pass_ids().map(str::to_string).collect();
  assert_eq!(restored, original);
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

const ORDER: [&str; 12] = [
  pass_ids::UPLOAD,
  pass_ids::CLEAR,
  pass_ids::RAYCAST,
  pass_ids::GPU_SPRITES,
  pass_ids::CPU_BLEND,
  pass_ids::TRAILS,
  pass_ids::LIGHTING,
  pass_ids::UNLIT_TRAILS,
  pass_ids::POST_PROCESS,
  pass_ids::UI,
  pass_ids::COMPOSITE,
  pass_ids::CAPTURE,
];

#[test]
fn default_2d_runs_the_built_in_passes_in_order() {
  assert_eq!(ids(&RenderPipeline::default_2d()), ORDER.to_vec());
}

#[test]
fn a_plain_frame_runs_the_scene_passes() {
  assert_eq!(
    RenderPipeline::default_2d().passes_to_run(&FrameInfo::default()),
    vec!["upload", "clear", "cpu_blend", "composite"]
  );
}

#[test]
fn a_full_2d_frame_runs_every_scene_pass() {
  let frame = FrameInfo {
    gpu_sprites: true,
    trails: true,
    lighting: true,
    effects: true,
    ui: true,
    capture: true,
    ..FrameInfo::default()
  };
  assert_eq!(
    RenderPipeline::default_2d().passes_to_run(&frame),
    vec![
      "upload",
      "clear",
      "gpu_sprites",
      "cpu_blend",
      "trails",
      "lighting",
      "unlit_trails",
      "post_process",
      "ui",
      "composite",
      "capture",
    ]
  );
}

#[test]
fn a_raycast_frame_skips_the_2d_scene() {
  let frame = FrameInfo {
    raycast: true,
    gpu_sprites: true,
    trails: true,
    lighting: true,
    effects: true,
    ui: true,
    ..FrameInfo::default()
  };
  assert_eq!(
    RenderPipeline::default_2d().passes_to_run(&frame),
    vec!["raycast", "post_process", "ui", "composite"]
  );
}

#[test]
fn world_2d_order() {
  assert_eq!(
    ids(&RenderPipeline::world_2d()),
    vec![
      "clear",
      "raycast",
      "world",
      "lighting",
      "unlit_world",
      "post_process",
      "ui",
      "composite",
      "capture",
    ]
  );
}

#[test]
fn a_full_world_frame_runs_both_world_passes() {
  let frame = FrameInfo {
    gpu_sprites: true,
    trails: true,
    lighting: true,
    ..FrameInfo::default()
  };
  assert_eq!(
    RenderPipeline::world_2d().passes_to_run(&frame),
    vec!["clear", "world", "lighting", "unlit_world", "composite"]
  );
}
