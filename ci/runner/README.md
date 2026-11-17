# CI runners

Exitium's CI runs on self-hosted runners on the owner's PC (INTERVIEW.md B15, R1), plus one GitHub-hosted macOS job (R2).

| Runner | Label | Where | Jobs |
|---|---|---|---|
| `exitium-linux-1`, `exitium-linux-2` | `exitium-linux` | Docker (Linux containers via WSL2) | fmt, clippy, tests, WASM, cargo-deny, Miri, benchmarks, PR gates |
| `exitium-windows-1` | `exitium-windows` | Native Windows host (Home edition cannot run Windows containers) | Windows clippy and tests |
| GitHub-hosted `macos-latest` | — | GitHub | macOS clippy and tests, only when platform or render code changed |

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
