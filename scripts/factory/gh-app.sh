#!/usr/bin/env bash
# Runs gh as the exitium-factory GitHub App (CLAUDE.md "GitHub identity").
# Usage: scripts/factory/gh-app.sh <gh args...>
#   e.g. scripts/factory/gh-app.sh pr view 42 --json title
#
# Fetches a fresh one-hour App token and hands it to gh through GH_TOKEN, set
# only in gh's own environment. The token is never written to disk, printed or
# exported to the calling shell.
#
# Fails closed: if the token fetch fails or returns nothing, the script exits 1
# without running gh. Running gh with an empty GH_TOKEN would make it fall back
# to its stored login, which on the factory machine is the owner's account.
#
# `gh auth ...` is refused: `auth status` and `auth token` print the token.
#
# Exit codes: gh's own exit code, or 1 when no token was available or the
# command was refused.
set -euo pipefail
# Tracing (bash -x) would print the token below; turn it off for this script.
set +x

case "${1-}" in
  auth)
    echo "gh-app.sh: 'gh auth' can print the App token and is not allowed; gh was not run" >&2
    exit 1
    ;;
esac

here="$(cd "$(dirname "$0")" && pwd)"

# A plain assignment keeps the fetch's exit status. `GH_TOKEN="$(cmd)" gh ...`
# and `export X="$(cmd)"` both lose it, so a failed fetch would go unnoticed.
token="$("$here/gh-app-token.sh")" || {
  echo "gh-app.sh: could not get a factory App token; gh was not run" >&2
  exit 1
}
[[ "$token" =~ ^[^[:space:]]+$ ]] || {
  echo "gh-app.sh: the factory App token is empty or malformed; gh was not run" >&2
  exit 1
}

GH_TOKEN="$token" exec gh "$@"
