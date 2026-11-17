//! Benchmark regression gate (CLAUDE.md design rule 4: CI fails on a regression
//! over 5%).
//!
//! `bench-ci` runs the criterion benchmarks on the base commit (saved as the
//! `base` baseline), then on HEAD compared against it. `bench-compare` reads
//! criterion's `change/estimates.json` files and fails if a benchmark's mean got
//! slower by more than the threshold *and* the slowdown is statistically
//! significant (the lower bound of its 95% confidence interval is above zero).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const DEFAULT_THRESHOLD_PERCENT: f64 = 5.0;

/// Set by CI when the PR carries the `bench:removal-ok` label, for PRs that
/// delete or rename benchmarks on purpose.
const ALLOW_REMOVED_ENV: &str = "EXITIUM_ALLOW_REMOVED_BENCHES";

pub fn bench_ci(args: &[String]) -> Result<(), String> {
    let base_ref = args
        .first()
        .ok_or("usage: cargo xtask bench-ci <base-ref>")?;
    let allow_removed = std::env::var(ALLOW_REMOVED_ENV).is_ok_and(|v| v == "true" || v == "1");
    let target_dir = target_dir()?;
    let criterion_dir = target_dir.join("criterion");
    let worktree = target_dir.join("bench-base");

    // Runners are persistent, so clear results from earlier runs first.
    if criterion_dir.exists() {
        fs::remove_dir_all(&criterion_dir)
            .map_err(|e| format!("clearing {criterion_dir:?}: {e}"))?;
    }
    let _ = Command::new("git")
        .args(["worktree", "remove", "--force"])
        .arg(&worktree)
        .status();

    crate::run(
        "git",
        &[
            "worktree",
            "add",
            "--detach",
            path_str(&worktree)?,
            base_ref,
        ],
    )?;
    let base = run_base(&worktree, &target_dir);
    let _ = Command::new("git")
        .args(["worktree", "remove", "--force"])
        .arg(&worktree)
        .status();
    let base_has_benches = base?;
    let head_has_benches = has_bench_targets(Path::new("."))?;

    match (base_has_benches, head_has_benches) {
        (false, false) => {
            println!("bench-ci: no bench targets on either side; nothing to gate");
            return Ok(());
        }
        (true, false) if !allow_removed => {
            return Err(removed_error("HEAD has no bench targets left"));
        }
        (true, false) => return Ok(()),
        _ => {}
    }

    cargo_bench(Path::new("."), &target_dir, &["--baseline-lenient", "base"])?;
    if base_has_benches && !allow_removed {
        let missing = uncompared(&criterion_dir);
        if !missing.is_empty() {
            return Err(removed_error(&format!(
                "no HEAD result for: {}",
                missing.join(", ")
            )));
        }
    }
    // Both sides ran benchmarks, so an empty comparison means something broke.
    gate(&criterion_dir, DEFAULT_THRESHOLD_PERCENT, base_has_benches)
}

/// Benchmarks the base worktree into the `base` baseline. Returns whether the
/// base had any benchmarks.
fn run_base(worktree: &Path, target_dir: &Path) -> Result<bool, String> {
    if !has_bench_targets(worktree)? {
        return Ok(false);
    }
    cargo_bench(worktree, target_dir, &["--save-baseline", "base"])?;
    Ok(true)
}

/// A removed or renamed benchmark could hide a regression, so it fails the gate
/// unless the PR opts out.
fn removed_error(detail: &str) -> String {
    format!(
        "benchmarks that ran on the base are missing at HEAD ({detail}). \
         If that is intended, add the `bench:removal-ok` label to the PR."
    )
}

