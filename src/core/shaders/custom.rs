//! Compiler for game-authored WGSL shaders
//!
//! Games write one function (`sprite` or `effect`) and optionally a `Params`
//! struct. This module wraps that code with the engine prelude and a generated
//! entry point, validates the result with naga, reflects the `Params` layout
//! and maps errors back to line numbers in the game's own file.

use crate::core::ecs::{ParamValue, ShaderParams};

/// Which kind of game shader is being compiled
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ShaderKind {
  /// Per-sprite fragment function, `fn sprite(in: SpriteIn, params: Params) -> vec4<f32>`
  Sprite,
  /// Full-screen post-process, `fn effect(in: EffectIn, params: Params) -> vec4<f32>`
  Effect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParamKind {
  F32,
  Vec2,
  Vec3,
  Vec4,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ParamField {
  pub(crate) name: String,
  pub(crate) offset: u32,
  pub(crate) kind: ParamKind,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ParamLayout {
  pub(crate) fields: Vec<ParamField>,
  pub(crate) size: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ShaderError {
  pub(crate) shader: String,
  pub(crate) line: Option<u32>,
  pub(crate) column: Option<u32>,
  pub(crate) message: String,
}

impl std::fmt::Display for ShaderError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}", self.shader)?;
    if let Some(line) = self.line {
      write!(f, ":{line}")?;
      if let Some(column) = self.column {
        write!(f, ":{column}")?;
      }
    }
    write!(f, ": {}", self.message)
  }
}

/// The validated, ready-to-compile module for a game shader
pub(crate) struct CompiledShader {
  pub(crate) wgsl: String,
  pub(crate) layout: ParamLayout,
}

/// Engine-side sprite helpers, appended after `sprite.wgsl` and before the game's code
const SPRITE_PRELUDE: &str = "
struct SpriteIn { uv: vec2<f32>, size: vec2<f32>, screen_pos: vec2<f32>, time: f32 }
var<private> ry_bounds: vec4<f32>;
fn sample(uv: vec2<f32>) -> vec4<f32> {
  return sample_masked(ry_bounds.xy + uv * (ry_bounds.zw - ry_bounds.xy), ry_bounds);
}
";

/// Self-contained effect module header (no reference to `Params`, which comes later)
///
/// The vertex stage draws the same six-vertex quad as the built-in full-screen pass.
const EFFECT_PRELUDE: &str = "
struct EffectGlobals { time_resolution: vec4<f32> }
@group(0) @binding(0) var ry_scene: texture_2d<f32>;
@group(0) @binding(1) var ry_sampler: sampler;
@group(0) @binding(3) var<uniform> ry_globals: EffectGlobals;
struct EffectIn { uv: vec2<f32>, time: f32, resolution: vec2<f32> }
fn scene(uv: vec2<f32>) -> vec4<f32> {
  return textureSampleLevel(ry_scene, ry_sampler, clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0);
}
struct RyVsOut { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> RyVsOut {
  var positions = array<vec2<f32>, 6>(
    vec2<f32>(-1.0, 1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, -1.0),
    vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(1.0, -1.0),
  );
  var uvs = array<vec2<f32>, 6>(
    vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
    vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
  );
  var out: RyVsOut;
  out.position = vec4<f32>(positions[i], 0.0, 1.0);
  out.uv = uvs[i];
  return out;
}
";

/// Used when the game declares no `Params`
const FALLBACK_PARAMS: &str = "struct Params { unused: f32 }\n";

const SPRITE_PARAM_LIMIT: u32 = 32;
const EFFECT_PARAM_LIMIT: u32 = 64;

/// The assembled source with the byte range holding the game's code
struct Source {
  text: String,
  game: std::ops::Range<usize>,
}

impl Source {
  /// Engine head + game code (+ fallback `Params`), without the generated entry
  fn new(head: &str, game: &str, fallback: bool) -> Self {
    let mut text = String::with_capacity(head.len() + game.len() + 64);
    text.push_str(head);
    if !text.ends_with('\n') {
      text.push('\n');
    }
    let start = text.len();
    text.push_str(game);
    let end = text.len();
    text.push('\n');
    if fallback {
      text.push_str(FALLBACK_PARAMS);
    }
    Self {
      text,
      game: start..end,
    }
  }

  /// 1-based line and column within the game's code, if `offset` lies in it
  fn locate(&self, offset: usize) -> Option<(u32, u32)> {
    if !self.game.contains(&offset) {
      return None;
    }
    self.position(offset)
  }

  fn position(&self, offset: usize) -> Option<(u32, u32)> {
    let before = self.text.get(self.game.start..offset)?;
    let line = before.matches('\n').count() as u32 + 1;
    let line_start = before.rfind('\n').map_or(0, |p| p + 1);
    let column = before[line_start..].chars().count() as u32 + 1;
    Some((line, column))
  }

  /// Position just past the game's last non-blank character
  fn end_of_game(&self) -> Option<(u32, u32)> {
    let game = self.text.get(self.game.clone())?;
    self.position(self.game.start + game.trim_end().len())
  }

  /// Build an error from an optional byte offset into the assembled source
  ///
  /// An offset at the end of the game's code or at the first generated token
  /// right after it means the game's code ended unexpectedly (an unclosed
  /// brace or parenthesis), so it is reported at the game's last line.
  fn error(&self, id: &str, offset: Option<usize>, message: impl Into<String>) -> ShaderError {
    let message = message.into();
    let at = |line, column, message| ShaderError {
      shader: id.to_string(),
      line,
      column,
      message,
    };
    let Some(o) = offset else {
      return at(None, None, message);
    };
    if let Some((line, column)) = self.locate(o) {
      return at(Some(line), Some(column), message);
    }
    if (self.game.end..=self.game.end + 1).contains(&o)
      && let Some((line, column)) = self.end_of_game()
    {
      return at(
        Some(line),
        Some(column),
        format!("unexpected end of shader: {message}"),
      );
    }
    at(None, None, format!("in engine-generated code: {message}"))
  }
}

/// Join an error and its sources: "outer: inner: innermost"
fn error_chain(err: &dyn std::error::Error) -> String {
  let mut out = err.to_string();
  let mut source = err.source();
  while let Some(inner) = source {
    out.push_str(": ");
    out.push_str(&inner.to_string());
    source = inner.source();
  }
  out
}

/// Parse failure before line mapping: byte offset and naga's message
struct RawParseError {
  offset: Option<usize>,
  message: String,
  /// Byte offsets of naga's labelled spans (for a redefinition, the earlier
  /// definition)
  labels: Vec<usize>,
}

impl RawParseError {
  /// Whether naga could not find `Params` (the game declared none)
  fn is_unresolved_params(&self) -> bool {
    let names_params = self.message.contains("`Params`") || self.message.contains("'Params'");
    names_params
      && (self.message.contains("unknown type") || self.message.contains("no definition in scope"))
  }
}

fn parse_raw(src: &Source) -> Result<naga::Module, RawParseError> {
  naga::front::wgsl::parse_str(&src.text).map_err(|e| RawParseError {
    offset: e.location(&src.text).map(|l| l.offset as usize),
    message: e.message().to_string(),
    labels: e
      .labels()
      .filter_map(|(span, _)| span.to_range().map(|r| r.start))
      .collect(),
  })
}

fn parse(src: &Source, id: &str) -> Result<naga::Module, ShaderError> {
  parse_raw(src).map_err(|e| src.parse_error(id, e))
}

impl Source {
  /// Map a parse failure to the game's code. A name defined both by the game
  /// and by engine code is reported at the game's definition, saying so.
  fn parse_error(&self, id: &str, e: RawParseError) -> ShaderError {
    if e.message.starts_with("redefinition") {
      let in_game = |offset: &usize| self.game.contains(offset);
      let at_game = e
        .offset
        .filter(in_game)
        .or_else(|| e.labels.iter().copied().find(in_game));
      let engine_too = e.offset.iter().chain(&e.labels).any(|o| !in_game(o));
      if let (Some(at), true) = (at_game, engine_too) {
        let message = if e.message.contains("`Params`") {
          "`Params` must be a struct".to_string()
        } else {
          format!(
            "{}: the engine defines this name, so rename yours",
            e.message
          )
        };
        return self.error(id, Some(at), message);
      }
    }
    self.error(id, e.offset, e.message)
  }
}

fn validate(src: &Source, id: &str, module: &naga::Module) -> Result<(), ShaderError> {
  use naga::valid::{Capabilities, ValidationFlags, Validator};
  Validator::new(ValidationFlags::all(), Capabilities::empty())
    .validate(module)
    .map(|_| ())
    .map_err(|e| {
      let offset = e
        .spans()
        .find_map(|(span, _)| span.to_range())
        .map(|r| r.start);
      src.error(id, offset, error_chain(e.as_inner()))
    })
}

/// Whether a parsed module has a struct type named `Params`
fn has_params_struct(module: &naga::Module) -> bool {
  module.types.iter().any(|(_, t)| {
    t.name.as_deref() == Some("Params") && matches!(t.inner, naga::TypeInner::Struct { .. })
  })
}

fn reflect_params(
  src: &Source,
  id: &str,
  module: &naga::Module,
  limit: u32,
) -> Result<ParamLayout, ShaderError> {
  use naga::{ScalarKind, TypeInner, VectorSize};

  let found = module.types.iter().find(|(_, t)| {
    t.name.as_deref() == Some("Params") && matches!(t.inner, TypeInner::Struct { .. })
  });
  let Some((handle, ty)) = found else {
    return Err(src.error(id, None, "no `Params` struct found"));
  };
  let at = module.types.get_span(handle).to_range().map(|r| r.start);
  let TypeInner::Struct { members, span } = &ty.inner else {
    return Err(src.error(id, at, "`Params` must be a struct"));
  };

  let mut fields = Vec::with_capacity(members.len());
  for member in members {
    let name = member.name.clone().unwrap_or_default();
    let float = |s: &naga::Scalar| s.kind == ScalarKind::Float && s.width == 4;
    let kind = match &module.types[member.ty].inner {
      TypeInner::Scalar(s) if float(s) => Some(ParamKind::F32),
      TypeInner::Vector { size, scalar } if float(scalar) => Some(match size {
        VectorSize::Bi => ParamKind::Vec2,
        VectorSize::Tri => ParamKind::Vec3,
        VectorSize::Quad => ParamKind::Vec4,
      }),
      _ => None,
    };
    let Some(kind) = kind else {
      return Err(src.error(
        id,
        at,
        format!(
          "`Params.{name}` has an unsupported type; fields must be f32, vec2<f32>, vec3<f32> or vec4<f32>"
        ),
      ));
    };
    fields.push(ParamField {
      name,
      offset: member.offset,
      kind,
    });
  }

  if *span > limit {
    return Err(src.error(
      id,
      at,
      format!(
        "`Params` is {span} bytes but at most {limit} bytes ({} floats) are supported here",
        limit / 4
      ),
    ));
  }
  Ok(ParamLayout {
    fields,
    size: *span,
  })
}

/// Check the game's entry function exists with the expected signature
fn check_entry(
  src: &Source,
  id: &str,
  module: &naga::Module,
  name: &str,
  input: &str,
) -> Result<(), ShaderError> {
  use naga::{ScalarKind, TypeInner, VectorSize};

  let expected = format!("fn {name}(in: {input}, params: Params) -> vec4<f32>");
  let Some((handle, func)) = module
    .functions
    .iter()
    .find(|(_, f)| f.name.as_deref() == Some(name))
  else {
    return Err(src.error(
      id,
      None,
      format!("missing entry function; expected `{expected}`"),
    ));
  };
  let at = module
    .functions
    .get_span(handle)
    .to_range()
    .map(|r| r.start);
  let type_name = |ty| module.types[ty].name.as_deref();
  let args_ok = func.arguments.len() == 2
    && type_name(func.arguments[0].ty) == Some(input)
    && type_name(func.arguments[1].ty) == Some("Params");
  let result_ok = func.result.as_ref().is_some_and(|r| {
    matches!(
      &module.types[r.ty].inner,
      TypeInner::Vector { size: VectorSize::Quad, scalar }
        if scalar.kind == ScalarKind::Float && scalar.width == 4
    )
  });
  if args_ok && result_ok {
    Ok(())
  } else {
    Err(src.error(
      id,
      at,
      format!("`{name}` has the wrong signature; expected `{expected}`"),
    ))
  }
}

/// Constructor arguments for `Params(...)` reading from the sprite's float array `f`
fn sprite_fields(layout: &ParamLayout) -> String {
  layout
    .fields
    .iter()
    .map(|field| {
      let i = (field.offset / 4) as usize;
      match field.kind {
        ParamKind::F32 => format!("f[{i}]"),
        ParamKind::Vec2 => format!("vec2<f32>(f[{i}], f[{}])", i + 1),
        ParamKind::Vec3 => format!("vec3<f32>(f[{i}], f[{}], f[{}])", i + 1, i + 2),
        ParamKind::Vec4 => {
          format!(
            "vec4<f32>(f[{i}], f[{}], f[{}], f[{}])",
            i + 1,
            i + 2,
            i + 3
          )
        }
      }
    })
    .collect::<Vec<_>>()
    .join(", ")
}

fn sprite_entry(layout: &ParamLayout) -> String {
  format!(
    "
@fragment
fn fs_custom(in: VertexOutput) -> @location(0) vec4<f32> {{
  ry_bounds = in.uv_bounds;
  let extent = in.uv_bounds.zw - in.uv_bounds.xy;
  let local = (in.uv - in.uv_bounds.xy) / extent;
  let size = abs(extent) / texel_size();
  let f = array<f32, 8>(in.effect_params.x, in.effect_params.y, in.effect_params.z, in.effect_params.w,
                        in.effect_extra.x, in.effect_extra.y, in.effect_extra.z, in.effect_extra.w);
  let params = Params({fields});
  return finish(sprite(SpriteIn(local, size, in.clip_position.xy, globals.time_resolution.x), params), in.tint);
}}
",
    fields = sprite_fields(layout)
  )
}

const EFFECT_ENTRY: &str = "
@group(0) @binding(2) var<uniform> ry_params: Params;
@fragment
fn fs_main(in: RyVsOut) -> @location(0) vec4<f32> {
  return effect(EffectIn(in.uv, ry_globals.time_resolution.x, ry_globals.time_resolution.yz), ry_params);
}
";

fn retry_with_fallback(
  head: &str,
  user_source: &str,
  id: &str,
) -> Result<(Source, bool, naga::Module), ShaderError> {
  let src = Source::new(head, user_source, true);
  let module = parse(&src, id)?;
  Ok((src, true, module))
}

/// Size in bytes of the uniform data for a layout: 32 for sprites (eight
/// floats in the instance), the struct size rounded up to 16 for effects
pub(crate) fn uniform_size(kind: ShaderKind, layout: &ParamLayout) -> usize {
  match kind {
    ShaderKind::Sprite => SPRITE_PARAM_LIMIT as usize,
    ShaderKind::Effect => (layout.size as usize).next_multiple_of(16),
  }
}

/// Assemble prelude + game code + generated entry, validate with naga, reflect `Params`
pub(crate) fn compile(
  kind: ShaderKind,
  id: &str,
  user_source: &str,
) -> Result<CompiledShader, ShaderError> {
  // Editors on Windows may save a byte order mark, which WGSL rejects
  let user_source = user_source.strip_prefix('\u{feff}').unwrap_or(user_source);
  let (head, entry_name, input, limit) = match kind {
    ShaderKind::Sprite => (
      format!(
        "{}{}",
        crate::core::graphics::sprite_gpu::SPRITE_SHADER,
        SPRITE_PRELUDE
      ),
      "sprite",
      "SpriteIn",
      SPRITE_PARAM_LIMIT,
    ),
    ShaderKind::Effect => (
      EFFECT_PRELUDE.to_string(),
      "effect",
      "EffectIn",
      EFFECT_PARAM_LIMIT,
    ),
  };

  // First pass: everything but the generated entry, so the game's own errors,
  // its entry signature and its `Params` are checked before the entry is built
  //
  // `Params` is declared by the game exactly when it parses without the
  // fallback and the module has the struct; otherwise the fallback is added.
  let plain = Source::new(&head, user_source, false);
  let (base, fallback, module) = match parse_raw(&plain) {
    Ok(module) if has_params_struct(&module) => (plain, false, module),
    Ok(_) => retry_with_fallback(&head, user_source, id)?,
    Err(e) if e.is_unresolved_params() => retry_with_fallback(&head, user_source, id)?,
    Err(e) => return Err(plain.parse_error(id, e)),
  };
  check_entry(&base, id, &module, entry_name, input)?;
  let layout = reflect_params(&base, id, &module, limit)?;

  let entry = match kind {
    ShaderKind::Sprite => sprite_entry(&layout),
    ShaderKind::Effect => EFFECT_ENTRY.to_string(),
  };
  let mut full = Source::new(&head, user_source, fallback);
  full.text.push_str(&entry);
  let module = parse(&full, id)?;
  validate(&full, id, &module)?;

  Ok(CompiledShader {
    wgsl: full.text,
    layout,
  })
}

/// Pack named values into the layout's bytes, calling `on_problem` for every
/// name the shader has no field for (`None`) and every value of the wrong type
/// for its field (`Some(expected)`)
///
/// `out` is zeroed first. Nothing is allocated here, so a sprite with correct
/// names costs no heap traffic; the caller decides whether to warn.
pub(crate) fn pack_with(
  layout: &ParamLayout,
  params: &ShaderParams,
  out: &mut [u8],
  mut on_problem: impl FnMut(&str, Option<ParamKind>),
) {
  out.fill(0);
  for (name, value) in params.iter() {
    let Some(field) = layout.fields.iter().find(|f| f.name == name) else {
      on_problem(name, None);
      continue;
    };
    let floats: &[f32] = match (&field.kind, &value) {
      (ParamKind::F32, ParamValue::F32(v)) => std::slice::from_ref(v),
      (ParamKind::Vec2, ParamValue::Vec2(v)) => v,
      (ParamKind::Vec3, ParamValue::Vec3(v)) => v,
      (ParamKind::Vec4, ParamValue::Vec4(v)) => v,
      (ParamKind::Vec3, ParamValue::Color(c)) => &c[..3],
      (ParamKind::Vec4, ParamValue::Color(c)) => c,
      _ => {
        on_problem(name, Some(field.kind));
        continue;
      }
    };
    for (i, v) in floats.iter().enumerate() {
      let at = field.offset as usize + i * 4;
      if let Some(slot) = out.get_mut(at..at + 4) {
        slot.copy_from_slice(&v.to_le_bytes());
      }
    }
  }
}

/// [`pack_with`], returning the problem names (tests)
#[cfg(test)]
pub(crate) fn pack(layout: &ParamLayout, params: &ShaderParams, out: &mut [u8]) -> Vec<String> {
  let mut problems = Vec::new();
  pack_with(layout, params, out, |name, _| {
    problems.push(name.to_string())
  });
  problems
}

/// Identifies a shader's source for its kind: handles whose sources are equal
/// share one compiled pipeline (and one error report)
pub(crate) fn source_key(kind: ShaderKind, source: &str) -> u64 {
  use std::hash::{Hash, Hasher};
  let mut hasher = std::collections::hash_map::DefaultHasher::new();
  kind.hash(&mut hasher);
  source.hash(&mut hasher);
  hasher.finish()
}

/// Log a shader error once per source (see [`source_key`]); returns whether it
/// was logged now
pub(crate) fn report_once(key: u64, error: &ShaderError) -> bool {
  use std::collections::HashSet;
  use std::sync::{Mutex, OnceLock};

  static SEEN: OnceLock<Mutex<HashSet<u64>>> = OnceLock::new();
  let seen = SEEN.get_or_init(|| Mutex::new(HashSet::new()));
  let first = seen.lock().unwrap_or_else(|e| e.into_inner()).insert(key);
  if first {
    crate::engine_error!(module: "Shaders", "{}", error);
  }
  first
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ui::Color;

  fn sprite_src(params: &str, body: &str) -> String {
    format!("{params}\nfn sprite(in: SpriteIn, params: Params) -> vec4<f32> {{\n{body}\n}}\n")
  }

  fn compile_sprite(src: &str) -> Result<CompiledShader, ShaderError> {
    compile(ShaderKind::Sprite, "test.wgsl", src)
  }

  #[test]
  fn layout_follows_wgsl_rules() {
    let src = sprite_src(
      "struct Params { a: f32, b: vec4<f32> }",
      "return params.b * params.a;",
    );
    let compiled = compiled_effect_or_sprite(&src);
    let layout = compiled.layout;
    assert_eq!(layout.size, 32);
    assert_eq!(
      layout.fields,
      vec![
        ParamField {
          name: "a".into(),
          offset: 0,
          kind: ParamKind::F32
        },
        ParamField {
          name: "b".into(),
          offset: 16,
          kind: ParamKind::Vec4
        },
      ]
    );

    let src = sprite_src(
      "struct Params { a: vec3<f32>, b: f32 }",
      "return vec4<f32>(params.a, params.b);",
    );
    let layout = compiled_effect_or_sprite(&src).layout;
    assert_eq!(layout.size, 16);
    assert_eq!(layout.fields[0].kind, ParamKind::Vec3);
    assert_eq!(layout.fields[1].offset, 12);
  }

  fn compiled_effect_or_sprite(src: &str) -> CompiledShader {
    match compile_sprite(src) {
      Ok(c) => c,
      Err(e) => panic!("compile failed: {e}"),
    }
  }

  #[test]
  fn sprite_params_over_32_bytes_are_rejected() {
    let src = sprite_src(
      "struct Params { a: vec4<f32>, b: vec4<f32>, c: f32 }",
      "return params.a;",
    );
    let err = compile_sprite(&src).err().expect("should be rejected");
    assert!(err.message.contains("32"), "{err}");
  }

  #[test]
  fn effect_params_up_to_64_bytes_are_accepted() {
    let src = "struct Params { a: vec4<f32>, b: vec4<f32>, c: vec4<f32>, d: vec4<f32> }\n\
      fn effect(in: EffectIn, params: Params) -> vec4<f32> { return scene(in.uv) * params.d; }\n";
    let compiled = compile(ShaderKind::Effect, "fx.wgsl", src).expect("64 bytes is allowed");
    assert_eq!(compiled.layout.size, 64);

    let too_big = "struct Params { a: vec4<f32>, b: vec4<f32>, c: vec4<f32>, d: vec4<f32>, e: f32 }\n\
      fn effect(in: EffectIn, params: Params) -> vec4<f32> { return scene(in.uv) * params.e; }\n";
    assert!(compile(ShaderKind::Effect, "fx.wgsl", too_big).is_err());
  }

  #[test]
  fn missing_entry_function_is_an_error() {
    let src = "fn not_sprite(in: SpriteIn, params: Params) -> vec4<f32> { return vec4<f32>(1.0); }";
    let err = compile_sprite(src).err().expect("should fail");
    assert!(err.message.contains("sprite"), "{err}");
  }

  #[test]
  fn unsupported_param_type_is_an_error() {
    let src = sprite_src("struct Params { m: mat2x2<f32> }", "return vec4<f32>(1.0);");
    let err = compile_sprite(&src).err().expect("should fail");
    assert!(err.message.contains("unsupported"), "{err}");
  }

  #[test]
  fn errors_point_at_the_games_line() {
    let src = "// comment\n\nfn sprite(in: SpriteIn, params: Params) -> vec4<f32> {\n  return vec4<f32>(nope);\n}\n";
    let err = compile_sprite(src).err().expect("should fail");
    assert_eq!(err.line, Some(4), "{err}");
    assert!(err.column.is_some());

    let src = "fn sprite(in: SpriteIn, params: Params) -> vec4<f32> {\n  let a = 1.0;\n  return vec4<f32>(zz);\n}\n";
    assert_eq!(compile_sprite(src).err().expect("fail").line, Some(3));
  }

  #[test]
  fn shaders_without_params_compile() {
    let src = "fn sprite(in: SpriteIn, params: Params) -> vec4<f32> { return sample(in.uv); }";
    let compiled = compile_sprite(src).expect("params are optional");
    assert!(compiled.wgsl.contains("fs_custom"));
  }

  #[test]
  fn pack_places_values_by_name() {
    let layout = ParamLayout {
      fields: vec![
        ParamField {
          name: "a".into(),
          offset: 0,
          kind: ParamKind::F32,
        },
        ParamField {
          name: "b".into(),
          offset: 16,
          kind: ParamKind::Vec4,
        },
      ],
      size: 32,
    };
    let params = ShaderParams::new()
      .with("a", 2.5)
      .with("b", Color::rgb(255, 128, 0))
      .with("missing", 1.0);
    let mut out = [0u8; 32];
    let unknown = pack(&layout, &params, &mut out);
    assert_eq!(unknown, vec!["missing".to_string()]);

    let floats: Vec<f32> = out
      .as_chunks::<4>()
      .0
      .iter()
      .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
      .collect();
    assert_eq!(floats[0], 2.5);
    assert_eq!(&floats[4..8], &[1.0, 128.0 / 255.0, 0.0, 1.0]);
    assert_eq!(&floats[1..4], &[0.0, 0.0, 0.0]);
  }

  #[test]
  fn pack_fills_a_vec3_from_a_color_without_its_alpha() {
    let layout = ParamLayout {
      fields: vec![
        ParamField {
          name: "edge".into(),
          offset: 0,
          kind: ParamKind::Vec3,
        },
        ParamField {
          name: "amount".into(),
          offset: 12,
          kind: ParamKind::F32,
        },
      ],
      size: 16,
    };
    // `amount` first, so alpha spilling past the vec3 would overwrite it
    let params = ShaderParams::new()
      .with("amount", 0.5)
      .with("edge", Color::rgb(255, 0, 51));
    let mut out = [0u8; 16];
    let problems = pack(&layout, &params, &mut out);
    assert!(problems.is_empty(), "{problems:?}");

    let floats: Vec<f32> = out
      .as_chunks::<4>()
      .0
      .iter()
      .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
      .collect();
    assert_eq!(floats, [1.0, 0.0, 0.2, 0.5]);
  }

  #[test]
  fn redefining_an_engine_name_says_so() {
    let src = "fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> { return c; }\nfn sprite(in: SpriteIn, params: Params) -> vec4<f32> { return sample(in.uv); }";
    let err = compile_sprite(src).err().expect("name clash");
    assert_eq!(err.line, Some(1), "{err}");
    assert!(err.message.contains("srgb_to_linear"), "{err}");
    assert!(err.message.contains("engine"), "{err}");
  }

  #[test]
  fn a_params_that_is_not_a_struct_is_the_games_error() {
    let src = "\nalias Params = vec4<f32>;\nfn sprite(in: SpriteIn, params: Params) -> vec4<f32> { return params; }";
    let err = compile_sprite(src).err().expect("non-struct Params");
    assert_eq!(err.line, Some(2), "{err}");
    assert!(err.message.contains("must be a struct"), "{err}");
    assert!(!err.message.contains("engine-generated"), "{err}");
  }

  #[test]
  fn a_byte_order_mark_is_ignored() {
    let src = "\u{feff}fn sprite(in: SpriteIn, params: Params) -> vec4<f32> {\n  return sample(in.uv);\n}\n";
    assert!(compile_sprite(src).is_ok());
    // Positions on the first line count from the first real character
    let broken = "\u{feff}fn sprite(in: SpriteIn, params: Params) -> vec4<f32> { return nope; }";
    let err = compile_sprite(broken).err().expect("unknown name");
    assert_eq!(err.line, Some(1), "{err}");
    assert_eq!(err.column, Some(63), "{err}");
  }

  #[test]
  fn report_once_logs_once_per_source() {
    let err = ShaderError {
      shader: "x".into(),
      line: Some(1),
      column: None,
      message: "bad".into(),
    };
    let key = source_key(ShaderKind::Sprite, "report-once-a");
    assert!(report_once(key, &err));
    assert!(!report_once(key, &err));
    assert!(report_once(
      source_key(ShaderKind::Sprite, "report-once-b"),
      &err
    ));
    assert!(report_once(
      source_key(ShaderKind::Effect, "report-once-a"),
      &err
    ));
  }

  #[test]
  fn validation_errors_point_at_the_games_line() {
    let src = "\n\nfn sprite(in: SpriteIn, params: Params) -> vec4<f32> {\n  let x: f32 = vec2<f32>(1.0);\n  return vec4<f32>(x);\n}\n";
    let err = compile_sprite(src).err().expect("type error");
    assert_eq!(err.line, Some(4), "{err}");
  }

  #[test]
  fn effects_without_params_compile_and_comments_are_ignored() {
    let src = "// struct Params { a: f32 }\nfn effect(in: EffectIn, params: Params) -> vec4<f32> { return scene(in.uv); }";
    let compiled = compile(ShaderKind::Effect, "fx.wgsl", src).expect("compiles");
    assert_eq!(compiled.layout.fields.len(), 1);
    assert_eq!(compiled.layout.fields[0].name, "unused");
  }

  #[test]
  fn unclosed_brace_points_at_the_games_last_line() {
    let src = "// header\nfn sprite(in: SpriteIn, params: Params) -> vec4<f32> {\n  return sample(in.uv);\n\n";
    let err = compile_sprite(src).err().expect("unclosed");
    assert_eq!(err.line, Some(3), "{err}");
    assert!(err.message.starts_with("unexpected end of shader"), "{err}");
    assert!(!err.message.contains("engine-generated"), "{err}");

    let src = "struct Params { a: f32 }\nfn sprite(in: SpriteIn, params: Params) -> vec4<f32> {\n  return vec4<f32>(max(params.a, 1.0);\n}";
    let err = compile_sprite(src).err().expect("unclosed paren");
    assert_eq!(err.line, Some(3), "{err}");
  }

  #[test]
  fn params_extra_is_not_params() {
    let src = "struct ParamsExtra { z: f32 }\nstruct Params { a: f32 }\n\
      fn sprite(in: SpriteIn, params: Params) -> vec4<f32> { return vec4<f32>(params.a); }";
    let layout = compile_sprite(src).expect("compiles").layout;
    assert_eq!(layout.fields.len(), 1);
    assert_eq!(layout.fields[0].name, "a");

    let src = "struct ParamsExtra { z: f32 }\n\
      fn sprite(in: SpriteIn, params: Params) -> vec4<f32> { return sample(in.uv); }";
    let layout = compile_sprite(src).expect("fallback used").layout;
    assert_eq!(layout.fields[0].name, "unused");
  }

  #[test]
  fn commented_params_is_not_declared() {
    let src = "/* /* struct Params {} */ */\n\
      fn sprite(in: SpriteIn, params: Params) -> vec4<f32> { return sample(in.uv); }";
    let layout = compile_sprite(src).expect("fallback used").layout;
    assert_eq!(layout.fields[0].name, "unused");
  }

  #[test]
  fn odd_whitespace_params_is_declared() {
    let src = "struct  Params{\n a:f32 }\n\
      fn sprite(in: SpriteIn, params: Params) -> vec4<f32> { return vec4<f32>(params.a); }";
    let layout = compile_sprite(src).expect("compiles").layout;
    assert_eq!(layout.fields[0].name, "a");
  }

  #[test]
  fn generated_fields_cover_every_kind() {
    let wgsl = |params: &str, body: &str| compiled_effect_or_sprite(&sprite_src(params, body)).wgsl;
    let w = wgsl("struct Params { a: f32 }", "return vec4<f32>(params.a);");
    assert!(w.contains("Params(f[0])"), "{w}");
    let w = wgsl(
      "struct Params { a: f32, b: vec2<f32> }",
      "return vec4<f32>(params.b, params.a, 1.0);",
    );
    assert!(w.contains("Params(f[0], vec2<f32>(f[2], f[3]))"), "{w}");
    let w = wgsl(
      "struct Params { a: vec3<f32>, b: f32 }",
      "return vec4<f32>(params.a, params.b);",
    );
    assert!(
      w.contains("Params(vec3<f32>(f[0], f[1], f[2]), f[3])"),
      "{w}"
    );
    let w = wgsl(
      "struct Params { a: f32, b: vec4<f32> }",
      "return params.b * params.a;",
    );
    assert!(
      w.contains("Params(f[0], vec4<f32>(f[4], f[5], f[6], f[7]))"),
      "{w}"
    );
  }

  #[test]
  fn effect_params_pack_to_sixteen_bytes() {
    let src = "struct Params { a: f32 }\n\
      fn effect(in: EffectIn, params: Params) -> vec4<f32> { return scene(in.uv) * params.a; }";
    let layout = compile(ShaderKind::Effect, "fx.wgsl", src)
      .expect("compiles")
      .layout;
    assert_eq!(uniform_size(ShaderKind::Effect, &layout), 16);
    assert_eq!(uniform_size(ShaderKind::Sprite, &layout), 32);
    let mut out = vec![0xFFu8; uniform_size(ShaderKind::Effect, &layout)];
    let unknown = pack(&layout, &ShaderParams::new().with("a", 3.0), &mut out);
    assert!(unknown.is_empty());
    assert_eq!(&out[..4], &3.0f32.to_le_bytes());
    assert!(out[4..].iter().all(|b| *b == 0));
  }
}
