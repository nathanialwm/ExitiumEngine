# Spec: Luau on wasm, the R5 spike report

- Issue: #136 (SP1 in `docs/specs/mvp-roadmap.md`)
- Crates: none (the spike's code is throwaway and stays outside the repo). It informs `exitium_script` (SP9) and the ADR AD2 (#145).
- Status: draft. A report with measurements, not a design. It ends with a pass or fail (see "Verdict").

## Goal

Answer risk R5: Luau is C++, and `wasm32-unknown-unknown` has no C or C++ standard library, so `mlua` with a vendored Luau might not build for the web. The roadmap asks three questions (Q1 to Q3 in "Findings"). AD2 (#145) turns this report into the recorded route.

## Non-goals

- No code lands in the repo. The experiments below live in throwaway crates outside it, and each command here reproduces them.
- No engine integration. The spike used hello-world sized modules, so `wgpu`, `winit` and the real ECS were not in the build (see "Limits of this spike").
- No decision. AD2 records it. This report gives AD2 the evidence and a recommendation.

## Verdict

**PASS. AD2 is a go for the real Luau VM on the web.** Route: `mlua` 0.12.3 with the `luau` feature on `wasm32-unknown-unknown`, with C and C++ built by **wasi-sdk 34** (clang, wasi-libc, libc++), WebAssembly exceptions, and a WASI preview 1 shim in JS. The fallback in owner decision 3 (native-only scripts, `exitium_script` builds a cfg-gated stub on wasm) is **not needed** and stays in the plan only as insurance for the conditions listed in "Conditions for AD2".

Evidence in five lines:

1. A documented recipe exists (`mlua` 0.12.3 `docs/wasm32.md`) and built on stable Rust 1.100.0, on Windows and in an Ubuntu 24.04 container.
2. In Chrome 158 a hello-world script ran, and mlua's own tests (eval, errors, callbacks, coroutines) passed.
3. Size: 451 KB gzip through `wasm-bindgen` (378 KB after `wasm-opt -Os`), about 0.45 MB of the 13 MB web budget.
4. One Lua to Rust call costs about 60 ns in Chrome (53 to 81 ns over 9 runs) against about 37 ns native: 1.6x at the median, 2.3x worst run. Pure Luau compute (`fib(30)`) is 1.0x to 1.6x native. Both are inside the 2.5x budget.
5. The cost is toolchain, not size: CI needs wasi-sdk (192 MB) and a loader shim, and three recipe gaps had to be closed (below).

## Design

### Environment

| Item | Value |
|---|---|
| Host | Windows 11, x86_64 (all browser and native numbers); Ubuntu 24.04 in Docker (build check) |
| Rust | 1.100.0 stable (`rust-toolchain.toml`), target `wasm32-unknown-unknown` |
| `mlua` / `mlua-sys` / `luau0-src` | 0.12.3 / 0.14.0 / 0.24.1+luau748 (Luau 0.748, `_VERSION` reports it) |
| C and C++ toolchain | wasi-sdk 34.0, clang 23.1.0-rc3, wasi-libc, libc++, libc++abi, libunwind |
| `wasm-bindgen` and CLI | 0.2.130 (the version `assets-scene-export.md` locks) |
| `wasm-opt` | binaryen version_133 |
| Browser | Chrome 158.0.8127.37, driven headless with `puppeteer-core` 25.18.1 |

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

1. **Every export after the first hangs.** Without a reactor entry point, wasm-ld wraps each export in `__wasm_call_ctors` and a destructor call. The first call works and the second never returns, even for a function that does nothing (checked: `scratch`, `empty_loop`). Fix: link wasi-sdk's `crt1-reactor.o`, which exports `_initialize`, and call `_initialize()` exactly once before any other export. With that, repeated calls, `run_tests`, `fib` and the benchmarks all work.
2. **Skipping `_initialize` breaks Luau, not just libc.** Luau's allocator size-class table is built by a C++ static constructor. Without it the first `lua_newstate` traps with "divide by zero" in `newclasspage`. Through `wasm-bindgen` this happens because `__wbindgen_start` does not call `_initialize`. Fix: a `#[wasm_bindgen(start)]` function that calls `_initialize` (declared `extern "C"`).
3. **The WASI shim needs the module's memory before the module starts.** `_initialize` calls `environ_sizes_get`, which writes to linear memory, and `wasm-bindgen` creates the instance inside its own `init()`. The loader must therefore capture `instance.exports.memory` as soon as the instance exists (we wrapped `WebAssembly.instantiateStreaming`; `exitium export` owns the loader page, so it can do this directly).

**A less intrusive link setup (E5).** `RUSTFLAGS` with `-lstatic=...` applies to every crate in the build, and rustc fails on crates compiled before the libraries are found. A `build.rs` in the `exitium_script` crate does the same job and propagates to the final link:

```rust
// build.rs (wasm32-unknown-unknown only)
let lib = PathBuf::from(env::var("WASI_SDK_PATH")?).join("share/wasi-sysroot/lib/wasm32-wasip1");
cc::Build::new().file("reactor.c").flag("--target=wasm32-wasip1").compile("reactor");
println!("cargo:rustc-link-search=native={}", lib.join("eh").display());
println!("cargo:rustc-link-search=native={}", lib.display());
for l in ["c++", "c++abi", "unwind", "wasi-emulated-process-clocks", "c"] { println!("cargo:rustc-link-lib=static={l}"); }
```

```c
// reactor.c: replaces crt1-reactor.o (export _initialize)
extern void __wasm_call_ctors(void);
__attribute__((export_name("_initialize"))) void _initialize(void) { __wasm_call_ctors(); }
```

With this, only the compiler variables (`CC_*`, `CXX_*`, `AR_*`, `CXXSTDLIB_*`, `CFLAGS_*`, `LUAU_CXXFLAGS`) remain, and `cargo xtask` can set them. It built in 40 s and passed the same checks in Chrome.

**Allocator.** Rust and wasi-libc must share one allocator, so the module's `#[global_allocator]` forwards to wasi-libc's `posix_memalign` and `free` (the recipe's `allocator` module). That makes wasi-libc's `malloc` the allocator for all Rust code in the wasm build, not just Luau. SP9 must benchmark that against the default Rust allocator.

**E7, Linux.** The same recipe (build.rs variant) on `ubuntu:24.04` with Rust 1.100.0 and the Linux wasi-sdk 34 tarball (192 MB) built: debug 62 s (24 MB `.wasm`, dev only), release 32 s (1.54 MB, 445 KB gzip). The self-hosted CI image (`ci/runner/linux/Dockerfile`) has distro `clang` and `lld` but no wasi-sdk, so CI must add it.

### Q2: if not, which route works?

Q1 passed, so Q2 is not needed to decide. Each route was still tried or ruled out, so AD2 has a fallback ladder before it reaches owner decision 3.

| Route | Tried | Outcome |
|---|---|---|
| A. A wasm clang target with a minimal libc and libc++ subset (wasi-sdk) | yes (E2, E4, E5, E7) | **Works.** This is the winner. It is the Q1 recipe, so the "minimal subset" is wasi-libc, libc++, libc++abi and libunwind, with eight WASI imports. |
| B. A separate Luau wasm module bridged from JS | yes (E6) | **Works, but worse.** Luau, its compiler and a 20-line C++ wrapper built as a standalone reactor module with the same wasi-sdk and flags: 2.19 MB raw, 583 KB gzip (no `wasm-opt`). A Lua to host call into JS cost 28 ns, and into JS then a second wasm module 33 ns, with `i32` arguments only. Cheaper per call than mlua, but it has two linear memories, so every entity, component or string crosses by copy. Reflection-derived bindings, userdata handles and the command bus would need a second binding layer on the web, next to mlua on Windows. It also needs the same wasi-sdk and exnref support, so it removes no toolchain cost. Keep it as the plan B if route A breaks. |
| C. `wasm32-unknown-emscripten` (mlua supports it) | no | Not tried. The engine's web backend is `wasm-bindgen` + `winit` + `wgpu` on `wasm32-unknown-unknown` (`app-platform.md`), and Emscripten is a different Rust target with its own runtime and JS glue. It would replace the whole web toolchain, not just scripting. |
| D. A pure-Rust Lua VM | no | Not tried. The pure-Rust VMs I know of target Lua 5.x, not Luau, which drops Luau's types, vectors, sandbox and the typed API the plan relies on. That changes the scripting contract (owner decision 1). |
| E. Native-only scripts with a cfg-gated stub (owner decision 3) | n/a | Not needed. Remains the documented fallback. |

### Q3: measurements for the winner (route A)

All browser numbers come from Chrome 158.0.8127.37 on the Windows host, headless, release builds, `opt-level` `s` with LTO unless noted, best of 5 per measurement, 2,000,000 iterations. The native baseline is the same Rust and Lua code on the host (`mlua` + vendored Luau built by the MSVC toolchain).

**Hello world in Chrome.** Passed for the raw module (E2), for the `wasm-bindgen` build (E4), and for the `build.rs` build (E5). The script `return ('hello from luau ' .. _VERSION):upper()` returned `HELLO FROM LUAU LUAU 0.748`. mlua's own tests ran in Chrome too and passed: `eval`, syntax and runtime errors, `pcall`, Rust callbacks returning errors, and coroutines (so exceptions thrown inside Luau and caught inside Luau work on the exnref ABI). Instantiation took 9 to 15 ms (up to 445 ms on a cold first load). Memory was 19 pages (1.2 MB) after load and 28 pages (1.8 MB) after the benchmarks.

**Size** (gzip level 9; browsers see gzip or brotli from the web server):

| Build | Raw | gzip | brotli 11 |
|---|---|---|---|
| E2, `opt-level` `s`, LTO | 1,536,349 | 443,262 | 350,627 |
| E2 + `wasm-opt -Os` (explicit feature flags) | 1,038,994 | 378,342 | n/a |
| E4 `wasm-bindgen` output (no `wasm-opt`) | 1,427,970 | 450,695 | n/a |
| E5 `build.rs` variant, Linux | 1,535,075 | 445,468 | n/a |
| `opt-level` 3 + LTO | 1,623,919 | 480,873 | n/a |
| E6, standalone Luau module with compiler (route B) | 2,192,835 | 582,813 | n/a |

These are module totals for a hello-world host, so they are close to the marginal cost of Luau in the game wasm. `wasm-opt` needs `--enable-exception-handling --enable-reference-types --enable-gc` plus the usual bulk-memory, sign-ext, mutable-globals, nontrapping-float-to-int and multivalue flags. `--all-features` turns on compact imports, which Chrome 158 rejects ("Invalid import kind 126").

**Cost of one Lua to Rust call** (`lua.create_function(|_, (a, b): (i64, i64)| Ok(a + b))`, called 2,000,000 times in a Lua loop, minus the same loop without the call):

| Build | Per call | `fib(30)` in Luau | Empty loop iteration |
|---|---|---|---|
| Chrome, nine runs over the E2, E2 `opt-level` 3, `wasm-opt`, E4 and E5 builds | 71, 55, 59, 53, 80, 58, 62, 68, 60 ns | 82 to 107 ms | 8 to 14 ns |
| Native, `opt-level` 3 | 35.5 to 38.2 ns | 67 to 78 ms | 7.5 to 8.6 ns |
| Native, `opt-level` `s` | 41.7 to 44.1 ns | 78 to 80 ms | 9.7 to 10.1 ns |

Median in Chrome is about 60 ns per call, against about 37 ns native. The ratios are 1.6x at the median and 2.3x at the worst run, and 1.0x to 1.6x for compute. The roadmap budget is "Luau on wasm at most 2.5x slower than native", and the per-call figure is a floor for the "reflected field get or set from Lua at most 200 ns" budget. Run-to-run noise in headless Chrome is large (the worst call run is 50% above the best), so SP9's benches must use more runs and a quiet machine.

**Does it fit the budgets?** The web build budget is 13 MB gzipped with Luau and 10 MB without (roadmap "Performance"). Luau adds about 0.38 to 0.48 MB gzipped, so it uses about 15% of the 3 MB gap, and the winner fits with a wide margin. Build and toolchain cost (the R5 "C or C++ risk"):

- **New CI tool:** wasi-sdk 34 on the Linux runner image (192 MB tarball, Apache-2.0 with LLVM exception for the LLVM parts; the licences are from memory and AD2 must check them), reached through `WASI_SDK_PATH`.
- **Build time:** about 40 s extra on a clean wasm build in release and about 60 s in debug. Cached after that. `cargo build --workspace --target wasm32-unknown-unknown` (the `xtask ci wasm` step) then needs `WASI_SDK_PATH` and the compiler variables, which `xtask` can set so developers need only the SDK.
- **Browser floor:** the module needs WebAssembly exception handling with `exnref`: Chrome 137 or later and Firefox 131 or later per MDN and caniuse; Safari was not verified. The engine already requires WebGPU, so this adds no new restriction on Chrome. Using the legacy exception ABI instead was not tried, and the SDK's libraries are built for the standard one.
- **Loader work:** `exitium export` must ship the WASI shim (about 15 lines), call `_initialize` through the `wasm-bindgen` start function, and give the shim the instance's memory (gap 3).
- **Allocator and C ABI:** the wasi-libc global allocator and the `-lc` link are new for the whole wasm build.

### Conditions for AD2

AD2 should record route A and these conditions. If any fails in SP9 or later, it falls back to native-only scripts (owner decision 3), keeping the cfg-gated stub:

1. wasi-sdk installs in CI and `cargo xtask ci wasm` builds `exitium_script` with it, in both the Docker Linux runner and the Windows runner.
2. The shim, start function and memory capture of gap 3 work in `exitium export`'s generated loader and `exitium_platform`'s example loader, under the locked `wasm-bindgen` 0.2.130.
3. The wasi-libc global allocator does not regress the existing wasm benches by more than 5% (CLAUDE.md, rule 4).
4. The full engine module (with `wgpu` and `winit`) still instantiates and keeps the web size budget. See "Limits".

## MCP / editor surface

None. This is a report. The Luau bindings, hot reload and the command-bus API belong to SP9 (`luau-scripting.md`).

## Performance

The numbers are in "Q3". The budgets that SP9 inherits from the roadmap and how this spike bears on them:

| Roadmap budget | Spike result |
|---|---|
| Web wasm at most 13 MB gzipped with Luau (10 MB without) | Luau adds 0.38 to 0.48 MB. Fits. |
| Luau on wasm at most 2.5x slower than native | 1.0x to 1.6x for compute, 1.6x median and 2.3x worst for one Rust call. Fits, with little margin on the worst call run. |
| A reflected field get or set from Lua at most 200 ns | About 60 ns per plain function call in Chrome. The floor fits. SP9 must measure real reflected access. |
| Hot reload of a 500-line script at most 50 ms | Not measured here. The compiler is in the module (`luau_compile` ran in `hello`). |

## Test plan

The spike's checks, as run, in order. Each is a command or a page that SP9 can copy into its CI.

1. `cargo build --release --target wasm32-unknown-unknown` with the E2 variables (Windows) and with the E5 `build.rs` (Windows and Ubuntu 24.04).
2. Load the module in headless Chrome (a static server and `puppeteer-core` driving the installed Chrome), call `_initialize` once, run `run_tests` and the hello script.
3. Call two or more exports in a row (this is what exposed gap 1).
4. Run the Lua to Rust call loop and `fib(30)` in Chrome and natively, five runs each, best of five.
5. Build with `wasm-bindgen --target web` 0.2.130 and load the output in Chrome through an import map for `wasi_snapshot_preview1` (gap 2 and gap 3).
6. Record raw, gzip and brotli sizes, with and without `wasm-opt`.

Not run: Firefox and Safari, Chrome on another OS, threads, the full engine module.

## Limits of this spike

- The module under test was a hello-world host, so interactions with `wgpu`, `winit`, `web-sys` and the ECS are untested. The shared allocator is the likeliest source of surprises.
- One machine (Windows 11) ran every browser number, with headless Chrome 158 only.
- The Linux container checked that the build works, not the browser run.
- Hot reload, the safe and privileged script tiers, and real reflected field access are SP9's work.
- Licence text for wasi-sdk, wasi-libc and Luau was not audited here (Luau and `mlua` are MIT).

## Open questions

1. **Safari and Firefox.** Does the `exnref` ABI load there? Matters only if the web build ever targets them (WebGPU already narrows it).
2. **Legacy exception ABI.** Is it worth building both ABIs to cover older browsers? Not tried.
3. **Allocator.** Does wasi-libc's dlmalloc change frame time in the engine's real allocation pattern? SP9 benchmarks it (condition 3).
4. **`wasm-opt` in the export step.** It saves about 15% gzip, but needs the exact feature flags above. Does `exitium export` run it? An export decision for `assets-scene-export.md` or SP18 (`export-examples.md`).
