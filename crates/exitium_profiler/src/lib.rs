//! Tracing, Tracy integration and frame statistics.
//!
//! This crate is the engine's single instrumentation API. Other crates mark
//! work with [`profile_scope!`] and never name `tracing`, Tracy or the
//! in-editor profiler directly, so those backends can change without touching
//! instrumented code.
//!
//! # The `profiling` feature
//!
//! Off by default. With it off, [`profile_scope!`] expands to nothing and
//! `tracing` is not compiled, so instrumentation costs nothing in shipped
//! builds (`benches/scope.rs` measures this). With it on, each scope is a
//! `tracing` span at level `INFO` with target `exitium`, which any `tracing`
//! subscriber (Tracy, the editor's profiler panel, a test collector) can
//! record. Cargo unifies features, so one crate in the build enabling
//! `exitium_profiler/profiling` turns spans on everywhere.
//!
//! # Span naming convention
//!
//! Name a span `crate::system`: the crate that owns the work, then the system
//! or function inside it, as a Rust path.
//!
//! ```
//! use exitium_profiler::profile_scope;
//!
//! fn tick_physics() {
//!     profile_scope!("exitium_physics::integrate");
//!     // ...
//! }
//! # tick_physics();
//! ```
//!
//! - Use the crate's real name (`exitium_ecs`, not `ecs`) so a span leads to
//!   its source.
//! - Add deeper segments only for distinct phases of one system, e.g.
//!   `exitium_render::sprites::sort` and `exitium_render::sprites::upload`.
//! - Names are string literals, fixed at compile time. Put per-call data
//!   (entity counts, asset paths) in logs, not in span names, or the profiler
//!   shows one row per value.

/// `true` when the crate was built with the `profiling` feature, i.e. when
/// [`profile_scope!`] records spans.
pub const ENABLED: bool = cfg!(feature = "profiling");

/// Re-exports for the macro expansion. Not public API.
#[cfg(feature = "profiling")]
#[doc(hidden)]
pub mod __private {
    pub use tracing;
}

/// Profiles the rest of the enclosing block as a span named `$name`.
///
/// The span is entered where the macro is written and exited when the block
/// ends, including on early `return`, `?` and panics. `$name` must be a string
/// literal following the `crate::system` naming convention (see the crate
/// docs). The span's level (`INFO`) and target (`exitium`) are fixed so one
/// filter selects all engine spans.
///
/// With the `profiling` feature off this expands to nothing. With it on, the
/// scope guard is `!Send`, so do not hold a scope across `.await` in code that
/// must be `Send`: it compiles with the feature off and fails with it on.
///
/// ```
/// use exitium_profiler::profile_scope;
///
/// fn update() -> u32 {
///     profile_scope!("exitium_sim::update");
///     42
/// }
/// assert_eq!(update(), 42);
/// ```
///
/// The macro is a statement, not an expression:
///
/// ```compile_fail
/// use exitium_profiler::profile_scope;
///
/// let _guard = profile_scope!("exitium_sim::update");
/// ```
///
/// and the name must be a literal:
///
/// ```compile_fail
/// use exitium_profiler::profile_scope;
///
/// let name = "exitium_sim::update";
/// profile_scope!(name);
/// ```
#[cfg(feature = "profiling")]
#[macro_export]
macro_rules! profile_scope {
    ($name:literal) => {
        // Macro hygiene keeps this binding invisible to the caller; it still
        // lives, and keeps the span entered, until the caller's block ends.
        let _exitium_profile_scope = $crate::__private::tracing::span!(
            target: "exitium",
            $crate::__private::tracing::Level::INFO,
            $name
        )
        .entered();
    };
}

/// Profiles the rest of the enclosing block as a span named `$name`.
///
/// The `profiling` feature is off in this build, so this expands to nothing.
/// With it on, the scope guard is `!Send`, so do not hold a scope across
/// `.await` in code that must be `Send`.
#[cfg(not(feature = "profiling"))]
#[macro_export]
macro_rules! profile_scope {
    ($name:literal) => {};
}

