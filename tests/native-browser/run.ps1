param(
    [ValidateSet('both','chromium','firefox')][string]$Browser = 'both',
    [switch]$SkipBuild,
    [string]$TargetDirectory = "$env:LOCALAPPDATA\Temp\opencode\osdm-p2-target"
)
$ErrorActionPreference = 'Stop'
$env:CARGO_TARGET_DIR = [IO.Path]::GetFullPath($TargetDirectory)
$env:CARGO_INCREMENTAL = '0'
$env:CARGO_PROFILE_DEV_DEBUG = '0'
$env:CARGO_BUILD_JOBS = '2'
if (!$SkipBuild) {
    pnpm tauri build --debug --no-bundle
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    cargo build --locked -p osdm-browser-bridge
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
node tests/native-browser/run.mjs --browser $Browser --app (Join-Path $env:CARGO_TARGET_DIR 'debug\osdm-desktop.exe') --host (Join-Path $env:CARGO_TARGET_DIR 'debug\osdm-native-host.exe')
exit $LASTEXITCODE