/// Benchmarks with a saved `base` baseline but no comparison from the HEAD run.
fn uncompared(criterion_dir: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for path in entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()) {
            if path.join("base/estimates.json").is_file()
                && !path.join("change/estimates.json").is_file()
            {
                let name = path.strip_prefix(root).unwrap_or(&path);
                out.push(name.to_string_lossy().replace('\\', "/"));
            } else {
                walk(root, &path, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(criterion_dir, criterion_dir, &mut out);
    out.sort();
    out
}

pub fn compare(args: &[String]) -> Result<(), String> {
    let threshold = match args.first() {
        Some(value) => value
            .parse::<f64>()
            .map_err(|_| format!("threshold must be a number, got {value:?}"))?,
        None => DEFAULT_THRESHOLD_PERCENT,
    };
    gate(&target_dir()?.join("criterion"), threshold, false)
}

fn gate(criterion_dir: &Path, threshold: f64, require_comparisons: bool) -> Result<(), String> {
    let mut changes = Vec::new();
    collect_changes(criterion_dir, criterion_dir, &mut changes)?;
    if changes.is_empty() {
        if require_comparisons {
            return Err(
                "base and HEAD both have benchmarks, but no comparisons were produced".into(),
            );
        }
        println!("bench-compare: no benchmark comparisons found; nothing to gate");
        return Ok(());
    }
    changes.sort_by(|a, b| a.name.cmp(&b.name));

    let mut regressions = 0;
    for change in &changes {
        let regressed = is_regression(change, threshold);
        if regressed {
            regressions += 1;
        }
        let flag = if regressed { "REGRESSION" } else { "ok" };
        println!("{flag:>10}  {:+7.2}%  {}", change.mean * 100.0, change.name);
    }
    if regressions > 0 {
        Err(format!(
            "{regressions} benchmark(s) regressed by more than {threshold}%"
        ))
    } else {
        println!("bench-compare: no regressions over {threshold}%");
        Ok(())
    }
}

/// One benchmark's relative change against the baseline, as fractions
/// (0.05 = 5% slower).
#[derive(Debug)]
struct Change {
    name: String,
    mean: f64,
    lower_bound: f64,
}

/// Slower than the threshold *and* statistically significant. A slowdown whose
/// confidence interval includes zero is treated as noise: runners share one
/// machine, and a false failure blocks the whole factory.
fn is_regression(change: &Change, threshold_percent: f64) -> bool {
    change.mean * 100.0 > threshold_percent && change.lower_bound > 0.0
}

fn collect_changes(root: &Path, dir: &Path, out: &mut Vec<Change>) -> Result<(), String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let estimates = path.join("estimates.json");
        if path.ends_with("change") && estimates.is_file() {
            let parent = path.parent().unwrap_or(&path);
            let name = parent
                .strip_prefix(root)
                .unwrap_or(parent)
                .to_string_lossy()
                .replace('\\', "/");
            let (mean, lower_bound) = fs::read_to_string(&estimates)
                .ok()
                .and_then(|json| parse_mean_change(&json))
                .ok_or_else(|| format!("unreadable criterion estimates for {name}"))?;
            out.push(Change {
                name,
                mean,
                lower_bound,
            });
        } else {
            collect_changes(root, &path, out)?;
        }
    }
    Ok(())
}

/// Extracts `mean.point_estimate` and `mean.confidence_interval.lower_bound`
/// from criterion's `estimates.json`, without a JSON dependency.
fn parse_mean_change(json: &str) -> Option<(f64, f64)> {
    let mean = &json[json.find("\"mean\"")?..];
    Some((
        number_after(mean, "\"point_estimate\":")?,
        number_after(mean, "\"lower_bound\":")?,
    ))
}

fn number_after(text: &str, key: &str) -> Option<f64> {
    let start = text.find(key)? + key.len();
    let rest = text[start..].trim_start();
    let end = rest.find([',', '}']).unwrap_or(rest.len());
    rest[..end].trim().parse().ok()
}

