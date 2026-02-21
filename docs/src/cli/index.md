# Command Line Interface

The `ryo2d` CLI provides a simple interface for building and running your games.

## Installation

Once Ryo2D is published to crates.io, you'll be able to install the CLI with:

```bash
cargo install ryo2d
```

For now, run commands from within your project directory.

## Commands

### `ryo2d init <project-name>`

Create a new Ryo2D project from scratch.

```bash
ryo2d init my-awesome-game
```

This command:
- Creates a new directory with the project name
- Scaffolds complete project structure (assets/, data/, src/)
- Generates Cargo.toml with Ryo2D dependency
- Creates Ryo.toml with sensible engine defaults (1280×720, master volume 0.8)
- Provides working main.rs with a Scene example
- Includes .gitignore with standard Rust patterns
- Generates README with quick start instructions

**Project name rules:**
- Must be lowercase
- Can contain hyphens and underscores
- Must be valid as both a directory name and Rust crate name

**When to use:** Starting a new game project.

**Example:**
```bash
ryo2d init my-game
cd my-game
ryo2d dev
```

### `ryo2d dev`

Development mode - the primary command during development.

```bash
ryo2d dev
```

This command:
- Builds your game in debug mode (faster compilation)
- Runs the game immediately
- Checks for dev profile optimization and offers to add it if missing
- *(Coming soon)* Watches for file changes and rebuilds automatically
- *(Coming soon)* Supports hot reload for assets

**When to use:** During active development when you're iterating quickly.

## Dev Build Performance

For playable performance in dev builds, your `Cargo.toml` should include:

```toml
# Optimize dependencies and engine in dev builds for playable performance
# Your game code stays unoptimized for fast compile times
[profile.dev.package."*"]
opt-level = 2

[profile.dev.package.ryo2d]
opt-level = 2
```

These settings optimize **dependencies** and the **Ryo2D engine** while keeping your game code unoptimized for fast compilation. Without them, dev builds can run extremely slowly because libraries like Rapier2D physics and the rendering code run unoptimized.

New projects created with `ryo2d init` include this automatically. If you're missing it, `ryo2d dev` will warn you and offer to add it.

| Build Type | Your Code | Dependencies | Use Case |
|------------|-----------|--------------|----------|
| `ryo2d dev` | Unoptimized | Optimized | Development |
| `ryo2d build` | Optimized | Optimized | Release |

### `ryo2d build`

Creates a production-optimized release build.

```bash
ryo2d build
```

This command:
- Compiles with `--release` optimizations
- Outputs to `target/release/`
- Enables all performance optimizations

**When to use:** When preparing for distribution or performance testing.

### `ryo2d run`

Runs the production build.

```bash
ryo2d run
```

This command:
- Executes the pre-built release binary
- Warns if no build exists (prompts to run `ryo2d build` first)
- Does not recompile - runs the existing binary

**When to use:** To test the release build or run a previously built game.

## Typical Workflow

### Starting a New Project

```bash
# Create and start a new game
ryo2d init my-game
cd my-game
ryo2d dev
```

### During Development

```bash
# Continue working on existing project
cd my_game
ryo2d dev
```

### Preparing for Release

```bash
# Create production build
ryo2d build

# Test the release build
ryo2d run
```

## Command Help

Each command supports `--help` for detailed information:

```bash
ryo2d --help
ryo2d init --help
ryo2d build --help
ryo2d run --help
ryo2d dev --help
```

## Workspace Detection

All commands automatically detect your Ryo2D workspace by searching for `Ryo.toml` in the current directory and parent directories. This means you can run commands from any subdirectory of your project.

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Build or runtime error |
| 101 | Workspace not found |

## Future Commands

The following commands are planned for future releases:

- `ryo2d assets` - Asset pipeline operations
- `ryo2d package` - Package for distribution (Windows installer, macOS app bundle, etc.)
