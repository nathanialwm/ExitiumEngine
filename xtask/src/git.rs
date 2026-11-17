//! Git helpers for agents: the `push` wrapper and hook installation.
//!
//! The hooks in `scripts/hooks` keep the local graphify graph current:
//! `post-checkout` builds it in a fresh clone or worktree and `pre-push`
//! refreshes it. `graphify-out/` is gitignored and never committed (ADR-0004),
//! so `push` is a plain `git push`. It stays a command so agent docs and
//! scripts have one stable entry point.

use std::process::Command;

pub fn push(args: &[String]) -> Result<(), String> {
    println!("$ git push {}", args.join(" "));
    let status = Command::new("git")
        .arg("push")
        .args(args)
        .status()
        .map_err(|e| format!("failed to start git: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("git push failed with {status}"))
    }
}

/// Configures the current clone (and all its worktrees, which share config).
pub fn install_hooks() -> Result<(), String> {
    crate::run("git", &["config", "core.hooksPath", "scripts/hooks"])?;
    println!("hooks installed: post-checkout builds and pre-push refreshes the local graph");
    Ok(())
}