/// True if any workspace package in `dir` declares a `[[bench]]` target.
fn has_bench_targets(dir: &Path) -> Result<bool, String> {
    let output = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(dir)
        .output()
        .map_err(|e| format!("failed to start cargo metadata: {e}"))?;
    if !output.status.success() {
        return Err(format!("cargo metadata in {} failed", dir.display()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).contains(r#""kind":["bench"]"#))
}

/// `--bench '*'` selects only `[[bench]]` targets. `--benches` would also run
/// libtest harnesses, which reject criterion's flags.
fn cargo_bench(dir: &Path, target_dir: &Path, criterion_args: &[&str]) -> Result<(), String> {
    let mut args = vec!["bench", "--workspace", "--bench", "*", "--"];
    args.extend_from_slice(criterion_args);
    println!("$ (in {}) cargo {}", dir.display(), args.join(" "));
    let status = Command::new("cargo")
        .args(&args)
        .current_dir(dir)
        .env("CARGO_TARGET_DIR", target_dir)
        .status()
        .map_err(|e| format!("failed to start cargo: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "cargo bench in {} failed with {status}",
            dir.display()
        ))
    }
}

fn target_dir() -> Result<PathBuf, String> {
    let dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target"));
    std::path::absolute(&dir).map_err(|e| format!("resolving target dir {dir:?}: {e}"))
}

fn path_str(path: &Path) -> Result<&str, String> {
    path.to_str()
        .ok_or_else(|| format!("path is not valid UTF-8: {path:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"mean":{"confidence_interval":{"confidence_level":0.95,"lower_bound":0.061,"upper_bound":0.089},"point_estimate":0.074,"standard_error":0.007},"median":{"confidence_interval":{"confidence_level":0.95,"lower_bound":-0.5,"upper_bound":0.5},"point_estimate":-0.2,"standard_error":0.1}}"#;

    #[test]
    fn parses_mean_not_median() {
        assert_eq!(parse_mean_change(SAMPLE), Some((0.074, 0.061)));
    }

    #[test]
    fn rejects_json_without_mean() {
        assert_eq!(parse_mean_change(r#"{"median":{}}"#), None);
    }

    fn change(mean: f64, lower_bound: f64) -> Change {
        Change {
            name: "b".into(),
            mean,
            lower_bound,
        }
    }

    #[test]
    fn regression_needs_threshold_and_significance() {
        assert!(is_regression(&change(0.074, 0.061), 5.0));
        assert!(!is_regression(&change(0.04, 0.01), 5.0), "under threshold");
        assert!(
            !is_regression(&change(0.08, -0.01), 5.0),
            "interval spans zero"
        );
        assert!(!is_regression(&change(-0.2, -0.3), 5.0), "faster");
    }

    #[test]
    fn gate_fails_on_unparseable_estimates() {
        let dir = std::env::temp_dir().join(format!("xtask-bench-{}", std::process::id()));
        let change_dir = dir.join("group/bench/change");
        fs::create_dir_all(&change_dir).unwrap();
        fs::write(change_dir.join("estimates.json"), "{}").unwrap();
        let result = gate(&dir, 5.0, false);
        fs::remove_dir_all(&dir).unwrap();
        assert!(result.is_err());
    }

    #[test]
    fn gate_requires_comparisons_when_asked() {
        let empty = std::env::temp_dir().join("xtask-bench-missing-dir");
        assert!(gate(&empty, 5.0, false).is_ok());
        assert!(gate(&empty, 5.0, true).is_err());
    }

    #[test]
    fn finds_benchmarks_without_a_head_result() {
        let dir = std::env::temp_dir().join(format!("xtask-uncompared-{}", std::process::id()));
        for (bench, has_change) in [("g/kept", true), ("g/removed", false)] {
            fs::create_dir_all(dir.join(bench).join("base")).unwrap();
            fs::write(dir.join(bench).join("base/estimates.json"), "{}").unwrap();
            if has_change {
                fs::create_dir_all(dir.join(bench).join("change")).unwrap();
                fs::write(dir.join(bench).join("change/estimates.json"), "{}").unwrap();
            }
        }
        let missing = uncompared(&dir);
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(missing, vec!["g/removed".to_string()]);
    }

    #[test]
    fn parses_negative_and_exponent_numbers() {
        assert_eq!(number_after(r#"{"x": -1.5e-3}"#, "\"x\":"), Some(-0.0015));
    }
}
