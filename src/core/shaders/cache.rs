//! Game shaders as one renderer keeps them: an entry per shader, by id, for
//! the version it draws, pointing at that version's source, compiled once
//! for every shader with the same text.
//!
//! - A shader that is not loaded (not yet, failed, or freed) has no entry and
//!   is drawn as one that failed to compile: a sprite plain, an effect
//!   skipped. Its load error is the asset server's to report.
//! - A shader drawn with a pipeline the GPU accepted keeps drawing it when
//!   its version moves on: the new version waits (`Entry::next`) while the
//!   renderer makes its pipelines on the shader thread (`compile.rs`), and
//!   takes over once they are made (`advance`, at the start of a frame).
//!   The renderer sends one version at a time (`wanted`); one saved over
//!   while it compiles, or whose shader is freed, is dropped when it comes
//!   back (`landed`). A shader with no such pipeline compiles at once, as
//!   does every shader on the web (no threads).
//! - A version that does not compile, or whose pipeline the GPU rejects,
//!   draws the shader's last version the GPU accepted, kept until a newer
//!   one is accepted; with none, it draws as one that is not loaded. It is
//!   warned about once per shader and version, as the asset server warns
//!   about a file that does not load (`could not load "name": line:column:
//!   message`).
//! - A freed shader's entry goes at once (`remove`). A source no shader uses
//!   or waits for drops what the renderer made from it (an effect's
//!   pipeline), and the batch drops a sprite shader's pipelines in the same
//!   frame (`in_use`). The source itself stays for a while (`prune`), so a
//!   shader of the same text added again is not compiled again.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::compile::OFF_THREAD;
use super::custom::{self, CompiledShader, ShaderError, ShaderKind};
use crate::core::asset::{AssetId, Assets};
use crate::core::ecs::{Shader, ShaderParams};

/// What the cache knows of one shader
struct Entry {
  /// The content version of `source`
  version: u64,
  /// That version's source, in [`ShaderCache::sources`]
  source: u64,
  /// The shader's last source the GPU accepted, drawn while `source` failed
  /// (it does not compile, or the GPU rejected it); held until `source` is
  /// accepted
  fallback: Option<u64>,
  /// The source it draws with: `source`, or `fallback` while `source` failed
  drawn: u64,
  /// `source` failed and that is not warned about yet
  unwarned: bool,
  /// A newer version whose pipelines are being made off the main thread,
  /// while `drawn` draws
  next: Option<Next>,
}

impl Entry {
  /// Draw `fallback` (or nothing) in place of `source`, which failed
  fn fall_back(&mut self) {
    self.drawn = self.fallback.unwrap_or(self.source);
    self.unwarned = true;
  }
}

/// A shader's newer version, waiting for its pipelines
struct Next {
  version: u64,
  /// Its source key
  key: u64,
  /// The value, which the job making its pipelines compiles
  shader: Shader,
}

/// One source text compiled, shared by every shader with that text
pub(crate) struct Compiled<P> {
  /// The composed WGSL and its `Params` layout, or why it failed (a failed
  /// source is not compiled again)
  pub(crate) shader: Result<Arc<CompiledShader>, ShaderError>,
  /// The first shader's name, which labels what is made from it
  pub(crate) name: String,
  /// What the renderer made from it (an effect's pipeline), dropped with
  /// the last shader using it
  pub(crate) made: Option<P>,
  /// The frame it was last prepared in
  last_used: u64,
  /// How many entries point at it: while any do, it is kept
  users: usize,
  /// The GPU accepted a pipeline made from it
  accepted: bool,
  /// Why the GPU rejected a pipeline made from it: it is not drawn again
  rejected: Option<String>,
}

impl<P> Compiled<P> {
  /// A source as `shader` compiles, nothing made from it yet
  fn new(shader: Result<Arc<CompiledShader>, ShaderError>, name: &str) -> Self {
    Self {
      shader,
      name: name.to_string(),
      made: None,
      last_used: 0,
      users: 0,
      accepted: false,
      rejected: None,
    }
  }

  /// Whether it cannot be drawn: it does not compile, or the GPU rejected it
  pub(crate) fn failed(&self) -> bool {
    self.shader.is_err() || self.rejected.is_some()
  }

