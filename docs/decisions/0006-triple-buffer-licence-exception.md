# ADR-0006: `triple_buffer` licence exception (MPL-2.0)

- Status: accepted
- Date: 2026-11-24
- Issue: #137

## Context

[ADR-0003](0003-mit-license.md) licenses the engine under MIT and keeps the `cargo-deny` allow list permissive: "copyleft stays banned". `deny.toml` enforces that on every PR.

The audio design in [assets-scene-export.md](../specs/assets-scene-export.md) ("Licence decision", "Dependencies") uses `kira` 0.12.6 with `default-features = false, features = ["cpal"]` behind `exitium_audio`. `kira` depends on `triple_buffer` 9.0.1, which is licensed **MPL-2.0**. The dependency is not optional: it is in the tree for every `kira` feature set. `cargo deny check` would fail on it today.

A `cargo metadata` check of the full scratch tree against the `deny.toml` allow list found exactly this one non-permissive licence. The other MPL-2.0 crates a kira setup can bring, `symphonia` and its codecs, are avoided by the feature choice: `kira`'s default features pull in `symphonia`, so the spec turns them off and decodes with `hound` and `lewton` in `exitium_assets`.

MPL-2.0 is a file-level weak copyleft. Linking an MPL-2.0 crate into an MIT program is permitted. The obligations attach to the MPL-covered source files, not to the rest of the program: keep those files available to recipients, say how to get them, and publish any change made to them. The engine does not modify the crate, and it is published on crates.io.

The owner decided this in the spec's "Licence decision". This ADR is the durable record. SP4 (#140) moved it out of intake 1 (#156) into its own PR, so that intake 1 keeps only the `deny.toml` entry.

## Decision

Allow a narrow exception for **`triple_buffer` only**: licence MPL-2.0, **unmodified**, **used through `kira`**. This amends ADR-0003's "copyleft stays banned" for this one crate and for nothing else. The allow list does not change and every other copyleft licence stays banned.

The exception holds under these conditions:

1. **Unmodified.** The engine never patches, vendors or forks `triple_buffer`. It resolves from crates.io (`unknown-git` and `unknown-registry` stay `deny`). Changing the crate would trigger the MPL-2.0 duty to publish those changes, so a change needs a new decision.
2. **Reached only through `kira`.** No engine crate names `triple_buffer` in its own `Cargo.toml`. `exitium_audio`'s public API exposes no `kira` type (and so no `triple_buffer` type), which keeps the dependency replaceable.
3. **One crate, one licence.** A second MPL-2.0 dependency, or any other copyleft dependency, needs its own decision (a new ADR and its own exception entry). Turning on `symphonia` is such a case. The exception does not generalise to "MPL-2.0 is allowed".
4. **Shipped games carry a licence notice.** A game exported by the engine includes the MPL-2.0 notice for `triple_buffer`, with a pointer to its source (its crates.io page and repository), alongside the MIT and Apache notices of the other dependencies. The spec does not generate this file; its open question 2 recommends a follow-up issue before the first public export, with the owner choosing the wording. This ADR only requires that the notice exists before a public export.

Enforcement is in `deny.toml`. Intake 1 (#156) adds exactly this entry and nothing wider, and records the result of `cargo deny check`:

```toml
[[licenses.exceptions]]
crate = "triple_buffer"
allow = ["MPL-2.0"]
```

The entry allows no other crate and no other licence, so a different crate under MPL-2.0 still fails the check. This ADR touches neither `deny.toml` nor any spec file.

## Alternatives considered

- **Drop `kira` and write a small mixer on `cpal`** (Apache-2.0, about 400 lines). Rejected: the engine would own and maintain mixing, resampling, tracks, tweens and a lock-free command queue for no user-visible gain. It stays possible later because `exitium_audio`'s API exposes no `kira` type. If the exception ever becomes a burden, replacing `kira` removes it.
- **Ban MPL-2.0 without exception and skip audio.** Rejected: audio is in the full MVP scope (`docs/specs/mvp-roadmap.md`).
- **Allow MPL-2.0 globally in the `deny.toml` allow list.** Rejected: it would admit every future MPL-2.0 crate silently, against ADR-0003. A per-crate exception keeps each new case a deliberate decision.

## Consequences

- Easier: `kira` is usable with `cargo deny check` green, and the exception is visible in one place (`deny.toml`) and one document (this ADR).
- Harder: the engine carries one weak-copyleft dependency, and reviewers must reject any change that patches `triple_buffer`, depends on it directly, or adds another MPL-2.0 crate without an ADR. A `kira` upgrade that changes `triple_buffer`'s licence or version needs the exception re-checked, since the entry names the crate, not a version.
- Follow-up: a licence-notice generator for exported games (`cargo metadata` or `cargo about`), filed as its own issue before the first public export (spec open question 2).
- ADR-0003 keeps its text. Read it together with this ADR: its copyleft ban has this single exception.
