mod engine;

use engine::EngineBuilder;

fn main() {
  println!("=== Ferrum2D - Engine Core Demo ===\n");

  let mut engine = EngineBuilder::new()
    .title("Ferrum2D Demo")
    .resolution(1280, 720)
    .fps(60)
    .build();

  if let Err(e) = engine.init() {
    eprintln!("Failed to initialize engine: {}", e);
    return;
  }

  println!();

  if let Err(e) = engine.run() {
    eprintln!("Error during game loop: {}", e);
  }

  println!();

  engine.shutdown();

  println!("\n=== Demo Complete ===");
}
