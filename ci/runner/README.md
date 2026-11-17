# CI runners

Exitium's CI runs on self-hosted runners on the owner's PC (INTERVIEW.md B15, R1), plus one GitHub-hosted macOS job (R2).

| Runner | Label | Where | Jobs |
|---|---|---|---|
| `exitium-linux-1`, `exitium-linux-2` | `exitium-linux` | Docker (Linux containers via WSL2) | fmt, clippy, tests, WASM, cargo-deny, Miri, benchmarks |
| `exitium-windows-1` | `exitium-windows` | Native Windows host (Home edition cannot run Windows containers) | Windows clippy and tests |
| GitHub-hosted `macos-latest` | — | GitHub | macOS clippy and tests, only when platform or render code changed |
| GitHub-hosted `ubuntu-latest` | — | GitHub | `pr-gates.yml` (`pr-title`, `human-gate`) |

Benchmarks run only on `exitium-linux-1` (label `exitium-bench`), so they queue instead of being cancelled and never overlap each other. They still share the CPU with other CI jobs, so the gate counts only slowdowns that are both over 5% and statistically significant.

Every runner keeps `CARGO_TARGET_DIR` outside the checkout, because `actions/checkout` deletes untracked files.

**Trust model.** These runners execute PR code on the owner's PC, and the Windows runner runs as the owner's account. That is no wider than the agents themselves, which already run on this machine as the owner (INTERVIEW.md A6). The PR gates (`pr-gates.yml`) hold a write token, so they run on GitHub-hosted runners and never here. Runner state (target dirs, cargo registry) persists across jobs for speed, so one PR's build script could affect a later run. That is acceptable while every PR comes from the owner or the factory; switch to `--ephemeral` runners before accepting outside contributions.

Runner labels are set at registration. To change one on a registered runner, edit it through the API (`POST repos/{repo}/actions/runners/{id}/labels`) or re-register with `docker compose down -v`.

Windows builds are checked natively rather than cross-compiled with `cargo-xwin`, so the Linux image does not carry the MSVC SDK.

## Linux runners

First start needs a registration token (valid for one hour). Registration is then kept in the `linux-*-state` volumes, so restarts and image rebuilds need no token.

```bash
cd ci/runner
RUNNER_TOKEN="$(gh api -X POST repos/nathanialwm/ExitiumEngine/actions/runners/registration-token --jq .token)" \
  docker compose up -d --build
```

Rebuild after changing the Dockerfile (for example a toolchain bump): `docker compose up -d --build`. To re-register from scratch, `docker compose down -v` and start again with a fresh token.

## Windows runner

```powershell
$token = gh api -X POST repos/nathanialwm/ExitiumEngine/actions/runners/registration-token --jq .token
ci/runner/windows/setup.ps1 -Token $token
```

It starts at logon through the `ExitiumActionsRunner` scheduled task.

## Toolchain bumps

`rust-toolchain.toml` pins the toolchain. Runners install a missing pinned toolchain on first use, but update `RUST_VERSION` in `linux/Dockerfile` too so the image doesn't download it on every fresh volume.
