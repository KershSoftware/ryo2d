# AGENTS.md - Ryo2D Game Engine

## Project Vision

**Ryo2D** is an **agent-enabled 2D game development platform** built in Rust. The primary goal is to make professional game development accessible through AI agents while maintaining a clean, well-documented codebase that works equally well for traditional developers.

### Core Principles

1. **Agent-First Design** - Clean APIs, clear documentation, and predictable patterns that AI agents can work with effectively
2. **No Coding Required** - Games can be built entirely through agentic mode (Claude, Copilot, etc.)
3. **Traditional Dev Friendly** - The codebase remains clean Rust that developers can read, modify, and extend without agents
4. **Production Quality** - Real games with real performance, not a toy engine
5. **Agent Skills** - Should always be included to improve efficiency and pattern repetition for game development (and engine development if applicable)

### How It Works

1. **Install globally** - Run `./install.ps1` (Windows) or `./install.sh` (Unix/Mac) from the engine repo
2. **Create anywhere** - Run `ryo2d init my-game` from any directory to scaffold a new game
3. **Build with agents** - Open the project in VS Code/Claude and use agentic mode to develop
4. **Ship it** - `ryo2d build` creates a production bundle

The engine installs Agent Skills automatically during `init`, giving your AI assistant domain-specific knowledge about the engine's APIs, patterns, and best practices.

---

## Architecture

**Entity-Component-System (hecs)** with Rapier2D physics integration. Hybrid CPU/GPU rendering with wgpu backend. Event-driven input and collision systems.

| Layer | Technology | Purpose |
|-------|------------|---------|
| ECS | hecs | Entity management, component storage |
| Physics | Rapier2D | Collision detection, rigid body simulation |
| Rendering | wgpu | GPU sprite batching, post-processing shaders |
| Audio | Kira | Sound effects, music playback |
| Windowing | winit | Cross-platform window management |

---

## CLI Tool (`ryo2d`)

The CLI manages game projects:

```bash
ryo2d init <name>       # Create a new game project
ryo2d build             # Build production release bundle
ryo2d run               # Run the production build
ryo2d dev               # Development mode with hot-reload
ryo2d export            # Package release into distributable zip
ryo2d install-skills    # Install Agent Skills to existing project
```

**Workspace-aware:** The CLI discovers `Ryo.toml` to find the project root.

---

## Agent Skills

When you run `ryo2d init`, Agent Skills are automatically installed to `.github/skills/`. These give AI assistants domain-specific knowledge about the engine without needing access to engine source code.

| Skill | Purpose |
|-------|---------|
| `ryo-api` | **Complete API reference** - all types, methods, signatures |
| `ryo-animation` | Sprite animations and state machines |
| `ryo-audio` | Sound effects and music |
| `ryo-cli` | CLI tool usage |
| `ryo-config-save` | Configuration and save system |
| `ryo-controller` | Gamepad/controller input - buttons, sticks, triggers |
| `ryo-ecs-internals` | ECS implementation details |
| `ryo-engine-dev` | For engine contributors |
| `ryo-events` | Event bus for decoupled inter-system communication |
| `ryo-game-dev` | Game development patterns, ECS, scenes, input |
| `ryo-network` | Multiplayer networking - client-server, replication, RPCs |
| `ryo-physics` | Rapier2D physics integration |
| `ryo-procgen` | Procedural generation - dungeons, caves, terrain |
| `ryo-renderer` | GPU rendering and shaders |
| `ryo-ui` | Hybrid UI system for HUDs, menus, and interfaces |
| `ryo-ecs-internals` | ECS implementation details |

The `ryo-api` skill is the canonical reference - other skills reference it for complete signatures.

---

## External Asset Resources

Suggested sources for game assets:

| Type | Resource | URL |
|------|----------|-----|
| Sound Effects | Sonniss | https://sonniss.com/category/sound-libraries/ |
| Background Music | Pixabay | https://pixabay.com/music/search/videogame/ |
| Pixel Art | Itch.io | https://itch.io/game-assets/free/tag-pixel-art |

---

## Engine Development

**For contributors working on the engine itself** (not game developers using it):

