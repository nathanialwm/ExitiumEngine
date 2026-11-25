# ADR-0007: Luau on wasm, the route chosen

- Status: accepted
- Date: 2026-11-25
- Issue: #145

## Context

Risk R5 in [mvp-roadmap.md](../specs/mvp-roadmap.md): Luau is C++, and `wasm32-unknown-unknown` has no C or C++ standard library, so `mlua` with a vendored Luau might not build for the web. Owner decision 3 already fixed the fallback: if no route works, scripts are native-only and `exitium_script` builds for wasm as a cfg-gated stub.

The R5 spike ([luau-wasm-spike.md](../specs/luau-wasm-spike.md), #136) is the evidence. This ADR records the route it chose and turns its conditions into rules that later work must follow. It does not re-run the spike. In short, the spike found:

- `mlua` 0.12.3 with the `luau` feature builds for `wasm32-unknown-unknown` on stable Rust, on Windows and on Ubuntu 24.04, when C and C++ are built by wasi-sdk 34.
- In Chrome 158, mlua's own tests passed. Luau adds about 0.46 MB gzipped to the web build (budget gap: 3 MB). Every speed ratio is under the 2.5x budget (one Lua to Rust call: about 69 ns against 43 ns native).
- The cost is toolchain, not size. The Linux CI image needs the SDK, the loader needs a shim, and the module needs WebAssembly exception handling with `exnref`, which moves the web floor from Chrome 113 to Chrome or Edge 137.
- The 10,000-entity `update` met its 4 ms budget at the median (2.2 to 2.8 ms) and exceeded it at p99 (5.1 ms) in a live loop in headless Chrome. That is not settled until it is measured in the real engine.

The owner's roadmap says AD2 records the outcome either way and nothing waits for the owner (ADR-0005 applies as usual: this ADR merges after the AI review).

## Decision

**Luau runs on the web through route A: `mlua` with the `luau` feature, with C and C++ built by wasi-sdk, as the real VM.** Owner decision 3's native-only fallback is not needed. It stays as a verified SDK-free build (the stub) and as the fallback if a tripwire below fires.

### Pins

| Item | Pin |
|---|---|
| `mlua` | `=0.12.3`, features `["luau"]` (not `vendored`: `luau` always vendors). `mlua-sys` 0.14.0, `luau0-src` 0.24.1+luau748 (Luau 0.748). |
| wasi-sdk | 34.0 (wasi-libc `2e6fb9d8ee0c`, llvm-project `895aa2c896ad`, clang 23.1.0; libc++, libc++abi, libunwind). Windows `wasi-sdk-34.0-x86_64-windows.tar.gz`, SHA-256 `cccb5c323a9b34f0349a9b09e8804a0a7632c68c3310f4b5f437ed57d7e71d8f`. Linux `wasi-sdk-34.0-x86_64-linux.tar.gz` (192 MB), SHA-256 `b761e3a0721dbae9c09a0059e5fdb2bf917d1b4a8a7b430fb3b5aafb0984b2c4`. |
| `wasm-bindgen` | `0.2.130` (the version `assets-scene-export.md` locks) |
| `wasm-opt` (if used) | binaryen `version_133`, explicit feature list, never `--all-features` (Chrome 158 rejects its compact imports) |
| Exceptions | WebAssembly exceptions with `exnref` (`-fwasm-exceptions -mllvm -wasm-use-legacy-eh=false`), WASI preview 1 shim in JS (eight imports) |

The Windows hash is the one the spike computed on its own copy. The Linux hash is the digest GitHub shows for the release asset, taken for this ADR. The provisioning code checks both, and fails on a mismatch.

### Rules

1. **Feature gate (whole workspace).** The wasm Luau build is behind a Cargo feature `luau` on `exitium_script`, off by default, so `cargo build --workspace --target wasm32-unknown-unknown` builds the SDK-free stub. Resolver 3 unifies features across every selected member, so three rules keep one manifest from turning it on for everyone:
   1. Native always builds Luau. `exitium_script` declares `mlua` under `[target.'cfg(not(target_arch = "wasm32"))'.dependencies]`.
   2. The `luau` feature only has an effect on `wasm32`. It adds the optional `mlua` dependency, the allocator and the link setup there. On native it changes nothing.
   3. **No manifest enables `luau`.** That covers `exitium_script`'s own `default`, every `examples/*` crate and the `exitium` facade. The facade and a game crate may **declare** a forwarding feature (`luau = ["exitium_script/luau"]`) that nothing enables by default, because a command-line `--features` needs something to receive it. The web export and the SDK-gated CI step pass it on the command line.
2. **`mlua` entries.** With the optional entry in `[dependencies]` and the target-scoped one, `features = ["luau"]` goes on **both**. The feature reaches wasm through the entry that wasm sees, which is the optional one.
3. **CI.**
   - `cargo xtask ci wasm` gains a step that builds `exitium_script` with the feature on, by adding `("exitium_script", "luau")` to `FEATURE_CHECKS` in `xtask/src/ci.rs`. The same entry makes clippy and test run the feature natively, where it is a no-op and needs no SDK.
   - The wasm half of that entry needs the SDK. **In CI it is a hard failure when the SDK is missing** (`CI` is set), so a broken Docker layer cannot pass silently. Locally it runs only when `WASI_SDK_PATH` is set or the SDK is provisioned, and prints a notice when skipped, so a plain `cargo xtask ci all` needs no SDK in an agent worktree.
   - The step runs only in the Linux Docker job (`ci.yml` `linux`). The Windows runner job stays clippy and test only, because it is the owner's host with no SDK.
   - `xtask` provisions the SDK from the pins above into a checksum-verified location. It sets the target-scoped compiler variables (`CC_`, `CXX_`, `AR_`, `CFLAGS_`, `CXXFLAGS_`, `CXXSTDLIB_wasm32_unknown_unknown`) for its child `cargo` only. No `RUSTFLAGS` and no global `LUAU_CXXFLAGS`.
   - The Linux Dockerfile (`ci/runner/linux/Dockerfile`) gains the SDK layer.
4. **Build script.** `exitium_script/build.rs` reads the target from `CARGO_CFG_TARGET_ARCH` and `CARGO_CFG_TARGET_OS` (not `cfg!`, which is the host in a build script), does nothing unless the target is wasm and `CARGO_FEATURE_LUAU` is set, and then links `c++`, `c++abi`, `unwind`, `wasi-emulated-process-clocks` and `c` from the SDK's `eh` and base lib directories, and compiles a 3-line `reactor.c` that exports `_initialize`. It needs `cc` as a build-dependency.
5. **Init and loader.**
   - `_initialize` runs exactly once and first, from the `#[wasm_bindgen(start)]` function. A Rust once-guard protects it and no Rust allocation happens before it. A second call hangs `init()` (observed). Allocation before it happened to work (observed) and must not be relied on.
   - The loader (generated by `exitium export`, and `exitium_platform`'s example loader) must capture `instance.exports.memory` as soon as the instance exists, give it to the WASI shim, and carry an import map for the bare specifier `wasi_snapshot_preview1`.
   - Without a reactor entry point every export after the first hangs, and without `_initialize` Luau traps in `newclasspage`. These are the spike's gaps 1 and 2.
6. **Allocator.**
   - Rust and wasi-libc must share one allocator, so under `luau` on wasm the module's `#[global_allocator]` forwards to wasi-libc's `posix_memalign`, `realloc` and `free`. It must be **gated on the same feature**: the spike observed the stub failing to link (undefined `posix_memalign`) when the allocator was on for every wasm32 build.
   - **Owner.** The `exitium` facade declares the single `#[global_allocator]`, under `cfg(all(target_arch = "wasm32", feature = "luau"))` only. It is in every game's graph exactly once, so a game cannot forget it, and no library crate may declare one. On native, and on wasm without `luau`, the engine declares no allocator: #64's counting allocator stays opt-in and set by the game or benchmark binary.
   - **Unsafe.** The forwarding type lives in a designated `#[allow(unsafe_code)]` module of `exitium_script` (it owns the feature and the link setup), with a `// SAFETY:` comment on each block. The facade's static is safe code. Miri cannot run the wasi-libc FFI, so the wrapper's logic is Miri-tested over an inner `System` allocator, and the `posix_memalign` and `free` forwarding is checked by the Chrome run in CI.
   - **Forwarding.** It forwards `alloc_zeroed` and `realloc` (for alignments up to the malloc alignment) too, because the spike's `alloc`/`dealloc`-only version made `Vec` growth pessimistic. The allocation-heavy benches are re-run after that change.
   - **Counting.** #64's counter must wrap any inner allocator (`CountingAllocator<A>`), so the facade can compose `Counting<Wasi>` on wasm and `Counting<System>` on native. See the follow-ups.
7. **Export contract.**
   - Scripts **off**: `exitium export --target web` builds the SDK-free stub. The page has no WASI shim, import map or `exnref` probe, and its floor stays Chrome or Edge 113.
   - Scripts **on**: `exitium export` and `exitium export --check` fail when the SDK is absent, print the fix command, and never fall back to the stub (the scripts would silently do nothing). The page carries the shim, the import map, the memory capture, the start function and the `exnref` probe.
   - A failing `wasm-opt` falls back to the unoptimised module with a warning. It never breaks the export.
8. **Browser floor: Chrome or Edge 137 for a game with scripts. Decided.** `exnref` ships in Chrome and Edge 137, so Chrome and Edge 113 to 136 (WebGPU but no `exnref`) are out for script games. The legacy exception ABI is **not** built: it would double the SDK libraries and add a runtime switch. The page tests the feature, not a version: a `WebAssembly.validate` on a 28-byte module with one `try_table` (opcode `0x1f`), and shows "This game needs a browser with WebAssembly exception handling (Chrome or Edge 137 or later)". The assets spec ([assets-scene-export.md](../specs/assets-scene-export.md), PR #166) keeps 113 as the base web floor and adds exactly this conditional 137 for script games, so this ADR needs no change to it. The probe's negative case (a browser without `exnref`) was not run in the spike, so the export PR must run it. If a later example's players need older Chrome, a new ADR supersedes this rule.
9. **Examples.** No example's scripted part is native-only. `topdown`'s Luau gameplay scripts run on the web, behind the 137 floor. Each example's rules still run in Rust (owner decision 3), so this is a choice and not a dependency: the stub arm stays available.
10. **Budgets.** The roadmap's budgets stay as written and SP9 may only tighten them: web wasm at most 13 MB gzipped with Luau (spike: +0.38 to 0.48 MB), Luau on wasm at most 2.5x native, a reflected field access at most 200 ns, 10,000 scripted entities at most 4 ms per frame, hot reload of 500 lines at most 50 ms (not measured). SP9's benches run in the real engine on the live loop, report p99, repeat the nine-run method on a quiet machine, and use the Windows "High performance" plan (the spike's E8 numbers ran under "Power saver").

### Licences (re-verified for this ADR)

The spike marked its licence table unverified until AD2. This is the result of reading each upstream licence file at the pinned version, on 2026-11-25. The wasi-sdk 34.0 Windows tarball ships no LICENSE, COPYING or NOTICE file, so the notices must come from upstream at these pins.

| Component (pinned at) | Licence, as read | Result |
|---|---|---|
| wasi-sdk repo (`wasi-sdk-34`) | `LICENSE`: Apache-2.0 with the LLVM exceptions section | permissive |
| wasi-libc (`2e6fb9d8ee0c`) | `LICENSE`: multi-licensed Apache-2.0 WITH LLVM-exception, Apache-2.0 and MIT. Third-party parts: `dlmalloc` CC0, `emmalloc` MIT, `cloudlibc` BSD-2-Clause, `musl` MIT, `musl-fts` BSD-3-Clause | permissive |
| musl code inside wasi-libc | `COPYRIGHT`: MIT, with bundled TRE regex (BSD-2-Clause), Sun and Arm math code and an Android memcpy (BSD-style), `crypt` code (BSD), and an MIT-style smoothsort | permissive; notices must be kept |
| libc++, libc++abi, libunwind, compiler-rt (llvm-project `895aa2c896ad`) | `LICENSE.TXT`: Apache-2.0 with LLVM exceptions. **`libcxx` also carries the "legacy" dual University of Illinois/NCSA and MIT licence section** | permissive; the exception covers embedded object form, and the legacy section needs its notice |
| Luau 0.748 | `LICENSE.txt`: MIT, Copyright Roblox Corporation and Lua.org, PUC-Rio | permissive |
| `luau0-src` 0.24.1+luau748 | crates.io licence `MIT`; its `LICENSE` adds the crate author's copyright to the Luau and Lua.org lines. The packaged `luau/` tree has no other third-party notice (the only non-Roblox copyright line is Lua.org's) | permissive |
| `mlua` 0.12.3, `mlua-sys` 0.14.0 | MIT (crates.io; `mlua`'s `LICENSE` read at `v0.12.3`) | permissive |

The check found three things the spike's table did not name: wasi-libc's `musl-fts` (BSD-3-Clause) and `emmalloc` (MIT) parts, and libc++'s legacy NCSA/MIT section. All are permissive and notice-retaining, so ADR-0003 holds: nothing here is copyleft, `deny.toml` needs no entry and no ADR-0006-style exception. `cargo deny` sees `mlua`, `mlua-sys`, `luau0-src` and `cc`, but not the SDK, so the check is not repeatable by tool. Not checked: which parts of wasi-libc actually link into the final module. The notice therefore lists every component above, and does not try to prune. This is a reading of licence files, not legal advice.

**Condition.** A game exported with scripts on the web ships a notice covering these components, with the text taken from the upstream repositories at the pins. This is the same notice file that ADR-0006 condition 4 requires for `triple_buffer`. It must exist before the first public export, and the follow-up issue below covers both.

### Tripwires

If any of these fires in SP9 or later, the fix is a new ADR that supersedes this one and flips the wasm arm to the stub (owner decision 3). The stub is kept and built by default for exactly this reason.

1. The full engine module (with `wgpu` and `winit`) fails to instantiate in Chrome 137 or later with Luau on, or does not fit the 13 MB gzipped budget. The spike tested only small hosts.
2. The shared allocator or the init order cannot be made safe in the real `main` plus `spawn_app` path (the spike tested a `requestAnimationFrame` shape).
3. The 10,000-entity `update` misses 4 ms at the median in the real engine on the live loop, after the `realloc` and `alloc_zeroed` forwarding and a quiet-machine re-run. A p99 above 4 ms alone is a finding that SP9 reports and fixes (or proposes a tighter script budget for), not a fallback.
4. The Linux SDK layer cannot be made reliable in CI.

## Alternatives considered

- **Route B: a separate Luau wasm module bridged from JS.** It works (E6): 583 KB gzipped, 28 to 33 ns per host call. But it has two linear memories, so every entity, component and string crosses by copy. Reflection-derived bindings, userdata handles and the command bus would need a second binding layer on the web next to mlua on Windows. It needs the same SDK and `exnref`, so it saves no toolchain cost. Kept as plan B if route A breaks, before the native-only fallback.
- **Native-only scripts with a cfg-gated stub (owner decision 3).** Cheaper in toolchain, but web examples could not run any Luau, and the spike shows the cost is acceptable. Kept as the fallback and as the default SDK-free build.
- **`wasm32-unknown-emscripten`.** Replaces the whole web toolchain (`wasm-bindgen`, `winit`, `wgpu`) for scripting's sake. Not tried.
- **A pure-Rust Lua VM.** The ones the spike knew of target Lua 5.x, not Luau: no types, vectors or sandbox, and it changes the scripting contract. Not tried.
- **Build the legacy exception ABI too (Chrome 113 to 136).** A second set of SDK libraries and a runtime switch, for browsers outside the audience. Rejected for the MVP (rule 8).
- **`RUSTFLAGS` or a global `LUAU_CXXFLAGS` instead of `build.rs` and target-scoped variables.** `RUSTFLAGS` applies to every crate and breaks crates compiled before the libraries are found, and an unscoped C++ flag set would feed wasm flags to native Luau builds.
- **Make `luau` a default feature, or let the facade enable it.** Every `--workspace` build and every agent worktree would then need the SDK (rule 1).
- **Let `exitium_script` declare the global allocator.** A library may not, since a final binary has one and #64's counter is a wrapper too. A game could also forget it (rule 6).

## Consequences

- **Easier.** Scripts run on the web with one VM and one binding layer. The workspace's own definition of done is unchanged for agents: the SDK-free stub keeps `cargo build --workspace --target wasm32-unknown-unknown` working.
- **Harder.** The Linux CI image needs a 192 MB SDK layer. `cargo xtask ci all` is no longer exactly what CI runs when the SDK is absent locally, so CLAUDE.md's definition of done gains one line. Script games on the web need Chrome or Edge 137. The web build links wasi-libc's allocator for all Rust code when `luau` is on, a new risk the existing wasm benches must show is within 5%.
- **High-risk areas touched downstream** (extra reviewer scrutiny, ADR-0005): the new `cc` and `mlua` dependencies, the `unsafe` forwarding allocator, CI and runner config, the `exitium` facade, and the licence notice.
- **Spike open questions, closed.** Q1 legacy ABI: not built, floor 137 (rule 8). Q2 allocator owner: the facade, gated (rule 6). Q3 allocator behaviour: SP9 benchmarks it (rule 10, tripwire 3). Q4 `wasm-opt` in the export step: this ADR fixes only the failure mode and the flag list (rule 7); whether `exitium export` runs it, and the pinned binaryen, are decided in the export spec (#155).

### Downstream issues

| Issue | Effect |
|---|---|
| #150 SP9 (`luau-scripting.md`) | **Unblocked.** Its wasm arm is the real VM, selected by the `luau` feature, and the stub is the default. The "real VM or stub" acceptance line becomes "both arms, selected by `luau`". Its spec carries rules 1 to 6 and 10, lists the reflected bindings, and owns the allocator module and the benches. |
| #157 intake 2 | **Unblocked on AD2.** Adds `mlua = "=0.12.3"` with `features = ["luau"]` and `cc` (build-dependency) to `[workspace.dependencies]`, the `exitium_script` edges, and the facade's declared, never-enabled `luau` forwarding feature. It records `cargo deny check`. It adds no `deny.toml` entry. `cc`'s justification goes in the PR body per CLAUDE.md. |
| #140 C4 (`exitium export --target web`, in `assets-scene-export.md`) | **Unblocked.** The question "does the page need an `exnref` test" is answered yes, for script games only. Its acceptance gains the rest of rule 7 (the loader contract, the `--check` SDK row, the `wasm-opt` failure mode), through the amendment listed below. |
| #155 SP18 (`export-examples.md`) | Cites rules 7 to 9. `topdown`'s Luau runs on the web; no scripted part is native-only. Decides whether `exitium export` runs `wasm-opt`. |
| #64 counting allocator | Its criterion "wrapping `std::alloc::System`" changes to a wrapper generic over the inner allocator. |
| CLAUDE.md definition of done | One line about `ci all` with and without the SDK (follow-up below). |

### Follow-up issues to file

File each with `status:needs-approval` (file-issue skill). None is filed by this ADR.

1. `chore(ci): wasi-sdk provisioning and the SDK-gated Luau wasm check`. Footprint: `xtask/`, `ci/runner/linux/Dockerfile`. The pinned download and checksum check, the target-scoped variables, the `("exitium_script", "luau")` entry, the skip-with-notice locally and hard-failure-when-`CI` rule. Blocked by intake 2 (#157) and #150. High-risk: CI config. If a `.github/workflows/` change is needed, the owner pushes it.
2. `docs: definition of done and the SDK` (CLAUDE.md). One line: `cargo xtask ci all` skips the Luau wasm step without the SDK, and CI runs it. Lands with issue 1.
3. `docs(spec): export loader contract for Luau` (amends `assets-scene-export.md` C4 and the `--check` preflight with rule 7, and drops "AD2 may instead choose a route that needs no `exnref`"). Blocked by #166 merging. Alternatively fold it into #155.
4. `feat(cli): third-party licence notice for exported games`. Covers `triple_buffer` (ADR-0006 condition 4) and the Luau toolchain components above. Needed before the first public export. The owner chooses the wording.
5. `feat(exitium): the global allocator under luau on wasm` (facade, plus the designated unsafe module in `exitium_script`, Miri tests of the wrapper). Part of the SP9 lane. Blocked by #64's generic wrapper.
6. `feat(script): the real-engine Luau wasm bench`. The 10,000-entity `update` on the live loop with p99, the table and string workloads, the reflected-field cost. Part of the SP9 lane, High performance power plan.
7. `docs: amend #64 to a generic counting allocator`. A comment or edit on #64, not a new issue, if the factory allows it.

### Open questions for the owner

None block this ADR. Rule 8 (floor 137, no legacy ABI) and rule 9 (all example scripts run on the web) are decided here and the owner may object.
