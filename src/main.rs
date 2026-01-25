mod engine;

use engine::EngineBuilder;

fn main() {
  let engine = EngineBuilder::new()
    .title("Ferrum2D Demo")
    .resolution(1280, 720)
    .fps(60)
    .build();

  if let Err(e) = engine.run() {
    eprintln!("Engine error: {}", e);
  }
}