### Root Run Scripts (`./run.ps1` / `./run.sh`)

**For Engine Development:** The root scripts are for working on the engine codebase itself:

```powershell
./run              # Interactive example selection menu
./run <example>    # Run specific example (e.g., ./run breakout)
./run check        # Fast compilation check (engine only)
./run check-all    # Check + lint ALL examples in parallel
./run lint         # Run clippy with strict warnings (engine only)
./run test         # Run all tests
./run coverage     # Generate code coverage report
./run help         # Show available commands
```

**Engine Quality Checks:**
- `check` - Fast syntax and type checking (engine workspace only)
- `check-all` - Parallel check + clippy on engine AND all examples with summary table
- `lint` - Clippy linting with `-D warnings` (fail on warnings)
- `test` - All unit tests
- `coverage` - Code coverage report using cargo-llvm-cov

**Example Workflow:**
1. When working on game projects → use `ryo2d build/run/dev` from the project directory
2. When working on the engine and validating with example projects → use `./run <project>` scripts from the repository root (this will build and run the built game)
OR
2. Simply run `cargo <cmd>` at root of the project if checking the engine codebase without example projects

---

## Nested AGENTS.md Files

This project uses **nested AGENTS.md files** in subdirectories where additional context is helpful. Each nested file explains the purpose and conventions of that specific area.

| Location | Purpose |
|----------|---------|
| `AGENTS.md` (root) | Project overview, phases, architecture |
| `src/AGENTS.md` | Source code structure and module responsibilities |
| `src/core/AGENTS.md` | Core engine module organization and import patterns |
| `src/core/ecs/AGENTS.md` | ECS implementation patterns and design decisions |
| `src/core/events/AGENTS.md` | Event bus system for decoupled communication |
| `src/core/network/AGENTS.md` | Networking module architecture and usage |
| `src/core/network/replication/AGENTS.md` | Entity/component replication system |
| `src/core/network/rpc/AGENTS.md` | Remote procedure call system |
| `src/cli/AGENTS.md` | CLI tool commands and design principles |
| `docs/AGENTS.md` | Documentation system (mdbook + rustdoc) |
| `.github/skills/` | AI skill definitions for specialized tasks |

When working in a subdirectory, check for a local AGENTS.md for relevant context.

---

## Development Philosophy

**Production-grade code with clear documentation.**

- **Code quality:** Idiomatic Rust, proper error handling, no unwrap in library code
- **Performance:** Cache-friendly data structures, minimize allocations in hot paths
- **API design:** Ergonomic, hard to misuse, well-documented
- **Testing:** Unit tests for core logic, integration tests for systems
- **Documentation:** Rustdoc for API, mdbook for guides and tutorials

**Code comments should be normal/minimal** - explain intent and non-obvious logic. Complex algorithms get thorough documentation.

We progress systematically, validating each change with tests and benchmarks where appropriate.

Unless explicitly told so, always confirm committing and the commit message with me before advancing.

## Notes for AI Agents

When assisting with this project:

1. **Write production-quality code** - proper error handling, no shortcuts
2. **Keep code comments minimal** - normal commenting practices, document intent
3. **Prioritize performance** - avoid allocations in hot paths, use appropriate data structures
4. **Validate changes** - run `cargo test`, `cargo clippy`, verify no regressions
5. **Progress incrementally** - don't jump ahead in the task list
6. **Run the full check suite** - `cargo check`, `cargo clippy`, `cargo test`
7. **Consider API ergonomics** - make the engine pleasant to use
8. **Simplicity but flexibility is key** - aim for similar simplicity in the API
9. **Use established crates** where appropriate - don't reinvent the wheel

### Commit Message Guidelines

- **Be concise** - focus on what changed and why
- **No test/build status** - don't include "tests passing" or "docs build successfully"
- **No unnecessary details** - avoid implementation minutiae unless critical to understanding
- **No AGENTS.md references** - these files are for AI context only, don't mention them in commits
- **No co-author lines** - never add "Co-Authored-By" or similar attribution lines
- **Format:** `type: brief description` followed by bullet points if needed
  - Types: `feat`, `fix`, `refactor`, `docs`, `test`, `chore`
