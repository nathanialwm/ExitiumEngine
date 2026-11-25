# Exitium Engine — Agent Guide

Exitium is a performance-first 2D game engine in Rust. **Its primary user is an AI agent.** Humans review and refine in a full standalone editor. This repo is an **AI software factory**: agents take approved GitHub Issues to merged PRs. Background: `PLAN.md` (architecture, roadmap) and `INTERVIEW.md` (requirements, decisions, risks). Read those only when your task needs them, not every session.

## Orient cheaply

1. Query the knowledge graph before reading files: `graphify query "<question>"`. You can also use `graphify explain "<Node>"` or `graphify path "A" "B"`. Start broad with `graphify-out/GRAPH_REPORT.md`.
2. Read the target crate's own `CLAUDE.md` before editing it.
3. Read only the files you will change, plus the files they touch. Do not sweep the repo.

## Design rules (non-negotiable)

1. **Agent parity.** Every editor capability is reachable through MCP (`exitium_mcp`). If you add an editor feature, you also add the MCP tool.
2. **Reflection is the source of truth.** Inspector UI, serialization, Luau bindings and MCP schemas are all derived from `#[derive(Reflect)]`. Never hand-write a parallel schema.
3. **All mutation goes through the command bus.** Editor, MCP and Lua changes are typed, serializable commands with undo.
4. **Measure performance.** Any hot-path change ships with a benchmark. CI fails on a regression over 5%. Min-spec target: 4 cores, 8 GB RAM.
5. **Glutton first, general always.** Prioritize what the first game needs, but never take a shortcut that blocks sidescroller, isometric, pixel art, multiplayer or destruction.
6. **Readable by a basic-level Rust reviewer.** Clear module docs, documented invariants, minimal macro magic in public APIs.

## Code rules

- Stable Rust only, pinned in `rust-toolchain.toml`. Edition 2024.
- `unsafe` is denied workspace-wide. Only designated modules may use `#[allow(unsafe_code)]`, and every `unsafe` block needs a `// SAFETY:` comment. Those modules need Miri tests, and the reviewer checks every `// SAFETY:` argument.
- Prefer std or an existing workspace dependency. A new dependency must be justified in the PR body (license, wasm support, why not std). Declare versions once in `[workspace.dependencies]`.
- Game-facing API goes through the `exitium` facade crate.
- Comment density: module-level `//!` docs on every module. Comment only non-obvious logic. No narrating comments.

## Definition of done

- `cargo fmt --all` passes.
- `cargo clippy --workspace --all-targets -- -D warnings` passes.
- `cargo test --workspace` passes, and new behavior has tests.
- `cargo build --workspace --target wasm32-unknown-unknown` passes for wasm-capable crates.
- No benchmark regression over 5%.
- Docs and MCP tool schemas are updated when behavior changed.

`cargo xtask ci all` runs the fmt, clippy, test and WASM checks exactly as CI does.

## Human escalation

The owner is involved only when a PR fails **three AI review rounds** (ADR-0005, ADR-0008). Every other PR merges once CI and the AI review pass. When the third round fails, the orchestrator labels the PR `needs-human`, comments a summary that @-mentions the owner, and stops work on it. The label is sticky: removing it does not lift the gate, and only the owner's `human-approved` label does. Never apply `human-approved` yourself.

High-risk changes still get extra reviewer scrutiny, not a human gate: new dependencies, `unsafe`, the `exitium` facade API, save and serialization formats, licensing, secrets, CI and factory config, and release workflows.

## Workflow

- **Branch:** `<type>/<issue#>-<slug>` (e.g. `feat/42-archetype-storage`). One issue → one branch → one worktree → one PR.
- **Push:** `cargo xtask push`. Run `cargo xtask install-hooks` once per clone.
- **Commits and PR titles:** Conventional Commits (`feat(ecs): ...`, `fix(render): ...`, `chore: ...`). PRs are squash-merged.
- **PR body:** `Closes #N`, a summary, what changed, how it was verified, high-risk areas touched (the Risk section).
- **Specs:** systems-level work needs a spec in `docs/specs/` or an ADR in `docs/decisions/`, approved before implementation. Small tasks don't.
- **Follow-up work:** file it as an issue with the `status:needs-approval` label. Never expand the current PR's scope.

## GitHub identity

Agents act as the **exitium-factory** GitHub App, never as the owner. For API actions (issues, PRs, labels, reviews, statuses), run gh through the wrapper:

```bash
scripts/factory/gh-app.sh <gh args...>
```

The wrapper fetches a fresh App token and sets it only in gh's own environment (gh hands it on to processes it starts, such as git). It refuses `gh auth ...`, because `auth status` and `auth token` print the token. If no token is available, it exits 1 without running gh, so a failed fetch can never fall back to the owner's stored login. Never fetch the token yourself or set `GH_TOKEN` by hand: `GH_TOKEN="$(scripts/factory/gh-app-token.sh)" gh ...` runs gh as the owner when the fetch fails.

Credentials live in `~/.exitium-factory/`. Never print, copy or commit them or the token, and never write the token to a file or gh config. Exception: the GitHub Projects roadmap board belongs to the owner's account, and Apps can't reach it, so use plain `gh` for `gh project` commands only.

## Factory operating rules

- At most **8** issues in progress at once (`status:in-progress`). An issue starts only when the orchestrator is certain it cannot conflict with any in-progress issue: disjoint footprints, no shared hotspot, no crate dependency between them now or after both land (`factory-run` step 3). When in doubt, it waits.
- Every issue declares a `## Footprint` (the paths it will edit). Implementers stay inside it, and the reviewer rejects a diff that leaves it.
- **Do not start a new issue when less than 5% of session usage remains.** Finish or hand off in-flight work.
- Agent-filed issues start as `status:needs-approval`. Only the `issue-approver` agent moves them to `status:approved`.
- Roles and models are defined in `.claude/agents/`. Skills are in `.claude/skills/`.
- Changes to factory rules, skills or agents are ordinary PRs that go through the same AI review as code. The factory App cannot push `.github/workflows/` changes, so the owner pushes those.

## graphify

This project has a knowledge graph at graphify-out/ with god nodes, community structure, and cross-file relationships.

Rules:
- When graphify-out/graph.json exists, run `graphify query "<question>"` before your first grep, search or source-file read for a codebase question, and put this rule in every subagent prompt that explores code. Read raw files only for lines the graph or an earlier read already located. Use `graphify path "<A>" "<B>"` for relationships and `graphify explain "<concept>"` for focused concepts. When the answer is under a Markdown heading, `graphify section "<heading>"` prints that section's text, so the .md need not be opened. These return a scoped subgraph, usually much smaller than GRAPH_REPORT.md or raw grep output.
- If graphify-out/wiki/index.md exists, use it for broad navigation instead of raw source browsing.
- Read graphify-out/GRAPH_REPORT.md only for broad architecture review or when query/path/explain do not surface enough context.
- After modifying code, run `graphify update .` to keep the graph current (AST-only, no API cost).
- graphify-out/ is local and gitignored (ADR-0004). Never commit it. The git hooks build it on checkout and refresh it before each push. If it is missing, run `graphify update .`.
