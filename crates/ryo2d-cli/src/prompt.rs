//! Questions put to a person: every prompt in the command goes through here.

use std::io::{self, BufRead, IsTerminal, Write};

/// Where answers come from. With no terminal there is no one to ask, so every
/// question is answered "no" without reading anything.
pub struct Prompt {
  /// Whether a person is there to answer
  pub interactive: bool,
  /// The answers, a line each
  pub input: Box<dyn BufRead>,
}

impl Prompt {
  /// The terminal on stdin; not interactive when stdin is a pipe or a file
  pub fn from_stdin() -> Prompt {
    Prompt {
      interactive: io::stdin().is_terminal(),
      input: Box::new(io::stdin().lock()),
    }
  }

  /// Fixed answers, for tests
  #[cfg(test)]
  pub fn scripted(interactive: bool, answers: &str) -> Prompt {
    Prompt {
      interactive,
      input: Box::new(io::Cursor::new(answers.as_bytes().to_vec())),
    }
  }

  /// Asks `question` and reads a line: `None` with no terminal (nothing is
  /// read) or at the end of input
  pub fn ask(&mut self, question: &str) -> Option<String> {
    if !self.interactive {
      return None;
    }
    print!("{question}");
    let _ = io::stdout().flush();
    let mut line = String::new();
    match self.input.read_line(&mut line) {
      Ok(0) | Err(_) => None,
      Ok(_) => Some(line.trim().to_owned()),
    }
  }

  /// A yes or no: an empty line is `default_yes`; no terminal, the end of
  /// input and any other answer are no
  pub fn yes_no(&mut self, question: &str, default_yes: bool) -> bool {
    let hint = if default_yes { "[Y/n]" } else { "[y/N]" };
    match self.ask(&format!("  {question} {hint} ")) {
      None => false,
      Some(answer) if answer.is_empty() => default_yes,
      Some(answer) => matches!(answer.to_lowercase().as_str(), "y" | "yes"),
    }
  }

  /// One of `options`, by number or name; an empty line is `default`. `None`
  /// with no terminal, at the end of input or for an answer that is neither
  pub fn choose(&mut self, question: &str, options: &[&str], default: usize) -> Option<usize> {
    if !self.interactive {
      return None;
    }
    println!();
    println!("{question}");
    for (number, option) in options.iter().enumerate() {
      println!("  {}) {option}", number + 1);
    }
    println!();
    let answer = self.ask(&format!(
      "Choice (1-{}, Enter for {}): ",
      options.len(),
      default + 1
    ))?;
    if answer.is_empty() {
      return (default < options.len()).then_some(default);
    }
    if let Ok(number) = answer.parse::<usize>() {
      return (1..=options.len()).contains(&number).then(|| number - 1);
    }
    let answer = answer.to_lowercase();
    options.iter().position(|option| {
      let option = option.to_lowercase();
      option == answer || option.split_whitespace().next() == Some(answer.as_str())
    })
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn end_of_file_is_no() {
    let mut prompt = Prompt::scripted(true, "");
    assert!(
      !prompt.yes_no("Go?", true),
      "no answer is no, even by default"
    );
    assert_eq!(prompt.choose("Which?", &["a", "b"], 0), None);
    assert_eq!(prompt.ask("Name: "), None);
  }

  #[test]
  fn with_no_terminal_nothing_is_read_and_the_answer_is_no() {
    let mut prompt = Prompt::scripted(false, "y\n1\nname\n");
    assert!(!prompt.yes_no("Go?", true));
    assert_eq!(prompt.choose("Which?", &["a", "b"], 0), None);
    assert_eq!(prompt.ask("Name: "), None);
    let mut left = String::new();
    prompt.input.read_to_string(&mut left).unwrap_or_default();
    assert_eq!(left, "y\n1\nname\n", "the input was not touched");
  }

  #[test]
  fn answers_are_read_a_line_each() {
    let mut prompt = Prompt::scripted(true, "y\nn\n\n\nno\n2\nBeta\n\n9\nname \n");
    assert!(prompt.yes_no("a?", false));
    assert!(!prompt.yes_no("b?", true));
    assert!(prompt.yes_no("c?", true), "empty is the default");
    assert!(!prompt.yes_no("d?", false), "empty is the default");
    assert!(!prompt.yes_no("e?", true), "anything but yes is no");
    let options = ["Alpha (x)", "Beta (y)"];
    assert_eq!(prompt.choose("?", &options, 0), Some(1), "by number");
    assert_eq!(prompt.choose("?", &options, 0), Some(1), "by name");
    assert_eq!(
      prompt.choose("?", &options, 0),
      Some(0),
      "empty is the default"
    );
    assert_eq!(prompt.choose("?", &options, 0), None, "out of range");
    assert_eq!(prompt.ask("Name: "), Some("name".to_owned()));
  }
}
