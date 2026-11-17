//! Benchmark regression gate (CLAUDE.md design rule 4: CI fails on a regression
//! over 5%).
//!
//! The runners share one PC with other CI jobs and agents, so a benchmark can
//! run 2-3x slower for a few seconds while something else compiles, and one
//! process run can be 10% faster or slower than the next for no visible
//! reason. One base run against one HEAD run cannot tell that apart from a real
//! slowdown (#82). So `bench-ci` builds both sides first, then runs every
//! benchmark in [`ROUNDS`] short rounds per side, alternating base and HEAD:
//!
//! ```text
//! base-1 head-1 head-2 base-2 base-3 head-3 ...
//! ```
//!
//! The pairs swap order so slow drift over the job does not favor one side.
//! Each round is a separate process run, saved as a criterion baseline named
//! `<side>-<round>`, and the gate reads that round's median time from its
//! `estimates.json`.
//!
//! The rule: a benchmark is flagged when its median HEAD round is more than
//! the threshold slower than its median base round. The median ignores bursts
//! and lucky runs as long as they hit fewer than half the rounds of a side. A
//! flagged benchmark is then measured again in a confirm pass of twice as many
//! fresh rounds (baselines `confirm-<side>-<round>`), and fails the gate only
//! if that pass flags it too. A real slowdown shows up in every round of both
//! passes, so it still fails. `docs/benchmarks.md` has the measurements behind this.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const DEFAULT_THRESHOLD_PERCENT: f64 = 5.0;

/// Rounds per side. Overridable with `EXITIUM_BENCH_ROUNDS` for noise studies.
const ROUNDS: usize = 10;
const ROUNDS_ENV: &str = "EXITIUM_BENCH_ROUNDS";