  /// Why it cannot be drawn: its compile error, or the GPU's rejection
  fn failure(&self) -> Option<ShaderError> {
    match (&self.shader, &self.rejected) {
      (Err(error), _) => Some(error.clone()),
      (Ok(_), Some(message)) => Some(ShaderError {
        shader: self.name.clone(),
        line: None,
        column: None,
        message: message.clone(),
      }),
      (Ok(_), None) => None,
    }
  }

  /// Whether a shader can fall back to it
  fn good(&self) -> bool {
    self.accepted && self.rejected.is_none() && self.shader.is_ok()
  }
}

/// A newer version a shader waits for (from [`ShaderCache::wanted`])
pub(crate) struct Wanted<'a, P> {
  /// Its source key
  pub(crate) key: u64,
  /// The source the shader draws meanwhile: the new one is made for what
  /// was made for this one
  pub(crate) drawn: u64,
  /// The value, to compile
  pub(crate) shader: &'a Shader,
  /// The source, when a shader of its text has it compiled already
  pub(crate) compiled: Option<&'a Compiled<P>>,
}

/// Game shaders of one kind, by id, for one renderer
pub(crate) struct ShaderCache<P> {
  kind: ShaderKind,
  entries: HashMap<AssetId<Shader>, Entry>,
  /// By [`custom::source_key`]
  sources: HashMap<u64, Compiled<P>>,
  /// Parameter names already warned about, by shader and version
  warned: HashMap<(AssetId<Shader>, u64), HashSet<String>>,
  /// How many entries wait for each source key (`Entry::next`)
  waiting: HashMap<u64, usize>,
  /// Whether a shader drawn with a pipeline the GPU accepted waits for its
  /// new version's pipelines, made off the main thread, rather than
  /// compiling it at once
  off_thread: bool,
}

impl<P> ShaderCache<P> {
  pub(crate) fn new(kind: ShaderKind) -> Self {
    Self::with_threads(kind, OFF_THREAD)
  }

  /// A cache whose new versions compile off the main thread, or at once
  pub(crate) fn with_threads(kind: ShaderKind, off_thread: bool) -> Self {
    Self {
      kind,
      entries: HashMap::new(),
      sources: HashMap::new(),
      warned: HashMap::new(),
      waiting: HashMap::new(),
      off_thread,
    }
  }

  /// The source key `id` draws with now and its compiled source: its
  /// current version's, compiled on first use and when the shader's version
  /// moves on, or, while that one has failed, the last one the GPU accepted
  /// (the failure warned about once per version). A shader drawn with a
  /// pipeline the GPU accepted keeps drawing it while its newer version
  /// waits for its pipelines. `None` while `id` is not loaded in `shaders`
  pub(crate) fn prepare(
    &mut self,
    id: AssetId<Shader>,
    shaders: &Assets<Shader>,
    frame: u64,
  ) -> Option<(u64, &mut Compiled<P>)> {
    let version = shaders.version(id)?;
    // This runs for every custom sprite of every frame: the source is
    // hashed only when the version moves on
    let entry = match self.entries.get_mut(&id) {
      Some(entry)
        if entry.version == version
          || entry
            .next
            .as_ref()
            .is_some_and(|next| next.version == version) =>
      {
        entry
      }
      _ => {
        self.update(id, shaders.get(id)?, version);
        self.entries.get_mut(&id)?
      }
    };
    if entry.unwarned {
      entry.unwarned = false;
      if let Some(failure) = self.sources.get(&entry.source).and_then(Compiled::failure) {
        let name = shaders
          .get(id)
          .map_or(failure.shader.as_str(), Shader::name);
        warn_failed(name, &failure);
      }
    }
    let key = entry.drawn;
    let compiled = self.sources.get_mut(&key)?;
    compiled.last_used = frame;
    Some((key, compiled))
  }

  /// Takes `id` to `shader`'s `version`: a shader drawing a source the GPU
  /// accepted waits for it (its pipelines made off the main thread), unless
  /// it is the text it has; any other compiles at once
  fn update(&mut self, id: AssetId<Shader>, shader: &Shader, version: u64) {
    let key = custom::source_key(self.kind, shader.source());
    let waits = self.off_thread
      && self.entries.get(&id).is_some_and(|entry| {
        key != entry.source
          && key != entry.drawn
          && self.sources.get(&entry.drawn).is_some_and(Compiled::good)
      });
    if waits {
      let next = Next {
        version,
        key,
        shader: shader.clone(),
      };
      if let Some(entry) = self.entries.get_mut(&id)
        && let Some(old) = entry.next.replace(next)
      {
        unwait(&mut self.waiting, old.key);
      }
      *self.waiting.entry(key).or_default() += 1;
    } else {
      self.stop_waiting(id);
      self.refresh(id, shader, version);
    }
  }

