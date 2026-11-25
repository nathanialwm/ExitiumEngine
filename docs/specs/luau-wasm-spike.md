# Spec: Luau on wasm, the R5 spike report

- Issue: #136 (SP1 in `docs/specs/mvp-roadmap.md`)
- Crates: none (the spike's code is throwaway and stays outside the repo; Appendix A holds the sources). It informs `exitium_script` (SP9) and the ADR AD2 (#145).
- Status: draft. A report with measurements, not a design. It ends with a pass or fail (see "Final verdict").

## Goal

Answer risk R5: Luau is C++, and `wasm32-unknown-unknown` has no C or C++ standard library, so `mlua` with a vendored Luau might not build for the web. The roadmap asks three questions (Q1 to Q3 in "Design"). AD2 (#145) turns this report into the recorded route.

## Non-goals

- No code lands in the repo. The experiments live in throwaway crates outside it. Appendix A has their sources and the exact commands, so a reviewer can rebuild them.
- No engine integration. The spike used small hosts, so `wgpu`, `winit` and the real ECS were not in the build (see "Limits of this spike").
- No decision. AD2 records it. This report gives AD2 the evidence, a recommendation and the conditions.

## Verdict

**PASS, with conditions. AD2 is a go for the real Luau VM on the web.** Route: `mlua` 0.12.3 with the `luau` feature on `wasm32-unknown-unknown`, with C and C++ built by **wasi-sdk 34** (clang, wasi-libc, libc++), WebAssembly exceptions, and a WASI preview 1 shim in JS. The fallback in owner decision 3 (native-only scripts, `exitium_script` builds a cfg-gated stub on wasm) is **not needed**. It stays as insurance for the conditions in "Conditions for AD2", and the spike showed the stub can be a real SDK-free build (E8).

Evidence in six lines:

1. A documented recipe exists (`mlua` 0.12.3 `docs/wasm32.md`) and built on stable Rust 1.100.0, on Windows and in an Ubuntu 24.04 container.
2. In Chrome 158 a hello-world script ran, and mlua's own tests (eval, errors, callbacks, coroutines) passed.
3. Size: 471 KB gzip for the full E8 module through `wasm-bindgen` (401 KB after `wasm-opt -Os`), against 11 KB for the SDK-free stub. That is about 0.46 MB of the 13 MB web budget.
4. One Lua to Rust call costs about 60 to 70 ns in Chrome against 37 to 43 ns native: 1.6x at the median, 2.3x at the worst run. Pure Luau compute (`fib(30)`) is 1.1x, allocation-heavy scripts are 0.9x to 1.25x, and 10,000 scripted entities take 2.2 ms per frame at the median against the 4 ms budget. The 2.5x budget holds; the 4 ms budget holds at the median and is exceeded at p99 in a live loop (see "Q3").
5. The cost is toolchain, not size: the Linux CI image needs wasi-sdk (192 MB), the web build needs a loader shim, and three recipe gaps had to be closed.
6. **Browser floor: Chrome or Edge 137.** The module needs WebAssembly exception handling with `exnref`. Chrome 113 to 136 have WebGPU but not `exnref`, so the web build's floor moves from 113 to 137 (see "Browser floor").

## Design

### Environment

| Item | Value |
|---|---|
| Host | Windows 11, AMD Ryzen 7 7800X3D (8 cores, 16 threads), x86_64. All browser and native numbers. Ubuntu 24.04 in Docker (build check only). |
| Power plan | The E8 runs of 2026-11-25 ran under the Windows "Power saver" plan. The plan of the earlier E2 to E5 runs was not recorded. Wasm and native ran on the same host in the same session, so the ratios stand, but absolute times are probably pessimistic. Re-run on "High performance" before SP9 sets thresholds. |
| Rust | 1.100.0 stable (`rust-toolchain.toml`), target `wasm32-unknown-unknown` |
| `mlua` / `mlua-sys` / `luau0-src` | 0.12.3 / 0.14.0 / 0.24.1+luau748 (Luau 0.748, `_VERSION` reports it) |
| C and C++ toolchain | wasi-sdk 34.0 (`VERSION`: wasi-libc `2e6fb9d8ee0c`, llvm `895aa2c896ad`, clang 23.1.0), libc++, libc++abi, libunwind. Windows tarball SHA-256 `cccb5c32...ed71d8f` (full hash in Appendix A). |
| `wasm-bindgen` and CLI | 0.2.130 (the version `assets-scene-export.md` locks) |
| `wasm-opt` | binaryen version_133 |
| Browser | Chrome 158.0.8127.37, headless, driven by `puppeteer-core` 25.18.1 (no extra Chrome flags) |

**Experiments.** The report refers to these by label. Sources are in Appendix A.

| Label | What | Where |
|---|---|---|
| E1 | `mlua` + `luau` with no recipe (expected failure) | Windows |
| E2 | The documented recipe as written (`RUSTFLAGS` plus `crt1-reactor.o`), raw module, `opt-level` `s` + LTO | Windows, Node and Chrome |
| E3 | E2 at `opt-level` 3 | Windows, Chrome |
| E4 | E2's sources through `wasm-bindgen --target web`, with start function, import map and memory capture | Windows, Chrome |
| E5 | `build.rs` instead of `RUSTFLAGS` | Windows, Chrome |
| E6 | Route B: a standalone Luau module (C++ wrapper, no Rust) bridged from JS | Windows, Chrome and Node |
| E7 | E5 on Ubuntu 24.04 in Docker | Linux, build only |
| E8 | The final harness: E5 plus a `luau` feature gate, a counting allocator over the wasi-libc one, the real-entry shape (`main` as start function, `requestAnimationFrame` loop), allocation-heavy benches, a 10,000-entity `update`, init-order negative tests, target-scoped compiler variables, and the SDK-free stub | Windows, Chrome and native |

### Q1: does a documented recipe build `mlua` (Luau) for `wasm32-unknown-unknown` on stable Rust?

**Yes.** Without the recipe it fails, and with it it builds.

**E1, no recipe (expected failure).** A crate with `mlua = { version = "0.12.3", features = ["luau", "vendored"] }`, built with `cargo build --target wasm32-unknown-unknown`, fails in `mlua-sys`'s build script on a host with no wasm-capable clang:

```
error occurred in cc-rs: failed to find tool "clang++": program not found
```

(`mlua-sys` also panics with "Please enable `vendored` feature to build for wasm32" if `vendored` is missing. `luau` always vendors, so this only matters for the Lua 5.x features.)

**E2, the documented recipe.** `mlua` 0.12.3 ships `docs/wasm32.md`, with `tests/wasm/setup.sh` and `run.sh` in its repository (tag `v0.12.3`, not in the crates.io package). It says Luau supports `wasm32-unknown-unknown` "with a WASI C runtime", and that the host must provide the WASI imports and support Wasm exceptions. Following it, translated to Windows paths (`cygpath -m`, because rustc rejects MSYS paths):

```bash
export WASI_SDK_PATH=<wasi-sdk-34.0 root>
lib="$WASI_SDK_PATH/share/wasi-sysroot/lib/wasm32-wasip1"
export CC_wasm32_unknown_unknown="$WASI_SDK_PATH/bin/clang"
export CXX_wasm32_unknown_unknown="$WASI_SDK_PATH/bin/clang++"
export AR_wasm32_unknown_unknown="$WASI_SDK_PATH/bin/llvm-ar"
export CXXSTDLIB_wasm32_unknown_unknown=c++
export CFLAGS_wasm32_unknown_unknown="--target=wasm32-wasip1"
export LUAU_CXXFLAGS="--target=wasm32-wasip1 -fexceptions -fwasm-exceptions -mllvm -wasm-use-legacy-eh=false"
export CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS="-Lnative=$lib/eh -Lnative=$lib -lstatic=c++abi -lstatic=unwind -lstatic=wasi-emulated-process-clocks -lstatic=c"
cargo build --release --target wasm32-unknown-unknown
```

Result: builds in 42 s on a clean target directory (the Linux container needed about 32 s in release). The module imports eight `wasi_snapshot_preview1` functions (`environ_get`, `environ_sizes_get`, `clock_time_get`, `fd_close`, `fd_fdstat_get`, `fd_seek`, `fd_write`, `proc_exit`) and uses the exnref exception instructions, so it only loads where those are supported. Node 22 needs `--experimental-wasm-exnref`.

**Three gaps the recipe does not cover.** The recipe's test calls one export once, so a game, which calls into the module every frame, hits all three:

1. **Every export after the first hangs.** Without a reactor entry point, wasm-ld wraps each export in `__wasm_call_ctors` and a destructor call. The first call works and the second never returns, even for a function that does nothing. Fix: link wasi-sdk's `crt1-reactor.o` (or the 3-line `reactor.c` below), which exports `_initialize`, and call `_initialize()` exactly once before any other export. With that, repeated calls, `run_tests`, `fib` and the benchmarks all work.
2. **Skipping `_initialize` breaks Luau, not just libc.** Luau's allocator size-class table is built by a C++ static constructor. Without it the first `lua_newstate` traps with "divide by zero" in `newclasspage`. Through `wasm-bindgen` this happens because `__wbindgen_start` does not call `_initialize`. Fix: a `#[wasm_bindgen(start)]` function that calls `_initialize` (declared `extern "C"`).
3. **The WASI shim needs the module's memory before the module starts.** `_initialize` calls `environ_sizes_get`, which writes to linear memory, and `wasm-bindgen` creates the instance inside its own `init()`. The loader must therefore capture `instance.exports.memory` as soon as the instance exists (the spike wraps `WebAssembly.instantiateStreaming`; `exitium export` owns the loader page, so it can do this directly). With `wasm-bindgen --target web` the generated JS also imports the bare specifier `wasi_snapshot_preview1`, so the page needs an import map for it (Appendix A).

**A less intrusive link setup (E5).** `RUSTFLAGS` with `-lstatic=...` applies to every crate in the build, and rustc fails on crates compiled before the libraries are found. A `build.rs` in the `exitium_script` crate does the same job and propagates to the final link. **It must read the target from `CARGO_CFG_TARGET_ARCH` and `CARGO_CFG_TARGET_OS`, not from `cfg!`**: in a build script `cfg!(target_arch)` is the host. It must also be gated on the Cargo feature that enables Luau, so a build without the feature needs no SDK. The gated script (the E8 one) is in Appendix A. In short:

```rust
let wasm = env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("wasm32")
    && env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("unknown");
if !(wasm && env::var_os("CARGO_FEATURE_LUAU").is_some()) { return; } // native or stub: no SDK
```

A `build.rs` that reads `WASI_SDK_PATH` unconditionally would break the stub fallback and every non-wasm build, so the gate above is required, not optional.

**Compiler variables are target-scoped (E8).** The recipe's `LUAU_CXXFLAGS` is not target-scoped: set in a shell that also builds native code, it would feed wasm flags to the native Luau build. The spike replaced it with `CXXFLAGS_wasm32_unknown_unknown` (the `cc` crate reads it only for that target). The build then produced the same module size as the `LUAU_CXXFLAGS` build, ran in Chrome and passed the same checks, and a native build with these variables exported kept working. All variables are `*_wasm32_unknown_unknown`, so exporting them globally is safe, and `cargo xtask` can set them for child processes only. No `RUSTFLAGS` remain.

**Allocator.** Rust and wasi-libc must share one allocator, so the module's `#[global_allocator]` forwards to wasi-libc's `posix_memalign` and `free` (the recipe's `allocator` module). That makes wasi-libc's `malloc` (dlmalloc) the allocator for all Rust code in the wasm build, not just Luau. E8 wraps it in a counting allocator, which composes (25,258 allocations counted over a full E8 run) and shows how roadmap issue #64 (the counting allocator) fits. See "Conditions for AD2", condition 3, for who owns the static and the `unsafe` policy.

**Init order (E8).** The spike used `main` as the `#[wasm_bindgen(start)]` function: it calls `_initialize` first, builds the app state in a thread-local, schedules 300 `requestAnimationFrame` frames and returns, which is the shape of the web runner contract in `app-platform.md` (the runner returns at once and the browser owns the loop). It worked. Two negative tests, each a separate build:

- **Allocation before `_initialize`** (1,000 `String`s allocated first): worked. wasi-libc's allocator initialises lazily, and the rest of the run was normal. This does not make it safe: other libc state (stdio, `environ`, clocks) was not exercised before init, so the rule stays "init first".
- **A second `_initialize`**: `init()` never resolved (checked for 20 s), so the page hung. `__wasm_call_ctors` is not idempotent. The init call needs a once-guard, and the wasm-bindgen start function must be the only caller.

**E7, Linux.** The same recipe (build.rs variant) on `ubuntu:24.04` with Rust 1.100.0 and the Linux wasi-sdk 34 tarball (192 MB) built: debug 62 s (24 MB `.wasm`, dev only), release 32 s (1.54 MB, 445 KB gzip). The self-hosted CI image (`ci/runner/linux/Dockerfile`) has distro `clang` and `lld` but no wasi-sdk, so CI must add it.

**The SDK-free stub (E8).** With the `luau` feature off, a wasm build on a machine with no `WASI_SDK_PATH` and no wasi-sdk compiled and linked: the module has no WASI imports, the generated JS never mentions `wasi_snapshot_preview1`, and it is 29 KB raw and 11 KB gzip for a small allocating function. This only works if the wasi-libc `#[global_allocator]` is behind the same feature. With the allocator on for every wasm32 build, the stub would fail to link as soon as any stub code allocated (an undefined `posix_memalign`). So the stub fallback is real, but only with the allocator and the link setup both gated on the feature.

### Q2: if not, which route works?

Q1 passed, so Q2 is not needed to decide. Each route was still tried or ruled out, so AD2 has a fallback ladder before it reaches owner decision 3.

| Route | Tried | Outcome |
|---|---|---|
| A. A wasm clang target with a minimal libc and libc++ subset (wasi-sdk) | yes (E2 to E5, E7, E8) | **Works.** This is the winner. It is the Q1 recipe, so the "minimal subset" is wasi-libc, libc++, libc++abi and libunwind, with eight WASI imports. |
| B. A separate Luau wasm module bridged from JS | yes (E6) | **Works, but worse.** Luau, its compiler and a 20-line C++ wrapper built as a standalone reactor module with the same wasi-sdk and flags: 2.19 MB raw, 583 KB gzip (no `wasm-opt`). A Lua to host call into JS cost 28 ns, and into JS then a second wasm module 33 ns, with `i32` arguments only. Cheaper per call than mlua, but it has two linear memories, so every entity, component or string crosses by copy. Reflection-derived bindings, userdata handles and the command bus would need a second binding layer on the web, next to mlua on Windows. It also needs the same wasi-sdk and exnref support, so it removes no toolchain cost. Keep it as the plan B if route A breaks. |
| C. `wasm32-unknown-emscripten` (mlua supports it) | no | Not tried. The engine's web backend is `wasm-bindgen` + `winit` + `wgpu` on `wasm32-unknown-unknown` (`app-platform.md`), and Emscripten is a different Rust target with its own runtime and JS glue. It would replace the whole web toolchain, not just scripting. |
| D. A pure-Rust Lua VM | no | Not tried. The pure-Rust VMs I know of target Lua 5.x, not Luau, which drops Luau's types, vectors, sandbox and the typed API the plan relies on. That changes the scripting contract (owner decision 1). |
| E. Native-only scripts with a cfg-gated stub (owner decision 3) | E8 built the stub | Not needed. Remains the documented fallback, and now a verified SDK-free build. |

### Q3: measurements for the winner (route A)

All browser numbers come from Chrome 158.0.8127.37 on the Windows host, headless, release builds, `opt-level` `s` with LTO unless noted. The native baseline is the same Rust and Lua code on the host (`mlua` + vendored Luau built by the MSVC toolchain), built with the same `opt-level` `s` profile for the E8 table and at `opt-level` 3 for the older rows. The E8 table below is the primary result: one build, nine runs of each workload after a warm-up run, wasm and native in the same session. The medians are per-run medians, not best-of-five.

**Hello world in Chrome.** Passed for the raw module (E2), for the `wasm-bindgen` build (E4), for the `build.rs` build (E5) and for E8. The script `return ('hello from luau ' .. _VERSION):upper()` returned `HELLO FROM LUAU LUAU 0.748`. mlua's own tests ran in Chrome too and passed: `eval`, syntax and runtime errors, `pcall`, Rust callbacks returning errors, and coroutines (so exceptions thrown inside Luau and caught inside Luau work on the exnref ABI). Instantiation took 9 to 15 ms in the early runs (up to 445 ms on a cold first load) and 38 ms for E8's full `init()`. Memory was 19 pages (1.2 MB) after load for the hello module and 80 pages (5 MB) for E8 after init, which includes the 10,000-entity state.

**Size** (gzip level 9; browsers see gzip or brotli from the web server):

| Build | Raw | gzip | brotli 11 |
|---|---|---|---|
| E2, `opt-level` `s`, LTO | 1,536,349 | 443,262 | 350,627 |
| E2 + `wasm-opt -Os` (explicit feature flags) | 1,038,994 | 378,342 | n/a |
| E4 `wasm-bindgen` output (no `wasm-opt`) | 1,427,970 | 450,695 | n/a |
| E5 `build.rs` variant, Linux | 1,535,075 | 445,468 | n/a |
| `opt-level` 3 + LTO (E3) | 1,623,919 | 480,873 | n/a |
| E6, standalone Luau module with compiler (route B) | 2,192,835 | 582,813 | n/a |
| E8 `wasm-bindgen` output, Luau on | 1,518,177 | 471,320 | n/a |
| E8 + `wasm-opt -Os` (explicit flags) | 1,048,877 | 400,889 | n/a |
| E8 + `wasm-opt -Oz` (binaryen 133, no feature flags) | 1,039,992 | 400,658 | n/a |
| E8 stub (`luau` off, no SDK) | 29,109 | 10,951 | n/a |

These are module totals for a small host. E8 minus the stub gives Luau's marginal cost as about 1.49 MB raw and 460 KB gzip before `wasm-opt`.

`wasm-opt`: with binaryen version_133 a plain `wasm-opt -Oz` on the E8 module succeeds and the output loads and runs in Chrome (explicit feature flags are not required with this version; older binaryen releases may differ). The explicit flags `--enable-exception-handling --enable-reference-types --enable-gc --enable-bulk-memory --enable-sign-ext --enable-mutable-globals --enable-nontrapping-float-to-int --enable-multivalue` also work. `--all-features` turns on compact imports, which Chrome 158 rejects ("Invalid import kind 126"). Export should pass the explicit list, not `--all-features`.

**Cost of one Lua to Rust call** (`lua.create_function(|_, (a, b): (i64, i64)| Ok(a + b))`, called 2,000,000 times in a Lua loop, minus the same loop without the call). One build (E8), nine runs:

| | Best | Median | Worst |
|---|---|---|---|
| Chrome, E8 | 66.6 ns | 68.9 ns | 83.9 ns |
| Native, `opt-level` `s` | 36.9 ns | 43.4 ns | n/a (not printed) |

The ratio is 1.6x at the median (68.9 / 43.4) and 2.3x for the worst wasm run over the best native run (83.9 / 36.9). The E2 to E5 runs, pooled (nine single results across five builds, so they mix build and run variance), were 71, 55, 59, 53, 80, 58, 62, 68 and 60 ns, with a median of 60 ns and a maximum of 80 ns, against 35.5 to 38.2 ns native at `opt-level` 3. The ratios in this report use the E8 table, not the pooled list.

**Other workloads (E8, nine runs each, Chrome against native, same `opt-level` `s` profile):**

| Workload | Chrome median (best to worst) | Native median (best to worst) | Ratio (median) |
|---|---|---|---|
| `fib(30)` | 100.3 ms (87.4 to 106.5) | 90.2 ms (83.9 to 140.4) | 1.1x |
| 100,000 small tables, each with a string field | 45.7 ms (38.7 to 55.9) | 51.6 ms (43.2 to 58.8) | 0.9x |
| Format and concatenate 100,000 strings | 62.2 ms (47.6 to 84.5) | 49.6 ms (45.5 to 56.4) | 1.25x |
| 10,000 entities, 10-line `update`, pure Luau | 2.20 ms per frame (2.03 to 2.43) | 1.43 ms (1.32 to 1.75) | 1.5x |
| Same, plus one Lua to Rust call per entity | 2.81 ms per frame (2.53 to 3.34) | 2.13 ms (1.94 to 2.49) | 1.3x |

The table and string workloads allocate heavily, so they exercise wasi-libc's dlmalloc through the counting wrapper. They did not regress against native. The 10,000-entity `update` is the roadmap's "10,000 scripted entities running a 10-line `update`" budget (at most 4 ms per frame): it fits at 2.2 to 2.8 ms (worst 3.3 ms). In the live loop (E8's `main`, 300 `requestAnimationFrame` frames, the version with one Rust call per entity) it took a median of 3.2 ms, p99 5.1 ms and a worst of 5.7 ms in headless Chrome. So the budget holds at the median and is exceeded at p99 there. Causes were not isolated (timer granularity, GC pauses of the page and a cold cache after each `requestAnimationFrame` wait are all candidates). It is a condition for SP9 to measure it in the real engine, and it argues against sizing the budget from a micro-benchmark.

The roadmap budget is "Luau on wasm at most 2.5x slower than native". Every ratio in the tables is under it, the worst being the 2.3x worst call run. The per-call figure is a floor for the "reflected field get or set from Lua at most 200 ns" budget. Run-to-run noise in headless Chrome is large (the worst call run is 26% above the best), so SP9's benches must use more runs and a quiet machine.

**Does it fit the budgets?** The web build budget is 13 MB gzipped with Luau and 10 MB without (roadmap "Performance"). Luau adds about 0.38 to 0.48 MB gzipped, so it uses about 15% of the 3 MB gap, and the winner fits with a wide margin. Build and toolchain cost (the R5 "C or C++ risk"):

- **New CI tool:** wasi-sdk 34 on the Linux runner image (192 MB tarball), reached through `WASI_SDK_PATH`. Licences: see "Licence audit".
- **Build time:** about 40 s extra on a clean wasm build in release and about 60 s in debug. Cached after that.
- **Who needs the SDK.** Only builds that pass the `luau` feature on the command line for wasm (see "Conditions for AD2", conditions 1 and 6). `cargo build --workspace --target wasm32-unknown-unknown`, the CLAUDE.md definition-of-done command, keeps building the SDK-free stub, so agents and the owner need no SDK for it. This holds only if no manifest enables `luau` (condition 1). A web game export with scripts on, the Linux CI job that checks it, and anyone building the real web module need the SDK, and `xtask` provisions it. A web export with scripts on and no SDK fails with the fix command (condition 6); it never falls back to the stub.
- **Browser floor:** see "Browser floor".
- **Loader work:** `exitium export` must ship the WASI shim (about 10 lines), an import map for `wasi_snapshot_preview1`, call `_initialize` through the `wasm-bindgen` start function, and give the shim the instance's memory (gap 3).
- **Allocator and C ABI:** the wasi-libc global allocator and the `-lc` link are new for the whole wasm build when `luau` is on.

### Browser floor

The module uses WebAssembly exception handling with `exnref` (the standardised form). From MDN browser-compat-data (`webassembly/exceptionsFinal.json` and `api/GPU.json`):

| Browser | `exnref` | WebGPU |
|---|---|---|
| Chrome, Edge | 137 | 113 |
| Firefox | 131 | 141 (Windows) |
| Safari | 18.4 | 26 |

So **Chrome or Edge 113 to 136 have WebGPU but not `exnref`**. A user there passes the `navigator.gpu` check and then fails with a `WebAssembly.CompileError`. Every other WebGPU browser also has `exnref`, so `exnref` shrinks the WebGPU set only in that Chrome range. Firefox and Safari are out of the MVP anyway (roadmap "Still out"). The consequences:

- The web build's floor is **Chrome or Edge 137** when Luau is on. Luau therefore does add a restriction on Chrome.
- The assets spec (PR #166) keeps Chrome or Edge 113 as the base gate and adds a conditional 137 floor, with a `WebAssembly.validate` probe on a `try_table`, for a game that uses the script feature. The probe and the message text below agree with it. This report does not edit that file.
- The export page feature-detects `exnref` before loading the module and says "Chrome or Edge 137 or later". A probe is a 28-byte module with one `try_table` (opcode `0x1f`), checked by `WebAssembly.validate`; it returned `true` in Chrome 158 (Appendix A, "exnref probe"). The negative case (a browser without `exnref` returning `false`) was not run, because only Chrome 158 was available.
- Using the legacy exception ABI instead would cover Chrome 113 to 136 (legacy exception handling shipped long before). It was not tried: the SDK's libraries are built for the standard ABI, and building both is extra toolchain work. This is open question 1.

### Licence audit

Licensing is a high-risk area, so this report states what the linked C and C++ pieces are. `cargo deny` (ADR-0003, `deny.toml`) only sees Cargo crates, so it cannot see wasi-libc, libc++ or the SDK. The results below come from reading the upstream licence files; AD2 re-verifies them at the pinned version. The one check run locally is that **the wasi-sdk 34.0 Windows tarball contains no LICENSE, COPYING or NOTICE file** (it holds `VERSION`, `bin`, `include`, `lib`, `share`), so the notices must be taken from the upstream repositories at the pinned commits.

| Component | Licence | Allowed by `deny.toml`? |
|---|---|---|
| wasi-libc | Apache-2.0 WITH LLVM-exception, Apache-2.0 and MIT; includes dlmalloc (CC0), cloudlibc (BSD-2-Clause) and musl code (MIT) | yes (all SPDX ids are on the list) |
| musl parts inside wasi-libc | MIT; bundles BSD-2-Clause (TRE regex), the Sun, FreeBSD and Arm math code under permissive notice-retaining terms, and MIT-style smoothsort | yes, but the notices must be kept |
| libc++, libc++abi, libunwind, compiler-rt | Apache-2.0 WITH LLVM-exception; the exception covers the binary form | yes |
| Luau, `mlua`, `mlua-sys`, `luau0-src` | MIT, vendored by crates `cargo deny` can see | yes |

What follows: nothing here is copyleft, so ADR-0003 holds. But the MIT, BSD and Sun-math terms require their notices in redistributed binaries, and the planned notice generator reads `cargo metadata` and will not list these C libraries. Condition 5 covers it, in the same way as ADR-0006 condition 4 does for `triple_buffer`.

## Conditions for AD2

AD2 should record route A and these conditions. If any fails in SP9 or later, it falls back to native-only scripts (owner decision 3), keeping the cfg-gated stub:

1. **CI and provisioning.**
   - The Luau web build is behind a Cargo feature (working name `luau`, on `exitium_script`). It is off by default, so `cargo build --workspace --target wasm32-unknown-unknown` builds the SDK-free stub, the CLAUDE.md definition of done keeps working unchanged, and agent worktrees need no SDK.
   - **The feature gate must hold across the whole workspace.** Resolver 3 unifies features across every member selected by `--workspace`, and the roadmap adds the `examples/*` crates as members. One manifest that enables `luau` would turn it on for every `--workspace` build and bring back the SDK requirement for every agent worktree. Three rules keep that from happening:
     1. **Native always builds Luau.** `exitium_script` declares `mlua` in `[target.'cfg(not(target_arch = "wasm32"))'.dependencies]`, so native has Luau without any feature. (The spike's `mlua` is optional on every target, which suits a standalone crate but would drop Luau from native builds in the engine.)
     2. **The `luau` feature only has an effect on `wasm32`.** It adds the `mlua` dependency, the wasi-libc allocator and the link setup there. Turning it on for a native build changes nothing.
     3. **No manifest in the workspace enables `luau`, not `exitium_script`'s own default, not an `examples/*` crate, not the `exitium` facade.** The web export and the `xtask` wasm step pass it on the command line (`--features`). AD2 must record this rule, or the definition-of-done promise fails.
   - `cargo xtask ci wasm` gains a second step that builds `exitium_script` with the feature on. It reuses `xtask`'s existing `FEATURE_CHECKS` table (`ci.rs`), which `ci wasm` already builds as `-p <crate> --features <f> --target wasm32-unknown-unknown`, by adding `("exitium_script", "luau")`. The same entry makes `ci test` run `cargo test -p exitium_script --features luau` natively, where the feature is a no-op and needs no SDK, so it is safe in the Windows job. `ci all` also calls `ci wasm`, so SP9 must make the wasm half of that entry run only where the SDK is available (always in CI, and locally only when the SDK is provisioned or `WASI_SDK_PATH` is set, with a printed notice when skipped). Otherwise a plain `cargo xtask ci all` would need the SDK on every agent worktree. The wasm step runs only in the Linux Docker job (`ci.yml` `linux`), the only job that runs `ci wasm` today. **The Windows runner job is clippy and test only, and it stays that way**: the Windows runner is the owner host with no SDK. Adding a wasm step there would be new work, which this report does not propose.
   - `xtask` provisions the SDK in a pinned, checksum-verified location and sets the target-scoped compiler variables for its child `cargo` only. The pin is the version plus a SHA-256 per platform (the Windows hash the spike used is in Appendix A; AD2 records the Linux one).
   - **Dependencies.** The `mlua` feature set for intake 2 (#157) is `=0.12.3` with features `["luau"]`. `vendored` is not needed: `luau` always vendors the Luau source (E1 passed it redundantly). `exitium_script`'s `build.rs` uses `cc` as a build-dependency, which needs a `[workspace.dependencies]` entry and a justification in the SP9 PR body (licence, why not std), per the CLAUDE.md dependency rule. The wasi-sdk is a build tool, not a crate, so `cargo deny` does not see it (see "Licence audit").
   - The Linux Dockerfile gains the SDK layer (a 192 MB download with the pinned version and SHA-256). These are changes to `ci/runner/linux/Dockerfile`, `xtask` and, if the build entry point changes, CLAUDE.md. SP9 (or the AD2 follow-up issue) owns them; this report does not touch those paths.
2. **Init and loader.** The shim, start function and memory capture of gap 3 work in `exitium export`'s generated loader and `exitium_platform`'s example loader, under the locked `wasm-bindgen` 0.2.130. `_initialize` runs **exactly once and first**: a Rust once-guard protects it, no Rust allocation happens before it, and it is tested against the real `main` plus `spawn_app` path, not only the spike's `requestAnimationFrame` loop (negative tests: a second call hangs `init()`; allocation before it happened to work and must not be relied on).
3. **Allocator and bench.**
   - **Owner.** Exactly one crate declares the `#[global_allocator]`. A library crate such as `exitium_script` must not, because the final binary may only have one and roadmap #64's counting allocator is a global-allocator wrapper too. Recommendation: the `exitium` facade owns the static (it is in every game's graph exactly once), and under `luau` on wasm it is `Counting<Wasi>`; otherwise `Counting<System>`. E8's `alloc.rs` shows the wrapper composing.
   - **Unsafe policy.** The forwarding allocator is `unsafe` code. CLAUDE.md requires a designated module with `// SAFETY:` comments and Miri tests. Miri cannot run the wasi-libc FFI, so AD2 sets the rule: the wrapper's logic is Miri-tested over an inner `System` allocator, and the `posix_memalign` and `free` forwarding is checked by the Chrome run.
   - **Forwarding.** The spike's `Wasi` forwards only `alloc` and `dealloc`, so `realloc` and `alloc_zeroed` fall back to allocate, copy and free, which makes `Vec` growth pessimistic on wasm. The real owner forwards `realloc` (for alignments up to the malloc alignment) and `alloc_zeroed` to wasi-libc. The allocation-heavy benches below are re-run after that change, because the E8 numbers were measured without it.
   - **Existing benches.** The wasi-libc allocator does not regress the existing wasm benches by more than 5% (CLAUDE.md rule 4). Those benches contain no Luau.
   - **New Luau benches.** SP9 adds an allocation-heavy Luau bench: 10,000 scripted entities running a 10-line `update` at most 4 ms per frame (roadmap), measured in the real engine on the live loop with p99 reported, plus the table and string workloads of Q3. The spike's live loop showed p99 5.1 ms against the 4 ms budget, so this condition is a real risk.
4. The full engine module (with `wgpu` and `winit`) still instantiates and keeps the web size budget. See "Limits".
5. **Licence.** The ADR names the exact C components and licences in "Licence audit"; exports ship a notice that covers them (wasi-libc, musl, dlmalloc, cloudlibc, libc++, libc++abi, libunwind, compiler-rt, Luau); the LICENSE files are read from the upstream repositories at the pinned SDK version (the tarball ships none); and the notice generator that ADR-0006 requires for `triple_buffer` includes them.
6. **Export contract.** Because every web author would otherwise need wasi-sdk 34, `exitium export --target web` builds the SDK-free stub when scripts are **off**, and that page needs no WASI shim, import map or `exnref` probe, so its browser floor stays Chrome or Edge 113. When scripts are **on** and the SDK is absent, `exitium export` and `exitium export --check` both fail and print the fix command (the `--check` preflight row for the SDK); the export never silently ships a stub for a script game, because its scripts would do nothing. For a script game, the loader page contract is fixed: the import map for `wasi_snapshot_preview1`, the memory capture, the start function, and the `exnref` probe. `wasm-opt`, which runs "if on PATH", must not break an export if it fails: the step falls back to the unoptimised module with a warning, and passes the explicit feature list, never `--all-features`. This decision belongs to AD2 and SP9 and lands in `assets-scene-export.md` and `export-examples.md` (SP18).
7. **Browser floor.** The documented floor is Chrome or Edge 137 with Luau on, and the export page shows it (see "Browser floor"), unless the owner chooses to build the legacy exception ABI as well (open question 1).

## MCP / editor surface

None. This is a report. The Luau bindings, hot reload and the command-bus API belong to SP9 (`luau-scripting.md`).

## Performance

The numbers are in "Q3". The budgets that SP9 inherits from the roadmap and how this spike bears on them:

| Roadmap budget | Spike result |
|---|---|
| Web wasm at most 13 MB gzipped with Luau (10 MB without) | Luau adds 0.38 to 0.48 MB. Fits. |
| Luau on wasm at most 2.5x slower than native | 0.9x to 1.25x for compute and allocation-heavy scripts, 1.5x for the 10,000-entity update, 1.6x median and 2.3x worst for one Rust call. Fits, with little margin on the worst call run. |
| A reflected field get or set from Lua at most 200 ns | About 60 to 70 ns per plain function call in Chrome. The floor fits. SP9 must measure real reflected access. |
| 10,000 scripted entities, 10-line `update`, at most 4 ms per frame | 2.2 ms (pure) and 2.8 ms (one Rust call per entity) in a micro-benchmark. In a live `requestAnimationFrame` loop the median is 3.2 ms and p99 is 5.1 ms. Fits at the median, not at p99: condition 3. |
| Hot reload of a 500-line script at most 50 ms | Not measured here. The compiler is in the module (`luau_compile` ran in `hello`). |

## Test plan

The spike's checks, as run, in order. Each is a command or a page that SP9 can copy into its CI (sources and commands in Appendix A).

1. `cargo build --release --target wasm32-unknown-unknown` with the E2 variables (Windows), with the E5 `build.rs` (Windows and Ubuntu 24.04), and with the E8 target-scoped variables and the `luau` feature.
2. The same build with the `luau` feature off and no SDK on the machine (the stub), and a check that the module has no `wasi_snapshot_preview1` import.
3. Load the module in headless Chrome (a static server and `puppeteer-core` driving the installed Chrome), call `_initialize` once, run `run_tests` and the hello script.
4. Call two or more exports in a row (this is what exposed gap 1).
5. Run the Lua to Rust call loop, `fib(30)`, the table and string workloads and the 10,000-entity `update` in Chrome and natively, nine runs each.
6. Build with `wasm-bindgen --target web` 0.2.130 and load the output in Chrome through an import map for `wasi_snapshot_preview1` (gap 2 and gap 3).
7. Negative init tests: allocation before `_initialize`, and a second `_initialize`.
8. Record raw and gzip sizes, with and without `wasm-opt`; check the `wasm-opt` output loads.
9. The `exnref` probe through `WebAssembly.validate`.

Not run: Firefox and Safari, Chrome on another OS, threads, the full engine module, the negative `exnref` probe (a browser without `exnref`), the Windows runner.

## Limits of this spike

- The module under test was a small host, so interactions with `wgpu`, `winit`, `web-sys` beyond `performance.now` and `requestAnimationFrame`, and the ECS are untested. The shared allocator is the likeliest source of surprises. The init-order tests ran against a `requestAnimationFrame` shape, not `winit`'s `spawn_app`.
- One machine (Windows 11) ran every browser number, with headless Chrome 158 only, and the E8 numbers ran under the "Power saver" plan.
- The Linux container checked that the build works, not the browser run.
- Hot reload, the safe and privileged script tiers, and real reflected field access are SP9's work.
- The licence results come from reading upstream files plus one local check (no licence files in the SDK tarball); AD2 re-verifies them at the pinned version.
- It was not tested whether a separate Rust allocator and wasi-libc's (two heaps in one linear memory) would work without the forwarding allocator. The recipe forwards, so this report did too.

## Open questions

1. **Legacy exception ABI.** AD2 (#145) proceeds with the Chrome and Edge 137 floor; nothing waits for the owner. The owner can still object. Building both ABIs would cover Chrome and Edge 113 to 136, which have WebGPU but not `exnref`, at the cost of a second set of SDK libraries and a runtime switch. Or the floor is Chrome or Edge 137 for the MVP. Recommendation: accept 137 for the MVP (the audience is the owner's own web build, and the page tells the user), and revisit only if an example's players need older Chrome.
2. **Allocator owner.** Is the `exitium` facade the right owner of the single `#[global_allocator]`? AD2 decides, together with the #64 counting allocator.
3. **Allocator behaviour.** Does wasi-libc's dlmalloc change frame time in the engine's real allocation pattern? SP9 benchmarks it (condition 3). Also, is the forwarding allocator needed at all?
4. **`wasm-opt` in the export step.** It saves about 15% gzip. With binaryen 133 a plain `-Oz` works, with the failure mode above for `--all-features`. Does `exitium export` run it, and pin a binaryen version? An export decision for `assets-scene-export.md` or SP18 (`export-examples.md`).

## Final verdict

**PASS.** Luau runs on `wasm32-unknown-unknown` in Chrome 158 through `mlua` and wasi-sdk 34, and it fits the size and 2.5x speed budgets. The conditions are a feature-gated SDK build in Linux CI, a once-only init, a single global-allocator owner, a licence notice, the export and loader contract, and a Chrome or Edge 137 floor. The 4 ms entity budget is met at the median and must be re-measured in the real engine. Owner decision 3's native-only fallback is not needed, and stays as the verified SDK-free stub.

## Appendix A: spike sources and commands

These are the as-run sources of E8, the final harness (earlier experiments differ only as noted in "Experiments"). Paths are relative to a scratch crate named `spike`. Everything runs on stable Rust 1.100.0.

**`rust-toolchain.toml`**

```toml
[toolchain]
channel = "1.100.0"
components = ["clippy", "rustfmt"]
targets = ["wasm32-unknown-unknown"]
```

**`Cargo.toml`**

```toml
[package]
name = "spike"
version = "0.0.0"
edition = "2024"
publish = false

[workspace]

[lib]
crate-type = ["cdylib", "rlib"]

[features]
# Luau is off by default: the SDK-free "stub" build compiles no C or C++.
luau = ["dep:mlua"]
test-alloc-before-init = []
test-double-init = []

[dependencies]
mlua = { version = "=0.12.3", features = ["luau"], optional = true }

[target.'cfg(target_arch = "wasm32")'.dependencies]
wasm-bindgen = "=0.2.130"
web-sys = { version = "0.3", features = ["Window", "Performance"] }

[build-dependencies]
cc = "1"

[[bin]]
name = "native"
required-features = ["luau"]

[profile.release]
opt-level = "s"
lto = true
codegen-units = 1
```

This is a standalone crate, so `mlua` is optional on every target and the `luau` feature switches it on everywhere. The engine's `exitium_script` differs: `mlua` is a `cfg(not(target_arch = "wasm32"))` dependency (always on for native), and no manifest enables `luau` (condition 1). `cc = "1"` is the spike's loose pin; in the repo it is declared once in `[workspace.dependencies]`.

**`build.rs`** (the build script reads the target from `CARGO_CFG_*`, because `cfg!` in a build script is the host)

```rust
use std::{env, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=WASI_SDK_PATH");
    let wasm = env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("wasm32")
        && env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("unknown");
    let luau = env::var_os("CARGO_FEATURE_LUAU").is_some();
    if !(wasm && luau) {
        return; // native build, or the stub build: nothing to link, no SDK needed
    }
    let Some(sdk) = env::var_os("WASI_SDK_PATH") else {
        panic!("feature `luau` on wasm32-unknown-unknown needs WASI_SDK_PATH (wasi-sdk 34); build without the feature for the stub");
    };
    let lib = PathBuf::from(sdk).join("share/wasi-sysroot/lib/wasm32-wasip1");
    cc::Build::new().file("reactor.c").flag("--target=wasm32-wasip1").compile("reactor");
    println!("cargo:rustc-link-search=native={}", lib.join("eh").display());
    println!("cargo:rustc-link-search=native={}", lib.display());
    for l in ["c++", "c++abi", "unwind", "wasi-emulated-process-clocks", "c"] {
        println!("cargo:rustc-link-lib=static={l}");
    }
}
```

**`reactor.c`** (replaces wasi-sdk's `crt1-reactor.o`)

```c
extern void __wasm_call_ctors(void);
__attribute__((export_name("_initialize"))) void _initialize(void) { __wasm_call_ctors(); }
```

**`src/alloc.rs`** (wasm32 and `luau` only; the `#[global_allocator]` is the spike's stand-in for the single owner of condition 3)

```rust
use std::alloc::{GlobalAlloc, Layout};
use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};

unsafe extern "C" {
    fn posix_memalign(ptr: *mut *mut c_void, align: usize, size: usize) -> i32;
    fn free(ptr: *mut c_void);
}

pub struct Wasi;
unsafe impl GlobalAlloc for Wasi {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mut ptr = std::ptr::null_mut();
        // SAFETY: posix_memalign gets a power-of-two alignment >= size_of::<usize>().
        if unsafe { posix_memalign(&mut ptr, layout.align().max(size_of::<usize>()), layout.size()) } != 0 {
            return std::ptr::null_mut();
        }
        ptr.cast()
    }
    unsafe fn dealloc(&self, ptr: *mut u8, _: Layout) {
        // SAFETY: `ptr` came from posix_memalign in `alloc`.
        unsafe { free(ptr.cast()) };
    }
}

pub static ALLOCS: AtomicUsize = AtomicUsize::new(0);
pub struct Counting<A>(pub A);
unsafe impl<A: GlobalAlloc> GlobalAlloc for Counting<A> {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        unsafe { self.0.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { self.0.dealloc(p, l) }
    }
}

#[global_allocator]
static GLOBAL: Counting<Wasi> = Counting(Wasi);
```

**`src/bench.rs`** (shared by wasm and native; `luau` only)

```rust
use mlua::{Function, Lua};
use std::cell::RefCell;
use std::rc::Rc;

pub fn hello() -> String {
    Lua::new().load("return ('hello from luau ' .. _VERSION):upper()").eval::<String>().unwrap()
}

/// `n` Lua -> Rust calls of a two-argument function.
pub fn call_loop(n: u32) -> i64 {
    let lua = Lua::new();
    let add = lua.create_function(|_, (a, b): (i64, i64)| Ok(a + b)).unwrap();
    lua.globals().set("add", add).unwrap();
    let f = lua
        .load("local add = add; local n = ...; local s = 0; for i = 1, n do s = add(s, 1) end return s")
        .into_function()
        .unwrap();
    f.call::<i64>(n).unwrap()
}

/// The same loop without the Rust call, to subtract the loop cost.
pub fn empty_loop(n: u32) -> i64 {
    let lua = Lua::new();
    let f = lua
        .load("local n = ...; local s = 0; for i = 1, n do s = s + 1 end return s")
        .into_function()
        .unwrap();
    f.call::<i64>(n).unwrap()
}

pub fn fib(n: u32) -> i64 {
    let lua = Lua::new();
    let src = "local function fib(n) if n < 2 then return n end return fib(n-1) + fib(n-2) end return fib(...)";
    lua.load(src).into_function().unwrap().call::<i64>(n).unwrap()
}

/// Allocation-heavy: `n` small tables, each with a string field.
pub fn tables(n: u32) -> i64 {
    let lua = Lua::new();
    let src = "local n = ...; local t = {} for i = 1, n do t[i] = {x = i, y = i * 2, name = 'e' .. i} end \
               local s = 0 for i = 1, n do s = s + t[i].x + #t[i].name end return s";
    lua.load(src).into_function().unwrap().call::<i64>(n).unwrap()
}

/// Allocation-heavy: formatting and concatenating `n` strings.
pub fn strings(n: u32) -> i64 {
    let lua = Lua::new();
    let src = "local n = ...; local p = {} for i = 1, n do p[#p + 1] = string.format('%d:%s', i, tostring(i * 3)) end \
               return #table.concat(p, ',')";
    lua.load(src).into_function().unwrap().call::<i64>(n).unwrap()
}

const UPDATE: &str = r#"
function update(es, dt)
  for i = 1, #es do
    local e = es[i]
    e.x = e.x + e.vx * dt
    e.y = e.y + e.vy * dt
    if e.x < 0 or e.x > 1000 then e.vx = -e.vx end
    if e.y < 0 or e.y > 1000 then e.vy = -e.vy end
    e.timer = e.timer - dt
    if e.timer <= 0 then e.timer = 1 e.hp = e.hp - 1 end
  end
end
function update_rust(es, dt)
  for i = 1, #es do
    local e = es[i]
    e.x = e.x + e.vx * dt
    e.y = e.y + e.vy * dt
    if e.x < 0 or e.x > 1000 then e.vx = -e.vx end
    if e.y < 0 or e.y > 1000 then e.vy = -e.vy end
    e.timer = e.timer - dt
    if e.timer <= 0 then e.timer = 1 e.hp = e.hp - 1 end
    store(i, e.x, e.y)
  end
end
"#;

/// 10,000 scripted entities with a 10-line `update`, kept alive across frames like a game would.
pub struct Entities {
    lua: Lua,
    #[allow(dead_code)]
    store: Rc<RefCell<Vec<[f64; 2]>>>,
}

impl Entities {
    pub fn new(count: u32) -> Self {
        let lua = Lua::new();
        let store = Rc::new(RefCell::new(vec![[0.0; 2]; count as usize + 1]));
        let s2 = store.clone();
        let f = lua
            .create_function(move |_, (i, x, y): (usize, f64, f64)| {
                s2.borrow_mut()[i] = [x, y];
                Ok(())
            })
            .unwrap();
        lua.globals().set("store", f).unwrap();
        lua.load(UPDATE).exec().unwrap();
        lua.load(
            "local n = ...; es = {} for i = 1, n do es[i] = {x = i % 1000, y = (i * 7) % 1000, \
             vx = (i % 13) - 6, vy = (i % 11) - 5, timer = (i % 10) / 10, hp = 100} end",
        )
        .into_function()
        .unwrap()
        .call::<()>(count)
        .unwrap();
        Self { lua, store }
    }
    pub fn frame(&self) {
        let es: mlua::Value = self.lua.globals().get("es").unwrap();
        let f: Function = self.lua.globals().get("update").unwrap();
        f.call::<()>((es, 0.016)).unwrap();
    }
    /// One frame where each entity also makes one Lua -> Rust call.
    pub fn frame_rust(&self) {
        let es: mlua::Value = self.lua.globals().get("es").unwrap();
        let f: Function = self.lua.globals().get("update_rust").unwrap();
        f.call::<()>((es, 0.016)).unwrap();
    }
}
```

**`src/lib.rs`**

```rust
#[cfg(feature = "luau")]
pub mod bench;

#[cfg(all(target_arch = "wasm32", feature = "luau"))]
mod alloc;

#[cfg(all(target_arch = "wasm32", feature = "luau"))]
mod web {
    use super::bench::{self, Entities};
    use std::cell::RefCell;
    use std::rc::Rc;
    use wasm_bindgen::prelude::*;

    unsafe extern "C" {
        /// From reactor.c: runs the C and C++ static constructors (libc, Luau size classes).
        fn _initialize();
    }

    thread_local! {
        static APP: RefCell<Option<Entities>> = const { RefCell::new(None) };
        static FRAMES: RefCell<Vec<f64>> = const { RefCell::new(Vec::new()) };
    }

    /// The real-entry shape: `main` is the wasm-bindgen start function, owns init order, builds
    /// app state in a thread-local, and returns at once while requestAnimationFrame drives frames.
    #[wasm_bindgen(start)]
    fn main() {
        #[cfg(feature = "test-alloc-before-init")]
        {
            let early: Vec<String> = (0..1000).map(|i| format!("early {i}")).collect();
            std::hint::black_box(&early);
            std::mem::forget(early);
        }
        // SAFETY: called once, before any C or C++ code runs.
        unsafe { _initialize() };
        #[cfg(feature = "test-double-init")]
        // SAFETY: deliberately wrong, to observe what a second call does.
        unsafe {
            _initialize()
        };
        APP.with(|a| *a.borrow_mut() = Some(Entities::new(10_000)));
        spawn_loop(300);
    }

    fn now() -> f64 {
        web_sys::window().unwrap().performance().unwrap().now()
    }

    fn spawn_loop(frames: u32) {
        let cb: Rc<RefCell<Option<Closure<dyn FnMut()>>>> = Rc::new(RefCell::new(None));
        let cb2 = cb.clone();
        let mut left = frames;
        *cb.borrow_mut() = Some(Closure::new(move || {
            let t = now();
            APP.with(|a| a.borrow().as_ref().unwrap().frame_rust());
            FRAMES.with(|f| f.borrow_mut().push(now() - t));
            left -= 1;
            if left > 0 {
                let w = web_sys::window().unwrap();
                w.request_animation_frame(cb2.borrow().as_ref().unwrap().as_ref().unchecked_ref()).unwrap();
            }
        }));
        let w = web_sys::window().unwrap();
        w.request_animation_frame(cb.borrow().as_ref().unwrap().as_ref().unchecked_ref()).unwrap();
    }

    #[wasm_bindgen]
    pub fn loop_frames() -> Vec<f64> { FRAMES.with(|f| f.borrow().clone()) }
    #[wasm_bindgen]
    pub fn alloc_count() -> f64 { super::alloc::ALLOCS.load(std::sync::atomic::Ordering::Relaxed) as f64 }
    #[wasm_bindgen]
    pub fn hello() -> String { bench::hello() }
    #[wasm_bindgen]
    pub fn call_loop(n: u32) -> f64 { bench::call_loop(n) as f64 }
    #[wasm_bindgen]
    pub fn empty_loop(n: u32) -> f64 { bench::empty_loop(n) as f64 }
    #[wasm_bindgen]
    pub fn fib(n: u32) -> f64 { bench::fib(n) as f64 }
    #[wasm_bindgen]
    pub fn tables(n: u32) -> f64 { bench::tables(n) as f64 }
    #[wasm_bindgen]
    pub fn strings(n: u32) -> f64 { bench::strings(n) as f64 }

    /// Mean ms per frame over `frames` frames of 10,000 scripted entities.
    #[wasm_bindgen]
    pub fn entity_frames(frames: u32, rust: bool) -> f64 {
        let e = Entities::new(10_000);
        for _ in 0..3 {
            if rust { e.frame_rust() } else { e.frame() }
        }
        let t = now();
        for _ in 0..frames {
            if rust { e.frame_rust() } else { e.frame() }
        }
        (now() - t) / frames as f64
    }
}

/// SDK-free stub build: the default Rust allocator, no C, no WASI imports.
#[cfg(all(target_arch = "wasm32", not(feature = "luau")))]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn stub_hello() -> String {
    (0..100).map(|i| format!("stub {i}")).collect::<Vec<_>>().join(",")
}
```

**`src/bin/native.rs`** (native baseline, nine runs, best, median and worst)

```rust
use spike::bench::{self, Entities};
use std::time::Instant;

fn runs<T>(mut f: impl FnMut() -> T) -> Vec<f64> {
    let mut v: Vec<f64> = (0..9)
        .map(|_| {
            let t = Instant::now();
            std::hint::black_box(f());
            t.elapsed().as_secs_f64() * 1e3
        })
        .collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v
}
fn show(name: &str, v: &[f64], div: f64) {
    println!("{name}: best {:.3} median {:.3} worst {:.3}", v[0] / div, v[4] / div, v[8] / div);
}
fn main() {
    println!("{}", bench::hello());
    let n = 2_000_000u32;
    bench::call_loop(1000);
    let c = runs(|| bench::call_loop(n));
    let e = runs(|| bench::empty_loop(n));
    println!("nsPerCall best {:.1} median {:.1}", (c[0] - e[0]) / n as f64 * 1e6, (c[4] - e[4]) / n as f64 * 1e6);
    show("fib30 ms", &runs(|| bench::fib(30)), 1.0);
    show("tables100k ms", &runs(|| bench::tables(100_000)), 1.0);
    show("strings100k ms", &runs(|| bench::strings(100_000)), 1.0);
    for (name, rust) in [("entities10k pure ms/frame", false), ("entities10k rust-call ms/frame", true)] {
        let en = Entities::new(10_000);
        for _ in 0..3 {
            if rust { en.frame_rust() } else { en.frame() }
        }
        let v = runs(|| (0..20).for_each(|_| if rust { en.frame_rust() } else { en.frame() }));
        show(name, &v, 20.0);
    }
}
```

**Build environment** (target-scoped; Windows paths via `cygpath -m`; on Linux drop `.exe`)

```bash
export WASI_SDK_PATH="$(cygpath -m <wasi-sdk-34.0 root>)"
export CC_wasm32_unknown_unknown="$WASI_SDK_PATH/bin/clang.exe"
export CXX_wasm32_unknown_unknown="$WASI_SDK_PATH/bin/clang++.exe"
export AR_wasm32_unknown_unknown="$WASI_SDK_PATH/bin/llvm-ar.exe"
export CXXSTDLIB_wasm32_unknown_unknown=c++
export CFLAGS_wasm32_unknown_unknown="--target=wasm32-wasip1"
export CXXFLAGS_wasm32_unknown_unknown="--target=wasm32-wasip1 -fexceptions -fwasm-exceptions -mllvm -wasm-use-legacy-eh=false"
unset LUAU_CXXFLAGS CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS
```

SDK downloads: `https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-34/wasi-sdk-34.0-x86_64-windows.tar.gz` (619,003,408 bytes, SHA-256 `cccb5c323a9b34f0349a9b09e8804a0a7632c68c3310f4b5f437ed57d7e71d8f`, computed on the copy the spike used) and `wasi-sdk-34.0-x86_64-linux.tar.gz` (192 MB; AD2 records its hash). Binaryen: release `version_133`.

**Commands**

```bash
# Luau build, then wasm-bindgen (0.2.130 CLI), then optional wasm-opt
cargo build --release --features luau --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir pkg --out-name spike target/wasm32-unknown-unknown/release/spike.wasm
wasm-opt -Os --enable-exception-handling --enable-reference-types --enable-gc --enable-bulk-memory \
  --enable-sign-ext --enable-mutable-globals --enable-nontrapping-float-to-int --enable-multivalue \
  pkg/spike_bg.wasm -o pkg/spike_bg.wasm   # plain `wasm-opt -Oz pkg/spike_bg.wasm -o ...` also works on binaryen 133

# SDK-free stub: no WASI_SDK_PATH, no compiler variables
env -u WASI_SDK_PATH cargo build --release --target wasm32-unknown-unknown

# Negative init tests: add --features luau,test-alloc-before-init  or  luau,test-double-init

# Native baseline (same sources, same profile)
cargo run --release --features luau --bin native

# Sizes
gzip -9 -c pkg/spike_bg.wasm | wc -c
```

**Loader page** (`index.html` beside `spike.js`, `spike_bg.wasm` and `wasi_shim.js`). The import map satisfies the bare specifier that `wasm-bindgen --target web` emits, and the `instantiateStreaming` wrapper hands the memory to the shim before the start function runs.

```html
<!doctype html><meta charset=utf-8><title>spike</title>
<script type="importmap">{"imports":{"wasi_snapshot_preview1":"./wasi_shim.js"}}</script>
<script type="module">
const real = WebAssembly.instantiateStreaming.bind(WebAssembly);
WebAssembly.instantiateStreaming = async (...a) => { const r = await real(...a); globalThis.__wasmMemory = r.instance.exports.memory; return r; };
import init, * as api from "./spike.js";
const stats = (v) => { const s = [...v].sort((a, b) => a - b); return { best: s[0], median: s[s.length >> 1], worst: s[s.length - 1], n: s.length }; };
window.run = async (wasmUrl) => {
  const r = {};
  const t0 = performance.now();
  try { const out = await init({ module_or_path: wasmUrl }); r.memPagesAfterInit = out.memory.buffer.byteLength / 65536; }
  catch (e) { return { initError: String(e) }; }
  r.initMs = performance.now() - t0;
  for (let i = 0; i < 200 && api.loop_frames().length < 300; i++) await new Promise(r => setTimeout(r, 50));
  const lf = api.loop_frames(); r.loopFrames = lf.length;
  if (lf.length > 10) { const s = stats(lf.slice(5)); r.loopMsPerFrame = { median: s.median, best: s.best, worst: s.worst }; const sorted = [...lf.slice(5)].sort((a, b) => a - b); r.loopP99 = sorted[Math.floor(sorted.length * 0.99)]; }
  r.hello = api.hello();
  const time = (f) => { const t = performance.now(); f(); return performance.now() - t; };
  const rep = (f, k = 9) => { f(); return Array.from({ length: k }, () => time(f)); };
  const n = 2000000;
  const c = rep(() => api.call_loop(n)), e = rep(() => api.empty_loop(n));
  const sc = stats(c), se = stats(e);
  r.nsPerCall = { best: (sc.best - se.best) / n * 1e6, median: (sc.median - se.median) / n * 1e6, worst: (sc.worst - se.best) / n * 1e6 };
  r.fib30Ms = stats(rep(() => api.fib(30)));
  r.tables100kMs = stats(rep(() => api.tables(100000)));
  r.strings100kMs = stats(rep(() => api.strings(100000)));
  r.entities10kPureMsPerFrame = stats(Array.from({ length: 9 }, () => api.entity_frames(20, false)));
  r.entities10kRustCallMsPerFrame = stats(Array.from({ length: 9 }, () => api.entity_frames(20, true)));
  r.allocCount = api.alloc_count();
  return r;
};
</script>
```

**`wasi_shim.js`** (the eight imports; a shim of about 10 lines)

```js
const dv = () => new DataView(globalThis.__wasmMemory.buffer);
export const environ_get = () => 0;
export const environ_sizes_get = (c, s) => { dv().setUint32(c, 0, true); dv().setUint32(s, 0, true); return 0; };
export const clock_time_get = (id, p, o) => { dv().setBigUint64(o, BigInt(Math.round(performance.now() * 1e6)), true); return 0; };
export const fd_close = () => 0;
export const fd_fdstat_get = () => 8;
export const fd_seek = () => 70;
export const fd_write = (fd, iovs, n, nw) => { let t = 0; for (let i = 0; i < n; i++) t += dv().getUint32(iovs + i * 8 + 4, true); dv().setUint32(nw, t, true); return 0; };
export const proc_exit = (c) => { throw new Error("proc_exit " + c); };
```

**Runner** (`node run.mjs spike_bg.wasm`; `index.html` and `wasi_shim.js` are copied into `pkg/` next to the `wasm-bindgen` output; with `puppeteer-core` 25.18.1 and the installed Chrome; serves the folder that holds the loader page and prints the result of `run`)

```js
import puppeteer from "puppeteer-core"; import http from "node:http"; import fs from "node:fs"; import path from "node:path";
const [,, wasmFile = "spike_bg.wasm"] = process.argv;
const types = { ".js": "text/javascript", ".html": "text/html", ".wasm": "application/wasm" };
const srv = http.createServer((q, s) => { const u = q.url === "/" ? "index.html" : q.url.slice(1).split("?")[0]; const f = path.join("pkg", u);
  if (!fs.existsSync(f)) { s.statusCode = 404; return s.end(); } s.setHeader("content-type", types[path.extname(f)] || "text/plain"); s.end(fs.readFileSync(f)); }).listen(0);
const b = await puppeteer.launch({ executablePath: "C:/Program Files/Google/Chrome/Application/chrome.exe", headless: true, protocolTimeout: 600000 });
const pg = await b.newPage(); pg.on("pageerror", e => console.log("[pageerror]", e.message));
await pg.goto(`http://localhost:${srv.address().port}/`);
console.log("chrome", await b.version(), "wasm", wasmFile);
console.log(JSON.stringify(await pg.evaluate(u => run(u), wasmFile), null, 1));
await b.close(); srv.close();
```

The double-init check used the same server with `init()` raced against a 20 s timer (`init did not resolve in 20 s`), and the controls resolved.

**`exnref` probe** (a function body of `try_table` with an empty catch list, `0x1f 0x40 0x00 0x0b 0x0b`):

```js
const mk = (body) => new Uint8Array([0,0x61,0x73,0x6d,1,0,0,0, 1,4,1,0x60,0,0, 3,2,1,0, 0x0a,body.length+2,1,body.length,...body]);
const hasExnref = WebAssembly.validate(mk([0,0x1f,0x40,0,0x0b,0x0b])); // true in Chrome 158
```

**Earlier experiments.** E2: the recipe's `RUSTFLAGS` with `-Clink-arg=$lib/crt1-reactor.o`, a `cdylib` with `mlua = { version = "=0.12.3", features = ["luau"] }` and the recipe's `allocator` module. E3 is E2 at `opt-level = 3`. E4 is E2's sources plus `wasm-bindgen = "=0.2.130"` and the start function. E5 is E4 with the first `build.rs` (it did not check `CARGO_CFG_TARGET_*` or a feature; E8's replaces it). E6 is a 20-line C++ file (`hello`, `call_loop`, `empty_loop`, `fib`) that links `lua.h`, `lualib.h` and `luacode.h` from `luau0-src`, built with the wasi-sdk flags above; its host call is the `env.host_add` import. E7 is E5 built by `apt-get install build-essential`, `rustup` 1.100.0, the Linux SDK tarball and the compiler variables above, in `ubuntu:24.04`.