/// Much shorter than criterion's defaults (3 s warm-up, 5 s measurement, 100
/// samples, 100k bootstrap resamples). Most of the noise differs from one
/// process run to the next, so many short rounds beat a few long ones. The gate
/// reads only the median point estimate, which needs no bootstrap.
const CRITERION_ARGS: [&str; 8] = [
    "--warm-up-time",
    "0.3",
    "--measurement-time",
    "0.5",
    "--sample-size",
    "10",
    "--nresamples",
    "1000",
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

/// The first pass measures every benchmark. The confirm pass re-measures only
/// the ones the first pass flagged, in fresh rounds.
#[derive(Clone, Copy)]
enum Pass {
    First,
    Confirm,
}

impl Pass {
    fn label(self) -> &'static str {
        match self {
            Pass::First => "first pass",
            Pass::Confirm => "confirm pass",
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
    let checkouts = Checkouts {
        base: worktree.clone(),
        head: PathBuf::from("."),
        base_target: target_dir.join("bench-base-target"),
        head_target: target_dir,
        criterion_dir,
    };
    // The base worktree must outlive every round, so remove it only at the end.
    let result = run_rounds(&checkouts, rounds, allow_removed);
    remove_worktree(&worktree);
    result
}

fn remove_worktree(worktree: &Path) {
    let _ = Command::new("git")
        .args(["worktree", "remove", "--force"])
        .arg(worktree)
        .status();
}

/// Where each side builds and runs. Each side has its own cargo target dir: the
/// two checkouts build to the same artifact paths, so in one shared dir each
/// side's build would overwrite the other's benchmark binaries. Both write to
/// one criterion dir, so the gate finds every round in one place.
struct Checkouts {
    base: PathBuf,
    head: PathBuf,
    base_target: PathBuf,
    head_target: PathBuf,
    criterion_dir: PathBuf,
}

impl Checkouts {
    fn bench(
        &self,
        side: Side,
        cargo_args: &[&str],
        criterion_args: &[String],
    ) -> Result<(), String> {
        let (dir, target) = match side {
            Side::Base => (&self.base, &self.base_target),
            Side::Head => (&self.head, &self.head_target),
        };
        cargo_bench(dir, target, &self.criterion_dir, cargo_args, criterion_args)
    }
}

fn run_rounds(checkouts: &Checkouts, rounds: usize, allow_removed: bool) -> Result<(), String> {
    let base_has_benches = has_bench_targets(&checkouts.base)?;
    let head_has_benches = has_bench_targets(&checkouts.head)?;

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
            let args = round_args(Pass::First, Side::Head, 1);
            return checkouts.bench(Side::Head, &[], &args);
        }
        (true, true) => {}
    }

    // Build both sides before timing anything, so no round shares the CPU
    // with a compile.
    for side in [Side::Base, Side::Head] {
        checkouts.bench(side, &["--no-run"], &[])?;
    }
    measure(checkouts, Pass::First, rounds, None)?;
    let first = collect_rounds(&checkouts.criterion_dir, Pass::First)?;
    if !allow_removed {
        let missing: Vec<&str> = first
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
    let flagged: Vec<&str> = first
        .iter()
        .filter(|b| flags(b, DEFAULT_THRESHOLD_PERCENT))
        .map(|b| b.id.as_str())
        .collect();
    if !flagged.is_empty() {
        // Twice the rounds: only the flagged benchmarks run, so it is cheap, and
        // a tighter median makes a second false flag much less likely.
        let confirm_rounds = 2 * rounds;
        println!(
            "bench-ci: {} benchmark(s) over {DEFAULT_THRESHOLD_PERCENT}%; \
             confirming in {confirm_rounds} fresh rounds: {}",
            flagged.len(),
            flagged.join(", ")
        );
        let filter = exact_filter(&flagged);
        measure(checkouts, Pass::Confirm, confirm_rounds, Some(&filter))?;
    }
    let confirm = collect_rounds(&checkouts.criterion_dir, Pass::Confirm)?;
    // Both sides ran benchmarks, so an empty comparison means something broke.
    gate(&first, &confirm, DEFAULT_THRESHOLD_PERCENT, true)
}

/// Runs `rounds` interleaved rounds per side, of every benchmark or only those
/// matching `filter`.
fn measure(
    checkouts: &Checkouts,
    pass: Pass,
    rounds: usize,
    filter: Option<&str>,
) -> Result<(), String> {
    for round in 1..=rounds {
        let order = if round % 2 == 1 {
            [Side::Base, Side::Head]
        } else {
            [Side::Head, Side::Base]
        };
        for side in order {
            let (label, name) = (pass.label(), side.name());
            println!("bench-ci: {label} round {round}/{rounds}, {name}");
            let mut args: Vec<String> = filter.map(String::from).into_iter().collect();
            args.extend(round_args(pass, side, round));
            checkouts.bench(side, &[], &args)?;
        }
    }
    Ok(())
}

fn round_args(pass: Pass, side: Side, round: usize) -> Vec<String> {
    let mut args = vec!["--save-baseline".to_string(), baseline(pass, side, round)];
    args.extend(CRITERION_ARGS.iter().map(|a| a.to_string()));
    args
}

/// The criterion baseline one round is saved as, e.g. `head-3` or
/// `confirm-base-1`.
fn baseline(pass: Pass, side: Side, round: usize) -> String {
    let prefix = match pass {
        Pass::First => "",
        Pass::Confirm => "confirm-",
    };
    format!("{prefix}{}-{round}", side.name())
}

/// A criterion filter (a regex) that matches exactly these benchmark IDs.
fn exact_filter(ids: &[&str]) -> String {
    let escape = |id: &str| {
        let mut out = String::new();
        for c in id.chars() {
            if r"\.+*?()|[]{}^$#&-~".contains(c) {
                out.push('\\');
            }
            out.push(c);
        }
        out
    };
    let escaped: Vec<String> = ids.iter().map(|id| escape(id)).collect();
    format!("^({})$", escaped.join("|"))
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
    let criterion_dir = target_dir()?.join("criterion");
    let first = collect_rounds(&criterion_dir, Pass::First)?;
    let confirm = collect_rounds(&criterion_dir, Pass::Confirm)?;
    gate(&first, &confirm, threshold, false)
}

/// One benchmark's per-round median times in nanoseconds, in round order,
/// from one pass.
#[derive(Debug)]
struct Rounds {
    /// Criterion's directory for the benchmark, relative to the criterion dir.
    name: String,
    /// Criterion's benchmark ID, which filters match against.
    id: String,
    base: Vec<f64>,
    head: Vec<f64>,
}

impl Rounds {
    /// Relative change of the median HEAD round against the median base round
    /// (0.05 = 5% slower). `None` when one side has no rounds.
    fn change(&self) -> Option<f64> {
        Some(median(&self.head)? / median(&self.base)? - 1.0)
    }
}

/// The middle value, or the mean of the two middle values for an even count.
fn median(times: &[f64]) -> Option<f64> {
    let mut sorted = times.to_vec();
    sorted.sort_by(f64::total_cmp);
    let mid = sorted.len() / 2;
    match sorted.len() {
        0 => None,
        n if n % 2 == 1 => Some(sorted[mid]),
        _ => Some((sorted[mid - 1] + sorted[mid]) / 2.0),
    }
}

/// True if one pass shows the benchmark slower than the threshold.
fn flags(bench: &Rounds, threshold_percent: f64) -> bool {
    bench
        .change()
        .is_some_and(|change| change * 100.0 > threshold_percent)
}

/// The rule: a benchmark regressed when the first pass flags it and the confirm
/// pass flags it again. Without confirm rounds (`bench-compare` at a lower
/// threshold than `bench-ci` used), the first pass decides alone.
fn regressed(first: &Rounds, confirm: Option<&Rounds>, threshold_percent: f64) -> bool {
    flags(first, threshold_percent) && confirm.is_none_or(|c| flags(c, threshold_percent))
}

fn gate(
    first: &[Rounds],
    confirm: &[Rounds],
    threshold: f64,
    require_comparisons: bool,
) -> Result<(), String> {
    let mut compared = 0;
    let mut regressions = 0;
    println!(
        "{:>10}  {:>8}  {:>8}  benchmark  [base rounds] [head rounds]",
        "", "change", "confirm"
    );
    for bench in first {
        let confirmed = confirm.iter().find(|c| c.name == bench.name);
        let flag = if bench.change().is_none() {
            if bench.base.is_empty() {
                "new"
            } else {
                "removed"
            }
        } else {
            compared += 1;
            if regressed(bench, confirmed, threshold) {
                regressions += 1;
                "REGRESSION"
            } else if flags(bench, threshold) {
                "noise"
            } else {
                "ok"
            }
        };
        let percent = |b: Option<&Rounds>| match b.and_then(Rounds::change) {
            Some(change) => format!("{:+7.2}%", change * 100.0),
            None => String::new(),
        };
        println!(
            "{flag:>10}  {:>8}  {:>8}  {}  [{}] [{}]",
            percent(Some(bench)),
            percent(confirmed),
            bench.name,
            format_times(&bench.base),
            format_times(&bench.head)
        );
        if let Some(c) = confirmed {
            println!(
                "{:>30}  [{}] [{}]",
                "confirm rounds:",
                format_times(&c.base),
                format_times(&c.head)
            );
        }
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
             (median HEAD round vs median base round, in both passes)"
        ))
    } else {
        println!("bench-compare: no regressions over {threshold}%");
        Ok(())
    }
}