#[cfg(test)]
mod tests {
    /// A `const fn` cannot create a span, so this only compiles if every scope
    /// (in a loop, nested, and as siblings in one block) expands to nothing.
    #[cfg(not(feature = "profiling"))]
    #[test]
    fn disabled_scopes_expand_to_nothing() {
        const fn scoped(n: u32) -> u32 {
            let mut sum = 0;
            let mut i = 0;
            while i < n {
                profile_scope!("exitium_profiler::tests::outer");
                profile_scope!("exitium_profiler::tests::sibling");
                {
                    profile_scope!("exitium_profiler::tests::inner");
                    sum += i;
                }
                i += 1;
            }
            sum
        }
        const SUM: u32 = scoped(10_000);
        assert_eq!(SUM, 49_995_000);
    }

    #[cfg(feature = "profiling")]
    mod collect {
        use std::sync::{Arc, Mutex};

        use tracing::span::{Attributes, Id, Record};
        use tracing::subscriber::Interest;
        use tracing::{Event, Level, Metadata, Subscriber};

        /// What the collector saw, in order.
        #[derive(Clone, Debug, PartialEq)]
        enum Seen {
            New {
                name: &'static str,
                target: &'static str,
                level: Level,
            },
            Enter(&'static str),
            Exit(&'static str),
        }

        /// A minimal subscriber that records span lifecycle calls. Span IDs
        /// are indexes (plus one, since 0 is invalid) into `names`.
        #[derive(Clone, Default)]
        struct Collector {
            /// When set, only spans with this target are enabled.
            target: Option<&'static str>,
            names: Arc<Mutex<Vec<&'static str>>>,
            seen: Arc<Mutex<Vec<Seen>>>,
        }

        impl Collector {
            fn only(target: &'static str) -> Self {
                Self {
                    target: Some(target),
                    ..Self::default()
                }
            }

            fn name(&self, id: &Id) -> &'static str {
                let index = usize::try_from(id.into_u64() - 1).expect("span id fits usize");
                self.names.lock().unwrap()[index]
            }

            /// Runs `f` with this collector as the thread's subscriber.
            fn run<T>(&self, f: impl FnOnce() -> T) -> T {
                tracing::subscriber::with_default(self.clone(), f)
            }

            fn assert_seen(&self, expected: &[Seen]) {
                assert_eq!(*self.seen.lock().unwrap(), expected);
            }
        }

        impl Subscriber for Collector {
            // Ask `enabled` on every call instead of caching an interest:
            // tests run in parallel, each with its own scoped subscriber.
            fn register_callsite(&self, _: &'static Metadata<'static>) -> Interest {
                Interest::sometimes()
            }

            fn enabled(&self, meta: &Metadata<'_>) -> bool {
                self.target.is_none_or(|target| meta.target() == target)
            }

            fn new_span(&self, attrs: &Attributes<'_>) -> Id {
                let meta = attrs.metadata();
                let mut names = self.names.lock().unwrap();
                names.push(meta.name());
                self.seen.lock().unwrap().push(Seen::New {
                    name: meta.name(),
                    target: meta.target(),
                    level: *meta.level(),
                });
                Id::from_u64(names.len() as u64)
            }

