//! Benchmark regression gate (CLAUDE.md design rule 4: CI fails on a regression
//! over 5%).
//!
//! The runners share one PC with other CI jobs and agents, so a benchmark can
//! run 2-3x slower for a few seconds while something else compiles. One base
//! run against one HEAD run cannot tell that apart from a real slowdown (#82).
//! So `bench-ci` builds both sides first, then runs every benchmark in
//! [`ROUNDS`] rounds per side, alternating base and HEAD:
//!
//! ```text
//! base-1 head-1 head-2 base-2 base-3 head-3
//! ```
//!
//! The pairs swap order so slow drift over the job does not favor one side.
//! Each round is saved as a criterion baseline named `<side>-<round>`, and the
//! gate reads the median time of each round from its `estimates.json`.
//!
//! The rule: a benchmark regressed when its **fastest** HEAD round is more than
//! the threshold slower than its fastest base round. Noise from other load only
//! ever adds time, so the fastest round is the best estimate of what the code
//! costs. Equivalently, every HEAD round must be that much slower than the best
//! base round. A burst that hits one or two rounds of a side cannot fail the
//! gate, and a real slowdown shows up in every round, so it still does.
//! `docs/benchmarks.md` has the measurements behind this.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const DEFAULT_THRESHOLD_PERCENT: f64 = 5.0;

/// Rounds per side. Overridable with `EXITIUM_BENCH_ROUNDS` for noise studies.
const ROUNDS: usize = 3;
const ROUNDS_ENV: &str = "EXITIUM_BENCH_ROUNDS";

/// Shorter than criterion's defaults (3 s warm-up, 5 s measurement, 100
/// samples): the rounds, not one long run, are what average out the noise, and
/// six rounds of the defaults would make the job take minutes per benchmark
/// file.
const CRITERION_ARGS: [&str; 6] = [
    "--warm-up-time",
    "1",
    "--measurement-time",
    "2",
    "--sample-size",
    "50",
];

/// Set by CI when the PR carries the `bench:removal-ok` label, for PRs that
/// delete or rename benchmarks on purpose.
const ALLOW_REMOVED_ENV: &str = "EXITIUM_ALLOW_REMOVED_BENCHES";

#[derive(Clone, Copy, PartialEq)]
enum Side {
    Base,
    Head,
}

impl Side {
    fn name(self) -> &'static str {
        match self {
            Side::Base => "base",
            Side::Head => "head",
        }
    }
}

pub fn bench_ci(args: &[String]) -> Result<(), String> {
    let base_ref = args
        .first()
        .ok_or("usage: cargo xtask bench-ci <base-ref>")?;
    let rounds = match std::env::var(ROUNDS_ENV) {
        Ok(value) => value
            .parse::<usize>()
            .ok()
            .filter(|&n| n > 0)
            .ok_or_else(|| format!("{ROUNDS_ENV} must be a positive integer, got {value:?}"))?,
        Err(_) => ROUNDS,
    };
    let allow_removed = std::env::var(ALLOW_REMOVED_ENV).is_ok_and(|v| v == "true" || v == "1");
    let target_dir = target_dir()?;
    let criterion_dir = target_dir.join("criterion");
    let worktree = target_dir.join("bench-base");

    // Runners are persistent, so clear results from earlier runs first.
    if criterion_dir.exists() {
        fs::remove_dir_all(&criterion_dir)
            .map_err(|e| format!("clearing {criterion_dir:?}: {e}"))?;
    }
    remove_worktree(&worktree);
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
    // The base worktree must outlive every round, so remove it only at the end.
    let result = run_rounds(&worktree, &target_dir, rounds, allow_removed);
    remove_worktree(&worktree);
    result
}

fn remove_worktree(worktree: &Path) {
    let _ = Command::new("git")
        .args(["worktree", "remove", "--force"])
        .arg(worktree)
        .status();
}

