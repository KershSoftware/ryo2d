# UI Files and Templates (4d) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Screens written as RON UI files load into 4c's `UiNode`, update in place while the game runs, share templates, and show values through two-way named bindings.

**Architecture:** A small reader turns RON syntax into values that know their line and column; one vocabulary table turns them into `UiNode`s (a file is a spelling of `UiNode`), with templates expanded first. A spawned tree is brought in line with a new `UiNode` by one reconcile, which `ctx.update_ui` and hot reload share. Bindings live on each root and write 4c's state components.

**Tech Stack:** Rust 2024, bevy_ecs 0.20 (only through `src/core/ecs/kernel.rs`), the 4b/4c UI, the asset manager's existing notify watcher. No new dependencies.

**Spec:** `docs/specs/2026-10-17-ui-files-design.md`

## Global Constraints

- No `unwrap`, `expect` or `panic!` in library code (tests may use them). A poisoned lock is recovered with `into_inner()`.
- LF line endings; 2-space indent (the repo's `rustfmt.toml`). Comments normal and minimal, in the surrounding code's style.
- Crate boundaries: bevy_ecs only through `src/core/ecs/kernel.rs` (add a re-export there if you need a new bevy item); taffy only in `src/core/ui/layout/`; parley, swash and fdsm only in text. No new dependencies; the reader is ours, not the `ron` crate (the `ron` crate is used only in a test that checks files stay valid RON).
- Debug-only code carries `#[cfg_attr(not(debug_assertions), allow(dead_code))]`.
- Warnings: `crate::core::ui::theme::warn_once(key: String, message: impl FnOnce() -> String)` (in `src/core/ui/theme/look.rs`) for once-per-key warnings; `crate::engine_warn!(module: "ui", ...)` otherwise.
- Paths in UI files (imports, images) resolve exactly as `AssetManager::load_sprite` resolves them: relative to the asset manager's base path, which is the game's working directory. Games write `assets/ui/pause.ui.ron`.
- Skills: edit `.claude/skills/<name>/SKILL.md`, then `cp` it to `src/cli/skills/<name>.md` and check with `cmp`.
- Commits: `type: brief description` and bullets. **No trailer or attribution line of any kind** (no Co-Authored-By, no Claude-Session). Never mention AGENTS.md in a commit. Stage files by name; never `git add -A`; never add `data/` or `target/`; no bare `git stash`.
- The gate, all green before a task is done:
  - `cargo fmt --check`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo clippy -p ryo2d --lib --target wasm32-unknown-unknown -- -D warnings`
  - `cargo test --workspace`
  - `cargo check --release -p ryo2d --lib`

## Rulings Made While Planning

These correct or sharpen the spec; the plan's commit updates the spec text to match.

1. **Paths** resolve like `load_sprite` (see Global Constraints), not "relative to the assets directory". The engine never sets a base path, so the base is the working directory and games already pass `assets/...` for sounds and fonts.
2. **Patterns** are the strings that become a `UiLabel` or `UiTooltip`: `Text`'s `text`, the `label` of `Button`, `Checkbox` and `Toggle`, any `tooltip`, and `Confirm`'s `title`, `message`, `yes` and `no` (they become text and button labels). A `Slider`'s, `Progress`'s or `Rebind`'s `label` is a kind option, not a component, and stays literal.
3. **A root's placement** (`UiRoot`'s `anchor`, `offset`, `at`) is three-way, not tree-owned: `UiRoot` documents it as the game's to change, and the dialogue page moves its HUD that way. `modal` stays tree-owned.
4. **Default keys** count only the siblings of that kind that have no `key` or `name`, so naming one button does not change the keys of the others.

## Review Focus

1. **A file read half-saved.** An editor's save can be seen mid-write: the reload fails, the old tree stays, and the next event (the finished save) reloads it. Warnings come once per distinct broken text, never every frame or twice for one save. Test in Task 7.
2. **A reload under an open, focused file modal** (the pause menu open with Resume focused): focus stays on Resume, and no `UiEvent` (no `Back`, no `Clicked`, no focus events) comes from the reconcile. Test in Task 8.
3. **Two trees from one file** (two HUDs): one reload updates both, and each keeps its own binding values. Test in Task 8.
4. **A template removed while code-built instances exist:** the instances stay as they were, one warning names the file and template, nothing panics, and restoring the template reloads them. Test in Task 8.
5. **Windows files and non-ASCII text:** a file saved with CRLF line endings and holding `é` or an emoji reports the right line and column (columns count characters) and keeps its strings intact. Test in Task 1.

## File Map

| File | Task | Responsibility |
|---|---|---|
| `src/core/ui/file/mod.rs` | 1 | The UI file module: declarations and re-exports |
| `src/core/ui/file/reader.rs` | 1 | RON syntax into positioned values |
| `src/core/ui/nodes/bindings.rs` | 2 | `BindValue`, patterns, per-root values, sync |
| `src/core/ui/nodes/reconcile.rs` | 3, 4 | Per-node update (3), tree reconcile and `Mode` (4) |
| `src/core/ui/file/vocab.rs` | 5 | The kinds and options table, suggestions |
| `src/core/ui/file/build.rs` | 5, 6 | `BuildCx`, values into `UiNode`, template expansion (6) |
| `src/core/ui/file/error.rs` | 5, 6 | `Issue` lists, `UiFileError`, `UiFileIssue` |
| `src/core/ui/file/document.rs` | 6 | `UiFile`, `UiArgs`, templates, imports, load checks |
| `src/core/ui/file/check.rs` | 9 | Disk checks for `ryo2d check-ui` |
| `src/cli/commands/check_ui.rs` | 9 | The `check-ui` command |
| `src/core/ui/nodes/{node,spawn,draw,components,events,mod}.rs` | 2, 3, 4, 8 | 4c's nodes, extended |
| `src/core/asset/{manager,hot_reload}.rs`, `src/core/error.rs` | 6, 7 | UI files in the asset manager |
| `src/core/scene/context.rs`, `src/core/testing/ui.rs` | 2, 4, 8 | Scene and harness API |
| `src/core/engine/frame.rs`, `src/core/messages/mod.rs`, `src/lib.rs`, `src/core/ui/mod.rs` | 2, 6, 8 | Wiring, messages, prelude |
| `benches/ui.rs` | 4 | Update-versus-spawn bench |
| `tests/e2e/ui_files.rs`, `tests/e2e.rs`, `tests/integration/ui_file_validity.rs`, `tests/integration.rs` | 8, 10 | End to end and RON validity |
| `examples/demos/ui/...`, `examples/games/breakout/...` | 10-12 | Acceptance |
| skills, AGENTS.md files, `docs/` | 13 | Documentation |

---

### Task 1: The reader

**Files:**
- Create: `src/core/ui/file/mod.rs`, `src/core/ui/file/reader.rs`
- Modify: `src/core/ui/mod.rs` (declare `pub(crate) mod file;`)
- Test: `src/core/ui/file/reader.rs` (`mod tests`)

**Interfaces:**
- Consumes: nothing.
- Produces (all `pub(crate)` in `crate::core::ui::file::reader`):
  ```rust
  pub(crate) struct Pos { pub(crate) line: u32, pub(crate) column: u32 }  // 1-based; columns count chars
  pub(crate) struct Value { pub(crate) kind: ValueKind, pub(crate) pos: Pos }
  pub(crate) enum ValueKind {
    Str(String), Num(f64), Bool(bool),
    List(Vec<Value>),                 // [a, b]
    Tuple(Vec<Value>),                // (a, b), and () as an empty tuple
    Map(Vec<(String, Pos, Value)>),   // { "key": value }
    Fields(Vec<Field>),               // an unnamed struct: (a: 1, b: 2)
    Named { name: String, body: Body },
    Any,                              // never read: an unknown template parameter while checking (Task 6)
  }
  pub(crate) enum Body { Unit, Single(Box<Value>), Fields(Vec<Field>) }
  pub(crate) struct Field { pub(crate) name: String, pub(crate) pos: Pos, pub(crate) value: Value }
  pub(crate) struct Issue { pub(crate) pos: Pos, pub(crate) message: String }
  pub(crate) fn read(text: &str) -> Result<Value, Issue>
  ```
  All derive `Debug, Clone, PartialEq` (`Pos` also `Copy, Eq, Default`).

- [ ] **Step 1: Declare the module**

`src/core/ui/file/mod.rs`:

```rust
//! UI files: screens written in RON that load into `UiNode`
//!
//! `reader` turns the file's text into values that know their line and
//! column. Later modules check those values against the vocabulary and
//! build `UiNode`s from them.

pub(crate) mod reader;
```

In `src/core/ui/mod.rs`, beside the other module declarations, add `pub(crate) mod file;`.

- [ ] **Step 2: Write the failing tests**

At the bottom of `src/core/ui/file/reader.rs` (create the file with only `use` lines and these tests first, so they fail to compile, then fail):

```rust
#[cfg(test)]
mod tests {
  use super::*;

  fn fields(value: &Value) -> &[Field] {
    match &value.kind {
      ValueKind::Fields(fields) => fields,
      ValueKind::Named { body: Body::Fields(fields), .. } => fields,
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
    assert_eq!(options[1].pos, Pos { line: 4, column: 26 });
    assert_eq!(options[1].value.kind, ValueKind::Bool(true));
    let ValueKind::List(children) = &options[2].value.kind else { panic!() };
    let (text_name, text_body) = named(&children[0]);
    assert_eq!(text_name, "Text");
    let Body::Single(hi) = text_body else { panic!() };
    assert_eq!(hi.kind, ValueKind::Str("Hi".into()));
    assert_eq!(named(&children[1]), ("Separator", &Body::Unit));
  }

  #[test]
  fn tuples_maps_lists_and_empty_parens_read() {
    let top = read("(a: (8, 12), b: {\"x\": 1, \"y\": \"two\",}, c: [], d: (), e: Pct(0.5))").unwrap();
    let f = fields(&top);
    assert!(matches!(&f[0].value.kind, ValueKind::Tuple(v) if v.len() == 2));
    let ValueKind::Map(entries) = &f[1].value.kind else { panic!() };
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
    let ValueKind::List(items) = top.kind else { panic!() };
    let nums: Vec<f64> = items
      .iter()
      .map(|v| match v.kind { ValueKind::Num(n) => n, _ => panic!() })
      .collect();
    assert_eq!(nums, [0.0, -3.0, 2.5, 0.5, 1000.0, -0.015, 1000.0]);
  }

  #[test]
  fn strings_keep_escapes_unicode_and_raw_text() {
    let top = read(r##"["a\"b\\c\n\t\u{e9}", r"no \n escape", r#"has "quotes""#, "multi
line"]"##).unwrap();
    let ValueKind::List(items) = top.kind else { panic!() };
    let strs: Vec<&str> = items
      .iter()
      .map(|v| match &v.kind { ValueKind::Str(s) => s.as_str(), _ => panic!() })
      .collect();
    assert_eq!(strs, ["a\"b\\c\n\té", "no \\n escape", "has \"quotes\"", "multi\nline"]);
  }

  #[test]
  fn crlf_and_non_ascii_text_keep_positions_right() {
    let text = "(\r\n  a: \"é😀\", b: 1,\r\n  c: 2,\r\n)";
    let top = read(text).unwrap();
    let f = fields(&top);
    assert_eq!(f[0].value.kind, ValueKind::Str("é😀".into()));
    assert_eq!(f[1].pos, Pos { line: 2, column: 13 }, "columns count characters");
    assert_eq!(f[2].pos, Pos { line: 3, column: 3 }, "\\r\\n is one line end");
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
    assert!(issue.message.contains("either one value or named fields"), "{}", issue.message);

    let issue = error("(a: Pair(1, 2))");
    assert!(issue.message.contains("takes one value"), "{}", issue.message);

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
}
```

- [ ] **Step 3: Run the tests to see them fail**

Run: `cargo test -p ryo2d --lib ui::file::reader`
Expected: FAIL to compile (`read`, `Value` and the other items do not exist yet).

- [ ] **Step 4: Write the reader**

Above the tests in `src/core/ui/file/reader.rs`:

```rust
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
  Named { name: String, body: Body },
  /// Never read: stands for a template parameter whose value is unknown
  /// while a template is checked
  Any,
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
    while ahead.peek().is_some_and(|c| c.is_alphanumeric() || *c == '_') {
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
```

- [ ] **Step 5: Run the tests to see them pass**

Run: `cargo test -p ryo2d --lib ui::file::reader`
Expected: PASS (all 9 tests). If a position assertion fails, recount the test text's columns before touching the reader: columns count characters from 1, and `\r` takes none.

- [ ] **Step 6: Run the gate and commit**

Run the five gate commands. The module is not used outside its tests yet: if clippy reports dead code, add `#![cfg_attr(not(test), allow(dead_code))]` at the top of `reader.rs` with the comment `// Used by the vocabulary and documents (Tasks 5 and 6)`, and remove it in Task 5.

```bash
git add src/core/ui/file/mod.rs src/core/ui/file/reader.rs src/core/ui/mod.rs
git commit -m "feat: read UI files' RON syntax with positions

- A reader of the RON forms UI files use, keeping each value's line and column
- Refused forms (extensions, Some/None, chars, byte strings, non-finite and non-decimal numbers) say why
- CRLF and non-ASCII text keep positions right; deep nesting is an error"
```

---

### Task 2: Bindings on code trees

**Files:**
- Create: `src/core/ui/nodes/bindings.rs`
- Modify: `src/core/ui/nodes/node.rs` (fields and methods `bind`, `bound`, crate `seed`; `Kind::takes` takes `"bind"` on value widgets), `src/core/ui/nodes/spawn.rs` (insert `UiBind`/`UiPattern`, seed and apply after `finish`), `src/core/ui/nodes/draw.rs` (sync each root before its walk; sync a bound widget after it draws), `src/core/ui/nodes/components.rs` (fix `UiLabel`'s doc: text, button, checkbox and toggle only), `src/core/ui/nodes/mod.rs`, `src/core/ui/mod.rs`, `src/core/scene/context.rs` (`ui_set`, `ui_get`), `src/core/testing/ui.rs` (`ui_set`, `ui_get`), `src/lib.rs` (prelude `BindValue`)
- Test: `src/core/ui/nodes/bindings.rs` (`mod tests`)

**Interfaces:**
- Consumes: 4c's `UiNode`, `spawn_ui`/`spawn_ui_in`/`finish`, `root_of`, the walk (`draw_node`, `draw_nodes`, `UiContext::node`), `UiHarness`.
- Produces:
  ```rust
  // public
  pub enum BindValue { Number(f64), Text(String), Bool(bool) }
  impl From<f32 | f64 | i32 | i64 | u32 | u64 | usize | bool | &str | String> for BindValue
  impl TryFrom<BindValue> for f32 | f64 | i32 | u32 | usize | bool | String  // Error = BindValue
  impl UiNode { pub fn bind(self, name: impl Into<String>) -> UiNode; pub fn bound(self) -> UiNode }
  impl SceneContext<'_> {
    pub fn ui_set(&mut self, node: Entity, name: &str, value: impl Into<BindValue>);
    pub fn ui_get<T: TryFrom<BindValue>>(&self, node: Entity, name: &str) -> Option<T>;
  }
  impl UiHarness { pub fn ui_set(..); pub fn ui_get<T: TryFrom<BindValue>>(..) -> Option<T> }  // same shapes
  // crate (bindings.rs)
  impl UiNode { pub(crate) fn seed(self, name: impl Into<String>, value: BindValue) -> UiNode }
  pub(crate) struct UiBind(pub(crate) String);                                  // Component
  pub(crate) struct UiPattern { pub(crate) label: Option<Pattern>, pub(crate) tooltip: Option<Pattern> }  // Component
  pub(crate) struct UiBindings { pub(crate) values: HashMap<String, BindValue>, index: Option<Index> }   // Component on a root
  pub(crate) struct Pattern(..);  // parse(&str) -> Pattern, names(), format(..)
  pub(crate) fn set(world: &mut World, node: Entity, name: &str, value: BindValue);
  pub(crate) fn get(world: &World, node: Entity, name: &str) -> Option<BindValue>;
  pub(crate) fn apply_subtree(world: &mut World, top: Entity, seeds: Vec<(String, BindValue)>);
  pub(crate) fn sync_root(world: &mut World, root: Entity);
  pub(crate) fn widget_wrote(world: &mut World, node: Entity);
  pub(crate) fn format_node(world: &mut World, root: Entity, node: Entity);
  pub(crate) fn invalidate(world: &mut World, node: Entity);   // the node's root re-indexes on next use
  ```
  `UiNode` gains `pub(crate) bind: Option<String>`, `pub(crate) bound: bool`, `pub(crate) seeds: Vec<(String, BindValue)>`.

Behaviour (the spec's Bindings section, restated as rules for the code):
- **Values live on the root** (`UiBindings`, inserted on first use). `set` and `get` take any node and use `root_of(node)`.
- **Bound kinds and their component:** `Slider`, `Progress` → `UiValue` (a `Number`); `Checkbox`, `Toggle` → `UiChecked` (a `Bool`); `RadioGroup`, `Dropdown`, `Tabs`, `ChoiceList` → `UiIndex` (a whole, non-negative `Number`); `TextInput` → `UiTextValue` (a `Text`, or a `Number`/`Bool` formatted as by `{name}`). A wrong type warns once (`warn_once(format!("ui-bind-type:{name}:{kind}"), ..)`, naming the binding and the kind) and writes nothing.
- **Patterns:** a node marked `bound` turns its `UiLabel` text (from `state.label`) and its tooltip into `Pattern`s, kept in `UiPattern`; formatting writes `UiLabel`/`UiTooltip` only when the text differs.
- **Comparing a stored value with a widget** compares numbers as `f32` (widgets hold `f32` and `usize`), so a slider's `0.8` and a stored `0.800000011920929` are the same.
- **`{name}` formatting:** a whole number (finite, `fract() == 0.0`, below `1e15` in size) prints as an integer; another number prints as the shortest text for its `f32` (`format!("{}", n as f32)`); `{name:.N}` prints `N` decimals (`format!("{n:.N$}")`); bools print `true`/`false`; text as is; `{{` and `}}` print braces; a `{..}` that is not a valid hole (`name` of `[A-Za-z0-9_-]` and an optional `:.N`) is kept as written. An unset name prints nothing and, in debug builds, warns once per name (`warn_once(format!("ui-binding-unset:{name}"), ..)`).
- **Seeding** (`apply_subtree(world, top, seeds)`, run after `spawn_ui`/`spawn_ui_in` finish, and by the reconcile in Tasks 3-4): first the given `seeds` fill names with no value; then each bound widget in the subtree, depth first, fills its name from its own component if the name has no value, or else is written from the stored value; then every pattern node in the subtree is formatted; every name newly seeded is also propagated to the whole root (a new slider's name may be shown by an existing label).
- **Syncing:** `sync_root(world, root)` runs for each root just before the walk draws it (in `draw_nodes`, and for an embedded root in `UiContext::node`): for each bound name, the first bound widget whose component differs from the stored value sets it and propagates to every other widget and pattern of that name. `widget_wrote(world, node)` runs right after a bound widget is drawn (after its `Write` is applied, in every arm that writes: check, slider, pick, radio group, choice list, text input), so patterns drawn later in the same walk show the new value.
- **The index** (`name → widgets`, `name → pattern nodes`, depth first) is rebuilt lazily when `None`; `invalidate` sets it `None` and is called by `spawn_ui_in` and the reconcile. Despawned entities in a stale index are skipped.
- `Kind::takes` gains `"bind"` for the bound kinds, and `bind` records `given("bind")`, so 4c's spawn already warns `UiNode::bind does nothing on a button node`. `bound` records nothing (any node may be bound; only labels and tooltips have patterns).

- [ ] **Step 1: Write the failing tests**

`src/core/ui/nodes/bindings.rs` starts with its tests (the items they use do not exist yet):

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::testing::UiHarness;
  use crate::core::ui::nodes::{UiChecked, UiIndex, UiLabel, UiTextValue, UiTooltip, UiValue};
  use crate::prelude::*;

  fn label(h: &UiHarness, node: Entity) -> String {
    h.world().get::<UiLabel>(node).map(|l| l.0.clone()).unwrap_or_default()
  }

  #[test]
  fn patterns_parse_holes_decimals_and_escapes() {
    let mut values = HashMap::new();
    values.insert("score".to_string(), BindValue::from(120));
    values.insert("volume".to_string(), BindValue::from(0.8_f32));
    values.insert("on".to_string(), BindValue::from(true));
    let format = |text: &str| Pattern::parse(text).format(&values);
    assert_eq!(format("Score: {score}"), "Score: 120");
    assert_eq!(format("{volume}"), "0.8", "an f32's shortest text");
    assert_eq!(format("{volume:.2}"), "0.80");
    assert_eq!(format("{on}"), "true");
    assert_eq!(format("{{score}}"), "{score}");
    assert_eq!(format("{not a hole}"), "{not a hole}");
    assert_eq!(format("{missing}!"), "!");
  }

  #[test]
  fn ui_set_writes_bound_widgets_and_patterns() {
    let mut h = UiHarness::new(400.0, 300.0);
    let root = h.spawn_ui(
      UiNode::column()
        .child(UiNode::text("Score: {score}").bound().name("score"))
        .child(UiNode::slider(0.0..=1.0, 0.5).bind("volume").name("slider"))
        .child(UiNode::checkbox("Mute", false).bind("mute").name("mute"))
        .child(UiNode::dropdown(&["A", "B", "C"], 0).bind("pick").name("pick"))
        .child(UiNode::text_input("").bind("who").name("who")),
    );
    h.ui_set(root, "score", 120);
    h.ui_set(root, "volume", 0.25_f32);
    h.ui_set(root, "mute", true);
    h.ui_set(root, "pick", 2_usize);
    h.ui_set(root, "who", "Kai");
    let find = |h: &UiHarness, name| h.find_ui(name).unwrap();
    assert_eq!(label(&h, find(&h, "score")), "Score: 120");
    assert_eq!(h.world().get::<UiValue>(find(&h, "slider")).unwrap().0, 0.25);
    assert!(h.world().get::<UiChecked>(find(&h, "mute")).unwrap().0);
    assert_eq!(h.world().get::<UiIndex>(find(&h, "pick")).unwrap().0, 2);
    assert_eq!(h.world().get::<UiTextValue>(find(&h, "who")).unwrap().0, "Kai");
    assert_eq!(h.ui_get::<u32>(root, "score"), Some(120));
    assert_eq!(h.ui_get::<String>(root, "who"), Some("Kai".to_string()));
  }

  #[test]
  fn a_widget_seeds_its_name_and_a_label_follows_it_the_same_frame() {
    let mut h = UiHarness::new(400.0, 300.0);
    let root = h.spawn_ui(
      UiNode::column()
        .focus_scope()
        .child(UiNode::slider(0.0..=1.0, 0.5).step(0.1).bind("volume").default_focus())
        .child(UiNode::text("Volume: {volume:.1}").bound().name("shown")),
    );
    let shown = h.find_ui("shown").unwrap();
    assert_eq!(label(&h, shown), "Volume: 0.5", "seeded from the slider at spawn");
    h.frame(|_| {});
    h.action("ui.right");
    h.frame(|_| {});
    assert_eq!(label(&h, shown), "Volume: 0.6", "drawn after the slider: same frame");
    assert_eq!(h.ui_get::<f32>(root, "volume"), Some(0.6));
  }

  #[test]
  fn a_game_write_to_a_bound_component_reaches_the_name() {
    let mut h = UiHarness::new(400.0, 300.0);
    let root = h.spawn_ui(
      UiNode::column()
        .child(UiNode::text("{hp:.0}").bound().name("shown"))
        .child(UiNode::progress(1.0).bind("hp").name("bar").hidden()),
    );
    let bar = h.find_ui("bar").unwrap();
    h.world_mut().get_mut::<UiValue>(bar).unwrap().0 = 0.4;
    h.frame(|_| {});
    assert_eq!(h.ui_get::<f32>(root, "hp"), Some(0.4), "synced though hidden");
    h.frame(|_| {});
    assert_eq!(label(&h, h.find_ui("shown").unwrap()), "0");
  }

  #[test]
  fn two_widgets_on_one_name_follow_each_other() {
    let mut h = UiHarness::new(400.0, 300.0);
    let root = h.spawn_ui(
      UiNode::column()
        .child(UiNode::checkbox("A", false).bind("on").name("a"))
        .child(UiNode::toggle("B", false).bind("on").name("b")),
    );
    let a = h.find_ui("a").unwrap();
    h.world_mut().get_mut::<UiChecked>(a).unwrap().0 = true;
    h.frame(|_| {});
    assert!(h.world().get::<UiChecked>(h.find_ui("b").unwrap()).unwrap().0);
    assert_eq!(h.ui_get::<bool>(root, "on"), Some(true));
  }

  #[test]
  fn values_belong_to_each_root() {
    let mut h = UiHarness::new(400.0, 300.0);
    let hud = || UiNode::column().child(UiNode::text("{score}").bound());
    let one = h.spawn_ui(hud());
    let two = h.spawn_ui(hud());
    h.ui_set(one, "score", 1);
    h.ui_set(two, "score", 2);
    assert_eq!(h.ui_get::<i32>(one, "score"), Some(1));
    assert_eq!(h.ui_get::<i32>(two, "score"), Some(2));
  }

  #[test]
  fn a_wrong_type_warns_once_and_writes_nothing() {
    let mut h = UiHarness::new(400.0, 300.0);
    let root = h.spawn_ui(UiNode::column().child(UiNode::slider(0.0..=1.0, 0.5).bind("level").name("s")));
    h.ui_set(root, "level", true);
    h.ui_set(root, "level", false);
    assert_eq!(h.world().get::<UiValue>(h.find_ui("s").unwrap()).unwrap().0, 0.5);
    assert_eq!(crate::core::ui::test_util::warnings_containing("\"level\""), 1);
  }

  #[test]
  fn seeds_fill_only_names_without_a_value_and_tooltips_are_patterns() {
    let mut h = UiHarness::new(400.0, 300.0);
    let root = h.spawn_ui(
      UiNode::column()
        .seed("gold", BindValue::from(5))
        .child(UiNode::button("Buy").tooltip("You have {gold} gold").bound().name("buy")),
    );
    let buy = h.find_ui("buy").unwrap();
    assert_eq!(h.world().get::<UiTooltip>(buy).unwrap().0, "You have 5 gold");
    h.ui_set(root, "gold", 9);
    assert_eq!(h.world().get::<UiTooltip>(buy).unwrap().0, "You have 9 gold");
  }

  #[test]
  fn a_subtree_added_later_joins_the_roots_values() {
    let mut h = UiHarness::new(400.0, 300.0);
    let root = h.spawn_ui(UiNode::column().name("list"));
    h.ui_set(root, "count", 3);
    let list = h.find_ui("list").unwrap();
    let added = crate::core::ui::nodes::spawn_ui_in(h.world_mut(), list, UiNode::text("{count} items").bound()).unwrap();
    assert_eq!(label(&h, added), "3 items");
    h.ui_set(root, "count", 4);
    assert_eq!(label(&h, added), "4 items");
  }

  #[test]
  fn bind_on_a_kind_without_a_value_warns() {
    let mut h = UiHarness::new(400.0, 300.0);
    h.spawn_ui(UiNode::column().child(UiNode::button("Go").bind("x")));
    assert_eq!(crate::core::ui::test_util::warnings_containing("UiNode::bind does nothing on a button node"), 1);
  }
}
```

If `crate::core::ui::test_util` has no `warnings_containing(&str) -> usize`, add it there (4c's `draw.rs` tests have a local `warnings(needle)` helper reading the captured log the same way; move that logic into `test_util` and make both use it).

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p ryo2d --lib ui::nodes::bindings`
Expected: FAIL to compile (`BindValue`, `Pattern`, `bind`, `bound`, `seed`, `ui_set` and `ui_get` do not exist).

- [ ] **Step 3: Write `BindValue` and `Pattern`**

In `bindings.rs`, above the tests:

```rust
//! Named bindings: values a tree's root keeps by name, written into the
//! state components of the widgets bound to each name and formatted into
//! the labels and tooltips that show them

use std::collections::HashMap;

use crate::core::ecs::kernel::{Children, Component, Entity, World};
use crate::core::ui::theme::warn_once;

use super::components::{UiChecked, UiIndex, UiLabel, UiTextValue, UiTooltip, UiValue};
use super::node::{Kind, UiNodeKind};
use super::spawn::root_of;

/// A value a binding holds
#[derive(Debug, Clone, PartialEq)]
pub enum BindValue {
  Number(f64),
  Text(String),
  Bool(bool),
}

impl BindValue {
  /// The text `{name}` shows, or `{name:.N}` with `decimals`
  pub(crate) fn display(&self, decimals: Option<usize>) -> String {
    match self {
      BindValue::Number(n) => match decimals {
        Some(d) => format!("{n:.d$}"),
        None if n.is_finite() && n.fract() == 0.0 && n.abs() < 1e15 => format!("{}", *n as i64),
        None => format!("{}", *n as f32),
      },
      BindValue::Text(text) => text.clone(),
      BindValue::Bool(on) => on.to_string(),
    }
  }

  /// The same value as a widget would hold it: numbers compared as `f32`
  fn same(&self, other: &BindValue) -> bool {
    match (self, other) {
      (BindValue::Number(a), BindValue::Number(b)) => (*a as f32) == (*b as f32),
      (a, b) => a == b,
    }
  }
}
```

Then the `From` impls (`f32` and `f64` → `Number(f64::from(..))`/`Number(..)`; `i32`, `i64`, `u32`, `u64`, `usize` → `Number(v as f64)`; `bool` → `Bool`; `&str`, `String` → `Text`) and the `TryFrom<BindValue>` impls (`f32`, `f64` from `Number`; `i32`, `u32`, `usize` from a whole `Number` in range; `bool` from `Bool`; `String` from `Text`; each `type Error = BindValue` returning the value unchanged on a mismatch). Write them with one small macro per family rather than ten hand-written blocks.

The pattern:

```rust
/// A label or tooltip with `{name}` holes, parsed once
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Pattern(Vec<Piece>);

#[derive(Debug, Clone, PartialEq)]
enum Piece {
  Text(String),
  Value { name: String, decimals: Option<usize> },
}

impl Pattern {
  pub(crate) fn parse(text: &str) -> Pattern {
    let mut pieces = Vec::new();
    let mut literal = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
      match c {
        '{' if chars.peek() == Some(&'{') => {
          chars.next();
          literal.push('{');
        }
        '}' if chars.peek() == Some(&'}') => {
          chars.next();
          literal.push('}');
        }
        '{' => {
          let mut hole = String::new();
          let mut closed = false;
          for c in chars.by_ref() {
            if c == '}' {
              closed = true;
              break;
            }
            hole.push(c);
          }
          match parse_hole(&hole).filter(|_| closed) {
            Some((name, decimals)) => {
              if !literal.is_empty() {
                pieces.push(Piece::Text(std::mem::take(&mut literal)));
              }
              pieces.push(Piece::Value { name, decimals });
            }
            // Not a hole: keep the text as written
            None => {
              literal.push('{');
              literal.push_str(&hole);
              if closed {
                literal.push('}');
              }
            }
          }
        }
        c => literal.push(c),
      }
    }
    if !literal.is_empty() {
      pieces.push(Piece::Text(literal));
    }
    Pattern(pieces)
  }

  /// The names the pattern shows
  pub(crate) fn names(&self) -> impl Iterator<Item = &str> {
    self.0.iter().filter_map(|piece| match piece {
      Piece::Value { name, .. } => Some(name.as_str()),
      Piece::Text(_) => None,
    })
  }

  /// The text with each hole filled from `values`; an unset name shows as nothing
  pub(crate) fn format(&self, values: &HashMap<String, BindValue>) -> String {
    let mut text = String::new();
    for piece in &self.0 {
      match piece {
        Piece::Text(literal) => text.push_str(literal),
        Piece::Value { name, decimals } => match values.get(name) {
          Some(value) => text.push_str(&value.display(*decimals)),
          None => warn_unset(name),
        },
      }
    }
    text
  }
}

/// `name` or `name:.N`
fn parse_hole(hole: &str) -> Option<(String, Option<usize>)> {
  let (name, spec) = match hole.split_once(':') {
    Some((name, spec)) => (name, Some(spec)),
    None => (hole, None),
  };
  let valid = !name.is_empty()
    && name
      .chars()
      .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
  if !valid {
    return None;
  }
  let decimals = match spec {
    None => None,
    Some(spec) => Some(spec.strip_prefix('.')?.parse::<usize>().ok()?),
  };
  Some((name.to_string(), decimals))
}

#[cfg_attr(not(debug_assertions), allow(unused_variables))]
fn warn_unset(name: &str) {
  #[cfg(debug_assertions)]
  warn_once(format!("ui-binding-unset:{name}"), || {
    format!("no value is set for the UI binding \"{name}\"; it shows as nothing")
  });
}
```

- [ ] **Step 4: Write the components, `set`, `get`, seeding and sync**

```rust
/// The binding a value widget is tied to
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub(crate) struct UiBind(pub(crate) String);

/// A node's label and tooltip patterns
#[derive(Component, Debug, Clone, PartialEq, Default)]
pub(crate) struct UiPattern {
  pub(crate) label: Option<Pattern>,
  pub(crate) tooltip: Option<Pattern>,
}

/// A root's binding values, and an index of the nodes that use each name
#[derive(Component, Debug, Default)]
pub(crate) struct UiBindings {
  pub(crate) values: HashMap<String, BindValue>,
  index: Option<Index>,
}

#[derive(Debug, Default)]
struct Index {
  widgets: HashMap<String, Vec<Entity>>,
  patterns: HashMap<String, Vec<Entity>>,
}
```

Write these functions with the behaviour listed above:

- `fn widget_value(world: &World, node: Entity) -> Option<BindValue>`: the bound kind's component as a `BindValue` (`UiValue` → `Number(f64::from(v))`, `UiIndex` → `Number(i as f64)`, `UiChecked` → `Bool`, `UiTextValue` → `Text`).
- `fn write_widget(world: &mut World, node: Entity, name: &str, value: &BindValue)`: by the node's `UiNodeKind` kind, converts and writes the component only if it differs (`get_mut` only then, so `Changed<..>` stays truthful), or warns once on a type mismatch.
- `fn index(world: &mut World, root: Entity) -> Option<&Index>`-style access: build the index by walking `Children` depth first from `root`, collecting `UiBind` names and every name of each `UiPattern`, when `UiBindings::index` is `None`. (Return owned `Vec<Entity>` lists for one name to avoid holding a borrow while writing.)
- `pub(crate) fn invalidate(world, node)`: `root_of(node)`, then `index = None` if the root has `UiBindings`.
- `fn propagate(world, root, name, except: Option<Entity>)`: every widget bound to `name` but `except` gets `write_widget`, every pattern node using `name` gets `format_node`.
- `pub(crate) fn format_node(world, root, node)`: format the node's `UiPattern` against the root's values and write `UiLabel`/`UiTooltip` (inserting `UiTooltip` if missing) only if the text differs.
- `pub(crate) fn set(world, node, name, value)`: no root → `warn_once` ("ui_set needs a node of a spawned UI tree") and return; store the value if it is not `same` as the stored one (insert `UiBindings` on the root if missing), then `propagate(world, root, name, None)`. Also propagate when the stored value was equal but a bound widget differs (a game wrote the component): simplest is to always propagate after storing; `write_widget` and `format_node` write nothing when unchanged.
- `pub(crate) fn get(world, node, name) -> Option<BindValue>`.
- `pub(crate) fn apply_subtree(world, top, seeds)`: the seeding rule above.
- `pub(crate) fn sync_root(world, root)`: return at once if the root has no `UiBindings` or its index has no widgets; otherwise the sync rule above.
- `pub(crate) fn widget_wrote(world, node)`: `root_of(node)`; if the node's `widget_value` is not `same` as the stored value for its `UiBind` name, store it and `propagate(world, root, name, Some(node))`.

- [ ] **Step 5: Wire nodes, spawn, the walk and the API**

- `node.rs`: add the three fields to `UiNode` (and to `UiNode::of`'s initialiser and the destructuring in `spawn_node`), and:

```rust
  /// Tie a value widget (slider, progress, checkbox, toggle, radio group,
  /// dropdown, tabs, choice list, text input) to a named binding: the tree's
  /// `ctx.ui_set(root, name, ..)` writes it, and its changes set the name
  pub fn bind(mut self, name: impl Into<String>) -> UiNode {
    self.bind = Some(name.into());
    self.given("bind")
  }

  /// Read the node's label and tooltip as patterns: `{name}` shows a
  /// binding's value, `{name:.1}` with one decimal, `{{` a brace
  pub fn bound(mut self) -> UiNode {
    self.bound = true;
    self
  }
```

  `seed` lives in `bindings.rs` as an `impl UiNode` block (`pub(crate)`), pushing onto `seeds`. In `Kind::takes`, add `"bind"` to the bound kinds' lists (the `RadioGroup` arm's list, and the widget arm's `match` for `Slider`, `Progress`, `Checkbox`, `Toggle`, `Dropdown`, `Tabs`, `ChoiceList`, `TextInput`).

- `spawn.rs`, in `spawn_node`: after the state components, insert `UiBind(name)` when `bind` is `Some` and `kind.takes("bind")`; insert `UiPattern` when `bound` and the node has a label or tooltip (`Pattern::parse` each). Only the top node's seeds count (a child's are ignored): `spawn_ui` and `spawn_ui_in` take them first (`let seeds = std::mem::take(&mut tree.seeds);`), and after `finish` call `bindings::invalidate(world, top)` and `bindings::apply_subtree(world, top, seeds)`.

- `draw.rs`: in `draw_nodes`, call `bindings::sync_root(world, root)` just before `draw_node(world, ui, root, ..)`; in `UiContext::node` likewise before drawing the embedded root. In `draw_node`, after a bound widget's write (each arm listed in the rules), when `entity.contains::<UiBind>()`, call `entity.world_scope(|world| bindings::widget_wrote(world, node))`.

- `context.rs`, beside `find_ui_in`:

```rust
  /// Set a named binding on the UI tree `node` belongs to: every widget bound
  /// to `name` and every label or tooltip showing it update. Values are kept
  /// per root, so two trees from one file never share them.
  pub fn ui_set(&mut self, node: Entity, name: &str, value: impl Into<BindValue>) {
    nodes::bindings::set(self.world, node, name, value.into());
  }

  /// A named binding's value on the UI tree `node` belongs to, as `T`
  /// (`f32`, `f64`, `i32`, `u32`, `usize`, `bool` or `String`)
  pub fn ui_get<T: TryFrom<BindValue>>(&self, node: Entity, name: &str) -> Option<T> {
    nodes::bindings::get(self.world, node, name).and_then(|value| T::try_from(value).ok())
  }
```

  The same two methods on `UiHarness` over its own world. Export `BindValue` from `nodes/mod.rs` (`pub use bindings::BindValue;`, `pub(crate) mod bindings;`), from `src/core/ui/mod.rs`, and in the prelude in `src/lib.rs`.

- `components.rs`: correct `UiLabel`'s doc to "The text of a text, button, checkbox or toggle node".

- [ ] **Step 6: Run the tests to see them pass**

Run: `cargo test -p ryo2d --lib ui::nodes`
Expected: PASS, including all of 4c's node tests (bindings change nothing for unbound trees).

- [ ] **Step 7: Run the gate and commit**

```bash
git add src/core/ui/nodes/bindings.rs src/core/ui/nodes/node.rs src/core/ui/nodes/spawn.rs src/core/ui/nodes/draw.rs src/core/ui/nodes/components.rs src/core/ui/nodes/mod.rs src/core/ui/mod.rs src/core/ui/test_util.rs src/core/scene/context.rs src/core/testing/ui.rs src/lib.rs
git commit -m "feat: named bindings for UI node trees

- UiNode::bind ties a value widget to a name; UiNode::bound reads labels and tooltips as {name} patterns
- ctx.ui_set and ctx.ui_get keep values per root and write the bound components
- A widget's change reaches its name and every label showing it, in the same frame when drawn after it"
```

---

### Task 3: Keys and the per-node update

**Files:**
- Create: `src/core/ui/nodes/reconcile.rs`
- Modify: `src/core/ui/nodes/node.rs` (`key`, crate fields `key`, `template`, `instance`; `PartialEq` derives), `src/core/ui/nodes/components.rs` (`UiTreeNode`, `UiAdded`, `UiApplied`), `src/core/ui/nodes/spawn.rs` (keys, `UiTreeNode`, `UiApplied`, `UiAdded`), `src/core/ui/nodes/mod.rs`, `src/core/asset/manager.rs` (`PartialEq for SpriteHandle`)
- Test: `src/core/ui/nodes/reconcile.rs` (`mod tests`), `src/core/ui/nodes/spawn.rs` (key tests)

**Interfaces:**
- Consumes: Task 2's `UiBind`, `UiPattern`, `bindings::{format_node, invalidate}`.
- Produces:
  ```rust
  impl UiNode { pub fn key(self, key: impl Into<String>) -> UiNode }   // public
  // UiNode crate fields
  pub(crate) key: Option<String>, pub(crate) template: Option<String>, pub(crate) instance: u64,
  // components.rs (crate)
  #[derive(Component)] pub(crate) struct UiTreeNode { pub(crate) key: String, pub(crate) instance: u64 }
  #[derive(Component)] pub(crate) struct UiAdded;               // the top of a spawn_ui_in subtree
  #[derive(Component, Clone, Default)] pub(crate) struct UiApplied { /* below */ }
  // spawn.rs (crate)
  pub(crate) fn child_keys(children: &[UiNode]) -> Vec<String>;
  pub(crate) fn root_key(node: &UiNode) -> String;
  // reconcile.rs (crate)
  pub(crate) fn same_kind(a: &Kind, b: &Kind) -> bool;            // same constructor
  pub(crate) struct NodeUpdate { pub(crate) children: Vec<UiNode>, pub(crate) nav: [Option<String>; 4], pub(crate) popup: Option<String>, pub(crate) modal: bool }
  pub(crate) fn update_node(world: &mut World, entity: Entity, node: UiNode, key: String) -> NodeUpdate;
  impl PartialEq for SpriteHandle   // the same image: the same shared cell
  ```

`UiApplied` stores what the tree last gave a node, for the three-way rule:

```rust
/// What a node's tree last gave it, so an update writes only what the tree
/// changed and leaves what the game or the player wrote
#[derive(Component, Debug, Clone, Default)]
pub(crate) struct UiApplied {
  pub(crate) state: NodeState,
  pub(crate) tooltip: Option<String>,
  pub(crate) hidden: bool,
  pub(crate) disabled: bool,
  /// A root's placement as the tree gave it
  pub(crate) place: Option<(Anchor, Vec2, Option<Rect>)>,
}
```

Rules for `update_node(world, entity, node, key)` (the caller has checked `same_kind`):
- **Tree-owned**, rewritten when different (compare first; write only on a change): `UiNodeKind`'s `kind`, `layout`, `flags`, `variant`, `action` (its `nav` and `anchor_node` are re-resolved by Task 4's `finish`); `UiName` (insert, replace or remove); `UiBind` and `UiPattern` (Task 2's components, built as `spawn_node` builds them; on any change call `bindings::invalidate`, and if the pattern changed call `bindings::format_node` after the state step); `UiTreeNode { key, instance: node.instance }`; a root's `UiRoot.embedded`.
- **Three-way**, for each field of `UiApplied`: when the new value differs from `UiApplied`'s, write the component (insert it, or remove it for `None`/`false`); otherwise leave the live component alone. The fields are `state.label` → `UiLabel`, `state.image` → `UiImage`, `state.checked` → `UiChecked`, `state.value` → `UiValue`, `state.index` → `UiIndex`, `state.options` → `UiOptions`, `state.text` → `UiTextValue`, `state.slot` → `UiSlot` (compare icon, count, selected), `state.reveal` → `TextReveal` (compare `per_second`, `punctuation_pause`, `fade`; a change inserts the new one, restarting the reveal), `tooltip` → `UiTooltip`, `hidden` → `UiHidden`, `disabled` → `UiDisabled`, and for a root `place` → `UiRoot.anchor`/`offset`/`at` (the default anchor rule of `spawn_ui`: a modal root without an anchor is `Center`).
- **Two exceptions:** a node with `UiBind` skips its bound value field (`checked`, `value`, `index` or `text`: the binding owns it); a node whose `UiPattern` has a label pattern skips `label`, and one with a tooltip pattern skips `tooltip` (bindings format them).
- Then `UiApplied` is set to the new values. `node.inserts` are dropped (they apply only at spawn). The node's children, nav names, popup anchor name and `modal` are returned in `NodeUpdate` for Task 4.

Keys (`child_keys`): a child's `key`, else its `name`, else its label (`template` if set, else `kind.name()`) and `#` and its index among the siblings with that label that have no key or name:

```rust
pub(crate) fn child_keys(children: &[UiNode]) -> Vec<String> {
  let mut seen: HashMap<&str, usize> = HashMap::new();
  children
    .iter()
    .map(|child| {
      if let Some(key) = child.key.as_ref().or(child.name.as_ref()) {
        return key.clone();
      }
      let label = child.template.as_deref().unwrap_or(child.kind.name());
      let index = seen.entry(label).or_insert(0);
      let key = format!("{label}#{index}");
      *index += 1;
      key
    })
    .collect()
}
```

`root_key(node)` is the node's `key`, else `name`, else `format!("{label}#0")`. `spawn_node` computes its children's keys before spawning them, warns once on duplicates in a code tree (`warn_once(format!("ui-key-twice:{key}"), ..)`: "two UI nodes under one parent are keyed \"{key}\"; they match in order"), and inserts `UiTreeNode` and `UiApplied` on every node; `spawn_ui_in` also inserts `UiAdded` on its top.

- [ ] **Step 1: Write the failing tests**

In `spawn.rs` tests:

```rust
  #[test]
  fn keys_come_from_key_then_name_then_kind_and_index() {
    let children = vec![
      UiNode::button("A"),
      UiNode::button("B").name("b"),
      UiNode::button("C"),
      UiNode::text("T"),
      UiNode::button("D").key("d"),
    ];
    assert_eq!(child_keys(&children), ["button#0", "b", "button#1", "text#0", "d"]);
  }
```

In `reconcile.rs`:

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::testing::UiHarness;
  use crate::core::ui::nodes::*;
  use crate::prelude::*;

  fn one(h: &mut UiHarness, node: UiNode) -> Entity {
    h.spawn_ui(node)
  }

  fn update(h: &mut UiHarness, entity: Entity, node: UiNode) {
    let key = root_key(&node);
    update_node(h.world_mut(), entity, node, key);
  }

  #[test]
  fn a_tree_change_to_a_label_replaces_it_and_a_game_write_survives_an_unrelated_change() {
    let mut h = UiHarness::new(400.0, 300.0);
    let e = one(&mut h, UiNode::button("Play").width(100.0));
    h.world_mut().get_mut::<UiLabel>(e).unwrap().0 = "Game wrote".into();
    update(&mut h, e, UiNode::button("Play").width(140.0));
    assert_eq!(h.world().get::<UiLabel>(e).unwrap().0, "Game wrote", "the tree did not change the label");
    let kind = h.world().get::<UiNodeKind>(e).unwrap();
    assert_eq!(kind.layout.width, Some(Length::Px(140.0)), "layout is the tree's");
    update(&mut h, e, UiNode::button("Start").width(140.0));
    assert_eq!(h.world().get::<UiLabel>(e).unwrap().0, "Start", "the tree changed the label");
  }

  #[test]
  fn a_player_moved_slider_survives_and_a_tree_value_change_wins() {
    let mut h = UiHarness::new(400.0, 300.0);
    let e = one(&mut h, UiNode::slider(0.0..=1.0, 0.5));
    h.world_mut().get_mut::<UiValue>(e).unwrap().0 = 0.9;
    update(&mut h, e, UiNode::slider(0.0..=2.0, 0.5));
    assert_eq!(h.world().get::<UiValue>(e).unwrap().0, 0.9);
    update(&mut h, e, UiNode::slider(0.0..=2.0, 1.5));
    assert_eq!(h.world().get::<UiValue>(e).unwrap().0, 1.5);
  }

  #[test]
  fn a_bound_value_is_never_written_by_an_update() {
    let mut h = UiHarness::new(400.0, 300.0);
    let e = one(&mut h, UiNode::slider(0.0..=1.0, 0.5).bind("v"));
    update(&mut h, e, UiNode::slider(0.0..=1.0, 0.2).bind("v"));
    assert_eq!(h.world().get::<UiValue>(e).unwrap().0, 0.5, "the binding owns the value");
  }

  #[test]
  fn a_changed_pattern_formats_again_and_an_unchanged_one_keeps_the_label() {
    let mut h = UiHarness::new(400.0, 300.0);
    let e = one(&mut h, UiNode::text("Score {s}").bound());
    h.ui_set(e, "s", 3);
    update(&mut h, e, UiNode::text("Score {s}").bound().size(20.0));
    assert_eq!(h.world().get::<UiLabel>(e).unwrap().0, "Score 3");
    update(&mut h, e, UiNode::text("Points {s}").bound());
    assert_eq!(h.world().get::<UiLabel>(e).unwrap().0, "Points 3");
  }

  #[test]
  fn hidden_disabled_tooltip_and_name_follow_the_rules() {
    let mut h = UiHarness::new(400.0, 300.0);
    let e = one(&mut h, UiNode::button("A").tooltip("tip").name("a"));
    h.world_mut().entity_mut(e).insert(UiHidden);   // the game hid it
    update(&mut h, e, UiNode::button("A").tooltip("tip").name("b").enabled(false));
    assert!(h.world().get::<UiHidden>(e).is_some(), "the tree did not change hidden");
    assert!(h.world().get::<UiDisabled>(e).is_some(), "the tree disabled it");
    assert_eq!(h.world().get::<UiName>(e).unwrap().0, "b", "names are the tree's");
    update(&mut h, e, UiNode::button("A").name("b").enabled(false));
    assert!(h.world().get::<UiTooltip>(e).is_none(), "the tree removed the tooltip");
  }

  #[test]
  fn a_root_moved_by_the_game_stays_until_the_tree_moves_it() {
    let mut h = UiHarness::new(400.0, 300.0);
    let e = one(&mut h, UiNode::column().anchor(Anchor::TopLeft).offset(10.0, 10.0));
    h.world_mut().get_mut::<UiRoot>(e).unwrap().offset.y = 99.0;
    update(&mut h, e, UiNode::column().anchor(Anchor::TopLeft).offset(10.0, 10.0).gap(4.0));
    assert_eq!(h.world().get::<UiRoot>(e).unwrap().offset.y, 99.0);
    update(&mut h, e, UiNode::column().anchor(Anchor::TopLeft).offset(10.0, 20.0));
    assert_eq!(h.world().get::<UiRoot>(e).unwrap().offset.y, 20.0);
  }

  #[test]
  fn an_unchanged_update_writes_nothing() {
    let mut h = UiHarness::new(400.0, 300.0);
    let e = one(&mut h, UiNode::checkbox("Mute", true).tooltip("t").name("m"));
    h.world_mut().clear_trackers();
    update(&mut h, e, UiNode::checkbox("Mute", true).tooltip("t").name("m"));
    let entity = h.world().entity(e);
    assert!(!entity.get_ref::<UiNodeKind>().unwrap().is_changed());
    assert!(!entity.get_ref::<UiChecked>().unwrap().is_changed());
    assert!(!entity.get_ref::<UiLabel>().unwrap().is_changed());
    assert!(!entity.get_ref::<UiName>().unwrap().is_changed());
  }

  #[test]
  fn the_same_kind_means_the_same_constructor() {
    assert!(same_kind(&UiNode::slider(0.0..=1.0, 0.0).kind, &UiNode::slider(0.0..=9.0, 3.0).kind));
    assert!(!same_kind(&UiNode::checkbox("a", true).kind, &UiNode::toggle("a", true).kind));
  }
}
```

(`world.clear_trackers()` and `EntityRef::get_ref` are bevy's change-detection API; re-export anything missing through the kernel. If the harness does not expose `ui_set` with an arbitrary node, it does: Task 2 made `set` take any node of a tree.)

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p ryo2d --lib ui::nodes::reconcile ui::nodes::spawn`
Expected: FAIL to compile (`key`, `child_keys`, `update_node` and the rest do not exist).

- [ ] **Step 3: Implement**

- `node.rs`: add the three fields (initialised `None`, `None`, `0`), `pub fn key` (doc: "The key the node is matched by when its tree is updated or its file reloads; defaults to its name, else its kind and position"), and derive `PartialEq` on `Kind` and its option structs, `NodeLayout`, `NodeFlags` (`TextStyle`, `Color`, `Filter`, `Length`, `Edges`, `Align`, `Justify`, `Anchor`, `Rect` and `TextAlign` already have it or get a derive; `ButtonOpts.icon` compares through the new `SpriteHandle` `PartialEq`).
- `manager.rs`: `impl PartialEq for SpriteHandle { fn eq(&self, other: &Self) -> bool { Arc::ptr_eq(&self.inner, &other.inner) } }` with the doc "The same image: two handles share one cell (the asset cache returns the same cell for one path)", plus `impl Eq`.
- `same_kind`: `std::mem::discriminant(a) == std::mem::discriminant(b)`.
- `update_node` by the rules above. Keep it as small helpers, one per group: `update_settings`, `update_state`, `update_place`.

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test -p ryo2d --lib ui::nodes`
Expected: PASS.

- [ ] **Step 5: Run the gate and commit**

```bash
git add src/core/ui/nodes/reconcile.rs src/core/ui/nodes/node.rs src/core/ui/nodes/components.rs src/core/ui/nodes/spawn.rs src/core/ui/nodes/mod.rs src/core/asset/manager.rs
git commit -m "feat: keys and an in-place update for spawned UI nodes

- Nodes are keyed by key, name, or kind and position among their siblings
- An update rewrites the tree's own settings and writes state only where the tree changed it
- Values the game or the player wrote, and bound values, survive an update"
```

---

### Task 4: The tree reconcile and `ctx.update_ui`

**Files:**
- Modify: `src/core/ui/nodes/reconcile.rs` (`reconcile`, `Mode`), `src/core/ui/nodes/spawn.rs` (make `spawn_node`, `finish` and `Pending` `pub(crate)`; a helper to spawn a root into an existing roots slot), `src/core/scene/context.rs` (`update_ui`), `src/core/testing/ui.rs` (`update_ui`), `benches/ui.rs`
- Test: `src/core/ui/nodes/reconcile.rs` (`mod tree_tests`)

**Interfaces:**
- Consumes: Task 3's `update_node`, `NodeUpdate`, `same_kind`, `child_keys`, `UiTreeNode`, `UiAdded`; 4c's `spawn_node`, `finish`, `Pending`, `UiNodes`; Task 2's `bindings::apply_subtree`.
- Produces:
  ```rust
  pub(crate) enum Mode { Update, Reload { instance: u64 } }
  pub(crate) fn reconcile(world: &mut World, entity: Entity, tree: UiNode, mode: Mode) -> Entity;
  impl SceneContext<'_> { pub fn update_ui(&mut self, node: Entity, tree: UiNode) -> Entity }
  impl UiHarness { pub fn update_ui(&mut self, node: Entity, tree: UiNode) -> Entity }
  ```

Rules (the spec's reconcile section):
- `reconcile` updates `entity` from `tree` and walks the children; afterwards it runs 4c's `finish(world, top, pending)` over every node it updated or spawned (re-resolving `nav_*`, popup anchors and confirms' `BackAnswer`), then `bindings::invalidate(world, top)` and `bindings::apply_subtree(world, top, seeds)` with the tree's top seeds.
- **Which children the tree owns:** in `Mode::Update`, every child with `UiTreeNode` and without `UiAdded`; in `Mode::Reload { instance }`, every child with `UiTreeNode { instance, .. }` of that instance and without `UiAdded`. Other children (the game's) are left alone.
- **Matching** the tree's children by `child_keys` against the owned children's `UiTreeNode.key`, first come first matched for a repeated key: a match of the same kind is updated (recursively); a match of a different kind is despawned and a new node spawned in its place; an unmatched tree child is spawned (`spawn_node`); an unmatched owned child is despawned with its subtree.
- **Order:** the tree's children end in the tree's order. Each kept child (not owned) stays right after the owned sibling it followed before the reconcile; if that sibling was despawned, after the nearest earlier owned sibling that survived; if none, first. The parent's children are reordered only if the order changed.
- **The top's kind changed:** despawn `entity` and spawn `tree` in its place: same parent and position, or, for a root, the same scene and the same slot in `UiNodes::roots` (so it draws in the same order). Return the new entity.
- `update_ui(node, tree)`: a node without `UiTreeNode` warns once ("update_ui needs a node spawned from a UiNode tree") and is returned unchanged; otherwise `reconcile(.., Mode::Update)`.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tree_tests {
  use super::*;
  use crate::core::testing::UiHarness;
  use crate::core::ui::nodes::*;
  use crate::prelude::*;

  fn kids(h: &UiHarness, of: Entity) -> Vec<Entity> {
    h.world().get::<Children>(of).map(|c| c.to_vec()).unwrap_or_default()
  }

  fn menu(labels: &[&str]) -> UiNode {
    UiNode::column()
      .focus_scope()
      .children(labels.iter().map(|l| UiNode::button(*l).name(*l)))
  }

  #[test]
  fn keyed_children_keep_their_entities_through_add_remove_and_reorder() {
    let mut h = UiHarness::new(400.0, 300.0);
    let root = h.spawn_ui(menu(&["a", "b", "c"]));
    let before = kids(&h, root);
    let same = h.update_ui(root, menu(&["c", "a", "d"]));
    assert_eq!(same, root);
    let after = kids(&h, root);
    assert_eq!(after[0], before[2], "c moved first, same entity");
    assert_eq!(after[1], before[0], "a kept");
    assert!(!before.contains(&after[2]), "d is new");
    assert!(h.world().get_entity(before[1]).is_err(), "b despawned");
  }

  #[test]
  fn a_kind_change_respawns_in_place() {
    let mut h = UiHarness::new(400.0, 300.0);
    let root = h.spawn_ui(UiNode::column().child(UiNode::button("x").key("k")).child(UiNode::text("after")));
    let old = kids(&h, root)[0];
    h.update_ui(root, UiNode::column().child(UiNode::checkbox("x", true).key("k")).child(UiNode::text("after")));
    let now = kids(&h, root);
    assert_ne!(now[0], old);
    assert_eq!(h.world().get::<UiNodeKind>(now[0]).unwrap().name(), "checkbox");
    assert_eq!(now.len(), 2);
  }

  #[test]
  fn a_root_kind_change_returns_the_new_root_in_the_same_draw_slot() {
    let mut h = UiHarness::new(400.0, 300.0);
    let first = h.spawn_ui(UiNode::column());
    let _second = h.spawn_ui(UiNode::column());
    let new = h.update_ui(first, UiNode::row());
    assert_ne!(new, first);
    let roots = &h.world().resource::<UiNodes>().roots;
    assert_eq!(roots[0].1, new, "drawn where the old root was");
  }

  #[test]
  fn the_games_children_and_components_stay() {
    let mut h = UiHarness::new(400.0, 300.0);
    #[derive(Component)]
    struct Mine;
    let root = h.spawn_ui(UiNode::column().child(UiNode::text("head")).child(UiNode::column().name("extras")));
    let extras = h.find_ui("extras").unwrap();
    let added = spawn_ui_in(h.world_mut(), extras, UiNode::button("game's")).unwrap();
    let head = kids(&h, root)[0];
    h.world_mut().entity_mut(head).insert(Mine);
    h.update_ui(root, UiNode::column().gap(3.0).child(UiNode::text("head")).child(UiNode::column().name("extras")));
    assert_eq!(kids(&h, extras), [added], "spawn_ui_in subtrees are the game's");
    assert!(h.world().get::<Mine>(head).is_some(), "inserted components stay");
  }

  #[test]
  fn a_kept_child_stays_after_the_sibling_it_followed() {
    let mut h = UiHarness::new(400.0, 300.0);
    let root = h.spawn_ui(menu(&["a", "b"]));
    let added = spawn_ui_in(h.world_mut(), root, UiNode::text("game's")).unwrap();
    // move the game's node between a and b
    let a = kids(&h, root)[0];
    h.world_mut().entity_mut(root).insert_children(1, &[added]);
    h.update_ui(root, menu(&["a", "z", "b"]));
    let now = kids(&h, root);
    assert_eq!(now[0], a);
    assert_eq!(now[1], added, "still right after a");
  }

  #[test]
  fn focus_and_ids_survive_an_update_and_nothing_is_sent() {
    let mut h = UiHarness::new(400.0, 300.0);
    let root = h.spawn_ui(menu(&["a", "b"]));
    h.frame(|_| {});
    h.action("ui.down");
    h.frame(|_| {});
    let focused = h.focused();
    h.update_ui(root, menu(&["a", "b"]).gap(10.0));
    h.frame(|_| {});
    assert_eq!(h.focused(), focused);
    assert!(h.events().is_empty(), "a reconcile sends no UiEvents");
  }

  #[test]
  fn nav_names_resolve_again_after_an_update() {
    let mut h = UiHarness::new(400.0, 300.0);
    let tree = |target: &str| {
      UiNode::column()
        .child(UiNode::button("a").name("a").nav_down(target))
        .child(UiNode::button("b").name("b"))
        .child(UiNode::button("c").name("c"))
    };
    let root = h.spawn_ui(tree("b"));
    h.update_ui(root, tree("c"));
    let a = h.find_ui("a").unwrap();
    let c = h.find_ui("c").unwrap();
    assert_eq!(h.world().get::<UiNodeKind>(a).unwrap().nav[1], Some(c));
  }

  #[test]
  fn reload_mode_leaves_other_instances_alone() {
    let mut h = UiHarness::new(400.0, 300.0);
    let mut file_child = UiNode::text("from the file");
    file_child.instance = 7;
    let mut code_child = UiNode::text("from code").key("code");
    code_child.instance = 0;
    let mut top = UiNode::column().child(file_child).child(code_child);
    top.instance = 7;
    let root = h.spawn_ui(top);
    let mut again = UiNode::column();
    again.instance = 7;
    reconcile(h.world_mut(), root, again, Mode::Reload { instance: 7 });
    let now = kids(&h, root);
    assert_eq!(now.len(), 1, "the file's child went; the code child stayed");
    assert_eq!(h.world().get::<UiLabel>(now[0]).unwrap().0, "from code");
  }

  #[test]
  fn update_ui_on_a_plain_entity_warns_and_returns_it() {
    let mut h = UiHarness::new(400.0, 300.0);
    let plain = h.world_mut().spawn_empty().id();
    assert_eq!(h.update_ui(plain, UiNode::column()), plain);
    assert_eq!(crate::core::ui::test_util::warnings_containing("update_ui needs a node"), 1);
  }
}
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p ryo2d --lib ui::nodes::reconcile::tree_tests`
Expected: FAIL to compile (`reconcile`, `Mode`, `update_ui` do not exist).

- [ ] **Step 3: Implement**

The shape of the reconcile (fill in the order and respawn helpers by the rules):

```rust
/// Which children of a node an update may touch
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
  /// `ctx.update_ui`: every child the tree spawned
  Update,
  /// A file reload: only the children built from the same file instance
  Reload { instance: u64 },
}

/// Bring `entity`'s spawned tree in line with `tree`; the entity, or its
/// replacement if `tree`'s top is a different kind
pub(crate) fn reconcile(world: &mut World, entity: Entity, mut tree: UiNode, mode: Mode) -> Entity {
  // Only the top's seeds count, as at spawn
  let seeds = std::mem::take(&mut tree.seeds);
  let mut pending = Vec::new();
  let top = reconcile_node(world, entity, tree, mode, &mut pending);
  finish(world, top, pending);
  bindings::invalidate(world, top);
  bindings::apply_subtree(world, top, seeds);
  top
}

fn reconcile_node(world: &mut World, entity: Entity, tree: UiNode, mode: Mode, pending: &mut Vec<Pending>) -> Entity {
  let same = world
    .get::<UiNodeKind>(entity)
    .is_some_and(|node| same_kind(&node.kind, &tree.kind));
  if !same {
    return respawn_in_place(world, entity, tree, pending);
  }
  let key = world
    .get::<UiTreeNode>(entity)
    .map(|node| node.key.clone())
    .unwrap_or_else(|| root_key(&tree));
  let NodeUpdate { children, nav, popup, modal } = update_node(world, entity, tree, key);
  pending.push(Pending { entity, nav, popup, modal });
  reconcile_children(world, entity, children, mode, pending);
  entity
}
```

`reconcile_children` collects the parent's `Children`, splits owned from kept by `Mode`, matches by key with a `HashMap<String, VecDeque<Entity>>`, and reorders by the rules. Moving an existing child: bevy's `EntityWorldMut::insert_children(index, &[child])` re-places an entity already in the list; prefer computing the whole final order and writing it once only when it differs. Matched children are passed their new key through `update_node`.

`SceneContext::update_ui` and `UiHarness::update_ui` call `nodes::reconcile(.., Mode::Update)` after the `UiTreeNode` check (export `reconcile` and `Mode` from `nodes/mod.rs` as `pub(crate)`).

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test -p ryo2d --lib ui::nodes`
Expected: PASS.

- [ ] **Step 5: Add the bench and check the guard**

In `benches/ui.rs`, add (and register both in `criterion_group!`; import `criterion::BatchSize`):

```rust
fn settings_update_unchanged(c: &mut Criterion) {
  let settings = Settings::new();
  let mut harness = UiHarness::new(640.0, 480.0);
  let root = harness.spawn_ui(settings_nodes(&settings));
  harness.frame(|_| {});
  c.bench_function("ui update, the 200-widget settings tree unchanged", |b| {
    b.iter_batched(
      || settings_nodes(&settings),
      |tree| harness.update_ui(root, tree),
      BatchSize::SmallInput,
    )
  });
}

fn settings_spawn(c: &mut Criterion) {
  let settings = Settings::new();
  let mut harness = UiHarness::new(640.0, 480.0);
  c.bench_function("ui spawn, the 200-widget settings tree (and despawn)", |b| {
    b.iter_batched(
      || settings_nodes(&settings),
      |tree| {
        let root = harness.spawn_ui(tree);
        harness.world_mut().despawn(root);
      },
      BatchSize::SmallInput,
    )
  });
}
```

Run: `cargo bench --bench ui -- "settings tree"`
Expected: the unchanged update's time is below the spawn's. Record both numbers in the task report. If the update is not cheaper, profile `update_node`'s comparisons (the usual cause is cloning `Kind` or options to compare them; compare by reference) before going on. Also run `cargo bench --bench ui -- "settings screen"` and check the 4c guard: the node settings frame within 10% of the closure one.

- [ ] **Step 6: Run the gate and commit**

```bash
git add src/core/ui/nodes/reconcile.rs src/core/ui/nodes/spawn.rs src/core/ui/nodes/mod.rs src/core/scene/context.rs src/core/testing/ui.rs benches/ui.rs
git commit -m "feat: ctx.update_ui reconciles a spawned UI tree in place

- Children match by key; new ones spawn, missing ones despawn, a changed kind respawns in place
- Subtrees the game added and its components stay; focus and widget state survive
- A reload mode touches only one file instance's nodes; bench against spawning"
```

---
### Task 5: The vocabulary and building plain nodes

**Files:**
- Create: `src/core/ui/file/vocab.rs`, `src/core/ui/file/build.rs`, `src/core/ui/file/error.rs`
- Modify: `src/core/ui/file/mod.rs`, `src/core/ui/file/reader.rs` (remove Task 1's dead-code allowance), `src/core/ui/nodes/node.rs` (a test-only `fingerprint`)
- Test: `src/core/ui/file/build.rs` (`mod tests`)

**Interfaces:**
- Consumes: Task 1's `Value`, `ValueKind`, `Body`, `Field`, `Pos`, `Issue`; Task 2's `UiNode::bind`, `UiNode::bound`; Task 3's `UiNode::key`; 4c's `UiNode` constructors and methods and `Kind::takes`.
- Produces (crate):
  ```rust
  // build.rs
  pub(crate) trait SpriteSource { fn sprite(&mut self, path: &str) -> Result<SpriteHandle, String>; }
  #[derive(Default)] pub(crate) struct Placeholders { /* path -> a shared 1x1 handle */ }   // impl SpriteSource; never fails
  pub(crate) struct BuildCx<'a> {
    pub(crate) sprites: &'a mut dyn SpriteSource,
    pub(crate) issues: Vec<Issue>,
    // Task 6 adds: the template scope, the expansion stack, the instance id
  }
  impl BuildCx<'_> { pub(crate) fn new(sprites: &mut dyn SpriteSource) -> BuildCx<'_> }
  pub(crate) fn build_node(value: &Value, cx: &mut BuildCx<'_>) -> Option<UiNode>;
  // vocab.rs
  pub(crate) enum Form { .. }                         // how a value is written
  pub(crate) struct OptionSpec { pub(crate) name: &'static str, pub(crate) form: Form, pub(crate) apply: fn(UiNode, &Value, &mut BuildCx<'_>) -> UiNode }
  pub(crate) struct KindSpec { pub(crate) name: &'static str, pub(crate) main: Option<&'static str>, pub(crate) args: &'static [ArgSpec], pub(crate) make: fn(&Ctor<'_>, &mut BuildCx<'_>) -> Option<UiNode> }
  pub(crate) struct ArgSpec { pub(crate) name: &'static str, pub(crate) form: Form, pub(crate) required: bool }
  pub(crate) static KINDS: &[KindSpec];
  pub(crate) static OPTIONS: &[OptionSpec];
  pub(crate) fn kind_spec(name: &str) -> Option<&'static KindSpec>;
  pub(crate) fn option_spec(name: &str) -> Option<&'static OptionSpec>;
  pub(crate) fn nearest<'a>(word: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<&'a str>;
  // node.rs, #[cfg(test)], re-exported from nodes/mod.rs as #[cfg(test)] pub(crate) use node::fingerprint;
  pub(crate) fn fingerprint(node: &UiNode) -> String;
  ```

The vocabulary is the spec's two tables (Nodes, Option values). Each kind is a `KindSpec` whose `make` calls the `UiNode` constructor; each option is an `OptionSpec` whose `apply` calls the `UiNode` method of the same name. Rules:

- **Building a node** (`build_node`): the value must be `Named`; the name must be a kind (Task 6 adds templates). `Body::Single(v)` is the kind's `main` option (`Slider has no short form; name its options` when it has none); `Body::Unit` gives no options; `Body::Fields` gives constructor arguments (the kind's `args`) and options. Missing required arguments: `Slider needs "range"`. The node is made with the arguments, then each other field is applied in file order: `children` (only where `kind.takes("child")`), the universal options (`name`, `action`, `hidden`, `key`, applying everywhere), `bind` (where `kind.takes("bind")`), and every other option where `kind.takes(option)`.
- **Every node a file builds is `bound()`**, including the nodes inside a `Confirm`: in a file, labels and tooltips are patterns.
- **Messages** (each an `Issue` at the field's or value's position, collected; building goes on past an error, skipping what failed):
  - unknown kind: `unknown node kind "Buton"; did you mean "Button"?` (the suggestion only when `nearest` finds one; same for the others);
  - unknown option: `Button has no option "fil_width"; did you mean "fill_width"?` (candidates: the options this kind takes);
  - an option of another kind: `Button does not take "step"`;
  - a wrong value: `"gap" takes a number, not a string` (the form's description below; the value's: a string, a number, a bool, a list, a tuple, a map, fields, or the name it is);
  - an enum name: `"anchor" takes one of TopLeft, TopCenter, ...; "Middle" is not one`, plus a suggestion;
  - a duplicate sibling key: `two children are keyed "resume"; give one a different key or name`, at the second;
  - a lowercase bare name outside a template: `"label" is not a value here; parameters exist only inside templates`.
- **Forms and their descriptions:** `Flag` "`true` or `false`" (true applies the no-argument method); `Bool` "a bool"; `Number` "a number"; `Whole` "a whole number" (non-negative integer); `Length` "a length (a number, Pct(..), Fill or Fit)"; `Edges` "a number, (x, y) or (left: .., top: .., right: .., bottom: ..)"; `Text` "a string"; `Asset` "an asset path (a string)"; `Color` "a colour (\"#rrggbb\", \"#rrggbbaa\" or (r: .., g: .., b: .., a: ..))"; `Pair` "(x, y)"; `Rect` "(x, y, width, height)"; `Quad` "four numbers"; `Numbers` "a list of numbers"; `Strings` "a list of strings"; `Range` "(min, max)"; `Typewriter` "a number or (per_second: .., punctuation_pause: .., fade: ..)"; `Enum(names)` "one of ...".
- **Colours:** `"#rrggbb"` and `"#rrggbbaa"` (hex, either case) and `(r:, g:, b:, a:)` with whole 0-255 values (`a` defaults to 255) make `Color::new(r, g, b, a)`. **Lengths:** a number is `Length::Px`, `Pct(x)` is `Length::Pct(x)`, `Fill` and `Fit` the variants. **Edges:** a number, `(x, y)` (both through `Edges::from`), or the four named fields (missing ones 0). **Typewriter:** a number is `TextReveal::new(n)`; the fields set `per_second` (required), `punctuation_pause` and `fade` (the public fields; defaults from `TextReveal::new`).
- **Images:** an `Asset` value asks `cx.sprites.sprite(path)`; an `Err(message)` becomes an `Issue` at the value: `cannot load the image "path": {message}`.

The kind and option entries, exactly (method names are `UiNode`'s):

| File kind | `make` | `main` | args (required in bold) |
|---|---|---|---|
| `Column`, `Row`, `Stack`, `Panel`, `Scroll`, `Slot`, `Separator`, `Spacer` | `column()` ... `spacer()` | none | none |
| `Grid` | `grid(columns as u16)` | `columns` | **`columns`** Whole |
| `Popup` | `popup(anchor_node)` | none | **`anchor_node`** Text |
| `Text` | `text(text)` | `text` | `text` Text |
| `Button` | `button(label)` | `label` | `label` Text |
| `Image` | `image(sprite)` | `sprite` | **`sprite`** Asset |
| `Checkbox` | `checkbox(label, checked)` | `label` | `label` Text, `checked` Bool |
| `Toggle` | `toggle(label, on)` | `label` | `label` Text, `on` Bool |
| `RadioGroup`, `Dropdown`, `Tabs` | `radio_group`/`dropdown`/`tabs(&options, selected)` | none | **`options`** Strings, `selected` Whole |
| `ChoiceList` | `choice_list(&options)` | none | **`options`** Strings |
| `Slider` | `slider(min..=max, value)`, value defaulting to min | none | **`range`** Range, `value` Number |
| `Progress` | `progress(fraction)` | `fraction` | `fraction` Number |
| `TextInput` | `text_input(text)` | `text` | `text` Text |
| `Prompt`, `Rebind` | `prompt(input)`/`rebind(input)` | `input` | **`input`** Text |
| `Confirm` | `confirm(title, message, yes, no)` | none | **`title`** Text, `message` Text (""), `yes` Text ("Yes"), `no` Text ("No") |

`selected` is a constructor argument on `RadioGroup`, `Dropdown` and `Tabs` (an index) and an option on `Slot` (a bool): look up a kind's args before the options table.

| Options (Form) | Methods |
|---|---|
| Flag | `fill_width`, `fill_height`, `fill`, `center`, `wrap_items`, `focus_scope`, `trap`, `wrap_focus`, `block_pointer`, `default_focus`, `nav_skip`, `drag_source`, `drop_target`, `modal`, `hidden`, `embedded`, `markup`, `heading`, `muted`, `ellipsis`, `show_value`, `password`, `horizontal`, `both`, `no_bar` |
| Bool | `enabled`, `focusable`, `selected` |
| Number | `min_width`, `max_width`, `min_height`, `max_height`, `gap`, `grow`, `aspect`, `size`, `wrap`, `step` |
| Whole | `max_lines` (u32), `max_len` (usize), `default_choice` (usize), `count` (u32), `binding_slot` (usize) |
| Length | `width`, `height` |
| Edges | `padding`, `margin` |
| Enum | `align` (Start, Center, End, Stretch), `justify` (Start, Center, End, SpaceBetween, SpaceAround, SpaceEvenly), `anchor` (the nine `Anchor`s), `text_align` (Start, Center, End, Justify), `filter` (Nearest, Linear), `role` (Body, Heading, Button, Caption, Mono) |
| Pair | `offset` (x, y), `image_size` (width, height) |
| Rect | `at` |
| Quad | `uv` |
| Numbers | `tracks` |
| Color | `background`, `color`, `tint` |
| Text | `tooltip`, `variant`, `action`, `name`, `key`, `bind`, `nav_up`, `nav_down`, `nav_left`, `nav_right`, `label`, `placeholder` |
| Asset | `icon` |
| Typewriter | `typewriter` |

Not in the table, and so "has no option": `insert`, `style`, `child` (use `children`).

- [ ] **Step 1: Add the fingerprint helper**

In `node.rs`, inside a `#[cfg(test)]` block outside `mod tests`:

```rust
/// Everything a node was given, as text, for comparing a file-built node with
/// a code-built one; sprites by path, since each handle has its own serial
#[cfg(test)]
pub(crate) fn fingerprint(node: &UiNode) -> String {
  let sprite = |handle: &Option<SpriteHandle>| handle.as_ref().map(|h| h.path().display().to_string());
  let state = &node.state;
  format!(
    "{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{}|{}|{}|{:?}|label={:?} image={:?} checked={:?} value={:?} index={:?} options={:?} text={:?} slot={:?} reveal={:?}|key={:?} bind={:?} bound={}|[{}]",
    node.kind, node.layout, node.flags, node.variant, node.action, node.name, node.tooltip,
    node.hidden, node.disabled, node.embedded, node.nav,
    state.label, sprite(&state.image), state.checked, state.value, state.index, state.options, state.text,
    state.slot.as_ref().map(|s| (sprite(&s.icon), s.count, s.selected)),
    state.reveal.as_ref().map(|r| (r.per_second, r.punctuation_pause, r.fade)),
    node.key, node.bind, node.bound,
    node.children.iter().map(fingerprint).collect::<Vec<_>>().join(", "),
  )
}
```

If `Kind`'s `Debug` prints a `SpriteHandle` serial (a button's icon), give `ButtonOpts` a manual `Debug` that prints the icon's path.

- [ ] **Step 2: Write the failing tests**

In `build.rs`:

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::graphics::Sprite;
  use crate::core::ui::file::reader::read;
  use crate::core::ui::nodes::fingerprint;
  use crate::prelude::*;

  fn build(text: &str) -> (Option<UiNode>, Vec<Issue>) {
    let value = read(text).expect("reads");
    let mut sprites = Placeholders::default();
    let mut cx = BuildCx::new(&mut sprites);
    let node = build_node(&value, &mut cx);
    (node, cx.issues)
  }

  fn same(text: &str, code: UiNode) {
    let (node, issues) = build(text);
    assert!(issues.is_empty(), "{text}: {issues:?}");
    assert_eq!(fingerprint(&node.expect("built")), fingerprint(&bound_all(code)), "{text}");
  }

  /// A code node as a file builds it: every node bound
  fn bound_all(mut node: UiNode) -> UiNode {
    node.bound = true;
    node.children = node.children.into_iter().map(bound_all).collect();
    node
  }

  fn sprite(path: &str) -> SpriteHandle {
    SpriteHandle::new(Sprite::solid_rect(1, 1, 0, 0, 0, 0), path.into())
  }

  fn messages(text: &str) -> Vec<String> {
    build(text).1.into_iter().map(|i| i.message).collect()
  }

  #[test]
  fn every_kind_builds_what_its_constructor_builds() {
    same("Column", UiNode::column());
    same("Row", UiNode::row());
    same("Stack", UiNode::stack());
    same("Panel", UiNode::panel());
    same("Scroll", UiNode::scroll());
    same("Grid(3)", UiNode::grid(3));
    same("Popup(anchor_node: \"sort\")", UiNode::popup("sort"));
    same("Text(\"Hi\")", UiNode::text("Hi"));
    same("Button(label: \"Go\")", UiNode::button("Go"));
    same("Image(\"items/sword.png\")", UiNode::image(sprite("items/sword.png")));
    same("Checkbox(label: \"Mute\", checked: true)", UiNode::checkbox("Mute", true));
    same("Toggle(\"Full\")", UiNode::toggle("Full", false));
    same("RadioGroup(options: [\"S\", \"M\"], selected: 1)", UiNode::radio_group(&["S", "M"], 1));
    same("Dropdown(options: [\"A\", \"B\"])", UiNode::dropdown(&["A", "B"], 0));
    same("Tabs(options: [\"A\", \"B\"], selected: 1)", UiNode::tabs(&["A", "B"], 1));
    same("ChoiceList(options: [\"Yes\", \"No\"])", UiNode::choice_list(&["Yes", "No"]));
    same("Slider(range: (0, 10), value: 4)", UiNode::slider(0.0..=10.0, 4.0));
    same("Slider(range: (2, 10))", UiNode::slider(2.0..=10.0, 2.0));
    same("Progress(0.25)", UiNode::progress(0.25));
    same("TextInput(text: \"Kai\")", UiNode::text_input("Kai"));
    same("Slot", UiNode::slot());
    same("Prompt(\"jump\")", UiNode::prompt("jump"));
    same("Rebind(input: \"jump\")", UiNode::rebind("jump"));
    same("Separator", UiNode::separator());
    same("Spacer", UiNode::spacer());
    same("Confirm(title: \"Quit?\")", UiNode::confirm("Quit?", "", "Yes", "No"));
  }

  #[test]
  fn every_option_applies_the_method_of_its_name() {
    same(
      "Column(width: 240, height: Pct(0.5), min_width: 1, max_width: 2, min_height: 3, max_height: 4, padding: (8, 12), margin: (left: 1, top: 2, right: 3, bottom: 4), gap: 6, align: Center, justify: SpaceBetween, grow: 1, wrap_items: true, aspect: 2, anchor: BottomRight, offset: (5, 6), background: \"#ff8800\", focus_scope: true, trap: true, wrap_focus: true, block_pointer: true, drag_source: true, drop_target: true, modal: true, embedded: true, name: \"n\", action: \"a\", key: \"k\", hidden: true)",
      UiNode::column().width(240.0).height(Length::Pct(0.5)).min_width(1.0).max_width(2.0).min_height(3.0).max_height(4.0)
        .padding((8.0, 12.0)).margin(Edges { left: 1.0, top: 2.0, right: 3.0, bottom: 4.0 }).gap(6.0).align(Align::Center)
        .justify(Justify::SpaceBetween).grow(1.0).wrap_items().aspect(2.0).anchor(Anchor::BottomRight).offset(5.0, 6.0)
        .background(Color::new(255, 136, 0, 255)).focus_scope().trap().wrap_focus().block_pointer().drag_source()
        .drop_target().modal().embedded().name("n").action("a").key("k").hidden(),
    );
    same("Column(fill: true, center: true)", UiNode::column().fill().center());
    same("Row(fill_width: true, fill_height: true, at: (1, 2, 3, 4))", UiNode::row().fill_width().fill_height().at(Rect::new(1.0, 2.0, 3.0, 4.0)));
    same("Grid(columns: 3, tracks: [1, 2, 1])", UiNode::grid(3).tracks(&[1.0, 2.0, 1.0]));
    same("Scroll(horizontal: true, both: true, no_bar: true)", UiNode::scroll().horizontal().both().no_bar());
    same(
      "Text(text: \"t\", markup: true, role: Caption, heading: true, size: 20, color: (r: 1, g: 2, b: 3), muted: true, text_align: Justify, wrap: 200, max_lines: 2, ellipsis: true, typewriter: (per_second: 40, punctuation_pause: 0.15))",
      UiNode::text("t").markup().role(TextRole::Caption).heading().size(20.0).color(Color::new(1, 2, 3, 255)).muted()
        .text_align(TextAlign::Justify).wrap(200.0).max_lines(2).ellipsis()
        .typewriter(TextReveal::new(40.0).with_punctuation_pause(0.15)),
    );
    same(
      "Button(label: \"b\", icon: \"i.png\", default_focus: true, nav_skip: true, focusable: false, nav_up: \"u\", nav_down: \"d\", nav_left: \"l\", nav_right: \"r\", tooltip: \"tip\", enabled: false, variant: \"danger\")",
      UiNode::button("b").icon(sprite("i.png")).default_focus().nav_skip().focusable(false).nav_up("u").nav_down("d")
        .nav_left("l").nav_right("r").tooltip("tip").enabled(false).variant("danger"),
    );
    same(
      "Image(sprite: \"a.png\", image_size: (16, 16), tint: \"#11223344\", filter: Nearest, uv: (0, 0, 0.5, 0.5))",
      UiNode::image(sprite("a.png")).image_size(16.0, 16.0).tint(Color::new(0x11, 0x22, 0x33, 0x44)).filter(Filter::Nearest).uv([0.0, 0.0, 0.5, 0.5]),
    );
    same("Slider(range: (0, 1), step: 0.1, show_value: true, label: \"Vol\", bind: \"v\")", UiNode::slider(0.0..=1.0, 0.0).step(0.1).show_value().label("Vol").bind("v"));
    same("TextInput(placeholder: \"name\", max_len: 12, password: true)", UiNode::text_input("").placeholder("name").max_len(12).password());
    same("ChoiceList(options: [\"a\"], default_choice: 0)", UiNode::choice_list(&["a"]).default_choice(0));
    same("Slot(icon: \"s.png\", count: 3, selected: true, filter: Linear)", UiNode::slot().icon(sprite("s.png")).count(3).selected(true).filter(Filter::Linear));
    same("Rebind(input: \"jump\", label: \"Jump\", binding_slot: 1)", UiNode::rebind("jump").label("Jump").binding_slot(1));
    same("Progress(fraction: 0.5, label: \"HP\")", UiNode::progress(0.5).label("HP"));
    same("RadioGroup(options: [\"a\", \"b\"], horizontal: true)", UiNode::radio_group(&["a", "b"], 0).horizontal());
    same("Column(width: Fill, height: Fit)", UiNode::column().width(Length::Fill).height(Length::Fit));
  }

  #[test]
  fn children_build_in_order() {
    same(
      "Column(gap: 8, children: [Text(\"a\"), Button(\"b\"), Separator])",
      UiNode::column().gap(8.0).child(UiNode::text("a")).child(UiNode::button("b")).child(UiNode::separator()),
    );
  }

  #[test]
  fn mistakes_name_the_fix() {
    assert_eq!(messages("Buton"), ["unknown node kind \"Buton\"; did you mean \"Button\"?"]);
    assert_eq!(messages("Button(label: \"x\", fil_width: true)"), ["Button has no option \"fil_width\"; did you mean \"fill_width\"?"]);
    assert_eq!(messages("Button(label: \"x\", step: 1)"), ["Button does not take \"step\""]);
    assert_eq!(messages("Column(gap: \"8\")"), ["\"gap\" takes a number, not a string"]);
    assert_eq!(messages("Slider(value: 1)"), ["Slider needs \"range\""]);
    assert_eq!(messages("Slider(1)"), ["Slider has no short form; name its options"]);
    assert_eq!(messages("Button(label: \"x\", children: [])"), ["Button does not take \"children\""]);
    assert!(messages("Column(anchor: Middle)")[0].contains("\"Middle\" is not one"));
    assert_eq!(messages("Column(children: [Buton, Button(name: \"r\"), Button(name: \"r\")])").len(), 2, "a broken child and a duplicate key, both reported");
    assert!(messages("Column(children: [Button(name: \"r\"), Button(name: \"r\")])")[0].contains("two children are keyed \"r\""));
    assert!(messages("Column(width: label)")[0].contains("parameters exist only inside templates"));
    assert!(messages("Button(label: \"x\", insert: 1)")[0].contains("has no option \"insert\""));
  }

  #[test]
  fn an_error_skips_its_option_and_keeps_building() {
    let (node, issues) = build("Column(gap: \"x\", width: 100)");
    assert_eq!(issues.len(), 1);
    assert_eq!(node.unwrap().layout.width, Some(Length::Px(100.0)));
  }

  #[test]
  fn positions_point_at_the_mistake() {
    let (_, issues) = build("Column(\n  gap: 4,\n  wdth: 1,\n)");
    assert_eq!(issues[0].pos, crate::core::ui::file::reader::Pos { line: 3, column: 3 });
  }

  #[test]
  fn a_failing_image_is_an_issue_at_its_value() {
    struct Missing;
    impl SpriteSource for Missing {
      fn sprite(&mut self, path: &str) -> Result<SpriteHandle, String> {
        Err(format!("no file {path}"))
      }
    }
    let value = read("Image(sprite: \"gone.png\")").unwrap();
    let mut missing = Missing;
    let mut cx = BuildCx::new(&mut missing);
    build_node(&value, &mut cx);
    assert_eq!(cx.issues.len(), 1);
    assert!(cx.issues[0].message.contains("cannot load the image \"gone.png\""));
    assert_eq!(cx.issues[0].pos.column, 15);
  }

  #[test]
  fn nearest_suggests_only_close_names() {
    assert_eq!(nearest("fil_width", ["fill_width", "width"]), Some("fill_width"));
    assert_eq!(nearest("colour", ["color", "size"]), Some("color"));
    assert_eq!(nearest("zzz", ["fill_width", "width"]), None);
  }
}
```

(If a constructor or method is spelled differently from the table, for example `TextReveal::with_punctuation_pause`, follow the code and say so in the report.)

- [ ] **Step 3: Run the tests to see them fail**

Run: `cargo test -p ryo2d --lib ui::file::build`
Expected: FAIL to compile.

- [ ] **Step 4: Implement**

`error.rs` for now holds a helper to describe a value for messages (`fn describe(kind: &ValueKind) -> String`: "a string", "a number", "a bool", "a list", "a tuple", "a map", "fields", or `` `Name` ``); Task 6 adds `UiFileError` there.

`vocab.rs`: the `Form` enum, `OptionSpec`, `ArgSpec`, `KindSpec`, `Ctor` (the resolved constructor arguments, read with `ctor.text("label")`, `ctor.whole("selected")`, `ctor.strings("options")`, ...), the `KINDS` and `OPTIONS` tables built from the two tables above, `kind_spec`, `option_spec`, and `nearest`: case-insensitive Levenshtein distance, accepting a distance up to `max(1, word.len() / 3)` and returning the closest (ties: the first). Write the option entries with a small macro so each is one line:

```rust
macro_rules! option {
  ($name:literal, Flag, $method:ident) => {
    OptionSpec { name: $name, form: Form::Flag, apply: |node, value, cx| if cx.flag(value) { node.$method() } else { node } }
  };
  ($name:literal, $form:ident, $read:ident, $method:ident) => {
    OptionSpec { name: $name, form: Form::$form, apply: |node, value, cx| match cx.$read(value) { Some(v) => node.$method(v), None => node } }
  };
}
```

Options whose method takes two numbers (`offset`, `image_size`) or a converted value (`at`, `uv`, `tracks`, `typewriter`, `icon`) get a hand-written `apply`.

`build.rs`: `SpriteSource`, `Placeholders` (a `HashMap<String, SpriteHandle>` of `SpriteHandle::new(Sprite::solid_rect(1, 1, 0, 0, 0, 0), path.into())`, the same handle for the same path), `BuildCx` with one reader per form (`flag`, `bool`, `number`, `whole`, `length`, `edges`, `text`, `asset`, `color`, `pair`, `rect`, `quad`, `numbers`, `strings`, `range`, `typewriter`, `enum_name`), each recording an `Issue` and returning `None` on a mismatch, and `build_node` by the rules above. A `ValueKind::Any` value is accepted by every reader and applies nothing (each reader returns `None` without an issue).

- [ ] **Step 5: Run the tests to see them pass**

Run: `cargo test -p ryo2d --lib ui::file`
Expected: PASS.

- [ ] **Step 6: Run the gate and commit**

```bash
git add src/core/ui/file/vocab.rs src/core/ui/file/build.rs src/core/ui/file/error.rs src/core/ui/file/mod.rs src/core/ui/file/reader.rs src/core/ui/nodes/node.rs
git commit -m "feat: build UiNode trees from UI file values

- One table of kinds and options, each calling the UiNode constructor or method of its name
- Labels and tooltips from files are binding patterns
- Mistakes are collected with positions and suggest the nearest name"
```

---

### Task 6: Templates, imports and `UiFile`

**Files:**
- Create: `src/core/ui/file/document.rs`
- Modify: `src/core/ui/file/build.rs` (template expansion, parameters, instances), `src/core/ui/file/error.rs` (`UiFileError`, `UiFileIssue`), `src/core/ui/file/mod.rs`, `src/core/ui/nodes/node.rs` (crate field `source`), `src/core/ui/mod.rs`, `src/lib.rs` (prelude `UiFile`, `UiFileError`, `UiFileIssue`, `UiArgs`, `UiArg`), `src/core/asset/manager.rs` (`impl SpriteSource for AssetManager`)
- Test: `src/core/ui/file/document.rs` (`mod tests`)

**Interfaces:**
- Consumes: Tasks 1, 2 (`UiNode::seed`), 3 (`UiNode.template`, `UiNode.instance`), 5.
- Produces:
  ```rust
  // public (prelude)
  #[derive(Clone)] pub struct UiFile { .. }
  impl UiFile {
    pub fn parse(name: &str, text: &str) -> Result<UiFile, UiFileError>;
    pub fn path(&self) -> &str;
    pub fn generation(&self) -> u64;
    pub fn root(&self, assets: &mut AssetManager) -> Result<UiNode, UiFileError>;
    pub fn template(&self, name: &str, args: &UiArgs, assets: &mut AssetManager) -> Result<UiNode, UiFileError>;
  }
  #[derive(Debug, Clone, Default)] pub struct UiArgs { .. }
  impl UiArgs { pub fn new() -> UiArgs; pub fn with(self, name: &str, value: impl Into<UiArg>) -> UiArgs }
  #[derive(Debug, Clone, PartialEq)] pub enum UiArg { Number(f64), Text(String), Bool(bool), List(Vec<UiArg>) }
  // From for f32, f64, i32, u32, usize, bool, &str, String, Vec<T: Into<UiArg>>, &[&str]
  #[derive(Debug, Clone, PartialEq)] pub struct UiFileError { pub issues: Vec<UiFileIssue> }  // Display: one per line; std::error::Error
  #[derive(Debug, Clone, PartialEq)] pub struct UiFileIssue { pub path: String, pub line: u32, pub column: u32, pub message: String }
  // Display: "path:line:column: message", or "path: message" when line is 0
  // crate
  pub(crate) trait ImportResolver { fn import(&mut self, path: &str) -> Result<UiFile, UiFileError>; }
  impl UiFileError { pub(crate) fn one(path: &str, message: impl Into<String>) -> UiFileError }   // one issue, line 0
  pub(crate) struct NoImports;   // refuses: "files read with UiFile::parse cannot import; load the file with assets.load_ui"
  impl UiFile {
    pub(crate) fn load(path: &str, text: &str, imports: &mut dyn ImportResolver, sprites: &mut dyn SpriteSource, embedded: bool) -> Result<UiFile, UiFileError>;
    pub(crate) fn check(path: &str, text: &str, imports: &mut dyn ImportResolver, sprites: &mut dyn SpriteSource) -> Result<Arc<Document>, UiFileError>;
    pub(crate) fn doc(&self) -> Arc<Document>;
    pub(crate) fn replace(&self, doc: Arc<Document>);          // bumps generation
    pub(crate) fn is_embedded(&self) -> bool;
    pub(crate) fn build_root(&self, sprites: &mut dyn SpriteSource) -> Result<UiNode, UiFileError>;     // root() with any sprite source
    pub(crate) fn build_template(&self, name: &str, args: &UiArgs, sprites: &mut dyn SpriteSource) -> Result<UiNode, UiFileError>;
    pub(crate) fn build_source(&self, source: &TreeSource, sprites: &mut dyn SpriteSource) -> Result<UiNode, UiFileError>;  // same instance
    pub(crate) fn imports_file(&self, other: &UiFile) -> bool; // directly
  }
  pub(crate) struct Document { pub(crate) text: String, pub(crate) root: Option<Value>, pub(crate) templates: HashMap<String, Arc<TemplateDef>>, pub(crate) imports: Vec<UiFile>, pub(crate) seeds: Vec<(String, BindValue)> }
  pub(crate) struct TemplateDef { pub(crate) name: String, pub(crate) pos: Pos, pub(crate) params: Vec<String>, pub(crate) defaults: Vec<(String, Value)>, pub(crate) node: Value }
  pub(crate) struct TreeSource { pub(crate) file: UiFile, pub(crate) template: Option<String>, pub(crate) args: UiArgs, pub(crate) instance: u64 }
  // UiNode crate field
  pub(crate) source: Option<Arc<TreeSource>>,
  ```

Rules (the spec's File, Templates, Imports and Loading sections):
- **Top level:** `Fields` with `root`, `templates` (a map of name to `(params: [..], defaults: {..}, node: ..)`; `params` and `defaults` optional, `node` required), `imports` (a list of path strings), `bindings` (a map of name to string, number or bool). Anything else: `a UI file has root, templates, imports and bindings; "x" is not one` (with a suggestion). A file with neither `root` nor `templates` is an error at 1:1: `a UI file needs a root or templates`.
- **Template names** are CamelCase (first character uppercase) and not a kind's name: `"button" is not a template name: template names are CamelCase`, `"Button" is a node kind; give the template another name`. A name defined in the file and in an import, or in two imports: `the template "X" is defined twice (here and in "path")`.
- **Imports** load through the `ImportResolver`; a failure is an issue at the import's string (`cannot import "path"`), followed by the imported file's own issues (they carry their own path). Imports are not transitive: a file sees its own templates and its direct imports' templates. A template's body resolves template names in its own file's scope.
- **Expansion** (in `build.rs`, called from `build_node` when the name is not a kind): find the template (local, then imports), push `(file path, name)` on `cx.stack` (a name already on the stack: `the template "A" uses itself (A -> B -> A)`), compute the arguments (defaults, overridden by use-site fields whose name is a parameter; a missing required parameter: `MenuButton needs "label"`), substitute them into a clone of the body, build it in the template's own scope, then apply the remaining use-site fields as options of the result's kind (by the Task 5 rules; an unknown one: `MenuButton has no parameter or option "x"`, with a suggestion among both), set `template = Some(name)` on the result, and pop the stack.
- **Substitution** (`fn substitute(value: &Value, args: &HashMap<String, Value>, template: &str, cx: &mut BuildCx) -> Value`): a `Named { name, body: Unit }` whose name starts lowercase is a parameter reference: replaced by the argument (`ValueKind::Any` stays `Any`), or an issue `"labl" is not a parameter of MenuButton; did you mean "label"?`. In every string, `{param}` and `{param:.N}` for a parameter are replaced by the argument's text (as `BindValue::display` formats; an `Any` argument leaves the hole as written); `{{` and `}}` and other holes are left for bindings. Substitution recurses into lists, tuples, maps, fields and bodies, including nested template uses' fields (their arguments are evaluated in the outer scope).
- **Checking at load:** read; check the top level; resolve imports; check each template by expanding it with every parameter `Any` (this finds unknown kinds, options and cycles in bodies); build the `root` with the given `SpriteSource` (Placeholders when loading, Task 9's disk check in the CLI). All issues collected; any issue fails the load.
- **Building** `root()` / `template()`: a fresh instance id (`static NEXT_INSTANCE: AtomicU64`, starting at 1; 0 is code) on every node built, the root's `bindings` as `seed`s on the top node, and `source = Some(Arc::new(TreeSource { .. }))` on the top node. `build_source` rebuilds from a `TreeSource` with its instance id (for reloads). `template()` turns `UiArgs` into values at the template's position; an argument that is not a parameter or an option of the template's root is an issue naming it.
- `UiFile` is `Arc<FileCell { path: String, embedded: bool, doc: RwLock<Arc<Document>>, generation: AtomicU64 }>`; `doc()` recovers a poisoned lock with `into_inner()`.
- **A parameter may stand for a node.** A template can take a node as an argument and place it (`"Labelled": (params: ["label", "widget"], node: Row(children: [Text(label), widget]))`, used as `Labelled(label: "Volume", widget: Slider(range: (0, 1)))`). While a template is checked, such a parameter is `Any`, and `build_node` on an `Any` value builds nothing and reports nothing.
- `impl SpriteSource for AssetManager`: `self.load_sprite(path).map_err(|e| e.to_string())`.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::asset::AssetManager;
  use crate::core::ui::file::build::Placeholders;
  use crate::core::ui::nodes::fingerprint;
  use crate::prelude::*;

  /// Imports from an in-memory set of files
  struct Files(HashMap<String, String>, Vec<String>);
  impl ImportResolver for Files {
    fn import(&mut self, path: &str) -> Result<UiFile, UiFileError> {
      if self.1.iter().any(|p| p == path) {
        return Err(UiFileError::one(path, "the imports go round in a circle"));
      }
      let text = self.0.get(path).cloned().ok_or_else(|| UiFileError::one(path, "no such file"))?;
      self.1.push(path.to_string());
      let file = UiFile::load(path, &text, self, &mut Placeholders::default(), false);
      self.1.pop();
      file
    }
  }

  fn files(list: &[(&str, &str)]) -> Files {
    Files(list.iter().map(|(p, t)| (p.to_string(), t.to_string())).collect(), Vec::new())
  }

  fn load(text: &str, imports: &mut Files) -> Result<UiFile, UiFileError> {
    UiFile::load("ui/test.ui.ron", text, imports, &mut Placeholders::default(), false)
  }

  fn root(file: &UiFile) -> UiNode {
    file.build_root(&mut Placeholders::default()).expect("builds")
  }

  fn messages(result: Result<UiFile, UiFileError>) -> Vec<String> {
    result.err().expect("should fail").issues.into_iter().map(|i| i.message).collect()
  }

  const MENU: &str = r#"(
    templates: {
      "MenuButton": (
        params: ["label", "action"],
        defaults: { "wide": true },
        node: Button(label: "[{label}]", action: action, fill_width: wide),
      ),
    },
    root: Column(children: [
      MenuButton(label: "Play", action: "play", default_focus: true),
      MenuButton(label: "Quit", action: "quit", wide: false),
    ]),
  )"#;

  #[test]
  fn a_template_expands_with_arguments_defaults_and_root_options() {
    let file = load(MENU, &mut files(&[])).unwrap();
    let built = root(&file);
    let play = &built.children[0];
    assert_eq!(play.state.label.as_deref(), Some("[Play]"), "{label} in a string");
    assert_eq!(play.action.as_deref(), Some("play"));
    assert!(play.flags.default_focus, "a root option over the template");
    assert_eq!(play.template.as_deref(), Some("MenuButton"));
    assert_eq!(play.layout.width, Some(Length::Fill), "the default");
    assert_eq!(built.children[1].layout.width, None, "wide: false");
  }

  #[test]
  fn keys_of_template_instances_use_the_template_name() {
    let file = load(MENU, &mut files(&[])).unwrap();
    let built = root(&file);
    assert_eq!(crate::core::ui::nodes::child_keys(&built.children), ["MenuButton#0", "MenuButton#1"]);
  }

  #[test]
  fn a_string_hole_that_is_not_a_parameter_stays_a_binding() {
    let file = load(r#"(templates: {"Gold": (params: ["who"], node: Text("{who} has {gold}"))}, root: Gold(who: "Kai"))"#, &mut files(&[])).unwrap();
    assert_eq!(root(&file).state.label.as_deref(), Some("Kai has {gold}"));
  }

  #[test]
  fn templates_come_from_imports_and_resolve_in_their_own_file() {
    let mut imports = files(&[(
      "ui/common.ui.ron",
      r#"(templates: {
        "Inner": (params: ["t"], node: Text(t)),
        "Outer": (params: ["t"], node: Column(children: [Inner(t: t)])),
      })"#,
    )]);
    let file = load(r#"(imports: ["ui/common.ui.ron"], root: Outer(t: "hi"))"#, &mut imports).unwrap();
    assert_eq!(root(&file).children[0].state.label.as_deref(), Some("hi"));
  }

  #[test]
  fn template_mistakes_are_reported() {
    let none = || files(&[]);
    assert_eq!(messages(load("(root: Column, other: 1)", &mut none())), ["a UI file has root, templates, imports and bindings; \"other\" is not one"]);
    assert_eq!(messages(load("()", &mut none())), ["a UI file needs a root or templates"]);
    assert!(messages(load(r#"(templates: {"button": (node: Column)})"#, &mut none()))[0].contains("template names are CamelCase"));
    assert!(messages(load(r#"(templates: {"Button": (node: Column)})"#, &mut none()))[0].contains("is a node kind"));
    assert!(messages(load(r#"(templates: {"A": (params: ["x"], node: Text(x))}, root: A())"#, &mut none()))[0].contains("A needs \"x\""));
    assert!(messages(load(r#"(templates: {"A": (params: ["x"], node: Text(x))}, root: A(x: "1", y: 2))"#, &mut none()))[0].contains("A has no parameter or option \"y\""));
    assert!(messages(load(r#"(templates: {"A": (params: ["label"], node: Text(labl))})"#, &mut none()))[0].contains("did you mean \"label\""));
    assert!(messages(load(r#"(templates: {"A": (node: B), "B": (node: A)})"#, &mut none()))[0].contains("uses itself"));
    assert!(messages(load(r#"(templates: {"A": (node: Buton)})"#, &mut none()))[0].contains("did you mean \"Button\""), "bodies are checked at load");
  }

  #[test]
  fn import_mistakes_are_reported() {
    let mut cycle = files(&[("a.ui.ron", r#"(imports: ["b.ui.ron"], templates: {"A": (node: Column)})"#), ("b.ui.ron", r#"(imports: ["a.ui.ron"], templates: {"B": (node: Column)})"#)]);
    let issues = messages(load(r#"(imports: ["a.ui.ron"], root: Column)"#, &mut cycle));
    assert!(issues.iter().any(|m| m.contains("circle")), "{issues:?}");
    let mut twice = files(&[("c.ui.ron", r#"(templates: {"X": (node: Column)})"#)]);
    let issues = messages(load(r#"(imports: ["c.ui.ron"], templates: {"X": (node: Row)}, root: X)"#, &mut twice));
    assert!(issues[0].contains("defined twice"), "{issues:?}");
    let issues = messages(load(r#"(imports: ["missing.ui.ron"], root: Column)"#, &mut files(&[])));
    assert!(issues[0].contains("cannot import \"missing.ui.ron\""), "{issues:?}");
  }

  #[test]
  fn parse_refuses_imports_and_errors_print_with_positions() {
    let error = UiFile::parse("ui/x.ui.ron", "(imports: [\"y.ui.ron\"], root: Column)").err().unwrap();
    assert!(error.to_string().contains("cannot import"), "{error}");
    let error = UiFile::parse("ui/x.ui.ron", "(root: Column(\n  gpa: 1))").err().unwrap();
    assert_eq!(error.to_string(), "ui/x.ui.ron:2:3: Column has no option \"gpa\"; did you mean \"gap\"?");
  }

  #[test]
  fn a_template_built_from_code_takes_args_and_carries_its_source() {
    let file = UiFile::parse("ui/slot.ui.ron", r#"(templates: {"Item": (params: ["name", "count"], node: Button(label: "{name} x{count}"))})"#).unwrap();
    let mut assets = AssetManager::new();
    let node = file.template("Item", &UiArgs::new().with("name", "Sword").with("count", 3), &mut assets).unwrap();
    assert_eq!(node.state.label.as_deref(), Some("Sword x3"));
    let source = node.source.as_ref().expect("the top node carries its source");
    assert_eq!(source.template.as_deref(), Some("Item"));
    assert_ne!(node.instance, 0);
    let error = file.template("Item", &UiArgs::new().with("name", "S").with("cnt", 1), &mut assets).err().unwrap();
    assert!(error.to_string().contains("\"cnt\""), "{error}");
    let other = file.template("Item", &UiArgs::new().with("name", "A").with("count", 1), &mut assets).unwrap();
    assert_ne!(other.instance, node.instance, "each build is its own instance");
  }

  #[test]
  fn the_root_seeds_its_bindings_and_rebuilds_with_the_same_instance() {
    let file = UiFile::parse("ui/hud.ui.ron", r#"(bindings: {"score": 0}, root: Text("{score}"))"#).unwrap();
    let built = root(&file);
    assert_eq!(built.seeds, [("score".to_string(), BindValue::Number(0.0))]);
    let source = built.source.clone().unwrap();
    let again = file.build_source(&source, &mut Placeholders::default()).unwrap();
    assert_eq!(again.instance, built.instance);
    assert_eq!(fingerprint(&again), fingerprint(&built));
  }
}
```

(`root()` and `template()` are `build_root` and `build_template` with the asset manager as the sprite source.)

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p ryo2d --lib ui::file::document`
Expected: FAIL to compile.

- [ ] **Step 3: Implement** by the rules above. Keep `document.rs` to the file, its top level, imports and building entry points; put expansion and substitution in `build.rs` beside `build_node`. Extend `BuildCx` with the scope (`Arc<Document>` of the file being built), `stack: Vec<(String, String)>` and `instance: u64`.

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test -p ryo2d --lib ui::file`
Expected: PASS.

- [ ] **Step 5: Run the gate and commit**

```bash
git add src/core/ui/file/document.rs src/core/ui/file/build.rs src/core/ui/file/error.rs src/core/ui/file/mod.rs src/core/ui/nodes/node.rs src/core/ui/mod.rs src/lib.rs src/core/asset/manager.rs
git commit -m "feat: UI file templates, imports and UiFile

- Templates with parameters and defaults, used like kinds, with root options over them
- Imports share templates; bodies resolve names in their own file; cycles are errors
- UiFile builds its root or a template into a UiNode that carries its source"
```

---

### Task 7: UI files in the asset manager

**Files:**
- Modify: `src/core/asset/manager.rs` (`ui_files`, `ui_loading`, `ui_warned`; `load_ui`, `add_ui`, `reload_ui`; the reload event arm; `ImportResolver` impl), `src/core/asset/hot_reload.rs` (watch `ron`), `src/core/error.rs` (`AssetError::UiFile`)
- Test: `src/core/asset/manager.rs` (`mod tests`), `src/core/asset/hot_reload.rs` (the extensions test)

**Interfaces:**
- Consumes: Task 6's `UiFile::{load, check, replace, doc, is_embedded, imports_file}`, `ImportResolver`, `Placeholders`.
- Produces:
  ```rust
  impl AssetManager {
    pub fn load_ui<P: AsRef<Path>>(&mut self, path: P) -> AssetResult<UiFile>;
    pub fn add_ui<P: AsRef<Path>>(&mut self, path: P, text: &str) -> Result<UiFile, UiFileError>;
    pub fn reload_ui<P: AsRef<Path>>(&mut self, path: P) -> AssetResult<()>;
  }
  pub enum AssetError { .., #[error("{0}")] UiFile(UiFileError) }
  impl ImportResolver for AssetManager
  ```

Rules:
- **Caching** like `load_shader`: by `cache_key(path)`; the second load returns the same `UiFile` (same cell). Issues name the path as the game gave it (`path.as_ref().display()`), and imports are given as written in the file.
- **Cycles:** `ui_loading: Vec<PathBuf>` holds the keys being loaded; importing one of them is an error at the import: `the imports go round in a circle: a -> b -> a`.
- `add_ui` checks the text (imports through the cache and the disk) and caches it as embedded (never reloaded from disk); later loads and imports of that path use it.
- `reload_ui`: a path not cached is `Err(AssetError::NotFound(resolved))`; otherwise read and check the file; on success `replace` the document, then recheck every cached file that imports it, directly or through others (re-run `UiFile::check` on its stored `text`; success replaces, failure warns and keeps it); on failure return `Err(AssetError::UiFile(..))` and keep the old document. An embedded file reloads from its stored text (useful only for importer rechecks; `reload_ui` on it is `Ok(())`).
- **The watcher:** `HotReloadConfig`'s default extensions gain `"ron"` (`x.ui.ron`'s extension is `ron`). In `handle_reload_event`, a modified or created path that is a cached, not embedded UI file: read it; if the text equals the current document's, do nothing (one save sends several events); otherwise check it; on success replace, recheck importers, push `Ok(key)` to `reload_outcomes` and return `true`; on failure, warn once per distinct broken text (`ui_warned: HashMap<PathBuf, u64>` holding a hash of the last text warned about) with `engine_warn!(module: "ui", "could not reload {path}; keeping the old version:\n{error}")`, push `Err((key, error.to_string()))`, and return `false`.

- [ ] **Step 1: Write the failing tests**

In `manager.rs`'s tests (they have `event_fixture()`, a temp dir and a manager based there):

```rust
  fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path
  }

  #[test]
  fn load_ui_caches_by_path_and_imports_through_the_cache() {
    let (dir, mut assets) = event_fixture();
    write(dir.path(), "common.ui.ron", r#"(templates: {"T": (node: Text("t"))})"#);
    write(dir.path(), "menu.ui.ron", r#"(imports: ["common.ui.ron"], root: T)"#);
    let menu = assets.load_ui("menu.ui.ron").unwrap();
    let again = assets.load_ui("menu.ui.ron").unwrap();
    assert_eq!(menu.generation(), again.generation());
    let common = assets.load_ui("common.ui.ron").unwrap();
    assert!(menu.imports_file(&common), "the import came from the cache");
  }

  #[test]
  fn a_broken_or_missing_ui_file_is_an_error() {
    let (dir, mut assets) = event_fixture();
    write(dir.path(), "bad.ui.ron", "(root: Buton)");
    assert!(matches!(assets.load_ui("bad.ui.ron"), Err(AssetError::UiFile(_))));
    assert!(matches!(assets.load_ui("nope.ui.ron"), Err(AssetError::NotFound(_))));
  }

  #[test]
  fn add_ui_serves_later_loads_and_imports() {
    let (dir, mut assets) = event_fixture();
    assets.add_ui("embedded/common.ui.ron", r#"(templates: {"T": (node: Text("t"))})"#).unwrap();
    write(dir.path(), "menu.ui.ron", r#"(imports: ["embedded/common.ui.ron"], root: T)"#);
    assert!(assets.load_ui("menu.ui.ron").is_ok());
  }

  #[test]
  fn reload_ui_replaces_the_document_and_a_broken_reload_keeps_the_old_one() {
    let (dir, mut assets) = event_fixture();
    let path = write(dir.path(), "hud.ui.ron", "(root: Text(\"a\"))");
    let file = assets.load_ui("hud.ui.ron").unwrap();
    let first = file.generation();
    std::fs::write(&path, "(root: Text(\"b\"))").unwrap();
    assets.reload_ui("hud.ui.ron").unwrap();
    assert!(file.generation() > first, "the same handle sees the new document");
    let good = file.generation();
    std::fs::write(&path, "(root: Text(").unwrap();
    assert!(assets.reload_ui("hud.ui.ron").is_err());
    assert_eq!(file.generation(), good, "kept");
  }

  #[test]
  fn reloading_an_import_bumps_its_importers() {
    let (dir, mut assets) = event_fixture();
    let common = write(dir.path(), "common.ui.ron", r#"(templates: {"T": (node: Text("t"))})"#);
    write(dir.path(), "menu.ui.ron", r#"(imports: ["common.ui.ron"], root: T)"#);
    let menu = assets.load_ui("menu.ui.ron").unwrap();
    let before = menu.generation();
    std::fs::write(&common, r#"(templates: {"T": (node: Text("u"))})"#).unwrap();
    assets.reload_ui("common.ui.ron").unwrap();
    assert!(menu.generation() > before);
  }

  #[test]
  fn ui_imports_in_a_circle_are_an_error() {
    let (dir, mut assets) = event_fixture();
    write(dir.path(), "a.ui.ron", r#"(imports: ["b.ui.ron"], templates: {"A": (node: Column)})"#);
    write(dir.path(), "b.ui.ron", r#"(imports: ["a.ui.ron"], templates: {"B": (node: Column)})"#);
    let Err(AssetError::UiFile(error)) = assets.load_ui("a.ui.ron") else { panic!() };
    assert!(error.to_string().contains("circle"), "{error}");
  }

  #[test]
  fn a_half_saved_file_warns_once_and_the_finished_save_reloads() {
    let (dir, mut assets) = event_fixture();
    let path = write(dir.path(), "menu.ui.ron", "(root: Text(\"a\"))");
    let file = assets.load_ui("menu.ui.ron").unwrap();
    let start = file.generation();
    std::fs::write(&path, "(root: Te").unwrap();
    for _ in 0..3 {
      assert!(!assets.handle_reload_event(HotReloadEvent::FileModified(path.clone())));
    }
    assert_eq!(crate::core::ui::test_util::warnings_containing("menu.ui.ron"), 1, "once per broken text");
    std::fs::write(&path, "(root: Text(\"b\"))").unwrap();
    assert!(assets.handle_reload_event(HotReloadEvent::FileModified(path.clone())));
    assert!(file.generation() > start);
    assert!(!assets.handle_reload_event(HotReloadEvent::FileModified(path)), "the same text again does nothing");
  }
```

In `hot_reload.rs`, extend `hot_reload_config_default_has_common_extensions` with `assert!(config.extensions.contains("ron"));`.

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p ryo2d --lib asset::`
Expected: FAIL to compile (`load_ui` and the rest do not exist).

- [ ] **Step 3: Implement** by the rules. Add the three fields to both constructors. Put the UI arm of `handle_reload_event` beside the shader and sprite arms, as its own block (the three maps are independent).

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test -p ryo2d --lib asset::`
Expected: PASS.

- [ ] **Step 5: Run the gate and commit**

```bash
git add src/core/asset/manager.rs src/core/asset/hot_reload.rs src/core/error.rs
git commit -m "feat: load, embed and hot reload UI files through the asset manager

- load_ui caches by path and loads imports through the cache; add_ui embeds text
- reload_ui and the watcher replace a file in place and recheck its importers
- A broken save keeps the old version and warns once per distinct text"
```

---

### Task 8: Spawning files and reloading spawned trees

**Files:**
- Modify: `src/core/ui/nodes/components.rs` (`UiSource`), `src/core/ui/nodes/spawn.rs` (insert `UiSource`), `src/core/ui/nodes/reconcile.rs` (`update_node` keeps `UiSource` in step in `Mode::Update`; `reload_trees`), `src/core/ui/nodes/events.rs` (`UiReloaded`), `src/core/ui/nodes/mod.rs`, `src/core/messages/mod.rs` (register `UiReloaded`), `src/core/engine/frame.rs` (`draw_ui` calls `reload_trees` first), `src/core/scene/context.rs` (`spawn_ui_file`, `ui_template`), `src/core/testing/ui.rs` (assets, `spawn_ui_file`, `reload_ui`, `reload_trees` in `frame`), `src/core/ui/file/error.rs` (`From<AssetError> for UiFileError`), `src/lib.rs` (prelude `UiReloaded`)
- Create: `tests/e2e/ui_files.rs`; Modify: `tests/e2e.rs`
- Test: `src/core/ui/nodes/reconcile.rs` (`mod reload_tests`), `tests/e2e/ui_files.rs`

**Interfaces:**
- Consumes: Tasks 4 (`reconcile`, `Mode`), 6 (`TreeSource`, `UiFile::build_source`, `UiNode.source`), 7 (`load_ui`, `reload_ui`).
- Produces:
  ```rust
  #[derive(Component)] pub(crate) struct UiSource { pub(crate) source: Arc<TreeSource>, pub(crate) generation: u64 }
  #[derive(Message, Debug, Clone, Copy, PartialEq, Eq)] pub struct UiReloaded { pub root: Entity }   // public, prelude
  pub(crate) fn reload_trees(world: &mut World);
  impl SceneContext<'_> {
    pub fn spawn_ui_file(&mut self, path: &str) -> Result<Entity, UiFileError>;
    pub fn ui_template(&mut self, path: &str, name: &str, args: &UiArgs) -> Result<UiNode, UiFileError>;
  }
  impl UiHarness {
    pub fn with_assets(self, dir: impl Into<PathBuf>) -> UiHarness;     // its asset manager's base path
    pub fn assets(&mut self) -> &mut AssetManager;
    pub fn spawn_ui_file(&mut self, path: &str) -> Result<Entity, UiFileError>;
    pub fn reload_ui(&mut self, path: &str) -> AssetResult<()>;
  }
  impl From<AssetError> for UiFileError   // UiFile(e) => e; anything else => one issue at line 0
  ```

Rules:
- `spawn_node` inserts `UiSource { source, generation: source.file.generation() }` on a node whose `UiNode.source` is `Some`.
- `update_node` in `Mode::Update` makes the node's `UiSource` match the new tree's `source` (insert, replace or remove); in `Mode::Reload` it leaves it (the caller sets the new generation).
- `reload_trees(world)`: collect `(entity, source, now)` for every `UiSource` whose `generation` differs from `source.file.generation()`; return at once if none (this runs every frame in every build, so the empty case must cost one query). For each, outer nodes first (fewer ancestors first) and skipping despawned ones, with the world's `AssetManager` resource (skip everything if there is none): `build_source`; on `Ok(tree)`, `reconcile(world, entity, tree, Mode::Reload { instance: source.instance })`, set the result's `UiSource.generation = now`, and remember its root; on `Err(error)`, `warn_once(format!("ui-rebuild:{}:{now}", path), ..)` with "could not rebuild a tree from {path}; keeping it as it was:\n{error}" and set `generation = now` so it is not retried every frame. Then write one `UiReloaded { root }` per root touched.
- `Engine::draw_ui` calls `nodes::reload_trees(world)` before the scene UI pass (beside `despawn_scene_roots`); `UiHarness::frame` and `frame_with_world` call it at the start of the frame.
- `spawn_ui_file`: `self.assets.load_ui(path)?`, `file.root(self.assets)?`, `self.spawn_ui(tree)`. `ui_template`: load, then `file.template(name, args, self.assets)`.
- `UiHarness` owns an `AssetManager` as a resource in its world (`AssetManager::new()` by default; `with_assets(dir)` sets the base path with `set_base_path`).

- [ ] **Step 1: Write the failing tests**

In `reconcile.rs`:

```rust
#[cfg(test)]
mod reload_tests {
  use super::*;
  use crate::core::testing::UiHarness;
  use crate::core::ui::nodes::*;
  use crate::prelude::*;

  fn harness(files: &[(&str, &str)]) -> (tempfile::TempDir, UiHarness) {
    let dir = tempfile::tempdir().unwrap();
    for (name, text) in files {
      std::fs::write(dir.path().join(name), text).unwrap();
    }
    let h = UiHarness::new(640.0, 480.0).with_assets(dir.path());
    (dir, h)
  }

  fn rewrite(dir: &tempfile::TempDir, h: &mut UiHarness, name: &str, text: &str) {
    std::fs::write(dir.path().join(name), text).unwrap();
    h.reload_ui(name).unwrap();
    h.frame(|_| {});
  }

  const PAUSE: &str = r#"(root: Panel(modal: true, focus_scope: true, children: [
    Text(text: "Paused", name: "title"),
    Button(label: "Resume", action: "resume", default_focus: true),
    Button(label: "Quit", action: "quit"),
  ]))"#;

  #[test]
  fn a_reload_updates_in_place_keeping_entities_game_values_and_focus() {
    let (dir, mut h) = harness(&[("pause.ui.ron", PAUSE)]);
    let root = h.spawn_ui_file("pause.ui.ron").unwrap();
    h.frame(|_| {});
    h.frame(|_| {});
    let focused = h.focused();
    let title = h.find_ui("title").unwrap();
    h.world_mut().get_mut::<UiLabel>(title).unwrap().0 = "Game wrote".into();
    rewrite(&dir, &mut h, "pause.ui.ron", &PAUSE.replace("modal: true,", "modal: true, gap: 20,"));
    assert_eq!(h.find_ui("title"), Some(title), "the same entity");
    assert_eq!(h.world().get::<UiLabel>(title).unwrap().0, "Game wrote");
    assert_eq!(h.world().get::<UiNodeKind>(root).unwrap().layout.gap, Some(20.0));
    assert_eq!(h.focused(), focused, "focus stays on Resume");
    assert!(h.events().is_empty(), "the reload sent no UiEvents");
    let reloaded: Vec<_> = h.messages::<UiReloaded>().collect();
    assert_eq!(reloaded, [UiReloaded { root }]);
  }

  #[test]
  fn two_trees_from_one_file_both_reload_and_keep_their_values() {
    let (dir, mut h) = harness(&[("hud.ui.ron", r#"(root: Column(children: [Text("{score}")]))"#)]);
    let one = h.spawn_ui_file("hud.ui.ron").unwrap();
    let two = h.spawn_ui_file("hud.ui.ron").unwrap();
    h.ui_set(one, "score", 1);
    h.ui_set(two, "score", 2);
    rewrite(&dir, &mut h, "hud.ui.ron", r#"(root: Column(gap: 4, children: [Text("Score {score}")]))"#);
    let label = |h: &UiHarness, root: Entity| {
      let text = h.world().get::<Children>(root).unwrap()[0];
      h.world().get::<UiLabel>(text).unwrap().0.clone()
    };
    assert_eq!(label(&h, one), "Score 1");
    assert_eq!(label(&h, two), "Score 2");
  }

  #[test]
  fn code_built_instances_reload_from_their_template_and_a_removed_template_keeps_them() {
    let file = r#"(templates: {"Item": (params: ["name"], node: Button(label: "{name}"))}, root: Column(name: "list"))"#;
    let (dir, mut h) = harness(&[("inv.ui.ron", file)]);
    h.spawn_ui_file("inv.ui.ron").unwrap();
    let list = h.find_ui("list").unwrap();
    let ui_file = h.assets().load_ui("inv.ui.ron").unwrap();
    let node = ui_file.template("Item", &UiArgs::new().with("name", "Sword"), h.assets()).unwrap();
    let item = spawn_ui_in(h.world_mut(), list, node).unwrap();
    rewrite(&dir, &mut h, "inv.ui.ron", &file.replace("\"{name}\"", "\"<{name}>\""));
    assert_eq!(h.world().get::<UiLabel>(item).unwrap().0, "<Sword>");
    assert_eq!(h.world().get::<Children>(list).unwrap().to_vec(), [item], "the root's reload left the code-built item");
    rewrite(&dir, &mut h, "inv.ui.ron", r#"(root: Column(name: "list"))"#);
    assert_eq!(h.world().get::<UiLabel>(item).unwrap().0, "<Sword>", "kept as it was");
    assert_eq!(crate::core::ui::test_util::warnings_containing("could not rebuild"), 1);
  }

  #[test]
  fn a_reload_that_cannot_build_keeps_the_tree_and_warns_once() {
    let (dir, mut h) = harness(&[("img.ui.ron", r#"(root: Column(children: [Text("a")]))"#)]);
    let root = h.spawn_ui_file("img.ui.ron").unwrap();
    rewrite(&dir, &mut h, "img.ui.ron", r#"(root: Column(children: [Image("missing.png")]))"#);
    h.frame(|_| {});
    let child = h.world().get::<Children>(root).unwrap()[0];
    assert_eq!(h.world().get::<UiNodeKind>(child).unwrap().name(), "text");
    assert_eq!(crate::core::ui::test_util::warnings_containing("missing.png"), 1);
  }

  #[test]
  fn children_the_game_added_before_spawning_survive_a_reload() {
    let (dir, mut h) = harness(&[("m.ui.ron", r#"(root: Column(children: [Text("file")]))"#)]);
    let file = h.assets().load_ui("m.ui.ron").unwrap();
    let tree = file.root(h.assets()).unwrap().child(UiNode::text("code").key("code"));
    let root = h.spawn_ui(tree);
    rewrite(&dir, &mut h, "m.ui.ron", r#"(root: Column(children: [Text("file2")]))"#);
    let labels: Vec<String> = h.world().get::<Children>(root).unwrap().iter()
      .map(|&c| h.world().get::<UiLabel>(c).unwrap().0.clone()).collect();
    assert_eq!(labels, ["file2", "code"]);
  }
}
```

If `UiHarness` has no way to read messages, add `pub fn messages<M: Message + Clone>(&self) -> impl Iterator<Item = M>` (reading the world's `Messages<M>` this frame), or test `UiReloaded` through `world().resource::<Messages<UiReloaded>>()`.

`tests/e2e/ui_files.rs` (register `mod ui_files;` in `tests/e2e.rs`): an engine-level check that `draw_ui` reloads. A scene spawns a file from a temp dir in `enter` (an absolute path), stores the root, and in `update` calls `ctx.assets.reload_ui(path)` once a shared `Arc<AtomicBool>` is set:

```rust
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

struct FileScene {
  path: PathBuf,
  reload: Arc<AtomicBool>,
}

impl Scene for FileScene {
  fn enter(&mut self, ctx: &mut SceneContext) {
    ctx.spawn_ui_file(self.path.to_str().unwrap()).expect("the file loads");
  }

  fn update(&mut self, ctx: &mut SceneContext) -> SceneAction {
    if self.reload.swap(false, Ordering::SeqCst) {
      ctx.assets.reload_ui(&self.path).expect("the new file loads");
    }
    SceneAction::None
  }
}

#[test]
fn an_edited_file_reloads_in_the_running_engine_and_keeps_its_ids() {
  let dir = tempfile::tempdir().unwrap();
  let path = dir.path().join("menu.ui.ron");
  let menu = |width: u32| format!(r#"(root: Column(focus_scope: true, anchor: Center, children: [Button(label: "Play", width: {width}, default_focus: true)]))"#);
  std::fs::write(&path, menu(100)).unwrap();
  let reload = Arc::new(AtomicBool::new(false));
  let scene = FileScene { path: path.clone(), reload: reload.clone() };
  let Some(mut engine) = HeadlessEngine::new_or_skip(EngineBuilder::new().resolution(640, 480).start_scene(scene)) else { return };
  engine.step_frames(4).unwrap();
  let (id, before) = engine.ui_focusables()[0];
  std::fs::write(&path, menu(200)).unwrap();
  reload.store(true, Ordering::SeqCst);
  engine.step_frames(3).unwrap();
  let (same, after) = engine.ui_focusables()[0];
  assert_eq!(same, id, "the button kept its entity, so its id");
  assert!(after.width > before.width + 50.0, "{before:?} -> {after:?}");
}
```

(Match the scene trait's real method names and signatures in `src/core/scene/`, as 4c's `tests/e2e/ui_nodes.rs` does.)

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p ryo2d --lib ui::nodes::reconcile::reload_tests` and `cargo test --test e2e ui_files`
Expected: FAIL to compile.

- [ ] **Step 3: Implement** by the rules.

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test -p ryo2d --lib ui::` and `cargo test --test e2e ui_files`
Expected: PASS (the e2e test skips without a GPU adapter, as the others do).

- [ ] **Step 5: Run the gate and commit**

```bash
git add src/core/ui/nodes/components.rs src/core/ui/nodes/spawn.rs src/core/ui/nodes/reconcile.rs src/core/ui/nodes/events.rs src/core/ui/nodes/mod.rs src/core/messages/mod.rs src/core/engine/frame.rs src/core/scene/context.rs src/core/testing/ui.rs src/core/ui/file/error.rs src/lib.rs tests/e2e/ui_files.rs tests/e2e.rs
git commit -m "feat: spawn UI files and reload their trees in place

- ctx.spawn_ui_file and ctx.ui_template build from files through the scene's assets
- A changed file reconciles every tree built from it, roots and code-built instances, before the UI pass
- UiReloaded tells the game; a tree that cannot be rebuilt is kept and warned about once"
```

---

### Task 9: `ryo2d check-ui`

**Files:**
- Create: `src/core/ui/file/check.rs`, `src/cli/commands/check_ui.rs`
- Modify: `src/core/ui/file/mod.rs` (`pub(crate) mod check;`), `src/cli/commands/mod.rs` (`mod check_ui;`, the `"check-ui"` arm after `netplay`, a row in `print_help`), `src/cli/mod.rs` (the commands doc list)
- Test: `src/core/ui/file/check.rs` (`mod tests`), `src/cli/commands/check_ui.rs` (`mod tests`)

**Interfaces:**
- Consumes: Task 6's `UiFile::check`, `ImportResolver`, `SpriteSource`, `UiFileIssue`; the CLI's `discover_workspace`, `Workspace::{root, assets_dir}`, `Output`, `CliError`, `CliResult`.
- Produces:
  ```rust
  // check.rs
  pub(crate) fn check_files(root: &Path, files: &[PathBuf]) -> Vec<UiFileIssue>;   // paths relative to root in issues
  pub(crate) fn ui_files_under(dir: &Path) -> Vec<PathBuf>;                       // every *.ui.ron, sorted
  // check_ui.rs
  pub fn execute(args: &[String]) -> CliResult<()>;
  ```

Rules:
- `check_files` checks each file with a disk resolver (imports resolved against `root`, each file read once per run, cycles reported) and a disk sprite source whose `sprite(path)` fails with `no image at "path"` when `root.join(path)` does not exist (else a placeholder), so missing images are issues at their values. An unreadable file is one issue at line 0. Issue paths are relative to `root` with `/` separators.
- `execute`: `--help`/`-h` prints help (USAGE, OPTIONS, DESCRIPTION, EXAMPLE, like `run.rs`); otherwise `discover_workspace()?`, the files are the arguments (relative to the current directory) or `ui_files_under(workspace.assets_dir())`; each issue is printed to stderr as its `Display`; with issues, return `Err(CliError::Other(format!("{issues} problem(s) in {files} UI file(s)")))` (main prints it and exits non-zero); without, `Output::success(&format!("checked {n} UI file(s)"))`. No UI files: `Output::warning` "no *.ui.ron files under {dir}" and `Ok(())`.

- [ ] **Step 1: Write the failing tests**

`check.rs`:

```rust
#[cfg(test)]
mod tests {
  use super::*;

  fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (name, text) in files {
      let path = dir.path().join(name);
      std::fs::create_dir_all(path.parent().unwrap()).unwrap();
      std::fs::write(path, text).unwrap();
    }
    dir
  }

  #[test]
  fn a_clean_project_has_no_issues() {
    let dir = project(&[
      ("assets/ui/common.ui.ron", r#"(templates: {"T": (node: Image("assets/icon.png"))})"#),
      ("assets/ui/menu.ui.ron", r#"(imports: ["assets/ui/common.ui.ron"], root: T)"#),
      ("assets/icon.png", ""),
    ]);
    let files = ui_files_under(&dir.path().join("assets"));
    assert_eq!(files.len(), 2);
    assert_eq!(check_files(dir.path(), &files), []);
  }

  #[test]
  fn broken_files_and_missing_images_are_reported_with_positions() {
    let dir = project(&[
      ("assets/ui/a.ui.ron", "(root: Column(\n  gpa: 1))"),
      ("assets/ui/b.ui.ron", r#"(root: Image("assets/gone.png"))"#),
    ]);
    let issues = check_files(dir.path(), &ui_files_under(&dir.path().join("assets")));
    let shown: Vec<String> = issues.iter().map(ToString::to_string).collect();
    assert_eq!(shown[0], "assets/ui/a.ui.ron:2:3: Column has no option \"gpa\"; did you mean \"gap\"?");
    assert!(shown[1].starts_with("assets/ui/b.ui.ron:1:14:") && shown[1].contains("no image at \"assets/gone.png\""), "{shown:?}");
  }
}
```

`check_ui.rs`:

```rust
#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn help_prints_and_succeeds() {
    assert!(execute(&["--help".to_string()]).is_ok());
  }

  #[test]
  fn the_summary_counts_problems_and_files() {
    assert_eq!(summary(3, 2), "3 problem(s) in 2 UI file(s)");
  }
}
```

(`fn summary(problems: usize, files: usize) -> String` is the message `execute` puts in `CliError::Other`.)

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p ryo2d --lib ui::file::check cli::commands::check_ui`
Expected: FAIL to compile.

- [ ] **Step 3: Implement** by the rules.

- [ ] **Step 4: Run the tests to see them pass, and try the command**

Run: `cargo test -p ryo2d --lib ui::file::check cli::commands::check_ui`
Expected: PASS.

Run: `cd examples/demos/ui && cargo run -q -p ryo2d --bin ryo2d -- check-ui; cd -`
Expected: `checked 0 UI file(s)` or the no-files warning (the demo's files arrive in Task 10; its `theme.ron` is not a `*.ui.ron`).

- [ ] **Step 5: Run the gate and commit**

```bash
git add src/core/ui/file/check.rs src/core/ui/file/mod.rs src/cli/commands/check_ui.rs src/cli/commands/mod.rs src/cli/mod.rs
git commit -m "feat: ryo2d check-ui checks UI files without running the game

- Checks every *.ui.ron under the assets directory, or the files given
- Reports each problem as path:line:column, including images that do not exist
- Exits non-zero when any file has a problem"
```

---
### Task 10: The `ui` demo's menu and settings pages from files

**Files:**
- Create: `examples/demos/ui/assets/ui/common.ui.ron`, `examples/demos/ui/assets/ui/menu.ui.ron`, `examples/demos/ui/assets/ui/settings.ui.ron`, `tests/integration/ui_file_validity.rs`
- Rename and rewrite: `examples/demos/ui/src/scenes/node_menu_page.rs` → `file_menu_page.rs` (`FileMenuPage`), `node_settings_page.rs` → `file_settings_page.rs` (`FileSettingsPage`)
- Modify: `examples/demos/ui/src/scenes/mod.rs`, `examples/demos/ui/src/lib.rs` (pages 11 and 12), `examples/demos/ui/tests/acceptance.rs` (names only), `tests/integration.rs`
- Test: `examples/demos/ui/tests/acceptance.rs` (existing walks over pages 11 and 12), `tests/integration/ui_file_validity.rs`

**Interfaces:**
- Consumes: Task 8's `ctx.spawn_ui_file`, `ctx.ui_template`; Task 2's `ctx.ui_set`/`ui_get`; 4c's `find_ui`, `find_ui_in`, `ui_events`.
- Produces: the demo files later tasks import (`common.ui.ron`); `FileMenuPage`, `FileSettingsPage`.

Rules:
- **The pages behave exactly as the node pages did**: same labels, sizes, layout, actions, names and focus order. The acceptance tests (`MENUS = [0, 11]`, `SETTINGS = [1, 12]`) must pass unchanged. Translate each Rust tree with the vocabulary table: `UiNode::button("Play").action("new").default_focus()` is `Button(label: "Play", action: "new", default_focus: true)`.
- **Paths:** the pages load `assets/ui/<name>.ui.ron` (the working directory is the demo's, as for `assets/ui/theme.ron`). Keep each path in a `const`.
- **No panics:** a page whose file fails to load keeps the error text and draws it with closure UI (`ui.text(..)`) in `ui`, and logs it once.
- **`common.ui.ron`** holds templates at least two page files use. Start with `"Labelled": (params: ["label", "widget"], node: Row(gap: 12, align: Center, children: [Text(text: label, width: ..), widget]))` for the settings rows and the menu's options dialog, plus any other repeated shape you find (a dialog's Close/Done button, for instance).
- **Menu page** (`menu.ui.ron`): the menu panel is the file's `root`. The title column, the bottom status column, and the Options and Credits dialogs are templates the page spawns with `ctx.spawn_ui(ctx.ui_template(MENU, "Options", &args)?)`. `Options` takes the current `volume`, `fullscreen` and `subtitles` as parameters. `Credits` holds a named placeholder that the page fills from `menu_data::CREDITS` with a `CreditLine` template (`role`, `name`), built by code. The quit dialog is a `Confirm` template in the file. Event handling stays as it is (by action and name).
- **Settings page** (`settings.ui.ron`): the whole panel is the `root`; the option lists (`OUTPUTS`, `WINDOWS`, `RESOLUTIONS`, the rebindable `ACTIONS`) are written in the file (the closure page keeps `settings_data.rs`). The Audio tab's sliders bind to names (`bind: "master"` and the others), and each row shows its value as bound text after the slider (`Text("{master:.2}")`) in place of `show_value`. Tab switching stays in code (the `tabs` index hides and shows the `tab-N` columns).
- **Validity test** (`tests/integration/ui_file_validity.rs`, registered in `tests/integration.rs`): find every `*.ui.ron` under the repository's `examples/` and `tests/` (skipping `target/`) and parse each with `ron::from_str::<ron::Value>(&text)`, failing with the file's path and error. It also loads each through `UiFile::parse` when the file has no imports (or through an `AssetManager` based at the file's project directory when it does), so a demo file with a mistake fails a test, not only a run.

- [ ] **Step 1: Capture the node pages before changing them**

Run: `./dev demos:tour`, then copy the `ui` demo's captures of pages 11-15 from `target/demo-tour/` to a scratch directory outside the repository (Task 11 compares pages 13-15 against them).
Expected: PNGs of the five node pages as they are now.

- [ ] **Step 2: Write the validity test and see it pass vacuously, then fail on a bad file**

Write the test. Run it: it passes (no files yet). Write a scratch `tests/integration/broken.ui.ron` containing `(root: Column(` and run it: it fails naming the file. Delete the scratch file.

Run: `cargo test --test integration ui_file_validity`
Expected: PASS, then FAIL with the scratch file, then PASS again.

- [ ] **Step 3: Write the files and rewrite the pages**

Write `common.ui.ron`, `menu.ui.ron` and `settings.ui.ron` from the current Rust trees, then rewrite the two pages to load them. Each file starts with a short comment saying what it is and that debug builds reload it when saved.

- [ ] **Step 4: Run the acceptance tests**

Run: `cargo test -p ui-demo --test acceptance` (check the package name in `examples/demos/ui/Cargo.toml`) and `cargo test --test integration ui_file_validity`
Expected: PASS: every walk over pages 11 and 12, and the files are valid RON and valid UI files.

- [ ] **Step 5: Compare captures, and check the files from the CLI**

Capture pages 11 and 12 again and compare with Step 1's: the same screens, apart from the settings rows' bound value text. Look at both pairs.

Run: `cd examples/demos/ui && cargo run -q -p ryo2d --bin ryo2d -- check-ui; cd -`
Expected: `checked 3 UI file(s)`.

- [ ] **Step 6: Run the gate and commit**

```bash
git add examples/demos/ui/assets/ui/common.ui.ron examples/demos/ui/assets/ui/menu.ui.ron examples/demos/ui/assets/ui/settings.ui.ron examples/demos/ui/src/scenes/file_menu_page.rs examples/demos/ui/src/scenes/file_settings_page.rs examples/demos/ui/src/scenes/mod.rs examples/demos/ui/src/lib.rs examples/demos/ui/tests/acceptance.rs tests/integration/ui_file_validity.rs tests/integration.rs
git rm --cached -q examples/demos/ui/src/scenes/node_menu_page.rs examples/demos/ui/src/scenes/node_settings_page.rs 2>/dev/null || true
git commit -m "feat: the ui demo's menu and settings pages load from UI files

- Shared templates in common.ui.ron; the menu's dialogs are file templates
- Settings rows show their sliders' values through bindings
- Every UI file in the repo is checked as RON and as a UI file in a test"
```

(Use `git mv` for the renames in Step 3 so history follows the files; then the `git rm --cached` line is a no-op.)

---

### Task 11: The `ui` demo's inventory, dialogue and Mixed pages from files

**Files:**
- Create: `examples/demos/ui/assets/ui/inventory.ui.ron`, `examples/demos/ui/assets/ui/dialogue.ui.ron`, `examples/demos/ui/assets/ui/mixed.ui.ron`
- Rename and rewrite: `node_inventory_page.rs` → `file_inventory_page.rs` (`FileInventoryPage`), `node_dialogue_page.rs` → `file_dialogue_page.rs` (`FileDialoguePage`); rewrite `mixed_page.rs`
- Modify: `examples/demos/ui/src/scenes/mod.rs`, `examples/demos/ui/src/lib.rs` (pages 13-15), `examples/demos/ui/tests/acceptance.rs` (the `Watchable` impl and imports name `FileInventoryPage`)
- Test: `examples/demos/ui/tests/acceptance.rs` (the existing walks over pages 13, 14 and 15)

**Interfaces:**
- Consumes: Tasks 2, 4 (`ctx.update_ui`), 8, 10 (`common.ui.ron`).
- Produces: `FileInventoryPage` with the same public `status()` and `item(SlotRef)` the acceptance test reads.

Rules (as Task 10: same behaviour, same acceptance tests, no panics, paths in `const`s):
- **Inventory** (`inventory.ui.ron`): the root holds the static layout (equipment column, the bag grid, the details panel with its named nodes `item`, `item-name`, `item-text`, `empty`, the Sort button, the discard slot, the status text) with named placeholders where slots go. Slots are a file template (`"InvSlot": (node: Slot(filter: Nearest, drag_source: true, drop_target: true))`, plus an equipment row template if useful) built by code with `ctx.ui_template(..)?.insert(SlotAt(at))` (and `.default_focus()` for `Bag(0)`) into the placeholders, in the same order as before, so the first focus is bag slot 0 and Left, Down, Down still reaches Weapon. `refresh` and `show_details` stay as they are.
- **Dialogue** (`dialogue.ui.ron`):
  - the panel is the root;
  - the portrait is a placeholder (`Column(name: "portrait")`) that code fills with `UiNode::image(self.portrait.clone())`, since the portrait is drawn in code, not a file;
  - the line text keeps its typewriter (`typewriter: (per_second: 40, punctuation_pause: 0.15)`);
  - the HUD is a `Hud` template with `Progress(fraction: 1, bind: "hp")`, `Text("{gold}")` and `Text("{gems}")`, which the page sets with `ctx.ui_set`;
  - the choices live in a named focus-scope column (`choices`) in the file, and a change of `(line, revealed)` rebuilds them with `ctx.update_ui(choices, tree)`, where `tree` is that column holding one of the `Skip`, `Continue` and `Choices` (a `ChoiceList` taking `options`) templates;
  - after the update, the page gives the new choice the focus with `ctx.focus_ui(..)` (found by its name), because an updated scope is not a newly opened one and keeps 4b's focus rule;
  - the leave dialog is a `Confirm` template.
- **Mixed** (`mixed.ui.ron`): the root is `Grid(columns: 4, gap: 6, embedded: true)`; cells are a `Cell` template (`params: ["n"]`, `Button(label: "Cell {n}", action: "cell", width: 80)`) built by code with `.insert(Cell(n))` and added with `spawn_ui_in`, including the first ones. The closure part of the page stays as it is (`ui.node(grid, ctx.world)`).

- [ ] **Step 1: Find the captures of pages 13-15** that Task 10 Step 1 kept (take them again with `./dev demos:tour` from the commit before Task 10 if they are gone).

- [ ] **Step 2: Write the files and rewrite the pages.**

- [ ] **Step 3: Run the acceptance tests**

Run: `cargo test -p ui-demo --test acceptance` and `cargo test --test integration ui_file_validity`
Expected: PASS, including `the_pad_moves_an_item_into_its_equipment_slot`, `escape_puts_back_an_item_the_pad_holds_over_a_button` (both pages) and `the_mixed_pages_focus_path_runs_through_the_node_grid`.

- [ ] **Step 4: Compare captures and hand-check the dialogue**

Capture pages 13-15 again and compare with Step 1's; they should match. Then run the demo (`./dev ui`), open the dialogue page and play it by keyboard: Skip, Continue and a choice should each take the focus as they appear.

- [ ] **Step 5: Run the gate and commit**

```bash
git add examples/demos/ui/assets/ui/inventory.ui.ron examples/demos/ui/assets/ui/dialogue.ui.ron examples/demos/ui/assets/ui/mixed.ui.ron examples/demos/ui/src/scenes/file_inventory_page.rs examples/demos/ui/src/scenes/file_dialogue_page.rs examples/demos/ui/src/scenes/mixed_page.rs examples/demos/ui/src/scenes/mod.rs examples/demos/ui/src/lib.rs examples/demos/ui/tests/acceptance.rs
git commit -m "feat: the ui demo's inventory, dialogue and Mixed pages load from UI files

- Inventory slots and Mixed cells are file templates built by code into placeholders
- The dialogue's HUD uses bindings and its choices are rebuilt with ctx.update_ui
- The acceptance walks run over the file pages"
```

---

### Task 12: Breakout's menus and HUD from files

**Files:**
- Create: `examples/games/breakout/assets/ui/menu.ui.ron`, `pause.ui.ron`, `quit_confirm.ui.ron`, `hud.ui.ron`
- Modify: `examples/games/breakout/src/look.rs` (`ui_file`), `src/scenes/menu.rs`, `src/scenes/pause.rs`, `src/scenes/quit_confirm.rs`, `src/scenes/game.rs`
- Test: `examples/games/breakout/tests/menus.rs` (existing pause test, plus a HUD test), `tests/integration/ui_file_validity.rs` (covers the new files)

**Interfaces:**
- Consumes: Tasks 2, 7 (`load_ui`, `add_ui`), 8.
- Produces: `look::ui_file(ctx: &mut SceneContext, path: &str, embedded: &str) -> Result<UiFile, UiFileError>`.

Rules:
- **Loading:** breakout runs on the web and its tests run in a temporary working directory, so each file is also embedded: `ui_file` tries `ctx.assets.load_ui(path)` (from disk, hot reloaded in debug builds), and if the file is not found, `ctx.assets.add_ui(path, embedded)`, with `embedded` from `include_str!("../assets/ui/<name>.ui.ron")`. Each scene spawns with `ctx.spawn_ui(file.root(ctx.assets)?)`; an error is logged with `log::error!`-style engine logging and the scene goes on without that UI (no panic).
- **Menu** (`menu.ui.ron`): one root, a `Stack(fill: true, background: "#141423")` holding what were separate roots: the three text lines (anchored `TopCenter` at the same offsets, with `High Score: {high}` bound), the row of six coloured blocks, and the start button's focus-scope column. The scene sets `high` with `ctx.ui_set`. The look must not change (compare captures).
- **Pause** (`pause.ui.ron`): one root, a dim `Stack(fill: true, background: "#000000b4")` holding the panel (anchored `Center`). The volume rows are a `VolumeRow` template (`params: ["label", "name", "shown"]`, `Slider(range: (0, 100), bind: name, name: name)`, then `Text(shown)`), used as `VolumeRow(label: "Master", name: "master", shown: "{master:.0}%")`. Sliders now run 0-100: the scene sets `master`, `music` and `sfx` from its settings times 100 when it spawns, and on a `Changed` event reads `ctx.ui_get::<f32>(root, name)` divided by 100. The percent labels follow by binding, so the code that wrote `{name}-percent` labels goes. `Score: {score}` is bound too.
- **Quit confirm** (`quit_confirm.ui.ron`): `(root: Confirm(title: "Quit to Desktop?", yes: "YES", no: "NO"))`.
- **HUD** (`hud.ui.ron`): one full-width row (`Row(anchor: TopLeft, width: Pct(1), padding: (20, 10), justify: SpaceBetween, ..)`) with `Lives: {lives}` and `Level: {level}` on the left and `Score: {score}` and `High: {high}` (yellow) on the right, spaced to match today's 18-unit line spacing. `GameScene` spawns it on entering and sets the four names in `ui` each frame with `ctx.ui_set` (which writes nothing when a value has not changed). The four `draw_text_anchored_styled` calls for them go; the "+1" fly-by and the game-over, level and new-high-score banners stay closure UI.
- **Test:** add `the_hud_shows_the_score_through_its_bindings` to `tests/menus.rs`: start `GameScene` headlessly (in a temporary working directory, like the pause test), step a few frames, and assert the HUD's score text reads `Score: 0` (find it through the engine's world access the other tests use, or by a frame capture against a known region if they read pixels).

- [ ] **Step 1: Capture the menu, the pause menu, the quit dialog and a game frame before changing them**

Run: `cd examples/games/breakout && RYO_HEADLESS=1 RYO_CAPTURE_FRAMES=8 RYO_CAPTURE_EXIT=1 RYO_CAPTURE_DIR=/tmp/4d-breakout-before cargo run -q; cd -`, and again with `RYO_INPUT` reaching the game (Enter), the pause menu (Escape in game) and the quit dialog (Escape on the menu).
Expected: four PNGs to compare in Step 5.

- [ ] **Step 2: Write the HUD test and see it fail**

Run: `cargo test -p breakout --test menus the_hud_shows_the_score`
Expected: FAIL (the HUD is still drawn as closure text, so there is no score node).

- [ ] **Step 3: Write the files, `ui_file`, and rewrite the scenes.**

- [ ] **Step 4: Run breakout's tests**

Run: `cargo test -p breakout` and `cargo test --test integration ui_file_validity`
Expected: PASS: the pause menu is still navigable by pad, the HUD test passes, the ball tests pass.

- [ ] **Step 5: Compare captures**

Capture the same four screens and compare: they should look the same. Any difference in text placement means the file's offsets or gaps need adjusting, not the engine.

- [ ] **Step 6: Run the gate and commit**

```bash
git add examples/games/breakout/assets/ui/menu.ui.ron examples/games/breakout/assets/ui/pause.ui.ron examples/games/breakout/assets/ui/quit_confirm.ui.ron examples/games/breakout/assets/ui/hud.ui.ron examples/games/breakout/src/look.rs examples/games/breakout/src/scenes/menu.rs examples/games/breakout/src/scenes/pause.rs examples/games/breakout/src/scenes/quit_confirm.rs examples/games/breakout/src/scenes/game.rs examples/games/breakout/tests/menus.rs
git commit -m "feat: breakout's menus and HUD load from UI files

- Menu, pause and quit dialogs are files; the HUD shows score, lives and level through bindings
- Pause volumes are bound sliders whose percent labels follow them
- Files load from disk (hot reloaded in debug) and fall back to embedded copies"
```

---

### Task 13: Documentation

**Files:**
- Modify: `.claude/skills/ryo-ui/SKILL.md` and `src/cli/skills/ryo-ui.md`, `.claude/skills/ryo-api/SKILL.md` and `src/cli/skills/ryo-api.md`, `.claude/skills/ryo-cli/SKILL.md` and `src/cli/skills/ryo-cli.md`, `.claude/skills/ryo-messages/SKILL.md` and `src/cli/skills/ryo-messages.md`, `src/cli/commands/install_skills.rs` (only if a skill's frontmatter description changes), `src/core/ui/AGENTS.md`, `src/core/asset/AGENTS.md`, `src/cli/AGENTS.md`, `docs/architecture/overview.md`, `docs/backlog/ui-retained-nodes.md`
- Test: `cargo test -p ryo2d --lib embedded_skills_are_their_sources`

**Interfaces:**
- Consumes: everything above (read the code, not this plan, for the final names).
- Produces: documentation only.

Contents:
- **`ryo-ui`**: a "Files" section after "Nodes":
  - when to use a file, a code tree or closure UI;
  - the syntax and short forms, with the vocabulary tables;
  - templates (parameters, defaults, root options, node parameters, `{param}` in strings and how it differs from a binding);
  - imports;
  - placeholders;
  - bindings (`bind`, patterns, `ui_set`/`ui_get`, seeds, per-root values, the same-frame rule);
  - `ctx.update_ui` and the reconcile rules a game needs to know (what survives, the focus rule after an update);
  - hot reload and `UiReloaded`;
  - `check-ui`.
  Recipes:
  - a menu file with events;
  - a HUD with bindings;
  - a list built from a template into a placeholder;
  - rebuilding choices with `update_ui`;
  - loading with an embedded fallback (breakout's `ui_file`).
- **`ryo-api`**: the new signatures:
  - `UiFile`, `UiFileError`, `UiFileIssue`, `UiArgs`, `UiArg`, `BindValue`, `UiReloaded`;
  - the `AssetManager` and `SceneContext` methods;
  - `UiNode::key`/`bind`/`bound`;
  - the `UiHarness` additions.
- **`ryo-cli`**: `check-ui`.
- **`ryo-messages`**: `UiReloaded`.
- **`src/core/ui/AGENTS.md`**, a Files section and its conventions:
  - the reader is ours and why;
  - the one vocabulary table and how to add an option (`OptionSpec` and the `Kind::takes` list);
  - the reconcile's ownership rules (`UiTreeNode`, `UiAdded`, instances, `UiApplied` and the three-way rule);
  - bindings (components stay the truth; the sync points);
  - reloads (`reload_trees` before the UI pass, generations).
- **`src/core/asset/AGENTS.md`**: UI files in the asset manager (`load_ui`, `add_ui`, `reload_ui`, importer rechecks, `ron` in the watch, warnings once per broken text).
- **`src/cli/AGENTS.md`**: `check-ui` and that its checks live in `src/core/ui/file/check.rs`.
- **`docs/architecture/overview.md`**: UI files in the UI section.
- **`docs/backlog/ui-retained-nodes.md`**: 4d done (what landed, the update-versus-spawn bench numbers from Task 4, the 4c guard result), and what 4d left (Rust templates in files, loops and conditionals, localisation, slider/progress/rebind labels as patterns).

- [ ] **Step 1: Write the docs.** After editing each skill, `cp .claude/skills/<name>/SKILL.md src/cli/skills/<name>.md` and `cmp` them.

- [ ] **Step 2: Check the embedded skills and every doc example**

Run: `cargo test -p ryo2d --lib embedded_skills_are_their_sources` and `cargo test -p ryo2d --doc`
Expected: PASS.

- [ ] **Step 3: Run the gate and commit**

```bash
git add .claude/skills/ryo-ui/SKILL.md src/cli/skills/ryo-ui.md .claude/skills/ryo-api/SKILL.md src/cli/skills/ryo-api.md .claude/skills/ryo-cli/SKILL.md src/cli/skills/ryo-cli.md .claude/skills/ryo-messages/SKILL.md src/cli/skills/ryo-messages.md src/core/ui/AGENTS.md src/core/asset/AGENTS.md src/cli/AGENTS.md docs/architecture/overview.md docs/backlog/ui-retained-nodes.md
git commit -m "docs: UI files, templates, bindings and update_ui

- The ui skill's Files section and recipes; API, CLI and message references
- Module notes for files, the reconcile, bindings and UI files in the asset manager
- The retained UI backlog marks 4d done with its bench numbers"
```

(Add `src/cli/commands/install_skills.rs` to the `git add` only if you changed a description there.)