  /// `id` no longer waits for a newer version
  fn stop_waiting(&mut self, id: AssetId<Shader>) {
    if let Some(next) = self
      .entries
      .get_mut(&id)
      .and_then(|entry| entry.next.take())
    {
      unwait(&mut self.waiting, next.key);
    }
  }

  /// Points `id`'s entry at its `version`'s source, compiling the source if
  /// no shader has it yet, and keeps the shader's last source the GPU
  /// accepted to fall back to until this one is accepted
  fn refresh(&mut self, id: AssetId<Shader>, shader: &Shader, version: u64) {
    let kind = self.kind;
    let key = custom::source_key(kind, shader.source());
    let compiled = self
      .sources
      .entry(key)
      .or_insert_with(|| Compiled::new(shader.compiled(kind).clone(), shader.name()));
    // Counted before the old sources are let go, which may be this one
    compiled.users += 1;
    let (settled, failed) = (compiled.good(), compiled.failed());
    let old = self.entries.remove(&id);
    let fallback = old
      .as_ref()
      .and_then(|old| {
        let was_good = self.sources.get(&old.source).is_some_and(Compiled::good);
        if was_good {
          Some(old.source)
        } else {
          old.fallback
        }
      })
      .filter(|&fallback| fallback != key && !settled);
    if let Some(compiled) = fallback.and_then(|fallback| self.sources.get_mut(&fallback)) {
      compiled.users += 1;
    }
    let mut entry = Entry {
      version,
      source: key,
      fallback,
      drawn: key,
      unwarned: false,
      next: None,
    };
    if failed {
      entry.fall_back();
    }
    self.entries.insert(id, entry);
    if let Some(old) = old {
      release_entry(&mut self.sources, &mut self.waiting, &old);
    }
  }

