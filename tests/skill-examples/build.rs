//! Writes each skill's Rust examples as a Rust file whose lines match the
//! skill's, and `skills.rs`, a module per skill that includes it (see the
//! crate's notes for what a block may declare)

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
  let skills = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.claude/skills");
  let out = PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
  println!("cargo:rerun-if-changed={}", skills.display());
  println!("cargo:rerun-if-env-changed=SKILL_EXAMPLES");
  // Only these skills, comma-separated, when set: while fixing some, the
  // others' errors stay out of the way
  let only = std::env::var("SKILL_EXAMPLES").ok();
  let wanted = |name: &str| {
    only
      .as_deref()
      .is_none_or(|only| only.split(',').any(|skill| skill.trim() == name))
  };
  let mut names: Vec<String> = fs::read_dir(&skills)
    .expect(".claude/skills is readable")
    .filter_map(|entry| {
      let entry = entry.ok()?;
      entry
        .path()
        .join("SKILL.md")
        .is_file()
        .then(|| entry.file_name().to_string_lossy().into_owned())
    })
    .filter(|name| wanted(name))
    .collect();
  names.sort();
  let mut modules = String::new();
  for name in names {
    let skill = skills.join(&name).join("SKILL.md");
    println!("cargo:rerun-if-changed={}", skill.display());
    let text = fs::read_to_string(&skill).expect("a skill is UTF-8 text");
    let file = out.join(&name).join("SKILL.md.rs");
    fs::create_dir_all(out.join(&name)).expect("OUT_DIR is writable");
    fs::write(&file, examples(&text)).expect("OUT_DIR is writable");
    let _ = writeln!(
      modules,
      "#[allow(warnings, clippy::all, clippy::pedantic)]\npub mod {} {{ include!({:?}); }}",
      name.replace('-', "_"),
      file.display().to_string(),
    );
  }
  fs::write(out.join("skills.rs"), modules).expect("OUT_DIR is writable");
}

/// The skill's Rust examples, each at its own lines: every other line of
/// the markdown is blank, a block's opening fence holds the function's head
/// and its closing fence the function's end
fn examples(text: &str) -> String {
  let lines: Vec<&str> = text.lines().collect();
  let mut out = vec![String::new(); lines.len()];
  let mut i = 0;
  while i < lines.len() {
    let Some(info) = lines[i].trim_start().strip_prefix("```") else {
      i += 1;
      continue;
    };
    let Some(end) = (i + 1..lines.len()).find(|&j| lines[j].trim() == "```") else {
      break;
    };
    let mut words = info.split([',', ' ']).filter(|word| !word.is_empty());
    let rust = words.next() == Some("rust");
    if rust && !words.any(|word| word == "ignore") {
      let code = &lines[i + 1..end];
      let (head, foot) = wrapper(i + 1, &notes(&lines[..i]), code);
      out[i] = head;
      for (j, line) in code.iter().enumerate() {
        out[i + 1 + j] = (*line).to_string();
      }
      out[end] = foot;
    }
    i = end + 1;
  }
  if let Some(first) = out.first_mut() {
    first.insert_str(0, "use ryo2d::prelude::*; ");
  }
  out.join("\n") + "\n"
}

/// What the HTML comments just above a block declare
#[derive(Default)]
struct Notes {
  given: Vec<String>,
  fields: Vec<String>,
  with: Vec<String>,
}

fn notes(above: &[&str]) -> Notes {
  let mut notes = Notes::default();
  for line in above.iter().rev() {
    let Some(note) = line
      .trim()
      .strip_prefix("<!--")
      .and_then(|note| note.strip_suffix("-->"))
    else {
      break;
    };
    let note = note.trim();
    if let Some(given) = note.strip_prefix("given:") {
      notes.given.insert(0, given.trim().to_string());
    } else if let Some(fields) = note.strip_prefix("self:") {
      notes.fields.insert(0, fields.trim().to_string());
    } else if let Some(with) = note.strip_prefix("with:") {
      notes.with.insert(0, with.trim().to_string());
    }
  }
  notes
}

/// The function's head and end around block `code`, which starts at line
/// `line` (from 1)
fn wrapper(line: usize, notes: &Notes, code: &[&str]) -> (String, String) {
  let with = notes.with.join(" ");
  if is_methods(code) {
    let fields = notes.fields.join(", ");
    return (
      format!("pub fn line_{line}() {{ {with} struct Example {{ {fields} }} impl Example {{"),
      "} }".to_string(),
    );
  }
  let mut params = String::from("ctx: &mut SceneContext<'_>, dt: f32");
  for given in &notes.given {
    params.push_str(", ");
    params.push_str(given);
  }
  let (returns, ok) = match uses_try(code) {
    true => (" -> Result<(), Box<dyn std::error::Error>>", "Ok(())"),
    false => ("", ""),
  };
  // The `with:` declarations come first, outside `run`, so the parameters
  // and fields can name them
  if notes.fields.is_empty() {
    (
      format!("pub fn line_{line}() {{ {with} fn run({params}){returns} {{"),
      format!("; {ok} }} }}"),
    )
  } else {
    // Statements from inside a method: a method of `Example`
    let fields = notes.fields.join(", ");
    (
      format!(
        "pub fn line_{line}() {{ {with} struct Example {{ {fields} }} impl Example {{ fn run(&mut self, {params}){returns} {{"
      ),
      format!("; {ok} }} }} }}"),
    )
  }
}

/// Whether the block is methods: its first line of code is a function
/// taking `self`
fn is_methods(code: &[&str]) -> bool {
  let Some(first) = code
    .iter()
    .map(|line| line.trim())
    .find(|line| !line.is_empty() && !line.starts_with("//") && !line.starts_with("#["))
  else {
    return false;
  };
  let first = first.strip_prefix("pub ").unwrap_or(first);
  first.starts_with("fn ")
    && ["(&self", "(&mut self", "(self", "(mut self"]
      .iter()
      .any(|receiver| first.contains(receiver))
}

/// Whether the block uses `?` outside its comments and strings
fn uses_try(code: &[&str]) -> bool {
  code.iter().any(|line| {
    let mut in_string = false;
    let chars: Vec<char> = line.chars().collect();
    for (k, &c) in chars.iter().enumerate() {
      match c {
        '"' if k == 0 || chars[k - 1] != '\\' => in_string = !in_string,
        '/' if !in_string && chars.get(k + 1) == Some(&'/') => return false,
        '?' if !in_string => {
          let next = chars[k + 1..].iter().find(|c| !c.is_whitespace());
          if next.is_none_or(|next| ";.),]".contains(*next)) {
            return true;
          }
        }
        _ => {}
      }
    }
    false
  })
}
