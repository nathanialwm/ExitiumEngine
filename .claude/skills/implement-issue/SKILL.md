---
name: implement-issue
description: Take one approved Exitium GitHub issue from branch to open PR, meeting the definition of done. Used by impl-* agents.
---

# Implement an issue

`T` is short for `scripts/factory/gh-app.sh`, which runs gh as the App (CLAUDE.md "GitHub identity"): `T issue view 42` means `scripts/factory/gh-app.sh issue view 42`.

1. **Read the issue:** `T issue view <N>`. Also read any linked spec or ADR. If the acceptance criteria are unclear, stop and comment a question on the issue. Do not guess.
2. **Branch:** first check whether the issue already has a branch. Use the one the prompt names; otherwise run `git ls-remote --heads origin | cut -f2 | grep -E '^refs/heads/[a-z]+/<N>-[^/]+$'`, which matches only `<type>/<N>-<slug>` names.
   - No output: there is no branch. Create `<type>/<N>-<slug>` from the latest `origin/main`. If you are not already in an isolated worktree, create one with `git worktree add ../exitium-wt/<branch> -b <branch> origin/main`.
   - One line: continue that branch (drop `refs/heads/`) instead of creating one: `git fetch origin`, then `git switch <branch>` (or `git worktree add ../exitium-wt/<branch> <branch>` outside a worktree). If it already has an open PR, step 7 returns that PR instead of opening another.
   - More than one line: stop and comment the branch names on the issue, as in step 1.
3. **Orient:** run `graphify query "<what you need>"`, then read the target crate's `CLAUDE.md`. Read only the files you will touch.
4. **Implement:** follow CLAUDE.md design and code rules. Make small Conventional Commits.
   - Edit only paths listed in the issue's `## Footprint`. Other issues run in parallel on the paths outside it. If you need a path outside it, stop, push your commits so far (step 6) so the issue can resume from the branch, comment the path and the reason on the issue, and return `blocked: footprint <paths>`.
   - Justify any new dependency in the PR body (license, wasm support, why not std).
   - Do not add `unsafe` outside designated modules.
   - If you add an editor command, add its MCP tool.
5. **Verify (definition of done)** and fix until all pass:
   ```bash
   cargo fmt --all
   cargo xtask ci all   # fmt check, clippy -D warnings, tests, WASM build: the same steps CI runs
   ```
   A crate that cannot target the web goes in `NATIVE_ONLY` in `xtask/src/ci.rs` (say why in the PR).
   If you touched hot paths, also run the relevant benchmarks and include the numbers.
6. **Push:** `cargo xtask push -u origin <branch>`. If `cargo xtask install-hooks` was never run in this clone, run it once first. Never commit `graphify-out/` (ADR-0004).
7. **Open the PR as the App:**
   ```bash
   T pr create --title "<type>(<scope>): <summary>" --body "Closes #<N>
   ## Summary
   ## Changes
   ## Verification
   ## Risk
   <none | high-risk areas touched and why they are safe>"
   ```
8. **Return** exactly: `PR <url>, head <sha>, risk <none|list>`.
