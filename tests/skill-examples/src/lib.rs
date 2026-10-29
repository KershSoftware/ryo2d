//! Every ```rust block in `.claude/skills/*/SKILL.md`, compiled. The build
//! script writes each skill as a Rust file whose lines match the
//! markdown's, so a compile error at `<skill>/SKILL.md.rs:LINE` is at that
//! line of the skill. The examples are only compiled, never run.
//!
//! Each block becomes a function with `ctx: &mut SceneContext` and
//! `dt: f32` in scope, under `use ryo2d::prelude::*`. A block that uses
//! `?` returns a `Result`, and one made of methods (`fn update(&mut self,
//! ..)`) is an `impl` of a struct named `Example`. What else a block needs
//! is said in HTML comments on the lines just above its fence, which a
//! reader of the skill sees as the example's inputs:
//!
//! - `<!-- given: sprite: Handle<Image>, frames: Vec<Handle<Image>> -->`:
//!   more of the function's parameters
//! - `<!-- self: player: Entity, speed: f32 -->`: the fields of `Example`;
//!   on a block of statements, they run in a method of `Example`, so they
//!   can use `self`
//! - `<!-- with: #[derive(Component)] struct Player; -->`: declarations
//!   the block uses (items, not statements), such as a type an earlier
//!   block defined or an import outside the prelude; `given:` and `self:`
//!   types can name them
//!
//! A block fenced ```rust,ignore is not compiled: a list of signatures, a
//! sketch, or code inside the engine that a game cannot reach.
//!
//! `SKILL_EXAMPLES=ryo-audio,ryo-ui cargo check -p skill-examples` compiles
//! only those skills' examples.

include!(concat!(env!("OUT_DIR"), "/skills.rs"));
