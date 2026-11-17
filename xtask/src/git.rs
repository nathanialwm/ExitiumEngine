//! Git helpers for agents: the `push` wrapper and hook installation.
//!
//! The `pre-push` hook (`scripts/hooks/pre-push`) refreshes the graphify graph.
//! A hook cannot add a commit to the push already in progress, so when the graph
//! changes it commits the refresh and aborts with "graph updated, push again"
//! (INTERVIEW.md R9). `push` retries in that case so agents push in one step.

use std::process::Command;

const RETRY_MARKER: &str = "graph updated, push again";
const MAX_ATTEMPTS: usize = 3;

pub fn push(args: &[String]) -> Result<(), String> {
    for attempt in 1..=MAX_ATTEMPTS {
        println!("$ git push {}", args.join(" "));
        let output = Command::new("git")
            .arg("push")
            .args(args)
            .output()
            .map_err(|e| format!("failed to start git: {e}"))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        print!("{stdout}");
        eprint!("{stderr}");
        if output.status.success() {
            return Ok(());
        }
        let hook_refreshed = stdout.contains(RETRY_MARKER) || stderr.contains(RETRY_MARKER);
        if !hook_refreshed {
            return Err(format!("git push failed with {}", output.status));
        }
        println!("graph refresh committed; retrying push ({attempt}/{MAX_ATTEMPTS})");
    }
    Err(format!(
        "pre-push hook kept refreshing the graph after {MAX_ATTEMPTS} attempts"
    ))
}

/// Configures the current clone (and all its worktrees, which share config).
pub fn install_hooks() -> Result<(), String> {
    let settings = [
        ("core.hooksPath", "scripts/hooks"),
        ("merge.graphify.name", "graphify graph.json union merge"),
        ("merge.graphify.driver", "graphify merge-driver %O %A %B"),
        ("merge.ours.name", "keep our side (regenerated files)"),
        ("merge.ours.driver", "true"),
    ];
    for (key, value) in settings {
        crate::run("git", &["config", key, value])?;
    }
    println!("hooks installed: pre-push refreshes graphify-out/, graph.json merges via graphify");
    Ok(())
}
