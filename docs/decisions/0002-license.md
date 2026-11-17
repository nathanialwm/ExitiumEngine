# ADR-0002: License

- Status: superseded by ADR-0003
- Date: 2026-11-16

## Context

The engine is private and single-owner for now, but may go public later (`INTERVIEW.md` Q4).

## Decision

Proprietary, all rights reserved (`LICENSE`, `license = "LicenseRef-Proprietary"`). Third-party dependencies must use licenses compatible with a later move to MIT/Apache-2.0 dual licensing (enforced via `cargo-deny` once CI lands). Copyleft dependencies (GPL/AGPL) are not allowed.

## Consequences

Relicensing later only requires the owner's decision, with no dependency audit backlog.
