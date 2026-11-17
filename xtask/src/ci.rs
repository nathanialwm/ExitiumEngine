//! CI steps. Each step is the single definition of a definition-of-done check
//! (CLAUDE.md), shared by agents running locally and by `.github/workflows/ci.yml`.

use std::fs;
use std::path::Path;

use crate::run;

/// Crates that cannot target `wasm32-unknown-unknown` (native-only tooling,
/// platform SDKs). Everything else must build for the web.
const NATIVE_ONLY: &[&str] = &[
    "xtask",
    "exitium_reflect_derive",
    "exitium_editor",
    "exitium_cli",
];

pub fn step(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("fmt") => fmt(),
        Some("clippy") => clippy(),
        Some("test") => test(),
        Some("wasm") => wasm(),
        Some("deny") => run("cargo", &["deny", "check"]),
        Some("miri") => miri(),
        // `all` is the local definition-of-done run; deny and miri need extra
        // tools and run in CI only.
        Some("all") => fmt().and(clippy()).and(test()).and(wasm()),
        other => Err(format!(
            "unknown ci step {other:?}; expected fmt | clippy | test | wasm | deny | miri | all"
        )),
    }
}

fn fmt() -> Result<(), String> {
    run("cargo", &["fmt", "--all", "--", "--check"])
}

fn clippy() -> Result<(), String> {
    run(
        "cargo",
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
    )
}

/// Uses nextest when installed (CI images have it), plain `cargo test`
/// otherwise. Nextest skips doctests, so those run separately.
fn test() -> Result<(), String> {
    let has_nextest = std::process::Command::new("cargo")
        .args(["nextest", "--version"])
        .output()
        .is_ok_and(|o| o.status.success());
    if has_nextest {
        run(
            "cargo",
            &["nextest", "run", "--workspace", "--no-tests=pass"],
        )?;
        run("cargo", &["test", "--workspace", "--doc"])
    } else {
        run("cargo", &["test", "--workspace"])
    }
}

fn wasm() -> Result<(), String> {
    let mut args = vec!["build", "--workspace", "--target", "wasm32-unknown-unknown"];
    for name in NATIVE_ONLY {
        args.extend(["--exclude", name]);
    }
    run("cargo", &args)
}

/// Runs Miri on every crate that contains a designated `unsafe` module, i.e.
/// any source file with `#[allow(unsafe_code)]`. Needs a nightly toolchain
/// with the `miri` component.
fn miri() -> Result<(), String> {
    let crates = unsafe_crates(Path::new("crates"))?;
    if crates.is_empty() {
        println!("no designated unsafe modules; nothing for miri to check");
        return Ok(());
    }
    for name in &crates {
        run("cargo", &["+nightly", "miri", "test", "-p", name])?;
    }
    Ok(())
}

fn unsafe_crates(crates_dir: &Path) -> Result<Vec<String>, String> {
    let mut found = Vec::new();
    let entries = fs::read_dir(crates_dir).map_err(|e| format!("reading {crates_dir:?}: {e}"))?;
    for entry in entries.flatten() {
        let src = entry.path().join("src");
        if src.is_dir() && dir_contains(&src, "allow(unsafe_code)") {
            found.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    found.sort();
    Ok(found)
}

fn dir_contains(dir: &Path, needle: &str) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        if path.is_dir() {
            dir_contains(&path, needle)
        } else {
            path.extension().is_some_and(|ext| ext == "rs")
                && fs::read_to_string(&path).is_ok_and(|text| text.contains(needle))
        }
    })
}
