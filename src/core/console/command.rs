//! The types a game writes console commands with: a command's description
//! ([`CommandSpec`]) and what its handler receives ([`Command`]).
//!
//! These exist in every build, so a game's registration compiles in a build
//! without tools too, where it does nothing.

use std::str::FromStr;

#[cfg(dev_tools)]
use crate::core::ecs::World;
use crate::core::scene::SceneAction;
use crate::core::ui::Color;

/// A console command's name and help: a one-line description, and
/// optionally a usage line and longer help text (`help <name>` shows both).
///
/// The name is lower-cased when the command is registered, so any spelling
/// runs it. A `(name, description)` pair converts into a spec.
#[derive(Debug, Clone)]
pub struct CommandSpec {
  /// The name typed to run it, such as `game:spawn`
  pub name: String,
  /// One line, shown by `help`
  pub description: String,
  /// The arguments it takes, such as `game:spawn <kind> [count]`
  pub usage: Option<String>,
  /// Longer text for `help <name>`
  pub help: Option<String>,
}

impl CommandSpec {
  /// A command called `name`, described in one line.
  pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
    Self {
      name: name.into(),
      description: description.into(),
      usage: None,
      help: None,
    }
  }

  /// The usage line, printed with a bad argument's error.
  pub fn usage(mut self, usage: impl Into<String>) -> Self {
    self.usage = Some(usage.into());
    self
  }

  /// The longer text `help <name>` shows.
  pub fn help(mut self, help: impl Into<String>) -> Self {
    self.help = Some(help.into());
    self
  }
}

impl From<(&str, &str)> for CommandSpec {
  fn from((name, description): (&str, &str)) -> Self {
    Self {
      name: name.to_string(),
      description: description.to_string(),
      usage: None,
      help: None,
    }
  }
}

/// A running command, as its handler receives it: its name and arguments,
/// typed access to them, the lines it prints, and what it asks of the
/// engine afterwards.
///
/// The arguments are the words after the name; double quotes keep a phrase
/// together as one argument. Indexes count from 0 (`args()[0]` is the first
/// argument); error messages count from 1, as a person reads them.
pub struct Command<'a> {
  name: &'a str,
  args: &'a [String],
  usage: Option<&'a str>,
  lines: Vec<(String, Option<Color>)>,
  action: SceneAction,
  quit: bool,
}

impl<'a> Command<'a> {
  /// A command called `name` with its split arguments and usage line.
  #[cfg(any(dev_tools, test))]
  pub(crate) fn new(name: &'a str, args: &'a [String], usage: Option<&'a str>) -> Self {
    Self {
      name,
      args,
      usage,
      lines: Vec::new(),
      action: SceneAction::Continue,
      quit: false,
    }
  }
}

impl Command<'_> {
  /// The command's name, as registered (lower case).
  pub fn name(&self) -> &str {
    self.name
  }

  /// The arguments: the words after the name.
  pub fn args(&self) -> &[String] {
    self.args
  }

  /// Argument `index` parsed as `T`. The error names the argument (counted
  /// from 1), its text and the usage line, ready to print:
  /// `argument 2 ("abc") is not a number; usage: game:spawn <kind> [count]`.
  pub fn arg<T: FromStr>(&self, index: usize) -> Result<T, String> {
    let position = index.saturating_add(1);
    let Some(text) = self.args.get(index) else {
      return Err(self.with_usage(format!("argument {position} is missing")));
    };
    text.parse().map_err(|_| {
      let problem = if is_number::<T>() {
        "is not a number"
      } else {
        "is not valid"
      };
      self.with_usage(format!("argument {position} ({text:?}) {problem}"))
    })
  }

  /// Argument `index` parsed as `T`, or `default` when there are fewer
  /// arguments. An argument that is there but does not parse is an error.
  pub fn arg_or<T: FromStr>(&self, index: usize, default: T) -> Result<T, String> {
    if index < self.args.len() {
      self.arg(index)
    } else {
      Ok(default)
    }
  }

  /// The arguments from `from` on, joined by spaces (empty when there are
  /// none), as for a message or a path.
  pub fn rest(&self, from: usize) -> String {
    self
      .args
      .get(from..)
      .map(|rest| rest.join(" "))
      .unwrap_or_default()
  }

  /// Print a line to the console.
  pub fn print(&mut self, text: impl Into<String>) {
    self.lines.push((text.into(), None));
  }

  /// Print a line in `color`.
  pub fn print_colored(&mut self, text: impl Into<String>, color: Color) {
    self.lines.push((text.into(), Some(color)));
  }

  /// Ask for a scene change, applied when the handler returns. The last
  /// request wins.
  pub fn act(&mut self, action: SceneAction) {
    self.action = action;
  }

  /// Ask the game to quit when the handler returns.
  pub fn quit(&mut self) {
    self.quit = true;
  }

  /// The lines printed so far, in order, with their colours (`None` for the
  /// console's text colour); the command keeps none.
  #[cfg(any(dev_tools, test))]
  pub(crate) fn take_lines(&mut self) -> Vec<(String, Option<Color>)> {
    std::mem::take(&mut self.lines)
  }

  /// The requested scene action (`Continue` when none), leaving `Continue`.
  #[cfg(any(dev_tools, test))]
  pub(crate) fn take_action(&mut self) -> SceneAction {
    std::mem::take(&mut self.action)
  }

  /// Whether the handler asked to quit.
  #[cfg(any(dev_tools, test))]
  pub(crate) fn quit_requested(&self) -> bool {
    self.quit
  }

  fn with_usage(&self, message: String) -> String {
    match self.usage {
      Some(usage) => format!("{message}; usage: {usage}"),
      None => message,
    }
  }
}

