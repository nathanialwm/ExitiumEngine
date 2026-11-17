# ADR-0001: Architecture overview

- Status: accepted
- Date: 2026-11-16

## Context

Requirements are recorded in `INTERVIEW.md`: a performance-first 2D engine, AI agents as the primary users, a standalone editor, and Glutton (an incremental roguelite) as the first consumer.

## Decision

- **Workspace layout:** a Cargo workspace of `exitium_*` crates, as laid out in `PLAN.md` § Target architecture. Games depend on the `exitium` facade crate.
- **ECS:** a custom archetype SoA ECS (`exitium_ecs`) with a parallel scheduler from day one.
- **Rendering:** wgpu (`exitium_render`). Editor UI uses egui. Game UI is a custom retained tree with an immediate-mode layer (`exitium_ui`).
- **Reflection:** one `Reflect` derive drives inspector, serialization, Luau, MCP schemas and save migration.
- **Command bus:** every mutation from editor, MCP or Lua is a typed, serializable command.
- **Numbers and scripting:** `Num` is a newtype over f64. Determinism is not required. Scripting is Luau via mlua.
- **Serialization:** a dual format. Human-editable text for authoring, compiled to a custom binary for shipping.

## Consequences

The reflection and command-bus crates become early, high-leverage dependencies, so their APIs need care and dedicated ADRs before implementation.
