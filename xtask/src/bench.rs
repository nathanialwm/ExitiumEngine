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

pub fn bench_ci(args: &[String]) -> Result<(), String> {
    let base_ref = args
        .first()
        .ok_or("usage: cargo xtask bench-ci <base-ref>")?;
    let target_dir = target_dir()?;
    let criterion_dir = target_dir.join("criterion");
    let worktree = target_dir.join("bench-base");

    if !has_bench_targets(Path::new("."))? {
        println!("bench-ci: no bench targets at HEAD; nothing to gate");
        return Ok(());
    }

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
    // A base without benchmarks just leaves nothing to compare against.
    let base = match has_bench_targets(&worktree) {
        Ok(true) => cargo_bench(&worktree, &target_dir, &["--save-baseline", "base"]),
        other => other.map(|_| ()),
    };
    let _ = Command::new("git")
        .args(["worktree", "remove", "--force"])
        .arg(&worktree)
        .status();
    base?;

    cargo_bench(Path::new("."), &target_dir, &["--baseline-lenient", "base"])?;
    compare(&[])
}

pub fn compare(args: &[String]) -> Result<(), String> {
    let threshold = match args.first() {
        Some(value) => value
            .parse::<f64>()
            .map_err(|_| format!("threshold must be a number, got {value:?}"))?,
        None => DEFAULT_THRESHOLD_PERCENT,
    };
    let criterion_dir = target_dir()?.join("criterion");
    let mut changes = Vec::new();
    collect_changes(&criterion_dir, &criterion_dir, &mut changes);
    if changes.is_empty() {
        println!("bench-compare: no benchmark comparisons found; nothing to gate");
        return Ok(());
    }
    changes.sort_by(|a, b| a.name.cmp(&b.name));

    let mut regressions = 0;
    for change in &changes {
        let percent = change.mean * 100.0;
        let regressed = percent > threshold && change.lower_bound > 0.0;
        if regressed {
            regressions += 1;
        }
        let flag = if regressed { "REGRESSION" } else { "ok" };
        println!("{flag:>10}  {percent:+7.2}%  {}", change.name);
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
struct Change {
    name: String,
    mean: f64,
    lower_bound: f64,
}

fn collect_changes(root: &Path, dir: &Path, out: &mut Vec<Change>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let estimates = path.join("estimates.json");
        if path.ends_with("change") && estimates.is_file() {
            let parsed = fs::read_to_string(&estimates)
                .ok()
                .and_then(|json| parse_mean_change(&json));
            if let (Some((mean, lower_bound)), Some(parent)) = (parsed, path.parent()) {
                let name = parent
                    .strip_prefix(root)
                    .unwrap_or(parent)
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push(Change {
                    name,
                    mean,
                    lower_bound,
                });
            }
        } else {
            collect_changes(root, &path, out);
        }
    }
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

    #[test]
    fn parses_negative_and_exponent_numbers() {
        assert_eq!(number_after(r#"{"x": -1.5e-3}"#, "\"x\":"), Some(-0.0015));
    }
}
