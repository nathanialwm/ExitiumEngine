#!/usr/bin/env bash
# Prints a short-lived (1h) installation access token for the exitium-factory GitHub App.
# Credentials live outside the repo in ~/.exitium-factory (see CLAUDE.md "GitHub identity").
#
# Agents do not call this script to run gh. They run gh through the wrapper,
# which fetches the token and fails closed:
#   scripts/factory/gh-app.sh pr create ...
#
# Never write GH_TOKEN="$(scripts/factory/gh-app-token.sh)" gh ...: a failed
# fetch leaves GH_TOKEN empty, and gh silently falls back to the owner's login.
# A caller that needs the token directly (like merge-pr.sh) assigns it to a
# variable first, checks it is non-empty, and only then exports it:
#   token="$("$here/gh-app-token.sh")"
#   [ -n "$token" ] || { echo "could not get a factory App token"; exit 1; }
#   export GH_TOKEN="$token"
set -euo pipefail

dir="${EXITIUM_FACTORY_DIR:-$HOME/.exitium-factory}"
# shellcheck source=/dev/null
source "$dir/config.env"
key="$dir/private-key.pem"

b64url() { openssl base64 -A | tr '+/' '-_' | tr -d '='; }

now=$(date +%s)
header=$(printf '{"alg":"RS256","typ":"JWT"}' | b64url)
payload=$(printf '{"iat":%d,"exp":%d,"iss":"%s"}' $((now - 60)) $((now + 540)) "$APP_ID" | b64url)
signature=$(printf '%s.%s' "$header" "$payload" | openssl dgst -sha256 -sign "$key" | b64url)

curl -fsS -X POST \
  -H "Authorization: Bearer $header.$payload.$signature" \
  -H "Accept: application/vnd.github+json" \
  "https://api.github.com/app/installations/$INSTALLATION_ID/access_tokens" \
  | python -c "import sys, json; print(json.load(sys.stdin)['token'])"
