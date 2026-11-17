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
   - Do not add a dependency without the `gate:human` label on the PR.
   - Do not add `unsafe` outside designated modules.
   - If you add an editor command, add its MCP tool.
5. **Verify (definition of done)** and fix until all pass:
   ```bash
   cargo fmt --all
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   cargo build --workspace --target wasm32-unknown-unknown --exclude exitium_reflect_derive --exclude exitium_editor --exclude exitium_cli
   ```
   If you touched hot paths, also run the relevant benchmarks and include the numbers.
6. **Push:** `git push -u origin <branch>`. The pre-push hook refreshes the graph; if it says "graph updated, push again", push again.
7. **Open the PR as the App:**
   ```bash
   T pr create --title "<type>(<scope>): <summary>" --body "Closes #<N>
   ## Summary
   ## Changes
   ## Verification
   ## Gates
   <none | list of gates touched and why>"
   ```
   If any gate applies, add the label: `T pr edit <PR> --add-label gate:human`.
8. **Return** exactly: `PR <url>, head <sha>, gates <none|list>`.