fn format_times(times: &[f64]) -> String {
    let unit = match median(times) {
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
/// `new/benchmark.json`) and reads its rounds from one pass, sorted by name.
/// Benchmarks with no rounds in that pass are left out.
fn collect_rounds(criterion_dir: &Path, pass: Pass) -> Result<Vec<Rounds>, String> {
    fn walk(root: &Path, dir: &Path, pass: Pass, out: &mut Vec<Rounds>) -> Result<(), String> {
        let Ok(entries) = fs::read_dir(dir) else {
            return Ok(());
        };
        for path in entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()) {
            let info = path.join("new/benchmark.json");
            if !info.is_file() {
                walk(root, &path, pass, out)?;
                continue;
            }
            let name = path.strip_prefix(root).unwrap_or(&path);
            let name = name.to_string_lossy().replace('\\', "/");
            let base = read_side(&path, pass, Side::Base, &name)?;
            let head = read_side(&path, pass, Side::Head, &name)?;
            if base.is_empty() && head.is_empty() {
                continue;
            }
            let id = fs::read_to_string(&info)
                .ok()
                .and_then(|json| parse_full_id(&json))
                .ok_or_else(|| format!("unreadable criterion benchmark.json for {name}"))?;
            out.push(Rounds {
                name,
                id,
                base,
                head,
            });
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(criterion_dir, criterion_dir, pass, &mut out)?;
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Reads rounds 1, 2, ... of one side until the first missing round.
fn read_side(bench_dir: &Path, pass: Pass, side: Side, name: &str) -> Result<Vec<f64>, String> {
    let mut times = Vec::new();
    for round in 1.. {
        let estimates = bench_dir
            .join(baseline(pass, side, round))
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

/// Extracts `full_id` from criterion's `benchmark.json`. IDs follow the
/// `<crate>/<area>/<case>/<size>` convention, so they hold no escaped quotes.
fn parse_full_id(json: &str) -> Option<String> {
    let key = "\"full_id\":\"";
    let start = json.find(key)? + key.len();
    let len = json[start..].find('"')?;
    Some(json[start..start + len].to_string())
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
    criterion_dir: &Path,
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
        .env("CRITERION_HOME", criterion_dir)
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

    #[test]
    fn parses_full_id() {
        let json = r#"{"group_id":"exitium_core/num","function_id":"mul","value_str":"1000","full_id":"exitium_core/num/mul/1000","directory_name":"exitium_core_num/mul/1000"}"#;
        assert_eq!(
            parse_full_id(json).as_deref(),
            Some("exitium_core/num/mul/1000")
        );
        assert_eq!(parse_full_id(r#"{"title":"x"}"#), None);
    }

    #[test]
    fn exact_filter_escapes_and_anchors() {
        assert_eq!(
            exact_filter(&["g/mul/1000", "g/a.b(c)"]),
            r"^(g/mul/1000|g/a\.b\(c\))$"
        );
    }

    /// Scales a typical round time by per-round factors (1.0 = the true cost).
    fn rounds(base: &[f64], head: &[f64]) -> Rounds {
        Rounds {
            name: "g/b".into(),
            id: "g/b".into(),
            base: base.iter().map(|f| 700.0 * f).collect(),
            head: head.iter().map(|f| 700.0 * f).collect(),
        }
    }

    fn flagged(bench: &Rounds) -> bool {
        flags(bench, DEFAULT_THRESHOLD_PERCENT)
    }

    /// Rounds of identical code: a few percent either way, as on the runner.
    const NOISE: [f64; 6] = [1.03, 0.97, 1.04, 1.00, 0.96, 1.02];

    fn slower(factor: f64) -> [f64; 6] {
        NOISE.map(|f| f * factor)
    }

    #[test]
    fn noise_only_passes() {
        let head = [1.04, 0.99, 0.98, 1.05, 1.01, 0.97];
        assert!(!flagged(&rounds(&NOISE, &head)));
    }

    #[test]
    fn real_regression_fails() {
        // A 10% slowdown is in every HEAD round, on top of the same noise.
        assert!(flagged(&rounds(&NOISE, &slower(1.10))));
    }

    #[test]
    fn regression_survives_noisy_base_rounds() {
        // Bursts on two base rounds must not hide a real 10% slowdown.
        let base = [3.10, 0.97, 1.04, 1.00, 1.60, 1.02];
        assert!(flagged(&rounds(&base, &slower(1.10))));
    }

    #[test]
    fn single_run_outlier_passes() {
        // +224%, as in CI run 44131141524, but in one HEAD round only.
        let head = [1.02, 3.24, 0.98, 1.01, 0.99, 1.03];
        assert!(!flagged(&rounds(&NOISE, &head)));
        // Two bursts still pass: the median needs half the rounds to move.
        let head = [1.02, 3.24, 0.98, 1.29, 0.99, 1.03];
        assert!(!flagged(&rounds(&NOISE, &head)));
    }

    #[test]
    fn lucky_fast_base_round_passes() {
        // CI run 44145347307 failed a fastest-round rule on one base round 9%
        // faster than the rest. The median ignores it.
        let base = [0.90, 1.03, 0.97, 1.04, 1.00, 1.02];
        assert!(!flagged(&rounds(&base, &NOISE)));
    }

    #[test]
    fn improvement_passes() {
        assert!(!flagged(&rounds(&NOISE, &slower(0.80))));
    }

    #[test]
    fn confirm_pass_clears_a_noisy_first_pass() {
        // Load during most HEAD rounds of the first pass moves its median.
        let first = rounds(&NOISE, &[1.12, 1.30, 0.98, 1.09, 1.21, 1.03]);
        let confirm = rounds(&NOISE, &[1.01, 0.99, 1.03, 0.97, 1.02, 1.00]);
        assert!(flagged(&first));
        assert!(!regressed(&first, Some(&confirm), 5.0));
    }

    #[test]
    fn confirm_pass_keeps_a_real_regression() {
        let first = rounds(&NOISE, &slower(1.10));
        let confirm = rounds(&[1.00, 1.02, 0.98, 1.01, 3.00, 0.99], &slower(1.11));
        assert!(regressed(&first, Some(&confirm), 5.0));
    }

    #[test]
    fn first_pass_decides_without_confirm_rounds() {
        assert!(regressed(&rounds(&NOISE, &slower(1.10)), None, 5.0));
        assert!(!regressed(&rounds(&NOISE, &NOISE), None, 5.0));
    }

    #[test]
    fn median_of_odd_and_even_counts() {
        assert_eq!(median(&[]), None);
        assert_eq!(median(&[3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(median(&[4.0, 1.0, 3.0, 2.0]), Some(2.5));
    }

    #[test]
    fn threshold_boundary() {
        assert!(!flags(&rounds(&[1.0], &[1.049]), 5.0));
        assert!(flags(&rounds(&[1.0], &[1.051]), 5.0));
    }

    #[test]
    fn change_needs_both_sides() {
        assert_eq!(rounds(&[], &[1.0]).change(), None);
        assert_eq!(rounds(&[1.0], &[]).change(), None);
    }

    fn write_estimates(dir: &Path, bench: &str, round: &str, median: f64) {
        let round_dir = dir.join(bench).join(round);
        fs::create_dir_all(&round_dir).unwrap();
        let json = format!(r#"{{"median":{{"point_estimate":{median}}}}}"#);
        fs::write(round_dir.join("estimates.json"), json).unwrap();
    }

    /// Criterion's `new/benchmark.json`, which marks a benchmark directory.
    fn write_info(dir: &Path, bench: &str) {
        let new_dir = dir.join(bench).join("new");
        fs::create_dir_all(&new_dir).unwrap();
        let json = format!(r#"{{"full_id":"id:{bench}","directory_name":"{bench}"}}"#);
        fs::write(new_dir.join("benchmark.json"), json).unwrap();
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("xtask-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn collects_rounds_per_benchmark_and_pass() {
        let dir = temp_dir("collect");
        for (bench, rounds) in [
            ("g/kept/1000", &["base-1", "base-2", "head-1", "head-2"][..]),
            ("g/removed", &["base-1"][..]),
            ("g/added", &["head-1"][..]),
            ("g/confirmed", &["base-1", "head-1", "confirm-base-1"][..]),
        ] {
            write_info(&dir, bench);
            for (i, round) in rounds.iter().enumerate() {
                write_estimates(&dir, bench, round, 10.0 + i as f64);
            }
        }
        // A gap ends a side: head-3 without head-2 is not read.
        write_estimates(&dir, "g/added", "head-3", 99.0);
        let first = collect_rounds(&dir, Pass::First).unwrap();
        let confirm = collect_rounds(&dir, Pass::Confirm).unwrap();
        fs::remove_dir_all(&dir).unwrap();

        let summary = |benches: &[Rounds]| -> Vec<(String, String, Vec<f64>, Vec<f64>)> {
            benches
                .iter()
                .map(|b| (b.name.clone(), b.id.clone(), b.base.clone(), b.head.clone()))
                .collect()
        };
        let row = |name: &str, base: Vec<f64>, head: Vec<f64>| {
            (name.to_string(), format!("id:{name}"), base, head)
        };
        assert_eq!(
            summary(&first),
            vec![
                row("g/added", vec![], vec![10.0]),
                row("g/confirmed", vec![10.0], vec![11.0]),
                row("g/kept/1000", vec![10.0, 11.0], vec![12.0, 13.0]),
                row("g/removed", vec![10.0], vec![]),
            ]
        );
        assert_eq!(
            summary(&confirm),
            vec![row("g/confirmed", vec![12.0], vec![])]
        );
    }

    #[test]
    fn unparseable_files_are_an_error() {
        let dir = temp_dir("unparseable");
        write_info(&dir, "g/b");
        fs::create_dir_all(dir.join("g/b/base-1")).unwrap();
        fs::write(dir.join("g/b/base-1/estimates.json"), "{}").unwrap();
        assert!(collect_rounds(&dir, Pass::First).is_err());

        write_estimates(&dir, "g/b", "base-1", 1.0);
        fs::write(dir.join("g/b/new/benchmark.json"), "{}").unwrap();
        let result = collect_rounds(&dir, Pass::First);
        fs::remove_dir_all(&dir).unwrap();
        assert!(result.is_err());
    }

    #[test]
    fn gate_fails_only_on_confirmed_regressions() {
        let ok = rounds(&NOISE, &NOISE);
        let slow = rounds(&NOISE, &slower(1.2));
        assert!(gate(&[ok], &[], 5.0, true).is_ok());
        assert!(gate(&[rounds(&NOISE, &slower(1.2))], &[slow], 5.0, true).is_err());
        let cleared = rounds(&NOISE, &NOISE);
        assert!(gate(&[rounds(&NOISE, &slower(1.2))], &[cleared], 5.0, true).is_ok());
    }

    #[test]
    fn gate_requires_comparisons_when_asked() {
        let added = || rounds(&[], &[1.0]);
        assert!(gate(&[], &[], 5.0, false).is_ok());
        assert!(gate(&[], &[], 5.0, true).is_err());
        assert!(gate(&[added()], &[], 5.0, true).is_err());
        assert!(gate(&[added()], &[], 5.0, false).is_ok());
    }
}
