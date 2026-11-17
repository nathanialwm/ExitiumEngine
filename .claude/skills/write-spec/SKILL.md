---
name: write-spec
description: Write a feature spec (docs/specs/) or architecture decision record (docs/decisions/) for systems-level Exitium work and open it as a PR. Used by the architect.
---

# Write a spec or ADR

- **Spec** (`docs/specs/<area>-<slug>.md`, from `docs/specs/TEMPLATE.md`): describes how a feature will be built. Use one for every issue labelled `needs-spec`.
- **ADR** (`docs/decisions/NNNN-<slug>.md`, from `docs/decisions/TEMPLATE.md`, next free number): records a choice between alternatives that constrains future work.

Steps:
1. Read the issue, the relevant PLAN.md and INTERVIEW.md sections, and related ADRs (find them with `graphify query`).
2. Fill the template. Be concrete about four things:
   - data layout
   - public API sketch
   - threading
   - the MCP and editor surface (agent-parity rule)

   Give performance budgets as numbers. Keep it to the minimum needed to implement correctly.
3. Open a PR `docs(spec): <feature>` (or `docs(adr): ...`) as the App, with `Closes #N`. It merges like any PR once the AI review passes; the owner is involved only on escalation (CLAUDE.md "Human escalation").
4. Once it merges, remove the `needs-spec` label from the dependent issues.
