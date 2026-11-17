//! Repository automation. Run with `cargo xtask <command>`.
//!
//! CI workflows call these commands instead of inlining shell, so the checks an
//! agent runs locally are exactly the checks CI runs.

mod bench;
mod ci;
mod git;

use std::process::ExitCode;

const USAGE: &str = "usage: cargo xtask <command>

commands:
  ci <step>                 run one CI step: fmt | clippy | test | wasm | deny | miri | all
  bench-ci <base-ref>       benchmark <base-ref> and HEAD, then run bench-compare
  bench-compare [percent]   re-gate the last bench-ci rounds (default 5%)
  push [git push args]      git push (the pre-push hook refreshes the local graph)
  install-hooks             point git at scripts/hooks (local graph build and refresh)";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest = args.get(1..).unwrap_or_default();
    let result = match args.first().map(String::as_str) {
        None | Some("help" | "--help" | "-h") => {
            println!("{USAGE}");
            Ok(())
        }
        Some("ci") => ci::step(rest),
        Some("bench-ci") => bench::bench_ci(rest),
        Some("bench-compare") => bench::compare(rest),
        Some("push") => git::push(rest),
        Some("install-hooks") => git::install_hooks(),
        Some(other) => Err(format!("unknown xtask command: {other}\n\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("xtask: {message}");
            ExitCode::FAILURE
        }
    }
}

/// Runs a command to completion, inheriting stdio. Errors if it exits non-zero.
pub(crate) fn run(program: &str, args: &[&str]) -> Result<(), String> {
    println!("$ {program} {}", args.join(" "));
    let status = std::process::Command::new(program)
        .args(args)
        .status()
        .map_err(|e| format!("failed to start `{program}`: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "`{program} {}` failed with {status}",
            args.join(" ")
        ))
    }
}
