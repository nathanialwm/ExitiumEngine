#!/usr/bin/env python3
"""Factory usage gate (INTERVIEW.md A8 and R4).

Prints how much of the owner's Claude subscription usage is left and exits:
  0  ok: new issues may start
  1  blocked: less than 5% left in the session (5-hour) or weekly window
  2  unknown: usage could not be read; the orchestrator must ask the owner

It reads the same endpoint Claude Code's /usage uses, with the OAuth token
Claude Code stores in ~/.claude/.credentials.json. The token is sent only to
api.anthropic.com and is never printed. The endpoint is undocumented, so any
failure is reported as "unknown", never as "ok".
"""

import json
import os
import sys
import time
import urllib.request

MIN_REMAINING_PERCENT = 5.0
USAGE_URL = "https://api.anthropic.com/api/oauth/usage"


def unknown(reason):
    print(f"usage-gate: unknown ({reason}); ask the owner before starting new issues")
    sys.exit(2)


def main():
    config_dir = os.environ.get("CLAUDE_CONFIG_DIR", os.path.expanduser("~/.claude"))
    try:
        with open(os.path.join(config_dir, ".credentials.json"), encoding="utf-8") as f:
            oauth = json.load(f)["claudeAiOauth"]
    except (OSError, KeyError, ValueError):
        unknown("no Claude Code OAuth credentials found")
    if oauth.get("expiresAt", 0) / 1000 < time.time():
        unknown("OAuth token expired; any Claude Code request refreshes it")

    request = urllib.request.Request(
        USAGE_URL,
        headers={
            "Authorization": f"Bearer {oauth['accessToken']}",
            "anthropic-beta": "oauth-2025-04-20",
            "User-Agent": "exitium-usage-gate",
        },
    )
    try:
        with urllib.request.urlopen(request, timeout=15) as response:
            usage = json.load(response)
        session = usage["five_hour"]
        weekly = usage["seven_day"]
        session_used = float(session["utilization"])
        weekly_used = float(weekly["utilization"])
    except Exception as error:  # any failure means we cannot vouch for usage
        unknown(f"usage endpoint failed: {type(error).__name__}")

    summary = (
        f"session {session_used:.0f}% used (resets {session.get('resets_at')}), "
        f"weekly {weekly_used:.0f}% used (resets {weekly.get('resets_at')})"
    )
    if min(100 - session_used, 100 - weekly_used) < MIN_REMAINING_PERCENT:
        print(f"usage-gate: blocked, {summary}")
        sys.exit(1)
    print(f"usage-gate: ok, {summary}")


if __name__ == "__main__":
    try:
        main()
    except Exception as error:  # an unexpected crash must read as "unknown", not "blocked"
        unknown(f"unexpected {type(error).__name__}")