            fn record(&self, _: &Id, _: &Record<'_>) {}

            fn record_follows_from(&self, _: &Id, _: &Id) {}

            fn event(&self, _: &Event<'_>) {}

            fn enter(&self, id: &Id) {
                let name = self.name(id);
                self.seen.lock().unwrap().push(Seen::Enter(name));
            }

            fn exit(&self, id: &Id) {
                let name = self.name(id);
                self.seen.lock().unwrap().push(Seen::Exit(name));
            }
        }

        fn new(name: &'static str) -> Seen {
            Seen::New {
                name,
                target: "exitium",
                level: Level::INFO,
            }
        }

        /// The full lifecycle of one span with no children.
        fn whole(name: &'static str) -> [Seen; 3] {
            [new(name), Seen::Enter(name), Seen::Exit(name)]
        }

        #[test]
        fn nested_scopes_are_recorded_in_order() {
            let collector = Collector::default();
            collector.run(|| {
                profile_scope!("exitium_profiler::outer");
                {
                    profile_scope!("exitium_profiler::inner");
                }
            });

            collector.assert_seen(&[
                new("exitium_profiler::outer"),
                Seen::Enter("exitium_profiler::outer"),
                new("exitium_profiler::inner"),
                Seen::Enter("exitium_profiler::inner"),
                Seen::Exit("exitium_profiler::inner"),
                Seen::Exit("exitium_profiler::outer"),
            ]);
        }

        /// The second guard must not drop the first: both live to the end of
        /// the block and exit in reverse order.
        #[test]
        fn sibling_scopes_exit_in_reverse_order() {
            let collector = Collector::default();
            collector.run(|| {
                profile_scope!("exitium_profiler::first");
                profile_scope!("exitium_profiler::second");
            });

            collector.assert_seen(&[
                new("exitium_profiler::first"),
                Seen::Enter("exitium_profiler::first"),
                new("exitium_profiler::second"),
                Seen::Enter("exitium_profiler::second"),
                Seen::Exit("exitium_profiler::second"),
                Seen::Exit("exitium_profiler::first"),
            ]);
        }

        #[test]
        fn scope_ends_with_its_block() {
            let collector = Collector::default();
            collector.run(|| {
                {
                    profile_scope!("exitium_profiler::block");
                }
                profile_scope!("exitium_profiler::after");
            });

            let mut expected = Vec::from(whole("exitium_profiler::block"));
            expected.extend(whole("exitium_profiler::after"));
            collector.assert_seen(&expected);
        }

        #[test]
        fn scope_exits_on_early_return() {
            fn early(stop: bool) -> u32 {
                profile_scope!("exitium_profiler::early");
                if stop {
                    return 1;
                }
                2
            }

            let collector = Collector::default();
            assert_eq!(collector.run(|| early(true)), 1);
            collector.assert_seen(&whole("exitium_profiler::early"));
        }

        #[test]
        fn scope_exits_on_question_mark() {
            fn parse(input: &str) -> Result<u32, std::num::ParseIntError> {
                profile_scope!("exitium_profiler::parse");
                let value: u32 = input.parse()?;
                Ok(value + 1)
            }

            let collector = Collector::default();
            assert!(collector.run(|| parse("not a number")).is_err());
            collector.assert_seen(&whole("exitium_profiler::parse"));
        }

        #[test]
        fn scope_exits_on_panic() {
            let collector = Collector::default();
            let result = collector.run(|| {
                std::panic::catch_unwind(|| {
                    profile_scope!("exitium_profiler::boom");
                    panic!("expected test panic");
                })
            });

            assert!(result.is_err());
            collector.assert_seen(&whole("exitium_profiler::boom"));
        }

        /// Each iteration's spans must close before the next iteration opens.
        #[test]
        fn scopes_in_loop_close_every_iteration() {
            let collector = Collector::default();
            collector.run(|| {
                for _ in 0..2 {
                    profile_scope!("exitium_profiler::iteration");
                    {
                        profile_scope!("exitium_profiler::step");
                    }
                }
            });

            let iteration = [
                new("exitium_profiler::iteration"),
                Seen::Enter("exitium_profiler::iteration"),
                new("exitium_profiler::step"),
                Seen::Enter("exitium_profiler::step"),
                Seen::Exit("exitium_profiler::step"),
                Seen::Exit("exitium_profiler::iteration"),
            ];
            let expected: Vec<Seen> = (0..2).flat_map(|_| iteration.clone()).collect();
            collector.assert_seen(&expected);
        }

        #[test]
        fn target_filter_selects_engine_spans() {
            let engine = Collector::only("exitium");
            engine.run(|| {
                profile_scope!("exitium_profiler::filtered");
            });
            engine.assert_seen(&whole("exitium_profiler::filtered"));

            let other = Collector::only("not_exitium");
            other.run(|| {
                profile_scope!("exitium_profiler::filtered");
            });
            other.assert_seen(&[]);
        }

        #[test]
        fn scope_runs_without_a_subscriber() {
            fn scoped() -> u32 {
                profile_scope!("exitium_profiler::unobserved");
                3
            }

            // A fresh thread has no scoped subscriber from another test.
            let value = std::thread::spawn(scoped).join().unwrap();
            assert_eq!(value, 3);
        }

        #[test]
        fn threads_record_only_their_own_spans() {
            let left = Collector::default();
            let right = Collector::default();
            std::thread::scope(|s| {
                s.spawn(|| {
                    left.run(|| {
                        for _ in 0..100 {
                            profile_scope!("exitium_profiler::left");
                        }
                    });
                });
                s.spawn(|| {
                    right.run(|| {
                        for _ in 0..100 {
                            profile_scope!("exitium_profiler::right");
                        }
                    });
                });
            });

            let expected = |name| -> Vec<Seen> { (0..100).flat_map(|_| whole(name)).collect() };
            left.assert_seen(&expected("exitium_profiler::left"));
            right.assert_seen(&expected("exitium_profiler::right"));
        }
    }
}
