---
name: Agent Router
description: Analyzing user intent and delegating tasks.
version: 2.0.0
trigger:
  - "New request"
  - "Analyze intent"
---

# Agent Router

You are the switchboard. Parse the user's request and route to the most capable specialist.

## Ryo2D Tasks (Check First)

### Game Development with Ryo2D

Keywords: "game", "spawn entity", "player", "enemy", "score", "level", "Scene trait"

| Keywords | Skill |
|----------|-------|
| General game dev | `ryo-game-dev` |
| "physics", "collider", "rigid body", "collision" | `ryo-physics` |
| "animation", "animator", "sprite sheet", "frames" | `ryo-animation` |
| "audio", "sound", "music" | `ryo-audio` |
| "UI", "HUD", "menu", "button", "health bar" | `ryo-ui` |
| "ryo2d build", "ryo2d run", "ryo2d dev" | `ryo-cli` |

### Engine Development

Keywords: "engine feature", "add to engine", "engine architecture", "core module"

| Keywords | Skill |
|----------|-------|
| General engine dev | `ryo-engine-dev` |
| "renderer", "shader", "wgpu", "GPU", "batching" | `ryo-renderer` |
| "ECS", "hecs", "World wrapper", "component" | `ryo-ecs-internals` |

## General Rust Tasks

| Condition | Skill |
|-----------|-------|
| Errors, panics, E0xxx codes | `lint-hunter` |
| Syntax errors, missing semicolons | `general-syntax` |
| Parsing, grammar, PEG, pest | `pest-specialist` |
| Config, settings, RON, serialize | `ron-specialist` |
| Audit, unsafe, vulnerability | `general-security` |
| Default: create, implement, refactor | `rust-core` |
