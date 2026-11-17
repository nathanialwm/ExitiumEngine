# ADR-0003: MIT license and public repository

- Status: accepted
- Date: 2026-11-17

## Context

[ADR-0002](0002-license.md) kept the engine proprietary and private, with a permissive-only dependency policy so a later move to open source needed no audit. The owner has decided to take that step now.

## Decision

The engine is licensed under the MIT license (`LICENSE`, `license = "MIT"` in the workspace `Cargo.toml`) and the GitHub repository is public. The `cargo-deny` allow list is unchanged: all of its licenses are MIT-compatible and copyleft stays banned.

## Alternatives considered

- MIT/Apache-2.0 dual license — adds a patent grant, but the owner chose plain MIT for simplicity.
- Stay proprietary — rejected by the owner.

## Consequences

Anyone can use, modify and redistribute the engine, including commercially. History, issues and PR discussion become public, so credentials must never be committed (they live in `~/.exitium-factory/`). Contributions arrive under MIT. No dependency audit is needed because the allow list was already permissive.
