# The `ryo2d` command, made trustworthy

The first of three sub-projects from the audit's remaining smaller items (the CLI, input, the engine's structure), built in that order. The owner asked for them after the audit's six were done, and said "skip questions": the design follows what a professional engine's tool does, and the calls below are mine unless marked as the owner's.

The October audit (`docs/research/2026-10-11-engine-audit.md`, "CLI") said to leave the command until the engine settled. It has settled. A re-audit on 2026-11-18 (main 51f7e0da) found it still hand-parsed, with copied helpers, templates nobody compiles, unsafe code to date a zip, and three faults that matter to someone shipping or scripting a game:

- `export`'s zip drops the executable bit, so a Mac or Linux player's game will not start without `chmod +x`.
- Prompts treat no input (end of file) as yes: `dev` rewrites a hand-made project's `Cargo.toml`, and `init` installs **Copilot** skills, when an agent or CI runs them.
- `run` exits 0 when the game crashes; `dev` reports a game that panicked as "Build failed" with exit code 1.

## Intent

An agent or a script can drive `ryo2d` and believe what it says: exit codes mean what they say, nothing is changed or installed without a person agreeing, a misspelt flag is an error, and what `export` ships runs. A developer reading the CLI's code finds one way to do each thing.

## Goals

1. **One argument parser for every command** (clap 4, derive), so every command rejects unknown flags and missing values with a message naming them, and `--help` is generated for each command. `clap` 4.6 is already in the lock file through another crate; take the version there.
2. **The CLI is its own crate**, `crates/ryo2d-cli` (package `ryo2d-cli`, binary `ryo2d`), depending on `ryo2d`. The engine library no longer contains `pub mod cli`, so games stop compiling the CLI and the `zip` crate. The skills embedding (`src/build.rs`'s `list_skills`) moves with it. Installing becomes `cargo install --path crates/ryo2d-cli`; every place that says how to install is updated. `sheet` and `check-ui` call the engine through its public API (`testing::skeleton_sheet`, the UI file checks); anything they need that is crate-private is made public with a doc comment, not reached by path tricks.
3. **Exit codes that mean something:**
   - 0 success; 1 the CLI failed (a missing project, an I/O error, a build that failed); 2 bad arguments (clap's own).
   - `run` and `dev` pass the game's own exit code through when the game ran and exited non-zero (a panic's 101 stays 101), and say "the game exited with code N", never "build failed".
   - `dev` tells a compile failure from a game failure by running `cargo build` first and then starting the built game itself (as `run` does), not by reading `cargo run`'s status.
   - An error is printed once.
4. **Nothing changes and nothing installs without a person agreeing:**
   - Prompts are asked only when stdin is a terminal (`std::io::IsTerminal`). With no terminal, a prompt takes the answer that changes nothing and prints what it would have done and the flag that does it.
   - End of file at a prompt is "no".
   - `dev`'s offer to add the dependency optimisation profile: no terminal → no change, a one-line hint.
   - `init`'s skills: a flag `--skills <claude|copilot|both|none>`. With a terminal and no flag it asks, and an empty answer means Claude (option 1). With no terminal and no flag it installs none and says how.
   - `netplay`'s installers (`cargo install matchbox_server`, cloudflared through brew or winget) ask first, every time, and never run without a terminal. Otherwise `netplay` is left as it is for the networking rethink.
5. **One way to find the target folder and the built game.**
   - The target folder comes from `cargo metadata --format-version 1 --no-deps` (`target_directory`), which honours `CARGO_TARGET_DIR`, `.cargo/config.toml` and workspaces. The CLI never sets `CARGO_TARGET_DIR` itself.
   - `build` finds the executable from cargo's own output (`--message-format=json-render-diagnostics`, the `compiler-artifact` message whose `executable` is set for the game's binary), not by guessing paths or comparing modification times.
   - The three copies of `find_workspace_target_dir` and the inline newest-binary search are deleted.
6. **Templates are files, and a test proves `init` makes a project that builds.**
   - `init`'s files move to `crates/ryo2d-cli/templates/` (the `main.rs`, `Cargo.toml`, `Ryo.toml`, README, `.gitignore`, `build.rs`), read with `include_str!`, with named placeholders replaced (`{{name}}`, `{{engine_dependency}}`, …). A placeholder left unreplaced is a test failure.
   - The `Cargo.toml` template loses the `your-username` git URL and the commented `rand = "0.8"`.
   - **A test runs `init` into a temporary folder with the engine as a path dependency and runs `cargo check` on it**, against the workspace's own target folder so the engine is not built twice. An API change that breaks the template fails the suite.
7. **`export` ships a game that runs.** The game's executable entry in the zip is written with mode `0o755` (the rest `0o644`), tested by reading the zip back. The date in the zip's name is the UTC date, worked out from `SystemTime` with no FFI and no new dependency; the `unsafe` block, `local_utc_offset_secs` and its structs are deleted.
8. **`build` keeps a portable build's saves**: rebuilding replaces everything in `release/` but `release/data/`.
9. **Errors carry their cause.** `CliError` becomes a `thiserror` enum (the engine already uses `thiserror` 2) whose variants hold their source errors (`io::Error`, the engine's `WorkspaceError`, cargo's failure with its status), so `source()` chains work. `Other(String)` remains only for messages with no underlying error.
10. **Help text says what is true**: asset hot reload exists; code reload does not ("coming soon" lines removed or corrected).
11. **Tests for every command.** `dev`, `run` and `netplay`'s prompt logic gain tests. An end-to-end test runs the built `ryo2d` binary (`CARGO_BIN_EXE_ryo2d`) for `--help`, an unknown flag (exit 2), and `init` → `check` above.

## Non-goals

- A macOS `.app` bundle or Windows installer (later; recorded in the follow-ups).
- A web build in the CLI.
- Reworking `netplay` beyond asking before installing (it belongs to the networking rethink).
- Code hot reload.
- Colour output.

## Anti-goals

- A prompt that hangs an agent or CI waiting on stdin.
- Anything that rewrites a user's file, installs software or deletes saves without a person saying yes.
- Shelling out to tools the user may not have, where cargo or the standard library can answer.

## Constraints

- Every command keeps its current behaviour and output style (`Output::status/success/warning/error`, Cargo-style labels, no colour) except where a goal changes it.
- `RYO_LOG`, `RYO_CAPTURE_*`, `RYO_INPUT` and `RYO_HEADLESS` still pass through to the game; the CLI still turns the engine log's stderr on first thing (`the_cli_shows_the_engine_s_warnings_in_every_build`).
- `build --dev-tools`, `dev -p`, the font licences in `release/licenses/`, `check-ui` and `sheet` behave as their notes say today; their existing tests move with them and stay green.
- The skills install test (`installing_writes_every_skill_folder_as_it_is`) keeps passing from the new crate.
- No `unwrap` outside tests; British spelling; module notes name each rule's test; the `ryo-cli` skill gets one minor bump and is updated for the flags, exit codes and install path; `src/cli/AGENTS.md` moves to the new crate and is updated.
- Breaking changes are fine (the engine is an experiment), but `ryo2d init`'s generated project must build.

## Left to the builder

- The exact clap structure (one `Parser` with a `Subcommand` enum is expected).
- How the `init` → `cargo check` test finds the workspace's target folder and stays reasonably fast; if it takes over a minute on a warm target, say so and propose `#[ignore]` with a `./dev` step instead, as a ruling.
- Where the TTY check lives (one `Prompt` helper, testable with a fake input, is expected).
