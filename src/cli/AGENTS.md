# AGENTS.md - CLI Module

## Conventions

- **Workspace-aware** - Resolve the project root with `discover_workspace()` (wraps `Workspace::discover()`, which finds `Ryo.toml`). Never use raw cwd paths
- **Wrap Cargo, don't reimplement it** - `build` → `cargo build --release`; `dev` → `cargo run`; `run` executes the built binary
- **Output** - Use the `Output::status/success/warning/error` helpers (Cargo-style right-aligned labels, no color)

## Skills

- `src/cli/skills/*.md` are compiled into the binary via `include_str!` in `EMBEDDED_SKILLS` (`commands/install_skills.rs`), and that is what `init`/`install-skills` ship to games
- They have drifted from `.claude/skills/*/SKILL.md` (most files differ). Update both when changing a skill, and add an `EMBEDDED_SKILLS` entry for a new one
