//! What a shipped build (no debug assertions, no `dev-tools` feature) leaves out.
//! Empty in a development test run.
#![cfg(not(dev_tools))]

#[test]
#[allow(clippy::assertions_on_constants)]
fn dev_tools_are_off_in_a_release_build() {
  assert!(!ryo2d::DEV_TOOLS);
}
