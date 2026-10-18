//! The reader: RON syntax into values that know their line and column
//!
//! It reads the part of RON a UI file uses and refuses the rest with a
//! message saying so. It is ours rather than serde's: a UI file's node names
//! are its meaning, and serde's self-describing values drop them.

use std::iter::Peekable;
use std::str::Chars;

/// How deep values may nest before the file is refused, so a broken or
/// hostile file cannot overflow the stack
const MAX_DEPTH: usize = 128;

/// A line and column, from 1; columns count characters
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Pos {
  pub(crate) line: u32,
  pub(crate) column: u32,
}

/// A value and where it starts
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Value {
  pub(crate) kind: ValueKind,
  pub(crate) pos: Pos,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ValueKind {
  Str(String),
  Num(f64),
  Bool(bool),
  /// `[a, b]`
  List(Vec<Value>),
  /// `(a, b)`; `()` is an empty tuple
  Tuple(Vec<Value>),
  /// `{ "key": value }`
  Map(Vec<(String, Pos, Value)>),
  /// An unnamed struct: `(a: 1, b: 2)`
  Fields(Vec<Field>),
  /// `Name`, `Name(value)` or `Name(field: value, ..)`
  Named {
    name: String,
    body: Body,
  },
  /// Never read: stands for a template parameter whose value is unknown
  /// while a template is checked
  Any,
  /// Never read: a template use passed as an argument to a template from
  /// another file, built in the scope it was written in (an index into the
  /// build's scopes)
  InScope {
    scope: usize,
    value: Box<Value>,
  },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Body {
  Unit,
  Single(Box<Value>),
  Fields(Vec<Field>),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Field {
  pub(crate) name: String,
  pub(crate) pos: Pos,
  pub(crate) value: Value,
}

/// A mistake in the file and where it is
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Issue {
  pub(crate) pos: Pos,
  pub(crate) message: String,
}

/// Read a whole file's text: one value, with blanks and comments around it
pub(crate) fn read(text: &str) -> Result<Value, Issue> {
  // A byte-order mark some editors write takes no column
  let text = text.strip_prefix('\u{feff}').unwrap_or(text);
  let mut reader = Reader {
    chars: text.chars().peekable(),
    pos: Pos { line: 1, column: 1 },
    depth: 0,
  };
  reader.skip_blank()?;
  if reader.peek() == Some('#') {
    return Err(reader.issue("RON extensions (`#![enable(..)]`) are not supported in UI files"));
  }
  let value = reader.value()?;
  reader.skip_blank()?;
  if let Some(c) = reader.peek() {
    return Err(reader.issue(format!("unexpected `{c}` after the file's value")));
  }
  Ok(value)
}

struct Reader<'t> {
  chars: Peekable<Chars<'t>>,
  pos: Pos,
  depth: usize,
}

impl Reader<'_> {
  fn peek(&mut self) -> Option<char> {
    self.chars.peek().copied()
  }

  fn bump(&mut self) -> Option<char> {
    let c = self.chars.next()?;
    match c {
      '\n' => {
        self.pos.line += 1;
        self.pos.column = 1;
      }
      // `\r\n` is one line end; a `\r` takes no column
      '\r' => {}
      _ => self.pos.column += 1,
    }
    Some(c)
  }

  fn issue(&self, message: impl Into<String>) -> Issue {
    Issue {
      pos: self.pos,
      message: message.into(),
    }
  }

  fn skip_blank(&mut self) -> Result<(), Issue> {
    loop {
      match self.peek() {
        Some(c) if c.is_whitespace() => {
          self.bump();
        }
        Some('/') => {
          let start = self.pos;
          self.bump();
          match self.peek() {
            Some('/') => {
              while self.peek().is_some_and(|c| c != '\n') {
                self.bump();
              }
            }
            Some('*') => {
              self.bump();
              self.block_comment(start)?;
            }
            _ => {
              return Err(Issue {
                pos: start,
                message: "a lone `/`: comments start with `//` or `/*`".into(),
              });
            }
          }
        }
        _ => return Ok(()),
      }
    }
  }

  /// The rest of a `/* */` comment; RON's block comments nest
  fn block_comment(&mut self, start: Pos) -> Result<(), Issue> {
    let mut depth = 1;
    while depth > 0 {
      match self.bump() {
        Some('*') if self.peek() == Some('/') => {
          self.bump();
          depth -= 1;
        }
        Some('/') if self.peek() == Some('*') => {
          self.bump();
          depth += 1;
        }
        Some(_) => {}
        None => {
          return Err(Issue {
            pos: start,
            message: "unterminated comment".into(),
          });
        }
      }
    }
    Ok(())
  }

  fn value(&mut self) -> Result<Value, Issue> {
    self.depth += 1;
    if self.depth > MAX_DEPTH {
      return Err(self.issue("the file is nested too deeply"));
    }
    let value = self.value_here();
    self.depth -= 1;
    value
  }

  fn value_here(&mut self) -> Result<Value, Issue> {
    let pos = self.pos;
    let kind = match self.peek() {
      None => return Err(self.issue("the file ends where a value should be")),
      Some('"') => ValueKind::Str(self.string()?),
      Some('[') => ValueKind::List(self.list()?),
      Some('{') => ValueKind::Map(self.map()?),
      Some('(') => self.parens()?,
      Some('\'') => return Err(self.issue("chars are not used in UI files; use a string")),
      Some(c) if c == '-' || c == '.' || c.is_ascii_digit() => ValueKind::Num(self.number()?),
      Some(c) if c.is_alphabetic() || c == '_' => return self.word(pos),
      Some(c) => return Err(self.issue(format!("unexpected `{c}`"))),
    };
    Ok(Value { kind, pos })
  }

  fn ident(&mut self) -> String {
    let mut word = String::new();
    while let Some(c) = self.peek().filter(|c| c.is_alphanumeric() || *c == '_') {
      word.push(c);
      self.bump();
    }
    word
  }

  /// A word: `true`, `false`, a raw string, or a name with its body
  fn word(&mut self, pos: Pos) -> Result<Value, Issue> {
    let word = self.ident();
    let at = |message: &str| Issue {
      pos,
      message: message.into(),
    };
    if word == "r" && matches!(self.peek(), Some('"' | '#')) {
      let text = self.raw_string(pos)?;
      return Ok(Value {
        kind: ValueKind::Str(text),
        pos,
      });
    }
    if word == "b" && matches!(self.peek(), Some('"' | '\'')) {
      return Err(at("byte strings are not used in UI files"));
    }
    let kind = match word.as_str() {
      "true" => ValueKind::Bool(true),
      "false" => ValueKind::Bool(false),
      "Some" | "None" => {
        return Err(at(
          "`Some(..)` and `None` are not used in UI files; give the value, or leave the option out",
        ));
      }
      "inf" | "NaN" => return Err(at("numbers in UI files are finite")),
      _ => {
        self.skip_blank()?;
        let body = if self.peek() == Some('(') {
          self.body(&word)?
        } else {
          Body::Unit
        };
        ValueKind::Named { name: word, body }
      }
    };
    Ok(Value { kind, pos })
  }

  /// Whether the next thing is `name:` (a field), without reading it
  fn field_ahead(&self) -> bool {
    let mut ahead = self.chars.clone();
    let mut any = false;
    while ahead
      .peek()
      .is_some_and(|c| c.is_alphanumeric() || *c == '_')
    {
      any = true;
      ahead.next();
    }
    while ahead.peek().is_some_and(|c| c.is_whitespace()) {
      ahead.next();
    }
    any && ahead.peek() == Some(&':')
  }

  /// A name's body, from its `(`: one value or named fields, not both
  fn body(&mut self, name: &str) -> Result<Body, Issue> {
    let open = self.pos;
    self.bump();
    self.skip_blank()?;
    if self.peek() == Some(')') {
      self.bump();
      return Ok(Body::Fields(Vec::new()));
    }
    if self.field_ahead() {
      return Ok(Body::Fields(self.fields()?));
    }
    let value = self.value()?;
    self.skip_blank()?;
    if self.peek() == Some(',') {
      self.bump();
      self.skip_blank()?;
    }
    match self.peek() {
      Some(')') => {
        self.bump();
        Ok(Body::Single(Box::new(value)))
      }
      _ if self.field_ahead() => Err(self.issue(format!(
        "`{name}` takes either one value or named fields, not both"
      ))),
      Some(_) => Err(self.issue(format!(
        "`{name}` takes one value; name its fields to give more"
      ))),
      None => Err(Issue {
        pos: open,
        message: format!("`{name}(` is never closed"),
      }),
    }
  }

  /// `name: value` fields up to and including the closing `)`
  fn fields(&mut self) -> Result<Vec<Field>, Issue> {
    let mut fields: Vec<Field> = Vec::new();
    loop {
      self.skip_blank()?;
      match self.peek() {
        Some(')') => {
          self.bump();
          return Ok(fields);
        }
        None => return Err(self.issue("a `(` is never closed")),
        _ => {}
      }
      let pos = self.pos;
      let name = self.ident();
      if name.is_empty() {
        return Err(self.issue("expected a field name"));
      }
      self.skip_blank()?;
      if self.peek() != Some(':') {
        return Err(self.issue(format!("expected `:` after `{name}`")));
      }
      self.bump();
      self.skip_blank()?;
      let value = self.value()?;
      if fields.iter().any(|field| field.name == name) {
        return Err(Issue {
          pos,
          message: format!("`{name}` is given twice"),
        });
      }
      fields.push(Field { name, pos, value });
      self.skip_blank()?;
      match self.peek() {
        Some(',') => {
          self.bump();
        }
        Some(')') => {}
        Some(c) => return Err(self.issue(format!("expected `,` or `)`, found `{c}`"))),
        None => return Err(self.issue("a `(` is never closed")),
      }
    }
  }

  /// `(..)` without a name: a struct of fields, or a tuple
  fn parens(&mut self) -> Result<ValueKind, Issue> {
    self.bump();
    self.skip_blank()?;
    if self.field_ahead() {
      return Ok(ValueKind::Fields(self.fields()?));
    }
    Ok(ValueKind::Tuple(self.items(')')?))
  }

  fn list(&mut self) -> Result<Vec<Value>, Issue> {
    self.bump();
    self.items(']')
  }

  /// Comma-separated values up to and including `close`
  fn items(&mut self, close: char) -> Result<Vec<Value>, Issue> {
    let mut items = Vec::new();
    loop {
      self.skip_blank()?;
      match self.peek() {
        Some(c) if c == close => {
          self.bump();
          return Ok(items);
        }
        None => return Err(self.issue(format!("expected `{close}` before the end of the file"))),
        _ => {}
      }
      items.push(self.value()?);
      self.skip_blank()?;
      match self.peek() {
        Some(',') => {
          self.bump();
        }
        Some(c) if c == close => {}
        Some(c) => return Err(self.issue(format!("expected `,` or `{close}`, found `{c}`"))),
        None => return Err(self.issue(format!("expected `{close}` before the end of the file"))),
      }
    }
  }

  fn map(&mut self) -> Result<Vec<(String, Pos, Value)>, Issue> {
    self.bump();
    let mut entries: Vec<(String, Pos, Value)> = Vec::new();
    loop {
      self.skip_blank()?;
      match self.peek() {
        Some('}') => {
          self.bump();
          return Ok(entries);
        }
        Some('"') => {}
        None => return Err(self.issue("expected `}` before the end of the file")),
        Some(_) => return Err(self.issue("map keys are strings")),
      }
      let pos = self.pos;
      let key = self.string()?;
      self.skip_blank()?;
      if self.peek() != Some(':') {
        return Err(self.issue(format!("expected `:` after \"{key}\"")));
      }
      self.bump();
      self.skip_blank()?;
      let value = self.value()?;
      if entries.iter().any(|(k, _, _)| *k == key) {
        return Err(Issue {
          pos,
          message: format!("`{key}` is given twice"),
        });
      }
      entries.push((key, pos, value));
      self.skip_blank()?;
      match self.peek() {
        Some(',') => {
          self.bump();
        }
        Some('}') => {}
        Some(c) => return Err(self.issue(format!("expected `,` or `}}`, found `{c}`"))),
        None => return Err(self.issue("expected `}` before the end of the file")),
      }
    }
  }

  fn string(&mut self) -> Result<String, Issue> {
    let start = self.pos;
    self.bump();
    let mut text = String::new();
    loop {
      match self.bump() {
        None => {
          return Err(Issue {
            pos: start,
            message: "unterminated string".into(),
          });
        }
        Some('"') => return Ok(text),
        Some('\\') => text.push(self.escape()?),
        Some(c) => text.push(c),
      }
    }
  }

  fn escape(&mut self) -> Result<char, Issue> {
    let at = self.pos;
    match self.bump() {
      Some('"') => Ok('"'),
      Some('\'') => Ok('\''),
      Some('\\') => Ok('\\'),
      Some('n') => Ok('\n'),
      Some('r') => Ok('\r'),
      Some('t') => Ok('\t'),
      Some('0') => Ok('\0'),
      Some('u') if self.peek() == Some('{') => {
        self.bump();
        let mut hex = String::new();
        while let Some(c) = self.peek().filter(|c| *c != '}') {
          hex.push(c);
          self.bump();
        }
        self.bump();
        u32::from_str_radix(&hex, 16)
          .ok()
          .and_then(char::from_u32)
          .ok_or_else(|| Issue {
            pos: at,
            message: format!("`\\u{{{hex}}}` is not a character"),
          })
      }
      Some(c) => Err(Issue {
        pos: at,
        message: format!("unknown escape `\\{c}`"),
      }),
      None => Err(Issue {
        pos: at,
        message: "unterminated string".into(),
      }),
    }
  }

  /// A raw string after its `r`: `r"..."` or `r#"..."#` with any number of `#`
  fn raw_string(&mut self, start: Pos) -> Result<String, Issue> {
    let mut hashes = 0;
    while self.peek() == Some('#') {
      self.bump();
      hashes += 1;
    }
    if self.bump() != Some('"') {
      return Err(Issue {
        pos: start,
        message: "a raw string starts `r\"` or `r#\"`".into(),
      });
    }
    let mut text = String::new();
    loop {
      match self.bump() {
        None => {
          return Err(Issue {
            pos: start,
            message: "unterminated string".into(),
          });
        }
        Some('"') => {
          let mut ahead = self.chars.clone();
          if (0..hashes).all(|_| ahead.next() == Some('#')) {
            for _ in 0..hashes {
              self.bump();
            }
            return Ok(text);
          }
          text.push('"');
        }
        Some(c) => text.push(c),
      }
    }
  }

  fn number(&mut self) -> Result<f64, Issue> {
    let start = self.pos;
    let mut text = String::new();
    if self.peek() == Some('-') {
      text.push('-');
      self.bump();
      if self.peek().is_some_and(char::is_alphabetic) {
        return Err(Issue {
          pos: start,
          message: "numbers in UI files are finite".into(),
        });
      }
    }
    if self.peek() == Some('0') {
      let mut ahead = self.chars.clone();
      ahead.next();
      if matches!(ahead.next(), Some('x' | 'o' | 'b')) {
        return Err(Issue {
          pos: start,
          message: "write numbers in decimal".into(),
        });
      }
    }
    let mut exponent = false;
    while let Some(c) = self.peek() {
      let sign_after_e = (c == '+' || c == '-') && text.ends_with(['e', 'E']);
      if c.is_ascii_digit() || c == '.' || c == '_' || sign_after_e {
        text.push(c);
      } else if (c == 'e' || c == 'E') && !exponent {
        exponent = true;
        text.push(c);
      } else {
        break;
      }
      self.bump();
    }
    let clean: String = text.chars().filter(|c| *c != '_').collect();
    clean.parse::<f64>().map_err(|_| Issue {
      pos: start,
      message: format!("`{text}` is not a number"),
    })
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn fields(value: &Value) -> &[Field] {
    match &value.kind {
      ValueKind::Fields(fields) => fields,
      ValueKind::Named {
        body: Body::Fields(fields),
        ..
      } => fields,
      other => panic!("no fields: {other:?}"),
    }
  }

  fn named(value: &Value) -> (&str, &Body) {
    match &value.kind {
      ValueKind::Named { name, body } => (name, body),
      other => panic!("not named: {other:?}"),
    }
  }

  fn error(text: &str) -> Issue {
    read(text).expect_err("the text should not read")
  }

  #[test]
  fn a_tree_with_comments_reads_with_positions() {
    let text = "// a menu\n(\n  root: Panel(\n    gap: 8, /* inline */ fill_width: true,\n    children: [Text(\"Hi\"), Separator],\n  ),\n)\n";
    let top = read(text).unwrap();
    let root = &fields(&top)[0];
    assert_eq!(root.name, "root");
    assert_eq!(root.pos, Pos { line: 3, column: 3 });
    assert_eq!(named(&root.value).0, "Panel");
    let options = fields(&root.value);
    assert_eq!(options[0].value.kind, ValueKind::Num(8.0));
    assert_eq!(options[1].name, "fill_width");
    assert_eq!(
      options[1].pos,
      Pos {
        line: 4,
        column: 26
      }
    );
    assert_eq!(options[1].value.kind, ValueKind::Bool(true));
    let ValueKind::List(children) = &options[2].value.kind else {
      panic!()
    };
    let (text_name, text_body) = named(&children[0]);
    assert_eq!(text_name, "Text");
    let Body::Single(hi) = text_body else {
      panic!()
    };
    assert_eq!(hi.kind, ValueKind::Str("Hi".into()));
    assert_eq!(named(&children[1]), ("Separator", &Body::Unit));
  }

  #[test]
  fn tuples_maps_lists_and_empty_parens_read() {
    let top =
      read("(a: (8, 12), b: {\"x\": 1, \"y\": \"two\",}, c: [], d: (), e: Pct(0.5))").unwrap();
    let f = fields(&top);
    assert!(matches!(&f[0].value.kind, ValueKind::Tuple(v) if v.len() == 2));
    let ValueKind::Map(entries) = &f[1].value.kind else {
      panic!()
    };
    assert_eq!(entries[1].0, "y");
    assert_eq!(entries[1].2.kind, ValueKind::Str("two".into()));
    assert_eq!(f[2].value.kind, ValueKind::List(vec![]));
    assert_eq!(f[3].value.kind, ValueKind::Tuple(vec![]));
    let (pct, body) = named(&f[4].value);
    assert_eq!(pct, "Pct");
    assert!(matches!(body, Body::Single(v) if v.kind == ValueKind::Num(0.5)));
  }

  #[test]
  fn numbers_read_in_their_forms() {
    let top = read("[0, -3, 2.5, .5, 1e3, -1.5E-2, 1_000]").unwrap();
    let ValueKind::List(items) = top.kind else {
      panic!()
    };
    let nums: Vec<f64> = items
      .iter()
      .map(|v| match v.kind {
        ValueKind::Num(n) => n,
        _ => panic!(),
      })
      .collect();
    assert_eq!(nums, [0.0, -3.0, 2.5, 0.5, 1000.0, -0.015, 1000.0]);
  }

  #[test]
  fn strings_keep_escapes_unicode_and_raw_text() {
    let top = read(
      r##"["a\"b\\c\n\t\u{e9}", r"no \n escape", r#"has "quotes""#, "multi
line"]"##,
    )
    .unwrap();
    let ValueKind::List(items) = top.kind else {
      panic!()
    };
    let strs: Vec<&str> = items
      .iter()
      .map(|v| match &v.kind {
        ValueKind::Str(s) => s.as_str(),
        _ => panic!(),
      })
      .collect();
    assert_eq!(
      strs,
      [
        "a\"b\\c\n\té",
        "no \\n escape",
        "has \"quotes\"",
        "multi\nline"
      ]
    );
  }

  #[test]
  fn crlf_and_non_ascii_text_keep_positions_right() {
    let text = "(\r\n  a: \"é😀\", b: 1,\r\n  c: 2,\r\n)";
    let top = read(text).unwrap();
    let f = fields(&top);
    assert_eq!(f[0].value.kind, ValueKind::Str("é😀".into()));
    assert_eq!(
      f[1].pos,
      Pos {
        line: 2,
        column: 12
      },
      "columns count characters"
    );
    assert_eq!(
      f[2].pos,
      Pos { line: 3, column: 3 },
      "\\r\\n is one line end"
    );
  }

  #[test]
  fn refused_forms_say_why() {
    for (text, says) in [
      ("#![enable(implicit_some)] ()", "extensions"),
      ("(a: Some(1))", "`Some(..)` and `None`"),
      ("(a: None)", "`Some(..)` and `None`"),
      ("(a: 'x')", "chars"),
      ("(a: b\"x\")", "byte strings"),
      ("(a: inf)", "finite"),
      ("(a: -inf)", "finite"),
      ("(a: NaN)", "finite"),
      ("(a: 0x10)", "decimal"),
    ] {
      let issue = error(text);
      assert!(issue.message.contains(says), "{text}: {}", issue.message);
    }
  }

  #[test]
  fn mistakes_are_reported_where_they_are() {
    let issue = error("(\n  a: 1,\n  a: 2,\n)");
    assert_eq!(issue.pos, Pos { line: 3, column: 3 });
    assert!(issue.message.contains("`a` is given twice"));

    let issue = error("(a: Button(\"x\", width: 2))");
    assert!(
      issue.message.contains("either one value or named fields"),
      "{}",
      issue.message
    );

    let issue = error("(a: Pair(1, 2))");
    assert!(
      issue.message.contains("takes one value"),
      "{}",
      issue.message
    );

    let issue = error("(a: \"open");
    assert_eq!(issue.pos, Pos { line: 1, column: 5 });
    assert!(issue.message.contains("unterminated string"));

    let issue = error("(a: 1 /* never closed");
    assert!(issue.message.contains("unterminated comment"));

    let issue = error("{1: 2}");
    assert!(issue.message.contains("map keys are strings"));

    let issue = error("(a: 1) extra");
    assert!(issue.message.contains("after the file's value"));

    let issue = error("(a: \"\\q\")");
    assert!(issue.message.contains("unknown escape"));
  }

  #[test]
  fn deep_nesting_is_an_error_not_a_crash() {
    let text = format!("{}{}", "[".repeat(10_000), "]".repeat(10_000));
    assert!(error(&text).message.contains("nested too deeply"));
  }

  #[test]
  fn lowercase_names_read_as_unit_names() {
    let top = read("(label: label, anchor: Center)").unwrap();
    let f = fields(&top);
    assert_eq!(named(&f[0].value), ("label", &Body::Unit));
    assert_eq!(named(&f[1].value), ("Center", &Body::Unit));
  }

  #[test]
  fn a_byte_order_mark_is_skipped_and_takes_no_column() {
    let value = read("\u{feff}(a: 1)").unwrap();
    assert_eq!(fields(&value)[0].pos, Pos { line: 1, column: 2 });
  }
}
