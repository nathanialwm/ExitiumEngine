//! Uses `profile_scope!` from outside the crate, by full path and from a
//! nested module, to check the expansion resolves through `$crate`.
//!
//! Limitation: integration tests can see this package's own dependencies,
//! `tracing` included, so this does not prove that a consumer without a
//! `tracing` dependency compiles. That needs a separate consumer crate.

#![cfg(feature = "profiling")]

mod nested {
    pub mod deeper {
        pub fn scoped(input: &str) -> Result<u32, std::num::ParseIntError> {
            exitium_profiler::profile_scope!("exitium_profiler::tests::macro_path");
            exitium_profiler::profile_scope!("exitium_profiler::tests::macro_path::sibling");
            let value: u32 = input.parse()?;
            Ok(value * 2)
        }
    }
}

#[test]
fn macro_works_by_full_path_from_nested_module() {
    assert_eq!(nested::deeper::scoped("21"), Ok(42));
    assert!(nested::deeper::scoped("x").is_err());
}
