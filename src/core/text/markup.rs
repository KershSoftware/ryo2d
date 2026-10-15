//! The markup syntax: `[tag]`, `[tag=value]`, `[/tag]`, `[[` for a bracket.
//! Style tags: color, size, font, b, i, outline, icon, pause; any other tag
//! is an effect. Malformed markup is drawn literally and warned once.

use std::collections::HashSet;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Mutex, OnceLock};

use crate::core::text::rich::{PauseMark, RichText, Span};
use crate::core::text::style::TextFont;
use crate::core::ui::Color;

#[derive(Debug, Clone, PartialEq)]
enum Token<'a> {
  Text(&'a str),
  /// An escaped `[`
  Bracket,
  Open {
    name: &'a str,
    value: Option<&'a str>,
    source: &'a str,
  },
  Close {
    name: &'a str,
    source: &'a str,
  },
}

/// What a token turned into
#[derive(Debug, Clone, PartialEq)]
enum Resolved {
  Text(String),
  Push(Span),
  Pop,
  Icon(String),
  Pause(f32),
}

impl RichText {
  /// Parse markup: `"Deal [color=#f44]20[/color] [icon=a_button]"`
  pub fn markup(source: &str) -> Self {
    let tokens = tokenize(source);
    let mut problems = Vec::new();
    let resolved = resolve(&tokens, &mut problems);
    if let Some(problem) = problems.first() {
      warn_once(source, problem);
    }
    build(&resolved)
  }

  /// Hold the typewriter reveal here for `seconds`
  pub fn pause(mut self, seconds: f32) -> Self {
    if seconds.is_finite() && seconds > 0.0 {
      self.pauses.push(PauseMark {
        at: self.text.len(),
        seconds,
      });
    }
    self
  }
}

fn tokenize(source: &str) -> Vec<Token<'_>> {
  let mut tokens = Vec::new();
  let mut rest = source;
  while !rest.is_empty() {
    if let Some(after) = rest.strip_prefix("[[") {
      tokens.push(Token::Bracket);
      rest = after;
      continue;
    }
    if rest.starts_with('[') {
      if let Some(end) = rest.find(']') {
        let source = &rest[..=end];
        let inner = &rest[1..end];
        if let Some(name) = inner.strip_prefix('/') {
          tokens.push(Token::Close { name, source });
        } else {
          let (name, value) = match inner.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None => (inner, None),
          };
          tokens.push(Token::Open {
            name,
            value,
            source,
          });
        }
        rest = &rest[end + 1..];
        continue;
      }
      // A bracket with no end: the rest is text
      tokens.push(Token::Text(rest));
      break;
    }
    let next = rest.find('[').unwrap_or(rest.len());
    tokens.push(Token::Text(&rest[..next]));
    rest = &rest[next..];
  }
  tokens
}

