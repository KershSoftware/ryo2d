# AGENTS.md - CLI Module

## Conventions

- **Workspace-aware** - Resolve the project root with `discover_workspace()` (wraps `Workspace::discover()`, which finds `Ryo.toml`). Never use raw cwd paths
- **Wrap Cargo, don't reimplement it** - `build` → `cargo build --release`; `dev` → `cargo run`; `run` executes the built binary
- **Build validates first** - `build` validates the `[release]` save folder name before compiling, and creates `release/data/` only in portable mode
- **Output** - Use the `Output::status/success/warning/error` helpers (Cargo-style right-aligned labels, no color)

## Skills

- `src/cli/skills/*.md` are compiled into the binary via `include_str!` in `EMBEDDED_SKILLS` (`commands/install_skills.rs`), and that is what `init`/`install-skills` ship to games
- They are verbatim copies of `.claude/skills/<name>/SKILL.md`: edit the skill there, copy it to `src/cli/skills/<name>.md`, and keep its `EMBEDDED_SKILLS` description in step with the skill's frontmatter (add an entry for a new skill)