/// Whether `T` is one of the standard number types, told by its name (for
/// the wording of a bad argument's error).
fn is_number<T>() -> bool {
  matches!(
    std::any::type_name::<T>(),
    "u8"
      | "u16"
      | "u32"
      | "u64"
      | "u128"
      | "usize"
      | "i8"
      | "i16"
      | "i32"
      | "i64"
      | "i128"
      | "isize"
      | "f32"
      | "f64"
  )
}

/// Split a console line into words. Spaces separate words; double quotes
/// keep a phrase (spaces and all) in one word, and `""` is an empty word; a
/// quote left open runs to the end of the line.
#[cfg(any(dev_tools, test))]
pub(crate) fn split(line: &str) -> Vec<String> {
  let mut words = Vec::new();
  let mut word = String::new();
  let mut in_word = false;
  let mut quoted = false;
  for c in line.chars() {
    match c {
      '"' => {
        quoted = !quoted;
        in_word = true;
      }
      c if c.is_whitespace() && !quoted => {
        if in_word {
          words.push(std::mem::take(&mut word));
          in_word = false;
        }
      }
      c => {
        word.push(c);
        in_word = true;
      }
    }
  }
  if in_word {
    words.push(word);
  }
  words
}

/// A command handler registered on the engine builder: called with the
/// world, for the whole run.
#[cfg(dev_tools)]
pub type WorldHandler = dyn Fn(&mut World, &mut Command<'_>) -> Result<(), String> + Send + Sync;

#[cfg(test)]
mod tests {
  use super::*;

  fn command<'a>(name: &'a str, args: &'a [String], usage: Option<&'a str>) -> Command<'a> {
    Command::new(name, args, usage)
  }

  #[test]
  fn words_split_on_spaces_and_quotes_keep_phrases() {
    assert_eq!(
      split(r#"say "hello there" now"#),
      ["say", "hello there", "now"]
    );
    assert_eq!(split(r#"say "open"#), ["say", "open"]);
    assert_eq!(split("  spaced   out  "), ["spaced", "out"]);
    assert_eq!(split(r#"empty "" arg"#), ["empty", "", "arg"]);
    assert!(split("   ").is_empty());
  }

  #[test]
  fn a_typed_argument_parses_or_names_itself() {
    let args = split("orc abc");
    let cmd = command("game:spawn", &args, Some("game:spawn <kind> [count]"));
    assert_eq!(cmd.arg::<String>(0).as_deref(), Ok("orc"));
    assert_eq!(
      cmd.arg::<u32>(1),
      Err(r#"argument 2 ("abc") is not a number; usage: game:spawn <kind> [count]"#.to_string())
    );
    assert_eq!(
      cmd.arg::<u32>(2),
      Err("argument 3 is missing; usage: game:spawn <kind> [count]".to_string())
    );
    assert_eq!(
      cmd.arg::<bool>(0),
      Err(r#"argument 1 ("orc") is not valid; usage: game:spawn <kind> [count]"#.to_string())
    );
    let bare = command("game:spawn", &args, None);
    assert_eq!(
      bare.arg::<f32>(1),
      Err(r#"argument 2 ("abc") is not a number"#.to_string())
    );
  }

  #[test]
  fn a_missing_argument_takes_its_default() {
    let args = split("orc 3");
    let cmd = command("game:spawn", &args, None);
    assert_eq!(cmd.arg_or(1, 1u32), Ok(3));
    assert_eq!(cmd.arg_or(2, 7u32), Ok(7));
    assert!(
      cmd.arg_or(0, 1u32).is_err(),
      "a present but bad argument is an error"
    );
  }

  #[test]
  fn rest_joins_what_is_left() {
    let args = split(r#"say hello "big world" again"#);
    let mut cmd = command("say", &args, None);
    assert_eq!(cmd.name(), "say");
    assert_eq!(cmd.args().len(), 4);
    assert_eq!(cmd.rest(1), "hello big world again");
    assert_eq!(cmd.rest(3), "again");
    assert_eq!(cmd.rest(4), "");
    assert_eq!(cmd.rest(9), "");

    cmd.print("plain");
    cmd.print_colored("red", Color::RED);
    cmd.act(SceneAction::Pop);
    cmd.quit();
    assert_eq!(
      cmd.take_lines(),
      [
        ("plain".to_string(), None),
        ("red".to_string(), Some(Color::RED))
      ]
    );
    assert!(cmd.take_lines().is_empty());
    assert!(matches!(cmd.take_action(), SceneAction::Pop));
    assert!(matches!(cmd.take_action(), SceneAction::Continue));
    assert!(cmd.quit_requested());
  }
}
