# AGENTS.md - CLI Module

## Conventions

- **Workspace-aware** - Resolve the project root with `discover_workspace()` (wraps `Workspace::discover()`, which finds `Ryo.toml`). Never use raw cwd paths
- **Wrap Cargo, don't reimplement it** - `build` → `cargo build --release`; `dev` → `cargo run`; `run` executes the built binary
- **Build validates first** - `build` validates the `[release]` save folder name before compiling, and creates `release/data/` only in portable mode
- **Output** - Use the `Output::status/success/warning/error` helpers (Cargo-style right-aligned labels, no color)
- **Font licences** - `build` writes `release/licenses/` and `export` writes it into the release before zipping: the built-in fonts' licence texts (`src/assets/fonts/`, two SIL OFL 1.1 and one CC0) and a README naming the fonts, compiled in with `include_str!` (`commands/licenses.rs`). OFL condition 2 asks that every copy of the fonts carries them. A new built-in font needs its licence added there. The CLI has no web build; a game shipped to the web by hand must ship `licenses/` itself

- **`check-ui`** (`commands/check_ui.rs`) checks `*.ui.ron` files without running the game: the files given, or every one under the assets directory. The checks themselves live in `src/core/ui/file/check.rs` (the load checks, imports read from disk, image paths that do not exist), so the command only finds files, prints `path:line:column: message` and exits non-zero on any problem. Paths in UI files are asset paths, relative to the assets directory, and issue paths are printed the same way (`ui/menu.ui.ron`), with `/` separators; a file given that is not under the assets directory is reported as not an asset path.

## Skills

- `.claude/skills/<name>/SKILL.md` is the one copy of each skill: `init` and `install-skills` ship it as it is. The build script (`src/build.rs`, `list_skills`) writes `embedded_skills.rs` to `OUT_DIR`, an `include_str!` of every skill folder's `SKILL.md`, which `EMBEDDED_SKILLS` (`commands/install_skills.rs`) includes. An edited skill rebuilds through `include_str!`, and a new or removed folder reruns the build script, so a skill needs no step beyond writing it
- The listing printed during install reads each description from the skill's frontmatter (`skill_description`). Only `SKILL.md` is shipped: a skill that grows other files needs the installer to ship them too
- `installing_writes_every_skill_folder_as_it_is` installs into a temporary folder and checks it holds every skill folder's `SKILL.md` byte for byte, and nothing else
