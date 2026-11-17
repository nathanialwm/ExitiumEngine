---
name: review-pr
description: Adversarially review an Exitium PR and post the verdict as the factory/ai-review commit status. Used by the reviewer agent (and test-reviewer for posting).
---

# Review a PR

`T` means `GH_TOKEN="$(scripts/factory/gh-app-token.sh)" gh`.

1. **Load:** `T pr view <N> --json title,body,headRefOid,files,labels`, then `T pr diff <N>`. Read the linked issue's acceptance criteria.
2. **Check out the head** in a scratch worktree. Run the definition-of-done commands from the `implement-issue` skill. A failure means changes requested.
3. **Review, in this order:**
   1. **Correctness:** logic errors, edge cases, panics on valid input, integer overflow, concurrency races, `unsafe` soundness. Every `unsafe` block needs a valid `// SAFETY:` argument.
   2. **Acceptance criteria:** is each one met?
   3. **Design rules:** agent parity (does each editor command have an MCP tool?), reflection as the source of truth, command bus for mutations, facade API.
   4. **Performance:** allocations in hot loops, cache-hostile layouts, needless locking, missing benchmarks on hot paths.
   5. **Scope:** unrelated changes mean changes requested. Out-of-scope ideas become follow-up issues, not blockers.
   6. **High-risk areas:** if the PR touches an area listed in CLAUDE.md "Human escalation" (dependencies, `unsafe`, facade API, save formats, licensing, secrets, CI/factory config, releases), review it with extra scrutiny and name the areas you checked in the verdict. No human gate backs you up.
   7. **Readability:** could a basic-level Rust reviewer follow it? Are invariants documented?
4. **Report only real findings.** Each one gets a file:line reference, the problem and a concrete fix. No style nits that rustfmt or clippy would catch. No praise padding.
5. **Post:**
   ```bash
   T pr comment <N> --body "<findings or 'No blocking issues.'>
   verdict: approve | changes requested"
   T api repos/nathanialwm/ExitiumEngine/statuses/<headRefOid> \
     -f state=<success|failure> -f context=factory/ai-review -f description="<one line>"
   ```
6. **Return:** `verdict <approve|changes requested>, <count> findings`.
