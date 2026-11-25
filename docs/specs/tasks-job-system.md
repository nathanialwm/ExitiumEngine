# Spec: Job system, thread budget and wasm fallback

- Issue: #19
- Crates: `exitium_tasks` (Cargo consumers: `exitium_ecs` #28/#48 (edge declared by intake 1), the facade glue that installs the default pool; it depends on `exitium_profiler` from #65. `exitium_mcp` has no Cargo edge to it (roadmap C3: `mcp -> command` only) and reaches it through the command bus and the registry; `xtask` #66 only sets `EXITIUM_THREADS`)
- Status: draft (amended by #77: Miri-clean job wrapper shape, restricted-helping argument, help-depth assertion, `SetThreadBudget` rebuild; amended by #138 / SP2: the executor description, the `reflect` feature and its edge, the `profiling` feature chain)

## Goal

`exitium_tasks` is the foundation of "multithreading from day one" (INTERVIEW #9). It gives the engine one pool of worker threads and a small, safe-to-use API on top: `scope`/`spawn` for jobs that borrow stack data, `parallel_for` and `par_chunks[_mut]` for data-parallel loops, and `join`. A `ThreadBudget` decides how many threads the engine may use (all cores minus a reserve by default, the 4-thread min-spec profile, an `EXITIUM_THREADS` override). The same API runs unchanged on `wasm32-unknown-unknown`, where it degrades to an inline executor on the calling thread (R5).

This spec decides **custom pool vs `rayon`** (custom, std-only, no new dependency), confines `unsafe` to one designated module, `scope.rs`, which holds every soundness obligation of scoped jobs (lifetime erasure, wrapper, wait and abort guard) with a soundness argument a reviewer can check, and states panic, nesting and wasm behaviour precisely so the ECS parallel iteration (#21 spec, #28) and executor (#48) can be built on it without guessing.

## Non-goals

- A frame/tick **system scheduler**. `PLAN.md` lists "frame/tick scheduler" in the crate blurb; system ordering, access-conflict batching and the tick loop belong to the ECS scheduler spec (#22) and `exitium_app`. This crate provides only the job primitives they run on. #26 updates the `lib.rs` crate doc line accordingly.
- Async/futures, I/O threads and **detached `'static` background jobs** (asset import, save compression). Additive later: the internal `Job` is already a boxed `'static` closure, so `spawn_detached` needs no layout change. See open question 8.
- Task priorities, **cancellation** of queued jobs, task graphs with dependencies, GPU compute dispatch, thread affinity, thread priority, SMT/physical-core awareness.
- N-dimensional or 2D range helpers (tilemap, destruction): callers flatten to `Range<usize>`.
- `parallel_fold`/reduce and parallel sort helpers (a safe addition on top of `join`, later).
- **Web threads** (SharedArrayBuffer, cross-origin isolation). That is a Phase 6 web-pipeline concern and needs nightly `build-std`; this spec only guarantees a correct single-thread path and a stable API a threaded web backend could later sit behind.
- Re-exporting through the `exitium` facade: a separate PR (the facade API is a high-risk area with extra reviewer scrutiny, ADR-0005).

## Design

### Decision: custom std-only pool, not `rayon`

| Criterion | A. `rayon` (+ `rayon-core`, `crossbeam-deque/epoch/utils`, `either`) | B. **Custom, std only, mutex-guarded deques (chosen)** | C. Custom + `crossbeam-deque` |
|---|---|---|---|
| **wasm32 support** | Needs OS threads. Stable `wasm32-unknown-unknown` has none; web threads need atomics + shared memory + nightly `-Zbuild-std` (`wasm-bindgen-rayon`), which conflicts with "stable Rust only" (CLAUDE.md). `rayon` does have a wasm fallback, but only for the implicit global pool (it behaves as one thread; `join` runs sequentially, `spawn` may not run until something yields). Explicit `ThreadPoolBuilder` pools return an error there (checked against the rayon-core 1.13 docs). We would still write and test our own inline path for scoped jobs, or hand our single-thread guarantee to a third party. | Inline backend is part of the design (same `Scope` type, same code path as `threads == 1` on native). Zero cfg-visible API differences. | Same as B. |
| **Thread budgets** | `ThreadPoolBuilder::num_threads` works, and rayon-core 1.13 has `use_current_thread`, which makes the caller pool thread 0, so the idle-main problem is solvable. The caveats: that thread does not run the work-stealing loop and only runs jobs when it yields into rayon (`scope`, `yield_now`), spawn/exit handlers do not run for it, a local pool built this way leaks its registry, and the call errors if the thread is already in another pool. Without it a caller outside the pool **blocks** in `scope` and "4-thread min-spec" means 4 workers + an idle main. Budget accounting is not ours either way. | `threads` **includes the calling thread** (it helps while waiting). 4-thread min-spec = main + 3 workers, all busy. Reserved cores and `EXITIUM_THREADS` are first-class. | Same as B. |
| **Scheduler integration** | Possible (explicit pools, `scope`), but stats for MCP, panic wrapping with system names, profiler lanes and the "worker index" hook all go through someone else's types. | Counters, names (`exitium-worker-N`), spans and `current_worker_index()` are ours and reflectable (rule 2). | Same as B. |
| **Dependency gate** | New dependency set (6 crates, all MIT/Apache, allowed by `deny.toml`): high-risk review (justified in the PR body), supply-chain surface. | **None.** | One new crate family: high-risk review. |
| **`unsafe` in our repo** | None (upstream, heavily used and Miri-tested). | One designated module (`scope.rs`) with one `unsafe fn` and four small `unsafe` blocks (about 25 lines in all, see "Unsafe: the designated module") and every soundness obligation of scoped jobs; Miri-tested (below). | Same as B (deque `unsafe` is upstream). |
| **Raw performance** | Best in class. | Adequate for the budgets below (single `Mutex<VecDeque>` per worker, uncontended lock about 15 to 25 ns; jobs are 10 us to 10 ms in the engine, not 10 ns). | Better under heavy stealing contention. |

**Decision: B.** The deciding facts are wasm (we need our own inline path regardless), zero new dependencies, and control of stats, panic payloads and worker naming. The caller-helps budget model is **not** decisive: `use_current_thread` could give rayon one too, with the caveats in the table. The price is one audited `unsafe` module. The public API is deliberately a small subset of what `rayon` offers (`scope`, `join`, chunked `for`), so the decision is **reversible without touching any caller**:

- **Trigger to move to C** (a dependency PR, high-risk review): after one optimisation pass in #30, the 100,000-job or the 16-job fan-out **job-rate** ceiling in the Performance table is missed by more than 2x. Only job rate is a queue-implementation question. A miss of the coarse-grained `parallel_for` scaling ratio (16 large chunks, compute-bound) is **not** a trigger: it is not limited by the queue, so it is investigated as chunking, SMT (open question 7), memory bandwidth or a perf issue in #30. 3.2x on 4 real cores is plausible for a std-only pool.
- **Trigger to move to A**: the owner decides own-`unsafe` is not acceptable, or C also misses. `Scope::spawn` maps to `rayon::Scope::spawn`, `join` to `rayon::join`, `parallel_for`/`par_chunks[_mut]` to `par_chunks`/`with_min_len`. The inline backend stays for wasm.

Rejected: D. an async executor (`smol`, `async-executor`): not a data-parallel API, adds a dependency, no scoped borrowing. E. `std::thread::scope` per call: 15 to 60 us per thread spawn, violates every spawn budget.

### Crate layout

```
crates/exitium_tasks/src/
  lib.rs        module docs, re-exports, free functions (`scope`, `parallel_for`, ...)
  budget.rs     ThreadBudget, ResolvedBudget, BudgetSource, EXITIUM_THREADS parsing     (safe)
  platform.rs   the only place with cfg(target_arch = "wasm32"): available threads, now_ns, worker spawn (safe)
  queue.rs      Queued, WorkerQueue (Mutex<VecDeque<Queued>>), Injector, steal scan      (safe)
  pool.rs       TaskPool, Shared, worker loop, sleep/wake protocol, shutdown            (safe)
  scope.rs      #[allow(unsafe_code)] Scope, Scope::spawn + job wrapper (raw scope pointer, `f` in
                MaybeUninit), `erase`, TaskPool::scope, wait region + abort guard, inline queue and
                drain, panic collection                                              (DESIGNATED UNSAFE MODULE)
  par.rs        Chunking, parallel_for, par_chunks, par_chunks_mut, join                (safe)
  stats.rs      PoolStats, WorkerStats, padded counters                                 (safe)
benches/pool.rs   (#30)
clippy.toml       disallowed-methods: std::time::Instant::now, std::thread::spawn,
                  std::thread::Builder::spawn, available_parallelism (outside platform.rs)
```

**Module boundary.** `scope.rs` owns everything the transmute's soundness depends on: the `Scope` type and its variance (I4), the spawn wrapper (I1, I2, I5), `erase`, and the wait region with its abort guard (I2, I3, I6). `Scope`, `ScopeShared` and their fields are private to `scope.rs`; `pool.rs`, `queue.rs` and `par.rs` cannot construct a `Scope` or touch `pending`. They offer `scope.rs` a small safe interface (`Shared::push(Queued)`, `Shared::find_job(filter) -> Option<Queued>`, `Shared::wake_one()`, `Shared::park_hint()`) and call `scope`/`spawn` through its public safe API. Consequence: a PR that could break soundness has to edit `scope.rs`, the designated unsafe module, which every reviewer must audit as high-risk (CLAUDE.md, ADR-0005): every `// SAFETY:` argument and the invariants table are re-checked on each such PR, and the soundness argument below rests on that single audited surface. Even a bug in the safe modules (a panic in `find_job`, a lost job) can only cause an abort or a hang, never a use-after-free, because the abort guard turns any unwind in the wait region into an abort and `scope` never returns while `pending > 0`.

`exitium_tasks` has **no external dependencies**. Optional workspace-internal path dependencies, each behind a feature that is off by default: `reflect` (`exitium_reflect`, derives `Reflect` on the stats/budget types, see MCP section) and `profiling` (`exitium_profiler` spans, #65). Neither pulls a third-party crate. **Both are workspace edges, and neither is in intake 1:** `tasks -> profiler` is added by #65 (ECS scheduler spec, "Profiler and tasks amendments" item 5), and `tasks -> reflect` is an **edge PR** (roadmap C5), see "Reflection" below. Until then `exitium_tasks` depends on no workspace crate other than the ones intake 1 declares (whichever way the `tasks -> core` question in the roadmap settles, this sentence stays true).

### Thread budget

```rust
/// How many threads the engine may run jobs on. Immutable once a pool is built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThreadBudget {
    threads: u32,   // concurrent job runners INCLUDING the calling thread, 1..=MAX_THREADS
    reserved: u32,  // logical cores deliberately left unclaimed (render/audio/OS); informational once resolved
}
pub const MAX_THREADS: u32 = 128;

impl ThreadBudget {
    pub fn single() -> Self;                    // threads = 1: no workers, inline executor (wasm, tests)
    pub fn min_spec() -> Self;                  // threads = 4: the INTERVIEW #13 profile, independent of the machine
    pub fn fixed(threads: u32) -> Self;         // clamped to 1..=MAX_THREADS
    pub fn detect() -> ResolvedBudget;          // env > default detection
    pub fn resolve(settings: ThreadSettings) -> ResolvedBudget;   // env > settings > default detection
    pub fn threads(&self) -> u32;
    pub fn workers(&self) -> u32;               // threads() - 1
    pub fn reserved(&self) -> u32;
}

pub struct ThreadSettings { pub threads: Option<u32>, pub reserved: Option<u32> }   // None = automatic; Some(n) is clamped to 1..=MAX_THREADS (the command rejects 0)


pub struct ResolvedBudget {
    pub budget: ThreadBudget,
    pub available: u32,                         // logical cores reported by the OS (1 on wasm32)
    pub source: BudgetSource,
    pub env_warning: Option<String>,            // set when EXITIUM_THREADS was present but invalid
}
pub enum BudgetSource {
    Env, Settings, Detected,
    Platform,   // wasm32: forced to 1
    Explicit,   // `TaskPool::new(ThreadBudget)` called directly (tests, benches); `available` is still probed
}
```

Resolution is a pure function `resolve_with(env: Option<&str>, settings, available) -> ResolvedBudget` (tested table-driven; **tests never call `std::env::set_var`, which is `unsafe` in edition 2024**). `detect`/`resolve` only read the process environment and `available_parallelism()` and call it.

1. **Platform:** on `wasm32` the result is always `threads = 1`, `source = Platform`; `EXITIUM_THREADS` is ignored (no environment there).
2. **`EXITIUM_THREADS=N`** (decimal, surrounding whitespace allowed): `1 <= N <= MAX_THREADS` is used as-is, **even above `available`** (oversubscription is allowed for tests and benches), `source = Env`. `0`, negative, non-numeric, empty or `> MAX_THREADS` is **ignored**, a message is placed in `env_warning`, and resolution falls through. The env var beats project settings so CI and `xtask bench-ci --min-spec` (#66) can pin a count without editing files.
3. **Settings** (`ThreadSettings`, embedded by `exitium_app` in project settings): `threads = Some(n)` clamps to `1..=MAX_THREADS`, `source = Settings`.
4. **Default detection:** `threads = clamp(available - reserved, 1, MAX_THREADS)` with `reserved` from `Settings` if set, else this table. The reserve covers a render submission thread, audio, async I/O and the OS; a 4-core min-spec machine keeps all four.

| `available` logical cores | default `reserved` | `threads` (main + workers) |
|---|---|---|
| 1 | 0 | 1 (inline) |
| 2 | 0 | 2 |
| 4 (min-spec) | 0 | 4 (main + 3 workers) |
| 6 | 1 | 5 |
| 8 | 1 | 7 |
| 12 | 2 | 10 |
| 16 | 2 | 14 |

`reserved = 0` if `available <= 4`, `1` if `<= 8`, else `2`. `ThreadBudget::min_spec()` is the benchmark profile, not the default on a big machine: it is how the dev PC reproduces a 4-core run.

`TaskPool::from_resolved` (and `new`, below) spawns **`threads - 1`** workers, named `exitium-worker-0 ..`. `threads == 1` spawns none and uses the inline backend. The invalid-env warning is not logged by this crate (no logging dependency yet); `exitium_app` reports `ResolvedBudget::env_warning` at startup and `tasks_get_thread_budget` returns it (together with `source`, so `Platform` and `Explicit` are visible there too).

### Data layout

```rust
pub struct TaskPool { shared: Arc<Shared>, workers: Vec<JoinHandle<()>>, resolved: ResolvedBudget, id: u32 }   // `id`: process-unique, from a static AtomicU32

// All pool operations are implemented on `Shared`; `TaskPool` is the owner of the threads.
// A worker holds `Arc<Shared>`, so free functions can find "the current pool" from a thread-local.
struct Shared {
    queues: Box<[WorkerQueue]>,         // one per worker, 64-byte aligned (no false sharing)
    injector: Mutex<VecDeque<Queued>>,  // jobs spawned by threads that are not workers (callers)
    sleep: Sleep,                       // Mutex<()> + Condvar + sleepers: AtomicU32 + epoch: AtomicU64
    shutdown: AtomicBool,
    stats: Box<[PaddedWorkerCounters]>, // one per worker + one shared slot for non-worker threads
    born: u64,                          // platform::now_ns() at creation (0 on wasm32)
}
#[repr(align(64))] struct WorkerQueue { deque: Mutex<VecDeque<Queued>> }
type Job = Box<dyn FnOnce() + Send + 'static>;       // lifetime-erased by `erase` in scope.rs
struct Queued { job: Job, scope: usize }              // `scope` = address of the owning `Arc<ScopeShared>`, used only by restricted helping (below)

struct ScopeShared {                    // one Arc per scope(); each job holds a clone
    pending: AtomicUsize,               // spawned and not yet finished
    panic: Mutex<Option<Box<dyn Any + Send>>>,   // first job panic payload
    waiter: Thread,                     // the thread blocked in scope(), unparked on completion
    #[cfg(feature = "profiling")]
    span: exitium_profiler::SpanHandle, // the scope's own `exitium_tasks::scope` span, made at scope() entry; EVERY spawn (also one made inside a job) propagates this, never Span::current() (ECS scheduler spec, "Profiler and tasks amendments" item 5)
}
pub struct Scope<'scope, 'env: 'scope> {
    shared: &'scope Shared,
    state: Arc<ScopeShared>,
    inline: Mutex<VecDeque<Job>>,       // used only when workers == 0; holds ERASED ('static) jobs, see below
    _invariant: PhantomData<&'scope mut &'scope ()>, _env: PhantomData<&'env mut &'env ()>,
}
```

**`Scope` owns no `'scope`-bearing data.** An earlier sketch stored boxed `dyn FnOnce(&'scope Scope) + 'scope` closures inside `Scope`. That does not compile: a local `Scope` that is borrowed for `'scope` while owning `'scope`-bearing values with drop glue is rejected by dropck (`E0597`, "borrow might be used here, when `scope` is dropped"). `std::thread::scope` avoids it because its `Scope` holds only an `Arc` and `PhantomData`. Here every queue, including the inline one, holds the lifetime-erased `'static` `Job` produced by `erase`, so `Scope` contains only `&Shared` (a `'scope` reference, no drop glue), an `Arc`, a `Mutex<VecDeque<Job>>` and `PhantomData`. The shape above was compiled and run (nested spawn, borrowed stack data, job panic, `scope` inside a `Drop` during unwind) in a throwaway crate on edition 2024 while writing this revision. The consequence is that the inline backend needs the same `unsafe` erasure and the same invariants I1 to I3 as the threaded one.

Memory: per job one heap allocation for the closure (typically 32 to 64 bytes) plus 24 bytes (fat pointer + scope tag) in a deque; 100k queued jobs is about 10 MB. Pool fixed cost at 4 threads: queues and counters under 2 KiB; each worker stack is **2 MiB** (`WORKER_STACK_BYTES`; virtual, committed lazily). Idle heap footprint of a pool: <= 64 KiB.

**Job lifecycle** (`spawn` to completion):

1. `Scope::spawn(f)` (in `scope.rs`): `pending.fetch_add(1, Relaxed)`, build the wrapper closure (below), erase its lifetime (`erase`). Threaded backend: push `Queued { job, scope }` to the caller's own deque if it is a worker, else to the injector, then, if `sleepers > 0`, wake one sleeper (protocol below). Inline backend (`workers == 0`): push the job to `Scope.inline`.
2. A runner (worker loop or a thread waiting in `scope`) pops: **own deque back (LIFO, cache-hot, depth-first)**, then **injector front (FIFO, oldest first)**, then **steals the front of another worker's deque** (scan starting at a per-thread rotating index, one pass). Each deque lock is held only for one `push/pop`; **no user code and no second lock runs under a queue lock**, so queue locks never nest and cannot deadlock, and user panics cannot poison them (a poisoned lock is recovered with `PoisonError::into_inner` regardless).
3. The wrapper (shape fixed under "Unsafe: the designated module") calls `f` with `&Scope` inside `catch_unwind`; both references exist only for that call. It then records the first panic payload and does the **only** decrement: `pending.fetch_sub(1, AcqRel)`; if it was the last, `waiter.unpark()`. Everything after `f` returns touches only the wrapper's own `Arc<ScopeShared>`.

**Sleep/wake protocol** (no lost wakeups, no unsafe). Producer: after pushing, `epoch.fetch_add(1, SeqCst)`; if `sleepers.load(SeqCst) > 0`, lock `sleep.mutex` and `notify_one`. Idle worker: spin for `SPIN_NS = 20 us` re-scanning queues (`0` under `cfg(miri)`), then `e0 = epoch.load(SeqCst)`, rescan once, lock `sleep.mutex`, `sleepers += 1` (SeqCst), `while epoch == e0 && !shutdown { cv.wait }`, `sleepers -= 1`. Either the producer sees `sleepers > 0` and notifies, or the worker sees the new epoch and does not sleep (Dekker-style, both sides SeqCst, worker re-checks under the lock). A worker that takes a job while more queued work and sleepers exist wakes one more (chain wake), so a burst of N spawns costs one syscall per woken worker, not per spawn. `TaskPool::drop` sets `shutdown`, `notify_all`, joins every worker (it skips joining a handle belonging to the current thread, so a drop on a worker cannot self-deadlock).

A thread blocked in `scope` **helps** (runs jobs it can find, subject to the depth rule under "Nested jobs") until `pending == 0`; only after one full scan finds nothing and a short spin (`SPIN_NS`) also finds nothing does it `thread::park()` on its own handle, looping on `pending.load(Acquire)` for spurious wakeups. A parked waiter does not steal work spawned after it parked (its own scope's remaining jobs are then all running elsewhere); that costs a little tail parallelism and is accepted for v1 (open question 6).

### Public API sketch

```rust
impl TaskPool {
    pub fn new(budget: ThreadBudget) -> TaskPool;              // = from_resolved(ResolvedBudget { source: Explicit, available: platform probe, .. })
    pub fn from_resolved(resolved: ResolvedBudget) -> TaskPool;   // spawns budget.workers() threads; keeps `resolved` for stats
    pub fn threads(&self) -> u32;                              // >= 1, includes the calling thread
    pub fn worker_count(&self) -> u32;                         // threads() - 1; 0 means inline
    pub fn budget(&self) -> &ThreadBudget;
    pub fn resolved(&self) -> &ResolvedBudget;                 // where `available` and `source` in PoolStats come from
    pub fn stats(&self) -> PoolStats;

    /// Runs `f`, then blocks (helping) until every job spawned through its `Scope` has finished.
    pub fn scope<'env, F, R>(&self, f: F) -> R
        where F: for<'scope> FnOnce(&'scope Scope<'scope, 'env>) -> R;

    /// `a` is spawned (may run on another thread); `b` always runs on the calling thread, so only `A`/`RA` need `Send`.
    pub fn join<A, B, RA, RB>(&self, a: A, b: B) -> (RA, RB)
        where A: FnOnce() -> RA + Send, RA: Send, B: FnOnce() -> RB;

    pub fn parallel_for<F>(&self, range: Range<usize>, chunking: Chunking, f: F)
        where F: Fn(Range<usize>) + Sync;
    pub fn par_chunks<T: Sync, F>(&self, data: &[T], chunking: Chunking, f: F)
        where F: Fn(usize /* offset of chunk in data */, &[T]) + Sync;
    pub fn par_chunks_mut<T: Send, F>(&self, data: &mut [T], chunking: Chunking, f: F)
        where F: Fn(usize, &mut [T]) + Sync;
}

impl<'scope, 'env> Scope<'scope, 'env> {
    /// `f` may borrow anything that outlives the `scope` call; it also receives the scope so it can spawn more.
    pub fn spawn<F>(&'scope self, f: F) where F: FnOnce(&'scope Scope<'scope, 'env>) + Send + 'scope;
    pub fn threads(&self) -> u32;
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Chunking {
    /// About 4 chunks per thread, but never fewer than `min_chunk` items per chunk (a short tail excepted).
    Auto { min_chunk: usize },
    /// Exactly `n` items per chunk, the last may be shorter. `Fixed(0)` panics (programmer error); `min_chunk == 0` is treated as 1.
    Fixed(usize),
}
impl Chunking {
    pub const fn auto() -> Self { Chunking::Auto { min_chunk: 1024 } }   // assumes a cheap per-item body
    pub fn chunk_len(self, len: usize, threads: u32) -> usize;
}

// Free functions: same signatures, run on the "current pool" (below).
pub fn scope(..); pub fn join(..); pub fn parallel_for(..); pub fn par_chunks(..); pub fn par_chunks_mut(..);
pub fn current_threads() -> u32;
pub fn current_worker_index() -> Option<u32>;    // Some(i) on exitium-worker-i, None on any other thread
```

**Which pool do the free functions use?** The ECS spec calls `exitium_tasks::parallel_for` with no pool argument, so the pool is found in this order: (1) the pool of the job currently running on this thread (a `thread_local` set by workers for their lifetime and by a helping thread for the duration of each job it runs), (2) the process **default pool**, installed once by the facade's `ThreadingPlugin` (app-platform spec, F4; not by `exitium_app`, which has no `exitium_tasks` edge) with `TaskPool::set_default(Arc<TaskPool>) -> Option<Arc<TaskPool>>` (an `RwLock<Option<Arc<TaskPool>>>`, read cost about 20 ns per free-function call, not per item; worker threads hit the thread-local first and skip it), (3) a static inline pool (`ThreadBudget::single()`), so library code and unit tests work with no setup. Unit tests of this crate use explicit `TaskPool` values and never touch the default.

**`parallel_for` contract.**

- `f` receives a **sub-range**, so the inner loop is a plain `for i in r` that autovectorizes; it is never called with an empty range, and every index in `range` is covered exactly once by disjoint sub-ranges.
- The **set of chunk ranges is a pure function of `(range, chunk_len)`**: boundaries are `range.start + k * chunk_len`, independent of timing and thread count for `Fixed(n)`. Only the executing thread and the order vary. Code that needs reproducible floating-point results uses `Fixed(n)` and combines per-chunk partials in chunk order (the chunk index is `(r.start - range.start) / n`).
- `chunk_len`: `Fixed(n) => n`; `Auto { min_chunk } => max(div_ceil(len, 4 * threads), max(min_chunk, 1))`. Examples: 1,000,000 items, 4 threads, `auto()`: 62,500 per chunk, 16 chunks. 10,000 items: 1,024 per chunk, 10 chunks. 1,000 items: one chunk (serial).
- **Serial fast path:** if `threads == 1` or `chunk_len >= len`, `f(range)` is called **once, inline, on the calling thread**, with no scope, no allocation and no job; the only shared-memory write is one `Relaxed` increment of the caller's padded `serial_fast_paths` counter. This is what makes `parallel_for` safe to use for small inputs, and is exactly the wasm path.
- Otherwise the range is split by **recursive halving at multiples of `chunk_len`**: a job spawns the right half and recurses on the left, until a piece is one chunk. Spawn cost is spread across workers (not all on the caller) and a steal grabs a large piece. Stack depth is `log2(chunks)`. `Fixed(1)` over 1M items creates 1M jobs by request; callers that over-split pay for it (the ECS passes `Fixed(batch)` with `batch >= 256`).
- `f: Fn + Sync` is shared by `&F` across jobs. `par_chunks_mut` uses recursive `split_at_mut`, so each job **owns** a `&mut [T]` over its own disjoint sub-slice; there is no unsafe and no `&mut` derived from a shared reference. Per-thread mutable scratch uses `thread_local!` or per-chunk output slices.

**Fit with the ECS specs.** `par_iter_mut` (ECS spec, Threading) builds its list of `(archetype, row range)` work items on the calling thread and calls `parallel_for(0..items.len(), Chunking::Fixed(1), |r| for i in r { ... })` or `parallel_for(0..total_rows, Chunking::Fixed(batch), ..)` with its own `batch = clamp(total_rows / (4 x threads), 256, 16_384)`; `threads` comes from `current_threads()`. The closure captures `Copy` raw-pointer `Fetch` values, which is why ECS adds `unsafe impl Send/Sync` for them (raw pointers are `!Sync`, and `parallel_for` needs `F: Sync`); that `unsafe` lives in the ECS designated modules, not here. The executor (#48) runs each **segment** of the plan (the nodes between two barriers) as a dataflow inside **one** `scope`: it spawns every node whose dependency counter is zero, and a finishing job spawns each successor whose counter reaches zero (`Scope::spawn` from inside a job, "Nested jobs" below). The scope end is the barrier before the next sync point or main-thread system, and systems that call `par_iter_mut` nest inside it on the same workers with no extra threads. The ECS scheduler spec ("Parallel executor (MVP, #48)") governs where this paragraph and it differ; an earlier version described a per-batch scope. To add a system name to a panic, the executor wraps its closure (`catch_unwind` then `resume_unwind(Box::new(SystemPanic { name, payload }))`); the payload passes through this crate unmodified.

### Panics

- A job panic is caught by `catch_unwind(AssertUnwindSafe(..))` inside the job wrapper, so **workers never die** and a pool survives any number of job panics. The **first** payload recorded in a scope is kept; later payloads of the same scope are dropped and counted (`panics_caught`).
- **No cancellation in v1: after a panic, every job already spawned in that scope still runs.** The scope must wait for all of them anyway (borrow soundness, below), and silently dropping queued jobs would break the scope's own accounting. The ECS executor (#48) adds its own cooperative stop: its job wrapper checks a shared `AtomicBool` first and returns without running its system, so the tasks crate still runs every spawned job (each returns at once) and needs no cancellation API. Wasted work after a panic is acceptable.
- `scope` returns or unwinds only after `pending == 0`. Order of results: if the **scope body** panicked, that payload is resumed (like `std::thread::scope`); otherwise, if a job panicked, `resume_unwind(first job payload)` (original payload and type preserved); otherwise `R`.
- `parallel_for`, `par_chunks[_mut]` and `join` propagate exactly like `scope` (they are built on it). The serial fast path propagates by ordinary unwinding. `join(a, b)` runs `b` on the calling thread and `a` as a spawned job; both always run to completion before a panic propagates.
- **`panic = "abort"` and wasm32:** `wasm32-unknown-unknown` (stable) aborts on panic; unwinding does not exist there, so `catch_unwind` is a no-op and a panic traps the whole module exactly as it would anywhere else on wasm. Native release builds that choose `panic = "abort"` behave the same. Nothing in this crate relies on unwinding for soundness (the abort guard below).

### Nested jobs and re-entrancy

- `scope`/`parallel_for` may be called from inside a job, from a scope body, and from a thread that is not a worker. A job may spawn into its own scope (`s.spawn(|s| ..)`); `pending` is incremented before the parent job's own decrement, so it cannot reach 0 early.
- **Help-first waiting.** A thread waiting in any scope runs queued jobs of **any** scope (own deque first, then injector, then steals), so nesting never blocks a thread that could have made progress.
- **Deadlock freedom.** A thread parks only after a full scan found nothing runnable. At that moment every pending job of its scope is running on some other thread. Induct on nesting depth: a job with no nested wait finishes (jobs are run-to-completion); a job with a nested wait has, by the same argument, a pending set that is running or finishing at strictly smaller depth. A job that queued work is itself a runner: it pops its own deque while waiting or in the worker loop afterwards, so queued work is never stranded. The guarantee is conditional on the **job rule: a job must not block on anything that needs another job to run** (channels, barriers, condvars waiting for a sibling job, a lock held across a `scope` call that its jobs also take, and **a thread-local `RefCell` borrow or other non-reentrant thread-local guard held across a `scope`/`parallel_for` call**: help-first may run another job on the same thread while the waiting frame is still live, and that job can re-borrow it and panic). This holds even with 1 thread, where the inline executor makes violations deterministic hangs in tests rather than rare ones.
- **Latency hazard (documented, accepted):** a thread helping inside scope `S` may pick up a long unrelated job and so finish `S` later than `S`'s own work. Own-deque-first LIFO makes `S`'s freshest jobs the first it picks.
- **Stack depth is bounded by rule, not by hope.** Depth does not come only from structural nesting: a thread waiting in a nested scope pulls the next sibling job (for example the next ready system of a segment) from the injector, and that job nests again, so with more siblings than the limit the stack can grow with the batch size. Rule: every thread keeps a `help_depth` (thread-local): the number of job frames on its stack that were entered by **unrestricted helping**, that is, picked by a waiter's unfiltered scan, as opposed to a job run from the worker loop, a nested `scope` call made by program code (structural nesting), or a restricted pick (below). While `help_depth < HELP_DEPTH_LIMIT` (**16**) a waiter helps freely as described above, and each such pick adds one. At or above the limit the waiter is **restricted**: it runs only jobs whose `Queued.scope` equals its own scope (own deque, injector, then steal scan, matching the tag; this scan is O(queue length) under one lock at a time and happens only in restricted mode) and otherwise parks. A restricted pick does not add to `help_depth`: it runs a job of the scope the waiter itself is waiting on, so the frame corresponds one to one to a level of the program's own scope nesting. Depth is then bounded by `HELP_DEPTH_LIMIT` plus the program's own scope-nesting depth, independent of the number of sibling jobs. With about 16 KiB per level this stays well below the 1 MiB main stack on Windows (the binding limit; workers have 2 MiB). `help_depth <= HELP_DEPTH_LIMIT` therefore holds by construction, and a debug assertion on exactly that catches a bug in the rule. It must **not** be a multiple of the limit or count structural nesting: a legitimate program may nest scopes deeper than 48 levels (stack permitting) and must not trip an assertion. `PoolStats.max_help_depth` (a per-thread high-water mark, updated only when a thread sets a new maximum) exposes it.
- **Restricted mode keeps deadlock freedom.** The argument has three parts and uses the job rule above (jobs run to completion unless they wait on a nested scope).
  1. *A restricted waiter can always finish its own scope.* Every unfinished job of scope `S` is either running on some thread or queued in one of the three places a scan covers: the waiter's own deque, the injector or another worker's deque (steal). The tag-matched scan checks all three, so a queued job of `S` is found and run by the waiter (or by another thread that gets there first). The waiter parks only when a full scan finds none, so at that moment every remaining job of `S` is running on another thread.
  2. *No cycle of mutual waiting.* Say thread `T1` is parked in scope `S1` because a job `J` of `S1` is running on `T2`, and `T2` is blocked in a wait on scope `Sn`, its innermost frame. `S1` was created before `J` was spawned, `J` started after that, and every frame above `J` on `T2`'s stack (a nested scope opened by `J`, or a foreign job `T2` picked up while waiting in it, and the scopes those open) started after `J` did. A scope's waiter creates its scope after the job it sits in started, so `Sn` was created after `S1`. Following "waits for a thread blocked in" edges therefore strictly increases scope creation time, and a cycle `T1 -> T2 -> ... -> T1` would need a scope created before itself. The chain ends at a scope whose jobs are all running and none blocked; by part 1 and the job rule they finish, and the chain unwinds from the newest scope outward.
  3. *Skipped foreign work is not stranded.* A restricted waiter leaves foreign jobs queued. Each such job is run by one of: any thread below the limit (a worker loop or an unrestricted waiter, which scans every queue), its own scope's waiter (the tag matches whatever its depth), or, at the latest, the skipping thread itself once it unwinds to a lower depth, where it is unrestricted again and its worker loop or next wait scans its own deque first. The skipping thread does unwind, because its blocked scope `S` finishes by parts 1 and 2, and by the job rule no job of `S` waits for the skipped job.

  Restricted waiting costs only latency (it may idle while foreign work is queued), never progress.
- A different pool's `scope` called from a job is treated as an external caller of that pool (injector, helps while waiting). Allowed, not recommended.

### Wasm32 and single-thread behaviour

- `platform.rs` is the only file with `cfg(target_arch = "wasm32")`. On wasm32 `available_threads()` returns 1 (it must not call `available_parallelism`), `now_ns()` returns 0 (**`std::time::Instant::now()` panics on `wasm32-unknown-unknown`**, so it is `disallowed-methods` everywhere else), and `spawn_worker` is not compiled. `clippy.toml` `disallowed-methods` enforces this in CI (`cargo clippy ... -D warnings`).
- **Inline backend** (used when `workers == 0`, i.e. `threads == 1` on any target): `Scope::spawn` builds the same wrapper, erases it with the same `erase` and pushes the `'static` `Job` on `Scope.inline: Mutex<VecDeque<Job>>`. **The inline path is not unsafe-free**: `Scope` cannot store `'scope`-bearing closures (dropck, see Data layout), so it needs the same erasure, and I1 to I3 apply to it exactly as to the threaded path (drain before return, abort guard around the drain). After the scope body returns, `scope` drains the queue FIFO on the calling thread (jobs may spawn more; they append). It runs only its own scope's jobs, so nested inline scopes have bounded stacks. Each job runs under the same `catch_unwind`, first-panic and all-jobs-run rules as the threaded path. The `Mutex` is uncontended (a lock costs about 15 ns native, a no-op-ish futex on single-threaded wasm). `thread::park`, `Condvar::wait` and `Instant` are never reached.
- **Deferred, not eager, on purpose:** running the job inside `spawn` would grow the stack by one frame per nested spawn and overflow wasm's small stack on a spawn chain; deferring keeps stack depth bounded. A job may therefore run at any time between `spawn` and the end of `scope` (immediately on another thread, or at the end inline); callers must not depend on either. Inline `parallel_for` degenerates to one serial call (see fast path), so it runs in **ascending index order**.
- The public API is identical on every target, including `Send` bounds (so wasm cannot compile code that native rejects), and **the whole test suite runs under `ThreadBudget::single()`** on native as the proxy for wasm (there is no wasm test runner in CI; `cargo xtask ci wasm` is the build check).
- **Web threads later:** a `wasm32` + atomics backend would replace `platform.rs` and `Shared` behind the same `TaskPool`; callers do not change. ThreadBudget on web stays 1 until then.

### Unsafe: the designated module

`crates/exitium_tasks/src/scope.rs`, marked `#[allow(unsafe_code)]` (the rest of the crate keeps the workspace `deny`). It contains **one `unsafe fn` and four `unsafe` blocks** (about 25 lines in all), but the module is deliberately larger (about 200 lines): it also holds the safe-looking code on which that `unsafe` depends (wrapper, `pending` protocol, wait region, abort guard, `Scope` variance), so that **soundness cannot be changed without touching the one module every reviewer audits as high-risk**. The whole module is the audit surface, not just the transmute. Unsafe inventory (CLAUDE.md), this module only, each with its own `// SAFETY:` comment:

| # | Operation | Where | Discharged by |
|---|---|---|---|
| U1 | `unsafe fn erase`: the lifetime transmute of a boxed job | `erase` (the `unsafe` block inside it is the one the 2024 edition requires) | I1, I2, I3, I4 |
| U2 | The call `erase(..)` | `Scope::spawn` | `pending` is already incremented and `scope` waits for it (I2, I3) |
| U3 | `MaybeUninit::assume_init` on `f` | inside the wrapper's inner closure | `f` is initialised in `spawn` before the wrapper is built and read exactly once, because a job is a `FnOnce` run once |
| U4 | Dereferencing the raw scope pointer (`&*ptr`) | inside the wrapper's inner closure, immediately before the call to `f` | `pending > 0` until after the wrapper's decrement, and `scope` does not return before that, so the `Scope` on the waiter's stack is alive for the whole call to `f` (I2, I5) |

U1 is the only `unsafe fn`; U2 to U4 are blocks. No other `unsafe` exists in the crate, and **there is still no `unsafe impl Send/Sync` anywhere**. The module is found by `xtask ci miri` (it greps `allow(unsafe_code)` and runs `cargo +nightly miri test -p exitium_tasks`). Every PR that touches it gets the reviewer's high-risk scrutiny (ADR-0005), and the Miri job must pass.

```rust
/// Erases the `'scope` lifetime of a job so it can sit in a `'static` queue.
///
/// # Safety
/// The caller guarantees the returned job is run to completion (consuming and dropping the
/// closure and everything it captured) before `'scope` ends. See the invariants table in the spec.
unsafe fn erase<'scope>(job: Box<dyn FnOnce() + Send + 'scope>) -> Job {
    // SAFETY: same layout; only the lifetime bound of the trait object changes. Soundness is the caller's contract.
    unsafe { mem::transmute::<Box<dyn FnOnce() + Send + 'scope>, Job>(job) }
}
```

`erase` is **private** to `scope.rs` and has a single call site, `Scope::spawn`, in the same module, which also builds the wrapper whose shape I1 and I5 fix. Nothing outside the module can call it. **Why a persistent pool needs this at all:** a worker thread outlives any one scope, so a job that borrows the caller's stack must be stored somewhere whose type says `'static`. There is no safe way to do that without cloning or `Arc`-ing everything (unusable for ECS column slices); `rayon` and `crossbeam` use the same mechanism upstream. It applies to the inline queue too (see Data layout).

**Job wrapper shape (fixed).** The text below is what the `scope.rs` module doc carries; I1 and I5 refer to it. It is a structure, not code to copy, and it replaces the earlier "move `f` into an inner closure called by value, hold `Arc<ScopeShared>`, not `&Scope`" wording, which is **not** Miri-clean under Stacked Borrows.

> The wrapper built by `Scope::spawn` captures **neither `&Scope` nor `f` as retagged reference fields**. Its entire state is three values: `state: Arc<ScopeShared>`, `scope: AtomicPtr<Scope<'scope, 'env>>` (the address of the `Scope` on the waiter's stack, taken from the `&'scope Scope` that `spawn` was called on) and `f: MaybeUninit<F>`. Running it:
>
> 1. Call an inner closure through `catch_unwind`. The inner closure takes the `MaybeUninit<F>` and the `AtomicPtr` and does, in this order: `f.assume_init()` (U3), then `&*scope.load(Relaxed)` (U4), then `f(scope_ref)`. The references handed to user code, `&Scope` and any `&`/`&mut` borrowed by `f`, therefore **exist only for the duration of that one call**.
> 2. After `catch_unwind` returns, the epilogue **touches only `state`**: store the first panic payload, `pending.fetch_sub(1, AcqRel)`, and `waiter.unpark()` if it was the last job. The raw pointer is never dereferenced again.

*Why the obvious shape is unsound.* A by-value function argument has its reference-typed fields retagged and **protected for the whole call**, including the epilogue after the final `fetch_sub`. If the wrapper held `&'scope Scope`, or a closure `f` containing a `&mut x`, as a field, the protector would still be live when the waiter observed `pending == 0`, returned from `scope`, dropped the `Scope` (`drop_glue::<Scope>`) or read `x`. Stacked Borrows reports that as undefined behaviour even though no byte is actually accessed late. `AtomicPtr<T>` and `MaybeUninit<T>` hold no retagged reference, so the wrapper has none until the inner closure creates them, and they end with the call to `f`, before the decrement. This is the same device `std::thread::scope` uses (its user closure is kept in `MaybeDangling`). Consequences to keep:

- `AtomicPtr<Scope>` is `Send + Sync` for any `Scope`, so the compiler no longer proves that a `&Scope` may be used on the worker. That requirement (`Scope: Sync`) is enforced by a compile-time assertion in `scope.rs` (`fn assert_sync<T: Sync>()` instantiated at `Scope<'static, 'static>`), and `F: Send` is still required by `spawn`'s bound, which makes `MaybeUninit<F>` `Send`. No `unsafe impl` is needed.
- The pointer is created from a shared reference and only ever reborrowed as `&*`, never written through, so no `&mut Scope` is derived from it.
- `MaybeUninit<F>` never drops `F`. A wrapper dropped without running would leak `f`, not corrupt memory, and I2 already forbids that case.
- A throwaway crate checked this shape while amending the spec: the wrapper above passed `cargo +nightly miri test` under `-Zmiri-many-seeds=0..40`, while the two variants (wrapper capturing `&'scope Scope`; wrapper capturing `f` as a field) failed with the Stacked Borrows protector errors the round-2 review of PR #75 reported.

**Invariants (each must hold for the transmute to be sound):**

| # | Invariant | Upheld by | Checked by |
|---|---|---|---|
| I1 | A job's closure **and all its captures are consumed and dropped before the job decrements `pending`**. The wrapper keeps `f` in a `MaybeUninit<F>` and `assume_init`s it **inside** the inner closure that `catch_unwind` calls, where `f(scope)` consumes it, so captures drop at the end of that call, on normal exit or unwind; the decrement is the next statement after `catch_unwind` returns. `f` is never a retagged field of the wrapper (wrapper shape above). A `Drop` that panics while unwinding aborts the process (never an unsound return). | `Scope::spawn` wrapper shape in `scope.rs` (fixed text in the module doc) | Miri test: drop-counting captures, counted before `scope` returns, on the threaded **and** the inline backend; panic in `Drop` is not tested (abort) |
| I2 | `pending` is incremented **before** the job is enqueued (either queue), and `scope` does not return or unwind until `pending.load(Acquire) == 0`. A queued job is only ever removed by running it, so no job is dropped unrun (which would hang, not break soundness). | `spawn` ordering; `scope` structure, both in `scope.rs` | Unit test with a barrier job; Miri nested-spawn test on both backends |
| I3 | Nothing can unwind out of `scope` between the first possible `spawn` and `pending == 0`. The guard is created **before** the body runs and is `mem::forget`-ed right after the wait completes; its `Drop` **unconditionally calls `process::abort`** (it does not look at `thread::panicking()`, which would also be true when `scope` is legitimately called from a `Drop` during an unrelated unwind, for example a resource guard dropped by a system panic). So the guard only ever runs if something unwinds out of the wait region, where it aborts instead of letting `scope` return early. A panicking scope **body** is caught first, then waited, then resumed. Applies to the inline drain as well. | `scope.rs` structure + guard | Tests: panic in body with live jobs, jobs still finish and borrowed data stays valid (Miri); **`scope`/`parallel_for` called from a `Drop` during `catch_unwind` of an unrelated panic returns normally and runs its jobs** (asserts `thread::panicking()` inside to prove the premise), on both backends; fault-injection test (a `cfg(test)` hook that panics inside the wait region, run in a child process, not under Miri) expects abnormal termination, never a normal return |
| I4 | `'scope` cannot be shortened or smuggled out, and `Scope` owns no `'scope`-bearing value (the dropck point above). `Scope<'scope, 'env>` is **invariant** over `'scope` and `'env` (the `PhantomData<&'scope mut &'scope ()>` pattern used by `std::thread::scope`), `spawn` requires `F: 'scope`, and `scope`'s closure is `for<'scope> FnOnce(&'scope Scope<'scope, 'env>)`, so `'scope` is a lifetime that ends inside the call and `'env: 'scope` means everything borrowed outlives the call. | Type definitions in `scope.rs` | `compile_fail` doctests: borrow a local that dies before the scope ends; return a reference to a spawned local; store the `Scope` outside; capture a `Rc` (not `Send`) |
| I5 | A job neither touches the `Scope` (which lives on the waiter's stack) **nor holds any reference derived from it or from `f`'s borrows after its decrement, nor across it as a retagged field**. The wrapper captures the scope only as `AtomicPtr<Scope>`, dereferenced solely inside the call to `f`; the references handed to user code (`&Scope`, and any `&`/`&mut` borrowed by `f`) exist only for the duration of that call. The epilogue touches only the wrapper's `Arc<ScopeShared>` clone (`'static` data: counter, panic slot, `Thread` handle), which keeps the counter and the waiter handle alive past the last decrement and the `unpark`. | Wrapper shape above: `Arc<ScopeShared>` + `AtomicPtr<Scope>` + `MaybeUninit<F>`, no `&Scope` field (in `scope.rs`) | Miri with `-Zmiri-many-seeds`: the many-small-scopes test below (a borrowed `&mut` read after `scope`, `Scope` dropped right after) and the nested-scope test; a use-after-free **and** a protector violation are both reported |
| I6 | **Happens-before:** all writes a job made are visible to the thread that returns from `scope`. `fetch_sub(1, AcqRel)` in the job, `load(Acquire)` in the waiter (RMW release sequence across all jobs). | Atomic orderings fixed in the wrapper and `scope` (`scope.rs`) | Miri data-race detector on stack data mutated by jobs then read after `scope` |
| I7 | Workers outlive every scope that uses them: they exit only after `shutdown` is set, which only `TaskPool::drop` does, and every scope entry point holds `&TaskPool` or an `Arc<TaskPool>` for its duration (default-pool lookups clone the `Arc`; a job that calls a free function runs inside a live outer scope). | Ownership | Test: drop with idle workers returns within 100 ms; start/stop 100x. (Dropping with a scope live cannot be written: `scope` borrows the pool.) |

**Aliasing argument (no `&mut` from shared refs, no aliasing `&mut` across tasks).** The erase function creates no reference of any kind; it re-labels one lifetime. The wrapper creates exactly one reference to the scope (`&*ptr`, shared, U4) and one to user data (the borrows inside `f`, moved in by `assume_init`), both only for the call to `f`. The pool never hands out `&mut` to user data. Exclusive access exists only through ordinary borrow-checked captures: a `&mut X` captured by a spawned closure is **moved** into that single job (the borrow checker rejects a second capture while the first exists, since both are `'scope` borrows of the same local); shared data is `&X` (`X: Sync`) with interior mutability the user chose. `parallel_for` shares `&F`; `par_chunks_mut` hands each job the `&mut [T]` produced by `split_at_mut`, which are disjoint by the standard library's own proof. `Scope`, `Shared` and `ScopeShared` are `Send + Sync` by auto-trait derivation (atomics, `Mutex`, `Thread`, `&Shared`), and the wrapper is `Send` by derivation too (`Arc<ScopeShared>`, `AtomicPtr<Scope>`, `MaybeUninit<F>` with `F: Send`); **there is no `unsafe impl Send/Sync` anywhere in this crate**, so no thread-safety claim rests on a comment.

**Miri test plan** (all tests are Miri-clean and run in the normal `cargo test` as well; iteration counts shrink under `cfg(miri)`; the Miri job runs with `MIRIFLAGS=-Zmiri-many-seeds=0..16` to explore thread schedules; the current `xtask ci miri` passes no `MIRIFLAGS`, so #27 includes that small `xtask/` change, a CI-config change the reviewer treats as high-risk). **The first test below is required to pass for #27 to merge**, under `-Zmiri-many-seeds=0..16` in CI, and the #27 PR body records one run over `0..40` (the round-2 review of PR #75 used 40 seeds):

- **Many small scopes, the shape that caught the round-2 wrapper bug (I1, I5, required):** a loop of several hundred iterations (fewer under `cfg(miri)`, but at least 30), each with a fresh local `let mut x`, one `scope` that spawns one job borrowing `&mut x` (and, in a second variant, a job that also spawns a nested job into its own scope), then **reads `x` right after `scope` returns** while the `Scope` is dropped immediately, on 2 and 4 threads and on `ThreadBudget::single()`. A wrapper that holds `&Scope` or `f` as a retagged field fails this test with a Stacked Borrows protector error ("strongly protected ... function argument", in `drop_glue::<Scope>` or at the read of `x`); the raw-pointer + `MaybeUninit` wrapper passes.
- Stack data borrowed by jobs, mutated through `&mut` moved into a job and through atomics, read after `scope` (I6, I4 at runtime).
- Drop-counting captures: exactly once each, and counted **before** `scope` returns (I1), on both backends.
- `scope` inside a `Drop` during an unrelated unwind returns normally (I3), on both backends.
- Panic in a job; panic in the body with live jobs; panic in both; panic in the first of many jobs; payload identity and downcast; pool still usable afterwards (I3).
- Nested `scope` three levels deep and a job spawning into its own scope (I2, I5), on 1, 2 and 4 threads; recursive `join` fib(12).
- Empty scope; 10k scopes with 1 job each (use-after-free hunting for I5); `par_chunks_mut` writing every element once.
- The same cases on `ThreadBudget::single()`. The inline backend is erased-job code under I1 to I3 like the threaded one, so Miri runs it in full (and it is fast there: no threads).
- Pool create/drop with and without jobs (no leaked threads: Miri's leak check is on).
- `compile_fail` doctests for I4 (rustdoc, not Miri).

### Dependencies

None added. The `rayon` / `crossbeam-deque` choice is recorded above with explicit switch triggers; any later adoption is its own PR with the dependency justified in the body (license, wasm support, why not std) and extra reviewer scrutiny (ADR-0005): `[workspace.dependencies]` once, `deny.toml` already allows their licenses.

### Implementation slices (existing issues)

| Issue | Delivers from this spec |
|---|---|
| #26 | `budget.rs`, `platform.rs`, `stats.rs` counters, `pool.rs` lifecycle (workers that scan, spin, park, shut down), `clippy.toml`, the `lib.rs` doc update |
| #27 | `queue.rs`, `scope.rs` **including the inline backend** (it is `scope` with zero workers, needed as soon as `threads == 1`; same erasure, same invariants), the restricted-helping rule, the sleep protocol, panic collection, Miri suite, and the `xtask ci miri` `MIRIFLAGS=-Zmiri-many-seeds=0..16` change. High-risk areas for the reviewer: the designated unsafe module, `xtask/` |
| #28 | `par.rs`: `Chunking`, `parallel_for`, `par_chunks[_mut]`, `join`, free functions and default-pool plumbing; safe code only |
| #29 | wasm32 build verification, `platform.rs` wasm shims proven by `cargo xtask ci wasm`, running the whole suite under `single()`, the `disallowed-methods` check. Little new code, because the inline backend ships in #27 |
| #30 | `benches/pool.rs` with the budgets below, including the physical-core rule for scaling benches |

After this spec merges, `needs-spec` is removed from #26 and #27 (the only open `needs-spec` issues in `area:tasks`; #48 also waits on the scheduler spec #22 and keeps its label until that lands). #29's body should be edited to note that the inline backend lands in #27.

## MCP / editor surface

Agent parity (rule 1), reflection as the source of truth (rule 2), mutation through the command bus (rule 3). Tool names are snake_case `<domain>_<verb>`, domain `tasks`. No `exitium_mcp` code is written in Phase 1; this fixes the API it will be built from, and the table is the contract Phase 4 implements.

**Reflection.** `exitium_tasks` stays free of the reflect crate by default. With the optional `reflect` feature a `cfg_attr(feature = "reflect", derive(Reflect))` is the only macro in the public types, applied to `PoolStats`, `WorkerStats`, `PoolMode`, `BudgetSource`, `BudgetOverride`, `ResolvedBudget` and `ThreadBudget` (all `u32`/`u64`/`String`/`Vec`/`Option`/enums: `usize` is not reflectable, per the reflection spec, so no `usize` appears in any of them). **The edge and who enables the feature (settled against roadmap R3 and C3).** The `reflect` feature needs an optional `exitium_reflect` dependency in `crates/exitium_tasks/Cargo.toml`, which is a workspace edge `tasks -> reflect`. R3 lets the reflect group, tasks and profiler lanes run **together in wave 1**, and an edge from tasks to reflect would stop that, so by C5 it is **not** in intake 1 but an **edge PR**: one crate, `Edits: crates/exitium_tasks/`, `Hotspots: crates/exitium_tasks/Cargo.toml, Cargo.lock`, `New crate edges: exitium_tasks -> exitium_reflect`, `Blocked by` the reflect group (#31 to #37) and #26 to #29, run at the Stage 1 tail beside no other manifest edit (the two tail edge PRs, #65 and the profiler `-> reflect` PR, share `Cargo.lock`, so the three run one at a time). It adds the `cfg_attr` derives and a reflection round-trip test. Until it lands the tasks types are plain Rust, and nothing reads them reflectively. The feature is **enabled by the crates that register the tasks commands and settings**: the facade glue (it embeds `ThreadSettings` in project settings and installs the pool) and `exitium_editor` (settings panel). It is **not** enabled by `exitium_mcp`: C3 gives `mcp` only a `command` edge, and `mcp` is a separate process, so it sees these types through the command registry's reflected schemas. `ThreadSettings` (defined in `budget.rs`) is derived the same way, with the `range` attribute (1 to 128) on `threads`; `exitium_app` embeds it in project settings, so the settings UI and the MCP schema are generated, never hand-written.

```rust
pub struct PoolStats {
    pub mode: PoolMode,              // Threaded | Inline
    pub pool_id: u32,                // process-unique id of this pool instance (changes on a SetThreadBudget rebuild)
    pub threads: u32, pub workers: u32, pub reserved: u32,
    pub available: u32, pub source: BudgetSource,   // copied from `TaskPool::resolved()`
    pub uptime_ns: u64,
    pub scopes: u64, pub jobs_spawned: u64, pub jobs_executed: u64, pub jobs_stolen: u64,
    pub parallel_fors: u64, pub serial_fast_paths: u64, pub panics_caught: u64,
    pub max_queue_depth: u32,        // high-water mark of any one deque since start
    pub max_help_depth: u32,         // high-water mark of any thread's help_depth: frames entered by unrestricted helping only, never structural scope nesting; <= HELP_DEPTH_LIMIT by construction (see Nested jobs)
    pub workers_detail: Vec<WorkerStats>,
}
pub struct WorkerStats { pub index: u32, pub jobs_executed: u64, pub steals: u64, pub busy_ns: u64, pub parked_ns: u64 }
```

Counters are per-worker, 64-byte padded `AtomicU64`s bumped with `Relaxed`; `busy_ns`/`parked_ns` are measured per **busy/idle period** (two clock reads each, never per job) and are 0 on wasm32. A snapshot sums and reads without stopping workers (so it is momentary, not transactional). Counters are monotonic for the life of **one pool** and never reset. A `SetThreadBudget` rebuild creates a new pool with fresh counters, so clients diff two snapshots only when `pool_id` is equal; a different `pool_id` (and usually a smaller `uptime_ns`) means a new pool and the diff is meaningless.

| Capability | MCP tool (in `exitium_mcp`) | Command | Editor | Built from |
|---|---|---|---|---|
| Read pool counters and per-worker utilisation | `tasks_get_stats` | none (read) | Profiler panel, "Threads" tab: mode badge, per-worker busy %, steals, queue high-water | `TaskPool::stats() -> PoolStats` |
| Read the resolved budget (threads, workers, reserved, available, source, env override, `env_warning`, min-spec value 4) | `tasks_get_thread_budget` | none (read) | Settings, "Threads" section (read-only line showing the effective value and why) | `ResolvedBudget`, `ThreadBudget::min_spec()` |
| Change the thread budget setting | `tasks_set_thread_budget` | `SetThreadBudget { threads: Option<u32>, reserved: Option<u32> }` | Settings, "Threads" field (inspector widget generated from `ThreadSettings`) | `ThreadSettings`, `ThreadBudget::resolve`, `BudgetOverride` |

`SetThreadBudget` semantics: it updates the persisted **setting**, resolves a new budget, and **rebuilds the pool at the next sync point** (between schedule runs, when no scope is live; it never resizes a live pool): `TaskPool::from_resolved(resolved)` with the `ResolvedBudget` it just computed (so `tasks_get_thread_budget` keeps reporting `Settings`, `Env` or `Platform` as `source`; `TaskPool::new` would force `Explicit` and is for tests and benches), `set_default(Arc::new(pool))`, and the old pool is joined when its last `Arc` drops. The returned record is `{ requested, effective, overridden_by: BudgetOverride }` with `enum BudgetOverride { None, Env, Platform }`. If `EXITIUM_THREADS` is set and valid it **wins** (`overridden_by = Env`); on `wasm32` the platform forces one thread (`overridden_by = Platform`, `effective = 1` whatever was requested; the setting is still persisted so a native run of the same project honours it). `effective != requested` therefore always comes with a non-`None` reason, so an agent is never silently told it succeeded. If the effective thread count is unchanged the pool is not rebuilt. Out-of-range input (`threads == 0` or `> 128`) returns `invalid_thread_count` and changes nothing. Undo restores the previous `ThreadSettings`. MCP and editor threads never run jobs on the pool; they read `stats()` from any thread (it is `&self`, lock-free counters) and send commands to the bus, which applies them on the main thread. MCP `build`/`test`/`bench` tools (Phase 4) take an optional `threads` argument that sets `EXITIUM_THREADS` on the child process; `exitium bench --min-spec` and `xtask bench-ci --min-spec` (#66) do the same with `4`.

## Performance

Budgets are for **release profile, `lto = "thin"`**, on the reference dev PC unless stated, medians from criterion (`benches/pool.rs`, #30, under the harness from #18, run through `cargo xtask bench-ci` on the 5% gate; the min-spec profile runs the same benches with `EXITIUM_THREADS=4`, #66). A pool is built once outside the timed region. **The ratios against a plain serial loop are the binding contract** (hardware-independent). **Absolute figures are provisional ceilings**: the first bench PR (#30) re-baselines them with the `perf` agent; a missed ceiling files a perf issue in #30 rather than widening its scope, except where the switch trigger above applies. "Warm" means iterations back to back (workers still spinning); "cold" means the pool was idle for 5 ms (workers parked).

| Benchmark | Budget | Kind |
|---|---|---|
| Empty `scope` (no jobs spawned) | <= 60 ns | ceiling |
| `scope` + 1 empty job, 4 threads, warm | <= 2 us | ceiling |
| `scope` + 1 empty job, 4 threads, cold | <= 12 us (the caller usually runs the job itself before a woken worker arrives; OS wake latency of 10 to 50 us is off the critical path) | ceiling, reported not gated by 5% (OS noise) |
| `scope` + 16 empty jobs (a typical batch fan-out), 4 threads, warm | <= 8 us (<= 500 ns per job) | ceiling, **job-rate switch trigger** (2x = 16 us) |
| 100,000 empty jobs spawned from one thread, 4 threads | <= 40 ms (<= 400 ns per job, >= 2.5 M jobs/s) | ceiling, **job-rate switch trigger** (2x = 80 ms) |
| 100,000 empty jobs, `single()` (inline) | <= 6 ms (<= 60 ns per job) | ceiling |
| `scope` + 1 empty job, inline | <= 100 ns | ceiling |
| `parallel_for` over 1M `f32`, compute-bound body (100 dependent `mul_add`, about 100 ns per item, about 100 ms serial), 4 threads | **>= 3.2x** vs a plain serial loop (80% efficiency); the issue's floor is 3x. A miss files a perf issue (chunking, SMT, bandwidth); it is **not** a switch trigger | **ratio, binding** |
| same body, 2 threads | >= 1.9x | ratio, binding |
| same body, 1 thread (`single()`) | >= 0.97x of a plain loop (<= 3% overhead: the serial fast path) | ratio, binding |
| same body, all threads | reported only (no claim past 4 threads) | informational |
| `parallel_for` over 1M `f32`, trivial body (`x * 2 + 1`, memory-bound), 4 threads | <= 1.10x the serial time (must not regress; no scaling claimed because it is bandwidth bound) | ratio, binding |
| `TaskPool::parallel_for` with `len <= chunk_len` (serial fast path) vs calling `f(range)` directly | <= +20 ns | ceiling |
| free `parallel_for` on the same input (adds the default-pool lookup: TLS hit on workers, about 20 ns `RwLock` read elsewhere) | <= +50 ns | ceiling |
| `Chunking::chunk_len` | <= 10 ns | ceiling |
| Pool create + drop, 4 threads | <= 2 ms; 100 cycles under 1 s in tests | ceiling |
| Idle pool CPU | workers parked >= 95% of a 1 s idle window (`parked_ns` over `uptime_ns`); no spinning past `SPIN_NS = 20 us` | measured by a test, not criterion |
| Stats overhead | <= 2% on the 16-job fan-out benchmark, measured once in #30 by temporarily stubbing the counters | one-off check |

**Skip rule for the scaling benches.** std reports only logical cores, so the bench computes `physical = $EXITIUM_BENCH_PHYSICAL_CORES` if set, else `ThreadBudget::detect().available / 2` (conservative: assume SMT). It asserts a scaling ratio for `n` threads only when `physical >= n`; otherwise it prints the measured ratio and skips the assertion (the 5% regression gate on absolute time still applies). The reference dev PC and the CI runners set `EXITIUM_BENCH_PHYSICAL_CORES` in their runner config (`xtask bench-ci`, #66, passes it through), so a 4-core/8-thread runner asserts 4-thread scaling while a 2-core/4-thread runner skips it instead of failing. Memory: pool heap <= 64 KiB idle, per queued job <= 64 bytes of closure + 24 bytes in a deque, worker stacks 2 MiB virtual each (min-spec: 3 workers = 6 MiB virtual, tens of KiB resident).

Complexity: `spawn` O(1) amortised, a steal scan is O(workers) (workers <= 127), `scope` wait is O(pending) in helping work and O(1) when parked, `parallel_for` creates `ceil(len / chunk_len) - 1` jobs and O(log chunks) stack.

## Threading model

| Thread | Role | Notes |
|---|---|---|
| Main / caller (owns `App`, runs the schedule) | Calls `scope`/`parallel_for`; **helps** run jobs while it waits | Counted in `ThreadBudget::threads`. Its jobs go to the `injector` |
| `exitium-worker-0 .. N-2` | Run jobs: own deque (LIFO), injector (FIFO), steal (FIFO) | `threads - 1` of them; named for Tracy lanes (#65); `current_worker_index() = Some(i)` |
| Other threads (MCP server, editor UI, asset I/O) | **Not** pool workers. They may call `scope` (they use the injector and help while waiting) but must not run ECS queries; world access is main-thread only and MCP requests are served at sync points (ECS spec) | `TaskPool: Send + Sync` |

- **Sharing:** `TaskPool` and `Shared` are `Send + Sync` through auto traits only. Job closures must be `Send`; closures used by `parallel_for` and `par_chunks[_mut]` must be `Sync` (shared by reference).
- **Locks:** queue locks and the injector lock are held for one push or pop, never together, never around user code or around `sleep`'s lock. The sleep lock is held only to read/modify `sleepers` and to wait. No lock order exists because no two are ever nested.
- **Orderings:** `pending` `AcqRel`/`Acquire`; `epoch` and `sleepers` `SeqCst` (Dekker-style pair); `shutdown` `Release`/`Acquire`; stats `Relaxed`.
- **Fairness:** injector is FIFO and scopes from several external threads interleave in arrival order; there is no priority and no per-scope fairness beyond that.
- **Interaction with the ECS:** structural world mutation happens only at sync points when no scope is live (ECS spec). The job system never touches the world; ECS holds the access-conflict and `UnsafeWorldCell` proofs.
- **Correctness never depends on timing:** spin and park durations affect performance only.

## Test plan

- **Budget (unit, pure):** table-driven `resolve_with` over `env in {None, "4", " 4 ", "0", "-1", "abc", "", "129", "9999999999999"}` x `settings in {None, threads, reserved}` x `available in {1, 2, 4, 6, 8, 12, 16, 64}`: source, clamping, the reserve table, env beats settings, oversubscription accepted, warning text present exactly for invalid values; wasm32 resolution (`cfg` test on a pure helper) is `1/Platform`. `min_spec()`/`single()`/`fixed()` clamps.
- **`Chunking` (unit):** `chunk_len` formula against the examples above; for random `len` (0, 1, chunk-1, chunk, chunk+1, random up to 1e6 from a seeded xorshift, no `proptest` dependency), `threads in 1..=16` and `Fixed`/`Auto`: leaf ranges partition `range` exactly, ascending boundaries at `start + k * chunk_len`, none empty, count `= ceil(len / chunk_len)`, deterministic across runs.
- **`parallel_for` and `par_chunks[_mut]` vs serial:** result equals the serial loop; every index visited exactly once (counter array); `par_chunks_mut` offsets correct and writes disjoint; empty and single-element inputs; serial fast path taken (`serial_fast_paths` counter) when it should be.
- **Scope semantics:** borrow stack data and read after; nested scope depth 4; job spawning into its own scope; recursive `join` fib(20) on 1, 2 and 4 threads (deadlock test: a 30 s watchdog fails the test, it does not hang CI); a long unrelated job does not deadlock a nested scope; spawn from non-worker threads concurrently (4 external threads, 1000 scopes each).
- **Panics and the abort guard:** `scope` called from a `Drop` during an unrelated unwind exits normally; the fault-injection child-process test aborts (I3). Job panic propagates with the original payload after **all** jobs ran (counter proves it); body panic wins over job panic; multiple job panics keep the first; pool still works afterwards on every thread count; `parallel_for` panic in chunk k still runs the others.
- **Lifecycle:** create/drop 100x with and without work; drop while workers are asleep returns in <= 100 ms; drop on a worker thread does not hang; stats counters add up (`jobs_spawned == jobs_executed` at quiescence; `sum(workers) <= jobs_executed`).
- **Help depth:** a batch of 1000 sibling jobs, each opening a nested scope with a few jobs, on 1, 2 and 4 threads: `max_help_depth <= HELP_DEPTH_LIMIT` (it counts only unrestricted helping, so the program's nesting depth is not added) and no deadlock (also with every thread restricted by an artificially low limit under `cfg(test)`). A second case nests scopes 64 levels deep (more than `3 * HELP_DEPTH_LIMIT`) on a test thread with an 8 MiB stack and must not trip the debug assertion, with `max_help_depth <= HELP_DEPTH_LIMIT` still holding.
- **Stress:** 1M tiny jobs on 4 threads completes with the exact count; lost-wakeup hunt: 100k iterations of `scope` + 1 job with random 0 to 200 us idle gaps; 1000x repeated heavy disjoint-chunk mutation checking the final data (the ECS #48 shape), all under a watchdog.
- **Budget matrix:** one macro generates every runtime test for `ThreadBudget::single()`, `fixed(2)`, `min_spec()` and `fixed(8)` (oversubscribed on small machines), so the inline backend (the wasm proxy) and the threaded backend run the same assertions.
- **Miri:** plan above; `cargo xtask ci miri` picks the crate up automatically because `scope.rs` contains `allow(unsafe_code)`.
- **wasm:** `cargo build --workspace --target wasm32-unknown-unknown` (CI `wasm` step) plus the `clippy.toml` disallowed-methods check; no wasm runtime tests in Phase 1 (open question 12).
- **`compile_fail` doctests** for I4 and for `parallel_for` rejecting a non-`Sync` closure.
- **Benchmarks:** per the Performance table (#30).
- **MCP (Phase 4, not here):** `tasks_get_stats`/`tasks_get_thread_budget` schemas generated from the `Reflect` derive; `SetThreadBudget` round trip with undo; `overridden_by` paths (`Env`, and `Platform` via a pure-helper cfg test).

## Open questions

1. **Default pool.** Free functions resolve worker TLS, then a process-wide default pool, then an inline pool (needed because `par_iter_mut` takes no pool argument). Confirm this global default, or have the ECS take the pool from a `World` resource and drop the free functions. (Recommendation: keep both, as specified.)
2. **Reserve table and a render thread.** Default `reserved` is 0/1/2 for <=4 / <=8 / more cores and assumes render submission runs on the main thread in Phase 1. If render gets its own thread later, min-spec (4 cores) would have 4 pool threads + render on 4 cores. Decide in the render spec.
3. **Help depth.** v1 bounds help-first stack depth with restricted helping past `HELP_DEPTH_LIMIT = 16` (Nested jobs). The limit is a guess; #30 and the ECS executor (#48) tune it from `max_help_depth`. A smarter "leapfrog" rule (steal only from whoever holds your join) is not needed unless restricted waiters show real idle time.
4. **Cancellation on panic.** v1 runs every spawned job after a panic, and the ECS executor skips its own un-started systems through its own flag (see Panics). If a second consumer wants fail-fast, add a `Scope::is_poisoned()` check (cooperative) rather than silently dropping queued jobs.
5. **Queue implementation.** Mutex deques (B) vs `crossbeam-deque` (C): the job-rate ceilings in the decision section settle it from #30's numbers; no decision is needed now.
6. **Parked waiters do not steal.** Sharing the workers' sleep protocol with waiting threads would recover tail parallelism but costs a `notify_all` per scope end. Revisit only if #30 shows a tail inefficiency on >= 8 threads.
7. **Physical cores and SMT.** `available_parallelism()` counts logical threads; a 2-core/4-thread laptop gets `threads = 4` and will not reach the 3.2x scaling target. The benches use `EXITIUM_BENCH_PHYSICAL_CORES` (Performance) so CI is not affected; a runtime physical-core probe (not in std) is a later decision for the default budget itself.
8. **Detached `'static` jobs.** Asset import, save compression and autosave want fire-and-forget background work with lower priority than frame jobs, probably a second small pool so they cannot starve a frame. Specified when `exitium_assets`/`exitium_save` need it.
9. **`parallel_fold`/reduce and sort.** A safe `parallel_fold(range, chunking, identity, fold, combine)` on `join` is the obvious next helper; out of scope until a consumer exists.
10. **`panic = "abort"` in shipped games.** If the game profile uses it, job panics terminate the process instead of propagating; confirm this is acceptable for the first game (affects ECS error handling, not this crate's soundness).
11. **Extra assurance.** `loom` (model checking of the sleep protocol) and ThreadSanitizer need a new dev-dependency / a nightly CI job, both high-risk changes (dependency justification in the PR body, and a CI change the owner pushes if it touches `.github/workflows/`). v1 relies on stress tests plus Miri `-Zmiri-many-seeds` (wired in by #27's `xtask` change).
12. **Wasm runtime tests and web threads.** A `wasm-bindgen-test` runner in CI, and the Phase 6 SharedArrayBuffer backend (nightly `build-std`, cross-origin isolation), are decided with the web-pipeline spec.
