//! Game shaders as one renderer keeps them: an entry per shader, by id, for
//! the version it was last prepared at, pointing at that version's source,
//! compiled once for every shader with the same text.
//!
//! - A shader that is not loaded (not yet, failed, or freed) has no entry and
//!   is drawn as one that failed to compile: a sprite plain, an effect
//!   skipped. Its load error is the asset server's to report.
//! - A compile error is reported once per shader and version, under the
//!   shader's own name.
//! - A freed shader's entry goes at once (`remove`). A source no shader uses
//!   any more drops what the renderer made from it (an effect's pipeline), and
//!   the batch drops a sprite shader's pipelines in the same frame
//!   (`in_use`). The source itself stays for a while (`prune`), so a shader
//!   of the same text added again is not compiled again.

use std::collections::{HashMap, HashSet};

use super::custom::{self, CompiledShader, ShaderError, ShaderKind};
use crate::core::asset::{AssetId, Assets};
use crate::core::ecs::{Shader, ShaderParams};

/// What the cache knows of one shader
struct Entry {
  /// The content version it was last prepared at
  version: u64,
  /// That version's source, in [`ShaderCache::sources`]
  source: u64,
}

/// One source text compiled, shared by every shader with that text
pub(crate) struct Compiled<P> {
  /// The composed WGSL and its `Params` layout, or why it failed (a failed
  /// source is not compiled again)
  pub(crate) shader: Result<CompiledShader, ShaderError>,
  /// The first shader's name, which labels what is made from it
  pub(crate) name: String,
  /// What the renderer made from it (an effect's pipeline), dropped with
  /// the last shader using it
  pub(crate) made: Option<P>,
  /// The frame it was last prepared in
  last_used: u64,
  /// How many entries point at it: while any do, it is kept
  users: usize,
}

/// Game shaders of one kind, by id, for one renderer
pub(crate) struct ShaderCache<P> {
  kind: ShaderKind,
  entries: HashMap<AssetId<Shader>, Entry>,
  /// By [`custom::source_key`]
  sources: HashMap<u64, Compiled<P>>,
  /// Parameter names already warned about, by shader and version
  warned: HashMap<(AssetId<Shader>, u64), HashSet<String>>,
}

impl<P> ShaderCache<P> {
  pub(crate) fn new(kind: ShaderKind) -> Self {
    Self {
      kind,
      entries: HashMap::new(),
      sources: HashMap::new(),
      warned: HashMap::new(),
    }
  }

  /// The source key of `id`'s current version and its compiled source,
  /// compiled on first use and when the shader's version moves on. `None`
  /// while `id` is not loaded in `shaders`
  pub(crate) fn prepare(
    &mut self,
    id: AssetId<Shader>,
    shaders: &Assets<Shader>,
    frame: u64,
  ) -> Option<(u64, &mut Compiled<P>)> {
    let version = shaders.version(id)?;
    let key = match self.entries.get(&id) {
      // This runs for every custom sprite of every frame: the source is
      // hashed only when the version moves on
      Some(entry) if entry.version == version => entry.source,
      _ => self.refresh(id, shaders.get(id)?, version),
    };
    let compiled = self.sources.get_mut(&key)?;
    compiled.last_used = frame;
    Some((key, compiled))
  }

  /// Points `id`'s entry at its `version`'s source, compiling the source if
  /// no shader has it yet. This runs once per shader and version, so a
  /// compile error is reported here, under this shader's name
  fn refresh(&mut self, id: AssetId<Shader>, shader: &Shader, version: u64) -> u64 {
    let kind = self.kind;
    let key = custom::source_key(kind, shader.source());
    let compiled = self.sources.entry(key).or_insert_with(|| Compiled {
      shader: custom::compile(kind, shader.name(), shader.source()),
      name: shader.name().to_string(),
      made: None,
      last_used: 0,
      users: 0,
    });
    if let Err(error) = &compiled.shader {
      report(shader.name(), error);
    }
    // Counted before the old source is let go, which may be this one
    compiled.users += 1;
    let entry = Entry {
      version,
      source: key,
    };
    if let Some(old) = self.entries.insert(id, entry) {
      release(&mut self.sources, old.source);
    }
    key
  }

