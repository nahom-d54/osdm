param([Parameter(ValueFromRemainingArguments = $true)][string[]]$CommandArgs)

$ErrorActionPreference = "Stop"
$exe = $CommandArgs[0]
$name = Split-Path $exe -Leaf

# Tauri unit-test binaries do not receive the application resource manifest.
# Without Common-Controls v6 they fail before main with a missing
# TaskDialogIndirect entry point, so place a side-by-side manifest first.
if ($name -like "osdm_desktop-*.exe") {
    $manifest = Resolve-Path (Join-Path $PSScriptRoot "..\src-tauri\test.manifest")
    Copy-Item $manifest "$exe.manifest" -Force
}

$rest = @($CommandArgs | Select-Object -Skip 1)
& $exe @rest
exit $LASTEXITCODE
