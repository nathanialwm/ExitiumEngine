---
name: implement-issue
description: Take one approved Exitium GitHub issue from branch to open PR, meeting the definition of done. Used by impl-* agents.
---

# Implement an issue

`T` means `GH_TOKEN="$(scripts/factory/gh-app-token.sh)" gh`.

1. **Read the issue:** `T issue view <N>`. Also read any linked spec or ADR. If the acceptance criteria are unclear, stop and comment a question on the issue. Do not guess.
2. **Branch:** create `<type>/<N>-<slug>` from the latest `origin/main`. If you are not already in an isolated worktree, create one with `git worktree add ../exitium-wt/<branch> -b <branch> origin/main`.
3. **Orient:** run `graphify query "<what you need>"`, then read the target crate's `CLAUDE.md`. Read only the files you will touch.
4. **Implement:** follow CLAUDE.md design and code rules. Make small Conventional Commits.
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
