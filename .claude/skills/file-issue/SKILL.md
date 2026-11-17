---
name: file-issue
description: File a well-formed Exitium GitHub issue as the factory App (follow-up work, bugs, perf findings, roadmap items). Any agent may use it; agent-filed issues need approval before work starts.
---

# File an issue

1. Search first: `T issue list --search "<keywords>" --state all`. If a matching issue exists, comment on it instead of filing a new one.
2. File it (`T` means `GH_TOKEN="$(scripts/factory/gh-app-token.sh)" gh`):
   ```bash
   T issue create --title "<type>(<scope>): <summary>" \
     --label status:needs-approval --label "area:<area>" \
     --body "## Problem / goal
   ## Acceptance criteria
   - [ ] ...
   ## Context
   Found while working on #<N> / PR #<M>.
   ## Dependencies
   Blocked by #<N> (if any)"
   ```
3. One issue is one PR-sized unit of work. Split anything bigger.
4. Return the issue URL.