fn run_rounds(
    worktree: &Path,
    target_dir: &Path,
    rounds: usize,
    allow_removed: bool,
) -> Result<(), String> {
    let head = Path::new(".");
    let base_has_benches = has_bench_targets(worktree)?;
    let head_has_benches = has_bench_targets(head)?;
    let dir = |side| if side == Side::Base { worktree } else { head };

    match (base_has_benches, head_has_benches) {
        (false, false) => {
            println!("bench-ci: no bench targets on either side; nothing to gate");
            return Ok(());
        }
        (true, false) if !allow_removed => {
            return Err(removed_error("HEAD has no bench targets left"));
        }
        (true, false) => return Ok(()),
        (false, true) => {
            // Nothing to compare against; one run checks the new benchmarks work.
            println!("bench-ci: the base has no bench targets; running HEAD once");
            return cargo_bench(head, target_dir, &[], &round_args(Side::Head, 1));
        }
        (true, true) => {}
    }

    // Build both sides before timing anything, so no round shares the CPU
    // with a compile.
    for side in [Side::Base, Side::Head] {
        cargo_bench(dir(side), target_dir, &["--no-run"], &[])?;
    }
    for round in 1..=rounds {
        let order = if round % 2 == 1 {
            [Side::Base, Side::Head]
        } else {
            [Side::Head, Side::Base]
        };
        for side in order {
            println!("bench-ci: round {round}/{rounds}, {}", side.name());
            cargo_bench(dir(side), target_dir, &[], &round_args(side, round))?;
        }
    }
    let benches = collect_rounds(&target_dir.join("criterion"))?;
    if !allow_removed {
        let missing: Vec<&str> = benches
            .iter()
            .filter(|b| b.head.is_empty())
            .map(|b| b.name.as_str())
            .collect();
        if !missing.is_empty() {
            return Err(removed_error(&format!(
                "no HEAD result for: {}",
                missing.join(", ")
            )));
        }
    }
    // Both sides ran benchmarks, so an empty comparison means something broke.
    gate(&benches, DEFAULT_THRESHOLD_PERCENT, true)
}

fn round_args(side: Side, round: usize) -> Vec<String> {
    let mut args = vec![
        "--save-baseline".to_string(),
        format!("{}-{round}", side.name()),
    ];
    args.extend(CRITERION_ARGS.iter().map(|a| a.to_string()));
    args
}

/// A removed or renamed benchmark could hide a regression, so it fails the gate
/// unless the PR opts out.
fn removed_error(detail: &str) -> String {
    format!(
        "benchmarks that ran on the base are missing at HEAD ({detail}). \
         If that is intended, add the `bench:removal-ok` label to the PR."
    )
}

pub fn compare(args: &[String]) -> Result<(), String> {
    let threshold = match args.first() {
        Some(value) => value
            .parse::<f64>()
            .map_err(|_| format!("threshold must be a number, got {value:?}"))?,
        None => DEFAULT_THRESHOLD_PERCENT,
    };
    let benches = collect_rounds(&target_dir()?.join("criterion"))?;
    gate(&benches, threshold, false)
}

/// One benchmark's per-round median times in nanoseconds, in round order.
#[derive(Debug)]
struct Rounds {
    name: String,
    base: Vec<f64>,
    head: Vec<f64>,
}

impl Rounds {
    /// Relative change of the fastest HEAD round against the fastest base round
    /// (0.05 = 5% slower). `None` when one side has no rounds.
    fn change(&self) -> Option<f64> {
        Some(fastest(&self.head)? / fastest(&self.base)? - 1.0)
    }
}

fn fastest(times: &[f64]) -> Option<f64> {
    times.iter().copied().reduce(f64::min)
}

fn is_regression(change: f64, threshold_percent: f64) -> bool {
    change * 100.0 > threshold_percent
}

fn gate(benches: &[Rounds], threshold: f64, require_comparisons: bool) -> Result<(), String> {
    let mut compared = 0;
    let mut regressions = 0;
    println!(
        "{:>10}  {:>8}  benchmark  [base rounds] [head rounds]",
        "", "change"
    );
    for bench in benches {
        let flag = match bench.change() {
            Some(change) => {
                compared += 1;
                let regressed = is_regression(change, threshold);
                regressions += usize::from(regressed);
                let flag = if regressed { "REGRESSION" } else { "ok" };
                format!("{flag:>10}  {:+7.2}%", change * 100.0)
            }
            None if bench.base.is_empty() => format!("{:>10}  {:>8}", "new", ""),
            None => format!("{:>10}  {:>8}", "removed", ""),
        };
        println!(
            "{flag}  {}  [{}] [{}]",
            bench.name,
            format_times(&bench.base),
            format_times(&bench.head)
        );
    }
    if compared == 0 {
        if require_comparisons {
            return Err(
                "base and HEAD both have benchmarks, but no comparisons were produced".into(),
            );
        }
        println!("bench-compare: no benchmark comparisons found; nothing to gate");
        return Ok(());
    }
    if regressions > 0 {
        Err(format!(
            "{regressions} benchmark(s) regressed by more than {threshold}% \
             (fastest HEAD round vs fastest base round)"
        ))
    } else {
        println!("bench-compare: no regressions over {threshold}%");
        Ok(())
    }
}

