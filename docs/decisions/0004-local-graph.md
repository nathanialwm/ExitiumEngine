# ADR-0004: Keep the graphify graph local, not committed

- Status: accepted
- Date: 2026-11-17

## Context

INTERVIEW.md R9 committed `graphify-out/graph.json` and `GRAPH_REPORT.md`: a pre-push hook rebuilt them and committed a `chore(graph): refresh`, and `cargo xtask push` retried the push. Every PR therefore rewrote the whole graph. With several factory PRs open at once, each merge left the others conflicting on `graphify-out/`, and the union merge driver only runs locally, so GitHub still reported the PRs as unmergeable.

## Decision

`graphify-out/` is gitignored and never committed. Each checkout builds its own graph: the `post-checkout` hook builds it when missing (new clones and worktrees), and the `pre-push` hook refreshes it without committing or blocking the push. `cargo xtask push` is a plain `git push`. The graph merge drivers are removed from `.gitattributes`.

## Alternatives considered

- Keep committing, rebuild only on `main` after merge — still one writer racing every open branch, and PR branches would carry a stale graph.
- Commit the graph from a CI bot on `main` — needs a write token in CI and still churns history for a derived file.

## Consequences

PRs no longer conflict on the graph and diffs show only real changes. The graph is a local cache: it costs about 2 seconds to build, and a checkout without graphify installed has none (agents then fall back to reading files). Clones that ran the old `install-hooks` keep stale `merge.graphify`/`merge.ours` git config, which is harmless.
