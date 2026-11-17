# Installs the native Windows GitHub Actions runner for Exitium (INTERVIEW.md R1).
# Windows 11 Home cannot run Windows containers, so this runner runs directly on
# the host and starts at logon through a scheduled task (no admin rights needed).
#
# Usage, from PowerShell:
#   $token = gh api -X POST repos/nathanialwm/ExitiumEngine/actions/runners/registration-token --jq .token
#   ci/runner/windows/setup.ps1 -Token $token
#
# Requires git and rustup on PATH. The toolchain pinned in rust-toolchain.toml is
# installed on first use.
param(
    [Parameter(Mandatory = $true)][string]$Token,
    [string]$Dir = "$env:USERPROFILE\actions-runner-exitium",
    [string]$Version = "2.338.2",
    [string]$Name = "exitium-windows-1"
)
$ErrorActionPreference = "Stop"

New-Item -ItemType Directory -Force $Dir | Out-Null
Set-Location $Dir

if (-not (Test-Path "$Dir\config.cmd")) {
    $zip = "$Dir\runner.zip"
    Invoke-WebRequest -UseBasicParsing -OutFile $zip `
        "https://github.com/actions/runner/releases/download/v$Version/actions-runner-win-x64-$Version.zip"
    Expand-Archive -Path $zip -DestinationPath $Dir -Force
    Remove-Item $zip
}

if (-not (Test-Path "$Dir\.runner")) {
    & "$Dir\config.cmd" --unattended --replace `
        --url https://github.com/nathanialwm/ExitiumEngine --token $Token `
        --name $Name --labels exitium-windows --work _work
    if ($LASTEXITCODE -ne 0) { throw "runner configuration failed" }
}

# Keep build output outside the checkout: actions/checkout cleans untracked files,
# which would delete an in-tree target/ on every job. The runner reads .env.
Set-Content -Path "$Dir\.env" -Value "CARGO_TARGET_DIR=$Dir\target" -Encoding ascii

$taskName = "ExitiumActionsRunner"
$action = New-ScheduledTaskAction -Execute "$Dir\run.cmd" -WorkingDirectory $Dir
$trigger = New-ScheduledTaskTrigger -AtLogOn -User $env:USERNAME
$settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries `
    -ExecutionTimeLimit ([TimeSpan]::Zero) -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1)
Register-ScheduledTask -TaskName $taskName -Action $action -Trigger $trigger -Settings $settings -Force | Out-Null
Start-ScheduledTask -TaskName $taskName
Write-Output "runner '$Name' registered and started (scheduled task '$taskName')"
