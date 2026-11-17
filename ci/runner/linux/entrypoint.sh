#!/usr/bin/env bash
# Registers the runner on first start, then runs it. Registration state is kept
# in /state (a volume) so restarts and image rebuilds reuse the same runner.
set -euo pipefail
cd /home/runner/actions-runner

state_files=(.runner .credentials .credentials_rsaparams)

if [ -f /state/.runner ]; then
  for f in "${state_files[@]}"; do cp "/state/$f" .; done
else
  : "${RUNNER_TOKEN:?first start needs RUNNER_TOKEN (see ci/runner/README.md)}"
  ./config.sh --unattended --replace \
    --url "${REPO_URL}" --token "${RUNNER_TOKEN}" \
    --name "${RUNNER_NAME}" --labels "${RUNNER_LABELS}" --work _work
  for f in "${state_files[@]}"; do cp "$f" /state/; done
fi

exec ./run.sh
