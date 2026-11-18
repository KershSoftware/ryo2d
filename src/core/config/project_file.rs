//! A game's Ryo.toml compiled into the binary, for targets with no disk to
//! read it from (the web).

use std::path::PathBuf;

use super::project::ProjectLayer;
use super::workspace::Workspace;

/// The text of a game's Ryo.toml, read when the game compiles
/// (`ryo2d::project!()`) and given to the engine with
/// [`EngineBuilder::project`](crate::core::engine::EngineBuilder::project).
///
/// The engine uses it where it finds no Ryo.toml of its own: on the web, or
/// natively when none is in the working directory or above. Its values resolve
/// as a `ryo2d build` of the same Ryo.toml would.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProjectFile {
  text: Option<&'static str>,
}

impl ProjectFile {
  /// A project file with this Ryo.toml text
  pub const fn from_toml(text: &'static str) -> Self {
    ProjectFile { text: Some(text) }
  }

  /// No project file: the game has no Ryo.toml
  pub const fn none() -> Self {
    ProjectFile { text: None }
  }

  /// Whether the game has no Ryo.toml
  pub const fn is_none(&self) -> bool {
    self.text.is_none()
  }

  /// The Ryo.toml text, when there is one
  pub const fn text(&self) -> Option<&'static str> {
    self.text
  }

  /// What the engine takes from the file, resolving as a release build does.
  /// `None` without a file, or when its text is not TOML (warned).
  pub(crate) fn read(&self) -> Option<FileProject> {
    let text = self.text?;
    let workspace = match Workspace::from_toml_str(text, PathBuf::new()) {
      Ok(workspace) => workspace,
      Err(error) => {
        log::warn!("{error}; the project file is ignored");
        return None;
      }
    };
    // Without a folder to name the project after, an unnamed one has no name
    let name = text
      .parse::<toml::Table>()
      .ok()
      .and_then(|table| {
        table
          .get("project")?
          .get("name")?
          .as_str()
          .map(str::to_string)
      })
      .filter(|name| !name.trim().is_empty());
    let mut layer = ProjectLayer::for_release(&workspace);
    layer.title = workspace.project().title.clone().or_else(|| name.clone());
    Some(FileProject {
      layer,
      name,
      save_folder: workspace.config().release.save_folder.clone(),
    })
  }
}

/// A [`ProjectFile`]'s values
pub(crate) struct FileProject {
  pub layer: ProjectLayer,
  /// `[project] name`, when the file sets one
  pub name: Option<String>,
  /// `[release] save_folder`
  pub save_folder: Option<String>,
}