  /// Forgets freed shader `id` at once
  pub(crate) fn remove(&mut self, id: AssetId<Shader>) {
    if let Some(entry) = self.entries.remove(&id) {
      release(&mut self.sources, entry.source);
      if !self.warned.is_empty() {
        self.warned.retain(|(shader, _), _| *shader != id);
      }
    }
  }

  /// Forgets every shader whose id or version is no longer current in
  /// `shaders`: one freed while its `Removed` went unread, or one changed
  /// and not prepared since
  pub(crate) fn sweep(&mut self, shaders: &Assets<Shader>) {
    let sources = &mut self.sources;
    self.entries.retain(|&id, entry| {
      let current = shaders.version(id) == Some(entry.version);
      if !current {
        release(sources, entry.source);
      }
      current
    });
    let entries = &self.entries;
    self.warned.retain(|(id, version), _| {
      entries
        .get(id)
        .is_some_and(|entry| entry.version == *version)
    });
  }

  /// Drops the sources no shader uses that were last prepared `ttl` or more
  /// frames ago
  pub(crate) fn prune(&mut self, frame: u64, ttl: u64) {
    self
      .sources
      .retain(|_, compiled| compiled.users > 0 || frame.saturating_sub(compiled.last_used) < ttl);
  }

  /// The compiled source under `key`, while it is kept
  pub(crate) fn source(&self, key: u64) -> Option<&Compiled<P>> {
    self.sources.get(&key)
  }

  /// Every compiled source, with its key
  pub(crate) fn sources_mut(&mut self) -> impl Iterator<Item = (u64, &mut Compiled<P>)> {
    self
      .sources
      .iter_mut()
      .map(|(&key, compiled)| (key, compiled))
  }

  /// Whether a shader uses the source under `key`
  pub(crate) fn in_use(&self, key: u64) -> bool {
    self
      .sources
      .get(&key)
      .is_some_and(|compiled| compiled.users > 0)
  }

  /// How many sources hold something the renderer made from them
  pub(crate) fn made_count(&self) -> usize {
    self
      .sources
      .values()
      .filter(|compiled| compiled.made.is_some())
      .count()
  }

  /// Puts `params` into `out` in the layout of the `Params` of `id`'s
  /// version as last prepared, and gives the bytes that layout takes. Names
  /// the shader has no field for, and values of the wrong type, are warned
  /// about once per shader version, under its name; correct names allocate
  /// nothing
  pub(crate) fn pack(
    &mut self,
    id: AssetId<Shader>,
    shaders: &Assets<Shader>,
    params: &ShaderParams,
    out: &mut [u8],
  ) -> Option<usize> {
    let entry = self.entries.get(&id)?;
    let compiled = self.sources.get(&entry.source)?;
    let layout = &compiled.shader.as_ref().ok()?.layout;
    let size = custom::uniform_size(self.kind, layout);
    let out = out.get_mut(..size)?;
    let seen = (id, entry.version);
    let warned = &mut self.warned;
    custom::pack_with(layout, params, out, |name, expected| {
      if warned.get(&seen).is_some_and(|names| names.contains(name)) {
        return;
      }
      warned.entry(seen).or_default().insert(name.to_string());
      let shader = shaders.get(id).map_or(compiled.name.as_str(), Shader::name);
      match expected {
        None => crate::engine_warn!(
          module: "Shaders",
          "{}: parameter `{}` is not a field of Params and is ignored",
          shader,
          name
        ),
        Some(kind) => crate::engine_warn!(
          module: "Shaders",
          "{}: parameter `{}` has the wrong type (Params expects {:?}) and is ignored",
          shader,
          name,
          kind
        ),
      }
    });
    Some(size)
  }
}

/// An entry lets go of the source under `key`: with no shader left using it,
/// what was made from it goes
fn release<P>(sources: &mut HashMap<u64, Compiled<P>>, key: u64) {
  if let Some(compiled) = sources.get_mut(&key) {
    compiled.users = compiled.users.saturating_sub(1);
    if compiled.users == 0 {
      compiled.made = None;
    }
  }
}

