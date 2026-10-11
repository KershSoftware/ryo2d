# AGENTS.md - Ryo2D Game Engine

## Project Vision

**Ryo2D** is an **agent-enabled 2D game development platform** built in Rust. The primary goal is to make professional game development accessible through AI agents while maintaining a clean, well-documented codebase that works equally well for traditional developers.

### Core Principles

1. **Agent-First Design** - Clean APIs, clear documentation, and predictable patterns that AI agents can work with effectively
2. **No Coding Required** - Games can be built entirely through agentic mode (Claude, Copilot, etc.)
3. **Traditional Dev Friendly** - The codebase remains clean Rust that developers can read, modify, and extend without agents
4. **Production Quality** - Real games with real performance, not a toy engine
5. **Agent Skills** - Should always be included to improve efficiency and pattern repetition for game development (and engine development if applicable)

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

### Root Dev Scripts (`./dev.ps1` / `./dev.sh`)

**For Engine Development:** The root scripts are for working on the engine codebase itself:

```powershell
./dev              # Interactive example selection menu
./dev <example>    # Run specific example (e.g., ./dev breakout)
./dev check        # Compilation check (engine + all examples)
./dev lint         # Run clippy (engine + all examples)
./dev test         # Run all tests
./dev coverage     # Generate code coverage report
./dev help         # Show available commands
```

**Example Workflow:**
1. When working on game projects → use `ryo2d build/run/dev` from the project directory
2. When working on the engine and validating with example projects → use `./dev <project>` scripts from the repository root (this will build and run the built game)
OR
2. Simply run `cargo <cmd>` at root of the project if checking the engine codebase without example projects

---

## Nested AGENTS.md Files

Subdirectories carry their own AGENTS.md with module-specific conventions and design decisions - read the nearest one when working in that area.

---

## Specs and Plans

Designs and implementation plans from the superpowers skills live here, not in the skills' default `docs/superpowers/`:

- **Specs** (`superpowers:brainstorming`): `docs/specs/YYYY-MM-DD-<topic>-design.md`. A spec records a design's decisions and why; nested AGENTS.md files may point to it. Keep specs after the work lands. When later work changes a decision, the code and AGENTS.md files are the current truth.
- **Plans** (`superpowers:writing-plans`): `docs/plans/YYYY-MM-DD-<topic>-plan.md`, with a `**Spec:**` line naming its spec. A plan is a task checklist: delete it once it has been executed and reviewed. The code and git history are its record.
- **Other docs:** `docs/architecture/` holds the engine overview (`overview.md`) and the network architecture (`network.md`), `docs/backlog/` ideas for later, `docs/research/` background notes.

---

## Development Philosophy

**Production-grade code with clear documentation.**

- **Code quality:** Idiomatic Rust, proper error handling, no unwrap in library code
- **Performance:** Cache-friendly data structures, minimize allocations in hot paths
- **API design:** Ergonomic, hard to misuse, well-documented
- **Testing:** Unit tests for core logic, integration tests for systems, e2e for complex-but-worth-know-shouldn't-break
- **Documentation:** Rustdoc for API; the skills (`src/cli/skills/`) for using the engine; nested AGENTS.md for module conventions and decisions; `docs/architecture/` for the big picture. There is no separate guide book

**Code comments should be normal/minimal** - explain intent and non-obvious logic. Complex algorithms get thorough documentation.

We progress systematically, validating each change with tests and benchmarks where appropriate.

Unless explicitly told so, always confirm committing and the commit message with me before advancing.

## Notes for AI Agents

- **Progress incrementally** - don't jump ahead in the task list
- **Before finishing** - run `cargo check`, `cargo clippy`, `cargo test` and verify no regressions

### Commit Message Guidelines

- **Be concise** - focus on what changed and why
- **No test/build status** - don't include "tests passing" or "docs build successfully"
- **No unnecessary details** - avoid implementation minutiae unless critical to understanding
- **No AGENTS.md references** - these files are for AI context only, don't mention them in commits
- **No co-author lines** - never add "Co-Authored-By" or similar attribution lines
- **Format:** `type: brief description` followed by bullet points if needed
  - Types: `feat`, `fix`, `refactor`, `docs`, `test`, `chore`
