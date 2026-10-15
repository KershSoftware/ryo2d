//! Per-glyph effects and the typewriter reveal

use std::collections::{HashMap, HashSet};
use std::f32::consts::TAU;
use std::sync::{Arc, Mutex};

use glam::Vec2;

use crate::core::ecs::Resource;
use crate::core::ui::Color;

/// What an effect sees for one glyph
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct GlyphContext {
  /// The glyph's index in its text
  pub index: u32,
  /// Its character cluster (the reveal's unit)
  pub cluster: u32,
  pub line: u32,
  /// Seconds: real time in the UI, game time on world text
  pub time: f32,
  /// The text's size in text units
  pub size: f32,
  /// Stable per text, for noise
  pub seed: u64,
  /// The tag's value (`[wave=3,2]` gives `"3,2"`), empty without one
  pub args: String,
}

/// How an effect changes one glyph
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlyphMod {
  /// Text units, +y down
  pub offset: Vec2,
  /// About the glyph's centre
  pub scale: f32,
  /// Replaces the glyph's colour
  pub color: Option<Color>,
  /// Multiplies its opacity
  pub alpha: f32,
}

impl Default for GlyphMod {
  fn default() -> Self {
    Self {
      offset: Vec2::ZERO,
      scale: 1.0,
      color: None,
      alpha: 1.0,
    }
  }
}

type EffectFn = Arc<dyn Fn(&GlyphContext) -> GlyphMod + Send + Sync>;

/// The effects text can name. Built in: `wave` (`[wave=amplitude,frequency]`),
/// `shake` (`[shake=intensity]`), `rainbow` (`[rainbow=speed]`).
#[derive(Resource, Clone)]
pub struct TextEffects {
  effects: HashMap<String, EffectFn>,
  warned: Arc<Mutex<HashSet<String>>>,
}

impl std::fmt::Debug for TextEffects {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    let mut names: Vec<&String> = self.effects.keys().collect();
    names.sort();
    f.debug_struct("TextEffects")
      .field("effects", &names)
      .finish()
  }
}

impl Default for TextEffects {
  fn default() -> Self {
    let mut effects = Self {
      effects: HashMap::new(),
      warned: Arc::default(),
    };
    effects.register("wave", wave);
    effects.register("shake", shake);
    effects.register("rainbow", rainbow);
    effects
  }
}

impl TextEffects {
  /// Add or replace an effect; markup names it `[name]..[/name]`
  pub fn register(
    &mut self,
    name: impl Into<String>,
    effect: impl Fn(&GlyphContext) -> GlyphMod + Send + Sync + 'static,
  ) {
    self.effects.insert(name.into(), Arc::new(effect));
  }

  pub fn contains(&self, name: &str) -> bool {
    self.effects.contains_key(name)
  }

  /// Run `effect` (`name` or `name=args`); `None` for an unknown name,
  /// warned once per name
  pub(crate) fn apply(&self, effect: &str, ctx: &GlyphContext) -> Option<GlyphMod> {
    let (name, args) = effect.split_once('=').unwrap_or((effect, ""));
    let Some(run) = self.effects.get(name) else {
      let first = self
        .warned
        .lock()
        .map(|mut set| set.insert(name.to_string()))
        .unwrap_or(false);
      if first {
        crate::engine_warn!(module: "text", "no text effect named {name:?}; the text draws without it");
      }
      return None;
    };
    let mut ctx = ctx.clone();
    ctx.args = args.to_string();
    let result = run(&ctx);
    let finite = result.offset.is_finite() && result.scale.is_finite() && result.alpha.is_finite();
    Some(if finite { result } else { GlyphMod::default() })
  }
}

fn args<const N: usize>(ctx: &GlyphContext, defaults: [f32; N]) -> [f32; N] {
  let mut out = defaults;
  for (slot, value) in out.iter_mut().zip(ctx.args.split(',')) {
    if let Ok(v) = value.trim().parse::<f32>()
      && v.is_finite()
    {
      *slot = v;
    }
  }
  out
}

fn wave(ctx: &GlyphContext) -> GlyphMod {
  let [amplitude, frequency] = args(ctx, [ctx.size * 0.15, 2.0]);
  let phase = (ctx.time * frequency + ctx.cluster as f32 * 0.12) * TAU;
  GlyphMod {
    offset: Vec2::new(0.0, -phase.sin() * amplitude),
    ..GlyphMod::default()
  }
}

