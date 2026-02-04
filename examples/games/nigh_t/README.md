# Nigh_t

A game built with the [Ryo2D](https://github.com/your-username/ryo2d) engine.

## Building and Running

### Development

```bash
# Run in debug mode (faster compilation, slower runtime)
cargo run

# Or use the Ryo2D CLI
ryo2d dev
```

### Release

```bash
# Build optimized release
ryo2d build

# Run the release build
ryo2d run

# Or distribute the ./release/ folder
```

## Project Structure

```
nigh_t/
├── main.rs          # Game entry point
├── Cargo.toml       # Rust package configuration
├── Ryo.toml         # Ryo2D project configuration
├── assets/          # Game assets (images, audio, etc.)
├── data/            # Save data (auto-generated)
└── src/             # Additional source modules (optional)
```

## Controls

Add your game controls here.

## License

Add your license here.