/// Logs a compile error under `name`: a source several shaders share was
/// compiled under the first one's
fn report(name: &str, error: &ShaderError) {
  let error = ShaderError {
    shader: name.to_string(),
    ..error.clone()
  };
  crate::engine_error!(module: "Shaders", "{error}");
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::asset::{Assets, Handle};
  use crate::core::dev_log;
  use crate::core::ecs::Shader;
  use crate::core::shaders::custom::ShaderKind;

  /// A sprite shader returning `colour`
  fn sprite(colour: &str) -> String {
    format!("fn sprite(in: SpriteIn, params: Params) -> vec4<f32> {{ return {colour}; }}")
  }

  const PLAIN: &str =
    "fn sprite(in: SpriteIn, params: Params) -> vec4<f32> { return sample(in.uv); }";

  /// The key `id` draws with now, if it compiles
  fn key_of(
    cache: &mut ShaderCache<()>,
    id: &Handle<Shader>,
    shaders: &Assets<Shader>,
  ) -> Option<u64> {
    let (key, compiled) = cache.prepare(id.id(), shaders, 1)?;
    compiled.shader.is_ok().then_some(key)
  }

  /// How many errors the log has under `name`
  fn errors_for(name: &str) -> usize {
    dev_log::get_entries()
      .iter()
      .filter(|entry| entry.level == dev_log::LogLevel::Error && entry.message.contains(name))
      .count()
  }

  #[test]
  fn a_reused_slot_compiles_its_new_shader_never_the_freed_one() {
    let mut shaders = Assets::<Shader>::default();
    let mut cache = ShaderCache::<()>::new(ShaderKind::Sprite);
    let old = shaders.add(Shader::from_wgsl(
      "old.wgsl",
      sprite("vec4<f32>(1.0, 0.0, 0.0, 1.0)"),
    ));
    let old_id = old.id();
    let old_key = cache.prepare(old_id, &shaders, 1).map(|(key, _)| key);
    assert!(old_key.is_some(), "the old shader compiles");
    drop(old);
    shaders.free_unheld(&mut Vec::new());
    // Its `Removed` was never read, so the cache still has its entry
    let new = shaders.add(Shader::from_wgsl(
      "new.wgsl",
      sprite("vec4<f32>(0.0, 1.0, 0.0, 1.0)"),
    ));
    assert_eq!(new.id().index(), old_id.index(), "the slot is reused");
    assert_ne!(new.id(), old_id, "in a new generation");
    assert!(
      cache.prepare(old_id, &shaders, 2).is_none(),
      "the freed id is not loaded: it draws plain"
    );
    let Some((key, compiled)) = cache.prepare(new.id(), &shaders, 2) else {
      panic!("the new shader compiles");
    };
    assert_eq!(compiled.name, "new.wgsl");
    let wgsl = compiled.shader.as_ref().map(|c| c.wgsl.as_str());
    assert!(
      wgsl.is_ok_and(|wgsl| wgsl.contains("vec4<f32>(0.0, 1.0, 0.0, 1.0)")),
      "the new source"
    );
    assert_ne!(Some(key), old_key);
  }

  #[test]
  fn shaders_of_one_source_share_it_until_the_last_goes() {
    let mut shaders = Assets::<Shader>::default();
    let mut cache = ShaderCache::<()>::new(ShaderKind::Sprite);
    let handles: Vec<_> = (0..3)
      .map(|i| shaders.add(Shader::from_wgsl(format!("shared-{i}.wgsl"), PLAIN)))
      .collect();
    let keys: Vec<_> = handles
      .iter()
      .map(|handle| key_of(&mut cache, handle, &shaders))
      .collect();
    let Some(key) = keys[0] else {
      panic!("the source compiles");
    };
    assert!(keys.iter().all(|k| *k == Some(key)), "{keys:?}");
    assert_eq!(cache.sources.len(), 1, "compiled once");
    cache.remove(handles[0].id());
    cache.remove(handles[1].id());
    assert!(cache.in_use(key), "one shader still draws with it");
    cache.remove(handles[2].id());
    assert!(!cache.in_use(key), "freed with its last shader");
    assert!(cache.entries.is_empty());
  }

  #[test]
  fn a_changed_shader_compiles_its_new_version() {
    let mut shaders = Assets::<Shader>::default();
    let mut cache = ShaderCache::<()>::new(ShaderKind::Sprite);
    let handle = shaders.add(Shader::from_wgsl("changing.wgsl", PLAIN));
    let Some(before) = key_of(&mut cache, &handle, &shaders) else {
      panic!("compiles");
    };
    let blue = sprite("vec4<f32>(0.0, 0.0, 1.0, 1.0)");
    if let Some(shader) = shaders.get_mut(&handle) {
      *shader = Shader::from_wgsl("changing.wgsl", blue);
    }
    let Some(after) = key_of(&mut cache, &handle, &shaders) else {
      panic!("the new version compiles");
    };
    assert_ne!(before, after);
    assert!(!cache.in_use(before), "the old version's source is let go");
    assert!(cache.in_use(after));
  }

  #[test]
  fn a_source_nothing_uses_is_kept_for_the_ttl_then_dropped() {
    let mut shaders = Assets::<Shader>::default();
    let mut cache = ShaderCache::<()>::new(ShaderKind::Sprite);
    let handle = shaders.add(Shader::from_wgsl("kept.wgsl", PLAIN));
    let Some(key) = key_of(&mut cache, &handle, &shaders) else {
      panic!("compiles");
    };
    cache.prune(1000, 600);
    assert!(
      cache.source(key).is_some(),
      "a source in use is never dropped"
    );
    cache.remove(handle.id());
    cache.prune(600, 600);
    assert!(
      cache.source(key).is_some(),
      "an unused source is kept a while"
    );
    cache.prune(601, 600);
    assert!(cache.source(key).is_none());
  }

  #[test]
  fn a_shader_freed_unseen_goes_at_the_next_sweep() {
    let mut shaders = Assets::<Shader>::default();
    let mut cache = ShaderCache::<()>::new(ShaderKind::Sprite);
    let kept = shaders.add(Shader::from_wgsl("swept-kept.wgsl", PLAIN));
    let freed = shaders.add(Shader::from_wgsl(
      "swept-freed.wgsl",
      sprite("vec4<f32>(1.0)"),
    ));
    let (Some(kept_key), Some(freed_key)) = (
      key_of(&mut cache, &kept, &shaders),
      key_of(&mut cache, &freed, &shaders),
    ) else {
      panic!("both compile");
    };
    drop(freed);
    shaders.free_unheld(&mut Vec::new());
    assert!(cache.in_use(freed_key), "kept until the sweep");
    cache.sweep(&shaders);
    assert!(!cache.in_use(freed_key));
    assert!(cache.in_use(kept_key), "a shader still loaded stays");
  }

  #[test]
  fn a_compile_error_is_reported_once_per_shader_version() {
    let mut shaders = Assets::<Shader>::default();
    let mut cache = ShaderCache::<()>::new(ShaderKind::Sprite);
    let broken = "fn sprite(in: SpriteIn, params: Params) -> vec4<f32> { return nope_t9; }";
    let other = "fn sprite(in: SpriteIn, params: Params) -> vec4<f32> { return also_nope_t9; }";
    let name = "t9-report-a.wgsl";
    let handle = shaders.add(Shader::from_wgsl(name, broken));
    for _ in 0..3 {
      assert_eq!(
        key_of(&mut cache, &handle, &shaders),
        None,
        "it draws plain"
      );
    }
    assert_eq!(errors_for(name), 1, "once for its version");
    // Another version, then back to the first text: each is told
    for source in [other, broken] {
      if let Some(shader) = shaders.get_mut(&handle) {
        *shader = Shader::from_wgsl(name, source);
      }
      key_of(&mut cache, &handle, &shaders);
    }
    assert_eq!(errors_for(name), 3);
    // Another shader of the same text is told under its own name
    let twin = "t9-report-b.wgsl";
    let second = shaders.add(Shader::from_wgsl(twin, broken));
    key_of(&mut cache, &second, &shaders);
    assert_eq!(errors_for(twin), 1);
    assert_eq!(errors_for(name), 3);
  }

  #[test]
  fn a_shader_that_is_not_loaded_has_no_entry() {
    let mut shaders = Assets::<Shader>::default();
    let mut cache = ShaderCache::<()>::new(ShaderKind::Sprite);
    let handle = shaders.add(Shader::from_wgsl("t9-unloaded.wgsl", PLAIN));
    let id = handle.id();
    drop(handle);
    shaders.free_unheld(&mut Vec::new());
    assert!(cache.prepare(id, &shaders, 1).is_none());
    assert!(cache.entries.is_empty() && cache.sources.is_empty());
  }
}