fn shake(ctx: &GlyphContext) -> GlyphMod {
  let [intensity] = args(ctx, [ctx.size * 0.06]);
  let tick = (ctx.time * 30.0).floor() as u64;
  let noise = |salt: u64| {
    let mut x =
      ctx.seed ^ (u64::from(ctx.cluster) << 32) ^ tick.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ salt;
    x ^= x >> 33;
    x = x.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    x ^= x >> 33;
    (x as f64 / u64::MAX as f64) as f32 * 2.0 - 1.0
  };
  GlyphMod {
    offset: Vec2::new(noise(1), noise(2)) * intensity,
    ..GlyphMod::default()
  }
}

fn rainbow(ctx: &GlyphContext) -> GlyphMod {
  let [speed] = args(ctx, [0.5]);
  let hue = (ctx.cluster as f32 * 0.08 + ctx.time * speed).rem_euclid(1.0);
  GlyphMod {
    color: Some(hsv(hue, 0.8, 1.0)),
    ..GlyphMod::default()
  }
}

fn hsv(h: f32, s: f32, v: f32) -> Color {
  let i = (h * 6.0).floor();
  let f = h * 6.0 - i;
  let (p, q, t) = (v * (1.0 - s), v * (1.0 - f * s), v * (1.0 - (1.0 - f) * s));
  let (r, g, b) = match i as i32 % 6 {
    0 => (v, t, p),
    1 => (q, v, p),
    2 => (p, v, t),
    3 => (p, q, v),
    4 => (t, p, v),
    _ => (v, p, q),
  };
  let byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
  Color::rgb(byte(r), byte(g), byte(b))
}

/// Opacity of `cluster` when `shown` clusters are revealed; revealed
/// glyphs fade in over `fade_clusters` clusters (0: they appear at once)
pub(crate) fn reveal_alpha(cluster: u32, shown: f32, fade_clusters: f32) -> f32 {
  let c = cluster as f32;
  if fade_clusters <= 0.0 {
    return if c + 1.0 <= shown { 1.0 } else { 0.0 };
  }
  ((shown - c) / fade_clusters).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn ctx(cluster: u32, time: f32) -> GlyphContext {
    GlyphContext {
      index: cluster,
      cluster,
      line: 0,
      time,
      size: 20.0,
      seed: 7,
      args: String::new(),
    }
  }

  #[test]
  fn wave_moves_glyphs_up_and_down_over_time() {
    let effects = TextEffects::default();
    let a = effects.apply("wave", &ctx(0, 0.0)).unwrap();
    let b = effects.apply("wave", &ctx(0, 0.125)).unwrap();
    assert_ne!(a.offset.y, b.offset.y);
    assert!(a.offset.y.abs() <= 20.0 * 0.15 + 1e-4);
  }

  #[test]
  fn wave_reads_its_arguments() {
    let effects = TextEffects::default();
    let mut c = ctx(0, 0.125);
    c.args = "5,1".into();
    let m = effects.apply("wave", &c).unwrap();
    assert!(m.offset.y.abs() <= 5.0 + 1e-4);
  }

  #[test]
  fn shake_is_bounded_and_rainbow_colours() {
    let effects = TextEffects::default();
    for cluster in 0..20 {
      let m = effects
        .apply("shake", &ctx(cluster, cluster as f32 * 0.1))
        .unwrap();
      assert!(m.offset.length() <= 20.0 * 0.06 * 1.5);
    }
    let c = effects.apply("rainbow", &ctx(3, 0.0)).unwrap();
    assert!(c.color.is_some());
  }

  #[test]
  fn a_registered_effect_runs_and_an_unknown_one_warns_once() {
    let mut effects = TextEffects::default();
    effects.register("lift", |ctx: &GlyphContext| GlyphMod {
      offset: glam::Vec2::new(0.0, -(ctx.cluster as f32)),
      ..GlyphMod::default()
    });
    assert_eq!(effects.apply("lift", &ctx(4, 0.0)).unwrap().offset.y, -4.0);
    assert!(effects.apply("nope-effect", &ctx(0, 0.0)).is_none());
    assert!(effects.apply("nope-effect", &ctx(0, 0.0)).is_none());
    let warned = crate::core::dev_log::get_entries()
      .iter()
      .filter(|e| e.message.contains("nope-effect"))
      .count();
    assert_eq!(warned, 1);
  }

  #[test]
  fn reveal_alpha_steps_or_fades() {
    assert_eq!(reveal_alpha(0, 1.0, 0.0), 1.0);
    assert_eq!(reveal_alpha(1, 1.0, 0.0), 0.0);
    assert_eq!(reveal_alpha(1, 1.5, 0.0), 0.0);
    assert!((reveal_alpha(1, 1.5, 1.0) - 0.5).abs() < 1e-6);
    assert_eq!(reveal_alpha(5, 100.0, 2.0), 1.0);
  }
}
