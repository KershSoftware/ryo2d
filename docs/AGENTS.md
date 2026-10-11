# AGENTS.md - Documentation

## Purpose

- **Guide docs** (`docs/src/*.md`, mdbook) - Tutorials, concepts, the "why" and "how to use"
- **API docs** (`///` rustdoc) - Exact signatures and type definitions. Don't copy signatures into guides, link to them instead

## Building Docs

**Windows:** `.\build-docs.ps1`  
**Unix/macOS:** `./build-docs.sh`

- No args → Build (mdbook + rustdoc copied to `docs/book/api`) and open in browser
- `open` → Open last build (no rebuild)
- `serve` → Build and start live reload server

## Adding New Pages

1. Create a `.md` file in the appropriate `docs/src/` subfolder
2. Add an entry to `docs/src/SUMMARY.md` (it defines the sidebar). Pages not listed there aren't built
3. Rebuild with `.\build-docs.ps1` or `./build-docs.sh`

## Writing Guidelines

- Link to API docs by relative path from the page, e.g. from `docs/src/concepts/`: `[EngineBuilder](../api/ryo2d/core/engine/struct.EngineBuilder.html)`
