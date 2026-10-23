use std::fmt::Write as _;
use std::sync::Arc;

use super::AssetPath;

/// Why an asset did not load. Each error names the asset's path and keeps
/// what caused it as its `source()`
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AssetError {
  /// The source has no file at the path
  #[error("no asset at \"{path}\"")]
  NotFound {
    /// The asset's path
    path: AssetPath,
  },
  /// The file is there but could not be read
  #[error("could not read \"{path}\"")]
  Io {
    /// The asset's path
    path: AssetPath,
    /// What the read failed with
    #[source]
    source: std::io::Error,
  },
  /// The loader could not make the asset from the file's bytes
  #[error("could not load \"{path}\"")]
  Decode {
    /// The asset's path
    path: AssetPath,
    /// What the loader failed with
    #[source]
    source: Box<dyn std::error::Error + Send + Sync>,
  },
  /// No registered loader takes the path's extension and makes the asked-for type
  #[error("no loader for \"{path}\" (extension \"{extension}\")")]
  NoLoader {
    /// The asset's path
    path: AssetPath,
    /// The path's longest extension, without the leading dot
    extension: String,
  },
  /// A dependency the loader asked for did not load
  #[error("\"{path}\" needs \"{dependency}\", which did not load")]
  Dependency {
    /// The asset's path
    path: AssetPath,
    /// The dependency's path
    dependency: AssetPath,
    /// Why the dependency did not load
    #[source]
    error: Arc<AssetError>,
  },
  /// Loading the asset needs the asset itself, through its dependencies
  #[error("\"{path}\" imports itself: {chain}")]
  Cycle {
    /// The asset that imports itself
    path: AssetPath,
    /// The imports from it back to it: `a.ui.ron → b.ui.ron → a.ui.ron`
    chain: String,
  },
  /// The path is absolute, leaves the asset root or names nothing
  #[error("\"{path}\" is not an asset path: {reason}")]
  InvalidPath {
    /// The path as given
    path: String,
    /// What is wrong with it
    reason: &'static str,
  },
}

impl AssetError {
  /// A `Decode` error from any error type
  pub fn decode(
    path: &AssetPath,
    error: impl Into<Box<dyn std::error::Error + Send + Sync>>,
  ) -> AssetError {
    AssetError::Decode {
      path: path.clone(),
      source: error.into(),
    }
  }

  /// The error and its sources on one line, for the log
  pub(crate) fn report(&self) -> String {
    let mut line = self.to_string();
    let mut source = std::error::Error::source(self);
    while let Some(error) = source {
      let _ = write!(line, ": {error}");
      source = error.source();
    }
    line
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_report_follows_the_sources() {
    let path = AssetPath::parse("a.txt").map_err(|e| e.to_string());
    let Ok(path) = path else {
      panic!("a valid path: {path:?}");
    };
    let inner = Arc::new(AssetError::decode(&path, "bad byte"));
    let outer = AssetError::Dependency {
      path: path.clone(),
      dependency: path,
      error: inner,
    };
    assert_eq!(
      outer.report(),
      "\"a.txt\" needs \"a.txt\", which did not load: could not load \"a.txt\": bad byte"
    );
  }
}
