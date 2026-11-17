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
/// With the `profiling` feature off this expands to nothing.
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
#[cfg(not(feature = "profiling"))]
#[macro_export]
macro_rules! profile_scope {
    ($name:literal) => {};
}

#[cfg(test)]
mod tests {
    #[test]
    fn enabled_matches_feature() {
        assert_eq!(super::ENABLED, cfg!(feature = "profiling"));
    }

    /// A `const fn` cannot create a span, so this only compiles if the macro
    /// expands to nothing.
    #[cfg(not(feature = "profiling"))]
    #[test]
    fn disabled_scope_expands_to_nothing() {
        const fn scoped() -> u32 {
            profile_scope!("exitium_profiler::tests::scoped");
            7
        }
        const VALUE: u32 = scoped();
        assert_eq!(VALUE, 7);
    }

    #[cfg(not(feature = "profiling"))]
    #[test]
    fn disabled_scope_heavy_loop_runs() {
        let mut sum = 0u64;
        for i in 0..10_000u64 {
            profile_scope!("exitium_profiler::tests::outer");
            {
                profile_scope!("exitium_profiler::tests::inner");
                sum += i;
            }
        }
        assert_eq!(sum, 49_995_000);
    }

    #[cfg(feature = "profiling")]
    mod collect {
        use std::sync::{Arc, Mutex};

        use tracing::span::{Attributes, Id, Record};
        use tracing::subscriber::Interest;
        use tracing::{Event, Level, Metadata, Subscriber};

        /// What the collector saw, in order.
        #[derive(Debug, PartialEq)]
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
            names: Arc<Mutex<Vec<&'static str>>>,
            seen: Arc<Mutex<Vec<Seen>>>,
        }

        impl Collector {
            fn name(&self, id: &Id) -> &'static str {
                let index = usize::try_from(id.into_u64() - 1).expect("span id fits usize");
                self.names.lock().unwrap()[index]
            }
        }

        impl Subscriber for Collector {
            // Ask `enabled` on every call instead of caching an interest:
            // tests run in parallel, each with its own scoped subscriber.
            fn register_callsite(&self, _: &'static Metadata<'static>) -> Interest {
                Interest::sometimes()
            }

            fn enabled(&self, _: &Metadata<'_>) -> bool {
                true
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

        #[test]
        fn nested_scopes_are_recorded_in_order() {
            let collector = Collector::default();
            tracing::subscriber::with_default(collector.clone(), || {
                profile_scope!("exitium_profiler::outer");
                {
                    profile_scope!("exitium_profiler::inner");
                }
            });

            let seen = collector.seen.lock().unwrap();
            assert_eq!(
                *seen,
                [
                    new("exitium_profiler::outer"),
                    Seen::Enter("exitium_profiler::outer"),
                    new("exitium_profiler::inner"),
                    Seen::Enter("exitium_profiler::inner"),
                    Seen::Exit("exitium_profiler::inner"),
                    Seen::Exit("exitium_profiler::outer"),
                ]
            );
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
            let result = tracing::subscriber::with_default(collector.clone(), || early(true));

            assert_eq!(result, 1);
            let seen = collector.seen.lock().unwrap();
            assert_eq!(
                *seen,
                [
                    new("exitium_profiler::early"),
                    Seen::Enter("exitium_profiler::early"),
                    Seen::Exit("exitium_profiler::early"),
                ]
            );
        }

        #[test]
        fn scope_in_loop_records_every_iteration() {
            let collector = Collector::default();
            tracing::subscriber::with_default(collector.clone(), || {
                for _ in 0..3 {
                    profile_scope!("exitium_profiler::iteration");
                }
            });

            let seen = collector.seen.lock().unwrap();
            let enters = seen
                .iter()
                .filter(|s| **s == Seen::Enter("exitium_profiler::iteration"))
                .count();
            let exits = seen
                .iter()
                .filter(|s| **s == Seen::Exit("exitium_profiler::iteration"))
                .count();
            assert_eq!((enters, exits), (3, 3));
        }
    }
}