  /// The newer versions shaders wait for, one per waiting shader: the
  /// renderer makes their pipelines off the main thread, one at a time
  pub(crate) fn wanted(&self) -> impl Iterator<Item = Wanted<'_, P>> {
    self.entries.values().filter_map(|entry| {
      let next = entry.next.as_ref()?;
      Some(Wanted {
        key: next.key,
        drawn: entry.drawn,
        shader: &next.shader,
        compiled: self.sources.get(&next.key),
      })
    })
  }

  /// Whether any shader waits for a newer version's pipelines
  pub(crate) fn waiting(&self) -> bool {
    !self.waiting.is_empty()
  }

  /// A source made off the main thread came back: `compiled`, what the
  /// renderer made from it (`made`, an effect's pipeline; the batch keeps a
  /// sprite shader's own), and whether the GPU accepted that (`outcome`).
  /// Kept only while a shader uses or waits for it: a version saved over
  /// while it compiled, or whose shader was freed, is dropped (`false`)
  pub(crate) fn landed(
    &mut self,
    key: u64,
    name: &str,
    compiled: Result<Arc<CompiledShader>, ShaderError>,
    made: Option<P>,
    outcome: Result<(), String>,
  ) -> bool {
    if !self.in_use(key) {
      return false;
    }
    let source = self
      .sources
      .entry(key)
      .or_insert_with(|| Compiled::new(compiled, name));
    if source.made.is_none() {
      source.made = made;
    }
    match outcome {
      Ok(()) if source.shader.is_ok() => self.accepted(key),
      Ok(()) => {}
      Err(message) => self.rejected(key, message),
    }
    true
  }

  /// The job compiling the source under `key` panicked: the source fails,
  /// so the shaders waiting for it draw their last good version (warned
  /// once) rather than wait on, or send it again
  pub(crate) fn panicked(&mut self, key: u64) {
    let failure = ShaderError {
      shader: String::new(),
      line: None,
      column: None,
      message: "compiling it panicked".to_string(),
    };
    self.landed(key, "", Err(failure), None, Ok(()));
  }

  /// Takes each shader waiting for a newer version to it once `ready` says
  /// what it is drawn with is made (given the new source's key, the source
  /// and the key of the one drawn meanwhile), or once it failed, so it
  /// draws its last good version as any failed version does. Call at the
  /// start of a frame, before anything is drawn
  pub(crate) fn advance(&mut self, mut ready: impl FnMut(u64, &Compiled<P>, u64) -> bool) {
    if self.waiting.is_empty() {
      return;
    }
    let sources = &self.sources;
    let moving: Vec<AssetId<Shader>> = self
      .entries
      .iter()
      .filter(|(_, entry)| {
        entry.next.as_ref().is_some_and(|next| {
          sources
            .get(&next.key)
            .is_some_and(|source| source.failed() || ready(next.key, source, entry.drawn))
        })
      })
      .map(|(&id, _)| id)
      .collect();
    for id in moving {
      let Some(next) = self
        .entries
        .get_mut(&id)
        .and_then(|entry| entry.next.take())
      else {
        continue;
      };
      unwait(&mut self.waiting, next.key);
      self.refresh(id, &next.shader, next.version);
    }
  }

  /// The GPU accepted a pipeline made from the source under `key`: the
  /// shaders drawing it let go of what they fell back to
  pub(crate) fn accepted(&mut self, key: u64) {
    match self.sources.get_mut(&key) {
      Some(compiled) if !compiled.accepted => compiled.accepted = true,
      _ => return,
    }
    for entry in self.entries.values_mut() {
      if entry.source == key
        && entry.drawn == key
        && let Some(fallback) = entry.fallback.take()
      {
        release(&mut self.sources, fallback);
      }
    }
  }

  /// The GPU rejected a pipeline made from the source under `key`, saying
  /// `message`: the shaders drawing it draw what they fall back to, and say
  /// why once
  pub(crate) fn rejected(&mut self, key: u64, message: String) {
    match self.sources.get_mut(&key) {
      Some(compiled) if compiled.rejected.is_none() => compiled.rejected = Some(message),
      _ => return,
    }
    for entry in self.entries.values_mut() {
      if entry.source == key {
        entry.fall_back();
      }
    }
  }

  /// Forgets freed shader `id` at once, and any newer version it waits for
  pub(crate) fn remove(&mut self, id: AssetId<Shader>) {
    if let Some(entry) = self.entries.remove(&id) {
      release_entry(&mut self.sources, &mut self.waiting, &entry);
      if !self.warned.is_empty() {
        self.warned.retain(|(shader, _), _| *shader != id);
      }
    }
  }

  /// Forgets every shader no longer loaded in `shaders`: one freed while
  /// its `Removed` went unread. A shader changed and not drawn since keeps
  /// its entry, so its last good version draws until its new one is made
  pub(crate) fn sweep(&mut self, shaders: &Assets<Shader>) {
    let (sources, waiting) = (&mut self.sources, &mut self.waiting);
    self.entries.retain(|&id, entry| {
      let loaded = shaders.version(id).is_some();
      if !loaded {
        release_entry(sources, waiting, entry);
      }
      loaded
    });
    let entries = &self.entries;
    self.warned.retain(|(id, version), _| {
      entries
        .get(id)
        .is_some_and(|entry| entry.version == *version)
    });
  }

  /// Drops the sources no shader uses or waits for that were last prepared
  /// `ttl` or more frames ago
  pub(crate) fn prune(&mut self, frame: u64, ttl: u64) {
    let waiting = &self.waiting;
    self.sources.retain(|key, compiled| {
      compiled.users > 0
        || waiting.contains_key(key)
        || frame.saturating_sub(compiled.last_used) < ttl
    });
  }

  /// The compiled source under `key`, while it is kept
  pub(crate) fn source(&self, key: u64) -> Option<&Compiled<P>> {
    self.sources.get(&key)
  }

  /// Settles what was made from each source with `settle`: `Some(Ok(()))`
  /// once the GPU accepted it, `Some(Err(message))` once it rejected it,
  /// `None` while its check is out (or there was none)
  pub(crate) fn settle(&mut self, mut settle: impl FnMut(&mut P) -> Option<Result<(), String>>) {
    let mut settled = Vec::new();
    for (&key, compiled) in &mut self.sources {
      if let Some(outcome) = compiled.made.as_mut().and_then(&mut settle) {
        settled.push((key, outcome));
      }
    }
    for (key, outcome) in settled {
      match outcome {
        Ok(()) => self.accepted(key),
        Err(message) => self.rejected(key, message),
      }
    }
  }

  /// Whether shader `id`, which this renderer draws, is not yet drawn at
  /// its current version in `shaders` by a pipeline the GPU accepted: a new
  /// version waits, or its pipeline's check is out. `false` for a shader
  /// this renderer has not drawn
  pub(crate) fn pending(&self, id: AssetId<Shader>, shaders: &Assets<Shader>) -> bool {
    let (Some(version), Some(entry)) = (shaders.version(id), self.entries.get(&id)) else {
      return false;
    };
    entry.next.is_some()
      || entry.version != version
      || !self.sources.get(&entry.drawn).is_some_and(Compiled::good)
  }

  /// Whether a shader uses the source under `key`, or waits for it
  pub(crate) fn in_use(&self, key: u64) -> bool {
    self.waiting.contains_key(&key)
      || self
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

  /// Puts `params` into `out` in the layout of the `Params` of the source
  /// `id` draws with (see [`prepare`](Self::prepare)), and gives the bytes
  /// that layout takes. Names the shader has no field for, and values of
  /// the wrong type, are warned about once per shader version, under its
  /// name; correct names allocate nothing
  pub(crate) fn pack(
    &mut self,
    id: AssetId<Shader>,
    shaders: &Assets<Shader>,
    params: &ShaderParams,
    out: &mut [u8],
  ) -> Option<usize> {
    let entry = self.entries.get(&id)?;
    let compiled = self.sources.get(&entry.drawn)?;
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

/// One fewer shader waits for the source under `key`
fn unwait(waiting: &mut HashMap<u64, usize>, key: u64) {
  if let Some(count) = waiting.get_mut(&key) {
    *count -= 1;
    if *count == 0 {
      waiting.remove(&key);
    }
  }
}

/// `entry` lets go of its source, what it falls back to, and the newer
/// version it waits for
fn release_entry<P>(
  sources: &mut HashMap<u64, Compiled<P>>,
  waiting: &mut HashMap<u64, usize>,
  entry: &Entry,
) {
  release(sources, entry.source);
  if let Some(fallback) = entry.fallback {
    release(sources, fallback);
  }
  if let Some(next) = &entry.next {
    unwait(waiting, next.key);
  }
}

/// Warns that shader `name` failed, as the asset server warns about a file
/// that does not load: its name once, then where in its source
fn warn_failed(name: &str, failure: &ShaderError) {
  crate::engine_warn!(
    module: "asset",
    "could not load \"{name}\": {}",
    failure.located()
  );
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

  /// How many warnings the log has about `name`, as the asset server
  /// warns about a file that does not load
  fn errors_for(name: &str) -> usize {
    let warned = format!("could not load \"{name}\": ");
    dev_log::get_entries()
      .iter()
      .filter(|entry| {
        entry.level == dev_log::LogLevel::Warn
          && entry.module.as_deref() == Some("asset")
          && entry.message.starts_with(&warned)
      })
      .count()
  }

  #[test]
  fn a_reused_slot_compiles_its_new_shader_never_the_freed_one() {
    let mut shaders = Assets::<Shader>::new();
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
    let mut shaders = Assets::<Shader>::new();
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
    let mut shaders = Assets::<Shader>::new();
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
    let mut shaders = Assets::<Shader>::new();
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
    let mut shaders = Assets::<Shader>::new();
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
    let mut shaders = Assets::<Shader>::new();
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
  fn a_version_that_does_not_compile_draws_the_last_one_the_gpu_accepted() {
    let name = "t33-cache-fallback.wgsl";
    let mut shaders = Assets::<Shader>::new();
    // Compiled at once, as on the web
    let mut cache = ShaderCache::<()>::with_threads(ShaderKind::Sprite, false);
    let handle = shaders.add(Shader::from_wgsl(name, PLAIN));
    let Some(good) = key_of(&mut cache, &handle, &shaders) else {
      panic!("compiles");
    };
    cache.accepted(good);
    if let Some(shader) = shaders.get_mut(&handle) {
      *shader = Shader::from_wgsl(name, "fn sprite( {");
    }
    for _ in 0..3 {
      assert_eq!(
        key_of(&mut cache, &handle, &shaders),
        Some(good),
        "the last good version draws"
      );
    }
    assert!(cache.in_use(good));
    assert_eq!(errors_for(name), 1, "warned once for the version");
  }

  #[test]
  fn a_shader_with_no_version_the_gpu_accepted_has_nothing_to_fall_back_to() {
    let name = "t33-cache-unaccepted.wgsl";
    let mut shaders = Assets::<Shader>::new();
    let mut cache = ShaderCache::<()>::new(ShaderKind::Sprite);
    let handle = shaders.add(Shader::from_wgsl(name, PLAIN));
    assert!(key_of(&mut cache, &handle, &shaders).is_some(), "compiles");
    if let Some(shader) = shaders.get_mut(&handle) {
      *shader = Shader::from_wgsl(name, "fn sprite( {");
    }
    assert_eq!(key_of(&mut cache, &handle, &shaders), None, "draws plain");
  }

  #[test]
  fn the_last_good_version_goes_once_the_gpu_accepts_the_new_one() {
    let name = "t33-cache-replaced.wgsl";
    let mut shaders = Assets::<Shader>::new();
    // Compiled at once, as on the web
    let mut cache = ShaderCache::<()>::with_threads(ShaderKind::Sprite, false);
    let handle = shaders.add(Shader::from_wgsl(name, PLAIN));
    let Some(good) = key_of(&mut cache, &handle, &shaders) else {
      panic!("compiles");
    };
    cache.accepted(good);
    if let Some(shader) = shaders.get_mut(&handle) {
      *shader = Shader::from_wgsl(name, sprite("vec4<f32>(0.0, 0.0, 1.0, 1.0)"));
    }
    let Some(new) = key_of(&mut cache, &handle, &shaders) else {
      panic!("the new version compiles");
    };
    assert_ne!(new, good, "the new version draws");
    assert!(cache.in_use(good), "the last good one is kept meanwhile");
    cache.accepted(new);
    assert!(
      !cache.in_use(good),
      "and let go once the new one is accepted"
    );
    assert_eq!(key_of(&mut cache, &handle, &shaders), Some(new));
  }

  #[test]
  fn a_version_the_gpu_rejects_draws_the_last_good_one() {
    let name = "t33-cache-rejected.wgsl";
    let mut shaders = Assets::<Shader>::new();
    // Compiled at once, as on the web
    let mut cache = ShaderCache::<()>::with_threads(ShaderKind::Sprite, false);
    let handle = shaders.add(Shader::from_wgsl(name, PLAIN));
    let Some(good) = key_of(&mut cache, &handle, &shaders) else {
      panic!("compiles");
    };
    cache.accepted(good);
    if let Some(shader) = shaders.get_mut(&handle) {
      *shader = Shader::from_wgsl(name, sprite("vec4<f32>(0.0, 0.0, 1.0, 1.0)"));
    }
    let Some(new) = key_of(&mut cache, &handle, &shaders) else {
      panic!("the new version compiles");
    };
    cache.rejected(new, "no such binding".to_string());
    for _ in 0..3 {
      assert_eq!(key_of(&mut cache, &handle, &shaders), Some(good));
    }
    let warned: Vec<_> = dev_log::get_entries()
      .into_iter()
      .filter(|entry| entry.message.contains(name))
      .map(|entry| entry.message)
      .collect();
    assert_eq!(
      warned,
      [format!("could not load \"{name}\": no such binding")]
    );
  }

  /// What the shader thread makes of `source`: it compiled
  fn compiled(source: &str) -> Result<Arc<CompiledShader>, ShaderError> {
    custom::compile(ShaderKind::Sprite, "thread.wgsl", source).map(Arc::new)
  }

  /// A cache whose shader `name` draws `PLAIN`, accepted by the GPU, then
  /// changed to `source`: the cache, the store, the handle and the key drawn
  fn changed(name: &str, source: &str) -> (ShaderCache<()>, Assets<Shader>, Handle<Shader>, u64) {
    let mut shaders = Assets::<Shader>::new();
    let mut cache = ShaderCache::<()>::with_threads(ShaderKind::Sprite, true);
    let handle = shaders.add(Shader::from_wgsl(name, PLAIN));
    let Some(good) = key_of(&mut cache, &handle, &shaders) else {
      panic!("compiles");
    };
    cache.accepted(good);
    if let Some(shader) = shaders.get_mut(&handle) {
      *shader = Shader::from_wgsl(name, source);
    }
    (cache, shaders, handle, good)
  }

  #[test]
  fn a_good_shaders_new_version_waits_and_the_old_one_draws_until_it_is_made() {
    let blue = sprite("vec4<f32>(0.0, 0.0, 1.0, 1.0)");
    let (mut cache, shaders, handle, good) = changed("t14c-waits.wgsl", &blue);
    assert_eq!(
      key_of(&mut cache, &handle, &shaders),
      Some(good),
      "the old one draws"
    );
    assert!(cache.pending(handle.id(), &shaders));
    let wanted: Vec<(u64, u64)> = cache.wanted().map(|w| (w.key, w.drawn)).collect();
    let new = custom::source_key(ShaderKind::Sprite, &blue);
    assert_eq!(wanted, [(new, good)], "made to match what the old one has");
    assert!(cache.in_use(new), "what is made for it is kept");
    assert!(cache.landed(new, "t14c-waits.wgsl", compiled(&blue), None, Ok(())));
    cache.advance(|_, _, _| false);
    assert_eq!(
      key_of(&mut cache, &handle, &shaders),
      Some(good),
      "until it is made"
    );
    cache.advance(|key, _, drawn| key == new && drawn == good);
    assert_eq!(
      key_of(&mut cache, &handle, &shaders),
      Some(new),
      "then the new one"
    );
    assert!(!cache.pending(handle.id(), &shaders));
    assert!(!cache.in_use(good), "and the old one goes");
    assert!(!cache.waiting());
  }

  #[test]
  fn a_version_saved_over_while_it_compiles_is_dropped_when_it_comes_back() {
    let (blue, white) = (
      sprite("vec4<f32>(0.0, 0.0, 1.0, 1.0)"),
      sprite("vec4<f32>(1.0)"),
    );
    let name = "t14c-saved-over.wgsl";
    let (mut cache, mut shaders, handle, good) = changed(name, &blue);
    key_of(&mut cache, &handle, &shaders);
    let stale = custom::source_key(ShaderKind::Sprite, &blue);
    if let Some(shader) = shaders.get_mut(&handle) {
      *shader = Shader::from_wgsl(name, white.as_str());
    }
    assert_eq!(key_of(&mut cache, &handle, &shaders), Some(good));
    let newest = custom::source_key(ShaderKind::Sprite, &white);
    let wanted: Vec<u64> = cache.wanted().map(|w| w.key).collect();
    assert_eq!(wanted, [newest], "only the newest is wanted");
    assert!(
      !cache.landed(stale, name, compiled(&blue), None, Ok(())),
      "dropped"
    );
    assert!(cache.source(stale).is_none());
    assert!(cache.landed(newest, name, compiled(&white), None, Ok(())));
    cache.advance(|_, _, _| true);
    assert_eq!(key_of(&mut cache, &handle, &shaders), Some(newest));
  }

  #[test]
  fn a_shader_freed_while_its_new_version_compiles_takes_nothing_in() {
    let blue = sprite("vec4<f32>(0.0, 0.0, 1.0, 1.0)");
    let (mut cache, shaders, handle, good) = changed("t14c-freed-cache.wgsl", &blue);
    key_of(&mut cache, &handle, &shaders);
    let new = custom::source_key(ShaderKind::Sprite, &blue);
    cache.remove(handle.id());
    assert!(!cache.waiting() && !cache.in_use(new) && !cache.in_use(good));
    assert!(!cache.landed(new, "t14c-freed-cache.wgsl", compiled(&blue), None, Ok(())));
    assert!(cache.source(new).is_none());
  }

  #[test]
  fn a_new_version_that_does_not_compile_keeps_the_old_one_and_is_warned_about_once() {
    let name = "t14c-broken-next.wgsl";
    let broken = "fn sprite(in: SpriteIn, params: Params) -> vec4<f32> { return nope_t14c; }";
    let (mut cache, shaders, handle, good) = changed(name, broken);
    key_of(&mut cache, &handle, &shaders);
    let new = custom::source_key(ShaderKind::Sprite, broken);
    let failed = custom::compile(ShaderKind::Sprite, name, broken).map(Arc::new);
    assert!(failed.is_err());
    assert!(cache.landed(new, name, failed, None, Ok(())));
    cache.advance(|_, _, _| false);
    for _ in 0..3 {
      assert_eq!(key_of(&mut cache, &handle, &shaders), Some(good));
    }
    assert!(!cache.waiting());
    assert_eq!(errors_for(name), 1);
  }

  #[test]
  fn a_new_version_the_gpu_rejects_keeps_the_old_one() {
    let name = "t14c-rejected-next.wgsl";
    let blue = sprite("vec4<f32>(0.0, 0.0, 1.0, 1.0)");
    let (mut cache, shaders, handle, good) = changed(name, &blue);
    key_of(&mut cache, &handle, &shaders);
    let new = custom::source_key(ShaderKind::Sprite, &blue);
    assert!(cache.landed(
      new,
      name,
      compiled(&blue),
      None,
      Err("no such binding".into())
    ));
    cache.advance(|_, _, _| false);
    assert_eq!(key_of(&mut cache, &handle, &shaders), Some(good));
    assert_eq!(key_of(&mut cache, &handle, &shaders), Some(good));
    let warned: Vec<_> = dev_log::get_entries()
      .into_iter()
      .filter(|entry| entry.message.contains(name))
      .map(|entry| entry.message)
      .collect();
    assert_eq!(
      warned,
      [format!("could not load \"{name}\": no such binding")]
    );
  }

  #[test]
  fn a_compile_that_panicked_fails_its_version_and_the_old_one_draws() {
    let name = "t14c-panicked.wgsl";
    let blue = sprite("vec4<f32>(0.0, 0.0, 1.0, 1.0)");
    let (mut cache, shaders, handle, good) = changed(name, &blue);
    key_of(&mut cache, &handle, &shaders);
    cache.panicked(custom::source_key(ShaderKind::Sprite, &blue));
    cache.advance(|_, _, _| false);
    assert!(
      !cache.waiting(),
      "it does not wait on, nor is it sent again"
    );
    assert_eq!(key_of(&mut cache, &handle, &shaders), Some(good));
    assert_eq!(key_of(&mut cache, &handle, &shaders), Some(good));
    let warned: Vec<_> = dev_log::get_entries()
      .into_iter()
      .filter(|entry| entry.message.contains(name))
      .map(|entry| entry.message)
      .collect();
    assert_eq!(
      warned,
      [format!("could not load \"{name}\": compiling it panicked")]
    );
  }

  #[test]
  fn the_sweep_keeps_the_last_good_version_of_a_shader_changed_while_undrawn() {
    let blue = sprite("vec4<f32>(0.0, 0.0, 1.0, 1.0)");
    let (mut cache, shaders, handle, good) = changed("t14c-swept.wgsl", &blue);
    cache.sweep(&shaders);
    assert!(cache.in_use(good), "kept past the sweep");
    assert_eq!(
      key_of(&mut cache, &handle, &shaders),
      Some(good),
      "and drawn while the new version is made"
    );
    assert!(cache.waiting());
  }

  #[test]
  fn going_back_to_the_text_it_draws_waits_for_nothing() {
    let name = "t14c-back.wgsl";
    let blue = sprite("vec4<f32>(0.0, 0.0, 1.0, 1.0)");
    let (mut cache, mut shaders, handle, good) = changed(name, &blue);
    key_of(&mut cache, &handle, &shaders);
    assert!(cache.waiting());
    if let Some(shader) = shaders.get_mut(&handle) {
      *shader = Shader::from_wgsl(name, PLAIN);
    }
    assert_eq!(key_of(&mut cache, &handle, &shaders), Some(good));
    assert!(!cache.waiting(), "nothing to make");
    assert!(!cache.pending(handle.id(), &shaders));
  }

  #[test]
  fn a_shader_that_is_not_loaded_has_no_entry() {
    let mut shaders = Assets::<Shader>::new();
    let mut cache = ShaderCache::<()>::new(ShaderKind::Sprite);
    let handle = shaders.add(Shader::from_wgsl("t9-unloaded.wgsl", PLAIN));
    let id = handle.id();
    drop(handle);
    shaders.free_unheld(&mut Vec::new());
    assert!(cache.prepare(id, &shaders, 1).is_none());
    assert!(cache.entries.is_empty() && cache.sources.is_empty());
  }
}