fn format_times(times: &[f64]) -> String {
    let unit = match fastest(times) {
        Some(t) if t >= 1e6 => (1e6, "ms"),
        Some(t) if t >= 1e3 => (1e3, "µs"),
        _ => (1.0, "ns"),
    };
    times
        .iter()
        .map(|t| format!("{:.2}{}", t / unit.0, unit.1))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Finds every benchmark under `criterion_dir` (a directory holding criterion's
/// `new/` output) and reads its `base-N` and `head-N` rounds, sorted by name.
fn collect_rounds(criterion_dir: &Path) -> Result<Vec<Rounds>, String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<Rounds>) -> Result<(), String> {
        let Ok(entries) = fs::read_dir(dir) else {
            return Ok(());
        };
        for path in entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()) {
            if path.join("new/estimates.json").is_file() {
                let name = path.strip_prefix(root).unwrap_or(&path);
                let name = name.to_string_lossy().replace('\\', "/");
                let base = read_side(&path, Side::Base, &name)?;
                let head = read_side(&path, Side::Head, &name)?;
                if !base.is_empty() || !head.is_empty() {
                    out.push(Rounds { name, base, head });
                }
            } else {
                walk(root, &path, out)?;
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(criterion_dir, criterion_dir, &mut out)?;
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Reads `<side>-1`, `<side>-2`, ... until the first missing round.
fn read_side(bench_dir: &Path, side: Side, name: &str) -> Result<Vec<f64>, String> {
    let mut times = Vec::new();
    for round in 1.. {
        let estimates = bench_dir
            .join(format!("{}-{round}", side.name()))
            .join("estimates.json");
        if !estimates.is_file() {
            break;
        }
        let median = fs::read_to_string(&estimates)
            .ok()
            .and_then(|json| parse_median(&json))
            .ok_or_else(|| format!("unreadable criterion estimates for {name}"))?;
        times.push(median);
    }
    Ok(times)
}

/// Extracts `median.point_estimate` (nanoseconds) from criterion's
/// `estimates.json`, without a JSON dependency. The median of a round's
/// samples ignores a burst that slowed fewer than half of them.
fn parse_median(json: &str) -> Option<f64> {
    let median = &json[json.find("\"median\":")?..];
    number_after(median, "\"point_estimate\":")
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
fn cargo_bench(
    dir: &Path,
    target_dir: &Path,
    cargo_args: &[&str],
    criterion_args: &[String],
) -> Result<(), String> {
    let mut args = vec!["bench", "--workspace", "--bench", "*"];
    args.extend_from_slice(cargo_args);
    if !criterion_args.is_empty() {
        args.push("--");
        args.extend(criterion_args.iter().map(String::as_str));
    }
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

    const SAMPLE: &str = r#"{"mean":{"confidence_interval":{"confidence_level":0.95,"lower_bound":700.1,"upper_bound":760.0},"point_estimate":719.7,"standard_error":7.0},"median":{"confidence_interval":{"confidence_level":0.95,"lower_bound":690.0,"upper_bound":705.5},"point_estimate":701.2,"standard_error":3.1},"median_abs_dev":{"confidence_interval":{"confidence_level":0.95,"lower_bound":1.0,"upper_bound":2.0},"point_estimate":1.5,"standard_error":0.1}}"#;

    #[test]
    fn parses_median_not_mean() {
        assert_eq!(parse_median(SAMPLE), Some(701.2));
    }

    #[test]
    fn rejects_json_without_median() {
        assert_eq!(parse_median(r#"{"mean":{"point_estimate":1.0}}"#), None);
        assert_eq!(
            parse_median(r#"{"median_abs_dev":{"point_estimate":1.0}}"#),
            None
        );
    }

    #[test]
    fn parses_negative_and_exponent_numbers() {
        assert_eq!(number_after(r#"{"x": -1.5e-3}"#, "\"x\":"), Some(-0.0015));
    }

    /// Scales a typical identical-code round set by per-round factors.
    fn rounds(base: [f64; 3], head: [f64; 3]) -> Rounds {
        Rounds {
            name: "g/b".into(),
            base: base.map(|f| 700.0 * f).to_vec(),
            head: head.map(|f| 700.0 * f).to_vec(),
        }
    }

    fn regressed(bench: &Rounds) -> bool {
        is_regression(bench.change().unwrap(), DEFAULT_THRESHOLD_PERCENT)
    }

    #[test]
    fn noise_only_passes() {
        // Every round within a few percent of the true cost, in both directions.
        assert!(!regressed(&rounds([1.03, 1.00, 1.04], [1.04, 1.02, 1.01])));
    }

    #[test]
    fn real_regression_fails() {
        // A 10% slowdown is in every HEAD round, on top of the same noise.
        assert!(regressed(&rounds([1.03, 1.00, 1.04], [1.13, 1.11, 1.10])));
    }

    #[test]
    fn regression_survives_a_noisy_base_round() {
        // A burst on one base round must not hide a real 10% slowdown.
        assert!(regressed(&rounds([3.10, 1.00, 1.02], [1.12, 1.10, 1.11])));
    }

    #[test]
    fn single_run_outlier_passes() {
        // +224%, as in CI run 44131141524, but in one HEAD round only.
        assert!(!regressed(&rounds([1.01, 1.00, 1.02], [3.24, 1.01, 1.00])));
        // Two of three HEAD rounds hit still pass.
        assert!(!regressed(&rounds([1.01, 1.00, 1.02], [3.24, 1.29, 1.02])));
    }

    #[test]
    fn improvement_passes() {
        assert!(!regressed(&rounds([1.00, 1.01, 1.02], [0.80, 0.81, 0.79])));
    }

    #[test]
    fn threshold_is_exclusive() {
        assert!(!is_regression(0.05, 5.0));
        assert!(is_regression(0.0501, 5.0));
    }

    #[test]
    fn change_needs_both_sides() {
        let new = Rounds {
            name: "g/new".into(),
            base: vec![],
            head: vec![1.0],
        };
        assert_eq!(new.change(), None);
    }

    fn write_round(dir: &Path, bench: &str, round: &str, median: f64) {
        let round_dir = dir.join(bench).join(round);
        fs::create_dir_all(&round_dir).unwrap();
        let json = format!(r#"{{"median":{{"point_estimate":{median}}}}}"#);
        fs::write(round_dir.join("estimates.json"), json).unwrap();
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("xtask-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn collects_rounds_per_benchmark() {
        let dir = temp_dir("collect");
        for (bench, rounds) in [
            ("g/kept/1000", &["base-1", "base-2", "head-1", "head-2"][..]),
            ("g/removed", &["base-1"][..]),
            ("g/added", &["head-1"][..]),
        ] {
            write_round(&dir, bench, "new", 1.0);
            for (i, round) in rounds.iter().enumerate() {
                write_round(&dir, bench, round, 10.0 + i as f64);
            }
        }
        // A gap ends a side: head-3 without head-2 is not read.
        write_round(&dir, "g/added", "head-3", 99.0);
        let benches = collect_rounds(&dir).unwrap();
        fs::remove_dir_all(&dir).unwrap();

        let summary: Vec<_> = benches
            .iter()
            .map(|b| (b.name.as_str(), b.base.clone(), b.head.clone()))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("g/added", vec![], vec![10.0]),
                ("g/kept/1000", vec![10.0, 11.0], vec![12.0, 13.0]),
                ("g/removed", vec![10.0], vec![]),
            ]
        );
    }

    #[test]
    fn unparseable_estimates_are_an_error() {
        let dir = temp_dir("unparseable");
        write_round(&dir, "g/b", "new", 1.0);
        fs::create_dir_all(dir.join("g/b/base-1")).unwrap();
        fs::write(dir.join("g/b/base-1/estimates.json"), "{}").unwrap();
        let result = collect_rounds(&dir);
        fs::remove_dir_all(&dir).unwrap();
        assert!(result.is_err());
    }

    #[test]
    fn gate_fails_only_on_regressions() {
        let ok = rounds([1.0, 1.0, 1.0], [1.02, 1.01, 1.03]);
        let slow = rounds([1.0, 1.0, 1.0], [1.2, 1.2, 1.2]);
        assert!(gate(&[ok], 5.0, true).is_ok());
        assert!(gate(&[slow], 5.0, true).is_err());
    }

    #[test]
    fn gate_requires_comparisons_when_asked() {
        let added = Rounds {
            name: "g/new".into(),
            base: vec![],
            head: vec![1.0],
        };
        assert!(gate(&[], 5.0, false).is_ok());
        assert!(gate(&[], 5.0, true).is_err());
        assert!(gate(&[added], 5.0, true).is_err());
    }
}