/// What an open tag means, before it is matched with a close
enum Open {
  /// Drawn as written
  Literal(&'static str),
  /// Applies a span until its close
  Span(Span),
  Icon(String),
  Pause(f32),
}

fn classify(name: &str, value: Option<&str>, source: &str) -> Open {
  let valid = !name.is_empty()
    && name
      .chars()
      .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
  if !valid {
    return Open::Literal("bad tag");
  }
  let bad = Open::Literal("bad value");
  match name {
    "icon" => match value {
      Some(v) if !v.is_empty() => Open::Icon(v.to_string()),
      _ => bad,
    },
    "pause" => match value.and_then(parse_positive) {
      Some(seconds) => Open::Pause(seconds),
      None => bad,
    },
    "color" => match value.and_then(parse_color) {
      Some(color) => Open::Span(Span::color(color)),
      None => bad,
    },
    "size" => match value.and_then(parse_positive) {
      Some(size) => Open::Span(Span::size(size)),
      None => bad,
    },
    "font" => match value {
      Some(v) if !v.is_empty() => Open::Span(Span::font(TextFont::Family(v.to_string()))),
      _ => bad,
    },
    "b" => match value {
      None => Open::Span(Span::bold()),
      Some(_) => bad,
    },
    "i" => match value {
      None => Open::Span(Span::italic()),
      Some(_) => bad,
    },
    "outline" => {
      let parsed = value.and_then(|v| {
        let (width, color) = v.split_once(',')?;
        Some((parse_positive(width)?, parse_color(color.trim())?))
      });
      match parsed {
        Some((width, color)) => Open::Span(Span::outline(width, color)),
        None => bad,
      }
    }
    // Any other tag names an effect, `name=value` kept whole
    _ => Open::Span(Span::effect(&source[1..source.len() - 1])),
  }
}

fn parse_positive(value: &str) -> Option<f32> {
  value
    .trim()
    .parse::<f32>()
    .ok()
    .filter(|v| v.is_finite() && *v > 0.0)
}

fn resolve(tokens: &[Token<'_>], problems: &mut Vec<&'static str>) -> Vec<Resolved> {
  let opens: Vec<Option<Open>> = tokens
    .iter()
    .map(|token| match token {
      Token::Open {
        name,
        value,
        source,
      } => Some(classify(name, *value, source)),
      _ => None,
    })
    .collect();

  // Pre-pass: match closes with opens. `pops[i]` is how many spans token
  // `i` (a close) ends; `literal[i]` marks tokens drawn as written.
  let mut pops = vec![0usize; tokens.len()];
  let mut literal = vec![false; tokens.len()];
  let mut stack: Vec<(&str, usize)> = Vec::new();
  for (i, token) in tokens.iter().enumerate() {
    match token {
      Token::Open { name, .. } => match &opens[i] {
        Some(Open::Span(_)) => stack.push((name, i)),
        Some(Open::Literal(problem)) => {
          literal[i] = true;
          problems.push(problem);
        }
        _ => {}
      },
      Token::Close { name, .. } => match stack.iter().rposition(|(n, _)| n == name) {
        Some(pos) => {
          let above = stack.len() - pos - 1;
          if above > 0 {
            problems.push("mis-nested tags");
          }
          pops[i] = above + 1;
          stack.truncate(pos);
        }
        None => {
          literal[i] = true;
          problems.push("close without open");
        }
      },
      _ => {}
    }
  }
  for (_, i) in stack {
    literal[i] = true;
    problems.push("unclosed tag");
  }

  let mut out = Vec::new();
  for (i, token) in tokens.iter().enumerate() {
    match token {
      Token::Text(t) => out.push(Resolved::Text((*t).to_string())),
      Token::Bracket => out.push(Resolved::Text("[".to_string())),
      Token::Open { source, .. } | Token::Close { source, .. } if literal[i] => {
        out.push(Resolved::Text((*source).to_string()));
      }
      Token::Open { .. } => match opens[i].as_ref() {
        Some(Open::Span(span)) => out.push(Resolved::Push(span.clone())),
        Some(Open::Icon(name)) => out.push(Resolved::Icon(name.clone())),
        Some(Open::Pause(seconds)) => out.push(Resolved::Pause(*seconds)),
        _ => {}
      },
      Token::Close { .. } => out.extend(std::iter::repeat_n(Resolved::Pop, pops[i])),
    }
  }
  out
}

fn build(resolved: &[Resolved]) -> RichText {
  let mut rich = RichText::default();
  let base = Span::default();
  let mut stack = vec![Span::default()];
  for item in resolved {
    let top = stack.last().unwrap_or(&base).clone();
    match item {
      Resolved::Push(span) => stack.push(span.over(&top)),
      Resolved::Pop => {
        if stack.len() > 1 {
          stack.pop();
        }
      }
      Resolved::Text(t) if t.is_empty() => {}
      Resolved::Text(t) if top == base => rich = rich.plain(t),
      Resolved::Text(t) => {
        let start = rich.text.len();
        match rich.spans.last_mut() {
          Some(mark) if mark.range.end == start && mark.span == top => {
            mark.range.end = start + t.len();
            rich.text.push_str(t);
          }
          _ => rich = rich.span(t, top),
        }
      }
      Resolved::Icon(name) => rich = rich.icon_named(name.clone()),
      Resolved::Pause(seconds) => rich = rich.pause(*seconds),
    }
  }
  rich
}

fn parse_color(value: &str) -> Option<Color> {
  let named = match value.to_ascii_lowercase().as_str() {
    "white" => Some(Color::WHITE),
    "black" => Some(Color::BLACK),
    "red" => Some(Color::rgb(255, 0, 0)),
    "green" => Some(Color::rgb(0, 255, 0)),
    "blue" => Some(Color::rgb(0, 0, 255)),
    "yellow" => Some(Color::YELLOW),
    "cyan" => Some(Color::CYAN),
    "magenta" => Some(Color::MAGENTA),
    "gray" | "grey" => Some(Color::GRAY),
    "orange" => Some(Color::rgb(255, 165, 0)),
    "purple" => Some(Color::rgb(128, 0, 128)),
    "pink" => Some(Color::rgb(255, 105, 180)),
    _ => None,
  };
  named.or_else(|| {
    let hex = value.strip_prefix('#')?;
    let digit = |i: usize| u8::from_str_radix(hex.get(i..i + 1)?, 16).ok();
    let pair = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
    match hex.len() {
      3 => Some(Color::rgb(digit(0)? * 17, digit(1)? * 17, digit(2)? * 17)),
      6 => Some(Color::rgb(pair(0)?, pair(2)?, pair(4)?)),
      8 => Some(Color::new(pair(0)?, pair(2)?, pair(4)?, pair(6)?)),
      _ => None,
    }
  })
}

/// Warn once per markup string (a global set, since parsing is a free
/// function)
fn warn_once(source: &str, problem: &str) {
  static WARNED: OnceLock<Mutex<HashSet<u64>>> = OnceLock::new();
  let mut hasher = DefaultHasher::new();
  source.hash(&mut hasher);
  let key = hasher.finish();
  let warned = WARNED.get_or_init(|| Mutex::new(HashSet::new()));
  // A poisoned mutex means no more warnings, which is acceptable
  let first = warned
    .lock()
    .map(|mut set| set.insert(key))
    .unwrap_or(false);
  if first {
    crate::engine_warn!(module: "text", "markup {source:?}: {problem}; drawn as written");
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::text::Span;
  use crate::core::ui::Color;

  #[test]
  fn plain_text_passes_through() {
    let rich = RichText::markup("hello world");
    assert_eq!(rich.as_str(), "hello world");
    assert!(rich.spans.is_empty());
  }

  #[test]
  fn a_colour_tag_makes_a_span() {
    let rich = RichText::markup("Deal [color=#ff4444]20[/color] damage");
    assert_eq!(rich.as_str(), "Deal 20 damage");
    assert_eq!(rich.spans.len(), 1);
    assert_eq!(rich.spans[0].range, 5..7);
    assert_eq!(rich.spans[0].span.color, Some(Color::rgb(255, 68, 68)));
  }

  #[test]
  fn nested_tags_merge_into_flat_spans() {
    let rich = RichText::markup("[b]bold [color=red]both[/color][/b] plain");
    assert_eq!(rich.as_str(), "bold both plain");
    let spans: Vec<(std::ops::Range<usize>, Span)> = rich
      .spans
      .iter()
      .map(|m| (m.range.clone(), m.span.clone()))
      .collect();
    assert_eq!(spans.len(), 2);
    assert_eq!(spans[0].0, 0..5);
    assert_eq!(spans[0].1.weight, Some(700.0));
    assert_eq!(spans[1].0, 5..9);
    assert_eq!(spans[1].1.weight, Some(700.0));
    assert_eq!(spans[1].1.color, Some(Color::rgb(255, 0, 0)));
  }

  #[test]
  fn icons_pauses_and_escapes() {
    let rich = RichText::markup("Press [icon=a_button] then[pause=0.5] wait [[not a tag]");
    assert_eq!(rich.as_str(), "Press  then wait [not a tag]");
    assert_eq!(rich.icons.len(), 1);
    assert_eq!(rich.icons[0].at, 6);
    assert_eq!(rich.pauses.len(), 1);
    assert_eq!(rich.pauses[0].seconds, 0.5);
  }

  #[test]
  fn unknown_tags_are_effects() {
    let rich = RichText::markup("[wave]hi[/wave] [sparkle=3]yo[/sparkle]");
    assert_eq!(rich.as_str(), "hi yo");
    assert_eq!(rich.spans[0].span.effect.as_deref(), Some("wave"));
    assert_eq!(rich.spans[1].span.effect.as_deref(), Some("sparkle=3"));
  }

  #[test]
  fn malformed_markup_renders_literally_and_never_panics() {
    for source in [
      "[color=red]never closed",
      "closed [/b] without open",
      "[size=huge]bad value[/size]",
      "dangling [ bracket",
      "[]",
      "[=x]y",
      "[color=]x[/color]",
      "\u{1F600}[b]\u{1F600}[/b]",
    ] {
      let rich = RichText::markup(source);
      assert!(!rich.as_str().is_empty() || source.is_empty(), "{source}");
    }
    let rich = RichText::markup("[color=red]never closed");
    assert_eq!(rich.as_str(), "[color=red]never closed");
    let rich = RichText::markup("[size=huge]bad value[/size]");
    assert_eq!(rich.as_str(), "[size=huge]bad value[/size]");
  }

  #[test]
  fn each_bad_markup_string_warns_once() {
    let source = "[color=red]warn me once";
    let _ = RichText::markup(source);
    let _ = RichText::markup(source);
    let count = crate::core::dev_log::get_entries()
      .iter()
      .filter(|e| e.message.contains("warn me once"))
      .count();
    assert_eq!(count, 1);
  }
}
