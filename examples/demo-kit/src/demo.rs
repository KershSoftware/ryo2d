//! The demo builder

use std::rc::Rc;

use ryo2d::prelude::*;

use crate::controls::reserved_key;
use crate::page::DemoPage;
use crate::scene::PageScene;
use crate::start::{StartPageError, pages_label, start_page};

type PageFactory = Box<dyn Fn() -> Box<dyn DemoPage>>;
type Configure = Box<dyn FnOnce(EngineBuilder) -> EngineBuilder>;

/// What every page of one demo shares
pub(crate) struct DemoDef {
  pub title: String,
  pub pages: Vec<PageFactory>,
}

/// A demo: a title, a window size and its pages.
///
/// ```rust,ignore
/// fn main() {
///   Demo::new("Bloom").resolution(960, 640).page(Glow::default).run();
/// }
/// ```
pub struct Demo {
  title: String,
  resolution: Option<(u32, u32)>,
  configure: Vec<Configure>,
  pages: Vec<PageFactory>,
  start: usize,
}

impl Demo {
  pub fn new(title: impl Into<String>) -> Self {
    Self {
      title: title.into(),
      resolution: None,
      configure: Vec::new(),
      pages: Vec::new(),
      start: 0,
    }
  }

  /// Game resolution
  pub fn resolution(mut self, width: u32, height: u32) -> Self {
    self.resolution = Some((width, height));
    self
  }

  /// Any other engine setting, e.g. `.configure(|b| b.physics_hz(144))`
  pub fn configure(mut self, f: impl FnOnce(EngineBuilder) -> EngineBuilder + 'static) -> Self {
    self.configure.push(Box::new(f));
    self
  }

  /// Add a page; `factory` builds it fresh each time it is shown or reset
  pub fn page<P: DemoPage, F: Fn() -> P + 'static>(mut self, factory: F) -> Self {
    self
      .pages
      .push(Box::new(move || Box::new(factory()) as Box<dyn DemoPage>));
    self
  }

  /// Start on this page (0-based); `RYO_DEMO_PAGE` overrides it in `run`
  pub fn start_page(mut self, index: usize) -> Self {
    self.start = index;
    self
  }

  /// The engine builder for this demo, for `run` and for headless tests.
  ///
  /// Panics if there are no pages, the start page is out of range, or a
  /// page lists a reserved key.
  pub fn engine_builder(self) -> EngineBuilder {
    assert!(
      !self.pages.is_empty(),
      "demo \"{}\" has no pages",
      self.title
    );
    assert!(
      self.start < self.pages.len(),
      "demo \"{}\": start page {} is out of range",
      self.title,
      self.start
    );
    for factory in &self.pages {
      let page = factory();
      if let Some(key) = reserved_key(&page.controls()) {
        panic!(
          "demo \"{}\", page \"{}\": {key:?} is reserved by the demo shell",
          self.title,
          page.name()
        );
      }
    }

    let def = Rc::new(DemoDef {
      title: self.title,
      pages: self.pages,
    });
    let mut builder = EngineBuilder::new().title(format!("Ryo2D - {}", def.title));
    if let Some((width, height)) = self.resolution {
      builder = builder.resolution(width, height);
    }
    for configure in self.configure {
      builder = configure(builder);
    }
    builder.start_scene(PageScene::new(def, self.start))
  }

  /// Run the demo in a window (or offscreen with `RYO_HEADLESS=1`)
  pub fn run(mut self) {
    let pages = self.pages.len();
    println!("[demo] {}: {}", self.title, pages_label(pages));
    if cfg!(debug_assertions) {
      let raw = std::env::var("RYO_DEMO_PAGE").ok();
      match start_page(raw.as_deref(), &self.title, pages) {
        Ok(Some(index)) => self.start = index,
        Ok(None) => {}
        Err(StartPageError::Invalid(message)) => eprintln!("{message}"),
        Err(StartPageError::OutOfRange(message)) => {
          eprintln!("{message}");
          std::process::exit(2);
        }
      }
    }
    let engine = self.engine_builder().build();
    if let Err(error) = engine.run() {
      eprintln!("Engine error: {error}");
      // Scripts and the demo tour read the exit code
      std::process::exit(1);
    }
  }
}
