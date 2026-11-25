---
name: review-pr
description: Adversarially review an Exitium PR and post the verdict as the factory/ai-review commit status. Used by the reviewer agent (and test-reviewer for posting).
---

# Review a PR

`T` is short for `scripts/factory/gh-app.sh`, which runs gh as the App (CLAUDE.md "GitHub identity"): `T pr diff 42` means `scripts/factory/gh-app.sh pr diff 42`.

1. **Load:** `T pr view <N> --json title,body,headRefOid,files,labels`, then `T pr diff <N>`. Read the linked issue's acceptance criteria.
2. **Check out the head** in a scratch worktree. Never run `git checkout`, `git switch` or `gh pr checkout` in the main checkout: parallel reviewers and the orchestrator share it, so moving its HEAD makes them read the wrong files.
   - Run `git fetch origin`, then `git worktree add ../exitium-wt/review-<N> <headRefOid>`. Run the definition-of-done commands from the `implement-issue` skill there. A failure means changes requested. Remove it after with `git worktree remove --force ../exitium-wt/review-<N>`.
   - Docs-only PR (Markdown only, nothing to build): skip the worktree. After `git fetch origin`, read each file with `git show <headRefOid>:<path>` and compare it with `git show origin/main:<path>`. In Git Bash, prefix these with `MSYS_NO_PATHCONV=1`, or a path like `origin/main:.claude/...` gets mangled.
3. **Review, in this order:**
   1. **Correctness:** logic errors, edge cases, panics on valid input, integer overflow, concurrency races, `unsafe` soundness. Every `unsafe` block needs a valid `// SAFETY:` argument.
   2. **Acceptance criteria:** is each one met?
   3. **Design rules:** agent parity (does each editor command have an MCP tool?), reflection as the source of truth, command bus for mutations, facade API.
   4. **Performance:** allocations in hot loops, cache-hostile layouts, needless locking, missing benchmarks on hot paths.
   5. **Scope:** every changed file must be inside the issue's `## Footprint` (`Edits` plus `Hotspots`). A file outside it means changes requested, because it can collide with parallel work. Unrelated changes also mean changes requested. Out-of-scope ideas become follow-up issues, not blockers.
   6. **High-risk areas:** if the PR touches an area listed in CLAUDE.md "Human escalation" (dependencies, `unsafe`, facade API, save formats, licensing, secrets, CI/factory config, releases), review it with extra scrutiny and name the areas you checked in the verdict. No human gate backs you up.
   7. **Readability:** could a basic-level Rust reviewer follow it? Are invariants documented?
4. **Report only real findings.** Each one gets a file:line reference, the problem and a concrete fix. No style nits that rustfmt or clippy would catch. No praise padding.
5. **Post.** The comment's first line is `AI review, round <R> of 3, head <sha7>`, where `<sha7>` is the first 7 characters of `headRefOid` and `<R>` is the round from your prompt; without one, apply the round rule. The last line is the verdict line, with nothing after it.
   **Round rule (ADR-0008):** `<R>` is 1 plus the number of distinct head SHAs, other than the head under review, among earlier comments by `exitium-factory[bot]` whose first line starts with `AI review` and whose last line matches `^verdict: (approve|changes requested)$`. A header without `head <sha7>` (posted before this rule) counts as its own head. A round that does not end in a merge has failed: it requested changes, or it approved and a gate check then failed on the same head. The PR escalates with `needs-human` when round 3 fails.
   This prints the first line of each comment the rule counts:
   ```bash
   T api repos/nathanialwm/ExitiumEngine/issues/<N>/comments --paginate --jq '.[]
     | select(.user.login == "exitium-factory[bot]") | .body | rtrimstr("\n") | split("\n")
     | select((.[0] | startswith("AI review")) and (last | test("^verdict: (approve|changes requested)$")))
     | .[0]'
   ```
   Then post:
   ```bash
   T pr comment <N> --body "AI review, round <R> of 3, head <sha7>
   <findings or 'No blocking issues.'>
   verdict: approve | changes requested"
   T api repos/nathanialwm/ExitiumEngine/statuses/<headRefOid> \
     -f state=<success|failure> -f context=factory/ai-review -f description="<one line>"
   ```
6. **Return:** `verdict <approve|changes requested>, <count> findings`.
