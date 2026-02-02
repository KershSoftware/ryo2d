# AGENTS.md - Documentation

This folder contains the **guide documentation** for Ryo2D, built with [mdbook](https://rust-lang.github.io/mdBook/).

## Purpose

The documentation has two parts:

| Type | Source | Purpose |
|------|--------|---------|
| **Guide Docs** | `docs/src/*.md` | Tutorials, concepts, examples, explanations |
| **API Docs** | `///` comments in Rust code | Auto-generated reference for all public types/functions |

**Guide docs** (this folder) are for:
- Getting started tutorials
- Core concept explanations
- Usage examples with context
- Architecture overviews

**API docs** are for:
- Exact function signatures
- Type definitions
- Method listings
- Auto-generated from code

## Structure

```
docs/
├── AGENTS.md          ← You are here
├── book.toml          ← mdbook configuration
├── book/              ← Generated output (git-ignored)
│   ├── index.html     ← Guide landing page
│   └── api/           ← rustdoc output copied here
└── src/
    ├── SUMMARY.md     ← Table of contents (sidebar)
    ├── introduction.md
    ├── getting-started/
    ├── concepts/
    └── examples/
```

## Building Docs

**Windows:** `.\build-docs.ps1`  
**Unix/macOS:** `./build-docs.sh`

Options:
- No args → Build and open in browser
- `--open` → Open last build (no rebuild)
- `--serve` → Build and start live reload server

## Adding New Pages

1. Create a `.md` file in the appropriate `docs/src/` subfolder
2. Add an entry to `docs/src/SUMMARY.md` (this defines the sidebar)
3. Rebuild with `.\build-docs.ps1` or `./build-docs.sh`

## Writing Guidelines

- Use practical code examples
- Link to API docs for detailed signatures: `[EngineBuilder](./api/ryo2d/engine/struct.EngineBuilder.html)`
- Keep explanations focused on "why" and "how to use"
- API docs handle the "what exactly"
